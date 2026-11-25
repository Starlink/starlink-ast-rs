// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A FrameSet read from a corrupt dump must give the C library's error, and
//! never index outside its Frames or nodes or loop for ever.
//!
//! Ported from `reference/starlink-ast/ast_tester/testframesetload.c`.
//! Divergences from the C:
//!
//! - The corrupt dumps the C test reads are replayed, with C's messages, by
//!   `test_hardening_oracle.rs`; this file ports its other checks.

use std::time::Duration;

use starlink_ast::error::ErrorCode;
use starlink_ast::{AnyObject, from_native_string};

const FRAME: &str = " Begin Frame\n Naxes = 2\n Domain = \"{}\"\n Ax1 =\n Begin Axis\n End Axis\n \
                     Ax2 =\n Begin Axis\n End Axis\n End Frame\n";

/// The valid two-Frame FrameSet the C test edits, with `extra` after its
/// Nframe.
fn dump(extra: &str) -> String {
    format!(
        " Begin FrameSet\n Nframe = 2\n{extra} Currnt = 2\n Lnk2 = 1\n Frm1 =\n{} Frm2 =\n{} \
         Map2 =\n Begin UnitMap\n Nin = 2\n IsA Mapping\n End UnitMap\n End FrameSet\n",
        FRAME.replace("{}", "BASE"),
        FRAME.replace("{}", "CURR"),
    )
}

#[test]
fn a_valid_frameset_reads_and_writes_the_same() {
    let text = dump("");
    let AnyObject::FrameSet(fs) = from_native_string(&text).unwrap() else {
        panic!("not a FrameSet");
    };
    assert_eq!(fs.nframe(), 2);
    let again = fs.to_native_string().unwrap();
    let AnyObject::FrameSet(fs2) = from_native_string(&again).unwrap() else {
        panic!("not a FrameSet");
    };
    assert_eq!(fs2.to_native_string().unwrap(), again);
}

/// Variant frames that mirror each other in a loop load, as in C, and the
/// Variant attribute then reports the corruption rather than looping.
#[test]
fn a_loop_of_mirrored_variants_is_reported() {
    let text = dump(" Vfr1 = 2\n Vfr2 = 1\n");
    let AnyObject::FrameSet(fs) = from_native_string(&text).unwrap() else {
        panic!("not a FrameSet");
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(fs.variant());
    });
    let err = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("Variant returned")
        .expect_err("Variant fails");
    assert_eq!(err.code(), ErrorCode::Frsin);
    assert_eq!(
        err.message(),
        "astGetVarFrm(FrameSet): Invalid or corrupt FrameSet - the chain of mirrored Frames is not valid."
    );
    assert!(err.context().is_empty());
}
