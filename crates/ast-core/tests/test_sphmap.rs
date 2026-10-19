//! Integration tests for SphMap.
//!
//! Tests the Cartesian ↔ spherical coordinate mapping, including
//! forward/inverse transforms, BAD propagation, attribute access,
//! inversion, and serialisation round-trips.

use crate::common::native_dump;
use std::f64::consts::{FRAC_PI_2, PI};

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, SphMap};
use ast_core::{AnyObject, from_native_string};

const TOL: f64 = 1.0e-10;

fn assert_close(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() < tol,
        "expected {a} ~= {b} (diff = {})",
        (a - b).abs()
    );
}

// -----------------------------------------------------------------------
// Basic properties
// -----------------------------------------------------------------------

#[test]
fn dimensions_default() {
    let m = SphMap::new();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 2);
    assert!(!m.is_inverted());
}

#[test]
fn dimensions_inverted() {
    let mut m = SphMap::new();
    m.invert();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 3);
    assert!(m.is_inverted());
}

#[test]
fn attributes() {
    let mut m = SphMap::new();
    assert!(!m.unit_radius());
    assert_eq!(m.polar_long(), 0.0);

    m.set_unit_radius(true);
    m.set_polar_long(1.23);
    assert!(m.unit_radius());
    assert_close(m.polar_long(), 1.23, TOL);
}

// -----------------------------------------------------------------------
// Forward: Cartesian -> Spherical
// -----------------------------------------------------------------------

#[test]
fn forward_axes() {
    let m = SphMap::new();
    let pts = PointSet::from_rows([
        [1.0, 0.0, 0.0],  // +x
        [0.0, 1.0, 0.0],  // +y
        [-1.0, 0.0, 0.0], // -x
        [0.0, -1.0, 0.0], // -y
    ]);
    let out = m.transform(&pts, true).unwrap();
    // +x -> lon=0, lat=0
    assert_close(out.point(0)[0], 0.0, TOL);
    assert_close(out.point(0)[1], 0.0, TOL);
    // +y -> lon=pi/2, lat=0
    assert_close(out.point(1)[0], FRAC_PI_2, TOL);
    assert_close(out.point(1)[1], 0.0, TOL);
    // -x -> lon=pi, lat=0
    assert_close(out.point(2)[0], PI, TOL);
    assert_close(out.point(2)[1], 0.0, TOL);
    // -y -> lon=-pi/2, lat=0
    assert_close(out.point(3)[0], -FRAC_PI_2, TOL);
    assert_close(out.point(3)[1], 0.0, TOL);
}

#[test]
fn forward_poles() {
    let m = SphMap::new();
    let pts = PointSet::from_rows([
        [0.0, 0.0, 1.0],  // north pole
        [0.0, 0.0, -1.0], // south pole
    ]);
    let out = m.transform(&pts, true).unwrap();
    // North pole: lon = polar_long (0), lat = pi/2
    assert_close(out.point(0)[0], 0.0, TOL);
    assert_close(out.point(0)[1], FRAC_PI_2, TOL);
    // South pole: lon = polar_long (0), lat = -pi/2
    assert_close(out.point(1)[0], 0.0, TOL);
    assert_close(out.point(1)[1], -FRAC_PI_2, TOL);
}

#[test]
fn forward_pole_custom_longitude() {
    let mut m = SphMap::new();
    m.set_polar_long(2.5);
    let pts = PointSet::from_rows([[0.0, 0.0, 1.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 2.5, TOL);
    assert_close(out.point(0)[1], FRAC_PI_2, TOL);
}

#[test]
fn forward_non_unit_vector() {
    // (2, 0, 0) -> same direction as (1, 0, 0) -> (0, 0)
    let m = SphMap::new();
    let pts = PointSet::from_rows([[2.0, 0.0, 0.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 0.0, TOL);
    assert_close(out.point(0)[1], 0.0, TOL);
}

// -----------------------------------------------------------------------
// Inverse: Spherical -> Cartesian
// -----------------------------------------------------------------------

#[test]
fn inverse_unit_vectors() {
    let m = SphMap::new();
    let pts = PointSet::from_rows([
        [0.0, 0.0],        // -> (1, 0, 0)
        [FRAC_PI_2, 0.0],  // -> (0, 1, 0)
        [0.0, FRAC_PI_2],  // -> (0, 0, 1)
        [0.0, -FRAC_PI_2], // -> (0, 0, -1)
    ]);
    let out = m.transform(&pts, false).unwrap();

    assert_close(out.point(0)[0], 1.0, TOL);
    assert_close(out.point(0)[1], 0.0, TOL);
    assert_close(out.point(0)[2], 0.0, TOL);

    assert_close(out.point(1)[0], 0.0, TOL);
    assert_close(out.point(1)[1], 1.0, TOL);
    assert_close(out.point(1)[2], 0.0, TOL);

    assert_close(out.point(2)[0], 0.0, TOL);
    assert_close(out.point(2)[1], 0.0, TOL);
    assert_close(out.point(2)[2], 1.0, TOL);

    assert_close(out.point(3)[0], 0.0, TOL);
    assert_close(out.point(3)[1], 0.0, TOL);
    assert_close(out.point(3)[2], -1.0, TOL);
}

// -----------------------------------------------------------------------
// Round-trip
// -----------------------------------------------------------------------

#[test]
fn round_trip_forward_then_inverse() {
    let m = SphMap::new();
    // Start with a unit vector
    let v = [0.6, 0.8, 0.0];
    let pts_cart = PointSet::from_rows([v]);
    let sph = m.transform(&pts_cart, true).unwrap();
    let cart = m.transform(&sph, false).unwrap();
    assert_close(cart.point(0)[0], v[0], TOL);
    assert_close(cart.point(0)[1], v[1], TOL);
    assert_close(cart.point(0)[2], v[2], TOL);
}

#[test]
fn round_trip_inverse_then_forward() {
    let m = SphMap::new();
    let lon = 1.234;
    let lat = 0.567;
    let pts_sph = PointSet::from_rows([[lon, lat]]);
    let cart = m.transform(&pts_sph, false).unwrap();
    let sph = m.transform(&cart, true).unwrap();
    assert_close(sph.point(0)[0], lon, TOL);
    assert_close(sph.point(0)[1], lat, TOL);
}

// -----------------------------------------------------------------------
// BAD propagation
// -----------------------------------------------------------------------

#[test]
fn bad_forward() {
    let m = SphMap::new();
    let pts = PointSet::from_rows([
        [BAD_VALUE, 1.0, 0.0],
        [1.0, BAD_VALUE, 0.0],
        [1.0, 0.0, BAD_VALUE],
    ]);
    let out = m.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert_eq!(out.point(i)[0], BAD_VALUE);
        assert_eq!(out.point(i)[1], BAD_VALUE);
    }
}

#[test]
fn bad_inverse() {
    let m = SphMap::new();
    let pts = PointSet::from_rows([[BAD_VALUE, 0.5], [0.5, BAD_VALUE]]);
    let out = m.transform(&pts, false).unwrap();
    for i in 0..2 {
        assert_eq!(out.point(i)[0], BAD_VALUE);
        assert_eq!(out.point(i)[1], BAD_VALUE);
        assert_eq!(out.point(i)[2], BAD_VALUE);
    }
}

// -----------------------------------------------------------------------
// Inverted mapping
// -----------------------------------------------------------------------

#[test]
fn inverted_forward_is_spherical_to_cartesian() {
    let mut m = SphMap::new();
    m.invert();
    // forward on an inverted SphMap is the inverse transform
    // (spherical -> cartesian)
    let pts = PointSet::from_rows([[0.0, 0.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 1.0, TOL);
    assert_close(out.point(0)[1], 0.0, TOL);
    assert_close(out.point(0)[2], 0.0, TOL);
}

#[test]
fn inverted_inverse_is_cartesian_to_spherical() {
    let mut m = SphMap::new();
    m.invert();
    // inverse on an inverted SphMap is the forward transform
    // (cartesian -> spherical)
    let pts = PointSet::from_rows([[1.0, 0.0, 0.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 0.0, TOL);
    assert_close(out.point(0)[1], 0.0, TOL);
}

// -----------------------------------------------------------------------
// try_merge: relaxed cancellation conditions
// -----------------------------------------------------------------------

#[test]
fn sphmap_inv_fwd_cancels_without_unit_radius() {
    use ast_core::mappings::UnitMap;

    // Inverse (sph->cart) followed by forward (cart->sph) should cancel
    // even without UnitRadius — palDcs2c always produces unit vectors.
    let s1 = SphMap::new(); // unit_radius=false
    let s2 = SphMap::new(); // unit_radius=false

    // self_inv=true means self is applied inverted (sph->cart = inverse of SphMap)
    // other_inv=false means other is applied forward (cart->sph = forward of SphMap)
    let merged = s1.try_merge(&s2, true, true, false);
    assert!(
        merged.is_some(),
        "inv->fwd should cancel without UnitRadius"
    );
    assert!(merged.unwrap().as_any().is::<UnitMap>());
}

#[test]
fn sphmap_fwd_inv_needs_unit_radius_on_first_only() {
    use ast_core::mappings::UnitMap;

    // Forward (cart->sph) then inverse (sph->cart).
    // Requires UnitRadius on the first (forward) map only.
    let mut s1 = SphMap::new();
    s1.set_unit_radius(true); // first, applied forward — needs UnitRadius
    let s2 = SphMap::new(); // second, applied inverted — no UnitRadius needed

    // self_inv=false means self applied forward (cart->sph)
    // other_inv=true means other applied inverted (sph->cart)
    let merged = s1.try_merge(&s2, true, false, true);
    assert!(
        merged.is_some(),
        "fwd->inv should cancel with UnitRadius on first only"
    );
    assert!(merged.unwrap().as_any().is::<UnitMap>());
}

#[test]
fn sphmap_fwd_inv_fails_without_unit_radius() {
    // Forward without UnitRadius → should NOT cancel.
    let s1 = SphMap::new(); // no UnitRadius (first, forward)
    let s2 = SphMap::new(); // (second, inverted)

    let merged = s1.try_merge(&s2, true, false, true);
    assert!(
        merged.is_none(),
        "fwd->inv should NOT cancel without UnitRadius on first"
    );
}

#[test]
fn sphmap_cancellation_requires_matching_polar_long() {
    // Even inv->fwd should not cancel if PolarLong differs.
    let s1 = SphMap::new(); // polar_long = 0.0
    let mut s2 = SphMap::new();
    s2.set_polar_long(1.0); // polar_long = 1.0

    let merged = s1.try_merge(&s2, true, true, false);
    assert!(
        merged.is_none(),
        "should not cancel with different PolarLong"
    );
}

// -----------------------------------------------------------------------
// Serialisation round-trip
// -----------------------------------------------------------------------

#[test]
fn serialise_default() {
    let m = SphMap::new();
    let s = native_dump(&AnyObject::SphMap(m)).unwrap();
    assert!(s.contains("SphMap"));
    let obj = from_native_string(&s).unwrap();
    let m2 = obj.as_mapping().unwrap();
    assert_eq!(m2.nin(), 3);
    assert_eq!(m2.nout(), 2);
    assert!(!m2.is_inverted());
}

#[test]
fn serialise_with_attributes() {
    let mut m = SphMap::new();
    m.set_unit_radius(true);
    m.set_polar_long(1.5);
    let s = native_dump(&AnyObject::SphMap(m)).unwrap();
    assert!(s.contains("UntRd"));
    assert!(s.contains("PlrLg"));
    let obj = from_native_string(&s).unwrap();
    let m2 = match obj {
        AnyObject::SphMap(s) => s,
        _ => panic!("expected SphMap"),
    };
    assert!(m2.unit_radius());
    assert_close(m2.polar_long(), 1.5, TOL);
}

#[test]
fn serialise_inverted() {
    let mut m = SphMap::new();
    m.invert();
    let s = native_dump(&AnyObject::SphMap(m)).unwrap();
    let obj = from_native_string(&s).unwrap();
    let m2 = obj.as_mapping().unwrap();
    assert!(m2.is_inverted());
    assert_eq!(m2.nin(), 2);
    assert_eq!(m2.nout(), 3);
}

#[test]
fn serialise_transform_equivalence() {
    let mut m = SphMap::new();
    m.set_polar_long(0.7);
    let pts = PointSet::from_rows([[0.3, 0.5, 0.8]]);
    let out1 = m.transform(&pts, true).unwrap();

    let s = native_dump(&AnyObject::SphMap(m)).unwrap();
    let obj = from_native_string(&s).unwrap();
    let m2 = obj.as_mapping().unwrap();
    let out2 = m2.transform(&pts, true).unwrap();

    assert_close(out1.point(0)[0], out2.point(0)[0], TOL);
    assert_close(out1.point(0)[1], out2.point(0)[1], TOL);
}
