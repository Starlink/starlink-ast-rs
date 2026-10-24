//! SplineMap built from SciPy-ordered coefficients, as C's `astSplineMap`
//! takes them.
//!
//! Ported from `reference/starlink-ast/ast_tester/testsplinemap_c.c`, which
//! reads the spline from `fixtures/programs/testsplinemap/2dspline_c.dat`.
//! The data were fitted by `scipy.interpolate.RectBivariateSpline`, whose
//! coefficient order is the transpose of the CMLIB order the C Fortran API
//! and `SplineMap::new` take; `SplineMap::from_scipy` is the C API's order.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::error::ErrorCode;
use starlink_ast::mappings::{Mapping, SplineMap};

const NX: usize = 150;
const NY: usize = 100;
const K: usize = 4;
const TOL: f64 = 1.0e-11;

const XIN: [f64; 10] = [0.0, 12.5, 12.0, 1.0, 15.0, 1.0, 95.0, 149.5, 151.2, 77.77];
const YIN: [f64; 10] = [-1.0, 8.8, 8.0, 1.0, 15.0, 76.0, 100.0, 99.8, 82.3, 54.3];

/// Positions from `RectBivariateSpline`, with bad values where the input is
/// outside the knots, since CMLIB does not extrapolate.
const UEXP: [f64; 10] = [
    BAD_VALUE,
    12.049767072857646,
    11.921107191183191,
    1.7788366846173014,
    14.441169003602148,
    0.30566103317717114,
    95.24464880295238,
    147.72545905681767,
    BAD_VALUE,
    77.68709057730592,
];
const VEXP: [f64; 10] = [
    BAD_VALUE,
    6.4394741329528635,
    5.788762120688823,
    1.5841275134629758,
    14.580876752701611,
    77.43010929535424,
    102.707899556638,
    99.94739777629977,
    BAD_VALUE,
    57.237295512435686,
];

struct Spline {
    tx: Vec<f64>,
    ty: Vec<f64>,
    cu: Vec<f64>,
    cv: Vec<f64>,
}

fn read_spline() -> Spline {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/programs/testsplinemap/2dspline_c.dat"
    );
    let text = std::fs::read_to_string(path).unwrap();
    let mut values = text
        .split_ascii_whitespace()
        .map(|v| v.parse::<f64>().unwrap());
    let mut take = |n: usize| -> Vec<f64> { values.by_ref().take(n).collect() };
    let spline = Spline {
        tx: take(NX + K),
        ty: take(NY + K),
        cu: take(NX * NY),
        cv: take(NX * NY),
    };
    assert_eq!(spline.cv.len(), NX * NY, "2dspline_c.dat is short");
    spline
}

fn scipy_spline_map(s: &Spline) -> SplineMap {
    SplineMap::from_scipy(K, K, NX, NY, s.tx.clone(), s.ty.clone(), &s.cu, &s.cv).unwrap()
}

/// testsplinemap_c.c Errors 100 and 101: the coefficients come back in the
/// order they were given.
#[test]
fn coeffs_are_returned_in_scipy_order() {
    let s = read_spline();
    let sm = scipy_spline_map(&s);
    assert_eq!(sm.coeffs(1).unwrap(), s.cu);
    assert_eq!(sm.coeffs(2).unwrap(), s.cv);
}

#[test]
fn coeffs_ignore_invert_and_check_the_axis() {
    let s = read_spline();
    let mut sm = scipy_spline_map(&s);
    sm.invert();
    assert_eq!(sm.coeffs(1).unwrap(), s.cu);
    for axis in [0, 3] {
        assert_eq!(sm.coeffs(axis).unwrap_err().code(), ErrorCode::Axiin);
    }
}

/// The two orders are each other's transpose, so the CMLIB constructor
/// given the stored coefficients builds the same SplineMap.
#[test]
fn scipy_and_cmlib_constructors_agree() {
    let s = read_spline();
    let sm = scipy_spline_map(&s);
    let cmlib = SplineMap::new(
        K,
        K,
        NX,
        NY,
        s.tx.clone(),
        s.ty.clone(),
        sm.cu().to_vec(),
        sm.cv().to_vec(),
    )
    .unwrap();
    assert!(sm.equal(&cmlib));
    assert_eq!(sm.cu()[1], s.cu[NY]);
}

/// testsplinemap_c.c Errors 1-8: forward against SciPy, and the iterative
/// inverse back to the input.
#[test]
fn forward_matches_scipy_and_inverse_recovers_the_input() {
    let s = read_spline();
    let sm = scipy_spline_map(&s);
    let pts = PointSet::from_columns(&[&XIN, &YIN]).unwrap();
    let out = sm.transform(&pts, true).unwrap();
    for i in 0..XIN.len() {
        let p = out.point(i);
        for (got, exp) in [(p[0], UEXP[i]), (p[1], VEXP[i])] {
            if exp == BAD_VALUE {
                assert_eq!(got, BAD_VALUE, "point {i}");
            } else {
                assert!(
                    (got - exp).abs() <= TOL * exp.abs(),
                    "point {i}: {got} != {exp}"
                );
            }
        }
    }
    let rec = sm.transform(&out, false).unwrap();
    for i in 0..XIN.len() {
        let (p, q) = (out.point(i), rec.point(i));
        if p[0] != BAD_VALUE && p[1] != BAD_VALUE {
            assert!(
                (q[0] - XIN[i]).abs() <= TOL * XIN[i].abs()
                    && (q[1] - YIN[i]).abs() <= TOL * YIN[i].abs(),
                "point {i}: ({}, {}) != ({}, {})",
                q[0],
                q[1],
                XIN[i],
                YIN[i]
            );
        } else {
            assert_eq!((q[0], q[1]), (BAD_VALUE, BAD_VALUE), "point {i}");
        }
    }
}

/// testsplinemap_c.c Errors 9 and 10: an order-1 spline has a singular
/// Jacobian, so its inverse gives bad values without an error.
#[test]
fn order_one_inverse_is_bad() {
    let sm =
        SplineMap::from_scipy(1, 1, 1, 1, vec![0.0, 1.0], vec![0.0, 1.0], &[1.5], &[2.5]).unwrap();
    let rec = sm
        .transform(&PointSet::from_rows([[0.5, 0.5]]), false)
        .unwrap();
    assert_eq!(rec.point(0), [BAD_VALUE, BAD_VALUE]);
}

/// testsplinemap_c.c Errors 11-13: the spline u = 2x, v = 3y has du/dx = 2,
/// and inverted, dx/du = 0.5 and dx/dv = 0, found numerically as C's
/// parent Rate does.
#[test]
fn inverted_rate_is_the_inverse_rate() {
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let mut sm = SplineMap::from_scipy(
        2,
        2,
        2,
        2,
        knots.clone(),
        knots,
        &[0.0, 0.0, 2.0, 2.0],
        &[0.0, 3.0, 0.0, 3.0],
    )
    .unwrap();
    assert!((sm.rate(&[0.5, 0.5], 1, 1).unwrap() - 2.0).abs() <= 1.0e-12);
    sm.invert();
    assert!((sm.rate(&[1.0, 1.5], 1, 1).unwrap() - 0.5).abs() <= 1.0e-6);
    assert!(sm.rate(&[1.0, 1.5], 1, 2).unwrap().abs() <= 1.0e-6);
}

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "spline_inverse_oracle/cases.rs"]
mod inverse_cases;

/// The forward transformation, and the iterative inverse of its results,
/// bit for bit against C's (`tools/oracle_fixtures/spline_inverse_probe.c`):
/// the inverse's Newton-Raphson steps are solved by palDmat, as in C.
#[test]
fn forward_and_inverse_match_the_c_library_bit_for_bit() {
    let s = read_spline();
    let sm = scipy_spline_map(&s);
    let (u, v) = sm.tran2(&XIN, &YIN, true).unwrap();
    let (x, y) = sm.tran2(&u, &v, false).unwrap();
    let bits = |a: &[f64]| a.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&u), bits(&inverse_cases::U), "forward u");
    assert_eq!(bits(&v), bits(&inverse_cases::V), "forward v");
    assert_eq!(bits(&x), bits(&inverse_cases::X), "inverse x");
    assert_eq!(bits(&y), bits(&inverse_cases::Y), "inverse y");
}
