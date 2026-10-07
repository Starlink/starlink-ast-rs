//! Tests for ChebyMap — Chebyshev polynomial transformation.
//!
//! Ported from C's testchebymap.c and the Step 6 design spec.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{ChebyMap, Mapping, PolyTerm, UnitMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ---------------------------------------------------------------------------
// Construction and basic properties
// ---------------------------------------------------------------------------

#[test]
fn chebymap_dimensions() {
    // 1D ChebyMap: f(x) = 1.5 (constant via T_0)
    let terms = vec![PolyTerm {
        coeff: 1.5,
        powers: vec![0],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();
    assert_eq!(m.nin(), 1);
    assert_eq!(m.nout(), 1);
    assert!(!m.is_inverted());
}

#[test]
fn chebymap_nin_zero_rejected() {
    let result = ChebyMap::new(0, 1, &[], None, &[], &[], None, None);
    assert!(result.is_err());
}

#[test]
fn chebymap_nout_zero_rejected() {
    let result = ChebyMap::new(1, 0, &[], None, &[0.0], &[1.0], None, None);
    assert!(result.is_err());
}

#[test]
fn chebymap_bad_bounds_len_rejected() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![0],
        out_axis: 1,
    }];
    // lbnd_f has wrong length.
    let result = ChebyMap::new(1, 1, &terms, None, &[0.0, 1.0], &[10.0], None, None);
    assert!(result.is_err());
}

#[test]
fn chebymap_zero_width_bounds_rejected() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![0],
        out_axis: 1,
    }];
    // Only a box of zero width is an error (chebymap.c astInitChebyMap);
    // reversed bounds give a negative scale.
    let err = ChebyMap::new(1, 1, &terms, None, &[5.0], &[5.0], None, None).unwrap_err();
    assert_eq!(err.code(), ast_core::error::ErrorCode::Badbx);
    assert!(ChebyMap::new(1, 1, &terms, None, &[10.0], &[0.0], None, None).is_ok());
}

// ---------------------------------------------------------------------------
// 1D constant: f(x) = 1.5 via T_0 for any x in [0, 10]
// ---------------------------------------------------------------------------

#[test]
fn chebymap_1d_constant() {
    let terms = vec![PolyTerm {
        coeff: 1.5,
        powers: vec![0],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();

    let pts = PointSet::from_rows(vec![vec![0.0], vec![5.0], vec![10.0]]);
    let out = m.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert!(
            (out.point(i)[0] - 1.5).abs() < 1e-12,
            "point {}: expected 1.5, got {}",
            i,
            out.point(i)[0]
        );
    }
}

// ---------------------------------------------------------------------------
// 1D quadratic via Chebyshev:
// f(x_norm) = 2.5 * T_0(x_norm) - 2.0 * T_2(x_norm)
//           = 2.5 - 2 * (2*x_norm^2 - 1) = 4.5 - 4*x_norm^2
// Domain [0, 1]: x_norm = 2*x - 1, so x=0 -> x_norm=-1, x=1 -> x_norm=1
// ---------------------------------------------------------------------------

#[test]
fn chebymap_1d_quadratic() {
    let terms = vec![
        PolyTerm {
            coeff: 2.5,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: -2.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let m = ChebyMap::new(1, 1, &terms, None, &[0.0], &[1.0], None, None).unwrap();

    // At x=0: x_norm = -1, T_0(-1) = 1, T_2(-1) = 2*1-1 = 1
    // f = 2.5 - 2.0 = 0.5
    let pts0 = PointSet::from_rows(vec![vec![0.0]]);
    let out0 = m.transform(&pts0, true).unwrap();
    assert!(
        (out0.point(0)[0] - 0.5).abs() < 1e-12,
        "at x=0: expected 0.5, got {}",
        out0.point(0)[0]
    );

    // At x=0.5: x_norm = 0, T_0(0) = 1, T_2(0) = 2*0-1 = -1
    // f = 2.5 - 2.0*(-1) = 4.5
    let pts_mid = PointSet::from_rows(vec![vec![0.5]]);
    let out_mid = m.transform(&pts_mid, true).unwrap();
    assert!(
        (out_mid.point(0)[0] - 4.5).abs() < 1e-12,
        "at x=0.5: expected 4.5, got {}",
        out_mid.point(0)[0]
    );

    // At x=1: x_norm = 1, T_0(1) = 1, T_2(1) = 1
    // f = 2.5 - 2.0 = 0.5
    let pts1 = PointSet::from_rows(vec![vec![1.0]]);
    let out1 = m.transform(&pts1, true).unwrap();
    assert!(
        (out1.point(0)[0] - 0.5).abs() < 1e-12,
        "at x=1: expected 0.5, got {}",
        out1.point(0)[0]
    );

    // At x=0.25: x_norm = -0.5, T_2(-0.5) = 2*0.25-1 = -0.5
    // f = 2.5 - 2.0*(-0.5) = 3.5
    let pts_q = PointSet::from_rows(vec![vec![0.25]]);
    let out_q = m.transform(&pts_q, true).unwrap();
    assert!(
        (out_q.point(0)[0] - 3.5).abs() < 1e-12,
        "at x=0.25: expected 3.5, got {}",
        out_q.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Out-of-bounds input produces BAD_VALUE
// ---------------------------------------------------------------------------

#[test]
fn chebymap_out_of_bounds() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();

    // Below lower bound.
    let pts = PointSet::from_rows(vec![vec![-0.1]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(
        out.point(0)[0],
        BAD_VALUE,
        "below lower bound should be BAD"
    );

    // Above upper bound.
    let pts2 = PointSet::from_rows(vec![vec![10.1]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert_eq!(
        out2.point(0)[0],
        BAD_VALUE,
        "above upper bound should be BAD"
    );

    // At boundary (should work).
    let pts3 = PointSet::from_rows(vec![vec![0.0], vec![10.0]]);
    let out3 = m.transform(&pts3, true).unwrap();
    assert_ne!(
        out3.point(0)[0],
        BAD_VALUE,
        "at lower bound should not be BAD"
    );
    assert_ne!(
        out3.point(1)[0],
        BAD_VALUE,
        "at upper bound should not be BAD"
    );
}

// ---------------------------------------------------------------------------
// BAD_VALUE input propagation
// ---------------------------------------------------------------------------

#[test]
fn chebymap_bad_value_propagation() {
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
    let m = ChebyMap::new(
        2,
        2,
        &terms,
        None,
        &[-10.0, -10.0],
        &[10.0, 10.0],
        None,
        None,
    )
    .unwrap();

    // A BAD input spoils only the output whose terms use it (chebymap.c
    // PolyPowers): y2 = T1(x2) stays good.
    let pts = PointSet::from_rows(vec![vec![BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert!((out.point(0)[1] - 0.3).abs() < 1e-15);

    // So does a NaN input.
    let pts2 = PointSet::from_rows(vec![vec![f64::NAN, 3.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], BAD_VALUE);
    assert!((out2.point(0)[1] - 0.3).abs() < 1e-15);
}

// ---------------------------------------------------------------------------
// 2D ChebyMap with mixed Chebyshev powers
// f(x,y) = 3.0 * T_1(x_norm) * T_2(y_norm)
// ---------------------------------------------------------------------------

#[test]
fn chebymap_2d_mixed_powers() {
    let terms = vec![PolyTerm {
        coeff: 3.0,
        powers: vec![1, 2],
        out_axis: 1,
    }];
    let m = ChebyMap::new(2, 1, &terms, None, &[-1.0, -1.0], &[1.0, 1.0], None, None).unwrap();

    // With domain [-1,1]x[-1,1], normalization is identity: x_norm=x, y_norm=y
    // At (0.5, 0.5): T_1(0.5)=0.5, T_2(0.5)=2*0.25-1=-0.5
    // f = 3.0 * 0.5 * (-0.5) = -0.75
    let pts = PointSet::from_rows(vec![vec![0.5, 0.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - (-0.75)).abs() < 1e-12,
        "expected -0.75, got {}",
        out.point(0)[0]
    );

    // At (1.0, 0.0): T_1(1)=1, T_2(0)=2*0-1=-1
    // f = 3.0 * 1.0 * (-1.0) = -3.0
    let pts2 = PointSet::from_rows(vec![vec![1.0, 0.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert!(
        (out2.point(0)[0] - (-3.0)).abs() < 1e-12,
        "expected -3.0, got {}",
        out2.point(0)[0]
    );

    // At (0.0, 0.0): T_1(0)=0, so entire product is 0
    let pts3 = PointSet::from_rows(vec![vec![0.0, 0.0]]);
    let out3 = m.transform(&pts3, true).unwrap();
    assert!(
        out3.point(0)[0].abs() < 1e-12,
        "expected 0.0, got {}",
        out3.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// 2D ChebyMap with non-trivial domain
// ---------------------------------------------------------------------------

#[test]
fn chebymap_2d_nontrivial_domain() {
    // f(x,y) = T_1(x_norm), domain [0, 10] x [0, 20]
    // scale_x = 2/10 = 0.2, offset_x = -(10)/10 = -1
    // x_norm = 0.2*x - 1
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1, 0],
        out_axis: 1,
    }];
    let m = ChebyMap::new(2, 1, &terms, None, &[0.0, 0.0], &[10.0, 20.0], None, None).unwrap();

    // At (5, 10): x_norm = 0.2*5 - 1 = 0.0, T_1(0) = 0
    let pts = PointSet::from_rows(vec![vec![5.0, 10.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!(
        out.point(0)[0].abs() < 1e-12,
        "expected 0.0, got {}",
        out.point(0)[0]
    );

    // At (10, 10): x_norm = 0.2*10 - 1 = 1.0, T_1(1) = 1
    let pts2 = PointSet::from_rows(vec![vec![10.0, 10.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert!(
        (out2.point(0)[0] - 1.0).abs() < 1e-12,
        "expected 1.0, got {}",
        out2.point(0)[0]
    );

    // At (0, 10): x_norm = -1.0, T_1(-1) = -1
    let pts3 = PointSet::from_rows(vec![vec![0.0, 10.0]]);
    let out3 = m.transform(&pts3, true).unwrap();
    assert!(
        (out3.point(0)[0] - (-1.0)).abs() < 1e-12,
        "expected -1.0, got {}",
        out3.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Forward/inverse round-trip
// ---------------------------------------------------------------------------

#[test]
fn chebymap_forward_inverse_roundtrip() {
    // Forward: f(x) = T_1(x_norm) on domain [0, 10]
    // Inverse: g(y) = T_1(y_norm) on domain [-1, 1]
    // These are approximate inverses: forward maps [0,10] to [-1,1],
    // inverse maps [-1,1] back to [-1,1] (in normalized coords).

    // For a true inverse, we need: forward maps x -> y, inverse maps y -> x.
    // Forward: y = T_1(x_norm) = x_norm = (2x/10 - 1)  on [0,10] -> [-1,1]
    // Inverse: x = T_1(y_norm) on [-1,1], but we want to map y back to x.
    // If y = (x/5 - 1), then x = 5(y+1), i.e. x = 5*y + 5.
    // As Chebyshev: x = 5*T_0(y_norm) + 5*T_1(y_norm) on domain [-1,1]
    // (with identity normalization since domain is already [-1,1]).

    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv = vec![
        PolyTerm {
            coeff: 5.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![1],
            out_axis: 1,
        },
    ];
    let m = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv),
        &[0.0],
        &[10.0],
        Some(&[-1.0]),
        Some(&[1.0]),
    )
    .unwrap();

    // Forward: x=5 -> x_norm=0 -> T_1(0)=0 -> y=0
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!(
        out.point(0)[0].abs() < 1e-12,
        "forward x=5: expected 0, got {}",
        out.point(0)[0]
    );

    // Inverse: y=0 -> y_norm=0 -> 5*T_0(0) + 5*T_1(0) = 5 + 0 = 5
    let inv_out = m.transform(&out, false).unwrap();
    assert!(
        (inv_out.point(0)[0] - 5.0).abs() < 1e-12,
        "inverse y=0: expected 5, got {}",
        inv_out.point(0)[0]
    );

    // Forward: x=0 -> x_norm=-1 -> T_1(-1)=-1 -> y=-1
    let pts2 = PointSet::from_rows(vec![vec![0.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert!(
        (out2.point(0)[0] - (-1.0)).abs() < 1e-12,
        "forward x=0: expected -1, got {}",
        out2.point(0)[0]
    );

    // Inverse: y=-1 -> 5*1 + 5*(-1) = 0
    let inv_out2 = m.transform(&out2, false).unwrap();
    assert!(
        inv_out2.point(0)[0].abs() < 1e-12,
        "inverse y=-1: expected 0, got {}",
        inv_out2.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Invert flag
// ---------------------------------------------------------------------------

#[test]
fn chebymap_invert() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv = vec![
        PolyTerm {
            coeff: 5.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![1],
            out_axis: 1,
        },
    ];
    let mut m = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv),
        &[0.0],
        &[10.0],
        Some(&[-1.0]),
        Some(&[1.0]),
    )
    .unwrap();

    // Before inversion: forward maps [0,10] -> [-1,1]
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!(
        out.point(0)[0].abs() < 1e-12,
        "before invert: expected 0, got {}",
        out.point(0)[0]
    );

    // Invert: now "forward" uses the inverse transform.
    m.invert();
    assert!(m.is_inverted());

    // After inversion: forward call uses inverse polynomial
    // Input y=0 -> 5*T_0(0) + 5*T_1(0) = 5
    let pts2 = PointSet::from_rows(vec![vec![0.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert!(
        (out2.point(0)[0] - 5.0).abs() < 1e-12,
        "after invert: expected 5, got {}",
        out2.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// No inverse: transforming inversely is an error
// ---------------------------------------------------------------------------

#[test]
fn chebymap_no_inverse_is_an_error() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &fwd, None, &[0.0], &[10.0], None, None).unwrap();

    // C's base Transform raises AST__TRNND for an undefined direction.
    assert!(!m.has_inverse());
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let err = m.transform(&pts, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Trnnd);
}

// ---------------------------------------------------------------------------
// Domain accessor
// ---------------------------------------------------------------------------

#[test]
fn chebymap_domain_accessor() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv),
        &[0.0],
        &[10.0],
        Some(&[-1.0]),
        Some(&[1.0]),
    )
    .unwrap();

    assert_eq!(m.forward_lbnd(), &[0.0]);
    assert_eq!(m.forward_ubnd(), &[10.0]);
    assert_eq!(m.inverse_lbnd(), Some(&[-1.0][..]));
    assert_eq!(m.inverse_ubnd(), Some(&[1.0][..]));

    // domain() accounts for inversion.
    let (lb, ub) = m.domain(true);
    assert_eq!(lb, [0.0]);
    assert_eq!(ub, [10.0]);

    let (lb_i, ub_i) = m.domain(false);
    assert_eq!(lb_i, [-1.0]);
    assert_eq!(ub_i, [1.0]);
}

/// A forward-only 2-D ChebyMap has no inverse box, so the inverse domain is
/// found by MapBox from the forward box. Inverting it swaps the two domains
/// (testchebymap.c errors 800-803). The values are from C's astChebyDomain.
#[test]
fn chebymap_domain_inverted_forward_only() {
    let terms = [
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
        PolyTerm {
            coeff: 0.25,
            powers: vec![1, 1],
            out_axis: 2,
        },
    ];
    let mut m = ChebyMap::new(2, 2, &terms, None, &[0.0, -1.0], &[1.0, 2.0], None, None).unwrap();
    let forward_box = (vec![0.0, -1.0000000000000002], vec![1.0, 2.0]);
    let inverse_box = (vec![-1.0, -1.25], vec![1.0, 1.25]);
    assert_eq!(m.domain(true), forward_box);
    assert_eq!(m.domain(false), inverse_box);
    m.invert();
    assert_eq!(m.domain(true), inverse_box);
    assert_eq!(m.domain(false), forward_box);
}

// ---------------------------------------------------------------------------
// Chebyshev T_n evaluation (indirect via transform)
// ---------------------------------------------------------------------------

#[test]
fn chebymap_higher_order_chebyshev() {
    // T_3(x) = 4x^3 - 3x
    // f(x) = T_3(x_norm) on domain [-1, 1] (identity normalization)
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![3],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();

    // T_3(0.5) = 4*0.125 - 1.5 = 0.5 - 1.5 = -1.0
    let pts = PointSet::from_rows(vec![vec![0.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - (-1.0)).abs() < 1e-12,
        "T_3(0.5): expected -1.0, got {}",
        out.point(0)[0]
    );

    // T_3(1.0) = 4 - 3 = 1.0
    let pts2 = PointSet::from_rows(vec![vec![1.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert!(
        (out2.point(0)[0] - 1.0).abs() < 1e-12,
        "T_3(1.0): expected 1.0, got {}",
        out2.point(0)[0]
    );

    // T_3(0) = 0
    let pts3 = PointSet::from_rows(vec![vec![0.0]]);
    let out3 = m.transform(&pts3, true).unwrap();
    assert!(
        out3.point(0)[0].abs() < 1e-12,
        "T_3(0): expected 0.0, got {}",
        out3.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn chebymap_serialisation_roundtrip() {
    let fwd = vec![
        PolyTerm {
            coeff: 2.5,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: -2.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let inv = vec![
        PolyTerm {
            coeff: 5.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![1],
            out_axis: 1,
        },
    ];
    let m = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv),
        &[0.0],
        &[1.0],
        Some(&[-1.0]),
        Some(&[1.0]),
    )
    .unwrap();

    let native = to_native_string(&AnyObject::ChebyMap(m.clone())).unwrap();
    let obj = from_native_string(&native).unwrap();
    let m2 = match obj {
        AnyObject::ChebyMap(c) => c,
        other => panic!("expected ChebyMap, got {:?}", other),
    };

    // Verify forward transform produces the same results.
    let test_points = vec![vec![0.0], vec![0.25], vec![0.5], vec![0.75], vec![1.0]];
    let pts = PointSet::from_rows(test_points);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    for i in 0..5 {
        assert!(
            (out1.point(i)[0] - out2.point(i)[0]).abs() < 1e-12,
            "fwd point {}: {} vs {}",
            i,
            out1.point(i)[0],
            out2.point(i)[0]
        );
    }

    // Verify inverse transform.
    let inv_pts = PointSet::from_rows(vec![vec![0.0], vec![0.5], vec![-0.5]]);
    let inv_out1 = m.transform(&inv_pts, false).unwrap();
    let inv_out2 = m2.transform(&inv_pts, false).unwrap();
    for i in 0..3 {
        assert!(
            (inv_out1.point(i)[0] - inv_out2.point(i)[0]).abs() < 1e-12,
            "inv point {}: {} vs {}",
            i,
            inv_out1.point(i)[0],
            inv_out2.point(i)[0]
        );
    }
}

#[test]
fn chebymap_serialisation_forward_only() {
    let fwd = vec![
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
    ];
    let m = ChebyMap::new(1, 1, &fwd, None, &[-5.0], &[5.0], None, None).unwrap();

    let native = to_native_string(&AnyObject::ChebyMap(m.clone())).unwrap();
    let obj = from_native_string(&native).unwrap();
    let m2 = match obj {
        AnyObject::ChebyMap(c) => c,
        _ => panic!("expected ChebyMap"),
    };

    let pts = PointSet::from_rows(vec![vec![-5.0], vec![0.0], vec![5.0]]);
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

    // Verify bounds survived the round-trip.
    assert_eq!(m2.forward_lbnd(), &[-5.0]);
    assert_eq!(m2.forward_ubnd(), &[5.0]);
}

// ---------------------------------------------------------------------------
// 2D serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn chebymap_2d_serialisation_roundtrip() {
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 0.5,
            powers: vec![1, 1],
            out_axis: 2,
        },
    ];
    let m = ChebyMap::new(2, 2, &fwd, None, &[0.0, 0.0], &[10.0, 20.0], None, None).unwrap();

    let native = to_native_string(&AnyObject::ChebyMap(m.clone())).unwrap();
    let obj = from_native_string(&native).unwrap();
    let m2 = match obj {
        AnyObject::ChebyMap(c) => c,
        other => panic!("expected ChebyMap, got {:?}", other),
    };

    let pts = PointSet::from_rows(vec![vec![5.0, 10.0], vec![2.5, 15.0]]);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    for i in 0..2 {
        for j in 0..2 {
            assert!(
                (out1.point(i)[j] - out2.point(i)[j]).abs() < 1e-12,
                "point {} axis {}: {} vs {}",
                i,
                j,
                out1.point(i)[j],
                out2.point(i)[j]
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Serialisation preserves the ChebyMap class name (not PolyMap)
// ---------------------------------------------------------------------------

#[test]
fn chebymap_serialisation_preserves_class() {
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();

    let native = to_native_string(&AnyObject::ChebyMap(m)).unwrap();
    assert!(
        native.contains("ChebyMap"),
        "native string should contain ChebyMap class name"
    );
}

// ---------------------------------------------------------------------------
// Rate (numeric differentiation)
// ---------------------------------------------------------------------------

#[test]
fn chebymap_rate() {
    // f(x) = T_1(x_norm) on [-1, 1] => f(x) = x
    // df/dx = 1 everywhere in domain.
    let terms = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();

    // At x=0 the derivative of f(x)=x is 1.0. Upstream e782c220 seeds the
    // search interval with a small step (not 1.0) so the sample points
    // stay inside the ChebyMap domain [-1,1]; astRate now returns the true
    // gradient at x0=0 instead of BAD.
    let rate = m.rate(&[0.0], 1, 1).unwrap();
    assert!(
        (rate - 1.0).abs() < 1e-6,
        "expected rate ~1.0 at x=0, got {rate}"
    );

    // Inside the domain, away from zero, the numeric rate is accurate.
    let rate_mid = m.rate(&[0.5], 1, 1).unwrap();
    assert!(
        (rate_mid - 1.0).abs() < 1e-6,
        "expected rate ~1.0 at x=0.5, got {rate_mid}"
    );
}

// ---------------------------------------------------------------------------
// try_merge: inverse self-cancellation
// ---------------------------------------------------------------------------

#[test]
fn chebymap_cancels_with_inverse() {
    // A ChebyMap followed by its inverse should simplify to UnitMap.
    let fwd = vec![
        PolyTerm {
            coeff: 2.5,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: -2.0,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let inv = vec![
        PolyTerm {
            coeff: 5.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 5.0,
            powers: vec![1],
            out_axis: 1,
        },
    ];
    let m = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv),
        &[0.0],
        &[1.0],
        Some(&[-1.0]),
        Some(&[1.0]),
    )
    .unwrap();

    // Same ChebyMap, self used forward, other used inverse -> cancels.
    let result = m.try_merge(&m, true, false, true);
    assert!(result.is_some(), "should cancel with its own inverse");
    let merged = result.unwrap();
    assert_eq!(merged.nin(), 1);
    assert_eq!(merged.nout(), 1);
    // Verify it behaves as identity.
    assert!(
        merged.as_any().downcast_ref::<UnitMap>().is_some(),
        "merged result should be a UnitMap"
    );
}

#[test]
fn chebymap_same_direction_does_not_cancel() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &fwd, None, &[0.0], &[10.0], None, None).unwrap();

    // Same direction (both forward) should NOT cancel.
    let result = m.try_merge(&m, true, false, false);
    assert!(result.is_none(), "same direction should not cancel");
}

#[test]
fn chebymap_different_maps_do_not_cancel() {
    let fwd1 = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m1 = ChebyMap::new(1, 1, &fwd1, None, &[0.0], &[10.0], None, None).unwrap();

    let fwd2 = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m2 = ChebyMap::new(1, 1, &fwd2, None, &[0.0], &[10.0], None, None).unwrap();

    // Different ChebyMaps in opposite directions should NOT cancel.
    let result = m1.try_merge(&m2, true, false, true);
    assert!(result.is_none(), "different maps should not cancel");
}

#[test]
fn chebymap_different_bounds_do_not_cancel() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m1 = ChebyMap::new(1, 1, &fwd, None, &[0.0], &[10.0], None, None).unwrap();
    let m2 = ChebyMap::new(1, 1, &fwd, None, &[0.0], &[20.0], None, None).unwrap();

    // Same coefficients but different domain bounds should NOT cancel.
    let result = m1.try_merge(&m2, true, false, true);
    assert!(result.is_none(), "different bounds should not cancel");
}

#[test]
fn chebymap_parallel_does_not_merge() {
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &fwd, None, &[0.0], &[10.0], None, None).unwrap();

    // Parallel composition is not handled.
    let result = m.try_merge(&m, false, false, true);
    assert!(result.is_none(), "parallel should not merge");
}

#[test]
fn chebymap_2d_cancels_with_inverse() {
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let inv = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.5,
            powers: vec![0, 1],
            out_axis: 2,
        },
    ];
    let m = ChebyMap::new(
        2,
        2,
        &fwd,
        Some(&inv),
        &[-1.0, -1.0],
        &[1.0, 1.0],
        Some(&[-1.0, -2.0]),
        Some(&[1.0, 2.0]),
    )
    .unwrap();

    // Forward then inverse -> UnitMap(2).
    let result = m.try_merge(&m, true, false, true);
    assert!(result.is_some(), "2D ChebyMap should cancel with inverse");
    let merged = result.unwrap();
    assert_eq!(merged.nin(), 2);
    assert_eq!(merged.nout(), 2);
    assert!(merged.as_any().downcast_ref::<UnitMap>().is_some());

    // Inverse then forward -> UnitMap(2).
    let result2 = m.try_merge(&m, true, true, false);
    assert!(
        result2.is_some(),
        "2D ChebyMap inverse then forward should cancel"
    );
    let merged2 = result2.unwrap();
    assert_eq!(merged2.nin(), 2);
    assert_eq!(merged2.nout(), 2);
}

#[test]
fn chebymap_does_not_cancel_when_the_inverse_shapes_differ() {
    use ast_core::mappings::{CmpMap, Composition};

    // Identical forward transformations and bounding boxes, inverse
    // transformations with different coefficient counts and maximum powers.
    // C compares the pair with astEqual, which reaches the comparison ChebyMap
    // inherits from PolyMap, and that half compares the inverse coefficient
    // counts and maximum powers (polymap.c:758-776), so the pair does not
    // cancel. Confirmed against the oracle.
    let fwd = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![2],
        out_axis: 1,
    }];
    let inv_a = vec![PolyTerm {
        coeff: 0.5,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv_b = vec![
        PolyTerm {
            coeff: 0.5,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.3,
            powers: vec![2],
            out_axis: 1,
        },
    ];
    let a = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv_a),
        &[0.0],
        &[10.0],
        Some(&[-5.0]),
        Some(&[5.0]),
    )
    .unwrap();
    let mut b = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv_b),
        &[0.0],
        &[10.0],
        Some(&[-5.0]),
        Some(&[5.0]),
    )
    .unwrap();
    b.invert();

    let cm = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();
    let s = <dyn Mapping>::simplify(&cm).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "the inverse transformations have different shapes, so the pair stands"
    );
}

#[test]
fn chebymap_does_not_cancel_when_only_inverse_coefficient_values_differ() {
    use ast_core::mappings::{CmpMap, Composition};

    // Identical forward transformations and bounding boxes; inverse
    // transformations with the same coefficient counts and maximum powers but
    // different coefficient values. ChebyMap's comparison calls the PolyMap one
    // before it compares the two bounding boxes (chebymap.c:462-507), and that
    // compares the inverse transformation's own coefficients
    // (polymap.c:772-822), so the pair stands.
    let fwd = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![2],
        out_axis: 1,
    }];
    let inv_a = vec![PolyTerm {
        coeff: 0.5,
        powers: vec![1],
        out_axis: 1,
    }];
    let inv_b = vec![PolyTerm {
        coeff: 0.7,
        powers: vec![1],
        out_axis: 1,
    }];
    let a = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv_a),
        &[0.0],
        &[10.0],
        Some(&[-5.0]),
        Some(&[5.0]),
    )
    .unwrap();
    let mut b = ChebyMap::new(
        1,
        1,
        &fwd,
        Some(&inv_b),
        &[0.0],
        &[10.0],
        Some(&[-5.0]),
        Some(&[5.0]),
    )
    .unwrap();
    b.invert();

    let cm = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();
    let s = <dyn Mapping>::simplify(&cm).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "the inverse coefficient values differ, so the opposed pair stands"
    );
}

/// A ChebyMap whose every term is linear in its Chebyshev basis reduces to a
/// `MatrixMap` and a `ShiftMap` under `simplify`. The coefficients apply to the
/// normalised input `z = scale*x + offset`, so the reduction has to fold that
/// normalisation in: over `[0,10]` the map `2*T1(z)` is `0.4x - 2`, not `2x`.
/// The replacement must therefore agree with the ChebyMap in the forward
/// direction, and its inverse must recover the input.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testchebymap.c`, the linear
/// reduction block following `stopit( 601 )`.
#[test]
fn a_linear_chebymap_reduces_with_its_normalisation_folded_in() {
    let terms = vec![PolyTerm {
        coeff: 2.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();
    let s = <dyn Mapping>::simplify(&m).unwrap();
    assert!(
        !s.as_any().is::<ChebyMap>(),
        "a linear ChebyMap must be replaced"
    );

    let x = vec![vec![0.0], vec![1.0], vec![10.0]];
    let pts = PointSet::from_rows(x.clone());
    let reference = m.transform(&pts, true).unwrap();
    let simplified = s.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert!(
            (simplified.point(i)[0] - reference.point(i)[0]).abs() < 1.0e-10,
            "point {i}: the replacement gives {} where the ChebyMap gives {}",
            simplified.point(i)[0],
            reference.point(i)[0]
        );
    }

    // 2*T1(z) over [0,10] is 0.4x - 2.
    assert!((reference.point(0)[0] + 2.0).abs() < 1.0e-12);
    assert!((reference.point(2)[0] - 2.0).abs() < 1.0e-12);

    let back = s.transform(&reference, false).unwrap();
    for (i, input) in x.iter().enumerate() {
        assert!(
            (back.point(i)[0] - input[0]).abs() < 1.0e-10,
            "point {i}: the inverse gives {} where the input was {}",
            back.point(i)[0],
            input[0]
        );
    }
}

/// The same reduction with two inputs whose bounds differ and whose outputs each
/// combine a linear term and a constant term: `fx = 3*T1(x') + 0.5` and
/// `fy = -T1(y') + 2`. Each linear term's normalisation offset adds to its
/// output's constant term, so both the matrix and the offset vector have to
/// accumulate rather than be assigned.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testchebymap.c`, the same block.
#[test]
fn a_linear_chebymap_accumulates_each_outputs_constant_contributions() {
    let terms = vec![
        PolyTerm {
            coeff: 3.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.5,
            powers: vec![0, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: -1.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![0, 0],
            out_axis: 2,
        },
    ];
    let m = ChebyMap::new(2, 2, &terms, None, &[0.0, -5.0], &[10.0, 5.0], None, None).unwrap();
    let s = <dyn Mapping>::simplify(&m).unwrap();
    assert!(
        !s.as_any().is::<ChebyMap>(),
        "a linear ChebyMap must be replaced"
    );

    let x = vec![
        vec![0.0, -5.0],
        vec![10.0, 5.0],
        vec![3.0, 1.0],
        vec![7.5, -2.0],
    ];
    let pts = PointSet::from_rows(x.clone());
    let reference = m.transform(&pts, true).unwrap();
    let simplified = s.transform(&pts, true).unwrap();
    for (i, _) in x.iter().enumerate() {
        for (axis, (got, want)) in simplified
            .point(i)
            .iter()
            .zip(reference.point(i))
            .enumerate()
        {
            assert!(
                (got - want).abs() < 1.0e-10,
                "point {i} axis {}: the replacement gives {got} where the ChebyMap gives {want}",
                axis + 1,
            );
        }
    }

    let back = s.transform(&reference, false).unwrap();
    for (i, input) in x.iter().enumerate() {
        for (axis, (got, want)) in back.point(i).iter().zip(input).enumerate() {
            assert!(
                (got - want).abs() < 1.0e-9,
                "point {i} axis {}: the inverse gives {got} where the input was {want}",
                axis + 1,
            );
        }
    }
}
