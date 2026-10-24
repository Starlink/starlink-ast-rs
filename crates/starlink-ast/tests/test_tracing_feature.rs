// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The `tracing` feature emits every constructed `AstError` as a
//! `tracing::error!` event.
//!
//! Design commitment: `docs/superpowers/specs/2026-04-20-rust-port-design.md`,
//! "Error model" — *"Optional `tracing` feature emits every constructed
//! `AstError` as a `tracing::error!` event; off by default."*
//!
//! No C ancestor: `ast_tester` has no analogue, because the C library reports
//! through global status and `astPutErr` rather than a structured event stream.
//!
//! Run with `cargo test -p starlink-ast --features tracing`. Without the feature
//! the whole file compiles to nothing, so the default build pays neither the
//! dependency nor the compile time. The subscriber below is hand-rolled for the
//! same reason: a `tracing-subscriber` dev-dependency cannot be feature-gated
//! and would be built on every `cargo test`.

#![cfg(feature = "tracing")]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use starlink_ast::error::{AstError, ErrorCode};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber, span};

/// Records the level and captured fields of every event it is sent.
#[derive(Default)]
struct Recorder {
    events: AtomicUsize,
    errors: AtomicUsize,
    fields: Mutex<Vec<(String, String)>>,
}

impl Recorder {
    fn event_count(&self) -> usize {
        self.events.load(Ordering::Relaxed)
    }

    fn error_count(&self) -> usize {
        self.errors.load(Ordering::Relaxed)
    }

    fn field(&self, name: &str) -> Option<String> {
        self.fields
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

/// Collects one event's fields. Separate from [`Recorder`] because `Visit`
/// takes `&mut self` while a `Subscriber` is only ever handed `&self`.
#[derive(Default)]
struct FieldGrab(Vec<(String, String)>);

impl Visit for FieldGrab {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .push((field.name().to_string(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_string(), value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.push((field.name().to_string(), value.to_string()));
    }
}

/// Minimal `Subscriber`: counts events and captures their fields. Spans are
/// unused — `AstError` construction emits a bare event — so the span methods
/// are inert.
struct Collector(std::sync::Arc<Recorder>);

impl Subscriber for Collector {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _attrs: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        self.0.events.fetch_add(1, Ordering::Relaxed);
        if *event.metadata().level() == Level::ERROR {
            self.0.errors.fetch_add(1, Ordering::Relaxed);
        }
        let mut grab = FieldGrab::default();
        event.record(&mut grab);
        self.0.fields.lock().unwrap().extend(grab.0);
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}

/// Run `f` with a fresh recorder installed, and return the recorder.
fn capture(f: impl FnOnce()) -> std::sync::Arc<Recorder> {
    let recorder = std::sync::Arc::new(Recorder::default());
    let collector = Collector(std::sync::Arc::clone(&recorder));
    tracing::subscriber::with_default(collector, f);
    recorder
}

#[test]
fn new_emits_one_error_event() {
    let rec = capture(|| {
        let _ = AstError::new(ErrorCode::Badat, "bad attribute Frobnicate");
    });

    assert_eq!(
        rec.event_count(),
        1,
        "exactly one event per constructed error"
    );
    assert_eq!(rec.error_count(), 1, "emitted at ERROR level");
    assert_eq!(
        rec.field("message").as_deref(),
        Some("bad attribute Frobnicate"),
        "the error text is the event message, as a subscriber would print it"
    );
    assert_eq!(
        rec.field("code.raw").as_deref(),
        Some(ErrorCode::Badat.as_raw().to_string().as_str()),
        "the raw numeric code is carried so it can be matched against ast_err.msg"
    );
}

#[test]
fn new_tracked_carries_the_caller_location() {
    let rec = capture(|| {
        let _ = AstError::new_tracked(ErrorCode::Axiin, "axis 0 is invalid");
    });

    assert_eq!(rec.error_count(), 1);
    let location = rec.field("location").expect("location field present");
    assert!(
        location.contains("test_tracing_feature.rs"),
        "location should name this test file, got {location:?}"
    );
}

#[test]
fn new_reports_no_location() {
    let rec = capture(|| {
        let _ = AstError::new(ErrorCode::Axiin, "axis 0 is invalid");
    });

    // `new` does not capture a location; the field is present but empty rather
    // than absent, so a consumer can distinguish "not tracked" from "missing".
    assert_eq!(rec.field("location").as_deref(), Some(""));
}

#[test]
fn with_context_does_not_re_emit() {
    let rec = capture(|| {
        let _ = AstError::new(ErrorCode::Badin, "bad input data")
            .with_context("while reading fixture")
            .with_context("while running the harness");
    });

    assert_eq!(
        rec.event_count(),
        1,
        "context decoration must not log the same failure once per frame"
    );
}

#[test]
fn errors_raised_from_inside_the_library_are_traced() {
    // Not a hand-built error: drive a real API failure so the feature is shown
    // to cover errors the library raises internally, not just direct
    // construction. A 1-based axis index of 0 is always rejected.
    use starlink_ast::frames::{BasicFrameBuilder, Frame};

    let rec = capture(|| {
        let frame = BasicFrameBuilder::new(2).build().expect("2-axis frame");
        let err = frame.format(0, 1.0).expect_err("axis 0 must be rejected");
        assert_eq!(err.code(), ErrorCode::Axiin);
    });

    assert!(
        rec.error_count() >= 1,
        "a library-raised error should emit an event"
    );
}
