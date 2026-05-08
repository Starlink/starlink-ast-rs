//! Tests for SpecMap — spectral coordinate conversion mapping.
//!
//! Port of relevant tests from `reference/starlink-ast/ast_tester/testspecmap.c`.

use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, SpecCvtType, SpecMap};

const C_LIGHT: f64 = 2.99792458e8;
const H_PLANCK: f64 = 6.6260755e-34;

/// Helper: transform a single value forward through a SpecMap.
fn transform_fwd(spec: &SpecMap, val: f64) -> f64 {
    let pts = PointSet::from_rows([[val]]);
    let out = spec.transform(&pts, true).unwrap();
    out.point(0)[0]
}

/// Helper: transform a single value inverse through a SpecMap.
fn transform_inv(spec: &SpecMap, val: f64) -> f64 {
    let pts = PointSet::from_rows([[val]]);
    let out = spec.transform(&pts, false).unwrap();
    out.point(0)[0]
}

/// Frequency to vacuum wavelength: lambda = c / f.
#[test]
fn freq_to_wavelength() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWv, &[]);

    let freq = 5.0e14; // Hz
    let wv = transform_fwd(&spec, freq);
    let expected = C_LIGHT / freq;
    assert!(
        (wv - expected).abs() < 1e-20,
        "FrToWv: got {wv}, expected {expected}"
    );
}

/// Frequency to energy: E = h * f.
#[test]
fn freq_to_energy() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToEn, &[]);

    let freq = 5.0e14;
    let energy = transform_fwd(&spec, freq);
    let expected = H_PLANCK * freq;
    assert!(
        (energy - expected).abs() / expected < 1e-10,
        "FrToEn: got {energy}, expected {expected}"
    );
}

/// Energy to frequency: f = E / h.
#[test]
fn energy_to_freq() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::EnToFr, &[]);

    let energy = 3.313e-19;
    let freq = transform_fwd(&spec, energy);
    let expected = energy / H_PLANCK;
    assert!(
        (freq - expected).abs() / expected < 1e-10,
        "EnToFr: got {freq}, expected {expected}"
    );
}

/// Round-trip: frequency -> wavelength -> frequency = identity.
#[test]
fn roundtrip_freq_wavelength() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWv, &[]);
    spec.add(SpecCvtType::WvToFr, &[]);

    let freq = 4.567e14;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "Round-trip freq->wv->freq: got {result}, expected {freq}"
    );
}

/// Round-trip: frequency -> velocity -> frequency = identity (with rest frequency).
#[test]
fn roundtrip_freq_velocity() {
    let rest_freq = 1.42e9; // 21cm hydrogen line
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToVl, &[rest_freq]);
    spec.add(SpecCvtType::VlToFr, &[rest_freq]);

    let freq = 1.41e9; // slightly shifted
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "Round-trip freq->vel->freq: got {result}, expected {freq}"
    );
}

/// Frequency to wave number: wn = f / c.
#[test]
fn freq_to_wavenum() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWn, &[]);

    let freq = 3.0e14;
    let wn = transform_fwd(&spec, freq);
    let expected = freq / C_LIGHT;
    assert!(
        (wn - expected).abs() / expected < 1e-12,
        "FrToWn: got {wn}, expected {expected}"
    );
}

/// Wave number to frequency: f = wn * c.
#[test]
fn wavenum_to_freq() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::WnToFr, &[]);

    let wn = 1.0e6; // 1 million cycles/m
    let freq = transform_fwd(&spec, wn);
    let expected = wn * C_LIGHT;
    assert!(
        (freq - expected).abs() / expected < 1e-12,
        "WnToFr: got {freq}, expected {expected}"
    );
}

/// Beta factor to relativistic velocity.
#[test]
fn beta_to_velocity() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::BtToVl, &[]);

    let beta = 0.5;
    let vel = transform_fwd(&spec, beta);
    let expected = beta * C_LIGHT;
    assert!(
        (vel - expected).abs() < 1.0,
        "BtToVl: got {vel}, expected {expected}"
    );
}

/// Relativistic velocity to beta factor.
#[test]
fn velocity_to_beta() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::VlToBt, &[]);

    let vel = 1.5e8; // 0.5c
    let beta = transform_fwd(&spec, vel);
    let expected = vel / C_LIGHT;
    assert!(
        (beta - expected).abs() < 1e-12,
        "VlToBt: got {beta}, expected {expected}"
    );
}

/// Redshift to relativistic velocity and back.
#[test]
fn roundtrip_redshift_velocity() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::ZoToVl, &[]);
    spec.add(SpecCvtType::VlToZo, &[]);

    let z = 0.1;
    let result = transform_fwd(&spec, z);
    assert!(
        (result - z).abs() < 1e-12,
        "Round-trip z->v->z: got {result}, expected {z}"
    );
}

/// Radio velocity to relativistic velocity and back.
#[test]
fn roundtrip_radio_velocity() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::VrToVl, &[]);
    spec.add(SpecCvtType::VlToVr, &[]);

    let vr = 1.0e7; // 10 km/s in m/s... actually 10000 km/s
    let result = transform_fwd(&spec, vr);
    assert!(
        (result - vr).abs() / vr.abs() < 1e-12,
        "Round-trip vr->vl->vr: got {result}, expected {vr}"
    );
}

/// Optical velocity to relativistic velocity and back.
#[test]
fn roundtrip_optical_velocity() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::VoToVl, &[]);
    spec.add(SpecCvtType::VlToVo, &[]);

    let vo = 5.0e7;
    let result = transform_fwd(&spec, vo);
    assert!(
        (result - vo).abs() / vo.abs() < 1e-12,
        "Round-trip vo->vl->vo: got {result}, expected {vo}"
    );
}

/// Air wavelength: forward and inverse are consistent.
#[test]
fn roundtrip_air_wavelength() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToAw, &[]);
    spec.add(SpecCvtType::AwToFr, &[]);

    let freq = 5.0e14; // visible light
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-10,
        "Round-trip freq->air_wv->freq: got {result}, expected {freq}"
    );
}

/// Frequency to relativistic velocity (FrToVl).
#[test]
fn freq_to_velocity() {
    let rest_freq = 1.42e9; // 21cm line
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToVl, &[rest_freq]);

    // At rest frequency, velocity should be 0
    let vel = transform_fwd(&spec, rest_freq);
    assert!(
        vel.abs() < 1.0e-5,
        "FrToVl at rest freq should be ~0, got {vel}"
    );

    // At half the rest frequency:
    // v = c * (rf^2 - f^2) / (rf^2 + f^2) = c * (1 - 0.25) / (1 + 0.25) = c * 0.6
    let vel2 = transform_fwd(&spec, rest_freq / 2.0);
    let expected = C_LIGHT * 0.6;
    assert!(
        (vel2 - expected).abs() / expected < 1e-10,
        "FrToVl at rf/2: got {vel2}, expected {expected}"
    );
}

/// Velocity to frequency (VlToFr).
#[test]
fn velocity_to_freq() {
    let rest_freq = 1.42e9;
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::VlToFr, &[rest_freq]);

    // At zero velocity, should give rest frequency
    let freq = transform_fwd(&spec, 0.0);
    assert!(
        (freq - rest_freq).abs() / rest_freq < 1e-12,
        "VlToFr at v=0: got {freq}, expected {rest_freq}"
    );
}

/// LSRK correction: non-zero Doppler shift for arbitrary RA/Dec.
#[test]
fn lsrk_correction() {
    let ra = std::f64::consts::PI / 4.0; // 45 degrees
    let dec = std::f64::consts::PI / 6.0; // 30 degrees

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::LkF2Hl, &[ra, dec]);

    let freq = 1.42e9;
    let shifted = transform_fwd(&spec, freq);

    // The shift should be non-zero (LSR correction exists for any position)
    assert!(
        (shifted - freq).abs() > 1.0,
        "LSRK correction should produce non-zero shift, got diff = {}",
        (shifted - freq).abs()
    );

    // The shift should be small compared to the frequency (< 0.1%)
    assert!(
        (shifted - freq).abs() / freq < 0.001,
        "LSRK correction too large: {:.6e}",
        (shifted - freq).abs() / freq
    );
}

/// LSRK round-trip: LkF2Hl then HlF2Lk should be identity.
#[test]
fn lsrk_roundtrip() {
    let ra = 1.0;
    let dec = 0.5;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::LkF2Hl, &[ra, dec]);
    spec.add(SpecCvtType::HlF2Lk, &[ra, dec]);

    let freq = 1.42e9;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "LSRK round-trip: got {result}, expected {freq}, diff = {}",
        (result - freq).abs()
    );
}

/// LSRD round-trip.
#[test]
fn lsrd_roundtrip() {
    let ra = 2.0;
    let dec = -0.3;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::LdF2Hl, &[ra, dec]);
    spec.add(SpecCvtType::HlF2Ld, &[ra, dec]);

    let freq = 5.0e14;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "LSRD round-trip: got {result}, expected {freq}"
    );
}

/// Local Group round-trip.
#[test]
fn lg_roundtrip() {
    let ra = 0.5;
    let dec = 0.8;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::LgF2Hl, &[ra, dec]);
    spec.add(SpecCvtType::HlF2Lg, &[ra, dec]);

    let freq = 1.0e10;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "LG round-trip: got {result}, expected {freq}"
    );
}

/// Galactic round-trip.
#[test]
fn galactic_roundtrip() {
    let ra = 3.0;
    let dec = -1.0;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::GlF2Hl, &[ra, dec]);
    spec.add(SpecCvtType::HlF2Gl, &[ra, dec]);

    let freq = 2.0e9;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "Galactic round-trip: got {result}, expected {freq}"
    );
}

/// Geocentric round-trip.
#[test]
fn geocentric_roundtrip() {
    let epoch = 53000.0; // MJD
    let ra = 1.5;
    let dec = 0.3;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::GeF2Hl, &[epoch, ra, dec]);
    spec.add(SpecCvtType::HlF2Ge, &[epoch, ra, dec]);

    let freq = 1.42e9;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "Geocentric round-trip: got {result}, expected {freq}"
    );
}

/// Barycentric round-trip.
#[test]
fn barycentric_roundtrip() {
    let epoch = 53000.0;
    let ra = 2.5;
    let dec = -0.5;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::ByF2Hl, &[epoch, ra, dec]);
    spec.add(SpecCvtType::HlF2By, &[epoch, ra, dec]);

    let freq = 1.42e9;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "Barycentric round-trip: got {result}, expected {freq}"
    );
}

/// Topocentric round-trip.
#[test]
fn topocentric_roundtrip() {
    let obslon = -0.1; // radians east
    let obslat = 0.9; // radians
    let obsalt = 100.0; // metres
    let epoch = 53000.0; // MJD
    let ra = 1.0;
    let dec = 0.5;

    let mut spec = SpecMap::new();
    spec.add(
        SpecCvtType::TpF2Hl,
        &[obslon, obslat, obsalt, epoch, ra, dec],
    );
    spec.add(
        SpecCvtType::HlF2Tp,
        &[obslon, obslat, obsalt, epoch, ra, dec],
    );

    let freq = 1.42e9;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-10,
        "Topocentric round-trip: got {result}, expected {freq}, diff = {:.3e}",
        (result - freq).abs() / freq
    );
}

/// User-defined frame round-trip.
#[test]
fn user_defined_roundtrip() {
    let voff = 30000.0; // 30 km/s in m/s
    let ra = 1.0;
    let dec = 0.5;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::UsF2Hl, &[voff, ra, dec]);
    spec.add(SpecCvtType::HlF2Us, &[voff, ra, dec]);

    let freq = 1.42e9;
    let result = transform_fwd(&spec, freq);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "User-defined round-trip: got {result}, expected {freq}"
    );
}

/// Inversion: inverted SpecMap should produce inverse transform.
#[test]
fn inversion() {
    let rest_freq = 1.42e9;
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToVl, &[rest_freq]);

    let freq = 1.41e9;

    // Forward
    let vel = transform_fwd(&spec, freq);

    // Inverse should recover original
    let recovered = transform_inv(&spec, vel);
    assert!(
        (recovered - freq).abs() / freq < 1e-12,
        "Inversion: got {recovered}, expected {freq}"
    );

    // Invert the map and check forward = old inverse
    let mut spec2 = spec.clone();
    spec2.invert();
    let result = transform_fwd(&spec2, vel);
    assert!(
        (result - freq).abs() / freq < 1e-12,
        "Inverted forward: got {result}, expected {freq}"
    );
}

/// Multi-step chain: freq -> wavelength -> air_wavelength works.
#[test]
fn multistep_chain() {
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWv, &[]); // freq -> vacuum wavelength
    // vacuum wavelength is NOT directly available as air wavelength conversion...
    // but we can go freq -> air wavelength via FrToAw
    let mut spec2 = SpecMap::new();
    spec2.add(SpecCvtType::FrToAw, &[]);

    let freq = 5.0e14;
    let air_wv = transform_fwd(&spec2, freq);
    let vac_wv = transform_fwd(&spec, freq);

    // Air wavelength should be slightly less than vacuum wavelength (n > 1)
    assert!(
        air_wv < vac_wv,
        "Air wavelength ({air_wv}) should be less than vacuum ({vac_wv})"
    );
    // Difference should be small (refractive index ~ 1.0003 for visible)
    let ratio = vac_wv / air_wv;
    assert!(
        (ratio - 1.0).abs() < 0.001,
        "Refractive index ratio: {ratio}"
    );
}

/// Serialisation round-trip.
#[test]
fn serialisation_roundtrip() {
    use ast_core::{AnyObject, from_native_string, to_native_string};

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWv, &[]);
    spec.add(SpecCvtType::WvToFr, &[]);
    spec.add(SpecCvtType::FrToVl, &[1.42e9]);
    spec.add(SpecCvtType::LkF2Hl, &[1.0, 0.5]);

    let native = to_native_string(&AnyObject::SpecMap(spec.clone())).unwrap();

    // Parse back
    let obj = from_native_string(&native).unwrap();
    let spec2 = match obj {
        AnyObject::SpecMap(s) => s,
        _ => panic!("Expected SpecMap from deserialisation"),
    };

    // Should have same number of conversions
    assert_eq!(spec.len(), spec2.len());

    // Should produce same results
    let freq = 1.42e9;
    let r1 = transform_fwd(&spec, freq);
    let r2 = transform_fwd(&spec2, freq);
    assert!(
        (r1 - r2).abs() < 1e-6,
        "Serialisation round-trip: {r1} vs {r2}"
    );
}

/// SpecCvtType::from_code and code_string are consistent.
#[test]
fn code_roundtrip() {
    for i in 1..=34 {
        let cvt = SpecCvtType::from_int(i).unwrap();
        let code = cvt.code_string();
        let recovered = SpecCvtType::from_code(code).unwrap();
        assert_eq!(cvt, recovered, "Code round-trip failed for {code}");
    }
}

/// SpecCvtType::inverse is consistent.
#[test]
fn inverse_pairs() {
    for i in 1..=34 {
        let cvt = SpecCvtType::from_int(i).unwrap();
        let inv = cvt.inverse();
        let inv_inv = inv.inverse();
        assert_eq!(cvt, inv_inv, "Double inverse failed for {:?}", cvt);
    }
}

/// BAD_VALUE input propagates to BAD_VALUE output.
#[test]
fn bad_value_propagation() {
    use ast_core::constants::BAD_VALUE;

    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWv, &[]);

    let result = transform_fwd(&spec, BAD_VALUE);
    assert_eq!(result, BAD_VALUE, "BAD input should produce BAD output");
}

/// Equality test.
#[test]
fn equality() {
    let mut spec1 = SpecMap::new();
    spec1.add(SpecCvtType::FrToWv, &[]);
    spec1.add(SpecCvtType::LkF2Hl, &[1.0, 0.5]);

    let mut spec2 = SpecMap::new();
    spec2.add(SpecCvtType::FrToWv, &[]);
    spec2.add(SpecCvtType::LkF2Hl, &[1.0, 0.5]);

    assert!(spec1.equal(&spec2));

    // Different args
    let mut spec3 = SpecMap::new();
    spec3.add(SpecCvtType::FrToWv, &[]);
    spec3.add(SpecCvtType::LkF2Hl, &[2.0, 0.5]);

    assert!(!spec1.equal(&spec3));
}

/// Adjacent SpecMaps should always merge (concatenating step lists) even when
/// no steps cancel. This matches C specmap.c:2259-2288 and enables future
/// simplification when a third SpecMap appears.
#[test]
fn specmap_always_merges_adjacent() {
    // Two SpecMaps with non-cancelling steps should still merge.
    let mut sm1 = SpecMap::new();
    sm1.add(SpecCvtType::FrToWv, &[]);
    let mut sm2 = SpecMap::new();
    sm2.add(SpecCvtType::VrToVl, &[]);

    let merged = sm1.try_merge(&sm2, true, false, false);
    assert!(merged.is_some(), "Adjacent SpecMaps should always merge");
    // Result should be a SpecMap with both steps.
    let m = merged.unwrap();
    assert!(m.as_any().is::<SpecMap>());
}
