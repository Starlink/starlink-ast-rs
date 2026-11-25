// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Input that is corrupt, malicious or very large must give the C library's
//! error, and never crash, hang or exhaust memory.
//!
//! Ported from `reference/starlink-ast/ast_tester/testcorruptinput.c`.
//! Divergences from the C:
//!
//! - Every dump the C test reads is replayed by `test_hardening_oracle.rs`;
//!   this file ports its other checks.
//! - The C test checks only the status; the messages are compared too, with
//!   those the C library at the pinned commit reports.
//! - C passes counts as `int`; the Rust constructors take a `usize` or a slice,
//!   so a negative count cannot be passed.

use starlink_ast::constants::MXAXES;
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::{BasicFrameBuilder, CmpFrame, Frame, SkyFrame};
use starlink_ast::mappings::{
    CmpMap, Composition, PermMap, ShiftMap, UnitMap, UnitNormMap, WinMap, ZoomMap,
};

fn expect<T>(result: AstResult<T>, code: ErrorCode, messages: &[&str]) {
    let Err(err) = result else {
        panic!("no error");
    };
    let got: Vec<&str> = err
        .message()
        .split('\n')
        .chain(err.context().iter().map(String::as_str))
        .collect();
    assert_eq!((err.code(), got), (code, messages.to_vec()));
}

fn too_many_inputs(class: &str, n: usize) -> [String; 2] {
    [
        format!("astInitMapping({class}): Bad number of input coordinates ({n})."),
        format!("This number should not exceed {MXAXES}."),
    ]
}

fn expect_inputs<T>(result: AstResult<T>, class: &str, n: usize) {
    let [a, b] = too_many_inputs(class, n);
    expect(result, ErrorCode::Badni, &[&a, &b]);
}

/// Constructors accept the limit and refuse one more.
#[test]
fn axis_limit() {
    assert_eq!(MXAXES, 65536);
    assert!(UnitMap::new(MXAXES).is_ok());
    expect_inputs(UnitMap::new(MXAXES + 1), "UnitMap", MXAXES + 1);
    expect_inputs(UnitMap::new(2147483647), "UnitMap", 2147483647);
    expect_inputs(ShiftMap::new(vec![0.0; MXAXES + 1]), "ShiftMap", MXAXES + 1);
    expect_inputs(ZoomMap::new(MXAXES + 1, 2.0), "ZoomMap", MXAXES + 1);
    expect_inputs(
        WinMap::new(&vec![0.0; MXAXES + 1], &vec![1.0; MXAXES + 1]),
        "WinMap",
        MXAXES + 1,
    );
    expect_inputs(
        UnitNormMap::new(vec![0.0; MXAXES + 1]),
        "UnitNormMap",
        MXAXES + 1,
    );
    let perm: Vec<i64> = (1..=65537).collect();
    expect_inputs(PermMap::new(&perm, &[], &[]), "PermMap", MXAXES + 1);
    expect(
        PermMap::new(&[], &perm, &[]),
        ErrorCode::Badno,
        &[
            "astInitMapping(PermMap): Bad number of output coordinates (65537).",
            "This number should not exceed 65536.",
        ],
    );
    expect_inputs(
        CmpMap::new(
            Box::new(UnitMap::new(MXAXES).unwrap()),
            Box::new(UnitMap::new(MXAXES / 2).unwrap()),
            Composition::Parallel,
        ),
        "CmpMap",
        MXAXES + MXAXES / 2,
    );

    expect(
        BasicFrameBuilder::new(MXAXES + 1).build(),
        ErrorCode::Naxin,
        &[
            "astInitFrame(Frame): Number of axes (65537) is invalid - this number should not exceed 65536.",
        ],
    );
    expect(
        BasicFrameBuilder::new(2147483647).build(),
        ErrorCode::Naxin,
        &[
            "astInitFrame(Frame): Number of axes (2147483647) is invalid - this number should not exceed 65536.",
        ],
    );

    // Two Frames each within the limit cannot make a CmpFrame beyond it.
    let big = || Box::new(BasicFrameBuilder::new(MXAXES).build().unwrap());
    let half = || Box::new(BasicFrameBuilder::new(MXAXES / 2).build().unwrap());
    assert!(CmpFrame::new(half(), half()).is_ok());
    expect(
        CmpFrame::new(big(), half()),
        ErrorCode::Naxin,
        &[
            "astInitCmpFrame(CmpFrame): Number of axes (98304) is invalid - this number should not exceed 65536.",
        ],
    );
}

/// A permutation that is not one gives C's two messages, naming the class.
#[test]
fn invalid_permutations() {
    let messages = |class: &str, n: usize| {
        [
            format!("astPermAxes({class}): Invalid axis permutation array."),
            format!("Each axis index should lie in the range 1 to {n} and should occur only once."),
        ]
    };
    let mut frame = BasicFrameBuilder::new(3).build().unwrap();
    for perm in [[1, 1, 2], [0, 1, 2], [1, 2, 4]] {
        let [a, b] = messages("Frame", 3);
        expect(frame.perm_axes(&perm), ErrorCode::Prmin, &[&a, &b]);
    }
    let mut cmp = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    )
    .unwrap();
    let [a, b] = messages("CmpFrame", 2);
    expect(cmp.perm_axes(&[1, 1]), ErrorCode::Prmin, &[&a, &b]);
    let mut sky = SkyFrame::unset_system();
    let [a, b] = messages("SkyFrame", 2);
    expect(sky.perm_axes(&[1, 1]), ErrorCode::Prmin, &[&a, &b]);
}
