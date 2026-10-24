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
/// point its forward transformation accepts, over a scan of the projection.
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
            (xout[i] - xin[i]).abs() <= 1.0e-8 && (yout[i] - yin[i]).abs() <= 1.0e-8,
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
