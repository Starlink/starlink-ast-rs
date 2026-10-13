//! Port of `fitschan.c::IsMapLinear` (fitschan.c:19388).
//!
//! Tests whether a specified output of a mapping is a linear function
//! of the inputs. Used by `other_axes` (and the deferred `LogAxis` /
//! `IsMapTab1D` paths) to gate the "always-linear ShiftMap"
//! assumption.

#![allow(dead_code)]
#![allow(clippy::needless_range_loop)] // mirrors C index loops line-for-line

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::mappings::Mapping;
use crate::mappings::helpers::ast_equal;

/// Number of test points used for the inverse-transform fit.
/// Mirrors `NP` at `fitschan.c:19452`.
const NP: usize = 20;

/// Return `true` if the specified output axis of `smap` is a linear
/// function of the mapping inputs over the input box `[lbnd_in,
/// ubnd_in]`, as C's `IsMapLinear` (fitschan.c:19388) decides it:
/// `astMapBox` gives the range of the output and an input position at
/// which it is lowest; `NP` points spread along the output, the other
/// outputs held at their values there, are transformed back to the inputs;
/// and a straight line fitted to each input that varies must reproduce it
/// to within 1e-7 of its range. The output is first split off the Mapping
/// when a single input feeds it.
///
/// `coord_out` is 0-based.
pub(crate) fn is_map_linear(
    smap: &dyn Mapping,
    lbnd_in: &[f64],
    ubnd_in: &[f64],
    coord_out: usize,
) -> AstResult<bool> {
    // Split off the requested output, inverting the Mapping so that any
    // outputs whose inverse is undefined do not stop the split. If a single
    // input feeds the output, use the one-input Mapping and that input's
    // bounds.
    let (map, lbnd_in, ubnd_in, coord_out) = {
        let mut inv = smap.clone_box();
        inv.invert();
        match inv.map_split_zero_based(&[coord_out])? {
            Some((ins, mut single)) if single.nout() == 1 => {
                single.invert();
                let k = ins[0];
                (single, vec![lbnd_in[k]], vec![ubnd_in[k]], 0_usize)
            }
            _ => (
                smap.clone_box(),
                lbnd_in.to_vec(),
                ubnd_in.to_vec(),
                coord_out,
            ),
        }
    };

    // The Mapping must be defined in both directions.
    if !map.has_forward() || !map.has_inverse() {
        return Ok(false);
    }
    let nin = map.nin();
    let nout = map.nout();

    // The bounds of the output, and an input position at which it is
    // lowest. A box that cannot be found makes the Mapping non-linear; equal
    // bounds are widened.
    let Ok(bx) = map.map_box(&lbnd_in, &ubnd_in, true, coord_out + 1) else {
        return Ok(false);
    };
    let (mut lbnd_out, mut ubnd_out) = (bx.lbnd_out, bx.ubnd_out);
    if ast_equal(lbnd_out, ubnd_out) {
        let m = 0.5 * (lbnd_out + ubnd_out);
        if m.abs() > 1.0e-15 {
            lbnd_out = 0.9 * m;
            ubnd_out = 1.1 * m;
        } else {
            lbnd_out = -1.0;
            ubnd_out = 1.0;
        }
    }

    // The outputs at that input position.
    let p0 = map.transform(&PointSet::from_rows([bx.xl.clone()]), true)?;
    let p0: Vec<f64> = (0..nout).map(|i| p0.point(0)[i]).collect();

    // `NP` positions evenly spread along the output, the other outputs held
    // at their values above, transformed back to the inputs.
    let delta = (ubnd_out - lbnd_out) / (NP - 1) as f64;
    let rows: Vec<Vec<f64>> = (0..NP)
        .map(|j| {
            let mut row = p0.clone();
            row[coord_out] = lbnd_out + j as f64 * delta;
            row
        })
        .collect();
    let out = PointSet::from_rows(rows);
    let inp = map.transform(&out, false)?;

    // Fit a straight line to each input coordinate as a function of the
    // output. The fit is made to values offset by the first value of each,
    // since the sums formed from values far from zero compared to their
    // range lose the fit to rounding.
    let s: Vec<f64> = (0..NP).map(|j| out.point(j)[coord_out]).collect();
    let soff = s[0];
    for i in 0..nin {
        let mut p: Vec<f64> = (0..NP).map(|j| inp.point(j)[i]).collect();
        let poff = p[0];

        // The sums, and the smallest and largest input value. C starts
        // the largest at DBL_MIN, the smallest positive double, rather than
        // -DBL_MAX, so for inputs that are all negative it stays DBL_MIN.
        let (mut sp, mut ss, mut sps, mut sn, mut ss2) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut in_lbnd = f64::MAX;
        let mut in_ubnd = f64::MIN_POSITIVE;
        for j in 0..NP {
            let (pv, sv) = (p[j], s[j]);
            if pv != BAD_VALUE && sv != BAD_VALUE {
                if pv < in_lbnd {
                    in_lbnd = pv;
                }
                if pv > in_ubnd {
                    in_ubnd = pv;
                }
                let (pv, sv) = (pv - poff, sv - soff);
                sp += pv;
                ss += sv;
                sps += pv * sv;
                sn += 1.0;
                ss2 += sv * sv;
            } else {
                sn = 0.0;
                break;
            }
        }

        // Inputs that do not vary with the output are ignored.
        if ast_equal(in_lbnd, in_ubnd) {
            continue;
        }
        let d = ss * ss - sn * ss2;
        if !(sn > 0.0 && d != 0.0) {
            return Ok(false);
        }

        // First fit, "input = m*output + c", and its residuals.
        let m = (sp * ss - sps * sn) / d;
        let c = (sps * ss - sp * ss2) / d;
        for j in 0..NP {
            p[j] -= poff + m * (s[j] - soff) + c;
        }

        // A second fit to the residuals, since the first sometimes leaves
        // them with a gradient, and their residuals.
        sp = 0.0;
        sps = 0.0;
        for j in 0..NP {
            sp += p[j];
            sps += p[j] * (s[j] - soff);
        }
        let m = (sp * ss - sps * sn) / d;
        let c = (sps * ss - sp * ss2) / d;
        for j in 0..NP {
            p[j] -= m * (s[j] - soff) + c;
        }

        // Every residual must be within 1e-7 of the range of the input.
        let tol = 1.0e-7 * (in_ubnd - in_lbnd);
        if p.iter().any(|r| r.abs() > tol) {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZoomMap over a narrow range far from zero: the Hz to GHz unit
    /// Mapping of 20 channels of 39062.5 Hz at 1.1e11 Hz.
    #[test]
    fn linear_far_from_zero() {
        let m = ZoomMap::new(1, 1.0e-9).unwrap();
        let lo = 1.102e11 - 9.0 * 39062.5;
        let hi = 1.102e11 + 10.0 * 39062.5;
        assert!(is_map_linear(&m, &[lo], &[hi], 0).unwrap());
    }
    use crate::mappings::{ShiftMap, UnitMap, WcsMap, WcsProjection, WinMap, ZoomMap};

    #[test]
    fn unit_map_is_linear() {
        let m = UnitMap::new(2);
        let r = is_map_linear(&m, &[0.0, 0.0], &[1.0, 1.0], 0).unwrap();
        assert!(r);
        let r = is_map_linear(&m, &[0.0, 0.0], &[1.0, 1.0], 1).unwrap();
        assert!(r);
    }

    #[test]
    fn shift_map_is_linear() {
        let m = ShiftMap::new(vec![5.0, 10.0]);
        let r = is_map_linear(&m, &[0.0, 0.0], &[100.0, 100.0], 0).unwrap();
        assert!(r);
    }

    #[test]
    fn zoom_map_is_linear() {
        let m = ZoomMap::new(2, 7.5).unwrap();
        let r = is_map_linear(&m, &[0.0, 0.0], &[1.0, 1.0], 0).unwrap();
        assert!(r);
    }

    #[test]
    fn winmap_is_linear() {
        let m = WinMap::new(&[-128.0, -128.0], &[0.001, 0.001]).unwrap();
        let r = is_map_linear(&m, &[1.0, 1.0], &[100.0, 100.0], 0).unwrap();
        assert!(r);
    }

    #[test]
    fn tan_over_large_box_is_not_linear() {
        // TAN over [-1, 1] rad is highly non-linear.
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let r = is_map_linear(&wm, &[-1.0, -1.0], &[1.0, 1.0], 0).unwrap();
        assert!(!r);
    }

    #[test]
    fn out_of_range_axis_returns_false() {
        let m = UnitMap::new(2);
        let r = is_map_linear(&m, &[0.0, 0.0], &[1.0, 1.0], 5).unwrap();
        assert!(!r);
    }
}
