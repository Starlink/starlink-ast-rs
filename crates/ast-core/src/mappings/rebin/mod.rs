//! Rebinning a gridded array through a Mapping.
//!
//! Each input pixel's center is taken through the Mapping's forward
//! transformation to a position in the output grid, and its value spread over
//! the output pixels around that position. To save transforming every pixel,
//! the input region is divided recursively along its longest axis until the
//! Mapping is linear within the tolerance over a section, or the section is too
//! small to be worth a linear fit; each section's output positions then come
//! from the fit or from the Mapping.

// A port of mapping.c `Rebin<X>`, `RebinSeq<X>` and their drivers. The
// recursive division is `RebinAdaptively`, which asks astLinearApprox whether
// the Mapping is linear within `tol`; each section is then handled in blocks
// of at most 2048 input pixels (`RebinWithBlocking`), whose output positions
// come from the linear fit or the Mapping (`RebinSection`) and are passed to
// the spreading function.
//
// C writes each driver once over a `void *` array and a type code, and each
// spreading function once per type from a macro. The port is generic over
// `RebinValue`, whose items are the macro arguments and the arithmetic the
// macros do in the pixel type.

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

/// The pixel spreading scheme of [`rebin`].
///
/// The kernel schemes share each input pixel between the output pixels within a distance `N` of its position along each axis, weighted by the product of a 1-d kernel evaluated at their offsets along each axis and normalized to sum to one; weight falling off the output grid is reflected back onto it.
/// The kernels and `params` are as for [`Interp`](crate::mappings::Interp)'s variants of the same names.
///
/// # See also
///
/// The constants that select these schemes in `astRebin<X>` and `astRebinSeq<X>`: `AST__NEAREST`, `AST__LINEAR`, `AST__SINC`, `AST__SINCSINC`, `AST__SINCCOS`, `AST__SINCGAUSS`, `AST__GAUSS`, `AST__SOMB` and `AST__SOMBCOS`, one per variant in order.
// The C `spread` argument; C defaults to AST__LINEAR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    /// All of the value to the output pixel containing the position.
    Nearest,
    /// The value shared between the nearest output pixels along each axis in proportion to their nearness.
    Linear,
    /// The kernel `sinc(pi x)`.
    Sinc,
    /// The kernel `sinc(pi x) sinc(k pi x)`.
    SincSinc,
    /// The kernel `sinc(pi x) cos(k pi x)`.
    SincCos,
    /// The kernel `sinc(pi x) exp(-k x^2)`.
    SincGauss,
    /// The kernel `exp(-k x^2)`.
    Gauss,
    /// The kernel `somb(pi x)`.
    Somb,
    /// The kernel `somb(pi x) cos(k pi x)`.
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

/// The flags of [`rebin`].
///
/// # See also
///
/// The `astRebin<X>` flag bits `AST__USEBAD` and `AST__USEVAR`, one per field in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RebinFlags {
    /// Input pixels (and variances) equal to the bad value are bad.
    pub use_bad: bool,
    /// Output variances are formed from the input variances, when both are given.
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
    /// The scheme's parameters, as for the [`Interp`](crate::mappings::Interp) variants of the same names.
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
    /// When positive, output pixels whose weights sum to less than this are set bad.
    pub wlim: f64,
}

/// Rebin the region of `input` into `output` through the forward
/// transformation of `map`.
///
/// `map` takes input grid positions to output grid positions, and must have
/// one input per input grid axis and one output per output grid axis; the
/// two grids may have different numbers of axes.
/// Pixel centers are at integer coordinates, each pixel having unit extent
/// on each axis, and the arrays are stored with the first axis varying
/// fastest.
/// The output arrays are set to zero, and each good input pixel in the
/// region is transformed to the output grid and its value shared among the
/// output pixels around that position, as `opts.spread` says.
/// Each output pixel is the sum of all the contributions it receives.
/// When [`RebinOptions::wlim`] is positive, output pixels whose summed
/// weights fall short of it are set to `badval`.
/// An input pixel equal to `badval` is ignored when
/// [`RebinFlags::use_bad`] is set.
///
/// Output variances, when both grids have them and
/// [`RebinFlags::use_var`] is set, assume that the errors on the input
/// values are independent and that their variances can be summed with
/// suitable weights; even then, the errors on neighboring output values are
/// often correlated, because of the spreading.
///
/// To transform fewer pixels, the Mapping is approximated by linear fits
/// over sections of the input region wherever they are accurate to within
/// [`RebinOptions::tol`] output pixels, as for
/// [`Mapping::tran_grid`], which describes `tol` and
/// [`RebinOptions::maxpix`].
/// A large `tol` can make the edges of the sections visible in an output
/// viewed at high contrast.
///
/// Rebinning can show artifacts that resampling does not: systems of curved
/// lines across the whole output, from beating between the input and output
/// pixel positions, whose strength depends on the Mapping and the spreading
/// scheme.
/// [`Spread::Nearest`] shows them most, and should be used with caution.
/// To rebin a sequence of inputs into one output, [`rebin_seq`] is more
/// efficient.
///
/// # Errors
///
/// - `ErrorCode::Ngdin` when the grids' axis counts do not match the
///   Mapping's inputs and outputs, or a grid has no axes.
/// - `ErrorCode::Gbdin` when a lower bound is above its upper bound, or the
///   region is not inside the input grid.
/// - `ErrorCode::Exspix` when a grid has too many pixels.
/// - `ErrorCode::Patin` when `tol` is negative.
/// - `ErrorCode::Trnnd` when the Mapping has no forward transformation.
/// - `ErrorCode::Dimin` when a data or variance array has the wrong length,
///   bounds differ in length, or `params` has fewer values than the scheme
///   uses.
///
/// The output arrays are still set to zero after any error but
/// `ErrorCode::Dimin`, but only their first element when the axis counts or
/// bounds are in error.
#[doc(alias = "astRebin")]
pub fn rebin<T: RebinValue>(
    map: &dyn Mapping,
    input: &RebinInput<'_, T>,
    output: &mut RebinOutput<'_, T>,
    opts: &RebinOptions<'_>,
    badval: T,
) -> AstResult<()> {
    let method = format!("astRebin{}", T::CODE);
    check_arrays(Some(input), output, &method)?;
    check_params(opts.spread, opts.params, 0, &method)?;

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
    let (kernel, neighb, params) = spread_setup(opts.spread, opts.params);
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

/// The flags of [`rebin_seq`].
///
/// # See also
///
/// The `astRebinSeq<X>` flag bits `AST__USEBAD`, `AST__USEVAR`, `AST__REBININIT`, `AST__REBINEND`, `AST__GENVAR`, `AST__VARWGT`, `AST__DISVAR`, `AST__NONORM`, `AST__PARWGT` and `AST__CONSERVEFLUX`, one per field in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RebinSeqFlags {
    /// Input pixels (and variances) equal to the bad value are bad.
    pub use_bad: bool,
    /// Output variances are formed from the input variances.
    pub use_var: bool,
    /// This is the first call of a sequence: the output arrays, the weights and the count of input pixels used are set to zero before the input is added.
    pub rebin_init: bool,
    /// This is the last call of a sequence: the sums are normalized into the output values and variances.
    pub rebin_end: bool,
    /// Output variances are formed from the spread of the input values added into each output pixel; the weights then need twice as many elements as the output.
    pub gen_var: bool,
    /// Input pixels are weighted by the reciprocals of their variances.
    pub var_wgt: bool,
    /// Variances generated with `gen_var` are of the spread of the input values rather than of their mean.
    pub dis_var: bool,
    /// The sums are left unnormalized, and the weights and count are not needed.
    pub no_norm: bool,
    /// `params[0]` weights every input pixel, and the scheme's parameters follow it.
    pub par_wgt: bool,
    /// Input values are scaled by the ratio of input to output pixel sizes, so that total flux is conserved.
    pub conserve_flux: bool,
}

/// How [`rebin_seq`] spreads.
#[derive(Debug, Clone, Copy)]
pub struct RebinSeqOptions<'a> {
    /// The spreading scheme.
    pub spread: Spread,
    /// The scheme's parameters, preceded by the weight of every input pixel with [`RebinSeqFlags::par_wgt`].
    pub params: &'a [f64],
    /// The flags.
    pub flags: RebinSeqFlags,
    /// As for [`RebinOptions::tol`].
    pub tol: f64,
    /// As for [`RebinOptions::maxpix`].
    pub maxpix: usize,
    /// When the sums are normalized, output pixels whose effective number of contributing input pixels is below this (at least 1e-10) are set bad.
    pub wlim: f64,
}

/// Add the region of `input` into `output` through the forward
/// transformation of `map`, as one of a sequence of calls that forms a
/// mosaic of the inputs.
///
/// Each call spreads the input as [`rebin`] does, but adds the weighted
/// values into `output` rather than overwriting it, adds the weight given to
/// each output pixel into `weights`, and adds the number of input pixels it
/// uses to `nused`.
/// An output pixel's weight is the number of input pixels accumulated in
/// it, counting a fraction of a pixel's value as that fraction of a pixel.
/// The first call of a sequence sets [`RebinSeqFlags::rebin_init`], which
/// sets the output, the weights and the count to zero first; the last sets
/// [`RebinSeqFlags::rebin_end`], which normalizes the sums.
/// The last call may have no input, to normalize only.
///
/// Normalizing divides each output value by its weight, giving the
/// weighted mean of the input values around the output pixel and removing
/// artifacts of aliasing between the grids, and divides each variance by
/// the square of the weight.
/// Output values whose weight is below [`RebinSeqOptions::wlim`] are then
/// set to `badval`; only then are bad output values produced.
/// With [`RebinSeqFlags::conserve_flux`], set on every call of the
/// sequence, normalized values are scaled by the ratio of output to input
/// pixel size, so that the total data value in a feature is kept, which
/// suits data that are a flux per pixel rather than a density.
/// The ratio is found once for each section of the linear approximation, so
/// the joins between sections may show at high contrast; a smaller `tol`
/// reduces this.
///
/// Output variances come either from the input variances
/// ([`RebinSeqFlags::use_var`]), or from the spread of the input values
/// contributing to each output pixel ([`RebinSeqFlags::gen_var`], set on
/// every call of the sequence, which uses the output variances to
/// accumulate the sums it needs until the sequence ends).
/// `weights` has one element per output pixel, or two with
/// [`RebinSeqFlags::gen_var`], the second half accumulating the squares of
/// the weights; with [`RebinSeqFlags::no_norm`], which leaves the sums
/// unnormalized, it and `nused` may be omitted.
///
/// # Errors
///
/// - The errors of [`rebin`].
/// - `ErrorCode::Bdpar` for incompatible flags, flags that need variance
///   arrays that are not given, or missing weights or count.
/// - `ErrorCode::Cnflx` when the Mapping is too non-linear to conserve
///   flux.
/// - `ErrorCode::Badin` when normalizing sums to which no good input pixel
///   contributed.
///
/// The output and weights are still set to zero by
/// [`RebinSeqFlags::rebin_init`], and normalized by
/// [`RebinSeqFlags::rebin_end`], after an error in the arguments.
#[doc(alias = "astRebinSeq")]
pub fn rebin_seq<T: RebinValue>(
    map: &dyn Mapping,
    input: Option<&RebinInput<'_, T>>,
    output: &mut RebinOutput<'_, T>,
    mut weights: Option<&mut [f64]>,
    mut nused: Option<&mut i64>,
    opts: &RebinSeqOptions<'_>,
    badval: T,
) -> AstResult<()> {
    let method = format!("astRebinSeq{}", T::CODE);
    let flags = opts.flags;
    check_arrays(input, output, &method)?;
    check_params(
        opts.spread,
        opts.params,
        usize::from(flags.par_wgt),
        &method,
    )?;
    if let (Some(w), Some(n)) = (weights.as_deref(), grid_size(output.lbnd, output.ubnd)) {
        let want = if flags.gen_var { 2 * n } else { n };
        if w.len() != want {
            return fail(
                &method,
                ErrorCode::Dimin,
                format!(
                    "The weights array has {} elements but {want} are needed.",
                    w.len()
                ),
            );
        }
    }

    // C counts the output pixels before checking the bounds; a count below
    // one sets nothing to zero and normalises nothing.
    let npix_out = output
        .lbnd
        .iter()
        .zip(output.ubnd)
        .fold(1_i64, |n, (l, u)| n.saturating_mul(u - l + 1))
        .max(0) as usize;

    // C reports each error and carries on. Most later checks are made only
    // while no error has been reported; the checks for the weights and the
    // count are made regardless, and their error replaces any earlier one.
    let mut status: Option<AstError> = None;
    let check = |status: &mut Option<AstError>, r: AstResult<()>| {
        if status.is_none() {
            *status = r.err();
        }
    };
    check(&mut status, check_ndim_out(map, output.lbnd.len(), &method));

    // The variances C uses: input variances only with use_var or var_wgt,
    // and output variances only with use_var or gen_var.
    let mut in_var_used = false;
    let mut out_var_used = output.var.is_some();
    if let Some(input) = input {
        check(&mut status, check_ndim_in(map, input.lbnd.len(), &method));
        check(
            &mut status,
            check_grid(input.lbnd, input.ubnd, "input", &method),
        );
        in_var_used = input.var.is_some() && (flags.use_var || flags.var_wgt);
        out_var_used = out_var_used && (flags.use_var || flags.gen_var);
        check(&mut status, check_tol(opts.tol, &method));
        check(
            &mut status,
            check_grid(output.lbnd, output.ubnd, "output", &method),
        );
        check(&mut status, check_region(input, &method));
        let bdpar = |msg: &str| {
            fail(
                &method,
                ErrorCode::Bdpar,
                format!("{msg} (programming error)."),
            )
        };
        if flags.use_var && flags.gen_var {
            check(
                &mut status,
                bdpar(
                    "Incompatible flags AST__GENVAR and AST__USEVAR have been specified together",
                ),
            );
        }
        if !in_var_used {
            if flags.use_var {
                check(
                    &mut status,
                    bdpar(
                        "The AST__USEVAR flag was specified but no input variance array was \
                         supplied",
                    ),
                );
            } else if flags.var_wgt {
                check(
                    &mut status,
                    bdpar(
                        "The AST__VARWGT flag was specified but no input variance array was \
                         supplied",
                    ),
                );
            }
        }
        if !out_var_used {
            if flags.use_var {
                check(
                    &mut status,
                    bdpar(
                        "The AST__USEVAR flag was specified but no output variance array was \
                         supplied",
                    ),
                );
            } else if flags.gen_var {
                check(
                    &mut status,
                    bdpar(
                        "The AST__GENVAR flag was specified but no output variance array was \
                         supplied",
                    ),
                );
            }
        }
        if flags.no_norm {
            for (set, name) in [
                (flags.gen_var, "AST__GENVAR"),
                (flags.var_wgt, "AST__VARWGT"),
                (flags.par_wgt, "AST__PARWGT"),
                (flags.conserve_flux, "AST__CONSERVEFLUX"),
            ] {
                if set {
                    check(
                        &mut status,
                        bdpar(&format!(
                            "Incompatible flags {name} and AST__NONORM have been specified \
                             together"
                        )),
                    );
                    break;
                }
            }
        } else if weights.is_none() {
            status = bdpar("No weights array supplied").err();
        } else if nused.is_none() {
            status = bdpar("No 'nused' pointer supplied").err();
        }

        let mut simple: Option<DynMapping> = None;
        if status.is_none() {
            let npix: usize = input
                .region_lbnd
                .iter()
                .zip(input.region_ubnd)
                .map(|(l, u)| (u - l + 1) as usize)
                .product();
            // Simplify only when there are enough pixels to make it
            // worthwhile.
            match if npix > 1024 {
                map.simplify()
            } else {
                Ok(map.clone_box())
            } {
                Ok(m) => simple = Some(m),
                Err(e) => status = Some(e),
            }
        }
        if let Some(m) = &simple
            && !m.has_forward()
        {
            check(
                &mut status,
                fail(
                    &method,
                    ErrorCode::Trnnd,
                    "An forward coordinate transformation is not defined by the Mapping \
                     supplied."
                        .to_string(),
                ),
            );
        }

        if flags.rebin_init {
            output
                .data
                .iter_mut()
                .take(npix_out)
                .for_each(|v| *v = T::ZERO);
            if out_var_used && let Some(v) = output.var.as_deref_mut() {
                v.iter_mut().take(npix_out).for_each(|v| *v = T::ZERO);
            }
            if let Some(w) = weights.as_deref_mut() {
                let n = if flags.gen_var {
                    2 * npix_out
                } else {
                    npix_out
                };
                w.iter_mut().take(n).for_each(|w| *w = 0.0);
            }
            if let Some(n) = nused.as_deref_mut() {
                *n = 0;
            }
        }

        if let (None, Some(simple)) = (&status, &simple) {
            // With AST__PARWGT the first parameter weights every pixel.
            let (conwgt, params) = if flags.par_wgt {
                (opts.params[0], &opts.params[1..])
            } else {
                (1.0, opts.params)
            };
            let (kernel, neighb, params) = spread_setup(opts.spread, params);
            let seq_input = RebinInput {
                var: if in_var_used { input.var } else { None },
                ..*input
            };
            let mut ctx = Rebinner {
                map: simple.as_ref(),
                input: &seq_input,
                spread: opts.spread,
                kernel,
                neighb,
                params,
                conwgt,
                flags: SpreadFlags {
                    use_bad: flags.use_bad,
                    use_var: flags.use_var,
                    gen_var: flags.gen_var,
                    var_wgt: flags.var_wgt,
                    conserve_flux: flags.conserve_flux,
                },
                tol: opts.tol,
                maxpix: opts.maxpix,
                badval,
                lbnd_out: output.lbnd,
                ubnd_out: output.ubnd,
                npix_out,
                out: &mut *output.data,
                out_var: if out_var_used {
                    output.var.as_deref_mut()
                } else {
                    None
                },
                work: weights.as_deref_mut(),
                nused: nused.as_deref_mut(),
            };
            match ctx.adaptively(input.region_lbnd, input.region_ubnd) {
                Ok(false) => {}
                Ok(true) => {
                    status = fail::<()>(
                        &method,
                        ErrorCode::Cnflx,
                        "Flux conservation was requested but could not be performed because \
                         the forward transformation of the supplied Mapping is too non-linear."
                            .to_string(),
                    )
                    .err();
                }
                Err(e) => status = Some(e),
            }
        }
    }

    if flags.rebin_end
        && !flags.no_norm
        && let (Some(weights), Some(nused)) = (weights.as_deref(), nused.as_deref())
    {
        let out_var = if out_var_used {
            output.var.as_deref_mut()
        } else {
            None
        };
        let in_var_given = input.is_some_and(|_| in_var_used);
        if !normalise(
            &flags,
            opts.wlim,
            npix_out,
            &mut *output.data,
            out_var,
            weights,
            *nused,
            badval,
        ) && status.is_none()
        {
            status = Some(AstError::new(
                ErrorCode::Badin,
                format!(
                    "{method}: The overlap between the {}-d input array and the {}-d output \
                     array contains no pixels with good data {}values.",
                    map.nin(),
                    map.nout(),
                    if in_var_given { "and variance " } else { "" }
                ),
            ));
        }
    }
    match status {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The final normalization of `RebinSeq<X>`: each output value divided by
/// its weight, and its variance formed, or set bad where the effective
/// number of contributing input pixels is below `wlim`. Without
/// `gen_var`, that number is the weight over the mean weight per input
/// pixel, which C estimates from the weights by iterated 3-sigma clipping.
/// Returns false if no input pixel contributed. C then reads an
/// uninitialised effective number of pixels; the port takes it as zero,
/// setting every output pixel bad, which is what C produces.
#[allow(clippy::too_many_arguments)]
fn normalise<T: RebinValue>(
    flags: &RebinSeqFlags,
    wlim: f64,
    npix_out: usize,
    out: &mut [T],
    mut out_var: Option<&mut [T]>,
    weights: &[f64],
    nused: i64,
    badval: T,
) -> bool {
    let wlim = if wlim < 1.0E-10 { 1.0E-10 } else { wlim };
    let mut mwpip = BAD_VALUE;
    if !flags.gen_var && nused > 0 && npix_out > 0 {
        let mut wlo = 0.0;
        let mut whi = f64::MAX;
        let mut nwlim: i64 = -1;
        let mut more = true;
        while more {
            let mut sw = 0.0;
            let mut sw2 = 0.0;
            let mut nw: i64 = 0;
            for &w in &weights[..npix_out] {
                if w > wlo && w <= whi {
                    sw += w;
                    sw2 += w * w;
                    nw += 1;
                }
            }
            if nwlim == -1 {
                nwlim = nw / 2;
            }
            if nw > nwlim {
                let newval = sw / nw as f64;
                if mwpip != BAD_VALUE {
                    more = (newval - mwpip).abs() > 0.05 * mwpip;
                }
                mwpip = newval;
                let mut std = sw2 / nw as f64 - mwpip * mwpip;
                std = if std > 0.0 {
                    crate::math::sqrt(std)
                } else {
                    0.0
                };
                wlo = mwpip - 3.0 * std;
                if wlo < 0.0 {
                    wlo = 0.0;
                }
                whi = mwpip + 3.0 * std;
            } else {
                more = false;
            }
        }
        mwpip *= (2.0 * nwlim as f64) / nused as f64;
    }
    let contributed = flags.gen_var || mwpip != BAD_VALUE;

    for i in 0..npix_out {
        let wgt = weights[i];
        let neff = if flags.gen_var {
            if wgt > 0.0 && weights[i + npix_out] > 0.0 {
                (wgt * wgt) / weights[i + npix_out]
            } else {
                0.0
            }
        } else if contributed {
            wgt / mwpip
        } else {
            0.0
        };
        if neff < wlim || neff == 0.0 {
            out[i] = badval;
            if let Some(v) = out_var.as_deref_mut() {
                v[i] = badval;
            }
        } else {
            out[i] = T::from_c(out[i].to_f64() / wgt);
            if let Some(v) = out_var.as_deref_mut() {
                if flags.use_var {
                    v[i] = T::from_c(v[i].to_f64() / (wgt * wgt));
                } else if flags.gen_var && neff > 1.0 {
                    v[i] = T::from_c(v[i].to_f64() / wgt);
                    v[i] = v[i].sub_square(out[i]);
                    if v[i].to_f64() < 0.0 {
                        v[i] = T::ZERO;
                    }
                    let factor = if flags.dis_var {
                        neff / (neff - 1.0)
                    } else {
                        1.0 / (neff - 1.0)
                    };
                    v[i] = T::from_c(v[i].to_f64() * factor);
                } else {
                    v[i] = badval;
                }
            }
        }
    }
    contributed
}

/// The number of pixels in a grid, or `None` if its bounds are
/// inconsistent, which is C's error to report.
fn grid_size(l: &[i64], u: &[i64]) -> Option<usize> {
    l.iter().zip(u).try_fold(1_usize, |n, (&l, &u)| {
        n.checked_mul(usize::try_from(u - l + 1).ok().filter(|&d| d > 0)?)
    })
}

/// Bounds of different lengths, and arrays of the wrong length for their
/// grids, which C cannot check.
fn check_arrays<T>(
    input: Option<&RebinInput<'_, T>>,
    output: &RebinOutput<'_, T>,
    method: &str,
) -> AstResult<()> {
    let ndim_out = output.lbnd.len();
    let mut bounds = vec![("output upper bounds", output.ubnd, ndim_out)];
    if let Some(input) = input {
        let ndim_in = input.lbnd.len();
        bounds.extend([
            ("input upper bounds", input.ubnd, ndim_in),
            ("input region lower bounds", input.region_lbnd, ndim_in),
            ("input region upper bounds", input.region_ubnd, ndim_in),
        ]);
    }
    for (name, a, n) in bounds {
        if a.len() != n {
            return fail(
                method,
                ErrorCode::Dimin,
                format!("{} {name} supplied for {n} dimensions.", a.len()),
            );
        }
    }
    let check_len = |what: &str, len: usize, want: Option<usize>| match want {
        Some(want) if len != want => fail(
            method,
            ErrorCode::Dimin,
            format!("The {what} array has {len} elements but its grid has {want}."),
        ),
        _ => Ok(()),
    };
    if let Some(input) = input {
        let n_in = grid_size(input.lbnd, input.ubnd);
        check_len("input data", input.data.len(), n_in)?;
        if let Some(v) = input.var {
            check_len("input variance", v.len(), n_in)?;
        }
    }
    let n_out = grid_size(output.lbnd, output.ubnd);
    check_len("output data", output.data.len(), n_out)?;
    if let Some(v) = &output.var {
        check_len("output variance", v.len(), n_out)?;
    }
    Ok(())
}

/// Fewer `params` than the scheme reads (after `extra` leading ones),
/// which C cannot check.
fn check_params(spread: Spread, params: &[f64], extra: usize, method: &str) -> AstResult<()> {
    let nparam = spread.builtin().map_or(0, Builtin::nparam) + extra;
    if params.len() < nparam {
        return fail(
            method,
            ErrorCode::Dimin,
            format!(
                "The {spread:?} scheme needs {nparam} parameter{} but {} {} given.",
                if nparam == 1 { "" } else { "s" },
                params.len(),
                if params.len() == 1 { "was" } else { "were" }
            ),
        );
    }
    Ok(())
}

/// `RebinSection`'s set-up of a spreading scheme: the kernel, the number of
/// neighboring pixels and the kernel's parameters.
fn spread_setup(spread: Spread, params: &[f64]) -> (Option<KernelFn>, i64, Vec<f64>) {
    match spread.builtin() {
        Some(b) => {
            let (f, n, p) = kernels::setup(b, params);
            (Some(f), n, p)
        }
        None => (None, 0, Vec::new()),
    }
}

fn fail<R>(method: &str, code: ErrorCode, msg: String) -> AstResult<R> {
    Err(AstError::new(code, format!("{method}: {msg}")))
}

/// The checks `Rebin<X>` and `RebinSeq<X>` both make, each reporting as
/// C does.
fn check_ndim_in(map: &dyn Mapping, ndim_in: usize, method: &str) -> AstResult<()> {
    let nin = map.nin();
    if ndim_in != nin || ndim_in < 1 {
        return fail(
            method,
            ErrorCode::Ngdin,
            format!(
                "Bad number of input grid dimensions ({ndim_in}). The Mapping given requires \
                 {nin} coordinate value{} to specify an input position.",
                if nin == 1 { "" } else { "s" }
            ),
        );
    }
    Ok(())
}

fn check_ndim_out(map: &dyn Mapping, ndim_out: usize, method: &str) -> AstResult<()> {
    let nout = map.nout();
    if ndim_out != nout || ndim_out < 1 {
        return fail(
            method,
            ErrorCode::Ngdin,
            format!(
                "Bad number of output grid dimensions ({ndim_out}). The Mapping given generates \
                 {}{nout} coordinate value{} for each output position.",
                if nout < ndim_out { "only " } else { "" },
                if nout == 1 { "" } else { "s" }
            ),
        );
    }
    Ok(())
}

/// The bounds of the input or output grid (`what`).
fn check_grid(lbnd: &[i64], ubnd: &[i64], what: &str, method: &str) -> AstResult<()> {
    let mut mpix: i64 = 1;
    for (idim, (&l, &u)) in lbnd.iter().zip(ubnd).enumerate() {
        if l > u {
            return fail(
                method,
                ErrorCode::Gbdin,
                format!(
                    "Lower bound of {what} grid ({l}) exceeds corresponding upper bound ({u}). \
                     Error in {what} dimension {}.",
                    idim + 1
                ),
            );
        }
        match mpix.checked_mul(u - l + 1) {
            Some(m) => mpix = m,
            None => {
                return fail(
                    method,
                    ErrorCode::Exspix,
                    format!("Supplied {what} array contains too many pixels."),
                );
            }
        }
    }
    Ok(())
}

fn check_tol(tol: f64, method: &str) -> AstResult<()> {
    if tol < 0.0 {
        return fail(
            method,
            ErrorCode::Patin,
            format!(
                "Invalid positional accuracy tolerance ({tol} pixel). This value should not be \
                 less than zero."
            ),
        );
    }
    Ok(())
}

/// The input region, which must lie within the input grid.
fn check_region<T>(input: &RebinInput<'_, T>, method: &str) -> AstResult<()> {
    let mut mpix: i64 = 1;
    for idim in 0..input.lbnd.len() {
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
            return fail(
                method,
                ErrorCode::Gbdin,
                format!("{detail} Error in input dimension {}.", idim + 1),
            );
        }
        match mpix.checked_mul(u - l + 1) {
            Some(m) => mpix = m,
            None => {
                return fail(
                    method,
                    ErrorCode::Exspix,
                    "Supplied input region contains too many pixels.".to_string(),
                );
            }
        }
    }
    Ok(())
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
    let checks = || -> AstResult<()> {
        check_ndim_in(map, input.lbnd.len(), method)?;
        check_ndim_out(map, output.lbnd.len(), method)?;
        check_grid(input.lbnd, input.ubnd, "input", method)?;
        check_tol(opts.tol, method)?;
        check_grid(output.lbnd, output.ubnd, "output", method)?;
        check_region(input, method)
    };
    checks().map_err(|e| (e, 1))?;
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
                        v[[0, point]] = ix as f64;
                        offset[point] = ix - lbnd_in[0];
                        point += 1;
                    }
                } else if ndim_in == 2 && ndim_out == 2 {
                    let mut off1 = stride[1] * (lbnd[1] - lbnd_in[1] - 1) - lbnd_in[0];
                    for iy in lbnd[1]..=ubnd[1] {
                        off1 += stride[1];
                        let mut off2 = off1 + lbnd[0];
                        for ix in lbnd[0]..=ubnd[0] {
                            v[[0, point]] = ix as f64;
                            v[[1, point]] = iy as f64;
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
                            v[[c, point]] = dim[c] as f64;
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
                    *c = v[[coord_out, point]];
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
