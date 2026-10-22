//! The spreading functions of [`super::rebin`]: mapping.c
//! `SpreadNearest<X>`, `SpreadLinear<X>` and `SpreadKernel1<X>`.
//!
//! Each adds every good input pixel into the output pixels around its
//! position, weighted, along with the weights and, as the flags ask,
//! variances. C writes each as eight or more loops, one per combination of
//! its flags, so the compiler can drop the tests a case does not need. The
//! port tests the flags in one loop; the arithmetic, and its order, are C's,
//! including each sum's arithmetic in the pixel type (see [`RebinValue`]).

use crate::constants::BAD_VALUE;
use crate::mappings::resample::kernels::KernelFn;
use crate::pixel::RebinValue;

use super::SpreadFlags;

/// What every spreading function is given: the output grid, and for each
/// input pixel its offset in the input array and its output position.
pub(super) struct Args<'a, T> {
    pub lbnd_out: &'a [i64],
    pub ubnd_out: &'a [i64],
    pub input: &'a [T],
    pub in_var: Option<&'a [T]>,
    /// A factor applied to every input value (`infac`), which conserves
    /// flux.
    pub infac: f64,
    pub offset: &'a [i64],
    pub coords: &'a [Vec<f64>],
    /// A weight applied to every input pixel (`conwgt`).
    pub conwgt: f64,
    pub flags: SpreadFlags,
    pub badval: T,
    pub npix_out: usize,
}

/// The arrays the spreading adds into. `work`, when given, holds the sum of
/// the weights given to each output pixel, followed, when output variances
/// are generated from the spread of input values, by the sum of their
/// squares.
pub(super) struct Accum<'a, T> {
    pub out: &'a mut [T],
    pub out_var: Option<&'a mut [T]>,
    pub work: Option<&'a mut [f64]>,
    pub nused: Option<&'a mut i64>,
}

/// The per-call cases C selects its loops by.
#[derive(Clone, Copy)]
struct Cases {
    usebad: bool,
    usevar: bool,
    genvar: bool,
    varwgt: bool,
}

impl Cases {
    fn new<T>(a: &Args<'_, T>, acc: &Accum<'_, T>) -> Self {
        let mut usevar = false;
        let mut genvar = false;
        if a.flags.gen_var {
            genvar = acc.out_var.is_some() && acc.work.is_some();
        } else if a.flags.use_var {
            usevar = a.in_var.is_some() && acc.out_var.is_some();
        }
        Cases {
            usebad: a.flags.use_bad,
            usevar,
            genvar,
            varwgt: a.flags.var_wgt && a.in_var.is_some() && acc.work.is_some(),
        }
    }
}

/// The input variance at `off_in`; only read where C reads it, which is
/// only when variances are given.
fn var_at<T: RebinValue>(a: &Args<'_, T>, off_in: usize) -> T {
    a.in_var.expect("variances are read only when given")[off_in]
}

/// Whether the input pixel at `off_in` is bad, or, when its variance is a
/// weight, has a variance that is bad or not positive.
fn input_bad<T: RebinValue>(a: &Args<'_, T>, k: Cases, off_in: usize) -> bool {
    if k.usebad {
        let mut bad = a.input[off_in] == a.badval;
        if k.varwgt {
            let var = var_at(a, off_in);
            bad = bad || var == a.badval || var.to_f64() <= 0.0;
        } else if k.usevar {
            bad = bad || var_at(a, off_in) == a.badval;
        }
        bad
    } else {
        k.varwgt && var_at(a, off_in).to_f64() <= 0.0
    }
}

/// Whether a coordinate lies outside `[xmin, xmax)` or is bad.
fn outside(x: f64, xmin: f64, xmax: f64) -> bool {
    x < xmin || x >= xmax || x == BAD_VALUE
}

/// The output grid's strides and coordinate limits, pixel edges included.
fn grid_limits(lbnd: &[i64], ubnd: &[i64]) -> (Vec<i64>, Vec<f64>, Vec<f64>) {
    let ndim = lbnd.len();
    let mut stride = vec![0_i64; ndim];
    let mut xmin = vec![0.0; ndim];
    let mut xmax = vec![0.0; ndim];
    let mut s = 1;
    for idim in 0..ndim {
        stride[idim] = s;
        s *= ubnd[idim] - lbnd[idim] + 1;
        xmin[idim] = lbnd[idim] as f64 - 0.5;
        xmax[idim] = ubnd[idim] as f64 + 0.5;
    }
    (stride, xmin, xmax)
}

impl<T: RebinValue> Accum<'_, T> {
    /// Add the input pixel at `off_in`, with weight `pixwt`, into the
    /// output pixel at `off_out`. `SpreadNearest<X>` and `SpreadLinear<X>`
    /// add the weighted value through `CONV`, a double; `SpreadKernel1<X>`
    /// adds it in the pixel type (`native`).
    fn deposit(
        &mut self,
        a: &Args<'_, T>,
        k: Cases,
        off_in: usize,
        off_out: usize,
        pixwt: f64,
        native: bool,
    ) {
        let in_val = a.input[off_in];
        let pfac = pixwt * a.infac;
        let c = T::from_c(T::conv(pfac * in_val.to_f64()));
        let o = &mut self.out[off_out];
        *o = if native {
            o.add(c)
        } else {
            T::from_c(o.to_f64() + T::conv(c.to_f64()))
        };
        if let Some(w) = self.work.as_deref_mut() {
            w[off_out] += pixwt;
        }
        if k.usevar {
            let ov = &mut self.out_var.as_deref_mut().expect("usevar")[off_out];
            let var = var_at(a, off_in).to_f64();
            *ov = T::from_c(ov.to_f64() + T::conv(var * pfac * pfac));
        } else if k.genvar && pixwt != 0.0 {
            let ov = &mut self.out_var.as_deref_mut().expect("genvar")[off_out];
            *ov = T::from_c(ov.to_f64() + c.square() / pixwt);
            self.work.as_deref_mut().expect("genvar")[off_out + a.npix_out] += pixwt * pixwt;
        }
    }

    fn count_used(&mut self) {
        if let Some(n) = self.nused.as_deref_mut() {
            *n += 1;
        }
    }
}

/// mapping.c `SpreadNearest<X>`: each input pixel is added into the output
/// pixel containing its position.
pub(super) fn nearest<T: RebinValue>(a: &Args<'_, T>, acc: &mut Accum<'_, T>) {
    let k = Cases::new(a, acc);
    let ndim = a.lbnd_out.len();
    let (stride, xmin, xmax) = grid_limits(a.lbnd_out, a.ubnd_out);
    let lbnd_out = a.lbnd_out;
    for point in 0..a.offset.len() {
        let off_in = a.offset[point] as usize;
        let mut bad = input_bad(a, k, off_in);
        let mut off_out: i64 = 0;
        if ndim == 1 {
            let x = a.coords[0][point];
            bad = bad || outside(x, xmin[0], xmax[0]);
            if !bad {
                off_out = (x + 0.5).floor() as i64 - lbnd_out[0];
            }
        } else if ndim == 2 {
            let y = a.coords[1][point];
            bad = bad || outside(y, xmin[1], xmax[1]);
            if !bad {
                let x = a.coords[0][point];
                bad = bad || outside(x, xmin[0], xmax[0]);
                if !bad {
                    let ix = (x + 0.5).floor() as i64 - lbnd_out[0];
                    let iy = (y + 0.5).floor() as i64 - lbnd_out[1];
                    off_out = ix + stride[1] * iy;
                }
            }
        } else if !bad {
            for idim in 0..ndim {
                let xn = a.coords[idim][point];
                bad = outside(xn, xmin[idim], xmax[idim]);
                if bad {
                    break;
                }
                let ixn = (xn + 0.5).floor() as i64 - lbnd_out[idim];
                off_out += ixn * stride[idim];
            }
        }
        if !bad {
            acc.count_used();
            let mut pixwt = a.conwgt;
            if k.varwgt {
                pixwt *= 1.0 / var_at(a, off_in).to_f64();
            }
            acc.deposit(a, k, off_in, off_out as usize, pixwt, false);
        }
    }
}

/// mapping.c `SpreadLinear<X>`: each input pixel is shared between the
/// output pixels around its position along each axis, in proportion to
/// their nearness.
pub(super) fn linear<T: RebinValue>(a: &Args<'_, T>, acc: &mut Accum<'_, T>) {
    let k = Cases::new(a, acc);
    let ndim = a.lbnd_out.len();
    let (stride, xmin, xmax) = grid_limits(a.lbnd_out, a.ubnd_out);
    let (lbnd_out, ubnd_out) = (a.lbnd_out, a.ubnd_out);

    // Workspace for the n-D path.
    let mut dim = vec![0_i64; ndim];
    let mut lo = vec![0_i64; ndim];
    let mut hi = vec![0_i64; ndim];
    let mut frac_lo = vec![0.0; ndim];
    let mut frac_hi = vec![0.0; ndim];
    let mut wt = vec![0.0; ndim];
    let mut wtprod = vec![0.0; ndim];

    for point in 0..a.offset.len() {
        let off_in = a.offset[point] as usize;
        let mut bad = input_bad(a, k, off_in);
        if ndim == 1 {
            let x = a.coords[0][point];
            bad = bad || outside(x, xmin[0], xmax[0]);
            if !bad {
                let lo_x = x.floor() as i64;
                let hi_x = lo_x + 1;
                let mut frac_lo_x = hi_x as f64 - x;
                let mut frac_hi_x = 1.0 - frac_lo_x;
                acc.count_used();
                let off_lo = lo_x - lbnd_out[0];
                let mut wgt = a.conwgt;
                if k.varwgt {
                    wgt *= 1.0 / var_at(a, off_in).to_f64();
                }
                frac_lo_x *= wgt;
                frac_hi_x *= wgt;
                if lo_x >= lbnd_out[0] {
                    acc.deposit(a, k, off_in, off_lo as usize, frac_lo_x, false);
                }
                if hi_x <= ubnd_out[0] {
                    acc.deposit(a, k, off_in, (off_lo + 1) as usize, frac_hi_x, false);
                }
            }
        } else if ndim == 2 {
            let ystride = stride[1];
            let y = a.coords[1][point];
            bad = bad || outside(y, xmin[1], xmax[1]);
            if !bad {
                let x = a.coords[0][point];
                bad = bad || outside(x, xmin[0], xmax[0]);
                if !bad {
                    acc.count_used();
                    let lo_x = x.floor() as i64;
                    let hi_x = lo_x + 1;
                    let mut frac_lo_x = hi_x as f64 - x;
                    let mut frac_hi_x = 1.0 - frac_lo_x;
                    let lo_y = y.floor() as i64;
                    let hi_y = lo_y + 1;
                    let mut frac_lo_y = hi_y as f64 - y;
                    let mut frac_hi_y = 1.0 - frac_lo_y;
                    // The weight is shared between the two axes' fractions,
                    // whose product forms each pixel's weight.
                    let mut wgt = a.conwgt;
                    if k.varwgt {
                        wgt *= 1.0 / crate::math::sqrt(var_at(a, off_in).to_f64());
                    }
                    frac_lo_x *= wgt;
                    frac_hi_x *= wgt;
                    frac_lo_y *= wgt;
                    frac_hi_y *= wgt;
                    let off_lo = lo_x - lbnd_out[0] + ystride * (lo_y - lbnd_out[1]);
                    if lo_y >= lbnd_out[1] {
                        if lo_x >= lbnd_out[0] {
                            let pixwt = frac_lo_x * frac_lo_y;
                            acc.deposit(a, k, off_in, off_lo as usize, pixwt, false);
                        }
                        if hi_x <= ubnd_out[0] {
                            let pixwt = frac_hi_x * frac_lo_y;
                            acc.deposit(a, k, off_in, (off_lo + 1) as usize, pixwt, false);
                        }
                    }
                    if hi_y <= ubnd_out[1] {
                        if lo_x >= lbnd_out[0] {
                            let pixwt = frac_lo_x * frac_hi_y;
                            let off = off_lo + ystride;
                            acc.deposit(a, k, off_in, off as usize, pixwt, false);
                        }
                        if hi_x <= ubnd_out[0] {
                            let pixwt = frac_hi_x * frac_hi_y;
                            let off = off_lo + ystride + 1;
                            acc.deposit(a, k, off_in, off as usize, pixwt, false);
                        }
                    }
                }
            }
        } else if !bad {
            let mut off_out: i64 = 0;
            for idim in 0..ndim {
                let xn = a.coords[idim][point];
                bad = outside(xn, xmin[idim], xmax[idim]);
                if bad {
                    break;
                }
                let ixn = xn.floor() as i64;
                lo[idim] = ixn.max(lbnd_out[idim]);
                hi[idim] = (ixn + 1).min(ubnd_out[idim]);
                frac_lo[idim] = 1.0 - (xn - lo[idim] as f64).abs();
                frac_hi[idim] = 1.0 - (xn - hi[idim] as f64).abs();
                dim[idim] = lo[idim];
                off_out += stride[idim] * (lo[idim] - lbnd_out[idim]);
                wt[idim] = frac_lo[idim];
            }
            // The weight is shared between the axes' fractions, whose
            // product forms each pixel's weight.
            let mut wgt = a.conwgt;
            if k.varwgt {
                wgt *= crate::math::powf(var_at(a, off_in).to_f64(), -1.0 / ndim as f64);
            }
            for idim in 0..ndim {
                frac_lo[idim] *= wgt;
                frac_hi[idim] *= wgt;
            }
            wt.copy_from_slice(&frac_lo);
            if !bad {
                acc.count_used();
                let mut idim = ndim - 1;
                wtprod[idim] = 1.0;
                loop {
                    for ii in (1..=idim).rev() {
                        wtprod[ii - 1] = wtprod[ii] * wt[ii];
                    }
                    let pixwt = wtprod[0] * wt[0];
                    acc.deposit(a, k, off_in, off_out as usize, pixwt, false);

                    // Step to the next pixel: the first axis still at its
                    // lower index moves to its higher one, and the axes
                    // before it return to their lower ones.
                    idim = 0;
                    let mut done = false;
                    loop {
                        if dim[idim] != hi[idim] {
                            dim[idim] = hi[idim];
                            off_out += stride[idim];
                            wt[idim] = frac_hi[idim];
                            break;
                        }
                        if dim[idim] != lo[idim] {
                            dim[idim] = lo[idim];
                            off_out -= stride[idim];
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
    }
}

/// Fill `filter[lo_j..=hi_j]` (of `nb2` values) with the kernel at offsets
/// `x0`, `x0 + 1`, ..., reflecting the values for pixels off the edge of
/// the output grid back onto it, as `SpreadKernel1<X>` does when the
/// kernel box overlaps an edge, so that the kernel's total weight lands on
/// the grid. The reflected part may itself overlap the opposite edge, which
/// C's modulus handles.
fn reflect(
    kernel: KernelFn,
    params: &[f64],
    mut x0: f64,
    lo_j: i64,
    hi_j: i64,
    nb2: i64,
    mut add: impl FnMut(usize, f64),
) {
    let nw = 2 * (hi_j - lo_j + 1);
    for j in 0..nb2 {
        let pixwt = kernel(x0, params);
        let mut jj = (j - lo_j) % nw + lo_j;
        if jj < lo_j {
            jj += nw;
        }
        if jj > hi_j {
            jj = 2 * hi_j - jj + 1;
        }
        add(jj as usize, pixwt);
        x0 += 1.0;
    }
}

/// mapping.c `SpreadKernel1<X>`: each input pixel is shared between the
/// output pixels within `neighb` pixels of its position along each axis,
/// weighted by the product of `kernel` evaluated at their offsets along
/// each axis, normalized to sum to one over the kernel box. Weight falling
/// off the grid is reflected back onto it. The kernel values are kept and
/// reused while successive points have the same offsets.
pub(super) fn kernel1<T: RebinValue>(
    a: &Args<'_, T>,
    acc: &mut Accum<'_, T>,
    kernel: KernelFn,
    neighb: i64,
    params: &[f64],
) {
    let k = Cases::new(a, acc);
    let ndim = a.lbnd_out.len();
    let (stride, _, _) = grid_limits(a.lbnd_out, a.ubnd_out);
    let (lbnd_out, ubnd_out) = (a.lbnd_out, a.ubnd_out);
    let nb2 = 2 * neighb;
    let nb2u = nb2 as usize;
    let kern = |x: f64| kernel(x, params);

    // C's filter: nb2 values per axis, or nb2 * nb2 in 2-D.
    let mut filter = vec![0.0; if ndim == 2 { nb2u * nb2u } else { nb2u }];
    let mut xfilter = vec![0.0; nb2u];
    let mut kval = vec![vec![0.0; nb2u]; if ndim > 2 { ndim } else { 0 }];
    let mut sum = 0.0;

    let weight = |sum: f64, off_in: usize| {
        if k.varwgt {
            a.conwgt / (sum * var_at(a, off_in).to_f64())
        } else {
            a.conwgt / sum
        }
    };

    if ndim == 1 {
        let mut xxl = BAD_VALUE;
        for point in 0..a.offset.len() {
            let off_in = a.offset[point] as usize;
            let mut bad = input_bad(a, k, off_in);
            let x = a.coords[0][point];
            let ix = (x + 0.5).floor() as i64;
            if ix < lbnd_out[0] || ix > ubnd_out[0] {
                bad = true;
            }
            bad = bad || x == BAD_VALUE;
            if bad {
                continue;
            }
            let ix = x.floor() as i64 - neighb + 1;
            let lo_ix = ix.max(lbnd_out[0]);
            let hi_ix = (ix + nb2 - 1).min(ubnd_out[0]);
            if lo_ix > hi_ix {
                continue;
            }
            acc.count_used();
            let lo_jx = lo_ix - ix;
            let hi_jx = hi_ix - ix;
            let off_xedge = hi_jx - lo_jx + 1 < nb2;
            let mut xx = ix as f64 - x;
            if xx != xxl || off_xedge {
                sum = 0.0;
                if off_xedge {
                    xxl = BAD_VALUE;
                    filter.fill(0.0);
                    reflect(kernel, params, xx, lo_jx, hi_jx, nb2, |j, w| {
                        filter[j] += w;
                        sum += w;
                    });
                } else {
                    xxl = xx;
                    for f in filter.iter_mut() {
                        let pixwt = kern(xx);
                        *f = pixwt;
                        sum += pixwt;
                        xx += 1.0;
                    }
                }
                if sum == 0.0 {
                    sum = 1.0;
                }
            }
            let wgt = weight(sum, off_in);
            let mut off_out = lo_ix - lbnd_out[0];
            for jx in lo_jx..=hi_jx {
                let pixwt = wgt * filter[jx as usize];
                acc.deposit(a, k, off_in, off_out as usize, pixwt, true);
                off_out += 1;
            }
        }
    } else if ndim == 2 {
        let ystride = stride[1];
        let (mut xxl, mut yyl) = (BAD_VALUE, BAD_VALUE);
        for point in 0..a.offset.len() {
            let off_in = a.offset[point] as usize;
            let mut bad = input_bad(a, k, off_in);
            let x = a.coords[0][point];
            let ix = (x + 0.5).floor() as i64;
            if ix < lbnd_out[0] || ix > ubnd_out[0] {
                bad = true;
            }
            bad = bad || x == BAD_VALUE;
            if bad {
                continue;
            }
            let y = a.coords[1][point];
            let iy = (y + 0.5).floor() as i64;
            if iy < lbnd_out[1] || iy > ubnd_out[1] {
                bad = true;
            }
            bad = bad || y == BAD_VALUE;
            if bad {
                continue;
            }
            let ix = x.floor() as i64 - neighb + 1;
            let lo_ix = ix.max(lbnd_out[0]);
            let hi_ix = (ix + nb2 - 1).min(ubnd_out[0]);
            let iy = y.floor() as i64 - neighb + 1;
            let lo_iy = iy.max(lbnd_out[1]);
            let hi_iy = (iy + nb2 - 1).min(ubnd_out[1]);
            if lo_ix > hi_ix || lo_iy > hi_iy {
                continue;
            }
            acc.count_used();
            let lo_jx = lo_ix - ix;
            let hi_jx = hi_ix - ix;
            let lo_jy = lo_iy - iy;
            let hi_jy = hi_iy - iy;
            let off_xedge = hi_jx - lo_jx + 1 < nb2;
            let off_yedge = hi_jy - lo_jy + 1 < nb2;
            let mut yy = iy as f64 - y;
            let mut xx = ix as f64 - x;
            if xx != xxl || yy != yyl || off_xedge || off_yedge {
                // The y kernel values fill each row of the filter.
                if off_yedge {
                    xxl = BAD_VALUE;
                    yyl = BAD_VALUE;
                    filter.fill(0.0);
                    reflect(kernel, params, yy, lo_jy, hi_jy, nb2, |jy, w| {
                        for f in &mut filter[jy * nb2u..(jy + 1) * nb2u] {
                            *f += w;
                        }
                    });
                } else {
                    xxl = xx;
                    yyl = yy;
                    for row in filter.chunks_mut(nb2u) {
                        let pixwt = kern(yy);
                        row.fill(pixwt);
                        yy += 1.0;
                    }
                }

                // Each column is then scaled by the x kernel value, and the
                // filter summed column by column.
                sum = 0.0;
                if off_xedge {
                    xxl = BAD_VALUE;
                    xfilter.fill(0.0);
                    reflect(kernel, params, xx, lo_jx, hi_jx, nb2, |jx, w| {
                        xfilter[jx] += w;
                    });
                    for jx in 0..nb2u {
                        for jy in 0..nb2u {
                            let f = &mut filter[jy * nb2u + jx];
                            *f *= xfilter[jx];
                            sum += *f;
                        }
                    }
                } else {
                    for jx in 0..nb2u {
                        let pixwt = kern(xx);
                        for jy in 0..nb2u {
                            let f = &mut filter[jy * nb2u + jx];
                            *f *= pixwt;
                            sum += *f;
                        }
                        xx += 1.0;
                    }
                }
                if sum == 0.0 {
                    sum = 1.0;
                }
            }
            let wgt = weight(sum, off_in);
            let mut off1 = lo_ix - lbnd_out[0] + ystride * (lo_iy - lbnd_out[1]);
            for jy in lo_jy..=hi_jy {
                let mut off_out = off1;
                for jx in lo_jx..=hi_jx {
                    let pixwt = wgt * filter[(jx + jy * nb2) as usize];
                    acc.deposit(a, k, off_in, off_out as usize, pixwt, true);
                    off_out += 1;
                }
                off1 += ystride;
            }
        }
    } else {
        // Each axis's kernel values are kept in `kval`, with the offset they
        // were computed for in `xnl`; `sum` is BAD_VALUE once any changes.
        let mut xnl = vec![BAD_VALUE; ndim];
        let mut lo = vec![0_i64; ndim];
        let mut hi = vec![0_i64; ndim];
        let mut jlo = vec![0_i64; ndim];
        let mut jhi = vec![0_i64; ndim];
        let mut w = vec![0_usize; ndim];
        let mut wtprod = vec![0.0; ndim];
        sum = BAD_VALUE;
        for point in 0..a.offset.len() {
            let off_in = a.offset[point] as usize;
            let mut bad = input_bad(a, k, off_in);
            if bad {
                continue;
            }
            let mut off_out: i64 = 0;
            for idim in 0..ndim {
                let xn = a.coords[idim][point];
                let ix = (xn + 0.5).floor() as i64;
                if ix < lbnd_out[idim] || ix > ubnd_out[idim] {
                    bad = true;
                }
                bad = bad || xn == BAD_VALUE;
                if bad {
                    break;
                }
                let ix = xn.floor() as i64 - neighb + 1;
                lo[idim] = ix.max(lbnd_out[idim]);
                hi[idim] = (ix + nb2 - 1).min(ubnd_out[idim]);
                jlo[idim] = lo[idim] - ix;
                jhi[idim] = hi[idim] - ix;
                if lo[idim] > hi[idim] {
                    bad = true;
                    break;
                }
                off_out += stride[idim] * (lo[idim] - lbnd_out[idim]);
                let off_xedge = jhi[idim] - jlo[idim] + 1 < nb2;
                let mut xxn = ix as f64 - xn;
                if xxn != xnl[idim] || off_xedge {
                    sum = BAD_VALUE;
                    let kv = &mut kval[idim];
                    if off_xedge {
                        xnl[idim] = BAD_VALUE;
                        kv.fill(0.0);
                        reflect(kernel, params, xxn, jlo[idim], jhi[idim], nb2, |j, wv| {
                            kv[j] += wv;
                        });
                    } else {
                        xnl[idim] = xxn;
                        for v in kv.iter_mut() {
                            *v = kern(xxn);
                            xxn += 1.0;
                        }
                    }
                }
            }
            if bad {
                continue;
            }

            // The sum of the kernel weights over the whole box, formed as
            // the weights themselves are below.
            if sum == BAD_VALUE {
                w.fill(0);
                let mut idim = ndim - 1;
                wtprod[idim] = 1.0;
                sum = 0.0;
                loop {
                    for ii in (1..=idim).rev() {
                        wtprod[ii - 1] = wtprod[ii] * kval[ii][w[ii]];
                    }
                    sum += wtprod[0] * kval[0][w[0]];
                    idim = 0;
                    let mut done = false;
                    loop {
                        if w[idim] != nb2u - 1 {
                            w[idim] += 1;
                            break;
                        }
                        w[idim] = 0;
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
                if sum == 0.0 {
                    sum = 1.0;
                }
            }
            for idim in 0..ndim {
                w[idim] = jlo[idim] as usize;
            }
            let wgt = weight(sum, off_in);
            acc.count_used();
            let mut idim = ndim - 1;
            wtprod[idim] = 1.0;
            loop {
                for ii in (1..=idim).rev() {
                    wtprod[ii - 1] = wtprod[ii] * kval[ii][w[ii]];
                }
                let pixwt = (wtprod[0] * kval[0][w[0]]) * wgt;
                acc.deposit(a, k, off_in, off_out as usize, pixwt, true);
                idim = 0;
                let mut done = false;
                loop {
                    if w[idim] != jhi[idim] as usize {
                        w[idim] += 1;
                        off_out += stride[idim];
                        break;
                    }
                    w[idim] = jlo[idim] as usize;
                    off_out -= stride[idim] * (jhi[idim] - jlo[idim]);
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
}
