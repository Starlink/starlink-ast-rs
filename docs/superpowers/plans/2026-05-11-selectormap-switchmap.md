# SelectorMap & SwitchMap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement SelectorMap and SwitchMap — the final two public mapping classes — completing the mapping layer.

**Architecture:** SelectorMap holds `Vec<DynRegion>` and classifies points by region containment (forward-only, Nin→1). SwitchMap holds selector mappings + route mappings, dispatching point batches to the appropriate route. Both implement `Mapping` trait, `Serializable` trait, and participate in the simplification engine via `simplified()`/`try_merge()`.

**Tech Stack:** Pure Rust, ast-core crate, existing `Mapping`/`Region` trait infrastructure, `Node`-based serialization.

---

## File Structure

```
crates/ast-core/src/mappings/
├── selectormap.rs          # SelectorMap struct, builder, Mapping impl, Clone, Debug
├── switchmap.rs            # SwitchMap struct, Mapping impl, Clone, Debug
└── mod.rs                  # (modify) Add pub mod + pub use for both

crates/ast-core/src/serial/mod.rs  # (modify) Add Serializable impls, dispatch entries

crates/ast-core/src/regions/mod.rs # (modify) Extend remove_regions() for SwitchMap

crates/ast-core/src/mappings/simplify.rs # (modify) Add fingerprint support

crates/ast-core/tests/
├── test_selectormap.rs     # Unit/integration tests for SelectorMap
├── test_switchmap.rs       # Port of testswitchmap.c
└── fixtures/
    ├── selectormap_inverse_cancel.map
    ├── selectormap_inverse_cancel.simp
    ├── switchmap_internal_simplify.map
    ├── switchmap_internal_simplify.simp
    ├── switchmap_inverse_cancel.map
    ├── switchmap_inverse_cancel.simp
    ├── switchmap_invert_normalize.map
    └── switchmap_invert_normalize.simp
```

---

### Task 1: SelectorMap struct, builder, and Mapping trait

**Files:**
- Create: `crates/ast-core/src/mappings/selectormap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`

- [ ] **Step 1: Create selectormap.rs with struct, builder, and basic Mapping impl**

```rust
// crates/ast-core/src/mappings/selectormap.rs
use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::is_bad;
use crate::regions::{DynRegion, Region};

use super::{DynMapping, Mapping};

/// A Mapping that identifies which Region contains a given input position.
///
/// A SelectorMap encapsulates a number of Regions that all have the same
/// number of axes. The forward transformation searches through the Regions
/// (in order) and returns the 1-based index of the first Region that
/// contains the input position. If no Region contains the point, 0 is
/// returned. If the input position has bad coordinates, `badval` is returned.
///
/// SelectorMaps have `nin` equal to the Region dimensionality and `nout` = 1.
/// No inverse transformation is defined.
pub struct SelectorMap {
    regions: Vec<DynRegion>,
    badval: f64,
    inverted: bool,
    do_not_simplify: bool,
}

/// Builder for constructing a [`SelectorMap`].
pub struct SelectorMapBuilder {
    regions: Vec<DynRegion>,
    badval: f64,
}

impl SelectorMapBuilder {
    /// Create a new builder with the given regions.
    ///
    /// All regions must have the same `naxes()`. At least one region is required.
    pub fn new(regions: Vec<DynRegion>) -> AstResult<Self> {
        if regions.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SelectorMap: at least one Region is required",
            ));
        }
        let naxes = regions[0].naxes();
        for (i, reg) in regions.iter().enumerate().skip(1) {
            if reg.naxes() != naxes {
                return Err(AstError::new(
                    ErrorCode::Bdpar,
                    format!(
                        "SelectorMap: Region {} has {} axes but Region 1 has {}",
                        i + 1,
                        reg.naxes(),
                        naxes
                    ),
                ));
            }
        }
        Ok(Self {
            regions,
            badval: BAD_VALUE,
        })
    }

    /// Set the value returned for input positions with bad coordinates.
    ///
    /// Default is `BAD_VALUE`.
    pub fn badval(mut self, val: f64) -> Self {
        self.badval = val;
        self
    }

    /// Build the SelectorMap.
    pub fn build(self) -> SelectorMap {
        SelectorMap {
            regions: self.regions,
            badval: self.badval,
            inverted: false,
            do_not_simplify: false,
        }
    }
}

impl SelectorMap {
    /// The encapsulated regions.
    pub fn regions(&self) -> &[DynRegion] {
        &self.regions
    }

    /// The value returned for bad input positions.
    pub fn badval(&self) -> f64 {
        self.badval
    }

    /// The intrinsic number of input axes (region dimensionality).
    fn intrinsic_nin(&self) -> usize {
        self.regions[0].naxes()
    }
}

impl fmt::Debug for SelectorMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectorMap")
            .field("nin", &self.intrinsic_nin())
            .field("nout", &1usize)
            .field("nreg", &self.regions.len())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for SelectorMap {
    fn clone(&self) -> Self {
        SelectorMap {
            regions: self.regions.clone(),
            badval: self.badval,
            inverted: self.inverted,
            do_not_simplify: self.do_not_simplify,
        }
    }
}

impl Mapping for SelectorMap {
    fn nin(&self) -> usize {
        if self.inverted {
            1
        } else {
            self.intrinsic_nin()
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.intrinsic_nin()
        } else {
            1
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn has_forward(&self) -> bool {
        true
    }

    fn has_inverse(&self) -> bool {
        false
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let effective_fwd = forward != self.inverted;
        let npoint = pts.npoint();

        if !effective_fwd {
            // Inverse not defined — return all BAD.
            let ncoord_out = self.intrinsic_nin();
            let mut out = PointSet::new(npoint, ncoord_out);
            for i in 0..npoint {
                for j in 0..ncoord_out {
                    out.point_mut(i)[j] = BAD_VALUE;
                }
            }
            return Ok(out);
        }

        let ncoord = self.intrinsic_nin();
        let mut out = PointSet::new(npoint, 1);

        // Initialise output: -1 for bad input positions, 0 for others.
        for i in 0..npoint {
            let pt = pts.point(i);
            let has_bad = pt.iter().take(ncoord).any(|&v| is_bad(v));
            out.point_mut(i)[0] = if has_bad { -1.0 } else { 0.0 };
        }

        // Working copy of remaining (unassigned) points.
        let mut working = pts.clone();

        for ireg in 0..self.regions.len() {
            let reg = &self.regions[ireg];

            // Negate the region and open its boundary, then transform.
            // Points INSIDE the (original) region will become BAD in the output
            // of the negated region's forward transform.
            let mut neg_reg = reg.negated_copy();
            let was_closed = neg_reg.is_closed();
            neg_reg.set_closed(!was_closed);

            let transformed = neg_reg.transform(&working, true)?;

            // Any point that hasn't been assigned yet (output == 0) and is now
            // BAD in the first coordinate of the transformed output is inside
            // this region.
            for i in 0..npoint {
                if out.point(i)[0] == 0.0 && is_bad(transformed.point(i)[0]) {
                    out.point_mut(i)[0] = (ireg + 1) as f64;
                }
            }

            // Update working buffer: swap in the transformed result.
            // Points that were interior are now BAD, so they won't match
            // any subsequent region test.
            working = transformed;
        }

        // Replace -1 sentinel (bad input) with badval.
        for i in 0..npoint {
            if out.point(i)[0] == -1.0 {
                out.point_mut(i)[0] = self.badval;
            }
        }

        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.copy_from(&result);
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        // Try to simplify each child region.
        let mut changed = false;
        let mut simplified_regions: Vec<DynRegion> = Vec::with_capacity(self.regions.len());
        for reg in &self.regions {
            let simp: DynMapping = reg.simplified()?;
            // Check if simplification changed the region (by equality test).
            if !simp.equal(reg.as_ref() as &dyn Mapping) {
                changed = true;
            }
            // The simplified result should still be a Region — downcast it back.
            // If it's no longer a region (e.g. simplified to UnitMap), keep original.
            if let Some(r) = simp.as_any().downcast_ref::<crate::regions::Circle>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::BoxRegion>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::Ellipse>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::Polygon>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::Interval>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::NullRegion>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::CmpRegion>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::PointList>() {
                simplified_regions.push(Box::new(r.clone()));
            } else if let Some(r) = simp.as_any().downcast_ref::<crate::regions::Prism>() {
                simplified_regions.push(Box::new(r.clone()));
            } else {
                // Simplification produced non-region — keep original.
                simplified_regions.push(reg.clone());
                changed = false;
            }
        }

        if changed {
            Ok(Box::new(SelectorMap {
                regions: simplified_regions,
                badval: self.badval,
                inverted: self.inverted,
                do_not_simplify: false,
            }))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }

        // Cancel adjacent SelectorMap + inverted SelectorMap → UnitMap.
        let other_sel = other.as_any().downcast_ref::<SelectorMap>()?;

        // One must be inverted, the other not.
        if self_inv == other_inv {
            return None;
        }

        // Check equality (ignoring inversion state).
        if self.regions.len() != other_sel.regions.len() {
            return None;
        }
        if !crate::mappings::helpers::ast_equal(self.badval, other_sel.badval) {
            return None;
        }
        for (a, b) in self.regions.iter().zip(other_sel.regions.iter()) {
            if !(a as &dyn Mapping).equal(b.as_ref() as &dyn Mapping) {
                return None;
            }
        }

        // They're equal-but-opposite → UnitMap.
        Some(Box::new(crate::mappings::UnitMap::new(self.intrinsic_nin())))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_sel) = other.as_any().downcast_ref::<SelectorMap>() else {
            return false;
        };
        if self.inverted != other_sel.inverted {
            return false;
        }
        if self.regions.len() != other_sel.regions.len() {
            return false;
        }
        if !crate::mappings::helpers::ast_equal(self.badval, other_sel.badval) {
            return false;
        }
        for (a, b) in self.regions.iter().zip(other_sel.regions.iter()) {
            if !(a as &dyn Mapping).equal(b.as_ref() as &dyn Mapping) {
                return false;
            }
        }
        true
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}
```

- [ ] **Step 2: Add module declaration and re-export to mod.rs**

Add to `crates/ast-core/src/mappings/mod.rs` after the `mathmap` line:

```rust
mod selectormap;
pub use selectormap::{SelectorMap, SelectorMapBuilder};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core 2>&1 | head -30`
Expected: Successful build (or only unrelated warnings).

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/mappings/selectormap.rs crates/ast-core/src/mappings/mod.rs
git commit -m "feat(mappings): add SelectorMap struct, builder, and Mapping impl"
```

---

### Task 2: SelectorMap unit tests

**Files:**
- Create: `crates/ast-core/tests/test_selectormap.rs`

- [ ] **Step 1: Write basic SelectorMap tests**

```rust
// crates/ast-core/tests/test_selectormap.rs
//! Tests for SelectorMap.
//!
//! Ancestor: C AST SelectorMap (selectormap.c) — tests region classification.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::BasicFrame;
use ast_core::mappings::{Mapping, SelectorMap, SelectorMapBuilder};
use ast_core::regions::{BoxRegion, BoxRegionBuilder, Circle, CircleBuilder, DynRegion};

fn make_2d_frame() -> BasicFrame {
    BasicFrame::new(2)
}

/// Two non-overlapping 2D boxes: Box1 covers [0,10]×[0,10], Box2 covers [20,30]×[20,30].
fn make_two_boxes() -> Vec<DynRegion> {
    let frame = make_2d_frame();
    let box1: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame.clone(), &[0.0, 0.0], &[10.0, 10.0])
            .build()
            .unwrap(),
    );
    let box2: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame, &[20.0, 20.0], &[30.0, 30.0])
            .build()
            .unwrap(),
    );
    vec![box1, box2]
}

#[test]
fn selectormap_basic_construction() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    assert_eq!(sm.nin(), 2);
    assert_eq!(sm.nout(), 1);
    assert!(!sm.is_inverted());
    assert!(sm.has_forward());
    assert!(!sm.has_inverse());
}

#[test]
fn selectormap_empty_regions_rejected() {
    let result = SelectorMapBuilder::new(vec![]);
    assert!(result.is_err());
}

#[test]
fn selectormap_mismatched_axes_rejected() {
    let frame2 = BasicFrame::new(2);
    let frame3 = BasicFrame::new(3);
    let box2d: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame2, &[0.0, 0.0], &[1.0, 1.0])
            .build()
            .unwrap(),
    );
    let box3d: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame3, &[0.0, 0.0, 0.0], &[1.0, 1.0, 1.0])
            .build()
            .unwrap(),
    );
    let result = SelectorMapBuilder::new(vec![box2d, box3d]);
    assert!(result.is_err());
}

#[test]
fn selectormap_point_in_region1() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0);
}

#[test]
fn selectormap_point_in_region2() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[25.0, 25.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
}

#[test]
fn selectormap_point_in_no_region() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[15.0, 15.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 0.0);
}

#[test]
fn selectormap_bad_input_returns_badval() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().badval(99.0).build();
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 99.0);
}

#[test]
fn selectormap_multiple_points() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([
        [5.0, 5.0],       // in region 1
        [25.0, 25.0],     // in region 2
        [15.0, 15.0],     // no region
        [BAD_VALUE, 5.0], // bad input
    ]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0);
    assert_eq!(out.point(1)[0], 2.0);
    assert_eq!(out.point(2)[0], 0.0);
    assert_eq!(out.point(3)[0], BAD_VALUE); // default badval
}

#[test]
fn selectormap_first_region_wins() {
    // Overlapping regions: both cover [0,10]×[0,10].
    let frame = make_2d_frame();
    let box1: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame.clone(), &[0.0, 0.0], &[10.0, 10.0])
            .build()
            .unwrap(),
    );
    let box2: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame, &[0.0, 0.0], &[10.0, 10.0])
            .build()
            .unwrap(),
    );
    let sm = SelectorMapBuilder::new(vec![box1, box2]).unwrap().build();
    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0); // first region wins
}

#[test]
fn selectormap_inverse_returns_bad() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[1.0]]);
    let out = sm.transform(&pts, false).unwrap();
    // Inverse not defined — all BAD.
    assert_eq!(out.ncoord(), 2);
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_selectormap 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_selectormap.rs
git commit -m "test(mappings): add SelectorMap unit tests"
```

---

### Task 3: SelectorMap serialization

**Files:**
- Modify: `crates/ast-core/src/serial/mod.rs`

- [ ] **Step 1: Add Serializable impl for SelectorMap**

Add the import at the top of `serial/mod.rs` alongside the other mapping imports:

```rust
use crate::mappings::SelectorMap;
```

Add the `Serializable` impl (after the MathMap impl or at the end of the mapping serialization section):

```rust
impl Serializable for SelectorMap {
    fn class_name(&self) -> &'static str {
        "SelectorMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Region identification Mapping".to_string());
        write_mapping_base(&mut node, self);

        // Write each region as Reg1, Reg2, etc.
        for (i, reg) in self.regions().iter().enumerate() {
            let key = format!("Reg{}", i + 1);
            let reg_node = RegionToNode::to_node(reg.as_ref())?;
            node.push_object(&key, reg_node, Some("Region of input space".to_string()));
        }

        // Write BadVal only if not the default (BAD_VALUE).
        if !is_bad(self.badval()) {
            node.push_double(
                "BadVal",
                self.badval(),
                Some("Output value for bad input positions".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        // Read regions: Reg1, Reg2, ...
        let mut regions: Vec<DynRegion> = Vec::new();
        let mut i = 1;
        loop {
            let key = format!("Reg{}", i);
            match node.get_object(&key) {
                Some(reg_node) => {
                    regions.push(region_from_node(reg_node)?);
                }
                None => break,
            }
            i += 1;
        }

        if regions.is_empty() {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "SelectorMap: no regions found in serialized data",
            ));
        }

        let badval = node.get_double("BadVal", BAD_VALUE);

        let mut sm = SelectorMapBuilder::new(regions)
            .map_err(|e| AstError::new(ErrorCode::Lderr, format!("SelectorMap: {e}")))?
            .badval(badval)
            .build();

        sm.set_do_not_simplify(base.do_not_simplify);
        if base.inverted {
            sm.invert();
        }
        Ok(sm)
    }
}
```

- [ ] **Step 2: Add dispatch entries**

In `mapping_from_node()`, add before the `other =>` catch-all:

```rust
"SelectorMap" => Ok(Box::new(SelectorMap::from_node(node)?)),
```

In the `impl MappingToNode for dyn Mapping` block, add before the `Err(...)` at the end:

```rust
if let Some(m) = self.as_any().downcast_ref::<SelectorMap>() {
    return m.to_node();
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core 2>&1 | head -20`
Expected: Successful build.

- [ ] **Step 4: Add serialization round-trip test**

Add to `crates/ast-core/tests/test_selectormap.rs`:

```rust
use ast_core::serial::Channel;

#[test]
fn selectormap_serialization_roundtrip() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().badval(99.0).build();

    let serialized = Channel::dump_to_string(&sm).unwrap();
    let loaded: SelectorMap = Channel::load_from_string(&serialized).unwrap();

    assert_eq!(loaded.regions().len(), 2);
    assert_eq!(loaded.badval(), 99.0);
    assert_eq!(loaded.nin(), 2);
    assert_eq!(loaded.nout(), 1);

    // Functional equivalence check.
    let pts = PointSet::from_rows([[5.0, 5.0], [25.0, 25.0], [15.0, 15.0]]);
    let out1 = sm.transform(&pts, true).unwrap();
    let out2 = loaded.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert_eq!(out1.point(i)[0], out2.point(i)[0]);
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p ast-core --test test_selectormap 2>&1 | tail -20`
Expected: All tests pass including round-trip.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/serial/mod.rs crates/ast-core/tests/test_selectormap.rs
git commit -m "feat(serial): add SelectorMap serialization and dispatch"
```

---

### Task 4: SwitchMap struct and Mapping trait

**Files:**
- Create: `crates/ast-core/src/mappings/switchmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`

- [ ] **Step 1: Create switchmap.rs with struct and Mapping impl**

```rust
// crates/ast-core/src/mappings/switchmap.rs
use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::is_bad;

use super::{DynMapping, Mapping, UnitMap};

/// A Mapping that dispatches positions to one of several route Mappings
/// based on a selector value.
///
/// A SwitchMap encapsulates:
/// - A forward selector Mapping (used for the forward transform)
/// - An inverse selector Mapping (used for the inverse transform)
/// - A set of route Mappings, all sharing the same nin and nout
///
/// The selector Mapping produces a single output value. This value is
/// rounded to the nearest integer and used as a 1-based index into the
/// route Mappings array. The selected route Mapping is then used to
/// transform the input position.
pub struct SwitchMap {
    fsmap: Option<DynMapping>,
    ismap: Option<DynMapping>,
    route_maps: Vec<DynMapping>,
    fsinv: bool,
    isinv: bool,
    route_inv: Vec<bool>,
    inverted: bool,
    do_not_simplify: bool,
    /// Intrinsic nin (route mappings' common nin).
    intrinsic_nin: usize,
    /// Intrinsic nout (route mappings' common nout).
    intrinsic_nout: usize,
}

impl SwitchMap {
    /// Create a new SwitchMap.
    ///
    /// - `fsmap`: forward selector Mapping (required for forward transform)
    /// - `ismap`: inverse selector Mapping (required for inverse transform)
    /// - `route_maps`: array of route Mappings (at least one required)
    ///
    /// At least one of `fsmap` or `ismap` must be `Some`.
    /// All route Mappings must share the same `nin()` and `nout()`.
    pub fn new(
        fsmap: Option<DynMapping>,
        ismap: Option<DynMapping>,
        route_maps: Vec<DynMapping>,
    ) -> AstResult<Self> {
        if route_maps.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SwitchMap: at least one route Mapping is required",
            ));
        }
        if fsmap.is_none() && ismap.is_none() {
            return Err(AstError::new(
                ErrorCode::Intrd,
                "SwitchMap: at least one selector Mapping must be supplied",
            ));
        }

        let nin = route_maps[0].nin();
        let nout = route_maps[0].nout();

        for (i, rm) in route_maps.iter().enumerate().skip(1) {
            if rm.nin() != nin {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SwitchMap: route Mapping {} has {} inputs but route 1 has {}",
                        i + 1,
                        rm.nin(),
                        nin
                    ),
                ));
            }
            if rm.nout() != nout {
                return Err(AstError::new(
                    ErrorCode::Badno,
                    format!(
                        "SwitchMap: route Mapping {} has {} outputs but route 1 has {}",
                        i + 1,
                        rm.nout(),
                        nout
                    ),
                ));
            }
        }

        if let Some(ref fs) = fsmap {
            if fs.nin() != nin {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SwitchMap: forward selector has {} inputs but routes have {}",
                        fs.nin(),
                        nin
                    ),
                ));
            }
            if fs.nout() != 1 {
                return Err(AstError::new(
                    ErrorCode::Badno,
                    format!(
                        "SwitchMap: forward selector has {} outputs but should have 1",
                        fs.nout()
                    ),
                ));
            }
        }

        if let Some(ref is) = ismap {
            if is.nout() != nout {
                return Err(AstError::new(
                    ErrorCode::Badno,
                    format!(
                        "SwitchMap: inverse selector has {} outputs but routes have {}",
                        is.nout(),
                        nout
                    ),
                ));
            }
            if is.nin() != 1 {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SwitchMap: inverse selector has {} inputs but should have 1",
                        is.nin()
                    ),
                ));
            }
        }

        let fsinv = fsmap.as_ref().map(|m| m.is_inverted()).unwrap_or(false);
        let isinv = ismap.as_ref().map(|m| m.is_inverted()).unwrap_or(false);
        let route_inv: Vec<bool> = route_maps.iter().map(|m| m.is_inverted()).collect();

        Ok(Self {
            fsmap,
            ismap,
            route_maps,
            fsinv,
            isinv,
            route_inv,
            inverted: false,
            do_not_simplify: false,
            intrinsic_nin: nin,
            intrinsic_nout: nout,
        })
    }

    /// Construct from pre-recorded invert flags (used by deserialization).
    pub(crate) fn from_parts(
        fsmap: Option<DynMapping>,
        ismap: Option<DynMapping>,
        route_maps: Vec<DynMapping>,
        fsinv: bool,
        isinv: bool,
        route_inv: Vec<bool>,
    ) -> AstResult<Self> {
        if route_maps.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SwitchMap: at least one route Mapping is required",
            ));
        }
        let nin = route_maps[0].nin();
        let nout = route_maps[0].nout();
        Ok(Self {
            fsmap,
            ismap,
            route_maps,
            fsinv,
            isinv,
            route_inv,
            inverted: false,
            do_not_simplify: false,
            intrinsic_nin: nin,
            intrinsic_nout: nout,
        })
    }

    /// The forward selector Mapping (if any).
    pub fn forward_selector(&self) -> Option<&dyn Mapping> {
        self.fsmap.as_deref()
    }

    /// The inverse selector Mapping (if any).
    pub fn inverse_selector(&self) -> Option<&dyn Mapping> {
        self.ismap.as_deref()
    }

    /// The route Mappings.
    pub fn route_maps(&self) -> &[DynMapping] {
        &self.route_maps
    }

    /// Number of route Mappings.
    pub fn nroute(&self) -> usize {
        self.route_maps.len()
    }

    /// Recorded forward selector invert flag.
    pub(crate) fn fsinv(&self) -> bool {
        self.fsinv
    }

    /// Recorded inverse selector invert flag.
    pub(crate) fn isinv(&self) -> bool {
        self.isinv
    }

    /// Recorded route invert flags.
    pub(crate) fn route_inv(&self) -> &[bool] {
        &self.route_inv
    }

    /// Get the effective selector mapping for the given direction,
    /// accounting for SwitchMap inversion.
    fn get_selector(&self, forward: bool) -> Option<&DynMapping> {
        let use_forward = forward != self.inverted;
        if use_forward {
            self.fsmap.as_ref()
        } else {
            self.ismap.as_ref()
        }
    }

    /// Get the recorded invert flag for the effective selector.
    fn get_selector_inv(&self, forward: bool) -> bool {
        let use_forward = forward != self.inverted;
        if use_forward {
            self.fsinv
        } else {
            self.isinv
        }
    }

    /// Determine whether the route mapping should be applied in its forward
    /// direction, accounting for recorded invert flags and SwitchMap inversion.
    fn route_effective_forward(&self, route_idx: usize, forward: bool) -> bool {
        // The route's forward direction matches the SwitchMap's effective
        // direction, but we need to account for the recorded invert flag
        // vs the route's current state, and whether the SwitchMap is inverted.
        let effective_fwd = forward != self.inverted;
        let inv_changed = self.route_inv[route_idx] != self.route_maps[route_idx].is_inverted();
        if self.inverted {
            // SwitchMap inverted: invert the route too.
            inv_changed == effective_fwd
        } else {
            effective_fwd != inv_changed
        }
    }

    /// Determine the effective forward flag for the selector mapping.
    fn selector_effective_forward(&self, forward: bool) -> bool {
        let sel = self.get_selector(forward);
        let sel_inv = self.get_selector_inv(forward);
        if let Some(s) = sel {
            let inv_changed = sel_inv != s.is_inverted();
            let effective_fwd = forward != self.inverted;
            if self.inverted {
                inv_changed == effective_fwd
            } else {
                effective_fwd != inv_changed
            }
        } else {
            true
        }
    }
}

impl fmt::Debug for SwitchMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwitchMap")
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("nroute", &self.route_maps.len())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for SwitchMap {
    fn clone(&self) -> Self {
        SwitchMap {
            fsmap: self.fsmap.clone(),
            ismap: self.ismap.clone(),
            route_maps: self.route_maps.clone(),
            fsinv: self.fsinv,
            isinv: self.isinv,
            route_inv: self.route_inv.clone(),
            inverted: self.inverted,
            do_not_simplify: self.do_not_simplify,
            intrinsic_nin: self.intrinsic_nin,
            intrinsic_nout: self.intrinsic_nout,
        }
    }
}

impl Mapping for SwitchMap {
    fn nin(&self) -> usize {
        if self.inverted {
            self.intrinsic_nout
        } else {
            self.intrinsic_nin
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.intrinsic_nin
        } else {
            self.intrinsic_nout
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn has_forward(&self) -> bool {
        self.fsmap.is_some()
    }

    fn has_inverse(&self) -> bool {
        self.ismap.is_some()
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let effective_fwd = forward != self.inverted;

        // Determine dimensions.
        let ncin = if effective_fwd { self.intrinsic_nin } else { self.intrinsic_nout };
        let ncout = if effective_fwd { self.intrinsic_nout } else { self.intrinsic_nin };
        let npoint = pts.npoint();
        let nroute = self.route_maps.len();

        // Get selector mapping.
        let sel = self.get_selector(forward).ok_or_else(|| {
            AstError::new(
                ErrorCode::Intrd,
                format!(
                    "SwitchMap: no {} selector Mapping available",
                    if effective_fwd { "forward" } else { "inverse" }
                ),
            )
        })?;

        // Transform through selector to get selector values.
        let sel_fwd = self.selector_effective_forward(forward);
        let sel_out = sel.transform(pts, sel_fwd)?;
        let sel_vals: Vec<f64> = (0..npoint).map(|i| sel_out.point(i)[0]).collect();

        // Count points per route.
        let mut pop: Vec<usize> = vec![0; nroute];
        for &sv in &sel_vals {
            if !is_bad(sv) {
                let idx = sv.round() as i64 - 1;
                if idx >= 0 && (idx as usize) < nroute {
                    pop[idx as usize] += 1;
                }
            }
        }

        let total_assigned: usize = pop.iter().sum();

        // Allocate output.
        let mut out = PointSet::new(npoint, ncout);

        // If some points unassigned, initialise output to BAD.
        if total_assigned < npoint {
            for i in 0..npoint {
                for j in 0..ncout {
                    out.point_mut(i)[j] = BAD_VALUE;
                }
            }
        }

        // Process each route.
        for iroute in 0..nroute {
            if pop[iroute] == 0 {
                continue;
            }

            let route_fwd = self.route_effective_forward(iroute, forward);

            // Gather input points for this route.
            let mut batch_in = PointSet::new(pop[iroute], ncin);
            let mut point_indices: Vec<usize> = Vec::with_capacity(pop[iroute]);
            let mut k = 0;
            for (ipoint, &sv) in sel_vals.iter().enumerate() {
                if !is_bad(sv) {
                    let idx = sv.round() as i64 - 1;
                    if idx == iroute as i64 {
                        for j in 0..ncin {
                            batch_in.point_mut(k)[j] = pts.point(ipoint)[j];
                        }
                        point_indices.push(ipoint);
                        k += 1;
                    }
                }
            }

            // Transform the batch.
            let batch_out = self.route_maps[iroute].transform(&batch_in, route_fwd)?;

            // Scatter results back.
            for (k, &ipoint) in point_indices.iter().enumerate() {
                for j in 0..ncout {
                    out.point_mut(ipoint)[j] = batch_out.point(k)[j];
                }
            }
        }

        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.copy_from(&result);
        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // Use forward selector to determine which route handles this point.
        let sel = match self.get_selector(true) {
            Some(s) => s,
            None => return crate::mappings::rate::numeric_rate(self, at, ax1, ax2),
        };

        let sel_fwd = self.selector_effective_forward(true);
        let pts = PointSet::from_slice(at, at.len());
        let sel_out = sel.transform(&pts, sel_fwd)?;
        let sv = sel_out.point(0)[0];

        if is_bad(sv) {
            return Ok(BAD_VALUE);
        }

        let idx = sv.round() as i64 - 1;
        if idx < 0 || idx as usize >= self.route_maps.len() {
            return Ok(BAD_VALUE);
        }

        let route_fwd = self.route_effective_forward(idx as usize, true);
        // Delegate to the route's rate method.
        // If the route is used in inverse direction, swap ax1/ax2 semantics
        // via the standard rate interface — the route handles it internally.
        if route_fwd {
            self.route_maps[idx as usize].rate(at, ax1, ax2)
        } else {
            // For inverse direction, use numeric rate as a safe fallback.
            crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_sw) = other.as_any().downcast_ref::<SwitchMap>() else {
            return false;
        };
        if self.inverted != other_sw.inverted {
            return false;
        }
        if self.route_maps.len() != other_sw.route_maps.len() {
            return false;
        }
        // Compare selectors.
        match (&self.fsmap, &other_sw.fsmap) {
            (Some(a), Some(b)) => {
                if !a.equal(b.as_ref()) {
                    return false;
                }
            }
            (None, None) => {}
            _ => return false,
        }
        match (&self.ismap, &other_sw.ismap) {
            (Some(a), Some(b)) => {
                if !a.equal(b.as_ref()) {
                    return false;
                }
            }
            (None, None) => {}
            _ => return false,
        }
        // Compare routes.
        for (a, b) in self.route_maps.iter().zip(other_sw.route_maps.iter()) {
            if !a.equal(b.as_ref()) {
                return false;
            }
        }
        true
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        // Normalize inversion: if inverted, create equivalent non-inverted SwitchMap.
        if self.inverted {
            let mut new_routes: Vec<DynMapping> = Vec::with_capacity(self.route_maps.len());
            for rm in &self.route_maps {
                let mut r = rm.clone();
                r.invert();
                new_routes.push(r);
            }
            let new_fsmap = self.ismap.clone().map(|mut m| { m.invert(); m });
            let new_ismap = self.fsmap.clone().map(|mut m| { m.invert(); m });
            let normalized = SwitchMap::new(new_fsmap, new_ismap, new_routes)?;
            return normalized.simplified();
        }

        // Simplify each child.
        let mut changed = false;

        let simp_fs = self.fsmap.as_ref().map(|m| m.simplified()).transpose()?;
        if let (Some(orig), Some(ref simp)) = (&self.fsmap, &simp_fs) {
            if !simp.equal(orig.as_ref()) || simp.is_inverted() != orig.is_inverted() {
                changed = true;
            }
        }

        let simp_is = self.ismap.as_ref().map(|m| m.simplified()).transpose()?;
        if let (Some(orig), Some(ref simp)) = (&self.ismap, &simp_is) {
            if !simp.equal(orig.as_ref()) || simp.is_inverted() != orig.is_inverted() {
                changed = true;
            }
        }

        let mut simp_routes: Vec<DynMapping> = Vec::with_capacity(self.route_maps.len());
        for rm in &self.route_maps {
            let s = rm.simplified()?;
            if !s.equal(rm.as_ref()) || s.is_inverted() != rm.is_inverted() {
                changed = true;
            }
            simp_routes.push(s);
        }

        if changed {
            let new_sw = SwitchMap::new(
                simp_fs.or_else(|| self.fsmap.clone()),
                simp_is.or_else(|| self.ismap.clone()),
                simp_routes,
            )?;
            Ok(Box::new(new_sw))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }

        // Cancel adjacent SwitchMap + inverted SwitchMap → UnitMap.
        let other_sw = other.as_any().downcast_ref::<SwitchMap>()?;

        if self_inv == other_inv {
            return None;
        }

        // Check equality ignoring inversion.
        if self.route_maps.len() != other_sw.route_maps.len() {
            return None;
        }
        // Compare selectors (accounting for the inversion swap).
        let (self_fs, self_is) = if self_inv {
            (&self.ismap, &self.fsmap)
        } else {
            (&self.fsmap, &self.ismap)
        };
        let (other_fs, other_is) = if other_inv {
            (&other_sw.ismap, &other_sw.fsmap)
        } else {
            (&other_sw.fsmap, &other_sw.ismap)
        };
        match (self_fs, other_fs) {
            (Some(a), Some(b)) => {
                if !a.equal(b.as_ref()) {
                    return None;
                }
            }
            (None, None) => {}
            _ => return None,
        }
        match (self_is, other_is) {
            (Some(a), Some(b)) => {
                if !a.equal(b.as_ref()) {
                    return None;
                }
            }
            (None, None) => {}
            _ => return None,
        }
        for (a, b) in self.route_maps.iter().zip(other_sw.route_maps.iter()) {
            if !a.equal(b.as_ref()) {
                return None;
            }
        }

        Some(Box::new(UnitMap::new(self.intrinsic_nin)))
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}
```

- [ ] **Step 2: Add module declaration and re-export to mod.rs**

Add to `crates/ast-core/src/mappings/mod.rs` after the `selectormap` line:

```rust
mod switchmap;
pub use switchmap::SwitchMap;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core 2>&1 | head -30`
Expected: Successful build.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/mappings/switchmap.rs crates/ast-core/src/mappings/mod.rs
git commit -m "feat(mappings): add SwitchMap struct and Mapping impl"
```

---

### Task 5: SwitchMap unit tests

**Files:**
- Create: `crates/ast-core/tests/test_switchmap.rs`

- [ ] **Step 1: Write SwitchMap unit tests**

```rust
// crates/ast-core/tests/test_switchmap.rs
//! Tests for SwitchMap.
//!
//! Ancestor: C AST testswitchmap.c — tests selector-based route dispatching.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::BasicFrame;
use ast_core::mappings::{
    Mapping, SelectorMap, SelectorMapBuilder, ShiftMap, SwitchMap, UnitMap, WinMap, ZoomMap,
};
use ast_core::regions::{BoxRegion, BoxRegionBuilder, DynRegion};

/// Create a SwitchMap with two routes:
/// - Route 1 (x in [0,10]): WinMap scaling [0,10]→[0,100]
/// - Route 2 (x in [20,30]): WinMap scaling [20,30]→[200,300]
/// Forward selector is a SelectorMap over 1D regions.
fn make_1d_switchmap() -> SwitchMap {
    let frame = BasicFrame::new(1);

    // Regions for the selector.
    let reg1: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame.clone(), &[0.0], &[10.0])
            .build()
            .unwrap(),
    );
    let reg2: DynRegion = Box::new(
        BoxRegionBuilder::from_corners(frame, &[20.0], &[30.0])
            .build()
            .unwrap(),
    );
    let sel = SelectorMapBuilder::new(vec![reg1, reg2]).unwrap().build();
    let fsmap: Box<dyn Mapping> = Box::new(sel);

    // Route mappings.
    let route1: Box<dyn Mapping> = Box::new(WinMap::new(&[0.0], &[10.0], &[0.0], &[100.0]).unwrap());
    let route2: Box<dyn Mapping> = Box::new(WinMap::new(&[20.0], &[30.0], &[200.0], &[300.0]).unwrap());

    SwitchMap::new(Some(fsmap), None, vec![route1, route2]).unwrap()
}

#[test]
fn switchmap_basic_construction() {
    let sw = make_1d_switchmap();
    assert_eq!(sw.nin(), 1);
    assert_eq!(sw.nout(), 1);
    assert_eq!(sw.nroute(), 2);
    assert!(sw.has_forward());
    assert!(!sw.has_inverse());
}

#[test]
fn switchmap_no_routes_rejected() {
    let result = SwitchMap::new(Some(Box::new(UnitMap::new(1))), None, vec![]);
    assert!(result.is_err());
}

#[test]
fn switchmap_no_selectors_rejected() {
    let route: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let result = SwitchMap::new(None, None, vec![route]);
    assert!(result.is_err());
}

#[test]
fn switchmap_mismatched_routes_rejected() {
    let r1: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let r2: Box<dyn Mapping> = Box::new(UnitMap::new(3));
    let fs: Box<dyn Mapping> = Box::new(UnitMap::new(2)); // wrong nout for selector but let's test route mismatch
    let result = SwitchMap::new(Some(Box::new(ZoomMap::new(2, 1.0).unwrap())), None, vec![r1, r2]);
    assert!(result.is_err());
}

#[test]
fn switchmap_forward_route1() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[5.0]]); // In region 1 → route 1
    let out = sw.transform(&pts, true).unwrap();
    // WinMap [0,10]→[0,100]: 5 → 50
    assert!((out.point(0)[0] - 50.0).abs() < 1e-10);
}

#[test]
fn switchmap_forward_route2() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[25.0]]); // In region 2 → route 2
    let out = sw.transform(&pts, true).unwrap();
    // WinMap [20,30]→[200,300]: 25 → 250
    assert!((out.point(0)[0] - 250.0).abs() < 1e-10);
}

#[test]
fn switchmap_forward_no_route() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[15.0]]); // In neither region → BAD
    let out = sw.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn switchmap_forward_bad_input() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = sw.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn switchmap_forward_multiple_points() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[5.0], [25.0], [15.0], [BAD_VALUE]]);
    let out = sw.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 50.0).abs() < 1e-10);
    assert!((out.point(1)[0] - 250.0).abs() < 1e-10);
    assert_eq!(out.point(2)[0], BAD_VALUE);
    assert_eq!(out.point(3)[0], BAD_VALUE);
}

#[test]
fn switchmap_equal() {
    let sw1 = make_1d_switchmap();
    let sw2 = make_1d_switchmap();
    assert!(sw1.equal(&sw2));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_switchmap 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_switchmap.rs
git commit -m "test(mappings): add SwitchMap unit tests"
```

---

### Task 6: SwitchMap serialization

**Files:**
- Modify: `crates/ast-core/src/serial/mod.rs`

- [ ] **Step 1: Add Serializable impl for SwitchMap**

Add the import:

```rust
use crate::mappings::SwitchMap;
```

Add the impl:

```rust
impl Serializable for SwitchMap {
    fn class_name(&self) -> &'static str {
        "SwitchMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Alternate regionalised Mapping".to_string());
        write_mapping_base(&mut node, self);

        // Forward selector.
        if let Some(ref fs) = self.forward_selector() {
            let fs_node = MappingToNode::to_node(*fs)?;
            node.push_object("FSMap", fs_node, Some("Forward selector Mapping".to_string()));
            if self.fsinv() {
                node.push_int(
                    "FSInv",
                    1,
                    Some("Fwd selector used in inverse direction".to_string()),
                );
            }
        }

        // Inverse selector.
        if let Some(ref is) = self.inverse_selector() {
            let is_node = MappingToNode::to_node(*is)?;
            node.push_object("ISMap", is_node, Some("Inverse selector Mapping".to_string()));
            if self.isinv() {
                node.push_int(
                    "ISInv",
                    1,
                    Some("Inv selector used in inverse direction".to_string()),
                );
            }
        }

        // Route mappings.
        for (i, rm) in self.route_maps().iter().enumerate() {
            let key = format!("RMap{}", i + 1);
            let rm_node = MappingToNode::to_node(rm.as_ref())?;
            node.push_object(&key, rm_node, Some("Route Mapping".to_string()));

            if self.route_inv()[i] {
                let inv_key = format!("RInv{}", i + 1);
                node.push_int(
                    &inv_key,
                    1,
                    Some("Route Mapping used in inverse direction".to_string()),
                );
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        // Read forward selector.
        let fsmap = node
            .get_object("FSMap")
            .map(|n| mapping_from_node(n))
            .transpose()?;
        let fsinv = node.get_int("FSInv", 0) != 0;

        // Read inverse selector.
        let ismap = node
            .get_object("ISMap")
            .map(|n| mapping_from_node(n))
            .transpose()?;
        let isinv = node.get_int("ISInv", 0) != 0;

        // Read route mappings.
        let mut route_maps: Vec<DynMapping> = Vec::new();
        let mut route_inv: Vec<bool> = Vec::new();
        let mut i = 1;
        loop {
            let key = format!("RMap{}", i);
            match node.get_object(&key) {
                Some(rm_node) => {
                    route_maps.push(mapping_from_node(rm_node)?);
                    let inv_key = format!("RInv{}", i);
                    route_inv.push(node.get_int(&inv_key, 0) != 0);
                }
                None => break,
            }
            i += 1;
        }

        if route_maps.is_empty() {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "SwitchMap: no route Mappings found in serialized data",
            ));
        }

        let mut sw = SwitchMap::from_parts(fsmap, ismap, route_maps, fsinv, isinv, route_inv)?;
        sw.set_do_not_simplify(base.do_not_simplify);
        if base.inverted {
            sw.invert();
        }
        Ok(sw)
    }
}
```

- [ ] **Step 2: Add dispatch entries**

In `mapping_from_node()`, add:

```rust
"SwitchMap" => Ok(Box::new(SwitchMap::from_node(node)?)),
```

In `impl MappingToNode for dyn Mapping`, add:

```rust
if let Some(m) = self.as_any().downcast_ref::<SwitchMap>() {
    return m.to_node();
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p ast-core 2>&1 | head -20`
Expected: Successful build.

- [ ] **Step 4: Add serialization round-trip test**

Add to `crates/ast-core/tests/test_switchmap.rs`:

```rust
use ast_core::serial::Channel;

#[test]
fn switchmap_serialization_roundtrip() {
    let sw = make_1d_switchmap();

    let serialized = Channel::dump_to_string(&sw).unwrap();
    let loaded: SwitchMap = Channel::load_from_string(&serialized).unwrap();

    assert_eq!(loaded.nroute(), 2);
    assert_eq!(loaded.nin(), 1);
    assert_eq!(loaded.nout(), 1);

    // Functional equivalence.
    let pts = PointSet::from_rows([[5.0], [25.0], [15.0]]);
    let out1 = sw.transform(&pts, true).unwrap();
    let out2 = loaded.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert_eq!(out1.point(i)[0], out2.point(i)[0]);
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p ast-core --test test_switchmap 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/serial/mod.rs crates/ast-core/tests/test_switchmap.rs
git commit -m "feat(serial): add SwitchMap serialization and dispatch"
```

---

### Task 7: Extend remove_regions() for SwitchMap

**Files:**
- Modify: `crates/ast-core/src/regions/mod.rs`

- [ ] **Step 1: Add SwitchMap handling to remove_regions()**

In `crates/ast-core/src/regions/mod.rs`, add after the `CmpMap` handling block and before the final `mapping.clone_box()` line:

```rust
if let Some(sw) = mapping.as_any().downcast_ref::<crate::mappings::SwitchMap>() {
    let new_fs = sw.forward_selector().map(|s| remove_regions(s));
    let new_is = sw.inverse_selector().map(|s| remove_regions(s));
    let new_routes: Vec<DynMapping> = sw.route_maps().iter().map(|r| remove_regions(r.as_ref())).collect();

    // Check if anything changed.
    let fs_changed = match (sw.forward_selector(), &new_fs) {
        (Some(orig), Some(new)) => !new.equal(orig),
        _ => false,
    };
    let is_changed = match (sw.inverse_selector(), &new_is) {
        (Some(orig), Some(new)) => !new.equal(orig),
        _ => false,
    };
    let routes_changed = sw.route_maps().iter().zip(new_routes.iter())
        .any(|(orig, new)| !new.equal(orig.as_ref()));

    if fs_changed || is_changed || routes_changed {
        let mut new_sw = crate::mappings::SwitchMap::new(new_fs, new_is, new_routes)
            .expect("remove_regions: SwitchMap reconstruction failed");
        if mapping.is_inverted() {
            new_sw.invert();
        }
        return Box::new(new_sw);
    }

    return mapping.clone_box();
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p ast-core 2>&1 | head -20`
Expected: Successful build.

- [ ] **Step 3: Add test for remove_regions with SwitchMap**

Add to `crates/ast-core/tests/test_switchmap.rs`:

```rust
use ast_core::regions::remove_regions;

#[test]
fn switchmap_remove_regions() {
    let sw = make_1d_switchmap();
    let cleaned = remove_regions(&sw);
    // The selector was a SelectorMap containing Regions.
    // After remove_regions, those Regions become UnitMaps, so the selector
    // changes. The result should still be a valid mapping.
    assert_eq!(cleaned.nin(), 1);
    assert_eq!(cleaned.nout(), 1);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_switchmap -- switchmap_remove 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/regions/mod.rs crates/ast-core/tests/test_switchmap.rs
git commit -m "feat(regions): extend remove_regions to handle SwitchMap"
```

---

### Task 8: Copy simplify fixtures and add fixture-based tests

**Files:**
- Create: `crates/ast-core/tests/fixtures/selectormap_inverse_cancel.map`
- Create: `crates/ast-core/tests/fixtures/selectormap_inverse_cancel.simp`
- Create: `crates/ast-core/tests/fixtures/switchmap_internal_simplify.map`
- Create: `crates/ast-core/tests/fixtures/switchmap_internal_simplify.simp`
- Create: `crates/ast-core/tests/fixtures/switchmap_inverse_cancel.map`
- Create: `crates/ast-core/tests/fixtures/switchmap_inverse_cancel.simp`
- Create: `crates/ast-core/tests/fixtures/switchmap_invert_normalize.map`
- Create: `crates/ast-core/tests/fixtures/switchmap_invert_normalize.simp`
- Modify: `crates/ast-core/tests/test_selectormap.rs`
- Modify: `crates/ast-core/tests/test_switchmap.rs`

- [ ] **Step 1: Copy fixture files from reference**

```bash
cp reference/starlink-ast/ast_tester/simplify_fixtures/selectormap_inverse_cancel.map crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/selectormap_inverse_cancel.simp crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/switchmap_internal_simplify.map crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/switchmap_internal_simplify.simp crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/switchmap_inverse_cancel.map crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/switchmap_inverse_cancel.simp crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/switchmap_invert_normalize.map crates/ast-core/tests/fixtures/
cp reference/starlink-ast/ast_tester/simplify_fixtures/switchmap_invert_normalize.simp crates/ast-core/tests/fixtures/
```

- [ ] **Step 2: Add simplify fixture tests to test_selectormap.rs**

```rust
#[test]
fn selectormap_simplify_inverse_cancel() {
    let input = std::fs::read_to_string("tests/fixtures/selectormap_inverse_cancel.map").unwrap();
    let expected = std::fs::read_to_string("tests/fixtures/selectormap_inverse_cancel.simp").unwrap();

    let mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&input).unwrap();
    let simplified = mapping.simplify().unwrap();
    let result = Channel::dump_to_string(simplified.as_ref()).unwrap();

    let expected_mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&expected).unwrap();
    assert!(simplified.equal(expected_mapping.as_ref()));
}
```

- [ ] **Step 3: Add simplify fixture tests to test_switchmap.rs**

```rust
#[test]
fn switchmap_simplify_internal() {
    let input = std::fs::read_to_string("tests/fixtures/switchmap_internal_simplify.map").unwrap();
    let expected = std::fs::read_to_string("tests/fixtures/switchmap_internal_simplify.simp").unwrap();

    let mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&input).unwrap();
    let simplified = mapping.simplify().unwrap();

    let expected_mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&expected).unwrap();
    assert!(simplified.equal(expected_mapping.as_ref()));
}

#[test]
fn switchmap_simplify_inverse_cancel() {
    let input = std::fs::read_to_string("tests/fixtures/switchmap_inverse_cancel.map").unwrap();
    let expected = std::fs::read_to_string("tests/fixtures/switchmap_inverse_cancel.simp").unwrap();

    let mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&input).unwrap();
    let simplified = mapping.simplify().unwrap();

    let expected_mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&expected).unwrap();
    assert!(simplified.equal(expected_mapping.as_ref()));
}

#[test]
fn switchmap_simplify_invert_normalize() {
    let input = std::fs::read_to_string("tests/fixtures/switchmap_invert_normalize.map").unwrap();
    let expected = std::fs::read_to_string("tests/fixtures/switchmap_invert_normalize.simp").unwrap();

    let mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&input).unwrap();
    let simplified = mapping.simplify().unwrap();

    let expected_mapping: Box<dyn Mapping> = Channel::load_mapping_from_string(&expected).unwrap();
    assert!(simplified.equal(expected_mapping.as_ref()));
}
```

- [ ] **Step 4: Run fixture tests**

Run: `cargo test -p ast-core --test test_selectormap --test test_switchmap -- simplify 2>&1 | tail -20`
Expected: All tests pass. If fixtures use classes not yet wired into deserialization, fix the dispatch entries.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/tests/fixtures/selectormap_* crates/ast-core/tests/fixtures/switchmap_*
git add crates/ast-core/tests/test_selectormap.rs crates/ast-core/tests/test_switchmap.rs
git commit -m "test(simplify): add SelectorMap and SwitchMap simplify fixture tests"
```

---

### Task 9: Add fingerprint support to simplify.rs

**Files:**
- Modify: `crates/ast-core/src/mappings/simplify.rs`

- [ ] **Step 1: Add SwitchMap fingerprint to mapping_fingerprint()**

In `crates/ast-core/src/mappings/simplify.rs`, add after the `TranMap` fingerprint block:

```rust
use super::switchmap::SwitchMap;
use super::selectormap::SelectorMap;
```

And in the `mapping_fingerprint` function body, add:

```rust
if let Some(sw) = mapping.as_any().downcast_ref::<SwitchMap>() {
    sw.nroute().hash(&mut hasher);
    if let Some(fs) = sw.forward_selector() {
        mapping_fingerprint(fs).hash(&mut hasher);
    }
    if let Some(is) = sw.inverse_selector() {
        mapping_fingerprint(is).hash(&mut hasher);
    }
    for rm in sw.route_maps() {
        mapping_fingerprint(rm.as_ref()).hash(&mut hasher);
    }
}
if let Some(sel) = mapping.as_any().downcast_ref::<SelectorMap>() {
    sel.regions().len().hash(&mut hasher);
}
```

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test -p ast-core 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/src/mappings/simplify.rs
git commit -m "feat(simplify): add SwitchMap/SelectorMap fingerprinting for recursion guard"
```

---

### Task 10: Final integration — cargo fmt, clippy, full test suite

**Files:**
- All modified files

- [ ] **Step 1: Run cargo fmt**

Run: `cargo fmt --all`

- [ ] **Step 2: Run cargo clippy**

Run: `cargo clippy -p ast-core -- -D warnings 2>&1 | tail -20`
Expected: No errors. Fix any warnings.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p ast-core 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 4: Commit any formatting/clippy fixes**

```bash
git add -u
git commit -m "style: apply fmt and clippy fixes for SelectorMap/SwitchMap"
```

- [ ] **Step 5: Verify clean build one final time**

Run: `cargo build -p ast-core && cargo test -p ast-core --test test_selectormap --test test_switchmap 2>&1 | tail -10`
Expected: Build clean, all new tests pass.
