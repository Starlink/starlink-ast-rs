//! Construction of the pixel-to-IWC (Intermediate World Coordinates) mapping.
//!
//! This module builds the linear (and optionally distorted) mapping from
//! pixel coordinates to intermediate world coordinates, as described by
//! FITS-WCS Papers I and IV. The pipeline is:
//!
//! ```text
//! pixel → subtract CRPIX → [SIP distortion] → PC rotation → CDELT scale → IWC
//! ```
//!
//! This follows the C library's `WcsIntWorld` function (fitschan.c ~line 37488).

use ndarray::Array2;

use super::fits_store::FitsStore;
use crate::error::AstResult;
use crate::mappings::{
    CmpMap, Composition, DynMapping, MatrixMap, PolyMap, PolyTerm, UnitMap, WinMap,
};

/// Build the pixel-to-Intermediate World Coordinate (IWC) mapping.
///
/// Composes: CRPIX shift -> [SIP distortion] -> PC matrix -> CDELT scaling.
///
/// The pipeline follows the C library's `WcsIntWorld`:
/// 1. A WinMap subtracts CRPIX from pixel coordinates.
/// 2. If SIP distortion is present, a PolyMap is applied in pixel-offset space.
/// 3. A MatrixMap applies the PC rotation matrix (identity if absent).
/// 4. A diagonal MatrixMap applies CDELT scaling (unity if absent).
///
/// If CD matrix is present instead of PC+CDELT, the CD matrix is used
/// directly as the rotation matrix and CDELT is set to unity. This mirrors
/// the C library's `WcsDecomp` step which splits CD into PC and CDELT.
pub(crate) fn build_pixel_to_iwc(store: &FitsStore) -> AstResult<DynMapping> {
    // Step 1: CRPIX shift.
    // The C WcsShift creates a WinMap mapping pixel → (pixel - CRPIX).
    // Input corners: (0, 0, ...) → Output corners: (-crpix[0], -crpix[1], ...)
    // Input corners: (1, 1, ...) → Output corners: (1-crpix[0], 1-crpix[1], ...)
    let shift_map = build_crpix_shift(store)?;

    // Step 2: SIP distortion (applied in pixel-offset space, before PC/CD).
    let sip_map = if store.has_sip() {
        Some(build_sip_mapping(store)?)
    } else {
        None
    };

    // Steps 3 & 4: PC rotation and CDELT scaling.
    // If CD matrix is present, it replaces both PC and CDELT.
    let (pc_map, cdelt_map) = build_rotation_and_scale(store)?;

    // Compose the pipeline: shift → [sip] → pc → cdelt.
    let mut result: DynMapping = Box::new(shift_map);

    // Add SIP if present.
    if let Some(sip) = sip_map {
        let cmp = CmpMap::new(result, sip, Composition::Series)?;
        result = Box::new(cmp);
    }

    // Add PC rotation.
    let cmp = CmpMap::new(result, pc_map, Composition::Series)?;
    result = Box::new(cmp);

    // Add CDELT scaling.
    let cmp = CmpMap::new(result, cdelt_map, Composition::Series)?;
    result = Box::new(cmp);

    Ok(result)
}

/// Build a WinMap that subtracts CRPIX from pixel coordinates.
///
/// This mirrors the C `WcsShift` function. The WinMap maps:
/// - pixel coordinate at CRPIX → 0
/// - shift[i] = -crpix[i], scale[i] = 1.0
///
/// The C implementation uses from_corners with:
/// - c1_in = [0, 0, ...], c2_in = [1, 1, ...]
/// - c1_out = [-crpix[0], ...], c2_out = [1 - crpix[0], ...]
fn build_crpix_shift(store: &FitsStore) -> AstResult<WinMap> {
    let naxis = store.naxis;
    let shifts: Vec<f64> = store
        .crpix
        .iter()
        .take(naxis)
        .map(|v| -v.unwrap_or(0.0))
        .collect();
    let scales = vec![1.0; naxis];

    WinMap::new(&shifts, &scales)
}

/// Build the PC rotation matrix and CDELT diagonal scaling.
///
/// A `CDj_i` matrix in the header never reaches here: `SpecTrans` has already
/// converted it into an equivalent `PCj_i` matrix plus per-row `CDELTj`, which
/// is why the FITS store the C library builds has no CD member either.
fn build_rotation_and_scale(store: &FitsStore) -> AstResult<(DynMapping, DynMapping)> {
    let pc_map = build_pc_matrix(store)?;
    let cdelt_map = build_cdelt_matrix(store)?;
    Ok((pc_map, cdelt_map))
}

/// Build a MatrixMap from the PC matrix elements.
///
/// Missing diagonal elements default to 1.0, off-diagonal to 0.0.
/// This mirrors the C `WcsPCMatrix` function.
fn build_pc_matrix(store: &FitsStore) -> AstResult<DynMapping> {
    let naxis = store.naxis;
    let mut mat = Array2::zeros((naxis, naxis));

    for i in 0..naxis {
        for j in 0..naxis {
            let default = if i == j { 1.0 } else { 0.0 };
            mat[[i, j]] = store.pc[i][j].unwrap_or(default);
        }
    }

    let mm = MatrixMap::new(mat)?;
    Ok(Box::new(mm))
}

/// Build a diagonal MatrixMap from the CDELT values.
///
/// Missing CDELT values default to 1.0.
/// This mirrors the C `WcsCDeltMatrix` function.
fn build_cdelt_matrix(store: &FitsStore) -> AstResult<DynMapping> {
    let naxis = store.naxis;
    let mut mat = Array2::zeros((naxis, naxis));

    for i in 0..naxis {
        mat[[i, i]] = store.cdelt[i].unwrap_or(1.0);
    }

    let mm = MatrixMap::new(mat)?;
    Ok(Box::new(mm))
}

/// Build a PolyMap representing SIP distortion.
///
/// SIP convention (applied in pixel-offset space after CRPIX subtraction):
/// ```text
/// U = u + sum(A_p_q * u^p * v^q)   (output axis 1)
/// V = v + sum(B_p_q * u^p * v^q)   (output axis 2)
/// ```
///
/// The polynomial includes the identity (linear pass-through) term.
/// For the forward direction, the coefficient for `u^1 * v^0` on output 1
/// is `1 + A_1_0`, and `v^0 * u^0 * v^1` on output 2 is `1 + B_0_1`.
///
/// If additional axes exist beyond the first 2, they pass through unchanged.
/// However, PolyMap only handles 2-in/2-out; we wrap with a PermMap if needed.
/// For simplicity, this implementation assumes SIP only applies to axes 1 and 2
/// (the standard SIP convention).
///
/// This mirrors the C `SIPMapping` function.
fn build_sip_mapping(store: &FitsStore) -> AstResult<DynMapping> {
    let naxis = store.naxis;

    // Build forward polynomial terms.
    let forward_terms = build_sip_forward_terms(store);

    // Build inverse polynomial terms (if available).
    let inverse_terms = build_sip_inverse_terms(store);

    let inv = if inverse_terms.is_empty() {
        None
    } else {
        Some(inverse_terms.as_slice())
    };

    // Create the 2-in, 2-out PolyMap for the SIP axes.
    let mut polymap = PolyMap::new(2, 2, &forward_terms, inv)?;
    // Mirrors fitschan.c:29346-29383: under SipReplace (default true)
    // the inverse transformations supplied within SIP headers are often
    // inaccurate, so replace the missing transformation — or any
    // existing inverse — by fitting a polynomial to samples of the
    // supplied transformation over an area three times the image size.
    // If the fit fails to reach 0.01 pixel accuracy, fall back to the
    // (slower) iterative inverse. With SipReplace off, the PolyMap is
    // used as supplied; an inverse-less PolyMap still inverts because
    // IterInverse defaults on when no inverse coefficients exist
    // (polymap.c:6311).
    if store.sip_replace {
        let lbnd = [
            store.dims[0].map_or(-1000.0, |d| -d),
            store.dims[1].map_or(-1000.0, |d| -d),
        ];
        let ubnd = [
            store.dims[0].map_or(2000.0, |d| 2.0 * d),
            store.dims[1].map_or(2000.0, |d| 2.0 * d),
        ];
        let replace_forward = forward_terms.is_empty();
        match polymap.poly_tran(replace_forward, 0.0001, 0.01, 7, &lbnd, &ubnd)? {
            Some(fitted) => polymap = fitted,
            None => {
                polymap.set_iter_inverse(true);
                polymap.set_niter_inverse(6);
                polymap.set_tol_inverse(1.0e-8);
            }
        }
    }

    if naxis == 2 {
        // Simple case: SIP applies to both axes directly.
        Ok(Box::new(polymap))
    } else {
        // SIP applies only to the first 2 axes; other axes pass through.
        // Compose PolyMap (axes 1,2) in parallel with UnitMap (remaining axes).
        let unit = UnitMap::new(naxis - 2);
        let cmp = CmpMap::new(Box::new(polymap), Box::new(unit), Composition::Parallel)?;
        Ok(Box::new(cmp))
    }
}

/// Build forward SIP polynomial terms from A and B coefficients.
///
/// The forward transformation is:
/// - Output 1 (U): u + sum(A_p_q * u^p * v^q)
/// - Output 2 (V): v + sum(B_p_q * u^p * v^q)
///
/// We include the identity term by adding 1.0 to the coefficient that
/// corresponds to the linear pass-through (p=1, q=0 for output 1;
/// p=0, q=1 for output 2).
fn build_sip_forward_terms(store: &FitsStore) -> Vec<PolyTerm> {
    let mut terms = Vec::new();

    // Output axis 1: U = u + sum(A_p_q * u^p * v^q)
    let a_order = store.sip_a_order.unwrap_or(0);
    let a_size = if store.sip_a.is_empty() {
        0
    } else {
        store.sip_a.len()
    };

    for p in 0..=a_order {
        for q in 0..=a_order {
            if p + q > a_order {
                continue;
            }
            let mut coeff = if p < a_size && q < store.sip_a.get(p).map_or(0, |r| r.len()) {
                store.sip_a[p][q]
            } else {
                0.0
            };

            // Add the identity term: for output 1, p=1, q=0 gets +1.
            if p == 1 && q == 0 {
                coeff += 1.0;
            }

            if coeff != 0.0 {
                terms.push(PolyTerm {
                    coeff,
                    powers: vec![p, q],
                    out_axis: 1,
                });
            }
        }
    }

    // If no A coefficients at all, still need the identity for output 1.
    if a_order == 0 && !terms.iter().any(|t| t.out_axis == 1) {
        terms.push(PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        });
    }

    // Output axis 2: V = v + sum(B_p_q * u^p * v^q)
    let b_order = store.sip_b_order.unwrap_or(0);
    let b_size = if store.sip_b.is_empty() {
        0
    } else {
        store.sip_b.len()
    };

    for p in 0..=b_order {
        for q in 0..=b_order {
            if p + q > b_order {
                continue;
            }
            let mut coeff = if p < b_size && q < store.sip_b.get(p).map_or(0, |r| r.len()) {
                store.sip_b[p][q]
            } else {
                0.0
            };

            // Add the identity term: for output 2, p=0, q=1 gets +1.
            if p == 0 && q == 1 {
                coeff += 1.0;
            }

            if coeff != 0.0 {
                terms.push(PolyTerm {
                    coeff,
                    powers: vec![p, q],
                    out_axis: 2,
                });
            }
        }
    }

    // If no B coefficients at all, still need the identity for output 2.
    if b_order == 0 && !terms.iter().any(|t| t.out_axis == 2) {
        terms.push(PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        });
    }

    terms
}

/// Build inverse SIP polynomial terms from AP and BP coefficients.
///
/// The inverse transformation is:
/// - Output 1 (u): U + sum(AP_p_q * U^p * V^q)
/// - Output 2 (v): V + sum(BP_p_q * U^p * V^q)
///
/// Same structure as forward but with AP/BP coefficients. The identity
/// terms are added the same way (p=1, q=0 for output 1; p=0, q=1 for
/// output 2).
fn build_sip_inverse_terms(store: &FitsStore) -> Vec<PolyTerm> {
    // If no inverse coefficients are defined, return empty.
    if store.sip_ap_order.is_none() && store.sip_bp_order.is_none() {
        return Vec::new();
    }

    let mut terms = Vec::new();

    // Output axis 1: u = U + sum(AP_p_q * U^p * V^q)
    let ap_order = store.sip_ap_order.unwrap_or(0);
    let ap_size = if store.sip_ap.is_empty() {
        0
    } else {
        store.sip_ap.len()
    };

    for p in 0..=ap_order {
        for q in 0..=ap_order {
            if p + q > ap_order {
                continue;
            }
            let mut coeff = if p < ap_size && q < store.sip_ap.get(p).map_or(0, |r| r.len()) {
                store.sip_ap[p][q]
            } else {
                0.0
            };

            // Add identity: for output 1, p=1, q=0 gets +1.
            if p == 1 && q == 0 {
                coeff += 1.0;
            }

            if coeff != 0.0 {
                terms.push(PolyTerm {
                    coeff,
                    powers: vec![p, q],
                    out_axis: 1,
                });
            }
        }
    }

    // If no AP coefficients, still need the identity for output 1.
    if ap_order == 0 && !terms.iter().any(|t| t.out_axis == 1) {
        terms.push(PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        });
    }

    // Output axis 2: v = V + sum(BP_p_q * U^p * V^q)
    let bp_order = store.sip_bp_order.unwrap_or(0);
    let bp_size = if store.sip_bp.is_empty() {
        0
    } else {
        store.sip_bp.len()
    };

    for p in 0..=bp_order {
        for q in 0..=bp_order {
            if p + q > bp_order {
                continue;
            }
            let mut coeff = if p < bp_size && q < store.sip_bp.get(p).map_or(0, |r| r.len()) {
                store.sip_bp[p][q]
            } else {
                0.0
            };

            // Add identity: for output 2, p=0, q=1 gets +1.
            if p == 0 && q == 1 {
                coeff += 1.0;
            }

            if coeff != 0.0 {
                terms.push(PolyTerm {
                    coeff,
                    powers: vec![p, q],
                    out_axis: 2,
                });
            }
        }
    }

    // If no BP coefficients, still need the identity for output 2.
    if bp_order == 0 && !terms.iter().any(|t| t.out_axis == 2) {
        terms.push(PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        });
    }

    terms
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use crate::mappings::Mapping;

    /// Helper to create a PointSet from a single 2D point.
    fn point2d(x: f64, y: f64) -> PointSet {
        PointSet::from_columns(&[&[x], &[y]]).unwrap()
    }

    /// Build a FitsStore for a simple CDELT-only case.
    fn simple_cdelt_store() -> FitsStore {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(128.0);
        store.crpix[1] = Some(128.0);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store
    }

    /// The store a rotated-and-skewed Spitzer-like `CDj_i` header produces once
    /// `SpecTrans` has split it into a unit-row `PCj_i` matrix and per-row
    /// `CDELTj`. Derived here by the same row-normalisation `SpecTrans` applies,
    /// so `PC * CDELT` reproduces `CD` to the last bit and the transform
    /// assertions below are the CD matrix's own.
    fn cd_matrix_store() -> FitsStore {
        const CD: [[f64; 2]; 2] = [[0.000250, 0.000230], [0.000230, -0.000250]];

        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(128.0);
        store.crpix[1] = Some(128.0);
        for (j, row) in CD.iter().enumerate() {
            // Row length, signed by the diagonal element.
            let rowsum2: f64 = row.iter().map(|v| v * v).sum();
            let diag = if row[j] >= 0.0 { 1.0 } else { -1.0 };
            let cdeltj = rowsum2.sqrt() * diag;
            store.cdelt[j] = Some(cdeltj);
            for (i, &cd) in row.iter().enumerate() {
                store.pc[j][i] = Some(cd / cdeltj);
            }
        }
        store
    }

    #[test]
    fn crpix_shift_maps_reference_to_zero() {
        let store = simple_cdelt_store();
        let shift = build_crpix_shift(&store).unwrap();

        // Transform the reference pixel: should map to (0, 0).
        let pts = point2d(128.0, 128.0);
        let out = shift.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - 0.0).abs() < 1e-10);
        assert!((result[1] - 0.0).abs() < 1e-10);
    }

    #[test]
    fn crpix_shift_arbitrary_point() {
        let store = simple_cdelt_store();
        let shift = build_crpix_shift(&store).unwrap();

        // Transform pixel (129, 130): should map to (1, 2).
        let pts = point2d(129.0, 130.0);
        let out = shift.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - 1.0).abs() < 1e-10);
        assert!((result[1] - 2.0).abs() < 1e-10);
    }

    #[test]
    fn cdelt_only_produces_diagonal() {
        let store = simple_cdelt_store();
        let mapping = build_pixel_to_iwc(&store).unwrap();

        // Reference pixel (128, 128) should map to IWC (0, 0).
        let pts = point2d(128.0, 128.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!(result[0].abs() < 1e-10);
        assert!(result[1].abs() < 1e-10);
    }

    #[test]
    fn cdelt_offset_pixel() {
        let store = simple_cdelt_store();
        let mapping = build_pixel_to_iwc(&store).unwrap();

        // Pixel (129, 129): offset (1, 1) from CRPIX, scaled by CDELT.
        // Expected IWC: (1 * -0.001, 1 * 0.001) = (-0.001, 0.001).
        let pts = point2d(129.0, 129.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - (-0.001)).abs() < 1e-12);
        assert!((result[1] - 0.001).abs() < 1e-12);
    }

    #[test]
    fn cd_matrix_reference_to_zero() {
        let store = cd_matrix_store();
        let mapping = build_pixel_to_iwc(&store).unwrap();

        // Reference pixel should map to IWC (0, 0).
        let pts = point2d(128.0, 128.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!(result[0].abs() < 1e-10);
        assert!(result[1].abs() < 1e-10);
    }

    #[test]
    fn cd_matrix_offset_pixel() {
        let store = cd_matrix_store();
        let mapping = build_pixel_to_iwc(&store).unwrap();

        // Pixel (129, 128): offset (1, 0), so IWC = CD * (1, 0) = (CD[0][0], CD[1][0]).
        let pts = point2d(129.0, 128.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - 0.000250).abs() < 1e-12);
        assert!((result[1] - 0.000230).abs() < 1e-12);
    }

    #[test]
    fn nin_nout_correct() {
        let store = simple_cdelt_store();
        let mapping = build_pixel_to_iwc(&store).unwrap();
        assert_eq!(mapping.nin(), 2);
        assert_eq!(mapping.nout(), 2);
    }

    #[test]
    fn pc_matrix_with_rotation() {
        // PC matrix with 45-degree rotation + CDELT.
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(1.0);
        store.crpix[1] = Some(1.0);
        store.cdelt[0] = Some(1.0);
        store.cdelt[1] = Some(1.0);
        let cos45 = std::f64::consts::FRAC_1_SQRT_2;
        let sin45 = cos45;
        store.pc[0][0] = Some(cos45);
        store.pc[0][1] = Some(-sin45);
        store.pc[1][0] = Some(sin45);
        store.pc[1][1] = Some(cos45);

        let mapping = build_pixel_to_iwc(&store).unwrap();

        // Pixel (2, 1): offset (1, 0) from CRPIX.
        // After PC rotation: (cos45, sin45) ≈ (0.707, 0.707).
        // After CDELT=1: unchanged.
        let pts = point2d(2.0, 1.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - cos45).abs() < 1e-10);
        assert!((result[1] - sin45).abs() < 1e-10);
    }

    #[test]
    fn identity_with_defaults() {
        // No CRPIX, no CDELT, no PC, no CD: should be identity.
        let store = FitsStore::new(2);
        let mapping = build_pixel_to_iwc(&store).unwrap();

        let pts = point2d(5.0, 7.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        // CRPIX defaults to 0, so shift is 0. CDELT defaults to 1. PC defaults to identity.
        assert!((result[0] - 5.0).abs() < 1e-10);
        assert!((result[1] - 7.0).abs() < 1e-10);
    }

    #[test]
    fn sip_distortion_forward() {
        // Minimal SIP: A_2_0 = 1e-5 (small quadratic distortion on axis 1).
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(0.0);
        store.crpix[1] = Some(0.0);
        store.cdelt[0] = Some(1.0);
        store.cdelt[1] = Some(1.0);
        store.sip_a_order = Some(2);
        store.sip_a = vec![vec![0.0; 3]; 3];
        store.sip_a[2][0] = 1e-5; // A_2_0
        store.sip_b_order = Some(0);
        store.sip_b = vec![vec![0.0; 1]; 1];

        let mapping = build_pixel_to_iwc(&store).unwrap();

        // At pixel (100, 0) with CRPIX = 0:
        // offset = (100, 0)
        // SIP: U = u + A_2_0 * u^2 = 100 + 1e-5 * 10000 = 100.1
        //      V = v = 0
        // CDELT = 1, PC = identity, so IWC = (100.1, 0).
        let pts = point2d(100.0, 0.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - 100.1).abs() < 1e-8);
        assert!(result[1].abs() < 1e-10);
    }
}
