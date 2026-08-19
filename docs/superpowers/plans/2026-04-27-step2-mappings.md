# Step 2: Simple Mappings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the six simplest Mapping classes (UnitMap, ZoomMap, ShiftMap, MatrixMap, PermMap, CmpMap) with transforms, inversion, basic simplification, numeric rate, and composition verbs.

**Architecture:** Each mapping is a struct in its own file under `src/mappings/`. A shared `rate.rs` provides numeric differentiation. All mappings handle BAD_VALUE consistently. Composition via `then()`/`alongside()` wraps `CmpMap::new()`.

**Tech Stack:** Rust 2024 edition, ndarray 0.16 (for MatrixMap and PointSet)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/mappings/mod.rs` | Trait + re-exports + `Box<dyn Mapping>` composition methods |
| `crates/ast-core/src/mappings/helpers.rs` | `is_bad()` helper, BAD_VALUE utilities |
| `crates/ast-core/src/mappings/rate.rs` | `numeric_rate()` shared implementation |
| `crates/ast-core/src/mappings/unit.rs` | UnitMap |
| `crates/ast-core/src/mappings/zoom.rs` | ZoomMap |
| `crates/ast-core/src/mappings/shift.rs` | ShiftMap |
| `crates/ast-core/src/mappings/matrix.rs` | MatrixMap |
| `crates/ast-core/src/mappings/perm.rs` | PermMap |
| `crates/ast-core/src/mappings/cmpmap.rs` | CmpMap + Composition enum |
| `crates/ast-core/tests/test_unitmap.rs` | UnitMap tests |
| `crates/ast-core/tests/test_zoommap.rs` | ZoomMap tests |
| `crates/ast-core/tests/test_shiftmap.rs` | ShiftMap tests |
| `crates/ast-core/tests/test_matrixmap.rs` | MatrixMap tests |
| `crates/ast-core/tests/test_permmap.rs` | PermMap tests |
| `crates/ast-core/tests/test_cmpmap.rs` | CmpMap + composition tests |

---

### Task 1: Shared Helpers and Rate Implementation

**Files:**
- Create: `crates/ast-core/src/mappings/helpers.rs`
- Create: `crates/ast-core/src/mappings/rate.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`

- [ ] **Step 1: Create helpers module**

Create `crates/ast-core/src/mappings/helpers.rs`:

```rust
//! Shared utilities for Mapping implementations.

use crate::constants::BAD_VALUE;

/// Check whether a coordinate value is "bad" (undefined).
///
/// A value is bad if it equals `BAD_VALUE` or is NaN.
#[inline]
pub fn is_bad(v: f64) -> bool {
    v == BAD_VALUE || v.is_nan()
}

/// Check whether any coordinate in a point is bad.
#[inline]
pub fn point_has_bad(point: &[f64]) -> bool {
    point.iter().any(|&v| is_bad(v))
}

/// Fill a slice with BAD_VALUE.
#[inline]
pub fn fill_bad(out: &mut [f64]) {
    out.fill(BAD_VALUE);
}
```

- [ ] **Step 2: Create rate module**

Create `crates/ast-core/src/mappings/rate.rs`:

```rust
//! Numeric rate-of-change computation via central finite differences.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;

use super::helpers::is_bad;
use super::Mapping;

/// Compute the rate of change of output axis `ax1` with respect to input axis `ax2`
/// at position `at`, using central finite differences.
///
/// `ax1` and `ax2` are 1-based axis indices.
pub fn numeric_rate(
    mapping: &dyn Mapping,
    at: &[f64],
    ax1: usize,
    ax2: usize,
) -> AstResult<f64> {
    let ax2_idx = ax2 - 1; // convert to 0-based for array indexing

    // Adaptive step size
    let h = {
        let base = at[ax2_idx].abs();
        if base > 1e-10 {
            base * 1e-6
        } else {
            1e-10
        }
    };

    // Evaluate at (at + h*e_ax2) and (at - h*e_ax2)
    let mut at_plus = at.to_vec();
    let mut at_minus = at.to_vec();
    at_plus[ax2_idx] += h;
    at_minus[ax2_idx] -= h;

    let pts_plus = PointSet::from_rows([at_plus.as_slice()]);
    let pts_minus = PointSet::from_rows([at_minus.as_slice()]);

    let out_plus = mapping.transform(&pts_plus, true)?;
    let out_minus = mapping.transform(&pts_minus, true)?;

    let val_plus = out_plus.point(0)[ax1 - 1];
    let val_minus = out_minus.point(0)[ax1 - 1];

    if is_bad(val_plus) || is_bad(val_minus) {
        Ok(BAD_VALUE)
    } else {
        Ok((val_plus - val_minus) / (2.0 * h))
    }
}
```

- [ ] **Step 3: Update mod.rs to declare submodules**

Replace `crates/ast-core/src/mappings/mod.rs` with:

```rust
//! The Mapping trait, concrete mapping classes, and composition.

mod cmpmap;
pub(crate) mod helpers;
mod matrix;
mod perm;
pub(crate) mod rate;
mod shift;
mod unit;
mod zoom;

pub use cmpmap::{CmpMap, Composition};
pub use matrix::MatrixMap;
pub use perm::PermMap;
pub use shift::ShiftMap;
pub use unit::UnitMap;
pub use zoom::ZoomMap;

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

impl dyn Mapping {
    /// Compose this mapping in series with `next`: apply self first, then next.
    pub fn then(self: Box<Self>, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(self, next, Composition::Series)
    }

    /// Compose this mapping in parallel with `other`: apply side by side.
    pub fn alongside(self: Box<Self>, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(self, other, Composition::Parallel)
    }
}
```

Note: The submodule files (unit.rs, zoom.rs, etc.) don't exist yet —
this won't compile until the following tasks create them. Create empty
placeholder files for each so the build doesn't fail:

```bash
touch crates/ast-core/src/mappings/unit.rs
touch crates/ast-core/src/mappings/zoom.rs
touch crates/ast-core/src/mappings/shift.rs
touch crates/ast-core/src/mappings/matrix.rs
touch crates/ast-core/src/mappings/perm.rs
touch crates/ast-core/src/mappings/cmpmap.rs
```

These placeholders should be empty files. The build will succeed because
`pub use` of non-existent items from empty modules will fail — so instead,
comment out the `pub use` lines for now and the `mod` declarations for
files that don't have content yet. Actually, the simplest approach: only
declare `mod helpers;` and `mod rate;` in this task. Add the other `mod`
declarations as each mapping is implemented.

Revised `mod.rs` for this task only:

```rust
//! The Mapping trait, concrete mapping classes, and composition.

pub(crate) mod helpers;
pub(crate) mod rate;

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
    /// `ax1` is the 1-based index of the output axis.
    /// `ax2` is the 1-based index of the input axis.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64>;
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/
git commit -m "Add mapping helpers (is_bad, BAD_VALUE utils) and numeric rate()"
```

---

### Task 2: UnitMap

**Files:**
- Create: `crates/ast-core/src/mappings/unit.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Create: `crates/ast-core/tests/test_unitmap.rs`

- [ ] **Step 1: Write UnitMap tests**

Create `crates/ast-core/tests/test_unitmap.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, UnitMap};

#[test]
fn unit_map_dimensions() {
    let m = UnitMap::new(3);
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
    assert!(!m.is_inverted());
}

#[test]
fn unit_map_forward_transform() {
    let m = UnitMap::new(2);
    let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
    assert_eq!(out.point(1), &[3.0, 4.0]);
}

#[test]
fn unit_map_inverse_transform() {
    let m = UnitMap::new(2);
    let pts = PointSet::from_rows([[5.0, 6.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[5.0, 6.0]);
}

#[test]
fn unit_map_bad_value_passthrough() {
    let m = UnitMap::new(2);
    let pts = PointSet::from_rows([[BAD_VALUE, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[BAD_VALUE, 2.0]);
}

#[test]
fn unit_map_invert_is_noop() {
    let mut m = UnitMap::new(2);
    m.invert();
    assert!(m.is_inverted());
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn unit_map_simplified_returns_self() {
    let m = UnitMap::new(2);
    let s = m.simplified().unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

#[test]
fn unit_map_rate() {
    let m = UnitMap::new(2);
    // Rate of output axis 1 w.r.t. input axis 1 should be 1.0
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 1.0).abs() < 1e-6);
    // Rate of output axis 1 w.r.t. input axis 2 should be 0.0
    let r = m.rate(&[1.0, 1.0], 1, 2).unwrap();
    assert!(r.abs() < 1e-6);
}
```

- [ ] **Step 2: Implement UnitMap**

Create `crates/ast-core/src/mappings/unit.rs`:

```rust
//! The identity mapping.

use crate::coords::PointSet;
use crate::error::AstResult;

use super::rate::numeric_rate;
use super::{DynMapping, Mapping};

/// An identity mapping that passes coordinates through unchanged.
#[derive(Debug, Clone)]
pub struct UnitMap {
    ncoord: usize,
    inverted: bool,
}

impl UnitMap {
    /// Create a new UnitMap with the given number of coordinates.
    pub fn new(ncoord: usize) -> Self {
        Self {
            ncoord,
            inverted: false,
        }
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<super::CmpMap> {
        super::CmpMap::new(Box::new(self), next, super::Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<super::CmpMap> {
        super::CmpMap::new(Box::new(self), other, super::Composition::Parallel)
    }
}

impl Mapping for UnitMap {
    fn nin(&self) -> usize {
        self.ncoord
    }

    fn nout(&self) -> usize {
        self.ncoord
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, _forward: bool) -> AstResult<PointSet> {
        Ok(pts.clone())
    }

    fn transform_into(
        &self,
        pts: &PointSet,
        _forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        out.view_mut().assign(&pts.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}
```

- [ ] **Step 3: Update mod.rs**

Add to `crates/ast-core/src/mappings/mod.rs` after `pub(crate) mod rate;`:

```rust
mod unit;
pub use unit::UnitMap;
```

Note: `UnitMap::then()` and `alongside()` reference `CmpMap` which doesn't
exist yet. To avoid compilation failure, we have two options:
1. Gate the composition methods behind a `cfg` or
2. Create a minimal CmpMap stub first

The cleanest approach: skip the `then()`/`alongside()` methods on UnitMap
for now. Add them in Task 7 (CmpMap) when CmpMap exists. Remove those
methods from the UnitMap implementation above for this task.

Revised UnitMap — remove the `then` and `alongside` methods. They'll be
added in Task 7 after CmpMap exists.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_unitmap`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/unit.rs crates/ast-core/src/mappings/mod.rs crates/ast-core/tests/test_unitmap.rs
git commit -m "Add UnitMap (identity mapping)"
```

---

### Task 3: ZoomMap

**Files:**
- Create: `crates/ast-core/src/mappings/zoom.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Create: `crates/ast-core/tests/test_zoommap.rs`

- [ ] **Step 1: Write ZoomMap tests**

Create `crates/ast-core/tests/test_zoommap.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, UnitMap, ZoomMap};

#[test]
fn zoom_map_dimensions() {
    let m = ZoomMap::new(3, 2.5).unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
}

#[test]
fn zoom_map_zero_factor_rejected() {
    assert!(ZoomMap::new(2, 0.0).is_err());
}

#[test]
fn zoom_map_forward() {
    let m = ZoomMap::new(2, 3.0).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0], [4.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 6.0]);
    assert_eq!(out.point(1), &[12.0, 15.0]);
}

#[test]
fn zoom_map_inverse() {
    let m = ZoomMap::new(2, 4.0).unwrap();
    let pts = PointSet::from_rows([[8.0, 12.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[2.0, 3.0]);
}

#[test]
fn zoom_map_bad_value() {
    let m = ZoomMap::new(2, 2.0).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn zoom_map_inverted_flag() {
    let mut m = ZoomMap::new(2, 2.0).unwrap();
    m.invert();
    // With inverted flag, forward=true should divide
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[2.0, 3.0]);
}

#[test]
fn zoom_map_simplified_identity() {
    let m = ZoomMap::new(2, 1.0).unwrap();
    let s = m.simplified().unwrap();
    assert_eq!(s.nin(), 2);
    // Check it behaves like identity
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

#[test]
fn zoom_map_rate() {
    let m = ZoomMap::new(2, 3.0).unwrap();
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 3.0).abs() < 1e-4);
}
```

- [ ] **Step 2: Implement ZoomMap**

Create `crates/ast-core/src/mappings/zoom.rs`:

```rust
//! Uniform scaling mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that multiplies all coordinates by a constant factor.
#[derive(Debug, Clone)]
pub struct ZoomMap {
    ncoord: usize,
    factor: f64,
    inverted: bool,
}

impl ZoomMap {
    /// Create a new ZoomMap.
    ///
    /// Fails if `factor` is zero (non-invertible).
    pub fn new(ncoord: usize, factor: f64) -> AstResult<Self> {
        if factor == 0.0 {
            return Err(AstError::new(
                ErrorCode::Zoomi,
                "zoom factor must not be zero",
            ));
        }
        Ok(Self {
            ncoord,
            factor,
            inverted: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }
}

impl Mapping for ZoomMap {
    fn nin(&self) -> usize {
        self.ncoord
    }

    fn nout(&self) -> usize {
        self.ncoord
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), self.ncoord);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.factor } else { 1.0 / self.factor };

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for (o, &v) in outp.iter_mut().zip(inp.iter()) {
                    *o = v * scale;
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.factor == 1.0 {
            Ok(Box::new(UnitMap::new(self.ncoord)))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}
```

- [ ] **Step 3: Update mod.rs**

Add to `crates/ast-core/src/mappings/mod.rs`:
```rust
mod zoom;
pub use zoom::ZoomMap;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_zoommap`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/zoom.rs crates/ast-core/src/mappings/mod.rs crates/ast-core/tests/test_zoommap.rs
git commit -m "Add ZoomMap (uniform scaling mapping)"
```

---

### Task 4: ShiftMap

**Files:**
- Create: `crates/ast-core/src/mappings/shift.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Create: `crates/ast-core/tests/test_shiftmap.rs`

- [ ] **Step 1: Write ShiftMap tests**

Create `crates/ast-core/tests/test_shiftmap.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ShiftMap};

#[test]
fn shift_map_dimensions() {
    let m = ShiftMap::new([1.0, 2.0, 3.0]);
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
}

#[test]
fn shift_map_forward() {
    let m = ShiftMap::new([10.0, 20.0]);
    let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[11.0, 22.0]);
    assert_eq!(out.point(1), &[13.0, 24.0]);
}

#[test]
fn shift_map_inverse() {
    let m = ShiftMap::new([10.0, 20.0]);
    let pts = PointSet::from_rows([[11.0, 22.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn shift_map_bad_value() {
    let m = ShiftMap::new([1.0, 2.0]);
    let pts = PointSet::from_rows([[f64::NAN, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn shift_map_simplified_zero() {
    let m = ShiftMap::new([0.0, 0.0]);
    let s = m.simplified().unwrap();
    let pts = PointSet::from_rows([[5.0, 6.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[5.0, 6.0]);
}

#[test]
fn shift_map_rate() {
    let m = ShiftMap::new([10.0, 20.0]);
    // Shift is constant offset, so rate of any output w.r.t. matching input is 1.0
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 1.0).abs() < 1e-6);
}
```

- [ ] **Step 2: Implement ShiftMap**

Create `crates/ast-core/src/mappings/shift.rs`:

```rust
//! Constant offset mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;

use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that adds a constant offset to each coordinate.
#[derive(Debug, Clone)]
pub struct ShiftMap {
    shift: Vec<f64>,
    inverted: bool,
}

impl ShiftMap {
    /// Create a new ShiftMap with the given shift vector.
    pub fn new(shift: impl Into<Vec<f64>>) -> Self {
        Self {
            shift: shift.into(),
            inverted: false,
        }
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }
}

impl Mapping for ShiftMap {
    fn nin(&self) -> usize {
        self.shift.len()
    }

    fn nout(&self) -> usize {
        self.shift.len()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), self.shift.len());
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let sign = if fwd { 1.0 } else { -1.0 };

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    *o = v + sign * self.shift[j];
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.shift.iter().all(|&s| s == 0.0) {
            Ok(Box::new(UnitMap::new(self.shift.len())))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}
```

- [ ] **Step 3: Update mod.rs**

Add:
```rust
mod shift;
pub use shift::ShiftMap;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_shiftmap`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/shift.rs crates/ast-core/src/mappings/mod.rs crates/ast-core/tests/test_shiftmap.rs
git commit -m "Add ShiftMap (constant offset mapping)"
```

---

### Task 5: MatrixMap

**Files:**
- Create: `crates/ast-core/src/mappings/matrix.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Create: `crates/ast-core/tests/test_matrixmap.rs`

- [ ] **Step 1: Write MatrixMap tests**

Create `crates/ast-core/tests/test_matrixmap.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, MatrixMap};
use ndarray::array;

#[test]
fn matrix_map_dimensions() {
    // 2x3 matrix: 3 inputs, 2 outputs
    let m = MatrixMap::new(array![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 2);
}

#[test]
fn matrix_map_identity_2x2() {
    let m = MatrixMap::new(array![[1.0, 0.0], [0.0, 1.0]]).unwrap();
    let pts = PointSet::from_rows([[3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 4.0]);
}

#[test]
fn matrix_map_rotation_90() {
    // 90-degree rotation: [[0, -1], [1, 0]]
    let m = MatrixMap::new(array![[0.0, -1.0], [1.0, 0.0]]).unwrap();
    let pts = PointSet::from_rows([[1.0, 0.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 0.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 1.0).abs() < 1e-10);
}

#[test]
fn matrix_map_inverse() {
    // Scale by 2
    let m = MatrixMap::new(array![[2.0, 0.0], [0.0, 2.0]]).unwrap();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-10);
}

#[test]
fn matrix_map_non_square_no_inverse() {
    // 2x3: can't invert
    let m = MatrixMap::new(array![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    assert!(m.transform(&pts, false).is_err());
}

#[test]
fn matrix_map_bad_value() {
    let m = MatrixMap::new(array![[2.0, 0.0], [0.0, 2.0]]).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn matrix_map_simplified_identity() {
    let m = MatrixMap::new(array![[1.0, 0.0], [0.0, 1.0]]).unwrap();
    let s = m.simplified().unwrap();
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}
```

- [ ] **Step 2: Implement MatrixMap**

Create `crates/ast-core/src/mappings/matrix.rs`:

```rust
//! Linear matrix transformation mapping.

use ndarray::Array2;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that applies a linear matrix transformation.
///
/// Forward transform: `output = matrix × input` (matrix-vector product).
/// The matrix has shape `(nout, nin)`.
#[derive(Debug, Clone)]
pub struct MatrixMap {
    matrix: Array2<f64>,
    inverse: Option<Array2<f64>>,
    inverted: bool,
}

impl MatrixMap {
    /// Create a new MatrixMap from a transformation matrix.
    ///
    /// The matrix shape `(nrow, ncol)` defines `nout = nrow`, `nin = ncol`.
    /// If the matrix is square, the inverse is computed. If singular or
    /// non-square, inverse transforms will return an error.
    pub fn new(matrix: Array2<f64>) -> AstResult<Self> {
        let inverse = if matrix.nrows() == matrix.ncols() {
            invert_matrix(&matrix)
        } else {
            None
        };
        Ok(Self {
            matrix,
            inverse,
            inverted: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    fn forward_matrix(&self) -> &Array2<f64> {
        &self.matrix
    }

    fn inverse_matrix(&self) -> AstResult<&Array2<f64>> {
        self.inverse.as_ref().ok_or_else(|| {
            AstError::new(ErrorCode::Nodef, "MatrixMap inverse is not defined")
        })
    }
}

impl Mapping for MatrixMap {
    fn nin(&self) -> usize {
        self.matrix.ncols()
    }

    fn nout(&self) -> usize {
        self.matrix.nrows()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        let mat = if fwd {
            self.forward_matrix()
        } else {
            self.inverse_matrix()?
        };
        let nout = mat.nrows();
        let mut out = PointSet::zeros(pts.npoint(), nout);
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for r in 0..nout {
                    let mut sum = 0.0;
                    for c in 0..inp.len() {
                        sum += mat[[r, c]] * inp[c];
                    }
                    outp[r] = sum;
                }
            }
        }
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if is_identity(&self.matrix) {
            Ok(Box::new(UnitMap::new(self.matrix.nrows())))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}

/// Check if a matrix is the identity.
fn is_identity(m: &Array2<f64>) -> bool {
    if m.nrows() != m.ncols() {
        return false;
    }
    let n = m.nrows();
    for r in 0..n {
        for c in 0..n {
            let expected = if r == c { 1.0 } else { 0.0 };
            if (m[[r, c]] - expected).abs() > 1e-15 {
                return false;
            }
        }
    }
    true
}

/// Invert a square matrix using Gauss-Jordan elimination.
/// Returns None if the matrix is singular.
fn invert_matrix(m: &Array2<f64>) -> Option<Array2<f64>> {
    let n = m.nrows();
    assert_eq!(n, m.ncols());

    // Augmented matrix [m | I]
    let mut aug = Array2::zeros((n, 2 * n));
    for r in 0..n {
        for c in 0..n {
            aug[[r, c]] = m[[r, c]];
        }
        aug[[r, n + r]] = 1.0;
    }

    // Forward elimination with partial pivoting
    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = aug[[col, col]].abs();
        for row in (col + 1)..n {
            let val = aug[[row, col]].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_val < 1e-15 {
            return None; // singular
        }

        // Swap rows
        if max_row != col {
            for c in 0..(2 * n) {
                let tmp = aug[[col, c]];
                aug[[col, c]] = aug[[max_row, c]];
                aug[[max_row, c]] = tmp;
            }
        }

        // Eliminate column
        let pivot = aug[[col, col]];
        for c in 0..(2 * n) {
            aug[[col, c]] /= pivot;
        }
        for row in 0..n {
            if row != col {
                let factor = aug[[row, col]];
                for c in 0..(2 * n) {
                    aug[[row, c]] -= factor * aug[[col, c]];
                }
            }
        }
    }

    // Extract inverse from right half
    let mut inv = Array2::zeros((n, n));
    for r in 0..n {
        for c in 0..n {
            inv[[r, c]] = aug[[r, n + c]];
        }
    }
    Some(inv)
}
```

- [ ] **Step 3: Update mod.rs**

Add:
```rust
mod matrix;
pub use matrix::MatrixMap;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_matrixmap`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/matrix.rs crates/ast-core/src/mappings/mod.rs crates/ast-core/tests/test_matrixmap.rs
git commit -m "Add MatrixMap (linear matrix transformation)"
```

---

### Task 6: PermMap

**Files:**
- Create: `crates/ast-core/src/mappings/perm.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Create: `crates/ast-core/tests/test_permmap.rs`

- [ ] **Step 1: Write PermMap tests**

Create `crates/ast-core/tests/test_permmap.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, PermMap};

#[test]
fn perm_map_swap_axes() {
    // Swap 2 axes: output gets input[2], input[1]
    let m = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);
    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[20.0, 10.0]);
}

#[test]
fn perm_map_constant_axis() {
    // 2 inputs, 3 outputs: out[0]=in[1], out[1]=constant(99), out[2]=in[2]
    let m = PermMap::new(&[2, 0, 1], &[2, -1, 1], &[99.0]).unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 3);
    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 20.0);  // in[2] = 20
    assert_eq!(out.point(0)[1], 99.0);  // constant
    assert_eq!(out.point(0)[2], 10.0);  // in[1] = 10
}

#[test]
fn perm_map_inverse() {
    // Swap axes: forward uses outperm, inverse uses inperm
    let m = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = m.transform(&pts, false).unwrap();
    // inverse also swaps
    assert_eq!(out.point(0), &[20.0, 10.0]);
}

#[test]
fn perm_map_zero_means_bad() {
    // Zero in perm means output gets BAD_VALUE
    let m = PermMap::new(&[1, 2], &[1, 0], &[]).unwrap();
    let pts = PointSet::from_rows([[5.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn perm_map_bad_value_propagation() {
    let m = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 20.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn perm_map_identity_simplifies() {
    let m = PermMap::new(&[1, 2, 3], &[1, 2, 3], &[]).unwrap();
    let s = m.simplified().unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0, 3.0]);
}
```

- [ ] **Step 2: Implement PermMap**

Create `crates/ast-core/src/mappings/perm.rs`:

```rust
//! Axis permutation/selection mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that permutes, selects, duplicates, or inserts constant axes.
///
/// Permutation encoding (1-based, matching C AST):
/// - Positive value `j`: output axis gets input axis `j`
/// - Negative value `-j`: output axis gets `constants[j-1]`
/// - Zero: output axis gets `BAD_VALUE`
#[derive(Debug, Clone)]
pub struct PermMap {
    inperm: Vec<i64>,
    outperm: Vec<i64>,
    constants: Vec<f64>,
    inverted: bool,
}

impl PermMap {
    /// Create a new PermMap.
    ///
    /// - `inperm`: permutation array for inverse transform (length = nin)
    /// - `outperm`: permutation array for forward transform (length = nout)
    /// - `constants`: constant values referenced by negative perm entries
    pub fn new(inperm: &[i64], outperm: &[i64], constants: &[f64]) -> AstResult<Self> {
        if inperm.is_empty() {
            return Err(AstError::new(ErrorCode::Prmin, "inperm must not be empty"));
        }
        if outperm.is_empty() {
            return Err(AstError::new(ErrorCode::Prmin, "outperm must not be empty"));
        }
        Ok(Self {
            inperm: inperm.to_vec(),
            outperm: outperm.to_vec(),
            constants: constants.to_vec(),
            inverted: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    fn apply_perm(perm: &[i64], constants: &[f64], input: &[f64], output: &mut [f64]) {
        for (i, &p) in perm.iter().enumerate() {
            if i >= output.len() {
                break;
            }
            output[i] = if p > 0 {
                let idx = (p - 1) as usize;
                if idx < input.len() {
                    input[idx]
                } else {
                    BAD_VALUE
                }
            } else if p < 0 {
                let idx = ((-p) - 1) as usize;
                if idx < constants.len() {
                    constants[idx]
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            };
        }
    }
}

impl Mapping for PermMap {
    fn nin(&self) -> usize {
        self.inperm.len()
    }

    fn nout(&self) -> usize {
        self.outperm.len()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        let perm = if fwd { &self.outperm } else { &self.inperm };
        let nout = perm.len();
        let mut out = PointSet::zeros(pts.npoint(), nout);

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                Self::apply_perm(perm, &self.constants, inp, outp);
            }
        }
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        // Check if it's the identity permutation
        if self.inperm.len() == self.outperm.len() {
            let n = self.outperm.len();
            let is_identity = self.outperm.iter().enumerate().all(|(i, &p)| p == (i as i64 + 1));
            if is_identity {
                return Ok(Box::new(UnitMap::new(n)));
            }
        }
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}
```

- [ ] **Step 3: Update mod.rs**

Add:
```rust
mod perm;
pub use perm::PermMap;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_permmap`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/perm.rs crates/ast-core/src/mappings/mod.rs crates/ast-core/tests/test_permmap.rs
git commit -m "Add PermMap (axis permutation/selection mapping)"
```

---

### Task 7: CmpMap and Composition

**Files:**
- Create: `crates/ast-core/src/mappings/cmpmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Create: `crates/ast-core/tests/test_cmpmap.rs`

- [ ] **Step 1: Write CmpMap tests**

Create `crates/ast-core/tests/test_cmpmap.rs`:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{CmpMap, Composition, DynMapping, Mapping, ShiftMap, UnitMap, ZoomMap};

#[test]
fn cmpmap_series_zoom_then_shift() {
    let zoom: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let shift: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    let cmp = CmpMap::new(zoom, shift, Composition::Series).unwrap();
    assert_eq!(cmp.nin(), 2);
    assert_eq!(cmp.nout(), 2);

    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = cmp.transform(&pts, true).unwrap();
    // zoom(1,2) = (2,4), then shift = (12, 24)
    assert_eq!(out.point(0), &[12.0, 24.0]);
}

#[test]
fn cmpmap_series_inverse() {
    let zoom: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let shift: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    let cmp = CmpMap::new(zoom, shift, Composition::Series).unwrap();

    let pts = PointSet::from_rows([[12.0, 24.0]]);
    let out = cmp.transform(&pts, false).unwrap();
    // inverse: un-shift (2, 4), then un-zoom (1, 2)
    assert!((out.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 2.0).abs() < 1e-10);
}

#[test]
fn cmpmap_parallel() {
    let zoom: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let shift: DynMapping = Box::new(ShiftMap::new([100.0]));
    let cmp = CmpMap::new(zoom, shift, Composition::Parallel).unwrap();
    assert_eq!(cmp.nin(), 2);
    assert_eq!(cmp.nout(), 2);

    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let out = cmp.transform(&pts, true).unwrap();
    // first coord zoomed: 5*3=15, second shifted: 7+100=107
    assert_eq!(out.point(0), &[15.0, 107.0]);
}

#[test]
fn cmpmap_series_dimension_mismatch() {
    let m1: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let m2: DynMapping = Box::new(ZoomMap::new(3, 2.0).unwrap());
    assert!(CmpMap::new(m1, m2, Composition::Series).is_err());
}

#[test]
fn cmpmap_bad_value_propagation() {
    let zoom: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let shift: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    let cmp = CmpMap::new(zoom, shift, Composition::Series).unwrap();

    let pts = PointSet::from_rows([[BAD_VALUE, 2.0]]);
    let out = cmp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn cmpmap_simplified_both_unit() {
    let u1: DynMapping = Box::new(UnitMap::new(2));
    let u2: DynMapping = Box::new(UnitMap::new(2));
    let cmp = CmpMap::new(u1, u2, Composition::Series).unwrap();
    let s = cmp.simplified().unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

#[test]
fn composition_then_method() {
    let zoom: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let shift: DynMapping = Box::new(ShiftMap::new([1.0, 1.0]));
    let cmp = zoom.then(shift).unwrap();
    let pts = PointSet::from_rows([[3.0, 4.0]]);
    let out = cmp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 9.0]);
}

#[test]
fn composition_alongside_method() {
    let zoom: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let shift: DynMapping = Box::new(ShiftMap::new([10.0]));
    let cmp = zoom.alongside(shift).unwrap();
    let pts = PointSet::from_rows([[5.0, 3.0]]);
    let out = cmp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[10.0, 13.0]);
}
```

- [ ] **Step 2: Implement CmpMap**

Create `crates/ast-core/src/mappings/cmpmap.rs`:

```rust
//! Compound (series or parallel) mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// Whether a CmpMap applies its components in series or parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    /// Apply first mapping, then feed output to second mapping.
    Series,
    /// Apply first and second side by side (split inputs, concatenate outputs).
    Parallel,
}

/// A compound mapping that combines two component mappings.
#[derive(Debug)]
pub struct CmpMap {
    first: DynMapping,
    second: DynMapping,
    kind: Composition,
    inverted: bool,
}

impl CmpMap {
    /// Create a new compound mapping.
    ///
    /// For `Series`: `first.nout()` must equal `second.nin()`.
    /// For `Parallel`: no constraint between the two.
    pub fn new(
        first: DynMapping,
        second: DynMapping,
        kind: Composition,
    ) -> AstResult<Self> {
        if kind == Composition::Series && first.nout() != second.nin() {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!(
                    "series composition: first.nout()={} != second.nin()={}",
                    first.nout(),
                    second.nin()
                ),
            ));
        }
        Ok(Self {
            first,
            second,
            kind,
            inverted: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    fn transform_series(
        &self,
        pts: &PointSet,
        forward: bool,
    ) -> AstResult<PointSet> {
        if forward {
            let mid = self.first.transform(pts, true)?;
            self.second.transform(&mid, true)
        } else {
            let mid = self.second.transform(pts, false)?;
            self.first.transform(&mid, false)
        }
    }

    fn transform_parallel(
        &self,
        pts: &PointSet,
        forward: bool,
    ) -> AstResult<PointSet> {
        let (nin1, nout1) = if forward {
            (self.first.nin(), self.first.nout())
        } else {
            (self.first.nout(), self.first.nin())
        };
        let nout2 = if forward {
            self.second.nout()
        } else {
            self.second.nin()
        };

        let npoint = pts.npoint();
        let mut out = PointSet::zeros(npoint, nout1 + nout2);

        // Split input into two halves
        let mut pts1_data = Vec::with_capacity(npoint * nin1);
        let nin2 = pts.ncoord() - nin1;
        let mut pts2_data = Vec::with_capacity(npoint * nin2);

        for i in 0..npoint {
            let p = pts.point(i);
            pts1_data.extend_from_slice(&p[..nin1]);
            pts2_data.extend_from_slice(&p[nin1..]);
        }

        let pts1 = PointSet::from_ndarray(
            ndarray::Array2::from_shape_vec((npoint, nin1), pts1_data).unwrap(),
        );
        let pts2 = PointSet::from_ndarray(
            ndarray::Array2::from_shape_vec((npoint, nin2), pts2_data).unwrap(),
        );

        let out1 = self.first.transform(&pts1, forward)?;
        let out2 = self.second.transform(&pts2, forward)?;

        // Concatenate outputs
        for i in 0..npoint {
            let outp = out.point_mut(i);
            let o1 = out1.point(i);
            let o2 = out2.point(i);
            outp[..nout1].copy_from_slice(o1);
            outp[nout1..].copy_from_slice(o2);
        }

        Ok(out)
    }
}

impl Mapping for CmpMap {
    fn nin(&self) -> usize {
        match self.kind {
            Composition::Series => self.first.nin(),
            Composition::Parallel => self.first.nin() + self.second.nin(),
        }
    }

    fn nout(&self) -> usize {
        match self.kind {
            Composition::Series => self.second.nout(),
            Composition::Parallel => self.first.nout() + self.second.nout(),
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.transform_series(pts, fwd),
            Composition::Parallel => self.transform_parallel(pts, fwd),
        }
    }

    fn transform_into(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        let s1 = self.first.simplified()?;
        let s2 = self.second.simplified()?;

        // If both are effectively identity, return UnitMap
        let n = self.nin();
        let test_pts = PointSet::from_rows([vec![1.0; n]]);
        let out1 = s1.transform(&test_pts, true)?;
        let out2 = s2.transform(&test_pts, true)?;

        let is_unit1 = s1.nin() == s1.nout()
            && !point_has_bad(out1.point(0))
            && out1.point(0) == test_pts.point(0);
        let is_unit2 = s2.nin() == s2.nout()
            && !point_has_bad(out2.point(0))
            && out2.point(0) == test_pts.point(0);

        if is_unit1 && is_unit2 && self.kind == Composition::Series {
            return Ok(Box::new(UnitMap::new(n)));
        }

        Ok(Box::new(CmpMap {
            first: s1,
            second: s2,
            kind: self.kind,
            inverted: self.inverted,
        }))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}
```

- [ ] **Step 3: Update mod.rs to final form**

Replace `crates/ast-core/src/mappings/mod.rs` with the complete version
including all `mod` declarations, `pub use` exports, and `Box<dyn Mapping>`
composition methods:

```rust
//! The Mapping trait, concrete mapping classes, and composition.

mod cmpmap;
pub(crate) mod helpers;
mod matrix;
mod perm;
pub(crate) mod rate;
mod shift;
mod unit;
mod zoom;

pub use cmpmap::{CmpMap, Composition};
pub use matrix::MatrixMap;
pub use perm::PermMap;
pub use shift::ShiftMap;
pub use unit::UnitMap;
pub use zoom::ZoomMap;

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
    /// `ax1` is the 1-based index of the output axis.
    /// `ax2` is the 1-based index of the input axis.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64>;
}

impl dyn Mapping {
    /// Compose this mapping in series with `next`: apply self first, then next.
    pub fn then(self: Box<Self>, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(self, next, Composition::Series)
    }

    /// Compose this mapping in parallel with `other`: apply side by side.
    pub fn alongside(self: Box<Self>, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(self, other, Composition::Parallel)
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_cmpmap`
Expected: all tests pass

- [ ] **Step 5: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/mappings/ crates/ast-core/tests/test_cmpmap.rs
git commit -m "Add CmpMap (compound mapping) and composition methods"
```

---

### Task 8: Add then/alongside to Concrete Types

**Files:**
- Modify: `crates/ast-core/src/mappings/unit.rs`
- Modify: `crates/ast-core/src/mappings/zoom.rs`
- Modify: `crates/ast-core/src/mappings/shift.rs`
- Modify: `crates/ast-core/src/mappings/matrix.rs`
- Modify: `crates/ast-core/src/mappings/perm.rs`

- [ ] **Step 1: Add composition methods to UnitMap**

Add to `crates/ast-core/src/mappings/unit.rs` in the `impl UnitMap` block:

```rust
impl UnitMap {
    // ... existing new() ...

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<super::CmpMap> {
        super::CmpMap::new(Box::new(self), next, super::Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<super::CmpMap> {
        super::CmpMap::new(Box::new(self), other, super::Composition::Parallel)
    }
}
```

- [ ] **Step 2: Add composition methods to ZoomMap, ShiftMap, MatrixMap, PermMap**

Add the same `then` and `alongside` methods to each (adjusting `self` type):

For ZoomMap, ShiftMap, MatrixMap, and PermMap, add an `impl` block (or extend existing one):

```rust
impl TypeName {
    pub fn then(self, next: DynMapping) -> AstResult<super::CmpMap> {
        super::CmpMap::new(Box::new(self), next, super::Composition::Series)
    }

    pub fn alongside(self, other: DynMapping) -> AstResult<super::CmpMap> {
        super::CmpMap::new(Box::new(self), other, super::Composition::Parallel)
    }
}
```

Each needs `use super::{DynMapping};` and `use crate::error::AstResult;` in scope.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/mappings/
git commit -m "Add then()/alongside() composition methods to all mapping types"
```

---

### Task 9: Clippy, Fmt, and Final Verification

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
Expected: all tests pass (previous tests + all new mapping tests)

- [ ] **Step 4: Commit fixes if any**

```bash
git add -u
git commit -m "Fix clippy and fmt issues in mapping implementations"
```

---

## Verification Checklist

After all tasks are complete:

1. `cargo build --workspace` — all crates compile
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. Each mapping handles BAD_VALUE correctly
6. Composition methods work on `Box<dyn Mapping>` and concrete types
7. `git status` — clean
