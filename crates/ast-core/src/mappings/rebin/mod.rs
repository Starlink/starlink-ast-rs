//! Rebinning a gridded array through a Mapping: C's `astRebin<X>`.
//!
//! A port of mapping.c `Rebin<X>` and its drivers. Each input pixel's
//! centre is taken through the Mapping's forward transformation to a
//! position in the output grid, and its value spread over the output pixels
//! around that position. The input region is divided recursively along its
//! longest axis until a section is too small to be worth a linear fit or
//! `astLinearApprox` finds the Mapping linear within `tol` over it
//! (`RebinAdaptively`); each section is then handled in blocks of at most
//! 2048 input pixels (`RebinWithBlocking`), whose output positions come from
//! the linear fit or the Mapping (`RebinSection`) and are passed to the
//! spreading function.
//!
//! C writes each driver once over a `void *` array and a type code, and each
//! spreading function once per type from a macro. The port is generic over
//! [`RebinValue`], whose items are the macro arguments and the arithmetic
//! the macros do in the pixel type.

// The loops mirror the C source index for index.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]

mod spread;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::resample::kernels::{self, Builtin, KernelFn};
use crate::mappings::resample::matrix_det;
use crate::mappings::{DynMapping, Mapping, substitute_nan_to_bad_in_place};
use crate::pixel::RebinValue;

/// The pixel spreading scheme of [`rebin`] (C's `spread`).
///
/// The kernel schemes share each input pixel between the output pixels
/// within a distance `N` of its position along each axis, weighted by the
/// product of a 1-d kernel evaluated at their offsets along each axis and
/// normalised to sum to one; weight falling off the output grid is
/// reflected back onto it. The kernels and `params` are as for
/// [`Interp`](crate::mappings::Interp)'s variants of the same names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    /// All of the value to the output pixel containing the position
    /// (`AST__NEAREST`).
    Nearest,
    /// The value shared between the nearest output pixels along each axis
    /// in proportion to their nearness (`AST__LINEAR`, C's default).
    Linear,
    /// The kernel `sinc(pi x)` (`AST__SINC`).
    Sinc,
    /// The kernel `sinc(pi x) sinc(k pi x)` (`AST__SINCSINC`).
    SincSinc,
    /// The kernel `sinc(pi x) cos(k pi x)` (`AST__SINCCOS`).
    SincCos,
    /// The kernel `sinc(pi x) exp(-k x^2)` (`AST__SINCGAUSS`).
    SincGauss,
    /// The kernel `exp(-k x^2)` (`AST__GAUSS`).
    Gauss,
    /// The kernel `somb(pi x)` (`AST__SOMB`).
    Somb,
    /// The kernel `somb(pi x) cos(k pi x)` (`AST__SOMBCOS`).
    SombCos,
}

impl Spread {
    fn builtin(self) -> Option<Builtin> {
        match self {
            Spread::Nearest | Spread::Linear => None,
            Spread::Sinc => Some(Builtin::Sinc),
            Spread::SincSinc => Some(Builtin::SincSinc),
            Spread::SincCos => Some(Builtin::SincCos),
            Spread::SincGauss => Some(Builtin::SincGauss),
            Spread::Gauss => Some(Builtin::Gauss),
            Spread::Somb => Some(Builtin::Somb),
            Spread::SombCos => Some(Builtin::SombCos),
        }
    }
}

/// The flags of [`rebin`] (C's `flags`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RebinFlags {
    /// Input pixels (and variances) equal to the bad value are bad
    /// (`AST__USEBAD`).
    pub use_bad: bool,
    /// Output variances are formed from the input variances
    /// (`AST__USEVAR`), when both are given.
    pub use_var: bool,
}

/// The flags of the spreading functions: [`RebinFlags`] and those only
/// `astRebinSeq<X>` accepts.
#[derive(Debug, Clone, Copy, Default)]
struct SpreadFlags {
    use_bad: bool,
    use_var: bool,
    /// `AST__GENVAR`: output variances from the spread of input values.
    gen_var: bool,
    /// `AST__VARWGT`: input pixels weighted by their inverse variances.
    var_wgt: bool,
    /// `AST__CONSERVEFLUX`.
    conserve_flux: bool,
}

/// An input grid, the region of it to rebin, its data and, optionally, its
/// variances.
#[derive(Debug, Clone, Copy)]
pub struct RebinInput<'a, T> {
    /// The lowest pixel index on each axis of the grid.
    pub lbnd: &'a [i64],
    /// The highest pixel index on each axis of the grid.
    pub ubnd: &'a [i64],
    /// The lowest pixel index on each axis of the region to rebin.
    pub region_lbnd: &'a [i64],
    /// The highest pixel index on each axis of the region to rebin.
    pub region_ubnd: &'a [i64],
    /// The data, one value per pixel of the grid, the first axis varying
    /// fastest.
    pub data: &'a [T],
    /// The variances, one per pixel of the grid.
    pub var: Option<&'a [T]>,
}

/// An output grid: its pixel index bounds, its data and, optionally, its
/// variances.
#[derive(Debug)]
pub struct RebinOutput<'a, T> {
    /// The lowest pixel index on each axis.
    pub lbnd: &'a [i64],
    /// The highest pixel index on each axis.
    pub ubnd: &'a [i64],
    /// The data, one value per pixel.
    pub data: &'a mut [T],
    /// The variances, one per pixel.
    pub var: Option<&'a mut [T]>,
}

/// How [`rebin`] spreads.
#[derive(Debug, Clone, Copy)]
pub struct RebinOptions<'a> {
    /// The spreading scheme.
    pub spread: Spread,
    /// The scheme's parameters (C's `params`), as for the
    /// [`Interp`](crate::mappings::Interp) variants of the same names.
    pub params: &'a [f64],
    /// The flags.
    pub flags: RebinFlags,
    /// The largest positional error, in output pixels, a linear
    /// approximation to the Mapping may introduce; 0 uses the Mapping for
    /// every pixel.
    pub tol: f64,
    /// The largest extent, in input pixels, over which a linear
    /// approximation is first tried.
    pub maxpix: usize,
    /// When positive, output pixels whose weights sum to less than this are
    /// set bad (C's `wlim`).
    pub wlim: f64,
}

/// Rebin the region of `input` into `output` through the forward
/// transformation of `map`, as C's `astRebin<X>` does.
///
/// `map` takes input grid positions to output grid positions; pixel
/// centres are at integer coordinates. The output arrays are set to zero
/// and each good input pixel in the region is added into the output pixels
/// around its position, as `opts.spread` shares it.
///
/// Errors: grids whose axis counts do not match the Mapping's inputs and
/// outputs, or that have none, are AST__NGDIN; a lower bound above its upper
/// bound, or a region outside the input grid, is AST__GBDIN; too many pixels
/// is AST__EXSPIX; a negative `tol` is AST__PATIN; a Mapping with no forward
/// transformation is AST__TRNND; and a data or variance array of the wrong
/// length, bounds of different lengths, or fewer `params` than the scheme
/// uses, is AST__DIMIN. As in C, the output arrays are set to zero after any
/// error but AST__DIMIN, but only their first element when the axis counts
/// or bounds are in error.
pub fn rebin<T: RebinValue>(
    map: &dyn Mapping,
    input: &RebinInput<'_, T>,
    output: &mut RebinOutput<'_, T>,
    opts: &RebinOptions<'_>,
    badval: T,
) -> AstResult<()> {
    let method = format!("astRebin{}", T::CODE);
    let ndim_in = input.lbnd.len();
    let ndim_out = output.lbnd.len();

    // Arrays shorter than their grids, and parameters fewer than the scheme
    // reads, which C cannot check.
    for (name, a, n) in [
        ("input upper bounds", input.ubnd, ndim_in),
        ("input region lower bounds", input.region_lbnd, ndim_in),
        ("input region upper bounds", input.region_ubnd, ndim_in),
        ("output upper bounds", output.ubnd, ndim_out),
    ] {
        if a.len() != n {
            return fail(
                &method,
                ErrorCode::Dimin,
                format!("{} {name} supplied for {n} dimensions.", a.len()),
            );
        }
    }
    // A grid whose bounds are inconsistent is C's error to report.
    let size = |l: &[i64], u: &[i64]| -> Option<usize> {
        l.iter().zip(u).try_fold(1_usize, |n, (&l, &u)| {
            n.checked_mul(usize::try_from(u - l + 1).ok().filter(|&d| d > 0)?)
        })
    };
    let check_len = |what: &str, len: usize, want: Option<usize>| match want {
        Some(want) if len != want => fail(
            &method,
            ErrorCode::Dimin,
            format!("The {what} array has {len} elements but its grid has {want}."),
        ),
        _ => Ok(()),
    };
    check_len("input data", input.data.len(), size(input.lbnd, input.ubnd))?;
    if let Some(v) = input.var {
        check_len("input variance", v.len(), size(input.lbnd, input.ubnd))?;
    }
    check_len(
        "output data",
        output.data.len(),
        size(output.lbnd, output.ubnd),
    )?;
    if let Some(v) = &output.var {
        check_len("output variance", v.len(), size(output.lbnd, output.ubnd))?;
    }
    let nparam = opts.spread.builtin().map_or(0, Builtin::nparam);
    if opts.params.len() < nparam {
        return fail(
            &method,
            ErrorCode::Dimin,
            format!(
                "The {:?} scheme needs {nparam} parameter{} but {} {} given.",
                opts.spread,
                if nparam == 1 { "" } else { "s" },
                opts.params.len(),
                if opts.params.len() == 1 {
                    "was"
                } else {
                    "were"
                }
            ),
        );
    }

    // C reports an error and carries on to zero the output, of one pixel
    // if the error came before it counted them.
    let (checked, npix_out) = match validate(map, input, output, opts, &method) {
        Ok(npix_out) => (Ok(()), npix_out),
        Err((e, npix_out)) => (Err(e), npix_out),
    };
    output
        .data
        .iter_mut()
        .take(npix_out)
        .for_each(|v| *v = T::ZERO);
    if let Some(v) = output.var.as_deref_mut() {
        v.iter_mut().take(npix_out).for_each(|v| *v = T::ZERO);
    }
    checked?;

    let npix: usize = input
        .region_lbnd
        .iter()
        .zip(input.region_ubnd)
        .map(|(l, u)| (u - l + 1) as usize)
        .product();
    // Simplify only when there are enough pixels to make it worthwhile.
    let simple: DynMapping = if npix > 1024 {
        map.simplify()?
    } else {
        map.clone_box()
    };
    if !simple.has_forward() {
        return fail(
            &method,
            ErrorCode::Trnnd,
            "An forward coordinate transformation is not defined by the Mapping supplied."
                .to_string(),
        );
    }

    let mut work = (opts.wlim > 0.0).then(|| vec![0.0; npix_out]);
    let (kernel, neighb, params) = match opts.spread.builtin() {
        Some(b) => {
            let (f, n, p) = kernels::setup(b, opts.params);
            (Some(f), n, p)
        }
        None => (None, 0, Vec::new()),
    };
    let mut ctx = Rebinner {
        map: simple.as_ref(),
        input,
        spread: opts.spread,
        kernel,
        neighb,
        params,
        conwgt: 1.0,
        flags: SpreadFlags {
            use_bad: opts.flags.use_bad,
            use_var: opts.flags.use_var,
            ..Default::default()
        },
        tol: opts.tol,
        maxpix: opts.maxpix,
        badval,
        lbnd_out: output.lbnd,
        ubnd_out: output.ubnd,
        npix_out,
        out: &mut *output.data,
        out_var: output.var.as_deref_mut(),
        work: work.as_deref_mut(),
        nused: None,
    };
    if ctx.adaptively(input.region_lbnd, input.region_ubnd)? {
        return fail(
            &method,
            ErrorCode::Cnflx,
            "Flux conservation was requested but could not be performed because the forward \
             transformation of the supplied Mapping is too non-linear."
                .to_string(),
        );
    }

    // Output pixels whose weights sum to less than wlim are bad.
    if let Some(work) = work {
        for (ipix, w) in work.iter().enumerate() {
            if w.abs() < opts.wlim {
                output.data[ipix] = badval;
                if let Some(v) = output.var.as_deref_mut() {
                    v[ipix] = badval;
                }
            }
        }
    }
    Ok(())
}

fn fail<R>(method: &str, code: ErrorCode, msg: String) -> AstResult<R> {
    Err(AstError::new(code, format!("{method}: {msg}")))
}

/// `Rebin<X>`'s checks of its arguments, in C's order, returning the number
/// of output pixels, or the first error with the number of output pixels C
/// had counted by then.
fn validate<T: RebinValue>(
    map: &dyn Mapping,
    input: &RebinInput<'_, T>,
    output: &RebinOutput<'_, T>,
    opts: &RebinOptions<'_>,
    method: &str,
) -> Result<usize, (AstError, usize)> {
    let err = |code, msg: String| Err((AstError::new(code, format!("{method}: {msg}")), 1));
    let ndim_in = input.lbnd.len();
    let ndim_out = output.lbnd.len();
    let (nin, nout) = (map.nin(), map.nout());
    if ndim_in != nin || ndim_in < 1 {
        return err(
            ErrorCode::Ngdin,
            format!(
                "Bad number of input grid dimensions ({ndim_in}). The Mapping given requires \
                 {nin} coordinate value{} to specify an input position.",
                if nin == 1 { "" } else { "s" }
            ),
        );
    }
    if ndim_out != nout || ndim_out < 1 {
        return err(
            ErrorCode::Ngdin,
            format!(
                "Bad number of output grid dimensions ({ndim_out}). The Mapping given generates \
                 {}{nout} coordinate value{} for each output position.",
                if nout < ndim_out { "only " } else { "" },
                if nout == 1 { "" } else { "s" }
            ),
        );
    }
    let grid = |lbnd: &[i64], ubnd: &[i64], what: &str, axes: &str| {
        let mut mpix: i64 = 1;
        for (idim, (&l, &u)) in lbnd.iter().zip(ubnd).enumerate() {
            if l > u {
                return Err((
                    ErrorCode::Gbdin,
                    format!(
                        "Lower bound of {what} grid ({l}) exceeds corresponding upper bound \
                         ({u}). Error in {axes} dimension {}.",
                        idim + 1
                    ),
                ));
            }
            mpix = mpix.checked_mul(u - l + 1).ok_or((
                ErrorCode::Exspix,
                format!("Supplied {what} array contains too many pixels."),
            ))?;
        }
        Ok(mpix)
    };
    if let Err((code, msg)) = grid(input.lbnd, input.ubnd, "input", "input") {
        return err(code, msg);
    }
    if opts.tol < 0.0 {
        return err(
            ErrorCode::Patin,
            format!(
                "Invalid positional accuracy tolerance ({} pixel). This value should not be \
                 less than zero.",
                opts.tol
            ),
        );
    }
    if let Err((code, msg)) = grid(output.lbnd, output.ubnd, "output", "output") {
        return err(code, msg);
    }
    let mut mpix: i64 = 1;
    for idim in 0..ndim_in {
        let (l, u) = (input.region_lbnd[idim], input.region_ubnd[idim]);
        let detail = if l > u {
            Some(format!(
                "Lower bound of input region ({l}) exceeds corresponding upper bound ({u})."
            ))
        } else if l < input.lbnd[idim] {
            Some(format!(
                "Lower bound of input region ({l}) is less than corresponding bound of input \
                 grid ({}).",
                input.lbnd[idim]
            ))
        } else if u > input.ubnd[idim] {
            Some(format!(
                "Upper bound of input region ({u}) exceeds corresponding bound of input grid \
                 ({}).",
                input.ubnd[idim]
            ))
        } else {
            None
        };
        if let Some(detail) = detail {
            return err(
                ErrorCode::Gbdin,
                format!("{detail} Error in input dimension {}.", idim + 1),
            );
        }
        match mpix.checked_mul(u - l + 1) {
            Some(m) => mpix = m,
            None => {
                return err(
                    ErrorCode::Exspix,
                    "Supplied input region contains too many pixels.".to_string(),
                );
            }
        }
    }
    Ok(output
        .lbnd
        .iter()
        .zip(output.ubnd)
        .map(|(l, u)| (u - l + 1) as usize)
        .product())
}

/// The arguments every level of the rebinning shares.
struct Rebinner<'a, 'b, T> {
    map: &'a dyn Mapping,
    input: &'a RebinInput<'b, T>,
    spread: Spread,
    kernel: Option<KernelFn>,
    neighb: i64,
    params: Vec<f64>,
    /// The weight of every input pixel (C's `conwgt`): 1, or with
    /// `AST__PARWGT` the first of the caller's parameters.
    conwgt: f64,
    flags: SpreadFlags,
    tol: f64,
    maxpix: usize,
    badval: T,
    lbnd_out: &'a [i64],
    ubnd_out: &'a [i64],
    npix_out: usize,
    out: &'a mut [T],
    out_var: Option<&'a mut [T]>,
    work: Option<&'a mut [f64]>,
    nused: Option<&'a mut i64>,
}

/// The most input pixels rebinned at once (`mxpix` in
/// `RebinWithBlocking`).
const MXPIX: i64 = 2 * 1024;

impl<T: RebinValue> Rebinner<'_, '_, T> {
    /// `RebinAdaptively`: rebin the input section `[lbnd, ubnd]`, dividing
    /// it in two along its longest axis while the Mapping is not linear
    /// enough over it. Returns whether flux conservation was impossible
    /// over every part of the section.
    fn adaptively(&mut self, lbnd: &[i64], ubnd: &[i64]) -> AstResult<bool> {
        let ndim_in = lbnd.len();
        let mut npix: i64 = 1;
        let mut mxdim: i64 = 0;
        let mut dimx = 1;
        let mut nvertex: i64 = 1;
        for coord_in in 0..ndim_in {
            let dim = ubnd[coord_in] - lbnd[coord_in] + 1;
            npix *= dim;
            if dim > mxdim {
                mxdim = dim;
                dimx = coord_in;
            }
            nvertex *= 2;
        }

        // The number of sample points astLinearApprox needs. A section with
        // fewer than four times as many pixels is not worth fitting.
        let npoint = 1 + 4 * ndim_in as i64 + 2 * nvertex;
        let toosmall = npix < 4 * npoint;
        let toobig = (i64::try_from(self.maxpix).unwrap_or(i64::MAX)) < mxdim;

        // Flux conservation always needs a fit.
        let mut need_fit = self.flags.conserve_flux;
        let mut divide = if toosmall || self.tol == 0.0 {
            false
        } else if toobig {
            true
        } else {
            need_fit = true;
            true
        };
        let mut linear_fit: Option<Vec<f64>> = None;
        if need_fit {
            // Fit over the pixel edges rather than the centres, so an axis
            // spanned by one pixel still has distinct bounds.
            let flbnd: Vec<f64> = lbnd.iter().map(|&l| l as f64 - 0.5).collect();
            let fubnd: Vec<f64> = ubnd.iter().map(|&u| u as f64 + 0.5).collect();
            linear_fit = self
                .map
                .linear_approx(&flbnd, &fubnd, self.tol)?
                .map(|fit| fit.as_c_layout())
                .filter(|fit| !fit.contains(&BAD_VALUE));
            if linear_fit.is_some() {
                divide = false;
            }
        }

        if !divide {
            self.with_blocking(linear_fit.as_deref(), lbnd, ubnd)
        } else {
            let mut lo = lbnd.to_vec();
            let mut hi = ubnd.to_vec();
            hi[dimx] = (0.5 * (lbnd[dimx] + ubnd[dimx]) as f64).floor() as i64;
            let res1 = self.adaptively(&lo, &hi)?;
            lo[dimx] = hi[dimx] + 1;
            hi[dimx] = ubnd[dimx];
            let res2 = if lo[dimx] <= hi[dimx] {
                self.adaptively(&lo, &hi)?
            } else {
                false
            };
            Ok(res1 && res2)
        }
    }

    /// `RebinWithBlocking`: rebin the section `[lbnd, ubnd]` in blocks of
    /// at most [`MXPIX`] input pixels, shaped like the section. Returns
    /// whether flux conservation was impossible, in which case nothing is
    /// rebinned.
    fn with_blocking(
        &mut self,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
    ) -> AstResult<bool> {
        let ndim_in = lbnd.len();
        let ndim_out = self.lbnd_out.len();
        let mut npix: i64 = 1;
        let mut mxdim_block: i64 = 0;
        for idim in 0..ndim_in {
            let dim = ubnd[idim] - lbnd[idim] + 1;
            npix *= dim;
            if mxdim_block < dim {
                mxdim_block = dim;
            }
        }
        if npix > MXPIX {
            let mut lolim: i64 = 1;
            let mut hilim = mxdim_block;
            while hilim - lolim > 1 {
                mxdim_block = (hilim + lolim) / 2;
                npix = 1;
                for idim in 0..ndim_in {
                    let dim = ubnd[idim] - lbnd[idim] + 1;
                    npix *= dim.min(mxdim_block);
                }
                if npix <= MXPIX {
                    lolim = mxdim_block;
                } else {
                    hilim = mxdim_block;
                }
            }
            mxdim_block = lolim;
        }
        if mxdim_block < 2 {
            mxdim_block = 2;
        }

        let mut dim_block = vec![0_i64; ndim_in];
        let mut lbnd_block = vec![0_i64; ndim_in];
        let mut ubnd_block = vec![0_i64; ndim_in];
        for idim in 0..ndim_in {
            let dim = ubnd[idim] - lbnd[idim] + 1;
            dim_block[idim] = dim.min(mxdim_block);
            lbnd_block[idim] = lbnd[idim];
            ubnd_block[idim] = (lbnd[idim] + dim_block[idim] - 1).min(ubnd[idim]);
        }

        // The factor that conserves flux: the ratio of input to output
        // pixel sizes, from the fit's gradients.
        let mut factor = 1.0;
        if self.flags.conserve_flux {
            match linear_fit {
                Some(fit) => {
                    factor = matrix_det(ndim_out, ndim_in, &fit[ndim_out..]);
                    if factor != 0.0 {
                        factor = 1.0 / factor.abs();
                    } else {
                        return Ok(true);
                    }
                }
                None => return Ok(true),
            }
        }

        loop {
            self.section(linear_fit, &lbnd_block, &ubnd_block, factor)?;
            let mut idim = 0;
            loop {
                if ubnd_block[idim] < ubnd[idim] {
                    lbnd_block[idim] = (lbnd_block[idim] + dim_block[idim]).min(ubnd[idim]);
                    ubnd_block[idim] = (lbnd_block[idim] + dim_block[idim] - 1).min(ubnd[idim]);
                    break;
                }
                lbnd_block[idim] = lbnd[idim];
                ubnd_block[idim] = (lbnd[idim] + dim_block[idim] - 1).min(ubnd[idim]);
                idim += 1;
                if idim == ndim_in {
                    return Ok(false);
                }
            }
        }
    }

    /// `RebinSection`: the output position of each input pixel of the
    /// block `[lbnd, ubnd]`, from the linear fit or the Mapping, and the
    /// spreading of its value there, scaled by `factor`.
    fn section(
        &mut self,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
        factor: f64,
    ) -> AstResult<()> {
        let ndim_in = lbnd.len();
        let ndim_out = self.lbnd_out.len();
        let (lbnd_in, ubnd_in) = (self.input.lbnd, self.input.ubnd);

        let mut npoint = 1_usize;
        for coord_in in 0..ndim_in {
            npoint *= (ubnd[coord_in] - lbnd[coord_in] + 1) as usize;
        }
        let mut offset = vec![0_i64; npoint];
        let mut stride = vec![0_i64; ndim_in];
        let mut s: i64 = 1;
        for coord_in in 0..ndim_in {
            stride[coord_in] = s;
            s *= ubnd_in[coord_in] - lbnd_in[coord_in] + 1;
        }

        // The output position of each input pixel, axis by axis, as C's
        // `ptr_out[coord][point]`.
        let mut coords: Vec<Vec<f64>> = vec![vec![0.0; npoint]; ndim_out];
        let mut point = 0;
        if let Some(linear_fit) = linear_fit {
            let zero = &linear_fit[..ndim_out];
            let grad = &linear_fit[ndim_out..];
            if ndim_in == 1 && ndim_out == 1 {
                // C steps along the row by adding the gradient.
                let mut off = lbnd[0] - lbnd_in[0];
                let mut xx1 = zero[0] + grad[0] * lbnd[0] as f64;
                for _ in lbnd[0]..=ubnd[0] {
                    coords[0][point] = xx1;
                    xx1 += grad[0];
                    offset[point] = off;
                    point += 1;
                    off += 1;
                }
            } else if ndim_in == 2 && ndim_out == 2 {
                // C steps along rows and columns by adding the gradients.
                let mut x1 = zero[0] + grad[1] * (lbnd[1] - 1) as f64;
                let mut y1 = zero[1] + grad[3] * (lbnd[1] - 1) as f64;
                let mut off1 = stride[1] * (lbnd[1] - lbnd_in[1] - 1) - lbnd_in[0];
                for _ in lbnd[1]..=ubnd[1] {
                    x1 += grad[1];
                    y1 += grad[3];
                    off1 += stride[1];
                    let mut xx1 = x1 + grad[0] * lbnd[0] as f64;
                    let mut yy1 = y1 + grad[2] * lbnd[0] as f64;
                    let mut off = off1 + lbnd[0];
                    for _ in lbnd[0]..=ubnd[0] {
                        coords[0][point] = xx1;
                        xx1 += grad[0];
                        coords[1][point] = yy1;
                        yy1 += grad[2];
                        offset[point] = off;
                        point += 1;
                        off += 1;
                    }
                }
            } else {
                // Partial sums of the matrix product for the more
                // significant input axes, recomputed only for those whose
                // index has changed since the previous pixel.
                let mut accum = vec![0.0_f64; ndim_in * ndim_out];
                let mut dim = lbnd.to_vec();
                let mut off: i64 = 0;
                for coord_in in 0..ndim_in {
                    off += stride[coord_in] * (dim[coord_in] - lbnd_in[coord_in]);
                }
                for coord_out in 0..ndim_out {
                    accum[(coord_out + 1) * ndim_in - 1] = zero[coord_out];
                }
                let mut coord_in = ndim_in - 1;
                loop {
                    for coord_out in 0..ndim_out {
                        let i1 = coord_out * ndim_in;
                        for idim in (1..=coord_in).rev() {
                            let i2 = i1 + idim;
                            accum[i2 - 1] = accum[i2] + dim[idim] as f64 * grad[i2];
                        }
                        coords[coord_out][point] = accum[i1] + dim[0] as f64 * grad[i1];
                    }
                    offset[point] = off;
                    point += 1;
                    if !next_index(&mut dim, &mut off, &mut coord_in, lbnd, ubnd, &stride) {
                        break;
                    }
                }
            }
        } else {
            let mut pset_in = PointSet::zeros(npoint, ndim_in);
            {
                let mut v = pset_in.view_mut();
                if ndim_in == 1 && ndim_out == 1 {
                    for ix in lbnd[0]..=ubnd[0] {
                        v[[point, 0]] = ix as f64;
                        offset[point] = ix - lbnd_in[0];
                        point += 1;
                    }
                } else if ndim_in == 2 && ndim_out == 2 {
                    let mut off1 = stride[1] * (lbnd[1] - lbnd_in[1] - 1) - lbnd_in[0];
                    for iy in lbnd[1]..=ubnd[1] {
                        off1 += stride[1];
                        let mut off2 = off1 + lbnd[0];
                        for ix in lbnd[0]..=ubnd[0] {
                            v[[point, 0]] = ix as f64;
                            v[[point, 1]] = iy as f64;
                            offset[point] = off2;
                            point += 1;
                            off2 += 1;
                        }
                    }
                } else {
                    let mut dim = lbnd.to_vec();
                    let mut off: i64 = 0;
                    for coord_in in 0..ndim_in {
                        off += stride[coord_in] * (dim[coord_in] - lbnd_in[coord_in]);
                    }
                    let mut coord_in = 0;
                    loop {
                        for c in 0..ndim_in {
                            v[[point, c]] = dim[c] as f64;
                        }
                        offset[point] = off;
                        point += 1;
                        if !next_index(&mut dim, &mut off, &mut coord_in, lbnd, ubnd, &stride) {
                            break;
                        }
                    }
                }
            }
            let mut pset_out = self.map.transform(&pset_in, true)?;
            substitute_nan_to_bad_in_place(&mut pset_out);
            let v = pset_out.view();
            for (coord_out, column) in coords.iter_mut().enumerate() {
                for (point, c) in column.iter_mut().enumerate() {
                    *c = v[[point, coord_out]];
                }
            }
        }

        let args = spread::Args {
            lbnd_out: self.lbnd_out,
            ubnd_out: self.ubnd_out,
            input: self.input.data,
            in_var: self.input.var,
            infac: factor,
            offset: &offset,
            coords: &coords,
            conwgt: self.conwgt,
            flags: self.flags,
            badval: self.badval,
            npix_out: self.npix_out,
        };
        let mut acc = spread::Accum {
            out: &mut *self.out,
            out_var: self.out_var.as_deref_mut(),
            work: self.work.as_deref_mut(),
            nused: self.nused.as_deref_mut(),
        };
        match (self.spread, self.kernel) {
            (Spread::Nearest, _) => spread::nearest(&args, &mut acc),
            (Spread::Linear, _) => spread::linear(&args, &mut acc),
            (_, Some(kernel)) => {
                spread::kernel1(&args, &mut acc, kernel, self.neighb, &self.params)
            }
            (_, None) => unreachable!("every kernel scheme has a kernel"),
        }
        Ok(())
    }
}

/// Step the pixel indices `dim` to the next pixel of the block
/// `[lbnd, ubnd]`, first axis fastest, keeping `off` its offset in the whole
/// grid. `coord` is left as the most significant axis that changed. Returns
/// false once every pixel has been visited.
fn next_index(
    dim: &mut [i64],
    off: &mut i64,
    coord: &mut usize,
    lbnd: &[i64],
    ubnd: &[i64],
    stride: &[i64],
) -> bool {
    *coord = 0;
    loop {
        let c = *coord;
        if dim[c] < ubnd[c] {
            dim[c] += 1;
            *off += stride[c];
            return true;
        }
        dim[c] = lbnd[c];
        *off -= stride[c] * (ubnd[c] - lbnd[c]);
        *coord += 1;
        if *coord == dim.len() {
            return false;
        }
    }
}
