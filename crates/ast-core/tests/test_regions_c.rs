//! `testregions.c` functions not covered elsewhere, one test per function.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testregions.c`. The functions
//! it shares with the class test files (`test_regions.rs`, `test_box_region.rs`,
//! `test_circle.rs`, ...) are covered there; the gaps are listed in
//! `docs/issues/c-test-coverage-audit.md`. Each test here follows its C
//! function's sequence, with the C error text in its assertion messages.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, SkyFrame, SkySystem};
use ast_core::mappings::{Mapping, WinMap};
use ast_core::regions::{BoxRegionBuilder, PolygonBuilder, Region, mask};

fn frame2() -> ast_core::frames::BasicFrame {
    BasicFrameBuilder::new(2).build().unwrap()
}

fn tran_point(m: &dyn Mapping, p: [f64; 2]) -> [f64; 2] {
    let out = m.transform(&PointSet::from_rows([p]), true).unwrap();
    [out.point(0)[0], out.point(0)[1]]
}

/// `checkBoxTransformAfterBounds`: a zero-width Box contains a point just off
/// its degenerate axis whether its bounds are asked for before or after the
/// transform, and the bounds stay at the degenerate value.
#[test]
fn check_box_transform_after_bounds() {
    let (l, u) = ([1.0, 5.0], [2.0, 5.0]);
    let p = [1.5, 5.0 + 1.0e-6];

    let a = BoxRegionBuilder::from_bounds(frame2(), &l, &u)
        .unwrap()
        .build()
        .unwrap();
    assert_ne!(
        tran_point(&a, p)[0],
        BAD_VALUE,
        "Box transform after bounds 1"
    );
    let (lb, ub) = a.get_region_bounds().unwrap();
    assert!(
        lb[1] == 5.0 && ub[1] == 5.0,
        "Box transform after bounds 2: {lb:?} {ub:?}"
    );

    let b = BoxRegionBuilder::from_bounds(frame2(), &l, &u)
        .unwrap()
        .build()
        .unwrap();
    let _ = b.get_region_bounds().unwrap();
    assert_ne!(
        tran_point(&b, p)[0],
        BAD_VALUE,
        "Box transform after bounds 3"
    );
}

/// `checkPolygonMaskLargeLobe`: an ICRS Polygon wound so that its interior is
/// the large lobe masks every pixel of a grid far from the small square its
/// vertices outline.
#[test]
fn check_polygon_mask_large_lobe() {
    let (ra0, dec0, half) = (0.5, 0.0, 0.001);
    let (fra, fdec) = (ra0 + 0.1, dec0);
    let g = 20_i64;
    let span = 0.002;

    let vertices = vec![
        vec![ra0 - half, dec0 - half],
        vec![ra0 + half, dec0 - half],
        vec![ra0 + half, dec0 + half],
        vec![ra0 - half, dec0 + half],
    ];
    let poly = PolygonBuilder::new(SkyFrame::new(SkySystem::Icrs), vertices)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        tran_point(&poly, [ra0, dec0])[0],
        BAD_VALUE,
        "setup: square centre is not outside the region"
    );
    assert_ne!(
        tran_point(&poly, [fra, fdec])[0],
        BAD_VALUE,
        "setup: far point is not inside the large lobe"
    );

    let sky2pix = WinMap::from_corners(
        &[fra - span, fdec - span],
        &[fra + span, fdec + span],
        &[0.0, 0.0],
        &[g as f64, g as f64],
    )
    .unwrap();
    let npix = ((g + 1) * (g + 1)) as usize;
    let mut data = vec![0_i32; npix];
    mask(&poly, Some(&sky2pix), true, &[0, 0], &[g, g], &mut data, 1).unwrap();
    let nmasked = data.iter().filter(|&&v| v != 0).count();
    assert_eq!(
        nmasked, npix,
        "astMaskI masked {nmasked} of {npix} in-region pixels"
    );
}

/// `checkRemoveRegions`: a Circle clips points outside it; a FrameSet that
/// `convert_to` makes from a Galactic Frame to the ICRS Circle clips them
/// too, through the Region in its Mapping; `remove_regions` drops the
/// clipping; and a TranMap of two Regions becomes a UnitMap.
#[test]
fn check_remove_regions() {
    use ast_core::frames::{Frame, FrameRef, FrameSet};
    use ast_core::mappings::{TranMap, UnitMap};
    use ast_core::regions::{CircleBuilder, remove_regions};

    let tran2 = |m: &dyn Mapping, pts: &[[f64; 2]; 2]| -> [[f64; 2]; 2] {
        let out = m.transform(&PointSet::from_rows(*pts), true).unwrap();
        [
            [out.point(0)[0], out.point(0)[1]],
            [out.point(1)[0], out.point(1)[1]],
        ]
    };
    let near =
        |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() <= 1.0e-10 && (a[1] - b[1]).abs() <= 1.0e-10;

    let sf1 = SkyFrame::new(SkySystem::Icrs);
    let reg = CircleBuilder::new(sf1.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(0.001)
        .build()
        .unwrap();
    let iin = [[0.0, 0.0], [0.01, 0.01]];

    let out = tran2(&reg, &iin);
    assert!(
        out[0][0] != BAD_VALUE && out[0][1] != BAD_VALUE,
        "RemoveRegions test 1 failed"
    );
    assert!(near(out[0], iin[0]), "RemoveRegions test 2 failed");
    assert_eq!(
        out[1],
        [BAD_VALUE, BAD_VALUE],
        "RemoveRegions test 3 failed"
    );

    let sf2 = SkyFrame::new(SkySystem::Galactic);
    let fs = sf1
        .convert_to(&sf2, &[])
        .unwrap()
        .expect("ICRS converts to Galactic");
    let gin = tran2(&fs, &iin);
    let mut fs2 = FrameSet::new(Box::new(sf2.clone()));
    fs2.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(sf2))
        .unwrap();
    let fs = fs2
        .convert_to(&reg, &[])
        .unwrap()
        .expect("the FrameSet converts to the Circle");
    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let out = tran2(map.as_ref(), &gin);
    assert!(
        out[0][0] != BAD_VALUE && out[0][1] != BAD_VALUE,
        "RemoveRegions test 4 failed"
    );
    assert!(near(out[0], iin[0]), "RemoveRegions test 5 failed");
    assert_eq!(
        out[1],
        [BAD_VALUE, BAD_VALUE],
        "RemoveRegions test 6 failed"
    );

    let fs2 = remove_regions(&fs);
    let out = tran2(fs2.as_ref(), &gin);
    assert!(
        out[0][0] != BAD_VALUE && out[0][1] != BAD_VALUE,
        "RemoveRegions test 7 failed"
    );
    assert!(near(out[0], iin[0]), "RemoveRegions test 8 failed");
    assert!(near(out[1], iin[1]), "RemoveRegions test 9 failed");

    let tm = TranMap::new(Box::new(reg.clone()), Box::new(reg)).unwrap();
    let map = remove_regions(&tm);
    assert!(
        map.as_ref().as_any().is::<UnitMap>(),
        "RemoveRegions test 10 failed"
    );
    assert_eq!(map.nin(), 2, "RemoveRegions test 11 failed");
}

/// C's `Overlap` code for a Rust [`ast_core::regions::Overlap`].
fn overlap_code(o: ast_core::regions::Overlap) -> i32 {
    use ast_core::regions::Overlap::*;
    match o {
        Incomparable => 0,
        None => 1,
        FirstInside => 2,
        SecondInside => 3,
        Partial => 4,
        Same => 5,
        Negation => 6,
    }
}

fn overlap(a: &dyn Region, b: &dyn Region) -> i32 {
    overlap_code(a.overlap(b).unwrap())
}

/// C's `hasframeset`: whether the Region's dump holds a FrameSet before its
/// uncertainty, i.e. whether its own Frame needs one.
fn has_frameset(reg: &ast_core::AnyObject) -> bool {
    let text = ast_core::to_native_string(reg).unwrap();
    for line in text.lines() {
        if line.contains("Unc =") {
            return false;
        }
        if line.contains("FrameSet") {
            return true;
        }
    }
    false
}

/// C's `checkdump`: the dump reads back to a Region that dumps the same, or
/// failing that has the same boundary (overlap 5).
fn check_dump(reg: &dyn Region, any: &ast_core::AnyObject, text: &str) {
    let s1 = ast_core::to_native_string(any).unwrap();
    let back = ast_core::from_native_string(&s1).unwrap_or_else(|e| panic!("{text}: {e}"));
    let s2 = ast_core::to_native_string(&back).unwrap();
    if s1 != s2 {
        let reg2 = back
            .as_region()
            .unwrap_or_else(|| panic!("{text}: not a Region"));
        assert_eq!(overlap(reg, reg2), 5, "{text}");
    }
}

/// `checkNullRegion`: a NullRegion over a 4-axis CmpFrame and a Circle in it;
/// their overlaps before and after negation; simplifying through a change of
/// sky system leaves no FrameSet; and masking with a negated and a plain
/// NullRegion.
#[test]
fn check_null_region() {
    use ast_core::AnyObject;
    use ast_core::frames::CmpFrame;
    use ast_core::regions::{CircleBuilder, NullRegion, NullRegionBuilder};

    let f1 = SkyFrame::new(SkySystem::Icrs);
    let f2 = frame2();
    let f3 = CmpFrame::new(Box::new(f1), Box::new(f2.clone()));
    let mut nr = NullRegionBuilder::new(f3).build().unwrap();
    check_dump(
        &nr,
        &AnyObject::NullRegion(nr.clone()),
        "checkdump NullRegion:nr",
    );

    let cir = CircleBuilder::new(nr.clone(), vec![1.0, 1.0, 3.0, 3.0])
        .unwrap()
        .edge_point(vec![1.01, 1.02, 3.01, 3.01])
        .build()
        .unwrap();
    check_dump(
        &cir,
        &AnyObject::Circle(cir.clone()),
        "checkdump NullRegion:cir",
    );
    assert_eq!(overlap(&cir, &nr), 1, "NullRegion: Error 1");
    assert_eq!(overlap(&nr, &cir), 1, "NullRegion: Error 2");
    assert_eq!(overlap(&nr, &nr), 5, "NullRegion: Error 3");
    nr.negate();
    assert_eq!(overlap(&cir, &nr), 2, "NullRegion: Error 4");
    assert_eq!(overlap(&nr, &cir), 3, "NullRegion: Error 5");
    assert_eq!(overlap(&nr, &nr), 5, "NullRegion: Error 6");

    let set_sky = |nr: &mut NullRegion, sys: SkySystem| {
        nr.with_integrity(|f| {
            let cmp = f
                .as_any_mut()
                .downcast_mut::<CmpFrame>()
                .expect("a CmpFrame");
            cmp.frame1_mut()
                .as_any_mut()
                .downcast_mut::<SkyFrame>()
                .expect("a SkyFrame")
                .set_system(sys);
            Ok(())
        })
        .unwrap();
    };
    set_sky(&mut nr, SkySystem::Fk4);
    let mut nr2 = (&nr as &dyn Mapping).simplify().unwrap();
    let nr2r = nr2
        .as_mut()
        .as_any_mut()
        .downcast_mut::<NullRegion>()
        .expect("a NullRegion simplifies to a NullRegion");
    set_sky(nr2r, SkySystem::Icrs);
    let nr3 = nr2.simplify().unwrap();
    let any = AnyObject::from_mapping(nr3.as_ref()).unwrap();
    assert!(!has_frameset(&any), "NullRegion: error 7");

    let (lbnd, ubnd) = ([1_i64, 1], [5_i64, 5]);
    let mut rin = vec![1.0_f64; 25];
    let mut nr = NullRegionBuilder::new(f2).build().unwrap();
    nr.negate();
    let res = mask(&nr, None, false, &lbnd, &ubnd, &mut rin, BAD_VALUE).unwrap();
    assert_eq!(res, 0, "res should be 0");
    assert!(rin.iter().all(|&v| v == 1.0), "Above value should be 1.0");
    nr.negate();
    let res = mask(&nr, None, false, &lbnd, &ubnd, &mut rin, BAD_VALUE).unwrap();
    assert_eq!(res, 25, "res should be 25");
    assert!(
        rin.iter().all(|&v| v == BAD_VALUE),
        "Above value should be BAD"
    );
}
