# Step 1: Error Model, Traits, and Coordinate Types — Design Spec

## Summary

Define the core public API surface of `ast-core`: the `Mapping`, `Frame`,
and `Region` traits with full method signatures (bodies unimplemented),
the expanded error model, `PointSet` (ndarray-backed coordinate block),
`PointList` (struct only, Region impl deferred), and the `BAD_VALUE`
constant.

## Deliverables

1. Expanded `AstError` with context stack and `#[track_caller]` support.
2. `pub trait Mapping` with all spec methods (unimplemented bodies).
3. `pub trait Frame` with all spec methods including integrity support.
4. `pub trait Region: Frame + Mapping` with core methods.
5. `PointSet` backed by `ndarray::Array2<f64>`.
6. `PointList` struct (no trait impls yet).
7. `BAD_VALUE` constant.
8. Module restructure: directory-per-trait layout.
9. All tests pass, clippy clean, fmt clean.

## Module Structure

```
crates/ast-core/src/
├── lib.rs              # crate root; re-exports public modules
├── constants.rs        # BAD_VALUE
├── error.rs            # AstError, AstResult, ErrorCode (generated)
├── mappings/
│   └── mod.rs          # pub trait Mapping + type DynMapping
├── frames/
│   └── mod.rs          # pub trait Frame + type DynFrame
├── regions/
│   └── mod.rs          # pub trait Region + type DynRegion + Overlap
└── coords/
    ├── mod.rs          # re-exports PointSet, PointList
    ├── pointset.rs     # PointSet
    └── pointlist.rs    # PointList (struct only)
```

## Error Model (src/error.rs)

```rust
use std::fmt;
use std::panic::Location;

include!(concat!(env!("OUT_DIR"), "/error_codes.rs"));

/// An error produced by the AST library.
#[derive(Debug, Clone)]
pub struct AstError {
    code: ErrorCode,
    message: String,
    context: Vec<String>,
    source_location: Option<&'static Location<'static>>,
}

impl AstError {
    /// Create a new AST error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            source_location: None,
        }
    }

    /// Create a new AST error capturing the caller's source location.
    #[track_caller]
    pub fn new_tracked(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            source_location: Some(Location::caller()),
        }
    }

    pub fn code(&self) -> ErrorCode { self.code }
    pub fn message(&self) -> &str { &self.message }
    pub fn context(&self) -> &[String] { &self.context }
    pub fn source_location(&self) -> Option<&'static Location<'static>> {
        self.source_location
    }

    /// Add context to this error (for propagation up call stacks).
    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context.push(ctx.into());
        self
    }
}

impl fmt::Display for AstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        for ctx in &self.context {
            write!(f, "\n  context: {ctx}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AstError {}

/// A Result type alias for AST operations.
pub type AstResult<T> = Result<T, AstError>;
```

## Mapping Trait (src/mappings/mod.rs)

```rust
use crate::coords::PointSet;
use crate::error::AstResult;

/// An owned, dynamically-dispatched Mapping.
pub type DynMapping = Box<dyn Mapping>;

/// A coordinate transformation.
///
/// All concrete mapping classes implement this trait. Mappings are `Send`
/// (movable between threads) but not `Sync` (no shared references across
/// threads — mutation is lock-free).
pub trait Mapping: Send {
    /// Number of input coordinates.
    fn nin(&self) -> usize;

    /// Number of output coordinates.
    fn nout(&self) -> usize;

    /// Whether this mapping is inverted.
    fn is_inverted(&self) -> bool;

    /// Invert this mapping in place.
    fn invert(&mut self);

    /// Transform a set of points.
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>;

    /// Transform points into a pre-allocated output buffer.
    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()>;

    /// Return a simplified version of this mapping.
    fn simplified(&self) -> AstResult<DynMapping>;

    /// Rate of change of a specified output with respect to a specified input.
    ///
    /// `ax1` and `ax2` are 1-based axis indices.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64>;
}
```

## Frame Trait (src/frames/mod.rs)

```rust
use crate::error::AstResult;
use crate::mappings::DynMapping;

/// An owned, dynamically-dispatched Frame.
pub type DynFrame = Box<dyn Frame>;

/// A coordinate frame defining the meaning of coordinate values.
///
/// All concrete frame classes implement this trait. Frames are `Send`
/// but not `Sync`.
pub trait Frame: Send {
    /// Number of axes in this frame.
    fn naxes(&self) -> usize;

    /// Format a coordinate value for display.
    ///
    /// `axis` is 1-based.
    fn format(&self, axis: usize, value: f64) -> AstResult<String>;

    /// Parse a formatted coordinate string back to a numeric value.
    ///
    /// `axis` is 1-based. Returns (value, characters_consumed).
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)>;

    /// Geodesic distance between two points in this frame.
    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64>;

    /// Angle at point b in the triangle a-b-c.
    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64>;

    /// Move a point along the geodesic from `from` toward `to` by `dist`.
    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>>;

    /// Normalise coordinates to their canonical range.
    fn norm(&self, coords: &mut [f64]) -> AstResult<()>;

    /// Deep-clone this frame for integrity delta computation.
    fn snapshot(&self) -> DynFrame;

    /// Compute the mapping from a previous state of this frame to the current state.
    ///
    /// Used by FrameSet/Region integrity: after mutating a frame attribute,
    /// this returns the transformation that maps old coordinates to new.
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;
}
```

## Region Trait (src/regions/mod.rs)

```rust
use crate::error::AstResult;
use crate::frames::Frame;
use crate::mappings::Mapping;

/// An owned, dynamically-dispatched Region.
pub type DynRegion = Box<dyn Region>;

/// Result of comparing two regions for overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// No overlap between the regions.
    None,
    /// Partial overlap.
    Partial,
    /// The first region is entirely inside the second.
    FirstInside,
    /// The second region is entirely inside the first.
    SecondInside,
    /// The regions are identical.
    Same,
}

/// A region of a coordinate frame.
///
/// Regions implement both `Frame` (they describe a coordinate system)
/// and `Mapping` (they transform points via the inside/outside predicate).
pub trait Region: Frame + Mapping {
    /// Test whether a point is inside this region.
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    /// Determine the overlap between this region and another.
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;

    /// Return a negated copy of this region.
    fn negated(&self) -> DynRegion;
}
```

## PointSet (src/coords/pointset.rs)

```rust
use ndarray::{Array2, ArrayView2, ArrayViewMut2};
use crate::error::AstResult;

/// Frame-unaware coordinate block.
///
/// Row-major layout: shape = `(npoint, ncoord)`. Each row is one point;
/// each column is one coordinate axis.
pub struct PointSet {
    data: Array2<f64>,
}

impl PointSet {
    /// Create a PointSet filled with zeros.
    pub fn zeros(npoint: usize, ncoord: usize) -> Self;

    /// Create from an iterator of points (each point is a slice of coordinates).
    pub fn from_rows(rows: impl IntoIterator<Item = impl AsRef<[f64]>>) -> Self;

    /// Create from column slices (one slice per axis). All must be the same length.
    pub fn from_columns(cols: &[&[f64]]) -> AstResult<Self>;

    /// Create from a pre-built ndarray.
    pub fn from_ndarray(a: Array2<f64>) -> Self;

    /// Number of points.
    pub fn npoint(&self) -> usize;

    /// Number of coordinates per point.
    pub fn ncoord(&self) -> usize;

    /// View the underlying array.
    pub fn view(&self) -> ArrayView2<f64>;

    /// Mutable view of the underlying array.
    pub fn view_mut(&mut self) -> ArrayViewMut2<f64>;

    /// Consume and return the underlying ndarray.
    pub fn into_ndarray(self) -> Array2<f64>;

    /// Access a single point by index.
    pub fn point(&self, i: usize) -> &[f64];

    /// Mutable access to a single point by index.
    pub fn point_mut(&mut self, i: usize) -> &mut [f64];

    /// Iterate over values in a single axis (column).
    ///
    /// `j` is 0-based (unlike the 1-based axis indices in Frame methods).
    /// This is because PointSet is frame-unaware and operates on raw array indices.
    pub fn axis_iter(&self, j: usize) -> impl Iterator<Item = f64> + '_;
}
```

Note: `axis_iter` uses 0-based indexing because PointSet is frame-unaware —
it's just an array. The 1-based convention applies only in Frame/Mapping
methods that have semantic axis meaning.

## PointList (src/coords/pointlist.rs)

```rust
use crate::frames::{DynFrame, Frame};
use crate::error::AstResult;
use super::PointSet;

/// Frame-aware point collection.
///
/// Wraps a `PointSet` with a `Frame` that gives meaning to the coordinate
/// values. Will implement `Region` (and thus `Frame + Mapping`) once
/// BasicFrame and the integrity mechanism are available (Step 3+).
pub struct PointList {
    frame: DynFrame,
    points: PointSet,
}

impl PointList {
    /// Create a new PointList. The PointSet's ncoord must match the frame's naxes.
    pub fn new(frame: DynFrame, points: PointSet) -> AstResult<Self>;

    /// Access the frame.
    pub fn frame(&self) -> &dyn Frame;

    /// Access the points.
    pub fn points(&self) -> &PointSet;
}
```

## Constants (src/constants.rs)

```rust
/// The AST "bad value" sentinel, equivalent to C's `AST__BAD`.
///
/// Used to mark undefined or out-of-bounds coordinate values in outputs.
/// Inputs to transform functions also accept `f64::NAN`; outputs always
/// use this value.
pub const BAD_VALUE: f64 = -f64::MAX;
```

## Dependencies

Add to `crates/ast-core/Cargo.toml`:

```toml
[dependencies]
ndarray = "0.16"
```

## Testing Strategy

- Unit tests for `PointSet` (construction, access, from_columns error case)
- Unit test for `BAD_VALUE` (verify it equals `-f64::MAX`)
- Unit test for `AstError` (with_context chaining, Display format)
- Compilation test: define a dummy struct implementing all three traits
  (verifying the trait signatures are coherent and `Region: Frame + Mapping`
  compiles)

## What This Step Does NOT Include

- No concrete Mapping implementations (Step 2: UnitMap, ZoomMap, etc.)
- No concrete Frame implementations (Step 3: BasicFrame, CmpFrame)
- No FrameSet (Step 4)
- No Region implementations or PointList's Region impl (Step 3+)
- No serialization
- No `convert()` method on Frame (it returns FrameSet, which is Step 4)
