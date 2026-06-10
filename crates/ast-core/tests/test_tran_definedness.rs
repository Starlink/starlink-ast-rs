//! Tests for `Mapping::has_forward` / `has_inverse` transform-definedness.
//!
//! C ancestors: polymap.c `GetTranForward`/`GetTranInverse` (~2222/2285),
//! cmpmap.c and tranmap.c `GetTranForward`/`GetTranInverse`, and
//! matrixmap.c (inverse defined only when the inverse matrix exists).
//! In C the flags report availability of each direction in the mapping's
//! current orientation; compound mappings AND their components.

use ast_core::mappings::{
    ChebyMap, CmpMap, Composition, Mapping, MatrixMap, PolyMap, PolyTerm, TranMap, ZoomMap,
};
use ndarray::Array2;

/// A 1-in/1-out PolyMap with forward coefficients only: y = x^2.
fn fwd_only_polymap() -> PolyMap {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![2],
        out_axis: 1,
    }];
    PolyMap::new(1, 1, &fwd, None).unwrap()
}

#[test]
fn polymap_forward_only_has_no_inverse() {
    let pm = fwd_only_polymap();
    assert!(pm.has_forward());
    assert!(!pm.has_inverse());
}

#[test]
fn polymap_iter_inverse_provides_inverse() {
    let mut pm = fwd_only_polymap();
    pm.set_iter_inverse(true);
    assert!(pm.has_forward());
    assert!(pm.has_inverse());
}

#[test]
fn polymap_invert_swaps_definedness() {
    let mut pm = fwd_only_polymap();
    pm.invert();
    assert!(!pm.has_forward());
    assert!(pm.has_inverse());
}

#[test]
fn chebymap_forward_only_has_no_inverse() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let cm = ChebyMap::new(1, 1, &terms, None, &[0.0], &[1.0], None, None).unwrap();
    assert!(cm.has_forward());
    assert!(!cm.has_inverse());
}

#[test]
fn matrixmap_non_square_has_no_inverse() {
    // 2 outputs from 3 inputs: no inverse matrix exists.
    let m = Array2::from_shape_vec((2, 3), vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap();
    let mm = MatrixMap::new(m).unwrap();
    assert!(mm.has_forward());
    assert!(!mm.has_inverse());
}

#[test]
fn matrixmap_invert_swaps_definedness() {
    let m = Array2::from_shape_vec((2, 3), vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap();
    let mut mm = MatrixMap::new(m).unwrap();
    mm.invert();
    assert!(!mm.has_forward());
    assert!(mm.has_inverse());
}

#[test]
fn cmpmap_series_definedness_follows_components() {
    let pm = fwd_only_polymap();
    let zm = ZoomMap::new(1, 2.0).unwrap();
    let cmp = CmpMap::new(Box::new(pm), Box::new(zm), Composition::Series).unwrap();
    assert!(cmp.has_forward());
    assert!(!cmp.has_inverse());
}

#[test]
fn cmpmap_parallel_definedness_follows_components() {
    let pm = fwd_only_polymap();
    let zm = ZoomMap::new(1, 2.0).unwrap();
    let cmp = CmpMap::new(Box::new(pm), Box::new(zm), Composition::Parallel).unwrap();
    assert!(cmp.has_forward());
    assert!(!cmp.has_inverse());
}

#[test]
fn cmpmap_invert_swaps_definedness() {
    let pm = fwd_only_polymap();
    let zm = ZoomMap::new(1, 2.0).unwrap();
    let mut cmp = CmpMap::new(Box::new(pm), Box::new(zm), Composition::Series).unwrap();
    cmp.invert();
    assert!(!cmp.has_forward());
    assert!(cmp.has_inverse());
}

#[test]
fn cmpmap_of_two_way_components_has_both_directions() {
    let z1 = ZoomMap::new(2, 2.0).unwrap();
    let z2 = ZoomMap::new(2, 0.5).unwrap();
    let cmp = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Series).unwrap();
    assert!(cmp.has_forward());
    assert!(cmp.has_inverse());
}

#[test]
fn tranmap_definedness_follows_components() {
    // TranMap forward uses map1's forward; inverse uses map2's inverse.
    // map2 has no inverse, so the TranMap has no inverse.
    let zm = ZoomMap::new(1, 2.0).unwrap();
    let pm = fwd_only_polymap();
    let tm = TranMap::new(Box::new(zm), Box::new(pm)).unwrap();
    assert!(tm.has_forward());
    assert!(!tm.has_inverse());
}
