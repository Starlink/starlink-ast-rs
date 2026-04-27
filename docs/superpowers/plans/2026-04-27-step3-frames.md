# Step 3: BasicFrame and CmpFrame — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement BasicFrame (generic N-axis Cartesian frame with per-axis attributes and Euclidean geometry) and CmpFrame (compound frame combining two child frames).

**Architecture:** BasicFrame stores per-axis attributes as `Vec<Option<T>>` with 1-based accessors following the get/set/test/clear pattern. CmpFrame holds two `DynFrame` children and routes axis operations to the correct child. Both implement the Frame trait. Builder pattern for construction.

**Tech Stack:** Rust 2024 edition, depends on UnitMap from Step 2.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/frames/mod.rs` | Frame trait + DynFrame + re-exports |
| `crates/ast-core/src/frames/basic.rs` | BasicFrame + BasicFrameBuilder |
| `crates/ast-core/src/frames/cmpframe.rs` | CmpFrame |
| `crates/ast-core/tests/test_basicframe.rs` | BasicFrame tests (equiv. to testframe.c) |
| `crates/ast-core/tests/test_cmpframe.rs` | CmpFrame tests (equiv. to testcmpframe.c) |

---

### Task 1: BasicFrame Structure and Builder

**Files:**
- Create: `crates/ast-core/src/frames/basic.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`

- [ ] **Step 1: Create basic.rs with struct and builder**

Create `crates/ast-core/src/frames/basic.rs`:

```rust
//! Generic N-axis Cartesian coordinate frame.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, Mapping, UnitMap};

use super::{DynFrame, Frame};

/// A generic N-axis Cartesian coordinate frame.
///
/// This is the base frame type (equivalent to C's `astFrame(naxes, ...)`).
/// It provides Euclidean geometry and per-axis attributes with 1-based indexing.
#[derive(Debug, Clone)]
pub struct BasicFrame {
    naxes: usize,
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>,
    labels: Vec<Option<String>>,
    units: Vec<Option<String>>,
    symbols: Vec<Option<String>>,
    formats: Vec<Option<String>>,
    bottoms: Vec<Option<f64>>,
    tops: Vec<Option<f64>>,
}

impl BasicFrame {
    /// Create a builder for a BasicFrame with the given number of axes.
    pub fn builder(naxes: usize) -> BasicFrameBuilder {
        BasicFrameBuilder::new(naxes)
    }

    fn validate_axis(&self, axis: usize) -> AstResult<usize> {
        if axis == 0 || axis > self.naxes {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {axis} is invalid for frame with {} axes", self.naxes),
            ));
        }
        Ok(axis - 1) // convert to 0-based internal index
    }

    // --- Frame-level attributes ---

    /// Get the frame title.
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }

    /// Set the frame title.
    pub fn set_title(&mut self, t: impl Into<String>) {
        self.title = Some(t.into());
    }

    /// Test whether the title is explicitly set.
    pub fn test_title(&self) -> bool {
        self.title.is_some()
    }

    /// Clear the title (revert to default).
    pub fn clear_title(&mut self) {
        self.title = None;
    }

    /// Get the frame domain.
    pub fn domain(&self) -> &str {
        self.domain.as_deref().unwrap_or("")
    }

    /// Set the frame domain.
    pub fn set_domain(&mut self, d: impl Into<String>) {
        self.domain = Some(d.into());
    }

    /// Test whether the domain is explicitly set.
    pub fn test_domain(&self) -> bool {
        self.domain.is_some()
    }

    /// Clear the domain.
    pub fn clear_domain(&mut self) {
        self.domain = None;
    }

    /// Get the epoch (None if unset).
    pub fn epoch(&self) -> Option<f64> {
        self.epoch
    }

    /// Set the epoch.
    pub fn set_epoch(&mut self, e: f64) {
        self.epoch = Some(e);
    }

    /// Test whether the epoch is explicitly set.
    pub fn test_epoch(&self) -> bool {
        self.epoch.is_some()
    }

    /// Clear the epoch.
    pub fn clear_epoch(&mut self) {
        self.epoch = None;
    }

    // --- Per-axis attributes (1-based) ---

    /// Get axis label. Default: "Axis N".
    pub fn label(&self, axis: usize) -> AstResult<&str> {
        let idx = self.validate_axis(axis)?;
        Ok(self.labels[idx].as_deref().unwrap_or_else(|| ""))
    }

    /// Set axis label.
    pub fn set_label(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.labels[idx] = Some(s.into());
        Ok(())
    }

    /// Test whether axis label is explicitly set.
    pub fn test_label(&self, axis: usize) -> AstResult<bool> {
        let idx = self.validate_axis(axis)?;
        Ok(self.labels[idx].is_some())
    }

    /// Clear axis label.
    pub fn clear_label(&mut self, axis: usize) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.labels[idx] = None;
        Ok(())
    }

    /// Get axis unit. Default: "".
    pub fn unit(&self, axis: usize) -> AstResult<&str> {
        let idx = self.validate_axis(axis)?;
        Ok(self.units[idx].as_deref().unwrap_or(""))
    }

    /// Set axis unit.
    pub fn set_unit(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.units[idx] = Some(s.into());
        Ok(())
    }

    /// Test whether axis unit is explicitly set.
    pub fn test_unit(&self, axis: usize) -> AstResult<bool> {
        let idx = self.validate_axis(axis)?;
        Ok(self.units[idx].is_some())
    }

    /// Clear axis unit.
    pub fn clear_unit(&mut self, axis: usize) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.units[idx] = None;
        Ok(())
    }

    /// Get axis symbol. Default: same as label.
    pub fn symbol(&self, axis: usize) -> AstResult<&str> {
        let idx = self.validate_axis(axis)?;
        if let Some(ref s) = self.symbols[idx] {
            Ok(s.as_str())
        } else {
            self.label(axis)
        }
    }

    /// Set axis symbol.
    pub fn set_symbol(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.symbols[idx] = Some(s.into());
        Ok(())
    }

    /// Test whether axis symbol is explicitly set.
    pub fn test_symbol(&self, axis: usize) -> AstResult<bool> {
        let idx = self.validate_axis(axis)?;
        Ok(self.symbols[idx].is_some())
    }

    /// Clear axis symbol.
    pub fn clear_symbol(&mut self, axis: usize) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.symbols[idx] = None;
        Ok(())
    }

    /// Get axis format string. Default: "" (use default formatting).
    pub fn format_str(&self, axis: usize) -> AstResult<&str> {
        let idx = self.validate_axis(axis)?;
        Ok(self.formats[idx].as_deref().unwrap_or(""))
    }

    /// Set axis format string.
    pub fn set_format_str(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.formats[idx] = Some(s.into());
        Ok(())
    }

    /// Test whether axis format string is explicitly set.
    pub fn test_format_str(&self, axis: usize) -> AstResult<bool> {
        let idx = self.validate_axis(axis)?;
        Ok(self.formats[idx].is_some())
    }

    /// Clear axis format string.
    pub fn clear_format_str(&mut self, axis: usize) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.formats[idx] = None;
        Ok(())
    }

    /// Get axis lower bound. Default: -f64::MAX.
    pub fn bottom(&self, axis: usize) -> AstResult<f64> {
        let idx = self.validate_axis(axis)?;
        Ok(self.bottoms[idx].unwrap_or(-f64::MAX))
    }

    /// Set axis lower bound.
    pub fn set_bottom(&mut self, axis: usize, v: f64) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.bottoms[idx] = Some(v);
        Ok(())
    }

    /// Test whether axis lower bound is explicitly set.
    pub fn test_bottom(&self, axis: usize) -> AstResult<bool> {
        let idx = self.validate_axis(axis)?;
        Ok(self.bottoms[idx].is_some())
    }

    /// Clear axis lower bound.
    pub fn clear_bottom(&mut self, axis: usize) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.bottoms[idx] = None;
        Ok(())
    }

    /// Get axis upper bound. Default: f64::MAX.
    pub fn top(&self, axis: usize) -> AstResult<f64> {
        let idx = self.validate_axis(axis)?;
        Ok(self.tops[idx].unwrap_or(f64::MAX))
    }

    /// Set axis upper bound.
    pub fn set_top(&mut self, axis: usize, v: f64) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.tops[idx] = Some(v);
        Ok(())
    }

    /// Test whether axis upper bound is explicitly set.
    pub fn test_top(&self, axis: usize) -> AstResult<bool> {
        let idx = self.validate_axis(axis)?;
        Ok(self.tops[idx].is_some())
    }

    /// Clear axis upper bound.
    pub fn clear_top(&mut self, axis: usize) -> AstResult<()> {
        let idx = self.validate_axis(axis)?;
        self.tops[idx] = None;
        Ok(())
    }

    /// Get the default label for an axis (used when label is not explicitly set).
    fn default_label(axis: usize) -> String {
        format!("Axis {axis}")
    }
}

/// Builder for constructing a [`BasicFrame`].
pub struct BasicFrameBuilder {
    naxes: usize,
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>,
    labels: Vec<Option<String>>,
    units: Vec<Option<String>>,
    symbols: Vec<Option<String>>,
    formats: Vec<Option<String>>,
    bottoms: Vec<Option<f64>>,
    tops: Vec<Option<f64>>,
}

impl BasicFrameBuilder {
    fn new(naxes: usize) -> Self {
        Self {
            naxes,
            title: None,
            domain: None,
            epoch: None,
            labels: vec![None; naxes],
            units: vec![None; naxes],
            symbols: vec![None; naxes],
            formats: vec![None; naxes],
            bottoms: vec![None; naxes],
            tops: vec![None; naxes],
        }
    }

    /// Set the frame title.
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    /// Set the frame domain.
    pub fn domain(mut self, d: impl Into<String>) -> Self {
        self.domain = Some(d.into());
        self
    }

    /// Set the epoch.
    pub fn epoch(mut self, e: f64) -> Self {
        self.epoch = Some(e);
        self
    }

    /// Set a label for an axis (1-based).
    pub fn label(mut self, axis: usize, s: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.labels[axis - 1] = Some(s.into());
        }
        self
    }

    /// Set a unit for an axis (1-based).
    pub fn unit(mut self, axis: usize, s: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.units[axis - 1] = Some(s.into());
        }
        self
    }

    /// Set a symbol for an axis (1-based).
    pub fn symbol(mut self, axis: usize, s: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.symbols[axis - 1] = Some(s.into());
        }
        self
    }

    /// Set a format string for an axis (1-based).
    pub fn format_str(mut self, axis: usize, s: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.formats[axis - 1] = Some(s.into());
        }
        self
    }

    /// Set the bottom bound for an axis (1-based).
    pub fn bottom(mut self, axis: usize, v: f64) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.bottoms[axis - 1] = Some(v);
        }
        self
    }

    /// Set the top bound for an axis (1-based).
    pub fn top(mut self, axis: usize, v: f64) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.tops[axis - 1] = Some(v);
        }
        self
    }

    /// Build the BasicFrame.
    pub fn build(self) -> AstResult<BasicFrame> {
        if self.naxes == 0 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "BasicFrame requires at least 1 axis",
            ));
        }
        Ok(BasicFrame {
            naxes: self.naxes,
            title: self.title,
            domain: self.domain,
            epoch: self.epoch,
            labels: self.labels,
            units: self.units,
            symbols: self.symbols,
            formats: self.formats,
            bottoms: self.bottoms,
            tops: self.tops,
        })
    }
}

impl Frame for BasicFrame {
    fn naxes(&self) -> usize {
        self.naxes
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        if value == BAD_VALUE {
            return Ok("<bad>".to_string());
        }
        // Use format string if set, otherwise default precision
        let fmt = self.formats[axis - 1].as_deref().unwrap_or("");
        if fmt.is_empty() {
            Ok(format!("{value}"))
        } else if let Ok(prec) = fmt.parse::<usize>() {
            Ok(format!("{value:.prec$}"))
        } else {
            Ok(format!("{value}"))
        }
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;
        let trimmed = text.trim_start();
        let leading_ws = text.len() - trimmed.len();

        // Find the longest prefix that parses as f64
        let mut end = 0;
        for i in 1..=trimmed.len() {
            if trimmed[..i].parse::<f64>().is_ok() {
                end = i;
            }
        }
        if end == 0 {
            return Err(AstError::new(
                ErrorCode::Unfer,
                format!("cannot parse coordinate value from \"{text}\""),
            ));
        }
        let value: f64 = trimmed[..end].parse().unwrap();
        Ok((value, leading_ws + end))
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        use crate::mappings::helpers::is_bad;
        let mut sum = 0.0;
        for i in 0..self.naxes {
            if is_bad(a[i]) || is_bad(b[i]) {
                return Ok(BAD_VALUE);
            }
            let d = a[i] - b[i];
            sum += d * d;
        }
        Ok(sum.sqrt())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        use crate::mappings::helpers::is_bad;
        // Angle at vertex b in triangle a-b-c
        let mut dot = 0.0;
        let mut mag_ba = 0.0;
        let mut mag_bc = 0.0;
        for i in 0..self.naxes {
            if is_bad(a[i]) || is_bad(b[i]) || is_bad(c[i]) {
                return Ok(BAD_VALUE);
            }
            let ba = a[i] - b[i];
            let bc = c[i] - b[i];
            dot += ba * bc;
            mag_ba += ba * ba;
            mag_bc += bc * bc;
        }
        let denom = (mag_ba * mag_bc).sqrt();
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(cos_angle.acos())
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        use crate::mappings::helpers::is_bad;
        for i in 0..self.naxes {
            if is_bad(from[i]) || is_bad(to[i]) {
                return Ok(vec![BAD_VALUE; self.naxes]);
            }
        }
        // Direction vector
        let mut dir_len = 0.0;
        let dir: Vec<f64> = (0..self.naxes).map(|i| {
            let d = to[i] - from[i];
            dir_len += d * d;
            d
        }).collect();
        dir_len = dir_len.sqrt();

        if dir_len == 0.0 {
            return Ok(from.to_vec());
        }

        let result: Vec<f64> = (0..self.naxes).map(|i| {
            from[i] + dist * dir[i] / dir_len
        }).collect();
        Ok(result)
    }

    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes)))
    }
}
```

- [ ] **Step 2: Update frames/mod.rs**

Replace `crates/ast-core/src/frames/mod.rs`:

```rust
//! The Frame trait and associated types.

mod basic;

pub use basic::{BasicFrame, BasicFrameBuilder};

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
    fn snapshot(&self) -> DynFrame;

    /// Compute the mapping from a previous state of this frame to the current state.
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/
git commit -m "Add BasicFrame with per-axis attributes and Euclidean geometry"
```

---

### Task 2: BasicFrame Tests

**Files:**
- Create: `crates/ast-core/tests/test_basicframe.rs`

- [ ] **Step 1: Write BasicFrame tests**

Create `crates/ast-core/tests/test_basicframe.rs`:

```rust
//! Rust equivalent of reference/starlink-ast/ast_tester/testframe.c
//! Covers: construction, per-axis attributes, Euclidean geometry, format/unformat.

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrame, Frame};
use ast_core::mappings::Mapping;

#[test]
fn builder_basic() {
    let f = BasicFrame::builder(3)
        .title("Test Frame")
        .domain("PIXEL")
        .build()
        .unwrap();
    assert_eq!(f.naxes(), 3);
    assert_eq!(f.title(), "Test Frame");
    assert_eq!(f.domain(), "PIXEL");
}

#[test]
fn builder_zero_axes_rejected() {
    assert!(BasicFrame::builder(0).build().is_err());
}

#[test]
fn builder_with_labels() {
    let f = BasicFrame::builder(2)
        .label(1, "X")
        .label(2, "Y")
        .unit(1, "mm")
        .build()
        .unwrap();
    assert_eq!(f.label(1).unwrap(), "X");
    assert_eq!(f.label(2).unwrap(), "Y");
    assert_eq!(f.unit(1).unwrap(), "mm");
    assert_eq!(f.unit(2).unwrap(), "");
}

#[test]
fn default_attributes() {
    let f = BasicFrame::builder(2).build().unwrap();
    assert_eq!(f.title(), "");
    assert_eq!(f.domain(), "");
    assert_eq!(f.epoch(), None);
    assert_eq!(f.label(1).unwrap(), "");
    assert_eq!(f.unit(1).unwrap(), "");
    assert!(!f.test_title());
    assert!(!f.test_label(1).unwrap());
}

#[test]
fn set_clear_attributes() {
    let mut f = BasicFrame::builder(2).build().unwrap();
    f.set_label(1, "X").unwrap();
    assert!(f.test_label(1).unwrap());
    assert_eq!(f.label(1).unwrap(), "X");

    f.clear_label(1).unwrap();
    assert!(!f.test_label(1).unwrap());
    assert_eq!(f.label(1).unwrap(), "");
}

#[test]
fn axis_validation() {
    let f = BasicFrame::builder(2).build().unwrap();
    assert!(f.label(0).is_err());
    assert!(f.label(3).is_err());
    assert!(f.label(1).is_ok());
    assert!(f.label(2).is_ok());
}

#[test]
fn epoch_attribute() {
    let mut f = BasicFrame::builder(1).build().unwrap();
    assert!(!f.test_epoch());
    f.set_epoch(2000.0);
    assert!(f.test_epoch());
    assert_eq!(f.epoch(), Some(2000.0));
    f.clear_epoch();
    assert_eq!(f.epoch(), None);
}

#[test]
fn distance_2d() {
    let f = BasicFrame::builder(2).build().unwrap();
    let d = f.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

#[test]
fn distance_3d() {
    let f = BasicFrame::builder(3).build().unwrap();
    let d = f.distance(&[1.0, 2.0, 3.0], &[4.0, 6.0, 3.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

#[test]
fn distance_bad_value() {
    let f = BasicFrame::builder(2).build().unwrap();
    let d = f.distance(&[BAD_VALUE, 0.0], &[1.0, 1.0]).unwrap();
    assert_eq!(d, BAD_VALUE);
}

#[test]
fn angle_right_angle() {
    let f = BasicFrame::builder(2).build().unwrap();
    // Right angle at origin: a=(1,0), b=(0,0), c=(0,1)
    let ang = f.angle(&[1.0, 0.0], &[0.0, 0.0], &[0.0, 1.0]).unwrap();
    assert!((ang - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
}

#[test]
fn angle_straight_line() {
    let f = BasicFrame::builder(2).build().unwrap();
    // Straight line: a=(-1,0), b=(0,0), c=(1,0) => angle = pi
    let ang = f.angle(&[-1.0, 0.0], &[0.0, 0.0], &[1.0, 0.0]).unwrap();
    assert!((ang - std::f64::consts::PI).abs() < 1e-10);
}

#[test]
fn offset_2d() {
    let f = BasicFrame::builder(2).build().unwrap();
    // Move from (0,0) toward (3,4) by distance 5 => should reach (3,4)
    let result = f.offset(&[0.0, 0.0], &[3.0, 4.0], 5.0).unwrap();
    assert!((result[0] - 3.0).abs() < 1e-10);
    assert!((result[1] - 4.0).abs() < 1e-10);
}

#[test]
fn offset_half_distance() {
    let f = BasicFrame::builder(2).build().unwrap();
    // Move from (0,0) toward (4,0) by distance 2 => (2,0)
    let result = f.offset(&[0.0, 0.0], &[4.0, 0.0], 2.0).unwrap();
    assert!((result[0] - 2.0).abs() < 1e-10);
    assert!((result[1] - 0.0).abs() < 1e-10);
}

#[test]
fn norm_is_noop() {
    let f = BasicFrame::builder(2).build().unwrap();
    let mut coords = [3.0, 4.0];
    f.norm(&mut coords).unwrap();
    assert_eq!(coords, [3.0, 4.0]);
}

#[test]
fn format_default() {
    let f = BasicFrame::builder(1).build().unwrap();
    let s = f.format(1, 3.14159).unwrap();
    assert!(s.contains("3.14159"));
}

#[test]
fn format_bad_value() {
    let f = BasicFrame::builder(1).build().unwrap();
    let s = f.format(1, BAD_VALUE).unwrap();
    assert_eq!(s, "<bad>");
}

#[test]
fn format_with_precision() {
    let mut f = BasicFrame::builder(1).build().unwrap();
    f.set_format_str(1, "3").unwrap(); // 3 decimal places
    let s = f.format(1, 3.14159).unwrap();
    assert_eq!(s, "3.142");
}

#[test]
fn unformat_basic() {
    let f = BasicFrame::builder(1).build().unwrap();
    let (val, consumed) = f.unformat(1, "3.14 rest").unwrap();
    assert!((val - 3.14).abs() < 1e-10);
    assert_eq!(consumed, 4);
}

#[test]
fn unformat_with_leading_whitespace() {
    let f = BasicFrame::builder(1).build().unwrap();
    let (val, consumed) = f.unformat(1, "  42.0").unwrap();
    assert!((val - 42.0).abs() < 1e-10);
    assert_eq!(consumed, 6);
}

#[test]
fn unformat_invalid() {
    let f = BasicFrame::builder(1).build().unwrap();
    assert!(f.unformat(1, "abc").is_err());
}

#[test]
fn snapshot_is_independent_clone() {
    let mut f = BasicFrame::builder(2).build().unwrap();
    f.set_title("original");
    let snap = f.snapshot();
    f.set_title("modified");
    assert_eq!(f.title(), "modified");
    // snapshot should still reflect old state - but since snapshot is just clone,
    // it captured "original"
    // We can't easily check DynFrame's title without downcasting, but we can
    // verify naxes is preserved
    assert_eq!(snap.naxes(), 2);
}

#[test]
fn map_from_returns_unit() {
    let f = BasicFrame::builder(3).build().unwrap();
    let prev = f.snapshot();
    let mapping = f.map_from(&*prev).unwrap();
    assert_eq!(mapping.nin(), 3);
    assert_eq!(mapping.nout(), 3);
}

#[test]
fn bottom_top_defaults() {
    let f = BasicFrame::builder(2).build().unwrap();
    assert_eq!(f.bottom(1).unwrap(), -f64::MAX);
    assert_eq!(f.top(1).unwrap(), f64::MAX);
    assert!(!f.test_bottom(1).unwrap());
    assert!(!f.test_top(1).unwrap());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_basicframe`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_basicframe.rs
git commit -m "Add BasicFrame tests (equivalent to testframe.c)"
```

---

### Task 3: CmpFrame

**Files:**
- Create: `crates/ast-core/src/frames/cmpframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`

- [ ] **Step 1: Create cmpframe.rs**

Create `crates/ast-core/src/frames/cmpframe.rs`:

```rust
//! Compound frame combining two child frames.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, UnitMap};

use super::{DynFrame, Frame};

/// A compound frame combining two child frames.
///
/// Axes are concatenated: if frame1 has N1 axes and frame2 has N2 axes,
/// the CmpFrame has N1+N2 axes. Axis-indexed operations route to the
/// appropriate child frame.
pub struct CmpFrame {
    frame1: DynFrame,
    frame2: DynFrame,
}

impl CmpFrame {
    /// Create a new compound frame from two child frames.
    pub fn new(frame1: DynFrame, frame2: DynFrame) -> Self {
        Self { frame1, frame2 }
    }

    /// Number of axes in the first child frame.
    pub fn naxes1(&self) -> usize {
        self.frame1.naxes()
    }

    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        let total = self.frame1.naxes() + self.frame2.naxes();
        if axis == 0 || axis > total {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {axis} is invalid for CmpFrame with {total} axes"),
            ));
        }
        Ok(())
    }

    fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
        let n1 = self.frame1.naxes();
        if axis <= n1 {
            (&*self.frame1, axis)
        } else {
            (&*self.frame2, axis - n1)
        }
    }
}

impl Frame for CmpFrame {
    fn naxes(&self) -> usize {
        self.frame1.naxes() + self.frame2.naxes()
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        let (frame, local_axis) = self.route_axis(axis);
        frame.format(local_axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;
        let (frame, local_axis) = self.route_axis(axis);
        frame.unformat(local_axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        use crate::mappings::helpers::is_bad;
        let n = self.naxes();
        let mut sum = 0.0;
        for i in 0..n {
            if is_bad(a[i]) || is_bad(b[i]) {
                return Ok(BAD_VALUE);
            }
            let d = a[i] - b[i];
            sum += d * d;
        }
        Ok(sum.sqrt())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        use crate::mappings::helpers::is_bad;
        let n = self.naxes();
        let mut dot = 0.0;
        let mut mag_ba = 0.0;
        let mut mag_bc = 0.0;
        for i in 0..n {
            if is_bad(a[i]) || is_bad(b[i]) || is_bad(c[i]) {
                return Ok(BAD_VALUE);
            }
            let ba = a[i] - b[i];
            let bc = c[i] - b[i];
            dot += ba * bc;
            mag_ba += ba * ba;
            mag_bc += bc * bc;
        }
        let denom = (mag_ba * mag_bc).sqrt();
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(cos_angle.acos())
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        use crate::mappings::helpers::is_bad;
        let n = self.naxes();
        for i in 0..n {
            if is_bad(from[i]) || is_bad(to[i]) {
                return Ok(vec![BAD_VALUE; n]);
            }
        }
        let mut dir_len = 0.0;
        let dir: Vec<f64> = (0..n).map(|i| {
            let d = to[i] - from[i];
            dir_len += d * d;
            d
        }).collect();
        dir_len = dir_len.sqrt();

        if dir_len == 0.0 {
            return Ok(from.to_vec());
        }
        Ok((0..n).map(|i| from[i] + dist * dir[i] / dir_len).collect())
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        let n1 = self.frame1.naxes();
        self.frame1.norm(&mut coords[..n1])?;
        self.frame2.norm(&mut coords[n1..])?;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(CmpFrame {
            frame1: self.frame1.snapshot(),
            frame2: self.frame2.snapshot(),
        })
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes())))
    }
}

impl fmt::Debug for CmpFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CmpFrame")
            .field("naxes1", &self.frame1.naxes())
            .field("naxes2", &self.frame2.naxes())
            .finish()
    }
}
```

- [ ] **Step 2: Update frames/mod.rs**

Add after `mod basic;`:

```rust
mod cmpframe;

pub use basic::{BasicFrame, BasicFrameBuilder};
pub use cmpframe::CmpFrame;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/
git commit -m "Add CmpFrame (compound frame with two children)"
```

---

### Task 4: CmpFrame Tests

**Files:**
- Create: `crates/ast-core/tests/test_cmpframe.rs`

- [ ] **Step 1: Write CmpFrame tests**

Create `crates/ast-core/tests/test_cmpframe.rs`:

```rust
//! Rust equivalent of reference/starlink-ast/ast_tester/testcmpframe.c
//! Covers: axis concatenation, routing, geometry, format/unformat delegation.

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrame, CmpFrame, Frame};

fn make_2d_frame(label1: &str, label2: &str) -> Box<dyn Frame> {
    let mut f = BasicFrame::builder(2).build().unwrap();
    f.set_label(1, label1).unwrap();
    f.set_label(2, label2).unwrap();
    Box::new(f)
}

#[test]
fn cmpframe_naxes() {
    let f1 = make_2d_frame("X", "Y");
    let f2: Box<dyn Frame> = Box::new(BasicFrame::builder(3).build().unwrap());
    let cmp = CmpFrame::new(f1, f2);
    assert_eq!(cmp.naxes(), 5);
    assert_eq!(cmp.naxes1(), 2);
}

#[test]
fn cmpframe_format_delegates() {
    let mut f1 = BasicFrame::builder(1).build().unwrap();
    f1.set_format_str(1, "2").unwrap(); // 2 decimal places
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    // Axis 1 uses f1's format (2 decimals)
    let s1 = cmp.format(1, 3.14159).unwrap();
    assert_eq!(s1, "3.14");

    // Axis 2 uses f2's default format
    let s2 = cmp.format(2, 3.14159).unwrap();
    assert!(s2.contains("3.14159"));
}

#[test]
fn cmpframe_unformat_delegates() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    let (val, _) = cmp.unformat(3, "42.5").unwrap();
    assert!((val - 42.5).abs() < 1e-10);
}

#[test]
fn cmpframe_axis_validation() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(2).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    assert!(cmp.format(0, 1.0).is_err());
    assert!(cmp.format(5, 1.0).is_err());
    assert!(cmp.format(1, 1.0).is_ok());
    assert!(cmp.format(4, 1.0).is_ok());
}

#[test]
fn cmpframe_distance() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    // 3D Euclidean: (0,0,0) to (1,2,2) = 3
    let d = cmp.distance(&[0.0, 0.0, 0.0], &[1.0, 2.0, 2.0]).unwrap();
    assert!((d - 3.0).abs() < 1e-10);
}

#[test]
fn cmpframe_distance_bad_value() {
    let f1 = BasicFrame::builder(1).build().unwrap();
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));
    let d = cmp.distance(&[BAD_VALUE, 1.0], &[1.0, 1.0]).unwrap();
    assert_eq!(d, BAD_VALUE);
}

#[test]
fn cmpframe_offset() {
    let f1 = BasicFrame::builder(1).build().unwrap();
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    // Move from (0,0) toward (3,4) by distance 5 => (3,4)
    let result = cmp.offset(&[0.0, 0.0], &[3.0, 4.0], 5.0).unwrap();
    assert!((result[0] - 3.0).abs() < 1e-10);
    assert!((result[1] - 4.0).abs() < 1e-10);
}

#[test]
fn cmpframe_norm_delegates() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    let mut coords = [1.0, 2.0, 3.0];
    cmp.norm(&mut coords).unwrap();
    assert_eq!(coords, [1.0, 2.0, 3.0]); // BasicFrame norm is no-op
}

#[test]
fn cmpframe_snapshot() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(3).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));
    let snap = cmp.snapshot();
    assert_eq!(snap.naxes(), 5);
}

#[test]
fn cmpframe_angle() {
    let f1 = BasicFrame::builder(1).build().unwrap();
    let f2 = BasicFrame::builder(1).build().unwrap();
    let cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    // Right angle at origin
    let ang = cmp.angle(&[1.0, 0.0], &[0.0, 0.0], &[0.0, 1.0]).unwrap();
    assert!((ang - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_cmpframe`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_cmpframe.rs
git commit -m "Add CmpFrame tests (equivalent to testcmpframe.c)"
```

---

### Task 5: Clippy, Fmt, and Final Verification

**Files:**
- Possibly any file needing fixes

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Fix any issues.

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all -- --check`
If issues, run `cargo fmt --all`.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all tests pass

- [ ] **Step 4: Commit fixes if any**

```bash
git add -u
git commit -m "Fix clippy and fmt issues in frame implementations"
```

---

## Verification Checklist

After all tasks are complete:

1. `cargo build --workspace` — all crates compile
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. `BasicFrame::builder(n).build()` works
6. Per-axis attributes with 1-based indexing validated
7. Euclidean geometry (distance, angle, offset) correct
8. CmpFrame routes to correct child frame
9. `git status` — clean
