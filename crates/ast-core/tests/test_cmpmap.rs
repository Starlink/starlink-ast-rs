//! Tests for CmpMap (compound mapping) and composition methods.
//!
//! Ancestor: behaviour derived from the C AST CmpMap documentation and
//! reference implementation.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, UnitMap, ZoomMap};

// ------------------------------------------------------------------ //
// Series composition                                                   //
// ------------------------------------------------------------------ //

/// zoom(2) then shift(10): y = 2*x + 10.
#[test]
fn cmpmap_series_zoom_then_shift() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    assert_eq!(cm.nin(), 1);
    assert_eq!(cm.nout(), 1);
    let pts = PointSet::from_rows([[5.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 20.0).abs() < 1e-12);
}

/// Series inverse: undo shift then undo zoom → (y - 10) / 2.
#[test]
fn cmpmap_series_inverse() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    // Forward: 5 → 20.  Inverse of 20 → 5.
    let pts = PointSet::from_rows([[20.0]]);
    let out = cm.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
}

// ------------------------------------------------------------------ //
// Parallel composition                                                 //
// ------------------------------------------------------------------ //

/// zoom(3) alongside shift(100): first coord scaled by 3, second shifted by 100.
#[test]
fn cmpmap_parallel_zoom_alongside_shift() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([100.0]));
    let cm = CmpMap::new(z, s, Composition::Parallel).unwrap();
    assert_eq!(cm.nin(), 2);
    assert_eq!(cm.nout(), 2);
    let pts = PointSet::from_rows([[2.0, 5.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 105.0).abs() < 1e-12);
}

// ------------------------------------------------------------------ //
// Error cases                                                          //
// ------------------------------------------------------------------ //

/// Series dimension mismatch must return an error.
#[test]
fn cmpmap_series_dimension_mismatch_error() {
    let a: Box<dyn Mapping> = Box::new(ZoomMap::new(2, 2.0).unwrap()); // nout=2
    let b: Box<dyn Mapping> = Box::new(ZoomMap::new(3, 3.0).unwrap()); // nin=3
    assert!(CmpMap::new(a, b, Composition::Series).is_err());
}

// ------------------------------------------------------------------ //
// BAD_VALUE propagation                                                //
// ------------------------------------------------------------------ //

/// BAD_VALUE propagates through a series CmpMap.
#[test]
fn cmpmap_series_bad_value_propagation() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = cm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

/// BAD_VALUE propagates through a parallel CmpMap.
#[test]
fn cmpmap_parallel_bad_value_propagation() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Parallel).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

// ------------------------------------------------------------------ //
// simplified()                                                         //
// ------------------------------------------------------------------ //

/// Two UnitMaps in series simplify to a single UnitMap.
#[test]
fn cmpmap_simplified_two_unitmaps_series() {
    let a: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let b: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let cm = CmpMap::new(a, b, Composition::Series).unwrap();
    let simp = cm.simplified().unwrap();
    let pts = PointSet::from_rows([[3.0, 7.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 7.0]);
}

/// Two UnitMaps in parallel simplify to a single wider UnitMap.
#[test]
fn cmpmap_simplified_two_unitmaps_parallel() {
    let a: Box<dyn Mapping> = Box::new(UnitMap::new(1));
    let b: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let cm = CmpMap::new(a, b, Composition::Parallel).unwrap();
    let simp = cm.simplified().unwrap();
    assert_eq!(simp.nin(), 3);
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0, 3.0]);
}

// ------------------------------------------------------------------ //
// Composition methods on Box<dyn Mapping>                             //
// ------------------------------------------------------------------ //

/// `then()` on Box<dyn Mapping> constructs a series CmpMap.
#[test]
fn dyn_mapping_then_method() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([5.0]));
    let cm = z.then(s).unwrap();
    let pts = PointSet::from_rows([[3.0]]);
    let out = cm.transform(&pts, true).unwrap();
    // 3.0 * 2 + 5 = 11
    assert!((out.point(0)[0] - 11.0).abs() < 1e-12);
}

/// `alongside()` on Box<dyn Mapping> constructs a parallel CmpMap.
#[test]
fn dyn_mapping_alongside_method() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 4.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([100.0]));
    let cm = z.alongside(s).unwrap();
    assert_eq!(cm.nin(), 2);
    assert_eq!(cm.nout(), 2);
    let pts = PointSet::from_rows([[2.0, 50.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 8.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 150.0).abs() < 1e-12);
}
