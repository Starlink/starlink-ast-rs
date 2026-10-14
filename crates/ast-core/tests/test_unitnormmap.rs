//! Tests for UnitNormMap — port of reference/starlink-ast/ast_tester/testunitnormmap.c logic.
//!
//! UnitNormMap decomposes a vector into a unit vector and its norm,
//! relative to a specified centre.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, UnitNormMap, WinMap};

fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

// ---------------------------------------------------------------------------
// Forward transform tests
// ---------------------------------------------------------------------------

#[test]
fn forward_basic_origin() {
    // centre = [0, 0], input = [3, 4] → norm = 5, unit = [0.6, 0.8]
    let unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    assert_eq!(unm.nin(), 2);
    assert_eq!(unm.nout(), 3);

    let pts = PointSet::from_rows([[3.0, 4.0]]);
    let out = unm.transform(&pts, true).unwrap();
    assert_eq!(out.ncoord(), 3);

    let p = out.point(0);
    assert!((p[0] - 0.6).abs() < 1e-14);
    assert!((p[1] - 0.8).abs() < 1e-14);
    assert!((p[2] - 5.0).abs() < 1e-14);
}

#[test]
fn forward_with_centre() {
    // centre = [1, -1], input = [5, 2] → v = [4, 3], norm = 5
    // unit = [0.8, 0.6], output = [0.8, 0.6, 5.0]
    let unm = UnitNormMap::new(vec![1.0, -1.0]).unwrap();
    let pts = PointSet::from_rows([[5.0, 2.0]]);
    let out = unm.transform(&pts, true).unwrap();

    let p = out.point(0);
    assert!((p[0] - 0.8).abs() < 1e-14);
    assert!((p[1] - 0.6).abs() < 1e-14);
    assert!((p[2] - 5.0).abs() < 1e-14);
}

#[test]
fn forward_zero_vector() {
    // Input equals centre → norm = 0, unit vector = BAD
    let unm = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = unm.transform(&pts, true).unwrap();

    let p = out.point(0);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));
    assert_eq!(p[2], 0.0);
}

#[test]
fn forward_bad_input() {
    // Any bad input → all outputs bad
    let unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 4.0]]);
    let out = unm.transform(&pts, true).unwrap();

    let p = out.point(0);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));
    assert!(is_bad(p[2]));
}

#[test]
fn forward_multiple_points() {
    let unm = UnitNormMap::new(vec![0.0, 0.0, 0.0]).unwrap();
    // 3D: [1,0,0] → unit=[1,0,0], norm=1
    //     [0,0,0] → zero vector case
    let pts = PointSet::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, 0.0]]);
    let out = unm.transform(&pts, true).unwrap();

    // First point: [1, 0, 0] → [1, 0, 0, 1]
    let p0 = out.point(0);
    assert!((p0[0] - 1.0).abs() < 1e-14);
    assert!((p0[1] - 0.0).abs() < 1e-14);
    assert!((p0[2] - 0.0).abs() < 1e-14);
    assert!((p0[3] - 1.0).abs() < 1e-14);

    // Second point: [0, 0, 0] → [BAD, BAD, BAD, 0]
    let p1 = out.point(1);
    assert!(is_bad(p1[0]));
    assert!(is_bad(p1[1]));
    assert!(is_bad(p1[2]));
    assert_eq!(p1[3], 0.0);
}

// ---------------------------------------------------------------------------
// Inverse transform tests
// ---------------------------------------------------------------------------

#[test]
fn inverse_basic() {
    // Inverse: unit = [0.6, 0.8], norm = 5, centre = [0, 0] → [3, 4]
    let unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let pts = PointSet::from_rows([[0.6, 0.8, 5.0]]);
    let out = unm.transform(&pts, false).unwrap();

    let p = out.point(0);
    assert!((p[0] - 3.0).abs() < 1e-14);
    assert!((p[1] - 4.0).abs() < 1e-14);
}

#[test]
fn inverse_with_centre() {
    // unit = [0.8, 0.6], norm = 5, centre = [1, -1] → [5, 2]
    let unm = UnitNormMap::new(vec![1.0, -1.0]).unwrap();
    let pts = PointSet::from_rows([[0.8, 0.6, 5.0]]);
    let out = unm.transform(&pts, false).unwrap();

    let p = out.point(0);
    assert!((p[0] - 5.0).abs() < 1e-14);
    assert!((p[1] - 2.0).abs() < 1e-14);
}

#[test]
fn inverse_zero_norm() {
    // Norm = 0 → output is the centre, regardless of unit vector
    let unm = UnitNormMap::new(vec![3.0, 7.0]).unwrap();
    let pts = PointSet::from_rows([[999.0, -42.0, 0.0]]);
    let out = unm.transform(&pts, false).unwrap();

    let p = out.point(0);
    assert_eq!(p[0], 3.0);
    assert_eq!(p[1], 7.0);
}

#[test]
fn inverse_bad_norm() {
    // Bad norm → all outputs bad
    let unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let pts = PointSet::from_rows([[0.6, 0.8, BAD_VALUE]]);
    let out = unm.transform(&pts, false).unwrap();

    let p = out.point(0);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));
}

#[test]
fn inverse_bad_component() {
    // Bad unit vector component → corresponding output is bad
    let unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 0.8, 5.0]]);
    let out = unm.transform(&pts, false).unwrap();

    let p = out.point(0);
    assert!(is_bad(p[0]));
    assert!((p[1] - 4.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_forward_inverse() {
    let unm = UnitNormMap::new(vec![2.0, -3.0, 1.5]).unwrap();
    let pts = PointSet::from_rows([[5.0, -1.0, 4.0]]);

    // Forward
    let mid = unm.transform(&pts, true).unwrap();
    // Inverse
    let out = unm.transform(&mid, false).unwrap();

    let p = out.point(0);
    assert!((p[0] - 5.0).abs() < 1e-12);
    assert!((p[1] - (-1.0)).abs() < 1e-12);
    assert!((p[2] - 4.0).abs() < 1e-12);
}

#[test]
fn roundtrip_inverse_forward() {
    // Start with a unit-norm representation and go inverse then forward.
    let unm = UnitNormMap::new(vec![1.0, 1.0]).unwrap();

    // Start with unit=[0.6, 0.8], norm=10 → point=[7, 9]
    let pts = PointSet::from_rows([[0.6, 0.8, 10.0]]);
    let mid = unm.transform(&pts, false).unwrap();
    let out = unm.transform(&mid, true).unwrap();

    let p = out.point(0);
    assert!((p[0] - 0.6).abs() < 1e-12);
    assert!((p[1] - 0.8).abs() < 1e-12);
    assert!((p[2] - 10.0).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Invert flag tests
// ---------------------------------------------------------------------------

#[test]
fn inverted_nin_nout() {
    let mut unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    assert_eq!(unm.nin(), 2);
    assert_eq!(unm.nout(), 3);
    assert!(!unm.is_inverted());

    unm.invert();
    assert_eq!(unm.nin(), 3);
    assert_eq!(unm.nout(), 2);
    assert!(unm.is_inverted());
}

#[test]
fn inverted_transform() {
    // When inverted, forward request does the inverse math and vice versa.
    let mut unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    unm.invert();

    // "forward" on inverted map = inverse math: [0.6, 0.8, 5.0] → [3, 4]
    let pts = PointSet::from_rows([[0.6, 0.8, 5.0]]);
    let out = unm.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - 3.0).abs() < 1e-14);
    assert!((p[1] - 4.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// equal() tests
// ---------------------------------------------------------------------------

#[test]
fn equal_same() {
    let a = UnitNormMap::new(vec![1.0, 2.0, 3.0]).unwrap();
    let b = UnitNormMap::new(vec![1.0, 2.0, 3.0]).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn equal_different_centre() {
    let a = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let b = UnitNormMap::new(vec![1.0, 3.0]).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn equal_different_dim() {
    let a = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let b = UnitNormMap::new(vec![1.0, 2.0, 0.0]).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn equal_different_invert() {
    let a = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let mut b = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    b.invert();
    assert!(!a.equal(&b));
}

// ---------------------------------------------------------------------------
// Serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn serialization_roundtrip() {
    use ast_core::{AnyObject, from_native_string, to_native_string};

    let unm = UnitNormMap::new(vec![1.5, -2.0, 0.0]).unwrap();
    let obj = AnyObject::UnitNormMap(unm.clone());
    let s = to_native_string(&obj).unwrap();

    // Check the string contains expected tokens.
    assert!(s.contains("UnitNormMap"));
    assert!(s.contains("Ctr1"));
    assert!(s.contains("Ctr2"));
    // Ctr3 is 0.0, so should be omitted.
    assert!(!s.contains("Ctr3"));

    let obj2 = from_native_string(&s).unwrap();
    let unm2 = match obj2 {
        AnyObject::UnitNormMap(m) => m,
        _ => panic!("expected UnitNormMap"),
    };

    assert!(unm.equal(&unm2));
    assert_eq!(unm2.centre(), &[1.5, -2.0, 0.0]);
}

#[test]
fn serialization_inverted_roundtrip() {
    use ast_core::{AnyObject, from_native_string, to_native_string};

    let mut unm = UnitNormMap::new(vec![10.0, 20.0]).unwrap();
    unm.invert();

    let obj = AnyObject::UnitNormMap(unm.clone());
    let s = to_native_string(&obj).unwrap();
    assert!(s.contains("Invert"));

    let obj2 = from_native_string(&s).unwrap();
    let unm2 = match obj2 {
        AnyObject::UnitNormMap(m) => m,
        _ => panic!("expected UnitNormMap"),
    };

    assert!(unm.equal(&unm2));
    assert!(unm2.is_inverted());
}

// ---------------------------------------------------------------------------
// Simplification / merge tests
// ---------------------------------------------------------------------------

#[test]
fn merge_inverse_cancel_same_centre() {
    // UnitNormMap(fwd) + UnitNormMap(inv) with same centre → UnitMap
    // In the simplify engine, mappings are stored in forward state,
    // with effective inversion carried by the bool flags.
    use ast_core::mappings::UnitMap;

    let unm_fwd = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let unm_other = UnitNormMap::new(vec![1.0, 2.0]).unwrap(); // stored in forward state

    // self_inv=false (forward), other_inv=true (effectively inverted)
    let result = unm_fwd
        .try_merge(&unm_other as &dyn Mapping, true, false, true)
        .expect("should merge");
    assert!(result.as_any().is::<UnitMap>());
    assert_eq!(result.nin(), 2);
}

#[test]
fn merge_different_centre_to_shift() {
    // UnitNormMap(fwd, centre1) + UnitNormMap(inv, centre2) → ShiftMap(centre2 - centre1)
    let unm1 = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let unm2 = UnitNormMap::new(vec![4.0, 6.0]).unwrap(); // stored forward, inv flag = true

    // self_inv=false (forward), other_inv=true (effectively inverted)
    let result = unm1
        .try_merge(&unm2 as &dyn Mapping, true, false, true)
        .expect("should merge");

    // Verify it's a ShiftMap by checking nin/nout and applying it.
    let shift_map = result.as_any().downcast_ref::<ShiftMap>().unwrap();
    assert_eq!(shift_map.nin(), 2);
    // Apply the shift: [0, 0] + [3, 4] = [3, 4]
    let pts = PointSet::from_rows([[0.0, 0.0]]);
    let out = shift_map.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - 3.0).abs() < 1e-14);
    assert!((p[1] - 4.0).abs() < 1e-14);
}

#[test]
fn merge_unitnormmap_inv_plus_shift() {
    // UnitNormMap(inv) + ShiftMap → UnitNormMap(inv) with adjusted centre
    // In the simplify engine, self has inverted=false, self_inv=true
    let unm = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let shift = ShiftMap::new(vec![3.0, -1.0]);

    // self=unm (stored forward, but effectively inverted via self_inv=true)
    // other=shift (forward, other_inv=false)
    let result = unm
        .try_merge(&shift as &dyn Mapping, true, true, false)
        .expect("should merge");

    let merged = result.as_any().downcast_ref::<UnitNormMap>().unwrap();
    assert!(merged.is_inverted());
    // New centre = old centre + shift = [4.0, 1.0]
    assert!((merged.centre()[0] - 4.0).abs() < 1e-14);
    assert!((merged.centre()[1] - 1.0).abs() < 1e-14);
}

#[test]
fn merge_shift_plus_unitnormmap_fwd() {
    // ShiftMap + UnitNormMap(fwd) → UnitNormMap(fwd) with adjusted centre
    let shift = ShiftMap::new(vec![3.0, -1.0]);
    let unm = UnitNormMap::new(vec![5.0, 10.0]).unwrap();

    // ShiftMap.try_merge(UnitNormMap, series=true, shift_inv=false, unm_inv=false)
    let result = shift
        .try_merge(&unm as &dyn Mapping, true, false, false)
        .expect("should merge");

    let merged = result.as_any().downcast_ref::<UnitNormMap>().unwrap();
    assert!(!merged.is_inverted());
    // New centre = old centre - shift = [2.0, 11.0]
    assert!((merged.centre()[0] - 2.0).abs() < 1e-14);
    assert!((merged.centre()[1] - 11.0).abs() < 1e-14);
}

#[test]
fn merge_winmap_unit_scale_plus_unitnormmap_fwd() {
    // WinMap(unit scale, shift=[2,3]) + UnitNormMap(fwd, centre=[5,10])
    // → UnitNormMap(fwd, centre=[5-2, 10-3]=[3, 7])
    let win = WinMap::new(&[2.0, 3.0], &[1.0, 1.0]).unwrap();
    let unm = UnitNormMap::new(vec![5.0, 10.0]).unwrap();

    let result = win
        .try_merge(&unm as &dyn Mapping, true, false, false)
        .expect("should merge");

    let merged = result.as_any().downcast_ref::<UnitNormMap>().unwrap();
    assert!(!merged.is_inverted());
    assert!((merged.centre()[0] - 3.0).abs() < 1e-14);
    assert!((merged.centre()[1] - 7.0).abs() < 1e-14);
}

#[test]
fn no_merge_winmap_non_unit_scale() {
    // WinMap with non-unit scale should NOT merge with UnitNormMap.
    let win = WinMap::new(&[2.0, 3.0], &[2.0, 1.0]).unwrap();
    let unm = UnitNormMap::new(vec![5.0, 10.0]).unwrap();

    let result = win.try_merge(&unm as &dyn Mapping, true, false, false);
    assert!(result.is_none());
}

#[test]
fn merge_unitnormmap_inv_plus_winmap_unit_scale() {
    // UnitNormMap(inv, centre=[1,2]) + WinMap(unit scale, shift=[3,-1])
    // → UnitNormMap(inv, centre=[4,1])
    // Stored in forward state, effective inv via self_inv=true
    let unm = UnitNormMap::new(vec![1.0, 2.0]).unwrap();
    let win = WinMap::new(&[3.0, -1.0], &[1.0, 1.0]).unwrap();

    let result = unm
        .try_merge(&win as &dyn Mapping, true, true, false)
        .expect("should merge");

    let merged = result.as_any().downcast_ref::<UnitNormMap>().unwrap();
    assert!(merged.is_inverted());
    assert!((merged.centre()[0] - 4.0).abs() < 1e-14);
    assert!((merged.centre()[1] - 1.0).abs() < 1e-14);
}

#[test]
fn no_merge_parallel() {
    // UnitNormMaps cannot merge in parallel.
    let unm1 = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let unm2 = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let result = unm1.try_merge(&unm2 as &dyn Mapping, false, false, false);
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// 1-D case
// ---------------------------------------------------------------------------

#[test]
fn one_dimensional() {
    // 1D: centre = [2], input = [5] → v = [3], norm = 3, unit = [1]
    let unm = UnitNormMap::new(vec![2.0]).unwrap();
    assert_eq!(unm.nin(), 1);
    assert_eq!(unm.nout(), 2);

    let pts = PointSet::from_rows([[5.0]]);
    let out = unm.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - 1.0).abs() < 1e-14);
    assert!((p[1] - 3.0).abs() < 1e-14);

    // Negative direction: input = [-1] → v = [-3], norm = 3, unit = [-1]
    let pts = PointSet::from_rows([[-1.0]]);
    let out = unm.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - (-1.0)).abs() < 1e-14);
    assert!((p[1] - 3.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Overflow protection (scaled sum)
// ---------------------------------------------------------------------------

#[test]
fn large_values_no_overflow() {
    // Values near f64::MAX / 2 should not overflow in norm calculation.
    let big = 1.0e300;
    let unm = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let pts = PointSet::from_rows([[big, big]]);
    let out = unm.transform(&pts, true).unwrap();

    let p = out.point(0);
    // Unit vector should be [1/sqrt(2), 1/sqrt(2)]
    let expected = 1.0 / 2.0_f64.sqrt();
    assert!((p[0] - expected).abs() < 1e-10);
    assert!((p[1] - expected).abs() < 1e-10);
    // Norm should be big*sqrt(2)
    let expected_norm = big * 2.0_f64.sqrt();
    assert!((p[2] - expected_norm).abs() / expected_norm < 1e-10);
}

// ---------------------------------------------------------------------------
// testunitnormmap.c main loop: IsLinear and checkroundtrip
// ---------------------------------------------------------------------------

/// C's `checkroundtrip` (testunitnormmap.c:66-87): `pos` survives the
/// forward then inverse transform, and the series CmpMap of the Mapping and
/// its inverse.
fn round_trips(map: &UnitNormMap, pos: &[f64]) -> bool {
    let pts = PointSet::from_rows([pos.to_vec()]);
    let out = map.transform(&pts, true).unwrap();
    let back = map.transform(&out, false).unwrap();
    let mut inv = map.clone();
    inv.invert();
    let cmp = CmpMap::new(Box::new(map.clone()), Box::new(inv), Composition::Series).unwrap();
    let through = cmp.transform(&pts, true).unwrap();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-10 * a.abs().max(b.abs()).max(1.0);
    pos.iter()
        .enumerate()
        .all(|(i, &p)| close(back.point(0)[i], p) && close(through.point(0)[i], p))
}

/// Errors 5, 6 and 6b of testunitnormmap.c for one to three axes. At the
/// centre the forward transform gives BAD unit components and a zero norm,
/// and the inverse gives the centre back because a zero norm takes precedence
/// over BAD components (unitnormmap.c inverse branch).
#[test]
fn not_linear_and_round_trips_including_the_centre() {
    let centre = [-1.0, 1.0, 2.0];
    let frompos = [-22.0, 3.0, 0.5];
    for nin in 1..=3 {
        let map = UnitNormMap::new(centre[..nin].to_vec()).unwrap();
        assert!(!map.is_linear(), "Error 5, nin={nin}");
        assert!(round_trips(&map, &frompos[..nin]), "Error 6, nin={nin}");
        assert!(round_trips(&map, &centre[..nin]), "Error 6b, nin={nin}");
    }
}

/// `testunitnormmap.c` `testsimplify`: seven series pairs of UnitNormMaps,
/// a ShiftMap and unit-scale WinMaps, each simplified to C's class with the
/// same inputs, outputs and transformed values (to 1e-14 relative, C's
/// `differ`).
#[test]
fn c_testsimplify() {
    let differ = |a: f64, b: f64| (a - b).abs() > (0.5 * (a + b)).abs().max(1.0) * 1.0e-14;
    let centre1 = [2.0, -1.0, 0.0];
    let centre2 = [-1.0, 6.0, 4.0];
    let shift = [3.0, 7.0, -9.0];
    let unm1 = UnitNormMap::new(centre1).unwrap();
    let mut unm1inv = unm1.clone();
    unm1inv.invert();
    let unm2 = UnitNormMap::new(centre2).unwrap();
    let mut unm2inv = unm2.clone();
    unm2inv.invert();
    let shiftmap = ShiftMap::new(shift.to_vec());
    let winmap = |scale: f64| {
        let b: Vec<f64> = shift.iter().map(|s| scale + s).collect();
        WinMap::from_corners(&[0.0; 3], &shift, &[1.0; 3], &b).unwrap()
    };
    let (winmap1, winmap2) = (winmap(1.0), winmap(2.0));

    let cases: [(Box<dyn Mapping>, Box<dyn Mapping>, &str); 7] = [
        (Box::new(unm1.clone()), Box::new(unm2inv), "ShiftMap"),
        (Box::new(shiftmap.clone()), Box::new(unm2), "UnitNormMap"),
        (
            Box::new(winmap1.clone()),
            Box::new(unm1.clone()),
            "UnitNormMap",
        ),
        (Box::new(winmap2.clone()), Box::new(unm1), "CmpMap"),
        (Box::new(unm1inv.clone()), Box::new(shiftmap), "UnitNormMap"),
        (Box::new(unm1inv.clone()), Box::new(winmap1), "UnitNormMap"),
        (Box::new(unm1inv), Box::new(winmap2), "CmpMap"),
    ];
    // C's test points, coordinate by coordinate.
    let coords = [
        [1.0, 3.0, -5.0],
        [2.0, 3.0, 99.0],
        [-6.0, -5.0, -7.0],
        [30.0, 21.0, 37.0],
    ];
    for (i, (m1, m2, cls)) in cases.into_iter().enumerate() {
        let cmp = CmpMap::new(m1, m2, Composition::Series).unwrap();
        let simp = (&cmp as &dyn Mapping).simplify().unwrap();
        let dump =
            ast_core::to_native_string(&ast_core::AnyObject::from_mapping(simp.as_ref()).unwrap())
                .unwrap();
        let got = dump
            .lines()
            .find_map(|l| l.trim().strip_prefix("Begin "))
            .and_then(|l| l.split_whitespace().next())
            .unwrap();
        assert_eq!(got, cls, "case {i}: Simplify error 1");
        assert_eq!(simp.nin(), cmp.nin(), "case {i}: Simplify error 2");
        assert_eq!(simp.nout(), cmp.nout(), "case {i}: Simplify error 3");
        let cols: Vec<&[f64]> = coords[..cmp.nin()].iter().map(|c| &c[..]).collect();
        let pts = PointSet::from_columns(&cols).unwrap();
        let a = cmp.transform(&pts, true).unwrap();
        let b = simp.transform(&pts, true).unwrap();
        for k in 0..3 {
            for j in 0..cmp.nout() {
                assert!(
                    !differ(a.point(k)[j], b.point(k)[j]),
                    "case {i}: Simplify error 4: {:?} vs {:?}",
                    a.point(k),
                    b.point(k)
                );
            }
        }
    }
}
