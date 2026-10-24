//! Tests for `Mapping::has_forward` / `has_inverse` transform-definedness.
//!
//! C ancestors: polymap.c `GetTranForward`/`GetTranInverse` (~2222/2285),
//! cmpmap.c and tranmap.c `GetTranForward`/`GetTranInverse`, and
//! matrixmap.c (inverse defined only when the inverse matrix exists).
//! In C the flags report availability of each direction in the mapping's
//! current orientation; compound mappings AND their components.

use ndarray::Array2;
use starlink_ast::mappings::{
    ChebyMap, CmpMap, Composition, Mapping, MatrixMap, PolyMap, PolyTerm, TranMap, ZoomMap,
};

/// A 1-in/1-out PolyMap with no usable inverse: forward coefficients
/// only and the iterative inverse explicitly disabled (IterInverse
/// otherwise *defaults on* when no inverse coefficients are supplied —
/// polymap.c:6311).
fn fwd_only_polymap() -> PolyMap {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![2],
        out_axis: 1,
    }];
    let mut pm = PolyMap::new(1, 1, &fwd, None).unwrap();
    pm.set_iter_inverse(false).unwrap();
    pm
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
    pm.set_iter_inverse(true).unwrap();
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
    // Unlike PolyMap, ChebyMap keeps the iterative inverse disabled
    // (the Rust NR machinery is power-basis-specific), so a
    // forward-only ChebyMap genuinely has no inverse.
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let cm = ChebyMap::new(1, 1, &terms, None, &[0.0], &[1.0], None, None).unwrap();
    assert!(cm.has_forward());
    assert!(!cm.has_inverse());
}

/// A 2-out/3-in matrix that stays in FULL form: no off-diagonal element is
/// small enough to be zeroed, so it cannot compress and a non-square full
/// matrix has no inverse (matrixmap.c `InvertMatrix`, 1554).
fn non_square_full_matrixmap() -> MatrixMap {
    let m = Array2::from_shape_vec((2, 3), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).unwrap();
    MatrixMap::new(m).unwrap()
}

#[test]
fn matrixmap_non_square_full_has_no_inverse() {
    let mm = non_square_full_matrixmap();
    assert!(mm.has_forward());
    assert!(!mm.has_inverse());
}

#[test]
fn matrixmap_invert_swaps_definedness() {
    let mut mm = non_square_full_matrixmap();
    mm.invert();
    assert!(!mm.has_forward());
    assert!(mm.has_inverse());
}

#[test]
fn matrixmap_singular_square_has_no_inverse() {
    // Rank-1: palDmat reports it singular, so no inverse is stored.
    let m = Array2::from_shape_vec((2, 2), vec![1.0, 2.0, 2.0, 4.0]).unwrap();
    let mm = MatrixMap::new(m).unwrap();
    assert!(mm.has_forward());
    assert!(!mm.has_inverse());
}

#[test]
fn matrixmap_non_square_unit_has_both_directions() {
    // A 2-out/3-in selection matrix compresses to UNIT form, and C declares
    // every UNIT MatrixMap defined in both directions regardless of shape
    // (matrixmap.c `GetTranForward` 4946 / `GetTranInverse` 5028). The
    // inverse copies the two available axes back and zero-fills the third.
    let m = Array2::from_shape_vec((2, 3), vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap();
    let mut mm = MatrixMap::new(m).unwrap();
    assert!(mm.has_forward());
    assert!(mm.has_inverse());
    mm.invert();
    assert!(mm.has_forward());
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
fn tranmap_needs_both_directions() {
    // TranMap forward uses map1's forward and its inverse map2's inverse, so
    // C refuses a map2 with no inverse (tranmap.c astInitTranMap,
    // AST__INTRD) rather than making a TranMap without one.
    let zm = ZoomMap::new(1, 2.0).unwrap();
    let pm = fwd_only_polymap();
    let err = TranMap::new(Box::new(zm), Box::new(pm)).unwrap_err();
    assert_eq!(err.code(), starlink_ast::error::ErrorCode::Intrd);
}
