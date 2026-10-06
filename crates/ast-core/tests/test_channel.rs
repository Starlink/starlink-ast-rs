//! Channel integration tests — port of reference/starlink-ast/ast_tester/testchannel.c
//!
//! Tests the Channel system: NativeChannel with StringSink/StringSource,
//! file I/O, configuration options (comment, full, indent), multiple object
//! types, and equivalence between Channel-based and convenience-function APIs.

use ast_core::channel::{Channel, NativeChannel};
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, FrameRef, FrameSet, SkyFrame, SkySystem};
use ast_core::mappings::{ShiftMap, ZoomMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ===========================================================================
// Helper
// ===========================================================================

/// Assert two f64 values are close.
fn assert_close(actual: f64, expected: f64, tol: f64, label: &str) {
    assert!(
        (actual - expected).abs() < tol,
        "{label}: expected {expected}, got {actual} (diff {})",
        (actual - expected).abs()
    );
}

// ===========================================================================
// 1. Round-trip via StringSink/StringSource
// ===========================================================================

#[test]
fn round_trip_zoom_via_channel() {
    let zoom = ZoomMap::new(2, 3.5).unwrap();
    let obj = AnyObject::ZoomMap(zoom);

    // Write to string sink.
    let mut ch = NativeChannel::with_string_sink();
    ch.write(&obj).unwrap();
    let text = ch.into_string();
    assert!(!text.is_empty());
    assert!(text.contains("Begin ZoomMap"));
    assert!(text.contains("End ZoomMap"));

    // Read back from string source.
    let mut ch2 = NativeChannel::from_string(&text);
    let restored = ch2.read().unwrap().unwrap();

    // Verify mapping properties.
    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);

    // Verify functional equivalence: 3.5 * [1.0, 2.0] = [3.5, 7.0].
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 3.5, 1e-10, "zoom axis 1");
    assert_close(out.point(0)[1], 7.0, 1e-10, "zoom axis 2");
}

#[test]
fn round_trip_shiftmap_via_channel() {
    let shift = ShiftMap::new(vec![10.0, -20.0, 30.0]);
    let obj = AnyObject::ShiftMap(shift);

    let mut ch = NativeChannel::with_string_sink();
    ch.write(&obj).unwrap();
    let text = ch.into_string();
    assert!(text.contains("Begin ShiftMap"));

    let mut ch2 = NativeChannel::from_string(&text);
    let restored = ch2.read().unwrap().unwrap();

    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 3);
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 11.0, 1e-10, "shift axis 1");
    assert_close(out.point(0)[1], -18.0, 1e-10, "shift axis 2");
    assert_close(out.point(0)[2], 33.0, 1e-10, "shift axis 3");
}

// ===========================================================================
// 2. Round-trip via temp file
// ===========================================================================

#[test]
fn round_trip_skyframe_via_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("ast_channel_test_skyframe.ast");

    let sky = SkyFrame::new(SkySystem::Fk5);
    let obj = AnyObject::SkyFrame(sky);

    // Write to file.
    let mut ch = NativeChannel::to_file(&path).unwrap();
    ch.write(&obj).unwrap();
    drop(ch); // Flush via Drop.

    // Read from file.
    let mut ch2 = NativeChannel::from_file(&path).unwrap();
    let restored = ch2.read().unwrap().unwrap();

    let f = restored.as_frame().unwrap();
    assert_eq!(f.naxes(), 2);
    assert_eq!(f.system_string().unwrap(), Some("FK5"));

    // Cleanup.
    let _ = std::fs::remove_file(&path);
}

#[test]
fn round_trip_zoommap_via_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("ast_channel_test_zoommap.ast");

    let zoom = ZoomMap::new(3, -2.5).unwrap();
    let obj = AnyObject::ZoomMap(zoom);

    let mut ch = NativeChannel::to_file(&path).unwrap();
    ch.write(&obj).unwrap();
    drop(ch);

    let mut ch2 = NativeChannel::from_file(&path).unwrap();
    let restored = ch2.read().unwrap().unwrap();

    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);

    let pts = PointSet::from_rows([[2.0, 4.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], -5.0, 1e-10, "zoom file axis 1");
    assert_close(out.point(0)[1], -10.0, 1e-10, "zoom file axis 2");
    assert_close(out.point(0)[2], -15.0, 1e-10, "zoom file axis 3");

    let _ = std::fs::remove_file(&path);
}

// ===========================================================================
// 3. Config: comment=false
// ===========================================================================

#[test]
fn config_comment_false_suppresses_comments() {
    let zoom = ZoomMap::new(2, 4.0).unwrap();
    let obj = AnyObject::ZoomMap(zoom);

    // Write with comments enabled (default).
    let mut ch_with = NativeChannel::with_string_sink();
    ch_with.write(&obj).unwrap();
    let text_with = ch_with.into_string();

    // Write with comments disabled.
    let mut ch_without = NativeChannel::with_string_sink();
    ch_without.config_mut().comment = false;
    ch_without.write(&obj).unwrap();
    let text_without = ch_without.into_string();

    // Default output should contain comment markers.
    assert!(
        text_with.contains('#'),
        "default output should contain '#' comment markers"
    );

    // No-comment output should contain no '#' characters at all
    // (since ZoomMap has no string values with '#').
    assert!(
        !text_without.contains('#'),
        "comment=false output should not contain '#'"
    );

    // Both should still parse correctly.
    let mut ch = NativeChannel::from_string(&text_without);
    let restored = ch.read().unwrap().unwrap();
    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 2);
}

// ===========================================================================
// 4. Config: full=-1
// ===========================================================================

#[test]
fn config_full_minus_one_shorter_output() {
    // Use a BasicFrame with labels — its to_node produces Unset items
    // for default labels/symbols, which full=-1 will skip.
    let frame = BasicFrameBuilder::new(3)
        .domain("PIXEL")
        .label(1, "X")
        .label(2, "Y")
        .label(3, "Z")
        .build()
        .unwrap();
    let obj = AnyObject::BasicFrame(frame);

    // Write with full=0 (normal).
    let mut ch_normal = NativeChannel::with_string_sink();
    ch_normal.write(&obj).unwrap();
    let text_normal = ch_normal.into_string();

    // Write with full=-1 (minimal — skip Unset/default lines).
    let mut ch_minimal = NativeChannel::with_string_sink();
    ch_minimal.config_mut().full = -1;
    ch_minimal.write(&obj).unwrap();
    let text_minimal = ch_minimal.into_string();

    // Minimal output should be strictly shorter (Unset lines are skipped).
    assert!(
        text_minimal.len() < text_normal.len(),
        "full=-1 output ({} bytes) should be shorter than full=0 ({} bytes)",
        text_minimal.len(),
        text_normal.len()
    );

    // Minimal output should not contain commented-out (Unset) lines.
    // In the Native format, Unset lines begin with "#" followed by spaces and
    // an attribute key.
    for line in text_minimal.lines() {
        let trimmed = line.trim();
        assert!(
            !trimmed.starts_with('#'),
            "full=-1 should not contain any commented-out lines, found: {trimmed}"
        );
    }

    // Both should still parse to produce a valid Frame.
    let mut ch = NativeChannel::from_string(&text_minimal);
    let restored = ch.read().unwrap().unwrap();
    let f = restored.as_frame().unwrap();
    assert_eq!(f.naxes(), 3);
}

// ===========================================================================
// 5. Config: indent=6
// ===========================================================================

#[test]
fn config_indent_6_wider_indentation() {
    let zoom = ZoomMap::new(1, 2.0).unwrap();
    let obj = AnyObject::ZoomMap(zoom);

    // Write with default indent (3).
    let mut ch_default = NativeChannel::with_string_sink();
    ch_default.write(&obj).unwrap();
    let text_default = ch_default.into_string();

    // Write with indent=6.
    let mut ch_wide = NativeChannel::with_string_sink();
    ch_wide.config_mut().indent = 6;
    ch_wide.write(&obj).unwrap();
    let text_wide = ch_wide.into_string();

    // With wider indentation, the attribute lines should have more leading spaces.
    // Find an attribute line (contains "=" but not Begin/End) and check its indentation.
    let attr_indent_default = text_default
        .lines()
        .find(|l| l.contains("Zoom ="))
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    let attr_indent_wide = text_wide
        .lines()
        .find(|l| l.contains("Zoom ="))
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    assert!(
        attr_indent_wide > attr_indent_default,
        "indent=6 should produce wider indentation ({} vs {})",
        attr_indent_wide,
        attr_indent_default
    );

    // Both should still parse correctly.
    let mut ch = NativeChannel::from_string(&text_wide);
    let restored = ch.read().unwrap().unwrap();
    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 1);
}

// ===========================================================================
// 6. Multiple object types
// ===========================================================================

#[test]
fn round_trip_multiple_types() {
    // ZoomMap.
    let zoom = ZoomMap::new(2, 5.0).unwrap();
    let obj = AnyObject::ZoomMap(zoom);
    let text = to_native_string(&obj).unwrap();
    let restored = from_native_string(&text).unwrap();
    assert_eq!(restored.as_mapping().unwrap().nin(), 2);

    // ShiftMap.
    let shift = ShiftMap::new(vec![1.0, 2.0]);
    let obj = AnyObject::ShiftMap(shift);
    let text = to_native_string(&obj).unwrap();
    let restored = from_native_string(&text).unwrap();
    assert_eq!(restored.as_mapping().unwrap().nin(), 2);

    // SkyFrame.
    let sky = SkyFrame::new(SkySystem::Galactic);
    let obj = AnyObject::SkyFrame(sky);
    let text = to_native_string(&obj).unwrap();
    let restored = from_native_string(&text).unwrap();
    let f = restored.as_frame().unwrap();
    assert_eq!(f.naxes(), 2);
    assert_eq!(f.system_string().unwrap(), Some("GALACTIC"));

    // FrameSet.
    let base = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
    let mut fs = FrameSet::new(Box::new(base));
    let zoom_map = ZoomMap::new(2, 2.0).unwrap();
    let current = BasicFrameBuilder::new(2).domain("SKY").build().unwrap();
    fs.add_frame(FrameRef::Base, Box::new(zoom_map), Box::new(current))
        .unwrap();
    let obj = AnyObject::FrameSet(fs);
    let text = to_native_string(&obj).unwrap();
    assert!(text.contains("Begin FrameSet"));
    let restored = from_native_string(&text).unwrap();
    let f = restored.as_frame().unwrap();
    assert_eq!(f.naxes(), 2);
}

// ===========================================================================
// 7. NativeChannel::from_string matches from_native_string
// ===========================================================================

#[test]
fn channel_from_string_matches_convenience_function() {
    let zoom = ZoomMap::new(2, 7.7).unwrap();
    let obj = AnyObject::ZoomMap(zoom);

    // Produce text via Channel.
    let mut ch = NativeChannel::with_string_sink();
    ch.write(&obj).unwrap();
    let text = ch.into_string();

    // Read via Channel.
    let mut ch2 = NativeChannel::from_string(&text);
    let via_channel = ch2.read().unwrap().unwrap();

    // Read via convenience function.
    let via_convenience = from_native_string(&text).unwrap();

    // Both should produce equivalent mappings.
    let m1 = via_channel.as_mapping().unwrap();
    let m2 = via_convenience.as_mapping().unwrap();
    assert_eq!(m1.nin(), m2.nin());
    assert_eq!(m1.nout(), m2.nout());

    // Functional equivalence: transform the same point.
    let pts = PointSet::from_rows([[3.0, 4.0]]);
    let out1 = m1.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    assert_close(out1.point(0)[0], out2.point(0)[0], 1e-15, "axis 1");
    assert_close(out1.point(0)[1], out2.point(0)[1], 1e-15, "axis 2");
}

#[test]
fn to_native_string_matches_channel_write() {
    let shift = ShiftMap::new(vec![100.0, -200.0]);
    let obj = AnyObject::ShiftMap(shift);

    // Via convenience function.
    let text_convenience = to_native_string(&obj).unwrap();

    // Via Channel.
    let mut ch = NativeChannel::with_string_sink();
    ch.write(&obj).unwrap();
    let text_channel = ch.into_string();

    // Should produce identical output.
    assert_eq!(text_convenience, text_channel);
}

// ===========================================================================
// 8. Error: read with no source
// ===========================================================================

#[test]
fn read_with_no_source_returns_none() {
    let mut ch = NativeChannel::new();
    let result = ch.read().unwrap();
    assert!(
        result.is_none(),
        "reading from a channel with no source should return None"
    );
}

#[test]
fn read_empty_string_returns_none() {
    let mut ch = NativeChannel::from_string("");
    let result = ch.read().unwrap();
    assert!(
        result.is_none(),
        "reading from empty string should return None"
    );
}

#[test]
fn read_whitespace_only_returns_error_or_none() {
    // Whitespace-only input: the source collects non-empty text (spaces),
    // but the parser fails to find a Begin token. This may return None
    // (if the source decides it's "empty") or an error (parser rejects it).
    let mut ch = NativeChannel::from_string("   \n\n  \n");
    let result = ch.read();
    match result {
        Ok(None) => {} // acceptable: treated as empty
        Err(_) => {}   // acceptable: parser rejects garbage
        Ok(Some(_)) => panic!("whitespace-only input should not produce an object"),
    }
}

// ===========================================================================
// 9. Write then read in sequence (multiple objects)
// ===========================================================================

#[test]
fn write_multiple_objects_read_back() {
    // Write two objects to separate channels and read them back.
    // (NativeChannel reads all source at once, so we test with separate texts.)
    let zoom = ZoomMap::new(1, 9.0).unwrap();
    let shift = ShiftMap::new(vec![5.0]);

    // Write each to its own string.
    let mut ch1 = NativeChannel::with_string_sink();
    ch1.write(&AnyObject::ZoomMap(zoom)).unwrap();
    let text1 = ch1.into_string();

    let mut ch2 = NativeChannel::with_string_sink();
    ch2.write(&AnyObject::ShiftMap(shift)).unwrap();
    let text2 = ch2.into_string();

    // Read them back.
    let mut r1 = NativeChannel::from_string(&text1);
    let obj1 = r1.read().unwrap().unwrap();
    let m1 = obj1.as_mapping().unwrap();
    assert_eq!(m1.nin(), 1);
    let pts = PointSet::from_rows([[2.0]]);
    let out1 = m1.transform(&pts, true).unwrap();
    assert_close(out1.point(0)[0], 18.0, 1e-10, "zoom 9*2");

    let mut r2 = NativeChannel::from_string(&text2);
    let obj2 = r2.read().unwrap().unwrap();
    let m2 = obj2.as_mapping().unwrap();
    assert_eq!(m2.nin(), 1);
    let out2 = m2.transform(&pts, true).unwrap();
    assert_close(out2.point(0)[0], 7.0, 1e-10, "shift 2+5");
}

#[test]
fn write_and_read_frameset_via_channel() {
    // Build a FrameSet: PIXEL --[zoom x2]--> GRID.
    let pixel = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
    let mut fs = FrameSet::new(Box::new(pixel));
    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let grid = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
    fs.add_frame(FrameRef::Base, Box::new(zoom), Box::new(grid))
        .unwrap();

    let obj = AnyObject::FrameSet(fs);

    // Write via Channel.
    let mut ch = NativeChannel::with_string_sink();
    ch.write(&obj).unwrap();
    let text = ch.into_string();
    assert!(text.contains("Begin FrameSet"));
    assert!(text.contains("PIXEL"));

    // Read back.
    let mut ch2 = NativeChannel::from_string(&text);
    let restored = ch2.read().unwrap().unwrap();

    // Verify as a Mapping (FrameSet is both Frame and Mapping).
    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);

    let pts = PointSet::from_rows([[3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 6.0, 1e-10, "fs zoom axis 1");
    assert_close(out.point(0)[1], 8.0, 1e-10, "fs zoom axis 2");
}

// ===========================================================================
// 10. Config round-trip verification
// ===========================================================================

#[test]
fn config_defaults_are_sensible() {
    let ch = NativeChannel::new();
    let cfg = ch.config();
    assert!(cfg.comment);
    assert_eq!(cfg.full, 0);
    assert_eq!(cfg.indent, 3);
    assert!(!cfg.skip);
    assert!(!cfg.strict);
}

#[test]
fn config_mut_is_honoured() {
    let mut ch = NativeChannel::with_string_sink();
    ch.config_mut().comment = false;
    ch.config_mut().full = -1;
    ch.config_mut().indent = 5;

    assert!(!ch.config().comment);
    assert_eq!(ch.config().full, -1);
    assert_eq!(ch.config().indent, 5);
}

// ===========================================================================
// 11. Warnings accessor
// ===========================================================================

#[test]
fn warnings_initially_empty() {
    let ch = NativeChannel::new();
    assert!(ch.warning_texts().is_empty());
    // C's astWarnings returns NULL rather than an empty KeyMap when the
    // channel issued no warnings (channel.c:1698 guards on `this->nwarn`).
    assert!(ch.warnings().is_none());
}
