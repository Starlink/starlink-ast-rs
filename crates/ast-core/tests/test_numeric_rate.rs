//! Tests for the adaptive numeric_rate algorithm.
//!
//! These tests verify that the adaptive interval-selection algorithm
//! converges correctly for both linear and nonlinear mappings.

use ast_core::constants::BAD_VALUE;
use ast_core::mappings::{ChebyMap, Mapping, MathMap, PolyTerm, WcsMap, WcsProjection, ZoomMap};

// ===========================================================================
// Helpers
// ===========================================================================

fn assert_close(actual: f64, expected: f64, tol: f64, label: &str) {
    assert!(
        (actual - expected).abs() < tol,
        "{label}: expected {expected}, got {actual} (diff {})",
        (actual - expected).abs()
    );
}

// ===========================================================================
// Basic convergence tests
// ===========================================================================

#[test]
fn numeric_rate_linear_mapping() {
    // ZoomMap(2, 3.0) maps (x,y) -> (3x, 3y).
    // d(out1)/d(in1) = 3.0 everywhere.
    let zoom = ZoomMap::new(2, 3.0).unwrap();
    let rate = zoom.rate(&[1.0, 2.0], 1, 1).unwrap();
    assert_close(rate, 3.0, 1e-6, "zoom rate d(out1)/d(in1)");

    // d(out1)/d(in2) = 0.0 (output 1 doesn't depend on input 2).
    let rate_cross = zoom.rate(&[1.0, 2.0], 1, 2).unwrap();
    assert_close(rate_cross, 0.0, 1e-6, "zoom rate d(out1)/d(in2)");

    // d(out2)/d(in2) = 3.0.
    let rate2 = zoom.rate(&[1.0, 2.0], 2, 2).unwrap();
    assert_close(rate2, 3.0, 1e-6, "zoom rate d(out2)/d(in2)");
}

#[test]
fn numeric_rate_at_zero() {
    // Test that the algorithm handles x0 = 0 correctly.
    let zoom = ZoomMap::new(1, 5.0).unwrap();
    let rate = zoom.rate(&[0.0], 1, 1).unwrap();
    assert_close(rate, 5.0, 1e-6, "zoom rate at origin");
}

#[test]
fn numeric_rate_tan_at_zero_matches_c_artifact() {
    // d/dx tan(x) at x=0 is mathematically 1.0, but C's astRate seeds the
    // interval at h0=1.0 for x0=0. The wide first interval makes the mean
    // gradient of tan over [-h/2, h/2] dominated by the cubic term, so C
    // returns a small *negative* value (-1.9950849488779807e-4), not 1.0.
    // This is a C bug (see docs/issues/c-library-quirks.md); we match it
    // bit-for-bit rather than returning the correct derivative.
    // Oracle: astRate(MathMap "y=tan(x)", at=0) = -1.9950849488779807e-4.
    let m = MathMap::new(1, 1, &["y = tan(x)"], &["x = atan(y)"]).unwrap();
    let rate = m.rate(&[0.0], 1, 1).unwrap();
    assert!(
        (rate - (-1.9950849488779807e-4)).abs() < 1e-15,
        "expected C artifact -1.9950849488779807e-4, got {rate}"
    );

    // Away from zero the same Mapping converges to the true derivative
    // 1/cos^2(1) = 3.4255188224.
    let rate1 = m.rate(&[1.0], 1, 1).unwrap();
    assert!(
        (rate1 - 3.4255188224).abs() < 1e-6,
        "expected ~3.4255188224 at x=1, got {rate1}"
    );
}

#[test]
fn numeric_rate_wcsmap_nonlinear() {
    // WcsMap with TAN projection is nonlinear.
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();

    // Rate of output 1 w.r.t. input 1, away from singularity.
    let at = [0.1, 0.1];
    let rate = wcs.rate(&at, 1, 1).unwrap();

    // Should be finite and reasonable (not BAD).
    assert_ne!(rate, BAD_VALUE, "rate should not be BAD");
    assert!(rate.is_finite(), "rate should be finite, got {rate}");
    assert!(rate.abs() > 0.01, "rate should be non-trivial, got {rate}");
}

#[test]
fn numeric_rate_wcsmap_near_pole() {
    // Near the pole for TAN projection — more challenging nonlinear region.
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();

    // Rate of latitude output w.r.t. latitude input near the pole.
    let at = [0.01, 1.4]; // lat ~ 80 degrees (1.4 rad)
    let rate = wcs.rate(&at, 2, 2).unwrap();

    assert_ne!(rate, BAD_VALUE, "rate near pole should not be BAD");
    assert!(
        rate.is_finite(),
        "rate near pole should be finite, got {rate}"
    );
    // The rate should be substantial near the pole.
    assert!(
        rate.abs() > 0.1,
        "rate near pole should be non-trivial, got {rate}"
    );
}

#[test]
fn numeric_rate_consistency() {
    // The rate should be consistent at nearby points for a smooth function.
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();

    let r1 = wcs.rate(&[0.1, 0.1], 1, 1).unwrap();
    let r2 = wcs.rate(&[0.1001, 0.1], 1, 1).unwrap();

    // For a smooth function, rates at nearby points should be close.
    assert_ne!(r1, BAD_VALUE);
    assert_ne!(r2, BAD_VALUE);
    let relative_diff = (r1 - r2).abs() / r1.abs().max(1e-10);
    assert!(
        relative_diff < 0.01,
        "rates at nearby points should be similar: r1={r1}, r2={r2}, diff={relative_diff}"
    );
}

#[test]
fn numeric_rate_large_position() {
    // Test that the algorithm handles large coordinate values correctly.
    // The adaptive interval should scale with the position magnitude.
    let zoom = ZoomMap::new(1, 2.0).unwrap();
    let rate = zoom.rate(&[1e10], 1, 1).unwrap();
    assert_close(rate, 2.0, 1e-4, "zoom rate at large position");
}

#[test]
fn numeric_rate_small_position() {
    // Test at a very small coordinate value.
    let zoom = ZoomMap::new(1, 7.0).unwrap();
    let rate = zoom.rate(&[1e-15], 1, 1).unwrap();
    assert_close(rate, 7.0, 1e-4, "zoom rate at small position");
}

// ===========================================================================
// Tests using mappings that call numeric_rate (ChebyMap, WcsMap)
// ===========================================================================

#[test]
fn numeric_rate_chebymap_quadratic() {
    // f(x) = x^2 on [-2, 2] (using Chebyshev terms).
    // ChebyMap with T_2 term: T_2(u) = 2u^2 - 1 where u = (x - 0) / 2.
    // For f(x) = x^2, we need: coeff_0 * T_0 + coeff_2 * T_2
    //   T_0(u) = 1, T_2(u) = 2u^2 - 1
    //   With u = x/2: T_2(x/2) = 2*(x/2)^2 - 1 = x^2/2 - 1
    //   So x^2 = 2*T_2(x/2) + 2*T_0(x/2) = 2*T_2 + 2
    // Actually easier: use PolyTerm with power 2 directly.
    // ChebyMap T_1(u) with domain [-2,2] maps u = x/2, T_1(u) = u = x/2.
    // Let's just use power=2 for T_2.
    //
    // Simpler approach: use a ChebyMap with a known linear function
    // and verify the rate matches. f(x) = 3x on [-1,1] has df/dx = 3.
    let terms = vec![PolyTerm {
        coeff: 3.0,
        powers: vec![1],
        out_axis: 1,
    }];
    let m = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();

    // df/dx = 3 everywhere in the domain.
    let rate = m.rate(&[0.5], 1, 1).unwrap();
    assert_close(rate, 3.0, 1e-4, "chebymap linear rate at x=0.5");

    // At x=0, C's astRate seeds the interval at h0=1.0 and immediately
    // scales it up by 10, producing a sampling interval ([-5,5]) that
    // overflows the ChebyMap domain [-1,1]. Every FindGradient call then
    // fails and astRate returns AST__BAD. Oracle-verified; we match C
    // rather than the mathematically-correct gradient of 3.
    let rate_at_zero = m.rate(&[0.0], 1, 1).unwrap();
    assert_eq!(
        rate_at_zero, BAD_VALUE,
        "chebymap linear rate at x=0 (C: BAD)"
    );
}

#[test]
fn numeric_rate_chebymap_cubic() {
    // f(x) = x^3 on [-1, 1]. df/dx = 3x^2.
    // T_3(u) = 4u^3 - 3u. With domain [-1,1], u = x.
    // T_1(u) = u = x.
    // x^3 = (T_3(x) + 3*T_1(x))/4
    // So: coeff for T_3 = 1/4, coeff for T_1 = 3/4.
    let terms = vec![
        PolyTerm {
            coeff: 0.75,
            powers: vec![1],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.25,
            powers: vec![3],
            out_axis: 1,
        },
    ];
    let m = ChebyMap::new(1, 1, &terms, None, &[-1.0], &[1.0], None, None).unwrap();

    // At x=0.5, f(x) = 0.125, df/dx = 3*(0.5)^2 = 0.75.
    let rate = m.rate(&[0.5], 1, 1).unwrap();
    assert_close(rate, 0.75, 1e-3, "chebymap cubic rate at x=0.5");

    // At x=0, C returns AST__BAD for the same domain-overflow reason as the
    // linear case above (h0=1.0 → first interval [-5,5] leaves the [-1,1]
    // domain, every FindGradient fails). Oracle-verified.
    let rate_zero = m.rate(&[0.0], 1, 1).unwrap();
    assert_eq!(rate_zero, BAD_VALUE, "chebymap cubic rate at x=0 (C: BAD)");
}
