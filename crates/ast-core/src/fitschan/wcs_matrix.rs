//! Construction of the pixel-to-IWC (Intermediate World Coordinates) mapping.
//!
//! This module builds the linear (and optionally distorted) mapping from
//! pixel coordinates to intermediate world coordinates, as described by
//! FITS-WCS Papers I and IV. The pipeline is:
//!
//! ```text
//! pixel → [mapd1] → subtract CRPIX → [mapd2] → PC rotation → [mapd3]
//!       → CDELT scale → [mapd4] → IWC
//! ```
//!
//! The four optional `mapd` stages are the FITS-WCS Paper IV distortion
//! corrections `DistortMaps` extracts from the CTYPE distortion codes.
//!
//! This follows the C library's `WcsIntWorld` function (fitschan.c:37624).

use super::warning::{Warning, Warnings};
use ndarray::Array2;

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::constants::BAD_VALUE;
use crate::error::AstResult;
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, PolyMap, PolyTerm, UnitMap, WcsMap, WinMap,
};
use crate::wcslib::WcsProjection;

/// Build the pixel-to-Intermediate World Coordinate (IWC) mapping.
///
/// Port of C `WcsIntWorld` (fitschan.c:37624). The chain is
/// `[mapd1] → WcsShift → [mapd2] → WcsPCMatrix → [mapd3] → WcsCDeltMatrix
/// → [mapd4]`, where the `mapd` stages come from [`distort_maps`].
///
/// `store` is mutated: `DistortMaps` strips any distortion code from the
/// CTYPE values so that later steps — `WcsCelestial` in particular — see a
/// plain FITS-WCS projection code. `cards` is the FitsChan's own card list,
/// which is where the IRAF `WAT` keywords live. Any warning text is appended
/// to `warnings`.
pub(crate) fn build_pixel_to_iwc(
    store: &mut FitsStore,
    cards: &CardList,
    warnings: &mut Warnings,
) -> AstResult<DynMapping> {
    let naxes = store.naxis;

    // Distortion corrections for each stage of the chain.
    let dist = distort_maps(store, cards, naxes, warnings)?;

    // Step 1: CRPIX shift.
    // The C WcsShift creates a WinMap mapping pixel → (pixel - CRPIX).
    // Input corners: (0, 0, ...) → Output corners: (-crpix[0], -crpix[1], ...)
    // Input corners: (1, 1, ...) → Output corners: (1-crpix[0], 1-crpix[1], ...)
    let shift_map = build_crpix_shift(store)?;

    // Steps 3 & 4: PC rotation and CDELT scaling.
    // If CD matrix is present, it replaces both PC and CDELT.
    let (pc_map, cdelt_map) = build_rotation_and_scale(store)?;

    // A PC/CD matrix with a wholly zero row or column has no inverse, which
    // makes the whole pixel→IWC chain uninvertible (fitschan.c:37738-37743).
    if !pc_map.has_inverse() {
        warnings.push(Warning::new("badmat", BADMAT_WARNING));
    }

    let mut result: Option<DynMapping> = dist.map1;
    result = Some(append(result, Box::new(shift_map))?);
    if let Some(m) = dist.map2 {
        result = Some(append(result, m)?);
    }
    result = Some(append(result, pc_map)?);
    if let Some(m) = dist.map3 {
        result = Some(append(result, m)?);
    }
    result = Some(append(result, cdelt_map)?);
    if let Some(m) = dist.map4 {
        result = Some(append(result, m)?);
    }

    // `result` is set by the WcsShift append above.
    Ok(result.expect("pixel→IWC chain always contains the CRPIX shift"))
}

/// The `badmat` warning text (fitschan.c:37739-37742). `badmat` is one of the
/// conditions enabled by the default `Warnings` value (fitschan.c:43389), so
/// it is always emitted.
const BADMAT_WARNING: &str = "The pixel rotation matrix in the original FITS header (specified by CD or PC keywords) \
     could not be inverted. This may be because the matrix contains rows or columns which \
     are entirely zero.";

/// Append `next` in series after `acc`, or return it unchanged when `acc` is
/// empty. Mirrors C's repeated
/// `if( ret ) ret = astCmpMap( ret, map1, 1, "" ); else ret = map1;`.
fn append(acc: Option<DynMapping>, next: DynMapping) -> AstResult<DynMapping> {
    match acc {
        Some(prev) => Ok(Box::new(CmpMap::new(prev, next, Composition::Series)?)),
        None => Ok(next),
    }
}

/// The four distortion corrections a FITS-WCS Paper IV distortion code
/// contributes to the pixel→IWC chain, in the positions C `WcsIntWorld`
/// splices them into (fitschan.c:37693-37766).
#[derive(Default)]
struct DistortionMaps {
    /// Applied to raw pixel coordinates, before the CRPIX translation.
    map1: Option<DynMapping>,
    /// Applied to translated pixel coordinates, before the PC matrix.
    map2: Option<DynMapping>,
    /// Applied to unscaled IWC coordinates, before the CDELT matrix.
    map3: Option<DynMapping>,
    /// Applied to scaled IWC coordinates, after the CDELT matrix.
    map4: Option<DynMapping>,
}

/// Port of C `DistortMaps` (fitschan.c:8759).
///
/// Inspects the CTYPE values for a Paper IV distortion code and returns the
/// Mappings that implement it. Every distortion code found — recognised or
/// not — is stripped from the CTYPE value in `store`, so later steps never
/// have to allow for one being present.
///
/// A `-SIP` code beyond the first two axes, more than two `-ZPX` axes, and
/// any other distortion code each raise a `distortion` warning.
fn distort_maps(
    store: &mut FitsStore,
    cards: &CardList,
    naxes: usize,
    warnings: &mut Warnings,
) -> AstResult<DistortionMaps> {
    let mut ret = DistortionMaps::default();

    // "-SIP": Spitzer distortion, limited to axes 1 and 2.
    //
    // C strips the "-SIP" suffix from the first two CTYPE values here; in this
    // reader `wcs_keywords::extract_sip` does that during keyword extraction,
    // which is also where the coefficients are collected, so by this point the
    // store records the distortion through `has_sip` rather than the CTYPE.
    if store.has_sip() {
        ret.map2 = Some(build_sip_mapping(store)?);
    }

    // "-SIP" on any axis beyond the first two is not supported; C warns once
    // and removes the code.
    let mut warned = false;
    for i in 2..naxes {
        if let Some(ct) = store.ctype.get_mut(i).and_then(|c| c.as_mut()) {
            let trimmed = ct.trim_end();
            if trimmed.len() >= 4 && trimmed.ends_with("-SIP") {
                if !warned {
                    warned = true;
                    warnings.push(Warning::new(
                        "distortion",
                        format!(
                            "The \"-SIP\" distortion code can only be used on axes 1 and 2, \
                             but was found in keyword CTYPE{} (='{ct}'). The distortion will \
                             be ignored.",
                            i + 1
                        ),
                    ));
                }
                let nc = trimmed.len() - 4;
                ct.truncate(nc);
            }
        }
    }

    // "-ZPX": IRAF distortion. `SpecTrans` has already rewritten the CTYPE
    // values from "-ZPX" to "-ZPN-ZPX", so the basic ZPN projection is read in
    // the normal way and only the distortion code is handled here.
    let mut found_axes: Vec<usize> = Vec::new();
    for i in 0..naxes {
        let is_zpx = store
            .ctype
            .get(i)
            .and_then(|c| c.as_deref())
            .and_then(split_4_3_3)
            .is_some_and(|(_, _, dist)| dist == "ZPX");
        if is_zpx {
            found_axes.push(i);
            if let Some(ct) = store.ctype[i].as_mut() {
                truncate_at(ct, 8);
            }
        }
    }

    // More than two ZPX axes is meaningless; C warns and falls back to the
    // plain ZPN projection, which the truncated CTYPE values already select.
    if found_axes.len() > 2 {
        warnings.push(Warning::new(
            "distortion",
            "More than two axes were found with the \"-ZPX\" projection code. A ZPN \
             projection will be used instead.",
        ));
    } else if found_axes.len() == 2 {
        let zpxaxes = [found_axes[0], found_axes[1]];
        if let Some(m) = zpx_mapping(cards, naxes, zpxaxes, warnings)? {
            ret.map4 = Some(match ret.map4.take() {
                Some(prev) => Box::new(CmpMap::new(prev, m, Composition::Series)?),
                None => m,
            });
        }
    }

    // There are currently no other supported distortion codes. Strip any
    // remaining one, so later steps see a plain projection code, warning
    // once.
    let mut warned = false;
    for i in 0..naxes {
        let Some(ct) = store.ctype.get_mut(i).and_then(|c| c.as_mut()) else {
            continue;
        };
        if let Some((_, _, dist)) = split_4_3_3(ct) {
            if !warned {
                warned = true;
                warnings.push(Warning::new(
                    "distortion",
                    format!(
                        "The header contains CTYPE values (e.g. CTYPE{} = '{ct}') which \
                         include a distortion code \"-{dist}\". AST currently ignores this \
                         distortion. The code has been removed from the CTYPE values.",
                        i + 1
                    ),
                ));
            }
            truncate_at(ct, 8);
        }
    }

    Ok(ret)
}

/// Split a CTYPE value of the FITS-WCS Paper IV "4-3-3" form into its axis
/// type, projection code and distortion code: C's
/// `3 == astSscanf( ctype, "%4s-%3s-%3s", type, code, dist )`
/// (fitschan.c:8938). Each `%s` skips white space and then reads one to its
/// width of other characters, and each `-` must follow directly.
fn split_4_3_3(ctype: &str) -> Option<(&str, &str, &str)> {
    // A `%Ns` conversion starting at byte `at`: the field read and the byte
    // after it.
    let field = |at: usize, width: usize| -> Option<(&str, usize)> {
        let b = ctype.as_bytes();
        let start = at
            + b[at..]
                .iter()
                .take_while(|c| c.is_ascii_whitespace())
                .count();
        let len = b[start..]
            .iter()
            .take(width)
            .take_while(|c| !c.is_ascii_whitespace())
            .count();
        (len > 0).then(|| (&ctype[start..start + len], start + len))
    };
    let dash = |at: usize| (ctype.as_bytes().get(at) == Some(&b'-')).then_some(at + 1);
    let (axis, at) = field(0, 4)?;
    let (code, at) = field(dash(at)?, 3)?;
    let (dist, _) = field(dash(at)?, 3)?;
    Some((axis, code, dist))
}

/// C's `ctype[ n ] = 0`: end the value at byte `n`, if it is that long.
fn truncate_at(ct: &mut String, n: usize) {
    if ct.len() > n {
        ct.truncate(n);
    }
}

/// Port of C `ZPXMapping` (fitschan.c:41597).
///
/// Builds the Mapping implementing the `lngcor` / `latcor` corrections the
/// IRAF `WAT` keywords describe for the two `-ZPX` axes (`zpxaxes`, 0-based).
/// The basic ZPN projection is read in the normal way; this covers only the
/// polynomial correction on top of it.
///
/// Returns `None` when the header uses IRAF features AST cannot express,
/// having raised the `zpx` warning.
fn zpx_mapping(
    cards: &CardList,
    naxes: usize,
    zpxaxes: [usize; 2],
    warnings: &mut Warnings,
) -> AstResult<Option<DynMapping>> {
    let mut coeffs: [Vec<(usize, f64)>; 2] = [Vec::new(), Vec::new()];
    let mut ok = true;

    for i in 0..2 {
        // Concatenate all the IRAF "WAT" keywords for this axis, then read the
        // polynomial coefficients out as a list of TPN PVi_m values.
        let watstr = super::spec_trans::concat_wat(cards, zpxaxes[i] + 1);
        let (c, axis_ok) = super::spec_trans::wat_coeffs(&watstr, i);
        coeffs[i] = c;
        ok = axis_ok;
        if !ok {
            break;
        }
    }

    if !ok {
        warnings.push(Warning::new("zpx", ZPX_WARNING));
        return Ok(None);
    }

    // With no correction surface on either axis there is no distortion to
    // apply (fitschan.c:41720).
    if coeffs[0].is_empty() && coeffs[1].is_empty() {
        return Ok(Some(Box::new(UnitMap::new(naxes))));
    }

    // Store the coefficients in an inverted TPN WcsMap. It corrects the ZPN
    // WcsMap created later, so clear FITSProj to keep the FitsChan write path
    // from mistaking it for the header's projection, and clear TPNTan so only
    // the polynomial part of the TPN projection is applied.
    let mut wcs = WcsMap::new(naxes, WcsProjection::Tpn, zpxaxes[0] + 1, zpxaxes[1] + 1)?;
    wcs.invert();
    wcs.set_fits_proj(false);
    wcs.set_tpn_tan(false);
    for (i, axis_coeffs) in coeffs.iter().enumerate() {
        for &(m, value) in axis_coeffs {
            wcs.set_pv(zpxaxes[i] + 1, m, value)?;
        }
    }
    Ok(Some(Box::new(wcs)))
}

/// The `zpx` warning text (fitschan.c:41723-41726). `zpx` is one of the
/// conditions enabled by the default `Warnings` value (fitschan.c:43389), so
/// it is always emitted.
const ZPX_WARNING: &str = "This FITS header includes, or was derived from, a ZPX projection which requires \
     unsupported IRAF-specific corrections. The WCS information may therefore be incorrect.";

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
/// is `1 + A_1_0`, and `u^0 * v^1` on output 2 is `1 + B_0_1`.
///
/// Axes beyond the first two pass through unchanged, added in parallel with a
/// UnitMap as C `AddUnitMaps` does.
///
/// Port of C `SIPMapping` (fitschan.c:29245). When neither direction has any
/// coefficient in the store the whole distortion is a UnitMap over every axis,
/// which is the answer for a header that names `-SIP` in its CTYPE values but
/// supplies no `A_i_j` / `B_i_j` cards.
fn build_sip_mapping(store: &FitsStore) -> AstResult<DynMapping> {
    let naxis = store.naxis;

    // Build forward polynomial terms.
    let forward_terms = build_sip_forward_terms(store);

    // Build inverse polynomial terms (if available).
    let inverse_terms = build_sip_inverse_terms(store);

    if forward_terms.is_empty() && inverse_terms.is_empty() {
        return Ok(Box::new(UnitMap::new(naxis)));
    }

    let inv = if inverse_terms.is_empty() {
        None
    } else {
        Some(inverse_terms.as_slice())
    };

    // Create the 2-in, 2-out PolyMap for the SIP axes.
    let mut polymap = PolyMap::new(2, 2, &forward_terms, inv)?;
    // Mirrors fitschan.c:29415-29452: under SipReplace (default true)
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
                polymap.set_iter_inverse(true)?;
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

/// Largest first index stored in a SIP coefficient table.
///
/// Port of C `GetMaxI` (fitschan.c:13450): the largest intermediate-axis index
/// for which the table holds a row, or -1 when the table is empty. C skips
/// trailing NULL row pointers; the Rust equivalent is a trailing empty row.
fn sip_max_i(table: &[Vec<f64>]) -> i32 {
    let mut ret = -1;
    for (i, row) in table.iter().enumerate() {
        if !row.is_empty() {
            ret = i as i32;
        }
    }
    ret
}

/// Largest second index stored in a SIP coefficient table.
///
/// Port of C `GetMaxJM` (fitschan.c:13237): the maximum over all rows of the
/// largest index holding a value other than `AST__BAD`, or -1 when the table
/// holds no values at all.
fn sip_max_jm(table: &[Vec<f64>]) -> i32 {
    let mut ret = -1;
    for row in table {
        for (jm, value) in row.iter().enumerate() {
            if *value != BAD_VALUE && (jm as i32) > ret {
                ret = jm as i32;
            }
        }
    }
    ret
}

/// Coefficient descriptions for one half of the SIP distortion.
///
/// Port of the two coefficient-gathering loops of C `SIPMapping`
/// (fitschan.c:29332-29459), which differ only in which pair of `FitsStore`
/// items they read and in whether the axis index is called `iout` or `iin`.
/// `tables` holds the table for each of the two axes in order.
///
/// For each axis the powers actually swept come from the store, not from any
/// `*_ORDER` keyword: `pmax` and `qmax` are the largest indices present, and an
/// absent slot contributes zero. Since the polynomial describes a *correction*
/// to the input while the Mapping is a direct transformation, the term matching
/// the axis itself is incremented by one. Terms whose coefficient works out to
/// zero are dropped, and if the store held no value at all for either axis the
/// whole transformation is undefined and no terms are returned.
fn build_sip_terms(tables: [&[Vec<f64>]; 2]) -> Vec<PolyTerm> {
    let mut terms = Vec::new();
    let mut def = false;

    for (iax, table) in tables.iter().enumerate() {
        let pmax = sip_max_i(table);
        let qmax = sip_max_jm(table);

        for p in 0..=pmax {
            for q in 0..=qmax {
                let mut cof = table
                    .get(p as usize)
                    .and_then(|row| row.get(q as usize))
                    .copied()
                    .unwrap_or(BAD_VALUE);
                if cof == BAD_VALUE {
                    cof = 0.0;
                } else {
                    def = true;
                }

                if p as usize == 1 - iax && q as usize == iax {
                    cof += 1.0;
                }

                if cof != 0.0 {
                    terms.push(PolyTerm {
                        coeff: cof,
                        powers: vec![p as usize, q as usize],
                        out_axis: iax + 1,
                    });
                }
            }
        }
    }

    if !def {
        terms.clear();
    }
    terms
}

/// Forward SIP coefficient descriptions, from the `A` and `B` tables.
fn build_sip_forward_terms(store: &FitsStore) -> Vec<PolyTerm> {
    build_sip_terms([&store.sip_a, &store.sip_b])
}

/// Inverse SIP coefficient descriptions, from the `AP` and `BP` tables.
fn build_sip_inverse_terms(store: &FitsStore) -> Vec<PolyTerm> {
    build_sip_terms([&store.sip_ap, &store.sip_bp])
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
        let mut store = simple_cdelt_store();
        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

        // Reference pixel (128, 128) should map to IWC (0, 0).
        let pts = point2d(128.0, 128.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!(result[0].abs() < 1e-10);
        assert!(result[1].abs() < 1e-10);
    }

    #[test]
    fn cdelt_offset_pixel() {
        let mut store = simple_cdelt_store();
        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

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
        let mut store = cd_matrix_store();
        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

        // Reference pixel should map to IWC (0, 0).
        let pts = point2d(128.0, 128.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!(result[0].abs() < 1e-10);
        assert!(result[1].abs() < 1e-10);
    }

    #[test]
    fn cd_matrix_offset_pixel() {
        let mut store = cd_matrix_store();
        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

        // Pixel (129, 128): offset (1, 0), so IWC = CD * (1, 0) = (CD[0][0], CD[1][0]).
        let pts = point2d(129.0, 128.0);
        let out = mapping.transform(&pts, true).unwrap();
        let result = out.point(0);
        assert!((result[0] - 0.000250).abs() < 1e-12);
        assert!((result[1] - 0.000230).abs() < 1e-12);
    }

    #[test]
    fn nin_nout_correct() {
        let mut store = simple_cdelt_store();
        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();
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

        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

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
        let mut store = FitsStore::new(2);
        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

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
        store.sip = true;
        store.sip_a = vec![vec![BAD_VALUE; 3]; 3];
        store.sip_a[2][0] = 1e-5; // A_2_0
        store.sip_b = vec![vec![BAD_VALUE, 0.0]]; // B_0_1 = 0

        let mapping = build_pixel_to_iwc(&mut store, &CardList::new(), &mut Vec::new()).unwrap();

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
