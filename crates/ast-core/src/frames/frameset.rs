//! The FrameSet: a graph of Frames connected by Mappings.

use std::collections::VecDeque;
use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{AsAny, CmpMap, Composition, DynMapping, Mapping, PermMap, UnitMap};
use crate::serial::{
    FrameToNode, MappingToNode, Node, NodeValue, Serializable, frame_from_node, mapping_from_node,
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

/// A directed edge in the FrameSet node graph.
struct Edge {
    /// Source node (0-based node index).
    from: usize,
    /// Destination node (0-based node index).
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
    /// Total number of nodes in the graph (>= nframe).
    nnode: usize,
    /// Maps each frame (0-based) to its node (0-based).
    /// When `nnode == nframe`, this is the identity mapping.
    node_map: Vec<usize>,
}

impl Clone for FrameSet {
    fn clone(&self) -> Self {
        FrameSet {
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
            nnode: self.nnode,
            node_map: self.node_map.clone(),
        }
    }
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
            nnode: 1,
            node_map: vec![0],
        }
    }

    // ------------------------------------------------------------------ //
    // Frame access                                                        //
    // ------------------------------------------------------------------ //

    /// Number of frames in this FrameSet.
    pub fn nframe(&self) -> usize {
        self.frames.len()
    }

    /// 0-based index of the base frame.
    pub(crate) fn base_idx(&self) -> usize {
        self.base_idx
    }

    /// 0-based index of the current frame.
    pub(crate) fn current_idx(&self) -> usize {
        self.current_idx
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
        let anchor_node = self.node_map[anchor_idx];
        let new_node = self.nnode;
        self.nnode += 1;
        self.frames.push(new_frame);
        self.node_map.push(new_node);
        self.edges.push(Edge {
            from: anchor_node,
            to: new_node,
            mapping: map,
        });
        self.current_idx = self.frames.len() - 1;
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

        // Drop only the frame's *association* with its node — the node
        // itself stays in the graph so conversion paths that route
        // through it survive; `tidy_nodes` then composes the bridged
        // mappings and prunes dead ends. Mirrors C `RemoveFrame`
        // (frameset.c:8740-8800) + `TidyNodes`.
        self.frames.remove(idx);
        self.node_map.remove(idx);

        // Fix base/current indices. C clears the attribute when the
        // removed frame *was* the base/current frame, so the defaults
        // apply: Base defaults to the first frame and Current to the
        // *last* frame (frameset.c:5022/5091). Set values above the
        // removed index are decremented so they keep following the
        // same frame.
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

        self.tidy_nodes()?;
        Ok(())
    }

    /// Modify a Frame's relationship to other Frames in the FrameSet.
    ///
    /// Mirrors C `astRemapFrame` (frameset.c:8486). The supplied `map`
    /// describes how the new (modified) frame relates to the old (pre-
    /// modification) frame: forward direction takes old-frame coords
    /// into new-frame coords. The old node retains its position in the
    /// graph; a new node is created associated with the named frame
    /// (so subsequent mappings touch the new node), with `map` as the
    /// link from old to new. Then [`tidy_nodes`](Self::tidy_nodes) is
    /// run to collapse any redundant nodes left in the graph.
    pub fn remap_frame(&mut self, which: FrameRef, map: DynMapping) -> AstResult<()> {
        let idx = self.resolve(which)?;
        let old_node = self.node_map[idx];

        // Validate map dimensions match the frame's axis count.
        let naxes = self.frames[idx].naxes();
        if map.nin() != naxes || map.nout() != naxes {
            return Err(AstError::new_tracked(
                ErrorCode::Ncpin,
                format!(
                    "remap_frame: map dims {}/{} do not match frame axes {}",
                    map.nin(),
                    map.nout(),
                    naxes,
                ),
            ));
        }

        // Add a new node connected to the old via `map`. The frame
        // moves from old_node to new_node — old_node becomes
        // referenced only by the new edge plus whatever existing
        // edges touched it, so TidyNodes can later prune it.
        let new_node = self.nnode;
        self.nnode += 1;
        self.edges.push(Edge {
            from: old_node,
            to: new_node,
            mapping: map,
        });
        self.node_map[idx] = new_node;

        // Run TidyNodes to remove any node that, after the remap, is
        // referenced by exactly two edges and no frame — combining
        // and simplifying the bridged mapping. Mirrors C's
        // post-RemapFrame `TidyNodes` call (frameset.c:8669).
        self.tidy_nodes()?;
        Ok(())
    }

    /// Remove redundant nodes from the FrameSet's node graph.
    ///
    /// Mirrors C `TidyNodes` (frameset.c:11076-11250). A node is
    /// "redundant" when it has no associated frame and is referenced
    /// by exactly two edges; it joins two mappings without
    /// contributing structurally. We collapse the two edges into a
    /// single edge whose mapping is `(left ; right).simplified()`.
    /// Repeats until no more redundant nodes are found.
    pub(crate) fn tidy_nodes(&mut self) -> AstResult<()> {
        loop {
            // Build a per-node reference count: count how many edges
            // touch each node (either end), and count how many
            // frames are associated with each node.
            let mut frame_count: Vec<usize> = vec![0; self.nnode];
            for &n in &self.node_map {
                frame_count[n] += 1;
            }
            let mut edges_at_node: Vec<Vec<usize>> = vec![Vec::new(); self.nnode];
            for (eidx, edge) in self.edges.iter().enumerate() {
                edges_at_node[edge.from].push(eidx);
                edges_at_node[edge.to].push(eidx);
            }

            // Frameless dead-end (one edge) and isolated (no edge)
            // nodes are simply pruned — C TidyNodes' "unnecessary or
            // dead-end" case.
            let mut pruned = false;
            for node in 0..self.nnode {
                if frame_count[node] != 0 || edges_at_node[node].len() >= 2 {
                    continue;
                }
                if let Some(&eidx) = edges_at_node[node].first() {
                    self.edges.remove(eidx);
                }
                for n in &mut self.node_map {
                    if *n > node {
                        *n -= 1;
                    }
                }
                for edge in &mut self.edges {
                    if edge.from > node {
                        edge.from -= 1;
                    }
                    if edge.to > node {
                        edge.to -= 1;
                    }
                }
                self.nnode -= 1;
                pruned = true;
                break;
            }
            if pruned {
                continue;
            }

            // Find a redundant node.
            let mut found: Option<usize> = None;
            for node in 0..self.nnode {
                if frame_count[node] != 0 {
                    continue;
                }
                if edges_at_node[node].len() == 2 {
                    found = Some(node);
                    break;
                }
            }
            let Some(node) = found else {
                break;
            };

            // The two edges touching the redundant node — exactly
            // one has it as its destination (incoming) and one has
            // it as its source (outgoing).
            let e_indices = edges_at_node[node].clone();
            let e1 = e_indices[0];
            let e2 = e_indices[1];

            // Determine direction: combine into a single edge from
            // some "from" to some "to", inserting an inversion if
            // the original edges' directions don't align with a
            // forward composition.
            let (left_map, left_inv, mid_a) = {
                let edge = &self.edges[e1];
                if edge.to == node {
                    (edge.mapping.clone_box(), false, edge.from)
                } else {
                    // edge.from == node — left half goes from `mid` to `node`,
                    // so we need to invert it to get mid → ... composition.
                    (edge.mapping.clone_box(), true, edge.to)
                }
            };
            let (right_map, right_inv, mid_b) = {
                let edge = &self.edges[e2];
                if edge.from == node {
                    (edge.mapping.clone_box(), false, edge.to)
                } else {
                    (edge.mapping.clone_box(), true, edge.from)
                }
            };

            // The new edge spans mid_a → mid_b. Build the combined
            // mapping (left[, inverted] ; right[, inverted]).
            let mut left_inverted = left_map;
            if left_inv {
                left_inverted.invert();
            }
            let mut right_inverted = right_map;
            if right_inv {
                right_inverted.invert();
            }
            let combined: DynMapping = Box::new(CmpMap::new(
                left_inverted,
                right_inverted,
                Composition::Series,
            )?);
            let simplified = combined.simplified()?;

            // Remove the two edges (highest index first) and insert
            // the combined edge.
            let (a, b) = if e1 > e2 { (e1, e2) } else { (e2, e1) };
            self.edges.remove(a);
            self.edges.remove(b);
            self.edges.push(Edge {
                from: mid_a,
                to: mid_b,
                mapping: simplified,
            });

            // Renumber nodes to close the gap left by the removed
            // node (so `nnode` stays compact and node indices are
            // contiguous).
            for n in &mut self.node_map {
                if *n > node {
                    *n -= 1;
                }
            }
            for edge in &mut self.edges {
                if edge.from > node {
                    edge.from -= 1;
                }
                if edge.to > node {
                    edge.to -= 1;
                }
            }
            self.nnode -= 1;
        }
        Ok(())
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
        let from_node = self.node_map[from_idx];
        let to_node = self.node_map[to_idx];
        let path = self.find_path(from_node, to_node)?;
        self.compose_path(&path)
    }

    // ------------------------------------------------------------------ //
    // Integrity                                                           //
    // ------------------------------------------------------------------ //

    /// Mutate the current frame with integrity enforcement.
    ///
    /// Takes a snapshot of the current frame, applies `f` to mutate it,
    /// then computes the correction mapping via `map_from` and adds a
    /// new node carrying the delta as a fresh edge (mirrors C
    /// `astRemapFrame` + `TidyNodes`, frameset.c:8486 / 11076).
    /// `tidy_nodes` runs at the end to collapse any node that became
    /// redundant — combining and simplifying the bridged mapping in
    /// the process, which is where adjacent inverse pairs (e.g.
    /// Hfk5z+Fk5hz from a SetSystem ICRS → FK5 → ICRS round-trip)
    /// finally cancel.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        let snap = self.frames[self.current_idx].snapshot();
        f(self.frames[self.current_idx].as_mut())?;
        let delta = self.frames[self.current_idx].map_from(snap.as_ref())?;
        let delta = delta.simplified()?;
        if delta.as_any().is::<UnitMap>() {
            return Ok(());
        }
        self.remap_frame(FrameRef::Current, delta)?;
        Ok(())
    }

    // ------------------------------------------------------------------ //
    // find_frame                                                          //
    // ------------------------------------------------------------------ //

    /// Search for a frame in the FrameSet matching a template.
    ///
    /// Mirrors C `FrameSet::FindFrame` (frameset.c:3790) — iterates the
    /// internal frames in priority order (current → base → others),
    /// invokes `Frame::find_frame_template` on each with the supplied
    /// domain field, and prefixes the FrameSet's base→that-frame
    /// Mapping when a match is found. The result is a 2-frame FrameSet
    /// whose base is `self.base()` and current is the matched Frame.
    ///
    /// `domains` is a comma-priority list: each non-empty field is
    /// tried in turn; an empty field permits any domain.
    pub fn find_frame(
        &self,
        template: &dyn Frame,
        domains: &[&str],
    ) -> AstResult<Option<FrameSet>> {
        // Synthesize the comma-separated string domainlist from the
        // slice form. An empty slice → "" (allows any). Any empty
        // entry appears in the synthesised string as adjacent commas
        // (which the canonicaliser reads as "blank field").
        let joined = if domains.is_empty() {
            String::new()
        } else {
            domains.join(",")
        };
        Frame::find_frame_template(self, template, &joined)
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

    /// Permute the axes of the current frame.
    ///
    /// `perm` is a 1-based permutation array. This:
    /// 1. Permutes the current frame's per-axis data
    /// 2. Composes a PermMap onto the base→current edge so coordinate
    ///    values are reordered to match the new axis assignment
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        let naxes = self.current().naxes();
        super::validate_permutation(perm, naxes)?;

        // Permute the current frame's internal axis data.
        self.frames[self.current_idx].perm_axes(perm)?;

        // Build a PermMap that reorders coordinate values.
        // Forward: output[i] = input[perm[i]-1] (reorder to new axis order)
        // The PermMap uses 1-based encoding.
        let outperm: Vec<i64> = perm.iter().map(|&p| p as i64).collect();

        // Inverse: inperm[perm[i]-1] = i+1
        let mut inperm = vec![0i64; naxes];
        for (i, &p) in perm.iter().enumerate() {
            inperm[p - 1] = (i + 1) as i64;
        }

        let pm = PermMap::new(&inperm, &outperm, &[])?;

        // Compose onto the existing edge touching the current frame.
        // The new mapping is: old_mapping then PermMap (forward direction).
        let cur_node = self.node_map[self.current_idx];
        for edge in &mut self.edges {
            if edge.to == cur_node {
                // Edge goes from→current: compose PermMap after existing.
                let combined =
                    CmpMap::new(edge.mapping.clone(), Box::new(pm), Composition::Series)?;
                edge.mapping = Box::new(combined);
                return Ok(());
            }
            if edge.from == cur_node {
                // Edge goes current→to: compose PermMap (inverted) before existing.
                let mut pm_inv = pm;
                pm_inv.invert();
                let combined =
                    CmpMap::new(Box::new(pm_inv), edge.mapping.clone(), Composition::Series)?;
                edge.mapping = Box::new(combined);
                return Ok(());
            }
        }

        // No edge (single-frame FrameSet) — nothing to compose onto.
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

    /// BFS over the undirected node graph, returning the node path.
    ///
    /// Edges are treated as undirected: `from→to` and `to→from` are both
    /// valid traversal directions. The returned path includes both
    /// endpoints. Arguments are 0-based node indices.
    fn find_path(&self, from: usize, to: usize) -> AstResult<Vec<usize>> {
        let n = self.nnode;
        let mut visited = vec![false; n];
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut queue = VecDeque::new();

        visited[from] = true;
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            if current == to {
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
                "no path found between nodes {} and {} in FrameSet",
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

    fn find_frame_template(
        &self,
        template: &dyn Frame,
        domainlist: &str,
    ) -> AstResult<Option<FrameSet>> {
        frameset_find_frame(self, template, domainlist)
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

    // ------------------------------------------------------------------
    // C forwards every remaining Frame virtual to the current frame
    // (frameset.c:6260-6350); without these, trait defaults (constant
    // values) would silently apply.
    // ------------------------------------------------------------------

    fn domain(&self) -> &str {
        self.current().domain()
    }

    fn test_domain(&self) -> bool {
        self.current().test_domain()
    }

    fn ident(&self) -> &str {
        self.current().ident()
    }

    fn min_axes(&self) -> usize {
        self.current().min_axes()
    }

    fn set_min_axes(&mut self, n: usize) {
        self.frames[self.current_idx].set_min_axes(n);
    }

    fn test_min_axes(&self) -> bool {
        self.current().test_min_axes()
    }

    fn max_axes(&self) -> usize {
        self.current().max_axes()
    }

    fn set_max_axes(&mut self, n: usize) {
        self.frames[self.current_idx].set_max_axes(n);
    }

    fn test_max_axes(&self) -> bool {
        self.current().test_max_axes()
    }

    fn match_end(&self) -> bool {
        self.current().match_end()
    }

    fn set_match_end(&mut self, v: bool) {
        self.frames[self.current_idx].set_match_end(v);
    }

    fn test_match_end(&self) -> bool {
        self.current().test_match_end()
    }

    fn preserve_axes(&self) -> bool {
        self.current().preserve_axes()
    }

    fn set_preserve_axes(&mut self, v: bool) {
        self.frames[self.current_idx].set_preserve_axes(v);
    }

    fn test_preserve_axes(&self) -> bool {
        self.current().test_preserve_axes()
    }

    fn permute(&self) -> bool {
        self.current().permute()
    }

    fn set_permute(&mut self, v: bool) {
        self.frames[self.current_idx].set_permute(v);
    }

    fn test_permute(&self) -> bool {
        self.current().test_permute()
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        self.current().match_template(target, match_sub)
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        self.current()
            .sub_frame(template, result_naxes, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, axes: &[i32]) -> AstResult<DynFrame> {
        self.current().pick_axes_snapshot(axes)
    }

    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        self.frames[self.current_idx].overlay_template(template, template_axes)
    }
}

/// `FrameSet::FindFrame` override (frameset.c:3790).
///
/// Iterate the FrameSet's frames in priority order — current first,
/// base next, then everything else — calling
/// `Frame::find_frame_template` on each. The first successful match
/// wins. The resulting Mapping is prefixed by the FrameSet's
/// base→matched-frame Mapping and simplified, then packaged as a
/// 2-frame FrameSet (base = self.base, current = matched-frame's
/// result).
///
/// `domainlist` is the standard comma-separated, case-insensitive,
/// whitespace-stripped priority list. Each field (including the
/// blank one introduced by adjacent commas) is tried in order.
fn frameset_find_frame(
    target: &FrameSet,
    template: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<FrameSet>> {
    let nframe = target.nframe();
    let current_idx = target.current_idx() + 1; // 1-based
    let base_idx = target.base_idx() + 1;

    // Build search order: current, then base (if different), then
    // every other frame in 1..=nframe (cmpframe.c:3922-3934 in C).
    let mut order: Vec<usize> = Vec::with_capacity(nframe);
    order.push(current_idx);
    if base_idx != current_idx {
        order.push(base_idx);
    }
    for i in 1..=nframe {
        if i != current_idx && i != base_idx {
            order.push(i);
        }
    }

    // Iterate the comma-separated domainlist fields. Trailing comma
    // semantics: an explicit empty field (e.g. "FOO,," or just "")
    // matches any domain.
    let fields: Vec<String> = if domainlist.is_empty() {
        vec![String::new()]
    } else {
        domainlist.split(',').map(|s| s.to_string()).collect()
    };

    for domain_field in &fields {
        for &target_idx_1based in &order {
            let selected = target.frame(FrameRef::Index(target_idx_1based))?;
            let Some(found) = Frame::find_frame_template(selected, template, domain_field)? else {
                continue;
            };

            // `found` is a 2-frame FrameSet with base = selected and
            // current = result. Extract the base→current Mapping and
            // the current Frame (snapshot).
            let map_inner = found.mapping(FrameRef::Base, FrameRef::Current)?;
            let result_frame = found.current().snapshot();

            // Prefix the FrameSet's base→selected Mapping (or
            // identity when selected is the base itself).
            let prefix = target.mapping(FrameRef::Base, FrameRef::Index(target_idx_1based))?;
            let combined: DynMapping =
                Box::new(CmpMap::new(prefix, map_inner, Composition::Series)?);
            let simplified = crate::mappings::Mapping::simplified(combined.as_ref())?;

            // Build the result 2-frame FrameSet.
            let mut result_fs = FrameSet::new(target.base().snapshot());
            result_fs.add_frame(FrameRef::Base, simplified, result_frame)?;
            return Ok(Some(result_fs));
        }
    }

    Ok(None)
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
        // C FrameSet::Simplify (frameset.c:10111) returns a *copy of
        // the FrameSet* with each internal edge mapping individually
        // simplified — not the composed base→current mapping.
        let mut copy = self.clone();
        for edge in &mut copy.edges {
            edge.mapping = edge.mapping.simplified()?;
        }
        Ok(Box::new(copy))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // C forwards astRate to the base→current mapping
        // (frameset.c:8341) so analytic component rates apply.
        self.mapping(FrameRef::Base, FrameRef::Current)?
            .rate(at, ax1, ax2)
    }

    fn has_forward(&self) -> bool {
        // C GetTranForward forwards to the base→current mapping
        // (frameset.c:877-906).
        self.mapping(FrameRef::Base, FrameRef::Current)
            .map(|m| m.has_forward())
            .unwrap_or(false)
    }

    fn has_inverse(&self) -> bool {
        self.mapping(FrameRef::Base, FrameRef::Current)
            .map(|m| m.has_inverse())
            .unwrap_or(false)
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
        Box::new(self.clone())
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
        node.class_comment = Some("Set of inter-related coordinate systems".to_string());

        // Write proxy attributes from the current frame as commented (unset).
        // The C library writes all current-frame attributes as informational
        // commented lines at the top of the FrameSet dump. Only scalar
        // attributes (not nested objects) are proxied.
        let current_node = FrameToNode::to_node(self.current())?;
        let is_cmpframe = self
            .current()
            .as_any()
            .downcast_ref::<super::CmpFrame>()
            .is_some();
        let mut has_epoch = false;
        let mut has_system = false;
        let mut injected_system = false;
        for item in &current_node.items {
            // Stop at IsA Frame (not IsA Object).
            if item.key == "IsA" {
                if let NodeValue::String(ref class) = item.value {
                    if class == "Frame" {
                        break;
                    }
                }
                // Skip IsA Object markers.
                continue;
            }
            // Skip Object-level and class-specific attributes — not proxied.
            if item.key == "Ident" || item.key == "AlSys" {
                continue;
            }
            let inner = match &item.value {
                NodeValue::Unset(inner) => inner.as_ref(),
                other => other,
            };
            if matches!(inner, NodeValue::Object(_)) {
                continue;
            }
            if item.key == "Epoch" {
                has_epoch = true;
            }
            if item.key == "System" {
                has_system = true;
            }
            // For CmpFrame: inject System="Compound" before first Uni entry.
            if is_cmpframe && !has_system && !injected_system && item.key.starts_with("Uni") {
                node.push_unset(
                    "System",
                    NodeValue::String("Compound".to_string()),
                    Some("Coordinate system type".to_string()),
                );
                injected_system = true;
            }
            // After Domain, inject Epoch from child frames if CmpFrame doesn't have it.
            if item.key == "Domain" && !has_epoch && is_cmpframe {
                node.push_unset(&item.key, inner.clone(), item.comment.clone());
                if let Some(cmp) = self.current().as_any().downcast_ref::<super::CmpFrame>() {
                    let child_node = FrameToNode::to_node(cmp.frame1())?;
                    if let Some(epoch_val) = child_node.get_any("Epoch") {
                        node.push_unset(
                            "Epoch",
                            epoch_val.clone(),
                            Some("Julian epoch of observation".to_string()),
                        );
                        has_epoch = true;
                    }
                    if !has_epoch {
                        let child_node = FrameToNode::to_node(cmp.frame2())?;
                        if let Some(epoch_val) = child_node.get_any("Epoch") {
                            node.push_unset(
                                "Epoch",
                                epoch_val.clone(),
                                Some("Julian epoch of observation".to_string()),
                            );
                            has_epoch = true;
                        }
                    }
                }
                continue;
            }
            // ActUnt is written as active (not commented) in the FrameSet proxy.
            if item.key == "ActUnt" {
                let val = match inner {
                    NodeValue::Int(v) => *v,
                    _ => 0,
                };
                node.push_int("ActUnt", val, item.comment.clone());
            } else {
                node.push_unset(&item.key, inner.clone(), item.comment.clone());
            }
        }

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // Nframe — number of frames (always).
        let nframe = self.frames.len();
        node.push_int(
            "Nframe",
            nframe as i64,
            Some("Number of Frames in FrameSet".to_string()),
        );

        // Base — 1-based base frame index (commented if default == 1).
        if self.base_idx == 0 {
            node.push_unset(
                "Base",
                NodeValue::Int(1),
                Some("Index of base Frame".to_string()),
            );
        } else {
            node.push_int(
                "Base",
                (self.base_idx + 1) as i64,
                Some("Index of base Frame".to_string()),
            );
        }

        // Currnt — 1-based current frame index.
        node.push_int(
            "Currnt",
            (self.current_idx + 1) as i64,
            Some("Index of current Frame".to_string()),
        );

        // Nnode — only written when nnode != nframe (tree structure).
        if self.nnode != nframe {
            node.push_int(
                "Nnode",
                self.nnode as i64,
                Some("Number of nodes in FrameSet".to_string()),
            );
        }

        // Nod{i} — frame-to-node mapping (only when differs from identity).
        for (i, &nod) in self.node_map.iter().enumerate() {
            let val = nod + 1; // 1-based
            if val != i + 1 {
                let key = format!("Nod{}", i + 1);
                let comment = format!("Frame {} is associated with node {}", i + 1, val);
                node.push_int(&key, val as i64, Some(comment));
            }
        }

        // Lnk{node+1} = parent+1 for each edge (1-based node indices).
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

        // Map{node+1} — each edge mapping as a nested object.
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
        let nnode = node.get_int("Nnode", nframe as i64) as usize;

        // Read frame-to-node mapping (Nod{i}, defaults to identity).
        let mut node_map: Vec<usize> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("Nod{}", i + 1);
            let nod = node.get_int(&key, (i + 1) as i64) as usize;
            node_map.push(nod - 1); // convert to 0-based
        }

        // Read frames.
        let mut frames: Vec<DynFrame> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("Frm{}", i + 1);
            let frame_node = node.get_object(&key).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("FrameSet: missing {key}"))
            })?;
            frames.push(frame_from_node(frame_node)?);
        }

        // Read edges: for nodes 2..nnode (1-based), each has Lnk, Map.
        let mut edges: Vec<Edge> = Vec::with_capacity(nnode.saturating_sub(1));
        for inode in 1..nnode {
            let lnk_key = format!("Lnk{}", inode + 1);
            let parent = node.get_int(&lnk_key, 0) as usize;
            if parent == 0 {
                return Err(AstError::new(
                    ErrorCode::Lderr,
                    format!("FrameSet: missing or invalid {lnk_key}"),
                ));
            }

            let map_key = format!("Map{}", inode + 1);
            let map_node = node.get_object(&map_key).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("FrameSet: missing {map_key}"))
            })?;
            let mapping = mapping_from_node(map_node)?;

            edges.push(Edge {
                from: parent - 1, // 0-based node index
                to: inode,        // 0-based node index
                mapping,
            });
        }

        Ok(FrameSet {
            frames,
            edges,
            base_idx: base - 1,
            current_idx: current - 1,
            variants: Vec::new(),
            nnode,
            node_map,
        })
    }
}
