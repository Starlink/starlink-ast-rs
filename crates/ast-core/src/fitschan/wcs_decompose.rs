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
#[derive(Debug)]
pub(crate) struct LinearDecomposition {
    /// Reference pixel position (1-based FITS convention, i.e. CRPIX values).
    pub crpix: Vec<f64>,
    /// CD matrix (combined rotation + scale): cd\[i\]\[j\].
    /// This encodes the linear part: IWC = CD * (pixel - CRPIX).
    pub cd: Array2<f64>,
    /// Reference world coordinate values (CRVAL per axis).
    pub crval: Vec<f64>,
}

/// Decompose a pixel-to-world mapping into linear WCS components.
///
/// Extracts the linear transformation by evaluating the mapping at the
/// reference pixel and at unit offsets along each axis.
///
/// `mapping` is the forward pixel-to-world transform, `naxis` is the
/// dimensionality, and `ref_pixel` supplies the reference pixel position
/// (CRPIX).
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

    Ok(LinearDecomposition {
        crpix: ref_pixel.to_vec(),
        cd,
        crval,
    })
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
}
