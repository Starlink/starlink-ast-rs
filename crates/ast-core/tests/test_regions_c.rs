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

/// `checkPointList`: a PointList of two spectral positions with a Box
/// uncertainty overlaps itself (5) and its negation (6), contains only its
/// points, maps through y=x² to a Region holding 1.21 but not 1.5, and masks
/// two pixels through a WinMap.
#[test]
fn check_point_list() {
    use ast_core::AnyObject;
    use ast_core::frames::{SpecFrame, SpecSystem};
    use ast_core::mappings::MathMap;
    use ast_core::regions::{BoxRegion, PointListBuilder};

    let frm = SpecFrame::new(SpecSystem::Wavelen);
    let unc: BoxRegion = BoxRegionBuilder::new(frm.clone(), vec![0.0], vec![1.0e-5])
        .unwrap()
        .build()
        .unwrap();
    let reg = PointListBuilder::new(frm, vec![vec![1.0], vec![1.1]])
        .unwrap()
        .uncertainty(Box::new(unc))
        .build()
        .unwrap();
    check_dump(&reg, &AnyObject::PointList(reg.clone()), "checkdump reg");
    assert_eq!(
        overlap(&reg, &reg),
        5,
        "PointList: self is not identical with self"
    );
    let mut reg2 = reg.clone();
    reg2.negate();
    check_dump(&reg2, &AnyObject::PointList(reg2.clone()), "checkdump reg2");
    assert_eq!(
        overlap(&reg, &reg2),
        6,
        "PointList: overlap with self-exclusion"
    );

    let tran1 = |m: &dyn Mapping, xs: &[f64]| -> Vec<f64> {
        let rows: Vec<[f64; 1]> = xs.iter().map(|&x| [x]).collect();
        let out = m.transform(&PointSet::from_rows(rows), true).unwrap();
        (0..xs.len()).map(|i| out.point(i)[0]).collect()
    };
    assert_eq!(
        tran1(&reg, &[1.0, 1.05, 1.1]),
        [1.0, BAD_VALUE, 1.1],
        "PointList: Errors 1-3"
    );
    assert_eq!(
        tran1(&reg2, &[1.0, 1.05, 1.1]),
        [BAD_VALUE, 1.05, BAD_VALUE],
        "PointList: Errors 4-6"
    );

    let mm = MathMap::new(1, 1, &["y=x**2"], &["x=y**0.5"]).unwrap();
    let reg3 = reg
        .map_region(&mm, &BasicFrameBuilder::new(1).build().unwrap())
        .unwrap();
    let reg3: &dyn Mapping = reg3.as_ref();
    let reg4 = reg3.simplify().unwrap();
    let any = AnyObject::from_mapping(reg4.as_ref()).unwrap();
    check_dump(any.as_region().unwrap(), &any, "checkdump reg4");
    assert_eq!(
        tran1(reg4.as_ref(), &[1.21, 1.5]),
        [1.21, BAD_VALUE],
        "PointList: Errors 7-8"
    );

    let map = WinMap::from_corners(&[1.01], &[1.11], &[2.0], &[7.0]).unwrap();
    let (lbnd, ubnd) = ([-1_i64], [15_i64]);
    let mut mdata = vec![0_i32; 17];
    let nbad = mask(&reg, Some(&map), true, &lbnd, &ubnd, &mut mdata, 2).unwrap();
    assert_eq!(nbad, 2, "nbad");
    let at = |p: i64| mdata[(p - lbnd[0]) as usize];
    for (p, want) in [(1, 0), (2, 2), (3, 0), (6, 0), (7, 2), (8, 0)] {
        assert_eq!(at(p), want, "mdata({p})");
    }
}

/// `checkPrism`: an FK5 Box extruded through a wavelength Interval. It
/// simplifies to a Box with no FrameSet; its overlaps with other Prisms, a
/// negated copy, open Prisms and half-unbounded Intervals in other systems;
/// and the bounds of a Prism over the south pole.
#[test]
fn check_prism() {
    use ast_core::AnyObject;
    use ast_core::frames::{CmpFrame, Frame, SpecFrame, SpecSystem};
    use ast_core::regions::{BoxRegion, IntervalBuilder, Prism, PrismBuilder};

    let fk5 = || SkyFrame::new(SkySystem::Fk5);
    let angstrom = |system: Option<SpecSystem>| {
        let mut f = match system {
            Some(s) => SpecFrame::new(s),
            None => SpecFrame::unset_system(),
        };
        f.set_unit(1, "Angstrom").unwrap();
        f
    };
    let sky_box = |c: [f64; 2], corner: [f64; 2]| -> BoxRegion {
        BoxRegionBuilder::new(fk5(), c.to_vec(), corner.to_vec())
            .unwrap()
            .build()
            .unwrap()
    };
    let interval = |f: &SpecFrame, lo: f64, hi: f64| {
        IntervalBuilder::new(f.clone(), vec![lo], vec![hi])
            .unwrap()
            .build()
            .unwrap()
    };
    let prism = |a: &BoxRegion, b: ast_core::regions::Interval| -> Prism {
        PrismBuilder::new(a.clone(), b).unwrap().build().unwrap()
    };
    let as_mapping =
        |p: &Prism| -> ast_core::mappings::DynMapping { (p as &dyn Mapping).simplify().unwrap() };
    let set_component_system =
        |p: &mut Prism, first: bool, sky: Option<SkySystem>, spec: Option<SpecSystem>| {
            p.with_integrity(|f| {
                let cmp = f
                    .as_any_mut()
                    .downcast_mut::<CmpFrame>()
                    .expect("a CmpFrame");
                if first {
                    cmp.frame1_mut()
                        .as_any_mut()
                        .downcast_mut::<SkyFrame>()
                        .unwrap()
                        .set_system(sky.unwrap());
                } else {
                    cmp.frame2_mut()
                        .as_any_mut()
                        .downcast_mut::<SpecFrame>()
                        .unwrap()
                        .set_system(spec.unwrap())?;
                }
                Ok(())
            })
            .unwrap();
        };

    let f2 = angstrom(None);
    let r1 = sky_box([0.0, 0.0], [1.0e-4, 1.0e-4]);
    let r3 = prism(&r1, interval(&f2, 5000.0, 6000.0));
    check_dump(&r3, &AnyObject::Prism(r3.clone()), "checkdump Prism 1");
    assert_eq!(overlap(&r3, &r3), 5, "Prism 1");
    let r4 = as_mapping(&r3);
    assert!(r4.as_ref().as_any().is::<BoxRegion>(), "Prism 1b");
    let r4any = AnyObject::from_mapping(r4.as_ref()).unwrap();
    assert!(!has_frameset(&r4any), "Prism 1c");
    assert_eq!(overlap(&r3, r4any.as_region().unwrap()), 5, "Prism 1d");

    let r4 = prism(&r1, interval(&f2, 5500.0, 5800.0));
    assert_eq!(overlap(&r3, &r4), 3, "Prism 2");
    assert_eq!(overlap(&r4, &r3), 2, "Prism 3");
    let r4 = prism(&r1, interval(&f2, 5500.0, 6500.0));
    assert_eq!(overlap(&r3, &r4), 4, "Prism 4");
    assert_eq!(overlap(&r4, &r3), 4, "Prism 5");
    let r4 = prism(&r1, interval(&f2, 6500.0, 7500.0));
    assert_eq!(overlap(&r3, &r4), 1, "Prism 6");
    assert_eq!(overlap(&r4, &r3), 1, "Prism 7");
    let mut r4 = r3.clone();
    r4.negate();
    assert_eq!(overlap(&r4, &r3), 6, "Prism 8");

    let r1b = sky_box([2.0e-4, 2.0e-4], [1.1e-4, 1.0e-4]);
    let r4 = prism(&r1b, interval(&f2, 5000.0, 6000.0));
    assert_eq!(overlap(&r3, &r4), 1, "Prism 9");
    let r1c = sky_box([2.0e-4, 2.0e-4], [1.0e-4, 1.0e-4]);
    let mut r4 = prism(&r1c, interval(&f2, 5000.0, 6000.0));
    assert_eq!(overlap(&r3, &r4), 4, "Prism 10");
    let mut r3o = r3.clone();
    r3o.set_closed(false);
    r4.set_closed(false);
    assert_eq!(overlap(&r3o, &r4), 1, "Prism 11");

    let f2 = angstrom(Some(SpecSystem::Wavelen));
    let r1 = sky_box([0.0, 0.0], [1.0e-4, 1.0e-4]);
    let mut r3 = prism(&r1, interval(&f2, 5000.0, BAD_VALUE));
    let r4 = prism(&r1, interval(&f2, 6000.0, BAD_VALUE));
    set_component_system(&mut r3, true, Some(SkySystem::Galactic), None);
    assert_eq!(overlap(&r3, &r4), 3, "Prism 12");
    let r4 = prism(&r1, interval(&f2, BAD_VALUE, 6000.0));
    assert_eq!(overlap(&r3, &r4), 4, "Prism 13");
    let mut r4 = prism(&r1, interval(&f2, BAD_VALUE, 5000.0));
    set_component_system(&mut r4, false, None, Some(SpecSystem::Freq));
    assert_eq!(overlap(&r3, &r4), 4, "Prism 14");
    r4.set_closed(false);
    assert_eq!(overlap(&r3, &r4), 1, "Prism 15");

    let r1 = sky_box([0.0, -1.57], [0.8, -1.5]);
    let r3 = prism(&r1, interval(&angstrom(None), 5000.0, 6000.0));
    let r4 = as_mapping(&r3);
    let r4any = AnyObject::from_mapping(r4.as_ref()).unwrap();
    let (lb, ub) = r4any.as_region().unwrap().get_region_bounds().unwrap();
    let near = |a: f64, b: f64, t: f64| (a - b).abs() <= t;
    assert!(near(lb[0], -0.8, 1e-6), "Prism 16: {lb:?}");
    assert!(near(lb[1], -1.64, 1e-6), "Prism 17: {lb:?}");
    assert!(near(ub[0], 0.8, 1e-6), "Prism 18: {ub:?}");
    assert!(near(ub[1], -1.5, 1e-6), "Prism 19: {ub:?}");
    assert!(near(lb[2], 5000.0, 1e-10), "Prism 20: {lb:?}");
    assert!(near(ub[2], 6000.0, 1e-6), "Prism 21: {ub:?}");
}

/// `checkCmpRegion`: an FK5 Box combined with a Galactic Circle under AND
/// and OR, with each negated in turn; the OR's bounding disc; converting
/// and mapping one CmpRegion onto its negation (overlap 6); A OR ¬A and
/// A AND ¬A simplifying to a negated and a plain NullRegion; and a Box with
/// two Circles cut out of it. C negates Regions after composing them and the
/// later CmpRegions see the change through shared pointers; each CmpRegion
/// here is built from its components in the state C's have at that point.
#[test]
fn check_cmp_region() {
    use ast_core::AnyObject;
    use ast_core::frames::{Frame, FrameRef};
    use ast_core::regions::{
        BoxRegion, Circle, CircleBuilder, CmpOperator, CmpRegion, CmpRegionBuilder, NullRegion,
    };

    let cmp = |a: &dyn Region, b: &dyn Region, op: CmpOperator| -> CmpRegion {
        CmpRegionBuilder::from_dyn(a.clone_region(), b.clone_region(), op)
            .unwrap()
            .build()
            .unwrap()
    };
    let tran2 = |m: &dyn Mapping, xs: &[f64; 4], ys: &[f64; 4]| -> Vec<[f64; 2]> {
        let rows: Vec<[f64; 2]> = (0..4).map(|i| [xs[i], ys[i]]).collect();
        let out = m.transform(&PointSet::from_rows(rows), true).unwrap();
        (0..4).map(|i| [out.point(i)[0], out.point(i)[1]]).collect()
    };
    // `inside[i]` says whether C expects point i to pass unchanged.
    let expect =
        |label: &str, out: &[[f64; 2]], xs: &[f64; 4], ys: &[f64; 4], inside: [bool; 4]| {
            for i in 0..4 {
                let want = if inside[i] {
                    [xs[i], ys[i]]
                } else {
                    [BAD_VALUE, BAD_VALUE]
                };
                assert_eq!(out[i], want, "CmpRegion: {label} Error {}", i + 1);
            }
        };

    let mut r1: BoxRegion = BoxRegionBuilder::new(
        SkyFrame::new(SkySystem::Fk5),
        vec![0.0, 0.0],
        vec![1.0e-4, 1.0e-4],
    )
    .unwrap()
    .build()
    .unwrap();
    let mut r2: Circle = CircleBuilder::new(
        SkyFrame::new(SkySystem::Galactic),
        vec![1.68166715892457, -1.050436507472],
    )
    .unwrap()
    .edge_point(vec![1.68140254777194, -1.05048840003467])
    .build()
    .unwrap();
    assert_eq!(overlap(&r1, &r2), 4, "CmpRegion: Error 0");
    let cr = cmp(&r1, &r2, CmpOperator::And).clone();
    assert_eq!(overlap(&cr, &cr), 5, "CmpRegion: Error 1");

    let xin = [0.5e-4, 1.5e-4, -0.5e-4, 1.1e-4];
    let yin = [0.5e-4, 1.5e-4, -0.5e-4, -1.1e-4];
    expect(
        "AND",
        &tran2(&cr, &xin, &yin),
        &xin,
        &yin,
        [true, false, false, false],
    );
    let cr = cmp(&r1, &r2, CmpOperator::Or);
    expect(
        "OR",
        &tran2(&cr, &xin, &yin),
        &xin,
        &yin,
        [true, true, true, false],
    );
    // C's 1e-7 tolerance, relaxed from the Fortran original's for its build.
    let (centre, radius) = cr.get_region_disc().unwrap();
    assert!(
        (centre[0] - 5.3625e-5).abs() <= 1.0e-7,
        "CmpRegion: Disc error 1: {centre:?}"
    );
    assert!(
        (centre[1] - 5.1030e-5).abs() <= 1.0e-7,
        "CmpRegion: Disc error 2: {centre:?}"
    );
    assert!(
        (radius - 2.1543e-4).abs() <= 1.0e-7,
        "CmpRegion: Disc error 3: {radius}"
    );

    r2.negate();
    let cr = cmp(&r1, &r2, CmpOperator::And);
    expect(
        "ANDb",
        &tran2(&cr, &xin, &yin),
        &xin,
        &yin,
        [false, false, true, false],
    );
    r1.negate();
    let cr = cmp(&r1, &r2, CmpOperator::And);
    expect(
        "ANDc",
        &tran2(&cr, &xin, &yin),
        &xin,
        &yin,
        [false, false, false, true],
    );
    let mut cr = cmp(&r1, &r2, CmpOperator::And);
    cr.negate();
    expect(
        "ANDd",
        &tran2(&cr, &xin, &yin),
        &xin,
        &yin,
        [true, true, true, false],
    );

    let cr2 = cmp(&r2, &r1, CmpOperator::And);
    let fs = cr
        .convert_to(&cr2, &[])
        .unwrap()
        .expect("CmpRegion: Error 5");
    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let frm = fs.current().snapshot();
    let cr3 = cr.map_region(map.as_ref(), frm.as_ref()).unwrap();
    assert_eq!(overlap(cr3.as_ref(), &cr2), 6, "CmpRegion: Error 6");
    check_dump(
        &cr,
        &AnyObject::CmpRegion(cr.clone()),
        "checkdump CmpRegion: cr",
    );

    let mut cr2 = cr.clone();
    cr2.negate();
    let simp = |a: &CmpRegion, b: &CmpRegion, op| -> ast_core::mappings::DynMapping {
        let c = cmp(a, b, op);
        (&c as &dyn Mapping).simplify().unwrap()
    };
    let cr3 = simp(&cr2, &cr, CmpOperator::Or);
    let nr = cr3
        .as_ref()
        .as_any()
        .downcast_ref::<NullRegion>()
        .expect("CmpRegion: Error 7");
    assert!(nr.is_negated(), "CmpRegion: Error 8");
    let cr3 = simp(&cr2, &cr, CmpOperator::And);
    let nr = cr3
        .as_ref()
        .as_any()
        .downcast_ref::<NullRegion>()
        .expect("CmpRegion: Error 9");
    assert!(!nr.is_negated(), "CmpRegion: Error 10");

    let f1 = frame2();
    let r1 = BoxRegionBuilder::new(f1.clone(), vec![0.0, 0.0], vec![1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let circ = |c: f64| {
        CircleBuilder::new(f1.clone(), vec![c, 0.0])
            .unwrap()
            .edge_point(vec![0.0, 0.0])
            .build()
            .unwrap()
    };
    let mut cr = cmp(&circ(-1.0), &circ(1.0), CmpOperator::Or);
    check_dump(
        &cr,
        &AnyObject::CmpRegion(cr.clone()),
        "checkdump CmpRegion: cr",
    );
    cr.negate();
    let cr2 = cmp(&cr, &r1, CmpOperator::And);
    check_dump(
        &cr2,
        &AnyObject::CmpRegion(cr2.clone()),
        "checkdump CmpRegion: cr2",
    );
    let cr2 = (&cr2 as &dyn Mapping).simplify().unwrap();
    let xin = [0.0, 0.2, 0.5, -0.5];
    let yin = [0.5, 1.5, 0.5, 0.5];
    expect(
        "Errors 11-18",
        &tran2(cr2.as_ref(), &xin, &yin),
        &xin,
        &yin,
        [true, false, false, false],
    );
    let any = AnyObject::from_mapping(cr2.as_ref()).unwrap();
    let mut neg = any.as_region().unwrap().clone_region();
    neg.negate();
    let neg: &dyn Mapping = neg.as_ref();
    let neg = neg.simplify().unwrap();
    expect(
        "Errors 19-26",
        &tran2(neg.as_ref(), &xin, &yin),
        &xin,
        &yin,
        [false, true, true, true],
    );
}

// The expected values are C's own literals, some of which approximate pi/2
// and 2*pi to the digits C wrote.
#[allow(clippy::approx_constant)]
/// `checkCircle`: Circles in the SkyFrame of a TAN header (bounds, disc,
/// boundary and interior meshes, membership near the edge, a change of
/// system and its FrameSet, overlaps, pole bounds), in a Frame of another
/// Domain (overlap 0), in a 4-axis CmpFrame, and 3-D spheres.
#[test]
fn check_circle() {
    use ast_core::AnyObject;
    use ast_core::channel::Channel;
    use ast_core::fitschan::FitsChan;
    use ast_core::frames::{CmpFrame, Frame};
    use ast_core::regions::{BoxRegion, Circle, CircleBuilder};

    let header = "CTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\nCRPIX1  = 100\nCRPIX2  = 100\n\
                  CRVAL1  = 70.0\nCRVAL2  = 80.0\nCDELT1  = 0.6\nCDELT2  = 0.6\n";
    let Some(AnyObject::FrameSet(fs)) = FitsChan::from_header(header).unwrap().read().unwrap()
    else {
        panic!("the header did not read as a FrameSet");
    };
    let frm1 = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .unwrap()
        .clone();
    let near = |a: f64, b: f64, t: f64| (a - b).abs() <= t;
    let radius_circle = |f: &dyn Frame, c: &[f64], r: f64, unc: Option<&BoxRegion>| -> Circle {
        let mut b = CircleBuilder::from_dyn(f.snapshot(), c.to_vec())
            .unwrap()
            .radius(r);
        if let Some(u) = unc {
            b = b.uncertainty(Box::new(u.clone()));
        }
        b.build().unwrap()
    };
    let edge_circle = |f: &dyn Frame, c: &[f64], e: &[f64], unc: Option<&BoxRegion>| -> Circle {
        let mut b = CircleBuilder::from_dyn(f.snapshot(), c.to_vec())
            .unwrap()
            .edge_point(e.to_vec());
        if let Some(u) = unc {
            b = b.uncertainty(Box::new(u.clone()));
        }
        b.build().unwrap()
    };
    let tran = |m: &dyn Mapping, pts: Vec<Vec<f64>>| -> Vec<Vec<f64>> {
        let n = pts.len();
        let out = m.transform(&PointSet::from_rows(pts), true).unwrap();
        (0..n).map(|i| out.point(i).to_vec()).collect()
    };
    // Points just inside and just outside along the direction to `toward`.
    let probe = |f: &dyn Frame, c: &[f64], toward: &[f64], rad: f64| -> Vec<Vec<f64>> {
        vec![
            f.offset(c, toward, rad * 0.999).unwrap(),
            f.offset(c, toward, rad * 1.001).unwrap(),
        ]
    };
    let check_probe = |m: &dyn Mapping, pts: Vec<Vec<f64>>, label: &str| {
        let out = tran(m, pts.clone());
        assert_eq!(out[0], pts[0], "Circle: {label} inside");
        assert!(
            out[1].iter().all(|&v| v == BAD_VALUE),
            "Circle: {label} outside"
        );
    };

    let cir1 = radius_circle(&frm1, &[0.0, 1.0], 0.01, None);
    let (lb, ub) = cir1.get_region_bounds().unwrap();
    assert!(near(lb[0], -0.01850666061475259, 1e-6), "Circle: Error AA1");
    assert!(near(lb[1], 0.9900000002235173, 1e-6), "Circle: Error AA2");
    assert!(near(ub[0], 0.01850666061475276, 1e-6), "Circle: Error AA3");
    assert!(near(ub[1], 1.009994987166073, 1e-6), "Circle: Error AA4");
    let (centre, radius) = cir1.get_region_disc().unwrap();
    assert!(
        near(centre[0], 0.0, 1e-6) && near(centre[1], 1.0, 1e-6),
        "Circle: Error AA5-6"
    );
    assert!(near(radius, 0.01, 1e-6), "Circle: Error AA7");

    let p1 = [0.0, 1.57];
    let cir1 = radius_circle(&frm1, &p1, 0.01, None);
    let (lb, ub) = cir1.get_region_bounds().unwrap();
    assert!(near(lb[0], 0.0, 1e-6), "Circle: Error AA5");
    assert!(near(lb[1], 1.560000052675599, 1e-6), "Circle: Error AA6");
    assert!(near(ub[0], 6.283185307179586, 1e-6), "Circle: Error AA7");
    assert!(near(ub[1], 1.5707963267948966, 1e-6), "Circle: Error AA8");
    let mesh = cir1.region_mesh().unwrap();
    assert_eq!(mesh.npoint(), 200, "Circle: Error mesh 1");
    for i in 0..mesh.npoint() {
        let d = frm1.distance(&p1, mesh.point(i)).unwrap();
        assert!(near(d, 0.01, 1e-6), "Circle: Error mesh 2: {d}");
    }
    let grid = cir1.region_grid().unwrap();
    assert_eq!(grid.npoint(), 201, "Circle: Error mesh 3");
    for i in 0..grid.npoint() {
        assert!(
            frm1.distance(&p1, grid.point(i)).unwrap() <= 0.01,
            "Circle: Error mesh 4"
        );
    }

    let p1 = [1.2217305, 1.3962634];
    let mut cir1 = edge_circle(&frm1, &p1, &[0.8, 0.8], None);
    check_dump(&cir1, &AnyObject::Circle(cir1.clone()), "checkdump cir1");
    let rad = Frame::distance(&cir1, &p1, &[0.8, 0.8]).unwrap();
    check_probe(&cir1, probe(&frm1, &p1, &[0.8, 0.8], rad), "Errors 1-4");
    let out = tran(&cir1, vec![vec![0.0, 1.5707963]]);
    assert_eq!(out[0], [0.0, 1.5707963], "Circle: Error 1b-2b");
    check_probe(&cir1, probe(&frm1, &p1, &[0.0, 0.0], rad), "Errors 5-8");
    cir1.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SkyFrame>()
            .unwrap()
            .set_system(SkySystem::Galactic);
        Ok(())
    })
    .unwrap();
    let simp = (&cir1 as &dyn Mapping).simplify().unwrap();
    assert!(
        has_frameset(&AnyObject::from_mapping(simp.as_ref()).unwrap()),
        "Circle: error 9"
    );

    let unc = BoxRegionBuilder::new(frm1.clone(), vec![0.0, 0.0], vec![1.0e-6, 1.0e-6])
        .unwrap()
        .build()
        .unwrap();
    let e = [1.2218, 1.3963];
    let cir1 = edge_circle(&frm1, &p1, &e, Some(&unc));
    let rad = Frame::distance(&cir1, &p1, &e).unwrap();
    check_probe(&cir1, probe(&frm1, &p1, &e, rad), "Errors 1b-4b");
    check_probe(&cir1, probe(&frm1, &p1, &[0.0, 0.0], rad), "Errors 5b-8b");
    let mut cir2 = cir1.clone();
    cir2.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SkyFrame>()
            .unwrap()
            .set_system(SkySystem::Galactic);
        Ok(())
    })
    .unwrap();
    check_dump(&cir2, &AnyObject::Circle(cir2.clone()), "checkdump cir2");
    let cir2 = (&cir2 as &dyn Mapping).simplify().unwrap();
    let cir2 = AnyObject::from_mapping(cir2.as_ref()).unwrap();
    assert!(!has_frameset(&cir2), "Circle: error 9b");
    let cir2 = cir2.as_region().unwrap();
    assert_eq!(overlap(&cir1, cir2), 5, "Circle: Error 10");
    assert_eq!(overlap(cir2, &cir1), 5, "Circle: Error 11");

    let cir2 = edge_circle(&frm1, &[1.2217305, 1.3964], &e, Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 4, "Circle: Error 12");
    assert_eq!(overlap(&cir2, &cir1), 4, "Circle: Error 13");
    let cir2 = edge_circle(&frm1, &p1, &[1.221731, 1.396268], Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 3, "Circle: Error 14");
    assert_eq!(overlap(&cir2, &cir1), 2, "Circle: Error 15");
    let cir2 = edge_circle(&frm1, &[0.8, 1.0], &[0.88, 1.05], Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 1, "Circle: Error 16");
    let cir2 = radius_circle(&frm1, &[0.8, 1.5707963], 0.1, Some(&unc));
    let (lb, ub) = cir2.get_region_bounds().unwrap();
    assert_eq!(lb[0], 0.0, "Circle: Error 16a");
    assert!(near(lb[1], 1.47079625, 1e-6), "Circle: Error 16b");
    assert!(near(ub[0], 6.28318531, 1e-6), "Circle: Error 16c");
    assert!(near(ub[1], 1.57079633, 1e-6), "Circle: Error 16d");

    let mut aa = frame2();
    Frame::set_domain(&mut aa, "aa").unwrap();
    let unc_aa = BoxRegionBuilder::new(aa.clone(), vec![0.0, 0.0], vec![1.0e-6, 1.0e-6])
        .unwrap()
        .build()
        .unwrap();
    let cir2 = edge_circle(&aa, &p1, &e, Some(&unc_aa));
    assert_eq!(overlap(&cir1, &cir2), 0, "Circle: Error 17");
    assert_eq!(overlap(&cir2, &cir1), 0, "Circle: Error 18");
    let f3 = CmpFrame::new(Box::new(SkyFrame::new(SkySystem::Icrs)), Box::new(frame2()));
    let cir2 = edge_circle(&f3, &[1.0, 1.0, 3.0, 3.0], &[1.01, 1.02, 3.01, 3.01], None);
    assert_eq!(overlap(&cir2, &cir2), 5, "Circle: Error 18b");

    // 3-D spheres.
    let f = BasicFrameBuilder::new(3).build().unwrap();
    let unc = BoxRegionBuilder::new(f.clone(), vec![0.0; 3], vec![1.0e-6, 2.0e-6, 2.0e-6])
        .unwrap()
        .build()
        .unwrap();
    let c = [1.0, 2.0, 3.0];
    let cir1 = edge_circle(&f, &c, &[0.0, -1.0, -2.0], Some(&unc));
    check_dump(&cir1, &AnyObject::Circle(cir1.clone()), "checkdump sph1");
    let rad = Frame::distance(&cir1, &c, &[0.0, -1.0, -2.0]).unwrap();
    check_probe(
        &cir1,
        probe(&f, &c, &[0.0, -1.0, -2.0], rad),
        "Sphere: Errors 1-4z",
    );
    check_probe(
        &cir1,
        probe(&f, &c, &[0.0, 0.0, 0.0], rad),
        "Sphere: Errors 5-8z",
    );
    assert_eq!(overlap(&cir1, &cir1), 5, "Sphere: Error 10");
    let cir2 = edge_circle(&f, &c, &[0.5, 0.0, -1.0], Some(&unc));
    check_dump(&cir2, &AnyObject::Circle(cir2.clone()), "checkdump sph2");
    assert_eq!(overlap(&cir2, &cir1), 2, "Sphere: Error 11");
    assert_eq!(overlap(&cir1, &cir2), 3, "Sphere: Error 12");
    let cir2 = edge_circle(&f, &[1.0, 0.0, 3.0], &[0.0, -1.0, -2.0], Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 4, "Sphere: Error 13");
    let cir2 = edge_circle(&f, &[1.0, 102.0, 3.0], &[0.0, 99.0, -2.0], Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 1, "Sphere: Error 14");
    let mut cir1 = edge_circle(&f, &[0.0; 3], &[0.0, 0.0, 1.0], Some(&unc));
    let cir2 = edge_circle(
        &f,
        &[2.0000001, 0.0, 0.0],
        &[2.000001, 1.0, 0.0],
        Some(&unc),
    );
    assert_eq!(overlap(&cir1, &cir2), 4, "Sphere: Error 15");
    let mut cir2 = edge_circle(&f, &[2.000001, 0.0, 0.0], &[2.000001, 1.0, 0.0], Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 4, "Sphere: Error 16");
    cir1.set_closed(false);
    cir2.set_closed(false);
    assert_eq!(overlap(&cir1, &cir2), 1, "Sphere: Error 17");
    cir1.clear_closed();
    let cir2 = edge_circle(&f, &[2.000004, 0.0, 0.0], &[2.000004, 1.0, 0.0], Some(&unc));
    assert_eq!(overlap(&cir1, &cir2), 1, "Sphere: Error 18");
}
