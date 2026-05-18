//! Acceptance tests for [`dyn Mapping::transform_nan_protocol`].
//!
//! Sketch validation: the public NaN-as-bad entry must produce results
//! identical (to BAD-output equivalence) to the existing `transform` method
//! across a CmpMap chain. Only one polynomial-arithmetic class (WinMap) has
//! its `transform_into_nan` override implemented in this commit — that's
//! enough to demonstrate the wiring works end-to-end. Remaining classes
//! still go through the default-impl fallback (which calls `transform_into`
//! with BAD-checking semantics), so the answer is correct regardless of
//! how many stages have opted in.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::from_native_string;
use ast_core::mappings::Mapping;

fn load_lsst() -> Box<dyn Mapping> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/lsst.ast"
    ))
    .expect("lsst.ast");
    from_native_string(&text)
        .unwrap()
        .into_mapping()
        .unwrap()
        .simplified()
        .unwrap()
}

fn make_points(n: usize) -> PointSet {
    PointSet::from_rows(
        (0..n)
            .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
            .collect::<Vec<_>>(),
    )
}

#[test]
fn nan_protocol_forward_matches_transform() {
    let map = load_lsst();
    let pts = make_points(1024);

    let regular = map.transform(&pts, true).unwrap();
    let via_nan = map.transform_nan_protocol(&pts, true).unwrap();

    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..2 {
            let diff = (r[axis] - n[axis]).abs();
            assert!(
                diff < 1e-14,
                "pt {i} axis {axis}: regular={:.17e} nan_protocol={:.17e} diff={:.2e}",
                r[axis],
                n[axis],
                diff
            );
        }
    }
}

#[test]
fn nan_protocol_inverse_matches_transform() {
    let map = load_lsst();
    let pts = make_points(1024);
    let sky = map.transform(&pts, true).unwrap();

    let regular = map.transform(&sky, false).unwrap();
    let via_nan = map.transform_nan_protocol(&sky, false).unwrap();

    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..2 {
            let diff = (r[axis] - n[axis]).abs();
            assert!(
                diff < 1e-10,
                "pt {i} axis {axis}: regular={:.17e} nan_protocol={:.17e} diff={:.2e}",
                r[axis],
                n[axis],
                diff
            );
        }
    }
}

#[test]
fn nan_protocol_slamap_matches_transform() {
    // Build a minimal SlaMap (Equatorial → Galactic) and exercise the
    // NaN-as-bad fast path directly, alongside a few BAD-injected points.
    // The pal-backed `transform_into_nan` should produce results identical
    // to the BAD-checked `transform_into`, including BAD propagation on
    // input.
    use ast_core::mappings::{SlaCvtType, SlaMap};

    let mut map = SlaMap::new();
    map.add(SlaCvtType::Eqgal, &[]);

    let mut rows: Vec<Vec<f64>> = (0..256)
        .map(|i| {
            // RA in [0, 2π), Dec in [-π/2, π/2]
            let ra = (i as f64) * 0.01;
            let dec = -0.5 + (i as f64) * 0.001;
            vec![ra, dec]
        })
        .collect();
    rows[10][0] = BAD_VALUE;
    rows[42][1] = f64::NAN;
    rows[100][0] = f64::NAN;
    rows[100][1] = BAD_VALUE;
    let pts = PointSet::from_rows(rows);

    let regular = map.transform(&pts, true).expect("scalar");
    // Wrap as a trait object to access the NaN-protocol entry.
    let map_dyn: &dyn Mapping = &map;
    let via_nan = map_dyn
        .transform_nan_protocol(&pts, true)
        .expect("nan protocol");

    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..2 {
            // Either both are finite and close, or both are BAD.
            if r[axis] == BAD_VALUE || n[axis] == BAD_VALUE {
                assert_eq!(
                    r[axis], n[axis],
                    "pt {i} ax {axis}: BAD mismatch r={} n={}",
                    r[axis], n[axis]
                );
            } else {
                let diff = (r[axis] - n[axis]).abs();
                assert!(
                    diff < 1e-14,
                    "pt {i} ax {axis}: regular={:.17e} nan={:.17e} diff={:.2e}",
                    r[axis],
                    n[axis],
                    diff
                );
            }
        }
    }
}

#[test]
fn nan_protocol_propagates_bad_and_nan_inputs() {
    let map = load_lsst();
    let mut rows: Vec<Vec<f64>> = (0..200)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    rows[0][0] = BAD_VALUE;
    rows[5][1] = f64::NAN;
    rows[100][0] = f64::NAN;
    rows[100][1] = BAD_VALUE;
    let pts = PointSet::from_rows(rows);

    let out = map.transform_nan_protocol(&pts, true).unwrap();

    for &i in &[0usize, 5, 100] {
        let p = out.point(i);
        assert_eq!(p[0], BAD_VALUE, "pt {i} axis 0: {}", p[0]);
        assert_eq!(p[1], BAD_VALUE, "pt {i} axis 1: {}", p[1]);
    }
    // Adjacent valid points must remain finite (NaN-or-BAD didn't leak).
    let p1 = out.point(1);
    assert!(p1[0].is_finite() && p1[0] != BAD_VALUE);
    assert!(p1[1].is_finite() && p1[1] != BAD_VALUE);
}
