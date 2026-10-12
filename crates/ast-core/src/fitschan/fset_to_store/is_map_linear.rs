//! Port of `fitschan.c::IsMapLinear` (~19136).
//!
//! Tests whether a specified output of a mapping is a linear function
//! of the inputs. Used by `other_axes` (and the deferred `LogAxis` /
//! `IsMapTab1D` paths) to gate the "always-linear ShiftMap"
//! assumption.

#![allow(dead_code)]
#![allow(clippy::needless_range_loop)] // mirrors C index loops line-for-line

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::mappings::{DynMapping, Mapping};

/// Number of test points used for the inverse-transform fit.
/// Mirrors `NP` at `fitschan.c:19254`.
const NP: usize = 20;

/// Return `true` if the specified output axis of `smap` is a linear
/// function of the mapping inputs over the input box `[lbnd_in,
/// ubnd_in]`. Returns `false` otherwise (including when the mapping
/// has no defined inverse, when the bounding box can't be sampled,
/// or when the linear-fit residuals exceed `1e-7 *
/// (in_ubnd - in_lbnd)` per input axis).
///
/// `coord_out` is 0-based.
///
/// Mirrors `fitschan.c::IsMapLinear` (~19136).
///
/// Implementation notes:
/// - Uses `Mapping::map_split` on the inverted mapping to isolate the
///   single output, falling back to the full mapping when the split
///   doesn't apply. Mirrors the C `astInvert; astMapSplit; astInvert`
///   dance at lines 19246-19277.
/// - Replaces `astMapBox` (which we don't have ported) with a simple
///   corner-sampling estimator: transform every corner of the input
///   box, take min/max of the chosen output. For the typical use case
///   (single-axis sub-mappings) the corner extrema match the output
///   range exactly. For unusual cases the estimator under-bounds
///   the range, which is conservative — the linearity check sees a
///   smaller sample-grid and might fail on a curve that would have
///   been ruled linear under the true range; we accept that as the
///   safe direction.
pub(crate) fn is_map_linear(
    smap: &dyn Mapping,
    lbnd_in: &[f64],
    ubnd_in: &[f64],
    coord_out: usize,
) -> AstResult<bool> {
    let nin_orig = smap.nin();
    let nout_orig = smap.nout();
    if lbnd_in.len() != nin_orig || ubnd_in.len() != nin_orig {
        return Ok(false);
    }
    if coord_out >= nout_orig {
        return Ok(false);
    }

    // Try to split off the requested output axis. Mirrors C:19246-19277.
    let (working_map, working_lbnd, working_ubnd, working_coord_out) = {
        let mut inv = smap.clone_box();
        inv.invert();
        match inv.map_split_zero_based(&[coord_out])? {
            Some((kept_in_axes, mut single)) if single.nout() == 1 => {
                // Splitted: now `single` goes (1 input from coord_out)
                // → (kept_in_axes' worth of outputs, length 1 because
                // we asked for 1). Invert back so it goes input →
                // output again. The lone output corresponds to the
                // first kept input; restrict the box bounds accordingly.
                single.invert();
                let kept_idx = kept_in_axes[0];
                let lbnd = vec![lbnd_in[kept_idx]];
                let ubnd = vec![ubnd_in[kept_idx]];
                (single, lbnd, ubnd, 0_usize)
            }
            _ => {
                // Fall back to the full mapping.
                (
                    smap.clone_box(),
                    lbnd_in.to_vec(),
                    ubnd_in.to_vec(),
                    coord_out,
                )
            }
        }
    };

    if !working_map.has_forward() || !working_map.has_inverse() {
        return Ok(false);
    }

    let nin = working_map.nin();
    let nout = working_map.nout();

    // Step 1: corner-sample the input box to find the output range
    // for `working_coord_out`. Also record the input position at the
    // minimum-output corner — used as the seed for the test points.
    let nverts = 1usize << nin;
    let mut corner_inputs: Vec<Vec<f64>> = Vec::with_capacity(nverts);
    for k in 0..nverts {
        let mut row = Vec::with_capacity(nin);
        for i in 0..nin {
            row.push(if (k >> i) & 1 == 0 {
                working_lbnd[i]
            } else {
                working_ubnd[i]
            });
        }
        corner_inputs.push(row);
    }
    let pts_corner_in = PointSet::from_rows(corner_inputs.clone());
    let pts_corner_out = match working_map.transform(&pts_corner_in, true) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };

    let mut lbnd_out = f64::INFINITY;
    let mut ubnd_out = f64::NEG_INFINITY;
    let mut min_idx = 0_usize;
    for k in 0..nverts {
        let v = pts_corner_out.point(k)[working_coord_out];
        if !v.is_finite() {
            return Ok(false);
        }
        if v < lbnd_out {
            lbnd_out = v;
            min_idx = k;
        }
        if v > ubnd_out {
            ubnd_out = v;
        }
    }
    // Mirrors C:19314-19323 — handle the bounds-equal degenerate.
    if (ubnd_out - lbnd_out).abs() < 1e-12 {
        let m = 0.5 * (lbnd_out + ubnd_out);
        if m.abs() > 1e-15 {
            lbnd_out = 0.9 * m;
            ubnd_out = 1.1 * m;
        } else {
            lbnd_out = -1.0;
            ubnd_out = 1.0;
        }
    }

    // Step 2: build NP output positions evenly spaced along
    // working_coord_out, with the other output coords held at the
    // values produced by the min-corner. Mirrors C:19343-19356.
    let xl = corner_inputs[min_idx].clone();
    let pts_seed_in = PointSet::from_rows([xl.clone()]);
    let pts_seed_out = match working_map.transform(&pts_seed_in, true) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    let seed_out: Vec<f64> = (0..nout).map(|i| pts_seed_out.point(0)[i]).collect();

    let delta = (ubnd_out - lbnd_out) / ((NP - 1) as f64);
    let mut out_rows: Vec<Vec<f64>> = Vec::with_capacity(NP);
    for j in 0..NP {
        let mut row = seed_out.clone();
        row[working_coord_out] = lbnd_out + (j as f64) * delta;
        out_rows.push(row);
    }
    let pts_test_out = PointSet::from_rows(out_rows);

    // Step 3: inverse-transform to get the corresponding input
    // positions. C:19360.
    let pts_test_in = match working_map.transform(&pts_test_out, false) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };

    // Step 4: per-input-axis linear fit + residual check.
    // Mirrors C:19367-19449. The fit is made to values offset by the first
    // value of each, since the sums formed from values far from zero
    // compared to their range lose the fit to rounding.
    let s: Vec<f64> = (0..NP)
        .map(|j| pts_test_out.point(j)[working_coord_out])
        .collect();
    let soff = s[0];

    for i in 0..nin {
        let mut p: Vec<f64> = (0..NP).map(|j| pts_test_in.point(j)[i]).collect();
        let poff = p[0];

        // Compute sums + bounds (no BAD handling — mapping outputs
        // were validated finite above).
        let mut sp = 0.0_f64;
        let mut ss = 0.0_f64;
        let mut sps = 0.0_f64;
        let mut ss2 = 0.0_f64;
        let mut sn = 0.0_f64;
        let mut in_lbnd = f64::INFINITY;
        let mut in_ubnd = f64::NEG_INFINITY;
        let mut had_bad = false;
        for j in 0..NP {
            let pv = p[j];
            let sv = s[j];
            if !pv.is_finite() || !sv.is_finite() {
                had_bad = true;
                break;
            }
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
            ss2 += sv * sv;
            sn += 1.0;
        }
        if had_bad {
            return Ok(false);
        }

        // Skip input axes independent of the output axis.
        if (in_lbnd - in_ubnd).abs() < 1e-12 {
            continue;
        }

        // First fit: input = m*output + c.
        let d = ss * ss - sn * ss2;
        if sn <= 0.0 || d == 0.0 {
            return Ok(false);
        }
        let m = (sp * ss - sps * sn) / d;
        let c = (sps * ss - sp * ss2) / d;
        for j in 0..NP {
            p[j] -= poff + m * (s[j] - soff) + c;
        }

        // Second fit on the residuals (sometimes the first fit leaves
        // a residual gradient — mirrors C:19410-19426).
        sp = 0.0;
        sps = 0.0;
        for j in 0..NP {
            sp += p[j];
            sps += p[j] * (s[j] - soff);
        }
        let m2 = (sp * ss - sps * sn) / d;
        let c2 = (sps * ss - sp * ss2) / d;
        for j in 0..NP {
            p[j] -= m2 * (s[j] - soff) + c2;
        }

        // Tolerance: 1e-7 of the input-axis range. Mirrors C:19437.
        let tol = 1.0e-7 * (in_ubnd - in_lbnd);
        for j in 0..NP {
            if p[j].abs() > tol {
                return Ok(false);
            }
        }
    }

    let _ = (nout_orig, working_map as DynMapping);
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
    fn wrong_dim_returns_false() {
        let m = UnitMap::new(2);
        let r = is_map_linear(&m, &[0.0], &[1.0, 2.0], 0).unwrap();
        assert!(!r);
    }

    #[test]
    fn out_of_range_axis_returns_false() {
        let m = UnitMap::new(2);
        let r = is_map_linear(&m, &[0.0, 0.0], &[1.0, 1.0], 5).unwrap();
        assert!(!r);
    }
}
