// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Regions that are unusual in shape, extent or dimension: grids, masks,
//! overlaps, bounds, meshes, Ellipses, Polygons and Regions mapped into fewer
//! axes must give the C library's result or its error, and never panic, hang
//! or overflow the stack.
//!
//! Ported from `reference/starlink-ast/ast_tester/testregionhardening.c`, one
//! test per C section.
//! Divergences from the C:
//!
//! - C checks a few results only for being in range, as the sanitizers find
//!   the faults it guards against; each case here also pins the value the C
//!   library gives at the pinned commit, from the C test or a probe.
//! - An expected error is compared by its code and every message C reports,
//!   in order, without C's first "Error in routine" line.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::{BasicFrame, BasicFrameBuilder, CmpFrame, Frame, SkyFrame};
use starlink_ast::mappings::{Mapping, MathMap, PermMap};
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, CmpRegionBuilder, EllipseBuilder,
    IntervalBuilder, NullRegionBuilder, Overlap, PointListBuilder, PolygonBuilder, PrismBuilder,
    Region, mask,
};

/// Check that `result` is an error with `code` whose messages, one per
/// line, are `messages`, with no context that C would not report.
pub(crate) fn expect_err<T>(result: AstResult<T>, code: ErrorCode, messages: &[&str]) {
    let Err(err) = result else {
        panic!("an error was expected");
    };
    assert_eq!(err.code(), code, "error code of {err}");
    let got: Vec<&str> = err.message().split('\n').collect();
    assert_eq!(got, messages, "messages of {err}");
    assert!(err.context().is_empty(), "context C does not report: {err}");
}

/// Run `f` on its own thread and return its result, failing if it panics or
/// takes more than 10 seconds, so that a case that used to hang fails rather
/// than stopping the test program.
pub(crate) fn within_10s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("took more than 10 s"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("panicked"),
    }
}

fn plain(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

/// The overlap code C's `astOverlap` returns.
fn overlap_code(a: &dyn Region, b: &dyn Region) -> i32 {
    a.overlap(b).unwrap() as i32
}

/// The grid of a Box with corners `p1` and `p2` in `frame` and a MeshSize of
/// 30, as `check_grid` in the C test makes it.
fn box_grid(
    frame: impl Frame + 'static,
    p1: &[f64],
    p2: &[f64],
) -> AstResult<starlink_ast::coords::PointSet> {
    let mut bx = BoxRegionBuilder::from_bounds(frame, p1, p2)
        .unwrap()
        .build()
        .unwrap();
    bx.set_mesh_size(30);
    bx.region_grid()
}

/// Check that a grid has `npoint` points, each inside the Box with corners
/// `p1` and `p2`.
fn check_grid_inside(grid: &starlink_ast::coords::PointSet, npoint: usize, p1: &[f64], p2: &[f64]) {
    assert_eq!(grid.npoint(), npoint);
    for (j, (&a, &b)) in p1.iter().zip(p2).enumerate() {
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let tol = 1.0e-9 * (hi - lo);
        for &v in grid.axis(j) {
            assert!(
                v >= lo - tol && v <= hi + tol,
                "axis {} point {v} outside the Box",
                j + 1
            );
        }
    }
}

#[test]
fn grids() {
    // A Box with no width on its last axis gets two ticks on that axis.
    let (a, b) = ([0.0, 0.0], [1.0, 0.0]);
    let grid = within_10s(move || box_grid(plain(2), &a, &b)).unwrap();
    check_grid_inside(&grid, 10, &a, &b);
    let (a, b) = ([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]);
    let grid = within_10s(move || box_grid(plain(3), &a, &b)).unwrap();
    check_grid_inside(&grid, 18, &a, &b);

    // A Box so much longer than it is wide that it would need more points
    // than a PointSet can hold.
    expect_err(
        within_10s(|| box_grid(plain(2), &[0.0, 0.0], &[1.0e12, 1.0e-12])),
        ErrorCode::Attin,
        &[
            "astFrameGrid(Frame): The supplied bounds and grid size (30) need a grid of \
           more points than can be created.",
        ],
    );

    // Likewise for a CmpFrame whose two components' grids can each be held
    // but not their product.
    expect_err(
        within_10s(|| {
            let cmp = CmpFrame::new(Box::new(plain(2)), Box::new(plain(2)));
            box_grid(cmp, &[0.0; 4], &[1.0e5, 1.0e-5, 1.0e5, 1.0e-5])
        }),
        ErrorCode::Attin,
        &[
            "astFrameGrid(CmpFrame): The supplied bounds and grid size (30) need a grid of \
           more points than can be created.",
        ],
    );

    // A sky Box with no width is treated as a very narrow one is: no
    // parallel gets a sample.
    for width in [1.0e-7, 0.0] {
        expect_err(
            within_10s(move || {
                box_grid(SkyFrame::unset_system(), &[0.5, -0.1], &[0.5 + width, 0.1])
            }),
            ErrorCode::Nptin,
            &[
                "astSetNpoint(PointSet): Number of points (0) is not valid.",
                "Should be in the range 1 to 60.",
            ],
        );
    }

    // A Box with no width on its last axis, in a Frame whose axes have
    // different Units.
    let mut frame = plain(2);
    frame.set_unit(1, "m").unwrap();
    frame.set_unit(2, "s").unwrap();
    let (a, b) = ([0.0, 0.0], [1.0, 0.0]);
    check_grid_inside(&box_grid(frame, &a, &b).unwrap(), 10, &a, &b);

    // A 3-D Box with no width on axis 3, in a CmpFrame.
    let cmp = CmpFrame::new(Box::new(plain(2)), Box::new(plain(1)));
    let (a, b) = ([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]);
    check_grid_inside(&box_grid(cmp, &a, &b).unwrap(), 18, &a, &b);

    // A sky Box with no height.
    let grid = box_grid(SkyFrame::unset_system(), &[0.5, 0.1], &[0.6, 0.1]).unwrap();
    assert_eq!(grid.npoint(), 28);
}

/// The values the attribute cases give a Title: a "%", a comma and an equals
/// sign, quotes, and white space at each end.
const TITLES: [&str; 6] = [
    "50%s done, 100%n to do",
    "Flux, units=Jy",
    "Sky, Domain=NEW",
    "\"\"quoted\"\"",
    "\"open, a=b",
    "  padded  ",
];

/// A Box in a Frame whose Domain is OLD, with Adaptive set as given.
fn old_box(adaptive: bool) -> starlink_ast::regions::BoxRegion {
    let frame = BasicFrameBuilder::new(2).domain("OLD").build().unwrap();
    let mut bx = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    bx.set_adaptive(adaptive);
    bx
}

/// A Region passes a Frame attribute it is given on to its FrameSet, and
/// must store the value a Frame stores and change nothing else. The C test
/// sets them as attribute strings, which `REGION_HARDENING_EXCLUDED` holds;
/// here they are set through the typed setters.
#[test]
fn attributes() {
    for title in TITLES {
        let mut frame = BasicFrameBuilder::new(2).domain("OLD").build().unwrap();
        Frame::set_title(&mut frame, title).unwrap();
        let wanted = Frame::title(&frame).unwrap();
        for adaptive in [false, true] {
            let mut bx = old_box(adaptive);
            bx.set_title(title).unwrap();
            assert_eq!(bx.title().unwrap(), wanted, "Adaptive={adaptive}");
            assert_eq!(bx.domain(), "OLD", "Adaptive={adaptive}, Title {title}");
        }
    }

    // A Format is the value most likely to hold a "%".
    for adaptive in [false, true] {
        let mut bx = old_box(adaptive);
        bx.set_format_str(1, "%10.3f").unwrap();
        assert_eq!(bx.format_str(1).unwrap(), "%10.3f");
    }
}

/// Whether the attribute cases, set and read as attribute strings as the C
/// test sets them, give a Box the values they give a Frame.
fn attribute_strings_match_c() -> bool {
    use starlink_ast::attributes::Attributes;
    for title in TITLES {
        let mut frame = BasicFrameBuilder::new(2).domain("OLD").build().unwrap();
        frame.set_attribute("Title", title).unwrap();
        let wanted = frame.get_attribute("Title").unwrap();
        for adaptive in [false, true] {
            let mut bx = old_box(adaptive);
            if bx.set_attribute("Title", title).is_err()
                || bx.get_attribute("Title").ok() != Some(wanted.clone())
                || bx.get_attribute("Domain").ok().as_deref() != Some("OLD")
            {
                return false;
            }
        }
    }
    [false, true].into_iter().all(|adaptive| {
        let mut bx = old_box(adaptive);
        bx.set_attributes("Format(1)=%10.3f").is_ok()
            && bx.get_attribute("Format(1)").ok().as_deref() == Some("%10.3f")
    })
}

/// Mask the pixels 1 to 10 with `region`, and return the number changed and
/// which were changed.
fn mask_ten(region: &dyn Region, inside: bool) -> (usize, String) {
    let mut d = [0.0f64; 10];
    let n = mask(region, None, inside, &[1], &[10], &mut d, 9.0).unwrap();
    let got = d
        .iter()
        .map(|&v| if v == 9.0 { '1' } else { '0' })
        .collect();
    (n, got)
}

#[test]
fn masks() {
    // The upper bound of a Region with no upper limit is too large for an
    // integer pixel index, which must not make every pixel outside it.
    let above = IntervalBuilder::new(plain(1), vec![3.0], vec![BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        within_10s(move || mask_ten(&above, false)),
        (2, "1100000000".to_string())
    );
    let above = IntervalBuilder::new(plain(1), vec![3.0], vec![BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        within_10s(move || mask_ten(&above, true)),
        (7, "0001111111".to_string())
    );

    // A PointList with the wrong number of grid dimensions.
    let mut d = [0i32; 6];
    expect_err(
        mask(&two_points(), None, true, &[1], &[6], &mut d, 1),
        ErrorCode::Ngdin,
        &[
            "astMaskI(PointList): Bad number of input grid dimensions (1).",
            "The PointList given requires 2 coordinate values to specify an input position.",
        ],
    );
    assert_eq!(d, [0; 6]);

    // A 64-bit integer array is masked by C's astMaskL.
    let mut d = [0i64; 6];
    expect_err(
        mask(&two_points(), None, true, &[1], &[6], &mut d, 1),
        ErrorCode::Ngdin,
        &[
            "astMaskL(PointList): Bad number of input grid dimensions (1).",
            "The PointList given requires 2 coordinate values to specify an input position.",
        ],
    );

    // A pixel on the edge of a closed Region is in neither the Region nor
    // its negation.
    let below = IntervalBuilder::new(plain(1), vec![BAD_VALUE], vec![7.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(mask_ten(&below, false), (3, "0000000111".to_string()));
    assert_eq!(mask_ten(&below, true), (6, "1111110000".to_string()));
}

fn two_points() -> impl Region {
    PointListBuilder::new(plain(2), vec![vec![1.0, 2.0], vec![3.0, 4.0]])
        .unwrap()
        .build()
        .unwrap()
}

fn box_0_5() -> impl Region {
    BoxRegionBuilder::from_bounds(plain(2), &[0.0, 0.0], &[5.0, 5.0])
        .unwrap()
        .build()
        .unwrap()
}

fn interval(naxes: usize, lo: &[f64], hi: &[f64]) -> impl Region {
    IntervalBuilder::new(plain(naxes), lo.to_vec(), hi.to_vec())
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn overlaps() {
    // A Region with one base Frame axis and two current Frame axes and no
    // boundary mesh: a half-line mapped into two dimensions by a MathMap,
    // so that it is not simplified. A position is inside it if the inverse
    // puts it in the half-line, whatever its second coordinate.
    let half_line = || {
        let iv = interval(1, &[0.0], &[BAD_VALUE]);
        let map = MathMap::new(1, 2, &["a = x", "b = 0*x"], &["x = a"]).unwrap();
        iv.map_region(&map, &plain(2)).unwrap()
    };
    let in_box = || {
        BoxRegionBuilder::from_bounds(plain(2), &[1.0, -1.0], &[2.0, 1.0])
            .unwrap()
            .build()
            .unwrap()
    };
    let got = within_10s(move || overlap_code(half_line().as_ref(), &in_box()));
    assert_eq!(got, 3, "a half-line mapped into two dimensions and a Box");
    let got = within_10s(move || overlap_code(&in_box(), half_line().as_ref()));
    assert_eq!(got, 2, "a Box and a half-line mapped into two dimensions");

    // Two 2-D Intervals, one the negation of the other on axis 1: each is
    // the other's negation when axis 2 has no limit, and they do not overlap
    // when it has one.
    let got = within_10s(|| {
        overlap_code(
            &interval(2, &[2.0, BAD_VALUE], &[1.0, BAD_VALUE]),
            &interval(2, &[1.0, BAD_VALUE], &[2.0, BAD_VALUE]),
        )
    });
    assert_eq!(got, 6, "no limit on axis 2");
    let got = within_10s(|| {
        overlap_code(
            &interval(2, &[2.0, 1.0], &[1.0, 0.0]),
            &interval(2, &[1.0, 0.0], &[2.0, 1.0]),
        )
    });
    assert_eq!(got, 1, "the same range on axis 2");

    // A Prism has no boundary mesh if one of its components has no limit on
    // one side, and the part of its boundary within a box cannot then be
    // found. It used to ask for the same mesh again, without end.
    let no_mesh_prism = || {
        let unit_box = BoxRegionBuilder::from_bounds(plain(2), &[0.0, 0.0], &[1.0, 1.0])
            .unwrap()
            .build()
            .unwrap();
        PrismBuilder::new(interval(1, &[0.0], &[BAD_VALUE]), unit_box)
            .unwrap()
            .build()
            .unwrap()
    };
    let ball = || {
        CircleBuilder::new(plain(3), vec![0.5, 0.5, 0.5])
            .unwrap()
            .radius(0.3)
            .build()
            .unwrap()
    };
    let message = "astOverlap(Prism): The current implementation of astOverlap cannot \
                   determine the overlap between the supplied Regions (a Prism and a Circle).";
    expect_err(
        within_10s(move || ball().overlap(&no_mesh_prism())),
        ErrorCode::Inter,
        &[message],
    );
    expect_err(
        within_10s(move || no_mesh_prism().overlap(&ball())),
        ErrorCode::Inter,
        &[message],
    );

    // A PointList and a Box that contains it.
    assert_eq!(overlap_code(&two_points(), &box_0_5()), 2);

    // Two PointLists that hold the same positions, one of them twice.
    let three = PointListBuilder::new(
        plain(2),
        vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![1.0, 2.0]],
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(overlap_code(&two_points(), &three), 5);

    // Two CmpRegions that each hold a PointList.
    let and1 = CmpRegionBuilder::new(two_points(), box_0_5(), CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    let and2 = CmpRegionBuilder::new(two_points(), box_0_5(), CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(overlap_code(&and1, &and2), 5);

    // Two 1-D Intervals, one the negation of the other.
    assert_eq!(
        overlap_code(&interval(1, &[2.0], &[1.0]), &interval(1, &[1.0], &[2.0])),
        6
    );
}

/// The PermMap that keeps the first of two axes and gives the second the
/// value 0 on the way back.
fn first_of_two() -> PermMap {
    PermMap::new(&[1, -1], &[1], &[0.0]).unwrap()
}

#[test]
fn axes() {
    // A PointList with two base Frame axes and one current Frame axis.
    let mapped = two_points().map_region(&first_of_two(), &plain(1)).unwrap();
    let (lbnd, ubnd) = mapped.get_region_bounds().unwrap();
    assert_eq!((lbnd, ubnd), (vec![1.0], vec![3.0]));

    // A NullRegion likewise: its bounds are the empty interval.
    let null = NullRegionBuilder::new(plain(2)).build().unwrap();
    let mapped = null.map_region(&first_of_two(), &plain(1)).unwrap();
    let (lbnd, ubnd) = mapped.get_region_bounds().unwrap();
    assert_eq!((lbnd.len(), ubnd.len()), (1, 1));
    assert!(lbnd[0] > ubnd[0], "bounds {lbnd:?} to {ubnd:?}");

    // A Prism with three base Frame axes and four current Frame axes.
    let mapped = cylinder()
        .map_region(
            &PermMap::new(&[1, 2, 3], &[1, 2, 3, -1], &[0.0]).unwrap(),
            &plain(4),
        )
        .unwrap();
    let mesh = mapped.region_mesh().unwrap();
    assert!(mesh.npoint() >= 1);
    assert_eq!(mesh.ncoord(), 4);
}

/// A Prism made from a Circle of radius 5 and the Interval 2 to 8.
pub(crate) fn cylinder() -> impl Region {
    let circle = CircleBuilder::new(plain(2), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    PrismBuilder::new(circle, interval(1, &[2.0], &[8.0]))
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn ellipses() {
    // An Ellipse used as the uncertainty of a Box is moved to the centre of
    // the Box and keeps its size, whether it starts at the origin or is
    // already there.
    for c in [0.0, 5.0] {
        let ellipse = EllipseBuilder::new(plain(2), vec![c, c], [0.2, 0.1], 0.3)
            .unwrap()
            .build()
            .unwrap();
        let (elbnd, eubnd) = ellipse.get_region_bounds().unwrap();
        let bx = BoxRegionBuilder::from_bounds(plain(2), &[0.0, 0.0], &[10.0, 10.0])
            .unwrap()
            .uncertainty(Box::new(ellipse))
            .build()
            .unwrap();
        let unc = bx.get_unc(false).unwrap().expect("an uncertainty");
        let (lbnd, ubnd) = unc
            .get_region_bounds()
            .unwrap_or_else(|e| panic!("centre {c}: {e}"));
        for i in 0..2 {
            assert!(
                (0.5 * (lbnd[i] + ubnd[i]) - 5.0).abs() <= 1.0e-6,
                "centre {c}"
            );
            assert!(
                ((ubnd[i] - lbnd[i]) - (eubnd[i] - elbnd[i])).abs() <= 1.0e-6,
                "centre {c}"
            );
        }
    }

    // Three points in a line do not define an ellipse, which is reported
    // each time the Ellipse is used.
    let line =
        EllipseBuilder::from_points(plain(2), vec![0.0, 0.0], vec![2.0, 0.0], vec![1.0, 0.0])
            .unwrap()
            .build()
            .unwrap();
    let first = line.tran2(&[0.5], &[0.1], true).unwrap_err();
    let second = line.tran2(&[0.5], &[0.1], true).unwrap_err();
    assert_eq!(first.code(), ErrorCode::Badin);
    assert_eq!(second.code(), ErrorCode::Badin);
    assert_eq!(first.message(), second.message());

    // A Circle mapped so that no point of its boundary is away from the new
    // centre is still mapped.
    let circle = CircleBuilder::new(plain(2), vec![1.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let map = MathMap::new(
        2,
        2,
        &["a = qif( x > 0, 5, <bad> )", "b = qif( x > 0, 7, <bad> )"],
        &["x = a", "y = b"],
    )
    .unwrap();
    circle.map_region(&map, &plain(2)).unwrap();
}

/// The positions each Polygon case transforms: a vertex of the first, a
/// far position, a vertex of the other two, a position outside them and one
/// inside the last two.
const POLY_X: [f64; 5] = [1.0, 50.0, 0.0, -1.0, 0.1];
const POLY_Y: [f64; 5] = [2.0, 50.0, 0.0, -1.0, 0.1];

/// Make a Polygon from `vertices` and return whether it is bounded, its
/// bounds, which of `POLY_X`, `POLY_Y` it keeps (each kept position is
/// unchanged) and its overlap with itself.
fn polygon_case(vertices: Vec<Vec<f64>>) -> (bool, (Vec<f64>, Vec<f64>), String, Overlap) {
    let polygon = PolygonBuilder::new(plain(2), vertices)
        .unwrap()
        .build()
        .unwrap();
    let (xo, yo) = polygon.tran2(&POLY_X, &POLY_Y, true).unwrap();
    let mut kept = String::new();
    for i in 0..POLY_X.len() {
        if xo[i] == BAD_VALUE && yo[i] == BAD_VALUE {
            kept.push('0');
        } else {
            assert_eq!((xo[i], yo[i]), (POLY_X[i], POLY_Y[i]));
            kept.push('1');
        }
    }
    (
        polygon.is_bounded().unwrap(),
        polygon.get_region_bounds().unwrap(),
        kept,
        polygon.overlap(&polygon).unwrap(),
    )
}

#[test]
fn polygons() {
    // Vertices that all coincide: the first vertex is the position inside.
    let got = within_10s(|| polygon_case(vec![vec![1.0, 2.0]; 3]));
    assert_eq!(
        got,
        (
            true,
            (vec![1.0, 2.0], vec![1.0, 2.0]),
            "00000".to_string(),
            Overlap::Same
        )
    );

    // Vertices so far apart that ten times half an edge is not finite. C
    // keeps every position, (-1, -1) outside the triangle too, as the
    // length of the long edge overflows (quirk "A Polygon with vertices
    // near 1e308 contains positions outside it").
    let got =
        within_10s(|| polygon_case(vec![vec![0.0, 0.0], vec![1.0e308, 0.0], vec![0.0, 1.0e308]]));
    assert_eq!(
        got,
        (
            true,
            (vec![0.0, 0.0], vec![1.0e308, 1.0e308]),
            "11111".to_string(),
            Overlap::Same
        )
    );

    // A bad or NaN vertex is refused when the Polygon is made.
    for bad in [BAD_VALUE, f64::NAN] {
        expect_err(
            PolygonBuilder::new(
                plain(2),
                vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![bad, 1.0]],
            ),
            ErrorCode::Badin,
            &[
                "astInitPolygon(Polygon): One or more bad axis values supplied for the vertex \
               number 3.",
            ],
        );
    }

    // A vertex at infinity. The C library takes an infinite coordinate to
    // be a number, and puts (0.1, 0.1) outside, though it is inside the
    // triangle (quirk "A Polygon with a vertex at infinity leaves out
    // positions inside it").
    let got = within_10s(|| polygon_case(infinite_vertex()));
    assert_eq!(
        got,
        (
            true,
            (vec![0.0, 0.0], vec![f64::INFINITY, 1.0]),
            "00100".to_string(),
            Overlap::Same
        )
    );
}

/// The vertices of a Polygon with a vertex at infinity.
fn infinite_vertex() -> Vec<Vec<f64>> {
    vec![vec![0.0, 0.0], vec![f64::INFINITY, 0.0], vec![0.0, 1.0]]
}

/// Cases whose result is not yet the C library's, each with the reason.
const REGION_HARDENING_EXCLUDED: &[(&str, &str)] = &[(
    "attribute_strings",
    "the attribute-strings layer has no Region classes, so a Box refuses a \
         setting with AST__BADAT; the typed setters are tested in `attributes`",
)];

/// Whether the excluded case `name` now gives the C library's result.
fn excluded_case_matches_c(name: &str) -> bool {
    match name {
        "attribute_strings" => attribute_strings_match_c(),
        _ => panic!("no case named {name}"),
    }
}

/// An excluded case that starts matching C must leave the exclusion list.
#[test]
fn excluded_cases_still_differ() {
    for (name, reason) in REGION_HARDENING_EXCLUDED {
        assert!(
            !excluded_case_matches_c(name),
            "{name} now matches C; remove it from REGION_HARDENING_EXCLUDED ({reason})"
        );
    }
}

#[test]
fn fewer_axes() {
    let regions: Vec<(&str, Box<dyn Region>)> = vec![
        (
            "Box",
            Box::new(
                BoxRegionBuilder::from_bounds(plain(2), &[1.5, 2.5], &[3.5, 4.5])
                    .unwrap()
                    .build()
                    .unwrap(),
            ),
        ),
        (
            "Circle",
            Box::new(
                CircleBuilder::new(plain(2), vec![3.0, 3.5])
                    .unwrap()
                    .radius(1.6)
                    .build()
                    .unwrap(),
            ),
        ),
        (
            "PointList",
            Box::new(
                PointListBuilder::new(
                    plain(2),
                    vec![vec![2.0, 1.0], vec![3.0, 5.0], vec![4.0, 9.0]],
                )
                .unwrap()
                .build()
                .unwrap(),
            ),
        ),
    ];
    let counts = [4, 6, 3];
    let outside = ["100111", "111111", "100011"];

    // The Mapping keeps the first axis, and its inverse gives the second
    // axis no value.
    let map = PermMap::new(&[1, 0], &[1], &[]).unwrap();
    let copy = map.clone();
    for (i, (name, region)) in regions.iter().enumerate() {
        region
            .map_region(&map, &plain(1))
            .unwrap_or_else(|e| panic!("a {name} could not be mapped: {e}"));
        assert!(map.equal(&copy), "mapping a {name} changed the Mapping");

        // Masking with the same Mapping maps the Region in the same way.
        let mut d = [0i32; 6];
        let n = mask(region.as_ref(), Some(&map), false, &[1], &[6], &mut d, 1).unwrap();
        let got: String = d.iter().map(|&v| if v != 0 { '1' } else { '0' }).collect();
        assert_eq!(
            (n, got.as_str()),
            (counts[i], outside[i]),
            "masking outside a {name}"
        );
        assert!(
            map.equal(&copy),
            "masking with a {name} changed the Mapping"
        );
    }
}
