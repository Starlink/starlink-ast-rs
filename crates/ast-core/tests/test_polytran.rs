//! Tests for `PolyMap::poly_tran` — fitting a replacement forward or
//! inverse polynomial by least squares.
//!
//! C ancestor: polymap.c `astPolyTran` / `ReplaceTransformation` /
//! `FitPoly1D` / `FitPoly2D` (sampling the other transformation on a
//! regular grid of `2*order` points per axis, fitting with increasing
//! order until the target accuracy `acc` is met, accepting a
//! max-order fit only if better than `maxacc`).

use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, PolyMap, PolyTerm};

fn term(coeff: f64, out_axis: usize, powers: &[usize]) -> PolyTerm {
    PolyTerm {
        coeff,
        powers: powers.to_vec(),
        out_axis,
    }
}

/// 1-D linear: y = 2x + 1, forward only.
fn linear_1d() -> PolyMap {
    PolyMap::new(1, 1, &[term(2.0, 1, &[1]), term(1.0, 1, &[0])], None).unwrap()
}

#[test]
fn polytran_fits_linear_inverse() {
    let pm = linear_1d();
    let fitted = pm
        .poly_tran(false, 1e-8, 1e-6, 5, &[0.0], &[10.0])
        .unwrap()
        .expect("fit should succeed");

    // The fitted inverse must invert y = 2x + 1: x = 0.5y - 0.5.
    let pts = PointSet::from_columns(&[&[1.0_f64, 11.0, 21.0]]).unwrap();
    let out = fitted.transform(&pts, false).unwrap();
    for (i, expect) in [0.0, 5.0, 10.0].iter().enumerate() {
        assert!(
            (out.point(i)[0] - expect).abs() < 1e-6,
            "point {i}: got {}, want {expect}",
            out.point(i)[0]
        );
    }
}

#[test]
fn polytran_round_trip_nonlinear_1d() {
    // y = x + 0.01 x^2 over [0, 10]: smooth and monotonic, so a
    // polynomial inverse fit must achieve the target accuracy.
    let pm = PolyMap::new(1, 1, &[term(1.0, 1, &[1]), term(0.01, 1, &[2])], None).unwrap();
    let fitted = pm
        .poly_tran(false, 1e-7, 1e-5, 10, &[0.0], &[10.0])
        .unwrap()
        .expect("fit should succeed");

    // Forward then fitted-inverse must round-trip interior points.
    let xs = [0.5_f64, 3.0, 7.25, 9.5];
    let pts = PointSet::from_columns(&[&xs]).unwrap();
    let ys = fitted.transform(&pts, true).unwrap();
    let back = fitted.transform(&ys, false).unwrap();
    for (i, &x) in xs.iter().enumerate() {
        assert!(
            (back.point(i)[0] - x).abs() < 1e-6,
            "round trip {i}: got {}, want {x}",
            back.point(i)[0]
        );
    }
}

#[test]
fn polytran_returns_none_when_accuracy_unachievable() {
    // y = x^3 over [-1, 1] has inverse y^(1/3), which no low-order
    // polynomial approximates to 1e-10 over the full range.
    let pm = PolyMap::new(1, 1, &[term(1.0, 1, &[3])], None).unwrap();
    let fitted = pm
        .poly_tran(false, 1e-10, 1e-10, 3, &[-1.0], &[1.0])
        .unwrap();
    assert!(fitted.is_none());
}

#[test]
fn polytran_clears_iter_inverse() {
    let mut pm = linear_1d();
    pm.set_iter_inverse(true);
    let fitted = pm
        .poly_tran(false, 1e-8, 1e-6, 5, &[0.0], &[10.0])
        .unwrap()
        .expect("fit should succeed");
    assert!(!fitted.iter_inverse());
}

#[test]
fn polytran_rejects_unequal_nin_nout() {
    // 2-in 1-out PolyMap cannot be used.
    let pm = PolyMap::new(2, 1, &[term(1.0, 1, &[1, 0])], None).unwrap();
    assert!(
        pm.poly_tran(false, 1e-8, 1e-6, 5, &[0.0, 0.0], &[1.0, 1.0])
            .is_err()
    );
}

#[test]
fn polytran_rejects_missing_sampled_transformation() {
    // Creating an inverse requires the forward transformation
    // coefficients; an inverse-only PolyMap must error.
    let pm = PolyMap::new(1, 1, &[], Some(&[term(1.0, 1, &[1])])).unwrap();
    assert!(pm.poly_tran(false, 1e-8, 1e-6, 5, &[0.0], &[1.0]).is_err());
}

#[test]
fn polytran_fits_2d_sip_like_inverse() {
    // SIP-style distortion: y1 = x1 + 1e-3 x1 x2, y2 = x2 - 5e-4 x2^2
    // over [0, 100]^2.
    let pm = PolyMap::new(
        2,
        2,
        &[
            term(1.0, 1, &[1, 0]),
            term(1e-3, 1, &[1, 1]),
            term(1.0, 2, &[0, 1]),
            term(-5e-4, 2, &[0, 2]),
        ],
        None,
    )
    .unwrap();
    let fitted = pm
        .poly_tran(false, 1e-6, 1e-4, 10, &[0.0, 0.0], &[100.0, 100.0])
        .unwrap()
        .expect("fit should succeed");

    let x1 = [10.0_f64, 50.0, 90.0];
    let x2 = [20.0_f64, 60.0, 80.0];
    let pts = PointSet::from_columns(&[&x1, &x2]).unwrap();
    let ys = fitted.transform(&pts, true).unwrap();
    let back = fitted.transform(&ys, false).unwrap();
    for i in 0..3 {
        assert!(
            (back.point(i)[0] - x1[i]).abs() < 1e-4,
            "axis 1 round trip {i}: got {}, want {}",
            back.point(i)[0],
            x1[i]
        );
        assert!(
            (back.point(i)[1] - x2[i]).abs() < 1e-4,
            "axis 2 round trip {i}: got {}, want {}",
            back.point(i)[1],
            x2[i]
        );
    }
}

#[test]
fn polytran_replaces_forward_from_inverse() {
    // Inverse-only PolyMap (original inverse: x = 0.5 y): fit a new
    // forward transformation, which must approximate y = 2x.
    let pm = PolyMap::new(1, 1, &[], Some(&[term(0.5, 1, &[1])])).unwrap();
    let fitted = pm
        .poly_tran(true, 1e-8, 1e-6, 5, &[0.0], &[10.0])
        .unwrap()
        .expect("fit should succeed");
    let pts = PointSet::from_columns(&[&[4.0_f64]]).unwrap();
    let out = fitted.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 8.0).abs() < 1e-6,
        "got {}",
        out.point(0)[0]
    );
}
