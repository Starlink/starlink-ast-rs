//! Channel integration tests — port of reference/starlink-ast/ast_tester/testchannel.c
//!
//! Tests the Channel system: NativeChannel with StringSink/StringSource,
//! file I/O, configuration options (comment, full, indent), multiple object
//! types, and equivalence between Channel-based and convenience-function APIs.

use crate::common::native_dump;
use ast_core::channel::{Channel, NativeChannel};
use ast_core::coords::PointSet;
use ast_core::frames::FrameSearch;
use ast_core::frames::{BasicFrameBuilder, FrameRef, FrameSet, SkyFrame, SkySystem};
use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};
use ast_core::{AnyObject, from_native_string};

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
    ch_without.config_mut().set_comment(false);
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
    ch_minimal.config_mut().set_full(-1);
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
    ch_wide.config_mut().set_indent(6);
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
    let text = native_dump(&obj).unwrap();
    let restored = from_native_string(&text).unwrap();
    assert_eq!(restored.as_mapping().unwrap().nin(), 2);

    // ShiftMap.
    let shift = ShiftMap::new(vec![1.0, 2.0]);
    let obj = AnyObject::ShiftMap(shift);
    let text = native_dump(&obj).unwrap();
    let restored = from_native_string(&text).unwrap();
    assert_eq!(restored.as_mapping().unwrap().nin(), 2);

    // SkyFrame.
    let sky = SkyFrame::new(SkySystem::Galactic);
    let obj = AnyObject::SkyFrame(sky);
    let text = native_dump(&obj).unwrap();
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
    let text = native_dump(&obj).unwrap();
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

/// The text a NativeChannel with `configure` applied writes for `obj`.
fn channel_write(obj: &AnyObject, configure: impl FnOnce(&mut NativeChannel)) -> String {
    let mut ch = NativeChannel::with_string_sink();
    configure(&mut ch);
    ch.write(obj).unwrap();
    ch.into_string()
}

/// A CmpMap of a ZoomMap with an ID and a ShiftMap: nested objects and a set
/// Object attribute, which the compact form must keep.
fn nested_object() -> AnyObject {
    let mut zoom = ZoomMap::new(2, 4.0).unwrap();
    zoom.set_id("zoom_id");
    let shift = ShiftMap::new(vec![100.0, -200.0]);
    let cmp = CmpMap::new(Box::new(zoom), Box::new(shift), Composition::Series).unwrap();
    AnyObject::CmpMap(cmp)
}

#[test]
fn to_native_string_matches_a_channel_set_as_ast_to_string_sets_it() {
    let obj = nested_object();
    let want = channel_write(&obj, |ch| {
        let config = ch.config_mut();
        config.set_comment(false);
        config.set_full(-1);
        config.set_indent(0);
    });
    assert_eq!(obj.to_native_string().unwrap(), want);
    let AnyObject::CmpMap(cmp) = &obj else {
        unreachable!()
    };
    assert_eq!(cmp.to_native_string().unwrap(), want);
}

// ===========================================================================
// 8. Error: read with no source
// ===========================================================================

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
    assert!(cfg.comment());
    assert_eq!(cfg.full(), 0);
    assert_eq!(cfg.indent(), 3);
    assert!(!cfg.skip());
    assert!(!cfg.strict());
    assert_eq!(cfg.report_level(), 1);
    assert!(!cfg.test_comment() && !cfg.test_full() && !cfg.test_indent());
}

#[test]
fn config_mut_is_honoured() {
    let mut ch = NativeChannel::with_string_sink();
    ch.config_mut().set_comment(false);
    ch.config_mut().set_full(-4);
    ch.config_mut().set_indent(5);

    assert!(!ch.config().comment());
    assert_eq!(ch.config().full(), -1);
    assert_eq!(ch.config().indent(), 5);
    assert!(ch.config().test_full());
    ch.config_mut().clear_full();
    assert_eq!(ch.config().full(), 0);
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

// ===========================================================================
// testchannel.c Errors 1-6 and testobject.c TestCheckToString
// ===========================================================================

/// testchannel.c Errors 1-6: a default SkyFrame written to a file and read
/// back is equal to the original, for two files in turn. C's Error 7 (a
/// read while `SinkFile` is still set) has no counterpart: a Channel here is
/// built to write or to read.
#[test]
fn default_skyframe_file_round_trip_is_equal() {
    let sf = SkyFrame::new(SkySystem::Icrs);
    let obj = AnyObject::SkyFrame(sf.clone());
    for (n, name) in ["ast_testchannel_fred.txt", "ast_testchannel_fred2.txt"]
        .into_iter()
        .enumerate()
    {
        let path = std::env::temp_dir().join(name);
        let mut ch = NativeChannel::to_file(&path).unwrap();
        ch.write(&obj).unwrap();
        drop(ch);
        let restored = NativeChannel::from_file(&path).unwrap().read().unwrap();
        let _ = std::fs::remove_file(&path);
        let restored = restored.unwrap_or_else(|| panic!("Error {}", 2 + 3 * n));
        let frame = restored
            .as_frame()
            .expect("a SkyFrame reads back as a Frame");
        assert!(frame.equal(&sf), "Error {}", 3 + 3 * n);
    }
}

/// testobject.c `TestCheckToString`: the FrameSet `convert_to` makes from a
/// SKY-domain Frame to a SkyFrame writes to a string, reads back, writes
/// the same string again, and is equal to the original.
#[test]
fn converted_frameset_string_round_trip() {
    use ast_core::frames::Frame;
    let mut sf = SkyFrame::new(SkySystem::Icrs);
    let mut bf = BasicFrameBuilder::new(2).build().unwrap();
    bf.set_domain("SKY");
    let fs = bf
        .convert_to(&mut sf, &[])
        .unwrap()
        .expect("astConvert failed");
    let pickle1 = native_dump(&AnyObject::FrameSet(fs.clone())).unwrap();
    let AnyObject::FrameSet(fs2) = from_native_string(&pickle1).unwrap() else {
        panic!("the string did not read back as a FrameSet");
    };
    let pickle2 = native_dump(&AnyObject::FrameSet(fs2.clone())).unwrap();
    assert_eq!(pickle1, pickle2, "round-tripped strings differ");
    assert!(
        Frame::equal(&fs2, &fs),
        "reloaded Object is not equal to the original"
    );
}

// ===========================================================================
// SourceFile and SinkFile
// ===========================================================================

/// The native dump of a ZoomMap with `nin` inputs.
fn zoom_dump(nin: usize) -> String {
    native_dump(&AnyObject::ZoomMap(ZoomMap::new(nin, 2.0).unwrap())).unwrap()
}

/// A path in the temporary directory that no other test uses.
fn scratch_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ast_channel_{}_{name}", std::process::id()))
}

fn nin_of(object: AnyObject) -> usize {
    object.as_mapping().unwrap().nin()
}

/// SourceFile is read in place of the source; clearing it restores the
/// source, and setting it again reads the file from its start.
// C: channel.c `SetSourceFile` and `ClearSourceFile` close the open file,
// and `ReadData` opens it again on the next read.
#[test]
fn source_file_replaces_source_and_clear_restores_it() {
    let path = scratch_path("source.ast");
    std::fs::write(&path, format!("{}{}", zoom_dump(3), zoom_dump(4))).unwrap();
    let name = path.to_str().unwrap();

    let mut ch = NativeChannel::from_string(&zoom_dump(2));
    assert!(!ch.test_source_file());
    assert_eq!(ch.source_file(), None);

    ch.set_source_file(name);
    assert!(ch.test_source_file());
    assert_eq!(ch.source_file(), Some(name));
    assert_eq!(nin_of(ch.read().unwrap().unwrap()), 3);

    // The file is closed and the next read goes to the source, which no
    // read has touched.
    ch.clear_source_file();
    assert!(!ch.test_source_file());
    assert_eq!(nin_of(ch.read().unwrap().unwrap()), 2);
    assert!(ch.read().unwrap().is_none());

    // A new open starts at the beginning of the file, not where the closed
    // one stopped.
    ch.set_source_file(name);
    assert_eq!(nin_of(ch.read().unwrap().unwrap()), 3);
    // Setting the same name again closes the file and so starts again.
    ch.set_source_file(name);
    assert_eq!(nin_of(ch.read().unwrap().unwrap()), 3);
    assert_eq!(nin_of(ch.read().unwrap().unwrap()), 4);
    assert!(ch.read().unwrap().is_none());
    let _ = std::fs::remove_file(&path);
}

/// SinkFile is written in place of the sink; clearing it restores the sink,
/// and setting it again creates the file anew.
#[test]
fn sink_file_replaces_sink_and_clear_restores_it() {
    let first = scratch_path("sink1.ast");
    let second = scratch_path("sink2.ast");
    let zoom = |nin| AnyObject::ZoomMap(ZoomMap::new(nin, 2.0).unwrap());

    let mut ch = NativeChannel::with_string_sink();
    assert!(!ch.test_sink_file());
    ch.set_sink_file(first.to_str().unwrap());
    assert!(ch.test_sink_file());
    assert_eq!(ch.sink_file(), first.to_str());
    ch.write(&zoom(2)).unwrap();
    ch.write(&zoom(3)).unwrap();

    // A different name closes the first file and writes to the second.
    ch.set_sink_file(second.to_str().unwrap());
    ch.write(&zoom(4)).unwrap();

    // Clearing returns to the sink.
    ch.clear_sink_file();
    assert_eq!(ch.sink_file(), None);
    ch.write(&zoom(5)).unwrap();

    // The first name again replaces the first file's contents.
    ch.set_sink_file(first.to_str().unwrap());
    ch.write(&zoom(6)).unwrap();
    ch.clear_sink_file();

    assert_eq!(
        std::fs::read_to_string(&first).unwrap(),
        zoom_dump(6),
        "the first file was written to by a handle that was not reopened"
    );
    assert_eq!(std::fs::read_to_string(&second).unwrap(), zoom_dump(4));
    assert_eq!(ch.into_string(), zoom_dump(5));
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
}

/// A SourceFile that cannot be opened is an error at the first read, a
/// SinkFile at the first write, and a SourceFile the SinkFile has open is
/// refused.
#[test]
fn source_and_sink_file_open_failures() {
    let missing = scratch_path("no_such_dir").join("x.ast");
    let mut ch = NativeChannel::from_string(&zoom_dump(2));
    ch.set_source_file(missing.to_str().unwrap());
    let e = ch.read().unwrap_err();
    assert_eq!(e.code(), ast_core::error::ErrorCode::Rderr);
    assert!(
        e.message()
            .contains("astRead(Channel): Failed to open input SourceFile"),
        "{e}"
    );

    let mut ch = NativeChannel::with_string_sink();
    ch.set_sink_file(missing.to_str().unwrap());
    let e = ch
        .write(&AnyObject::ZoomMap(ZoomMap::new(2, 2.0).unwrap()))
        .unwrap_err();
    assert_eq!(e.code(), ast_core::error::ErrorCode::Wrerr);
    assert!(
        e.message()
            .contains("astWrite(Channel): Failed to open output SinkFile"),
        "{e}"
    );

    let path = scratch_path("both.ast");
    let name = path.to_str().unwrap();
    let mut ch = NativeChannel::new();
    ch.set_sink_file(name);
    ch.write(&AnyObject::ZoomMap(ZoomMap::new(2, 2.0).unwrap()))
        .unwrap();
    ch.set_source_file(name);
    let e = ch.read().unwrap_err();
    assert_eq!(e.code(), ast_core::error::ErrorCode::Rderr);
    assert!(
        e.message().contains("being used as the output SinkFile"),
        "{e}"
    );
    let _ = std::fs::remove_file(&path);
}

/// A clone has neither attribute, and no file open.
#[test]
fn clone_does_not_inherit_source_file_or_sink_file() {
    let mut ch = NativeChannel::new();
    ch.set_source_file("a.ast");
    ch.set_sink_file("b.ast");
    let copy = ch.clone();
    assert!(!copy.test_source_file());
    assert!(!copy.test_sink_file());
    assert!(ch.test_source_file() && ch.test_sink_file());
}

// ===========================================================================
// Standard input and output
// ===========================================================================

/// The variable that tells a copy of this test program which standard I/O
/// check it is the child of.
const STDIO_CHILD: &str = "AST_TEST_CHANNEL_STDIO_CHILD";

/// Run the test `name` in a child of this test program with standard input
/// `input` and `mode` in [`STDIO_CHILD`], returning its standard output.
///
/// A test that uses the real standard input or output would hang waiting for
/// the harness's terminal or interleave with other tests' output.
fn run_stdio_child(name: &str, mode: &str, input: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(STDIO_CHILD, mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{mode}: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// A NativeChannel with no source reads standard input.
// C: channel.c `ReadData` reads stdin when the Channel has neither a
// SourceFile nor a source function.
#[test]
fn channel_without_source_reads_standard_input() {
    let name = "test_channel::channel_without_source_reads_standard_input";
    match std::env::var(STDIO_CHILD).as_deref() {
        Ok("read") => {
            let mut ch = NativeChannel::new();
            assert_eq!(nin_of(ch.read().unwrap().unwrap()), 3);
            assert_eq!(nin_of(ch.read().unwrap().unwrap()), 4);
            assert!(ch.read().unwrap().is_none());
        }
        Ok(_) => {}
        Err(_) => {
            run_stdio_child(name, "read", &format!("{}{}", zoom_dump(3), zoom_dump(4)));
        }
    }
}

/// A NativeChannel with no sink writes standard output, and clearing the
/// SinkFile returns to it.
// C: channel.c `WriteData` printfs to stdout when the Channel has neither a
// SinkFile nor a sink function.
#[test]
fn channel_without_sink_writes_standard_output() {
    let name = "test_channel::channel_without_sink_writes_standard_output";
    match std::env::var(STDIO_CHILD).as_deref() {
        Ok("write") => {
            let path = scratch_path("stdout_sink.ast");
            let mut ch = NativeChannel::new();
            ch.set_sink_file(path.to_str().unwrap());
            ch.write(&AnyObject::ZoomMap(ZoomMap::new(3, 2.0).unwrap()))
                .unwrap();
            ch.clear_sink_file();
            ch.write(&AnyObject::ZoomMap(ZoomMap::new(4, 2.0).unwrap()))
                .unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), zoom_dump(3));
            let _ = std::fs::remove_file(&path);
        }
        Ok(_) => {}
        Err(_) => {
            let out = run_stdio_child(name, "write", "");
            assert!(out.contains(&zoom_dump(4)), "{out}");
            assert!(!out.contains(&zoom_dump(3)), "{out}");
        }
    }
}
