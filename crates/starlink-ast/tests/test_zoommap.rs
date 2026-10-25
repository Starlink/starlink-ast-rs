// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{CmpMap, Composition, Mapping, MatrixMap, UnitMap, ZoomMap};

#[test]
fn zoom_map_dimensions() {
    let m = ZoomMap::new(3, 2.5).unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
}

#[test]
fn zoom_map_zero_factor_rejected() {
    assert!(ZoomMap::new(2, 0.0).is_err());
}

#[test]
fn zoom_map_forward() {
    let m = ZoomMap::new(2, 3.0).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0], [4.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 6.0]);
    assert_eq!(out.point(1), &[12.0, 15.0]);
}

#[test]
fn zoom_map_inverse() {
    let m = ZoomMap::new(2, 4.0).unwrap();
    let pts = PointSet::from_rows([[8.0, 12.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[2.0, 3.0]);
}

#[test]
fn zoom_map_bad_value() {
    let m = ZoomMap::new(2, 2.0).unwrap();
    // Per-axis BAD handling: only the BAD axis is blanked; the other computes.
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 6.0);
}

#[test]
fn zoom_map_inverted_flag() {
    let mut m = ZoomMap::new(2, 2.0).unwrap();
    m.invert();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[2.0, 3.0]);
}

#[test]
fn zoom_map_simplified_identity() {
    let m = ZoomMap::new(2, 1.0).unwrap();
    let s = <dyn Mapping>::simplify(&m).unwrap();
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

#[test]
fn zoom_map_rate() {
    let m = ZoomMap::new(2, 3.0).unwrap();
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 3.0).abs() < 1e-4);
}

#[test]
fn zoom_rate_analytical() {
    let zm = ZoomMap::new(2, 3.0).unwrap();
    assert_eq!(zm.rate(&[1.0, 2.0], 1, 1).unwrap(), 3.0);
    assert_eq!(zm.rate(&[1.0, 2.0], 1, 2).unwrap(), 0.0);
    assert_eq!(zm.rate(&[1.0, 2.0], 2, 2).unwrap(), 3.0);

    let mut zm_inv = ZoomMap::new(2, 3.0).unwrap();
    zm_inv.invert();
    let r = zm_inv.rate(&[1.0, 2.0], 1, 1).unwrap();
    assert!((r - 1.0 / 3.0).abs() < 1e-15);
    assert_eq!(zm_inv.rate(&[1.0, 2.0], 1, 2).unwrap(), 0.0);
}

#[test]
fn zoom_bad_per_axis() {
    let zm = ZoomMap::new(3, 2.0).unwrap();
    // Only axis 1 is BAD; axes 0 and 2 should still compute.
    let pts = PointSet::from_rows([[1.0, BAD_VALUE, 3.0]]);
    let out = zm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
    assert_eq!(out.point(0)[1], BAD_VALUE);
    assert_eq!(out.point(0)[2], 6.0);

    // Inverse too
    let pts_inv = PointSet::from_rows([[BAD_VALUE, 4.0, 6.0]]);
    let out_inv = zm.transform(&pts_inv, false).unwrap();
    assert_eq!(out_inv.point(0)[0], BAD_VALUE);
    assert_eq!(out_inv.point(0)[1], 2.0);
    assert_eq!(out_inv.point(0)[2], 3.0);
}

#[test]
fn zoom_parallel_merge_different_factors() {
    let z1 = ZoomMap::new(1, 2.0).unwrap();
    let z2 = ZoomMap::new(1, 3.0).unwrap();

    let merged = z1.try_merge(&z2, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    // Different factors → diagonal MatrixMap
    assert!(m.as_any().is::<MatrixMap>());
    assert_eq!(m.nin(), 2);

    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 10.0);
    assert_eq!(out.point(0)[1], 21.0);
}

#[test]
fn zoom_parallel_merge_same_factor() {
    let z1 = ZoomMap::new(2, 5.0).unwrap();
    let z2 = ZoomMap::new(1, 5.0).unwrap();

    let merged = z1.try_merge(&z2, false, false, false);
    assert!(merged.is_some());
    // Same factor → should be a ZoomMap with ncoord=3
    let m = merged.unwrap();
    assert!(m.as_any().is::<ZoomMap>());
    assert_eq!(m.nin(), 3);
}

/// `testzoommap.c` Errors 1-8: Zoom is set by construction, reverts to 1.0
/// when cleared, and reads back a new value; composing a copy into a CmpMap
/// leaves the original as it was. C's Errors 9 and 11 (`AST__IMMUT` on a
/// ZoomMap shared with a CmpMap) belong to `starlink-ast-capi`, since a composed
/// Mapping here is a copy rather than a shared pointer.
#[test]
fn zoom_attribute_set_clear_and_get() {
    let mut zm = ZoomMap::new(1, -1.0).unwrap();
    assert!(zm.test_zoom(), "Error 1");
    assert_eq!(zm.zoom(), -1.0, "Error 2");

    zm.clear_zoom();
    assert!(!zm.test_zoom(), "Error 3");
    assert_eq!(zm.zoom(), 1.0, "Error 4");

    zm.set_zoom(2.5).unwrap();
    assert!(zm.test_zoom(), "Error 5");
    assert_eq!(zm.zoom(), 2.5, "Error 6");

    let _cmp = CmpMap::new(
        Box::new(zm.clone()),
        Box::new(UnitMap::new(1)),
        Composition::Series,
    )
    .unwrap();
    assert!(zm.test_zoom(), "Error 7");
    assert_eq!(zm.zoom(), 2.5, "Error 8");
}

/// Two ZoomMaps whose product underflows to zero cannot merge into a ZoomMap,
/// and C passes the constructor's error up from the simplification.
#[test]
fn a_merged_zoom_that_underflows_to_zero_is_an_error() {
    let map = CmpMap::new(
        Box::new(ZoomMap::new(1, 1e-200).unwrap()),
        Box::new(ZoomMap::new(1, 1e-200).unwrap()),
        Composition::Series,
    )
    .unwrap();
    let err = <dyn Mapping>::simplify(&map).unwrap_err();
    assert_eq!(err.code(), starlink_ast::error::ErrorCode::Zoomi);
}
