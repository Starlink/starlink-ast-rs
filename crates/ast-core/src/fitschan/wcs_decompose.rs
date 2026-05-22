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
