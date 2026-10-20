//! Resampling a gridded array through a Mapping.
//!
//! Each output pixel's centre is taken back through the Mapping's inverse to
//! a position in the input grid, and the input is interpolated there. To save
//! transforming every pixel, the output region is divided recursively along
//! its longest axis until the inverse is linear within the tolerance over a
//! section, or the section is too small to be worth a linear fit; each
//! section's input positions then come from the fit or from the Mapping.

// A port of mapping.c `Resample<X>` and its drivers. The recursive division is
// `ResampleAdaptively`, which asks astLinearApprox whether the inverse is
// linear within `tol`; each section is then handled in blocks of at most 2048
// output pixels (`ResampleWithBlocking`), whose input positions come from the
// linear fit or the Mapping (`ResampleSection`) and are passed to the
// interpolator.
//
// C writes each driver once over a `void *` array and a type code, and each
// interpolator once per type from a macro. The port is generic over
// `PixelValue`, whose items are the macro arguments.

// The loops mirror the C source index for index.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]

mod interp;
pub(crate) mod kernels;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, Mapping, substitute_nan_to_bad_in_place};
use crate::pixel::PixelValue;

use kernels::Builtin;

/// The sub-pixel interpolation scheme of [`resample`] (C's `interp` and
/// `finterp`), for pixel type `T`.
///
/// The kernel schemes weight the input pixels within a distance `N` of the
/// position along each axis by the product of a 1-d kernel evaluated at
/// their offsets along each axis. `N` is `params[0]` rounded to the nearest
/// integer; where it is zero or less a default applies, as the variants
/// say. Where a variant uses `params[1]`, `params` needs two elements.
pub enum Interp<'a, T> {
    /// The value of the input pixel whose centre is nearest
    /// (`AST__NEAREST`).
    Nearest,
    /// Linear interpolation between the nearest input pixels along each axis
    /// (`AST__LINEAR`, C's default).
    Linear,
    /// The kernel `sinc(pi x)` (`AST__SINC`); `N` defaults to 2.
    Sinc,
    /// The kernel `sinc(pi x) sinc(k pi x)`, out to the first zero of the
    /// second factor, where `params[1]` is that zero's offset and `k` is a
    /// half over it (`AST__SINCSINC`). `N` is at most `params[1]` rounded
    /// up, and defaults to it.
    SincSinc,
    /// The kernel `sinc(pi x) cos(k pi x)`, out to the first zero of the
    /// second factor, defined as for [`Interp::SincSinc`]
    /// (`AST__SINCCOS`).
    SincCos,
    /// The kernel `sinc(pi x) exp(-k x^2)`, where `params[1]` is the full
    /// width at half maximum of the Gaussian (at least 0.1) (`AST__SINCGAUSS`).
    /// `N` defaults to where the Gaussian falls to 1% of its peak.
    SincGauss,
    /// The kernel `exp(-k x^2)`, with `params[1]` and `N` as for
    /// [`Interp::SincGauss`] (`AST__GAUSS`).
    Gauss,
    /// The kernel `somb(pi x)`, that is `2 J1(pi x) / (pi x)` (`AST__SOMB`);
    /// `N` defaults to 2.
    Somb,
    /// The kernel `somb(pi x) cos(k pi x)`, with `params[1]` and `N` as for
    /// [`Interp::SincCos`] (`AST__SOMBCOS`). As in C, the first factor is
    /// `J1(pi x) / (pi x)`, half of [`Interp::Somb`]'s, except at zero
    /// offset, where it is 1.
    SombCos,
    /// The mean of the input pixels in the block reaching `params[0]`
    /// pixels (rounded) either side of the position along each axis,
    /// weighted by the reciprocals of their variances when variances are
    /// resampled (`AST__BLOCKAVE`).
    BlockAve,
    /// A 1-d kernel of the caller's (`AST__UKERN1`), given each pixel's
    /// offset along an axis, `params` and the flags. `N` is at least 1.
    UKern1(Kernel1Fn<'a>),
    /// An interpolator of the caller's (`AST__UINTERP`). It is given the
    /// input and the positions of a block of output pixels, sets each output
    /// pixel (and variance, if any) at its offset, and returns the number
    /// it set bad.
    UInterp(InterpFn<'a, T>),
}

/// A caller's 1-d kernel ([`Interp::UKern1`]): the kernel's value at an
/// offset, given the offset, the parameters and the flags.
///
/// The offset is that of an input pixel from the interpolation point, in
/// pixels, and may be negative; most kernels ignore its sign.
/// In more than one dimension, a pixel's weight is the product of the
/// kernel's values at its offsets along each axis.
/// `params` are those given to [`resample`], whose first value has already
/// set how many neighboring pixels contribute.
/// A good kernel is usually symmetric about zero, peaks (usually at 1) at
/// zero, and is zero at every other integer offset, so that the
/// interpolated values pass through the data; it may be negative in places.
/// An error returned by the kernel ends the resampling with that error.
#[doc(alias = "astUkern1")]
pub type Kernel1Fn<'a> = &'a dyn Fn(f64, &[f64], ResampleFlags) -> AstResult<f64>;

/// A caller's interpolator ([`Interp::UInterp`]): given the input and the
/// positions of a block of output pixels, it sets the output values (and
/// variances, if given) and returns the number it set bad.
///
/// It may be called more than once per call of [`resample`].
/// It should put each output point's value in the output array at the
/// point's offset ([`InterpArgs::offset`]), and leave the elements no point
/// names as they are.
/// It should give a point with a bad input coordinate `badval`, or leave it
/// as it is when [`ResampleFlags::no_bad`] is set, and treat input pixels
/// equal to `badval` as bad only when [`ResampleFlags::use_bad`] is set.
/// Output variances should assume that the errors on the input values are
/// independent and that their variances can be summed with suitable
/// weights.
/// The count it returns is of points for which no value could be found, from
/// zero to the number of points.
/// An error returned by the interpolator ends the resampling with that
/// error.
#[doc(alias = "astUinterp")]
pub type InterpFn<'a, T> =
    &'a dyn Fn(&InterpArgs<'_, T>, &mut [T], Option<&mut [T]>) -> AstResult<usize>;

impl<T> Clone for Interp<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Interp<'_, T> {}

impl<T> std::fmt::Debug for Interp<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Interp::Nearest => "Nearest",
            Interp::Linear => "Linear",
            Interp::Sinc => "Sinc",
            Interp::SincSinc => "SincSinc",
            Interp::SincCos => "SincCos",
            Interp::SincGauss => "SincGauss",
            Interp::Gauss => "Gauss",
            Interp::Somb => "Somb",
            Interp::SombCos => "SombCos",
            Interp::BlockAve => "BlockAve",
            Interp::UKern1(_) => "UKern1",
            Interp::UInterp(_) => "UInterp",
        })
    }
}

/// The flags of [`resample`] (C's `flags`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResampleFlags {
    /// Input pixels equal to the bad value are bad (`AST__USEBAD`).
    pub use_bad: bool,
    /// Output pixels that would be bad are left as they are
    /// (`AST__NOBAD`).
    pub no_bad: bool,
    /// Output values are scaled by the ratio of output to input pixel
    /// sizes, and variances by its square, so that total flux is conserved
    /// (`AST__CONSERVEFLUX`). It needs a non-zero `tol`, a Mapping with as
    /// many inputs as outputs and linear enough to fit within `tol`, and
    /// cannot be used with `no_bad`.
    pub conserve_flux: bool,
}

/// What an interpolator is given: the input grid, and for each output
/// point its input position and its offset in the output array.
#[derive(Debug, Clone, Copy)]
pub struct InterpArgs<'a, T> {
    /// The lowest pixel index of the input grid on each axis.
    pub lbnd_in: &'a [i64],
    /// The highest pixel index of the input grid on each axis.
    pub ubnd_in: &'a [i64],
    /// The input data.
    pub input: &'a [T],
    /// The input variances, when variances are resampled.
    pub in_var: Option<&'a [T]>,
    /// The offset of each output point in the output arrays.
    pub offset: &'a [i64],
    /// The input position of each output point: `coords[axis][point]`,
    /// bad where there is none.
    pub coords: &'a [Vec<f64>],
    /// The parameters given to [`resample`].
    pub params: &'a [f64],
    /// The flags given to [`resample`].
    pub flags: ResampleFlags,
    /// The bad value.
    pub badval: T,
}

/// An input grid: its pixel index bounds, with the first axis varying
/// fastest, its data and, optionally, its variances.
#[derive(Debug, Clone, Copy)]
pub struct ResampleInput<'a, T> {
    /// The lowest pixel index on each axis.
    pub lbnd: &'a [i64],
    /// The highest pixel index on each axis.
    pub ubnd: &'a [i64],
    /// The data, one value per pixel.
    pub data: &'a [T],
    /// The variances, one per pixel. Variances are resampled only when both
    /// this and the output's are given.
    pub var: Option<&'a [T]>,
}

/// An output grid and the region of it to fill.
#[derive(Debug)]
pub struct ResampleOutput<'a, T> {
    /// The lowest pixel index on each axis of the grid.
    pub lbnd: &'a [i64],
    /// The highest pixel index on each axis of the grid.
    pub ubnd: &'a [i64],
    /// The lowest pixel index on each axis of the region to fill.
    pub region_lbnd: &'a [i64],
    /// The highest pixel index on each axis of the region to fill.
    pub region_ubnd: &'a [i64],
    /// The data, one value per pixel of the grid.
    pub data: &'a mut [T],
    /// The variances, one per pixel of the grid.
    pub var: Option<&'a mut [T]>,
}

/// How [`resample`] interpolates.
#[derive(Debug, Clone, Copy)]
pub struct ResampleOptions<'a, T> {
    /// The interpolation scheme.
    pub interp: Interp<'a, T>,
    /// The scheme's parameters (C's `params`), as [`Interp`] describes.
    pub params: &'a [f64],
    /// The flags.
    pub flags: ResampleFlags,
    /// The largest positional error, in input pixels, a linear
    /// approximation to the Mapping may introduce; 0 uses the Mapping for
    /// every pixel.
    pub tol: f64,
    /// The largest extent, in output pixels, over which a linear
    /// approximation is first tried.
    pub maxpix: usize,
}

/// Resample `input` into the region of `output` through the inverse of
/// `map`, returning the number of output pixels (and variances) given the
/// bad value.
///
/// `map` takes input grid positions to output grid positions, and must have
/// one input per input grid axis and one output per output grid axis; the
/// two grids may have different numbers of axes.
/// Pixel centres are at integer coordinates, each pixel having unit extent
/// on each axis, and the arrays are stored with the first axis varying
/// fastest.
/// Each output pixel's centre in the region is transformed to the input
/// grid, and the input interpolated there as `opts.interp` says.
///
/// This samples the input rather than integrating it, so the total data
/// value is not in general kept.
/// [`ResampleFlags::conserve_flux`] scales the output to keep it
/// approximately, by the ratio of output to input pixel size, found once for
/// each section of the linear approximation, so the joins between sections
/// may show at high contrast; for accurate flux conservation use
/// [`rebin`](crate::mappings::rebin()) or
/// [`rebin_seq`](crate::mappings::rebin_seq).
///
/// An output pixel is set to `badval` (or left as it is with
/// [`ResampleFlags::no_bad`], and counted either way) when its input
/// position is bad or outside the input grid, falls in a bad input pixel
/// (one equal to `badval`, with [`ResampleFlags::use_bad`]), has
/// neighboring input pixels unsuitable for the scheme, or gives a value the
/// pixel type cannot represent.
/// A position on a pixel boundary belongs to the pixel with the larger
/// index.
/// An output variance is bad when its value is, or when the contributing
/// input variances include a bad or negative one.
/// Output variances, formed when both grids have them, assume that the
/// errors on the input values are independent.
///
/// To transform fewer pixels, the inverse is approximated by linear fits
/// over sections of the output region wherever they are accurate to within
/// [`ResampleOptions::tol`] input pixels, as for [`Mapping::tran_grid`],
/// which describes `tol` and [`ResampleOptions::maxpix`].
///
/// # Errors
///
/// - `ErrorCode::Ngdin` when the grids' axis counts do not match the
///   Mapping's inputs and outputs, or a grid has no axes.
/// - `ErrorCode::Gbdin` when a lower bound is above its upper bound, or the
///   region is not inside the output grid.
/// - `ErrorCode::Exspix` when a grid has too many pixels.
/// - `ErrorCode::Patin` when `tol` is negative.
/// - `ErrorCode::Trnnd` when the Mapping has no inverse transformation.
/// - `ErrorCode::Dimin` when a data or variance array has the wrong length,
///   bounds differ in length, or `params` has fewer values than the scheme
///   uses.
/// - `ErrorCode::Cnflx` when flux conservation is asked for with a zero
///   `tol`, a Mapping with different numbers of inputs and outputs, or a
///   section of the output over which no linear fit is found.
/// - `ErrorCode::Badflg` when flux conservation is asked for with
///   [`ResampleFlags::no_bad`].
/// - An error from a caller's kernel or interpolator is returned with
///   context added.
#[doc(alias = "astResample")]
pub fn resample<T: PixelValue>(
    map: &dyn Mapping,
    input: &ResampleInput<'_, T>,
    output: &mut ResampleOutput<'_, T>,
    opts: &ResampleOptions<'_, T>,
    badval: T,
) -> AstResult<usize> {
    let method = format!("astResample{}", T::CODE);

    let ndim_in = input.lbnd.len();
    let ndim_out = output.lbnd.len();
    let nin = map.nin();
    let nout = map.nout();
    if ndim_in != nin || ndim_in < 1 {
        return fail(
            &method,
            ErrorCode::Ngdin,
            format!(
                "Bad number of input grid dimensions ({ndim_in}). The Mapping given requires \
                 {nin} coordinate value{} to specify an input position.",
                if nin == 1 { "" } else { "s" }
            ),
        );
    }
    if ndim_out != nout || ndim_out < 1 {
        return fail(
            &method,
            ErrorCode::Ngdin,
            format!(
                "Bad number of output grid dimensions ({ndim_out}). The Mapping given \
                 generates {}{nout} coordinate value{} for each output position.",
                if nout < ndim_out { "only " } else { "" },
                if nout == 1 { "" } else { "s" }
            ),
        );
    }
    for (name, a, n) in [
        ("input upper bounds", input.ubnd, ndim_in),
        ("output upper bounds", output.ubnd, ndim_out),
        ("output region lower bounds", output.region_lbnd, ndim_out),
        ("output region upper bounds", output.region_ubnd, ndim_out),
    ] {
        if a.len() != n {
            return fail(
                &method,
                ErrorCode::Dimin,
                format!("{} {name} supplied for {n} dimensions.", a.len()),
            );
        }
    }

    let npix_in =
        grid_size(input.lbnd, input.ubnd, "input grid").map_err(|m| prefix(&method, m))?;
    if opts.tol < 0.0 {
        return fail(
            &method,
            ErrorCode::Patin,
            format!(
                "Invalid positional accuracy tolerance ({} pixel). This value should not be \
                 less than zero.",
                opts.tol
            ),
        );
    }
    let npix_out =
        grid_size(output.lbnd, output.ubnd, "output grid").map_err(|m| prefix(&method, m))?;
    let mut npix: i64 = 1;
    for idim in 0..ndim_out {
        let (l, u) = (output.region_lbnd[idim], output.region_ubnd[idim]);
        let detail = if l > u {
            Some(format!(
                "Lower bound of output region ({l}) exceeds corresponding upper bound ({u})."
            ))
        } else if l < output.lbnd[idim] {
            Some(format!(
                "Lower bound of output region ({l}) is less than corresponding bound of \
                 output grid ({}).",
                output.lbnd[idim]
            ))
        } else if u > output.ubnd[idim] {
            Some(format!(
                "Upper bound of output region ({u}) exceeds corresponding bound of output \
                 grid ({}).",
                output.ubnd[idim]
            ))
        } else {
            None
        };
        if let Some(detail) = detail {
            return fail(
                &method,
                ErrorCode::Gbdin,
                format!("{detail} Error in output dimension {}.", idim + 1),
            );
        }
        npix = npix.checked_mul(u - l + 1).ok_or_else(|| {
            AstError::new(
                ErrorCode::Exspix,
                format!("{method}: Supplied output region contains too many pixels."),
            )
        })?;
    }

    if opts.flags.conserve_flux {
        if opts.tol == 0.0 {
            return fail(
                &method,
                ErrorCode::Cnflx,
                "Flux conservation was requested but cannot be performed because zero \
                 tolerance was also specified."
                    .to_string(),
            );
        } else if nin != nout {
            return fail(
                &method,
                ErrorCode::Cnflx,
                "Flux conservation was requested but cannot be performed because the Mapping \
                 has different numbers of inputs and outputs."
                    .to_string(),
            );
        }
    }

    // Arrays shorter than their grids, and parameters fewer than the scheme
    // reads, which C cannot check.
    let check_len = |what: &str, len: usize, want: usize| {
        if len != want {
            fail(
                &method,
                ErrorCode::Dimin,
                format!("The {what} array has {len} elements but its grid has {want}."),
            )
        } else {
            Ok(())
        }
    };
    check_len("input data", input.data.len(), npix_in)?;
    if let Some(v) = input.var {
        check_len("input variance", v.len(), npix_in)?;
    }
    check_len("output data", output.data.len(), npix_out)?;
    if let Some(v) = &output.var {
        check_len("output variance", v.len(), npix_out)?;
    }
    let nparam = match opts.interp {
        Interp::Nearest | Interp::Linear | Interp::UInterp(_) => 0,
        Interp::Sinc | Interp::Somb | Interp::BlockAve | Interp::UKern1(_) => 1,
        Interp::SincSinc
        | Interp::SincCos
        | Interp::SombCos
        | Interp::SincGauss
        | Interp::Gauss => 2,
    };
    if opts.params.len() < nparam {
        return fail(
            &method,
            ErrorCode::Dimin,
            format!(
                "The {:?} scheme needs {nparam} parameter{} but {} {} given.",
                opts.interp,
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

    // Simplify only when there are enough pixels to make it worthwhile.
    let simple: DynMapping = if npix > 1024 {
        map.simplify()?
    } else {
        map.clone_box()
    };
    if !simple.has_inverse() {
        return fail(
            &method,
            ErrorCode::Trnnd,
            "An inverse coordinate transformation is not defined by the Mapping supplied."
                .to_string(),
        );
    }

    let mut ctx = Resampler {
        map: simple.as_ref(),
        input,
        opts,
        scheme: Scheme::new(opts),
        method: &method,
        badval,
        lbnd_out: output.lbnd,
        ubnd_out: output.ubnd,
        out: &mut *output.data,
        out_var: output.var.as_deref_mut(),
    };
    ctx.adaptively(output.region_lbnd, output.region_ubnd)
}

fn fail<R>(method: &str, code: ErrorCode, msg: String) -> AstResult<R> {
    Err(AstError::new(code, format!("{method}: {msg}")))
}

fn prefix(method: &str, e: AstError) -> AstError {
    AstError::new(e.code(), format!("{method}: {}", e.message()))
}

/// The number of pixels in a grid, after checking its bounds as
/// `Resample<X>` does.
fn grid_size(lbnd: &[i64], ubnd: &[i64], what: &str) -> AstResult<usize> {
    let mut mpix: i64 = 1;
    for (idim, (&l, &u)) in lbnd.iter().zip(ubnd).enumerate() {
        if l > u {
            return Err(AstError::new(
                ErrorCode::Gbdin,
                format!(
                    "Lower bound of {what} ({l}) exceeds corresponding upper bound ({u}). Error \
                     in {} dimension {}.",
                    if what.starts_with("input") {
                        "input"
                    } else {
                        "output"
                    },
                    idim + 1
                ),
            ));
        }
        mpix = mpix.checked_mul(u - l + 1).ok_or_else(|| {
            AstError::new(
                ErrorCode::Exspix,
                format!("Supplied {what} array contains too many pixels."),
            )
        })?;
    }
    usize::try_from(mpix).map_err(|_| {
        AstError::new(
            ErrorCode::Exspix,
            format!("Supplied {what} array contains too many pixels."),
        )
    })
}

/// The interpolator `ResampleSection` calls, with the kernel, number of
/// neighbouring pixels and kernel parameters it sets up for a kernel scheme.
enum Scheme<'a, T> {
    Nearest,
    Linear,
    Kernel1 {
        kernel: interp::Kernel<'a>,
        neighb: i64,
        params: Vec<f64>,
    },
    BlockAve,
    UInterp(InterpFn<'a, T>),
}

impl<'a, T> Scheme<'a, T> {
    /// `ResampleSection`'s set-up of the scheme `opts.interp`.
    fn new(opts: &ResampleOptions<'a, T>) -> Self {
        let builtin = |kernel| {
            let (f, neighb, params) = kernels::setup(kernel, opts.params);
            Scheme::Kernel1 {
                kernel: interp::Kernel::Builtin(f),
                neighb,
                params,
            }
        };
        match opts.interp {
            Interp::Nearest => Scheme::Nearest,
            Interp::Linear => Scheme::Linear,
            Interp::Sinc => builtin(Builtin::Sinc),
            Interp::SincSinc => builtin(Builtin::SincSinc),
            Interp::SincCos => builtin(Builtin::SincCos),
            Interp::SincGauss => builtin(Builtin::SincGauss),
            Interp::Gauss => builtin(Builtin::Gauss),
            Interp::Somb => builtin(Builtin::Somb),
            Interp::SombCos => builtin(Builtin::SombCos),
            Interp::BlockAve => Scheme::BlockAve,
            // C's `MaxI( 1, (int) floor( params[ 0 ] + 0.5 ) )`.
            Interp::UKern1(f) => Scheme::Kernel1 {
                kernel: interp::Kernel::User(f),
                neighb: i64::from(((opts.params[0] + 0.5).floor() as i32).max(1)),
                params: opts.params.to_vec(),
            },
            Interp::UInterp(f) => Scheme::UInterp(f),
        }
    }
}

/// The arguments every level of the resampling shares.
struct Resampler<'a, 'b, T> {
    map: &'a dyn Mapping,
    input: &'a ResampleInput<'b, T>,
    opts: &'a ResampleOptions<'b, T>,
    scheme: Scheme<'b, T>,
    method: &'a str,
    badval: T,
    lbnd_out: &'a [i64],
    ubnd_out: &'a [i64],
    out: &'a mut [T],
    out_var: Option<&'a mut [T]>,
}

/// The most output pixels resampled at once (`mxpix` in
/// `ResampleWithBlocking`).
const MXPIX: i64 = 2 * 1024;

impl<T: PixelValue> Resampler<'_, '_, T> {
    /// `ResampleAdaptively`: resample the output section `[lbnd, ubnd]`,
    /// dividing it in two along its longest axis while the inverse Mapping
    /// is not linear enough over it.
    fn adaptively(&mut self, lbnd: &[i64], ubnd: &[i64]) -> AstResult<usize> {
        let ndim_out = lbnd.len();
        let mut npix: i64 = 1;
        let mut mxdim: i64 = 0;
        let mut dimx = 1;
        let mut nvertex: i64 = 1;
        for coord_out in 0..ndim_out {
            let dim = ubnd[coord_out] - lbnd[coord_out] + 1;
            npix *= dim;
            if dim > mxdim {
                mxdim = dim;
                dimx = coord_out;
            }
            nvertex *= 2;
        }

        // The number of sample points astLinearApprox needs. A section with
        // fewer than four times as many pixels is not worth fitting.
        let npoint = 1 + 4 * ndim_out as i64 + 2 * nvertex;
        let toosmall = npix < 4 * npoint;
        let toobig = (i64::try_from(self.opts.maxpix).unwrap_or(i64::MAX)) < mxdim;

        let mut linear_fit: Option<Vec<f64>> = None;
        let divide = if toosmall || self.opts.tol == 0.0 {
            false
        } else if toobig {
            true
        } else {
            // Fit the inverse over the pixel edges rather than the centres,
            // so an axis spanned by one pixel still has distinct bounds.
            let flbnd: Vec<f64> = lbnd.iter().map(|&l| l as f64 - 0.5).collect();
            let fubnd: Vec<f64> = ubnd.iter().map(|&u| u as f64 + 0.5).collect();
            let mut inverse = self.map.clone_box();
            inverse.invert();
            linear_fit = inverse
                .linear_approx(&flbnd, &fubnd, self.opts.tol)?
                .map(|fit| fit.as_c_layout())
                .filter(|fit| !fit.contains(&BAD_VALUE));
            linear_fit.is_none()
        };

        if !divide {
            self.with_blocking(linear_fit.as_deref(), lbnd, ubnd)
        } else {
            let mut lo = lbnd.to_vec();
            let mut hi = ubnd.to_vec();
            hi[dimx] = (0.5 * (lbnd[dimx] + ubnd[dimx]) as f64).floor() as i64;
            let mut result = self.adaptively(&lo, &hi)?;
            lo[dimx] = hi[dimx] + 1;
            hi[dimx] = ubnd[dimx];
            if lo[dimx] <= hi[dimx] {
                result += self.adaptively(&lo, &hi)?;
            }
            Ok(result)
        }
    }

    /// `ResampleWithBlocking`: resample the section `[lbnd, ubnd]` in blocks
    /// of at most [`MXPIX`] output pixels, shaped like the section.
    fn with_blocking(
        &mut self,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
    ) -> AstResult<usize> {
        let ndim_out = lbnd.len();
        let mut npix: i64 = 1;
        let mut mxdim_block: i64 = 0;
        for idim in 0..ndim_out {
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
                for idim in 0..ndim_out {
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

        let mut dim_block = vec![0_i64; ndim_out];
        let mut lbnd_block = vec![0_i64; ndim_out];
        let mut ubnd_block = vec![0_i64; ndim_out];
        for idim in 0..ndim_out {
            let dim = ubnd[idim] - lbnd[idim] + 1;
            dim_block[idim] = dim.min(mxdim_block);
            lbnd_block[idim] = lbnd[idim];
            ubnd_block[idim] = (lbnd[idim] + dim_block[idim] - 1).min(ubnd[idim]);
        }

        // The factor that conserves flux: the ratio of output to input pixel
        // sizes, from the fit's gradients.
        let factor = match linear_fit {
            Some(fit) if self.opts.flags.conserve_flux => {
                let ndim_in = self.input.lbnd.len();
                matrix_det(ndim_in, ndim_out, &fit[ndim_in..]).abs()
            }
            _ => 1.0,
        };

        let mut result = 0;
        loop {
            result += self.section(linear_fit, &lbnd_block, &ubnd_block, factor)?;
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
                if idim == ndim_out {
                    return Ok(result);
                }
            }
        }
    }

    /// `ResampleSection`: the input position of each output pixel of the
    /// block `[lbnd, ubnd]`, from the linear fit or the Mapping, and the
    /// interpolated values there, scaled by `factor` when conserving flux.
    fn section(
        &mut self,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
        factor: f64,
    ) -> AstResult<usize> {
        // Flux conservation needs bad values to tell which output pixels
        // were assigned.
        let conserve = self.opts.flags.conserve_flux;
        if self.opts.flags.no_bad && conserve {
            return Err(AstError::new(
                ErrorCode::Badflg,
                "astResample: Cannot use the AST__NOBAD and AST__CONSERVEFLUX flags together \
                 (programming error).",
            ));
        }
        let ndim_out = lbnd.len();
        let ndim_in = self.input.lbnd.len();
        let (lbnd_out, ubnd_out) = (self.lbnd_out, self.ubnd_out);

        let mut npoint = 1_usize;
        for coord_out in 0..ndim_out {
            npoint *= (ubnd[coord_out] - lbnd[coord_out] + 1) as usize;
        }
        let mut offset = vec![0_i64; npoint];
        let mut stride = vec![0_i64; ndim_out];
        let mut s: i64 = 1;
        for coord_out in 0..ndim_out {
            stride[coord_out] = s;
            s *= ubnd_out[coord_out] - lbnd_out[coord_out] + 1;
        }

        // The input position of each output pixel, axis by axis, as C's
        // `ptr_in[coord][point]`.
        let mut coords: Vec<Vec<f64>> = vec![vec![0.0; npoint]; ndim_in];
        if let Some(linear_fit) = linear_fit {
            let zero = &linear_fit[..ndim_in];
            let grad = &linear_fit[ndim_in..];
            let mut point = 0;
            if ndim_in == 1 && ndim_out == 1 {
                for ix in lbnd[0]..=ubnd[0] {
                    coords[0][point] = zero[0] + grad[0] * ix as f64;
                    offset[point] = ix - lbnd_out[0];
                    point += 1;
                }
            } else if ndim_in == 2 && ndim_out == 2 {
                for iy in lbnd[1]..=ubnd[1] {
                    let x1 = zero[0] + grad[1] * iy as f64;
                    let y1 = zero[1] + grad[3] * iy as f64;
                    let off1 = stride[1] * (iy - lbnd_out[1]) - lbnd_out[0];
                    for ix in lbnd[0]..=ubnd[0] {
                        coords[0][point] = x1 + grad[0] * ix as f64;
                        coords[1][point] = y1 + grad[2] * ix as f64;
                        offset[point] = off1 + ix;
                        point += 1;
                    }
                }
            } else {
                // Partial sums of the matrix product for the more
                // significant output axes, recomputed only for those whose
                // index has changed since the previous pixel.
                let mut accum = vec![0.0_f64; ndim_in * ndim_out];
                let mut dim = lbnd.to_vec();
                let mut off: i64 = 0;
                for coord_out in 0..ndim_out {
                    off += stride[coord_out] * (dim[coord_out] - lbnd_out[coord_out]);
                }
                for coord_in in 0..ndim_in {
                    accum[(coord_in + 1) * ndim_out - 1] = zero[coord_in];
                }
                let mut coord_out = ndim_out - 1;
                loop {
                    for coord_in in 0..ndim_in {
                        let i1 = coord_in * ndim_out;
                        for idim in (1..=coord_out).rev() {
                            let i2 = i1 + idim;
                            accum[i2 - 1] = accum[i2] + dim[idim] as f64 * grad[i2];
                        }
                        coords[coord_in][point] = accum[i1] + dim[0] as f64 * grad[i1];
                    }
                    offset[point] = off;
                    point += 1;
                    if !next_index(&mut dim, &mut off, &mut coord_out, lbnd, ubnd, &stride) {
                        break;
                    }
                }
            }
        } else {
            // Flux can be conserved only where a linear fit gives the ratio
            // of pixel sizes.
            if conserve {
                return Err(AstError::new(
                    ErrorCode::Cnflx,
                    "astResampleSection: Flux conservation was requested but cannot be \
                     performed because either the Mapping is too non-linear, or the requested \
                     tolerance is too small.",
                ));
            }
            let mut pset_out = PointSet::zeros(npoint, ndim_out);
            {
                let mut v = pset_out.view_mut();
                let mut point = 0;
                if ndim_out == 1 && ndim_in == 1 {
                    for ix in lbnd[0]..=ubnd[0] {
                        v[[0, point]] = ix as f64;
                        offset[point] = ix - lbnd_out[0];
                        point += 1;
                    }
                } else if ndim_out == 2 && ndim_in == 2 {
                    for iy in lbnd[1]..=ubnd[1] {
                        let off1 = stride[1] * (iy - lbnd_out[1]) - lbnd_out[0];
                        for ix in lbnd[0]..=ubnd[0] {
                            v[[0, point]] = ix as f64;
                            v[[1, point]] = iy as f64;
                            offset[point] = off1 + ix;
                            point += 1;
                        }
                    }
                } else {
                    let mut dim = lbnd.to_vec();
                    let mut off: i64 = 0;
                    for coord_out in 0..ndim_out {
                        off += stride[coord_out] * (dim[coord_out] - lbnd_out[coord_out]);
                    }
                    let mut coord_out = 0;
                    loop {
                        for c in 0..ndim_out {
                            v[[c, point]] = dim[c] as f64;
                        }
                        offset[point] = off;
                        point += 1;
                        if !next_index(&mut dim, &mut off, &mut coord_out, lbnd, ubnd, &stride) {
                            break;
                        }
                    }
                }
            }
            let mut pset_in = self.map.transform(&pset_out, false)?;
            substitute_nan_to_bad_in_place(&mut pset_in);
            let v = pset_in.view();
            for (coord_in, column) in coords.iter_mut().enumerate() {
                for (point, c) in column.iter_mut().enumerate() {
                    *c = v[[coord_in, point]];
                }
            }
        }

        let input = self.input;
        let usevar = input.var.is_some() && self.out_var.is_some();
        let args = InterpArgs {
            lbnd_in: input.lbnd,
            ubnd_in: input.ubnd,
            input: input.data,
            in_var: if usevar { input.var } else { None },
            offset: &offset,
            coords: &coords,
            params: self.opts.params,
            flags: self.opts.flags,
            badval: self.badval,
        };
        let out = &mut *self.out;
        let mut out_var = if usevar {
            self.out_var.as_deref_mut()
        } else {
            None
        };
        let result = match &self.scheme {
            Scheme::Nearest => interp::nearest(&args, out, out_var.as_deref_mut()),
            Scheme::Linear => interp::linear(&args, out, out_var.as_deref_mut()),
            Scheme::Kernel1 {
                kernel,
                neighb,
                params,
            } => interp::kernel1(&args, *kernel, *neighb, params, out, out_var.as_deref_mut())
                .map_err(|e| {
                    e.with_context(format!(
                        "{}: Error signalled by user-supplied 1-d interpolation kernel.",
                        self.method
                    ))
                })?,
            Scheme::BlockAve => interp::block_average(&args, out, out_var.as_deref_mut()),
            Scheme::UInterp(f) => f(&args, out, out_var.as_deref_mut()).map_err(|e| {
                e.with_context(format!(
                    "{}: Error signalled by user-supplied sub-pixel interpolation function.",
                    self.method
                ))
            })?,
        };

        // mapping.c `ConserveFlux<X>`: C multiplies each value in its own
        // type by the double factor, so an integer result is truncated.
        if conserve {
            for &off in &offset {
                let v = &mut out[off as usize];
                if *v != self.badval {
                    *v = T::from_f64(v.to_f64() * factor);
                }
            }
            if let Some(out_var) = out_var {
                let factor = factor * factor;
                for &off in &offset {
                    let v = &mut out_var[off as usize];
                    if *v != self.badval {
                        *v = T::from_f64(v.to_f64() * factor);
                    }
                }
            }
        }
        Ok(result)
    }
}

/// mapping.c `MatrixDet`: the determinant of the `nrow` by `ncol` matrix
/// `matrix` (row-major) once the rows and columns holding only zeros or bad
/// values are removed, or `BAD_VALUE` if what remains is not square.
pub(crate) fn matrix_det(nrow: usize, ncol: usize, matrix: &[f64]) -> f64 {
    let used = |v: f64| v != BAD_VALUE && v != 0.0;
    let userow: Vec<bool> = (0..nrow)
        .map(|r| (0..ncol).any(|c| used(matrix[r * ncol + c])))
        .collect();
    let usecol: Vec<bool> = (0..ncol)
        .map(|c| (0..nrow).any(|r| used(matrix[r * ncol + c])))
        .collect();
    let ndim = userow.iter().filter(|&&u| u).count();
    if ndim != usecol.iter().filter(|&&u| u).count() {
        return BAD_VALUE;
    }
    let sqmat: Vec<f64> = (0..nrow)
        .filter(|&r| userow[r])
        .flat_map(|r| {
            let usecol = &usecol;
            (0..ncol)
                .filter(move |&c| usecol[c])
                .map(move |c| matrix[r * ncol + c])
        })
        .collect();
    match ndim {
        // C allocates nothing for an empty matrix, so leaves the result bad.
        0 => BAD_VALUE,
        1 => sqmat[0],
        2 => sqmat[0] * sqmat[3] - sqmat[1] * sqmat[2],
        _ => {
            let mut a = sqmat;
            let mut y = vec![1.0; ndim];
            crate::pal::dmat_det(ndim, &mut a, &mut y).1
        }
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
