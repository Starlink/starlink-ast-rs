use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, WinMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

#[test]
fn winmap_dimensions() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);
}

#[test]
fn winmap_forward() {
    // output = shift + scale * input
    // axis 1: 10 + 2 * 1 = 12
    // axis 2: 20 + 3 * 2 = 26
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[12.0, 26.0]);
}

#[test]
fn winmap_inverse() {
    // inverse: (input - shift) / scale
    // axis 1: (12 - 10) / 2 = 1
    // axis 2: (26 - 20) / 3 = 2
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[12.0, 26.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn winmap_from_corners() {
    // [0,0]->[10,10] maps to [100,200]->[200,400]
    // axis 1: scale = (200-100)/(10-0) = 10, shift = 100 - 10*0 = 100
    // axis 2: scale = (400-200)/(10-0) = 20, shift = 200 - 20*0 = 200
    let m =
        WinMap::from_corners(&[0.0, 0.0], &[10.0, 10.0], &[100.0, 200.0], &[200.0, 400.0]).unwrap();

    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    // axis 1: 100 + 10 * 5 = 150
    // axis 2: 200 + 20 * 5 = 300
    assert_eq!(out.point(0), &[150.0, 300.0]);

    // Check the corners themselves
    let pts_c1 = PointSet::from_rows([[0.0, 0.0]]);
    let out_c1 = m.transform(&pts_c1, true).unwrap();
    assert_eq!(out_c1.point(0), &[100.0, 200.0]);

    let pts_c2 = PointSet::from_rows([[10.0, 10.0]]);
    let out_c2 = m.transform(&pts_c2, true).unwrap();
    assert_eq!(out_c2.point(0), &[200.0, 400.0]);
}

#[test]
fn winmap_from_corners_zero_range() {
    // If input range is zero, shift and scale become BAD_VALUE
    let m =
        WinMap::from_corners(&[5.0, 0.0], &[5.0, 10.0], &[100.0, 200.0], &[200.0, 400.0]).unwrap();

    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    // axis 1 is BAD (zero input range), axis 2 is fine
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 300.0);
}

#[test]
fn winmap_zero_scale_inverse() {
    // A zero scale makes the inverse undefined on that axis
    let m = WinMap::new(&[10.0, 20.0], &[0.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[12.0, 26.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 2.0);
}

#[test]
fn winmap_bad_input_propagation() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();

    // BAD_VALUE in axis 1 -> only axis 1 output BAD (per-axis handling)
    let pts = PointSet::from_rows([[BAD_VALUE, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 26.0); // 20 + 3*2

    // NaN in axis 1 -> only axis 1 output BAD
    let pts_nan = PointSet::from_rows([[f64::NAN, 2.0]]);
    let out_nan = m.transform(&pts_nan, true).unwrap();
    assert_eq!(out_nan.point(0)[0], BAD_VALUE);
    assert_eq!(out_nan.point(0)[1], 26.0); // 20 + 3*2
}

#[test]
fn winmap_bad_shift_propagation() {
    // Axis 1 has BAD shift -> output axis 1 is BAD, axis 2 is fine
    let m = WinMap::new(&[BAD_VALUE, 20.0], &[2.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 26.0);
}

#[test]
fn winmap_rate_diagonal() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    // On-diagonal: rate = scale for that axis
    let r1 = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r1 - 2.0).abs() < 1e-10);
    let r2 = m.rate(&[1.0, 1.0], 2, 2).unwrap();
    assert!((r2 - 3.0).abs() < 1e-10);
}

#[test]
fn winmap_rate_off_diagonal() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let r = m.rate(&[1.0, 1.0], 1, 2).unwrap();
    assert_eq!(r, 0.0);
    let r2 = m.rate(&[1.0, 1.0], 2, 1).unwrap();
    assert_eq!(r2, 0.0);
}

#[test]
fn winmap_rate_inverted() {
    let mut m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    m.invert();
    // When inverted, rate = 1/scale
    let r1 = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r1 - 0.5).abs() < 1e-10);
    let r2 = m.rate(&[1.0, 1.0], 2, 2).unwrap();
    assert!((r2 - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn winmap_invert_flag() {
    let mut m = WinMap::new(&[10.0], &[2.0]).unwrap();
    assert!(!m.is_inverted());
    m.invert();
    assert!(m.is_inverted());

    // After invert, forward applies the inverse formula
    let pts = PointSet::from_rows([[12.0]]);
    let out = m.transform(&pts, true).unwrap();
    // "forward" on inverted map = inverse: (12 - 10) / 2 = 1
    assert_eq!(out.point(0), &[1.0]);
}

#[test]
fn winmap_serialisation_round_trip() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let text = to_native_string(&AnyObject::WinMap(m.clone())).unwrap();
    let restored = from_native_string(&text).unwrap();
    let m2 = restored.into_mapping().unwrap();

    // Verify the restored mapping produces the same result
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    assert_eq!(out1.point(0), out2.point(0));
}

#[test]
fn winmap_serialisation_text_content() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let text = to_native_string(&AnyObject::WinMap(m)).unwrap();
    assert!(text.contains("Begin WinMap"));
    assert!(text.contains("Sft1"));
    assert!(text.contains("Sft2"));
    assert!(text.contains("Scl1"));
    assert!(text.contains("Scl2"));
    assert!(text.contains("End WinMap"));
}

#[test]
fn winmap_serialisation_default_omission() {
    // Zero shifts and unit scales are omitted from native output
    let m = WinMap::new(&[0.0, 0.0], &[1.0, 1.0]).unwrap();
    let text = to_native_string(&AnyObject::WinMap(m)).unwrap();
    assert!(text.contains("Begin WinMap"));
    assert!(!text.contains("Sft1"));
    assert!(!text.contains("Sft2"));
    assert!(!text.contains("Scl1"));
    assert!(!text.contains("Scl2"));
}

#[test]
fn winmap_validation_mismatched_lengths() {
    let result = WinMap::new(&[1.0, 2.0], &[3.0]);
    assert!(result.is_err());
}

#[test]
fn winmap_validation_empty() {
    let result = WinMap::new(&[], &[]);
    assert!(result.is_err());
}

#[test]
fn winmap_bad_per_axis() {
    // WinMap is diagonal: each output depends only on its corresponding input.
    // BAD on one axis must not contaminate other axes.
    let wm = WinMap::new(&[1.0, 2.0, 3.0], &[2.0, 3.0, 4.0]).unwrap();

    // Forward with axis 2 (middle) BAD
    let pts = PointSet::from_rows([[5.0, BAD_VALUE, 7.0]]);
    let out = wm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0 + 2.0 * 5.0); // 11.0
    assert_eq!(out.point(0)[1], BAD_VALUE);
    assert_eq!(out.point(0)[2], 3.0 + 4.0 * 7.0); // 31.0

    // Inverse with axis 1 (first) BAD
    let pts_inv = PointSet::from_rows([[BAD_VALUE, 20.0, 31.0]]);
    let out_inv = wm.transform(&pts_inv, false).unwrap();
    assert_eq!(out_inv.point(0)[0], BAD_VALUE);
    assert_eq!(out_inv.point(0)[1], (20.0 - 2.0) / 3.0); // 6.0
    assert_eq!(out_inv.point(0)[2], (31.0 - 3.0) / 4.0); // 7.0
}

#[test]
fn winmap_parallel_merge() {
    use ast_core::mappings::WinMap;

    let w1 = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    let w2 = WinMap::new(&[5.0], &[6.0]).unwrap();

    // Parallel merge: result should be a 3-axis WinMap.
    let merged = w1.try_merge(&w2, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);

    // Verify output: w1 on [1,2] then w2 on [3]
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let actual = m.transform(&pts, true).unwrap();
    assert_eq!(actual.point(0)[0], 1.0 + 3.0 * 1.0); // 4.0
    assert_eq!(actual.point(0)[1], 2.0 + 4.0 * 2.0); // 10.0
    assert_eq!(actual.point(0)[2], 5.0 + 6.0 * 3.0); // 23.0
}

#[test]
fn winmap_parallel_merge_with_zoommap() {
    use ast_core::mappings::ZoomMap;

    let w = WinMap::new(&[1.0], &[2.0]).unwrap();
    let z = ZoomMap::new(2, 3.0).unwrap();

    // WinMap parallel with ZoomMap
    let merged = w.try_merge(&z, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert_eq!(m.nin(), 3);

    let pts = PointSet::from_rows([[5.0, 7.0, 9.0]]);
    let actual = m.transform(&pts, true).unwrap();
    assert_eq!(actual.point(0)[0], 1.0 + 2.0 * 5.0); // 11.0
    assert_eq!(actual.point(0)[1], 3.0 * 7.0); // 21.0
    assert_eq!(actual.point(0)[2], 3.0 * 9.0); // 27.0
}

#[test]
fn winmap_merge_with_diagonal_matrix() {
    use ast_core::mappings::{MatrixMap, WinMap};
    use ndarray::array;

    // WinMap [shift=1, scale=2] for each axis, followed by diagonal MatrixMap [[3,0],[0,4]]
    let wm = WinMap::new(&[1.0, 1.0], &[2.0, 2.0]).unwrap();
    let m = array![[3.0, 0.0], [0.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();

    // Should merge: WinMap then MatrixMap = (shift=1, scale=2) then (scale=3 or 4)
    // Combined: shift = 3*1 = 3, scale = 3*2 = 6 for axis 0
    //           shift = 4*1 = 4, scale = 4*2 = 8 for axis 1
    let merged = wm.try_merge(&mm, true, false, false);
    assert!(merged.is_some());

    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let wm_out = wm.transform(&pts, true).unwrap();
    let expected = mm.transform(&wm_out, true).unwrap();
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
    assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
}

#[test]
fn winmap_no_merge_with_full_matrix() {
    use ast_core::mappings::{MatrixMap, WinMap};
    use ndarray::array;

    // Non-diagonal matrix should NOT merge
    let wm = WinMap::new(&[1.0, 1.0], &[2.0, 2.0]).unwrap();
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();

    let merged = wm.try_merge(&mm, true, false, false);
    assert!(merged.is_none());
}

#[test]
fn winmap_simplifies_to_shiftmap() {
    use ast_core::mappings::{Mapping, ShiftMap, WinMap};

    // All scales = 1.0 → should simplify to ShiftMap
    let wm = WinMap::new(&[3.0, 5.0], &[1.0, 1.0]).unwrap();
    let s = <dyn Mapping>::simplify(&wm).unwrap();
    assert!(s.as_any().is::<ShiftMap>());
    assert_eq!(s.nin(), 2);
}

#[test]
fn winmap_simplifies_to_zoommap() {
    use ast_core::mappings::{Mapping, WinMap, ZoomMap};

    // All shifts = 0.0, uniform scale → should simplify to ZoomMap
    let wm = WinMap::new(&[0.0, 0.0], &[4.0, 4.0]).unwrap();
    let s = <dyn Mapping>::simplify(&wm).unwrap();
    assert!(s.as_any().is::<ZoomMap>());
}

#[test]
fn winmap_simplifies_to_unitmap() {
    use ast_core::mappings::{Mapping, UnitMap, WinMap};

    // Identity: all shifts=0, all scales=1
    let wm = WinMap::new(&[0.0, 0.0], &[1.0, 1.0]).unwrap();
    let s = <dyn Mapping>::simplify(&wm).unwrap();
    assert!(s.as_any().is::<UnitMap>());
}

#[test]
fn winmap_non_uniform_stays_winmap() {
    use ast_core::mappings::{Mapping, WinMap};

    // Non-uniform scales → stays WinMap
    let wm = WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
    let s = <dyn Mapping>::simplify(&wm).unwrap();
    assert!(s.as_any().is::<WinMap>());
}

#[test]
fn winmap_cancels_with_own_inverse_exactly() {
    // C winmap.c WinWin: a WinMap composed in series with its own inverse
    // simplifies to the *exact* identity. The equal-and-opposite check
    // compares the raw (uninverted) shift/scale terms, so it never carries the
    // last-bit drift that `scale * (1/scale)` would introduce. Scales chosen to
    // be deliberately not exactly representable.
    use ast_core::mappings::{CmpMap, Composition, DynMapping, Mapping, UnitMap, WinMap};

    let w = WinMap::new(&[3.5, -2.0], &[0.999_999_999_977_888, 1.000_000_3]).unwrap();
    let mut winv = w.clone();
    winv.invert();

    let cmp: DynMapping =
        Box::new(CmpMap::new(Box::new(w), Box::new(winv), Composition::Series).unwrap());
    let simp = cmp.simplify().unwrap();

    // The exact identity collapses to a UnitMap (the merge yields scale 1.0 /
    // shift 0.0 exactly, which the series loop then strips).
    assert!(
        simp.as_any().is::<UnitMap>(),
        "W ∘ W^-1 should simplify to the exact identity (UnitMap)"
    );
}

#[test]
fn offdiagonal_cd_chain_fuses_deg2rad_into_the_matrixmap() {
    // The FITS-WCS read path for a header with an off-diagonal CD matrix builds
    //
    //   WinMap(-CRPIX, unit scale) ; MatrixMap(CD) ; UnitMap ;
    //   MatrixMap(deg2rad, diagonal) ; WcsMap(TAN)^-1
    //
    // and C simplifies it to a leading ShiftMap plus a single full MatrixMap
    // carrying CD·(pi/180). Reaching that shape depends on the WinMap-family
    // direct merges staying WinMaps: `matrixmap.c::MapMerge` will swap a
    // nominated MatrixMap past a WinMap or a PermMap (matrixmap.c:2102-2104)
    // but not past a ShiftMap, so a merged product narrowed to a ShiftMap too
    // early strands the deg2rad MatrixMap between the CD matrix and the
    // WcsMap. The matrix values below are the C oracle's.
    use ast_core::mappings::{
        CmpMap, Composition, DynMapping, Mapping, MatrixMap, UnitMap, WcsMap, WcsProjection, WinMap,
    };
    use ndarray::array;

    let crpix = [75.910_204_f64, 60.359_184];
    let win = WinMap::new(&[-crpix[0], -crpix[1]], &[1.0, 1.0]).unwrap();
    let cd = MatrixMap::new(array![
        [-0.001_831_773_954_f64, -8.541_702_226e-4],
        [0.000_854_170_222_6, -0.001_831_773_954]
    ])
    .unwrap();
    let d2r = std::f64::consts::PI / 180.0;
    let deg2rad = MatrixMap::new(array![[d2r, 0.0], [0.0, d2r]]).unwrap();
    let mut wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    wcs.invert();

    let c1 = CmpMap::new(Box::new(win), Box::new(cd), Composition::Series).unwrap();
    let c2 = CmpMap::new(Box::new(c1), Box::new(UnitMap::new(2)), Composition::Series).unwrap();
    let c3 = CmpMap::new(Box::new(c2), Box::new(deg2rad), Composition::Series).unwrap();
    let chain: DynMapping =
        Box::new(CmpMap::new(Box::new(c3), Box::new(wcs), Composition::Series).unwrap());

    let simp = chain.simplify().unwrap();
    let outer = simp
        .as_any()
        .downcast_ref::<CmpMap>()
        .expect("simplified chain is a series CmpMap");
    let text = to_native_string(&AnyObject::CmpMap(outer.clone())).unwrap();

    // Exactly three leaves, in series order: ShiftMap, MatrixMap, WcsMap. No
    // WinMap survives, and no second MatrixMap is stranded behind the
    // projection.
    let classes: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Begin "))
        .map(|l| l.split_whitespace().next().unwrap_or(""))
        .filter(|c| *c != "CmpMap")
        .collect();
    assert_eq!(classes, ["ShiftMap", "MatrixMap", "WcsMap"], "{text}");

    // CRPIX survives as the leading shift; the CD matrix carries the deg2rad
    // scaling, so its off-diagonals stay non-zero and the form stays Full.
    for token in [
        "Sft1 = -75.910203999999993",
        "Sft2 = -60.359183999999999",
        "M0 = -3.1970486649575151e-05",
        "M1 = -1.4908082756862879e-05",
        "M2 = 1.4908082756862879e-05",
        "M3 = -3.1970486649575151e-05",
        "Form = \"Full\"",
    ] {
        assert!(text.contains(token), "expected {token} in:\n{text}");
    }
}

#[test]
fn nonuniform_cdelt_chain_keeps_deg2rad_in_the_winmap() {
    // The FITS-WCS read path for the ZPX header
    // (`fixtures/wcsconv/inputs/zpx.head`) builds
    //
    //   WinMap(-CRPIX, unit scale) ; MatrixMap(PC) ; MatrixMap(CDELT) ;
    //   UnitMap ; MatrixMap(deg2rad, diagonal) ; WcsMap(ZPN)^-1
    //
    // where the UnitMap is the ZPX distortion correction (`DistortMaps`'
    // fourth slot, which degenerates to a UnitMap because the header carries
    // only `projp` terms and no correction surface). `SpecTrans` split the
    // header's CD matrix by row norm, so the two CDELT values differ in
    // magnitude and the CDELT matrix stays a non-uniform diagonal MatrixMap
    // rather than narrowing to a ZoomMap.
    //
    // C's normal form for that chain keeps the degrees→radians factor in a
    // WinMap's scales and leaves the fused PC·CDELT product in degrees, behind
    // the WinMap in series order
    // (`fixtures/wcsconv/expected/zpx.native:52-92`). The values below are the
    // C oracle's.
    use ast_core::mappings::{
        CmpMap, Composition, DynMapping, Mapping, MatrixMap, UnitMap, WcsMap, WcsProjection, WinMap,
    };
    use ndarray::array;

    let crpix = [-319.077_517_318_151_f64, 3_035.649_826_355_72];
    let win = WinMap::new(&[-crpix[0], -crpix[1]], &[1.0, 1.0]).unwrap();
    let pc = MatrixMap::new(array![
        [0.017_123_380_684_552_016_f64, 0.999_853_384_168_865],
        [-0.999_844_421_650_356_9, 0.017_638_948_230_076_08]
    ])
    .unwrap();
    let cdelt = MatrixMap::new(array![
        [-9.253_475_713_904_881e-5_f64, 0.0],
        [0.0, 9.255_325_538_402_174e-5]
    ])
    .unwrap();
    let d2r = std::f64::consts::PI / 180.0;
    let deg2rad = MatrixMap::new(array![[d2r, 0.0], [0.0, d2r]]).unwrap();
    let mut wcs = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    wcs.invert();

    let c1 = CmpMap::new(Box::new(win), Box::new(pc), Composition::Series).unwrap();
    let c2 = CmpMap::new(Box::new(c1), Box::new(cdelt), Composition::Series).unwrap();
    let c3 = CmpMap::new(Box::new(c2), Box::new(UnitMap::new(2)), Composition::Series).unwrap();
    let c4 = CmpMap::new(Box::new(c3), Box::new(deg2rad), Composition::Series).unwrap();
    let chain: DynMapping =
        Box::new(CmpMap::new(Box::new(c4), Box::new(wcs), Composition::Series).unwrap());

    let simp = chain.simplify().unwrap();
    let outer = simp
        .as_any()
        .downcast_ref::<CmpMap>()
        .expect("simplified chain is a series CmpMap");
    let text = to_native_string(&AnyObject::CmpMap(outer.clone())).unwrap();

    // Three leaves: the fused PC·CDELT matrix first, then the WinMap carrying
    // the CRPIX shift and the deg2rad scaling, then the projection. No
    // ShiftMap: narrowing the WinMap would strand the deg2rad factor.
    let classes: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Begin "))
        .map(|l| l.split_whitespace().next().unwrap_or(""))
        .filter(|c| *c != "CmpMap")
        .collect();
    assert_eq!(classes, ["MatrixMap", "WinMap", "WcsMap"], "{text}");

    // PC·CDELT reproduces the header's CD matrix, still in degrees, and the
    // WinMap holds CD·(-CRPIX)·d2r as its shifts.
    for token in [
        "M0 = -1.5845078730445003e-06",
        "M1 = -9.2521190078721999e-05",
        "M2 = -9.2538856101294999e-05",
        "M3 = 1.6325420802437699e-06",
        "Form = \"Full\"",
        "Sft1 = 0.0048931414519699694",
        "Scl1 = 0.017453292519943295",
        "Sft2 = -0.00060184004538763232",
        "Scl2 = 0.017453292519943295",
    ] {
        assert!(text.contains(token), "expected {token} in:\n{text}");
    }
}
