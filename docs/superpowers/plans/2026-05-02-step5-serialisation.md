# Step 5: Serialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Node tree, Serializable trait, Native text parser/renderer, and to_node/from_node for all 9 existing types. Public API via AnyObject enum.

**Architecture:** Each concrete type implements `Serializable` in its own module (accessing its own private fields). The `serial` module provides the Node types, the trait, Native format parser/renderer, and `AnyObject` dispatch. The Node tree is the intermediate representation — classes serialize to/from Nodes, format backends read/write Nodes to text.

**Tech Stack:** Rust 2024 edition. No new dependencies (Node tree and Native format are hand-written; no serde needed).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/starlink-ast/src/serial/mod.rs` | `Serializable` trait, `AnyObject` enum, public API (`to_native_string`, `from_native_string`), dispatch functions |
| `crates/starlink-ast/src/serial/node.rs` | `Node`, `NodeItem`, `NodeValue` types + helper methods |
| `crates/starlink-ast/src/serial/native_read.rs` | Native text → `Node` tree parser |
| `crates/starlink-ast/src/serial/native_write.rs` | `Node` tree → Native text renderer + `format_double_g` |
| `crates/starlink-ast/src/mappings/unit.rs` | Add `Serializable` impl for UnitMap |
| `crates/starlink-ast/src/mappings/zoom.rs` | Add `Serializable` impl for ZoomMap |
| `crates/starlink-ast/src/mappings/shift.rs` | Add `Serializable` impl for ShiftMap |
| `crates/starlink-ast/src/mappings/matrix.rs` | Add `Serializable` impl for MatrixMap |
| `crates/starlink-ast/src/mappings/perm.rs` | Add `Serializable` impl for PermMap |
| `crates/starlink-ast/src/mappings/cmpmap.rs` | Add `Serializable` impl for CmpMap |
| `crates/starlink-ast/src/frames/basic.rs` | Add `Serializable` impl for BasicFrame |
| `crates/starlink-ast/src/frames/cmpframe.rs` | Add `Serializable` impl for CmpFrame |
| `crates/starlink-ast/src/frames/frameset.rs` | Add `Serializable` impl for FrameSet |
| `crates/starlink-ast/src/lib.rs` | Add `pub mod serial;` and re-exports |
| `crates/starlink-ast/tests/test_serialisation.rs` | Node, Serializable, and round-trip tests |
| `crates/starlink-ast/tests/test_native_format.rs` | Native parser/renderer tests + byte-exact round-trip |
| `crates/starlink-ast/tests/test_native_parse_reference.rs` | Parse-only tests on reference .ast/.map files |
| `crates/ast-ffi-tests/src/bin/generate_fixtures.rs` | C program that generates .ast fixtures via astToString |
| `crates/starlink-ast/tests/fixtures/native/` | Directory tree for fixture files |

---

### Task 1: Node Types and Serializable Trait

**Files:**
- Create: `crates/starlink-ast/src/serial/mod.rs`
- Create: `crates/starlink-ast/src/serial/node.rs`
- Modify: `crates/starlink-ast/src/lib.rs`

- [ ] **Step 1: Create serial/node.rs with Node, NodeItem, NodeValue**

Create `crates/starlink-ast/src/serial/node.rs`:

```rust
//! The Node tree — intermediate representation for serialisation.

use std::fmt;

/// A single typed value in a serialised AST object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeValue {
    /// Integer attribute.
    Int(i64),
    /// Floating-point attribute.
    Double(f64),
    /// String attribute.
    String(String),
    /// Nested object.
    Object(Node),
    /// Commented-out default — preserves `# Key = Value` lines
    /// from C output for byte-exact round-trip.
    Unset(Box<NodeValue>),
}

/// One item in a Node's ordered attribute list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeItem {
    /// Attribute key (e.g. "Zoom", "Nin"). For IsA markers, key is "IsA".
    pub key: String,
    /// The value.
    pub value: NodeValue,
    /// Comment text (after `#` on the same line).
    pub comment: Option<String>,
}

/// A serialised AST object as a tree of named attributes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    /// AST class name (e.g. "ZoomMap", "FrameSet").
    pub class: String,
    /// Comment on the Begin line (e.g. "Compound Mapping").
    pub class_comment: Option<String>,
    /// Ordered list of items: attributes, IsA markers, and commented
    /// defaults. IsA markers use key `"IsA"` and value
    /// `NodeValue::String(class_name)`.
    pub items: Vec<NodeItem>,
}

impl Node {
    /// Create a new empty Node with the given class name.
    pub fn new(class: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            class_comment: None,
            items: Vec::new(),
        }
    }

    /// Add an integer attribute.
    pub fn push_int(&mut self, key: &str, value: i64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Int(value),
            comment: comment.into(),
        });
    }

    /// Add a double attribute.
    pub fn push_double(&mut self, key: &str, value: f64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Double(value),
            comment: comment.into(),
        });
    }

    /// Add a string attribute.
    pub fn push_string(&mut self, key: &str, value: &str, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::String(value.to_string()),
            comment: comment.into(),
        });
    }

    /// Add a nested object attribute.
    pub fn push_object(&mut self, key: &str, node: Node, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Object(node),
            comment: comment.into(),
        });
    }

    /// Add an IsA marker.
    pub fn push_isa(&mut self, class: &str, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: "IsA".to_string(),
            value: NodeValue::String(class.to_string()),
            comment: comment.into(),
        });
    }

    /// Add a commented-out (unset) attribute.
    pub fn push_unset(&mut self, key: &str, value: NodeValue, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Unset(Box::new(value)),
            comment: comment.into(),
        });
    }

    /// Look up the first item with the given key, returning its value.
    /// Returns `None` if not found or if the item is `Unset`.
    pub fn get(&self, key: &str) -> Option<&NodeValue> {
        for item in &self.items {
            if item.key == key {
                if let NodeValue::Unset(_) = &item.value {
                    return None;
                }
                return Some(&item.value);
            }
        }
        None
    }

    /// Look up an integer attribute, returning a default if absent.
    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        match self.get(key) {
            Some(NodeValue::Int(v)) => *v,
            _ => default,
        }
    }

    /// Look up a double attribute, returning a default if absent.
    pub fn get_double(&self, key: &str, default: f64) -> f64 {
        match self.get(key) {
            Some(NodeValue::Double(v)) => *v,
            _ => default,
        }
    }

    /// Look up a string attribute, returning a default if absent.
    pub fn get_string(&self, key: &str, default: &str) -> String {
        match self.get(key) {
            Some(NodeValue::String(v)) => v.clone(),
            _ => default.to_string(),
        }
    }

    /// Look up a nested object attribute.
    pub fn get_object(&self, key: &str) -> Option<&Node> {
        match self.get(key) {
            Some(NodeValue::Object(n)) => Some(n),
            _ => None,
        }
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.class)
    }
}
```

- [ ] **Step 2: Create serial/mod.rs with Serializable trait**

Create `crates/starlink-ast/src/serial/mod.rs`:

```rust
//! Serialisation: Node tree, Serializable trait, and Native format.

pub(crate) mod node;
pub(crate) mod native_read;
pub(crate) mod native_write;

pub(crate) use node::{Node, NodeItem, NodeValue};

use crate::error::{AstError, AstResult, ErrorCode};

/// Internal trait for serialising AST objects to/from Node trees.
pub(crate) trait Serializable {
    /// The AST class name for Begin/End markers.
    fn class_name(&self) -> &'static str;

    /// Serialise this object to a Node tree.
    fn to_node(&self) -> AstResult<Node>;

    /// Reconstruct from a Node tree.
    fn from_node(node: &Node) -> AstResult<Self>
    where
        Self: Sized;
}
```

- [ ] **Step 3: Create stub files for native_read and native_write**

Create `crates/starlink-ast/src/serial/native_read.rs`:

```rust
//! Native text format parser: text → Node tree.
```

Create `crates/starlink-ast/src/serial/native_write.rs`:

```rust
//! Native text format renderer: Node tree → text.
```

- [ ] **Step 4: Add serial module to lib.rs**

In `crates/starlink-ast/src/lib.rs`, add:

```rust
pub(crate) mod serial;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add crates/starlink-ast/src/serial/ crates/starlink-ast/src/lib.rs
git commit -m "Add serial module: Node types and Serializable trait"
```

---

### Task 2: Native Text Parser

**Files:**
- Modify: `crates/starlink-ast/src/serial/native_read.rs`

The parser is line-oriented: reads `Begin`/`End` blocks, parses
`Key = Value` pairs, handles `IsA` markers, commented defaults,
and nested objects. It produces a `Node` tree from any valid
Native text, regardless of class name.

- [ ] **Step 1: Implement native_read**

Write `crates/starlink-ast/src/serial/native_read.rs`:

```rust
//! Native text format parser: text → Node tree.

use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::node::{Node, NodeValue};

/// Parse a Native-format text string into a Node tree.
///
/// The input may contain leading/trailing whitespace and comments.
/// Returns the first complete object found.
pub(crate) fn parse_native(text: &str) -> AstResult<Node> {
    let lines: Vec<&str> = text.lines().collect();
    let mut pos = 0;
    skip_non_begin(&lines, &mut pos);
    if pos >= lines.len() {
        return Err(AstError::new(
            ErrorCode::Rderr,
            "no Begin found in native format input",
        ));
    }
    let (node, _) = parse_object(&lines, &mut pos)?;
    Ok(node)
}

/// Skip lines until we find one starting with "Begin" (after trimming).
fn skip_non_begin(lines: &[&str], pos: &mut usize) {
    while *pos < lines.len() {
        let trimmed = lines[*pos].trim();
        if trimmed.starts_with("Begin ") || trimmed.starts_with("Begin\t") {
            return;
        }
        *pos += 1;
    }
}

/// Parse a single object starting at the current Begin line.
/// Returns the Node and advances pos past the End line.
fn parse_object(lines: &[&str], pos: &mut usize) -> AstResult<(Node, usize)> {
    // Parse "Begin ClassName # comment"
    let begin_line = lines[*pos].trim();
    let (class, class_comment) = parse_begin_line(begin_line)?;
    let mut node = Node::new(class);
    node.class_comment = class_comment;
    *pos += 1;

    while *pos < lines.len() {
        let trimmed = lines[*pos].trim();

        if trimmed.is_empty() {
            *pos += 1;
            continue;
        }

        // End ClassName
        if trimmed.starts_with("End ") || trimmed.starts_with("End\t") {
            *pos += 1;
            return Ok((node, *pos));
        }

        // IsA ClassName # comment
        if trimmed.starts_with("IsA ") || trimmed.starts_with("IsA\t") {
            let (isa_class, isa_comment) = parse_isa_line(trimmed)?;
            node.push_isa(&isa_class, isa_comment);
            *pos += 1;
            continue;
        }

        // Commented-out line: "# Key = Value # comment" or plain "# comment"
        if trimmed.starts_with('#') {
            if let Some(item) = try_parse_commented_attr(trimmed) {
                node.items.push(item);
            }
            // Skip plain comments that aren't attribute defaults
            *pos += 1;
            continue;
        }

        // Active attribute: "Key = Value # comment"
        parse_attribute(lines, pos, &mut node)?;
    }

    Err(AstError::new(
        ErrorCode::Rderr,
        format!("unexpected end of input inside {}", node.class),
    ))
}

/// Parse "Begin ClassName # comment" → (class_name, optional comment)
fn parse_begin_line(line: &str) -> AstResult<(String, Option<String>)> {
    let rest = line
        .strip_prefix("Begin")
        .ok_or_else(|| AstError::new(ErrorCode::Rderr, "expected Begin"))?
        .trim_start();
    let (class, comment) = split_comment(rest);
    if class.is_empty() {
        return Err(AstError::new(ErrorCode::Rderr, "missing class name after Begin"));
    }
    Ok((class.to_string(), comment.map(|s| s.to_string())))
}

/// Parse "IsA ClassName # comment" → (class_name, optional comment)
fn parse_isa_line(line: &str) -> AstResult<(String, Option<String>)> {
    let rest = line
        .strip_prefix("IsA")
        .ok_or_else(|| AstError::new(ErrorCode::Rderr, "expected IsA"))?
        .trim_start();
    let (class, comment) = split_comment(rest);
    Ok((class.to_string(), comment.map(|s| s.to_string())))
}

/// Try to parse a commented line as "# Key = Value # comment".
/// Returns None if the line doesn't match the attribute pattern.
fn try_parse_commented_attr(line: &str) -> Option<crate::serial::node::NodeItem> {
    // Strip leading "# " or "#  " etc.
    let inner = line.strip_prefix('#')?.trim_start();
    // Must contain " = "
    let eq_pos = inner.find(" = ")?;
    let key = inner[..eq_pos].trim();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let after_eq = &inner[eq_pos + 3..];
    let (value_str, comment) = split_comment(after_eq);
    let value = parse_scalar_value(value_str.trim());
    Some(crate::serial::node::NodeItem {
        key: key.to_string(),
        value: NodeValue::Unset(Box::new(value)),
        comment: comment.map(|s| s.to_string()),
    })
}

/// Parse an active attribute line. If the value is empty (object
/// reference), recursively parse the nested object from subsequent
/// lines.
fn parse_attribute(lines: &[&str], pos: &mut usize, node: &mut Node) -> AstResult<()> {
    let line = lines[*pos].trim();
    let eq_pos = line.find(" = ").ok_or_else(|| {
        AstError::new(
            ErrorCode::Rderr,
            format!("expected 'Key = Value' but got: {}", line),
        )
    })?;

    let key = line[..eq_pos].trim();
    let after_eq = &line[eq_pos + 3..];
    let (value_str, comment) = split_comment(after_eq);
    let value_str = value_str.trim();

    if value_str.is_empty() {
        // Nested object — next non-blank line should be Begin
        *pos += 1;
        skip_non_begin(lines, pos);
        if *pos >= lines.len() {
            return Err(AstError::new(
                ErrorCode::Rderr,
                format!("expected nested object for key {}", key),
            ));
        }
        let (child, _) = parse_object(lines, pos)?;
        node.push_object(key, child, comment.map(|s| s.to_string()));
    } else {
        let value = parse_scalar_value(value_str);
        node.items.push(crate::serial::node::NodeItem {
            key: key.to_string(),
            value,
            comment: comment.map(|s| s.to_string()),
        });
        *pos += 1;
    }
    Ok(())
}

/// Split "content # comment" into (content, Some(comment)).
/// If no `#` or `#` is inside quotes, returns (whole, None).
fn split_comment(s: &str) -> (&str, Option<&str>) {
    // Find the first # that's not inside a quoted string
    let mut in_quote = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            in_quote = !in_quote;
        } else if c == '#' && !in_quote {
            let content = s[..i].trim_end();
            let comment = s[i + 1..].trim();
            return (content, if comment.is_empty() { None } else { Some(comment) });
        }
    }
    (s.trim_end(), None)
}

/// Parse a scalar value string into a NodeValue.
/// Tries integer first, then double, then treats as string.
fn parse_scalar_value(s: &str) -> NodeValue {
    // Quoted string
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        // Unescape doubled quotes
        let unescaped = inner.replace("\"\"", "\"");
        return NodeValue::String(unescaped);
    }

    // Try integer
    if let Ok(v) = s.parse::<i64>() {
        return NodeValue::Int(v);
    }

    // Try double
    if let Ok(v) = s.parse::<f64>() {
        return NodeValue::Double(v);
    }

    // Unquoted string (e.g. class names in System = GALACTIC)
    NodeValue::String(s.to_string())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/starlink-ast/src/serial/native_read.rs
git commit -m "Add Native text parser (text → Node tree)"
```

---

### Task 3: Native Text Renderer and Float Formatting

**Files:**
- Modify: `crates/starlink-ast/src/serial/native_write.rs`

- [ ] **Step 1: Implement format_double_g helper**

The C library uses `sprintf(buff, "%.*g", 17, value)`. Rust's
default float formatting doesn't match C's `%g` (which switches
between `%f` and `%e` notation and trims trailing zeros). We need
a faithful reproduction.

In `crates/starlink-ast/src/serial/native_write.rs`:

```rust
//! Native text format renderer: Node tree → text.

use std::fmt::Write;

use crate::error::AstResult;
use crate::serial::node::{Node, NodeValue};

/// Precision for double formatting — matches C's AST__DBL_DIG.
const DBL_DIG: usize = 17;

/// Format a double using C's `%.*g` behaviour.
///
/// Rules (matching C `printf %g`):
/// - Uses `%e` notation if exponent < -4 or >= precision.
/// - Otherwise uses `%f` notation.
/// - Trailing zeros after the decimal point are trimmed.
/// - Trailing decimal point is trimmed.
pub(crate) fn format_double_g(value: f64, precision: usize) -> String {
    if value == 0.0 {
        // Handle negative zero: -0 should print as "0"
        // C's %g prints "0" for both +0 and -0
        return "0".to_string();
    }

    // Use Rust's formatting to get the %e representation,
    // then decide which form to use based on the exponent.
    // This is simpler and more reliable than trying to replicate
    // the full %g algorithm from scratch.
    //
    // Step 1: Get the exponent
    let exp = if value != 0.0 {
        value.abs().log10().floor() as i32
    } else {
        0
    };

    if exp < -4 || exp >= precision as i32 {
        // Use scientific notation
        format_e(value, precision)
    } else {
        // Use fixed notation
        format_f(value, precision, exp)
    }
}

/// Format in scientific notation matching C's %e within %g.
fn format_e(value: f64, precision: usize) -> String {
    // %g with %e: precision means total significant digits,
    // so %e digits = precision - 1
    let digits = if precision > 1 { precision - 1 } else { 0 };
    let s = format!("{value:.digits$e}");
    // Rust uses "e" as separator; C uses "e+" or "e-" with
    // at least 2-digit exponent. Reformat.
    reformat_rust_e_to_c(&s)
}

/// Format in fixed notation matching C's %f within %g.
fn format_f(value: f64, precision: usize, exp: i32) -> String {
    // %g with %f: digits after decimal = precision - (exp + 1),
    // but at least 0.
    let digits_after = if precision as i32 > exp + 1 {
        (precision as i32 - exp - 1) as usize
    } else {
        0
    };
    let s = format!("{value:.digits_after$}");
    trim_trailing_zeros(&s)
}

/// Convert Rust's "1.23e5" to C's "1.23e+05" format,
/// then trim trailing zeros from the mantissa.
fn reformat_rust_e_to_c(s: &str) -> String {
    let (mantissa, exp_str) = match s.split_once('e') {
        Some((m, e)) => (m, e),
        None => return s.to_string(),
    };
    let mantissa = trim_trailing_zeros(mantissa);
    let exp: i32 = exp_str.parse().unwrap_or(0);
    if exp >= 0 {
        format!("{mantissa}e+{exp:02}")
    } else {
        format!("{mantissa}e-{:02}", exp.unsigned_abs())
    }
}

/// Trim trailing zeros after a decimal point, and the point itself
/// if no digits remain after it.
fn trim_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0');
    let trimmed = trimmed.trim_end_matches('.');
    trimmed.to_string()
}
```

- [ ] **Step 2: Implement render_native**

Continue in the same file:

```rust
/// Render a Node tree to Native text format.
pub(crate) fn render_native(node: &Node) -> AstResult<String> {
    let mut buf = String::new();
    render_node(&mut buf, node, 0)?;
    Ok(buf)
}

/// Render a single node at the given indentation level.
fn render_node(buf: &mut String, node: &Node, indent: usize) -> AstResult<()> {
    let pad = " ".repeat(indent);

    // Begin line
    write!(buf, "{pad} Begin {}", node.class).unwrap();
    if let Some(ref c) = node.class_comment {
        write!(buf, " \t# {c}").unwrap();
    }
    writeln!(buf).unwrap();

    // Items
    for item in &node.items {
        render_item(buf, item, indent + 3)?;
    }

    // End line
    writeln!(buf, "{pad} End {}", node.class).unwrap();
    Ok(())
}

/// Render one item (attribute, IsA marker, or unset default).
fn render_item(buf: &mut String, item: &crate::serial::node::NodeItem, indent: usize) -> AstResult<()> {
    let pad = " ".repeat(indent);

    // IsA marker
    if item.key == "IsA" {
        if let NodeValue::String(ref class) = item.value {
            write!(buf, "{pad}IsA {class}").unwrap();
            if let Some(ref c) = item.comment {
                write!(buf, " \t# {c}").unwrap();
            }
            writeln!(buf).unwrap();
            return Ok(());
        }
    }

    // Unset (commented-out) attribute
    if let NodeValue::Unset(ref inner) = item.value {
        write!(buf, "#  {}", item.key).unwrap();
        write!(buf, " = {}", format_value(inner)).unwrap();
        if let Some(ref c) = item.comment {
            write!(buf, " \t# {c}").unwrap();
        }
        writeln!(buf).unwrap();
        return Ok(());
    }

    // Nested object
    if let NodeValue::Object(ref child) = item.value {
        write!(buf, "{pad}{} =", item.key).unwrap();
        if let Some(ref c) = item.comment {
            write!(buf, " \t# {c}").unwrap();
        }
        writeln!(buf).unwrap();
        render_node(buf, child, indent)?;
        return Ok(());
    }

    // Scalar attribute
    write!(buf, "{pad}{} = {}", item.key, format_value(&item.value)).unwrap();
    if let Some(ref c) = item.comment {
        write!(buf, " \t# {c}").unwrap();
    }
    writeln!(buf).unwrap();
    Ok(())
}

/// Format a scalar NodeValue as a string for Native output.
fn format_value(value: &NodeValue) -> String {
    match value {
        NodeValue::Int(v) => v.to_string(),
        NodeValue::Double(v) => format_double_g(*v, DBL_DIG),
        NodeValue::String(v) => format!("\"{}\"", v.replace('"', "\"\"")),
        NodeValue::Object(_) => String::new(),
        NodeValue::Unset(inner) => format_value(inner),
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/src/serial/native_write.rs
git commit -m "Add Native text renderer (Node tree → text) with C-compatible float formatting"
```

---

### Task 4: Mapping Serialization (UnitMap, ZoomMap, ShiftMap, MatrixMap, PermMap, CmpMap)

**Files:**
- Modify: `crates/starlink-ast/src/mappings/unit.rs`
- Modify: `crates/starlink-ast/src/mappings/zoom.rs`
- Modify: `crates/starlink-ast/src/mappings/shift.rs`
- Modify: `crates/starlink-ast/src/mappings/matrix.rs`
- Modify: `crates/starlink-ast/src/mappings/perm.rs`
- Modify: `crates/starlink-ast/src/mappings/cmpmap.rs`
- Modify: `crates/starlink-ast/src/serial/mod.rs`

Each mapping implements `Serializable` in its own file (where it has
access to private fields). The `serial` module provides a dispatch
function `mapping_from_node` that routes by class name.

Every mapping shares common Mapping base attributes (Nin, Nout,
Invert, IsSimp, etc.) written before `IsA Mapping`. A helper
module provides the shared logic.

- [ ] **Step 1: Add mapping serialization helpers to serial/mod.rs**

Add to `crates/starlink-ast/src/serial/mod.rs`:

```rust
use crate::mappings::{DynMapping, Mapping};

/// Write the common Mapping base attributes into a Node.
///
/// Called by each concrete mapping's `to_node` before adding
/// class-specific attributes.
pub(crate) fn write_mapping_base(node: &mut Node, mapping: &dyn Mapping) {
    let nin = mapping.nin();
    let nout = mapping.nout();
    node.push_int("Nin", nin as i64, Some("Number of input coordinates".to_string()));
    if nout != nin {
        node.push_int("Nout", nout as i64, Some("Number of output coordinates".to_string()));
    }
    if mapping.is_inverted() {
        node.push_int("Invert", 1, Some("Mapping inverted".to_string()));
    } else {
        node.push_int("Invert", 0, Some("Mapping not inverted".to_string()));
    }
    node.push_isa("Mapping", Some("Mapping between coordinate systems".to_string()));
}

/// Read common Mapping base attributes from a Node.
/// Returns (nin, nout, inverted).
pub(crate) fn read_mapping_base(node: &Node) -> (usize, usize, bool) {
    let nin = node.get_int("Nin", 0) as usize;
    let nout = node.get_int("Nout", nin as i64) as usize;
    let inverted = node.get_int("Invert", 0) != 0;
    (nin, nout, inverted)
}

/// Dispatch: reconstruct a concrete Mapping from a Node by class name.
pub(crate) fn mapping_from_node(node: &Node) -> AstResult<DynMapping> {
    use crate::mappings::*;
    match node.class.as_str() {
        "UnitMap" => Ok(Box::new(UnitMap::from_node(node)?)),
        "ZoomMap" => Ok(Box::new(ZoomMap::from_node(node)?)),
        "ShiftMap" => Ok(Box::new(ShiftMap::from_node(node)?)),
        "MatrixMap" => Ok(Box::new(MatrixMap::from_node(node)?)),
        "PermMap" => Ok(Box::new(PermMap::from_node(node)?)),
        "CmpMap" => Ok(Box::new(CmpMap::from_node(node)?)),
        "FrameSet" => {
            use crate::frames::FrameSet;
            Ok(Box::new(FrameSet::from_node(node)?))
        }
        _ => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown Mapping class: {}", node.class),
        )),
    }
}
```

(Use `ErrorCode::Lderr` — "class loader error".)

- [ ] **Step 2: Implement Serializable for UnitMap**

Add to `crates/starlink-ast/src/mappings/unit.rs`:

```rust
use crate::serial::{self, Node, Serializable};

impl Serializable for UnitMap {
    fn class_name(&self) -> &'static str {
        "UnitMap"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("UnitMap");
        node.class_comment = Some("Unit (null) Mapping".to_string());
        serial::write_mapping_base(&mut node, self);
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let (nin, _nout, inverted) = serial::read_mapping_base(node);
        let mut m = UnitMap::new(nin);
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
```

- [ ] **Step 3: Implement Serializable for ZoomMap**

Add to `crates/starlink-ast/src/mappings/zoom.rs`:

```rust
use crate::serial::{self, Node, Serializable};

impl Serializable for ZoomMap {
    fn class_name(&self) -> &'static str {
        "ZoomMap"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("ZoomMap");
        node.class_comment = Some("Zoom about the origin".to_string());
        serial::write_mapping_base(&mut node, self);
        node.push_double("Zoom", self.factor, Some("Zoom factor".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let (nin, _nout, inverted) = serial::read_mapping_base(node);
        let factor = node.get_double("Zoom", 1.0);
        let mut m = ZoomMap::new(nin, factor)?;
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
```

- [ ] **Step 4: Implement Serializable for ShiftMap**

Add to `crates/starlink-ast/src/mappings/shift.rs`:

```rust
use crate::serial::{self, Node, Serializable};

impl Serializable for ShiftMap {
    fn class_name(&self) -> &'static str {
        "ShiftMap"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("ShiftMap");
        node.class_comment = Some("Shift each coordinate axis".to_string());
        serial::write_mapping_base(&mut node, self);
        for (i, &s) in self.shift.iter().enumerate() {
            if s != 0.0 {
                node.push_double(
                    &format!("Sft{}", i + 1),
                    s,
                    Some(format!("Shift for axis {}", i + 1)),
                );
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let (nin, _nout, inverted) = serial::read_mapping_base(node);
        let mut shift = vec![0.0; nin];
        for i in 0..nin {
            shift[i] = node.get_double(&format!("Sft{}", i + 1), 0.0);
        }
        let mut m = ShiftMap::new(shift);
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
```

- [ ] **Step 5: Implement Serializable for MatrixMap**

Add to `crates/starlink-ast/src/mappings/matrix.rs`:

```rust
use crate::serial::{self, Node, Serializable};
use crate::constants::BAD_VALUE;

impl Serializable for MatrixMap {
    fn class_name(&self) -> &'static str {
        "MatrixMap"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("MatrixMap");
        node.class_comment = Some("Matrix transformation".to_string());
        serial::write_mapping_base(&mut node, self);

        // Forward matrix elements
        let (nrow, ncol) = self.matrix.dim();
        for (i, &v) in self.matrix.iter().enumerate() {
            if v != BAD_VALUE {
                node.push_double(
                    &format!("M{i}"),
                    v,
                    Some("Forward matrix value".to_string()),
                );
            }
        }

        // Inverse matrix elements
        if let Some(ref inv) = self.inverse {
            for (i, &v) in inv.iter().enumerate() {
                if v != BAD_VALUE {
                    node.push_double(
                        &format!("IM{i}"),
                        v,
                        Some("Inverse matrix value".to_string()),
                    );
                }
            }
        }

        // Form
        let form = if nrow == ncol {
            "Full"
        } else {
            "Full"
        };
        node.push_string("Form", form, Some("Matrix storage form".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let (nin, nout, inverted) = serial::read_mapping_base(node);
        let nout = if nout == 0 { nin } else { nout };

        let nel = nin * nout;
        let mut data = vec![0.0; nel];
        for i in 0..nel {
            data[i] = node.get_double(&format!("M{i}"), 0.0);
        }

        let matrix = ndarray::Array2::from_shape_vec((nout, nin), data).map_err(|e| {
            crate::error::AstError::new(
                crate::error::ErrorCode::Mtrmt,
                format!("invalid matrix shape: {e}"),
            )
        })?;

        let mut m = MatrixMap::new(matrix)?;
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
```

- [ ] **Step 6: Implement Serializable for PermMap**

Add to `crates/starlink-ast/src/mappings/perm.rs`:

```rust
use crate::serial::{self, Node, NodeValue, Serializable};
use crate::constants::BAD_VALUE;

impl Serializable for PermMap {
    fn class_name(&self) -> &'static str {
        "PermMap"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("PermMap");
        node.class_comment = Some("Coordinate permutation".to_string());
        serial::write_mapping_base(&mut node, self);

        // Output permutation
        for (i, &v) in self.outperm.iter().enumerate() {
            node.push_int(
                &format!("Out{}", i + 1),
                v,
                Some(format!("Output coordinate {}", i + 1)),
            );
        }

        // Input permutation
        for (i, &v) in self.inperm.iter().enumerate() {
            node.push_int(
                &format!("In{}", i + 1),
                v,
                Some(format!("Input coordinate {}", i + 1)),
            );
        }

        // Constants
        if !self.constants.is_empty() {
            node.push_int(
                "Nconst",
                self.constants.len() as i64,
                Some("Number of constants".to_string()),
            );
            for (i, &v) in self.constants.iter().enumerate() {
                if v == BAD_VALUE {
                    node.push_string(
                        &format!("Con{}", i + 1),
                        "<bad>",
                        Some(format!("Constant number {}", i + 1)),
                    );
                } else {
                    node.push_double(
                        &format!("Con{}", i + 1),
                        v,
                        Some(format!("Constant number {}", i + 1)),
                    );
                }
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let (nin, nout, inverted) = serial::read_mapping_base(node);

        let mut outperm = vec![0i64; nout];
        for i in 0..nout {
            outperm[i] = node.get_int(&format!("Out{}", i + 1), 0);
        }

        let mut inperm = vec![0i64; nin];
        for i in 0..nin {
            inperm[i] = node.get_int(&format!("In{}", i + 1), 0);
        }

        let nconst = node.get_int("Nconst", 0) as usize;
        let mut constants = vec![0.0; nconst];
        for i in 0..nconst {
            let key = format!("Con{}", i + 1);
            match node.get(&key) {
                Some(NodeValue::String(s)) if s == "<bad>" => {
                    constants[i] = BAD_VALUE;
                }
                Some(NodeValue::Double(v)) => {
                    constants[i] = *v;
                }
                _ => {}
            }
        }

        let mut m = PermMap::new(&inperm, &outperm, &constants)?;
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
```

- [ ] **Step 7: Implement Serializable for CmpMap**

Add to `crates/starlink-ast/src/mappings/cmpmap.rs`:

```rust
use crate::serial::{self, Node, Serializable};

impl Serializable for CmpMap {
    fn class_name(&self) -> &'static str {
        "CmpMap"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("CmpMap");
        node.class_comment = Some("Compound Mapping".to_string());
        serial::write_mapping_base(&mut node, self);

        if self.kind == Composition::Parallel {
            node.push_int("Series", 0, Some("Component Mappings applied in parallel".to_string()));
        }

        let first_node = self.first.to_node()?;
        let second_node = self.second.to_node()?;

        node.push_object("MapA", first_node, Some("First component Mapping".to_string()));
        node.push_object("MapB", second_node, Some("Second component Mapping".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let (_nin, _nout, inverted) = serial::read_mapping_base(node);

        let series = node.get_int("Series", 1);
        let kind = if series == 0 {
            Composition::Parallel
        } else {
            Composition::Series
        };

        let map_a_node = node.get_object("MapA").ok_or_else(|| {
            crate::error::AstError::new(crate::error::ErrorCode::Rderr, "CmpMap missing MapA")
        })?;
        let map_b_node = node.get_object("MapB").ok_or_else(|| {
            crate::error::AstError::new(crate::error::ErrorCode::Rderr, "CmpMap missing MapB")
        })?;

        let first = serial::mapping_from_node(map_a_node)?;
        let second = serial::mapping_from_node(map_b_node)?;

        let mut m = CmpMap::new(first, second, kind)?;
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
```

Note: `CmpMap::to_node` calls `self.first.to_node()` — this requires
`Serializable` to be callable on `dyn Mapping`. Since we can't add
`Serializable` as a supertrait of `Mapping` (it's private and not all
future Mappings will be serializable immediately), we need a dispatch
approach. Add a helper to `serial/mod.rs`:

```rust
/// Trait object helper: call to_node on a DynMapping by dispatching
/// through known concrete types.
pub(crate) trait MappingToNode {
    fn to_node(&self) -> AstResult<Node>;
}

impl MappingToNode for dyn Mapping {
    fn to_node(&self) -> AstResult<Node> {
        // Downcast via AsAny to known concrete types
        if let Some(m) = self.as_any().downcast_ref::<crate::mappings::UnitMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<crate::mappings::ZoomMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<crate::mappings::ShiftMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<crate::mappings::MatrixMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<crate::mappings::PermMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<crate::mappings::CmpMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<crate::frames::FrameSet>() {
            return Serializable::to_node(m);
        }
        Err(AstError::new(
            ErrorCode::Lderr,
            "cannot serialise unknown Mapping type",
        ))
    }
}
```

Similarly for `dyn Frame`:

```rust
pub(crate) trait FrameToNode {
    fn to_node(&self) -> AstResult<Node>;
}

impl FrameToNode for dyn crate::frames::Frame {
    fn to_node(&self) -> AstResult<Node> {
        if let Some(f) = self.as_any().downcast_ref::<crate::frames::BasicFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<crate::frames::CmpFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<crate::frames::FrameSet>() {
            return Serializable::to_node(f);
        }
        Err(AstError::new(
            ErrorCode::Lderr,
            "cannot serialise unknown Frame type",
        ))
    }
}
```

This requires `Frame` to also implement `AsAny`. Add that as a
supertrait of `Frame` (similar to how `Mapping` already has it).
In `crates/starlink-ast/src/frames/mod.rs`:

```rust
use crate::mappings::AsAny;

pub trait Frame: Send + fmt::Debug + AsAny {
```

Then add the blanket `AsAny` bound — it's already blanket-implemented
for all `T: Any`, and `BasicFrame`, `CmpFrame`, `FrameSet` are all
`'static`, so this should work.

- [ ] **Step 8: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 9: Commit**

```bash
git add crates/starlink-ast/src/mappings/ crates/starlink-ast/src/serial/ crates/starlink-ast/src/frames/mod.rs
git commit -m "Add Serializable impls for all Mapping types"
```

---

### Task 5: Frame Serialization (BasicFrame, CmpFrame, Axis)

**Files:**
- Modify: `crates/starlink-ast/src/frames/basic.rs`
- Modify: `crates/starlink-ast/src/frames/cmpframe.rs`
- Modify: `crates/starlink-ast/src/serial/mod.rs`

- [ ] **Step 1: Add frame serialization helpers to serial/mod.rs**

```rust
use crate::frames::{DynFrame, Frame};

/// Write common Frame base attributes into a Node.
pub(crate) fn write_frame_base(node: &mut Node, frame: &dyn Frame, naxes: usize) {
    // Frame inherits from Mapping in the serialisation hierarchy,
    // so write Mapping base first. Frame's nin/nout = naxes.
    node.push_int("Naxes", naxes as i64, Some("Number of coordinate axes".to_string()));
    node.push_isa("Frame", Some("Coordinate system description".to_string()));
}

/// Dispatch: reconstruct a concrete Frame from a Node by class name.
pub(crate) fn frame_from_node(node: &Node) -> AstResult<DynFrame> {
    use crate::frames::*;
    match node.class.as_str() {
        "Frame" => Ok(Box::new(BasicFrame::from_node(node)?)),
        "CmpFrame" => Ok(Box::new(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(Box::new(FrameSet::from_node(node)?)),
        _ => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown Frame class: {}", node.class),
        )),
    }
}
```

- [ ] **Step 2: Implement Serializable for BasicFrame**

Add to `crates/starlink-ast/src/frames/basic.rs`:

```rust
use crate::serial::{self, Node, NodeValue, Serializable};

impl Serializable for BasicFrame {
    fn class_name(&self) -> &'static str {
        "Frame"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("Frame");
        node.class_comment = Some("Coordinate system description".to_string());

        // Title
        if let Some(ref title) = self.title {
            if !title.is_empty() {
                node.push_string("Title", title, Some("Title of coordinate system".to_string()));
            }
        }

        // Naxes
        node.push_int("Naxes", self.naxes as i64, Some("Number of coordinate axes".to_string()));

        // Domain
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() {
                node.push_string("Domain", domain, Some("Coordinate system domain".to_string()));
            }
        }

        // Epoch
        if let Some(epoch) = self.epoch {
            node.push_double("Epoch", epoch, Some("Julian epoch of observation".to_string()));
        }

        // Per-axis labels (informational, commented)
        for i in 0..self.naxes {
            if let Some(ref label) = self.labels[i] {
                node.push_unset(
                    &format!("Lbl{}", i + 1),
                    NodeValue::String(label.clone()),
                    Some(format!("Label for axis {}", i + 1)),
                );
            }
        }

        // Per-axis symbols (informational, commented)
        for i in 0..self.naxes {
            if let Some(ref sym) = self.symbols[i] {
                node.push_unset(
                    &format!("Sym{}", i + 1),
                    NodeValue::String(sym.clone()),
                    Some(format!("Symbol for axis {}", i + 1)),
                );
            }
        }

        // Per-axis units
        for i in 0..self.naxes {
            if let Some(ref unit) = self.units[i] {
                if !unit.is_empty() {
                    node.push_string(
                        &format!("Uni{}", i + 1),
                        unit,
                        Some(format!("Units for axis {}", i + 1)),
                    );
                }
            }
        }

        // Per-axis Axis objects
        for i in 0..self.naxes {
            let mut axis_node = Node::new("Axis");
            axis_node.class_comment = Some("Coordinate axis".to_string());
            if let Some(ref label) = self.labels[i] {
                if !label.is_empty() {
                    axis_node.push_string("Label", label, Some("Axis Label".to_string()));
                }
            }
            if let Some(ref sym) = self.symbols[i] {
                if !sym.is_empty() {
                    axis_node.push_string("Symbol", sym, Some("Axis symbol".to_string()));
                }
            }
            if let Some(ref unit) = self.units[i] {
                if !unit.is_empty() {
                    axis_node.push_string("Unit", unit, Some("Axis units".to_string()));
                }
            }
            if let Some(ref fmt) = self.format_strs[i] {
                if !fmt.is_empty() {
                    axis_node.push_string("Format", fmt, Some("Axis format specifier".to_string()));
                }
            }
            node.push_object(
                &format!("Ax{}", i + 1),
                axis_node,
                Some(format!("Axis number {}", i + 1)),
            );
        }

        node.push_isa("Frame", Some("Coordinate system description".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let naxes = node.get_int("Naxes", 0) as usize;
        let mut builder = BasicFrameBuilder::new(naxes);

        if let Some(NodeValue::String(ref title)) = node.get("Title") {
            builder = builder.title(title.as_str());
        }
        if let Some(NodeValue::String(ref domain)) = node.get("Domain") {
            builder = builder.domain(domain.as_str());
        }
        if let Some(NodeValue::Double(epoch)) = node.get("Epoch") {
            builder = builder.epoch(*epoch);
        }

        // Read per-axis attributes from Axis child objects
        for i in 0..naxes {
            let key = format!("Ax{}", i + 1);
            if let Some(axis_node) = node.get_object(&key) {
                let label = axis_node.get_string("Label", "");
                if !label.is_empty() {
                    builder = builder.label(i + 1, label);
                }
                let sym = axis_node.get_string("Symbol", "");
                if !sym.is_empty() {
                    builder = builder.symbol(i + 1, sym);
                }
                let unit = axis_node.get_string("Unit", "");
                if !unit.is_empty() {
                    builder = builder.unit(i + 1, unit);
                }
                let fmt = axis_node.get_string("Format", "");
                if !fmt.is_empty() {
                    builder = builder.format_str(i + 1, fmt);
                }
            }
        }

        builder.build()
    }
}
```

- [ ] **Step 3: Implement Serializable for CmpFrame**

Add to `crates/starlink-ast/src/frames/cmpframe.rs`:

```rust
use crate::serial::{self, Node, Serializable, MappingToNode, FrameToNode};

impl Serializable for CmpFrame {
    fn class_name(&self) -> &'static str {
        "CmpFrame"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("CmpFrame");
        node.class_comment = Some("Compound coordinate system description".to_string());

        // Write Frame base attributes (naxes = total axes)
        node.push_int("Naxes", self.naxes() as i64, Some("Number of coordinate axes".to_string()));
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // CmpFrame-specific
        let frame_a_node = FrameToNode::to_node(&*self.frame1)?;
        let frame_b_node = FrameToNode::to_node(&*self.frame2)?;
        node.push_object("FrameA", frame_a_node, Some("First component Frame".to_string()));
        node.push_object("FrameB", frame_b_node, Some("Second component Frame".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let frame_a_node = node.get_object("FrameA").ok_or_else(|| {
            crate::error::AstError::new(crate::error::ErrorCode::Rderr, "CmpFrame missing FrameA")
        })?;
        let frame_b_node = node.get_object("FrameB").ok_or_else(|| {
            crate::error::AstError::new(crate::error::ErrorCode::Rderr, "CmpFrame missing FrameB")
        })?;

        let frame1 = serial::frame_from_node(frame_a_node)?;
        let frame2 = serial::frame_from_node(frame_b_node)?;
        Ok(CmpFrame::new(frame1, frame2))
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/frames/ crates/starlink-ast/src/serial/
git commit -m "Add Serializable impls for BasicFrame and CmpFrame"
```

---

### Task 6: FrameSet Serialization

**Files:**
- Modify: `crates/starlink-ast/src/frames/frameset.rs`

FrameSet serialization is the most complex: it must encode the graph
structure as frame objects, mapping objects, node-to-frame indices,
and node links.

- [ ] **Step 1: Implement Serializable for FrameSet**

Add to `crates/starlink-ast/src/frames/frameset.rs`:

```rust
use crate::serial::{self, Node, Serializable, MappingToNode, FrameToNode};

impl Serializable for FrameSet {
    fn class_name(&self) -> &'static str {
        "FrameSet"
    }

    fn to_node(&self) -> crate::error::AstResult<Node> {
        let mut node = Node::new("FrameSet");
        node.class_comment = Some("Set of inter-related coordinate systems".to_string());

        // Frame base (delegate to current for naxes etc.)
        node.push_int(
            "Naxes",
            self.current().naxes() as i64,
            Some("Number of coordinate axes".to_string()),
        );
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // FrameSet-specific
        let nframe = self.frames.len();
        node.push_int("Nframe", nframe as i64, Some("Number of Frames in FrameSet".to_string()));
        node.push_int(
            "Base",
            (self.base_idx + 1) as i64,
            Some("Index of base Frame".to_string()),
        );
        node.push_int(
            "Currnt",
            (self.current_idx + 1) as i64,
            Some("Index of current Frame".to_string()),
        );

        // Links: edges encode parent→child relationships.
        // Lnk{child+1} = parent+1
        for edge in &self.edges {
            node.push_int(
                &format!("Lnk{}", edge.to + 1),
                (edge.from + 1) as i64,
                Some(format!(
                    "Node {} is derived from node {}",
                    edge.to + 1,
                    edge.from + 1
                )),
            );
        }

        // Frame objects
        for (i, frame) in self.frames.iter().enumerate() {
            let frame_node = FrameToNode::to_node(frame.as_ref())?;
            node.push_object(
                &format!("Frm{}", i + 1),
                frame_node,
                Some(format!("Frame number {}", i + 1)),
            );
        }

        // Mapping objects (one per edge)
        for edge in &self.edges {
            let map_node = MappingToNode::to_node(edge.mapping.as_ref())?;
            node.push_object(
                &format!("Map{}", edge.to + 1),
                map_node,
                Some(format!(
                    "Mapping between nodes {} and {}",
                    edge.from + 1,
                    edge.to + 1
                )),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> crate::error::AstResult<Self> {
        let nframe = node.get_int("Nframe", 0) as usize;
        let base_1 = node.get_int("Base", 1) as usize;
        let current_1 = node.get_int("Currnt", nframe as i64) as usize;

        // Read frames
        let mut frames: Vec<crate::frames::DynFrame> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let frame_node = node.get_object(&format!("Frm{}", i + 1)).ok_or_else(|| {
                crate::error::AstError::new(
                    crate::error::ErrorCode::Rderr,
                    format!("FrameSet missing Frm{}", i + 1),
                )
            })?;
            frames.push(serial::frame_from_node(frame_node)?);
        }

        // Read links and mappings → reconstruct edges
        let mut edges = Vec::new();
        for i in 1..nframe {
            let child = i; // 0-based
            let child_1 = i + 1; // 1-based
            let parent_1 = node.get_int(&format!("Lnk{child_1}"), 1) as usize;
            let parent = parent_1 - 1; // 0-based

            let map_node = node.get_object(&format!("Map{child_1}")).ok_or_else(|| {
                crate::error::AstError::new(
                    crate::error::ErrorCode::Rderr,
                    format!("FrameSet missing Map{child_1}"),
                )
            })?;
            let mapping = serial::mapping_from_node(map_node)?;

            edges.push(Edge {
                from: parent,
                to: child,
                mapping,
            });
        }

        Ok(FrameSet {
            frames,
            edges,
            base_idx: base_1 - 1,
            current_idx: current_1 - 1,
            variants: Vec::new(),
        })
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/starlink-ast/src/frames/frameset.rs
git commit -m "Add Serializable impl for FrameSet"
```

---

### Task 7: AnyObject Enum and Public API

**Files:**
- Modify: `crates/starlink-ast/src/serial/mod.rs`
- Modify: `crates/starlink-ast/src/lib.rs`

- [ ] **Step 1: Add AnyObject and public functions**

Add to `crates/starlink-ast/src/serial/mod.rs`:

```rust
use crate::frames::{BasicFrame, CmpFrame, FrameSet};
use crate::mappings::{CmpMap, MatrixMap, PermMap, ShiftMap, UnitMap, ZoomMap};

/// A type-erased AST object for serialisation round-trips.
///
/// This is the Rust equivalent of C's `AstObject *`. Variants
/// grow as new types are implemented.
#[derive(Debug)]
pub enum AnyObject {
    /// A unit (identity) mapping.
    UnitMap(UnitMap),
    /// A zoom mapping.
    ZoomMap(ZoomMap),
    /// A shift mapping.
    ShiftMap(ShiftMap),
    /// A matrix mapping.
    MatrixMap(MatrixMap),
    /// A permutation mapping.
    PermMap(PermMap),
    /// A compound mapping.
    CmpMap(CmpMap),
    /// A basic coordinate frame.
    BasicFrame(BasicFrame),
    /// A compound coordinate frame.
    CmpFrame(CmpFrame),
    /// A set of inter-related coordinate frames.
    FrameSet(FrameSet),
}

impl AnyObject {
    /// If this is a Mapping, return a reference to it.
    pub fn as_mapping(&self) -> Option<&dyn Mapping> {
        match self {
            AnyObject::UnitMap(m) => Some(m),
            AnyObject::ZoomMap(m) => Some(m),
            AnyObject::ShiftMap(m) => Some(m),
            AnyObject::MatrixMap(m) => Some(m),
            AnyObject::PermMap(m) => Some(m),
            AnyObject::CmpMap(m) => Some(m),
            AnyObject::FrameSet(m) => Some(m),
            _ => None,
        }
    }

    /// If this is a Frame, return a reference to it.
    pub fn as_frame(&self) -> Option<&dyn Frame> {
        match self {
            AnyObject::BasicFrame(f) => Some(f),
            AnyObject::CmpFrame(f) => Some(f),
            AnyObject::FrameSet(f) => Some(f),
            _ => None,
        }
    }
}

/// Dispatch: reconstruct any AST object from a Node.
fn any_object_from_node(node: &Node) -> AstResult<AnyObject> {
    match node.class.as_str() {
        "UnitMap" => Ok(AnyObject::UnitMap(UnitMap::from_node(node)?)),
        "ZoomMap" => Ok(AnyObject::ZoomMap(ZoomMap::from_node(node)?)),
        "ShiftMap" => Ok(AnyObject::ShiftMap(ShiftMap::from_node(node)?)),
        "MatrixMap" => Ok(AnyObject::MatrixMap(MatrixMap::from_node(node)?)),
        "PermMap" => Ok(AnyObject::PermMap(PermMap::from_node(node)?)),
        "CmpMap" => Ok(AnyObject::CmpMap(CmpMap::from_node(node)?)),
        "Frame" => Ok(AnyObject::BasicFrame(BasicFrame::from_node(node)?)),
        "CmpFrame" => Ok(AnyObject::CmpFrame(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(AnyObject::FrameSet(FrameSet::from_node(node)?)),
        _ => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown AST class: {}", node.class),
        )),
    }
}

/// Serialise an AST object to its native text representation.
///
/// Equivalent to C's `astToString`.
pub fn to_native_string(obj: &AnyObject) -> AstResult<String> {
    let node = match obj {
        AnyObject::UnitMap(m) => m.to_node()?,
        AnyObject::ZoomMap(m) => m.to_node()?,
        AnyObject::ShiftMap(m) => m.to_node()?,
        AnyObject::MatrixMap(m) => m.to_node()?,
        AnyObject::PermMap(m) => m.to_node()?,
        AnyObject::CmpMap(m) => m.to_node()?,
        AnyObject::BasicFrame(f) => f.to_node()?,
        AnyObject::CmpFrame(f) => f.to_node()?,
        AnyObject::FrameSet(f) => Serializable::to_node(f)?,
    };
    native_write::render_native(&node)
}

/// Deserialise an AST object from its native text representation.
///
/// Equivalent to C's `astFromString`.
pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    let node = native_read::parse_native(s)?;
    any_object_from_node(&node)
}
```

- [ ] **Step 2: Re-export public API from lib.rs**

In `crates/starlink-ast/src/lib.rs`, change `pub(crate) mod serial;`
to `pub mod serial;` and add re-exports as needed. Or keep the
module private and re-export just the public items:

```rust
pub(crate) mod serial;
pub use serial::{to_native_string, from_native_string, AnyObject};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/src/serial/ crates/starlink-ast/src/lib.rs
git commit -m "Add AnyObject enum and public to_native_string/from_native_string API"
```

---

### Task 8: Serialization Unit Tests (Tier A)

**Files:**
- Create: `crates/starlink-ast/tests/test_serialisation.rs`

Tests: Node construction/access, to_node/from_node round-trip for
all 9 types.

- [ ] **Step 1: Write test_serialisation.rs**

Create `crates/starlink-ast/tests/test_serialisation.rs` with tests
for:

- `Node::new`, `push_int`, `push_double`, `push_string`, `push_object`,
  `push_isa`, `push_unset`, `get`, `get_int`, `get_double`,
  `get_string`, `get_object` — basic Node API.
- For each of the 9 types: create an object → `to_node` → `from_node`
  → verify key attributes match. Use the **public Rust API** to create
  objects and verify their behavior after round-trip (e.g. transform
  a point through a ZoomMap before and after round-trip, check results
  match).
- Full `to_native_string` → `from_native_string` round-trip per type.

The test file is large; the implementer should write tests following
the patterns in `test_frameset.rs` and `test_zoommap.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test -p starlink-ast --test test_serialisation`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add crates/starlink-ast/tests/test_serialisation.rs
git commit -m "Add serialisation unit tests (Tier A): Node API and to_node/from_node round-trips"
```

---

### Task 9: C Fixture Generator and Byte-Exact Tests (Tier B)

**Files:**
- Create: `crates/ast-ffi-tests/src/bin/generate_fixtures.rs`
- Create: `crates/starlink-ast/tests/fixtures/native/mappings/` (directory)
- Create: `crates/starlink-ast/tests/fixtures/native/frames/` (directory)
- Create: `crates/starlink-ast/tests/fixtures/native/framesets/` (directory)
- Create: `crates/starlink-ast/tests/test_native_format.rs`

- [ ] **Step 1: Write C fixture generator**

Create a C program (or Rust program that calls the C library via
`ast-ffi-tests`) that creates representative instances of our 9
types using the C AST library, serializes them with `astToString`,
and writes the output to fixture files. The exact implementation
depends on the `ast-ffi-tests` build infrastructure.

The generator should create:
- `unitmap.ast` — `UnitMap(3)`
- `zoommap.ast` — `ZoomMap(2, 3.5)`
- `shiftmap.ast` — `ShiftMap(2)` with shifts `[1.5, -2.7]`
- `matrixmap.ast` — `MatrixMap` 2×2 rotation
- `permmap.ast` — `PermMap` swapping 2 axes with a constant
- `cmpmap.ast` — `CmpMap` series of zoom+shift
- `basicframe.ast` — `Frame(2)` with title, domain, labels
- `cmpframe.ast` — `CmpFrame` of two `Frame(1)`s
- `frameset.ast` — `FrameSet` with 2 frames + ZoomMap

- [ ] **Step 2: Run the generator and commit fixtures**

```bash
# Run fixture generator (exact command depends on build setup)
# Copy output files to crates/starlink-ast/tests/fixtures/native/
git add crates/starlink-ast/tests/fixtures/native/
git commit -m "Add C-generated Native format fixtures for byte-exact round-trip tests"
```

- [ ] **Step 3: Write byte-exact round-trip tests**

Create `crates/starlink-ast/tests/test_native_format.rs` with tests
that:
- Read each fixture file → parse to Node → render back → diff
  against original (byte-exact).
- Read each fixture → parse → `from_node` → verify object
  attributes/behavior.

- [ ] **Step 4: Run tests**

Run: `cargo test -p starlink-ast --test test_native_format`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/tests/test_native_format.rs
git commit -m "Add byte-exact Native format round-trip tests (Tier B)"
```

---

### Task 10: Parse-Only Reference Tests (Tier C)

**Files:**
- Create: `crates/starlink-ast/tests/fixtures/native/reference/` (directory)
- Create: `crates/starlink-ast/tests/test_native_parse_reference.rs`

- [ ] **Step 1: Copy selected reference fixtures**

Copy a selection of `.ast` and `.map` files from
`reference/starlink-ast/ast_tester/` into
`crates/starlink-ast/tests/fixtures/native/reference/`:

```bash
cp reference/starlink-ast/ast_tester/splittest1.ast crates/starlink-ast/tests/fixtures/native/reference/
cp reference/starlink-ast/ast_tester/degen1.ast crates/starlink-ast/tests/fixtures/native/reference/
cp reference/starlink-ast/ast_tester/brad.map crates/starlink-ast/tests/fixtures/native/reference/
cp reference/starlink-ast/ast_tester/specflux.ast crates/starlink-ast/tests/fixtures/native/reference/
```

- [ ] **Step 2: Write parse-only tests**

Create `crates/starlink-ast/tests/test_native_parse_reference.rs` with
tests that:
- Parse each reference fixture into a Node tree.
- Verify parsing succeeds.
- Check top-level class name is correct.
- Check the Node has a reasonable number of items.
- Check nested objects are present where expected.

No `from_node` — these files contain types not yet implemented.

- [ ] **Step 3: Run tests**

Run: `cargo test -p starlink-ast --test test_native_parse_reference`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/tests/fixtures/native/reference/ crates/starlink-ast/tests/test_native_parse_reference.rs
git commit -m "Add parse-only tests for reference .ast/.map fixtures (Tier C)"
```

---

### Task 11: Clippy, Fmt, and Final Verification

**Files:**
- Possibly any file needing fixes

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Fix any issues.

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all -- --check`
If issues, run `cargo fmt --all`.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

- [ ] **Step 4: Commit fixes if any**

```bash
git add -u
git commit -m "Fix clippy and fmt issues in serialisation implementation"
```

---

## Verification Checklist

After all tasks are complete:

1. `cargo build --workspace` — all crates compile
2. `cargo test -p starlink-ast` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. Node tree can represent any Native-format AST object
6. Native parser handles Begin/End/IsA/attributes/nested objects/commented defaults
7. Native renderer produces correctly indented output with comments
8. All 9 types round-trip through to_node/from_node
9. `to_native_string` / `from_native_string` public API works
10. AnyObject enum with as_mapping/as_frame accessors
11. Byte-exact round-trip against C-generated fixtures (when available)
12. Reference .ast/.map files parse without errors
13. `git status` — clean
