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
    assert!(!m.iter_inverse());
    assert_eq!(m.niter_inverse(), 4);
    assert!((m.tol_inverse() - 1e-6).abs() < 1e-15);
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
    let m = PolyMap::new(1, 1, &fwd, None).unwrap();

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
// LM inverse with 2D->2D polynomial
// ---------------------------------------------------------------------------

#[test]
fn polymap_lm_inverse_2d() {
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
