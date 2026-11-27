// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The pieces of a line of Native text that the Channel's reader
//! ([`crate::serial::native_stream`]) reads: the comment after an unquoted
//! `#`, a scalar value, and a commented-out default (`# Key = Value`).

use crate::serial::node::{NodeItem, NodeValue};

/// Read one Native dump from `text` as a Channel reads it, for tests of
/// the reader and the writer: the Node of its first Object, with no report of
/// the items no loader read.
#[cfg(test)]
pub(crate) fn parse_native(text: &str) -> crate::error::AstResult<crate::serial::node::Node> {
    use crate::error::{AstError, ErrorCode};
    let mut lines = text.lines().map(str::to_string);
    let mut next_line = || lines.next();
    let mut reader = super::native_stream::Reader::new(&mut next_line, "Channel", false, 1);
    reader
        .read(true, true)?
        .map(|object| object.node)
        .ok_or_else(|| AstError::new(ErrorCode::Eochn, "no Object in the text"))
}

/// Split a line at the first `#` that is not inside double quotes.
///
/// Returns `(content_before_hash, optional_comment_text_after_hash)`.
/// The comment text is trimmed of leading/trailing whitespace.
/// If no unquoted `#` is found the whole line is returned as content.
pub(crate) fn split_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_quotes = false;
    for (i, b) in line.bytes().enumerate() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b'#' && !in_quotes {
            let before = &line[..i];
            let after = line[i + 1..].trim();
            let comment = if after.is_empty() { None } else { Some(after) };
            return (before, comment);
        }
    }
    (line, None)
}

/// Parse a scalar value string into a [`NodeValue`].
///
/// Quoted strings have their delimiters removed and internal `""` sequences
/// are un-escaped to a single `"`.  Otherwise the value is tried as an
/// integer (`i64`), then as a float (`f64`), and finally kept as an
/// unquoted string.
pub(crate) fn parse_scalar_value(s: &str) -> NodeValue {
    let s = s.trim();

    // Quoted string.
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let unescaped = inner.replace("\"\"", "\"");
        return NodeValue::String(unescaped);
    }

    // AST__BAD is written externally as the unquoted token `<bad>`
    // (channel.c:146); read it back as the bad double so `get_double`
    // returns it directly.
    if s == "<bad>" {
        return NodeValue::Double(crate::constants::BAD_VALUE);
    }

    // Integer.
    if let Ok(v) = s.parse::<i64>() {
        return NodeValue::Int(v);
    }

    // Floating-point (reject inf/nan which Rust parses but AST never emits).
    if !s.eq_ignore_ascii_case("inf")
        && !s.eq_ignore_ascii_case("infinity")
        && !s.eq_ignore_ascii_case("nan")
    {
        if let Ok(v) = s.parse::<f64>() {
            return NodeValue::Double(v);
        }
    }

    // Unquoted string (e.g. `GALACTIC`).
    NodeValue::String(s.to_string())
}

/// Try to interpret text after a leading `#` as a commented-out attribute
/// default (`# Key = Value # comment`).  Returns `None` when the text does
/// not match the `Key = Value` pattern (i.e. it is a plain comment).
pub(crate) fn try_parse_commented_default(after_hash: &str) -> Option<NodeItem> {
    let trimmed = after_hash.trim_start();

    // Key: starts with an ASCII letter, then alphanumeric / underscore.
    let key_end = trimmed.find(|c: char| !c.is_alphanumeric() && c != '_')?;
    let key = &trimmed[..key_end];
    if key.is_empty() || !key.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }

    // Must be followed (after optional whitespace) by `=`.
    let rest = trimmed[key_end..].trim_start();
    if !rest.starts_with('=') {
        return None;
    }
    let after_eq = &rest[1..];

    let (value_str, comment) = split_comment(after_eq);
    let value_str = value_str.trim();

    let inner_value = if value_str.is_empty() {
        // Commented-out empty value — shouldn't normally occur, but
        // represent it as an empty string to be safe.
        NodeValue::String(String::new())
    } else {
        parse_scalar_value(value_str)
    };

    Some(NodeItem {
        key: key.to_string(),
        value: NodeValue::Unset(Box::new(inner_value)),
        text: None,
        comment: comment.map(|s| s.to_string()),
        read: Default::default(),
        helpful: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::serial::node::Node;

    #[test]
    fn split_comment_basic() {
        let (before, after) = split_comment("Nin = 4 \t# Number of input coordinates");
        assert_eq!(before, "Nin = 4 \t");
        assert_eq!(after, Some("Number of input coordinates"));
    }

    #[test]
    fn split_comment_no_hash() {
        let (before, after) = split_comment("L2 = 1.5");
        assert_eq!(before, "L2 = 1.5");
        assert_eq!(after, None);
    }

    #[test]
    fn split_comment_hash_in_quotes() {
        let (before, after) = split_comment(r#"Fmt = "value # with hash" 	# real comment"#);
        assert_eq!(before, "Fmt = \"value # with hash\" \t");
        assert_eq!(after, Some("real comment"));
    }

    #[test]
    fn split_comment_bare_hash() {
        let (before, after) = split_comment("something #");
        assert_eq!(before, "something ");
        assert_eq!(after, None);
    }

    #[test]
    fn parse_scalar_integer() {
        assert_eq!(parse_scalar_value("42"), NodeValue::Int(42));
        assert_eq!(parse_scalar_value("-129"), NodeValue::Int(-129));
        assert_eq!(parse_scalar_value("0"), NodeValue::Int(0));
    }

    #[test]
    fn parse_scalar_double() {
        assert_eq!(parse_scalar_value("0.5"), NodeValue::Double(0.5));
        assert_eq!(parse_scalar_value("-0.5"), NodeValue::Double(-0.5));
        assert_eq!(
            parse_scalar_value("5.23598775598299e-06"),
            NodeValue::Double(5.23598775598299e-06)
        );
        assert_eq!(parse_scalar_value("2e-06"), NodeValue::Double(2e-06));
    }

    #[test]
    fn parse_scalar_quoted_string() {
        assert_eq!(
            parse_scalar_value(r#""Full""#),
            NodeValue::String("Full".to_string())
        );
        assert_eq!(
            parse_scalar_value(r#""""#),
            NodeValue::String(String::new())
        );
    }

    #[test]
    fn parse_scalar_quoted_string_with_escaped_quotes() {
        assert_eq!(
            parse_scalar_value(r#""has""quotes""#),
            NodeValue::String("has\"quotes".to_string())
        );
    }

    #[test]
    fn parse_scalar_unquoted_string() {
        assert_eq!(
            parse_scalar_value("GALACTIC"),
            NodeValue::String("GALACTIC".to_string())
        );
    }

    #[test]
    fn parse_commented_default() {
        let item =
            try_parse_commented_default("   Title = \"test\" \t# Title of coordinate system")
                .unwrap();
        assert_eq!(item.key, "Title");
        assert_eq!(
            item.value,
            NodeValue::Unset(Box::new(NodeValue::String("test".to_string())))
        );
        assert_eq!(item.comment, Some("Title of coordinate system".to_string()));
    }

    #[test]
    fn parse_commented_default_int() {
        let item = try_parse_commented_default("   Base = 1 \t# Index of base Frame").unwrap();
        assert_eq!(item.key, "Base");
        assert_eq!(item.value, NodeValue::Unset(Box::new(NodeValue::Int(1))));
    }

    #[test]
    fn plain_comment_not_default() {
        // A line like "# This is a comment" should not match.
        assert!(try_parse_commented_default(" This is a comment").is_none());
        // Empty after hash.
        assert!(try_parse_commented_default("").is_none());
        assert!(try_parse_commented_default("  ").is_none());
    }

    #[test]
    fn parse_simple_object() {
        let text = r#"
 Begin ZoomMap 	# Zoom about the origin
    Nin = 2 	# Number of input coordinates
    Invert = 0 	# Mapping not inverted
 IsA Mapping 	# Mapping between coordinate systems
    Zoom = 5.23598775598299e-06 	# Zoom factor
 End ZoomMap
"#;
        let node = parse_native(text).unwrap();
        assert_eq!(node.class, "ZoomMap");
        assert_eq!(
            node.class_comment,
            Some("Zoom about the origin".to_string())
        );
        assert_eq!(node.get_int("Nin", 0), 2);
        assert_eq!(node.get_int("Invert", -1), 0);
        assert_eq!(node.get_double("Zoom", 0.0), 5.23598775598299e-06);

        // Check IsA marker is present.
        let isa = node
            .items
            .iter()
            .find(|i| i.key == "IsA")
            .expect("IsA marker missing");
        assert_eq!(isa.value, NodeValue::String("Mapping".to_string()));
    }

    #[test]
    fn parse_nested_object() {
        let text = r#"
 Begin CmpMap 	# Compound Mapping
    Nin = 2 	# Number of input coordinates
 IsA Mapping 	# Mapping between coordinate systems
    MapA = 	# First component Mapping
       Begin UnitMap 	# Unit (null) Mapping
          Nin = 2 	# Number of input coordinates
       IsA Mapping 	# Mapping between coordinate systems
       End UnitMap
    MapB = 	# Second component Mapping
       Begin ZoomMap 	# Zoom about the origin
          Nin = 2 	# Number of input coordinates
       IsA Mapping 	# Mapping between coordinate systems
          Zoom = 2 	# Zoom factor
       End ZoomMap
 End CmpMap
"#;
        let node = parse_native(text).unwrap();
        assert_eq!(node.class, "CmpMap");

        let map_a = node.get_object("MapA").expect("MapA missing");
        assert_eq!(map_a.class, "UnitMap");

        let map_b = node.get_object("MapB").expect("MapB missing");
        assert_eq!(map_b.class, "ZoomMap");
        assert_eq!(map_b.get_int("Zoom", 0), 2);
    }

    #[test]
    fn parse_commented_defaults_in_object() {
        let text = r#"
 Begin FrameSet 	# Set of inter-related coordinate systems
#   Title = "test title" 	# Title of coordinate system
#   Naxes = 3 	# Number of coordinate axes
    ActUnt = 0 	# Unit strings do not affect alignment
 IsA Frame 	# Coordinate system description
    Nframe = 1 	# Number of Frames in FrameSet
 End FrameSet
"#;
        let node = parse_native(text).unwrap();
        assert_eq!(node.class, "FrameSet");

        // Title should be Unset, so get() returns None.
        assert!(node.get("Title").is_none());

        // But the Unset item should be present in items.
        let title_item = node
            .items
            .iter()
            .find(|i| i.key == "Title")
            .expect("Title item missing");
        assert!(matches!(&title_item.value, NodeValue::Unset(_)));

        // ActUnt should be a regular value.
        assert_eq!(node.get_int("ActUnt", -1), 0);
    }

    #[test]
    fn parse_begin_no_comment() {
        let text = "  Begin FrameSet\n  IsA Frame\n  End FrameSet\n";
        let node = parse_native(text).unwrap();
        assert_eq!(node.class, "FrameSet");
        assert_eq!(node.class_comment, None);
    }

    #[test]
    fn parse_leading_comments_skipped() {
        let text = r#"
#
#  This is a comment block.
#
 Begin UnitMap 	# Unit (null) Mapping
    Nin = 1 	# Number of input coordinates
 IsA Mapping 	# Mapping between coordinate systems
 End UnitMap
"#;
        let node = parse_native(text).unwrap();
        assert_eq!(node.class, "UnitMap");
        assert_eq!(node.get_int("Nin", 0), 1);
    }

    #[test]
    fn parse_value_no_comment() {
        let text = " Begin LutMap\n IsA Mapping\n    L2 = 1.5\n End LutMap\n";
        let node = parse_native(text).unwrap();
        assert_eq!(node.get_double("L2", 0.0), 1.5);

        let item = node.items.iter().find(|i| i.key == "L2").unwrap();
        assert_eq!(item.comment, None);
    }

    #[test]
    fn parse_empty_string_value() {
        let text = " Begin Axis\n    Unit = \"\" \t# Axis units\n End Axis\n";
        let node = parse_native(text).unwrap();
        assert_eq!(node.get_string("Unit", "default"), "");
    }

    #[test]
    fn error_missing_begin() {
        let result = parse_native("  ");
        assert!(result.is_err());
    }

    #[test]
    fn error_unknown_class() {
        let text = " Begin Foo\n End Foo\n";
        let err = parse_native(text).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Ocluk, "{err}");
    }

    #[test]
    fn error_mismatched_end() {
        let text = " Begin UnitMap\n    Nin = 1\n IsA Mapping\n End ZoomMap\n";
        let err = parse_native(text).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Badin, "{err}");
    }

    #[test]
    fn error_no_end() {
        let text = " Begin UnitMap\n    Nin = 1\n";
        let err = parse_native(text).unwrap_err();
        assert!(err.to_string().contains("Unexpected end of input"), "{err}");
    }

    // ---------------------------------------------------------------
    // Smoke tests against real reference files (if the submodule is
    // present). These verify the parser handles real-world complexity.
    // ---------------------------------------------------------------

    /// Helper: read a reference file if available, returning None if the
    /// submodule is not initialized.
    fn read_reference_file(relative: &str) -> Option<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../reference/starlink-ast/ast_tester")
            .join(relative);
        std::fs::read_to_string(path).ok()
    }

    /// Count all Object nodes in a tree (including the root).
    fn count_nodes(node: &Node) -> usize {
        let mut count = 1;
        for item in &node.items {
            if let NodeValue::Object(child) = &item.value {
                count += count_nodes(child);
            }
        }
        count
    }

    #[test]
    fn smoke_splittest1_ast() {
        let Some(text) = read_reference_file("splittest1.ast") else {
            eprintln!("skipping: reference submodule not available");
            return;
        };
        let node = parse_native(&text).unwrap();
        assert_eq!(node.class, "CmpMap");
        assert_eq!(node.get_int("Nin", 0), 4);

        // The file has nested CmpMaps, WinMaps, MatrixMap, LutMaps.
        let total = count_nodes(&node);
        assert!(total >= 15, "expected at least 15 objects, got {total}");

        // Spot-check: top-level MapA should be a UnitMap.
        let map_a = node.get_object("MapA").expect("MapA missing");
        assert_eq!(map_a.class, "UnitMap");
        assert_eq!(map_a.get_int("Nin", 0), 4);
        assert_eq!(map_a.get_int("Invert", -1), 1);

        // Check a commented-out default (the `# Incr = 1` lines).
        // They should be parsed as Unset inside the LutMap objects.
        // Drill down: CmpMap -> MapB -> CmpMap -> MapA -> WinMap (skip)
        //  -> MapB -> CmpMap -> ... -> LutMap
        // Instead just verify that at least one Unset Incr exists.
        fn has_unset_incr(node: &Node) -> bool {
            for item in &node.items {
                if item.key == "Incr" && matches!(&item.value, NodeValue::Unset(_)) {
                    return true;
                }
                if let NodeValue::Object(child) = &item.value {
                    if has_unset_incr(child) {
                        return true;
                    }
                }
            }
            false
        }
        assert!(has_unset_incr(&node), "should have at least one Unset Incr");
    }

    #[test]
    fn smoke_degen1_ast() {
        let Some(text) = read_reference_file("degen1.ast") else {
            eprintln!("skipping: reference submodule not available");
            return;
        };
        let node = parse_native(&text).unwrap();
        assert_eq!(node.class, "FrameSet");

        // FrameSet has Nframe = 4.
        assert_eq!(node.get_int("Nframe", 0), 4);
        assert_eq!(node.get_int("Currnt", 0), 4);

        // Frame 4 should be a CmpFrame.
        let frm4 = node.get_object("Frm4").expect("Frm4 missing");
        assert_eq!(frm4.class, "CmpFrame");

        // The CmpFrame's FrameA should be a SkyFrame.
        let frame_a = frm4.get_object("FrameA").expect("FrameA missing");
        assert_eq!(frame_a.class, "SkyFrame");
        assert_eq!(frame_a.get_string("System", ""), "FK5");
    }

    #[test]
    fn smoke_brad_map() {
        let Some(text) = read_reference_file("brad.map") else {
            eprintln!("skipping: reference submodule not available");
            return;
        };
        let node = parse_native(&text).unwrap();
        assert_eq!(node.class, "CmpMap");
        assert_eq!(node.get_int("Nin", 0), 3);

        // MapA should be a CmpMap (spectral axis).
        let map_a = node.get_object("MapA").expect("MapA missing");
        assert_eq!(map_a.class, "CmpMap");

        // MapB should be a CmpMap (sky axes).
        let map_b = node.get_object("MapB").expect("MapB missing");
        assert_eq!(map_b.class, "CmpMap");

        // Inside MapB, the MapA is a ZoomMap.
        let zoom = map_b.get_object("MapA").expect("MapB.MapA missing");
        assert_eq!(zoom.class, "ZoomMap");
        assert_eq!(zoom.get_double("Zoom", 0.0), 5.23598775598299e-06);
    }

    // ---------------------------------------------------------------
    // Parse-only reference tests against committed fixture files.
    // These always run (no submodule required).
    // ---------------------------------------------------------------

    /// Read a committed fixture, named by its path relative to the fixture
    /// root -- the same paths the upstream harness reads.
    fn read_committed_fixture(relpath: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(relpath);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
    }

    /// Collect all distinct class names in a Node tree.
    fn collect_classes(node: &Node, classes: &mut std::collections::BTreeSet<String>) {
        classes.insert(node.class.clone());
        for item in &node.items {
            if let NodeValue::Object(child) = &item.value {
                collect_classes(child, classes);
            }
        }
    }

    #[test]
    fn ref_splittest1_parse() {
        let text = read_committed_fixture("programs/testcmpmap/splittest1.ast");
        let node = parse_native(&text).unwrap();

        // Top-level structure.
        assert_eq!(node.class, "CmpMap");
        assert_eq!(node.get_int("Nin", 0), 4);
        assert_eq!(node.class_comment.as_deref(), Some("Compound Mapping"));

        // Object count: 17 Begin lines in the file.
        let total = count_nodes(&node);
        assert_eq!(total, 17);

        // Top-level children.
        let map_a = node.get_object("MapA").expect("MapA missing");
        assert_eq!(map_a.class, "UnitMap");
        assert_eq!(map_a.get_int("Nin", 0), 4);
        assert_eq!(map_a.get_int("Invert", -1), 1);

        let map_b = node.get_object("MapB").expect("MapB missing");
        assert_eq!(map_b.class, "CmpMap");

        // Verify class diversity: file contains CmpMap, UnitMap, WinMap,
        // LutMap, MatrixMap at minimum.
        let mut classes = std::collections::BTreeSet::new();
        collect_classes(&node, &mut classes);
        for expected in &["CmpMap", "UnitMap", "WinMap", "LutMap", "MatrixMap"] {
            assert!(
                classes.contains(*expected),
                "expected class {expected} not found; have: {classes:?}"
            );
        }

        // Commented-out defaults: at least one Unset "Incr" deep inside.
        fn has_unset_key(node: &Node, key: &str) -> bool {
            for item in &node.items {
                if item.key == key && matches!(&item.value, NodeValue::Unset(_)) {
                    return true;
                }
                if let NodeValue::Object(child) = &item.value {
                    if has_unset_key(child, key) {
                        return true;
                    }
                }
            }
            false
        }
        assert!(
            has_unset_key(&node, "Incr"),
            "should have at least one Unset Incr"
        );
    }

    #[test]
    fn ref_degen1_parse() {
        let text = read_committed_fixture("wcsconv/inputs/degen1.ast");
        let node = parse_native(&text).unwrap();

        // Top-level FrameSet with leading comment block.
        assert_eq!(node.class, "FrameSet");
        assert_eq!(node.get_int("Nframe", 0), 4);
        assert_eq!(node.get_int("Currnt", 0), 4);
        // Base is a commented-out default (Unset), so get_int returns
        // the fallback. Verify the Unset item is present instead.
        assert!(node.get("Base").is_none(), "Base should be Unset");
        let base_item = node
            .items
            .iter()
            .find(|i| i.key == "Base")
            .expect("Base item missing");
        assert!(matches!(&base_item.value, NodeValue::Unset(_)));

        // Object count: 38 Begin lines.
        let total = count_nodes(&node);
        assert_eq!(total, 38);

        // Frame 4 is a CmpFrame containing a SkyFrame.
        let frm4 = node.get_object("Frm4").expect("Frm4 missing");
        assert_eq!(frm4.class, "CmpFrame");
        let frame_a = frm4.get_object("FrameA").expect("FrameA missing");
        assert_eq!(frame_a.class, "SkyFrame");
        assert_eq!(frame_a.get_string("System", ""), "FK5");

        // SkyFrame should have SkyAxis children.
        let ax1 = frame_a.get_object("Ax1").expect("SkyFrame Ax1 missing");
        assert_eq!(ax1.class, "SkyAxis");

        // Class diversity: should include FrameSet, CmpFrame, SkyFrame,
        // Frame, SkyAxis, Axis, CmpMap, PermMap, WcsMap, SphMap, etc.
        let mut classes = std::collections::BTreeSet::new();
        collect_classes(&node, &mut classes);
        for expected in &[
            "FrameSet", "CmpFrame", "SkyFrame", "Frame", "SkyAxis", "CmpMap", "PermMap", "WcsMap",
            "SphMap", "UnitMap",
        ] {
            assert!(
                classes.contains(*expected),
                "expected class {expected} not found; have: {classes:?}"
            );
        }
    }

    #[test]
    #[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
    fn ref_brad_map_parse() {
        let text = read_committed_fixture("simplify/brad.map");
        let node = parse_native(&text).unwrap();

        // Top-level parallel CmpMap with 3 inputs.
        assert_eq!(node.class, "CmpMap");
        assert_eq!(node.get_int("Nin", 0), 3);
        assert_eq!(node.get_int("Series", 0), 0); // parallel

        // Object count: 7 Begin lines.
        let total = count_nodes(&node);
        assert_eq!(total, 7);

        // MapA is a CmpMap (spectral axis).
        let map_a = node.get_object("MapA").expect("MapA missing");
        assert_eq!(map_a.class, "CmpMap");

        // MapB is a CmpMap (sky axes).
        let map_b = node.get_object("MapB").expect("MapB missing");
        assert_eq!(map_b.class, "CmpMap");

        // MapB.MapA is a ZoomMap with a known zoom factor.
        let zoom = map_b.get_object("MapA").expect("MapB.MapA missing");
        assert_eq!(zoom.class, "ZoomMap");
        assert_eq!(zoom.get_double("Zoom", 0.0), 5.23598775598299e-06);

        // MapB.MapB is a WcsMap (TAN projection).
        let wcsmap = map_b.get_object("MapB").expect("MapB.MapB missing");
        assert_eq!(wcsmap.class, "WcsMap");
        assert_eq!(wcsmap.get_string("Type", ""), "TAN");

        // Class diversity.
        let mut classes = std::collections::BTreeSet::new();
        collect_classes(&node, &mut classes);
        for expected in &["CmpMap", "ZoomMap", "WcsMap", "WinMap", "LutMap"] {
            assert!(
                classes.contains(*expected),
                "expected class {expected} not found; have: {classes:?}"
            );
        }
    }

    #[test]
    fn ref_specflux_parse() {
        let text = read_committed_fixture("wcsconv/inputs/specflux.ast");
        let node = parse_native(&text).unwrap();

        // Top-level CmpFrame.
        assert_eq!(node.class, "CmpFrame");
        assert_eq!(
            node.class_comment.as_deref(),
            Some("Compound coordinate system description")
        );

        // Object count: 5 Begin lines.
        let total = count_nodes(&node);
        assert_eq!(total, 5);

        // FrameA is a SpecFrame.
        let frame_a = node.get_object("FrameA").expect("FrameA missing");
        assert_eq!(frame_a.class, "SpecFrame");
        assert_eq!(frame_a.get_int("Naxes", 0), 1);
        assert_eq!(frame_a.get_string("System", ""), "WAVE");

        // SpecFrame has an Axis child with wavelength units.
        let spec_ax = frame_a.get_object("Ax1").expect("SpecFrame Ax1 missing");
        assert_eq!(spec_ax.class, "Axis");
        assert_eq!(spec_ax.get_string("Unit", ""), "um");

        // FrameB is a basic Frame.
        let frame_b = node.get_object("FrameB").expect("FrameB missing");
        assert_eq!(frame_b.class, "Frame");
        assert_eq!(frame_b.get_int("Naxes", 0), 1);

        // FrameB axis has flux-density units with AST escape sequences.
        let flux_ax = frame_b.get_object("Ax1").expect("FrameB Ax1 missing");
        assert_eq!(flux_ax.class, "Axis");
        let flux_unit = flux_ax.get_string("Unit", "");
        assert!(
            flux_unit.contains("W/m"),
            "expected flux unit containing 'W/m', got: {flux_unit}"
        );

        // Commented-out defaults: Title, Naxes, Domain, labels, units
        // should all be Unset on the top-level CmpFrame.
        let title_item = node
            .items
            .iter()
            .find(|i| i.key == "Title")
            .expect("Title item missing");
        assert!(
            matches!(&title_item.value, NodeValue::Unset(_)),
            "Title should be Unset"
        );
        let domain_item = node
            .items
            .iter()
            .find(|i| i.key == "Domain")
            .expect("Domain item missing");
        assert!(
            matches!(&domain_item.value, NodeValue::Unset(_)),
            "Domain should be Unset"
        );
    }
}
