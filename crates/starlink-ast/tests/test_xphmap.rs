// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! XphMap, the Mapping between grid coordinates in different HEALPix
//! projections.
//!
//! Ported from `reference/starlink-ast/ast_tester/testxphmap.c`. XphMap has
//! no public constructor in C or here, so every XphMap is read from a dump,
//! as the C test reads them. The C test's leak check has no counterpart: Rust
//! frees the projection name it reads.
//!
//! `xphmap_oracle/cases.rs`, from `tools/oracle_fixtures/xphmap_probe.c`,
//! holds C's forward and inverse transformations of each projection type
//! over a half-pixel grid, compared bit for bit.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "xphmap_oracle/cases.rs"]
mod cases;

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::error::ErrorCode;
use starlink_ast::mappings::{Mapping, XphMap};
use starlink_ast::{AnyObject, from_native_string};

fn dump(order: i64, projection: &str) -> String {
    format!(
        " Begin XphMap\n    Nin = 2\n IsA Mapping\n    Order = {order}\n    \
         Type = \"{projection}\"\n End XphMap\n"
    )
}

fn load(order: i64, projection: &str) -> XphMap {
    match from_native_string(&dump(order, projection)).unwrap() {
        AnyObject::XphMap(m) => m,
        other => panic!("loaded {other:?}, not an XphMap"),
    }
}

/// testxphmap.c `check_type`: each projection name loads as an XphMap of two
/// inputs and outputs, a re-dump keeps its Order and Type, and two loads of
/// the same dump are equal.
#[test]
fn every_projection_type_loads_and_round_trips() {
    for (projection, order) in [("HPX0", 14), ("HPX12", 8), ("XPHN", 10), ("XPHS", 10)] {
        let map = load(order, projection);
        assert_eq!((map.nin(), map.nout()), (2, 2), "{projection}");
        let text = native_dump(&AnyObject::XphMap(map.clone())).unwrap();
        assert!(
            text.contains(&format!("Type = \"{projection}\"")),
            "{projection}: {text}"
        );
        assert!(
            text.contains(&format!("Order = {order}")),
            "{projection}: {text}"
        );
        assert!(load(order, projection).equal(&map), "{projection}");
    }
}

/// testxphmap.c `check_transform`: an HPX12 XphMap's inverse recovers every
/// point its forward transformation accepts, over a scan of the projection,
/// or the used copy of a point in an unused copy of a cell, four facets away
/// on both axes.
#[test]
fn hpx12_round_trips_every_good_point() {
    let map = load(8, "HPX12");
    let lim = 5.0 * f64::from(1 << 8);
    let (mut xin, mut yin) = (Vec::new(), Vec::new());
    let mut x = 0.0;
    while x <= lim {
        let mut y = 0.0;
        while y <= lim {
            xin.push(x);
            yin.push(y);
            y += 37.0;
        }
        x += 37.0;
    }
    let (xm, ym) = map.tran2(&xin, &yin, true).unwrap();
    let (xout, yout) = map.tran2(&xm, &ym, false).unwrap();
    let mut ngood = 0;
    for i in 0..xin.len() {
        if xm[i] == BAD_VALUE || ym[i] == BAD_VALUE {
            continue;
        }
        ngood += 1;
        assert!(
            xout[i] != BAD_VALUE && yout[i] != BAD_VALUE,
            "({}, {}) transformed but did not come back",
            xin[i],
            yin[i]
        );
        let mut shift = xout[i] - xin[i];
        if shift.abs() != 4.0 * f64::from(1 << 8) {
            shift = 0.0;
        }
        assert!(
            (xout[i] - xin[i] - shift).abs() <= 1.0e-8
                && (yout[i] - yin[i] - shift).abs() <= 1.0e-8,
            "({}, {}) round tripped to ({}, {})",
            xin[i],
            yin[i],
            xout[i],
            yout[i]
        );
    }
    assert!(
        ngood >= 20,
        "only {ngood} of {} scanned points were on the sphere",
        xin.len()
    );
}

/// testxphmap.c `check_bad_type`: a Type that is not a projection name is
/// refused, with C's AST__OPT.
#[test]
fn an_unknown_type_is_refused() {
    let err = from_native_string(&dump(8, "NOSUCHPROJECTION")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Opt);
}

/// Each projection type's forward and inverse transformations against C's,
/// bit for bit, over the probe's half-pixel grid.
#[test]
fn transformations_match_the_c_library_bit_for_bit() {
    let mut xin = Vec::new();
    let mut yin = Vec::new();
    for i in 0..cases::STEPS {
        for j in 0..cases::STEPS {
            xin.push(-1.0 + 0.5 * i as f64);
            yin.push(-1.0 + 0.5 * j as f64);
        }
    }
    let mut failures = Vec::new();
    for &(projection, forward, want_x, want_y) in cases::CASES {
        let map = load(cases::ORDER, projection);
        let (x, y) = map.tran2(&xin, &yin, forward).unwrap();
        for i in 0..xin.len() {
            if x[i].to_bits() != want_x[i].to_bits() || y[i].to_bits() != want_y[i].to_bits() {
                failures.push(format!(
                    "{projection} {}: ({}, {}) gave ({}, {}), C ({}, {})",
                    if forward { "forward" } else { "inverse" },
                    xin[i],
                    yin[i],
                    x[i],
                    y[i],
                    want_x[i],
                    want_y[i]
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// xphmap.c `GetIsLinear`: only an HPX12 XphMap is linear.
#[test]
fn only_hpx12_is_linear() {
    for (projection, linear) in [
        ("HPX0", false),
        ("HPX12", true),
        ("XPHN", false),
        ("XPHS", false),
    ] {
        assert_eq!(load(4, projection).is_linear(), linear, "{projection}");
    }
}

/// A NaN or infinite input is bad, as for every Mapping (a NaN input counts
/// as bad), in both directions and for every projection; it is not taken as
/// a position in a cell.
#[test]
fn a_non_finite_input_gives_bad_outputs() {
    use starlink_ast::coords::PointSet;
    for projection in ["HPX0", "HPX12", "XPHN", "XPHS"] {
        let map = load(3, projection);
        for forward in [true, false] {
            for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                for pt in [[v, 5.0], [5.0, v]] {
                    let out = map.transform(&PointSet::from_rows([pt]), forward).unwrap();
                    assert_eq!(
                        out.point(0).to_vec(),
                        vec![BAD_VALUE, BAD_VALUE],
                        "{projection} forward={forward} input {pt:?}"
                    );
                }
            }
        }
    }
}

const TYPES: [&str; 4] = ["HPX0", "HPX12", "XPHN", "XPHS"];

/// testxphmap.c `check_coverage`: of the pixel centers of a projection's
/// grid, those an XphMap transforms number the pixels on the sky, and each
/// comes back through the opposite transformation.
/// An HPX grid also holds an unused copy of each pixel of the facet split
/// between its two ends; both directions accept it, and it comes back as the
/// used copy, four facets away on both axes.
#[test]
fn each_pixel_appears_once_in_each_projection() {
    for projection in TYPES {
        for order in 0..=3 {
            let map = load(order, projection);
            let nppf = 1usize << order;
            for forward in [true, false] {
                let ncell = if forward && projection.starts_with('X') {
                    4 * nppf
                } else {
                    5 * nppf
                };
                let (mut xin, mut yin) = (Vec::new(), Vec::new());
                for iy in 1..=ncell {
                    for ix in 1..=ncell {
                        xin.push(ix as f64);
                        yin.push(iy as f64);
                    }
                }
                let (xm, ym) = map.tran2(&xin, &yin, forward).unwrap();
                let (xout, yout) = map.tran2(&xm, &ym, !forward).unwrap();
                let (mut ngood, mut ncopy) = (0, 0);
                for i in 0..xin.len() {
                    if xm[i] == BAD_VALUE || ym[i] == BAD_VALUE {
                        continue;
                    }
                    ngood += 1;
                    if xout[i] == xin[i] && yout[i] == yin[i] {
                        continue;
                    }
                    if (!forward || projection.starts_with('H'))
                        && (xout[i] - xin[i]).abs() == (4 * nppf) as f64
                        && yout[i] - yin[i] == xout[i] - xin[i]
                    {
                        ncopy += 1;
                        continue;
                    }
                    panic!(
                        "{projection} order {order} forward {forward}: ({}, {}) came back as \
                         ({}, {}); a pixel center did not make the return trip",
                        xin[i], yin[i], xout[i], yout[i]
                    );
                }
                assert_eq!(
                    ngood - ncopy,
                    12 * nppf * nppf,
                    "{projection} order {order} forward {forward}"
                );
                let want_copies = if forward && projection.starts_with('X') {
                    0
                } else {
                    nppf * nppf
                };
                assert_eq!(
                    ncopy, want_copies,
                    "{projection} order {order} forward {forward}"
                );
            }
        }
    }
}

/// testxphmap.c `check_far_positions`: positions too far from the grid for
/// an integer to index, and positions that are not finite, give bad values in
/// both directions; each far value is tried on both axes, with every other
/// far value and with a position inside the grid on the other axis.
/// The forward transformation of an XPH grid tests the grid's edge as well
/// as the sides of a gore, so it is no exception.
/// Run with overflow checks, as `cargo test` builds, this is the Rust
/// counterpart of the C test's run under UndefinedBehaviorSanitizer.
#[test]
fn far_positions_give_bad_values() {
    let far = [
        1e10,
        -1e10,
        1e300,
        -1e300,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    let (mut xin, mut yin) = (Vec::new(), Vec::new());
    for &b in &far {
        for &a in &far {
            xin.push(a);
            yin.push(b);
        }
        xin.extend([b, 3.0]);
        yin.extend([3.0, b]);
    }
    for projection in TYPES {
        for order in [0, 3, 27] {
            let map = load(order, projection);
            for forward in [true, false] {
                let (xout, yout) = map.tran2(&xin, &yin, forward).unwrap();
                for i in 0..xin.len() {
                    assert!(
                        xout[i] == BAD_VALUE && yout[i] == BAD_VALUE,
                        "{projection} order {order} forward {forward}: ({}, {}) gave ({}, {})",
                        xin[i],
                        yin[i],
                        xout[i],
                        yout[i]
                    );
                }
            }
        }
    }
}

/// testxphmap.c `check_outside_grid`: every position outside the grid of
/// the projection being transformed gives bad values, however near.
/// The scan is a lattice a quarter of a pixel apart over the grid and a
/// margin wider than the grid on every side, so it crosses the line of each
/// gore of an XPH grid beyond the corner where the gore ends.
/// The edges of the grid are part of it.
#[test]
fn positions_outside_the_grid_are_bad() {
    for projection in TYPES {
        for forward in [true, false] {
            for order in 0..=3 {
                let map = load(order, projection);
                let nppf = f64::from(1u32 << order);
                let nfacet = if forward && projection.starts_with('X') {
                    4.0
                } else {
                    5.0
                };
                let lo = 0.5;
                let hi = nfacet * nppf + 0.5;
                let margin = 5.0 * nppf;
                let n = ((hi - lo + 2.0 * margin) * 4.0) as usize + 1;
                let (mut ninside, mut noutside) = (0, 0);
                for j in 0..n {
                    let xin: Vec<f64> = (0..n).map(|i| lo - margin + 0.25 * i as f64).collect();
                    let yin = vec![lo - margin + 0.25 * j as f64; n];
                    let (xout, yout) = map.tran2(&xin, &yin, forward).unwrap();
                    for i in 0..n {
                        if xin[i] >= lo && xin[i] <= hi && yin[i] >= lo && yin[i] <= hi {
                            if xout[i] != BAD_VALUE && yout[i] != BAD_VALUE {
                                ninside += 1;
                            }
                        } else {
                            noutside += 1;
                            assert!(
                                xout[i] == BAD_VALUE && yout[i] == BAD_VALUE,
                                "{projection} order {order} forward {forward}: ({}, {}) is \
                                 outside the grid but gave ({}, {})",
                                xin[i],
                                yin[i],
                                xout[i],
                                yout[i]
                            );
                        }
                    }
                }
                assert!(
                    ninside >= 12 && noutside >= 12,
                    "{projection} order {order} forward {forward}: the scan transformed \
                     {ninside} positions inside the grid and tried {noutside} outside it"
                );
            }
        }
    }
}

/// testxphmap.c `check_boundaries`: a position on the boundary of a used
/// cell continues the transformation of one of the used cells it bounds, and
/// a position that bounds no used cell gives bad values.
/// The scan is a lattice a quarter of a pixel apart over the grid and a
/// margin of one pixel, so it holds the center of each cell, positions a
/// quarter of a pixel from the center along each axis, and the corners and
/// edge centers.
/// A cell is used if its center is transformed, and the positions within it
/// give the shift and rotation it is transformed by.
/// Every value is a multiple of a quarter, so the comparison is exact.
#[test]
fn cell_boundaries_continue_a_used_cell() {
    for projection in TYPES {
        for forward in [true, false] {
            for order in 0..=2 {
                let map = load(order, projection);
                let nppf: i64 = 1 << order;
                let ncell: i64 = if forward && projection.starts_with('X') {
                    4 * nppf
                } else {
                    5 * nppf
                };
                let nlat: i64 = 4 * (ncell + 2) + 1;
                // Lattice index `a` is at grid coordinate `0.25 * a - 0.5`,
                // so the center of the cell with one-based index `k` is at
                // lattice index `4 * k + 2` and its edges two steps either side.
                let (mut xin, mut yin) = (Vec::new(), Vec::new());
                for b in 0..nlat {
                    for a in 0..nlat {
                        xin.push(0.25 * a as f64 - 0.5);
                        yin.push(0.25 * b as f64 - 0.5);
                    }
                }
                let (xout, yout) = map.tran2(&xin, &yin, forward).unwrap();
                let cells = |a: i64| -> Vec<i64> {
                    if a % 4 == 2 {
                        vec![(a - 2) / 4]
                    } else {
                        vec![a / 4 - 1, a / 4]
                    }
                };
                let (mut nedge, mut nbound) = (0, 0);
                for b in (0..nlat).step_by(2) {
                    for a in (0..nlat).step_by(2) {
                        let c = (b * nlat + a) as usize;
                        let (kx, ky) = (cells(a), cells(b));
                        if kx.len() * ky.len() == 1 {
                            continue;
                        }
                        nedge += 1;
                        let (mut nused, mut matched) = (0, false);
                        for &jy in &ky {
                            for &jx in &kx {
                                if jx < 1 || jx > ncell || jy < 1 || jy > ncell {
                                    continue;
                                }
                                let (ca, cb) = (4 * jx + 2, 4 * jy + 2);
                                let k = (cb * nlat + ca) as usize;
                                if xout[k] == BAD_VALUE {
                                    continue;
                                }
                                nused += 1;
                                let nl = nlat as usize;
                                assert!(
                                    xout[k + 1] != BAD_VALUE && xout[k + nl] != BAD_VALUE,
                                    "{projection} order {order} forward {forward}: the cell at \
                                     ({}, {}) is not transformed as a whole",
                                    xin[k],
                                    yin[k]
                                );
                                let wantx = xout[k]
                                    + (a - ca) as f64 * (xout[k + 1] - xout[k])
                                    + (b - cb) as f64 * (xout[k + nl] - xout[k]);
                                let wanty = yout[k]
                                    + (a - ca) as f64 * (yout[k + 1] - yout[k])
                                    + (b - cb) as f64 * (yout[k + nl] - yout[k]);
                                if xout[c] == wantx && yout[c] == wanty {
                                    matched = true;
                                }
                            }
                        }
                        if nused > 0 {
                            nbound += 1;
                            assert!(
                                matched,
                                "{projection} order {order} forward {forward}: ({}, {}) is on \
                                 the boundary of a used cell but gave ({}, {})",
                                xin[c], yin[c], xout[c], yout[c]
                            );
                        } else {
                            assert!(
                                xout[c] == BAD_VALUE && yout[c] == BAD_VALUE,
                                "{projection} order {order} forward {forward}: ({}, {}) bounds \
                                 no used cell but gave ({}, {})",
                                xin[c],
                                yin[c],
                                xout[c],
                                yout[c]
                            );
                        }
                    }
                }
                assert!(
                    nbound >= 12 && nedge - nbound >= 12,
                    "{projection} order {order} forward {forward}: {nbound} of {nedge} boundary \
                     positions bound a used cell"
                );
            }
        }
    }
}

/// testxphmap.c `check_poles`: a pole is one point on the sky.
/// In an HPX grid the north pole is the top left corner of each of the four
/// north polar facets and the south pole the bottom right corner of each of
/// the four south polar facets; in an XPH grid the pole the projection is
/// centered on is the center of the grid and the other pole its four corners.
/// Each is transformed, and to a position of the same pole.
#[test]
fn every_pole_transforms_to_the_same_pole() {
    for order in [0, 1, 3, 27] {
        let n = f64::from(1u32 << order);
        let ntip: Vec<(f64, f64)> = (0..4)
            .map(|k| (f64::from(k) * n + 0.5, f64::from(k + 2) * n + 0.5))
            .collect();
        let stip: Vec<(f64, f64)> = (0..4)
            .map(|k| (f64::from(k + 2) * n + 0.5, f64::from(k) * n + 0.5))
            .collect();
        let corner: Vec<(f64, f64)> = (0..4)
            .map(|k| {
                (
                    if k % 2 == 1 { 4.0 * n + 0.5 } else { 0.5 },
                    if k / 2 == 1 { 4.0 * n + 0.5 } else { 0.5 },
                )
            })
            .collect();
        let center = vec![(2.0 * n + 0.5, 2.0 * n + 0.5); 4];
        for projection in TYPES {
            let map = load(order, projection);
            let (north, south) = match projection {
                "XPHN" => (&center, &corner),
                "XPHS" => (&corner, &center),
                _ => (&ntip, &stip),
            };
            let check_one_of = |forward: bool, what: &str, p: (f64, f64), set: &[(f64, f64)]| {
                let (xo, yo) = map.tran2(&[p.0], &[p.1], forward).unwrap();
                assert!(
                    set.iter().any(|&(x, y)| xo[0] == x && yo[0] == y),
                    "{projection} order {order} {what}: ({}, {}) gave ({}, {})",
                    p.0,
                    p.1,
                    xo[0],
                    yo[0]
                );
            };
            for k in 0..4 {
                check_one_of(true, &format!("forward north pole {k}"), north[k], &ntip);
                check_one_of(true, &format!("forward south pole {k}"), south[k], &stip);
                check_one_of(false, &format!("inverse north pole {k}"), ntip[k], north);
                check_one_of(false, &format!("inverse south pole {k}"), stip[k], south);
            }
        }
    }
}

/// testxphmap.c `check_rounding`: the left edge of a north polar facet of an
/// HPX12 grid is an edge of the sky, as the cells to its left are not used.
/// A position displaced to the left of it by less than the margin the HPX
/// projection allows for rounding, 1E-12 degrees where a pixel is
/// 45/2^order degrees, is transformed as the position on the edge is; one
/// displaced by much more gives bad values.
#[test]
fn a_position_within_the_rounding_margin_is_on_the_edge() {
    for projection in TYPES {
        for order in [0, 3, 27] {
            let map = load(order, projection);
            let n = f64::from(1u32 << order);
            let tol = 1.0e-12 * n / 45.0;
            let x = n + 0.5;
            let y = 2.0 * n + 0.75;
            let inverse = |x: f64, y: f64| {
                let (xo, yo) = map.tran2(&[x], &[y], false).unwrap();
                (xo[0], yo[0])
            };
            let edge = inverse(x, y);
            assert!(
                edge.0 != BAD_VALUE && edge.1 != BAD_VALUE,
                "{projection} order {order}: ({x}, {y}) on the edge of a polar facet gave bad \
                 values"
            );
            assert_eq!(
                inverse(x - 0.5 * tol, y),
                edge,
                "{projection} order {order} just off the edge"
            );
            assert_eq!(
                inverse(x - 100.0 * tol, y),
                (BAD_VALUE, BAD_VALUE),
                "{projection} order {order} well off the edge"
            );
        }
    }
}

/// testxphmap.c `check_bad_order`: the order is a shift count, so a dump's
/// Order must be one the constructor accepts, 0 to 27.
#[test]
fn an_order_out_of_range_is_refused() {
    for order in [-1, 28, 31, 32] {
        let err = from_native_string(&dump(order, "HPX12")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Lderr, "{order}");
        assert_eq!(
            err.message(),
            format!(
                "astLoad(XphMap): The HEALPix order read for a XphMap ({order}) is not in the \
                 range 0 to 27."
            )
        );
    }
}

/// testxphmap.c `check_max_order`: at the largest order an HPX grid is
/// 5*2^27 pixels wide and an XPH grid 4*2^27; pixels at both ends of the
/// grid, positions beyond them, the unused copy of a pixel, and a pole.
#[test]
fn the_largest_order_works() {
    let n = 134217728.0;
    let check = |map: &XphMap, forward: bool, x: f64, y: f64, want: Option<(f64, f64)>| {
        let (xo, yo) = map.tran2(&[x], &[y], forward).unwrap();
        let want = want.unwrap_or((BAD_VALUE, BAD_VALUE));
        assert_eq!((xo[0], yo[0]), want, "({x}, {y}) forward {forward}");
    };
    let hpx = load(27, "HPX12");
    check(&hpx, true, 1.0, n + 1.0, Some((1.0, n + 1.0)));
    check(&hpx, true, -n, n + 1.0, None);
    check(&hpx, true, 0.0, 1.5 * n + 1.0, None);
    check(
        &hpx,
        true,
        5.0 * n,
        4.0 * n + 1.0,
        Some((5.0 * n, 4.0 * n + 1.0)),
    );
    check(&hpx, true, 6.0 * n, 4.0 * n + 1.0, None);
    check(&hpx, true, 5.0 * n, 5.0 * n, Some((n, n)));
    check(&hpx, false, 5.0 * n, 5.0 * n, Some((n, n)));
    let xphn = load(27, "XPHN");
    check(&xphn, true, 4.0 * n, 1.0, Some((2.0 * n, 1.0)));
    check(&xphn, true, 4.0 * n + 1.0, 1.0, None);
    check(
        &xphn,
        false,
        n + 0.5,
        3.0 * n + 0.5,
        Some((2.0 * n + 0.5, 2.0 * n + 0.5)),
    );
    check(
        &xphn,
        true,
        2.0 * n + 0.5,
        2.0 * n + 0.5,
        Some((3.0 * n + 0.5, 5.0 * n + 0.5)),
    );
}
