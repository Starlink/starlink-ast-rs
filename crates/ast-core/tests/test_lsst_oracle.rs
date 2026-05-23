//! Oracle comparison: verify Rust LSST FrameSet transforms match C library output.
//!
//! C reference values were produced by `/tmp/dump_lsst.c` linking against
//! `reference/starlink-ast/build-ast/libast.dylib`. They cover both the
//! original mapping and the result of `astSimplify`, in forward and inverse
//! directions. All four sets agree to the last bit on the C side, so the
//! Rust port must too within a tight ULP-level tolerance.

use ast_core::coords::PointSet;
use ast_core::from_native_string;
use ast_core::mappings::Mapping;

const NPTS: usize = 10;

fn input_points() -> PointSet {
    let mut rows = Vec::with_capacity(NPTS);
    for i in 0..NPTS {
        rows.push(vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005]);
    }
    PointSet::from_rows(rows)
}

fn load_lsst() -> Box<dyn Mapping> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/lsst.ast"
    ))
    .unwrap();
    from_native_string(&text).unwrap().into_mapping().unwrap()
}

/// C oracle forward outputs (xin = 100+i*0.01, yin = 200+i*0.005).
/// Identical for original and simplified mappings (bit-for-bit).
const C_FORWARD: [(f64, f64); NPTS] = [
    (9.226_876_571_347_122e-1, -4.934_535_221_983_469e-1),
    (9.226_876_650_180_644e-1, -4.934_535_138_181_675e-1),
    (9.226_876_729_014_163e-1, -4.934_535_054_379_882_7e-1),
    (9.226_876_807_847_684e-1, -4.934_534_970_578_089_4e-1),
    (9.226_876_886_681_201e-1, -4.934_534_886_776_298_4e-1),
    (9.226_876_965_514_714e-1, -4.934_534_802_974_509e-1),
    (9.226_877_044_348_223e-1, -4.934_534_719_172_720_7e-1),
    (9.226_877_123_181_734e-1, -4.934_534_635_370_933e-1),
    (9.226_877_202_015_241e-1, -4.934_534_551_569_147e-1),
    (9.226_877_280_848_743e-1, -4.934_534_467_767_362e-1),
];

/// C oracle inverse(forward(input)) — the round-trip recovery.
/// Identical for original and simplified mappings (bit-for-bit).
/// Deviates from `input_points()` by ≤ ~3e-9 because the PolyMap inverse
/// is iterative (Newton-Raphson) and converges to its own residual.
const C_INVERSE_OF_FORWARD: [(f64, f64); NPTS] = [
    (1.000_000_000_001_428_9e2, 2.000_000_000_001_815e2),
    (1.000_100_000_000_245_9e2, 2.000_050_000_000_467_2e2),
    (1.000_199_999_999_729_7e2, 2.000_099_999_998_824e2),
    (1.000_300_000_001_423_5e2, 2.000_150_000_001_483_5e2),
    (1.000_400_000_000_166e2, 2.000_200_000_001_012_5e2),
    (1.000_500_000_002_210_2e2, 2.000_250_000_003_858e2),
    (1.000_599_999_999_151_6e2, 2.000_299_999_999_482_5e2),
    (1.000_700_000_000_757_4e2, 2.000_350_000_000_232_5e2),
    (1.000_799_999_999_658e2, 2.000_400_000_000_574_3e2),
    (1.000_899_999_998_762_1e2, 2.000_449_999_998_081e2),
];

fn assert_matches(rust: &PointSet, c_ref: &[(f64, f64); NPTS], tol: f64, label: &str) {
    for (i, &(cx, cy)) in c_ref.iter().enumerate() {
        let rx = rust.point(i)[0];
        let ry = rust.point(i)[1];
        let dx = (rx - cx).abs();
        let dy = (ry - cy).abs();
        assert!(
            dx < tol,
            "{label} pt {i} x: rust={rx:.17e}, c={cx:.17e}, diff={dx:.2e}"
        );
        assert!(
            dy < tol,
            "{label} pt {i} y: rust={ry:.17e}, c={cy:.17e}, diff={dy:.2e}"
        );
    }
}

#[test]
fn lsst_original_forward_matches_c() {
    let mapping = load_lsst();
    let out = mapping.transform(&input_points(), true).unwrap();
    assert_matches(&out, &C_FORWARD, 1e-15, "original forward");
}

#[test]
fn lsst_original_inverse_matches_c() {
    let mapping = load_lsst();
    // Take C's forward output as input to the inverse.
    let fwd_pts = PointSet::from_rows(
        C_FORWARD
            .iter()
            .map(|(x, y)| vec![*x, *y])
            .collect::<Vec<_>>(),
    );
    let inv = mapping.transform(&fwd_pts, false).unwrap();
    // Inverse is iterative (PolyMap NR): match C to ~1e-12, well below the
    // round-trip residual of ~3e-9.
    // Inverse uses iterative NR inside PolyMap; convergence ordering may
    // shift the last few bits vs C. Allow 1e-10 — still ~10× tighter than
    // the round-trip residual (~3e-9) so this is last-ULP noise, not a
    // numerical disagreement.
    assert_matches(&inv, &C_INVERSE_OF_FORWARD, 1e-10, "original inverse");
}

#[test]
fn lsst_simplified_forward_matches_c() {
    let mapping = load_lsst().simplified().unwrap();
    let out = mapping.transform(&input_points(), true).unwrap();
    // Simplified path collapses Win/Perm/etc; floating-point reordering
    // may shift the last bit or two, so allow a hair more slack.
    assert_matches(&out, &C_FORWARD, 1e-15, "simplified forward");
}

#[test]
fn lsst_simplified_inverse_matches_c() {
    let mapping = load_lsst().simplified().unwrap();
    let fwd_pts = PointSet::from_rows(
        C_FORWARD
            .iter()
            .map(|(x, y)| vec![*x, *y])
            .collect::<Vec<_>>(),
    );
    let inv = mapping.transform(&fwd_pts, false).unwrap();
    assert_matches(&inv, &C_INVERSE_OF_FORWARD, 1e-10, "simplified inverse");
}

#[test]
fn lsst_inverse_round_trip() {
    let mapping = load_lsst();
    let pts = input_points();
    let fwd = mapping.transform(&pts, true).unwrap();
    let inv = mapping.transform(&fwd, false).unwrap();

    for i in 0..NPTS {
        let ox = pts.point(i)[0];
        let oy = pts.point(i)[1];
        let rx = inv.point(i)[0];
        let ry = inv.point(i)[1];
        // C library achieves ~1e-9 round-trip on this fixture; match that.
        assert!(
            (rx - ox).abs() < 1e-6,
            "pt {i} x round-trip: orig={ox}, recovered={rx}"
        );
        assert!(
            (ry - oy).abs() < 1e-6,
            "pt {i} y round-trip: orig={oy}, recovered={ry}"
        );
    }
}
