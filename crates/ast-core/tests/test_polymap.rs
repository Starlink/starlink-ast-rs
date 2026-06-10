//! Tests for PolyMap — N-dimensional polynomial transformation.
//!
//! Ported from C's testpolymap.c and the Step 6 design spec.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, PolyMap, PolyTerm};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ---------------------------------------------------------------------------
// Construction and basic properties
// ---------------------------------------------------------------------------

#[test]
fn polymap_dimensions() {
    // y = 2x^2 + 3x + 1 (1D -> 1D)
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let m = PolyMap::new(1, 1, &terms, None).unwrap();
    assert_eq!(m.nin(), 1);
    assert_eq!(m.nout(), 1);
    assert!(!m.is_inverted());
}

#[test]
fn polymap_nin_zero_rejected() {
    let result = PolyMap::new(0, 1, &[], None);
    assert!(result.is_err());
}

#[test]
fn polymap_nout_zero_rejected() {
    let result = PolyMap::new(1, 0, &[], None);
    assert!(result.is_err());
}

#[test]
fn polymap_bad_powers_len_rejected() {
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 0],
            out_axis: 1,
        }, // 2 powers but nin=1
    ];
    let result = PolyMap::new(1, 1, &terms, None);
    assert!(result.is_err());
}

#[test]
fn polymap_bad_out_axis_rejected() {
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1],
            out_axis: 2,
        }, // out_axis=2 but nout=1
    ];
    let result = PolyMap::new(1, 1, &terms, None);
    assert!(result.is_err());
}

#[test]
fn polymap_zero_coeff_rejected() {
    let terms = vec![PolyTerm {
        coeff: 0.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let result = PolyMap::new(1, 1, &terms, None);
    assert!(result.is_err());
}

#[test]
fn polymap_defaults() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = PolyMap::new(1, 1, &terms, None).unwrap();
    // IterInverse defaults to "no inverse coefficients supplied"
    // (polymap.c:6311), so this forward-only PolyMap iterates by default.
    assert!(m.iter_inverse());
    assert_eq!(m.niter_inverse(), 4);
    assert!((m.tol_inverse() - 1e-6).abs() < 1e-15);
}

#[test]
fn polymap_iter_inverse_default_follows_inverse_coefficients() {
    // C: GetIterInverse defaults to (ncoeff_i == 0) — polymap.c:6311.
    let fwd = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = PolyMap::new(1, 1, &fwd, None).unwrap();
    assert!(m.iter_inverse(), "no inverse coefficients: default on");
    // The default makes the inverse usable immediately: x = y/2.
    let pts = PointSet::from_columns(&[&[8.0_f64]]).unwrap();
    let out = m.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 4.0).abs() < 1e-6);

    let inv = vec![PolyTerm {
        coeff: 0.5,
        powers: vec![1],
        out_axis: 1,
    }];
    let m2 = PolyMap::new(1, 1, &fwd, Some(&inv)).unwrap();
    assert!(
        !m2.iter_inverse(),
        "inverse coefficients present: default off"
    );

    // An explicit setting beats the default in both directions.
    let mut m3 = PolyMap::new(1, 1, &fwd, None).unwrap();
    m3.set_iter_inverse(false);
    assert!(!m3.iter_inverse());
    assert!(!m3.has_inverse());
}

// ---------------------------------------------------------------------------
// 1D polynomial: y = 2x^2 + 3x + 1
// ---------------------------------------------------------------------------

#[test]
fn polymap_1d_polynomial() {
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let m = PolyMap::new(1, 1, &terms, None).unwrap();

    // f(0) = 1, f(1) = 6, f(2) = 15
    let pts = PointSet::from_rows(vec![vec![0.0], vec![1.0], vec![2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0]);
    assert_eq!(out.point(1), &[6.0]);
    assert_eq!(out.point(2), &[15.0]);
}

// ---------------------------------------------------------------------------
// 2D->2D polynomial: out1 = x + y, out2 = x*y
// ---------------------------------------------------------------------------

#[test]
fn polymap_2d_to_2d() {
    let terms = vec![
        // out1 = x + y
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 1,
        },
        // out2 = x * y
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 1],
            out_axis: 2,
        },
    ];
    let m = PolyMap::new(2, 2, &terms, None).unwrap();

    // (2, 3) -> (5, 6)
    let pts = PointSet::from_rows(vec![vec![2.0, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 6.0).abs() < 1e-12);

    // (0, 0) -> (0, 0)
    let pts2 = PointSet::from_rows(vec![vec![0.0, 0.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0), &[0.0, 0.0]);

    // (-1, 4) -> (3, -4)
    let pts3 = PointSet::from_rows(vec![vec![-1.0, 4.0]]);
    let out3 = m.transform(&pts3, true).unwrap();
    assert!((out3.point(0)[0] - 3.0).abs() < 1e-12);
    assert!((out3.point(0)[1] - (-4.0)).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Explicit inverse: forward y = 2x, inverse x = y/2
// ---------------------------------------------------------------------------

#[test]
fn polymap_explicit_inverse_roundtrip() {
    let fwd = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv = vec![PolyTerm {
        coeff: 0.5,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = PolyMap::new(1, 1, &fwd, Some(&inv)).unwrap();

    // Forward: x=5 -> y=10
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 10.0).abs() < 1e-12);

    // Inverse: y=10 -> x=5
    let inv_out = m.transform(&out, false).unwrap();
    assert!((inv_out.point(0)[0] - 5.0).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Iterative (LM) inverse: forward y = x^2 + 1
// ---------------------------------------------------------------------------

#[test]
fn polymap_iterative_inverse() {
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![0],
            out_axis: 1,
        }, // constant 1
        PolyTerm {
            coeff: 1.0,
            powers: vec![2],
            out_axis: 1,
        }, // x^2
    ];
    let mut m = PolyMap::new(1, 1, &fwd, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(100);
    m.set_tol_inverse(1e-10);

    // y = 2 -> x^2 = 1 -> x = +/-1
    // The LM solver will find one root depending on the initial guess.
    // Initial guess is the target value (y=2), so x0=2, which should
    // converge to x=1.
    let pts = PointSet::from_rows(vec![vec![2.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert!(
        (out.point(0)[0] - 1.0).abs() < 1e-4,
        "expected ~1.0, got {}",
        out.point(0)[0]
    );

    // y = 5 -> x^2 = 4 -> x = +/-2
    // Initial guess x0=5, should converge to x=2.
    let pts2 = PointSet::from_rows(vec![vec![5.0]]);
    let out2 = m.transform(&pts2, false).unwrap();
    assert!(
        (out2.point(0)[0] - 2.0).abs() < 1e-4,
        "expected ~2.0, got {}",
        out2.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// No inverse available: all outputs BAD
// ---------------------------------------------------------------------------

#[test]
fn polymap_no_inverse_returns_bad() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![2],
        out_axis: 1,
    }];
    // IterInverse defaults on for an inverse-less PolyMap
    // (polymap.c:6311); disable it to exercise the no-inverse path.
    let mut m = PolyMap::new(1, 1, &fwd, None).unwrap();
    m.set_iter_inverse(false);

    let pts = PointSet::from_rows(vec![vec![3.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

// ---------------------------------------------------------------------------
// BAD_VALUE propagation
// ---------------------------------------------------------------------------

#[test]
fn polymap_bad_value_propagation() {
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let m = PolyMap::new(2, 2, &terms, None).unwrap();

    // One BAD input -> all outputs BAD
    let pts = PointSet::from_rows(vec![vec![BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);

    // NaN input also -> BAD output
    let pts2 = PointSet::from_rows(vec![vec![f64::NAN, 3.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], BAD_VALUE);
    assert_eq!(out2.point(0)[1], BAD_VALUE);
}

// ---------------------------------------------------------------------------
// Multiple terms contributing to same axis
// ---------------------------------------------------------------------------

#[test]
fn polymap_multi_term_same_axis() {
    // y = 5x^3 + 3x^2 + x + 7
    let terms = vec![
        PolyTerm {
            coeff: 7.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![2],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![3],
            out_axis: 1,
        },
    ];
    let m = PolyMap::new(1, 1, &terms, None).unwrap();

    // f(0) = 7
    // f(1) = 5 + 3 + 1 + 7 = 16
    // f(2) = 40 + 12 + 2 + 7 = 61
    let pts = PointSet::from_rows(vec![vec![0.0], vec![1.0], vec![2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 7.0).abs() < 1e-12);
    assert!((out.point(1)[0] - 16.0).abs() < 1e-12);
    assert!((out.point(2)[0] - 61.0).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Inversion of the mapping
// ---------------------------------------------------------------------------

#[test]
fn polymap_invert() {
    let fwd = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv = vec![PolyTerm {
        coeff: 0.5,
        powers: vec![1],
        out_axis: 1,
    }];
    let mut m = PolyMap::new(1, 1, &fwd, Some(&inv)).unwrap();

    // Before inversion: forward is y=2x, inverse is x=y/2
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 10.0).abs() < 1e-12);

    // After inversion: "forward" call uses inverse polynomial
    m.invert();
    assert!(m.is_inverted());
    assert_eq!(m.nin(), 1); // nin/nout swap when inverted
    assert_eq!(m.nout(), 1);

    let out2 = m.transform(&pts, true).unwrap();
    assert!((out2.point(0)[0] - 2.5).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Setters
// ---------------------------------------------------------------------------

#[test]
fn polymap_setters() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let mut m = PolyMap::new(1, 1, &terms, None).unwrap();

    m.set_iter_inverse(true);
    assert!(m.iter_inverse());

    m.set_niter_inverse(10);
    assert_eq!(m.niter_inverse(), 10);

    m.set_tol_inverse(1e-8);
    assert!((m.tol_inverse() - 1e-8).abs() < 1e-15);
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn polymap_serialisation_roundtrip() {
    // 2D->2D with forward and inverse
    let fwd = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
    ];
    let inv = vec![
        PolyTerm {
            coeff: 0.5,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.25,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let m = PolyMap::new(2, 2, &fwd, Some(&inv)).unwrap();

    let native = to_native_string(&AnyObject::PolyMap(m.clone())).unwrap();
    let obj = from_native_string(&native).unwrap();
    let m2 = match obj {
        AnyObject::PolyMap(p) => p,
        other => panic!("expected PolyMap, got {:?}", other),
    };

    // Verify forward transform produces the same results.
    let pts = PointSet::from_rows(vec![vec![2.0, 3.0]]);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    for j in 0..2 {
        assert!(
            (out1.point(0)[j] - out2.point(0)[j]).abs() < 1e-12,
            "axis {}: {} vs {}",
            j,
            out1.point(0)[j],
            out2.point(0)[j]
        );
    }

    // Verify inverse transform.
    let inv_out1 = m.transform(&pts, false).unwrap();
    let inv_out2 = m2.transform(&pts, false).unwrap();
    for j in 0..2 {
        assert!(
            (inv_out1.point(0)[j] - inv_out2.point(0)[j]).abs() < 1e-12,
            "inv axis {}: {} vs {}",
            j,
            inv_out1.point(0)[j],
            inv_out2.point(0)[j]
        );
    }
}

#[test]
fn polymap_serialisation_forward_only() {
    let fwd = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![2],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0],
            out_axis: 1,
        },
    ];
    let m = PolyMap::new(1, 1, &fwd, None).unwrap();

    let native = to_native_string(&AnyObject::PolyMap(m.clone())).unwrap();
    let obj = from_native_string(&native).unwrap();
    let m2 = match obj {
        AnyObject::PolyMap(p) => p,
        _ => panic!("expected PolyMap"),
    };

    // Check that transform still works after round-trip.
    let pts = PointSet::from_rows(vec![vec![0.0], vec![1.0], vec![2.0]]);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert!(
            (out1.point(i)[0] - out2.point(i)[0]).abs() < 1e-12,
            "point {}: {} vs {}",
            i,
            out1.point(i)[0],
            out2.point(i)[0]
        );
    }
}

#[test]
fn polymap_serialisation_iter_inverse_attributes() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let mut m = PolyMap::new(1, 1, &fwd, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(10);
    m.set_tol_inverse(1e-8);

    let native = to_native_string(&AnyObject::PolyMap(m)).unwrap();
    let obj = from_native_string(&native).unwrap();
    let m2 = match obj {
        AnyObject::PolyMap(p) => p,
        _ => panic!("expected PolyMap"),
    };

    assert!(m2.iter_inverse());
    assert_eq!(m2.niter_inverse(), 10);
    assert!((m2.tol_inverse() - 1e-8).abs() < 1e-15);
}

// ---------------------------------------------------------------------------
// 2D->1D polynomial: out = x^2 + y^2
// ---------------------------------------------------------------------------

#[test]
fn polymap_2d_to_1d() {
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![2, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 2],
            out_axis: 1,
        },
    ];
    let m = PolyMap::new(2, 1, &terms, None).unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 1);

    let pts = PointSet::from_rows(vec![vec![3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 25.0).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Constant polynomial: y = 42
// ---------------------------------------------------------------------------

#[test]
fn polymap_constant() {
    let terms = vec![PolyTerm {
        coeff: 42.0,
        powers: vec![0],
        out_axis: 1,
    }];
    let m = PolyMap::new(1, 1, &terms, None).unwrap();

    let pts = PointSet::from_rows(vec![vec![0.0], vec![100.0], vec![-50.0]]);
    let out = m.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert!((out.point(i)[0] - 42.0).abs() < 1e-12);
    }
}

// ---------------------------------------------------------------------------
// Empty forward terms (no forward transform)
// ---------------------------------------------------------------------------

#[test]
fn polymap_empty_forward() {
    // A PolyMap with no forward terms — all forward outputs are 0.
    let m = PolyMap::new(1, 1, &[], None).unwrap();
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 0.0);
}

// ---------------------------------------------------------------------------
// Newton-Raphson inverse with 2D->2D polynomial
// ---------------------------------------------------------------------------

#[test]
fn polymap_nr_inverse_2d() {
    // Forward: u = x + 1, v = y + 2 (simple linear shift)
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
    ];
    let mut m = PolyMap::new(2, 2, &fwd, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(100);
    m.set_tol_inverse(1e-10);

    // Forward (3, 4) -> (4, 6)
    let pts = PointSet::from_rows(vec![vec![3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 4.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 6.0).abs() < 1e-12);

    // Inverse (4, 6) -> should give back (3, 4)
    let inv = m.transform(&out, false).unwrap();
    assert!(
        (inv.point(0)[0] - 3.0).abs() < 1e-3,
        "expected ~3.0, got {}",
        inv.point(0)[0]
    );
    assert!(
        (inv.point(0)[1] - 4.0).abs() < 1e-3,
        "expected ~4.0, got {}",
        inv.point(0)[1]
    );
}

// ---------------------------------------------------------------------------
// Bulk Newton-Raphson inverse: 1D, 1000 points
// ---------------------------------------------------------------------------

#[test]
fn polymap_bulk_nr_inverse_1d() {
    // Forward: y = 2x + 3 (linear)
    let fwd = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0],
            out_axis: 1,
        },
    ];
    let mut m = PolyMap::new(1, 1, &fwd, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(20);
    m.set_tol_inverse(1e-10);

    // Generate 1000 input points spanning [-50, 50].
    let npoints = 1000;
    let inputs: Vec<Vec<f64>> = (0..npoints)
        .map(|i| vec![-50.0 + 100.0 * (i as f64) / (npoints as f64 - 1.0)])
        .collect();
    let pts = PointSet::from_rows(inputs.clone());

    // Forward transform.
    let fwd_out = m.transform(&pts, true).unwrap();

    // Inverse: should recover original inputs.
    let inv_out = m.transform(&fwd_out, false).unwrap();

    for (i, input) in inputs.iter().enumerate().take(npoints) {
        let expected = input[0];
        let got = inv_out.point(i)[0];
        assert!(
            (got - expected).abs() < 1e-6,
            "point {}: expected {}, got {}",
            i,
            expected,
            got
        );
    }
}

// ---------------------------------------------------------------------------
// Bulk Newton-Raphson inverse: 2D, 1000 points, non-linear polynomial
// ---------------------------------------------------------------------------

#[test]
fn polymap_bulk_nr_inverse_2d() {
    // Forward: u = x + 0.01*x*y, v = y + 0.01*x^2
    // This is a mildly nonlinear 2D->2D map that is invertible near
    // the origin.
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.01,
            powers: vec![1, 1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 0.01,
            powers: vec![2, 0],
            out_axis: 2,
        },
    ];
    let mut m = PolyMap::new(2, 2, &fwd, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(20);
    m.set_tol_inverse(1e-10);

    // Generate a 32x32 grid of points in [-5, 5] x [-5, 5] = 1024 points.
    let n_side = 32;
    let mut inputs = Vec::with_capacity(n_side * n_side);
    for i in 0..n_side {
        for j in 0..n_side {
            let x = -5.0 + 10.0 * (i as f64) / (n_side as f64 - 1.0);
            let y = -5.0 + 10.0 * (j as f64) / (n_side as f64 - 1.0);
            inputs.push(vec![x, y]);
        }
    }
    let npoints = inputs.len();
    let pts = PointSet::from_rows(inputs.clone());

    // Forward transform.
    let fwd_out = m.transform(&pts, true).unwrap();

    // Inverse: should recover original inputs.
    let inv_out = m.transform(&fwd_out, false).unwrap();

    for (i, input) in inputs.iter().enumerate().take(npoints) {
        let ex = input[0];
        let ey = input[1];
        let gx = inv_out.point(i)[0];
        let gy = inv_out.point(i)[1];
        assert!(
            (gx - ex).abs() < 1e-6 && (gy - ey).abs() < 1e-6,
            "point {} ({}, {}): expected ({}, {}), got ({}, {})",
            i,
            ex,
            ey,
            ex,
            ey,
            gx,
            gy
        );
    }
}

// ---------------------------------------------------------------------------
// Coefficient consolidation in simplified()
// ---------------------------------------------------------------------------

#[test]
fn polymap_removes_zero_terms() {
    // Two terms that cancel: 2*x + (-2)*x → 0
    let terms = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: -2.0,
            powers: vec![1],
            out_axis: 1,
        },
    ];
    let pm = PolyMap::new(1, 1, &terms, None).unwrap();
    let s = pm.simplified().unwrap();

    // The result should produce 0 for any input.
    let pts = PointSet::from_rows(vec![vec![7.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 0.0);
}

#[test]
fn polymap_consolidates_duplicate_terms() {
    // Two terms with same powers: 2*x^2 + 3*x^2 → 5*x^2
    let terms = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![2],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let pm = PolyMap::new(1, 1, &terms, None).unwrap();
    let s = pm.simplified().unwrap();

    let pts = PointSet::from_rows(vec![vec![3.0]]);
    let out = s.transform(&pts, true).unwrap();
    // 5 * 3^2 = 45
    assert_eq!(out.point(0)[0], 45.0);
}

#[test]
fn polymap_simplified_no_change_when_no_duplicates() {
    // No duplicates: y = 2x^2 + 3x + 1
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let pm = PolyMap::new(1, 1, &terms, None).unwrap();
    let s = pm.simplified().unwrap();

    // f(2) = 2*4 + 3*2 + 1 = 15
    let pts = PointSet::from_rows(vec![vec![2.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 15.0);
}

#[test]
fn polymap_simplified_consolidates_multi_output() {
    // 2D->2D with duplicate terms on different outputs
    let terms = vec![
        // out1: 2*x + 3*x = 5*x
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        // out2: 4*y (no duplicates)
        PolyTerm {
            coeff: 4.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let pm = PolyMap::new(2, 2, &terms, None).unwrap();
    let s = pm.simplified().unwrap();

    // (3, 2) -> out1 = 5*3 = 15, out2 = 4*2 = 8
    let pts = PointSet::from_rows(vec![vec![3.0, 2.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 15.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 8.0).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Affine decomposition in simplified()
// ---------------------------------------------------------------------------

#[test]
fn polymap_linear_decomposes_to_matrixmap() {
    use ast_core::mappings::MatrixMap;

    // Purely linear: out1 = 2*x + 3*y, out2 = 4*x + 5*y
    let terms = vec![
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 4.0,
            powers: vec![1, 0],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let pm = PolyMap::new(2, 2, &terms, None).unwrap();
    let s = pm.simplified().unwrap();
    assert!(
        s.as_any().is::<MatrixMap>(),
        "Expected MatrixMap, got {:?}",
        s
    );

    let pts = PointSet::from_rows(vec![vec![1.0, 2.0]]);
    let expected = pm.transform(&pts, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
    assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
}

#[test]
fn polymap_affine_decomposes() {
    // Affine: out1 = 10 + 2*x, out2 = 20 + 3*y
    let terms = vec![
        PolyTerm {
            coeff: 10.0,
            powers: vec![0, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 20.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let pm = PolyMap::new(2, 2, &terms, None).unwrap();
    let s = pm.simplified().unwrap();
    // Should NOT be a PolyMap anymore
    assert!(!s.as_any().is::<PolyMap>(), "Should decompose from PolyMap");

    let pts = PointSet::from_rows(vec![vec![3.0, 4.0]]);
    let expected = pm.transform(&pts, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
    assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
}

#[test]
fn polymap_affine_diagonal_decomposes_to_winmap() {
    use ast_core::mappings::WinMap;

    // Diagonal affine: out1 = 10 + 2*x, out2 = 20 + 3*y
    let terms = vec![
        PolyTerm {
            coeff: 10.0,
            powers: vec![0, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 20.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let pm = PolyMap::new(2, 2, &terms, None).unwrap();
    let s = pm.simplified().unwrap();
    assert!(
        s.as_any().is::<WinMap>(),
        "Expected WinMap for diagonal affine, got {:?}",
        s
    );
}

#[test]
fn polymap_affine_general_decomposes_to_cmpmap() {
    use ast_core::mappings::CmpMap;

    // General affine (non-diagonal): out1 = 10 + 2*x + 3*y, out2 = 20 + 4*x + 5*y
    let terms = vec![
        PolyTerm {
            coeff: 10.0,
            powers: vec![0, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 20.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 4.0,
            powers: vec![1, 0],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let pm = PolyMap::new(2, 2, &terms, None).unwrap();
    let s = pm.simplified().unwrap();
    assert!(
        s.as_any().is::<CmpMap>(),
        "Expected CmpMap for general affine, got {:?}",
        s
    );

    let pts = PointSet::from_rows(vec![vec![1.0, 2.0]]);
    let expected = pm.transform(&pts, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
    assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
}

#[test]
fn polymap_nonlinear_stays_polymap() {
    // Quadratic term: out = x^2
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![2],
        out_axis: 1,
    }];
    let pm = PolyMap::new(1, 1, &terms, None).unwrap();
    let s = pm.simplified().unwrap();
    assert!(s.as_any().is::<PolyMap>());
}

// ---------------------------------------------------------------------------
// Bulk Newton-Raphson inverse: BAD_VALUE propagation in bulk
// ---------------------------------------------------------------------------

#[test]
fn polymap_bulk_nr_inverse_bad_values() {
    // Forward: u = x + 1, v = y + 2
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
    ];
    let mut m = PolyMap::new(2, 2, &fwd, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(20);
    m.set_tol_inverse(1e-10);

    // Mix of good and BAD points.
    let pts = PointSet::from_rows(vec![
        vec![4.0, 6.0],       // good: should invert to (3, 4)
        vec![BAD_VALUE, 6.0], // BAD input
        vec![5.0, 7.0],       // good: should invert to (4, 5)
        vec![4.0, BAD_VALUE], // BAD input
    ]);
    let inv = m.transform(&pts, false).unwrap();

    // Point 0: good
    assert!((inv.point(0)[0] - 3.0).abs() < 1e-3);
    assert!((inv.point(0)[1] - 4.0).abs() < 1e-3);

    // Point 1: BAD
    assert_eq!(inv.point(1)[0], BAD_VALUE);
    assert_eq!(inv.point(1)[1], BAD_VALUE);

    // Point 2: good
    assert!((inv.point(2)[0] - 4.0).abs() < 1e-3);
    assert!((inv.point(2)[1] - 5.0).abs() < 1e-3);

    // Point 3: BAD
    assert_eq!(inv.point(3)[0], BAD_VALUE);
    assert_eq!(inv.point(3)[1], BAD_VALUE);
}

// ---------------------------------------------------------------------------
// IterInverse priority over explicit inverse coefficients
// ---------------------------------------------------------------------------

#[test]
fn iter_inverse_takes_priority_over_explicit_inverse_coefficients() {
    // C uses the iterative inverse in preference to any inverse
    // coefficients whenever IterInverse is set (polymap.c:6092) — the
    // supplied coefficients are typically only an approximation (the
    // FITS-SIP convention). Forward: y = 2x. Supplied inverse is the
    // deliberately wrong x = y so the two paths are distinguishable.
    let fwd = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let mut m = PolyMap::new(1, 1, &fwd, Some(&inv)).unwrap();

    // Without IterInverse the explicit (approximate) coefficients are used.
    let pts = PointSet::from_columns(&[&[8.0_f64]]).unwrap();
    let out = m.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 8.0).abs() < 1e-12);

    // With IterInverse set, the iterative inverse must win: x = y/2.
    m.set_iter_inverse(true);
    m.set_niter_inverse(20);
    m.set_tol_inverse(1e-10);
    let out = m.transform(&pts, false).unwrap();
    assert!(
        (out.point(0)[0] - 4.0).abs() < 1e-6,
        "expected iterative inverse (4.0), got {}",
        out.point(0)[0]
    );
}
