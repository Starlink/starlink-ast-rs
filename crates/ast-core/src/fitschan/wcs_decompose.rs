//! Decompose a FrameSet mapping into FITS-WCS linear components.
//!
//! The decomposition extracts: CRPIX (reference pixel shift), a rotation/scale
//! matrix (PC or CD), and identifies the non-linear portion (projection) if
//! present. This mirrors the C library's `MakeIntWorld` function (fitschan.c
//! ~line 21689).

use ndarray::Array2;

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::mappings::Mapping;

/// Result of decomposing a pixel-to-world mapping into linear WCS components.
///
/// CRPIX is not stored here — callers pass `ref_pixel` to
/// [`decompose_linear`] and resolve the final CRPIX algebraically from
/// `cd` and `crval` via `solve_crpix_from_decomp` (mirrors the C
/// library's `MakeIntWorld` / `palDmat` step). Keeping CRPIX out of
/// the struct avoids a stale copy of `ref_pixel` that nothing reads.
#[derive(Debug)]
pub(crate) struct LinearDecomposition {
    /// CD matrix (combined rotation + scale): cd\[i\]\[j\].
    /// This encodes the linear part: IWC = CD * (pixel - CRPIX).
    pub cd: Array2<f64>,
    /// Reference world coordinate values (CRVAL per axis).
    pub crval: Vec<f64>,
}

/// Decompose a pixel-to-world mapping into linear WCS components.
///
/// Extracts the linear transformation by evaluating the mapping at
/// `ref_pixel` and at unit offsets along each axis. The returned
/// `crval` is the world coordinate at `ref_pixel`; `cd` is the
/// numerical Jacobian. CRPIX is not returned — callers resolve it
/// algebraically from `cd` and `crval` (see `solve_crpix_from_decomp`
/// in `wcs_write.rs`).
pub(crate) fn decompose_linear(
    mapping: &dyn Mapping,
    naxis: usize,
    ref_pixel: &[f64],
) -> AstResult<LinearDecomposition> {
    let ref_pts = PointSet::from_rows(std::iter::once(ref_pixel.to_vec()));
    let ref_out = mapping.transform(&ref_pts, true)?;
    let crval: Vec<f64> = ref_out.point(0).to_vec();

    let mut cd = Array2::zeros((naxis, naxis));
    for j in 0..naxis {
        let mut offset_pixel = ref_pixel.to_vec();
        offset_pixel[j] += 1.0;

        let offset_pts = PointSet::from_rows(std::iter::once(offset_pixel));
        let offset_out = mapping.transform(&offset_pts, true)?;
        let offset_world = offset_out.point(0);

        for i in 0..naxis {
            cd[[i, j]] = offset_world[i] - crval[i];
        }
    }

    Ok(LinearDecomposition { cd, crval })
}

// ---------------------------------------------------------------------------
// FindBasisVectors / FitLine port (fitschan.c:10299-15075)
// ---------------------------------------------------------------------------

/// Result of [`find_basis_vectors`]: the chosen root pixel, the IWC
/// coordinates at it, and a per-output-axis tolerance derived from
/// the local Jacobian (used by `FitLine` to decide whether the fit is
/// usable). All vectors are length `nout` for `w0` / `tol` and length
/// `nin` for `g0`.
///
/// Currently exported with `#[allow(dead_code)]` while the
/// surrounding `MakeIntWorld` port is in progress — the helpers are
/// fully functional, but wiring them into `store_from_frameset_to`
/// requires the IWC-mapping reconstruction step that C performs in
/// `OtherAxes` / `SpectralAxes` / `CelestialAxes` before calling
/// `MakeIntWorld`. Pulling them in early without that step
/// regresses fixtures whose CDELT/CD values come from finite-
/// difference unit-conversion noise.
#[allow(dead_code)]
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
/// transformed root and unit-step neighbours are all finite. Mirrors
/// `MakeBasisVectors` (fitschan.c:10299-10422). Returns `Some((w0,
/// step_iwc))` where `step_iwc[i]` is the IWC vector at `g0 + e_i`,
/// or `None` if any output is non-finite.
#[allow(dead_code)]
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
        let v = view[[0, j]];
        if !v.is_finite() {
            return None;
        }
        w0[j] = v;
    }
    let mut step = vec![vec![0.0_f64; nout]; nin];
    for i in 0..nin {
        for j in 0..nout {
            let v = view[[i + 1, j]];
            if !v.is_finite() {
                return None;
            }
            step[i][j] = v;
        }
    }
    Some((w0, step))
}

/// Pick a root pixel `g0` for which the mapping produces all-finite
/// IWC coordinates at `g0` and at each unit-step neighbour. Mirrors
/// `FindBasisVectors` (fitschan.c:10424-10569): try grid centre
/// `0.5*(1+dim)` first, then `(1, 1, …)`, then progressively further
/// out along the diagonal and individual axes. Returns the chosen
/// `g0` plus the tolerance derived from the local IWC step sizes.
#[allow(dead_code)]
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

    // Attempt 1: grid centre when all dims are known.
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
        // Diagonal across the centre, scaled by aspect ratio when
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
    // step magnitude × `fitstol`. fitschan.c:21953-21965.
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
            t = f64::MIN_POSITIVE.sqrt();
        }
        tol[j] = t;
    }

    Some(BasisVectors { g0, w0, tol })
}

/// RMS-residual gate. Mirrors `FitOK` (fitschan.c:10972).
#[allow(dead_code)]
fn fit_ok(actual: &[f64], estimated: &[f64], tol: f64) -> bool {
    let mut s1 = 0.0_f64;
    let mut s2 = 0.0_f64;
    for (a, e) in actual.iter().zip(estimated.iter()) {
        if !a.is_finite() {
            continue;
        }
        let d = a - e;
        s1 += d * d;
        s2 += 1.0;
    }
    if s2 == 0.0 {
        return false;
    }
    (s1 / s2).sqrt() <= tol
}

/// Least-squares straight-line fit along input axis `axis` of
/// `mapping`. Returns the per-pixel slope as a vector of `nout`
/// elements (the column of the CD matrix), or `None` if the fit
/// is not linear (RMS residual exceeds the supplied tolerance per
/// output axis). Mirrors `FitLine` (fitschan.c:14835-15075).
#[allow(dead_code)]
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
    if !w0.iter().all(|v| v.is_finite()) {
        return None;
    }
    let dim_v = dim?;
    if dim_v == 0.0 {
        return None;
    }
    let gap = dim_v / NP_USIZE as f64;

    // Sample NP_USIZE points along input axis `axis`, centred at g0,
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
            let v = view[[idx, j]];
            if !v.is_finite() {
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
        let actual: Vec<f64> = (0..NP_USIZE).map(|idx| view[[idx, j]]).collect();
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
/// `MakeIntWorld` (fitschan.c:21978-21995).
#[allow(dead_code)]
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
/// CD = diag(CDELT) * PC, where each row of PC is normalised to unit length.
/// This mirrors the C `SplitMat` function (fitschan.c ~line 33141).
pub(crate) fn split_cd(cd: &Array2<f64>) -> (Vec<f64>, Array2<f64>) {
    let naxis = cd.nrows();
    let mut cdelt = vec![0.0; naxis];
    let mut pc = Array2::zeros((naxis, naxis));

    for i in 0..naxis {
        let mut mag_sq = 0.0;
        for j in 0..naxis {
            mag_sq += cd[[i, j]] * cd[[i, j]];
        }
        let mag = mag_sq.sqrt();

        if mag == 0.0 {
            cdelt[i] = 1.0;
        } else {
            cdelt[i] = mag;
            for j in 0..naxis {
                pc[[i, j]] = cd[[i, j]] / mag;
            }
            if pc[[i, i]] < 0.0 {
                cdelt[i] = -cdelt[i];
                for j in 0..naxis {
                    pc[[i, j]] = -pc[[i, j]];
                }
            }
        }
    }

    (cdelt, pc)
}

/// Apply the C library's `SplitMat` 1e-7 PC-element threshold: split
/// CD into (CDELT, PC), zero any PC element whose absolute value is
/// below 1e-7, then recompose CD = CDELT × PC. Returns the cleaned CD.
///
/// Mirrors `fitschan.c::SplitMat` line 33238:
/// `if( fabs( *a ) < 1.E-7 ) *a = 0.0;`
///
/// Rationale (from C comment, line 33232): "Set to zero any PC values
/// which are less than 1.0E-7 (such values may be produced by rounding
/// errors)." The threshold is scale-relative because PC has unit row
/// magnitude — a PC value of 1e-7 corresponds to a CD value of
/// 1e-7 × CDELT, which is the round-trip noise floor.
pub(crate) fn clean_cd_residue(cd: &Array2<f64>) -> Array2<f64> {
    let naxis = cd.nrows();
    let (cdelt, mut pc) = split_cd(cd);

    // Apply the 1e-7 PC snap.
    for i in 0..naxis {
        for j in 0..naxis {
            if pc[[i, j]].abs() < 1.0e-7 {
                pc[[i, j]] = 0.0;
            }
        }
    }

    // Recompose CD = diag(CDELT) * PC.
    let mut cleaned = Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cleaned[[i, j]] = cdelt[i] * pc[[i, j]];
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{UnitMap, WinMap};

    #[test]
    fn decompose_identity() {
        let unit = UnitMap::new(2);
        let result = decompose_linear(&unit, 2, &[1.0, 1.0]).unwrap();
        assert!((result.crval[0] - 1.0).abs() < 1e-10);
        assert!((result.crval[1] - 1.0).abs() < 1e-10);
        assert!((result.cd[[0, 0]] - 1.0).abs() < 1e-10);
        assert!((result.cd[[1, 1]] - 1.0).abs() < 1e-10);
        assert!(result.cd[[0, 1]].abs() < 1e-10);
        assert!(result.cd[[1, 0]].abs() < 1e-10);
    }

    #[test]
    fn decompose_shift_and_scale() {
        let wm = WinMap::new(&[-128.0, -128.0], &[0.001, 0.001]).unwrap();
        let result = decompose_linear(&wm, 2, &[128.0, 128.0]).unwrap();
        // WinMap forward: output = shift + scale * input
        // At ref_pixel [128, 128]: crval = -128 + 0.001*128 = -127.872
        // CD[i][j] = (output at offset) - crval = scale[j] for i==j
        assert!((result.cd[[0, 0]] - 0.001).abs() < 1e-12);
        assert!((result.cd[[1, 1]] - 0.001).abs() < 1e-12);
        assert!(result.cd[[0, 1]].abs() < 1e-12);
        assert!(result.cd[[1, 0]].abs() < 1e-12);
    }

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
        let row0_mag = (pc[[0, 0]].powi(2) + pc[[0, 1]].powi(2)).sqrt();
        let row1_mag = (pc[[1, 0]].powi(2) + pc[[1, 1]].powi(2)).sqrt();
        assert!((row0_mag - 1.0).abs() < 1e-12);
        assert!((row1_mag - 1.0).abs() < 1e-12);
    }

    #[test]
    fn split_cd_snap_zeroes_residue_pc_elements() {
        // CD with rotation cos(30°)/sin(30°) at scale 1e-3 plus a
        // 1e-15 off-diagonal residue. The residue should snap to 0
        // after the PC threshold is applied.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        let scale = 1e-3;
        let mut cd = ndarray::Array2::zeros((2, 2));
        cd[[0, 0]] = scale * cos30;
        cd[[0, 1]] = -scale * sin30;
        cd[[1, 0]] = scale * sin30;
        cd[[1, 1]] = scale * cos30 + 1e-15; // residue

        let cleaned = clean_cd_residue(&cd);

        // Diagonal cos(30°)*scale stays.
        assert!((cleaned[[0, 0]] - scale * cos30).abs() < 1e-12);
        // The 1e-15 residue: pre-split, it was scale * cos30 + 1e-15.
        // Post-split: (cos30 + 1e-15/scale) magnitude ≈ 1.
        // The 1e-15/scale term in PC is 1e-12 — well below 1e-7 ?
        // Actually 1e-15 / 1e-3 = 1e-12. That's < 1e-7, so the
        // perturbation snaps out of PC. Cleaned CD becomes scale * cos30.
        assert!((cleaned[[1, 1]] - scale * cos30).abs() < 1e-13);
    }

    #[test]
    fn split_cd_snap_preserves_real_off_diagonals() {
        // Real rotation off-diagonals (sin(30°) * 1e-3 ≈ 5e-4) must NOT
        // be zeroed, because PC[0][1] = -sin(30°) ≈ -0.5, well above 1e-7.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        let scale = 1e-3;
        let mut cd = ndarray::Array2::zeros((2, 2));
        cd[[0, 0]] = scale * cos30;
        cd[[0, 1]] = -scale * sin30;
        cd[[1, 0]] = scale * sin30;
        cd[[1, 1]] = scale * cos30;

        let cleaned = clean_cd_residue(&cd);

        // All four entries should round-trip exactly.
        assert!((cleaned[[0, 0]] - scale * cos30).abs() < 1e-15);
        assert!((cleaned[[0, 1]] - (-scale * sin30)).abs() < 1e-15);
        assert!((cleaned[[1, 0]] - scale * sin30).abs() < 1e-15);
        assert!((cleaned[[1, 1]] - scale * cos30).abs() < 1e-15);
    }
}
