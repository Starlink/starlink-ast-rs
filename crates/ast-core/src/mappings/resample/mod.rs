//! Resampling a gridded array through a Mapping: C's `astResample<X>`.
//!
//! A port of mapping.c `Resample<X>` and its drivers. Each output pixel's
//! centre is taken back through the Mapping's inverse to a position in the
//! input grid, and the input is interpolated there. The output region is
//! divided recursively along its longest axis until a section is too small
//! to be worth a linear fit or `astLinearApprox` finds the inverse linear
//! within `tol` over it (`ResampleAdaptively`); each section is then handled
//! in blocks of at most 2048 output pixels (`ResampleWithBlocking`), whose
//! input positions come from the linear fit or the Mapping
//! (`ResampleSection`) and are passed to the interpolator.
//!
//! C writes each driver once over a `void *` array and a type code, and each
//! interpolator once per type from a macro. The port is generic over
//! [`PixelValue`], whose items are the macro arguments.

// The loops mirror the C source index for index.
#![allow(clippy::needless_range_loop)]

mod interp;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, Mapping, substitute_nan_to_bad_in_place};
use crate::pixel::PixelValue;

/// The sub-pixel interpolation scheme of [`resample`] (C's `interp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interp {
    /// The value of the input pixel whose centre is nearest
    /// (`AST__NEAREST`).
    Nearest,
    /// Linear interpolation between the nearest input pixels along each axis
    /// (`AST__LINEAR`, C's default).
    Linear,
}

/// The flags of [`resample`] (C's `flags`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResampleFlags {
    /// Input pixels equal to the bad value are bad (`AST__USEBAD`).
    pub use_bad: bool,
    /// Output pixels that would be bad are left as they are
    /// (`AST__NOBAD`).
    pub no_bad: bool,
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
pub struct ResampleOptions<'a> {
    /// The interpolation scheme.
    pub interp: Interp,
    /// The scheme's parameters (C's `params`); the schemes here use none.
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
/// `map`, as C's `astResample<X>` does, returning the number of output
/// pixels (and variances) given the bad value.
///
/// `map` takes input grid positions to output grid positions; pixel
/// centres are at integer coordinates. An output pixel whose input position
/// is bad or outside the input grid, or whose interpolated value cannot be
/// formed, is set to `badval` (or left as it is with
/// [`ResampleFlags::no_bad`]).
///
/// Errors: grids whose axis counts do not match the Mapping's inputs and
/// outputs, or that have none, are AST__NGDIN; a lower bound above its upper
/// bound, or a region outside the output grid, is AST__GBDIN; too many
/// pixels is AST__EXSPIX; a negative `tol` is AST__PATIN; a Mapping with no
/// inverse is AST__TRNND; and a data or variance array of the wrong length,
/// or bounds of different lengths, is AST__DIMIN.
pub fn resample<T: PixelValue>(
    map: &dyn Mapping,
    input: &ResampleInput<'_, T>,
    output: &mut ResampleOutput<'_, T>,
    opts: &ResampleOptions<'_>,
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

    // Arrays shorter than their grids, which C cannot check.
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

/// The arguments every level of the resampling shares.
struct Resampler<'a, 'b, T> {
    map: &'a dyn Mapping,
    input: &'a ResampleInput<'b, T>,
    opts: &'a ResampleOptions<'b>,
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
                .filter(|fit| !fit.contains(&crate::constants::BAD_VALUE));
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

        let mut result = 0;
        loop {
            result += self.section(linear_fit, &lbnd_block, &ubnd_block)?;
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
    /// interpolated values there.
    fn section(
        &mut self,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
    ) -> AstResult<usize> {
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
            let mut pset_out = PointSet::zeros(npoint, ndim_out);
            {
                let mut v = pset_out.view_mut();
                let mut point = 0;
                if ndim_out == 1 && ndim_in == 1 {
                    for ix in lbnd[0]..=ubnd[0] {
                        v[[point, 0]] = ix as f64;
                        offset[point] = ix - lbnd_out[0];
                        point += 1;
                    }
                } else if ndim_out == 2 && ndim_in == 2 {
                    for iy in lbnd[1]..=ubnd[1] {
                        let off1 = stride[1] * (iy - lbnd_out[1]) - lbnd_out[0];
                        for ix in lbnd[0]..=ubnd[0] {
                            v[[point, 0]] = ix as f64;
                            v[[point, 1]] = iy as f64;
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
                            v[[point, c]] = dim[c] as f64;
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
                    *c = v[[point, coord_in]];
                }
            }
        }

        let input = self.input;
        let usevar = input.var.is_some() && self.out_var.is_some();
        let args = interp::Args {
            lbnd_in: input.lbnd,
            ubnd_in: input.ubnd,
            input: input.data,
            in_var: if usevar { input.var } else { None },
            offset: &offset,
            coords: &coords,
            flags: self.opts.flags,
            badval: self.badval,
        };
        let out_var = if usevar {
            self.out_var.as_deref_mut()
        } else {
            None
        };
        Ok(match self.opts.interp {
            Interp::Nearest => interp::nearest(&args, self.out, out_var),
            Interp::Linear => interp::linear(&args, self.out, out_var),
        })
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
