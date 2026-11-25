// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Input nested beyond a reader's limit is refused with an error rather than
//! overflowing the stack.
//!
//! The C library's recursive readers have no such limits; a dump, an
//! expression or a unit string nested deep enough crashes it. Each test runs
//! on a thread with a 2 MiB stack, the default for a spawned thread, so that
//! a reader that recursed without a limit would crash the test program, and
//! each checks that input nested exactly to the limit still reads there.

use starlink_ast::AnyObject;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::constants::{MAX_EXPRESSION_NESTING, MAX_NATIVE_NESTING, MAX_UNIT_NESTING};
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::{BasicFrameBuilder, CmpFrame, FluxFrame, Frame};
use starlink_ast::from_native_string;
use starlink_ast::keymap::KeyMap;
use starlink_ast::mappings::{MathMap, TranMap, UnitMap};
use starlink_ast::regions::{CircleBuilder, CmpOperator, CmpRegionBuilder};

/// Far deeper than any reader's limit.
const DEEP: usize = 100_000;

/// Run `f` on a thread with a 2 MiB stack and return what it returns.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

/// A Native dump of CmpMaps nested `depth` deep, each with a UnitMap as its
/// second Mapping.
fn nested_cmpmaps(depth: usize) -> String {
    let mut dump = String::new();
    for _ in 1..depth {
        dump.push_str(" Begin CmpMap\n Nin = 1\n IsA Mapping\n MapA =\n");
    }
    dump.push_str(" Begin UnitMap\n Nin = 1\n IsA Mapping\n End UnitMap\n");
    for _ in 1..depth {
        dump.push_str(
            " MapB =\n Begin UnitMap\n Nin = 1\n IsA Mapping\n End UnitMap\n End CmpMap\n",
        );
    }
    dump
}

/// `inner` in `depth` pairs of parentheses.
fn parenthesised(inner: &str, depth: usize) -> String {
    format!("{}{inner}{}", "(".repeat(depth), ")".repeat(depth))
}

/// `operand` joined by `operator` to make `count` operands.
fn chain(operand: &str, operator: &str, count: usize) -> String {
    vec![operand; count].join(operator)
}

fn mathmap(fwd: &str, inv: &str) -> AstResult<MathMap> {
    MathMap::new(1, 1, &[&format!("y = {fwd}")], &[&format!("x = {inv}")])
}

/// A MathMap whose expressions are nested `depth` deep in parentheses.
fn parenthesised_mathmap(depth: usize) -> AstResult<MathMap> {
    mathmap(&parenthesised("x", depth), &parenthesised("y", depth))
}

/// A MathMap whose expressions raise to a power `depth` times; `**` takes
/// its right operand first, so each nests the rest.
fn power_mathmap(depth: usize) -> AstResult<MathMap> {
    mathmap(&chain("x", "**", depth + 1), &chain("y", "**", depth + 1))
}

fn flux_unit(unit: &str) -> AstResult<()> {
    let mut frame = FluxFrame::with_spec_val(BAD_VALUE, None);
    frame.set_unit(1, unit)
}

/// A unit nested `depth` deep in parentheses.
fn parenthesised_unit(depth: usize) -> AstResult<()> {
    flux_unit(&parenthesised("Jy", depth))
}

/// A unit that multiplies `depth` times; each product nests those before it.
fn product_unit(depth: usize) -> AstResult<()> {
    flux_unit(&chain("Jy", "*", depth + 1))
}

#[test]
fn a_dump_nested_beyond_the_limit_is_refused() {
    let err = on_small_stack(|| from_native_string(&nested_cmpmaps(DEEP)).unwrap_err());
    assert_refused(&err, ErrorCode::Lderr, MAX_NATIVE_NESTING);
}

#[test]
fn a_dump_nested_to_the_limit_loads() {
    on_small_stack(|| from_native_string(&nested_cmpmaps(MAX_NATIVE_NESTING)).unwrap());
}

fn assert_refused(err: &starlink_ast::error::AstError, code: ErrorCode, limit: usize) {
    assert_eq!(err.code(), code, "{err}");
    assert!(err.to_string().contains(&limit.to_string()), "{err}");
}

#[test]
fn an_expression_nested_beyond_the_limit_is_refused() {
    let err = on_small_stack(|| parenthesised_mathmap(DEEP).unwrap_err());
    assert_refused(&err, ErrorCode::Miopa, MAX_EXPRESSION_NESTING);
    let err = on_small_stack(|| power_mathmap(DEEP).unwrap_err());
    assert_refused(&err, ErrorCode::Miopa, MAX_EXPRESSION_NESTING);
}

#[test]
fn an_expression_nested_to_the_limit_compiles() {
    on_small_stack(|| parenthesised_mathmap(MAX_EXPRESSION_NESTING).unwrap());
    on_small_stack(|| power_mathmap(MAX_EXPRESSION_NESTING).unwrap());
}

#[test]
fn a_unit_nested_beyond_the_limit_is_refused() {
    let err = on_small_stack(|| parenthesised_unit(DEEP).unwrap_err());
    assert_refused(&err, ErrorCode::Badun, MAX_UNIT_NESTING);
    let err = on_small_stack(|| product_unit(DEEP).unwrap_err());
    assert_refused(&err, ErrorCode::Badun, MAX_UNIT_NESTING);
}

#[test]
fn a_unit_nested_to_the_limit_is_used() {
    on_small_stack(|| parenthesised_unit(MAX_UNIT_NESTING).unwrap());
    on_small_stack(|| product_unit(MAX_UNIT_NESTING).unwrap());
}

/// Build an Object of class `kind` holding `depth` levels of its class, on a
/// large stack, and dump it.
fn built_dump(kind: &'static str, depth: usize) -> String {
    std::thread::Builder::new()
        .stack_size(1 << 28)
        .spawn(move || {
            let frame = |n| BasicFrameBuilder::new(n).build().unwrap();
            let circle = || {
                CircleBuilder::new(frame(2), vec![0.0, 0.0])
                    .unwrap()
                    .radius(1.0)
                    .build()
                    .unwrap()
            };
            match kind {
                "cmpframe" => {
                    let mut f = CmpFrame::new(Box::new(frame(1)), Box::new(frame(1))).unwrap();
                    for _ in 2..depth {
                        f = CmpFrame::new(Box::new(f), Box::new(frame(1))).unwrap();
                    }
                    f.to_native_string().unwrap()
                }
                "cmpregion" => {
                    let mut r = CmpRegionBuilder::new(circle(), circle(), CmpOperator::And)
                        .unwrap()
                        .build()
                        .unwrap();
                    for _ in 2..depth {
                        r = CmpRegionBuilder::new(r, circle(), CmpOperator::And)
                            .unwrap()
                            .build()
                            .unwrap();
                    }
                    r.to_native_string().unwrap()
                }
                "keymap" => {
                    let mut k = KeyMap::new();
                    for _ in 1..depth {
                        let mut outer = KeyMap::new();
                        outer
                            .put_0a("k", AnyObject::KeyMap(Box::new(k)), None)
                            .unwrap();
                        k = outer;
                    }
                    k.to_native_string().unwrap()
                }
                "tranmap" => {
                    let unit = || Box::new(UnitMap::new(1).unwrap());
                    let mut m = TranMap::new(unit(), unit()).unwrap();
                    for _ in 2..depth {
                        m = TranMap::new(Box::new(m), unit()).unwrap();
                    }
                    m.to_native_string().unwrap()
                }
                _ => panic!(),
            }
        })
        .unwrap()
        .join()
        .unwrap()
}

/// How deep the Objects in a Native dump nest.
fn dump_depth(dump: &str) -> usize {
    let (mut depth, mut deepest) = (0usize, 0usize);
    for line in dump.lines() {
        let word = line.split_whitespace().next();
        if word == Some("Begin") {
            depth += 1;
            deepest = deepest.max(depth);
        } else if word == Some("End") {
            depth -= 1;
        }
    }
    deepest
}

/// Each class that holds other Objects, in a dump nested to the limit,
/// loads: the Mapping, Frame, Region and KeyMap loaders each recurse in
/// their own way.
#[test]
fn each_container_class_nested_to_the_limit_loads() {
    for kind in ["cmpframe", "cmpregion", "keymap", "tranmap"] {
        // The Objects inside the innermost level, such as a Frame's Axes,
        // nest further.
        let mut levels = MAX_NATIVE_NESTING - (dump_depth(&built_dump(kind, 2)) - 2);
        let mut dump = built_dump(kind, levels);
        while dump_depth(&dump) > MAX_NATIVE_NESTING {
            levels -= 1;
            dump = built_dump(kind, levels);
        }
        assert_eq!(dump_depth(&dump), MAX_NATIVE_NESTING, "{kind}");
        on_small_stack(move || drop(from_native_string(&dump).unwrap()));
    }
}
