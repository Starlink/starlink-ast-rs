//! The interpolators of [`super::resample`]: mapping.c
//! `InterpolateNearest<X>` and `InterpolateLinear<X>`.
//!
//! C writes each as eight loops, one per combination of the `Usebad`,
//! `Usevar` and `Nobad` cases, so the compiler can drop the tests a case does
//! not need. The port tests the three flags in one loop; the arithmetic, and
//! its order, are C's.

use crate::constants::BAD_VALUE;
use crate::pixel::{PixelValue, SumValue};

use super::ResampleFlags;

/// What every interpolator is given: the input grid, and for each output
/// point its input position and its offset in the output array.
pub(super) struct Args<'a, T> {
    pub lbnd_in: &'a [i64],
    pub ubnd_in: &'a [i64],
    pub input: &'a [T],
    pub in_var: Option<&'a [T]>,
    pub offset: &'a [i64],
    pub coords: &'a [Vec<f64>],
    pub flags: ResampleFlags,
    pub badval: T,
}

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

/// The sums from which a linearly interpolated value is formed.
struct LinearSums<S> {
    sum: S,
    sum_var: S,
    wtsum: S,
    bad_var: bool,
}

impl<S: SumValue> LinearSums<S> {
    fn new() -> Self {
        LinearSums {
            sum: S::ZERO,
            sum_var: S::ZERO,
            wtsum: S::ZERO,
            bad_var: false,
        }
    }

    /// mapping.c `FORM_LINEAR_INTERPOLATION_SUM`: add the input pixel at
    /// `off` with weight `wt`.
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
        let mut sums = LinearSums::<T::Sum>::new();
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
