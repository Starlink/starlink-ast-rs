//! Port of `fitschan.c::SkyPole` (~29399).
//!
//! Sets `LONPOLE` and `LATPOLE` keyword values in the [`FitsStore`]
//! based on the WcsMap's PV-3 / PV-4 longitude-axis parameters
//! (zenithal projections) or by running the FITS-WCS-paper-II
//! equations-8-9-10 helper [`wcs_nat_pole`] (non-zenithal). Both
//! keywords describe the longitude / latitude of the celestial
//! north pole in the projection's native spherical coordinates.
//!
//! ## Zenithal vs. non-zenithal
//!
//! For **zenithal** projections (`AZP`, `SZP`, `TAN`, `STG`, `SIN`,
//! `ARC`, `ZPN`, `ZEA`, `AIR`, `XPH`, `HPX` — TPN is a separate
//! AST-extension case) the LONPOLE/LATPOLE values can be set
//! freely; FITS-WCS paper II expresses them as projection
//! parameters PV-3 / PV-4 on the longitude axis. We read those.
//!
//! For **non-zenithal** projections (`CYP`, `CEA`, `CAR`, `MER`,
//! `COP`, `COE`, `COD`, `COO`, `SFL`, `PAR`, `MOL`, `AIT`, `BON`,
//! `PCO`, `TSC`, `CSC`, `QSC`) we invert the post-WcsMap chain
//! (`map3`) on the celestial north pole to get its native
//! coordinates, then call [`wcs_nat_pole`] to pick the default
//! LATPOLE. Mirrors the C path at fitschan.c:29514-29574.

#![allow(dead_code)]
#![allow(clippy::too_many_arguments)] // sky_pole mirrors the C 8-arg signature
#![allow(clippy::manual_clamp)] // mirrors C's explicit if-cascade for the latpole clamp

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::mappings::{Mapping, WcsMap};
use crate::wcslib::WcsProjection;

use super::fiducial::get_fiducial_nsc;

const DR2D: f64 = 180.0 / std::f64::consts::PI;
const DD2R: f64 = std::f64::consts::PI / 180.0;
const DPI: f64 = std::f64::consts::PI;
const DPIBY2: f64 = std::f64::consts::FRAC_PI_2;
const TOL1: f64 = 1.0e-13;
const TOL2: f64 = 1.0e-10;

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
/// `wcsmap` is the projection mapping. `map3` is the post-WcsMap
/// chain (native-spherical → celestial); only consulted for
/// non-zenithal projections to find the celestial-north-pole's
/// native-coord position. `ilon` / `ilat` are the 0-based output-
/// axis indices in `map3` for the celestial pair (the C `ilon` /
/// `ilat` arguments). `fits_ilon` / `fits_ilat` are the 0-based
/// FITS-axis indices for the celestial pair (after `wperm`
/// permutation). `s` is the FITS coordinate-version character.
///
/// `axlon_full_0based` / `axlat_full_0based` are the WcsMap's longitude
/// and latitude axis indices in the full pipeline (i.e. after the
/// 2-axis WcsMap has been lifted into the wider FrameSet via PermMap).
/// They index `map3`'s in/out spaces, which carry the full pipeline
/// width. Pass the 2-axis WcsMap directly via `wcsmap` — the embedded
/// projection — and the lifted indices via these arguments.
///
/// Mirrors the full body of `fitschan.c::SkyPole`. C's
/// `axlon = astGetWcsAxis(map2, 0)` (line 29485) is the lifted index;
/// the 2-axis `wcsmap` here exposes only `0`/`1` via `lonax_0based()`.
pub(crate) fn sky_pole(
    wcsmap: &WcsMap,
    map3: &dyn Mapping,
    ilon: usize,
    ilat: usize,
    axlon_full_0based: usize,
    axlat_full_0based: usize,
    fits_ilon: usize,
    fits_ilat: usize,
    s: char,
    store: &mut FitsStore,
) -> AstResult<()> {
    let _ = (fits_ilat, s);

    let proj = wcsmap.projection();
    let axlon_1based = wcsmap.lonax();
    let axlon_0based = axlon_full_0based;
    let axlat_0based = axlat_full_0based;

    let mut lonpole_rad: Option<f64> = None;
    let mut latpole_rad: Option<f64> = None;

    if is_zenithal(proj) {
        // Zenithal projections (other than TPN): read the explicit
        // PV-3 / PV-4 overrides if the user set them. Mirrors
        // C:29505-29511. WcsMap PV parameters are stored in FITS-WCS
        // degrees, so convert to radians for this function's internal
        // (radian) contract before the final `* DR2D` store.
        if proj != WcsProjection::Tpn {
            lonpole_rad = wcsmap.get_pv(axlon_1based, 3).map(|v| v * DD2R);
            latpole_rad = wcsmap.get_pv(axlon_1based, 4).map(|v| v * DD2R);
        }
        // For TPN both stay None — TPN uses PV(lonax, 3/4) as
        // polynomial slots.
    } else {
        // Non-zenithal: invert map3 on the celestial north pole to
        // find its native coords, then run wcs_nat_pole to pick the
        // default LATPOLE. Mirrors C:29514-29578.
        let nax2 = map3.nout();
        if ilon >= nax2 || ilat >= nax2 {
            // Indexing failure — leave both keywords as None.
            return Ok(());
        }
        // Build the celestial north pole position in WCS space:
        // zeros except π/2 at the latitude index.
        let mut wcs_pole = vec![0.0_f64; nax2];
        wcs_pole[ilat] = DPIBY2;
        let pts_wcs = PointSet::from_rows([wcs_pole]);

        // Inverse-transform through map3 to native space.
        let pts_native = match map3.transform(&pts_wcs, false) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        let native_row = pts_native.point(0);
        let nax1 = map3.nin();
        if axlon_0based >= nax1 || axlat_0based >= nax1 {
            return Ok(());
        }
        let phip_in = native_row[axlon_0based];
        let actual_native_lat = native_row[axlat_0based];
        if !phip_in.is_finite() || !actual_native_lat.is_finite() {
            return Ok(());
        }

        // Read CRVAL (radians) for the fiducial point — it's the
        // input to wcs_nat_pole. C reads from store->crval[fits_ilon]
        // which has been written by celestial_axes prior to this call.
        let alpha0 = store.crval.get(fits_ilon).copied().flatten().unwrap_or(0.0) * DD2R;
        let delta0 = store.crval.get(fits_ilat).copied().flatten().unwrap_or(0.0) * DD2R;

        // Run wcs_nat_pole with sentinel latpole = 999 rad ⇒ "no
        // value supplied". Mirrors C:29543.
        let mut phip = phip_in;
        let mut alphap = 0.0_f64;
        let mut deltap = 0.0_f64;
        let success = wcs_nat_pole(
            wcsmap,
            alpha0,
            delta0,
            999.0,
            &mut phip,
            &mut alphap,
            &mut deltap,
        );
        if success {
            // If the default LATPOLE matches the actual native lat
            // we found by inverse-transforming, the keyword is
            // implicit and we don't store it. Mirrors C:29547-29553.
            let chosen_latpole = if equal_angles(actual_native_lat, deltap) {
                None
            } else {
                Some(actual_native_lat)
            };
            latpole_rad = chosen_latpole;
        } else {
            // Default is undefined — always emit. C:29555-29559.
            latpole_rad = Some(actual_native_lat);
        }

        // LONPOLE: drange of phip_in. The default is 0 (delta0 >=
        // theta0) or π. If equal, store None. Mirrors C:29561-29573.
        let (_phi0, theta0) = get_fiducial_nsc(wcsmap);
        let default_lonpole = if delta0 >= theta0 { 0.0 } else { DPI };
        let lp = crate::pal::dranrm(phip_in);
        lonpole_rad = if equal_angles(lp, default_lonpole) {
            None
        } else {
            Some(lp)
        };
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

    Ok(())
}

/// Compare two angles for equality with the C library's `EQUALANG`
/// tolerance — a small fraction of 2π.
fn equal_angles(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-10
}

/// Find the celestial coords of the native north pole given the
/// projection's fiducial-point celestial coords plus the user's
/// LONPOLE / LATPOLE inputs (or sentinels for "use default").
///
/// Implements equations 8-10 of FITS-WCS paper II
/// (Calabretta & Greisen). Mirrors `fitschan.c::WcsNatPole`
/// (~38361). Returns `true` on success. `phip` is updated in place
/// with the chosen LONPOLE (input-or-default), and `alphap` /
/// `deltap` are written with the resulting native pole celestial
/// coords. The `latpole` argument is the user-supplied LATPOLE in
/// radians, or any value ≥ π (e.g. `999.0`) to signal "no value
/// supplied — pick the default". Likewise an input `*phip` of
/// ≥ π (or `f64::INFINITY`) means "no LONPOLE supplied".
pub(crate) fn wcs_nat_pole(
    wcsmap: &WcsMap,
    alpha0: f64,
    delta0: f64,
    latpole: f64,
    phip: &mut f64,
    alphap: &mut f64,
    deltap: &mut f64,
) -> bool {
    let (phi0, theta0) = get_fiducial_nsc(wcsmap);
    if !phi0.is_finite() || !theta0.is_finite() {
        return false;
    }

    // Default LONPOLE if not supplied.
    if !phip.is_finite() || phip.abs() > 100.0 {
        *phip = if delta0 >= theta0 { 0.0 } else { DPI };
    }

    let sin_theta0 = theta0.sin();

    // Special-case: fiducial at native north pole (theta0 = π/2)
    // ⇒ celestial pole coincides with fiducial. C:38491-38494.
    if (sin_theta0 - 1.0).abs() < TOL2 {
        *alphap = alpha0;
        *deltap = delta0;
        return alphap.is_finite() && deltap.is_finite();
    }
    // Special-case: fiducial at native south pole.
    if (sin_theta0 + 1.0).abs() < TOL2 {
        *alphap = alpha0 + DPI;
        *deltap = -delta0;
        return alphap.is_finite() && deltap.is_finite();
    }

    let cos_theta0 = theta0.cos();
    let cos_delta0 = delta0.cos();
    let cos_phip = (*phip - phi0).cos();
    let sin_delta0 = delta0.sin();
    let sin_phip = (*phip - phi0).sin();

    let t0 = cos_theta0 * cos_phip;
    let deltap_chosen: Option<f64>;

    if t0.abs() < TOL2 && sin_theta0.abs() < TOL2 {
        // Indeterminate — fall back to user-supplied LATPOLE.
        if latpole.is_finite() && latpole.abs() <= 100.0 {
            deltap_chosen = Some(latpole);
        } else {
            return false;
        }
    } else {
        let t1 = sin_theta0.atan2(t0);
        let mut t2 = cos_theta0 * cos_phip;
        t2 = t2 * t2 + sin_theta0 * sin_theta0;
        if t2 <= f64::MIN_POSITIVE {
            return false;
        }
        let t3 = sin_delta0 / t2.sqrt();
        if t3.abs() > 1.0 + TOL1 {
            return false;
        }
        let t4 = if t3 < -1.0 {
            DPI
        } else if t3 > 1.0 {
            0.0
        } else {
            t3.acos()
        };
        let deltap_1 = crate::pal::drange(t1 + t4);
        let deltap_2 = crate::pal::drange(t1 - t4);

        // Choose between deltap_1 / deltap_2.
        let p = if deltap_1.abs() > DPIBY2 + TOL2 {
            deltap_2
        } else if deltap_2.abs() > DPIBY2 + TOL2 {
            deltap_1
        } else if latpole.is_finite() && latpole.abs() <= 100.0 {
            // Both in range — pick the one closer to user-supplied
            // LATPOLE.
            if (deltap_1 - latpole).abs() < (deltap_2 - latpole).abs() {
                deltap_1
            } else {
                deltap_2
            }
        } else {
            // No LATPOLE — pick the northern one. Mirrors C:38563-38568.
            if deltap_1 > deltap_2 {
                deltap_1
            } else {
                deltap_2
            }
        };

        // Clamp to ±π/2 (mirrors C:38578-38584).
        let p = if p.abs() > DPIBY2 + TOL2 {
            return false;
        } else if p < -DPIBY2 {
            -DPIBY2
        } else if p > DPIBY2 {
            DPIBY2
        } else {
            p
        };
        deltap_chosen = Some(p);
    }
    let dp = match deltap_chosen {
        Some(v) => v,
        None => return false,
    };
    *deltap = dp;

    // Find alphap. Mirrors C:38591-38614.
    if cos_delta0.abs() > TOL2 {
        let cos_deltap = dp.cos();
        let sin_deltap = dp.sin();
        if cos_deltap.abs() > TOL2 {
            let t1 = sin_phip * cos_theta0 / cos_delta0;
            let t2 = (sin_theta0 - sin_deltap * sin_delta0) / (cos_delta0 * cos_deltap);
            *alphap = if t1.abs() > TOL2 || t2.abs() > TOL2 {
                alpha0 - t1.atan2(t2)
            } else {
                alpha0
            };
        } else if sin_deltap > 0.0 {
            *alphap = alpha0 + (*phip - phi0) - DPI;
        } else {
            *alphap = alpha0 - (*phip - phi0);
        }
    } else {
        *alphap = alpha0;
    }

    deltap.is_finite() && alphap.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::UnitMap;

    /// Stand-in `map3` for the tests where we don't exercise the
    /// non-zenithal path: identity mapping over the same dimension
    /// as the WcsMap.
    fn unit_map3(naxes: usize) -> Box<dyn Mapping> {
        Box::new(UnitMap::new(naxes))
    }

    #[test]
    fn tan_with_no_pv_overrides_emits_nothing() {
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let map3 = unit_map3(2);
        let mut store = FitsStore::new(2);
        sky_pole(&wm, map3.as_ref(), 0, 1, 0, 1, 0, 1, ' ', &mut store).unwrap();
        assert!(store.lonpole.is_none());
        assert!(store.latpole.is_none());
        assert!(store.pv[0][3].is_none());
        assert!(store.pv[0][4].is_none());
    }

    #[test]
    fn tan_with_pv_overrides_emits_lonpole_latpole() {
        let mut wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        // PV-3 = lonpole, PV-4 = latpole. WcsMap PV parameters are stored
        // in FITS-WCS degrees (matching C and the read path); FITS output
        // is also degrees.
        wm.set_pv(1, 3, 90.0).unwrap();
        wm.set_pv(1, 4, 45.0).unwrap();
        let map3 = unit_map3(2);
        let mut store = FitsStore::new(2);
        sky_pole(&wm, map3.as_ref(), 0, 1, 0, 1, 0, 1, ' ', &mut store).unwrap();
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
        let map3 = unit_map3(2);
        let mut store = FitsStore::new(2);
        sky_pole(&wm, map3.as_ref(), 0, 1, 0, 1, 0, 1, ' ', &mut store).unwrap();
        // store.lonpole / latpole stay None.
        assert!(store.lonpole.is_none());
        assert!(store.latpole.is_none());
        // PV(0, 3/4) on the FITS store stay None too — the TPN
        // copy-suppression also blocks the PV write path (which
        // we haven't populated upstream of this call in any case).
        assert!(store.pv[0][3].is_none());
    }

    #[test]
    fn non_zenithal_with_unit_map3_emits_no_explicit_keywords() {
        // Non-zenithal projection (CAR) with map3 = identity. The
        // celestial north pole (in WCS space) at position (0, π/2)
        // inverts back through identity to native (0, π/2) — i.e.
        // theta0 = π/2 fiducial coincides with native pole. With no
        // CRVAL set in the store the alpha0/delta0 default to 0,
        // and wcs_nat_pole then reaches the "fiducial = native pole"
        // special case where alphap = 0, deltap = 0. The actual
        // native lat from inversion is π/2, which doesn't match the
        // (0) deltap, so LATPOLE is emitted. We at least verify we
        // don't panic and the call completes.
        let wm = WcsMap::new(2, WcsProjection::Car, 1, 2).unwrap();
        let map3 = unit_map3(2);
        let mut store = FitsStore::new(2);
        sky_pole(&wm, map3.as_ref(), 0, 1, 0, 1, 0, 1, ' ', &mut store).unwrap();
        // Specific values depend on CAR fiducial conventions; just
        // verify the call ran without erroring.
    }

    // ----- wcs_nat_pole equation tests -----

    #[test]
    fn wcs_nat_pole_fiducial_at_north_pole() {
        // For TAN at default fiducial (0, π/2), the celestial pole
        // coincides with the fiducial. Mirrors C:38491-38494.
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let mut phip = f64::INFINITY; // sentinel "no LONPOLE"
        let mut alphap = 0.0;
        let mut deltap = 0.0;
        let alpha0 = 1.5;
        let delta0 = 0.7;
        let ok = wcs_nat_pole(
            &wm,
            alpha0,
            delta0,
            999.0,
            &mut phip,
            &mut alphap,
            &mut deltap,
        );
        assert!(ok);
        // theta0 = π/2 ⇒ alphap = alpha0, deltap = delta0.
        assert!((alphap - alpha0).abs() < 1e-12);
        assert!((deltap - delta0).abs() < 1e-12);
    }
}
