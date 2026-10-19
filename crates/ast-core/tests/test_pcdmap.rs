//! Tests for PcdMap (pincushion/barrel distortion mapping).
//!
//! Ancestor: reference/starlink-ast/src/pcdmap.c

use crate::common::native_dump;
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{CmpMap, Composition, DynMapping, Mapping, PcdMap, UnitMap, ZoomMap};
use ast_core::{AnyObject, from_native_string};

// ------------------------------------------------------------------ //
// Construction                                                         //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_new_basic() {
    let pm = PcdMap::new(0.001, [100.0, 200.0]);
    assert_eq!(pm.nin(), 2);
    assert_eq!(pm.nout(), 2);
    assert!(!pm.is_inverted());
    assert!((pm.disc() - 0.001).abs() < 1e-15);
    assert!((pm.pcdcen()[0] - 100.0).abs() < 1e-15);
    assert!((pm.pcdcen()[1] - 200.0).abs() < 1e-15);
}

#[test]
fn pcdmap_zero_disc() {
    let pm = PcdMap::new(0.0, [0.0, 0.0]);
    assert_eq!(pm.disc(), 0.0);
}

// ------------------------------------------------------------------ //
// Forward transform                                                    //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_forward_at_centre() {
    // At the distortion centre, output == input regardless of disc.
    let pm = PcdMap::new(0.5, [10.0, 20.0]);
    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 10.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 20.0).abs() < 1e-12);
}

#[test]
fn pcdmap_forward_known_point() {
    // disc = 0.001, centre at origin
    // input (10, 0): dx=10, dy=0, r^2=100, f=1+0.001*100=1.1
    // output: (10*1.1, 0*1.1) = (11, 0)
    let pm = PcdMap::new(0.001, [0.0, 0.0]);
    let pts = PointSet::from_rows([[10.0, 0.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 11.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 0.0).abs() < 1e-12);
}

#[test]
fn pcdmap_forward_with_centre() {
    // disc = 0.01, centre at (5, 5)
    // input (8, 5): dx=3, dy=0, r^2=9, f=1+0.01*9=1.09
    // output: (3*1.09 + 5, 0*1.09 + 5) = (8.27, 5.0)
    let pm = PcdMap::new(0.01, [5.0, 5.0]);
    let pts = PointSet::from_rows([[8.0, 5.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 8.27).abs() < 1e-12);
    assert!((out.point(0)[1] - 5.0).abs() < 1e-12);
}

#[test]
fn pcdmap_forward_bad_value() {
    let pm = PcdMap::new(0.001, [0.0, 0.0]);
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

// ------------------------------------------------------------------ //
// Inverse transform (approximate)                                      //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_inverse_roundtrip() {
    // Forward then inverse should approximately recover the input.
    let pm = PcdMap::new(0.001, [1.0, -2.0]);
    let pts = PointSet::from_rows([[5.0, 3.0], [10.0, -7.0], [0.5, 0.5]]);
    let fwd = pm.transform(&pts, true).unwrap();
    let inv = pm.transform(&fwd, false).unwrap();
    for i in 0..pts.npoint() {
        let orig = pts.point(i);
        let recovered = inv.point(i);
        // The SLA_UNPCD approximation is not exact. For disc=0.001 and
        // points up to r~12 from centre, errors can be O(1e-5).
        // This is still negligible for astrometric purposes.
        let tol = 1e-4;
        assert!(
            (recovered[0] - orig[0]).abs() < tol,
            "axis 0, point {i}: {:.15} vs {:.15}",
            recovered[0],
            orig[0]
        );
        assert!(
            (recovered[1] - orig[1]).abs() < tol,
            "axis 1, point {i}: {:.15} vs {:.15}",
            recovered[1],
            orig[1]
        );
    }
}

#[test]
fn pcdmap_inverse_bad_value() {
    let pm = PcdMap::new(0.001, [0.0, 0.0]);
    let pts = PointSet::from_rows([[5.0, BAD_VALUE]]);
    let out = pm.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

// ------------------------------------------------------------------ //
// Inverted flag                                                        //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_invert_swaps_direction() {
    let mut pm = PcdMap::new(0.001, [0.0, 0.0]);
    let pts = PointSet::from_rows([[10.0, 0.0]]);

    // Forward without inversion: (10, 0) -> (11, 0).
    let fwd = pm.transform(&pts, true).unwrap();
    assert!((fwd.point(0)[0] - 11.0).abs() < 1e-12);

    // After inverting, forward applies the old inverse.
    pm.invert();
    assert!(pm.is_inverted());
    let inv_fwd = pm.transform(&pts, true).unwrap();
    // The inverse of (10, 0) should be different from (11, 0).
    assert!((inv_fwd.point(0)[0] - 11.0).abs() > 1e-4);
}

// ------------------------------------------------------------------ //
// equal()                                                              //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_equal_same() {
    let a = PcdMap::new(0.001, [1.0, 2.0]);
    let b = PcdMap::new(0.001, [1.0, 2.0]);
    assert!(a.equal(&b));
}

#[test]
fn pcdmap_equal_different_disc() {
    let a = PcdMap::new(0.001, [1.0, 2.0]);
    let b = PcdMap::new(0.002, [1.0, 2.0]);
    assert!(!a.equal(&b));
}

#[test]
fn pcdmap_equal_different_centre() {
    let a = PcdMap::new(0.001, [1.0, 2.0]);
    let b = PcdMap::new(0.001, [1.0, 3.0]);
    assert!(!a.equal(&b));
}

#[test]
fn pcdmap_equal_different_inversion() {
    let a = PcdMap::new(0.001, [1.0, 2.0]);
    let mut b = PcdMap::new(0.001, [1.0, 2.0]);
    b.invert();
    assert!(!a.equal(&b));
}

#[test]
fn pcdmap_not_equal_to_unitmap() {
    let a = PcdMap::new(0.001, [0.0, 0.0]);
    let b = UnitMap::new(2);
    assert!(!a.equal(&b));
}

// ------------------------------------------------------------------ //
// simplified()                                                         //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_disc_zero_simplifies_to_unitmap() {
    let pm = PcdMap::new(0.0, [5.0, 3.0]);
    let simp = <dyn Mapping>::simplify(&pm).unwrap();
    assert!(simp.as_any().downcast_ref::<UnitMap>().is_some());
    assert_eq!(simp.nin(), 2);
}

#[test]
fn pcdmap_nonzero_disc_stays_pcdmap() {
    let pm = PcdMap::new(0.001, [0.0, 0.0]);
    let simp = <dyn Mapping>::simplify(&pm).unwrap();
    assert!(simp.as_any().downcast_ref::<PcdMap>().is_some());
}

// ------------------------------------------------------------------ //
// try_merge: inverse cancellation                                      //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_merge_with_inverse_cancels() {
    let pm = PcdMap::new(0.001, [1.25, -0.75]);
    // self_inv=false, other_inv=true means they are applied in opposite directions.
    let result = pm.try_merge(&pm, true, false, true);
    assert!(result.is_some());
    let merged = result.unwrap();
    assert!(merged.as_any().downcast_ref::<UnitMap>().is_some());
}

#[test]
fn pcdmap_merge_same_direction_no_cancel() {
    let pm = PcdMap::new(0.001, [1.0, 2.0]);
    // Both forward — should NOT cancel.
    let result = pm.try_merge(&pm, true, false, false);
    assert!(result.is_none());
}

#[test]
fn pcdmap_merge_different_disc_no_cancel() {
    let a = PcdMap::new(0.001, [0.0, 0.0]);
    let b = PcdMap::new(0.002, [0.0, 0.0]);
    let result = a.try_merge(&b, true, false, true);
    assert!(result.is_none());
}

// ------------------------------------------------------------------ //
// try_merge: with UnitMap                                              //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_merge_with_unitmap() {
    let pm = PcdMap::new(0.001, [100.0, 100.0]);
    let um = UnitMap::new(2);
    let result = pm.try_merge(&um, true, false, false);
    assert!(result.is_some());
    let merged = result.unwrap();
    let merged_pcd = merged.as_any().downcast_ref::<PcdMap>().unwrap();
    assert!((merged_pcd.disc() - 0.001).abs() < 1e-15);
}

// ------------------------------------------------------------------ //
// Serialization roundtrip                                              //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_serialization_roundtrip() {
    let pm = PcdMap::new(0.001, [1.25, -0.75]);
    let obj = AnyObject::PcdMap(pm.clone());
    let text = native_dump(&obj).unwrap();
    let obj2 = from_native_string(&text).unwrap();
    let mapping = obj2.into_mapping().unwrap();
    let pm2 = mapping.as_any().downcast_ref::<PcdMap>().unwrap();
    assert!(pm.equal(pm2));
}

#[test]
fn pcdmap_serialization_inverted() {
    let mut pm = PcdMap::new(0.005, [10.0, 20.0]);
    pm.invert();
    let obj = AnyObject::PcdMap(pm.clone());
    let text = native_dump(&obj).unwrap();
    let obj2 = from_native_string(&text).unwrap();
    let mapping = obj2.into_mapping().unwrap();
    let pm2 = mapping.as_any().downcast_ref::<PcdMap>().unwrap();
    assert!(pm.equal(pm2));
    assert!(pm2.is_inverted());
}

#[test]
fn pcdmap_serialization_zero_disc() {
    // A PcdMap with disc=0 should round-trip through serialization.
    let pm = PcdMap::new(0.0, [0.0, 0.0]);
    let obj = AnyObject::PcdMap(pm.clone());
    let text = native_dump(&obj).unwrap();
    let obj2 = from_native_string(&text).unwrap();
    let mapping = obj2.into_mapping().unwrap();
    let pm2 = mapping.as_any().downcast_ref::<PcdMap>().unwrap();
    assert!(pm.equal(pm2));
}

// ------------------------------------------------------------------ //
// Deserialization from reference fixture                                //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_deserialize_fixture() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/simplify/pcd_zero_to_unit.map"
    ))
    .unwrap();
    let obj = from_native_string(&text).unwrap();
    let mapping = obj.into_mapping().unwrap();
    let pm = mapping.as_any().downcast_ref::<PcdMap>().unwrap();
    assert!((pm.disc() - 0.0).abs() < 1e-15);
    assert!((pm.pcdcen()[0] - 0.0).abs() < 1e-15);
    assert!((pm.pcdcen()[1] - 0.0).abs() < 1e-15);
}

// ------------------------------------------------------------------ //
// Multi-point transform                                                //
// ------------------------------------------------------------------ //

#[test]
fn pcdmap_multiple_points() {
    let pm = PcdMap::new(0.001, [0.0, 0.0]);
    let pts = PointSet::from_rows([[0.0, 0.0], [10.0, 0.0], [0.0, 10.0], [10.0, 10.0]]);
    let out = pm.transform(&pts, true).unwrap();

    // Point at origin: unchanged
    assert!((out.point(0)[0] - 0.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 0.0).abs() < 1e-12);

    // (10, 0): r^2=100, f=1.1, output=(11, 0)
    assert!((out.point(1)[0] - 11.0).abs() < 1e-12);
    assert!((out.point(1)[1] - 0.0).abs() < 1e-12);

    // (0, 10): r^2=100, f=1.1, output=(0, 11)
    assert!((out.point(2)[0] - 0.0).abs() < 1e-12);
    assert!((out.point(2)[1] - 11.0).abs() < 1e-12);

    // (10, 10): r^2=200, f=1.2, output=(12, 12)
    assert!((out.point(3)[0] - 12.0).abs() < 1e-12);
    assert!((out.point(3)[1] - 12.0).abs() < 1e-12);
}

// ------------------------------------------------------------------ //
// Swapping past a ZoomMap                                              //
// ------------------------------------------------------------------ //

/// Swapping a PcdMap past a neighbouring ZoomMap preserves the composition when
/// the ZoomMap is used in the inverse direction.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testmapping.c`,
/// `testpcdzoominverted`.
///
/// The swap sets both Mappings' `Invert` flags to the values the merge list
/// holds before reading their attributes. That is effective for the PcdMap,
/// whose `Disco` and `PcdCen` accessors consult `Invert`, but not for the
/// ZoomMap: `Zoom` is the stored factor whatever `Invert` says, and it is
/// `Transform` that takes the reciprocal. So the factor the list applies has to
/// be formed explicitly, or the swap emits a forward ZoomMap of the original
/// factor and a PcdMap derived from it.
///
/// The trailing inverted PcdMap is what gives the leading one a merge target to
/// swap towards; without it the swap is not attempted.
#[test]
fn pcdmap_swap_past_an_inverted_zoommap_preserves_the_transformation() {
    let centre = [0.3, -0.2];
    let points = PointSet::from_rows([[0.1, 0.2], [-0.25, 0.05]]);

    for zoom in [2.0, 0.5] {
        for inverted in [false, true] {
            let pm1 = PcdMap::new(0.01, centre);
            let mut pm2 = PcdMap::new(0.01, centre);
            pm2.invert();
            let mut zm = ZoomMap::new(2, zoom).unwrap();
            if inverted {
                zm.invert();
            }

            let inner: DynMapping =
                Box::new(CmpMap::new(Box::new(zm), Box::new(pm2), Composition::Series).unwrap());
            let outer: DynMapping =
                Box::new(CmpMap::new(Box::new(pm1), inner, Composition::Series).unwrap());
            let simp = outer.simplify().unwrap();

            let before = outer.transform(&points, true).unwrap();
            let after = simp.transform(&points, true).unwrap();

            let direction = if inverted { "inverted" } else { "forward" };
            for i in 0..points.npoint() {
                let (bx, by) = (before.point(i)[0], before.point(i)[1]);
                let (ax, ay) = (after.point(i)[0], after.point(i)[1]);
                let mag = bx.abs() + by.abs();
                let dev = (ax - bx).abs() + (ay - by).abs();
                assert!(
                    mag == 0.0 || dev <= 1.0e-9 * mag,
                    "zoom {zoom} {direction}: ({}, {}) gave ({bx:.12}, {by:.12}) before \
                     simplification and ({ax:.12}, {ay:.12}) after",
                    points.point(i)[0],
                    points.point(i)[1],
                );
            }
        }
    }
}
