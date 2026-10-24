# Step 4: FrameSet — Design Spec

## Summary

Implement `FrameSet` — the central coordination class that holds multiple
Frames connected by Mappings in a graph structure, provides the
base→current transformation, and maintains integrity when frame attributes
are mutated. Also implement `Frame::convert()` for BasicFrame.

## Deliverables

1. `FrameSet` struct with graph-based internal storage.
2. Full public API: new, add_frame, remove_frame, remap_frame, frame,
   frame_mut, mapping, base, current, nframe, set_base, set_current,
   find_frame, add_variant.
3. `Frame` trait impl with integrity mechanism (snapshot/map_from/fuse).
4. `Mapping` trait impl (base→current transform).
5. `FrameRef` enum for frame addressing.
6. `Frame::convert()` added to the trait; implemented for BasicFrame.
7. FrameSet flattening (when a FrameSet is added as a Frame).
8. Tests equivalent to C's `testframeset.c`.

## Module Structure

```
crates/starlink-ast/src/frames/
├── mod.rs           # Frame trait (updated with convert()) + re-exports
├── basic.rs         # BasicFrame (updated: add convert() impl)
├── cmpframe.rs      # CmpFrame (updated: add convert() impl)
└── frameset.rs      # FrameSet + FrameRef
```

## Internal Storage: Graph

```rust
struct FrameNode {
    frame: DynFrame,
}

struct Edge {
    from: usize,
    to: usize,
    mapping: DynMapping,  // transforms from→to direction
}

pub struct FrameSet {
    nodes: Vec<FrameNode>,
    edges: Vec<Edge>,
    base_idx: usize,
    current_idx: usize,
    variants: Vec<(String, DynMapping)>,
}
```

The graph is undirected (edges can be traversed in both directions by
inverting the mapping). Nodes are indexed 0-based internally; the public
API uses `FrameRef` which is 1-based for `Index`.

### Path Finding

`mapping(from, to)` finds the shortest path between two nodes using BFS,
then composes the mappings along the path (inverting edges traversed
backwards). For the common linear-chain case this is just walking from
`from` to `to` in sequence.

## FrameRef

```rust
#[derive(Debug, Clone, Copy)]
pub enum FrameRef {
    /// The base frame.
    Base,
    /// The current frame.
    Current,
    /// A frame by 1-based index.
    Index(usize),
}
```

`Name` variant deferred until Ident attribute is available.

Resolving a `FrameRef` to an internal 0-based index:
- `Base` → `self.base_idx`
- `Current` → `self.current_idx`
- `Index(n)` → `n - 1` (validated against `nodes.len()`)

## Public API

```rust
impl FrameSet {
    /// Create a new FrameSet with a single base frame.
    pub fn new(base: DynFrame) -> Self;

    /// Add a frame connected to `anchor` via `map`.
    /// The new frame becomes the current frame.
    pub fn add_frame(
        &mut self,
        anchor: FrameRef,
        map: DynMapping,
        new_frame: DynFrame,
    ) -> AstResult<()>;

    /// Remove a frame from the FrameSet.
    /// Cannot remove the last frame.
    pub fn remove_frame(&mut self, which: FrameRef) -> AstResult<()>;

    /// Replace the mapping connecting `which` to its neighbor.
    pub fn remap_frame(&mut self, which: FrameRef, map: DynMapping) -> AstResult<()>;

    /// Access a frame by reference.
    pub fn frame(&self, which: FrameRef) -> AstResult<&dyn Frame>;

    /// Mutable access to a frame WITHOUT integrity propagation (escape hatch).
    pub fn frame_mut(&mut self, which: FrameRef) -> AstResult<&mut dyn Frame>;

    /// Get the mapping between two frames (composing the path).
    pub fn mapping(&self, from: FrameRef, to: FrameRef) -> AstResult<DynMapping>;

    /// Access the base frame.
    pub fn base(&self) -> &dyn Frame;

    /// Access the current frame.
    pub fn current(&self) -> &dyn Frame;

    /// Number of frames.
    pub fn nframe(&self) -> usize;

    /// Change which frame is the base.
    pub fn set_base(&mut self, which: FrameRef) -> AstResult<()>;

    /// Change which frame is the current.
    pub fn set_current(&mut self, which: FrameRef) -> AstResult<()>;

    /// Find a frame matching a template (by domain).
    /// `domains` is a priority-ordered list of domains to try.
    pub fn find_frame(
        &self,
        template: &dyn Frame,
        domains: &[&str],
    ) -> AstResult<Option<FrameSet>>;

    /// Add a named variant mapping (alternative coordinate interpretation).
    pub fn add_variant(&mut self, map: DynMapping, name: &str) -> AstResult<()>;
}
```

## Frame Trait Implementation (Integrity)

FrameSet implements `Frame` by delegating to the current frame for
read-only operations. The trait doesn't include setters (those are on
concrete types), but FrameSet provides its own attribute setters that
trigger integrity:

```rust
impl FrameSet {
    /// Set the epoch on the current frame with integrity propagation.
    pub fn set_epoch(&mut self, e: f64) -> AstResult<()> {
        let previous = self.current().snapshot();
        // Mutate the current frame directly
        self.nodes[self.current_idx].frame.set_epoch(e)?;  // via Any downcast or similar
        let delta = self.current().map_from(&*previous)?;
        self.fuse_into_current_edges(delta)?;
        Ok(())
    }
}
```

Wait — there's a problem. The Frame trait doesn't have `set_epoch()`.
It's on concrete types like BasicFrame. And we can't call concrete
methods through `DynFrame` (Box<dyn Frame>).

**Revised approach**: The integrity mechanism works through the Frame
trait's `snapshot()` and `map_from()`. The actual mutation happens via
`frame_mut()` (the escape hatch), and the caller is responsible for
integrity. OR — FrameSet exposes high-level methods that know how to
trigger integrity for common operations.

Actually, re-reading the spec: "every Frame attribute follows the same
pattern" — but the Frame trait as defined doesn't include setters.
The C library achieves this because it has a generic `astSet` function
that works on any attribute by name. In Rust, without string-typed
attribute access on the trait, we need a different approach.

**The practical approach for Step 4**: Since BasicFrame's `map_from()`
always returns UnitMap (its attribute changes don't affect geometry),
the integrity mechanism is a no-op for the only Frame type we have.
The machinery is built and tested, but produces trivial results.
When SkyFrame lands (Step 5+), its `map_from()` returns real mappings
and integrity becomes meaningful.

For now, we provide:
1. `frame_mut()` — the escape hatch (no integrity)
2. A method `mutate_current_with_integrity(&mut self, f: impl FnOnce(&mut dyn Frame))` that captures the snapshot/map_from/fuse pattern generically.

Actually, the simplest correct approach: **just implement the
snapshot/map_from/fuse pattern as a helper**, and document that callers
should use it when mutating through Frame trait methods. The concrete
FrameSet-level setters can be added as SkyFrame-specific wrappers later.

```rust
impl FrameSet {
    /// Apply a mutation to the current frame with integrity propagation.
    ///
    /// Takes a closure that mutates the current frame. After the closure
    /// runs, computes the delta mapping and fuses it into all edges
    /// connecting to the current frame.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        let previous = self.current().snapshot();
        f(&mut *self.nodes[self.current_idx].frame)?;
        let delta = self.current().map_from(&*previous)?;
        self.fuse_into_current_edges(delta)?;
        Ok(())
    }
}
```

This is the generic integrity mechanism. Concrete attribute-specific
wrappers are syntactic sugar on top.

### fuse_into_current_edges

When a delta mapping is produced by `map_from`, it must be composed
into all edges connecting to the current frame, so that paths through
the current frame remain correct:

For each edge `(current_idx, neighbor)`:
- Old mapping: `M` (current → neighbor)
- New mapping: `delta.inverse() then M` (undo delta first, then apply old mapping)

For each edge `(neighbor, current_idx)`:
- Old mapping: `M` (neighbor → current)
- New mapping: `M then delta` (apply old mapping, then apply delta)

## Mapping Trait Implementation

```rust
impl Mapping for FrameSet {
    fn nin(&self) -> usize { self.base().naxes() }
    fn nout(&self) -> usize { self.current().naxes() }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let map = self.mapping(FrameRef::Base, FrameRef::Current)?;
        map.transform(pts, forward)
    }
    // ... other methods delegate to the composed mapping
}
```

## Frame::convert() Addition

Add `convert()` to the Frame trait:

```rust
pub trait Frame: Send {
    // ... existing methods ...

    /// Find a mapping to convert coordinates from this frame to `to`.
    ///
    /// `domains` is a priority-ordered list of domains to search
    /// (e.g. `&["SKY", "PIXEL", "GRID"]`). Tries each in order; returns
    /// the first match. The CFFI layer splits C's comma-separated string
    /// before calling this.
    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>>;
}
```

### BasicFrame::convert() Implementation

```rust
fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
    // BasicFrame can convert to another BasicFrame with same naxes
    if self.naxes() == to.naxes() {
        let mut fs = FrameSet::new(self.snapshot());
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(self.naxes())), to.snapshot())?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}
```

### CmpFrame::convert() Implementation

```rust
fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
    // Same logic as BasicFrame for now
    if self.naxes() == to.naxes() {
        let mut fs = FrameSet::new(self.snapshot());
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(self.naxes())), to.snapshot())?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}
```

## FrameSet Flattening

When `add_frame` is called with a FrameSet as the `new_frame`:
- Extract all frames and mappings from the inner FrameSet
- Connect the inner's base frame to the anchor via the supplied mapping
- Add all inner frames/edges to the outer graph
- Set current to the inner's current frame

This is the "merge/flatten" behavior described in the spec.

## add_variant

Stores named alternative mappings for the current frame:
```rust
pub fn add_variant(&mut self, map: DynMapping, name: &str) -> AstResult<()> {
    self.variants.push((name.to_string(), map));
    Ok(())
}
```

## find_frame

Basic domain-matching implementation:
```rust
pub fn find_frame(&self, template: &dyn Frame, domain: &str) -> AstResult<Option<FrameSet>> {
    // Search nodes for a frame whose domain matches
    // If found, return a new FrameSet with base=found, connected via the path mapping
}
```

## Testing Strategy

Tests equivalent to `testframeset.c`:
- Construct FrameSet with single frame
- add_frame to build a chain
- mapping(base, current) composes correctly
- set_base/set_current change which frames are used
- Frame trait delegation works (format, distance via current)
- Mapping trait (transform via base→current)
- frame_mut() escape hatch
- with_integrity() triggers fuse
- remove_frame
- remap_frame
- find_frame by domain
- FrameSet flattening

## What This Step Does NOT Include

- SkyFrame (Step 5+) — so integrity produces trivial UnitMap deltas
- FrameRef::Name (needs Ident attribute)
- Serialization
- The full convert() logic for SkyFrame coordinate system conversion
