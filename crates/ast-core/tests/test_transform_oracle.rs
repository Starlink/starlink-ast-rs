//! Investigative transform-output oracle checker.
//!
//! Ported from the C `check_transform_oracle` / `transform_oracle_util`
//! (reference/starlink-ast `ast_tester/`, submodule commit 3a3c3e51). Reads
//! the committed `.oracle` files, reloads each named fixture through the
//! public Rust API, re-transforms the recorded inputs, and reports how far
//! the Rust outputs diverge from the C reference values. This harness never
//! regenerates the oracle: the C values are the frozen baseline.

// Constants and helpers below are consumed by later tasks in this oracle suite.
#![allow(dead_code)]

use ast_core::constants::BAD_VALUE;

/// Golden tolerance: live outputs vs recorded oracle values. Wide enough to
/// absorb cross-architecture libm differences (~5e-9 near singularities)
/// while still catching real algorithmic change (>= ~1e-6 relative).
const GOLDEN_RTOL: f64 = 1e-7;
const GOLDEN_ATOL: f64 = 1e-7;
/// Equivalence tolerance: live `.map` vs live `.simp`, same run/machine.
const EQUIV_RTOL: f64 = 1e-9;
const EQUIV_ATOL: f64 = 1e-9;
/// Round-trip accuracy: absolute pixel tolerance for GRID-domain corpora.
const RTRIP_RTOL: f64 = 1e-6;
const RTRIP_ATOL: f64 = 0.05;

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// Port of C `oracle_within_tol`: `|got-ref| <= atol + rtol*|ref|`.
/// `BAD_VALUE` matches only `BAD_VALUE`; any NaN never matches.
fn within_tol(got: f64, reference: f64, rtol: f64, atol: f64) -> bool {
    let got_bad = got == BAD_VALUE;
    let ref_bad = reference == BAD_VALUE;
    if got_bad || ref_bad {
        return got_bad && ref_bad;
    }
    if got.is_nan() || reference.is_nan() {
        return false;
    }
    (got - reference).abs() <= atol + rtol * reference.abs()
}

/// Port of C `oracle_within_tol_wrap`: as `within_tol`, but also accepts a
/// difference of one full turn (2*pi) — a longitude may emerge as 0 vs 2*pi
/// or +pi vs -pi across architectures. Angular axes only.
fn within_tol_wrap(got: f64, reference: f64, rtol: f64, atol: f64) -> bool {
    if within_tol(got, reference, rtol, atol) {
        return true;
    }
    if got == BAD_VALUE || reference == BAD_VALUE || got.is_nan() || reference.is_nan() {
        return false;
    }
    ((got - reference).abs() - TWO_PI).abs() <= atol + rtol * reference.abs()
}

#[cfg(test)]
mod primitive_tests {
    use super::*;

    #[test]
    fn within_tol_matches_c_semantics() {
        assert!(within_tol(1.0, 1.0, 0.0, 0.0));
        assert!(within_tol(1.0 + 4e-16, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(1.0001, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol(5e-13, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol(BAD_VALUE, BAD_VALUE, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(BAD_VALUE, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(1.0, BAD_VALUE, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(f64::NAN, 1.0, 1e9, 1e9));
    }

    #[test]
    fn within_tol_wrap_accepts_full_turn_and_ci_pairs() {
        assert!(within_tol_wrap(1.0, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(TWO_PI, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(0.0, TWO_PI, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(
            std::f64::consts::PI,
            -std::f64::consts::PI,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(!within_tol_wrap(1.0, 2.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol_wrap(BAD_VALUE, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        // Exact arm64-ref vs x86-64-got pairs observed in C CI.
        assert!(within_tol_wrap(
            5.0999352672935785e-17,
            6.2831853071795862,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(within_tol_wrap(
            1.9478449064398693e-20,
            6.2831853071795862,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(within_tol_wrap(
            3.1415926535897931,
            -3.1415926535897931,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(within_tol_wrap(
            1.0,
            1.0000000047018207,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
    }
}
