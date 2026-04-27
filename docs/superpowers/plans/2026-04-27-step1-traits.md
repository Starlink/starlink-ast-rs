# Step 1: Traits, Error Model, and Coordinate Types — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Define the core public API surface of ast-core: Mapping/Frame/Region traits, expanded error model, PointSet, PointList struct, and BAD_VALUE constant.

**Architecture:** Directory-per-trait module structure. Traits have full method signatures with unimplemented bodies. PointSet is backed by ndarray::Array2<f64>. PointList is a struct wrapping DynFrame + PointSet (no trait impls yet).

**Tech Stack:** Rust 2024 edition, ndarray 0.16

---

## File Structure

| File | Responsibility |
|------|---------------|
| `Cargo.toml` (workspace) | Add ndarray to workspace deps |
| `crates/ast-core/Cargo.toml` | Add ndarray dependency |
| `crates/ast-core/src/lib.rs` | Declare all modules |
| `crates/ast-core/src/constants.rs` | BAD_VALUE constant |
| `crates/ast-core/src/error.rs` | Expanded AstError with context, track_caller |
| `crates/ast-core/src/mappings/mod.rs` | trait Mapping + DynMapping |
| `crates/ast-core/src/frames/mod.rs` | trait Frame + DynFrame |
| `crates/ast-core/src/regions/mod.rs` | trait Region + DynRegion + Overlap |
| `crates/ast-core/src/coords/mod.rs` | Re-exports PointSet, PointList |
| `crates/ast-core/src/coords/pointset.rs` | PointSet implementation |
| `crates/ast-core/src/coords/pointlist.rs` | PointList struct (no trait impls) |
| `crates/ast-core/tests/error_model.rs` | Tests for expanded AstError |
| `crates/ast-core/tests/pointset.rs` | Tests for PointSet |
| `crates/ast-core/tests/traits_compile.rs` | Compilation test for trait coherence |

---

### Task 1: Add ndarray Dependency and Constants Module

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/ast-core/Cargo.toml`
- Create: `crates/ast-core/src/constants.rs`
- Modify: `crates/ast-core/src/lib.rs`

- [ ] **Step 1: Add ndarray to workspace dependencies**

Edit `Cargo.toml` (workspace root) — add ndarray to `[workspace.dependencies]`:

```toml
[workspace.dependencies]
cc = "1"
ndarray = "0.16"
```

- [ ] **Step 2: Add ndarray to ast-core's dependencies**

Edit `crates/ast-core/Cargo.toml` — add a `[dependencies]` section:

```toml
[package]
name = "ast-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Pure-Rust port of the Starlink AST world coordinate system library"

[dependencies]
ndarray.workspace = true

[build-dependencies]
cc.workspace = true
```

- [ ] **Step 3: Create constants module**

Create `crates/ast-core/src/constants.rs`:

```rust
//! Library-wide constants.

/// The AST "bad value" sentinel, equivalent to C's `AST__BAD`.
///
/// Used to mark undefined or out-of-bounds coordinate values in outputs.
/// Inputs to transform functions also accept `f64::NAN`; outputs always
/// use this value.
pub const BAD_VALUE: f64 = -f64::MAX;
```

- [ ] **Step 4: Update lib.rs to declare constants module**

Edit `crates/ast-core/src/lib.rs`:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod error;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/ast-core/Cargo.toml crates/ast-core/src/constants.rs crates/ast-core/src/lib.rs
git commit -m "Add ndarray dependency and constants module (BAD_VALUE)"
```

---

### Task 2: Expand Error Model

**Files:**
- Modify: `crates/ast-core/src/error.rs`
- Create: `crates/ast-core/tests/error_model.rs`

- [ ] **Step 1: Write tests for expanded error model**

Create `crates/ast-core/tests/error_model.rs`:

```rust
use ast_core::error::{AstError, ErrorCode};

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
    let err = AstError::new(ErrorCode::Atger, "failed")
        .with_context("in transform");
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ast-core --test error_model`
Expected: compilation failure (no `context()`, `with_context()`, `new_tracked()`, `source_location()`)

- [ ] **Step 3: Implement expanded error model**

Replace `crates/ast-core/src/error.rs` with:

```rust
//! Error types for the AST library.

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
    /// Create a new AST error with the given code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            source_location: None,
        }
    }

    /// Create a new AST error, capturing the caller's source location.
    #[track_caller]
    pub fn new_tracked(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            source_location: Some(Location::caller()),
        }
    }

    /// Return the error code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Return the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return the context stack.
    pub fn context(&self) -> &[String] {
        &self.context
    }

    /// Return the source location where this error was created, if tracked.
    pub fn source_location(&self) -> Option<&'static Location<'static>> {
        self.source_location
    }

    /// Add context to this error for propagation up call stacks.
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

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ast-core --test error_model`
Expected: all 5 tests pass

- [ ] **Step 5: Verify existing error_codes tests still pass**

Run: `cargo test -p ast-core`
Expected: all tests pass (error_codes + error_model)

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/error.rs crates/ast-core/tests/error_model.rs
git commit -m "Expand AstError: context stack, source_location, track_caller"
```

---

### Task 3: Define Mapping Trait

**Files:**
- Create: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/lib.rs`

- [ ] **Step 1: Create mappings directory**

```bash
mkdir -p crates/ast-core/src/mappings
```

- [ ] **Step 2: Write the Mapping trait**

Create `crates/ast-core/src/mappings/mod.rs`:

```rust
//! The Mapping trait and associated types.

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

    /// Whether this mapping is currently inverted.
    fn is_inverted(&self) -> bool;

    /// Invert this mapping in place.
    fn invert(&mut self);

    /// Transform a set of points.
    ///
    /// If `forward` is true, applies the forward transformation; otherwise
    /// applies the inverse.
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>;

    /// Transform points into a pre-allocated output buffer.
    ///
    /// `out` must have shape `(pts.npoint(), self.nout())` for forward or
    /// `(pts.npoint(), self.nin())` for inverse transforms.
    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()>;

    /// Return a simplified version of this mapping.
    ///
    /// The simplified mapping is functionally equivalent but may be more
    /// efficient (e.g. cancelling inverse pairs, merging linear maps).
    fn simplified(&self) -> AstResult<DynMapping>;

    /// Rate of change of a specified output with respect to a specified input.
    ///
    /// `ax1` is the 1-based index of the output axis.
    /// `ax2` is the 1-based index of the input axis.
    /// `at` is the position at which to evaluate the rate.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64>;
}
```

- [ ] **Step 3: Update lib.rs**

Edit `crates/ast-core/src/lib.rs`:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod coords;
pub mod error;
pub mod frames;
pub mod mappings;
pub mod regions;
```

Note: This will not compile yet because `coords`, `frames`, and `regions`
modules don't exist. We'll create them in subsequent tasks. For now, only
add `pub mod mappings;` — the other modules will be added as they're created.

Actually, to keep each task independently compilable, update lib.rs to only:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod error;
pub mod mappings;
```

But `mappings/mod.rs` references `crate::coords::PointSet` which doesn't
exist yet. We need to define the modules in dependency order. The dependency
chain is:

- `constants` — no deps
- `error` — no deps
- `coords` — depends on `error`
- `mappings` — depends on `coords`, `error`
- `frames` — depends on `mappings`, `error`
- `regions` — depends on `frames`, `mappings`, `error`

So we must create `coords` (at least `PointSet`) before `mappings`. Let me
restructure: this task will create the `coords` module first, then
`mappings`.

Revised lib.rs for this task:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod coords;
pub mod error;
pub mod mappings;
```

- [ ] **Step 4: Create coords module (PointSet only for now)**

```bash
mkdir -p crates/ast-core/src/coords
```

Create `crates/ast-core/src/coords/mod.rs`:

```rust
//! Coordinate array types.

mod pointset;

pub use pointset::PointSet;
```

Create `crates/ast-core/src/coords/pointset.rs`:

```rust
//! Frame-unaware coordinate block.

use ndarray::{Array2, ArrayView2, ArrayViewMut2};

use crate::error::{AstError, AstResult, ErrorCode};

/// Frame-unaware coordinate block.
///
/// Row-major layout: shape = `(npoint, ncoord)`. Each row is one point;
/// each column is one coordinate axis.
#[derive(Debug, Clone)]
pub struct PointSet {
    data: Array2<f64>,
}

impl PointSet {
    /// Create a PointSet filled with zeros.
    pub fn zeros(npoint: usize, ncoord: usize) -> Self {
        Self {
            data: Array2::zeros((npoint, ncoord)),
        }
    }

    /// Create from an iterator of points (each point is a slice of coordinates).
    ///
    /// All points must have the same length. Panics if the iterator is empty.
    pub fn from_rows(rows: impl IntoIterator<Item = impl AsRef<[f64]>>) -> Self {
        let rows: Vec<Vec<f64>> = rows.into_iter().map(|r| r.as_ref().to_vec()).collect();
        assert!(!rows.is_empty(), "PointSet::from_rows requires at least one row");
        let ncoord = rows[0].len();
        let npoint = rows.len();
        let flat: Vec<f64> = rows.into_iter().flatten().collect();
        let data = Array2::from_shape_vec((npoint, ncoord), flat).unwrap();
        Self { data }
    }

    /// Create from column slices (one slice per coordinate axis).
    ///
    /// All columns must have the same length.
    pub fn from_columns(cols: &[&[f64]]) -> AstResult<Self> {
        if cols.is_empty() {
            return Err(AstError::new(
                ErrorCode::Ncoin,
                "from_columns requires at least one column",
            ));
        }
        let npoint = cols[0].len();
        for (i, col) in cols.iter().enumerate() {
            if col.len() != npoint {
                return Err(AstError::new(
                    ErrorCode::Ncpin,
                    format!(
                        "column {i} has length {} but expected {npoint}",
                        col.len()
                    ),
                ));
            }
        }
        let ncoord = cols.len();
        let mut data = Array2::zeros((npoint, ncoord));
        for (j, col) in cols.iter().enumerate() {
            for (i, &val) in col.iter().enumerate() {
                data[[i, j]] = val;
            }
        }
        Ok(Self { data })
    }

    /// Create from a pre-built ndarray.
    pub fn from_ndarray(a: Array2<f64>) -> Self {
        Self { data: a }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.data.nrows()
    }

    /// Number of coordinates per point.
    pub fn ncoord(&self) -> usize {
        self.data.ncols()
    }

    /// View the underlying array.
    pub fn view(&self) -> ArrayView2<f64> {
        self.data.view()
    }

    /// Mutable view of the underlying array.
    pub fn view_mut(&mut self) -> ArrayViewMut2<f64> {
        self.data.view_mut()
    }

    /// Consume and return the underlying ndarray.
    pub fn into_ndarray(self) -> Array2<f64> {
        self.data
    }

    /// Access a single point by index.
    ///
    /// Panics if `i >= npoint()`.
    pub fn point(&self, i: usize) -> &[f64] {
        self.data.row(i).to_slice().expect("row is contiguous in C-order array")
    }

    /// Mutable access to a single point by index.
    ///
    /// Panics if `i >= npoint()`.
    pub fn point_mut(&mut self, i: usize) -> &mut [f64] {
        self.data
            .row_mut(i)
            .into_slice()
            .expect("row is contiguous in C-order array")
    }

    /// Iterate over values in a single coordinate axis (column).
    ///
    /// `j` is 0-based (PointSet is frame-unaware and uses raw array indices).
    /// Panics if `j >= ncoord()`.
    pub fn axis_iter(&self, j: usize) -> impl Iterator<Item = f64> + '_ {
        self.data.column(j).iter().copied()
    }
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/mappings/ crates/ast-core/src/coords/ crates/ast-core/src/lib.rs
git commit -m "Add Mapping trait and PointSet (ndarray-backed coordinate block)"
```

---

### Task 4: Define Frame Trait

**Files:**
- Create: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/lib.rs`

- [ ] **Step 1: Create frames directory**

```bash
mkdir -p crates/ast-core/src/frames
```

- [ ] **Step 2: Write the Frame trait**

Create `crates/ast-core/src/frames/mod.rs`:

```rust
//! The Frame trait and associated types.

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
    /// `axis` is 1-based. Returns `(value, characters_consumed)`.
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)>;

    /// Geodesic distance between two points in this frame.
    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64>;

    /// Angle at point `b` in the triangle a-b-c.
    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64>;

    /// Move along the geodesic from `from` toward `to` by distance `dist`.
    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>>;

    /// Normalise coordinates to their canonical range.
    fn norm(&self, coords: &mut [f64]) -> AstResult<()>;

    /// Deep-clone this frame for integrity delta computation.
    ///
    /// Used by FrameSet and Region integrity: a snapshot is taken before
    /// mutation, then `map_from` computes the correction mapping.
    fn snapshot(&self) -> DynFrame;

    /// Compute the mapping from a previous state of this frame to the current state.
    ///
    /// After mutating a frame attribute, this returns the transformation that
    /// maps coordinates in the old frame to coordinates in the new frame.
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;
}
```

- [ ] **Step 3: Add frames module to lib.rs**

Edit `crates/ast-core/src/lib.rs`:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod coords;
pub mod error;
pub mod frames;
pub mod mappings;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/ crates/ast-core/src/lib.rs
git commit -m "Add Frame trait with integrity support methods"
```

---

### Task 5: Define Region Trait

**Files:**
- Create: `crates/ast-core/src/regions/mod.rs`
- Modify: `crates/ast-core/src/lib.rs`

- [ ] **Step 1: Create regions directory**

```bash
mkdir -p crates/ast-core/src/regions
```

- [ ] **Step 2: Write the Region trait**

Create `crates/ast-core/src/regions/mod.rs`:

```rust
//! The Region trait and associated types.

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
/// Regions implement both [`Frame`] (they describe a coordinate system)
/// and [`Mapping`] (they transform points via the inside/outside predicate).
pub trait Region: Frame + Mapping {
    /// Test whether a point is inside this region.
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    /// Determine the overlap between this region and another.
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;

    /// Return a negated copy of this region.
    fn negated(&self) -> DynRegion;
}
```

- [ ] **Step 3: Add regions module to lib.rs**

Edit `crates/ast-core/src/lib.rs`:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod coords;
pub mod error;
pub mod frames;
pub mod mappings;
pub mod regions;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/regions/ crates/ast-core/src/lib.rs
git commit -m "Add Region trait (inherits Frame + Mapping) and Overlap enum"
```

---

### Task 6: Add PointList Struct

**Files:**
- Create: `crates/ast-core/src/coords/pointlist.rs`
- Modify: `crates/ast-core/src/coords/mod.rs`

- [ ] **Step 1: Write PointList**

Create `crates/ast-core/src/coords/pointlist.rs`:

```rust
//! Frame-aware point collection.

use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame};

use super::PointSet;

/// Frame-aware point collection.
///
/// Wraps a [`PointSet`] with a [`Frame`] that gives meaning to the coordinate
/// values. Will implement `Region` (and thus `Frame + Mapping`) once
/// BasicFrame and the integrity mechanism are available (Step 3+).
#[derive(Debug)]
pub struct PointList {
    frame: DynFrame,
    points: PointSet,
}

impl PointList {
    /// Create a new PointList.
    ///
    /// The PointSet's `ncoord` must equal the frame's `naxes`.
    pub fn new(frame: DynFrame, points: PointSet) -> AstResult<Self> {
        if points.ncoord() != frame.naxes() {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!(
                    "PointSet has {} coordinates but frame has {} axes",
                    points.ncoord(),
                    frame.naxes()
                ),
            ));
        }
        Ok(Self { frame, points })
    }

    /// Access the frame.
    pub fn frame(&self) -> &dyn Frame {
        &*self.frame
    }

    /// Access the points.
    pub fn points(&self) -> &PointSet {
        &self.points
    }
}
```

- [ ] **Step 2: Update coords/mod.rs to export PointList**

Edit `crates/ast-core/src/coords/mod.rs`:

```rust
//! Coordinate array types.

mod pointlist;
mod pointset;

pub use pointlist::PointList;
pub use pointset::PointSet;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success. Note: `PointList` has a `Debug` derive which requires
`DynFrame` (i.e. `Box<dyn Frame>`) to implement `Debug`. Since `Frame: Send`
doesn't require `Debug`, we need the `PointList` to use manual Debug or
skip deriving. Let's handle this: remove the `#[derive(Debug)]` from
`PointList` and implement it manually:

Replace the derive line with:

```rust
impl fmt::Debug for PointList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointList")
            .field("naxes", &self.frame.naxes())
            .field("npoint", &self.points.npoint())
            .finish()
    }
}
```

And add `use std::fmt;` at the top.

If the build fails due to Debug, apply this fix.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/coords/
git commit -m "Add PointList struct (frame-aware coordinate collection)"
```

---

### Task 7: Add PointSet Tests

**Files:**
- Create: `crates/ast-core/tests/pointset.rs`

- [ ] **Step 1: Write PointSet tests**

Create `crates/ast-core/tests/pointset.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;

#[test]
fn zeros_creates_correct_shape() {
    let ps = PointSet::zeros(10, 3);
    assert_eq!(ps.npoint(), 10);
    assert_eq!(ps.ncoord(), 3);
    assert_eq!(ps.point(0), &[0.0, 0.0, 0.0]);
}

#[test]
fn from_rows_basic() {
    let ps = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]);
    assert_eq!(ps.npoint(), 3);
    assert_eq!(ps.ncoord(), 2);
    assert_eq!(ps.point(0), &[1.0, 2.0]);
    assert_eq!(ps.point(2), &[5.0, 6.0]);
}

#[test]
fn from_columns_basic() {
    let x = [1.0, 2.0, 3.0];
    let y = [4.0, 5.0, 6.0];
    let ps = PointSet::from_columns(&[&x, &y]).unwrap();
    assert_eq!(ps.npoint(), 3);
    assert_eq!(ps.ncoord(), 2);
    assert_eq!(ps.point(0), &[1.0, 4.0]);
    assert_eq!(ps.point(1), &[2.0, 5.0]);
}

#[test]
fn from_columns_mismatched_lengths() {
    let x = [1.0, 2.0, 3.0];
    let y = [4.0, 5.0];
    let result = PointSet::from_columns(&[&x, &y]);
    assert!(result.is_err());
}

#[test]
fn from_columns_empty() {
    let result = PointSet::from_columns(&[]);
    assert!(result.is_err());
}

#[test]
fn point_mut_modifies_in_place() {
    let mut ps = PointSet::zeros(2, 3);
    ps.point_mut(0)[1] = 42.0;
    assert_eq!(ps.point(0), &[0.0, 42.0, 0.0]);
}

#[test]
fn axis_iter_returns_column() {
    let ps = PointSet::from_rows([[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]);
    let col0: Vec<f64> = ps.axis_iter(0).collect();
    let col1: Vec<f64> = ps.axis_iter(1).collect();
    assert_eq!(col0, vec![1.0, 2.0, 3.0]);
    assert_eq!(col1, vec![10.0, 20.0, 30.0]);
}

#[test]
fn view_and_into_ndarray() {
    let ps = PointSet::from_rows([[1.0, 2.0]]);
    let view = ps.view();
    assert_eq!(view[[0, 0]], 1.0);
    assert_eq!(view[[0, 1]], 2.0);

    let arr = ps.into_ndarray();
    assert_eq!(arr.shape(), &[1, 2]);
}

#[test]
fn bad_value_constant() {
    assert_eq!(BAD_VALUE, -f64::MAX);
    assert!(BAD_VALUE.is_finite());
    assert!(BAD_VALUE < 0.0);
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ast-core --test pointset`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/pointset.rs
git commit -m "Add PointSet and BAD_VALUE tests"
```

---

### Task 8: Trait Compilation Test

**Files:**
- Create: `crates/ast-core/tests/traits_compile.rs`

- [ ] **Step 1: Write compilation test**

This test verifies that the trait definitions are coherent: you can define
a struct implementing all three traits, and `Region: Frame + Mapping` compiles.

Create `crates/ast-core/tests/traits_compile.rs`:

```rust
//! Compilation test: verifies trait signatures are coherent and Region: Frame + Mapping works.

use ast_core::coords::PointSet;
use ast_core::error::{AstError, AstResult, ErrorCode};
use ast_core::frames::{DynFrame, Frame};
use ast_core::mappings::{DynMapping, Mapping};
use ast_core::regions::{DynRegion, Overlap, Region};

/// A dummy type that implements all three traits, proving they're coherent.
struct DummyRegion;

impl Mapping for DummyRegion {
    fn nin(&self) -> usize { 2 }
    fn nout(&self) -> usize { 2 }
    fn is_inverted(&self) -> bool { false }
    fn invert(&mut self) {}
    fn transform(&self, _pts: &PointSet, _forward: bool) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn transform_into(&self, _pts: &PointSet, _forward: bool, _out: &mut PointSet) -> AstResult<()> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn simplified(&self) -> AstResult<DynMapping> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn rate(&self, _at: &[f64], _ax1: usize, _ax2: usize) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
}

impl Frame for DummyRegion {
    fn naxes(&self) -> usize { 2 }
    fn format(&self, _axis: usize, _value: f64) -> AstResult<String> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn unformat(&self, _axis: usize, _text: &str) -> AstResult<(f64, usize)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn distance(&self, _a: &[f64], _b: &[f64]) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn angle(&self, _a: &[f64], _b: &[f64], _c: &[f64]) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn offset(&self, _from: &[f64], _to: &[f64], _dist: f64) -> AstResult<Vec<f64>> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn snapshot(&self) -> DynFrame {
        panic!("not implemented")
    }
    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
}

impl Region for DummyRegion {
    fn point_in_region(&self, _point: &[f64]) -> AstResult<bool> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn overlap(&self, _other: &dyn Region) -> AstResult<Overlap> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn negated(&self) -> DynRegion {
        panic!("not implemented")
    }
}

#[test]
fn dummy_region_implements_all_traits() {
    let r = DummyRegion;

    // Verify trait methods are callable
    assert_eq!(r.nin(), 2);
    assert_eq!(r.naxes(), 2);
    assert!(r.point_in_region(&[0.0, 0.0]).is_err());
}

#[test]
fn region_is_object_safe() {
    // Verify we can create trait objects
    let _: Box<dyn Region> = Box::new(DummyRegion);
    let _: Box<dyn Frame> = Box::new(DummyRegion);
    let _: Box<dyn Mapping> = Box::new(DummyRegion);
}

#[test]
fn dyn_types_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Box<dyn Mapping>>();
    assert_send::<Box<dyn Frame>>();
    assert_send::<Box<dyn Region>>();
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p ast-core --test traits_compile`
Expected: all 3 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/traits_compile.rs
git commit -m "Add trait compilation tests verifying coherence and object safety"
```

---

### Task 9: Clippy and Fmt Cleanup

**Files:**
- Possibly modify any file with clippy/fmt issues

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

If there are warnings, fix them.

- [ ] **Step 2: Run fmt check**

Run: `cargo fmt --all -- --check`

If there are formatting issues, run `cargo fmt --all` to fix.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all tests pass (error_codes, error_model, pointset, traits_compile)

- [ ] **Step 4: Commit any fixes**

```bash
git add -u
git commit -m "Fix clippy and fmt issues"
```

(Skip this commit if there were no issues.)

---

## Verification Checklist

After all tasks are complete, verify:

1. `cargo build --workspace` — all crates compile
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. Module structure matches the spec layout
6. `git status` — clean working tree
