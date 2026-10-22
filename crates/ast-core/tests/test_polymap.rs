//! Tests for PolyMap — N-dimensional polynomial transformation.
//!
//! Ported from C's testpolymap.c and the Step 6 design spec.

use crate::common::native_dump;
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{Mapping, PolyMap, PolyTerm};
use ast_core::{AnyObject, from_native_string};

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
    m3.set_iter_inverse(false).unwrap();
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
    m.set_iter_inverse(true).unwrap();
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
// No inverse available: transforming inversely is an error
// ---------------------------------------------------------------------------

#[test]
fn polymap_no_inverse_is_an_error() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![2],
        out_axis: 1,
    }];
    // IterInverse defaults on for an inverse-less PolyMap
    // (polymap.c:6311); disable it to exercise the no-inverse path.
    let mut m = PolyMap::new(1, 1, &fwd, None).unwrap();
    m.set_iter_inverse(false).unwrap();

    // C's base Transform raises AST__TRNND for an undefined direction
    // (mapping.c).
    let pts = PointSet::from_rows(vec![vec![3.0]]);
    let err = m.transform(&pts, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Trnnd);
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

    // A BAD input spoils only the output whose terms use it (polymap.c
    // Transform): y2 = x2 stays good.
    let pts = PointSet::from_rows(vec![vec![BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 3.0);

    // So does a NaN input.
    let pts2 = PointSet::from_rows(vec![vec![f64::NAN, 3.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], BAD_VALUE);
    assert_eq!(out2.point(0)[1], 3.0);
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

    m.set_iter_inverse(true).unwrap();
    assert!(m.iter_inverse());

    m.set_niter_inverse(10);
    assert_eq!(m.niter_inverse(), 10);

    m.set_tol_inverse(1e-8);
    assert!((m.tol_inverse() - 1e-8).abs() < 1e-15);
}

// ---------------------------------------------------------------------------
// Serialization round-trip
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

    let native = native_dump(&AnyObject::PolyMap(m.clone())).unwrap();
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

    let native = native_dump(&AnyObject::PolyMap(m.clone())).unwrap();
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
    m.set_iter_inverse(true).unwrap();
    m.set_niter_inverse(10);
    m.set_tol_inverse(1e-8);

    let native = native_dump(&AnyObject::PolyMap(m)).unwrap();
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
    // A PolyMap with no forward terms has no forward transformation
    // (polymap.c GetTranForward), so transforming forward is AST__TRNND.
    let m = PolyMap::new(1, 1, &[], None).unwrap();
    assert!(!m.has_forward());
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let err = m.transform(&pts, true).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Trnnd);
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
    m.set_iter_inverse(true).unwrap();
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
    m.set_iter_inverse(true).unwrap();
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
    m.set_iter_inverse(true).unwrap();
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
    let s = <dyn Mapping>::simplify(&pm).unwrap();

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
    let s = <dyn Mapping>::simplify(&pm).unwrap();

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
    let s = <dyn Mapping>::simplify(&pm).unwrap();

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
    let s = <dyn Mapping>::simplify(&pm).unwrap();

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
    let s = <dyn Mapping>::simplify(&pm).unwrap();
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
    let s = <dyn Mapping>::simplify(&pm).unwrap();
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
    let s = <dyn Mapping>::simplify(&pm).unwrap();
    assert!(
        s.as_any().is::<WinMap>(),
        "Expected WinMap for diagonal affine, got {:?}",
        s
    );
}

/// The general affine PolyMap out1 = 10 + 2x + 3y, out2 = 20 + 4x + 5y.
fn general_affine() -> PolyMap {
    PolyMap::new(
        2,
        2,
        &[
            term(10.0, &[0, 0], 1),
            term(2.0, &[1, 0], 1),
            term(3.0, &[0, 1], 1),
            term(20.0, &[0, 0], 2),
            term(4.0, &[1, 0], 2),
            term(5.0, &[0, 1], 2),
        ],
        None,
    )
    .unwrap()
}

/// [`general_affine`] followed by a ZoomMap of 2.
fn general_affine_then_zoom() -> ast_core::mappings::CmpMap {
    use ast_core::mappings::{CmpMap, Composition, ZoomMap};
    CmpMap::new(
        Box::new(general_affine()),
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Composition::Series,
    )
    .unwrap()
}

/// The classes in a Mapping's dump, in order.
fn dump_classes(m: &dyn Mapping) -> Vec<String> {
    native_dump(&AnyObject::from_mapping(m).unwrap())
        .unwrap()
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Begin "))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

/// A general affine PolyMap, alone and followed by a ZoomMap, simplifies to
/// Mappings that transform as it does.
#[test]
fn polymap_affine_general_simplifies_to_an_equivalent_mapping() {
    let pts = PointSet::from_rows(vec![vec![1.0, 2.0]]);
    for m in [
        Box::new(general_affine()) as ast_core::mappings::DynMapping,
        Box::new(general_affine_then_zoom()),
    ] {
        let s = m.simplify().unwrap();
        let expected = m.transform(&pts, true).unwrap();
        let actual = s.transform(&pts, true).unwrap();
        assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
        assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
    }
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
    let s = <dyn Mapping>::simplify(&pm).unwrap();
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
    m.set_iter_inverse(true).unwrap();
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

    // Point 1: a bad u gives a bad x, and C's iteration carries the bad
    // value through its arithmetic without spoiling y, which C gives as 4.
    assert_eq!(inv.point(1)[0], BAD_VALUE);
    assert_eq!(inv.point(1)[1], 4.0);

    // Point 2: good
    assert!((inv.point(2)[0] - 4.0).abs() < 1e-3);
    assert!((inv.point(2)[1] - 5.0).abs() < 1e-3);

    // Point 3: likewise, C gives x = 3 and a bad y.
    assert_eq!(inv.point(3)[0], 3.0);
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
    m.set_iter_inverse(true).unwrap();
    m.set_niter_inverse(20);
    m.set_tol_inverse(1e-10);
    let out = m.transform(&pts, false).unwrap();
    assert!(
        (out.point(0)[0] - 4.0).abs() < 1e-6,
        "expected iterative inverse (4.0), got {}",
        out.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// The two reductions C runs at the head of `MapMerge`
// ---------------------------------------------------------------------------

fn term(coeff: f64, powers: &[usize], out_axis: usize) -> PolyTerm {
    PolyTerm {
        coeff,
        powers: powers.to_vec(),
        out_axis,
    }
}

#[test]
fn polymap_whose_terms_all_cancel_becomes_a_matrixmap() {
    use ast_core::mappings::MatrixMap;

    // Two constant terms that sum to zero. Consolidating them leaves the
    // forward transformation with no coefficients, but it is still defined, so
    // C's linear test runs and rebuilds the PolyMap out of a zero MatrixMap and
    // a zero ShiftMap (polymap.c:3757).
    let fwd = vec![term(1.0, &[0], 1), term(-1.0, &[0], 1)];
    let pm = PolyMap::new(1, 1, &fwd, None).unwrap();

    let s = <dyn Mapping>::simplify(&pm).unwrap();
    assert!(
        s.as_any().is::<MatrixMap>(),
        "a PolyMap whose terms cancel is linear, so it reduces like any other \
         linear PolyMap"
    );
}

#[test]
fn polymap_does_not_cancel_against_a_different_inverse() {
    use ast_core::mappings::{CmpMap, Composition, UnitMap};

    // Identical forward transformations, inverse ones with different coefficient
    // counts. C forces both Invert flags to the list entry's direction and
    // compares the pair with astEqual (polymap.c:3917-3920), whose PolyMap
    // implementation tests the inverse coefficient counts (polymap.c:758-766),
    // so the two do not cancel.
    //
    // The forward transformation has to be non-linear: C's linear half runs
    // first, and it replaces a linear PolyMap with a MatrixMap and a ShiftMap
    // built from the forward coefficients alone, discarding the inverse
    // polynomial that distinguishes the pair.
    let fwd = vec![term(2.0, &[2], 1)];
    let a = PolyMap::new(1, 1, &fwd, Some(&[term(0.5, &[1], 1)])).unwrap();
    let b = PolyMap::new(1, 1, &fwd, Some(&[term(0.5, &[1], 1), term(1.0, &[2], 1)])).unwrap();

    let mut b_inv = b;
    b_inv.invert();
    let cm = CmpMap::new(Box::new(a), Box::new(b_inv), Composition::Series).unwrap();
    let s = <dyn Mapping>::simplify(&cm).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "PolyMaps whose inverse transformations have different coefficient \
         counts are not equal, so the pair does not cancel"
    );
}

/// `equal` compares the inverse transformation's own coefficients, and every
/// array by the axis count it was allocated with.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testpolymap.c`, checks
/// 8019-8024.
///
/// The forward transformation is `y = x^2` throughout, because a linear one is
/// rebuilt as a MatrixMap and a ShiftMap by the branch above the neighbor half
/// of `MapMerge`, which does not consult the explicit inverse at all; a linear
/// pair cancels whatever `equal` says.
#[test]
fn polymap_equal_compares_the_inverse_coefficients() {
    use ast_core::mappings::{CmpMap, Composition, UnitMap};

    let fwd = vec![term(1.0, &[2], 1)];
    let half = [term(0.5, &[1], 1)];
    let seven = [term(0.7, &[1], 1)];

    // Same forward transformation and same inverse: equal, both ways round.
    let a = PolyMap::new(1, 1, &fwd, Some(&half)).unwrap();
    let same = PolyMap::new(1, 1, &fwd, Some(&half)).unwrap();
    assert!(a.equal(&same), "identical PolyMaps compare equal");
    assert!(same.equal(&a), "equality is symmetric");

    // Same forward transformation, inverses of the same shape but different
    // coefficient values: not equal.
    let differ = PolyMap::new(1, 1, &fwd, Some(&seven)).unwrap();
    assert!(
        !a.equal(&differ),
        "the inverse coefficient values differ, so the two are not equal"
    );
    assert!(!differ.equal(&a), "in either direction");

    // The consequence in MapMerge: a PolyMap and a neighbor used in the
    // opposite direction are replaced by a UnitMap when the two compare equal,
    // so a pair whose inverses differ must not cancel.
    let mut differ_inv = differ;
    differ_inv.invert();
    let cm = CmpMap::new(
        Box::new(PolyMap::new(1, 1, &fwd, Some(&half)).unwrap()),
        Box::new(differ_inv),
        Composition::Series,
    )
    .unwrap();
    let s = <dyn Mapping>::simplify(&cm).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "the inverse transformations are different polynomials, so the pair \
         stands"
    );

    // The same pair with equal inverses still cancels. This asserts what `equal`
    // permits, not that the composition is preserved: y = x^2 and x = 0.5y are
    // not mutually inverse, so cancelling them really does change the transform.
    // That is a property of the cancellation branch assuming the two directions
    // of a PolyMap are a genuine inverse pair, not of the comparison tested
    // here; `polymap_fitted_inverse_cancels_soundly` is the control that holds
    // the transform.
    let mut same_inv = PolyMap::new(1, 1, &fwd, Some(&half)).unwrap();
    same_inv.invert();
    let cm = CmpMap::new(
        Box::new(PolyMap::new(1, 1, &fwd, Some(&half)).unwrap()),
        Box::new(same_inv),
        Composition::Series,
    )
    .unwrap();
    let s = <dyn Mapping>::simplify(&cm).unwrap();
    assert!(
        s.as_any().is::<UnitMap>(),
        "identical PolyMaps in opposite directions cancel"
    );
}

/// With a fitted inverse, the cancellation is sound: the composition it
/// collapses is close to the identity that replaces it.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testpolymap.c`, checks
/// 8025-8028.
///
/// A non-linear PolyMap cannot have an exact polynomial inverse — if `f` and `g`
/// are both polynomials with `g(f(x)) = x` their degrees multiply to one, so
/// both are linear — and the forward has to stay non-linear to reach `equal` at
/// all. A fitted inverse is therefore the best a real caller has, and is what
/// makes this the meaningful positive control.
#[test]
fn polymap_fitted_inverse_cancels_soundly() {
    use ast_core::mappings::{CmpMap, Composition, UnitMap};

    let raw = PolyMap::new(1, 1, &[term(1.0, &[2], 1)], None).unwrap();
    // Fit the inverse over x in [1, 1.1], i.e. y in [1, 1.21].
    let pf = raw
        .poly_tran(false, 1.0e-9, 1.0e-6, 6, &[1.0], &[1.21])
        .unwrap()
        .expect("an inverse fit over [1, 1.21] converges");

    let xs: Vec<f64> = (0..5).map(|i| 1.0 + f64::from(i) * 0.025).collect();
    let mut pts = PointSet::zeros(5, 1);
    for (i, &x) in xs.iter().enumerate() {
        pts.point_mut(i)[0] = x;
    }

    // The fitted inverse really does invert the forward over the fitted range.
    let fwd_out = pf.transform(&pts, true).unwrap();
    let back = pf.transform(&fwd_out, false).unwrap();
    for (i, &x) in xs.iter().enumerate() {
        let got = back.point(i)[0];
        assert!(
            (got - x).abs() <= 1.0e-6,
            "the fit does not invert the forward at x = {x}: got {got}"
        );
    }

    // The pair cancels, and the composition agrees with the UnitMap it becomes.
    let mut pfi = pf.clone();
    pfi.invert();
    let series = CmpMap::new(Box::new(pf), Box::new(pfi), Composition::Series).unwrap();
    let simp = <dyn Mapping>::simplify(&series).unwrap();
    assert!(
        simp.as_any().is::<UnitMap>(),
        "a PolyMap and its own inverse cancel"
    );

    let before = series.transform(&pts, true).unwrap();
    let after = simp.transform(&pts, true).unwrap();
    for i in 0..5 {
        let diff = (after.point(i)[0] - before.point(i)[0]).abs();
        assert!(
            diff <= 1.0e-6,
            "the cancellation changed the transform by {diff}"
        );
    }
}

/// A PolyMap with more outputs than inputs compares equal to its own copy.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testpolymap.c`, checks
/// 8023-8024.
///
/// The forward arrays have one element per output except `mxpow_f`, which holds
/// the maximum power of each input; the inverse arrays are the other way round.
/// C indexed `mxpow_f` and `ncoeff_i` by the output count, reading past the end
/// of both, so for `nin = 1` and `nout >= 5` a PolyMap and its own copy could
/// compare unequal on whatever the heap happened to hold. The port compares term
/// lists rather than the derived arrays, so it has no equivalent of the
/// out-of-range read; this covers the shape upstream's sanitizer run caught.
#[test]
fn polymap_with_more_outputs_than_inputs_equals_its_copy() {
    let nwide = 8;
    let fwd: Vec<PolyTerm> = (1..=nwide).map(|io| term(io as f64, &[1], io)).collect();
    let inv = [term(1.0, &vec![1; nwide], 1)];

    let wide = PolyMap::new(1, nwide, &fwd, Some(&inv)).unwrap();
    let copy = wide.clone();
    assert!(wide.equal(&copy), "a PolyMap equals its own copy");
    assert!(copy.equal(&wide), "in either direction");
}

#[test]
fn ident_set_polymap_nominated_in_a_list_is_left_alone() {
    use ast_core::AnyObject;
    use ast_core::mappings::{CmpMap, Composition, ZoomMap};

    // Two cubic terms over the same powers would consolidate into one, but the
    // PolyMap carries an Ident. C's MapMerge dispatcher returns without calling
    // the class method at all in that case (mapping.c:24257), and the
    // replace-with-simpler halves live inside the class method, so the
    // coefficients survive as constructed even though the PolyMap is nominated
    // inside a list rather than simplified on its own.
    let fwd = vec![term(1.0, &[3], 1), term(2.0, &[3], 1)];
    let mut pm = PolyMap::new(1, 1, &fwd, None).unwrap();
    pm.set_ident("keepme");

    let cm = CmpMap::new(
        Box::new(pm),
        Box::new(ZoomMap::new(1, 3.0).unwrap()),
        Composition::Parallel,
    )
    .unwrap();
    let s = <dyn Mapping>::simplify(&cm).unwrap();

    let text = native_dump(&AnyObject::from_mapping(s.as_ref()).unwrap()).unwrap();
    assert!(
        text.contains("Ident = \"keepme\""),
        "the Ident survives:\n{text}"
    );
    assert!(
        text.contains("NCF1 = 2"),
        "both cubic coefficients survive unconsolidated:\n{text}"
    );
}

/// A zero coefficient contributes nothing, and C's AddCoeff drops it rather
/// than refusing it (polymap.c), so a PolyMap given one equals one without.
#[test]
fn a_zero_coefficient_is_dropped() {
    let term = |coeff: f64, p: usize| PolyTerm {
        coeff,
        powers: vec![p],
        out_axis: 1,
    };
    let with_zero = PolyMap::new(1, 1, &[term(0.0, 0), term(2.0, 1)], None).unwrap();
    let without = PolyMap::new(1, 1, &[term(2.0, 1)], None).unwrap();
    assert!(with_zero.equal(&without));
    assert_eq!(with_zero.forward_terms().len(), 1);
}

/// `coeffs` is C's `astPolyCoeffs`: relative to the Invert flag, so an
/// inverted PolyMap's forward coefficients are the original inverse ones,
/// grouped by output (testmapping.c `testpolycoeffsinverted`, after the
/// upstream fix 436f069f for a PolyMap with different numbers of inputs and
/// outputs).
#[test]
fn coeffs_are_relative_to_invert_and_grouped_by_output() {
    let term = |coeff, out_axis, powers: Vec<usize>| PolyTerm {
        coeff,
        powers,
        out_axis,
    };
    let fwd = vec![term(2.0, 1, vec![1, 0]), term(3.0, 1, vec![0, 1])];
    let inv = vec![term(0.25, 2, vec![1]), term(0.5, 1, vec![1])];
    let mut pm = PolyMap::new(2, 1, &fwd, Some(&inv)).unwrap();
    let grouped = vec![term(0.5, 1, vec![1]), term(0.25, 2, vec![1])];
    assert_eq!(pm.coeffs(true), fwd);
    assert_eq!(pm.coeffs(false), grouped);
    pm.invert();
    assert_eq!(pm.coeffs(true), grouped);
    assert_eq!(pm.coeffs(false), fwd);
}

// ---------------------------------------------------------------------------
// testpolymap.c, in its order: coefficients, PolyTran fits of the inverse
// with their round trips (explicit, then iterative), simplification, and the
// IterInverse attribute. Assertion messages carry C's error numbers. Errors
// 8019-8028 are the tests above.
// ---------------------------------------------------------------------------

/// Terms from a C coefficient array: each coefficient is followed by its
/// output axis and its `nin` powers.
fn c_terms(nin: usize, cofs: &[f64]) -> Vec<PolyTerm> {
    cofs.chunks(nin + 2)
        .map(|c| {
            term(
                c[0],
                &c[2..].iter().map(|&p| p as usize).collect::<Vec<_>>(),
                c[1] as usize,
            )
        })
        .collect()
}

const C_COEFF: [f64; 16] = [
    1.0, 1.0, 0.0, 0.0, 2.0, 1.0, 1.0, 0.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0, 0.0, 1.0,
];
const C_COEFFB: [f64; 24] = [
    1.0, 1.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 2.0, 1.0, 1.0, 0.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0, 0.0,
    1.0, -0.5, 2.0, 0.0, 1.0,
];
const C_COEFF2: [f64; 24] = [
    1.0, 1.0, 0.0, 0.0, 2.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 2.0, 0.0, 0.0, 1.0, 2.0, 1.0,
    0.0, 2.0, 2.0, 0.0, 1.0,
];
const C_COEFF3: [f64; 24] = [
    -0.1, 1.0, 0.0, 0.0, 0.99, 1.0, 1.0, 0.0, 1.0e-4, 1.0, 1.0, 1.0, -0.1, 2.0, 0.0, 0.0, 0.99,
    2.0, 0.0, 1.0, 1.0e-4, 2.0, 1.0, 1.0,
];
const C_ACC: f64 = 1.0e-7;
const C_MAXACC: f64 = 1.0e-3;

fn c_polymap(nin: usize, cofs: &[f64]) -> PolyMap {
    PolyMap::new(nin, nin, &c_terms(nin, cofs), None).unwrap()
}

fn rows(m: &dyn Mapping, pts: &[Vec<f64>], forward: bool) -> Vec<Vec<f64>> {
    let out = m
        .transform(&PointSet::from_rows(pts.to_vec()), forward)
        .unwrap();
    (0..pts.len()).map(|i| out.point(i).to_vec()).collect()
}

/// C's PolyTran check: fit the inverse over [-1000, 1000] on each axis,
/// transform three points forward and back through the fit, then back
/// again through the iterative inverse, each within 1000 times the
/// accuracy. `errs` are C's error numbers for the two round trips.
fn c_poly_tran_round_trips(pm: &PolyMap, errs: (u32, u32)) {
    let errlim = 1000.0 * C_ACC;
    let n = pm.nin();
    let mut pm2 = pm
        .poly_tran(
            false,
            C_ACC,
            C_MAXACC,
            10,
            &vec![-1000.0; n],
            &vec![1000.0; n],
        )
        .unwrap()
        .unwrap_or_else(|| panic!("Error {}: no fit", errs.0));
    let xin: Vec<Vec<f64>> = [1.0, 100.0, -50.0].iter().map(|&v| vec![v; n]).collect();
    let xout = rows(&pm2, &xin, true);
    let check = |back: Vec<Vec<f64>>, err: u32| {
        for (b, x) in back.iter().zip(&xin) {
            for (bv, xv) in b.iter().zip(x) {
                assert!((bv - xv).abs() <= errlim, "Error {err}: {b:?} vs {x:?}");
            }
        }
    };
    check(rows(&pm2, &xout, false), errs.0);
    pm2.set_iter_inverse(true).unwrap();
    check(rows(&pm2, &xout, false), errs.1);
}

#[test]
fn c_coefficients() {
    let pm = c_polymap(2, &C_COEFF);
    assert_eq!(pm.coeffs(true).len(), 4, "Error -1");
    assert_eq!(pm.coeffs(false).len(), 0, "Error -2");
    assert_eq!(pm.coeffs(true), c_terms(2, &C_COEFF), "Error -3, -4");
}

#[test]
fn c_poly_tran_2d_linear() {
    c_poly_tran_round_trips(&c_polymap(2, &C_COEFF), (1, 1001));
}

#[test]
fn c_poly_tran_1d() {
    c_poly_tran_round_trips(&c_polymap(1, &[1.0, 1.0, 0.0, 2.0, 1.0, 1.0]), (3, 3001));
}

#[test]
fn c_poly_tran_2d_nonlinear() {
    c_poly_tran_round_trips(&c_polymap(2, &C_COEFF2), (4, 4001));
}

#[test]
fn c_poly_tran_2d_near_unit() {
    c_poly_tran_round_trips(&c_polymap(2, &C_COEFF3), (6, 6001));
}

/// Linear PolyMaps simplify to WinMaps, with or without terms that add up;
/// a non-linear one stays an equal PolyMap.
#[test]
fn c_simplify() {
    use ast_core::mappings::WinMap;

    for (cofs, want, errs) in [
        (&C_COEFF[..], [3.0, 7.0], (1003, 1004)),
        (&C_COEFFB[..], [3.5, 6.0], (1005, 1006)),
    ] {
        let pm = c_polymap(2, cofs);
        let simp = (&pm as &dyn Mapping).simplify().unwrap();
        assert!(simp.as_ref().as_any().is::<WinMap>(), "Error {}", errs.0);
        assert_eq!(
            rows(simp.as_ref(), &[vec![1.0, 2.0]], true)[0],
            want,
            "Error {}",
            errs.1
        );
    }

    // A linear PolyMap whose matrix is not diagonal becomes a MatrixMap and a
    // ShiftMap, which C's lone-Mapping simplify cannot return as one Mapping,
    // so it stays the PolyMap (mapping.c:16871-16891).
    let pm = c_polymap(2, &C_COEFF2);
    let simp = (&pm as &dyn Mapping).simplify().unwrap();
    assert!(simp.as_ref().as_any().is::<PolyMap>(), "Error 3002");
    assert!(simp.equal(&pm), "Error 3003");
}

/// A general affine PolyMap followed by a ZoomMap: the PolyMap becomes a
/// MatrixMap and a ShiftMap, split into the list when next nominated
/// (cmpmap.c:1677-1734), so the ShiftMap merges with the ZoomMap into C's
/// MatrixMap followed by a WinMap that scales by 2 and shifts by (20, 40).
#[test]
fn polymap_affine_general_merges_with_a_neighbour() {
    let s = (&general_affine_then_zoom() as &dyn Mapping)
        .simplify()
        .unwrap();
    assert_eq!(dump_classes(s.as_ref()), ["CmpMap", "MatrixMap", "WinMap"]);
    let text = native_dump(&AnyObject::from_mapping(s.as_ref()).unwrap()).unwrap();
    assert!(
        text.contains("Sft1 = 20") && text.contains("Sft2 = 40"),
        "{text}"
    );
}

/// TranForward, IterInverse and TranInverse as IterInverse is set and the
/// PolyMap inverted, and for a forward-only PolyMap with two inputs and one
/// output, which cannot have an iterative inverse.
#[test]
fn c_iter_inverse_attributes() {
    let mut pm = c_polymap(2, &C_COEFF3);
    let state = |pm: &PolyMap| (pm.has_forward(), pm.iter_inverse(), pm.has_inverse());
    assert_eq!(state(&pm), (true, true, true), "Error 8001-8003");
    pm.set_iter_inverse(false).unwrap();
    assert_eq!(state(&pm), (true, false, false), "Error 8004-8006");
    pm.invert();
    assert_eq!(state(&pm), (false, false, true), "Error 8007-8009");
    pm.set_iter_inverse(true).unwrap();
    assert_eq!(state(&pm), (true, true, true), "Error 8010-8012");
    pm.invert();
    assert_eq!(state(&pm), (true, true, true), "Error 8013-8015");

    let pm2 = PolyMap::new(
        2,
        1,
        &c_terms(2, &[2.0, 1.0, 2.0, 0.0, 3.0, 1.0, 0.0, 1.0]),
        None,
    )
    .unwrap();
    assert_eq!(state(&pm2), (true, false, false), "Error 8016-8018");
}

/// Over a long array, each point gets the result it gets alone, and a bad
/// input makes bad only the outputs whose terms use a power of it above
/// zero, as in C: the second output here does not depend on the first input.
#[test]
fn polymap_long_array_matches_each_point_and_keeps_independent_outputs() {
    let t = |coeff: f64, powers: [usize; 2], out_axis: usize| PolyTerm {
        coeff,
        powers: powers.to_vec(),
        out_axis,
    };
    let terms = [
        t(0.5, [0, 0], 1),
        t(1.25, [1, 0], 1),
        t(-0.75, [2, 1], 1),
        t(1e-3, [0, 3], 1),
        t(2.0, [0, 1], 2),
        t(-0.5, [0, 2], 2),
        t(0.0, [0, 0], 2),
    ];
    let m = PolyMap::new(2, 2, &terms, None).unwrap();
    let n = 1000;
    let x = |i: usize| match i % 53 {
        0 => BAD_VALUE,
        7 => f64::NAN,
        _ => (i as f64 - 500.0) * 0.731,
    };
    let y = |i: usize| match i % 61 {
        0 => BAD_VALUE,
        _ => (i as f64 * 0.0137).sin() * 40.0,
    };
    let pts = PointSet::from_rows((0..n).map(|i| vec![x(i), y(i)]));
    let all = m.transform(&pts, true).unwrap();
    for i in 0..n {
        let one = m
            .transform(&PointSet::from_rows([vec![x(i), y(i)]]), true)
            .unwrap();
        for a in 0..2 {
            assert_eq!(
                all.point(i)[a].to_bits(),
                one.point(0)[a].to_bits(),
                "point {i} output {a}"
            );
        }
        if x(i) == BAD_VALUE && y(i) != BAD_VALUE {
            assert_eq!(all.point(i)[0], BAD_VALUE, "point {i}");
            assert_ne!(all.point(i)[1], BAD_VALUE, "point {i}");
        }
    }
}

/// A PolyMap with no inverse refuses an inverse transformation on either
/// path: the NaN protocol's entry point gives the same error as
/// `transform_into` rather than outputs.
#[test]
fn polymap_without_an_inverse_refuses_it_on_the_nan_path_too() {
    use ast_core::coords::{Points, PointsMut};
    // Two inputs and one output, so there is no iterative inverse either.
    let terms = [PolyTerm {
        coeff: 2.0,
        powers: vec![1, 1],
        out_axis: 1,
    }];
    let m = PolyMap::new(2, 1, &terms, None).unwrap();
    assert!(!m.has_inverse());
    let x = [1.0, 2.0];
    let (mut a0, mut a1) = ([0.0; 2], [0.0; 2]);
    let (mut b0, mut b1) = ([0.0; 2], [0.0; 2]);
    let plain = m.transform_into(
        Points::new([&x[..]]).unwrap(),
        false,
        PointsMut::new([&mut a0[..], &mut a1[..]]).unwrap(),
    );
    let nan = m.transform_into_nan(
        Points::new([&x[..]]).unwrap(),
        false,
        PointsMut::new([&mut b0[..], &mut b1[..]]).unwrap(),
    );
    let b = [b0, b1];
    let code = |r: &ast_core::error::AstResult<()>| r.as_ref().unwrap_err().code();
    assert!(plain.is_err(), "{plain:?}");
    assert!(nan.is_err(), "{nan:?} wrote {b:?}");
    assert_eq!(code(&plain), code(&nan));
}
