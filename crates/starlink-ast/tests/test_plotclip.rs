// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A Plot survives an error raised while clipping.
//!
//! Ported from `reference/starlink-ast/ast_tester/testplotclip.c`.
//! A Plot clipped in a Frame it reaches through an IntraMap whose
//! transformation can be told to fail: clipping works while it succeeds,
//! transforming and drawing report the error while it fails, and the Plot
//! gives the same results once it succeeds again.
//! Divergences from the C:
//!
//! - The transformation fails through a flag it owns rather than a global,
//!   and is registered by giving the IntraMap the object, not by name.
//! - The border and grid are drawn into a `LogGrf`; nothing reaches it, as
//!   in the C, where no graphics are drawn.
//! - The messages are compared too, with those of a probe of the pinned C
//!   library, and a failure that reports a message of its own is added.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::{Points, PointsMut};
use starlink_ast::error::{AstError, AstResult, ErrorCode};
use starlink_ast::frames::{BasicFrameBuilder, DynFrame, Frame, FrameRef, FrameSet};
use starlink_ast::grf::LogGrf;
use starlink_ast::mappings::{IntraFlags, IntraMap, IntraTransform, Mapping, UnitMap};
use starlink_ast::plot::{Draw, Plot};

/// Copies its inputs to its outputs, or fails while `fail` is set: with no
/// message of its own (1), as C's function does by setting the status
/// AST__ITFER, or with one (2), as a function that reports one does.
pub(crate) struct CopyOrFail {
    pub(crate) fail: AtomicU8,
}

impl IntraTransform for CopyOrFail {
    fn name(&self) -> &str {
        "CopyOrFail"
    }
    fn nin(&self) -> Option<usize> {
        Some(2)
    }
    fn nout(&self) -> Option<usize> {
        Some(2)
    }
    fn flags(&self) -> IntraFlags {
        IntraFlags::default()
    }
    fn purpose(&self) -> &str {
        "Copy positions, or fail on request"
    }
    fn author(&self) -> &str {
        "AST test suite"
    }
    fn contact(&self) -> &str {
        ""
    }
    fn transform(
        &self,
        _map: &IntraMap,
        input: &Points<'_>,
        _forward: bool,
        out: &mut PointsMut<'_>,
    ) -> AstResult<()> {
        match self.fail.load(Ordering::SeqCst) {
            0 => {}
            1 => return Err(AstError::new(ErrorCode::Itfer, "")),
            _ => return Err(AstError::new(ErrorCode::Itfer, "my own message")),
        }
        for j in 0..input.ncoord() {
            for (o, i) in out.axis_mut(j).iter_mut().zip(input.axis(j)) {
                *o = *i;
            }
        }
        Ok(())
    }
}

fn frame(domain: &str) -> DynFrame {
    let mut f = BasicFrameBuilder::new(2).build().unwrap();
    Frame::set_domain(&mut f, domain).unwrap();
    Box::new(f)
}

/// A Plot whose clipping Frame, the third, is reached from the base Frame
/// through `transform`, and whose current Frame is reached by a UnitMap.
pub(crate) fn clipped_plot(transform: Arc<CopyOrFail>) -> Plot {
    let mut fset = FrameSet::new(frame("GRID"));
    let intra = IntraMap::new(transform, 2, 2).unwrap();
    fset.add_frame(FrameRef::Base, Box::new(intra), frame("CLIP"))
        .unwrap();
    fset.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), frame("SKY"))
        .unwrap();
    let mut plot = Plot::new(&fset, [0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]).unwrap();
    plot.set_clip_region(FrameRef::Index(3), &[0.1, 0.1], &[0.9, 0.9])
        .unwrap();
    plot
}

/// The first position is inside the clipping box and unchanged; the
/// second is outside it and bad.
fn check_clipping(when: &str, plot: &Plot) {
    let (x, y) = plot
        .tran2(&[0.5, 0.95], &[0.5, 0.5], true)
        .unwrap_or_else(|e| panic!("could not transform positions {when}: {e}"));
    assert_eq!(
        (x, y),
        (vec![0.5, BAD_VALUE], vec![0.5, BAD_VALUE]),
        "{when}"
    );
}

/// The message C reports for the failure, after any of the function's
/// own.
const SIGNALLED: &str =
    "astTransform(IntraMap): Error signalled by \"CopyOrFail\" transformation function.";

/// Check that `result` is the failure of the transformation, with C's
/// messages `messages`.
fn check_failed<T>(what: &str, result: AstResult<T>, messages: &[&str]) {
    let Err(err) = result else {
        panic!("{what} did not fail");
    };
    assert_eq!(err.code(), ErrorCode::Itfer, "{what}");
    let got: Vec<&str> = err
        .message()
        .split('\n')
        .chain(err.context().iter().map(String::as_str))
        .collect();
    assert_eq!(got, messages, "{what}");
}

#[test]
fn clipping_survives_an_error() {
    let transform = Arc::new(CopyOrFail {
        fail: AtomicU8::new(0),
    });
    let mut plot = clipped_plot(transform.clone());
    check_clipping("before the error", &plot);

    // While the transformation fails, each operation that clips reports it.
    transform.fail.store(1, Ordering::SeqCst);
    check_failed("tran2", plot.tran2(&[0.5], &[0.5], true), &[SIGNALLED]);
    let mut grf = LogGrf::new(Vec::new());
    check_failed("border", plot.draw_border(&mut grf), &[SIGNALLED]);
    let mut grf = LogGrf::new(Vec::new());
    check_failed("grid", plot.draw_grid(&mut grf), &[SIGNALLED]);

    // A message the function reports comes first, as in C.
    transform.fail.store(2, Ordering::SeqCst);
    check_failed(
        "tran2 with a message",
        plot.tran2(&[0.5], &[0.5], true),
        &["my own message", SIGNALLED],
    );
    transform.fail.store(0, Ordering::SeqCst);

    // The Plot is unharmed.
    check_clipping("after the error", &plot);
}
