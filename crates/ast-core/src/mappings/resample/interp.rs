//! The interpolators of [`super::resample`]: mapping.c
//! `InterpolateNearest<X>`, `InterpolateLinear<X>`, `InterpolateKernel1<X>`
//! and `InterpolateBlockAverage<X>`.
//!
//! C writes each as eight loops, one per combination of the `Usebad`,
//! `Usevar` and `Nobad` cases, so the compiler can drop the tests a case does
//! not need. The port tests the three flags in one loop; the arithmetic, and
//! its order, are C's.

use crate::constants::BAD_VALUE;
use crate::error::AstResult;
use crate::pixel::{PixelValue, SumValue};

use super::kernels::KernelFn;
use super::{InterpArgs as Args, Kernel1Fn, ResampleFlags};

/// The input grid's strides and coordinate limits, pixel edges included.
fn grid_limits(lbnd_in: &[i64], ubnd_in: &[i64]) -> (Vec<i64>, Vec<f64>, Vec<f64>) {
    let ndim = lbnd_in.len();
    let mut stride = vec![0_i64; ndim];
    let mut xmin = vec![0.0; ndim];
    let mut xmax = vec![0.0; ndim];
    let mut s = 1;
    for idim in 0..ndim {
        stride[idim] = s;
        s *= ubnd_in[idim] - lbnd_in[idim] + 1;
        xmin[idim] = lbnd_in[idim] as f64 - 0.5;
        xmax[idim] = ubnd_in[idim] as f64 + 0.5;
    }
    (stride, xmin, xmax)
}

/// Whether a coordinate lies outside `[xmin, xmax)` or is bad.
fn outside(x: f64, xmin: f64, xmax: f64) -> bool {
    x < xmin || x >= xmax || x == BAD_VALUE
}

/// mapping.c `InterpolateNearest<X>`: each output pixel takes the value of
/// the input pixel containing its position. C's 1-D, 2-D and n-D paths do
/// the same arithmetic, so the n-D one serves for all.
pub(super) fn nearest<T: PixelValue>(
    a: &Args<'_, T>,
    out: &mut [T],
    out_var: Option<&mut [T]>,
) -> usize {
    let ndim = a.lbnd_in.len();
    let (stride, xmin, xmax) = grid_limits(a.lbnd_in, a.ubnd_in);
    let usebad = a.flags.use_bad;
    let nobad = a.flags.no_bad;
    let mut out_var = out_var;
    let mut result = 0;
    for point in 0..a.offset.len() {
        let mut bad = false;
        let mut off_in: i64 = 0;
        for idim in 0..ndim {
            let xn = a.coords[idim][point];
            bad = outside(xn, xmin[idim], xmax[idim]);
            if bad {
                break;
            }
            off_in += stride[idim] * ((xn + 0.5).floor() as i64 - a.lbnd_in[idim]);
        }
        if !bad && usebad {
            bad = a.input[off_in as usize] == a.badval;
        }

        let off_out = a.offset[point] as usize;
        if bad {
            if !nobad {
                out[off_out] = a.badval;
                if let Some(v) = out_var.as_deref_mut() {
                    v[off_out] = a.badval;
                }
            }
            result += 1;
        } else {
            out[off_out] = a.input[off_in as usize];
            if let (Some(in_var), Some(ov)) = (a.in_var, out_var.as_deref_mut()) {
                let var = in_var[off_in as usize];
                let mut bad = usebad && var == a.badval;
                if T::SIGNED {
                    bad = bad || var < T::ZERO;
                }
                if (T::SIGNED || usebad) && bad {
                    if !nobad {
                        ov[off_out] = a.badval;
                    }
                    result += 1;
                } else {
                    ov[off_out] = var;
                }
            }
        }
    }
    result
}

/// The sums from which an interpolated value is formed.
struct Sums<S> {
    sum: S,
    sum_var: S,
    wtsum: S,
    bad_var: bool,
}

impl<S: SumValue> Sums<S> {
    fn new() -> Self {
        Sums {
            sum: S::ZERO,
            sum_var: S::ZERO,
            wtsum: S::ZERO,
            bad_var: false,
        }
    }

    /// mapping.c `FORM_LINEAR_INTERPOLATION_SUM`, and the same sums in
    /// `InterpolateKernel1<X>`: add the input pixel at `off` with weight
    /// `wt`.
    fn add<T: PixelValue<Sum = S>>(&mut self, a: &Args<'_, T>, off: i64, wt: f64) {
        let usebad = a.flags.use_bad;
        let v = a.input[off as usize];
        if !usebad || v != a.badval {
            let pixwt = wt;
            self.sum += v.to_sum() * S::from_f64(pixwt);
            self.wtsum += S::from_f64(pixwt);
            if let Some(in_var) = a.in_var
                && (!(T::SIGNED || usebad) || !self.bad_var)
            {
                let var = in_var[off as usize];
                if usebad {
                    self.bad_var = var == a.badval;
                }
                if T::SIGNED {
                    self.bad_var = self.bad_var || var < T::ZERO;
                }
                if !(T::SIGNED || usebad) || !self.bad_var {
                    self.sum_var += S::from_f64(pixwt * pixwt) * var.to_sum();
                }
            }
        }
    }
}

/// mapping.c `InterpolateLinear<X>`.
pub(super) fn linear<T: PixelValue>(
    a: &Args<'_, T>,
    out: &mut [T],
    out_var: Option<&mut [T]>,
) -> usize {
    let ndim = a.lbnd_in.len();
    let (stride, xmin, xmax) = grid_limits(a.lbnd_in, a.ubnd_in);
    let usebad = a.flags.use_bad;
    let nobad = a.flags.no_bad;
    let (lbnd_in, ubnd_in) = (a.lbnd_in, a.ubnd_in);
    let mut out_var = out_var;
    let mut result = 0;

    // Workspace for the n-D path.
    let mut dim = vec![0_i64; ndim];
    let mut lo = vec![0_i64; ndim];
    let mut hi = vec![0_i64; ndim];
    let mut frac_lo = vec![0.0; ndim];
    let mut frac_hi = vec![0.0; ndim];
    let mut wt = vec![0.0; ndim];
    let mut wtprod = vec![0.0; ndim];

    for point in 0..a.offset.len() {
        let mut sums = Sums::<T::Sum>::new();
        let mut bad;
        if ndim == 1 {
            let x = a.coords[0][point];
            bad = outside(x, xmin[0], xmax[0]);
            if !bad {
                if usebad {
                    let pixel = (x + 0.5).floor() as i64 - lbnd_in[0];
                    bad = a.input[pixel as usize] == a.badval;
                }
                if !bad {
                    let lo_x = x.floor() as i64;
                    let hi_x = lo_x + 1;
                    let frac_lo_x = hi_x as f64 - x;
                    let frac_hi_x = 1.0 - frac_lo_x;
                    let off_lo = lo_x - lbnd_in[0];
                    if lo_x >= lbnd_in[0] {
                        sums.add(a, off_lo, frac_lo_x);
                    }
                    if hi_x <= ubnd_in[0] {
                        sums.add(a, off_lo + 1, frac_hi_x);
                    }
                }
            }
        } else if ndim == 2 {
            let ystride = stride[1];
            let x = a.coords[0][point];
            bad = outside(x, xmin[0], xmax[0]);
            if !bad {
                let y = a.coords[1][point];
                bad = outside(y, xmin[1], xmax[1]);
                if !bad {
                    if usebad {
                        let ix = (x + 0.5).floor() as i64;
                        let iy = (y + 0.5).floor() as i64;
                        let pixel = ix - lbnd_in[0] + ystride * (iy - lbnd_in[1]);
                        bad = a.input[pixel as usize] == a.badval;
                    }
                    if !bad {
                        let lo_x = x.floor() as i64;
                        let hi_x = lo_x + 1;
                        let frac_lo_x = hi_x as f64 - x;
                        let frac_hi_x = 1.0 - frac_lo_x;
                        let lo_y = y.floor() as i64;
                        let hi_y = lo_y + 1;
                        let frac_lo_y = hi_y as f64 - y;
                        let frac_hi_y = 1.0 - frac_lo_y;
                        let off_lo = lo_x - lbnd_in[0] + ystride * (lo_y - lbnd_in[1]);
                        if lo_y >= lbnd_in[1] {
                            if lo_x >= lbnd_in[0] {
                                sums.add(a, off_lo, frac_lo_x * frac_lo_y);
                            }
                            if hi_x <= ubnd_in[0] {
                                sums.add(a, off_lo + 1, frac_hi_x * frac_lo_y);
                            }
                        }
                        if hi_y <= ubnd_in[1] {
                            if lo_x >= lbnd_in[0] {
                                sums.add(a, off_lo + ystride, frac_lo_x * frac_hi_y);
                            }
                            if hi_x <= ubnd_in[0] {
                                sums.add(a, off_lo + ystride + 1, frac_hi_x * frac_hi_y);
                            }
                        }
                    }
                }
            }
        } else {
            let mut off_in: i64 = 0;
            let mut pixel: i64 = 0;
            bad = false;
            for idim in 0..ndim {
                let xn = a.coords[idim][point];
                bad = outside(xn, xmin[idim], xmax[idim]);
                if bad {
                    break;
                }
                if usebad {
                    pixel += stride[idim] * ((xn + 0.5).floor() as i64 - lbnd_in[idim]);
                }
                let ixn = xn.floor() as i64;
                lo[idim] = ixn.max(lbnd_in[idim]);
                hi[idim] = (ixn + 1).min(ubnd_in[idim]);
                frac_lo[idim] = 1.0 - (xn - lo[idim] as f64).abs();
                frac_hi[idim] = 1.0 - (xn - hi[idim] as f64).abs();
                dim[idim] = lo[idim];
                off_in += stride[idim] * (lo[idim] - lbnd_in[idim]);
                wt[idim] = frac_lo[idim];
            }
            if usebad {
                bad = bad || a.input[pixel as usize] == a.badval;
            }
            if !bad {
                let mut idim = ndim - 1;
                wtprod[idim] = 1.0;
                loop {
                    for ii in (1..=idim).rev() {
                        wtprod[ii - 1] = wtprod[ii] * wt[ii];
                    }
                    sums.add(a, off_in, wtprod[0] * wt[0]);

                    // Step to the next pixel: the first axis still at its
                    // lower index moves to its higher one, and the axes
                    // before it return to their lower ones.
                    idim = 0;
                    let mut done = false;
                    loop {
                        if dim[idim] != hi[idim] {
                            dim[idim] = hi[idim];
                            off_in += stride[idim];
                            wt[idim] = frac_hi[idim];
                            break;
                        }
                        if dim[idim] != lo[idim] {
                            dim[idim] = lo[idim];
                            off_in -= stride[idim];
                            wt[idim] = frac_lo[idim];
                        }
                        idim += 1;
                        if idim == ndim {
                            done = true;
                            break;
                        }
                    }
                    if done {
                        break;
                    }
                }
            }
        }

        // mapping.c `CALC_AND_ASSIGN_OUTPUT`.
        let off_out = a.offset[point] as usize;
        if bad {
            if !nobad {
                out[off_out] = a.badval;
                if let Some(v) = out_var.as_deref_mut() {
                    v[off_out] = a.badval;
                }
            }
            result += 1;
        } else {
            out[off_out] = T::from_sum(sums.sum / sums.wtsum);
            if let Some(ov) = out_var.as_deref_mut() {
                if (T::SIGNED || usebad) && sums.bad_var {
                    if !nobad {
                        ov[off_out] = a.badval;
                    }
                    result += 1;
                } else {
                    ov[off_out] = T::from_sum(sums.sum_var / (sums.wtsum * sums.wtsum));
                }
            }
        }
    }
    result
}

/// A 1-d kernel: one of mapping.c's, or the caller's (`AST__UKERN1`).
#[derive(Clone, Copy)]
pub(super) enum Kernel<'a> {
    /// A kernel of mapping.c, given the parameters `ResampleSection`
    /// derives for it.
    Builtin(KernelFn),
    /// The caller's kernel, given the caller's parameters and flags.
    User(Kernel1Fn<'a>),
}

impl Kernel<'_> {
    fn eval(&self, offset: f64, params: &[f64], flags: ResampleFlags) -> AstResult<f64> {
        match self {
            Kernel::Builtin(f) => Ok(f(offset, params)),
            Kernel::User(f) => f(offset, params, flags),
        }
    }
}

/// mapping.c `CALC_AND_ASSIGN_OUTPUT` as `InterpolateKernel1<X>` and
/// `InterpolateBlockAverage<X>` define it: the value and variance from the
/// sums, bad where the weights sum to zero or the value would overflow an
/// integer type, assigned to the output pixel at `off_out`. Returns the
/// number of bad values assigned.
fn calc_and_assign<T: PixelValue>(
    a: &Args<'_, T>,
    mut bad: bool,
    s: &Sums<T::Sum>,
    off_out: usize,
    out: &mut [T],
    out_var: Option<&mut [T]>,
) -> usize {
    let zero = <T::Sum as SumValue>::ZERO;
    let usevar = out_var.is_some();
    let mut bad_var = s.bad_var;
    let mut val = zero;
    let mut val_var = zero;
    if !bad {
        bad = s.wtsum == zero;
        if !bad {
            val = s.sum / s.wtsum;
            if !T::FLOATING {
                bad = val <= T::LO_LIM || val >= T::HI_LIM;
            }
        }
        if usevar && bad {
            bad_var = true;
        }
    }
    if usevar && !bad_var {
        let wtsum_sq = s.wtsum * s.wtsum;
        bad_var = wtsum_sq == zero;
        if !bad_var {
            val_var = s.sum_var / wtsum_sq;
            if !T::FLOATING {
                bad_var = val_var <= T::LO_LIM || val_var >= T::HI_LIM;
            }
        }
    }

    let mut nbad = 0;
    if bad {
        if !a.flags.no_bad {
            out[off_out] = a.badval;
            if let Some(v) = out_var {
                v[off_out] = a.badval;
            }
        }
        nbad += 1;
    } else {
        out[off_out] = T::from_sum(val);
        if let Some(v) = out_var {
            if bad_var {
                if !a.flags.no_bad {
                    v[off_out] = a.badval;
                }
                nbad += 1;
            } else {
                v[off_out] = T::from_sum(val_var);
            }
        }
    }
    nbad
}

/// mapping.c `InterpolateKernel1<X>`: each output value is the sum of the
/// input pixels within `neighb` pixels of its position along each axis,
/// weighted by the product of `kernel` evaluated at their offsets along
/// each axis, divided by the sum of those weights. `params` are passed to
/// the kernel. An error from a caller's kernel stops the resampling.
pub(super) fn kernel1<T: PixelValue>(
    a: &Args<'_, T>,
    kernel: Kernel<'_>,
    neighb: i64,
    params: &[f64],
    out: &mut [T],
    out_var: Option<&mut [T]>,
) -> AstResult<usize> {
    let ndim = a.lbnd_in.len();
    let (stride, xmin, xmax) = grid_limits(a.lbnd_in, a.ubnd_in);
    let usebad = a.flags.use_bad;
    let (lbnd_in, ubnd_in) = (a.lbnd_in, a.ubnd_in);
    let mut out_var = out_var;
    let mut result = 0;
    let k = |offset: f64| kernel.eval(offset, params, a.flags);

    // Kernel values along each axis. C allocates 2 * neighb per axis; no
    // more than the axis's extent can be used.
    let mut kval: Vec<Vec<f64>> = (0..ndim)
        .map(|idim| {
            let extent = ubnd_in[idim] - lbnd_in[idim] + 1;
            vec![0.0; (2 * neighb).min(extent) as usize]
        })
        .collect();
    // Workspace for the n-D path.
    let mut lo = vec![0_i64; ndim];
    let mut hi = vec![0_i64; ndim];
    let mut wt = vec![0_usize; ndim];
    let mut wtprod = vec![0.0; ndim];

    for point in 0..a.offset.len() {
        let mut s = Sums::<T::Sum>::new();
        let mut bad;
        if ndim == 1 {
            let x = a.coords[0][point];
            bad = outside(x, xmin[0], xmax[0]);
            if !bad {
                if usebad {
                    let pixel = (x + 0.5).floor() as i64 - lbnd_in[0];
                    bad = a.input[pixel as usize] == a.badval;
                }
                if !bad {
                    let ix = x.floor() as i64;
                    let lo_x = (ix - neighb + 1).max(lbnd_in[0]);
                    let hi_x = (ix + neighb).min(ubnd_in[0]);
                    let mut off_in = lo_x - lbnd_in[0];
                    for ix in lo_x..=hi_x {
                        if !usebad || a.input[off_in as usize] != a.badval {
                            let pixwt = k(ix as f64 - x)?;
                            s.add(a, off_in, pixwt);
                        }
                        off_in += 1;
                    }
                }
            }
        } else if ndim == 2 {
            let ystride = stride[1];
            let x = a.coords[0][point];
            bad = outside(x, xmin[0], xmax[0]);
            if !bad {
                let y = a.coords[1][point];
                bad = outside(y, xmin[1], xmax[1]);
                if !bad {
                    if usebad {
                        let ix = (x + 0.5).floor() as i64;
                        let iy = (y + 0.5).floor() as i64;
                        let pixel = ix - lbnd_in[0] + ystride * (iy - lbnd_in[1]);
                        bad = a.input[pixel as usize] == a.badval;
                    }
                    if !bad {
                        let ix = x.floor() as i64;
                        let lo_x = (ix - neighb + 1).max(lbnd_in[0]);
                        let hi_x = (ix + neighb).min(ubnd_in[0]);
                        let iy = y.floor() as i64;
                        let lo_y = (iy - neighb + 1).max(lbnd_in[1]);
                        let hi_y = (iy + neighb).min(ubnd_in[1]);
                        for ix in lo_x..=hi_x {
                            kval[0][(ix - lo_x) as usize] = k(ix as f64 - x)?;
                        }
                        let mut off1 = lo_x - lbnd_in[0] + ystride * (lo_y - lbnd_in[1]);
                        for iy in lo_y..=hi_y {
                            let wt_y = k(iy as f64 - y)?;
                            let mut off_in = off1;
                            for ix in lo_x..=hi_x {
                                if !usebad || a.input[off_in as usize] != a.badval {
                                    let pixwt = kval[0][(ix - lo_x) as usize] * wt_y;
                                    s.add(a, off_in, pixwt);
                                }
                                off_in += 1;
                            }
                            off1 += ystride;
                        }
                    }
                }
            }
        } else {
            let mut pixel: i64 = 0;
            let mut off_in: i64 = 0;
            bad = false;
            for idim in 0..ndim {
                let xn = a.coords[idim][point];
                bad = outside(xn, xmin[idim], xmax[idim]);
                if bad {
                    break;
                }
                if usebad {
                    pixel += stride[idim] * ((xn + 0.5).floor() as i64 - lbnd_in[idim]);
                }
                let ixn = xn.floor() as i64;
                lo[idim] = (ixn - neighb + 1).max(lbnd_in[idim]);
                hi[idim] = (ixn + neighb).min(ubnd_in[idim]);
                off_in += stride[idim] * (lo[idim] - lbnd_in[idim]);
            }
            if usebad {
                bad = bad || a.input[pixel as usize] == a.badval;
            }
            if !bad {
                for idim in 0..ndim {
                    wt[idim] = 0;
                    let xn = a.coords[idim][point];
                    for ixn in lo[idim]..=hi[idim] {
                        kval[idim][(ixn - lo[idim]) as usize] = k(ixn as f64 - xn)?;
                    }
                }

                // Each pixel's weight is the product of the kernel values
                // along each axis. The products over the more significant
                // axes are kept in `wtprod` and recomputed only from the
                // most significant axis whose index has changed.
                let mut idim = ndim - 1;
                wtprod[idim] = 1.0;
                loop {
                    for ii in (1..=idim).rev() {
                        wtprod[ii - 1] = wtprod[ii] * kval[ii][wt[ii]];
                    }
                    if !usebad || a.input[off_in as usize] != a.badval {
                        let pixwt = wtprod[0] * kval[0][wt[0]];
                        s.add(a, off_in, pixwt);
                    }

                    idim = 0;
                    let mut done = false;
                    loop {
                        let last = (hi[idim] - lo[idim]) as usize;
                        if wt[idim] != last {
                            wt[idim] += 1;
                            off_in += stride[idim];
                            break;
                        }
                        wt[idim] = 0;
                        off_in -= stride[idim] * (hi[idim] - lo[idim]);
                        idim += 1;
                        if idim == ndim {
                            done = true;
                            break;
                        }
                    }
                    if done {
                        break;
                    }
                }
            }
        }
        result += calc_and_assign(
            a,
            bad,
            &s,
            a.offset[point] as usize,
            out,
            out_var.as_deref_mut(),
        );
    }
    Ok(result)
}

/// mapping.c `InterpolateBlockAverage<X>`: each output value is the mean
/// of the good input pixels in the block reaching `neighb` pixels either
/// side of its position along each axis, weighted by the reciprocals of
/// their variances when variances are given.
///
/// As in C, only the n-D path tests for an empty block, and a position
/// outside the input grid is bad only if its block misses the grid. For a
/// signed type without `use_bad`, a negative variance excludes the rest of
/// its block, and the block's output variance is bad if its last good
/// pixel's variance was; both follow from C's handling of `bad_var`.
pub(super) fn block_average<T: PixelValue>(
    a: &Args<'_, T>,
    out: &mut [T],
    out_var: Option<&mut [T]>,
) -> usize {
    let ndim = a.lbnd_in.len();
    let (stride, _, _) = grid_limits(a.lbnd_in, a.ubnd_in);
    let usebad = a.flags.use_bad;
    let (lbnd_in, ubnd_in) = (a.lbnd_in, a.ubnd_in);
    let neighb = (a.params[0] + 0.5).floor() as i32 as i64;
    let mut out_var = out_var;
    let usevar = a.in_var.is_some() && out_var.is_some();
    let one = <T::Sum as SumValue>::ONE;
    let mut result = 0;

    // C's `bad_var` is set at the start, then only where a block's sums
    // are initialized.
    let mut bad_var = false;
    let add = |s: &mut Sums<T::Sum>, bad_var: &mut bool, off_in: i64| {
        let v = a.input[off_in as usize];
        if !usebad || v != a.badval {
            if let (true, Some(in_var)) = (usevar, a.in_var) {
                let var = in_var[off_in as usize];
                if usebad {
                    *bad_var = var == a.badval;
                }
                if T::SIGNED {
                    *bad_var = *bad_var || var < T::ZERO;
                }
                if !(T::SIGNED || usebad) || !*bad_var {
                    let pixwt = one / var.to_sum();
                    s.sum += pixwt * v.to_sum();
                    s.wtsum += pixwt;
                    s.sum_var += pixwt;
                }
            } else {
                s.sum += v.to_sum();
                s.wtsum += one;
            }
        }
    };

    let mut lo = vec![0_i64; ndim];
    let mut hi = vec![0_i64; ndim];
    let mut ixm = vec![0_i64; ndim];

    for point in 0..a.offset.len() {
        let mut s = Sums::<T::Sum>::new();
        let mut bad;
        if ndim == 1 {
            let x = a.coords[0][point];
            bad = x == BAD_VALUE;
            if !bad {
                let ix = x.floor() as i64;
                let lo_x = (ix - neighb + 1).max(lbnd_in[0]);
                let hi_x = (ix + neighb).min(ubnd_in[0]);
                bad_var = false;
                let mut off_in = lo_x - lbnd_in[0];
                for _ in lo_x..=hi_x {
                    add(&mut s, &mut bad_var, off_in);
                    off_in += 1;
                }
            }
        } else if ndim == 2 {
            let ystride = stride[1];
            let x = a.coords[0][point];
            bad = x == BAD_VALUE;
            if !bad {
                let y = a.coords[1][point];
                bad = y == BAD_VALUE;
                if !bad {
                    let ix = x.floor() as i64;
                    let lo_x = (ix - neighb + 1).max(lbnd_in[0]);
                    let hi_x = (ix + neighb).min(ubnd_in[0]);
                    let iy = y.floor() as i64;
                    let lo_y = (iy - neighb + 1).max(lbnd_in[1]);
                    let hi_y = (iy + neighb).min(ubnd_in[1]);
                    bad_var = false;
                    let mut off1 = lo_x - lbnd_in[0] + ystride * (lo_y - lbnd_in[1]);
                    for _ in lo_y..=hi_y {
                        let mut off_in = off1;
                        for _ in lo_x..=hi_x {
                            add(&mut s, &mut bad_var, off_in);
                            off_in += 1;
                        }
                        off1 += ystride;
                    }
                }
            }
        } else {
            let mut off_in: i64 = 0;
            bad = false;
            for idim in 0..ndim {
                let xn = a.coords[idim][point];
                bad = xn == BAD_VALUE;
                if bad {
                    break;
                }
                let ixn = xn.floor() as i64;
                lo[idim] = (ixn - neighb + 1).max(lbnd_in[idim]);
                hi[idim] = (ixn + neighb).min(ubnd_in[idim]);
                bad = lo[idim] > hi[idim];
                if bad {
                    break;
                }
                off_in += stride[idim] * (lo[idim] - lbnd_in[idim]);
                ixm[idim] = lo[idim];
            }
            if !bad {
                bad_var = false;
                loop {
                    add(&mut s, &mut bad_var, off_in);
                    let mut idim = 0;
                    while idim < ndim {
                        if ixm[idim] < hi[idim] {
                            off_in += stride[idim];
                            ixm[idim] += 1;
                            break;
                        }
                        off_in -= stride[idim] * (hi[idim] - lo[idim]);
                        ixm[idim] = lo[idim];
                        idim += 1;
                    }
                    if idim == ndim {
                        break;
                    }
                }
            }
        }
        s.bad_var = bad_var;
        let ov = if usevar { out_var.as_deref_mut() } else { None };
        result += calc_and_assign(a, bad, &s, a.offset[point] as usize, out, ov);
    }
    result
}
