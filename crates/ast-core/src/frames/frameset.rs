//! The FrameSet: a graph of Frames connected by Mappings.

use std::collections::VecDeque;
use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{AsAny, CmpMap, Composition, DynMapping, Mapping, UnitMap};
use crate::serial::{
    FrameToNode, MappingToNode, Node, Serializable, frame_from_node, mapping_from_node,
};

use super::{DynFrame, Frame};

/// Identifies a frame within a [`FrameSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRef {
    /// The base frame.
    Base,
    /// The current frame.
    Current,
    /// A 1-based frame index.
    Index(usize),
}

/// A directed edge in the FrameSet graph.
struct Edge {
    /// Source node (0-based index into `frames`).
    from: usize,
    /// Destination node (0-based index into `frames`).
    to: usize,
    /// Mapping that transforms coordinates from `from` to `to`.
    mapping: DynMapping,
}

impl fmt::Debug for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Edge")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish()
    }
}

/// A graph of [`Frame`]s connected by [`Mapping`]s.
///
/// A `FrameSet` stores a collection of frames linked by coordinate
/// transformations. Two distinguished frames — the *base* and the
/// *current* — define the overall transformation: applying the
/// `FrameSet` as a [`Mapping`] converts coordinates from the base
/// frame to the current frame (forward) or vice-versa (inverse).
///
/// Frames are numbered 1-based and can be addressed via [`FrameRef`].
///
/// # Integrity
///
/// When a frame attribute is mutated *through* a FrameSet (via
/// [`with_integrity`](FrameSet::with_integrity)), the FrameSet
/// automatically compensates by composing a correction mapping into
/// all edges that touch the current frame, so that the overall
/// coordinate transformation remains consistent.
///
/// [`frame_mut`](FrameSet::frame_mut) is the escape hatch that
/// bypasses integrity enforcement.
pub struct FrameSet {
    frames: Vec<DynFrame>,
    edges: Vec<Edge>,
    base_idx: usize,
    current_idx: usize,
    variants: Vec<(String, DynMapping)>,
}

impl fmt::Debug for FrameSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameSet")
            .field("nframe", &self.frames.len())
            .field("nedge", &self.edges.len())
            .field("base_idx", &self.base_idx)
            .field("current_idx", &self.current_idx)
            .field("nvariants", &self.variants.len())
            .finish()
    }
}

impl FrameSet {
    // ------------------------------------------------------------------ //
    // Construction                                                        //
    // ------------------------------------------------------------------ //

    /// Create a new `FrameSet` containing a single frame.
    ///
    /// The frame serves as both the base and the current frame.
    pub fn new(base: DynFrame) -> Self {
        Self {
            frames: vec![base],
            edges: Vec::new(),
            base_idx: 0,
            current_idx: 0,
            variants: Vec::new(),
        }
    }

    // ------------------------------------------------------------------ //
    // Frame access                                                        //
    // ------------------------------------------------------------------ //

    /// Number of frames in this FrameSet.
    pub fn nframe(&self) -> usize {
        self.frames.len()
    }

    /// Return a reference to the base frame.
    pub fn base(&self) -> &dyn Frame {
        self.frames[self.base_idx].as_ref()
    }

    /// Return a reference to the current frame.
    pub fn current(&self) -> &dyn Frame {
        self.frames[self.current_idx].as_ref()
    }

    /// Return a reference to a frame identified by [`FrameRef`].
    pub fn frame(&self, which: FrameRef) -> AstResult<&dyn Frame> {
        let idx = self.resolve(which)?;
        Ok(self.frames[idx].as_ref())
    }

    /// Return a mutable reference to a frame — **escape hatch**.
    ///
    /// Mutations through this reference bypass integrity enforcement.
    /// Use [`with_integrity`](FrameSet::with_integrity) instead when
    /// the FrameSet's internal mappings should be kept consistent.
    pub fn frame_mut(&mut self, which: FrameRef) -> AstResult<&mut dyn Frame> {
        let idx = self.resolve(which)?;
        Ok(self.frames[idx].as_mut())
    }

    // ------------------------------------------------------------------ //
    // Base / current selection                                             //
    // ------------------------------------------------------------------ //

    /// Set the base frame.
    pub fn set_base(&mut self, which: FrameRef) -> AstResult<()> {
        let idx = self.resolve(which)?;
        self.base_idx = idx;
        Ok(())
    }

    /// Set the current frame.
    pub fn set_current(&mut self, which: FrameRef) -> AstResult<()> {
        let idx = self.resolve(which)?;
        self.current_idx = idx;
        Ok(())
    }

    // ------------------------------------------------------------------ //
    // Graph mutation                                                       //
    // ------------------------------------------------------------------ //

    /// Add a new frame to the FrameSet.
    ///
    /// The new frame is connected to `anchor` via `map`. After this call
    /// the newly added frame becomes the current frame.
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
    /// The last remaining frame cannot be removed.
    /// If the removed frame was base or current, those are reset to
    /// the first remaining frame.
    pub fn remove_frame(&mut self, which: FrameRef) -> AstResult<()> {
        if self.frames.len() <= 1 {
            return Err(AstError::new_tracked(
                ErrorCode::Remin,
                "cannot remove the last frame from a FrameSet",
            ));
        }
        let idx = self.resolve(which)?;

        // Remove the frame.
        self.frames.remove(idx);

        // Remove all edges touching the removed frame.
        self.edges.retain(|e| e.from != idx && e.to != idx);

        // Renumber edges: any index > idx must decrease by 1.
        for edge in &mut self.edges {
            if edge.from > idx {
                edge.from -= 1;
            }
            if edge.to > idx {
                edge.to -= 1;
            }
        }

        // Fix base/current indices.
        if self.base_idx == idx {
            self.base_idx = 0;
        } else if self.base_idx > idx {
            self.base_idx -= 1;
        }

        if self.current_idx == idx {
            self.current_idx = 0;
        } else if self.current_idx > idx {
            self.current_idx -= 1;
        }

        Ok(())
    }

    /// Replace the first edge touching a frame.
    ///
    /// This replaces the mapping on the first edge whose `from` or `to`
    /// matches the resolved frame index.
    pub fn remap_frame(&mut self, which: FrameRef, map: DynMapping) -> AstResult<()> {
        let idx = self.resolve(which)?;
        for edge in &mut self.edges {
            if edge.from == idx || edge.to == idx {
                edge.mapping = map;
                return Ok(());
            }
        }
        Err(AstError::new_tracked(
            ErrorCode::Frsin,
            format!("no edge found touching frame {} in FrameSet", idx + 1),
        ))
    }

    // ------------------------------------------------------------------ //
    // Path finding and mapping composition                                //
    // ------------------------------------------------------------------ //

    /// Get the compound mapping between two frames.
    ///
    /// Uses BFS to find the shortest path from `from` to `to` in the
    /// undirected frame graph, then composes the edge mappings along that
    /// path.
    pub fn mapping(&self, from: FrameRef, to: FrameRef) -> AstResult<DynMapping> {
        let from_idx = self.resolve(from)?;
        let to_idx = self.resolve(to)?;
        if from_idx == to_idx {
            let naxes = self.frames[from_idx].naxes();
            return Ok(Box::new(UnitMap::new(naxes)));
        }
        let path = self.find_path(from_idx, to_idx)?;
        self.compose_path(&path)
    }

    // ------------------------------------------------------------------ //
    // Integrity                                                           //
    // ------------------------------------------------------------------ //

    /// Mutate the current frame with integrity enforcement.
    ///
    /// Takes a snapshot of the current frame, applies `f` to mutate it,
    /// then computes the correction mapping via `map_from` and fuses it
    /// into all edges that touch the current frame.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        let snap = self.frames[self.current_idx].snapshot();
        f(self.frames[self.current_idx].as_mut())?;
        let delta = self.frames[self.current_idx].map_from(snap.as_ref())?;
        self.fuse_into_current_edges(delta)?;
        Ok(())
    }

    // ------------------------------------------------------------------ //
    // find_frame (stub)                                                   //
    // ------------------------------------------------------------------ //

    /// Search for a frame matching a template.
    ///
    /// This is a stub that always returns `None`. A full implementation
    /// will match against domain, class, and axis properties.
    pub fn find_frame(
        &self,
        _template: &dyn Frame,
        _domains: &[&str],
    ) -> AstResult<Option<FrameSet>> {
        Ok(None)
    }

    // ------------------------------------------------------------------ //
    // Variants                                                            //
    // ------------------------------------------------------------------ //

    /// Register a variant mapping under a given name.
    ///
    /// Variants provide alternative base-to-current mappings that can
    /// be selected at runtime (e.g., for different calibration epochs).
    pub fn add_variant(&mut self, map: DynMapping, name: &str) -> AstResult<()> {
        self.variants.push((name.to_string(), map));
        Ok(())
    }

    // ================================================================== //
    // Private helpers                                                     //
    // ================================================================== //

    /// Resolve a [`FrameRef`] to a 0-based index into `self.frames`.
    fn resolve(&self, which: FrameRef) -> AstResult<usize> {
        match which {
            FrameRef::Base => Ok(self.base_idx),
            FrameRef::Current => Ok(self.current_idx),
            FrameRef::Index(i) => {
                if i < 1 || i > self.frames.len() {
                    Err(AstError::new_tracked(
                        ErrorCode::Frmin,
                        format!(
                            "frame index {} is out of range 1..={}",
                            i,
                            self.frames.len()
                        ),
                    ))
                } else {
                    Ok(i - 1)
                }
            }
        }
    }

    /// BFS over the undirected frame graph, returning the node path.
    ///
    /// Edges are treated as undirected: `from→to` and `to→from` are both
    /// valid traversal directions. The returned path includes both
    /// endpoints.
    fn find_path(&self, from: usize, to: usize) -> AstResult<Vec<usize>> {
        let n = self.frames.len();
        let mut visited = vec![false; n];
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut queue = VecDeque::new();

        visited[from] = true;
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Reconstruct path.
                let mut path = Vec::new();
                let mut node = to;
                path.push(node);
                while node != from {
                    node = parent[node].unwrap();
                    path.push(node);
                }
                path.reverse();
                return Ok(path);
            }
            for edge in &self.edges {
                let neighbor = if edge.from == current {
                    Some(edge.to)
                } else if edge.to == current {
                    Some(edge.from)
                } else {
                    None
                };
                if let Some(nb) = neighbor {
                    if !visited[nb] {
                        visited[nb] = true;
                        parent[nb] = Some(current);
                        queue.push_back(nb);
                    }
                }
            }
        }

        Err(AstError::new_tracked(
            ErrorCode::Frsin,
            format!(
                "no path found between frames {} and {} in FrameSet",
                from + 1,
                to + 1,
            ),
        ))
    }

    /// Compose edge mappings along a node path into a single mapping.
    fn compose_path(&self, path: &[usize]) -> AstResult<DynMapping> {
        debug_assert!(path.len() >= 2);

        let mut result = self.edge_mapping(path[0], path[1])?;
        for pair in path.windows(2).skip(1) {
            let next = self.edge_mapping(pair[0], pair[1])?;
            result = Box::new(CmpMap::new(result, next, Composition::Series)?);
        }
        Ok(result)
    }

    /// Get the mapping for a single edge between two adjacent nodes.
    ///
    /// If the edge is stored as `from→to`, returns a copy of the mapping.
    /// If the edge is stored as `to→from`, returns an inverted copy.
    fn edge_mapping(&self, from: usize, to: usize) -> AstResult<DynMapping> {
        for edge in &self.edges {
            if edge.from == from && edge.to == to {
                // Forward direction — return a copy via simplified().
                return edge.mapping.simplified();
            }
            if edge.from == to && edge.to == from {
                // Reverse direction — get a copy and invert it.
                let mut m = edge.mapping.simplified()?;
                m.invert();
                return Ok(m);
            }
        }
        Err(AstError::new_tracked(
            ErrorCode::Frsin,
            format!(
                "no edge between frames {} and {} in FrameSet",
                from + 1,
                to + 1,
            ),
        ))
    }

    /// Fuse a delta mapping into all edges touching the current frame.
    ///
    /// For edges arriving at current (`edge.to == current_idx`):
    ///   new = CmpMap(old, delta, Series)
    ///
    /// For edges leaving current (`edge.from == current_idx`):
    ///   new = CmpMap(delta_inv, old, Series)
    ///
    /// Skips if the simplified delta is a `UnitMap` (a class-membership
    /// test mirroring C's `astIsAUnitMap`).
    fn fuse_into_current_edges(&mut self, delta: DynMapping) -> AstResult<()> {
        let delta = delta.simplified()?;
        if delta.as_any().is::<UnitMap>() {
            return Ok(());
        }

        let cur = self.current_idx;
        for edge in &mut self.edges {
            if edge.to == cur {
                // Arriving at current: compose old then delta.
                let old =
                    std::mem::replace(&mut edge.mapping, Box::new(UnitMap::new(1)) as DynMapping);
                let delta_copy = delta.simplified()?;
                edge.mapping = Box::new(CmpMap::new(old, delta_copy, Composition::Series)?);
            } else if edge.from == cur {
                // Leaving current: compose delta_inv then old.
                let old =
                    std::mem::replace(&mut edge.mapping, Box::new(UnitMap::new(1)) as DynMapping);
                let mut delta_inv = delta.simplified()?;
                delta_inv.invert();
                edge.mapping = Box::new(CmpMap::new(delta_inv, old, Composition::Series)?);
            }
        }
        Ok(())
    }
}

// ====================================================================== //
// Frame trait implementation                                              //
// ====================================================================== //

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
        self.current().snapshot()
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.current().map_from(previous)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<FrameSet>() else {
            return false;
        };
        if self.frames.len() != other.frames.len() {
            return false;
        }
        if self.base_idx != other.base_idx || self.current_idx != other.current_idx {
            return false;
        }
        if self.edges.len() != other.edges.len() {
            return false;
        }
        for (a, b) in self.frames.iter().zip(&other.frames) {
            if !a.equal(b.as_ref()) {
                return false;
            }
        }
        for (a, b) in self.edges.iter().zip(&other.edges) {
            if a.from != b.from || a.to != b.to {
                return false;
            }
            if !a.mapping.equal(b.mapping.as_ref()) {
                return false;
            }
        }
        true
    }

    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.current().convert(to, domains)
    }

    fn obs_lat(&self) -> Option<f64> {
        self.current().obs_lat()
    }
    fn obs_lon(&self) -> Option<f64> {
        self.current().obs_lon()
    }
    fn obs_alt(&self) -> Option<f64> {
        self.current().obs_alt()
    }
    fn dtai(&self) -> Option<f64> {
        self.current().dtai()
    }
    fn dut1(&self) -> Option<f64> {
        self.current().dut1()
    }
    fn system_string(&self) -> Option<&str> {
        self.current().system_string()
    }
}

// ====================================================================== //
// Mapping trait implementation                                            //
// ====================================================================== //

impl Mapping for FrameSet {
    fn nin(&self) -> usize {
        self.base().naxes()
    }

    fn nout(&self) -> usize {
        self.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        // Inversion is handled by swapping base/current, so this is
        // always false.
        false
    }

    fn invert(&mut self) {
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

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let map = if forward {
            self.mapping(FrameRef::Base, FrameRef::Current)?
        } else {
            self.mapping(FrameRef::Current, FrameRef::Base)?
        };
        map.transform_into(pts, true, out)
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        let composed = self.mapping(FrameRef::Base, FrameRef::Current)?;
        composed.simplified()
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<FrameSet>() else {
            return false;
        };
        if self.frames.len() != other.frames.len() {
            return false;
        }
        if self.base_idx != other.base_idx || self.current_idx != other.current_idx {
            return false;
        }
        if self.edges.len() != other.edges.len() {
            return false;
        }
        // Compare all frames.
        for (a, b) in self.frames.iter().zip(&other.frames) {
            if !a.equal(b.as_ref()) {
                return false;
            }
        }
        // Compare all edges.
        for (a, b) in self.edges.iter().zip(&other.edges) {
            if a.from != b.from || a.to != b.to {
                return false;
            }
            if !a.mapping.equal(b.mapping.as_ref()) {
                return false;
            }
        }
        true
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(FrameSet {
            frames: self.frames.iter().map(|f| f.snapshot()).collect(),
            edges: self
                .edges
                .iter()
                .map(|e| Edge {
                    from: e.from,
                    to: e.to,
                    mapping: e.mapping.clone(),
                })
                .collect(),
            base_idx: self.base_idx,
            current_idx: self.current_idx,
            variants: self
                .variants
                .iter()
                .map(|(s, m)| (s.clone(), m.clone()))
                .collect(),
        })
    }
}

// ====================================================================== //
// Serialisation                                                          //
// ====================================================================== //

impl Serializable for FrameSet {
    fn class_name(&self) -> &'static str {
        "FrameSet"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Set of inter-related coordinate Frames".to_string());

        // Naxes — current frame's naxes (informational).
        node.push_int(
            "Naxes",
            self.current().naxes() as i64,
            Some("Number of coordinate axes".to_string()),
        );

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate Frame".to_string()));

        // Nframe — number of frames (always).
        let nframe = self.frames.len();
        node.push_int(
            "Nframe",
            nframe as i64,
            Some("Number of Frames in FrameSet".to_string()),
        );

        // Base — 1-based base frame index.
        node.push_int(
            "Base",
            (self.base_idx + 1) as i64,
            Some("Index of base Frame".to_string()),
        );

        // Currnt — 1-based current frame index.
        node.push_int(
            "Currnt",
            (self.current_idx + 1) as i64,
            Some("Index of current Frame".to_string()),
        );

        // Lnk{to+1} = from+1 for each edge (1-based).
        for edge in &self.edges {
            let key = format!("Lnk{}", edge.to + 1);
            let comment = format!(
                "Node {} is derived from node {}",
                edge.to + 1,
                edge.from + 1,
            );
            node.push_int(&key, (edge.from + 1) as i64, Some(comment));
        }

        // Frm{i+1} — each frame as a nested object.
        for (i, frame) in self.frames.iter().enumerate() {
            let key = format!("Frm{}", i + 1);
            let frame_node = FrameToNode::to_node(frame.as_ref())?;
            let comment = format!("Frame number {}", i + 1);
            node.push_object(&key, frame_node, Some(comment));
        }

        // Map{to+1} — each edge mapping as a nested object.
        for edge in &self.edges {
            let key = format!("Map{}", edge.to + 1);
            let map_node = MappingToNode::to_node(edge.mapping.as_ref())?;
            let comment = format!(
                "Mapping between nodes {} and {}",
                edge.from + 1,
                edge.to + 1,
            );
            node.push_object(&key, map_node, Some(comment));
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let nframe = node.get_int("Nframe", 0) as usize;
        if nframe == 0 {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "FrameSet: Nframe is 0 or missing",
            ));
        }

        let base = node.get_int("Base", 1) as usize;
        let current = node.get_int("Currnt", 1) as usize;

        // Read frames.
        let mut frames: Vec<DynFrame> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("Frm{}", i + 1);
            let frame_node = node.get_object(&key).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("FrameSet: missing {key}"))
            })?;
            frames.push(frame_from_node(frame_node)?);
        }

        // Read edges: for frames 1..nframe (0-based: 1..nframe), each
        // has a Lnk and Map entry.
        let mut edges: Vec<Edge> = Vec::with_capacity(nframe.saturating_sub(1));
        for i in 1..nframe {
            let lnk_key = format!("Lnk{}", i + 1);
            let parent = node.get_int(&lnk_key, 0) as usize;
            if parent == 0 {
                return Err(AstError::new(
                    ErrorCode::Lderr,
                    format!("FrameSet: missing or invalid {lnk_key}"),
                ));
            }

            let map_key = format!("Map{}", i + 1);
            let map_node = node.get_object(&map_key).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("FrameSet: missing {map_key}"))
            })?;
            let mapping = mapping_from_node(map_node)?;

            edges.push(Edge {
                from: parent - 1,
                to: i,
                mapping,
            });
        }

        Ok(FrameSet {
            frames,
            edges,
            base_idx: base - 1,
            current_idx: current - 1,
            variants: Vec::new(),
        })
    }
}
