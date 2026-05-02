//! Native text format parser: text -> Node tree.
//!
//! Reads AST's plain-text "Native" serialisation format and produces a
//! class-agnostic [`Node`] tree. The format is line-oriented:
//!
//! - `Begin ClassName # comment` starts an object block.
//! - `Key = Value # comment` is a scalar attribute.
//! - `Key = \t# comment` (empty value) introduces a nested object on the
//!   next line.
//! - `IsA ClassName # comment` is an inheritance marker.
//! - `End ClassName` closes the block.
//! - `# Key = Value # comment` is a commented-out default (Unset).
//! - Other `#` lines are plain comments and are skipped.

use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::node::{Node, NodeItem, NodeValue};

/// Parse a Native-format text string into a [`Node`] tree.
///
/// The input should contain a single top-level object (one `Begin`/`End`
/// pair). Leading blank lines and comment lines before the first `Begin`
/// are silently skipped.
pub(crate) fn parse_native(text: &str) -> AstResult<Node> {
    let lines: Vec<&str> = text.lines().collect();
    let mut pos = 0;

    // Skip leading blank lines and comment-only lines until we find Begin.
    while pos < lines.len() {
        let trimmed = lines[pos].trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            pos += 1;
            continue;
        }
        break;
    }

    if pos >= lines.len() {
        return Err(AstError::new(
            ErrorCode::Rderr,
            "no Begin found in native text",
        ));
    }

    let trimmed = lines[pos].trim();
    let (class, class_comment) = parse_begin_line(trimmed)?;
    pos += 1;

    let (node, _end_pos) = parse_object(&lines, pos, &class, class_comment)?;
    Ok(node)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse a `Begin ClassName # comment` line.
///
/// Returns `(class_name, optional_comment)`.
fn parse_begin_line(trimmed: &str) -> AstResult<(String, Option<String>)> {
    if !trimmed.starts_with("Begin ") {
        return Err(AstError::new(
            ErrorCode::Rderr,
            format!("expected 'Begin', got: {trimmed}"),
        ));
    }

    let rest = &trimmed["Begin ".len()..];
    let (content, comment) = split_comment(rest);
    let class = content.trim().to_string();

    if class.is_empty() {
        return Err(AstError::new(ErrorCode::Rderr, "empty class name in Begin"));
    }

    Ok((class, comment.map(|s| s.to_string())))
}

/// Split a line at the first `#` that is not inside double quotes.
///
/// Returns `(content_before_hash, optional_comment_text_after_hash)`.
/// The comment text is trimmed of leading/trailing whitespace.
/// If no unquoted `#` is found the whole line is returned as content.
fn split_comment(line: &str) -> (&str, Option<&str>) {
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

/// Find the byte position of the first `=` that is not inside double
/// quotes. Returns `None` if there is no such `=`.
fn find_equals(line: &str) -> Option<usize> {
    let mut in_quotes = false;
    for (i, b) in line.bytes().enumerate() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b'=' && !in_quotes {
            return Some(i);
        }
    }
    None
}

/// Parse a scalar value string into a [`NodeValue`].
///
/// Quoted strings have their delimiters removed and internal `""` sequences
/// are un-escaped to a single `"`.  Otherwise the value is tried as an
/// integer (`i64`), then as a float (`f64`), and finally kept as an
/// unquoted string.
fn parse_scalar_value(s: &str) -> NodeValue {
    let s = s.trim();

    // Quoted string.
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let unescaped = inner.replace("\"\"", "\"");
        return NodeValue::String(unescaped);
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
fn try_parse_commented_default(after_hash: &str) -> Option<NodeItem> {
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
        comment: comment.map(|s| s.to_string()),
    })
}

/// Parse the body of an object block (lines after `Begin` up to and
/// including `End`).
///
/// Returns `(node, line_index_after_End)`.
fn parse_object(
    lines: &[&str],
    start: usize,
    class: &str,
    class_comment: Option<String>,
) -> AstResult<(Node, usize)> {
    let mut node = Node::new(class);
    node.class_comment = class_comment;
    let mut pos = start;

    while pos < lines.len() {
        let line = lines[pos];
        let trimmed = line.trim();

        // Skip blank lines.
        if trimmed.is_empty() {
            pos += 1;
            continue;
        }

        // End marker — must match the class name.
        if let Some(rest) = trimmed.strip_prefix("End ") {
            let (end_class, _comment) = split_comment(rest);
            let end_class = end_class.trim();
            if end_class != class {
                return Err(AstError::new(
                    ErrorCode::Rderr,
                    format!(
                        "mismatched End on line {}: expected '{class}', got '{end_class}'",
                        pos + 1,
                    ),
                ));
            }
            return Ok((node, pos + 1));
        }

        // IsA marker.
        if let Some(rest) = trimmed.strip_prefix("IsA ") {
            let (content, comment) = split_comment(rest);
            let isa_class = content.trim();
            node.push_isa(isa_class, comment.map(|s| s.to_string()));
            pos += 1;
            continue;
        }

        // Comment line: either a commented-out default or a plain comment.
        if let Some(after_hash) = trimmed.strip_prefix('#') {
            if let Some(item) = try_parse_commented_default(after_hash) {
                node.items.push(item);
            }
            // Plain comments are silently skipped.
            pos += 1;
            continue;
        }

        // Attribute: Key = Value # comment
        if let Some(eq_pos) = find_equals(trimmed) {
            let key = trimmed[..eq_pos].trim();
            let after_eq = &trimmed[eq_pos + 1..];

            let (value_str, comment) = split_comment(after_eq);
            let value_str = value_str.trim();

            if value_str.is_empty() {
                // Empty value → nested object on the next non-blank line.
                pos += 1;
                while pos < lines.len() && lines[pos].trim().is_empty() {
                    pos += 1;
                }

                if pos >= lines.len() {
                    return Err(AstError::new(
                        ErrorCode::Rderr,
                        format!(
                            "expected Begin for nested object after '{key}', \
                             got end of input"
                        ),
                    ));
                }

                let next_trimmed = lines[pos].trim();
                let (child_class, child_comment) = parse_begin_line(next_trimmed)?;
                pos += 1;
                let (child_node, new_pos) = parse_object(lines, pos, &child_class, child_comment)?;

                node.push_object(key, child_node, comment.map(|s| s.to_string()));
                pos = new_pos;
            } else {
                // Scalar value.
                let value = parse_scalar_value(value_str);
                node.items.push(NodeItem {
                    key: key.to_string(),
                    value,
                    comment: comment.map(|s| s.to_string()),
                });
                pos += 1;
            }
        } else {
            return Err(AstError::new(
                ErrorCode::Rderr,
                format!("unexpected line {}: {trimmed}", pos + 1),
            ));
        }
    }

    Err(AstError::new(
        ErrorCode::Rderr,
        format!("unexpected end of input: no End found for class '{class}'"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn error_mismatched_end() {
        let text = " Begin Foo\n End Bar\n";
        let result = parse_native(text);
        assert!(result.is_err());
        let msg = result.unwrap_err().message().to_string();
        assert!(msg.contains("mismatched End"));
    }

    #[test]
    fn error_no_end() {
        let text = " Begin Foo\n    Nin = 1\n";
        let result = parse_native(text);
        assert!(result.is_err());
        let msg = result.unwrap_err().message().to_string();
        assert!(msg.contains("no End found"));
    }

    // ---------------------------------------------------------------
    // Smoke tests against real reference files (if the submodule is
    // present). These verify the parser handles real-world complexity.
    // ---------------------------------------------------------------

    /// Helper: read a reference file if available, returning None if the
    /// submodule is not initialised.
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
}
