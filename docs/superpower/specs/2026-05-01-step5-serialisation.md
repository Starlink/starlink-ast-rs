# Step 5: Serialisation — Design Spec

## Summary

Implement the serialisation infrastructure: a `Node` tree intermediate
representation, the `Serializable` trait, and the plain-text Native
format parser and renderer. Implement `to_node` / `from_node` for the
9 types already built in Rust. Byte-exact round-trip regression tests
against C-generated fixtures.

JSON, XML, YAML, FITS-header encoding, and the Channel trait are
deferred to later steps.

## Deliverables

1. Public API: `AnyObject` enum, `to_native_string`, `from_native_string`.
2. `Node` / `NodeValue` / `NodeItem` types (pub(crate)).
3. `Serializable` trait (pub(crate)) with `class_name`, `to_node`,
   `from_node`.
4. Dispatch functions: `mapping_from_node`, `frame_from_node`,
   `any_object_from_node`.
5. Native text parser (`native_read`): text → `Node` tree.
6. Native text renderer (`native_write`): `Node` tree → text.
7. `Serializable` implementations for: UnitMap, ZoomMap, ShiftMap,
   MatrixMap, PermMap, CmpMap, BasicFrame, CmpFrame, FrameSet.
8. C fixture generator in `ast-ffi-tests`.
9. Three tiers of tests.

## Module Structure

```
crates/ast-core/src/serial/
├── mod.rs          # Serializable trait, dispatch functions, re-exports
├── node.rs         # Node, NodeValue types
├── native_read.rs  # Native text → Node tree
└── native_write.rs # Node tree → Native text
```

## Node Tree

```rust
// pub(crate) in serial/node.rs

/// A single typed value in a serialised AST object.
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
pub(crate) struct NodeItem {
    /// Attribute key (e.g. "Zoom", "Nin"). For IsA markers, key is "IsA".
    pub key: String,
    /// The value.
    pub value: NodeValue,
    /// Comment text (after `#` on the same line). Preserved for
    /// byte-exact round-trip. E.g. "Zoom factor", "Number of input
    /// coordinates".
    pub comment: Option<String>,
}

/// A serialised AST object as a tree of named attributes.
pub(crate) struct Node {
    /// AST class name (e.g. "ZoomMap", "FrameSet").
    pub class: String,
    /// Comment on the Begin line (e.g. "Compound Mapping").
    pub class_comment: Option<String>,
    /// Ordered list of items: attributes, IsA markers, and commented
    /// defaults. Order matters — byte-exact Native output requires
    /// matching C library attribute ordering.
    ///
    /// IsA markers are stored as items with key `"IsA"` and value
    /// `NodeValue::String(class_name)`.
    pub items: Vec<NodeItem>,
}
```

### Design Notes

- **Ordered items**: Native format is line-oriented and order-dependent.
  A HashMap would lose ordering. `indexmap::IndexMap` preserves
  insertion order with O(1) lookup, but doesn't support duplicate
  keys — and `"IsA"` appears multiple times (e.g. `IsA Mapping` then
  `IsA Frame`). `Vec<NodeItem>` with linear scan is simpler and
  adequate given typical attribute counts (< 30).

- **Unset variant**: The C library writes commented-out lines for
  informational attributes (e.g. `# Title = "..."`, `# Lbl1 = "..."`).
  These carry the current default value but are not read back on
  deserialisation. Preserving them in the Node tree is necessary for
  byte-exact round-trip.

- **IsA markers**: In the Native text format, `IsA Mapping` separates
  Mapping-level attributes from subclass attributes. The Node stores
  these as special entries in the items vec with the sentinel key
  `"IsA"` and value `NodeValue::String(class_name)`. This preserves
  the interleaving of attributes and IsA markers naturally. The `isa`
  field on Node is redundant with this approach and is removed — the
  items vec is the single source of truth for ordering.

## Public API

The Rust equivalents of C's `astToString` / `astFromString`. Since
there is no unified `Object` trait, a closed `AnyObject` enum
provides the return type for deserialisation (this is the same enum
the design spec prescribes for `Channel::read` and the CFFI handle
registry, pulled forward):

```rust
// Public, in ast-core/src/serial/mod.rs (re-exported from lib.rs)

/// A type-erased AST object for serialisation round-trips.
///
/// This is the Rust equivalent of C's `AstObject *`. It grows as
/// new types are implemented.
pub enum AnyObject {
    UnitMap(UnitMap),
    ZoomMap(ZoomMap),
    ShiftMap(ShiftMap),
    MatrixMap(MatrixMap),
    PermMap(PermMap),
    CmpMap(CmpMap),
    BasicFrame(BasicFrame),
    CmpFrame(CmpFrame),
    FrameSet(FrameSet),
}

/// Serialise any AST object to Native text format.
///
/// Equivalent to C's `astToString`.
pub fn to_native_string(obj: &AnyObject) -> AstResult<String>;

/// Deserialise an AST object from Native text format.
///
/// Equivalent to C's `astFromString`.
pub fn from_native_string(s: &str) -> AstResult<AnyObject>;
```

`AnyObject` provides convenience accessors:

```rust
impl AnyObject {
    pub fn as_mapping(&self) -> Option<&dyn Mapping> { ... }
    pub fn as_frame(&self) -> Option<&dyn Frame> { ... }
    pub fn into_mapping(self) -> Option<DynMapping> { ... }
    pub fn into_frame(self) -> Option<DynFrame> { ... }
}
```

The internal `Serializable` trait (below) does the actual work;
the public functions dispatch through `AnyObject`.

## Serializable Trait

```rust
// pub(crate) in serial/mod.rs

pub(crate) trait Serializable {
    /// The AST class name for Begin/End markers.
    fn class_name(&self) -> &'static str;

    /// Serialise this object to a Node tree.
    fn to_node(&self) -> AstResult<Node>;

    /// Reconstruct from a Node tree.
    fn from_node(node: &Node) -> AstResult<Self> where Self: Sized;
}
```

### Dispatch Functions

Reconstruct a concrete type from a `Node` by matching on `node.class`:

```rust
pub(crate) fn mapping_from_node(node: &Node) -> AstResult<DynMapping> {
    match node.class.as_str() {
        "UnitMap" => Ok(Box::new(UnitMap::from_node(node)?)),
        "ZoomMap" => Ok(Box::new(ZoomMap::from_node(node)?)),
        "ShiftMap" => Ok(Box::new(ShiftMap::from_node(node)?)),
        "MatrixMap" => Ok(Box::new(MatrixMap::from_node(node)?)),
        "PermMap" => Ok(Box::new(PermMap::from_node(node)?)),
        "CmpMap" => Ok(Box::new(CmpMap::from_node(node)?)),
        "FrameSet" => Ok(Box::new(FrameSet::from_node(node)?)),
        _ => Err(AstError::new(
            ErrorCode::Loader,
            format!("unknown Mapping class: {}", node.class),
        )),
    }
}

pub(crate) fn frame_from_node(node: &Node) -> AstResult<DynFrame> {
    match node.class.as_str() {
        "Frame" => Ok(Box::new(BasicFrame::from_node(node)?)),
        "CmpFrame" => Ok(Box::new(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(Box::new(FrameSet::from_node(node)?)),
        _ => Err(AstError::new(
            ErrorCode::Loader,
            format!("unknown Frame class: {}", node.class),
        )),
    }
}
```

These grow as new types are implemented in later steps.

## Native Text Format

### Grammar

```
object       := "Begin" CLASSNAME comment? LF items "End" CLASSNAME comment? LF
items        := (item LF)*
item         := attribute | isa_marker | comment_line
attribute    := INDENT KEY "=" value comment?
isa_marker   := INDENT "IsA" CLASSNAME comment?
comment_line := INDENT "#" text
value        := INTEGER | DOUBLE | QUOTED_STRING | "" LF object
```

- **Indentation**: 3 spaces per nesting level. Cosmetic on input
  (parser ignores leading whitespace), required on output for
  byte-exact match.
- **Empty value + newline**: Signals a nested object on subsequent
  lines. The key line ends with `=` followed only by a tab and comment.
- **Commented defaults**: Lines of the form `#   Key = Value # comment`
  carry informational defaults. The `#` is at the indentation level,
  followed by spaces aligning with active attributes.
- **Strings**: Quoted with `"`. Internal `"` escaped by doubling.

### Float Formatting

The C library formats doubles with:
```c
sprintf(buff, "%.*g", AST__DBL_DIG, value)
```
where `AST__DBL_DIG` is typically 17 (IEEE 754 `DBL_DECIMAL_DIG`).

The Rust renderer must produce identical output. This means using
`format!("{:.prec$e}", value)` will NOT work — we need to replicate
C's `%g` behaviour (which switches between `%f` and `%e` based on
the exponent, and trims trailing zeros).

A helper function `format_double_g(value: f64, precision: usize)`
that mirrors C's `%.*g` is required. This is a known portability
concern; the function should be tested against C output for a range
of values including edge cases (very large, very small, exact
integers, negative zero).

### native_read (Parser)

Line-oriented parser:

1. Skip blank lines and leading whitespace.
2. On `Begin ClassName`: push a new Node context.
3. On `Key = Value`: parse value type (int, double, string, or nested
   object if value is empty). Append to current Node's items.
4. On `# Key = Value`: parse as `Unset(inner_value)`. Append to items.
5. On `IsA ClassName`: append IsA marker to current Node.
6. On `End ClassName`: pop context, return completed Node.
7. On `#` (standalone comment without key=value structure): skip or
   store as needed for round-trip.

The parser should accept any `.ast` or `.map` file, including those
with class names we haven't implemented yet. The Node tree is
class-agnostic — only `from_node` dispatch is class-aware.

### native_write (Renderer)

Walks the Node tree, emitting:

1. `Begin ClassName # comment` (comment from C is the class
   description — we store and reproduce it, or generate from a
   lookup table).
2. For each item in order:
   - Active attributes: `   Key = Value \t# Comment`
   - Unset attributes: `#  Key = Value \t# Comment`
   - IsA markers: `IsA ClassName \t# Comment`
   - Nested objects: `   Key = \t# Comment\n` followed by recursive
     object rendering at increased indent.
3. `End ClassName # comment`

Indentation is 3 spaces per nesting level, matching C output.

## Per-Class Serialisation Details

### Common: Mapping Base Attributes

All Mapping subclasses inherit these (written before `IsA Mapping`):

| Attribute | Key | Condition (write if) | Comment |
|-----------|-----|---------------------|---------|
| Input count | `Nin` | always (if != 0) | "Number of input coordinates" |
| Output count | `Nout` | nin != nout | "Number of output coordinates" |
| Simplified | `IsSimp` | always | "Mapping has been simplified" / "...not..." |
| Restricted simplify | `ReSimp` | always | "Simplify restricted..." / "...uses all..." |
| Allow simplify | `AlSimp` | always | "Will be used by restricted Simplify" / "...not..." |
| Invert flag | `Invert` | always | "Mapping inverted" / "...not inverted" |
| Forward defined | `Fwd` | tran_forward == 0 | "...not defined" |
| Inverse defined | `Inv` | tran_inverse == 0 | "...not defined" |
| Report | `Report` | if set | "Report coordinate transformations" |

After these: `IsA Mapping`.

### UnitMap

No class-specific attributes.

### ZoomMap

| Key | Condition | Comment |
|-----|-----------|---------|
| `Zoom` | if explicitly set | "Zoom factor" |

### ShiftMap

| Key | Condition | Comment |
|-----|-----------|---------|
| `Sft1`..`SftN` | if shift != 0.0 | "Shift for axis N" |

### MatrixMap

| Key | Condition | Comment |
|-----|-----------|---------|
| `M0`..`Mk` | if element != BAD_VALUE | "Forward matrix value" |
| `IM0`..`IMk` | if element != BAD_VALUE | "Inverse matrix value" |
| `Form` | always | "Matrix storage form" ("Full", "Diagonal", "Unit") |

Element count depends on form: Full = nin*nout, Diagonal = min(nin,nout).

### PermMap

| Key | Condition | Comment |
|-----|-----------|---------|
| `OutCpy` | if != 0 | "Output coordinates = input coordinates" |
| `Out1`..`OutN` | if outperm exists | "Output coordinate N = input coordinate M" |
| `InCpy` | if != 0 | "Input coordinates = output coordinates" |
| `In1`..`InM` | if inperm exists | "Input coordinate N = output coordinate M" |
| `Nconst` | if != 0 | "Number of constants" |
| `Con1`..`ConK` | per constant | "Constant number K" |

Constants equal to BAD_VALUE are written as the string `<bad>`.

### CmpMap

| Key | Condition | Comment |
|-----|-----------|---------|
| `Series` | if == 0 (parallel) | "Component Mappings applied in parallel" |
| `InvA` | if != 0 | "First Mapping used in inverse direction" |
| `InvB` | if != 0 | "Second Mapping used in inverse direction" |
| `MapA` | always (object) | "First component Mapping" |
| `MapB` | always (object) | "Second component Mapping" |

### Common: Frame Base Attributes

All Frame subclasses inherit these (written before `IsA Frame`).
Only the attributes our Rust types currently support are listed:

| Key | Condition | Comment |
|-----|-----------|---------|
| `Title` | if set | "Title of coordinate system" |
| `Naxes` | always | "Number of coordinate axes" |
| `Domain` | if set and non-blank | "Coordinate system domain" |
| `Epoch` | if set | "Julian epoch of observation" |
| `Lbl1`..`LblN` | informational (commented) | "Label for axis N" |
| `Sym1`..`SymN` | informational (commented) | "Symbol for axis N" |
| `Uni1`..`UniN` | if non-blank | "Units for axis N" |
| `Fmt1`..`FmtN` | if set | "Format specifier for axis N" |
| `Dir1`..`DirN` | if == 0 | "Plot axis N in reverse direction" |
| `Bot1`..`BotN` | if != -DBL_MAX | "Lowest legal axis value" |
| `Top1`..`TopN` | if != DBL_MAX | "Highest legal axis value" |
| `Ax1`..`AxN` | always (Axis objects) | "Axis number N" |

Then: `IsA Frame`.

Note: Several Frame attributes (System, AlSys, ObsLat, ObsLon,
ObsAlt, Dtai, Dut1, etc.) exist in the C library but are not yet
on BasicFrame. They will be serialised as `Unset` for now and added
to the Rust type in later steps.

### BasicFrame

Class name in Native format: `Frame` (not `BasicFrame`).

No class-specific attributes beyond the Frame base.

### CmpFrame

| Key | Condition | Comment |
|-----|-----------|---------|
| `Axp1`..`AxpN` | if permuted | "Axis N permuted to use internal axis M" |
| `FrameA` | always (object) | "First component Frame" |
| `FrameB` | always (object) | "Second component Frame" |

### FrameSet

| Key | Condition | Comment |
|-----|-----------|---------|
| `Nframe` | always | "Number of Frames in FrameSet" |
| `Base` | always | "Index of base Frame" |
| `Currnt` | always | "Index of current Frame" |
| `Nnode` | if != nframe | "Number of nodes in FrameSet" |
| `Nod1`..`NodN` | if != frame number | "Frame N is associated with node M" |
| `Lnk2`..`LnkN` | always | "Node N is derived from node M" |
| `Inv2`..`InvN` | if != 0 | "The inverse mapping is used" |
| `Frm1`..`FrmN` | always (objects) | "Frame number N" |
| `Map2`..`MapN` | always (objects) | "Mapping between nodes M and N" |

### Axis Object

The C library serialises Axis objects as children of Frame (`Ax1`,
`Ax2`, etc.). An `Axis` Node has class name `Axis` and attributes:

| Key | Condition | Comment |
|-----|-----------|---------|
| `Label` | if set | "Axis Label" |
| `Symbol` | if set | "Axis symbol" |
| `Unit` | if set | "Axis units" |
| `Format` | if set | "Axis format specifier" |
| `Digits` | if set | "Individual precision" |

`SkyAxis` is a subclass (deferred until SkyFrame).

There is no Rust `Axis` struct — BasicFrame stores per-axis
attributes directly. The `to_node` / `from_node` for BasicFrame
constructs and parses Axis child Nodes from its per-axis storage.

## Test Fixtures

```
crates/ast-core/tests/fixtures/
└── native/
    ├── mappings/          # C-generated for byte-exact round-trip
    │   ├── unitmap.ast
    │   ├── zoommap.ast
    │   ├── shiftmap.ast
    │   ├── matrixmap.ast
    │   ├── permmap.ast
    │   └── cmpmap.ast
    ├── frames/
    │   ├── basicframe.ast
    │   └── cmpframe.ast
    ├── framesets/
    │   └── frameset.ast
    └── reference/         # From ast_tester, parse-only (types not yet in Rust)
        ├── splittest1.ast
        ├── degen1.ast
        ├── brad.map
        └── ...
```

Files in `reference/` graduate to the proper type subdirectory
as each class gains its `from_node` implementation.

## Testing Strategy

### Tier A: Node and Serializable Unit Tests

- `Node` construction, attribute access, nested objects.
- `to_node` / `from_node` round-trip for each of the 9 types:
  create in Rust → `to_node` → `from_node` → verify attributes.
- Edge cases: omitted defaults, empty strings, BAD_VALUE constants.

### Tier B: Native Text Round-Trip (Byte-Exact)

- C fixture generator: a small C program in `crates/ast-ffi-tests/`
  creates representative objects of our 9 types, writes them via
  `astToString`, output committed as fixtures.
- Test: parse fixture → `Node` → render back to text → diff against
  original. Must be byte-exact.
- Test: parse fixture → `Node` → `from_node` → reconstruct Rust
  object → verify attributes and transform behaviour.

### Tier C: Parse-Only on Reference Fixtures

- Parse existing `.ast` and `.map` files from the reference
  directory into `Node` trees.
- Verify: parsing succeeds, class names correct, attribute counts
  reasonable, nesting structure well-formed.
- No `from_node` — validates parser handles real-world files with
  types not yet implemented.

## What This Step Does NOT Include

- JSON, XML, YAML format renderers/parsers.
- FITS-header-card encoding (FitsChan, Step 8).
- Channel trait and Channel implementations.
- SkyFrame, WinMap, PolyMap, or any types not yet in Rust.
- STC-S, MOC formats.
