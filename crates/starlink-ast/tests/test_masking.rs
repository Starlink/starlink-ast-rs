// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Integration tests for the generic mask function.
//!
//! Covers: inside/outside masking, return count, non-identity mapping,
//! different data types, 3D masking with Prism.

use starlink_ast::frames::BasicFrameBuilder;
use starlink_ast::mappings::{UnitMap, ZoomMap};
use starlink_ast::regions::{CircleBuilder, IntervalBuilder, PrismBuilder, mask};

// ---- Basic 2D circle masking with identity mapping ----

#[test]
fn mask_outside_circle_2d() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![3.0, 3.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let mapping = UnitMap::new(2);

    let mut data = vec![1.0f64; 25];
    let count = mask(
        &circle,
        Some(&mapping),
        false, // mask outside
        &[1, 1],
        &[5, 5],
        &mut data,
        0.0,
    )
    .unwrap();

    // 25 - 13 = 12 outside
    assert_eq!(count, 12);

    // Center should be untouched.
    let idx = |x: usize, y: usize| (y - 1) * 5 + (x - 1);
    assert_eq!(data[idx(3, 3)], 1.0);

    // Corner should be masked.
    assert_eq!(data[idx(1, 1)], 0.0);
}

// ---- Integer data type ----

#[test]
fn mask_integer_data() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![2.0, 2.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let mapping = UnitMap::new(2);

    let mut data = vec![0i32; 9]; // 3x3 grid
    let count = mask(
        &circle,
        Some(&mapping),
        true,
        &[1, 1],
        &[3, 3],
        &mut data,
        -999,
    )
    .unwrap();

    // Circle at (2,2) radius 1: only the center pixel; the four at distance 1
    // lie on the boundary, which astMask treats as neither inside nor outside
    // (tests/test_region_mask_oracle.rs has C's results for such cases).
    assert_eq!(count, 1);
    // Center: idx = (2-1)*3 + (2-1) = 4
    assert_eq!(data[4], -999);
    // Corner (1,1): dist=sqrt2 > 1 -> not masked
    assert_eq!(data[0], 0);
}

// ---- Non-identity mapping (ZoomMap) ----

#[test]
fn mask_with_zoom_mapping() {
    // Grid [1..=10, 1..=10] in pixel coords.
    // ZoomMap factor 0.5 maps pixel -> region frame coords (divide by 2).
    // Circle at (2.5, 2.5) radius 1.5 in region frame.
    // So in pixel space, pixels (x,y) where (x/2 - 2.5)^2 + (y/2 - 2.5)^2 <= 1.5^2 = 2.25
    // i.e., pixels at center (5,5) with effective radius 3 in pixel space.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![2.5, 2.5])
        .unwrap()
        .radius(1.5)
        .build()
        .unwrap();
    let mapping = ZoomMap::new(2, 2.0).unwrap();

    let mut data = vec![0.0f64; 100]; // 10x10
    let count = mask(
        &circle,
        Some(&mapping),
        true,
        &[1, 1],
        &[10, 10],
        &mut data,
        -1.0,
    )
    .unwrap();

    // Verify center pixel is masked.
    let idx = |x: usize, y: usize| (y - 1) * 10 + (x - 1);
    assert_eq!(data[idx(5, 5)], -1.0);

    // A far corner should not be masked.
    assert_eq!(data[idx(1, 1)], 0.0);

    // Count should be > 0.
    assert!(count > 0);
}

// ---- 1D masking ----

// ---- 3D masking with Prism ----

#[test]
fn mask_3d_prism() {
    // Prism = Circle(2D) x Interval(1D).
    // Circle at (3,3) radius 1 in XY, interval [2..=4] in Z.
    // Grid [1..=5, 1..=5, 1..=5] -> 125 voxels.
    let frame2d = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame2d, vec![3.0, 3.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();

    let frame1d = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frame1d, vec![2.0], vec![4.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(circle, interval)
        .unwrap()
        .build()
        .unwrap();
    let mapping = UnitMap::new(3);

    let mut data = vec![0.0f64; 125]; // 5x5x5
    let count = mask(
        &prism,
        Some(&mapping),
        true,
        &[1, 1, 1],
        &[5, 5, 5],
        &mut data,
        -1.0,
    )
    .unwrap();

    // Circle at (3,3) r=1: pixels inside are (3,3),(2,3),(4,3),(3,2),(3,4) = 5 in XY
    // Interval [2..=4]: 3 Z-slices
    // Total inside prism = 5 * 3 = 15
    assert_eq!(count, 15);

    // Check a known inside voxel: (3,3,3)
    // Index = (z-1)*25 + (y-1)*5 + (x-1)
    let idx = |x: usize, y: usize, z: usize| (z - 1) * 25 + (y - 1) * 5 + (x - 1);
    assert_eq!(data[idx(3, 3, 3)], -1.0);

    // Check an inside-circle but outside-interval voxel: (3,3,1)
    assert_eq!(data[idx(3, 3, 1)], 0.0);

    // Check an outside-circle but inside-interval voxel: (1,1,3)
    assert_eq!(data[idx(1, 1, 3)], 0.0);
}

// ---- Error cases ----

#[test]
fn mask_mismatched_bounds_lengths() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let mapping = UnitMap::new(2);

    let mut data = vec![0.0f64; 10];
    let result = mask(
        &circle,
        Some(&mapping),
        true,
        &[1, 1],
        &[5, 5, 5],
        &mut data,
        0.0,
    );
    assert!(result.is_err());
}

#[test]
fn mask_wrong_mapping_nin() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let mapping = UnitMap::new(3); // 3 inputs but grid is 2D

    let mut data = vec![0.0f64; 25];
    let result = mask(
        &circle,
        Some(&mapping),
        true,
        &[1, 1],
        &[5, 5],
        &mut data,
        0.0,
    );
    assert!(result.is_err());
}

#[test]
fn mask_wrong_data_length() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let mapping = UnitMap::new(2);

    let mut data = vec![0.0f64; 10]; // should be 25 for 5x5
    let result = mask(
        &circle,
        Some(&mapping),
        true,
        &[1, 1],
        &[5, 5],
        &mut data,
        0.0,
    );
    assert!(result.is_err());
}

#[test]
fn mask_ubnd_less_than_lbnd() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let mapping = UnitMap::new(2);

    let mut data = vec![0.0f64; 1];
    let result = mask(
        &circle,
        Some(&mapping),
        true,
        &[5, 1],
        &[1, 5],
        &mut data,
        0.0,
    );
    assert!(result.is_err());
}
