//! Tests for `Mapping::map_split` — extracting sub-mappings for a contiguous
//! range of output axes.

use ast_core::coords::PointSet;
use ast_core::mappings::{
    CmpMap, Composition, Mapping, MatrixMap, ShiftMap, UnitMap, WinMap, ZoomMap,
};

/// Helper: transform a single point and return the output coordinates.
fn transform_point(m: &dyn Mapping, input: &[f64]) -> Vec<f64> {
    let pts = PointSet::from_rows(vec![input.to_vec()]);
    let out = m.transform(&pts, true).unwrap();
    out.point(0).to_vec()
}

// ========================================================================
// UnitMap
// ========================================================================

#[test]
fn map_split_unitmap_full() {
    let u = UnitMap::new(3);
    let split = u.map_split(0, 3).unwrap();
    assert_eq!(split.nin(), 3);
    assert_eq!(split.nout(), 3);
}

#[test]
fn map_split_unitmap_subset() {
    let u = UnitMap::new(5);
    let split = u.map_split(2, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    // Should be an identity on 2 axes.
    let out = transform_point(split.as_ref(), &[7.0, 8.0]);
    assert_eq!(out, vec![7.0, 8.0]);
}

#[test]
fn map_split_unitmap_invalid() {
    let u = UnitMap::new(3);
    assert!(u.map_split(0, 0).is_none());
    assert!(u.map_split(0, 4).is_none());
    assert!(u.map_split(2, 2).is_none());
}

// ========================================================================
// ZoomMap
// ========================================================================

#[test]
fn map_split_zoommap_subset() {
    let z = ZoomMap::new(4, 3.0).unwrap();
    let split = z.map_split(1, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    let out = transform_point(split.as_ref(), &[2.0, 5.0]);
    assert_eq!(out, vec![6.0, 15.0]);
}

#[test]
fn map_split_zoommap_inverted() {
    let mut z = ZoomMap::new(3, 2.0).unwrap();
    z.invert();
    let split = z.map_split(0, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    // Inverted ZoomMap(2) should divide by 2.
    let out = transform_point(split.as_ref(), &[10.0, 6.0]);
    assert_eq!(out, vec![5.0, 3.0]);
}

#[test]
fn map_split_zoommap_invalid() {
    let z = ZoomMap::new(3, 5.0).unwrap();
    assert!(z.map_split(0, 0).is_none());
    assert!(z.map_split(0, 4).is_none());
    assert!(z.map_split(2, 2).is_none());
}

// ========================================================================
// ShiftMap
// ========================================================================

#[test]
fn map_split_shiftmap_subset() {
    let s = ShiftMap::new(vec![1.0, 2.0, 3.0, 4.0]);
    let split = s.map_split(1, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    let out = transform_point(split.as_ref(), &[10.0, 20.0]);
    assert_eq!(out, vec![12.0, 23.0]);
}

#[test]
fn map_split_shiftmap_inverted() {
    let mut s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    s.invert();
    let split = s.map_split(0, 2).unwrap();
    // Inverted ShiftMap subtracts the shifts.
    let out = transform_point(split.as_ref(), &[10.0, 20.0]);
    assert_eq!(out, vec![9.0, 18.0]);
}

#[test]
fn map_split_shiftmap_invalid() {
    let s = ShiftMap::new(vec![1.0, 2.0]);
    assert!(s.map_split(0, 0).is_none());
    assert!(s.map_split(1, 2).is_none());
}

// ========================================================================
// WinMap
// ========================================================================

#[test]
fn map_split_winmap_subset() {
    let w = WinMap::new(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]).unwrap();
    let split = w.map_split(1, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    // out[0] = 2.0 + 5.0 * input[0], out[1] = 3.0 + 6.0 * input[1]
    let out = transform_point(split.as_ref(), &[1.0, 1.0]);
    assert_eq!(out, vec![7.0, 9.0]);
}

#[test]
fn map_split_winmap_inverted() {
    let mut w = WinMap::new(&[2.0, 4.0], &[3.0, 5.0]).unwrap();
    w.invert();
    let split = w.map_split(0, 1).unwrap();
    assert_eq!(split.nin(), 1);
    assert_eq!(split.nout(), 1);
    // Inverted: out = (in - shift) / scale = (in - 2) / 3
    let out = transform_point(split.as_ref(), &[8.0]);
    assert_eq!(out, vec![2.0]);
}

#[test]
fn map_split_winmap_invalid() {
    let w = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    assert!(w.map_split(0, 0).is_none());
    assert!(w.map_split(1, 2).is_none());
}

// ========================================================================
// CmpMap (parallel)
// ========================================================================

#[test]
fn map_split_parallel_cmpmap_first_component() {
    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(vec![1.0]);
    let par = CmpMap::new(Box::new(z), Box::new(s), Composition::Parallel).unwrap();

    // Split requesting the first 2 outputs (the ZoomMap portion).
    let split = par.map_split(0, 2);
    assert!(split.is_some());
    let m = split.unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);
    let out = transform_point(m.as_ref(), &[2.0, 4.0]);
    assert_eq!(out, vec![6.0, 12.0]);
}

#[test]
fn map_split_parallel_cmpmap_second_component() {
    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(vec![1.0]);
    let par = CmpMap::new(Box::new(z), Box::new(s), Composition::Parallel).unwrap();

    // Split requesting output 2 (the ShiftMap portion).
    let split = par.map_split(2, 1);
    assert!(split.is_some());
    let m = split.unwrap();
    assert_eq!(m.nin(), 1);
    assert_eq!(m.nout(), 1);
    let out = transform_point(m.as_ref(), &[5.0]);
    assert_eq!(out, vec![6.0]);
}

#[test]
fn map_split_parallel_cmpmap_nested() {
    // (ZoomMap(2,3) || ShiftMap([1,2,3])) — split outputs [2..4) should
    // give ShiftMap([1,2]) (first two axes of the ShiftMap).
    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    let par = CmpMap::new(Box::new(z), Box::new(s), Composition::Parallel).unwrap();

    let split = par.map_split(2, 2);
    assert!(split.is_some());
    let m = split.unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);
    let out = transform_point(m.as_ref(), &[10.0, 20.0]);
    assert_eq!(out, vec![11.0, 22.0]);
}

#[test]
fn map_split_parallel_cmpmap_spanning_fails() {
    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(vec![1.0, 2.0]);
    let par = CmpMap::new(Box::new(z), Box::new(s), Composition::Parallel).unwrap();

    // Request outputs [1..3) which spans both components.
    assert!(par.map_split(1, 2).is_none());
}

#[test]
fn map_split_series_cmpmap_returns_none() {
    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(vec![1.0, 2.0]);
    let ser = CmpMap::new(Box::new(z), Box::new(s), Composition::Series).unwrap();

    // Series CmpMap cannot be split.
    assert!(ser.map_split(0, 2).is_none());
}

#[test]
fn map_split_parallel_cmpmap_inverted() {
    // Build (ZoomMap(2,3) || ShiftMap([5])), then invert.
    // Inverted parallel: applies inverse of each component.
    let z = ZoomMap::new(2, 3.0).unwrap();
    let s = ShiftMap::new(vec![5.0]);
    let mut par = CmpMap::new(Box::new(z), Box::new(s), Composition::Parallel).unwrap();
    par.invert();

    // Inverted: nout is first.nin() + second.nin() = 2 + 1 = 3.
    // But now "output" is the inputs, so split on output axis 0..2 should
    // give the inverted ZoomMap (divides by 3).
    let split = par.map_split(0, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    let out = transform_point(split.as_ref(), &[9.0, 12.0]);
    assert_eq!(out, vec![3.0, 4.0]);

    // Split output 2..3 should give inverted ShiftMap (subtracts 5).
    let split2 = par.map_split(2, 1).unwrap();
    assert_eq!(split2.nin(), 1);
    assert_eq!(split2.nout(), 1);
    let out2 = transform_point(split2.as_ref(), &[15.0]);
    assert_eq!(out2, vec![10.0]);
}

// ========================================================================
// MatrixMap (diagonal only)
// ========================================================================

#[test]
fn map_split_diagonal_matrixmap() {
    use ndarray::Array2;
    let mut diag = Array2::<f64>::zeros((3, 3));
    diag[[0, 0]] = 2.0;
    diag[[1, 1]] = 3.0;
    diag[[2, 2]] = 4.0;
    let mm = MatrixMap::new(diag).unwrap();

    let split = mm.map_split(1, 2).unwrap();
    assert_eq!(split.nin(), 2);
    assert_eq!(split.nout(), 2);
    let out = transform_point(split.as_ref(), &[1.0, 1.0]);
    assert_eq!(out, vec![3.0, 4.0]);
}

#[test]
fn map_split_non_diagonal_matrixmap_returns_none() {
    use ndarray::array;
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    assert!(mm.map_split(0, 1).is_none());
}

// ========================================================================
// Default (non-splittable mappings return None)
// ========================================================================

#[test]
fn map_split_default_returns_none() {
    use ast_core::mappings::PermMap;
    // PermMap doesn't implement map_split, so it returns None.
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    assert!(pm.map_split(0, 1).is_none());
}
