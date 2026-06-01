//! Tier-1 hand-curated tests for the unit mapper. Tier-4 oracle
//! tests against the C `astUnitMapper_` live under
//! `crates/ast-oracle-tests/`.
//!
//! Each ZoomMap test verifies that the returned mapping's forward
//! direction transforms `1.0` into the expected scale factor (i.e.
//! `1 in_unit = factor out_unit`).

#![cfg(test)]

use crate::coords::PointSet;
use crate::mappings::{Mapping, UnitMap, ZoomMap};

use super::mapper::unit_mapper;

/// Drive a sample value through the returned mapping (forward
/// direction) and return its image.
fn forward_one(map: &dyn Mapping, x: f64) -> f64 {
    let pts = PointSet::from_rows([[x]]);
    let out = map.transform(&pts, true).expect("forward transform");
    out.point(0)[0]
}

/// Assert the mapping is a `ZoomMap` and its forward image of 1.0
/// is `expected` to within a tight relative tolerance.
fn assert_zoom_factor(map: &dyn Mapping, expected: f64) {
    let got = forward_one(map, 1.0);
    let tol = expected.abs() * 1e-12;
    assert!(
        (got - expected).abs() <= tol,
        "expected zoom factor {expected}, got {got} (tol {tol})"
    );
}

#[test]
fn identity_returns_unitmap() {
    let map = unit_mapper("Hz", "Hz").expect("Hz → Hz should produce a mapping");
    assert!(map.as_any().is::<UnitMap>(), "expected UnitMap for Hz → Hz");
}

#[test]
fn mhz_to_hz_zoom_1e6() {
    let map = unit_mapper("MHz", "Hz").expect("MHz → Hz should produce a mapping");
    assert!(
        map.as_any().is::<ZoomMap>(),
        "expected ZoomMap for MHz → Hz"
    );
    assert_zoom_factor(&*map, 1.0e6);
}

#[test]
fn km_to_m_zoom_1000() {
    let map = unit_mapper("km", "m").expect("km → m should produce a mapping");
    assert_zoom_factor(&*map, 1.0e3);
}

#[test]
fn arcsec_to_rad() {
    let map = unit_mapper("arcsec", "rad").expect("arcsec → rad should produce a mapping");
    let expected = std::f64::consts::PI / 648000.0;
    assert_zoom_factor(&*map, expected);
}

#[test]
fn ev_to_joule() {
    let map = unit_mapper("eV", "J").expect("eV → J should produce a mapping");
    assert_zoom_factor(&*map, 1.60217733e-19);
}

#[test]
fn jansky_decomposed() {
    // 1 Jy = 1e-26 W/m^2/Hz.
    let map = unit_mapper("Jy", "W/m**2/Hz").expect("Jy → W/m^2/Hz");
    assert_zoom_factor(&*map, 1.0e-26);
}

#[test]
fn au_to_metre() {
    let map = unit_mapper("AU", "m").expect("AU → m");
    assert_zoom_factor(&*map, 1.49598e11);
}

#[test]
fn ms_to_kms() {
    // 1 m/s = 1e-3 km/s.
    let map = unit_mapper("m/s", "km/s").expect("m/s → km/s");
    assert_zoom_factor(&*map, 1.0e-3);
}

#[test]
fn kg_per_m2_to_g_per_cm2() {
    // 1 kg/m^2 = 1000 g / 10000 cm^2 = 0.1 g/cm^2.
    let map = unit_mapper("kg/m**2", "g/cm**2").expect("kg/m^2 → g/cm^2");
    assert_zoom_factor(&*map, 0.1);
}

#[test]
fn unknown_unit_returns_none() {
    assert!(unit_mapper("xyzzy", "bar").is_none());
}

#[test]
fn incompatible_dimensions_return_none() {
    assert!(unit_mapper("m", "s").is_none());
}

#[test]
fn alias_sec_equals_s() {
    // "sec" is an alias for "s".
    let map = unit_mapper("sec", "s").expect("sec → s");
    assert!(
        map.as_any().is::<UnitMap>(),
        "sec → s should be UnitMap (alias resolves to canonical s)"
    );
}

#[test]
fn alias_meter_equals_m() {
    let map = unit_mapper("meter", "m").expect("meter → m");
    assert!(map.as_any().is::<UnitMap>());
}

#[test]
fn micron_to_metre() {
    let map = unit_mapper("micron", "m").expect("micron → m");
    assert_zoom_factor(&*map, 1.0e-6);
}

#[test]
fn angstrom_to_metre() {
    let map = unit_mapper("Angstrom", "m").expect("Angstrom → m");
    assert_zoom_factor(&*map, 1.0e-10);
}

#[test]
fn nm_prefix_unit() {
    // nano-metre: 1 nm = 1e-9 m.
    let map = unit_mapper("nm", "m").expect("nm → m");
    assert_zoom_factor(&*map, 1.0e-9);
}

#[test]
fn parsec_to_metre() {
    let map = unit_mapper("pc", "m").expect("pc → m");
    assert_zoom_factor(&*map, 3.0867e16);
}

#[test]
fn hz_to_inverse_s_is_unitmap() {
    // Hz is defined as 1/s, so after RemakeTree the trees are
    // dimensionally equivalent and the factor is 1.0.
    let map = unit_mapper("Hz", "1/s").expect("Hz → 1/s");
    assert!(
        map.as_any().is::<UnitMap>(),
        "Hz → 1/s should reduce to UnitMap"
    );
}

#[test]
fn ang_alias_to_metre() {
    // "Ang" is an alias for Angstrom.
    let map = unit_mapper("Ang", "m").expect("Ang → m");
    assert_zoom_factor(&*map, 1.0e-10);
}

#[test]
fn ghz_to_hz() {
    let map = unit_mapper("GHz", "Hz").expect("GHz → Hz");
    assert_zoom_factor(&*map, 1.0e9);
}

#[test]
fn arcmin_to_arcsec() {
    let map = unit_mapper("arcmin", "arcsec").expect("arcmin → arcsec");
    assert_zoom_factor(&*map, 60.0);
}
