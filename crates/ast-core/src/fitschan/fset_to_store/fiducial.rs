//! Port of `fitschan.c::GetFiducialNSC` (~12649) and `GetFiducialWCS`
//! (~12823).
//!
//! Computes the celestial coordinates of a WcsMap projection's
//! fiducial point — the structural input that lets `celestial_axes`
//! pick CRVAL = the projection's exact fiducial rather than
//! `world(g0)`. The "use the projection's own fiducial" rule is the
//! fix the spec calls out for the ~µpixel CRPIX residual.

// Wired in by celestial_axes (Task 5.3); until then nothing here is
// reachable from non-test code.
#![allow(dead_code)]

use std::f64::consts::PI;

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::mappings::{Mapping, WcsMap};
use crate::wcslib::WcsProjection;

const DD2R: f64 = PI / 180.0;

/// Return the native spherical coordinates `(phi, theta)` at the
/// projection's fiducial point.
///
/// For non-TPN projections the values of PV-parameters 1 and 2 on
/// the longitude axis are used (defaulting to the projection's
/// reference-point native coords when PV-parameter 0 is zero — the
/// C `astGetPV(map, axlon, 0) != 0.0` test). For TPN the PV array
/// holds polynomial coefficients, so we always use the native
/// reference-point.
///
/// Returns radians.
///
/// Mirrors `fitschan.c::GetFiducialNSC`.
pub(crate) fn get_fiducial_nsc(wcsmap: &WcsMap) -> (f64, f64) {
    // get_pv() takes the 1-based axis convention used by WcsMap's
    // PV-attribute API.
    let axlon_pv = wcsmap.lonax();
    if wcsmap.projection() != WcsProjection::Tpn {
        // PV(axlon, 0) == 0 means "use the projection-reference-point
        // native lon/lat"; PV(axlon, 0) != 0 means "use PV(axlon, 1/2)
        // (in degrees, convert to radians)". Mirrors C:12706-12714.
        //
        // FITS-WCS paper II gives defaults for the longitude-axis PV
        // parameters when not explicitly set: PV1_1 defaults to
        // `nat_lon` (degrees), PV1_2 defaults to `nat_lat` (degrees).
        // C's `astGetPV` (wcsmap.c:2125-2144) applies those defaults
        // before returning. Our raw `get_pv` returns `None` for
        // unset slots, so we mirror the C default here.
        let pv0 = wcsmap.get_pv(axlon_pv, 0).unwrap_or(0.0);
        if pv0 != 0.0 {
            let nat_lon_deg = wcsmap.nat_lon().to_degrees();
            let nat_lat_deg = wcsmap.nat_lat().to_degrees();
            let phi_deg = wcsmap.get_pv(axlon_pv, 1).unwrap_or(nat_lon_deg);
            let theta_deg = wcsmap.get_pv(axlon_pv, 2).unwrap_or(nat_lat_deg);
            return (DD2R * phi_deg, DD2R * theta_deg);
        }
    }
    (wcsmap.nat_lon(), wcsmap.nat_lat())
}

/// Return the celestial WCS coordinates `(fidlon, fidlat)` at the
/// projection's fiducial point — i.e. transform the native spherical
/// fiducial through `map2` (native-spherical → WCS) and extract the
/// `colon` / `colat` outputs.
///
/// Returns `None` if either output is non-finite (the C `AST__BAD`
/// branch at line 12923).
///
/// Mirrors `fitschan.c::GetFiducialWCS`.
pub(crate) fn get_fiducial_wcs(
    wcsmap: &WcsMap,
    map2: &dyn Mapping,
    colon: usize,
    colat: usize,
) -> AstResult<Option<(f64, f64)>> {
    let naxin = map2.nin();
    if naxin == 0 {
        return Ok(None);
    }
    // 0-based axis indices for the native-spherical input slot.
    let axlon = wcsmap.lonax_0based();
    let axlat = wcsmap.latax_0based();
    if axlon >= naxin || axlat >= naxin {
        return Ok(None);
    }

    let (phi, theta) = get_fiducial_nsc(wcsmap);
    if !phi.is_finite() || !theta.is_finite() {
        return Ok(None);
    }

    // Native-spherical input: zeros except phi at axlon, theta at axlat.
    let mut input = vec![0.0_f64; naxin];
    input[axlon] = phi;
    input[axlat] = theta;
    let pts = PointSet::from_rows([input]);
    let out = map2.transform(&pts, true)?;
    let row = out.point(0);
    if colon >= row.len() || colat >= row.len() {
        return Ok(None);
    }
    let fidlon = row[colon];
    let fidlat = row[colat];
    if !fidlon.is_finite() || !fidlat.is_finite() {
        return Ok(None);
    }
    Ok(Some((fidlon, fidlat)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::UnitMap;

    #[test]
    fn nsc_default_tan_is_zero_zero() {
        // Default TAN: native fiducial = (0, π/2) per WcsMap; but if
        // PV0 is unset, falls through to nat_lon / nat_lat. Verify
        // those defaults round-trip.
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let (phi, theta) = get_fiducial_nsc(&wm);
        // For TAN with no PV overrides, the projection reference point
        // is at native (0, π/2) (i.e. north pole — TAN projects from a
        // tangent at the pole).
        assert!((phi - wm.nat_lon()).abs() < 1e-12);
        assert!((theta - wm.nat_lat()).abs() < 1e-12);
    }

    #[test]
    fn nsc_pv_overrides_native_when_pv0_set() {
        // PV(lonax, 0) != 0 ⇒ use PV(lonax, 1) and PV(lonax, 2) in
        // degrees, converted to radians.
        let mut wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        wm.set_pv(1, 0, 1.0).unwrap(); // mark "use PV"
        wm.set_pv(1, 1, 30.0).unwrap(); // phi = 30°
        wm.set_pv(1, 2, 60.0).unwrap(); // theta = 60°
        let (phi, theta) = get_fiducial_nsc(&wm);
        assert!((phi - 30.0_f64.to_radians()).abs() < 1e-12);
        assert!((theta - 60.0_f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn nsc_tpn_always_uses_native() {
        // TPN ignores PV(lonax, 0/1/2) — those slots hold polynomial
        // coefficients, not fiducial coords.
        let mut wm = WcsMap::new(2, WcsProjection::Tpn, 1, 2).unwrap();
        // Try to set PV that would override for non-TPN.
        let _ = wm.set_pv(1, 0, 1.0);
        let _ = wm.set_pv(1, 1, 30.0);
        let _ = wm.set_pv(1, 2, 60.0);
        let (phi, theta) = get_fiducial_nsc(&wm);
        // Should still be the native reference-point.
        assert!((phi - wm.nat_lon()).abs() < 1e-12);
        assert!((theta - wm.nat_lat()).abs() < 1e-12);
    }

    #[test]
    fn wcs_with_unit_map2_returns_native_fiducial() {
        // map2 = identity ⇒ celestial fiducial == native fiducial.
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let map2 = UnitMap::new(2);
        let (phi, theta) = get_fiducial_nsc(&wm);
        let r = get_fiducial_wcs(&wm, &map2, 0, 1).unwrap().unwrap();
        assert!((r.0 - phi).abs() < 1e-12);
        assert!((r.1 - theta).abs() < 1e-12);
    }

    #[test]
    fn wcs_with_pv_overrides_passes_through_to_celestial() {
        let mut wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        wm.set_pv(1, 0, 1.0).unwrap();
        wm.set_pv(1, 1, 45.0).unwrap();
        wm.set_pv(1, 2, 30.0).unwrap();
        let map2 = UnitMap::new(2);
        let r = get_fiducial_wcs(&wm, &map2, 0, 1).unwrap().unwrap();
        assert!((r.0 - 45.0_f64.to_radians()).abs() < 1e-12);
        assert!((r.1 - 30.0_f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn invalid_axes_return_none() {
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let map2 = UnitMap::new(2);
        assert!(get_fiducial_wcs(&wm, &map2, 5, 1).unwrap().is_none());
    }
}
