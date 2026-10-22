//! `testregions.c` functions not covered elsewhere, one test per function.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testregions.c`. The functions
//! it shares with the class test files (`test_regions.rs`, `test_box_region.rs`,
//! `test_circle.rs`, ...) are covered there; the gaps are listed in
//! `docs/issues/c-test-coverage-audit.md`. Each test here follows its C
//! function's sequence, with the C error text in its assertion messages.

// The expected values are C's own literals, some of which approximate pi/2
// and 2*pi to the digits C wrote.
#![allow(clippy::approx_constant)]

use crate::common::native_dump;

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

    let mut sf1 = SkyFrame::new(SkySystem::Icrs);
    let mut reg = CircleBuilder::new(sf1.clone(), vec![0.0, 0.0])
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

    let mut sf2 = SkyFrame::new(SkySystem::Galactic);
    let fs = sf1
        .convert_to(&mut sf2, &[])
        .unwrap()
        .expect("ICRS converts to Galactic");
    let gin = tran2(&fs, &iin);
    let mut fs2 = FrameSet::new(Box::new(sf2.clone()));
    fs2.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(sf2))
        .unwrap();
    let fs = fs2
        .convert_to(&mut reg, &[])
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
    let text = native_dump(reg).unwrap();
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
    let s1 = native_dump(any).unwrap();
    let back = ast_core::from_native_string(&s1).unwrap_or_else(|e| panic!("{text}: {e}"));
    let s2 = native_dump(&back).unwrap();
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

    let mut cr2 = cmp(&r2, &r1, CmpOperator::And);
    let fs = cr
        .convert_to(&mut cr2, &[])
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
        let d = frm1.distance(&p1, &mesh.point(i)).unwrap();
        assert!(near(d, 0.01, 1e-6), "Circle: Error mesh 2: {d}");
    }
    let grid = cir1.region_grid().unwrap();
    assert_eq!(grid.npoint(), 201, "Circle: Error mesh 3");
    for i in 0..grid.npoint() {
        assert!(
            frm1.distance(&p1, &grid.point(i)).unwrap() <= 0.01,
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

/// `checkEllipse`: an Ellipse in a permuted FK4 longitude + wavelength
/// CmpFrame; Ellipses in the SkyFrame of a TAN header at the pole (bounds,
/// membership either side of both axes, a change of system, overlaps with
/// copies shifted in pixel coordinates); mapping through a MatrixMap and
/// back; a circular Ellipse simplifying to a Circle; and overlaps across
/// RA = 0. C's form 0 (three defining points) is `EllipseBuilder::from_points`.
#[test]
fn check_ellipse() {
    use ast_core::AnyObject;
    use ast_core::channel::Channel;
    use ast_core::fitschan::FitsChan;
    use ast_core::frames::{CmpFrame, Frame, FrameRef, SpecFrame, SpecSystem, pick_axes};
    use ast_core::regions::{BoxRegion, Circle, CircleBuilder, Ellipse, EllipseBuilder};

    let near = |a: f64, b: f64, t: f64| (a - b).abs() <= t;
    let form0 = |f: &dyn Frame,
                 c: [f64; 2],
                 p: [f64; 2],
                 q: [f64; 2],
                 unc: Option<&BoxRegion>|
     -> Ellipse {
        let mut b =
            EllipseBuilder::from_dyn_points(f.snapshot(), c.to_vec(), p.to_vec(), q.to_vec())
                .unwrap();
        if let Some(u) = unc {
            b = b.uncertainty(Box::new(u.clone()));
        }
        b.build().unwrap()
    };
    let form1 = |f: &dyn Frame, c: [f64; 2], axes: [f64; 2], angle: f64| -> Ellipse {
        EllipseBuilder::from_dyn(f.snapshot(), c.to_vec(), axes, angle)
            .unwrap()
            .build()
            .unwrap()
    };
    let tran2 = |m: &dyn Mapping, pts: &[[f64; 2]], forward: bool| -> Vec<[f64; 2]> {
        let out = m
            .transform(&PointSet::from_rows(pts.to_vec()), forward)
            .unwrap();
        (0..pts.len())
            .map(|i| [out.point(i)[0], out.point(i)[1]])
            .collect()
    };
    fn unc_box<F: Frame + Clone + 'static>(f: &F) -> BoxRegion {
        BoxRegionBuilder::from_bounds(f.clone(), &[-1.0e-7, -1.0e-7], &[1.0e-7, 1.0e-7])
            .unwrap()
            .build()
            .unwrap()
    }
    let as2 = |v: Vec<f64>| [v[0], v[1]];

    // An Ellipse in a permuted CmpFrame of an FK4 longitude and a wavelength.
    let (picked, _) = pick_axes(&SkyFrame::new(SkySystem::Fk4), &[1]).unwrap();
    let mut spec = SpecFrame::new(SpecSystem::Wavelen);
    spec.set_unit(1, "um").unwrap();
    let mut f3 = CmpFrame::new(picked, Box::new(spec));
    f3.perm_axes(&[2, 1]).unwrap();
    let ell1 = form0(&f3, [0.0, 0.0], [0.001, 0.001], [-0.001, 0.001], None);
    let pts = [
        [0.0, 0.00141421],
        [0.0, 0.00141422],
        [-0.000999, 0.0009999],
        [-0.001001, 0.001001],
    ];
    let out = tran2(&ell1, &pts, true);
    assert_eq!(out[0], pts[0], "Ellipse: Cmp 1-2");
    assert_eq!(out[1], [BAD_VALUE; 2], "Ellipse: Cmp 3-4");
    assert_eq!(out[2], pts[2], "Ellipse: Cmp 5-6");
    assert_eq!(out[3], [BAD_VALUE; 2], "Ellipse: Cmp 7-8");
    check_dump(
        &ell1,
        &AnyObject::Ellipse(ell1.clone()),
        "checkdump ell1 cmp",
    );
    let ell2 = (&ell1 as &dyn Mapping).simplify().unwrap();
    let ell2 = AnyObject::from_mapping(ell2.as_ref()).unwrap();
    check_dump(ell2.as_region().unwrap(), &ell2, "checkdump ell2 cmp");
    assert_eq!(
        overlap(&ell1, ell2.as_region().unwrap()),
        5,
        "ellipse: Error 5 cmp"
    );

    // Ellipses in the SkyFrame of a TAN header at the pole.
    let header = "NAXIS1  = 300\nNAXIS2  = 300\nCTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\n\
                  CRPIX1  = 100\nCRPIX2  = 100\nCRVAL1  = 0.0\nCRVAL2  = 90.0\nCDELT1  = 0.6\nCDELT2  = 0.6\n";
    let Some(AnyObject::FrameSet(fs)) = FitsChan::from_header(header).unwrap().read().unwrap()
    else {
        panic!("the header did not read as a FrameSet");
    };
    let sky_frame = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .unwrap()
        .clone();
    let sky = fs.current().snapshot();
    let (p1, p2, p3) = (
        [1.2217305, 1.570796],
        [0.9, 1.470796],
        [2.9217305, 1.370796],
    );
    let ell1 = form0(sky.as_ref(), p1, p2, p3, None);
    check_dump(&ell1, &AnyObject::Ellipse(ell1.clone()), "checkdump ell1");
    let (lb, ub) = ell1.get_region_bounds().unwrap();
    assert!(lb[0].abs() <= 1e-10, "Error b1: {lb:?}");
    assert!(near(lb[1], 1.19059777, 1e-6), "Error b2: {lb:?}");
    assert!(near(ub[0], 6.28318531, 1e-6), "Error b3: {ub:?}");
    assert!(near(ub[1], 1.57079633, 1e-6), "Error b4: {ub:?}");
    for (toward, label) in [(p2, "Errors 1-4"), (p3, "Errors 1c-4c")] {
        let rad = Frame::distance(&ell1, &p1, &toward).unwrap();
        for (sign, suffix) in [(1.0, ""), (-1.0, " (opposite)")] {
            let inside = as2(sky.offset(&p1, &toward, sign * rad * 0.999).unwrap());
            let outside = as2(sky.offset(&p1, &toward, sign * rad * 1.001).unwrap());
            let out = tran2(&ell1, &[inside, outside], true);
            assert_eq!(out[0], inside, "Ellipse: {label}{suffix} inside");
            assert_eq!(out[1], [BAD_VALUE; 2], "Ellipse: {label}{suffix} outside");
        }
    }
    let mut ell2 = ell1.clone();
    assert_eq!(overlap(&ell1, &ell2), 5, "ellipse: Error 5");
    assert_eq!(overlap(&ell2, &ell1), 5, "ellipse: Error 6");
    ell2.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SkyFrame>()
            .unwrap()
            .set_system(SkySystem::Galactic);
        Ok(())
    })
    .unwrap();
    assert_eq!(overlap(&ell1, &ell2), 5, "ellipse: Error 7");
    assert_eq!(overlap(&ell2, &ell1), 5, "ellipse: Error 8");

    // Copies shifted in pixel coordinates by 1.95 and 2.05 times the
    // primary semi-axis.
    let q = tran2(&fs, &[p1, p2, p3], false);
    let grid = fs.frame(FrameRef::Base).unwrap().snapshot();
    let rad = grid.distance(&q[0], &q[1]).unwrap();
    // C measures the 2.05 shift with `frm1` already reassigned to the current
    // (sky) Frame, so that offset is taken in the SkyFrame.
    for (factor, offset_frame, want, err) in
        [(1.95, grid.as_ref(), 4, 9), (2.05, sky.as_ref(), 1, 11)]
    {
        let q1b = as2(offset_frame.offset(&q[0], &q[1], rad * factor).unwrap());
        let d = [q1b[0] - q[0][0], q1b[1] - q[0][1]];
        let shifted = [
            q1b,
            [q[1][0] + d[0], q[1][1] + d[1]],
            [q[2][0] + d[0], q[2][1] + d[1]],
        ];
        let pb = tran2(&fs, &shifted, true);
        let unc = unc_box(&sky_frame);
        let ell2 = form0(sky.as_ref(), pb[0], pb[1], pb[2], Some(&unc));
        assert_eq!(overlap(&ell2, &ell1), want, "ellipse: Error {err}");
        assert_eq!(overlap(&ell1, &ell2), want, "ellipse: Error {}", err + 1);
    }
    let unc = unc_box(&sky_frame);
    let p2b = [p2[0], 0.9 * p2[1] + 0.1 * p1[1]];
    let p3b = [p3[0], 0.9 * p3[1] + 0.1 * p1[1]];
    let ell2 = form0(sky.as_ref(), p1, p2b, p3b, Some(&unc));
    assert_eq!(overlap(&ell2, &ell1), 2, "ellipse: Error 13");
    assert_eq!(overlap(&ell1, &ell2), 3, "ellipse: Error 14");

    // Through a MatrixMap and back (Errors 15-17): REGIONS_C_GAP_EXCLUDED.
    let f = frame2();
    let unc = unc_box(&f);

    // A circular Ellipse simplifies to a Circle.
    let ell1 = form0(&f, [0.0, 0.0], [1.0, 0.0], [0.0, 1.0], Some(&unc));
    let reg = (&ell1 as &dyn Mapping).simplify().unwrap();
    assert!(reg.as_ref().as_any().is::<Circle>(), "ellipse: Error 18");
    let cir = CircleBuilder::new(f.clone(), vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();
    let reg = AnyObject::from_mapping(reg.as_ref()).unwrap();
    assert_eq!(
        overlap(reg.as_region().unwrap(), &cir),
        5,
        "Ellipse: Error 19"
    );

    // Across RA = 0.
    let sf = SkyFrame::new(SkySystem::Icrs);
    let ell1 = form1(&sf, [0.0, 0.0], [0.01, 0.01], 0.0);
    let ell2 = form1(&sf, [-0.015, 0.0], [0.01, 0.01], 0.0);
    assert_eq!(overlap(&ell1, &ell2), 4, "Ellipse: Error 20");
    let ell2 = form1(&sf, [6.2681853, 0.0], [0.01, 0.01], 0.0);
    assert_eq!(overlap(&ell1, &ell2), 4, "Ellipse: Error 21");
    let ell1 = form1(&sf, [-0.015, 0.0], [0.01, 0.01], 0.0);
    assert_eq!(overlap(&ell1, &ell2), 5, "Ellipse: Error 22");
}

/// Checks of `testregions.c` the port does not match yet, each with what is
/// known of the gap. `region_gaps_still_differ` fails when one closes.
const REGIONS_C_GAP_EXCLUDED: &[(&str, &str)] = &[(
    "checkEllipse Errors 15-17",
    "an Ellipse with a Box uncertainty mapped through a full MatrixMap and \
     back. C's mapped Ellipse carries an unrotated Box uncertainty (astGetUnc \
     gives bounds of +/-1e-7) and maps back to an equal Ellipse; the port's \
     carries the Box with the rotation in its FrameSet, and mapping back fails \
     when the uncertainty simplifies to a Polygon, which has no centre to \
     re-centre for the stability check. C's Ellipse Simplify makes the new \
     Ellipse with astEllipse, whose SetUnc maps the uncertainty into the new \
     base Frame; the port's best_ellipse stores it directly \
     (Ellipse::from_defining_points). Not yet traced to the end",
)];

/// The `checkEllipse` MatrixMap round trip, which differs from C: see
/// [`REGIONS_C_GAP_EXCLUDED`].
fn ellipse_matrixmap_round_trip() -> ast_core::error::AstResult<i32> {
    use ast_core::mappings::MatrixMap;
    use ast_core::regions::EllipseBuilder;
    use ndarray::array;
    let f = frame2();
    let unc = BoxRegionBuilder::from_bounds(f.clone(), &[-1.0e-7, -1.0e-7], &[1.0e-7, 1.0e-7])?
        .build()?;
    let ell1 =
        EllipseBuilder::from_points(f.clone(), vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 0.5])?
            .uncertainty(Box::new(unc))
            .build()?;
    let mut mm = MatrixMap::new(array![[1.73, 0.5003], [-1.0006, 0.866]])?;
    let ell2 = ell1.map_region(&mm, &f)?;
    mm.invert();
    let ell3 = ell2.map_region(&mm, &f)?;
    Ok(overlap(&ell1, ell3.as_ref()))
}

#[test]
fn region_gaps_still_differ() {
    for (name, reason) in REGIONS_C_GAP_EXCLUDED {
        let matches_c = match *name {
            "checkEllipse Errors 15-17" => matches!(ellipse_matrixmap_round_trip(), Ok(5)),
            _ => panic!("no check for {name}"),
        };
        assert!(
            !matches_c,
            "{name} now matches C; port the check and remove it from \
             REGIONS_C_GAP_EXCLUDED. Reason on record: {reason}"
        );
    }
}

/// `checkPolygon`: an open sky Polygon, then closed, then negated and open,
/// against the same five points; whether it needs a FrameSet before and
/// after a change of system and after simplification; and a Polygon round
/// the pole (containing it), its bounds and disc, and a Prism made from it.
#[test]
fn check_polygon() {
    use ast_core::AnyObject;
    use ast_core::frames::{Frame, SpecFrame};
    use ast_core::regions::{CircleBuilder, IntervalBuilder, Polygon, PrismBuilder};
    use std::f64::consts::PI;

    let unc = || {
        CircleBuilder::new(SkyFrame::unset_system(), vec![0.0, 0.5 * PI])
            .unwrap()
            .radius(0.01)
            .build()
            .unwrap()
    };
    let polygon = |v: &[[f64; 2]]| -> Polygon {
        PolygonBuilder::new(
            SkyFrame::unset_system(),
            v.iter().map(|p| p.to_vec()).collect(),
        )
        .unwrap()
        .uncertainty(Box::new(unc()))
        .build()
        .unwrap()
    };
    let tran = |m: &dyn Mapping, pts: &[[f64; 2]]| -> Vec<[f64; 2]> {
        let out = m
            .transform(&PointSet::from_rows(pts.to_vec()), true)
            .unwrap();
        (0..pts.len())
            .map(|i| [out.point(i)[0], out.point(i)[1]])
            .collect()
    };
    let expect =
        |label: &str, out: &[[f64; 2]], pts: &[[f64; 2]], inside: [bool; 5], first: u32| {
            for i in 0..5 {
                let want = if inside[i] { pts[i] } else { [BAD_VALUE; 2] };
                assert_eq!(out[i], want, "{label}: Poly {}", first + i as u32);
            }
        };

    let mut pol1 = polygon(&[
        [0.0, 0.0],
        [1.0, 0.5 * PI],
        [0.5 * PI, 0.25 * PI],
        [0.25 * PI, 0.0],
        [0.25 * PI, 0.25 * PI],
    ]);
    pol1.set_closed(false);
    check_dump(&pol1, &AnyObject::Polygon(pol1.clone()), "checkdump pol1");
    let p = [
        [0.0, 0.0],
        [1.0, 0.5 * PI + 0.1],
        [0.5 * PI - 0.1, 0.25 * PI],
        [0.0, 0.1],
        [0.25 * PI, 0.25 * PI + 0.1],
    ];
    expect(
        "open",
        &tran(&pol1, &p),
        &p,
        [false, false, true, false, true],
        1,
    );
    pol1.set_closed(true);
    expect(
        "closed",
        &tran(&pol1, &p),
        &p,
        [true, false, true, true, true],
        6,
    );
    pol1.set_negated(true);
    expect(
        "negated",
        &tran(&pol1, &p),
        &p,
        [true, true, false, true, false],
        11,
    );
    pol1.set_closed(false);
    expect(
        "negated open",
        &tran(&pol1, &p),
        &p,
        [false, true, false, false, false],
        16,
    );

    assert!(
        !has_frameset(&AnyObject::Polygon(pol1.clone())),
        "pol1 has FrameSet"
    );
    pol1.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SkyFrame>()
            .unwrap()
            .set_system(SkySystem::Fk5);
        Ok(())
    })
    .unwrap();
    check_dump(&pol1, &AnyObject::Polygon(pol1.clone()), "checkdump pol2");
    assert!(
        has_frameset(&AnyObject::Polygon(pol1.clone())),
        "pol1 does not have FrameSet"
    );
    pol1.set_mesh_size(30);
    let pol2 = (&pol1 as &dyn Mapping).simplify().unwrap();
    assert!(
        !has_frameset(&AnyObject::from_mapping(pol2.as_ref()).unwrap()),
        "pol2 has FrameSet"
    );

    // Round the pole, which the Polygon contains.
    let pol1 = polygon(&[
        [1.5 * PI, 0.4 * PI],
        [PI, 0.4 * PI],
        [0.5 * PI, 0.4 * PI],
        [0.0, 0.4 * PI],
    ]);
    assert_eq!(
        tran(&pol1, &[[0.0, 0.5 * PI]])[0],
        [0.0, 0.5 * PI],
        "Poly 21-22"
    );
    let near = |a: f64, b: f64, t: f64| (a - b).abs() <= t;
    let bounds_ok = |lb: &[f64], ub: &[f64], first: u32| {
        assert!(lb[0].abs() <= 1e-10, "Poly {first}: {lb:?}");
        assert!(near(lb[1], 1.25663708, 1e-6), "Poly {}: {lb:?}", first + 1);
        assert!(near(ub[0], 6.28318531, 1e-6), "Poly {}: {ub:?}", first + 2);
        assert!(near(ub[1], 1.57079633, 1e-6), "Poly {}: {ub:?}", first + 3);
    };
    let (lb, ub) = pol1.get_region_bounds().unwrap();
    bounds_ok(&lb, &ub, 23);
    let (centre, radius) = pol1.get_region_disc().unwrap();
    assert!(near(centre[1], 0.5 * PI, 1e-6), "Poly 26A: {centre:?}");
    assert!(near(radius, 0.1 * PI, 1e-6), "Poly 26B: {radius}");

    let mut f2 = SpecFrame::unset_system();
    f2.set_unit(1, "Angstrom").unwrap();
    let r2 = IntervalBuilder::new(f2, vec![5000.0], vec![6000.0])
        .unwrap()
        .build()
        .unwrap();
    let r3 = PrismBuilder::new(pol1, r2).unwrap().build().unwrap();
    let r4 = (&r3 as &dyn Mapping).simplify().unwrap();
    let r4 = AnyObject::from_mapping(r4.as_ref()).unwrap();
    let (lb, ub) = r4.as_region().unwrap().get_region_bounds().unwrap();
    bounds_ok(&lb, &ub, 27);
    assert!(near(lb[2], 5000.0, 1e-10), "Poly 31: {lb:?}");
    assert!(near(ub[2], 6000.0, 1e-6), "Poly 32: {ub:?}");
}

/// `checkInterval`: InternalUnit of a SkyFrame, a SpecFrame and their
/// CmpFrame; 3-D Intervals with BAD limits (one-sided, reversed so that they
/// exclude a range, and with a single finite axis), each closed, negated and
/// open; overlaps of half-unbounded Intervals; and mapping Intervals through
/// PermMaps that add or drop axes, giving Intervals or NullRegions. The
/// inside/outside patterns are C's checks, transcribed by test block.
#[test]
fn check_interval() {
    use ast_core::AnyObject;
    use ast_core::frames::{CmpFrame, Frame, SpecFrame};
    use ast_core::mappings::PermMap;
    use ast_core::regions::{Interval, IntervalBuilder, NullRegion};

    let frm1 = SkyFrame::unset_system();
    let mut frm2 = SpecFrame::unset_system();
    frm2.set_unit(1, "Angstrom").unwrap();
    let frm3 = CmpFrame::new(Box::new(frm1.clone()), Box::new(frm2.clone()));
    assert_eq!(frm1.internal_unit(1).unwrap(), "rad", "InternalUnit 1");
    assert_eq!(frm1.internal_unit(2).unwrap(), "rad", "InternalUnit 2");
    assert_eq!(frm2.internal_unit(1).unwrap(), "Angstrom", "InternalUnit 3");
    assert_eq!(frm3.internal_unit(1).unwrap(), "rad", "InternalUnit 4");
    assert_eq!(frm3.internal_unit(2).unwrap(), "rad", "InternalUnit 5");
    assert_eq!(frm3.internal_unit(3).unwrap(), "Angstrom", "InternalUnit 6");

    const B: f64 = BAD_VALUE;
    let interval = |f: &CmpFrame, lb: &[f64], ub: &[f64]| -> Interval {
        IntervalBuilder::new(f.clone(), lb.to_vec(), ub.to_vec())
            .unwrap()
            .build()
            .unwrap()
    };
    let tran = |m: &dyn Mapping, pts: &[Vec<f64>]| -> Vec<Vec<f64>> {
        let out = m
            .transform(&PointSet::from_rows(pts.to_vec()), true)
            .unwrap();
        (0..pts.len()).map(|i| out.point(i).to_vec()).collect()
    };
    let expect = |m: &dyn Mapping, pts: &[Vec<f64>], inside: &[u8], label: &str| {
        let out = tran(m, pts);
        for (i, &keep) in inside.iter().enumerate() {
            if keep == 1 {
                assert_eq!(out[i], pts[i], "{label}: point {} kept", i + 1);
            } else {
                assert!(
                    out[i].iter().all(|&v| v == BAD_VALUE),
                    "{label}: point {} BAD",
                    i + 1
                );
            }
        }
    };
    // Closed, negated, open and negated-open, in C's order for each Interval.
    let four_states = |reg: &mut Interval,
                       pts: &[Vec<f64>],
                       patterns: [[u8; 5]; 4],
                       label: &str,
                       start_negated: bool| {
        let states = [(true, false), (true, true), (false, false), (false, true)];
        for (k, &(closed, negated)) in states.iter().enumerate() {
            reg.set_closed(closed);
            reg.set_negated(negated != start_negated);
            expect(reg, pts, &patterns[k], &format!("{label} {}", 1 + 5 * k));
        }
    };
    let p1: Vec<Vec<f64>> = vec![
        vec![0.0, 0.0, 5000.0],
        vec![2.0, -1.0, 6000.0],
        vec![-2.0, 1.0, 5999.0],
        vec![2.0, -2.0, 6010.0],
        vec![1.0, -1.0, 4910.0],
    ];
    let pats = [
        [1, 1, 1, 0, 0],
        [1, 1, 0, 1, 1],
        [0, 0, 1, 0, 0],
        [0, 0, 0, 1, 1],
    ];

    let mut int1 = interval(&frm3, &[B, B, 5000.0], &[B, B, 6000.0]);
    check_dump(&int1, &AnyObject::Interval(int1.clone()), "checkdump int1");
    expect(&int1, &p1, &pats[0], "Interval 1");
    assert!(int1.point_in_region(&p1[0]).unwrap(), "Interval 5d");
    assert!(!int1.point_in_region(&p1[4]).unwrap(), "Interval 5e");
    four_states(&mut int1, &p1, pats, "Interval", false);

    // Reversed limits: the Interval excludes 5000-6000, so it is negated
    // first to give the same patterns.
    let mut int2 = interval(&frm3, &[B, B, 6000.0], &[B, B, 5000.0]);
    check_dump(&int2, &AnyObject::Interval(int2.clone()), "checkdump int2");
    four_states(&mut int2, &p1, pats, "Interval B", true);

    let p3: Vec<Vec<f64>> = vec![
        vec![0.0, 0.0, 5000.0],
        vec![0.5, -1.0, 6000.0],
        vec![-2.0, 0.4, 5999.0],
        vec![2.0, -2.0, 6010.0],
        vec![0.0, -3.0, 4910.0],
    ];
    let mut int3 = interval(&frm3, &[B, B, 5000.0], &[0.5, B, B]);
    check_dump(&int3, &AnyObject::Interval(int3.clone()), "checkdump int3");
    four_states(&mut int3, &p3, pats, "Interval C", false);

    // Overlaps of half-unbounded Intervals.
    int3.set_closed(true);
    int3.set_negated(false);
    let mut int4 = interval(&frm3, &[B, 0.0, B], &[B, -1.0, 6000.0]);
    let seq = |a: &mut Interval, b: &mut Interval, steps: &[(char, i32)], first: u32| {
        for (k, &(which, want)) in steps.iter().enumerate() {
            match which {
                'a' => a.negate(),
                'b' => b.negate(),
                _ => {}
            }
            assert_eq!(overlap(a, b), want, "Interval overlap {}", first + k as u32);
        }
    };
    seq(
        &mut int3,
        &mut int4,
        &[('-', 4), ('a', 4), ('b', 4), ('a', 4)],
        1,
    );
    let mut int4 = interval(&frm3, &[0.6, 0.0, B], &[B, -1.0, 6000.0]);
    seq(
        &mut int3,
        &mut int4,
        &[('-', 1), ('a', 3), ('b', 4), ('a', 2)],
        5,
    );
    let mut int4 = int3.clone();
    seq(&mut int3, &mut int4, &[('-', 5), ('b', 6)], 9);

    // Through PermMaps that add an axis.
    let mut fa = frame2();
    Frame::set_domain(&mut fa, "A").unwrap();
    let unc = BoxRegionBuilder::new(fa.clone(), vec![0.0, 0.0], vec![0.01, 0.01])
        .unwrap()
        .build()
        .unwrap();
    let mut int1 = IntervalBuilder::new(fa, vec![-2.0, 0.5], vec![0.0, B])
        .unwrap()
        .uncertainty(Box::new(unc))
        .build()
        .unwrap();
    let mut fb = BasicFrameBuilder::new(3).build().unwrap();
    Frame::set_domain(&mut fb, "B").unwrap();
    let mapped = |reg: &dyn Region,
                  pm: &PermMap,
                  f: &dyn Frame,
                  first: u32|
     -> ast_core::regions::DynRegion {
        let r = reg.map_region(pm, f).unwrap();
        assert!(r.as_any().is::<Interval>(), "Int: perm check {first}");
        assert!(
            !has_frameset(&AnyObject::from_mapping(r.as_ref()).unwrap()),
            "Int: perm check {}",
            first + 1
        );
        assert_eq!(r.naxes(), f.naxes(), "Int: perm check {}", first + 2);
        r
    };
    let pm0 = PermMap::new(&[3, 1], &[2, -1, 1], &[0.0; 6]).unwrap();
    let reg = mapped(&int1, &pm0, &fb, 1);
    let pts = vec![
        vec![0.0, 0.0, -0.5],
        vec![20.0, 0.0, -0.5],
        vec![20.0, -10.0, 0.5],
        vec![20.0, 0.0, -2.0],
    ];
    expect(reg.as_ref(), &pts, &[0, 1, 0, 1], "Int: pc 1-12");
    let pm15 = PermMap::new(&[3, 1], &[2, -1, 1], &[1.5; 6]).unwrap();
    let reg = mapped(&int1, &pm15, &fb, 4);
    let pts = vec![
        vec![20.0, 0.0, -0.5],
        vec![20.0, 1.5, -0.5],
        vec![20.0, 1.6, -0.5],
        vec![0.5, 1.5, 0.0],
    ];
    expect(reg.as_ref(), &pts, &[0, 1, 0, 1], "Int: pc 13-24");
    int1.negate();
    int1.set_closed(false);
    let reg = mapped(&int1, &pm15, &fb, 7);
    expect(reg.as_ref(), &pts, &[1, 0, 1, 0], "Int: pc 25-36");

    // Through PermMaps that drop an axis.
    let mut fa3 = BasicFrameBuilder::new(3).build().unwrap();
    Frame::set_domain(&mut fa3, "A").unwrap();
    let unc = BoxRegionBuilder::new(fa3.clone(), vec![0.0; 3], vec![0.01; 3])
        .unwrap()
        .build()
        .unwrap();
    let mut int1 = IntervalBuilder::new(fa3, vec![0.5, -1.0, -2.0], vec![B, B, 0.0])
        .unwrap()
        .uncertainty(Box::new(unc))
        .build()
        .unwrap();
    let mut fb2 = frame2();
    Frame::set_domain(&mut fb2, "B").unwrap();
    let pm = PermMap::new(&[1, -1, 2], &[1, 3], &[1.0; 6]).unwrap();
    let reg = mapped(&int1, &pm, &fb2, 10);
    let pts: Vec<Vec<f64>> = [
        [0.4, -1.0],
        [1.0, 0.1],
        [1.0, -2.1],
        [0.5, -1.0],
        [10.0, -0.1],
        [0.55, -2.0],
    ]
    .iter()
    .map(|p| p.to_vec())
    .collect();
    expect(reg.as_ref(), &pts, &[0, 0, 0, 1, 1, 1], "Int: pc 37-48");
    let pm = PermMap::new(&[1, -1, 2], &[1, 3], &[-2.0; 6]).unwrap();
    for (negate, first) in [(false, 13), (true, 17)] {
        if negate {
            int1.negate();
        }
        let r = int1.map_region(&pm, &fb2).unwrap();
        let nr = r
            .as_any()
            .downcast_ref::<NullRegion>()
            .unwrap_or_else(|| panic!("Int: perm check {first}"));
        assert!(
            !has_frameset(&AnyObject::NullRegion(nr.clone())),
            "Int: perm check {}",
            first + 1
        );
        assert_eq!(nr.naxes(), 2, "Int: perm check {}", first + 2);
        assert_eq!(nr.is_negated(), negate, "Int: perm check {}", first + 3);
    }
}

/// The header `checkBox` reads its first sky Frame from: a TAN projection
/// with a blank card where C leaves one.
const CHECK_BOX_CARDS: [&str; 9] = [
    "CTYPE1  = 'RA---TAN'",
    "CTYPE2  = 'DEC--TAN'",
    "CRPIX1  = 100",
    "CRPIX2  = 100",
    "CRVAL1  = 71.619724",
    "CRVAL2  = 42.971835",
    "  ",
    "CDELT1  = 0.6",
    "CDELT2  = 0.6",
];

fn read_fits_frameset(cards: &[&str]) -> ast_core::frames::FrameSet {
    use ast_core::AnyObject;
    use ast_core::channel::Channel;
    use ast_core::fitschan::FitsChan;

    let mut fc = FitsChan::new();
    for card in cards {
        fc.put_fits(card, false).unwrap();
    }
    fc.clear_card().unwrap();
    let Some(AnyObject::FrameSet(fs)) = fc.read().unwrap() else {
        panic!("the header does not read as a FrameSet");
    };
    fs
}

/// Change the sky system of a Box on a SkyFrame, as C's `astSetC(box,
/// "System", ...)` does.
fn set_box_sky_system(b: &mut ast_core::regions::BoxRegion, sys: Option<SkySystem>) {
    b.with_integrity(|f| {
        let sf = f.as_any_mut().downcast_mut::<SkyFrame>().unwrap();
        match sys {
            Some(s) => sf.set_system(s),
            None => sf.clear_system(),
        }
        Ok(())
    })
    .unwrap();
}

/// Transform points and check each against C's expectation: kept unchanged
/// (`1`), BAD on every axis (`0`), or not BAD on any axis (`2`).
fn expect_points(m: &dyn Mapping, pts: &[Vec<f64>], forward: bool, want: &[u8], label: &str) {
    let out = m
        .transform(&PointSet::from_rows(pts.to_vec()), forward)
        .unwrap();
    for (i, &w) in want.iter().enumerate() {
        let got = out.point(i);
        match w {
            1 => assert_eq!(got, &pts[i][..], "{label}: point {} kept", i + 1),
            0 => assert!(
                got.iter().all(|&v| v == BAD_VALUE),
                "{label}: point {} BAD, got {got:?}",
                i + 1
            ),
            _ => assert!(
                got.iter().all(|&v| v != BAD_VALUE),
                "{label}: point {} not BAD, got {got:?}",
                i + 1
            ),
        }
    }
}

/// `checkBox`, first part: a Box on the sky Frame of a FITS header, its
/// transforms through changes of sky system and axis order, with Adaptive
/// off, and its formatting; membership of nine points closed, negated and
/// open; the Box added to the FrameSet as a Frame, clipping its Mappings; and
/// the Box as one component of a permuted CmpFrame.
///
/// Divergences from C:
///
/// * C sets `Negated` on the FrameSet and checks the Box it added is not
///   changed. The FrameSet owns its copy here, so that cannot happen.
/// * C transforms the CmpFrame itself. A Frame is not a Mapping here
///   (PLAN.md §9.6), so the test builds the Mapping C's CmpFrame `Transform`
///   uses: its components in parallel, between PermMaps for its axis order.
#[test]
fn check_box_sky() {
    use ast_core::AnyObject;
    use ast_core::frames::{CmpFrame, Frame, FrameRef, SpecFrame, SpecSystem};
    use ast_core::mappings::{CmpMap, Composition, PermMap, UnitMap};
    use ast_core::regions::BoxRegion;

    let mut fs = read_fits_frameset(&CHECK_BOX_CARDS);
    let mut frm1 = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .unwrap()
        .clone();
    frm1.set_axis_digits(1, 12).unwrap();
    let mut box1 = BoxRegionBuilder::new(frm1.clone(), vec![1.25, 0.75], vec![1.5, 0.5])
        .unwrap()
        .build()
        .unwrap();
    check_dump(&box1, &AnyObject::BoxRegion(box1.clone()), "checkdump box1");
    assert_eq!(
        Frame::system_string(&box1).unwrap(),
        Some("ICRS"),
        "box1 system is not ICRS"
    );
    set_box_sky_system(&mut box1, Some(SkySystem::Galactic));
    box1.perm_axes(&[2, 1]).unwrap();
    let mut box3 = box1.clone();

    // C's inputs, rounded to single precision as C's casts do, in (b, l)
    // order.
    let f = |v: f64| v as f32 as f64;
    let pts: Vec<Vec<f64>> = vec![
        vec![f(-0.0269096590283195), f(2.82175432250852)],
        vec![f(0.2467384819891), f(2.70798275154741)],
    ];
    let check_gal = |m: &dyn Mapping, xtol: f64, ytol: f64, e3: &str, e4: &str| {
        let out = m
            .transform(&PointSet::from_rows(pts.clone()), true)
            .unwrap();
        assert!((out.point(0)[1] - 2.82175422).abs() <= ytol, "error 1");
        assert!((out.point(0)[0] + 0.0269096587).abs() <= xtol, "error 2");
        assert_eq!(out.point(1)[1], BAD_VALUE, "error {e3}");
        assert_eq!(out.point(1)[0], BAD_VALUE, "error {e4}");
    };
    check_gal(&box1, 1.0e-7, 1.0e-6, "3", "4");
    assert!(box3.is_adaptive(), "error 4a");
    box3.set_adaptive(false);
    set_box_sky_system(&mut box3, Some(SkySystem::Icrs));
    check_gal(&box3, 1.0e-8, 1.0e-8, "4b", "4c");
    set_box_sky_system(&mut box3, None);
    check_gal(&box3, 1.0e-8, 1.0e-8, "4d", "4e");
    let _box2 = (&box1 as &dyn Mapping).simplify().unwrap();
    set_box_sky_system(&mut box1, Some(SkySystem::Icrs));
    box1.perm_axes(&[2, 1]).unwrap();

    let t1 = frm1.format(1, 0.25).unwrap();
    let t2 = Frame::format(&box1, 1, 0.25).unwrap();
    assert_eq!(t1, t2, "ast_format is different for frm1 and box1");
    let (v2, _) = Frame::unformat(&box1, 1, &t2).unwrap();
    assert!((v2 - 0.25).abs() <= 1.0e-10, "ast_unformat failed for box1");
    assert_eq!(
        Frame::system_string(&box1).unwrap(),
        Some("ICRS"),
        "Box1(b) system is not ICRS"
    );
    let sky = box1.get_region_frame();
    let sky = sky.as_any().downcast_ref::<SkyFrame>().unwrap();
    assert_eq!(sky.equinox_year(), 2000.0, "Box1 equinox is not 2000.0");
    assert!(box1.is_closed(), "Box1 closed is not 1");

    let p: Vec<Vec<f64>> = [
        [1.25, 0.75],
        [1.0, 1.0],
        [1.0, 0.5],
        [1.5, 0.5],
        [1.5, 1.0],
        [1.0, 1.2],
        [0.8, 0.5],
        [1.5, 0.45],
        [1.501, 1.0],
    ]
    .iter()
    .map(|p| p.to_vec())
    .collect();
    let inside = [1, 1, 1, 1, 1, 0, 0, 0, 0];
    expect_points(&box1, &p, true, &inside, "error A");
    expect_points(&box1, &p, false, &inside, "error B");
    box1.negate();
    expect_points(&box1, &p, true, &[0, 1, 1, 1, 1, 1, 1, 1, 1], "error C");
    box1.set_closed(false);
    box1.negate();
    expect_points(&box1, &p, true, &[1, 0, 0, 0, 0, 0, 0, 0, 0], "error D");
    box1.set_negated(true);
    expect_points(&box1, &p, true, &[0, 0, 0, 0, 0, 1, 1, 1, 1], "error E");
    box1.clear_negated();
    box1.clear_closed();

    fs.add_frame(
        FrameRef::Current,
        Box::new(UnitMap::new(2)),
        Box::new(box1.clone()),
    )
    .unwrap();
    let map1 = fs.mapping(FrameRef::Current, FrameRef::Current).unwrap();
    assert!(
        map1.as_ref().as_any().is::<BoxRegion>(),
        "map1 is not a Region"
    );
    let map1 = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pix = map1
        .transform(&PointSet::from_rows(p.clone()), false)
        .unwrap();
    let pix: Vec<Vec<f64>> = (0..9).map(|i| pix.point(i).to_vec()).collect();
    expect_points(
        map1.as_ref(),
        &p,
        false,
        &[2, 2, 2, 2, 2, 0, 0, 0, 0],
        "error F",
    );
    let back = map1
        .transform(&PointSet::from_rows(pix.clone()), true)
        .unwrap();
    assert!((back.point(0)[0] - 1.25).abs() <= 1.0e-7, "error G1");
    assert!((back.point(0)[1] - 0.75).abs() <= 1.0e-7, "error G2");
    for i in 5..9 {
        assert!(
            back.point(i).iter().all(|&v| v == BAD_VALUE),
            "error G{}",
            2 * i + 1
        );
    }

    let mut frm2 = SpecFrame::unset_system();
    frm2.set_unit(1, "Angstrom").unwrap();
    let box2 = BoxRegionBuilder::new(frm2, vec![1000.0], vec![1100.0])
        .unwrap()
        .build()
        .unwrap();
    let mut frm3 = CmpFrame::new(Box::new(box1), Box::new(box2));
    frm3.perm_axes(&[2, 3, 1]).unwrap();
    // External axis 1 is the Box on the sky and external axis 2 the Box on
    // the spectrum, so these are C's "System(1)" and "System(2)".
    fn comp_box(f: &mut dyn Frame) -> &mut BoxRegion {
        f.as_any_mut().downcast_mut::<BoxRegion>().unwrap()
    }
    set_box_sky_system(comp_box(frm3.frame1_mut()), Some(SkySystem::Galactic));
    comp_box(frm3.frame2_mut())
        .with_integrity(|f| {
            f.as_any_mut()
                .downcast_mut::<SpecFrame>()
                .unwrap()
                .set_system(SpecSystem::Freq)
        })
        .unwrap();

    // cmpframe.c Transform: the components in parallel, preceded by the
    // PermMap from external to internal axes and followed by its inverse.
    let inperm: Vec<i64> = frm3.perm().iter().map(|&i| i as i64 + 1).collect();
    let mut outperm = vec![0; 3];
    for (ext, &int) in frm3.perm().iter().enumerate() {
        outperm[int] = ext as i64 + 1;
    }
    let pmap = PermMap::new(&inperm, &outperm, &[]).unwrap();
    let comps = CmpMap::new(
        Box::new(
            frm3.frame1()
                .as_any()
                .downcast_ref::<BoxRegion>()
                .unwrap()
                .clone(),
        ),
        Box::new(
            frm3.frame2()
                .as_any()
                .downcast_ref::<BoxRegion>()
                .unwrap()
                .clone(),
        ),
        Composition::Parallel,
    )
    .unwrap();
    let mut pinv = pmap.clone();
    pinv.invert();
    let map = CmpMap::new(
        Box::new(CmpMap::new(Box::new(pmap), Box::new(comps), Composition::Series).unwrap()),
        Box::new(pinv),
        Composition::Series,
    )
    .unwrap();
    let (b1, l1) = (-0.0269096590283195, 2.82175432250852);
    let (b2, l2) = (0.2467384819891, 2.70798275154741);
    let input = PointSet::from_rows([
        [b1, 2997924.58, l1],
        [b2, 2997924.58, l2],
        [b1, 4000000.0, l1],
        [b2, 4000000.0, l2],
    ]);
    let out = map.transform(&input, true).unwrap();
    let o = |i: usize, ax: usize| out.point(i)[ax];
    assert!((o(0, 0) + 0.0269096587).abs() <= 1.0e-8, "error H1");
    assert!((o(0, 1) - 2997924.5).abs() <= 1.0e-1, "error H2");
    assert!((o(0, 2) - 2.82175422).abs() <= 1.0e-6, "error H3");
    assert_eq!(o(1, 0), BAD_VALUE, "error H4");
    assert!((o(1, 1) - 2997924.5).abs() <= 1.0e-1, "error H5");
    assert_eq!(o(1, 2), BAD_VALUE, "error H6");
    assert!((o(2, 0) + 0.0269096587).abs() <= 1.0e-8, "error H7");
    assert_eq!(o(2, 1), BAD_VALUE, "error H8");
    assert!((o(2, 2) - 2.82175422).abs() <= 1.0e-6, "error H9");
    assert_eq!(o(3, 0), BAD_VALUE, "error H10");
    assert_eq!(o(3, 1), BAD_VALUE, "error H11");
    assert_eq!(o(3, 2), BAD_VALUE, "error H12");
    assert!(
        frm3.frame1()
            .as_any()
            .downcast_ref::<BoxRegion>()
            .unwrap()
            .is_closed(),
        "compound frame region is not closed"
    );
}

fn domain_frame(naxes: usize, domain: &str) -> ast_core::frames::BasicFrame {
    use ast_core::frames::Frame;
    let mut f = BasicFrameBuilder::new(naxes).build().unwrap();
    Frame::set_domain(&mut f, domain).unwrap();
    f
}

fn any_region(r: &dyn Region) -> ast_core::AnyObject {
    ast_core::AnyObject::from_mapping(r).unwrap()
}

/// `checkBox`, MapRegion part: a Box mapped by a diagonal and a sheared
/// MatrixMap, a 1-D Box by a ZoomMap, and a sky Box by the conversion to an
/// offset sky Frame, each into the new Frame with no FrameSet; the offset Box
/// clips as the original did, across zero longitude.
#[test]
fn check_box_map_region() {
    use ast_core::frames::{Frame, SkyRefIs};
    use ast_core::mappings::{MatrixMap, ZoomMap};
    use ndarray::array;

    let box1 = BoxRegionBuilder::new(
        domain_frame(3, "A"),
        vec![100.0, 200.0, 300.0],
        vec![0.0, 400.0, 250.0],
    )
    .unwrap()
    .build()
    .unwrap();
    let frm2 = domain_frame(3, "B");
    for (matrix, first) in [
        (array![[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 6.0]], 1),
        (array![[2.0, 0.1, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 6.0]], 3),
    ] {
        let map2 = MatrixMap::new(matrix).unwrap();
        let box2 = box1.map_region(&map2, &frm2).unwrap();
        assert_eq!(
            box2.domain(),
            "B",
            "ast_mapregion{first}: Box2 domain is not B"
        );
        assert!(
            !has_frameset(&any_region(box2.as_ref())),
            "ast_mapregion{}: Box2 has FrameSet",
            first + 1
        );
        if first == 3 {
            check_dump(box2.as_ref(), &any_region(box2.as_ref()), "checkdump box2");
        }
    }

    let box1 = BoxRegionBuilder::new(domain_frame(1, "A"), vec![100.0], vec![0.0])
        .unwrap()
        .build()
        .unwrap();
    let box2 = box1
        .map_region(&ZoomMap::new(1, 2.0).unwrap(), &domain_frame(1, "B"))
        .unwrap();
    assert_eq!(box2.domain(), "B", "ast_mapregion5: Box2 domain is not B");
    assert!(
        !has_frameset(&any_region(box2.as_ref())),
        "ast_mapregion6: Box2 has FrameSet (B)"
    );

    let mut frm1 = SkyFrame::unset_system();
    let box1 = BoxRegionBuilder::new(frm1.clone(), vec![0.0, 0.0], vec![0.001, 0.001])
        .unwrap()
        .build()
        .unwrap();
    let mut frm2 = frm1.clone();
    frm2.set_sky_ref(1, 0.0005).unwrap();
    frm2.set_sky_ref_is(SkyRefIs::Origin);
    let fs = frm1.convert_to(&mut frm2, &[""]).unwrap().unwrap();
    let mut box2 = box1.map_region(&fs, &frm2).unwrap();
    assert!(
        !has_frameset(&any_region(box2.as_ref())),
        "ast_mapregion7: Box2 has FrameSet (C)"
    );
    let p: Vec<Vec<f64>> = [
        [0.00049, 0.0009],
        [0.00051, 0.0009],
        [-0.0016, 0.0],
        [-0.0014, 0.0],
        [6.2815853, 0.0],
        [6.2817853, 0.0],
    ]
    .iter()
    .map(|p| p.to_vec())
    .collect();
    let out = box2
        .transform(&PointSet::from_rows(p.clone()), true)
        .unwrap();
    for (i, first) in [(0, 1), (3, 9), (5, 11)] {
        for (ax, &want) in p[i].iter().enumerate() {
            assert!(
                (out.point(i)[ax] - want).abs() <= 1.0e-10,
                "error I{}",
                first + ax
            );
        }
    }
    for (i, first) in [(1, 3), (4, 5), (2, 7)] {
        for ax in 0..2 {
            assert_eq!(out.point(i)[ax], BAD_VALUE, "error I{}", first + ax);
        }
    }

    let box2 = box2
        .as_any_mut()
        .downcast_mut::<ast_core::regions::BoxRegion>()
        .unwrap();
    box2.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SkyFrame>()
            .unwrap()
            .set_sky_ref_is(SkyRefIs::Pole);
        Ok(())
    })
    .unwrap();
    let box2 = (&*box2 as &dyn Mapping).simplify().unwrap();
    assert!(
        !has_frameset(&ast_core::AnyObject::from_mapping(box2.as_ref()).unwrap()),
        "ast_mapregion8: Box2 has FrameSet (B)"
    );
}

/// `checkBox`, Overlap part: Boxes in different Domains cannot be compared;
/// each Box is the same as itself, open or closed; and Boxes that enclose,
/// miss and cross each other.
#[test]
fn check_box_overlap() {
    use ast_core::frames::Frame;

    let make = |f: &ast_core::frames::BasicFrame, p1: [f64; 3], p2: [f64; 3]| {
        BoxRegionBuilder::new(f.clone(), p1.to_vec(), p2.to_vec())
            .unwrap()
            .build()
            .unwrap()
    };
    let (p1, p2) = ([100.0, 200.0, 300.0], [0.0, 400.0, 250.0]);
    let frm1 = domain_frame(3, "A");
    let mut box1 = make(&frm1, p1, p2);
    box1.set_closed(true);
    let mut frm2 = domain_frame(3, "B");
    let mut box2 = make(&frm2, p1, p2);
    box2.set_closed(false);
    assert_eq!(
        overlap(&box1, &box2),
        0,
        "ast_overlap A: result should be zero"
    );
    assert_eq!(
        overlap(&box1, &box1),
        5,
        "ast_overlap B: result should be 5"
    );
    assert_eq!(
        overlap(&box2, &box2),
        5,
        "ast_overlap C: result should be 5"
    );

    Frame::set_domain(&mut frm2, "A").unwrap();
    for (q1, q2, ab, ba, la, lb) in [
        (
            [100.0, 200.0, 300.0],
            [-100.0, 600.0, 400.0],
            2,
            3,
            "D",
            "E",
        ),
        ([300.0, 200.0, 300.0], [201.0, 400.0, 250.0], 1, 1, "F", "G"),
        ([150.0, 200.0, 300.0], [50.0, 400.0, 250.0], 4, 4, "H", "I"),
    ] {
        let box2 = make(&frm2, q1, q2);
        assert_eq!(
            overlap(&box1, &box2),
            ab,
            "ast_overlap {la}: result should be {ab}"
        );
        assert_eq!(
            overlap(&box2, &box1),
            ba,
            "ast_overlap {lb}: result should be {ba}"
        );
    }
}

/// `checkBox`, pixel mask part: masking outside a Box on a 5x5 grid, and
/// outside a negated Box on the sky through a FITS FrameSet, inverted so
/// that it maps the sky onto the pixel grid.
#[test]
fn check_box_mask() {
    use ast_core::mappings::Mapping as _;

    // VAL__BADR, the Fortran test's bad value for a REAL array.
    const VAL_BADR: f32 = f32::MIN;

    let box1 = BoxRegionBuilder::new(domain_frame(2, "A"), vec![1.0, 1.0], vec![3.1, 4.1])
        .unwrap()
        .build()
        .unwrap();
    let mut rin = vec![1.0_f32; 25];
    let res = mask(&box1, None, false, &[1, 1], &[5, 5], &mut rin, VAL_BADR).unwrap();
    assert_eq!(res, 13, "res should be 13");
    for i in 1..=5 {
        for j in 1..=5 {
            let v = rin[(i - 1) * 5 + (j - 1)];
            if j <= 3 && i <= 4 {
                assert_eq!(v, 1.0, "rin({j},{i}) should be 1.0");
            } else {
                assert_eq!(v, VAL_BADR, "rin({j},{i}) should be VAL__BADR");
            }
        }
    }

    let mut cards = CHECK_BOX_CARDS;
    cards[2] = "CRPIX1  = 20";
    cards[3] = "CRPIX2  = 20";
    cards[4] = "CRVAL1  = 0.0";
    cards[5] = "CRVAL2  = 0.0";
    cards[6] = "   ";
    cards[7] = "CDELT1  = 1.6";
    cards[8] = "CDELT2  = 1.6";
    let mut fs = read_fits_frameset(&cards);
    let mut box1 = BoxRegionBuilder::new(
        fs.clone(),
        vec![0.13089969, 0.17453293],
        vec![-0.13089971, -0.17453293],
    )
    .unwrap()
    .build()
    .unwrap();
    let mut image = vec![1.0_f32; 50 * 50];
    box1.negate();
    fs.invert();
    let res = mask(
        &box1,
        Some(&fs),
        false,
        &[1, 1],
        &[50, 50],
        &mut image,
        VAL_BADR,
    )
    .unwrap();
    assert_eq!(res, 522, "res should be 522");
    let at = |x: usize, y: usize| image[(y - 1) * 50 + (x - 1)];
    assert_eq!(at(34, 42), VAL_BADR, "image(34,42) should be VAL__BADR");
    assert_eq!(at(33, 42), 1.0, "image(33,42) should be 1.0");
    assert_eq!(at(16, 14), VAL_BADR, "image(16,14) should be VAL__BADR");
    assert_eq!(at(15, 13), 1.0, "image(15,13) should be 1.0");
}

/// `checkBox`, axis-count part: a Box mapped by PermMaps that add an axis
/// (a constant inside or outside it) or drop one, giving a Box with no
/// FrameSet, or a NullRegion when the dropped axis's constant is outside.
#[test]
fn check_box_perm() {
    use ast_core::frames::Frame;
    use ast_core::mappings::PermMap;
    use ast_core::regions::{BoxRegion, CircleBuilder, NullRegion};

    let frm1 = domain_frame(2, "A");
    let unc = CircleBuilder::new(frm1.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();
    let box1 = BoxRegionBuilder::new(frm1, vec![-1.0, 1.0], vec![-2.0, 1.5])
        .unwrap()
        .uncertainty(Box::new(unc))
        .build()
        .unwrap();
    let mapped = |reg: &dyn Region, pm: &PermMap, f: &dyn Frame, first: u32, null: bool| {
        let r = reg.map_region(pm, f).unwrap();
        if null {
            assert!(r.as_any().is::<NullRegion>(), "Box: perm check {first}");
        } else {
            assert!(r.as_any().is::<BoxRegion>(), "Box: perm check {first}");
        }
        assert!(
            !has_frameset(&any_region(r.as_ref())),
            "Box: perm check {}",
            first + 1
        );
        assert_eq!(r.naxes(), f.naxes(), "Box: perm check {}", first + 2);
        r
    };
    let rows = |pts: &[&[f64]]| -> Vec<Vec<f64>> { pts.iter().map(|p| p.to_vec()).collect() };

    let frm2 = domain_frame(3, "B");
    let pm = PermMap::new(&[3, 1], &[2, -1, 1], &[0.0; 6]).unwrap();
    let reg = mapped(&box1, &pm, &frm2, 1, false);
    let pts = rows(&[
        &[0.0, 0.0, 0.0],
        &[0.7, 0.0, -0.5],
        &[2.0, 0.0, -1.0],
        &[1.5, 0.0, 0.0],
    ]);
    expect_points(reg.as_ref(), &pts, true, &[0, 1, 0, 1], "box: pc 1-12");

    let pm = PermMap::new(&[3, 1], &[2, -1, 1], &[1.0; 6]).unwrap();
    let reg = mapped(&box1, &pm, &frm2, 4, false);
    let pts = rows(&[
        &[0.0, 0.0, 0.0],
        &[0.7, 1.0, -0.5],
        &[0.7, 1.1, -0.5],
        &[0.7, 0.9, -0.5],
    ]);
    expect_points(reg.as_ref(), &pts, true, &[0, 1, 0, 0], "box: pc 11-19");

    let frm2 = domain_frame(1, "B");
    let x = rows(&[&[-2.5], &[-1.9], &[0.0], &[0.5]]);
    let pm = PermMap::new(&[1, -1], &[1], &[1.4; 6]).unwrap();
    let reg = mapped(&box1, &pm, &frm2, 7, false);
    expect_points(reg.as_ref(), &x, true, &[0, 1, 1, 0], "box: pc 20-23");
    let pm = PermMap::new(&[1, -1], &[1], &[1.6; 6]).unwrap();
    let reg = mapped(&box1, &pm, &frm2, 10, true);
    expect_points(reg.as_ref(), &x, true, &[0, 0, 0, 0], "box: pc 24-27");

    let box1 =
        BoxRegionBuilder::from_bounds(domain_frame(3, "A"), &[0.5, -1.0, -2.0], &[30.0, 5.0, 0.0])
            .unwrap()
            .build()
            .unwrap();
    let pm = PermMap::new(&[1, -1, 2], &[1, 3], &[1.0; 6]).unwrap();
    let reg = mapped(&box1, &pm, &domain_frame(2, "B"), 13, false);
    let pts = rows(&[
        &[0.4, -1.0],
        &[1.0, 0.1],
        &[1.0, -2.1],
        &[0.5, -1.0],
        &[10.0, -0.1],
        &[0.55, -2.0],
    ]);
    expect_points(
        reg.as_ref(),
        &pts,
        true,
        &[0, 0, 0, 1, 1, 1],
        "Box: pc 37-48",
    );
}

/// `checkBox`, mesh part: the grid and boundary mesh of a Box in the pixel
/// Frame of a rotated TAN header, their point counts and positions, and the
/// Box mapped onto the sky becoming a Polygon with no FrameSet.
#[test]
fn check_box_mesh() {
    use ast_core::frames::FrameRef;
    use ast_core::regions::Polygon;

    let cards = [
        "CTYPE1  = 'RA---TAN'",
        "CTYPE2  = 'DEC--TAN'",
        "CRPIX1  = 20",
        "CRPIX2  = 20",
        "CRVAL1  = 0.0",
        "CRVAL2  = 0.0",
        "CROTA1  = 30.0",
        "CDELT1  = -0.00001",
        "CDELT2  = 0.00001",
    ];
    let fs = read_fits_frameset(&cards);
    let bfrm = fs
        .base()
        .as_any()
        .downcast_ref::<ast_core::frames::BasicFrame>()
        .unwrap()
        .clone();
    let box1 = BoxRegionBuilder::new(bfrm, vec![100.0, 150.0], vec![150.0, 170.0])
        .unwrap()
        .build()
        .unwrap();

    let grid = box1.region_grid().unwrap();
    assert_eq!(grid.npoint(), 176, "Box: Error mesh 3");
    for i in 0..grid.npoint() {
        let p = grid.point(i);
        assert!((p[0] - 100.0).abs() <= 50.0, "Box: Error mesh 1: {p:?}");
        assert!((p[1] - 150.0).abs() <= 20.0, "Box: Error mesh 2: {p:?}");
    }
    let mesh = box1.region_mesh().unwrap();
    assert_eq!(mesh.npoint(), 198, "Box: Error mesh 4");
    for i in 0..mesh.npoint() {
        let p = mesh.point(i);
        assert!(
            p[0] == 50.0 || p[0] == 150.0 || p[1] == 130.0 || p[1] == 170.0,
            "Box: Error mesh 5: {p:?}"
        );
    }

    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let reg1 = box1.map_region(map.as_ref(), fs.current()).unwrap();
    assert!(
        !has_frameset(&any_region(reg1.as_ref())),
        "Box: poly simp 1"
    );
    assert!(reg1.as_any().is::<Polygon>(), "Box: poly simp 2");
}
