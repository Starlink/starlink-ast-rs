// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

use starlink_ast::error::{AstError, ErrorCode};

#[test]
fn new_error_has_empty_context() {
    let err = AstError::new(ErrorCode::Atger, "test message");
    assert_eq!(err.code(), ErrorCode::Atger);
    assert_eq!(err.message(), "test message");
    assert!(err.context().is_empty());
}

#[test]
fn with_context_appends() {
    let err = AstError::new(ErrorCode::Axiin, "axis 0 invalid")
        .with_context("while setting label")
        .with_context("in SkyFrame");
    assert_eq!(err.context().len(), 2);
    assert_eq!(err.context()[0], "while setting label");
    assert_eq!(err.context()[1], "in SkyFrame");
}

#[test]
fn display_includes_context() {
    let err = AstError::new(ErrorCode::Atger, "failed").with_context("in transform");
    let msg = format!("{err}");
    assert!(msg.contains("attribute getting error"));
    assert!(msg.contains("failed"));
    assert!(msg.contains("in transform"));
}

#[test]
fn new_tracked_captures_location() {
    let err = AstError::new_tracked(ErrorCode::Inter, "bug");
    let loc = err.source_location().expect("should have location");
    assert!(loc.file().contains("error_model.rs"));
}

#[test]
fn new_has_no_location() {
    let err = AstError::new(ErrorCode::Inter, "bug");
    assert!(err.source_location().is_none());
}
