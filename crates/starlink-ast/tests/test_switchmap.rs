// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tests for SwitchMap.
//!
//! Ancestor: C AST testswitchmap.c — tests selector-based route dispatching.
//! Adapted to use the Rust public API (BasicFrameBuilder, BoxRegionBuilder,
//! WinMap::from_corners, PointSet::from_rows).

use crate::common::native_dump;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::frames::BasicFrameBuilder;
use starlink_ast::mappings::{
    DynMapping, Mapping, SelectorMapBuilder, SwitchMap, UnitMap, WinMap, ZoomMap,
};
use starlink_ast::regions::{BoxRegionBuilder, DynRegion, remove_regions};
use starlink_ast::{AnyObject, from_native_string};

/// Create a SwitchMap with two routes:
///
/// - Route 1 (x in [0,10]): WinMap scaling [0,10]→[0,100]
/// - Route 2 (x in [20,30]): WinMap scaling [20,30]→[200,300]
///
/// Forward selector is a SelectorMap over 1D regions.
fn make_1d_switchmap() -> SwitchMap {
    let frame1 = BasicFrameBuilder::new(1).build().unwrap();
    let frame2 = BasicFrameBuilder::new(1).build().unwrap();

    let reg1: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame1, &[0.0], &[10.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let reg2: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame2, &[20.0], &[30.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let sel = SelectorMapBuilder::new(vec![reg1, reg2]).unwrap().build();
    let fsmap: DynMapping = Box::new(sel);

    let route1: DynMapping =
        Box::new(WinMap::from_corners(&[0.0], &[10.0], &[0.0], &[100.0]).unwrap());
    let route2: DynMapping =
        Box::new(WinMap::from_corners(&[20.0], &[30.0], &[200.0], &[300.0]).unwrap());

    SwitchMap::new(Some(fsmap), None, vec![route1, route2]).unwrap()
}

#[test]
fn switchmap_basic_construction() {
    let sw = make_1d_switchmap();
    assert_eq!(sw.nin(), 1);
    assert_eq!(sw.nout(), 1);
    assert_eq!(sw.nroute(), 2);
    assert!(sw.has_forward());
    assert!(!sw.has_inverse());
}

#[test]
fn switchmap_no_routes_rejected() {
    let result = SwitchMap::new(Some(Box::new(UnitMap::new(1))), None, vec![]);
    assert!(result.is_err());
}

#[test]
fn switchmap_no_selectors_rejected() {
    let route: DynMapping = Box::new(UnitMap::new(2));
    let result = SwitchMap::new(None, None, vec![route]);
    assert!(result.is_err());
}

#[test]
fn switchmap_mismatched_routes_rejected() {
    let r1: DynMapping = Box::new(UnitMap::new(2));
    let r2: DynMapping = Box::new(UnitMap::new(3));
    let sel: DynMapping = Box::new(ZoomMap::new(2, 1.0).unwrap());
    let result = SwitchMap::new(Some(sel), None, vec![r1, r2]);
    assert!(result.is_err());
}

#[test]
fn switchmap_forward_route1() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[5.0]]);
    let out = sw.transform(&pts, true).unwrap();
    // WinMap [0,10]→[0,100]: 5 → 50
    assert!((out.point(0)[0] - 50.0).abs() < 1e-10);
}

#[test]
fn switchmap_forward_route2() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[25.0]]);
    let out = sw.transform(&pts, true).unwrap();
    // WinMap [20,30]→[200,300]: 25 → 250
    assert!((out.point(0)[0] - 250.0).abs() < 1e-10);
}

#[test]
fn switchmap_forward_no_route() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[15.0]]);
    let out = sw.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn switchmap_forward_bad_input() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = sw.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn switchmap_forward_multiple_points() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[5.0], [25.0], [15.0], [BAD_VALUE]]);
    let out = sw.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 50.0).abs() < 1e-10);
    assert!((out.point(1)[0] - 250.0).abs() < 1e-10);
    assert_eq!(out.point(2)[0], BAD_VALUE);
    assert_eq!(out.point(3)[0], BAD_VALUE);
}

#[test]
fn switchmap_equal() {
    let sw1 = make_1d_switchmap();
    let sw2 = make_1d_switchmap();
    assert!(sw1.equal(&sw2));
}

#[test]
fn switchmap_remove_regions() {
    let sw = make_1d_switchmap();
    let cleaned = remove_regions(&sw);
    // The selector was a SelectorMap containing Regions; after remove_regions,
    // those Regions become UnitMaps inside the selector. The SwitchMap dimensions
    // should still match the originals.
    assert_eq!(cleaned.nin(), 1);
    assert_eq!(cleaned.nout(), 1);
}

#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn switchmap_simplify_inverse_cancel_fixture() {
    // The fixture: CmpMap(SwitchMap, inv(SwitchMap), series) with matching content.
    // Should simplify to UnitMap(1).
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/switchmap_inverse_cancel.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    assert!(
        simplified.as_any().is::<UnitMap>(),
        "Expected UnitMap, got {:?}",
        simplified
    );
    assert_eq!(simplified.nin(), 1);
    assert_eq!(simplified.nout(), 1);
}

#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn switchmap_simplify_internal_fixture() {
    // The fixture: SwitchMap whose children can be simplified.
    // Result should still be a SwitchMap (or simpler), with same nin/nout.
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/switchmap_internal_simplify.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let original = obj.into_mapping().unwrap();
    let nin_orig = original.nin();
    let nout_orig = original.nout();

    let simplified = original.simplify().unwrap();
    assert_eq!(simplified.nin(), nin_orig);
    assert_eq!(simplified.nout(), nout_orig);
}

#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn switchmap_simplify_invert_normalize_fixture() {
    // The fixture: an inverted SwitchMap. Simplification normalizes the
    // inversion into the children, leaving a non-inverted SwitchMap.
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/switchmap_invert_normalize.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let original = obj.into_mapping().unwrap();
    let nin_orig = original.nin();
    let nout_orig = original.nout();

    let simplified = original.simplify().unwrap();
    assert_eq!(simplified.nin(), nin_orig);
    assert_eq!(simplified.nout(), nout_orig);
}

#[test]
fn switchmap_serialization_roundtrip() {
    let sw = make_1d_switchmap();
    let obj = AnyObject::SwitchMap(sw.clone());
    let text = native_dump(&obj).unwrap();
    let obj2 = from_native_string(&text).unwrap();
    let loaded: SwitchMap = match obj2 {
        AnyObject::SwitchMap(m) => m,
        other => panic!("Expected SwitchMap, got {:?}", other),
    };

    assert_eq!(loaded.nroute(), 2);
    assert_eq!(loaded.nin(), 1);
    assert_eq!(loaded.nout(), 1);

    let pts = PointSet::from_rows([[5.0], [25.0], [15.0]]);
    let out1 = sw.transform(&pts, true).unwrap();
    let out2 = loaded.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert_eq!(out1.point(i)[0], out2.point(i)[0]);
    }
}

// ---------------------------------------------------------------------------
// testswitchmap.c, one test per section
// ---------------------------------------------------------------------------

mod testswitchmap_c {
    //! The three sections of `reference/starlink-ast/ast_tester/testswitchmap.c`.
    //! C builds each SwitchMap, inverts it and transforms in the inverse
    //! direction to exercise the forward transformation; the same sequence
    //! is used here. C's later `astInvert` of a selector Mapping has no effect
    //! on the SwitchMap, which records the selector's orientation when it is
    //! built, so it has no counterpart.

    use crate::common::native_dump;
    use starlink_ast::channel::Channel;
    use starlink_ast::constants::BAD_VALUE;
    use starlink_ast::coords::PointSet;
    use starlink_ast::fitschan::FitsChan;
    use starlink_ast::frames::{BasicFrame, FrameRef};
    use starlink_ast::mappings::{
        CmpMap, Composition, DynMapping, Mapping, MathMap, PermMap, SelectorMapBuilder, SwitchMap,
        UnitMap, WinMap,
    };
    use starlink_ast::regions::{BoxRegionBuilder, DynRegion};
    use starlink_ast::{AnyObject, from_native_string};

    fn win(x1: f64, x2: f64, l1: f64, l2: f64) -> WinMap {
        WinMap::from_corners(&[x1], &[x2], &[l1], &[l2]).unwrap()
    }

    fn tran(m: &dyn Mapping, rows: &[Vec<f64>], forward: bool) -> Vec<Vec<f64>> {
        let out = m
            .transform(&PointSet::from_rows(rows.to_vec()), forward)
            .unwrap();
        (0..rows.len()).map(|i| out.point(i).to_vec()).collect()
    }

    fn close(got: f64, want: f64, tol: f64, err: u32) {
        assert!(got != BAD_VALUE, "Error {err}: BAD");
        assert!((got - want).abs() <= tol, "Error {err}: {got}, want {want}");
    }

    /// The checks C makes of every section after its transforms: simplifying
    /// the SwitchMap reports no change, an inverted one simplifies to a
    /// non-inverted one, it cancels against its inverse on either side, and
    /// it survives a dump.
    fn simplification_and_dump(swm: &SwitchMap, errs: [u32; 3], label: &str) {
        // C compares the result with the SwitchMap itself: with nothing to
        // simplify, astSimplify returns the same pointer.
        let (_, changed) = (swm as &dyn Mapping).simplify_reporting_change().unwrap();
        assert!(!changed, "{label}");
        let plain = native_dump(&AnyObject::from_mapping(swm).unwrap()).unwrap();

        let mut inv = swm.clone();
        inv.invert();
        assert!(
            !(&inv as &dyn Mapping).simplify().unwrap().is_inverted(),
            "Error {}",
            errs[0]
        );

        for (k, (a, b)) in [(inv.clone(), swm.clone()), (swm.clone(), inv.clone())]
            .into_iter()
            .enumerate()
        {
            let cm: DynMapping =
                Box::new(CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap());
            let s = cm.simplify().unwrap();
            assert!(s.as_ref().as_any().is::<UnitMap>(), "Error {}", errs[1 + k]);
        }

        let back = from_native_string(&plain).unwrap();
        let again = native_dump(&back).unwrap();
        assert_eq!(again, plain, "{label}: dump round trip");
    }

    /// Section 1: two overlapping spectra in the rows of a 2-row grid, a
    /// PermMap forward selector reading the row number and a MathMap inverse
    /// selector choosing a row by wavelength.
    #[test]
    fn section_1_two_rows() {
        let row = |l1: f64, l2: f64, con: f64| -> DynMapping {
            let pm = PermMap::new(&[1, -1], &[1], &[con]).unwrap();
            Box::new(
                CmpMap::new(
                    Box::new(pm),
                    Box::new(win(1.0, 101.0, l1, l2)),
                    Composition::Series,
                )
                .unwrap(),
            )
        };
        let fs_map = PermMap::new(&[0, 1], &[2], &[0.0]).unwrap();
        let is_map = MathMap::new(1, 1, &["y"], &["x=qif(y>1800,2,1)"]).unwrap();
        let mut swm = SwitchMap::new(
            Some(Box::new(fs_map)),
            Some(Box::new(is_map)),
            vec![row(1000.0, 2000.0, 1.0), row(1600.0, 2700.0, 2.0)],
        )
        .unwrap();
        swm.invert();

        let input = [
            vec![1.0, 1.0],
            vec![101.0, 2.0],
            vec![1.0, 2.0],
            vec![101.0, 1.0],
        ];
        let out = tran(&swm, &input, false);
        for (i, want) in [1000.0, 2700.0, 1600.0, 2000.0].into_iter().enumerate() {
            close(out[i][0], want, 1.0e-5, 5 + i as u32);
        }
        let back = tran(&swm, &out, true);
        let want = [(1.0, 1.0), (101.0, 2.0), (61.0, 1.0), (37.3636364, 2.0)];
        for (i, &(x, y)) in want.iter().enumerate() {
            close(back[i][0], x, 1.0e-5, 17 + 2 * i as u32);
            close(back[i][1], y, 1.0e-5, 18 + 2 * i as u32);
        }

        swm.invert();
        simplification_and_dump(&swm, [25, 26, 27], "ast_equal 1");

        close(swm.rate(&[20.0, 1.0], 1, 1).unwrap(), 10.0, 1.0e-6, 28);
        close(swm.rate(&[20.0, 2.0], 1, 1).unwrap(), 11.0, 1.0e-6, 29);
        swm.invert();
        close(swm.rate(&[1700.0], 1, 1).unwrap(), 0.1, 1.0e-6, 30);
        close(swm.rate(&[1900.0], 1, 1).unwrap(), 1.0 / 11.0, 1.0e-6, 31);
    }

    /// Section 2: the two halves of a 1-D grid, with one MathMap as both
    /// selectors: its forward picks a half by pixel and its inverse a half
    /// by wavelength.
    #[test]
    fn section_2_two_halves() {
        let sm = MathMap::new(1, 1, &["y=qif(x>101.5,2,1)"], &["x=qif(y>1800,2,1)"]).unwrap();
        let mut swm = SwitchMap::new(
            Some(Box::new(sm.clone())),
            Some(Box::new(sm)),
            vec![
                Box::new(win(1.0, 101.0, 1000.0, 2000.0)),
                Box::new(win(102.0, 202.0, 1600.0, 2700.0)),
            ],
        )
        .unwrap();
        swm.invert();

        let input: Vec<Vec<f64>> = [1.0, 202.0, 102.0, 101.0].map(|v| vec![v]).to_vec();
        let out = tran(&swm, &input, false);
        for (i, want) in [1000.0, 2700.0, 1600.0, 2000.0].into_iter().enumerate() {
            close(out[i][0], want, 1.0e-5, 105 + i as u32);
        }
        let back = tran(&swm, &out, true);
        for (i, want) in [1.0, 202.0, 61.0, 138.3636364].into_iter().enumerate() {
            close(back[i][0], want, 1.0e-5, 117 + 2 * i as u32);
        }

        swm.invert();
        simplification_and_dump(&swm, [125, 126, 127], "ast_equal 2");

        close(swm.rate(&[20.0], 1, 1).unwrap(), 10.0, 1.0e-6, 128);
        close(swm.rate(&[120.0], 1, 1).unwrap(), 11.0, 1.0e-6, 129);
        swm.invert();
        close(swm.rate(&[1700.0], 1, 1).unwrap(), 0.1, 1.0e-6, 130);
        close(swm.rate(&[1900.0], 1, 1).unwrap(), 1.0 / 11.0, 1.0e-6, 131);
    }

    /// The pixel-to-sky Mapping of a TAN header with reference pixel
    /// `crpix1`.
    fn tan_map(crpix1: f64) -> (DynMapping, BasicFrame) {
        let header = format!(
            "CRPIX1  = {crpix1}\nCRPIX2  = 45\nCRVAL1  = 45\nCRVAL2  = 89.9\n\
             CDELT1  = -0.01\nCDELT2  = 0.01\nCTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\n"
        );
        let mut fc = FitsChan::from_header(&header).unwrap();
        let Some(AnyObject::FrameSet(fs)) = fc.read().unwrap() else {
            panic!("the header did not read as a FrameSet");
        };
        let grid = fs
            .base()
            .as_any()
            .downcast_ref::<BasicFrame>()
            .expect("the base Frame is a basic Frame")
            .clone();
        (fs.mapping(FrameRef::Base, FrameRef::Current).unwrap(), grid)
    }

    /// Section 3: two images of the same sky side by side in one grid, a
    /// SelectorMap of two Boxes as the forward selector and a PermMap that
    /// always picks the first route as the inverse selector.
    #[test]
    fn section_3_two_images() {
        let (rm0, _) = tan_map(45.0);
        let (rm1, grid) = tan_map(135.0);
        let boxr = |lo: [f64; 2], hi: [f64; 2]| -> DynRegion {
            Box::new(
                BoxRegionBuilder::from_bounds(grid.clone(), &lo, &hi)
                    .unwrap()
                    .build()
                    .unwrap(),
            )
        };
        let selmap = SelectorMapBuilder::new(vec![
            boxr([10.0, 10.0], [80.0, 80.0]),
            boxr([100.0, 10.0], [170.0, 80.0]),
        ])
        .unwrap()
        .build();
        let is_perm = PermMap::new(&[-1], &[0, 0], &[1.0]).unwrap();
        let mut swm = SwitchMap::new(
            Some(Box::new(selmap)),
            Some(Box::new(is_perm)),
            vec![rm0.clone(), rm1],
        )
        .unwrap();
        swm.invert();

        let input = [
            vec![5.0, 5.0],
            vec![50.0, 50.0],
            vec![90.0, 50.0],
            vec![140.0, 50.0],
        ];
        let out = tran(&swm, &input, false);
        let rmout = tran(rm0.as_ref(), &input, true);
        assert_eq!(out[0], [BAD_VALUE, BAD_VALUE], "Errors 132-133");
        assert_eq!(out[1], rmout[1], "Errors 134-135");
        assert_eq!(out[2], [BAD_VALUE, BAD_VALUE], "Errors 136-137");
        for k in 0..2 {
            let tol = 1.0e-6 * rmout[1][k].abs();
            assert!((out[3][k] - rmout[1][k]).abs() <= tol, "Error {}", 138 + k);
        }

        let back = tran(&swm, &out, true);
        assert_eq!(back[0], [BAD_VALUE, BAD_VALUE], "Errors 140-141");
        close(back[1][0], 50.0, 1.0e-6, 142);
        close(back[1][1], 50.0, 1.0e-6, 143);
        assert_eq!(back[2], [BAD_VALUE, BAD_VALUE], "Errors 144-145");
        close(back[3][0], 50.0, 1.0e-6, 146);
        close(back[3][1], 50.0, 1.0e-6, 147);

        swm.invert();
        simplification_and_dump(&swm, [148, 149, 150], "ast_equal 3");

        for (k, (ax1, ax2)) in [(1, 1), (2, 2), (1, 2), (2, 1)].into_iter().enumerate() {
            let r1 = swm.rate(&[140.0, 50.0], ax1, ax2).unwrap();
            let r2 = rm0.rate(&[50.0, 50.0], ax1, ax2).unwrap();
            assert!(
                (r1 - r2).abs() <= (1.0e-6 * r2).abs(),
                "Error {}: {r1} vs {r2}",
                151 + k
            );
        }
    }
}

#[test]
fn a_selector_value_beyond_any_integer_selects_no_route() {
    // The selector is the input itself, so each input picks its route: C
    // takes round(sel) - 1 as the route index and ignores one out of range.
    let routes: Vec<DynMapping> = vec![
        Box::new(ZoomMap::new(1, 2.0).unwrap()),
        Box::new(ZoomMap::new(1, 3.0).unwrap()),
    ];
    let m = SwitchMap::new(Some(Box::new(UnitMap::new(1))), None, routes).unwrap();
    let inp = [-1.0e30, 0.49, 0.5, 2.25, 2.5, 1.0e30, -f64::MAX];
    let out = m
        .transform(&PointSet::from_rows(inp.iter().map(|&v| vec![v])), true)
        .unwrap();
    let want = [
        BAD_VALUE, BAD_VALUE, 1.0, 6.75, BAD_VALUE, BAD_VALUE, BAD_VALUE,
    ];
    assert_eq!(out.axis(0), &want[..]);
    assert_eq!(m.rate(&[-1.0e30], 1, 1).unwrap(), BAD_VALUE);
    assert_eq!(m.rate(&[1.0e30], 1, 1).unwrap(), BAD_VALUE);
}
