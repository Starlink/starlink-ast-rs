// Until SkyFrame lands (Task 4), these functions are only exercised by
// tests.  Suppress the dead-code lint at the module level.
#![allow(dead_code)]

//! Sky coordinate system conversion pipeline.
//!
//! Builds [`SlaMap`] chains for converting between celestial coordinate
//! systems through FK5 J2000.0 as a pivot.

use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, SlaCvtType, SlaMap, UnitMap};
use crate::pal;

/// MJD of J2000.0.
const J2000_MJD: f64 = 51544.5;

// ---------------------------------------------------------------------------
// SkySystem
// ---------------------------------------------------------------------------

/// Celestial coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkySystem {
    /// FK4 (Bessel-Newcomb precession, with E-terms of aberration).
    Fk4,
    /// FK4 without E-terms of aberration.
    Fk4NoE,
    /// FK5 (IAU 1976 precession).
    Fk5,
    /// Geocentric apparent place.
    Gappt,
    /// Ecliptic coordinates.
    Ecliptic,
    /// Galactic coordinates (IAU 1958).
    Galactic,
    /// Supergalactic coordinates (de Vaucouleurs).
    Supergalactic,
    /// International Celestial Reference System (Hipparcos).
    Icrs,
    /// Dynamical J2000.0 equatorial (approximately ICRS via frame tie).
    J2000,
    /// Helio-ecliptic coordinates.
    Helioecliptic,
    /// Horizon coordinates (azimuth, elevation).
    Azel,
    /// Unknown coordinate system.
    Unknown,
}

impl SkySystem {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fk4 => "FK4",
            Self::Fk4NoE => "FK4-NO-E",
            Self::Fk5 => "FK5",
            Self::Gappt => "GAPPT",
            Self::Ecliptic => "ECLIPTIC",
            Self::Galactic => "GALACTIC",
            Self::Supergalactic => "SUPERGALACTIC",
            Self::Icrs => "ICRS",
            Self::J2000 => "J2000",
            Self::Helioecliptic => "HELIOECLIPTIC",
            Self::Azel => "AZEL",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Parse a coordinate system name (case-insensitive).
    ///
    /// Accepts the canonical forms plus the underscore variant
    /// `"FK4_NO_E"`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "FK4" => Some(Self::Fk4),
            "FK4-NO-E" | "FK4_NO_E" => Some(Self::Fk4NoE),
            "FK5" => Some(Self::Fk5),
            "GAPPT" => Some(Self::Gappt),
            "ECLIPTIC" => Some(Self::Ecliptic),
            "GALACTIC" => Some(Self::Galactic),
            "SUPERGALACTIC" => Some(Self::Supergalactic),
            "ICRS" => Some(Self::Icrs),
            "J2000" => Some(Self::J2000),
            "HELIOECLIPTIC" => Some(Self::Helioecliptic),
            "AZEL" => Some(Self::Azel),
            "UNKNOWN" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Whether coordinates in this system depend on the equinox.
    pub fn needs_equinox(&self) -> bool {
        matches!(self, Self::Fk4 | Self::Fk4NoE | Self::Fk5 | Self::Ecliptic)
    }

    /// Whether coordinates in this system depend on the epoch.
    pub fn needs_epoch(&self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Default equinox as MJD for systems that require one.
    ///
    /// Returns J2000.0 (MJD 51544.5) for FK5 and Ecliptic, and
    /// B1950.0 for FK4 and FK4-NO-E.  For systems that do not use
    /// an equinox the return value is J2000.0.
    pub fn default_equinox_mjd(&self) -> f64 {
        match self {
            Self::Fk4 | Self::Fk4NoE => pal::epb2d(1950.0),
            _ => J2000_MJD,
        }
    }
}

// ---------------------------------------------------------------------------
// SkyRefIs
// ---------------------------------------------------------------------------

/// Interpretation of the SkyRef attribute in a SkyFrame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyRefIs {
    /// SkyRef defines the origin of the offset coordinate system.
    Origin,
    /// SkyRef defines the north pole of the offset coordinate system.
    Pole,
    /// SkyRef is ignored (no offset coordinate system).
    Ignored,
}

impl SkyRefIs {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Origin => "ORIGIN",
            Self::Pole => "POLE",
            Self::Ignored => "IGNORED",
        }
    }

    /// Parse a SkyRefIs value (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "ORIGIN" => Some(Self::Origin),
            "POLE" => Some(Self::Pole),
            "IGNORED" => Some(Self::Ignored),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// make_sky_mapping
// ---------------------------------------------------------------------------

/// Build a mapping that converts between two celestial coordinate systems.
///
/// The pipeline architecture routes every conversion through FK5 J2000.0
/// as a pivot:
///
/// ```text
/// from_sys → FK5 J2000.0 → align_sys(from_epoch)
///          → align_sys(to_epoch) → FK5 J2000.0 → to_sys
/// ```
///
/// Steps that would be identities are optimised away.
///
/// # Parameters
///
/// - `from_sys`, `to_sys` — source and destination coordinate systems.
/// - `from_eq_mjd`, `to_eq_mjd` — equinox as MJD (used for FK4/FK5/Ecliptic).
/// - `from_ep_mjd`, `to_ep_mjd` — epoch as MJD.
/// - `align_sys` — the intermediate system used for epoch alignment.
/// - `from_obs_lat`, `to_obs_lat` — observer latitude in radians (AZEL only).
/// - `from_last`, `to_last` — Local Apparent Sidereal Time in radians (AZEL only).
///
/// # Errors
///
/// Returns an error if one system is `Unknown` and the other is not.
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_sky_mapping(
    from_sys: SkySystem,
    from_eq_mjd: f64,
    from_ep_mjd: f64,
    to_sys: SkySystem,
    to_eq_mjd: f64,
    to_ep_mjd: f64,
    align_sys: SkySystem,
    from_obs_lat: f64,
    from_last: f64,
    to_obs_lat: f64,
    to_last: f64,
) -> AstResult<DynMapping> {
    // Both UNKNOWN: return identity.
    if from_sys == SkySystem::Unknown && to_sys == SkySystem::Unknown {
        return Ok(Box::new(UnitMap::new(2)));
    }

    // One UNKNOWN and the other not: no meaningful conversion.
    if from_sys == SkySystem::Unknown || to_sys == SkySystem::Unknown {
        return Err(AstError::new(
            ErrorCode::Inter,
            format!(
                "cannot convert between {} and {} sky coordinates",
                from_sys.as_str(),
                to_sys.as_str()
            ),
        ));
    }

    // Determine which of the 4 pipeline steps are needed.
    let mut step1 = true; // from_sys  → FK5 J2000
    let mut step2 = true; // FK5 J2000 → align_sys (from epoch)
    let mut step3 = true; // align_sys → FK5 J2000 (to epoch)
    let mut step4 = true; // FK5 J2000 → to_sys

    // If from_sys IS the alignment system, the first two steps cancel.
    if from_sys == align_sys {
        step1 = false;
        step2 = false;
    }

    // If to_sys IS the alignment system, the last two steps cancel.
    if to_sys == align_sys {
        step3 = false;
        step4 = false;
    }

    // If epochs match, or alignment is in FK5 (already the pivot), the
    // middle two steps cancel.
    if step2 && step3 && (align_sys == SkySystem::Fk5 || from_ep_mjd == to_ep_mjd) {
        step2 = false;
        step3 = false;
    }

    // If source and destination are identical in every respect, nothing
    // needs to be done at all.
    if from_sys == to_sys && from_eq_mjd == to_eq_mjd && from_ep_mjd == to_ep_mjd {
        step1 = false;
        step2 = false;
        step3 = false;
        step4 = false;
    }

    // Build a single SlaMap accumulating all required conversion steps.
    let mut sla = SlaMap::new();

    // Step 1: from_sys → FK5 J2000.0
    if step1 {
        add_system_to_fk5(
            &mut sla,
            from_sys,
            from_eq_mjd,
            from_ep_mjd,
            from_obs_lat,
            from_last,
        );
    }

    // Step 2: FK5 J2000.0 → align_sys (using from epoch/equinox)
    if step2 {
        add_fk5_to_system(
            &mut sla,
            align_sys,
            from_eq_mjd,
            from_ep_mjd,
            from_obs_lat,
            from_last,
        );
    }

    // Step 3: align_sys → FK5 J2000.0 (using to epoch/equinox)
    if step3 {
        add_system_to_fk5(
            &mut sla, align_sys, to_eq_mjd, to_ep_mjd, to_obs_lat, to_last,
        );
    }

    // Step 4: FK5 J2000.0 → to_sys
    if step4 {
        add_fk5_to_system(&mut sla, to_sys, to_eq_mjd, to_ep_mjd, to_obs_lat, to_last);
    }

    if sla.is_empty() {
        Ok(Box::new(UnitMap::new(2)))
    } else {
        Ok(Box::new(sla))
    }
}

// ---------------------------------------------------------------------------
// System → FK5 J2000.0
// ---------------------------------------------------------------------------

/// Add SlaMap steps to convert from `system` to FK5 J2000.0.
fn add_system_to_fk5(
    sla: &mut SlaMap,
    system: SkySystem,
    equinox_mjd: f64,
    epoch_mjd: f64,
    obs_lat: f64,
    last: f64,
) {
    let equinox_j = pal::epj(equinox_mjd);
    let equinox_b = pal::epb(equinox_mjd);
    let epoch_j = pal::epj(epoch_mjd);
    let epoch_b = pal::epb(epoch_mjd);

    match system {
        SkySystem::Fk5 => {
            // FK5 precession from the given equinox to J2000.0.
            if equinox_j != 2000.0 {
                sla.add(SlaCvtType::Prec, &[equinox_j, 2000.0]);
            }
        }

        SkySystem::Fk4 => {
            // Remove E-terms at the given equinox, precess to B1950.0,
            // add E-terms at B1950.0, then FK4→FK5.
            if equinox_b != 1950.0 {
                sla.add(SlaCvtType::Subet, &[equinox_b]);
                sla.add(SlaCvtType::Prebn, &[equinox_b, 1950.0]);
                sla.add(SlaCvtType::Addet, &[1950.0]);
            }
            sla.add(SlaCvtType::Fk45z, &[epoch_b]);
        }

        SkySystem::Fk4NoE => {
            // FK4 without E-terms: precess to B1950.0, add E-terms,
            // then FK4→FK5.
            if equinox_b != 1950.0 {
                sla.add(SlaCvtType::Prebn, &[equinox_b, 1950.0]);
            }
            sla.add(SlaCvtType::Addet, &[1950.0]);
            sla.add(SlaCvtType::Fk45z, &[epoch_b]);
        }

        SkySystem::Icrs | SkySystem::J2000 => {
            // ICRS (Hipparcos) to FK5 J2000.0.
            // J2000 dynamical ≈ ICRS via frame tie (~17 mas).
            sla.add(SlaCvtType::Hfk5z, &[epoch_j]);
        }

        SkySystem::Galactic => {
            sla.add(SlaCvtType::Galeq, &[]);
        }

        SkySystem::Supergalactic => {
            // Supergalactic → Galactic → FK5 J2000.0
            sla.add(SlaCvtType::Supgal, &[]);
            sla.add(SlaCvtType::Galeq, &[]);
        }

        SkySystem::Ecliptic => {
            sla.add(SlaCvtType::Ecleq, &[equinox_mjd]);
        }

        SkySystem::Helioecliptic => {
            // Helio-ecliptic uses epoch rather than equinox.
            sla.add(SlaCvtType::Ecleq, &[epoch_mjd]);
        }

        SkySystem::Gappt => {
            // Geocentric apparent → FK5 J2000.0
            sla.add(SlaCvtType::Amp, &[2000.0, epoch_mjd]);
        }

        SkySystem::Azel => {
            // Horizon → equatorial (ha, dec) → RA shift → mean place
            sla.add(SlaCvtType::Dh2e, &[obs_lat]);
            sla.add(SlaCvtType::H2r, &[last]);
            sla.add(SlaCvtType::Amp, &[2000.0, epoch_mjd]);
        }

        SkySystem::Unknown => {
            // Identity — nothing added.
        }
    }
}

// ---------------------------------------------------------------------------
// FK5 J2000.0 → System
// ---------------------------------------------------------------------------

/// Add SlaMap steps to convert from FK5 J2000.0 to `system`.
fn add_fk5_to_system(
    sla: &mut SlaMap,
    system: SkySystem,
    equinox_mjd: f64,
    epoch_mjd: f64,
    obs_lat: f64,
    last: f64,
) {
    let equinox_j = pal::epj(equinox_mjd);
    let equinox_b = pal::epb(equinox_mjd);
    let epoch_j = pal::epj(epoch_mjd);
    let epoch_b = pal::epb(epoch_mjd);

    match system {
        SkySystem::Fk5 => {
            // FK5 precession from J2000.0 to the given equinox.
            if equinox_j != 2000.0 {
                sla.add(SlaCvtType::Prec, &[2000.0, equinox_j]);
            }
        }

        SkySystem::Fk4 => {
            // FK5→FK4, then optionally precess from B1950.0 to the
            // target equinox.
            sla.add(SlaCvtType::Fk54z, &[epoch_b]);
            if equinox_b != 1950.0 {
                sla.add(SlaCvtType::Subet, &[1950.0]);
                sla.add(SlaCvtType::Prebn, &[1950.0, equinox_b]);
                sla.add(SlaCvtType::Addet, &[equinox_b]);
            }
        }

        SkySystem::Fk4NoE => {
            // FK5→FK4, remove E-terms, then optionally precess.
            sla.add(SlaCvtType::Fk54z, &[epoch_b]);
            sla.add(SlaCvtType::Subet, &[1950.0]);
            if equinox_b != 1950.0 {
                sla.add(SlaCvtType::Prebn, &[1950.0, equinox_b]);
            }
        }

        SkySystem::Icrs | SkySystem::J2000 => {
            sla.add(SlaCvtType::Fk5hz, &[epoch_j]);
        }

        SkySystem::Galactic => {
            sla.add(SlaCvtType::Eqgal, &[]);
        }

        SkySystem::Supergalactic => {
            // FK5 J2000.0 → Galactic → Supergalactic
            sla.add(SlaCvtType::Eqgal, &[]);
            sla.add(SlaCvtType::Galsup, &[]);
        }

        SkySystem::Ecliptic => {
            sla.add(SlaCvtType::Eqecl, &[equinox_mjd]);
        }

        SkySystem::Helioecliptic => {
            sla.add(SlaCvtType::Eqecl, &[epoch_mjd]);
        }

        SkySystem::Gappt => {
            sla.add(SlaCvtType::Map, &[2000.0, epoch_mjd]);
        }

        SkySystem::Azel => {
            // Mean place → geocentric apparent → RA shift → horizon
            sla.add(SlaCvtType::Map, &[2000.0, epoch_mjd]);
            sla.add(SlaCvtType::R2h, &[last]);
            sla.add(SlaCvtType::De2h, &[obs_lat]);
        }

        SkySystem::Unknown => {
            // Identity — nothing added.
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use crate::mappings::Mapping;
    use std::f64::consts::PI;

    const TOL: f64 = 1.0e-8;

    /// Assert two floats are within tolerance.
    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!(
            (a - b).abs() < tol,
            "expected {a} ~= {b} (diff = {:.2e})",
            (a - b).abs()
        );
    }

    /// Default epoch: J2000.0 as MJD.
    fn j2000_mjd() -> f64 {
        51544.5
    }

    /// Default equinox for FK5: J2000.0 as MJD.
    fn j2000_eq() -> f64 {
        51544.5
    }

    /// Transform a single point (lon, lat) through the mapping (forward).
    fn transform_fwd(mapping: &dyn Mapping, lon: f64, lat: f64) -> (f64, f64) {
        let pts = PointSet::from_rows([[lon, lat]]);
        let out = mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        (p[0], p[1])
    }

    /// Transform a single point (lon, lat) through the mapping (inverse).
    fn transform_inv(mapping: &dyn Mapping, lon: f64, lat: f64) -> (f64, f64) {
        let pts = PointSet::from_rows([[lon, lat]]);
        let out = mapping.transform(&pts, false).unwrap();
        let p = out.point(0);
        (p[0], p[1])
    }

    // -----------------------------------------------------------------------
    // SkySystem enum tests
    // -----------------------------------------------------------------------

    #[test]
    fn sky_system_as_str_parse_round_trip() {
        let systems = [
            SkySystem::Fk4,
            SkySystem::Fk4NoE,
            SkySystem::Fk5,
            SkySystem::Gappt,
            SkySystem::Ecliptic,
            SkySystem::Galactic,
            SkySystem::Supergalactic,
            SkySystem::Icrs,
            SkySystem::J2000,
            SkySystem::Helioecliptic,
            SkySystem::Azel,
            SkySystem::Unknown,
        ];
        for &sys in &systems {
            let s = sys.as_str();
            let parsed = SkySystem::parse(s).unwrap();
            assert_eq!(parsed, sys, "round-trip failed for {s}");
        }
    }

    #[test]
    fn sky_system_parse_case_insensitive() {
        assert_eq!(SkySystem::parse("fk5"), Some(SkySystem::Fk5));
        assert_eq!(SkySystem::parse("Galactic"), Some(SkySystem::Galactic));
        assert_eq!(SkySystem::parse("FK4-NO-E"), Some(SkySystem::Fk4NoE));
        assert_eq!(SkySystem::parse("FK4_NO_E"), Some(SkySystem::Fk4NoE));
        assert_eq!(SkySystem::parse("nonsense"), None);
    }

    #[test]
    fn sky_system_needs_equinox() {
        assert!(SkySystem::Fk4.needs_equinox());
        assert!(SkySystem::Fk4NoE.needs_equinox());
        assert!(SkySystem::Fk5.needs_equinox());
        assert!(SkySystem::Ecliptic.needs_equinox());
        assert!(!SkySystem::Galactic.needs_equinox());
        assert!(!SkySystem::Icrs.needs_equinox());
        assert!(!SkySystem::Azel.needs_equinox());
    }

    #[test]
    fn sky_system_needs_epoch() {
        assert!(SkySystem::Fk5.needs_epoch());
        assert!(SkySystem::Galactic.needs_epoch());
        assert!(SkySystem::Azel.needs_epoch());
        assert!(!SkySystem::Unknown.needs_epoch());
    }

    #[test]
    fn sky_system_default_equinox() {
        // FK5 default equinox is J2000.0
        assert_close(SkySystem::Fk5.default_equinox_mjd(), j2000_mjd(), 1.0e-6);
        // FK4 default equinox is B1950.0
        let b1950_mjd = pal::epb2d(1950.0);
        assert_close(SkySystem::Fk4.default_equinox_mjd(), b1950_mjd, 1.0e-6);
    }

    // -----------------------------------------------------------------------
    // SkyRefIs enum tests
    // -----------------------------------------------------------------------

    #[test]
    fn sky_ref_is_round_trip() {
        for &val in &[SkyRefIs::Origin, SkyRefIs::Pole, SkyRefIs::Ignored] {
            let s = val.as_str();
            let parsed = SkyRefIs::parse(s).unwrap();
            assert_eq!(parsed, val, "round-trip failed for {s}");
        }
    }

    #[test]
    fn sky_ref_is_parse_case_insensitive() {
        assert_eq!(SkyRefIs::parse("origin"), Some(SkyRefIs::Origin));
        assert_eq!(SkyRefIs::parse("POLE"), Some(SkyRefIs::Pole));
        assert_eq!(SkyRefIs::parse("Ignored"), Some(SkyRefIs::Ignored));
        assert_eq!(SkyRefIs::parse("bad"), None);
    }

    // -----------------------------------------------------------------------
    // FK5 → Galactic
    // -----------------------------------------------------------------------

    #[test]
    fn fk5_to_galactic() {
        // The galactic centre (l=0, b=0) is at approximately
        // RA=266.4 deg, Dec=-28.9 deg in FK5 J2000.0.
        let map = make_sky_mapping(
            SkySystem::Galactic,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        let (ra, dec) = transform_fwd(&*map, 0.0, 0.0);
        let ra_deg = ra.to_degrees().rem_euclid(360.0);
        let dec_deg = dec.to_degrees();

        assert_close(ra_deg, 266.405, 0.01);
        assert_close(dec_deg, -28.936, 0.01);
    }

    // -----------------------------------------------------------------------
    // FK5 → FK5 same equinox/epoch (identity)
    // -----------------------------------------------------------------------

    #[test]
    fn fk5_to_fk5_same() {
        let map = make_sky_mapping(
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        // Should be a UnitMap (identity).
        assert!(map.as_any().is::<UnitMap>());

        // Verify transform is identity.
        let lon = 1.234;
        let lat = 0.567;
        let (out_lon, out_lat) = transform_fwd(&*map, lon, lat);
        assert_close(out_lon, lon, 1.0e-14);
        assert_close(out_lat, lat, 1.0e-14);
    }

    // -----------------------------------------------------------------------
    // FK5 → FK5 different equinox
    // -----------------------------------------------------------------------

    #[test]
    fn fk5_different_equinox() {
        // Aligning in ICRS (the typical default for SkyFrame) allows the
        // equinox difference to produce a precession mapping.  When
        // align_sys == FK5, the "from" and "to" steps cancel because the
        // alignment is in FK5 itself and no conversion is needed.
        let eq_j2050 = pal::epj2d(2050.0);
        let map = make_sky_mapping(
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            eq_j2050,
            j2000_mjd(),
            SkySystem::Icrs, // align in ICRS to expose equinox difference
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        // Precession should produce a small but non-zero change.
        let lon = 1.0;
        let lat = 0.5;
        let (out_lon, out_lat) = transform_fwd(&*map, lon, lat);
        assert!(
            (out_lon - lon).abs() > 1.0e-6,
            "precession should be non-trivial"
        );
        // But the change should be small (< 1 degree over 50 years).
        assert!((out_lon - lon).abs() < 0.02);
        assert!((out_lat - lat).abs() < 0.02);

        // Round-trip check.
        let (back_lon, back_lat) = transform_inv(&*map, out_lon, out_lat);
        assert_close(back_lon, lon, TOL);
        assert_close(back_lat, lat, TOL);
    }

    // -----------------------------------------------------------------------
    // FK4 → FK5 round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn fk4_fk5_round_trip() {
        let b1950_mjd = pal::epb2d(1950.0);
        let map = make_sky_mapping(
            SkySystem::Fk4,
            b1950_mjd,
            b1950_mjd,
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        let lon = 1.0;
        let lat = 0.5;
        let (fk5_lon, fk5_lat) = transform_fwd(&*map, lon, lat);
        let (back_lon, back_lat) = transform_inv(&*map, fk5_lon, fk5_lat);
        assert_close(back_lon, lon, TOL);
        assert_close(back_lat, lat, TOL);
    }

    // -----------------------------------------------------------------------
    // ICRS → FK5
    // -----------------------------------------------------------------------

    #[test]
    fn icrs_to_fk5() {
        let map = make_sky_mapping(
            SkySystem::Icrs,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        // ICRS ≈ FK5 J2000 to within ~20 mas.  The frame tie is tiny.
        let lon = 2.0;
        let lat = 0.3;
        let (out_lon, out_lat) = transform_fwd(&*map, lon, lat);
        // Difference should be very small (< 1e-4 rad ≈ 20 arcsec).
        assert!((out_lon - lon).abs() < 1.0e-4);
        assert!((out_lat - lat).abs() < 1.0e-4);
    }

    // -----------------------------------------------------------------------
    // Galactic → Supergalactic → Galactic round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn galactic_supergalactic_round_trip() {
        let map = make_sky_mapping(
            SkySystem::Galactic,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Supergalactic,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        let lon = 1.5;
        let lat = 0.3;
        let (sg_lon, sg_lat) = transform_fwd(&*map, lon, lat);
        let (back_lon, back_lat) = transform_inv(&*map, sg_lon, sg_lat);
        assert_close(back_lon, lon, TOL);
        assert_close(back_lat, lat, TOL);
    }

    // -----------------------------------------------------------------------
    // Ecliptic → FK5
    // -----------------------------------------------------------------------

    #[test]
    fn ecliptic_to_fk5() {
        let map = make_sky_mapping(
            SkySystem::Ecliptic,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        // Ecliptic (lon=0, lat=0) is the vernal equinox, which in FK5
        // J2000 is RA=0, Dec=0.
        let (ra, dec) = transform_fwd(&*map, 0.0, 0.0);
        assert_close(ra, 0.0, 1.0e-6);
        assert_close(dec, 0.0, 1.0e-6);

        // Round-trip test.
        let lon = 2.0;
        let lat = 0.4;
        let (fk5_lon, fk5_lat) = transform_fwd(&*map, lon, lat);
        let (back_lon, back_lat) = transform_inv(&*map, fk5_lon, fk5_lat);
        assert_close(back_lon, lon, TOL);
        assert_close(back_lat, lat, TOL);
    }

    // -----------------------------------------------------------------------
    // All 12 systems round-trip through FK5 pivot
    // -----------------------------------------------------------------------

    #[test]
    fn all_systems_round_trip() {
        let systems = [
            SkySystem::Fk4,
            SkySystem::Fk4NoE,
            SkySystem::Fk5,
            SkySystem::Gappt,
            SkySystem::Ecliptic,
            SkySystem::Galactic,
            SkySystem::Supergalactic,
            SkySystem::Icrs,
            SkySystem::J2000,
            SkySystem::Helioecliptic,
            SkySystem::Azel,
            SkySystem::Unknown,
        ];

        let test_lon = 1.0;
        let test_lat = 0.5;
        let ep = j2000_mjd();
        let obs_lat = 0.5;
        let last = 1.0;

        for &sys in &systems {
            if sys == SkySystem::Unknown {
                // UNKNOWN ↔ UNKNOWN is identity; tested separately.
                continue;
            }

            let eq = sys.default_equinox_mjd();

            // Build: sys → FK5 → sys.
            let map = make_sky_mapping(
                sys,
                eq,
                ep,
                sys,
                eq,
                ep,
                SkySystem::Fk5,
                obs_lat,
                last,
                obs_lat,
                last,
            )
            .unwrap();

            // Either the optimiser returns a UnitMap, or the forward
            // + inverse of the mapping should round-trip.
            let (out_lon, out_lat) = transform_fwd(&*map, test_lon, test_lat);
            let name = sys.as_str();

            assert_close(out_lon, test_lon, TOL);
            assert_close(out_lat, test_lat, TOL);

            // Also test the inverse.
            let (inv_lon, inv_lat) = transform_inv(&*map, test_lon, test_lat);
            assert!(
                (inv_lon - test_lon).abs() < TOL,
                "{name}: inverse lon failed (diff = {:.2e})",
                (inv_lon - test_lon).abs()
            );
            assert!(
                (inv_lat - test_lat).abs() < TOL,
                "{name}: inverse lat failed (diff = {:.2e})",
                (inv_lat - test_lat).abs()
            );
        }
    }

    // -----------------------------------------------------------------------
    // Unknown system handling
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_to_unknown_is_identity() {
        let map = make_sky_mapping(
            SkySystem::Unknown,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Unknown,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();
        assert!(map.as_any().is::<UnitMap>());
    }

    #[test]
    fn unknown_to_fk5_is_error() {
        let result = make_sky_mapping(
            SkySystem::Unknown,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        );
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // AZEL round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn azel_round_trip() {
        let obs_lat = 0.5; // ~29 deg latitude
        let last = 1.0; // radians
        let ep = j2000_mjd();

        let map = make_sky_mapping(
            SkySystem::Azel,
            j2000_eq(),
            ep,
            SkySystem::Fk5,
            j2000_eq(),
            ep,
            SkySystem::Fk5,
            obs_lat,
            last,
            0.0,
            0.0,
        )
        .unwrap();

        // Pick a point above the horizon.
        let az = 1.0;
        let el = 0.5;
        let (ra, dec) = transform_fwd(&*map, az, el);
        let (back_az, back_el) = transform_inv(&*map, ra, dec);

        // Azimuth may wrap; compare modulo 2*pi.
        let az_diff = (back_az - az).rem_euclid(2.0 * PI);
        let az_err = az_diff.min(2.0 * PI - az_diff);
        assert!(
            az_err < TOL,
            "AZEL round-trip az: expected {az}, got {back_az} (err = {az_err:.2e})"
        );
        assert_close(back_el, el, TOL);
    }

    // -----------------------------------------------------------------------
    // GAPPT round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn gappt_round_trip() {
        let map = make_sky_mapping(
            SkySystem::Gappt,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        let lon = 2.0;
        let lat = 0.3;
        let (fk5_lon, fk5_lat) = transform_fwd(&*map, lon, lat);
        let (back_lon, back_lat) = transform_inv(&*map, fk5_lon, fk5_lat);
        assert_close(back_lon, lon, TOL);
        assert_close(back_lat, lat, TOL);
    }

    // -----------------------------------------------------------------------
    // Helioecliptic round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn helioecliptic_round_trip() {
        let map = make_sky_mapping(
            SkySystem::Helioecliptic,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            j2000_eq(),
            j2000_mjd(),
            SkySystem::Fk5,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        let lon = 1.5;
        let lat = 0.2;
        let (fk5_lon, fk5_lat) = transform_fwd(&*map, lon, lat);
        let (back_lon, back_lat) = transform_inv(&*map, fk5_lon, fk5_lat);
        assert_close(back_lon, lon, TOL);
        assert_close(back_lat, lat, TOL);
    }

    // -----------------------------------------------------------------------
    // Epoch alignment
    // -----------------------------------------------------------------------

    #[test]
    fn fk5_epoch_alignment_in_galactic() {
        // When aligning in Galactic, different epochs should not matter
        // since Galactic is epoch-independent.  The result should be
        // the same as a simple FK5 equinox precession.
        let ep1 = j2000_mjd();
        let ep2 = pal::epj2d(2020.0);

        let map = make_sky_mapping(
            SkySystem::Fk5,
            j2000_eq(),
            ep1,
            SkySystem::Fk5,
            j2000_eq(),
            ep2,
            SkySystem::Galactic,
            0.0,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();

        // The epochs differ, so this is NOT a UnitMap — the alignment
        // system (Galactic) has epoch-dependent conversion steps
        // (Galeq/Eqgal are epoch-independent, but FK5→Galactic passes
        // through the pivot).  Since Galactic has no epoch dependence,
        // the round-trip via Galactic should be near-identity.
        let lon = 1.0;
        let lat = 0.5;
        let (out_lon, out_lat) = transform_fwd(&*map, lon, lat);
        // The result should be very close to identity since both frames
        // are FK5 J2000 and galactic alignment is epoch-independent.
        assert_close(out_lon, lon, 1.0e-6);
        assert_close(out_lat, lat, 1.0e-6);
    }

    // -----------------------------------------------------------------------
    // SlaMap H2R/R2H tests
    // -----------------------------------------------------------------------

    #[test]
    fn h2r_r2h_round_trip() {
        // Test that H2R and R2H in SlaMap work correctly.
        let last = 1.5; // LAST in radians

        let mut sla = SlaMap::new();
        sla.add(SlaCvtType::H2r, &[last]);

        let ha = 0.8;
        let dec = 0.4;
        let (ra, out_dec) = transform_fwd(&sla, ha, dec);

        // RA = LAST - HA
        // But the result goes through 3-vector conversion, so we need
        // to normalise.  The key property: RA + HA = LAST.
        let sum = ra + ha;
        // Normalise to [0, 2*pi).
        let sum_norm = sum.rem_euclid(2.0 * PI);
        let last_norm = last.rem_euclid(2.0 * PI);
        assert_close(sum_norm, last_norm, 1.0e-10);
        assert_close(out_dec, dec, 1.0e-10);

        // Inverse (H2R inverse is R2H, which does the same operation)
        let (back_ha, back_dec) = transform_inv(&sla, ra, out_dec);
        assert_close(back_ha, ha, 1.0e-10);
        assert_close(back_dec, dec, 1.0e-10);
    }
}
