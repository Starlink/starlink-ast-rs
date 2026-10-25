# Step 4: FrameSet — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement FrameSet — the central class that holds Frames in a graph connected by Mappings, provides base→current transformation, and maintains integrity when frame attributes are mutated.

**Architecture:** FrameSet uses a graph (nodes + edges) internally, with BFS path-finding to compose mappings between any two frames. It implements both Frame (delegating to current) and Mapping (base→current transform). A `with_integrity()` method implements the snapshot/map_from/fuse pattern. `convert()` is added to the Frame trait.

**Tech Stack:** Rust 2024 edition. Depends on all Step 2 mappings (UnitMap, CmpMap, Composition) and Step 3 frames (BasicFrame, CmpFrame).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/starlink-ast/src/frames/mod.rs` | Frame trait (add convert method) + re-exports |
| `crates/starlink-ast/src/frames/basic.rs` | BasicFrame (add convert impl) |
| `crates/starlink-ast/src/frames/cmpframe.rs` | CmpFrame (add convert impl) |
| `crates/starlink-ast/src/frames/frameset.rs` | FrameSet + FrameRef + graph internals |
| `crates/starlink-ast/tests/test_frameset.rs` | FrameSet tests (equiv. to testframeset.c) |

---

### Task 1: Add convert() to Frame Trait

**Files:**
- Modify: `crates/starlink-ast/src/frames/mod.rs`
- Modify: `crates/starlink-ast/src/frames/basic.rs`
- Modify: `crates/starlink-ast/src/frames/cmpframe.rs`

This task adds `convert()` to the Frame trait. Since FrameSet doesn't exist yet,
the BasicFrame and CmpFrame implementations will construct a minimal FrameSet-like
response. We'll use a forward declaration approach: `convert()` returns
`AstResult<Option<ConvertResult>>` where `ConvertResult` is a simple struct holding
the mapping. We'll change this to return `FrameSet` once FrameSet exists.

Actually, the cleanest approach: add `convert()` to the trait now but have it
return `AstResult<Option<DynMapping>>` (just the mapping between frames).
FrameSet wraps this later. No — the spec says it returns `Option<FrameSet>`.
We need FrameSet to exist first.

**Revised approach:** Skip this task. Build FrameSet first (Tasks 2-5), then
add `convert()` to the trait in Task 6 once FrameSet exists.

---

### Task 1: FrameSet Core Structure and Basic Operations

**Files:**
- Create: `crates/starlink-ast/src/frames/frameset.rs`
- Modify: `crates/starlink-ast/src/frames/mod.rs`

- [ ] **Step 1: Create frameset.rs with struct, FrameRef, and basic operations**

Create `crates/starlink-ast/src/frames/frameset.rs`:

```rust
//! FrameSet — a set of Frames connected by Mappings.

use std::collections::VecDeque;
use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, UnitMap};

use super::{DynFrame, Frame};

/// Reference to a frame within a FrameSet.
#[derive(Debug, Clone, Copy)]
pub enum FrameRef {
    /// The base frame.
    Base,
    /// The current frame.
    Current,
    /// A frame by 1-based index.
    Index(usize),
}

struct Edge {
    from: usize,
    to: usize,
    mapping: DynMapping,
}

/// A set of Frames connected by Mappings in a graph structure.
///
/// FrameSet holds frames as nodes and mappings as edges. It provides
/// the base→current coordinate transformation and maintains integrity
/// when frame attributes are mutated.
///
/// FrameSet implements both [`Frame`] (delegating to the current frame)
/// and [`Mapping`] (providing the base→current transformation).
pub struct FrameSet {
    frames: Vec<DynFrame>,
    edges: Vec<Edge>,
    base_idx: usize,
    current_idx: usize,
    variants: Vec<(String, DynMapping)>,
}

impl FrameSet {
    /// Create a new FrameSet with a single base frame.
    ///
    /// The base frame is also the current frame initially.
    pub fn new(base: DynFrame) -> Self {
        Self {
            frames: vec![base],
            edges: Vec::new(),
            base_idx: 0,
            current_idx: 0,
            variants: Vec::new(),
        }
    }

    /// Number of frames in the FrameSet.
    pub fn nframe(&self) -> usize {
        self.frames.len()
    }

    /// Access the base frame.
    pub fn base(&self) -> &dyn Frame {
        &*self.frames[self.base_idx]
    }

    /// Access the current frame.
    pub fn current(&self) -> &dyn Frame {
        &*self.frames[self.current_idx]
    }

    /// Resolve a FrameRef to a 0-based internal index.
    fn resolve(&self, which: FrameRef) -> AstResult<usize> {
        match which {
            FrameRef::Base => Ok(self.base_idx),
            FrameRef::Current => Ok(self.current_idx),
            FrameRef::Index(n) => {
                if n == 0 || n > self.frames.len() {
                    Err(AstError::new(
                        ErrorCode::Frmin,
                        format!(
                            "frame index {n} is invalid (FrameSet has {} frames)",
                            self.frames.len()
                        ),
                    ))
                } else {
                    Ok(n - 1)
                }
            }
        }
    }

    /// Access a frame by reference.
    pub fn frame(&self, which: FrameRef) -> AstResult<&dyn Frame> {
        let idx = self.resolve(which)?;
        Ok(&*self.frames[idx])
    }

    /// Mutable access to a frame WITHOUT integrity propagation.
    ///
    /// This is the escape hatch for advanced users who need to mutate a
    /// frame directly. Changes made through this reference do NOT trigger
    /// the integrity mechanism — internal mappings are not updated.
    pub fn frame_mut(&mut self, which: FrameRef) -> AstResult<&mut dyn Frame> {
        let idx = self.resolve(which)?;
        Ok(&mut *self.frames[idx])
    }

    /// Change which frame is the base.
    pub fn set_base(&mut self, which: FrameRef) -> AstResult<()> {
        let idx = self.resolve(which)?;
        self.base_idx = idx;
        Ok(())
    }

    /// Change which frame is the current.
    pub fn set_current(&mut self, which: FrameRef) -> AstResult<()> {
        let idx = self.resolve(which)?;
        self.current_idx = idx;
        Ok(())
    }

    /// Add a frame connected to `anchor` via `map`.
    ///
    /// The new frame becomes the current frame. The mapping transforms
    /// coordinates from `anchor` to `new_frame`.
    pub fn add_frame(
        &mut self,
        anchor: FrameRef,
        map: DynMapping,
        new_frame: DynFrame,
    ) -> AstResult<()> {
        let anchor_idx = self.resolve(anchor)?;
        let new_idx = self.frames.len();
        self.frames.push(new_frame);
        self.edges.push(Edge {
            from: anchor_idx,
            to: new_idx,
            mapping: map,
        });
        self.current_idx = new_idx;
        Ok(())
    }

    /// Remove a frame from the FrameSet.
    ///
    /// Cannot remove the last frame. If the removed frame is base or current,
    /// those pointers are adjusted.
    pub fn remove_frame(&mut self, which: FrameRef) -> AstResult<()> {
        if self.frames.len() <= 1 {
            return Err(AstError::new(
                ErrorCode::Remin,
                "cannot remove the last frame from a FrameSet",
            ));
        }
        let idx = self.resolve(which)?;

        // Remove edges connected to this frame
        self.edges.retain(|e| e.from != idx && e.to != idx);

        // Remove the frame
        self.frames.remove(idx);

        // Adjust edge indices for frames after the removed one
        for edge in &mut self.edges {
            if edge.from > idx {
                edge.from -= 1;
            }
            if edge.to > idx {
                edge.to -= 1;
            }
        }

        // Adjust base and current indices
        if self.base_idx == idx {
            self.base_idx = 0;
        } else if self.base_idx > idx {
            self.base_idx -= 1;
        }
        if self.current_idx == idx {
            self.current_idx = self.frames.len() - 1;
        } else if self.current_idx > idx {
            self.current_idx -= 1;
        }

        Ok(())
    }

    /// Replace the mapping on the first edge connecting to `which`.
    pub fn remap_frame(&mut self, which: FrameRef, map: DynMapping) -> AstResult<()> {
        let idx = self.resolve(which)?;
        for edge in &mut self.edges {
            if edge.to == idx || edge.from == idx {
                edge.mapping = map;
                return Ok(());
            }
        }
        Err(AstError::new(
            ErrorCode::Frmin,
            "frame has no connected edges to remap",
        ))
    }

    /// Get the mapping between two frames by composing the path.
    ///
    /// Uses BFS to find the shortest path in the graph, then composes
    /// the mappings along the path (inverting edges traversed backwards).
    pub fn mapping(&self, from: FrameRef, to: FrameRef) -> AstResult<DynMapping> {
        let from_idx = self.resolve(from)?;
        let to_idx = self.resolve(to)?;

        if from_idx == to_idx {
            return Ok(Box::new(UnitMap::new(self.frames[from_idx].naxes())));
        }

        // BFS to find path
        let path = self.find_path(from_idx, to_idx)?;

        // Compose mappings along path
        self.compose_path(&path)
    }

    /// Find the shortest path between two nodes using BFS.
    /// Returns a list of node indices from `from` to `to`.
    fn find_path(&self, from: usize, to: usize) -> AstResult<Vec<usize>> {
        let n = self.frames.len();
        let mut visited = vec![false; n];
        let mut parent = vec![None; n];
        let mut queue = VecDeque::new();

        visited[from] = true;
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Reconstruct path
                let mut path = Vec::new();
                let mut node = to;
                while node != from {
                    path.push(node);
                    node = parent[node].unwrap();
                }
                path.push(from);
                path.reverse();
                return Ok(path);
            }

            // Find neighbors
            for edge in &self.edges {
                let neighbor = if edge.from == current {
                    edge.to
                } else if edge.to == current {
                    edge.from
                } else {
                    continue;
                };
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    parent[neighbor] = Some(current);
                    queue.push_back(neighbor);
                }
            }
        }

        Err(AstError::new(
            ErrorCode::Frsin,
            "no path found between frames in FrameSet",
        ))
    }

    /// Compose mappings along a path of node indices.
    fn compose_path(&self, path: &[usize]) -> AstResult<DynMapping> {
        if path.len() < 2 {
            return Ok(Box::new(UnitMap::new(self.frames[path[0]].naxes())));
        }

        let mut result: DynMapping = self.edge_mapping(path[0], path[1])?;

        for i in 2..path.len() {
            let step = self.edge_mapping(path[i - 1], path[i])?;
            result = Box::new(CmpMap::new(result, step, Composition::Series)?);
        }

        Ok(result)
    }

    /// Get the mapping for a single edge between two adjacent nodes.
    /// If the edge is stored from→to, return it directly.
    /// If stored to→from, return an inverted copy.
    fn edge_mapping(&self, from: usize, to: usize) -> AstResult<DynMapping> {
        for edge in &self.edges {
            if edge.from == from && edge.to == to {
                // Forward direction — clone the mapping
                // Since DynMapping doesn't impl Clone, we use simplified() as a clone proxy
                return edge.mapping.simplified();
            }
            if edge.from == to && edge.to == from {
                // Backward direction — need to invert
                let mut inv = edge.mapping.simplified()?;
                inv.invert();
                return Ok(inv);
            }
        }
        Err(AstError::new(
            ErrorCode::Frsin,
            format!("no edge between nodes {from} and {to}"),
        ))
    }

    /// Apply a mutation to the current frame with integrity propagation.
    ///
    /// Takes a closure that mutates the current frame. After the closure
    /// runs, computes the delta mapping via `map_from` and fuses it into
    /// all edges connecting to the current frame.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        let previous = self.frames[self.current_idx].snapshot();
        f(&mut *self.frames[self.current_idx])?;
        let delta = self.frames[self.current_idx].map_from(&*previous)?;
        self.fuse_into_current_edges(delta)
    }

    /// Fuse a delta mapping into all edges connecting to the current frame.
    ///
    /// For edges where current is the `to` node: compose as `old_map then delta`
    /// For edges where current is the `from` node: compose as `delta_inv then old_map`
    fn fuse_into_current_edges(&mut self, delta: DynMapping) -> AstResult<()> {
        let cur = self.current_idx;

        // Check if delta is effectively identity (UnitMap) — skip if so
        let nin = delta.nin();
        let nout = delta.nout();
        if nin == nout {
            let test_pt = PointSet::from_rows([vec![1.0; nin]]);
            let out = delta.transform(&test_pt, true)?;
            if out.point(0) == test_pt.point(0) {
                return Ok(());
            }
        }

        // Build replacement edges
        let mut new_edges = Vec::new();
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.to == cur {
                // old_map goes neighbor→current, delta goes old_current→new_current
                // new_map = old_map then delta
                let old = edge.mapping.simplified()?;
                let delta_clone = delta.simplified()?;
                let composed = CmpMap::new(old, delta_clone, Composition::Series)?;
                new_edges.push((i, Box::new(composed) as DynMapping));
            } else if edge.from == cur {
                // old_map goes current→neighbor
                // new_map = delta_inv then old_map
                let old = edge.mapping.simplified()?;
                let mut delta_inv = delta.simplified()?;
                delta_inv.invert();
                let composed = CmpMap::new(delta_inv, old, Composition::Series)?;
                new_edges.push((i, Box::new(composed) as DynMapping));
            }
        }

        for (idx, new_map) in new_edges {
            self.edges[idx].mapping = new_map;
        }

        Ok(())
    }

    /// Find a frame matching by domain.
    ///
    /// `domains` is a priority-ordered list. Tries each domain in order,
    /// returns a FrameSet connecting the found frame to itself if found.
    pub fn find_frame(
        &self,
        _template: &dyn Frame,
        domains: &[&str],
    ) -> AstResult<Option<FrameSet>> {
        for &domain in domains {
            for (i, frame) in self.frames.iter().enumerate() {
                // Check if frame's domain matches — we need domain() on Frame trait
                // For now, skip domain matching since it's not on the trait
                // This will be implemented properly when domain is accessible
                let _ = (i, frame, domain);
            }
        }
        Ok(None)
    }

    /// Add a named variant mapping.
    pub fn add_variant(&mut self, map: DynMapping, name: &str) -> AstResult<()> {
        self.variants.push((name.to_string(), map));
        Ok(())
    }
}

impl fmt::Debug for FrameSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameSet")
            .field("nframe", &self.frames.len())
            .field("base_idx", &self.base_idx)
            .field("current_idx", &self.current_idx)
            .finish()
    }
}
```

- [ ] **Step 2: Update frames/mod.rs**

Add to `crates/starlink-ast/src/frames/mod.rs`:

```rust
mod frameset;
pub use frameset::{FrameRef, FrameSet};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success (FrameSet doesn't impl Frame or Mapping yet, that's next)

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/src/frames/
git commit -m "Add FrameSet core: graph storage, path finding, add_frame, integrity"
```

---

### Task 2: Frame and Mapping Trait Impls for FrameSet

**Files:**
- Modify: `crates/starlink-ast/src/frames/frameset.rs`

- [ ] **Step 1: Add Frame impl for FrameSet**

Add to `frameset.rs`:

```rust
impl Frame for FrameSet {
    fn naxes(&self) -> usize {
        self.current().naxes()
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.current().format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.current().unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.current().distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.current().angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.current().offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.current().norm(coords)
    }

    fn snapshot(&self) -> DynFrame {
        // Clone the entire FrameSet — expensive but correct
        // For now, just snapshot the current frame
        self.current().snapshot()
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.current().map_from(previous)
    }
}
```

- [ ] **Step 2: Add Mapping impl for FrameSet**

Add to `frameset.rs`:

```rust
impl Mapping for FrameSet {
    fn nin(&self) -> usize {
        self.base().naxes()
    }

    fn nout(&self) -> usize {
        self.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert(&mut self) {
        // Swap base and current
        std::mem::swap(&mut self.base_idx, &mut self.current_idx);
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let map = if forward {
            self.mapping(FrameRef::Base, FrameRef::Current)?
        } else {
            self.mapping(FrameRef::Current, FrameRef::Base)?
        };
        map.transform(pts, true)
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
        self.mapping(FrameRef::Base, FrameRef::Current)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p starlink-ast`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/starlink-ast/src/frames/frameset.rs
git commit -m "Add Frame and Mapping trait impls for FrameSet"
```

---

### Task 3: Add convert() to Frame Trait

**Files:**
- Modify: `crates/starlink-ast/src/frames/mod.rs`
- Modify: `crates/starlink-ast/src/frames/basic.rs`
- Modify: `crates/starlink-ast/src/frames/cmpframe.rs`
- Modify: `crates/starlink-ast/src/frames/frameset.rs`
- Modify: `crates/starlink-ast/tests/traits_compile.rs`

- [ ] **Step 1: Add convert() to Frame trait**

In `crates/starlink-ast/src/frames/mod.rs`, add to the trait:

```rust
    /// Find a mapping to convert coordinates from this frame to `to`.
    ///
    /// `domains` is a priority-ordered list of domains to search.
    /// Returns a FrameSet connecting self to `to` if a conversion is found,
    /// or `None` if no conversion is possible.
    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>>;
```

- [ ] **Step 2: Implement convert() on BasicFrame**

In `crates/starlink-ast/src/frames/basic.rs`, add to the `impl Frame for BasicFrame`:

```rust
fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
    if self.naxes() == to.naxes() {
        let mut fs = FrameSet::new(self.snapshot());
        fs.add_frame(
            FrameRef::Base,
            Box::new(UnitMap::new(self.naxes)),
            to.snapshot(),
        )?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}
```

Add necessary imports: `use super::{FrameRef, FrameSet};`

- [ ] **Step 3: Implement convert() on CmpFrame**

In `crates/starlink-ast/src/frames/cmpframe.rs`, add to `impl Frame for CmpFrame`:

```rust
fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
    if self.naxes() == to.naxes() {
        let mut fs = FrameSet::new(self.snapshot());
        fs.add_frame(
            FrameRef::Base,
            Box::new(UnitMap::new(self.naxes())),
            to.snapshot(),
        )?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}
```

Add necessary imports: `use super::{FrameRef, FrameSet};`

- [ ] **Step 4: Implement convert() on FrameSet**

In `crates/starlink-ast/src/frames/frameset.rs`, add to `impl Frame for FrameSet`:

```rust
fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
    self.current().convert(to, domains)
}
```

- [ ] **Step 5: Update traits_compile.rs**

Add `convert()` to the DummyRegion's Frame impl in `crates/starlink-ast/tests/traits_compile.rs`:

```rust
fn convert(&self, _to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<starlink_ast::frames::FrameSet>> {
    Ok(None)
}
```

- [ ] **Step 6: Verify it compiles and tests pass**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/starlink-ast/src/frames/ crates/starlink-ast/tests/traits_compile.rs
git commit -m "Add convert() to Frame trait; implement for BasicFrame and CmpFrame"
```

---

### Task 4: FrameSet Tests

**Files:**
- Create: `crates/starlink-ast/tests/test_frameset.rs`

- [ ] **Step 1: Write comprehensive FrameSet tests**

Create `crates/starlink-ast/tests/test_frameset.rs`:

```rust
//! Rust equivalent of reference/starlink-ast/ast_tester/testframeset.c
//! Covers: construction, add_frame, mapping, Frame/Mapping delegation,
//! integrity, remove_frame, remap_frame, set_base/set_current, convert.

use starlink_ast::coords::PointSet;
use starlink_ast::frames::{BasicFrame, Frame, FrameRef, FrameSet};
use starlink_ast::mappings::{DynMapping, Mapping, ShiftMap, UnitMap, ZoomMap};

fn make_frame(naxes: usize, domain: &str) -> Box<dyn Frame> {
    let mut f = BasicFrame::builder(naxes).build().unwrap();
    f.set_domain(domain);
    Box::new(f)
}

#[test]
fn new_frameset_single_frame() {
    let fs = FrameSet::new(make_frame(2, "PIXEL"));
    assert_eq!(fs.nframe(), 1);
    assert_eq!(fs.base().naxes(), 2);
    assert_eq!(fs.current().naxes(), 2);
}

#[test]
fn add_frame_builds_chain() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let map: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, map, make_frame(2, "GRID")).unwrap();
    assert_eq!(fs.nframe(), 2);
}

#[test]
fn mapping_base_to_current() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let map: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, map, make_frame(2, "GRID")).unwrap();

    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 6.0]);
}

#[test]
fn mapping_current_to_base() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let map: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, map, make_frame(2, "GRID")).unwrap();

    let m = fs.mapping(FrameRef::Current, FrameRef::Base).unwrap();
    let pts = PointSet::from_rows([[3.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 2.0).abs() < 1e-10);
}

#[test]
fn mapping_chain_composes() {
    let mut fs = FrameSet::new(make_frame(2, "A"));
    // A --zoom(2)--> B --shift(10,20)--> C
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, make_frame(2, "B")).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    fs.add_frame(FrameRef::Current, s, make_frame(2, "C")).unwrap();

    assert_eq!(fs.nframe(), 3);
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    // zoom: (2,4), then shift: (12, 24)
    assert_eq!(out.point(0), &[12.0, 24.0]);
}

#[test]
fn frameset_as_mapping() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 5.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "SKY")).unwrap();

    assert_eq!(fs.nin(), 2);
    assert_eq!(fs.nout(), 2);
    let pts = PointSet::from_rows([[2.0, 3.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[10.0, 15.0]);
}

#[test]
fn frameset_as_mapping_inverse() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 5.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "SKY")).unwrap();

    let pts = PointSet::from_rows([[10.0, 15.0]]);
    let out = fs.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-10);
}

#[test]
fn frame_delegation() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "SKY")).unwrap();

    // Frame methods delegate to current
    assert_eq!(fs.naxes(), 2);
    let d = fs.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

#[test]
fn set_current_changes_delegation() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(3, "GRID")).unwrap();

    assert_eq!(fs.naxes(), 3); // current is 3-axis GRID
    fs.set_current(FrameRef::Index(1)).unwrap();
    assert_eq!(fs.naxes(), 2); // now current is 2-axis PIXEL
}

#[test]
fn frame_mut_escape_hatch() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "SKY")).unwrap();

    // Mutate current frame directly (no integrity)
    let frame = fs.frame_mut(FrameRef::Current).unwrap();
    assert_eq!(frame.naxes(), 2);
}

#[test]
fn with_integrity() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "SKY")).unwrap();

    // Integrity mutation (BasicFrame returns UnitMap, so no change)
    fs.with_integrity(|_frame| Ok(())).unwrap();

    // Mapping should still work correctly
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[2.0, 4.0]);
}

#[test]
fn remove_frame() {
    let mut fs = FrameSet::new(make_frame(2, "A"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, make_frame(2, "B")).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    fs.add_frame(FrameRef::Current, s, make_frame(2, "C")).unwrap();

    assert_eq!(fs.nframe(), 3);
    fs.remove_frame(FrameRef::Index(2)).unwrap(); // remove B
    assert_eq!(fs.nframe(), 2);
}

#[test]
fn remove_last_frame_fails() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    assert!(fs.remove_frame(FrameRef::Base).is_err());
}

#[test]
fn remap_frame() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "GRID")).unwrap();

    // Replace the mapping with a shift
    let s: DynMapping = Box::new(ShiftMap::new([5.0, 5.0]));
    fs.remap_frame(FrameRef::Current, s).unwrap();

    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[6.0, 7.0]);
}

#[test]
fn convert_same_naxes() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(2).build().unwrap();
    let result = f1.convert(&f2, &["SKY"]).unwrap();
    assert!(result.is_some());
    let fs = result.unwrap();
    assert_eq!(fs.nframe(), 2);
}

#[test]
fn convert_different_naxes() {
    let f1 = BasicFrame::builder(2).build().unwrap();
    let f2 = BasicFrame::builder(3).build().unwrap();
    let result = f1.convert(&f2, &["SKY"]).unwrap();
    assert!(result.is_none());
}

#[test]
fn tree_topology() {
    // Create a tree: A connected to B and C
    let mut fs = FrameSet::new(make_frame(2, "A"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Index(1), z, make_frame(2, "B")).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 10.0]));
    fs.add_frame(FrameRef::Index(1), s, make_frame(2, "C")).unwrap();

    assert_eq!(fs.nframe(), 3);

    // Map from B (index 2) to C (index 3) should go B→A→C
    let m = fs.mapping(FrameRef::Index(2), FrameRef::Index(3)).unwrap();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    // B→A: divide by 2 → (2,3), A→C: add 10 → (12, 13)
    assert!((out.point(0)[0] - 12.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 13.0).abs() < 1e-10);
}

#[test]
fn mapping_same_frame_is_unit() {
    let fs = FrameSet::new(make_frame(2, "PIXEL"));
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

#[test]
fn add_variant() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_variant(z, "ZOOMED").unwrap();
}

#[test]
fn frameset_invert() {
    let mut fs = FrameSet::new(make_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2, "SKY")).unwrap();

    // After invert, forward should be current→base (divide by 3)
    fs.invert();
    let pts = PointSet::from_rows([[9.0, 12.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 3.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 4.0).abs() < 1e-10);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p starlink-ast --test test_frameset`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/starlink-ast/tests/test_frameset.rs
git commit -m "Add FrameSet tests (equivalent to testframeset.c)"
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

Run: `cargo test -p starlink-ast`
Expected: all tests pass

- [ ] **Step 4: Commit fixes if any**

```bash
git add -u
git commit -m "Fix clippy and fmt issues in FrameSet implementation"
```

---

## Verification Checklist

After all tasks are complete:

1. `cargo build --workspace` — all crates compile
2. `cargo test -p starlink-ast` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. FrameSet graph supports linear chains and tree topologies
6. Path-finding composes mappings correctly (forward and inverse)
7. Frame delegation to current works
8. Mapping (base→current) transform works
9. Integrity mechanism (with_integrity + fuse) is operational
10. convert() returns FrameSet for compatible frames
11. `git status` — clean
