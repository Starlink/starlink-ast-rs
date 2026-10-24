//! Tests for SplineMap (bivariate B-spline mapping).
//!
//! Ported from reference/starlink-ast/ast_tester concepts and the
//! simplify fixture tests.

use crate::common::native_dump;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{Mapping, SplineMap};
use starlink_ast::{AnyObject, from_native_string};

/// Helper to build a simple order-1 (linear) SplineMap with known behavior.
///
/// With kx=1, ky=1, nx=1, ny=1:
/// - tx = [0, 1], ty = [0, 1] (domain [0,1] x [0,1])
/// - cu = [c_u], cv = [c_v]
///
/// The resulting spline is constant: u(x,y) = c_u, v(x,y) = c_v
/// for all (x,y) in [0,1] x [0,1].
fn make_constant_spline(cu_val: f64, cv_val: f64) -> SplineMap {
    SplineMap::new(
        1,
        1,
        1,
        1,
        vec![0.0, 1.0], // tx: kx + nx = 2
        vec![0.0, 1.0], // ty: ky + ny = 2
        vec![cu_val],   // cu
        vec![cv_val],   // cv
    )
    .unwrap()
}

/// Helper to build a linear (order 2) SplineMap that acts as identity: u=x, v=y.
///
/// With kx=2, ky=2, nx=2, ny=2:
/// - tx = [0, 0, 1, 1], ty = [0, 0, 1, 1]
/// - The B-spline basis for order 2 on [0,1] with knots [0,0,1,1] gives
///   two basis functions: B0(x) = 1-x, B1(x) = x.
/// - For u(x,y) = x: In CMLIB storage order (i varies fastest, then j):
///   c[ix + iy*nx] so c[0,0]=0, c[1,0]=1, c[0,1]=0, c[1,1]=1
///   → u(x,y) = (1-y)*[(1-x)*0 + x*1] + y*[(1-x)*0 + x*1] = x
/// - For v(x,y) = y: c[0,0]=0, c[1,0]=0, c[0,1]=1, c[1,1]=1
///   → v(x,y) = (1-y)*[(1-x)*0 + x*0] + y*[(1-x)*1 + x*1] = y
fn make_identity_spline() -> SplineMap {
    SplineMap::new(
        2,
        2,
        2,
        2,
        vec![0.0, 0.0, 1.0, 1.0], // tx
        vec![0.0, 0.0, 1.0, 1.0], // ty
        vec![0.0, 1.0, 0.0, 1.0], // cu: u(x,y) = x
        vec![0.0, 0.0, 1.0, 1.0], // cv: v(x,y) = y
    )
    .unwrap()
}

/// Helper to build a linear SplineMap with perturbation: u = x + 0.01, v = y + 0.02.
fn make_perturbed_identity() -> SplineMap {
    SplineMap::new(
        2,
        2,
        2,
        2,
        vec![0.0, 0.0, 1.0, 1.0], // tx
        vec![0.0, 0.0, 1.0, 1.0], // ty
        // cu: u(x,y) = x + 0.01. Coefficients: all shifted by 0.01
        vec![0.01, 1.01, 0.01, 1.01],
        // cv: v(x,y) = y + 0.02. Coefficients: all shifted by 0.02
        vec![0.02, 0.02, 1.02, 1.02],
    )
    .unwrap()
}

#[test]
fn test_basic_properties() {
    let m = make_constant_spline(1.0, 2.0);
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);
    assert_eq!(m.kx(), 1);
    assert_eq!(m.ky(), 1);
    assert_eq!(m.nx(), 1);
    assert_eq!(m.ny(), 1);
    assert_eq!(m.tx(), &[0.0, 1.0]);
    assert_eq!(m.ty(), &[0.0, 1.0]);
    assert_eq!(m.cu(), &[1.0]);
    assert_eq!(m.cv(), &[2.0]);
    assert!(!m.is_inverted());
    assert_eq!(m.inv_niter(), 6); // default
    assert!((m.inv_tol() - 1.0e-6).abs() < 1.0e-15); // default
    assert!(!m.out_unit());
}

#[test]
fn test_forward_constant() {
    let m = make_constant_spline(1.0, 2.0);
    let pts = PointSet::from_rows([[0.2, 0.3], [0.5, 0.5], [0.8, 0.7]]);
    let result = m.transform(&pts, true).unwrap();
    for i in 0..3 {
        let p = result.point(i);
        assert!(
            (p[0] - 1.0).abs() < 1.0e-10,
            "point {i}: u = {} expected 1.0",
            p[0]
        );
        assert!(
            (p[1] - 2.0).abs() < 1.0e-10,
            "point {i}: v = {} expected 2.0",
            p[1]
        );
    }
}

#[test]
fn test_forward_identity() {
    let m = make_identity_spline();
    let pts = PointSet::from_rows([[0.1, 0.2], [0.3, 0.4], [0.7, 0.6], [0.9, 0.8]]);
    let result = m.transform(&pts, true).unwrap();
    for i in 0..4 {
        let inp = pts.point(i);
        let outp = result.point(i);
        assert!(
            (outp[0] - inp[0]).abs() < 1.0e-10,
            "point {i}: u = {} expected {}",
            outp[0],
            inp[0]
        );
        assert!(
            (outp[1] - inp[1]).abs() < 1.0e-10,
            "point {i}: v = {} expected {}",
            outp[1],
            inp[1]
        );
    }
}

#[test]
fn test_forward_outside_domain() {
    let m = make_identity_spline();
    // Point (1.5, 0.5) is outside the x domain [0,1]
    let pts = PointSet::from_rows([[1.5, 0.5]]);
    let result = m.transform(&pts, true).unwrap();
    let p = result.point(0);
    assert_eq!(p[0], BAD_VALUE);
    assert_eq!(p[1], BAD_VALUE);
}

#[test]
fn test_forward_outside_with_outunit() {
    let mut m = make_identity_spline();
    m.set_out_unit(true);
    // Point (1.5, 0.5) is outside the x domain [0,1]
    // With out_unit=true, output should equal input
    let pts = PointSet::from_rows([[1.5, 0.5]]);
    let result = m.transform(&pts, true).unwrap();
    let p = result.point(0);
    assert!((p[0] - 1.5).abs() < 1.0e-10);
    assert!((p[1] - 0.5).abs() < 1.0e-10);
}

#[test]
fn test_inverse_roundtrip() {
    let m = make_perturbed_identity();
    // Forward: (0.5, 0.5) -> (0.51, 0.52)
    let pts = PointSet::from_rows([[0.5, 0.5]]);
    let fwd = m.transform(&pts, true).unwrap();
    let p = fwd.point(0);
    assert!(
        (p[0] - 0.51).abs() < 1.0e-10,
        "forward u = {} expected 0.51",
        p[0]
    );
    assert!(
        (p[1] - 0.52).abs() < 1.0e-10,
        "forward v = {} expected 0.52",
        p[1]
    );

    // Inverse: (0.51, 0.52) -> (0.5, 0.5)
    let inv = m.transform(&fwd, false).unwrap();
    let q = inv.point(0);
    assert!(
        (q[0] - 0.5).abs() < 1.0e-6,
        "inverse x = {} expected 0.5",
        q[0]
    );
    assert!(
        (q[1] - 0.5).abs() < 1.0e-6,
        "inverse y = {} expected 0.5",
        q[1]
    );
}

#[test]
fn test_inverse_multiple_points() {
    let m = make_perturbed_identity();
    let pts = PointSet::from_rows([[0.2, 0.3], [0.4, 0.5], [0.6, 0.7], [0.8, 0.9]]);
    let fwd = m.transform(&pts, true).unwrap();
    let inv = m.transform(&fwd, false).unwrap();
    for i in 0..4 {
        let orig = pts.point(i);
        let back = inv.point(i);
        assert!(
            (back[0] - orig[0]).abs() < 1.0e-6,
            "point {i}: x = {} expected {}",
            back[0],
            orig[0]
        );
        assert!(
            (back[1] - orig[1]).abs() < 1.0e-6,
            "point {i}: y = {} expected {}",
            back[1],
            orig[1]
        );
    }
}

#[test]
fn test_invert_flag() {
    let mut m = make_perturbed_identity();
    assert!(!m.is_inverted());
    m.invert();
    assert!(m.is_inverted());

    // With invert flag set, transform(true) should do the inverse
    let pts = PointSet::from_rows([[0.51, 0.52]]);
    let result = m.transform(&pts, true).unwrap();
    let p = result.point(0);
    assert!(
        (p[0] - 0.5).abs() < 1.0e-6,
        "inverted forward = {} expected 0.5",
        p[0]
    );
    assert!(
        (p[1] - 0.5).abs() < 1.0e-6,
        "inverted forward = {} expected 0.5",
        p[1]
    );
}

#[test]
fn test_equal() {
    let m1 = make_perturbed_identity();
    let m2 = make_perturbed_identity();
    assert!(m1.equal(&m2));
    assert!(m2.equal(&m1));

    // Different coefficients
    let m3 = make_identity_spline();
    assert!(!m1.equal(&m3));
}

#[test]
fn test_equal_inverted_not_equal() {
    let m1 = make_perturbed_identity();
    let mut m2 = make_perturbed_identity();
    m2.invert();
    assert!(!m1.equal(&m2));
}

#[test]
fn test_serialization_roundtrip() {
    let m = make_perturbed_identity();
    let obj = AnyObject::SplineMap(m.clone());
    let native_str = native_dump(&obj).unwrap();

    // Verify we can read it back
    let obj2 = from_native_string(&native_str).unwrap();
    let m2 = match obj2 {
        AnyObject::SplineMap(s) => s,
        other => panic!("expected SplineMap, got {:?}", other),
    };

    assert!(m.equal(&m2));
    assert_eq!(m2.kx(), 2);
    assert_eq!(m2.ky(), 2);
    assert_eq!(m2.nx(), 2);
    assert_eq!(m2.ny(), 2);
}

#[test]
fn test_serialization_roundtrip_with_attributes() {
    let mut m = make_perturbed_identity();
    m.set_inv_niter(10);
    m.set_inv_tol(1.0e-8);
    m.set_out_unit(true);

    let obj = AnyObject::SplineMap(m);
    let native_str = native_dump(&obj).unwrap();
    let obj2 = from_native_string(&native_str).unwrap();
    let m2 = match obj2 {
        AnyObject::SplineMap(s) => s,
        other => panic!("expected SplineMap, got {:?}", other),
    };

    assert_eq!(m2.inv_niter(), 10);
    assert!((m2.inv_tol() - 1.0e-8).abs() < 1.0e-15);
    assert!(m2.out_unit());
}

#[test]
fn test_inverse_cancellation_simplify() {
    use starlink_ast::mappings::{CmpMap, Composition, UnitMap};

    let m1 = make_perturbed_identity();
    let mut m2 = make_perturbed_identity();
    m2.invert();

    // Build CmpMap with m1 forward and m2 inverse
    let cmp = CmpMap::new(Box::new(m1), Box::new(m2), Composition::Series).unwrap();

    // Simplify should produce a UnitMap
    let simp = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(
        simp.as_any().is::<UnitMap>(),
        "expected UnitMap after simplification, got {:?}",
        simp
    );
}

#[test]
fn test_no_cancellation_same_direction() {
    use starlink_ast::mappings::{CmpMap, Composition, UnitMap};

    let m1 = make_perturbed_identity();
    let m2 = make_perturbed_identity();

    // Both in the same direction -- should NOT simplify to UnitMap
    let cmp = CmpMap::new(Box::new(m1), Box::new(m2), Composition::Series).unwrap();

    let simp = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(
        !simp.as_any().is::<UnitMap>(),
        "should not simplify same-direction splines to UnitMap"
    );
}

#[test]
fn test_no_cancellation_different_coeffs() {
    use starlink_ast::mappings::{CmpMap, Composition, UnitMap};

    let m1 = make_perturbed_identity();
    let mut m2 = make_identity_spline(); // different coefficients
    m2.invert();

    let cmp = CmpMap::new(Box::new(m1), Box::new(m2), Composition::Series).unwrap();

    let simp = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(
        !simp.as_any().is::<UnitMap>(),
        "should not simplify different splines to UnitMap"
    );
}

#[test]
fn test_bad_input_forward() {
    let m = make_identity_spline();
    let pts = PointSet::from_rows([[BAD_VALUE, 0.5]]);
    let result = m.transform(&pts, true).unwrap();
    let p = result.point(0);
    assert_eq!(p[0], BAD_VALUE);
    assert_eq!(p[1], BAD_VALUE);
}

#[test]
fn test_bad_input_inverse() {
    let m = make_perturbed_identity();
    let pts = PointSet::from_rows([[BAD_VALUE, 0.5]]);
    let result = m.transform(&pts, false).unwrap();
    let p = result.point(0);
    assert_eq!(p[0], BAD_VALUE);
    assert_eq!(p[1], BAD_VALUE);
}

#[test]
fn test_rate_forward() {
    let m = make_identity_spline();
    // For identity spline: du/dx = 1, du/dy = 0, dv/dx = 0, dv/dy = 1
    let at = [0.5, 0.5];
    let r11 = m.rate(&at, 1, 1).unwrap(); // du/dx
    let r12 = m.rate(&at, 1, 2).unwrap(); // du/dy
    let r21 = m.rate(&at, 2, 1).unwrap(); // dv/dx
    let r22 = m.rate(&at, 2, 2).unwrap(); // dv/dy
    assert!((r11 - 1.0).abs() < 1.0e-6, "du/dx = {} expected 1.0", r11);
    assert!(r12.abs() < 1.0e-6, "du/dy = {} expected 0.0", r12);
    assert!(r21.abs() < 1.0e-6, "dv/dx = {} expected 0.0", r21);
    assert!((r22 - 1.0).abs() < 1.0e-6, "dv/dy = {} expected 1.0", r22);
}

#[test]
fn test_set_inv_niter_and_tol() {
    let mut m = make_perturbed_identity();
    m.set_inv_niter(10);
    assert_eq!(m.inv_niter(), 10);
    m.set_inv_tol(1.0e-8);
    assert!((m.inv_tol() - 1.0e-8).abs() < 1.0e-15);
}

#[test]
fn test_clone() {
    let m = make_perturbed_identity();
    let m2 = m.clone();
    assert!(m.equal(&m2));
}

#[test]
fn test_edge_of_domain() {
    // Test that values at the lower edge and near the upper edge produce valid output.
    // Note: With minimal knots [0,0,1,1], the exact upper boundary (1.0, 1.0)
    // returns BAD due to the degenerate basis function arrangement — this matches
    // the C library behavior. Test points just inside the domain instead.
    let m = make_identity_spline();
    let pts = PointSet::from_rows([[0.0, 0.0], [0.999, 0.999]]);
    let result = m.transform(&pts, true).unwrap();
    let p0 = result.point(0);
    let p1 = result.point(1);
    assert!(
        (p0[0]).abs() < 1.0e-10,
        "edge (0,0): u = {} expected 0.0",
        p0[0]
    );
    assert!(
        (p0[1]).abs() < 1.0e-10,
        "edge (0,0): v = {} expected 0.0",
        p0[1]
    );
    assert!(
        (p1[0] - 0.999).abs() < 1.0e-10,
        "near-edge (0.999,0.999): u = {} expected 0.999",
        p1[0]
    );
    assert!(
        (p1[1] - 0.999).abs() < 1.0e-10,
        "near-edge (0.999,0.999): v = {} expected 0.999",
        p1[1]
    );
}
