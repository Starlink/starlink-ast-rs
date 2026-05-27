//! Port of `fitschan.c::SkyPole` (~29399).
//!
//! Sets `LONPOLE` and `LATPOLE` keyword values in the [`FitsStore`]
//! based on the WcsMap's PV-3 / PV-4 longitude-axis parameters.
//! Both keywords describe the longitude / latitude of the celestial
//! north pole in the projection's native spherical coordinates.
//!
//! ## Zenithal vs. non-zenithal
//!
//! For **zenithal** projections (`AZP`, `SZP`, `TAN`, `STG`, `SIN`,
//! `ARC`, `ZPN`, `ZEA`, `AIR`, `XPH`, `HPX` — TPN is a separate
//! AST-extension case) the LONPOLE/LATPOLE values can be set
//! freely; FITS-WCS paper II expresses them as projection
//! parameters PV-3 / PV-4 on the longitude axis. This port handles
//! that case.
//!
//! For **non-zenithal** projections (`CYP`, `CEA`, `CAR`, `MER`,
//! `COP`, `COE`, `COD`, `COO`, `SFL`, `PAR`, `MOL`, `AIT`, `BON`,
//! `PCO`, `TSC`, `CSC`, `QSC`) the C source falls into a more
//! involved path that inverts the post-WcsMap chain on the
//! celestial north pole and runs `WcsNatPole` (a wcslib helper) to
//! pick the default LATPOLE. That branch is **not yet ported** —
//! see `docs/issues/fset_to_store-port-gaps.md` ("SkyPole port")
//! for the dependency on `WcsNatPole`.

#![allow(dead_code)]

use crate::fitschan::fits_store::FitsStore;
use crate::mappings::WcsMap;
use crate::wcslib::WcsProjection;

const DR2D: f64 = 180.0 / std::f64::consts::PI;

/// Whether `proj` is one of the zenithal projections — for these the
/// LONPOLE/LATPOLE port path is simply "read PV(axlon, 3/4) if set".
fn is_zenithal(proj: WcsProjection) -> bool {
    matches!(
        proj,
        WcsProjection::Azp
            | WcsProjection::Szp
            | WcsProjection::Tan
            | WcsProjection::Stg
            | WcsProjection::Sin
            | WcsProjection::Arc
            | WcsProjection::Zpn
            | WcsProjection::Zea
            | WcsProjection::Air
            | WcsProjection::Xph
            | WcsProjection::Hpx
            | WcsProjection::Tpn
    )
}

/// Write LONPOLE / LATPOLE (and the PV-3 / PV-4 copies on the
/// longitude axis for non-TPN projections) into `store`. Mirrors
/// `fitschan.c::SkyPole`.
///
/// `wcsmap` is the projection mapping. `fits_ilon` / `fits_ilat`
/// are the 0-based FITS-axis indices for the celestial pair (after
/// `wperm` permutation). `s` is the FITS coordinate-version
/// character; only applies to the LONPOLE/LATPOLE writes (the C
/// source uses `s` for the SetItem call). Currently `s` is unused
/// here because our `FitsStore` doesn't carry per-version copies
/// — when alt-version support lands this will need to dispatch.
pub(crate) fn sky_pole(
    wcsmap: &WcsMap,
    fits_ilon: usize,
    fits_ilat: usize,
    s: char,
    store: &mut FitsStore,
) {
    let _ = (fits_ilat, s);

    let proj = wcsmap.projection();
    let axlon_1based = wcsmap.lonax();

    let mut lonpole_rad: Option<f64> = None;
    let mut latpole_rad: Option<f64> = None;

    if is_zenithal(proj) {
        // Zenithal projections (other than TPN): read the explicit
        // PV-3 / PV-4 overrides if the user set them.  Mirrors
        // C:29505-29511.
        if proj != WcsProjection::Tpn {
            lonpole_rad = wcsmap.get_pv(axlon_1based, 3);
            latpole_rad = wcsmap.get_pv(axlon_1based, 4);
        }
        // For TPN both stay None — TPN uses PV(lonax, 3/4) as
        // polynomial slots.
    } else {
        // Non-zenithal projections need the full WcsNatPole-driven
        // calculation. Tracked in fset_to_store-port-gaps.md.
    }

    // Convert to FITS degrees and store.
    if let Some(v) = lonpole_rad {
        store.lonpole = Some(v * DR2D);
    }
    if let Some(v) = latpole_rad {
        store.latpole = Some(v * DR2D);
    }

    // Per FITS-WCS paper II (and C:29593-29596), copy LONPOLE/LATPOLE
    // values into PV(axlon, 3/4) on the FITS side too. Skip for TPN
    // — those slots hold polynomial coefficients there.
    if proj != WcsProjection::Tpn && fits_ilon < store.pv.len() && store.pv[fits_ilon].len() >= 5 {
        if let Some(v) = lonpole_rad {
            store.pv[fits_ilon][3] = Some(v * DR2D);
        }
        if let Some(v) = latpole_rad {
            store.pv[fits_ilon][4] = Some(v * DR2D);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tan_with_no_pv_overrides_emits_nothing() {
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let mut store = FitsStore::new(2);
        sky_pole(&wm, 0, 1, ' ', &mut store);
        assert!(store.lonpole.is_none());
        assert!(store.latpole.is_none());
        assert!(store.pv[0][3].is_none());
        assert!(store.pv[0][4].is_none());
    }

    #[test]
    fn tan_with_pv_overrides_emits_lonpole_latpole() {
        let mut wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        // PV-3 = lonpole, PV-4 = latpole. Internal storage uses
        // radians; FITS output uses degrees.
        wm.set_pv(1, 3, 90.0_f64.to_radians()).unwrap();
        wm.set_pv(1, 4, 45.0_f64.to_radians()).unwrap();
        let mut store = FitsStore::new(2);
        sky_pole(&wm, 0, 1, ' ', &mut store);
        let lon = store.lonpole.unwrap();
        let lat = store.latpole.unwrap();
        assert!((lon - 90.0).abs() < 1e-9, "lonpole={lon}");
        assert!((lat - 45.0).abs() < 1e-9, "latpole={lat}");
        // Also copied into the longitude-axis PV slots.
        assert!((store.pv[0][3].unwrap() - 90.0).abs() < 1e-9);
        assert!((store.pv[0][4].unwrap() - 45.0).abs() < 1e-9);
    }

    #[test]
    fn tpn_skips_pv_copy() {
        let mut wm = WcsMap::new(2, WcsProjection::Tpn, 1, 2).unwrap();
        // For TPN, the function should NOT read PV(lonax, 3/4) at all
        // (those slots are polynomial coefficients), and should NOT
        // copy LONPOLE/LATPOLE into them either.
        let _ = wm.set_pv(1, 3, 99.0_f64.to_radians());
        let _ = wm.set_pv(1, 4, 99.0_f64.to_radians());
        let mut store = FitsStore::new(2);
        sky_pole(&wm, 0, 1, ' ', &mut store);
        // store.lonpole / latpole stay None.
        assert!(store.lonpole.is_none());
        assert!(store.latpole.is_none());
        // PV(0, 3/4) on the FITS store stay None too — the TPN
        // copy-suppression also blocks the PV write path (which
        // we haven't populated upstream of this call in any case).
        assert!(store.pv[0][3].is_none());
    }

    #[test]
    fn non_zenithal_currently_skips() {
        // Non-zenithal projection (CYP) — the current Rust port is a
        // no-op here because the WcsNatPole-driven calculation is
        // tracked in the gap doc. Verify we at least don't panic and
        // produce no LONPOLE/LATPOLE output.
        let wm = WcsMap::new(2, WcsProjection::Cyp, 1, 2).unwrap();
        let mut store = FitsStore::new(2);
        sky_pole(&wm, 0, 1, ' ', &mut store);
        assert!(store.lonpole.is_none());
        assert!(store.latpole.is_none());
    }
}
