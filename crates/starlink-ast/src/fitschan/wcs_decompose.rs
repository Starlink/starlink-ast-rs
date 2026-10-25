// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Decompose a FrameSet mapping into FITS-WCS linear components.
//!
//! The decomposition extracts: CRPIX (reference pixel shift), a rotation/scale
//! matrix (PC or CD), and identifies the non-linear portion (projection) if
//! present. This mirrors the C library's `MakeIntWorld` function (fitschan.c
//! ~line 21689).

use ndarray::Array2;

use crate::coords::PointSet;
use crate::mappings::Mapping;
use crate::mappings::helpers::is_bad;

// ---------------------------------------------------------------------------
// FindBasisVectors / FitLine port (fitschan.c:10393-15171)
// ---------------------------------------------------------------------------

/// Result of [`find_basis_vectors`]: the chosen root pixel, the IWC
/// coordinates at it, and a per-output-axis tolerance derived from
/// the local Jacobian (used by `FitLine` to decide whether the fit is
/// usable). All vectors are length `nout` for `w0` / `tol` and length
/// `nin` for `g0`.
///
/// Used by [`crate::fitschan::fset_to_store::make_int_world`] (the
/// MakeIntWorld port).
#[derive(Debug, Clone)]
pub(crate) struct BasisVectors {
    /// Root pixel position.
    pub g0: Vec<f64>,
    /// `mapping(g0)`.
    pub w0: Vec<f64>,
    /// Per-output-axis fit tolerance (RMS).
    pub tol: Vec<f64>,
}

/// Probe the mapping at a candidate pixel `g0` and check that the
/// transformed root and unit-step neighbors are all finite. Mirrors
/// `MakeBasisVectors` (fitschan.c:10393-10516). Returns `Some((w0,
/// step_iwc))` where `step_iwc[i]` is the IWC vector at `g0 + e_i`,
/// or `None` if any output is non-finite.
fn make_basis_vectors(
    mapping: &dyn Mapping,
    nin: usize,
    nout: usize,
    g0: &[f64],
) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(nin + 1);
    rows.push(g0.to_vec());
    for i in 0..nin {
        let mut row = g0.to_vec();
        row[i] += 1.0;
        rows.push(row);
    }
    let pts = PointSet::from_rows(rows);
    let out = mapping.transform(&pts, true).ok()?;
    let view = out.view();

    let mut w0 = vec![0.0_f64; nout];
    for j in 0..nout {
        let v = view[[j, 0]];
        if is_bad(v) {
            return None;
        }
        w0[j] = v;
    }
    let mut step = vec![vec![0.0_f64; nout]; nin];
    for i in 0..nin {
        for j in 0..nout {
            let v = view[[j, i + 1]];
            if is_bad(v) {
                return None;
            }
            step[i][j] = v;
        }
    }
    Some((w0, step))
}

/// Pick a root pixel `g0` for which the mapping produces all-finite
/// IWC coordinates at `g0` and at each unit-step neighbor. Mirrors
/// `FindBasisVectors` (fitschan.c:10518-10663): try grid center
/// `0.5*(1+dim)` first, then `(1, 1, …)`, then progressively further
/// out along the diagonal and individual axes. Returns the chosen
/// `g0` plus the tolerance derived from the local IWC step sizes.
pub(crate) fn find_basis_vectors(
    mapping: &dyn Mapping,
    nin: usize,
    nout: usize,
    dim: &[Option<f64>],
    fitstol: f64,
) -> Option<BasisVectors> {
    let mut ddlim: f64 = 0.0;
    let mut g0: Vec<f64>;
    let mut probe: Option<(Vec<f64>, Vec<Vec<f64>>)> = None;

    // Attempt 1: grid center when all dims are known.
    let centred: Option<Vec<f64>> = (0..nin)
        .map(|i| dim.get(i).copied().flatten())
        .collect::<Option<Vec<f64>>>();
    if let Some(centre_dims) = &centred {
        g0 = centre_dims.iter().map(|d| 0.5 * (1.0 + d)).collect();
        for &d in centre_dims {
            if d > ddlim {
                ddlim = d;
            }
        }
        probe = make_basis_vectors(mapping, nin, nout, &g0);
    } else {
        g0 = vec![1.0; nin];
    }

    // Attempt 2: (1, 1, ...).
    if probe.is_none() {
        g0 = vec![1.0; nin];
        probe = make_basis_vectors(mapping, nin, nout, &g0);
    }

    // Attempts 3+: diagonal sweep moving out from the origin.
    let mut dd = 10.0_f64;
    if ddlim == 0.0 {
        ddlim = 10240.0;
    }
    while probe.is_none() && dd <= ddlim {
        // Diagonal across the center, scaled by aspect ratio when
        // dims are known.
        for (i, slot) in g0.iter_mut().enumerate().take(nin) {
            *slot = match dim.get(i).copied().flatten() {
                Some(d) => dd * d / ddlim,
                None => dd,
            };
        }
        probe = make_basis_vectors(mapping, nin, nout, &g0);

        // Move out along each axis individually if the diagonal failed.
        if probe.is_none() {
            for ii in 0..nin {
                for v in g0.iter_mut() {
                    *v = 1.0;
                }
                g0[ii] = dd;
                probe = make_basis_vectors(mapping, nin, nout, &g0);
                if probe.is_some() {
                    break;
                }
            }
        }
        dd *= 2.0;
    }

    let (w0, step) = probe?;

    // Tolerance per output axis: max over input axes of the IWC
    // step magnitude × `fitstol`. fitschan.c:22064-22076.
    let mut tol = vec![0.0_f64; nout];
    for j in 0..nout {
        let mut t = 0.0_f64;
        for step_row in step.iter().take(nin) {
            let err = (step_row[j] - w0[j]).abs();
            if err > t {
                t = err;
            }
        }
        t *= fitstol;
        if t == 0.0 {
            t = w0[j].abs() * f64::EPSILON * 1.0e5;
        }
        if t == 0.0 {
            t = crate::math::sqrt(f64::MIN_POSITIVE);
        }
        tol[j] = t;
    }

    Some(BasisVectors { g0, w0, tol })
}

/// RMS-residual gate. Mirrors `FitOK` (fitschan.c:11066).
fn fit_ok(actual: &[f64], estimated: &[f64], tol: f64) -> bool {
    let mut s1 = 0.0_f64;
    let mut s2 = 0.0_f64;
    for (a, e) in actual.iter().zip(estimated.iter()) {
        if is_bad(*a) || is_bad(*e) {
            continue;
        }
        let d = a - e;
        s1 += d * d;
        s2 += 1.0;
    }
    if s2 == 0.0 {
        return false;
    }
    crate::math::sqrt(s1 / s2) <= tol
}

/// Least-squares straight-line fit along input axis `axis` of
/// `mapping`. Returns the per-pixel slope as a vector of `nout`
/// elements (the column of the CD matrix), or `None` if the fit
/// is not linear (RMS residual exceeds the supplied tolerance per
/// output axis). Mirrors `FitLine` (fitschan.c:14931-15171).
pub(crate) fn fit_line(
    mapping: &dyn Mapping,
    axis: usize,
    g0: &[f64],
    w0: &[f64],
    dim: Option<f64>,
    tol: &[f64],
) -> Option<Vec<f64>> {
    const NPO2: i64 = 50;
    const NP_USIZE: usize = (2 * NPO2 + 1) as usize;
    let nin = g0.len();
    let nout = w0.len();
    if nout < nin {
        return None;
    }
    if w0.iter().any(|&v| is_bad(v)) {
        return None;
    }
    // Mirrors C `FitLine` (~line 14950): when dim is BAD/unknown,
    // use a gap of 1.0 (corresponds to an axis length of NP pixels).
    let gap = match dim {
        Some(0.0) => return None,
        Some(d) => d / NP_USIZE as f64,
        None => 1.0,
    };

    // Sample NP_USIZE points along input axis `axis`, centered at g0,
    // spaced by `gap`.
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(NP_USIZE);
    for k in -NPO2..=NPO2 {
        let mut row = g0.to_vec();
        row[axis] += gap * (k as f64);
        rows.push(row);
    }
    let pts = PointSet::from_rows(rows);
    let out = mapping.transform(&pts, true).ok()?;
    let view = out.view();

    // Per output axis: least-squares fit
    //   pax[k] = slope·k + offset
    // (k counted from −NPO2 to +NPO2). The slope is `col[j]`; we
    // divide by `gap` at the end to get per-pixel.
    let mut col = vec![0.0_f64; nout];
    let mut offset = vec![0.0_f64; nout];
    for j in 0..nout {
        let mut sdw = 0.0_f64;
        let mut sw = 0.0_f64;
        let mut sd = 0.0_f64;
        let mut sd2 = 0.0_f64;
        let mut n = 0_usize;
        let mut wmax = f64::MIN;
        let mut wmin = f64::MAX;
        for (idx, k) in (-NPO2..=NPO2).enumerate() {
            let v = view[[j, idx]];
            if is_bad(v) {
                continue;
            }
            let kf = k as f64;
            sdw += kf * v;
            sw += v;
            sd += kf;
            sd2 += kf * kf;
            n += 1;
            if v > wmax {
                wmax = v;
            }
            if v < wmin {
                wmin = v;
            }
        }
        let denom = sd2 * (n as f64) - sd * sd;
        if n <= NP_USIZE / 4 || denom == 0.0 {
            return None;
        }
        col[j] = if wmax > wmin {
            (sdw * (n as f64) - sw * sd) / denom
        } else {
            0.0
        };
        offset[j] = (sw * sd2 - sdw * sd) / denom;
    }

    // Verify the fit is good on every output axis.
    let mut estimated = vec![0.0_f64; NP_USIZE];
    for j in 0..nout {
        for (idx, k) in (-NPO2..=NPO2).enumerate() {
            estimated[idx] = (k as f64) * col[j] + offset[j];
        }
        let actual: Vec<f64> = (0..NP_USIZE).map(|idx| view[[j, idx]]).collect();
        if !fit_ok(&actual, &estimated, tol[j]) {
            return None;
        }
    }

    // Per-pixel slope.
    for v in col.iter_mut() {
        *v /= gap;
    }
    Some(col)
}

/// Build a square `nout × nout` CD matrix using `FitLine` to extract
/// each column from the supplied `mapping`. Returns `None` if any
/// `FitLine` call fails (i.e. the mapping is not linear within
/// tolerance on some axis). Mirrors the per-axis `FitLine` loop in
/// `MakeIntWorld` (fitschan.c:22089-22106).
pub(crate) fn fit_line_cd(
    mapping: &dyn Mapping,
    nin: usize,
    nout: usize,
    g0: &[f64],
    w0: &[f64],
    dim: &[Option<f64>],
    tol: &[f64],
) -> Option<Array2<f64>> {
    let mut cd = Array2::<f64>::zeros((nout, nout));
    for i in 0..nin {
        let column = fit_line(mapping, i, g0, w0, dim.get(i).copied().flatten(), tol)?;
        for j in 0..nout {
            cd[[j, i]] = column[j];
        }
    }
    Some(cd)
}

/// Split a CD matrix into CDELT (diagonal scale) and PC (rotation) components.
///
/// `CD = diag(CDELT) * PC`, where each row of PC is normalized to unit
/// length. The sign of CDELT[i] is chosen so the diagonal element of the
/// resulting PC matrix is positive. PC entries with `|val| < 1e-7` are
/// snapped to 0 to suppress floating-point residue from upstream linear
/// extraction.
///
/// Mirrors C `SplitMat` (fitschan.c:33272-33375).
pub(crate) fn split_cd(cd: &Array2<f64>) -> (Vec<f64>, Array2<f64>) {
    const SPLITMAT_THRESHOLD: f64 = 1.0e-7;

    let naxis = cd.nrows();
    let mut cdelt = vec![0.0; naxis];
    let mut pc = Array2::zeros((naxis, naxis));

    for i in 0..naxis {
        // Note the sign of the diagonal term *before* magnitude
        // computation so we can flip CDELT below.
        let dineg = cd[[i, i]] < 0.0;

        // CDELT magnitude = √(Σ_j CD[i][j]²).
        let mut mag_sq = 0.0;
        for j in 0..naxis {
            mag_sq += cd[[i, j]] * cd[[i, j]];
        }
        let mut cdlt = crate::math::sqrt(mag_sq);
        if dineg {
            cdlt = -cdlt;
        }
        cdelt[i] = cdlt;

        // PC[i][j] = CD[i][j] / CDELT[i], with the C 1e-7 snap.
        for j in 0..naxis {
            if cdlt != 0.0 {
                let v = cd[[i, j]] / cdlt;
                pc[[i, j]] = if v.abs() < SPLITMAT_THRESHOLD { 0.0 } else { v };
            } else {
                pc[[i, j]] = 0.0;
            }
        }
    }

    (cdelt, pc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_cd_diagonal() {
        let mut cd = Array2::zeros((2, 2));
        cd[[0, 0]] = -0.001;
        cd[[1, 1]] = 0.001;
        let (cdelt, pc) = split_cd(&cd);
        assert!((cdelt[0] - (-0.001)).abs() < 1e-15);
        assert!((cdelt[1] - 0.001).abs() < 1e-15);
        assert!((pc[[0, 0]] - 1.0).abs() < 1e-15);
        assert!((pc[[1, 1]] - 1.0).abs() < 1e-15);
    }

    #[test]
    fn split_cd_rotated() {
        let s = 0.001_f64;
        let cos45 = std::f64::consts::FRAC_1_SQRT_2;
        let mut cd = Array2::zeros((2, 2));
        cd[[0, 0]] = s * cos45;
        cd[[0, 1]] = -s * cos45;
        cd[[1, 0]] = s * cos45;
        cd[[1, 1]] = s * cos45;
        let (cdelt, pc) = split_cd(&cd);
        assert!((cdelt[0].abs() - s).abs() < 1e-12);
        assert!((cdelt[1].abs() - s).abs() < 1e-12);
        let row0_mag =
            crate::math::sqrt(crate::math::powi(pc[[0, 0]], 2) + crate::math::powi(pc[[0, 1]], 2));
        let row1_mag =
            crate::math::sqrt(crate::math::powi(pc[[1, 0]], 2) + crate::math::powi(pc[[1, 1]], 2));
        assert!((row0_mag - 1.0).abs() < 1e-12);
        assert!((row1_mag - 1.0).abs() < 1e-12);
    }
}
