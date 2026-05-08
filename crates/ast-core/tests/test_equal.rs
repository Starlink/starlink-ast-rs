//! Tests for the `equal` method on Mapping and Frame types.
//!
//! Covers identity equality, value equality, inequality on field
//! differences, invert-aware comparisons, cross-type rejection,
//! recursive compound equality, float tolerance, and BAD_VALUE
//! handling.

use ndarray::array;

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{
    BasicFrameBuilder, CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkyRefIs, SkySystem,
};
use ast_core::mappings::{
    ChebyMap, CmpMap, Composition, LutMap, Mapping, MatrixMap, PermMap, PolyMap, PolyTerm,
    ShiftMap, SlaCvtType, SlaMap, SphMap, UnitMap, WcsMap, WcsProjection, WinMap, ZoomMap,
};

// =========================================================================
// ast_equal tolerance behaviour (via ZoomMap as a vehicle)
// =========================================================================

#[test]
fn float_tolerance_within() {
    // Two ZoomMaps whose factors differ by less than the AST tolerance
    // should be equal.
    let a = ZoomMap::new(2, 1.0).unwrap();
    let b = ZoomMap::new(2, 1.0 + 1.0e-14).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn float_tolerance_outside() {
    // Factors that differ by more than tolerance should be unequal.
    let a = ZoomMap::new(2, 1.0).unwrap();
    let b = ZoomMap::new(2, 1.1).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn bad_value_comparison() {
    // Two PermMaps with BAD_VALUE constants should be equal.
    let a = PermMap::new(&[-1], &[-1], &[BAD_VALUE]).unwrap();
    let b = PermMap::new(&[-1], &[-1], &[BAD_VALUE]).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn bad_vs_non_bad() {
    // BAD_VALUE vs a normal value should be unequal.
    let a = PermMap::new(&[-1], &[-1], &[BAD_VALUE]).unwrap();
    let b = PermMap::new(&[-1], &[-1], &[42.0]).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// UnitMap
// =========================================================================

#[test]
fn unitmap_identity() {
    let a = UnitMap::new(3);
    assert!(a.equal(&a));
}

#[test]
fn unitmap_same_ncoord() {
    let a = UnitMap::new(3);
    let b = UnitMap::new(3);
    assert!(a.equal(&b));
}

#[test]
fn unitmap_different_ncoord() {
    let a = UnitMap::new(3);
    let b = UnitMap::new(2);
    assert!(!a.equal(&b));
}

// =========================================================================
// ZoomMap
// =========================================================================

#[test]
fn zoommap_identity() {
    let a = ZoomMap::new(2, 3.5).unwrap();
    assert!(a.equal(&a));
}

#[test]
fn zoommap_same_values() {
    let a = ZoomMap::new(2, 3.5).unwrap();
    let b = ZoomMap::new(2, 3.5).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn zoommap_different_factor() {
    let a = ZoomMap::new(2, 3.5).unwrap();
    let b = ZoomMap::new(2, 4.0).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn zoommap_different_ncoord() {
    let a = ZoomMap::new(2, 3.5).unwrap();
    let b = ZoomMap::new(3, 3.5).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn zoommap_invert_aware() {
    // ZoomMap(2) inverted should equal ZoomMap(0.5) non-inverted.
    let mut a = ZoomMap::new(2, 2.0).unwrap();
    a.invert();
    let b = ZoomMap::new(2, 0.5).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn zoommap_invert_aware_mismatch() {
    // ZoomMap(3) inverted should NOT equal ZoomMap(0.5) non-inverted.
    let mut a = ZoomMap::new(2, 3.0).unwrap();
    a.invert();
    let b = ZoomMap::new(2, 0.5).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// ShiftMap
// =========================================================================

#[test]
fn shiftmap_identity() {
    let a = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    assert!(a.equal(&a));
}

#[test]
fn shiftmap_same_values() {
    let a = ShiftMap::new(vec![1.0, 2.0]);
    let b = ShiftMap::new(vec![1.0, 2.0]);
    assert!(a.equal(&b));
}

#[test]
fn shiftmap_different_shifts() {
    let a = ShiftMap::new(vec![1.0, 2.0]);
    let b = ShiftMap::new(vec![1.0, 3.0]);
    assert!(!a.equal(&b));
}

#[test]
fn shiftmap_different_length() {
    let a = ShiftMap::new(vec![1.0, 2.0]);
    let b = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    assert!(!a.equal(&b));
}

#[test]
fn shiftmap_invert_aware() {
    // ShiftMap([1, 2]) inverted should equal ShiftMap([-1, -2]) non-inverted.
    let mut a = ShiftMap::new(vec![1.0, 2.0]);
    a.invert();
    let b = ShiftMap::new(vec![-1.0, -2.0]);
    assert!(a.equal(&b));
}

#[test]
fn shiftmap_invert_aware_mismatch() {
    let mut a = ShiftMap::new(vec![1.0, 2.0]);
    a.invert();
    let b = ShiftMap::new(vec![1.0, 2.0]);
    assert!(!a.equal(&b));
}

// =========================================================================
// MatrixMap
// =========================================================================

#[test]
fn matrixmap_same() {
    let m = array![[2.0_f64, 0.0], [0.0, 3.0]];
    let a = MatrixMap::new(m.clone()).unwrap();
    let b = MatrixMap::new(m).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn matrixmap_different_element() {
    let a = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 3.0]]).unwrap();
    let b = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 4.0]]).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn matrixmap_inverted_not_equal() {
    let mut a = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 3.0]]).unwrap();
    let b = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 3.0]]).unwrap();
    a.invert();
    assert!(!a.equal(&b));
}

// =========================================================================
// PermMap
// =========================================================================

#[test]
fn permmap_same() {
    let a = PermMap::new(&[1, 2], &[2, 1], &[]).unwrap();
    let b = PermMap::new(&[1, 2], &[2, 1], &[]).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn permmap_different_perm() {
    let a = PermMap::new(&[1, 2], &[2, 1], &[]).unwrap();
    let b = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// WinMap
// =========================================================================

#[test]
fn winmap_same() {
    let a = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    let b = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn winmap_different_scale() {
    let a = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    let b = WinMap::new(&[1.0, 2.0], &[3.0, 5.0]).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn winmap_inverted_not_equal() {
    let mut a = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    let b = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    a.invert();
    assert!(!a.equal(&b));
}

// =========================================================================
// CmpMap (recursive)
// =========================================================================

#[test]
fn cmpmap_series_same() {
    let a = CmpMap::new(
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        Box::new(ShiftMap::new(vec![1.0, 2.0])),
        Composition::Series,
    )
    .unwrap();
    let b = CmpMap::new(
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        Box::new(ShiftMap::new(vec![1.0, 2.0])),
        Composition::Series,
    )
    .unwrap();
    assert!(a.equal(&b));
}

#[test]
fn cmpmap_different_component() {
    let a = CmpMap::new(
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        Box::new(ShiftMap::new(vec![1.0, 2.0])),
        Composition::Series,
    )
    .unwrap();
    let b = CmpMap::new(
        Box::new(ZoomMap::new(2, 4.0).unwrap()),
        Box::new(ShiftMap::new(vec![1.0, 2.0])),
        Composition::Series,
    )
    .unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn cmpmap_different_kind() {
    let a = CmpMap::new(
        Box::new(UnitMap::new(2)),
        Box::new(UnitMap::new(2)),
        Composition::Series,
    )
    .unwrap();
    let b = CmpMap::new(
        Box::new(UnitMap::new(2)),
        Box::new(UnitMap::new(2)),
        Composition::Parallel,
    )
    .unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// PolyMap
// =========================================================================

#[test]
fn polymap_same() {
    let terms = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let a = PolyMap::new(2, 2, &terms, None).unwrap();
    let b = PolyMap::new(2, 2, &terms, None).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn polymap_different_coeff() {
    let terms_a = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let terms_b = vec![PolyTerm {
        coeff: 3.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let a = PolyMap::new(1, 1, &terms_a, None).unwrap();
    let b = PolyMap::new(1, 1, &terms_b, None).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// ChebyMap
// =========================================================================

#[test]
fn chebymap_same() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let a = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();
    let b = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn chebymap_different_bounds() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let a = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();
    let b = ChebyMap::new(1, 1, &terms, None, &[-2.0], &[2.0], None, None).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// LutMap
// =========================================================================

#[test]
fn lutmap_same() {
    let a = LutMap::new(&[0.0, 1.0, 4.0, 9.0], 0.0, 1.0).unwrap();
    let b = LutMap::new(&[0.0, 1.0, 4.0, 9.0], 0.0, 1.0).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn lutmap_different_values() {
    let a = LutMap::new(&[0.0, 1.0, 4.0, 9.0], 0.0, 1.0).unwrap();
    let b = LutMap::new(&[0.0, 1.0, 4.0, 16.0], 0.0, 1.0).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn lutmap_different_start() {
    let a = LutMap::new(&[0.0, 1.0], 0.0, 1.0).unwrap();
    let b = LutMap::new(&[0.0, 1.0], 1.0, 1.0).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// WcsMap
// =========================================================================

#[test]
fn wcsmap_same() {
    let a = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let b = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn wcsmap_different_proj() {
    let a = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let b = WcsMap::new(2, WcsProjection::Sin, 1, 2).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn wcsmap_different_axes() {
    let a = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    let b = WcsMap::new(3, WcsProjection::Tan, 1, 3).unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// SlaMap
// =========================================================================

#[test]
fn slamap_same() {
    let mut a = SlaMap::new();
    a.add(SlaCvtType::Eqgal, &[]);
    let mut b = SlaMap::new();
    b.add(SlaCvtType::Eqgal, &[]);
    assert!(a.equal(&b));
}

#[test]
fn slamap_different_type() {
    let mut a = SlaMap::new();
    a.add(SlaCvtType::Eqgal, &[]);
    let mut b = SlaMap::new();
    b.add(SlaCvtType::Galeq, &[]);
    assert!(!a.equal(&b));
}

#[test]
fn slamap_different_args() {
    let mut a = SlaMap::new();
    a.add(SlaCvtType::Prec, &[50000.0, 51000.0]);
    let mut b = SlaMap::new();
    b.add(SlaCvtType::Prec, &[50000.0, 52000.0]);
    assert!(!a.equal(&b));
}

// =========================================================================
// SphMap
// =========================================================================

#[test]
fn sphmap_same() {
    let a = SphMap::new();
    let b = SphMap::new();
    assert!(a.equal(&b));
}

#[test]
fn sphmap_different_polar_long() {
    let mut a = SphMap::new();
    a.set_polar_long(1.5);
    let b = SphMap::new();
    assert!(!a.equal(&b));
}

#[test]
fn sphmap_different_unit_radius() {
    let mut a = SphMap::new();
    a.set_unit_radius(true);
    let b = SphMap::new();
    assert!(!a.equal(&b));
}

#[test]
fn sphmap_inverted() {
    let mut a = SphMap::new();
    a.invert();
    let b = SphMap::new();
    assert!(!a.equal(&b));
}

#[test]
fn cmpmap_equal_different_tree_structure() {
    // (A ; (B ; C)) vs ((A ; B) ; C) — same effective pipeline.
    let a = ShiftMap::new(vec![1.0, 2.0]);
    let b = ZoomMap::new(2, 3.0).unwrap();
    let c = ShiftMap::new(vec![4.0, 5.0]);

    // Tree 1: A ; (B ; C)
    let bc = CmpMap::new(
        Box::new(b.clone()),
        Box::new(c.clone()),
        Composition::Series,
    )
    .unwrap();
    let abc_1 = CmpMap::new(
        Box::new(a.clone()),
        Box::new(bc),
        Composition::Series,
    )
    .unwrap();

    // Tree 2: (A ; B) ; C
    let ab = CmpMap::new(
        Box::new(a.clone()),
        Box::new(b.clone()),
        Composition::Series,
    )
    .unwrap();
    let abc_2 = CmpMap::new(
        Box::new(ab),
        Box::new(c.clone()),
        Composition::Series,
    )
    .unwrap();

    assert!(abc_1.equal(&abc_2));
}

// =========================================================================
// Cross-type rejection
// =========================================================================

#[test]
fn cross_type_zoom_vs_shift() {
    let a = ZoomMap::new(2, 1.0).unwrap();
    let b = ShiftMap::new(vec![0.0, 0.0]);
    assert!(!a.equal(&b));
}

#[test]
fn cross_type_unit_vs_zoom() {
    let a = UnitMap::new(2);
    let b = ZoomMap::new(2, 1.0).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn cross_type_winmap_vs_shiftmap() {
    let a = WinMap::new(&[0.0], &[1.0]).unwrap();
    let b = ShiftMap::new(vec![0.0]);
    assert!(!a.equal(&b));
}

// =========================================================================
// BasicFrame
// =========================================================================

#[test]
fn basicframe_same() {
    let a = BasicFrameBuilder::new(2)
        .title("test")
        .domain("PIXEL")
        .build()
        .unwrap();
    let b = BasicFrameBuilder::new(2)
        .title("test")
        .domain("PIXEL")
        .build()
        .unwrap();
    assert!(a.equal(&b));
}

#[test]
fn basicframe_different_naxes() {
    let a = BasicFrameBuilder::new(2).build().unwrap();
    let b = BasicFrameBuilder::new(3).build().unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn basicframe_different_title() {
    let a = BasicFrameBuilder::new(2).title("A").build().unwrap();
    let b = BasicFrameBuilder::new(2).title("B").build().unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn basicframe_different_domain() {
    let a = BasicFrameBuilder::new(2).domain("X").build().unwrap();
    let b = BasicFrameBuilder::new(2).domain("Y").build().unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn basicframe_different_epoch() {
    let a = BasicFrameBuilder::new(2).epoch(50000.0).build().unwrap();
    let b = BasicFrameBuilder::new(2).epoch(51000.0).build().unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn basicframe_epoch_set_vs_unset() {
    let a = BasicFrameBuilder::new(2).epoch(50000.0).build().unwrap();
    let b = BasicFrameBuilder::new(2).build().unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn basicframe_different_labels() {
    let a = BasicFrameBuilder::new(2).label(1, "X").build().unwrap();
    let b = BasicFrameBuilder::new(2).label(1, "Y").build().unwrap();
    assert!(!a.equal(&b));
}

// =========================================================================
// CmpFrame
// =========================================================================

#[test]
fn cmpframe_same() {
    let a = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    let b = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    assert!(a.equal(&b));
}

#[test]
fn cmpframe_different_child() {
    let a = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).title("A").build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    let b = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).title("B").build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    assert!(!a.equal(&b));
}

// =========================================================================
// SkyFrame
// =========================================================================

#[test]
fn skyframe_same() {
    let a = SkyFrame::new(SkySystem::Fk5);
    let b = SkyFrame::new(SkySystem::Fk5);
    assert!(a.equal(&b));
}

#[test]
fn skyframe_different_system() {
    let a = SkyFrame::new(SkySystem::Fk5);
    let b = SkyFrame::new(SkySystem::Galactic);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_different_equinox() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_equinox(50000.0);
    let mut b = SkyFrame::new(SkySystem::Fk5);
    b.set_equinox(51000.0);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_equinox_set_vs_unset() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_equinox(51544.5);
    let b = SkyFrame::new(SkySystem::Fk5);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_different_epoch() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_epoch(50000.0);
    let mut b = SkyFrame::new(SkySystem::Fk5);
    b.set_epoch(51000.0);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_different_sky_ref_is() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_sky_ref_is(SkyRefIs::Origin);
    let b = SkyFrame::new(SkySystem::Fk5);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_different_sky_ref() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_sky_ref(1.0, 0.5);
    let b = SkyFrame::new(SkySystem::Fk5);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_different_neglon() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_neglon(true);
    let b = SkyFrame::new(SkySystem::Fk5);
    assert!(!a.equal(&b));
}

#[test]
fn skyframe_different_title() {
    let mut a = SkyFrame::new(SkySystem::Fk5);
    a.set_title("A");
    let mut b = SkyFrame::new(SkySystem::Fk5);
    b.set_title("B");
    assert!(!a.equal(&b));
}

// =========================================================================
// Cross-type frame rejection
// =========================================================================

#[test]
fn cross_type_skyframe_vs_basicframe() {
    let a = SkyFrame::new(SkySystem::Fk5);
    let b = BasicFrameBuilder::new(2).build().unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn cross_type_basicframe_vs_skyframe() {
    let a = BasicFrameBuilder::new(2).build().unwrap();
    let b = SkyFrame::new(SkySystem::Fk5);
    assert!(!a.equal(&b));
}

// =========================================================================
// FrameSet equality (both as Frame and as Mapping)
// =========================================================================

#[test]
fn frameset_same() {
    let mut a = FrameSet::new(Box::new(BasicFrameBuilder::new(2).build().unwrap()));
    a.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Box::new(BasicFrameBuilder::new(2).title("current").build().unwrap()),
    )
    .unwrap();

    let mut b = FrameSet::new(Box::new(BasicFrameBuilder::new(2).build().unwrap()));
    b.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Box::new(BasicFrameBuilder::new(2).title("current").build().unwrap()),
    )
    .unwrap();

    // Test as Frame.
    assert!(Frame::equal(&a, &b));
    // Test as Mapping.
    assert!(Mapping::equal(&a, &b));
}

#[test]
fn frameset_different_mapping() {
    let mut a = FrameSet::new(Box::new(BasicFrameBuilder::new(2).build().unwrap()));
    a.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
    )
    .unwrap();

    let mut b = FrameSet::new(Box::new(BasicFrameBuilder::new(2).build().unwrap()));
    b.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
    )
    .unwrap();

    assert!(!Frame::equal(&a, &b));
}

// =========================================================================
// Serialisation round-trip preserves equality
// =========================================================================

#[test]
fn serialise_round_trip_zoommap() {
    let original = ZoomMap::new(2, 3.14159).unwrap();
    let native =
        ast_core::to_native_string(&ast_core::AnyObject::ZoomMap(original.clone())).unwrap();
    let parsed = ast_core::from_native_string(&native).unwrap();
    if let ast_core::AnyObject::ZoomMap(ref restored) = parsed {
        assert!(original.equal(restored));
    } else {
        panic!("expected ZoomMap from round-trip");
    }
}

#[test]
fn serialise_round_trip_shiftmap() {
    let original = ShiftMap::new(vec![1.5, -2.7, 0.001]);
    let native =
        ast_core::to_native_string(&ast_core::AnyObject::ShiftMap(original.clone())).unwrap();
    let parsed = ast_core::from_native_string(&native).unwrap();
    if let ast_core::AnyObject::ShiftMap(ref restored) = parsed {
        assert!(original.equal(restored));
    } else {
        panic!("expected ShiftMap from round-trip");
    }
}

#[test]
fn serialise_round_trip_skyframe() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_epoch(52000.0);
    original.set_equinox(51544.5);
    original.set_neglon(true);
    original.set_title("Test Sky");

    let native =
        ast_core::to_native_string(&ast_core::AnyObject::SkyFrame(original.clone())).unwrap();
    let parsed = ast_core::from_native_string(&native).unwrap();
    if let ast_core::AnyObject::SkyFrame(ref restored) = parsed {
        assert!(original.equal(restored));
    } else {
        panic!("expected SkyFrame from round-trip");
    }
}

// =========================================================================
// MatrixMap equality with inversion / storage normalization
// =========================================================================

#[test]
fn matrixmap_equal_cross_inversion() {
    // A MatrixMap storing the inverse matrix (non-inverted)
    // should equal the original matrix with invert flag set.
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mut mm_inv = MatrixMap::new(m.clone()).unwrap();
    mm_inv.invert(); // Now uses the inverse matrix as its forward transform

    // Compute what the inverse matrix is: for [[1,2],[3,4]], inv = [[-2,1],[1.5,-0.5]]
    let m_inv = array![[-2.0, 1.0], [1.5, -0.5]];
    let mm_direct = MatrixMap::new(m_inv).unwrap();

    // Both compute the same effective forward transform => equal
    assert!(mm_inv.equal(&mm_direct));

    // But a non-inverted [[1,2],[3,4]] is NOT equal to inverted [[1,2],[3,4]]
    // (they do different things)
    let mm_fwd = MatrixMap::new(m).unwrap();
    assert!(!mm_fwd.equal(&mm_inv));
}
