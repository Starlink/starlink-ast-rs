//! Tests for the mesh-based overlap algorithm.
//!
//! Covers: Circle-Circle, Circle-Box, Circle-NullRegion,
//! separated regions, contained regions, partial overlap,
//! identical regions, and negated regions.

use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::regions::{BoxRegionBuilder, CircleBuilder, NullRegionBuilder, Overlap, Region};

// ---- Helper: create a 2-D BasicFrame ----

fn frame2d() -> impl Frame + Clone + 'static {
    BasicFrameBuilder::new(2).build().unwrap()
}

// ============================================================================
// Circle vs Circle
// ============================================================================

#[test]
fn identical_circles_same() {
    let frame = frame2d();
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let result = c1.overlap(&c2).unwrap();
    assert_eq!(result, Overlap::Same);
}

#[test]
fn separated_circles_none() {
    let frame = frame2d();
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![10.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let result = c1.overlap(&c2).unwrap();
    assert_eq!(result, Overlap::None);
}

#[test]
fn small_circle_inside_large() {
    let frame = frame2d();
    // Small circle at origin with radius 2.
    let small = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    // Large circle at origin with radius 10.
    let large = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(10.0)
        .build()
        .unwrap();

    // small.overlap(large): small is first, large is second.
    // The small circle is entirely inside the large circle.
    let result = small.overlap(&large).unwrap();
    assert_eq!(result, Overlap::FirstInside);
}

#[test]
fn large_circle_contains_small() {
    let frame = frame2d();
    let large = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(10.0)
        .build()
        .unwrap();
    let small = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    // large.overlap(small): large is first, small is second.
    // The small circle is entirely inside the large circle.
    let result = large.overlap(&small).unwrap();
    assert_eq!(result, Overlap::SecondInside);
}

#[test]
fn overlapping_circles_partial() {
    let frame = frame2d();
    // Two circles of radius 5, with centres 6 apart.
    // They overlap but neither contains the other.
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![6.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let result = c1.overlap(&c2).unwrap();
    assert_eq!(result, Overlap::Partial);
}

#[test]
fn off_centre_small_inside_large() {
    let frame = frame2d();
    // Small circle at (3, 0) with radius 2, inside large circle at origin
    // with radius 10.
    let small = CircleBuilder::new(frame.clone(), vec![3.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let large = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(10.0)
        .build()
        .unwrap();

    let result = small.overlap(&large).unwrap();
    assert_eq!(result, Overlap::FirstInside);
}

// ============================================================================
// Circle vs NullRegion
// ============================================================================

#[test]
fn circle_vs_nullregion_none() {
    let frame = frame2d();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let null = NullRegionBuilder::new(frame).build().unwrap();

    // A non-negated NullRegion is empty — no overlap with anything.
    let result = circle.overlap(&null).unwrap();
    assert_eq!(result, Overlap::None);
}

#[test]
fn circle_vs_negated_nullregion_first_inside() {
    let frame = frame2d();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let mut null = NullRegionBuilder::new(frame).build().unwrap();
    null.negate(); // Now it is the universal set.

    // Circle inside the universal set → FirstInside.
    let result = circle.overlap(&null).unwrap();
    assert_eq!(result, Overlap::FirstInside);
}

#[test]
fn negated_nullregion_vs_circle_second_inside() {
    let frame = frame2d();
    let mut null = NullRegionBuilder::new(frame.clone()).build().unwrap();
    null.negate(); // Universal set.
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Universal.overlap(circle): circle is entirely inside universal.
    let result = null.overlap(&circle).unwrap();
    assert_eq!(result, Overlap::SecondInside);
}

#[test]
fn two_nullregions_same() {
    let frame = frame2d();
    let null1 = NullRegionBuilder::new(frame.clone()).build().unwrap();
    let null2 = NullRegionBuilder::new(frame).build().unwrap();

    // Two empty regions are the "same" (both contain nothing).
    let result = null1.overlap(&null2).unwrap();
    assert_eq!(result, Overlap::Same);
}

#[test]
fn two_negated_nullregions_same() {
    let frame = frame2d();
    let mut null1 = NullRegionBuilder::new(frame.clone()).build().unwrap();
    let mut null2 = NullRegionBuilder::new(frame).build().unwrap();
    null1.negate();
    null2.negate();

    // Two universal regions are "same".
    let result = null1.overlap(&null2).unwrap();
    assert_eq!(result, Overlap::Same);
}

// ============================================================================
// Circle vs BoxRegion
// ============================================================================

#[test]
fn circle_inside_box() {
    let frame = frame2d();
    // Small circle at origin with radius 2.
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    // Large box from (-10, -10) to (10, 10).
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let result = circle.overlap(&bx).unwrap();
    assert_eq!(result, Overlap::FirstInside);
}

#[test]
fn box_inside_circle() {
    let frame = frame2d();
    // Small box from (-1, -1) to (1, 1).
    let bx = BoxRegionBuilder::new(frame.clone(), vec![0.0, 0.0], vec![1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    // Large circle at origin with radius 100.
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(100.0)
        .build()
        .unwrap();

    // box.overlap(circle): box is entirely inside circle.
    let result = bx.overlap(&circle).unwrap();
    assert_eq!(result, Overlap::FirstInside);
}

#[test]
fn circle_and_box_separated() {
    let frame = frame2d();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    // Box far away: centre (100,100), corner (101,101) → spans (99,99) to (101,101).
    let bx = BoxRegionBuilder::new(frame, vec![100.0, 100.0], vec![101.0, 101.0])
        .unwrap()
        .build()
        .unwrap();

    let result = circle.overlap(&bx).unwrap();
    assert_eq!(result, Overlap::None);
}

#[test]
fn circle_and_box_partial() {
    let frame = frame2d();
    // Circle centred at origin, radius 5.
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    // Box from (3, -2) to (8, 2) — partially overlaps the circle.
    let bx = BoxRegionBuilder::new(frame, vec![5.5, 0.0], vec![2.5, 2.0])
        .unwrap()
        .build()
        .unwrap();

    let result = circle.overlap(&bx).unwrap();
    assert_eq!(result, Overlap::Partial);
}

// ============================================================================
// Negated circle tests
// ============================================================================

#[test]
fn negated_circle_vs_small_circle() {
    let frame = frame2d();
    // Negated large circle: everything OUTSIDE radius 10 from origin is "inside".
    let mut neg_circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(10.0)
        .build()
        .unwrap();
    neg_circle.negate();

    // Small circle at origin, radius 2 — entirely inside the un-negated circle,
    // so entirely OUTSIDE the negated circle.
    let small = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let result = neg_circle.overlap(&small).unwrap();
    assert_eq!(result, Overlap::None);
}

#[test]
fn negated_circle_vs_far_circle() {
    let frame = frame2d();
    // Negated circle: everything outside radius 2 from origin is "inside".
    let mut neg_circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    neg_circle.negate();

    // Far circle at (50, 0), radius 3 — entirely outside the un-negated circle,
    // so entirely inside the negated circle.
    let far = CircleBuilder::new(frame, vec![50.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    // neg_circle.overlap(far): far is entirely inside neg_circle.
    let result = neg_circle.overlap(&far).unwrap();
    assert_eq!(result, Overlap::SecondInside);
}

// ============================================================================
// Symmetry test
// ============================================================================

#[test]
fn overlap_symmetry() {
    let frame = frame2d();
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![6.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Partial overlap should be symmetric.
    let r1 = c1.overlap(&c2).unwrap();
    let r2 = c2.overlap(&c1).unwrap();
    assert_eq!(r1, Overlap::Partial);
    assert_eq!(r2, Overlap::Partial);
}

#[test]
fn containment_swaps_with_argument_order() {
    let frame = frame2d();
    let small = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let large = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(10.0)
        .build()
        .unwrap();

    // small inside large.
    assert_eq!(small.overlap(&large).unwrap(), Overlap::FirstInside);
    // large contains small.
    assert_eq!(large.overlap(&small).unwrap(), Overlap::SecondInside);
}

// ============================================================================
// Bounding-box quick reject
// ============================================================================

#[test]
fn bounding_box_quick_reject() {
    let frame = frame2d();
    // Two circles very far apart — bounding boxes don't intersect.
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![1000.0, 1000.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();

    let result = c1.overlap(&c2).unwrap();
    assert_eq!(result, Overlap::None);
}
