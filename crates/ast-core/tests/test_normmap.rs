//! Tests for NormMap — coordinate normalisation mapping.
//!
//! Ported from reference/starlink-ast/ast_tester/testnormmap.c
//! where applicable.

use std::f64::consts::PI;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, SkyFrame, SkySystem};
use ast_core::mappings::{CmpMap, Composition, Mapping, NormMap, UnitMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Helper: check if a value equals BAD_VALUE.
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

#[test]
fn normmap_basic_construction() {
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));
    assert_eq!(nm.nin(), 2);
    assert_eq!(nm.nout(), 2);
    assert!(nm.has_forward());
    assert!(nm.has_inverse());
    assert!(!nm.is_inverted());
}

#[test]
fn normmap_skyframe_normalisation() {
    // SkyFrame norm wraps longitude into [0, 2*PI] and clamps
    // latitude into [-PI/2, PI/2].
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));

    // Test with a longitude value outside [0, 2*PI] and a valid declination.
    let lon_in = 3.0 * PI; // 3*PI should wrap to PI
    let dec_in = 0.25;

    let pts = PointSet::from_rows(vec![vec![lon_in, dec_in]]);
    let result = nm.transform(&pts, true).unwrap();
    let out = result.point(0);

    // After normalisation, longitude should be in [0, 2*PI].
    assert!(out[0] >= 0.0 && out[0] <= 2.0 * PI, "lon = {}", out[0]);
    // Check that 3*PI wraps to PI.
    assert!((out[0] - PI).abs() < 1e-10, "expected PI, got {}", out[0]);
    // Declination within range should be unchanged.
    assert!(
        (out[1] - dec_in).abs() < 1e-10,
        "expected {dec_in}, got {}",
        out[1]
    );
}

#[test]
fn normmap_forward_equals_inverse() {
    // Both forward and inverse perform the same normalisation.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));

    let lon_in = -0.5; // Negative longitude
    let dec_in = 0.3;

    let pts = PointSet::from_rows(vec![vec![lon_in, dec_in]]);
    let fwd = nm.transform(&pts, true).unwrap();
    let inv = nm.transform(&pts, false).unwrap();

    // Both should produce the same output.
    let fwd_pt = fwd.point(0);
    let inv_pt = inv.point(0);
    assert!(
        (fwd_pt[0] - inv_pt[0]).abs() < 1e-15,
        "fwd[0]={}, inv[0]={}",
        fwd_pt[0],
        inv_pt[0]
    );
    assert!(
        (fwd_pt[1] - inv_pt[1]).abs() < 1e-15,
        "fwd[1]={}, inv[1]={}",
        fwd_pt[1],
        inv_pt[1]
    );
}

#[test]
fn normmap_bad_value_propagation() {
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));

    // If any coordinate is BAD, the whole point should be passed through unchanged.
    let pts = PointSet::from_rows(vec![
        vec![BAD_VALUE, 0.5],
        vec![1.0, BAD_VALUE],
        vec![BAD_VALUE, BAD_VALUE],
    ]);

    let result = nm.transform(&pts, true).unwrap();

    // Point 0: longitude is BAD
    assert!(is_bad(result.point(0)[0]));
    // Point 1: declination is BAD
    assert!(is_bad(result.point(1)[1]));
    // Point 2: both BAD
    assert!(is_bad(result.point(2)[0]));
    assert!(is_bad(result.point(2)[1]));
}

#[test]
fn normmap_basic_frame_simplifies_to_unit() {
    // A NormMap with a basic Frame (whose norm is identity) simplifies to UnitMap.
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let nm = NormMap::new(Box::new(frame));

    let simplified = nm.simplified().unwrap();
    assert!(
        simplified.as_any().downcast_ref::<UnitMap>().is_some(),
        "expected UnitMap, got {:?}",
        simplified
    );
    assert_eq!(simplified.nin(), 3);
    assert_eq!(simplified.nout(), 3);
}

#[test]
fn normmap_inverse_cancel() {
    // NormMap(fwd) + NormMap(inv) with equal frames → UnitMap.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm1 = NormMap::new(Box::new(sky.clone()));
    let mut nm2 = NormMap::new(Box::new(sky));
    nm2.invert();

    // Compose in series.
    let cmp = CmpMap::new(Box::new(nm1), Box::new(nm2), Composition::Series).unwrap();
    let simplified = cmp.simplified().unwrap();
    assert!(
        simplified.as_any().downcast_ref::<UnitMap>().is_some(),
        "expected UnitMap, got {:?}",
        simplified
    );
}

#[test]
fn normmap_duplicate_elimination() {
    // Two adjacent NormMaps with same invert state and equal frames →
    // single NormMap (the duplicate is eliminated via UnitMap merge).
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm1 = NormMap::new(Box::new(sky.clone()));
    let nm2 = NormMap::new(Box::new(sky));

    let cmp = CmpMap::new(Box::new(nm1), Box::new(nm2), Composition::Series).unwrap();
    let simplified = cmp.simplified().unwrap();

    // The result should be a single NormMap (one was replaced by UnitMap,
    // then NormMap + UnitMap collapses to just NormMap).
    assert!(
        simplified.as_any().downcast_ref::<NormMap>().is_some(),
        "expected NormMap, got {:?}",
        simplified
    );
}

#[test]
fn normmap_equal() {
    let sky1 = SkyFrame::new(SkySystem::Icrs);
    let sky2 = SkyFrame::new(SkySystem::Icrs);
    let nm1 = NormMap::new(Box::new(sky1));
    let nm2 = NormMap::new(Box::new(sky2));

    assert!(nm1.equal(&nm2));

    // Different invert state → not equal.
    let mut nm3 = NormMap::new(Box::new(SkyFrame::new(SkySystem::Icrs)));
    nm3.invert();
    assert!(!nm1.equal(&nm3));

    // Different frame type → not equal.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nm4 = NormMap::new(Box::new(frame));
    assert!(!nm1.equal(&nm4));
}

#[test]
fn normmap_serialization_roundtrip() {
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));

    let obj = AnyObject::NormMap(nm.clone());
    let text = to_native_string(&obj).unwrap();

    // Check it contains the expected class marker.
    assert!(text.contains("NormMap"));
    assert!(text.contains("SkyFrame"));

    // Round-trip.
    let obj2 = from_native_string(&text).unwrap();
    let nm2 = obj2.into_mapping().unwrap();
    assert!(nm.equal(nm2.as_ref()));
}

#[test]
fn normmap_rate() {
    // NormMap rate: 1.0 for same axis, 0.0 for different axes.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));

    let at = [1.0, 0.5];
    assert_eq!(nm.rate(&at, 1, 1).unwrap(), 1.0);
    assert_eq!(nm.rate(&at, 2, 2).unwrap(), 1.0);
    assert_eq!(nm.rate(&at, 1, 2).unwrap(), 0.0);
    assert_eq!(nm.rate(&at, 2, 1).unwrap(), 0.0);
}

#[test]
fn normmap_inverted_transform() {
    // Even when inverted, both directions do the same normalization.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let mut nm = NormMap::new(Box::new(sky));
    nm.invert();

    let lon_in = 3.0 * PI;
    let dec_in = 0.1;
    let pts = PointSet::from_rows(vec![vec![lon_in, dec_in]]);

    let fwd = nm.transform(&pts, true).unwrap();
    let inv = nm.transform(&pts, false).unwrap();

    // Both should normalise.
    assert!((fwd.point(0)[0] - PI).abs() < 1e-10);
    assert!((inv.point(0)[0] - PI).abs() < 1e-10);
}

#[test]
fn normmap_multiple_points() {
    let sky = SkyFrame::new(SkySystem::Icrs);
    let nm = NormMap::new(Box::new(sky));

    let pts = PointSet::from_rows(vec![
        vec![0.0, 0.0],
        vec![PI, 0.5],
        vec![2.0 * PI + 0.1, -0.3],
        vec![-0.1, 0.0],
    ]);

    let result = nm.transform(&pts, true).unwrap();

    // Point 0: already normalised.
    assert!((result.point(0)[0]).abs() < 1e-10);

    // Point 1: PI is already normalised.
    assert!((result.point(1)[0] - PI).abs() < 1e-10);

    // Point 2: 2*PI+0.1 should wrap to 0.1.
    assert!((result.point(2)[0] - 0.1).abs() < 1e-10);

    // Point 3: -0.1 should wrap to 2*PI - 0.1.
    assert!(
        (result.point(3)[0] - (2.0 * PI - 0.1)).abs() < 1e-10,
        "got {}",
        result.point(3)[0]
    );
}
