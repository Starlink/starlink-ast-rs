// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Native text format renderer: Node tree -> text.
//!
//! Converts a [`Node`] tree back to AST's plain-text "Native" serialization
//! format.  The output must be byte-exact with the C library's `astWrite`
//! output for round-trip tests.

use crate::error::AstResult;
use crate::serial::node::{Node, NodeValue};

/// Number of significant digits for double formatting, matching C11's
/// `DBL_DECIMAL_DIG` (AST's `AST__DBL_DIG`).
pub(crate) const DBL_DIG: usize = 17;

// ---------------------------------------------------------------------------
// Double formatting — faithful reproduction of C's `sprintf(buf, "%.*g", prec, value)`
// ---------------------------------------------------------------------------

/// Format a double using C's `%.*g` conventions, the way `WriteDouble`
/// does it (`channel.c:4323-4468`): both `0.0` and `-0.0` produce `"0"`,
/// via `WriteDouble`'s own explicit `if(!strcmp(buff,"-0"))` fixup.
///
/// Every current caller of this function writes attribute values that
/// went through `WriteDouble` in C, so the fixup belongs here. A caller
/// that instead needs `%.*g` without the negative-zero fixup — as
/// `ConvertValue` does — should use [`format_double_g_signed`].
///
/// Rules (from the C standard):
///
/// - Uses scientific notation if exponent < -4 or >= precision.
/// - Otherwise uses fixed notation.
/// - Trailing zeros after the decimal point are trimmed.
/// - Trailing decimal point is trimmed.
/// - Scientific-notation exponent: `e+NN` or `e-NN` (at least 2 digits,
///   always signed).
pub(crate) fn format_double_g(value: f64, precision: usize) -> String {
    // WriteDouble normalizes both +0 and -0 to "0".
    if value == 0.0 {
        return "0".to_string();
    }
    format_double_g_signed(value, precision)
}

/// Format a double using C's `%.*g` conventions, with no negative-zero
/// fixup: `sprintf(buf, "%.*g", precision, value)` verbatim, so `-0.0`
/// formats as `"-0"`.
///
/// This is the formatter `ConvertValue`'s Double/Float-to-String arms use
/// (`keymap.c:1975-1988`, `keymap.c:2036`) — unlike `WriteDouble`, neither
/// arm has a negative-zero fixup.
pub(crate) fn format_double_g_signed(value: f64, precision: usize) -> String {
    // NaN / Inf — should not appear in valid AST data, but handle
    // gracefully.
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }

    let prec = if precision == 0 { 1 } else { precision };

    // Format in scientific notation to determine the (rounded) exponent.
    // Rust's `{:.Ne}` gives N decimal places = prec-1 significant digits
    // after the leading digit (= prec significant digits total).
    let dec_places = prec - 1;
    let sci = format!("{:.dec_places$e}", value);

    // Parse the exponent from Rust's output (e.g. "3.14e0", "-1.5e-7").
    let e_pos = sci
        .rfind('e')
        .expect("scientific notation must contain 'e'");
    let exp: i32 = sci[e_pos + 1..].parse().expect("exponent must parse");

    if exp >= -4 && exp < prec as i32 {
        // Fixed notation.  Number of decimal places chosen so that the
        // total significant digits equals `prec`.
        let fixed_dec = if prec as i32 - 1 - exp > 0 {
            (prec as i32 - 1 - exp) as usize
        } else {
            0
        };
        let fixed = format!("{:.fixed_dec$}", value);
        strip_trailing_zeros(fixed)
    } else {
        // Scientific notation.  Re-use the mantissa from the already-
        // formatted string and fix up the exponent to match C conventions.
        let mantissa = strip_trailing_zeros(sci[..e_pos].to_string());
        format_c_exponent(&mantissa, exp)
    }
}

/// Strip trailing zeros after a decimal point, and then the decimal point
/// itself if it becomes trailing.
fn strip_trailing_zeros(mut s: String) -> String {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Format a mantissa + exponent in C `%e` style: `e+NN` / `e-NN` with at
/// least 2 exponent digits and an explicit sign.
fn format_c_exponent(mantissa: &str, exp: i32) -> String {
    if exp >= 0 {
        // Positive exponents >= 100 naturally get 3+ digits from {:02}.
        format!("{}e+{:02}", mantissa, exp)
    } else {
        format!("{}e-{:02}", mantissa, exp.unsigned_abs())
    }
}

// ---------------------------------------------------------------------------
// Native text renderer
// ---------------------------------------------------------------------------

/// Options controlling Native format output.
#[derive(Debug, Clone)]
pub(crate) struct RenderOptions {
    /// Include comment text after `#` (default: true).
    pub comment: bool,
    /// Verbosity: -1=minimal (skip Unset), 0=normal, 1=verbose.
    pub full: i8,
    /// Indentation increment per nesting level (default: 3).
    pub indent: usize,
}

impl RenderOptions {
    /// The options a Native Channel with `config` writes with. C's
    /// indentation loops run no times for a negative Indent, so it writes
    /// what an Indent of 0 would.
    pub(crate) fn from_config(config: &crate::channel::ChannelConfig) -> Self {
        RenderOptions {
            comment: config.comment(),
            full: config.full() as i8,
            indent: config.indent().max(0) as usize,
        }
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            comment: true,
            full: 0,
            indent: 3,
        }
    }
}

/// Render a [`Node`] tree as AST's plain-text Native format.
///
/// The output faithfully reproduces the whitespace, indentation, and
/// formatting conventions of the C library's `astWrite` so that the text is
/// byte-exact with C output when given an identical Node tree.
///
/// This is a convenience wrapper around [`render_native_with_options`] with
/// default options (comments enabled, full=0, indent=3).
#[cfg(test)]
pub(crate) fn render_native(node: &Node) -> AstResult<String> {
    render_native_with_options(node, &RenderOptions::default())
}

/// Render a [`Node`] tree as AST's plain-text Native format, respecting
/// the given [`RenderOptions`].
///
/// Behavior with non-default options:
/// - `comment = false`: don't emit `\t# comment_text` — just key=value.
/// - `full = -1`: skip items whose value is `Unset(...)` (commented defaults).
/// - `full = 0`: write the `Unset` items C calls helpful.
/// - `full = 1`: write every `Unset` item, and every `IsA` marker.
/// - `indent`: controls indentation increment per nesting level.
pub(crate) fn render_native_with_options(
    node: &Node,
    options: &RenderOptions,
) -> AstResult<String> {
    let mut buf = String::new();
    write_object_with_options(&mut buf, node, 0, options);
    Ok(buf)
}

/// Append " \t# <comment>" when comments are enabled and `comment` is
/// present *and non-empty*.
///
/// Every C `Write*` function (`WriteBegin`, `WriteInt`, `WriteObject`, …)
/// gates the comment on `astGetComment(this) && *comment`
/// (`channel.c:4306`, `channel.c:4703`, `channel.c:5110`, etc.) — testing
/// the first character, not just the pointer. A `KeyMap` vector entry's
/// non-first elements carry an explicit empty-string comment
/// (`keymap.c:2796-2801`: `com = ""` after the first `astWriteInt`), which
/// must therefore write no `\t#` at all, matching C exactly.
fn push_comment(buf: &mut String, options: &RenderOptions, comment: Option<&str>) {
    if options.comment {
        if let Some(comment) = comment {
            if !comment.is_empty() {
                buf.push_str(" \t# ");
                buf.push_str(comment);
            }
        }
    }
}

/// Recursively write an object (Begin…End block) at the given base indent.
///
/// `base_indent` is the number of spaces before the `" "` that prefixes
/// `Begin`, `End`, and `IsA` lines.  Attribute lines use
/// `base_indent + 4` leading spaces.  Nested objects recurse with
/// `base_indent + 2 * indent`.
fn write_object_with_options(
    buf: &mut String,
    node: &Node,
    base_indent: usize,
    options: &RenderOptions,
) {
    // The C library uses indent=3 with Begin/End at base+1, attributes at
    // base+4, and nested objects at base+6.  The spacing arithmetic is:
    //   begin_prefix = base + 1 space (the " " before Begin)
    //   attr_prefix  = base + indent + 1  (=4 at default indent=3)
    //   nested       = base + 2*indent    (=6 at default indent=3)
    //   unset_prefix = "#" + (base + indent) spaces (= "#   " at indent=3)
    let indent = options.indent;
    let begin_indent = format!("{} ", " ".repeat(base_indent));
    let attr_indent = " ".repeat(base_indent + indent + 1);
    let unset_prefix = format!("#{}", " ".repeat(base_indent + indent));

    // Begin line.
    buf.push_str(&begin_indent);
    buf.push_str("Begin ");
    buf.push_str(&node.class);
    push_comment(buf, options, node.class_comment.as_deref());
    buf.push('\n');

    // Items. C's `astWriteIsA` (channel.c:4955) writes an `IsA` marker
    // only when at least one item has been written since the previous
    // `Begin`/`IsA` (or when Full > 0). Track that here so a marker whose
    // preceding class-attribute block is entirely commented-out/skipped is
    // suppressed — this matters for Frames nested inside a CmpFrame, where
    // the per-axis attributes are all unset at full = -1.
    let mut items_written = false;
    for item in &node.items {
        // IsA marker.
        if item.key == "IsA" {
            if !items_written && options.full <= 0 {
                continue;
            }
            if let NodeValue::String(ref class) = item.value {
                buf.push_str(&begin_indent);
                buf.push_str("IsA ");
                buf.push_str(class);
                push_comment(buf, options, item.comment.as_deref());
                buf.push('\n');
            }
            // Reset the per-class counter, matching C's WriteIsA.
            items_written = false;
            continue;
        }

        match &item.value {
            // Unset (commented-out default).
            NodeValue::Unset(inner) => {
                // channel.c `Use`: no unset value at full = -1, only the
                // helpful ones at full = 0.
                if options.full < 0 || (options.full == 0 && !item.helpful) {
                    continue;
                }
                buf.push_str(&unset_prefix);
                buf.push_str(&item.key);
                // An unset Object is its name alone (channel.c
                // `WriteObject`).
                if matches!(**inner, NodeValue::Object(_)) {
                    buf.push_str(" =");
                } else {
                    buf.push_str(" = ");
                    write_scalar_value(buf, inner);
                }
                push_comment(buf, options, item.comment.as_deref());
                buf.push('\n');
                // A commented value that is actually emitted (full >= 0)
                // still goes through C's OutputTextItem and increments
                // items_written, so it counts toward the IsA rule. At
                // full = -1 it is skipped above and does not count.
                items_written = true;
            }

            // Nested object.
            NodeValue::Object(child) => {
                // C's `WriteObject` (`channel.c:5106-5107`) appends the name
                // followed by `" ="` — no trailing space, since there is no
                // value token; the nested object's definition follows on
                // subsequent lines. The trailing space only appears if a
                // comment is written after it (`channel.c:5110-5112`).
                buf.push_str(&attr_indent);
                buf.push_str(&item.key);
                buf.push_str(" =");
                push_comment(buf, options, item.comment.as_deref());
                buf.push('\n');
                // A class that caps Full for a nested object writes it with
                // Full -1 unless Full is 1 (frame.c and cmpframe.c `Dump`).
                if child.full_capped && options.full <= 0 {
                    let capped = RenderOptions {
                        full: -1,
                        ..options.clone()
                    };
                    write_object_with_options(buf, child, base_indent + 2 * indent, &capped);
                } else {
                    write_object_with_options(buf, child, base_indent + 2 * indent, options);
                }
                items_written = true;
            }

            // Scalar attribute.
            _ => {
                buf.push_str(&attr_indent);
                buf.push_str(&item.key);
                buf.push_str(" = ");
                write_scalar_value(buf, &item.value);
                push_comment(buf, options, item.comment.as_deref());
                buf.push('\n');
                items_written = true;
            }
        }
    }

    // End line (never has a comment in C output).
    buf.push_str(&begin_indent);
    buf.push_str("End ");
    buf.push_str(&node.class);
    buf.push('\n');
}

/// Write a scalar value (Int, Double, or String) into the buffer.
fn write_scalar_value(buf: &mut String, value: &NodeValue) {
    match value {
        NodeValue::Int(v) => {
            buf.push_str(&v.to_string());
        }
        NodeValue::Double(v) => {
            // C `astWriteDouble` represents AST__BAD externally as the
            // unquoted token `<bad>` (channel.c:146), not a numeric literal.
            if *v == crate::constants::BAD_VALUE {
                buf.push_str("<bad>");
            } else {
                buf.push_str(&format_double_g(*v, DBL_DIG));
            }
        }
        NodeValue::String(s) => {
            // Strings are quoted; embedded `"` are doubled.
            buf.push('"');
            for ch in s.chars() {
                if ch == '"' {
                    buf.push_str("\"\"");
                } else {
                    buf.push(ch);
                }
            }
            buf.push('"');
        }
        // Object and Unset are handled by the caller; this arm should
        // not be reached but we handle it defensively.
        NodeValue::Object(_) => buf.push_str("<object>"),
        NodeValue::Unset(inner) => write_scalar_value(buf, inner),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::native_read::parse_native;
    use crate::serial::node::Node;

    // ---------------------------------------------------------------
    // format_double_g
    // ---------------------------------------------------------------

    #[test]
    fn fmt_zero() {
        assert_eq!(format_double_g(0.0, 17), "0");
    }

    #[test]
    fn fmt_neg_zero() {
        assert_eq!(format_double_g(-0.0, 17), "0");
    }

    #[test]
    fn fmt_integers() {
        assert_eq!(format_double_g(1.0, 17), "1");
        assert_eq!(format_double_g(42.0, 17), "42");
        assert_eq!(format_double_g(-129.0, 17), "-129");
    }

    #[test]
    fn fmt_small_decimals() {
        assert_eq!(format_double_g(-0.5, 17), "-0.5");
        assert_eq!(format_double_g(0.0001, 17), "0.0001");
        assert_eq!(format_double_g(-0.0001, 17), "-0.0001");
        assert_eq!(format_double_g(0.001, 17), "0.001");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn fmt_full_precision() {
        // 3.14 is not exactly representable; %.17g shows all 17 digits.
        assert_eq!(format_double_g(3.14, 17), "3.1400000000000001");
        assert_eq!(format_double_g(0.996194698, 17), "0.99619469800000005");
        assert_eq!(format_double_g(0.087155743, 17), "0.087155742999999994");
    }

    #[test]
    fn fmt_scientific_small() {
        // Exponent < -4 → scientific notation.
        assert_eq!(format_double_g(1e-5, 17), "1.0000000000000001e-05");
        assert_eq!(format_double_g(9.999e-5, 17), "9.9989999999999996e-05");
        assert_eq!(
            format_double_g(-5.84161700642502e-07, 17),
            "-5.8416170064250195e-07"
        );
        assert_eq!(
            format_double_g(5.23598775598299e-06, 17),
            "5.2359877559829903e-06"
        );
    }

    #[test]
    fn fmt_scientific_large() {
        // Exponent >= 17 → scientific notation.
        assert_eq!(format_double_g(1e17, 17), "1e+17");
        assert_eq!(format_double_g(1e100, 17), "1e+100");
        assert_eq!(
            format_double_g(1.234_567_890_123_456_9e100, 17),
            "1.2345678901234569e+100"
        );
    }

    #[test]
    fn fmt_boundary_fixed_large() {
        // 1e16 has exponent 16, which is < 17 → fixed notation.
        assert_eq!(format_double_g(1e16, 17), "10000000000000000");
    }

    #[test]
    fn fmt_exact_value() {
        // A value that is exactly representable.
        assert_eq!(format_double_g(2003.0173483725, 17), "2003.0173483725");
    }

    #[test]
    fn fmt_extreme_values() {
        assert_eq!(format_double_g(f64::MAX, 17), "1.7976931348623157e+308");
        assert_eq!(
            format_double_g(f64::MIN_POSITIVE, 17),
            "2.2250738585072014e-308"
        );
        // Subnormal
        assert_eq!(format_double_g(5e-324, 17), "4.9406564584124654e-324");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn fmt_low_precision() {
        // Precision 1 — one significant digit.
        assert_eq!(format_double_g(3.14, 1), "3");
        assert_eq!(format_double_g(0.0056, 1), "0.006");
        assert_eq!(format_double_g(99.9, 1), "1e+02");
    }

    // ---------------------------------------------------------------
    // render_native — unit tests
    // ---------------------------------------------------------------

    #[test]
    fn render_simple_object() {
        let mut node = Node::new("ZoomMap");
        node.class_comment = Some("Zoom about the origin".to_string());
        node.push_int("Nin", 2, Some("Number of input coordinates".to_string()));
        node.push_int("Invert", 0, Some("Mapping not inverted".to_string()));
        node.push_isa(
            "Mapping",
            Some("Mapping between coordinate systems".to_string()),
        );
        node.push_double(
            "Zoom",
            5.23598775598299e-06,
            Some("Zoom factor".to_string()),
        );

        let text = render_native(&node).unwrap();

        // Check key structural features.
        assert!(text.starts_with(" Begin ZoomMap \t# Zoom about the origin\n"));
        assert!(text.contains("    Nin = 2 \t# Number of input coordinates\n"));
        assert!(text.contains("    Invert = 0 \t# Mapping not inverted\n"));
        assert!(text.contains(" IsA Mapping \t# Mapping between coordinate systems\n"));
        assert!(text.contains("    Zoom = 5.2359877559829903e-06 \t# Zoom factor\n"));
        assert!(text.ends_with(" End ZoomMap\n"));
    }

    #[test]
    fn render_nested_object() {
        let mut inner = Node::new("UnitMap");
        inner.class_comment = Some("Unit (null) Mapping".to_string());
        inner.push_int("Nin", 2, Some("Number of input coordinates".to_string()));
        inner.push_isa(
            "Mapping",
            Some("Mapping between coordinate systems".to_string()),
        );

        let mut outer = Node::new("CmpMap");
        outer.class_comment = Some("Compound Mapping".to_string());
        outer.push_int("Nin", 2, Some("Number of input coordinates".to_string()));
        outer.push_isa(
            "Mapping",
            Some("Mapping between coordinate systems".to_string()),
        );
        outer.push_object("MapA", inner, Some("First component Mapping".to_string()));

        let text = render_native(&outer).unwrap();

        // Outer Begin at 1 space.
        assert!(text.starts_with(" Begin CmpMap \t# Compound Mapping\n"));
        // Outer attrs at 4 spaces.
        assert!(text.contains("    Nin = 2 \t# Number of input coordinates\n"));
        // Object key at 4 spaces, then nested Begin at 7 spaces.
        assert!(text.contains("    MapA = \t# First component Mapping\n"));
        assert!(text.contains("       Begin UnitMap \t# Unit (null) Mapping\n"));
        // Nested attrs at 10 spaces.
        assert!(text.contains("          Nin = 2 \t# Number of input coordinates\n"));
        // Nested IsA at 7 spaces.
        assert!(text.contains("       IsA Mapping \t# Mapping between coordinate systems\n"));
        // Nested End at 7 spaces.
        assert!(text.contains("       End UnitMap\n"));
        // Outer End at 1 space.
        assert!(text.ends_with(" End CmpMap\n"));
    }

    #[test]
    fn render_unset_attributes() {
        let mut node = Node::new("FrameSet");
        node.class_comment = Some("Set of inter-related coordinate systems".to_string());
        node.push_unset(
            "Title",
            NodeValue::String("test title".to_string()),
            Some("Title of coordinate system".to_string()),
        );
        node.push_unset(
            "Naxes",
            NodeValue::Int(3),
            Some("Number of coordinate axes".to_string()),
        );
        node.push_int(
            "ActUnt",
            0,
            Some("Unit strings do not affect alignment".to_string()),
        );
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        let text = render_native(&node).unwrap();

        // Unset lines: '#' + 3 spaces + key (= 4 chars prefix, same
        // width as the 4-space attribute indent at level 0).
        assert!(text.contains("#   Title = \"test title\" \t# Title of coordinate system\n"));
        assert!(text.contains("#   Naxes = 3 \t# Number of coordinate axes\n"));
        // Regular attribute.
        assert!(text.contains("    ActUnt = 0 \t# Unit strings do not affect alignment\n"));
    }

    #[test]
    fn render_string_with_quotes() {
        let mut node = Node::new("Axis");
        node.class_comment = Some("Coordinate axis".to_string());
        node.push_string("Label", "has\"quotes", Some("Axis Label".to_string()));

        let text = render_native(&node).unwrap();
        assert!(text.contains("    Label = \"has\"\"quotes\" \t# Axis Label\n"));
    }

    #[test]
    fn render_no_comment() {
        let mut node = Node::new("LutMap");
        node.class_comment = None;
        node.push_double("L2", 1.5, None);
        node.push_isa("Mapping", None);

        let text = render_native(&node).unwrap();
        assert!(text.starts_with(" Begin LutMap\n"));
        assert!(text.contains("    L2 = 1.5\n"));
        assert!(text.contains(" IsA Mapping\n"));
        assert!(text.ends_with(" End LutMap\n"));
    }

    // ---------------------------------------------------------------
    // Round-trip: parse → render → parse — structure preserved
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
    fn round_trip_splittest1() {
        let Some(text) = read_reference_file("splittest1.ast") else {
            eprintln!("skipping: reference submodule not available");
            return;
        };

        // Parse the reference file.
        let node = parse_native(&text).unwrap();

        // Render it back.
        let rendered = render_native(&node).unwrap();

        // Re-parse the rendered text.
        let node2 = parse_native(&rendered).unwrap();

        // Structural comparison: same class, same number of nodes,
        // same key set.
        assert_eq!(node.class, node2.class);
        assert_eq!(count_nodes(&node), count_nodes(&node2));
        assert_eq!(node.items.len(), node2.items.len());

        for (a, b) in node.items.iter().zip(node2.items.iter()) {
            assert_eq!(a.key, b.key, "key mismatch");
        }
    }

    #[test]
    fn round_trip_degen1() {
        let Some(text) = read_reference_file("degen1.ast") else {
            eprintln!("skipping: reference submodule not available");
            return;
        };

        let node = parse_native(&text).unwrap();
        let rendered = render_native(&node).unwrap();
        let node2 = parse_native(&rendered).unwrap();

        assert_eq!(node.class, node2.class);
        assert_eq!(count_nodes(&node), count_nodes(&node2));
        assert_eq!(node.items.len(), node2.items.len());

        // Spot-check a nested value.
        let frm4 = node.get_object("Frm4").unwrap();
        let frm4_rt = node2.get_object("Frm4").unwrap();
        assert_eq!(frm4.class, frm4_rt.class);
    }
}
