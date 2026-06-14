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
    /// All frames at once — only valid as the `anchor` of
    /// [`FrameSet::add_frame`], where it appends the supplied frame's axes to
    /// every frame in the set (mirrors C's `AST__ALLFRAMES`, frameset.h:465).
    /// It does not identify a single frame and cannot be passed to
    /// [`FrameSet::frame`], `set_base`, etc.
    AllFrames,
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
    /// Whether the Base attribute has been explicitly set (mirrors C's
    /// `base != -INT_MAX`, frameset.c:12554). Controls whether `Base` is
    /// serialised as an active value or a commented-out default: a FrameSet
    /// assembled from a FITS header has Base explicitly set (C `astSetBase`,
    /// fitschan.c:11990), whereas one built by `astAddFrame` leaves it
    /// defaulted. When `false`, `base_idx` still holds the effective base
    /// (frame 0), it is simply written as the implicit default.
    base_set: bool,
    /// Per-frame Variants FrameSet (indexed 0-based by frame). `Some(fs)` means
    /// the frame owns a set of named variant Mappings: `fs`'s member frames are
    /// the variants, each variant frame's Domain is its name, and `fs`'s
    /// current frame is the selected variant. `None` means no variants.
    ///
    /// C stores this on the Frame itself (`frame.h:636`); the Rust port holds
    /// it FrameSet-side keyed by frame index because the concrete Frame structs
    /// share no base struct and the variant API only ever acts on a FrameSet's
    /// current frame. A bare `Frame` carrying variants outside a FrameSet is
    /// therefore unsupported (a documented divergence — not in the C public
    /// test surface). Serialisation still matches C exactly (the `Vrnts` object
    /// is written into / read from each member Frame's node; see `to_node`).
    frame_variants: Vec<Option<FrameSet>>,
    /// Per-frame variant-mirror index (0-based frame → 1-based mirror target,
    /// 0 = owns its variants). Mirrors C's `varfrm[]` (frameset.c). `varfrm[i]
    /// = j > 0` means frame `i` mirrors frame `j`'s variants.
    varfrm: Vec<usize>,
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
            base_set: self.base_set,
            frame_variants: self.frame_variants.clone(),
            varfrm: self.varfrm.clone(),
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
            .field(
                "nvariants",
                &self.frame_variants.iter().filter(|v| v.is_some()).count(),
            )
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
            base_set: false,
            frame_variants: vec![None],
            varfrm: vec![0],
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
        self.base_set = true;
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
    ///
    /// As a special case, when `anchor` is [`FrameRef::AllFrames`] the `map`
    /// argument is **ignored** and the supplied frame's axes are appended to
    /// *every* frame in the set (see [`FrameSet::append_axes`]); this mirrors
    /// C `astAddFrame` with `iframe == AST__ALLFRAMES` (frameset.c:1336).
    pub fn add_frame(
        &mut self,
        anchor: FrameRef,
        map: DynMapping,
        new_frame: DynFrame,
    ) -> AstResult<()> {
        if anchor == FrameRef::AllFrames {
            // The Mapping is ignored in this mode, matching C.
            let _ = map;
            return self.append_axes(new_frame);
        }
        let anchor_idx = self.resolve(anchor)?;
        let anchor_node = self.node_map[anchor_idx];
        let new_node = self.nnode;
        self.nnode += 1;
        self.frames.push(new_frame);
        self.node_map.push(new_node);
        self.frame_variants.push(None);
        self.varfrm.push(0);
        self.edges.push(Edge {
            from: anchor_node,
            to: new_node,
            mapping: map,
        });
        self.current_idx = self.frames.len() - 1;
        Ok(())
    }

    /// Append the axes of `frame` to every Frame in the set.
    ///
    /// Mirrors C `AppendAxes` (frameset.c:1943), invoked by
    /// `astAddFrame(this, AST__ALLFRAMES, ..)`: each Frame is replaced by a
    /// `CmpFrame(original, frame)` and each internal Mapping by a parallel
    /// `CmpMap(original, UnitMap(naxes))` (the UnitMap's Nin/Nout = the number
    /// of axes in `frame`), each CmpMap simplified before storage. The base
    /// and current frame selections are unchanged, and so widening a 2-axis
    /// sky FrameSet with a 1-axis spectral Frame yields a 3-axis sky+spectral
    /// FrameSet in one call.
    ///
    /// (The Rust edge model bakes inversion into each stored Mapping, so the
    /// `Invert`-flag juggling C performs around each `astCmpMap` is not needed
    /// here.)
    fn append_axes(&mut self, frame: DynFrame) -> AstResult<()> {
        let nextra = frame.naxes();

        // Replace every Frame with CmpFrame(original, frame).
        let old_frames = std::mem::take(&mut self.frames);
        self.frames = old_frames
            .into_iter()
            .map(|orig| -> DynFrame { Box::new(super::CmpFrame::new(orig, frame.snapshot())) })
            .collect();

        // Replace every edge Mapping with a parallel CmpMap(original,
        // UnitMap(nextra)), simplified.
        let old_edges = std::mem::take(&mut self.edges);
        let mut new_edges = Vec::with_capacity(old_edges.len());
        for edge in old_edges {
            let cmp = CmpMap::new(
                edge.mapping,
                Box::new(UnitMap::new(nextra)),
                Composition::Parallel,
            )?;
            let mapping = Mapping::simplified(&cmp)?;
            new_edges.push(Edge {
                from: edge.from,
                to: edge.to,
                mapping,
            });
        }
        self.edges = new_edges;
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

        // Variants bookkeeping (frameset.c RemoveFrame/RemoveMirrors,
        // 8755-8777): drop any frame that mirrors the removed frame's variants
        // (`varfrm[ifr] == idx+1` → 0), then decrement higher mirror targets,
        // and finally compact the per-frame variant arrays at `idx`.
        let removed_1based = idx + 1;
        for vf in self.varfrm.iter_mut() {
            if *vf == removed_1based {
                *vf = 0;
            } else if *vf > removed_1based {
                *vf -= 1;
            }
        }
        self.frame_variants.remove(idx);
        self.varfrm.remove(idx);

        // Fix base/current indices. C only adjusts the Base attribute when
        // it is explicitly set (frameset.c:8759 `astTestBase`): if the removed
        // frame *was* the base, clear the attribute so the default (first
        // frame) applies; if it was below the base, decrement. When Base is
        // not set, the default already tracks the first frame, so nothing is
        // done. Current defaults to the *last* frame instead.
        if self.base_set {
            if self.base_idx == idx {
                self.base_set = false;
                self.base_idx = 0;
            } else if self.base_idx > idx {
                self.base_idx -= 1;
            }
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

    /// Resolve the variant-owning frame for a 1-based frame index, following
    /// the `varfrm` mirror chain. Mirrors C `GetVarFrm` (frameset.c:5975).
    fn get_var_frm(&self, iframe: usize) -> usize {
        let mut result = iframe;
        while self.varfrm[result - 1] > 0 {
            let next = self.varfrm[result - 1];
            if next == result {
                break; // corrupt; avoid infinite loop (C reports AST__INTER)
            }
            result = next;
        }
        result
    }

    /// Store a new variant Mapping for the current Frame, or rename the
    /// currently-selected variant. Mirrors C `astAddVariant` (frameset.c:1645).
    ///
    /// `map` converts coordinates from the current Frame to the new variant of
    /// the current Frame. If `None`, the currently-selected variant is simply
    /// renamed to `name`. The new variant becomes current on exit.
    ///
    /// Errors if the current Frame is a mirror (`Mirro`) or a variant with the
    /// given name already exists (`Bdvnm`).
    pub fn add_variant(&mut self, map: Option<DynMapping>, name: &str) -> AstResult<()> {
        let icur = self.current_idx; // 0-based

        // Reject if the current frame is a mirror.
        if self.varfrm[icur] > 0 {
            return Err(AstError::new(
                ErrorCode::Mirro,
                "addVariant: illegal attempt to add a variant Mapping to a mirror Frame",
            ));
        }

        // Clean the supplied name: strip leading blanks, uppercase, trim.
        let myname = name.trim().to_uppercase();

        // Get (or create) the Variants FrameSet for the current frame. When
        // absent, seed it with a copy of the current frame whose Domain is the
        // frame's current variant name (its own Domain).
        let is_new = self.frame_variants[icur].is_none();
        let mut vfs = match self.frame_variants[icur].take() {
            Some(v) => v,
            None => FrameSet::new(self.frames[icur].snapshot()),
        };

        // Reject a duplicate variant name.
        for i in 1..=vfs.nframe() {
            if vfs
                .frame(FrameRef::Index(i))?
                .domain()
                .eq_ignore_ascii_case(&myname)
            {
                self.frame_variants[icur] = Some(vfs);
                return Err(AstError::new(
                    ErrorCode::Bdvnm,
                    format!("addVariant: a variant named '{myname}' already exists"),
                ));
            }
        }

        match map {
            // No Mapping: just rename the currently-selected variant.
            None => {
                vfs.frame_mut(FrameRef::Current)?.set_domain(&myname);
            }
            // A Mapping: append a new variant frame.
            Some(map) => {
                // Mapping from the variants-FS current frame to "this"'s
                // current frame. C temporarily matches Domains so astConvert
                // succeeds; here both are copies of the same frame class, so
                // align them via the conversion driver with matching domains.
                let vcur = vfs.current().snapshot();
                let frm = self.frames[icur].snapshot();
                let map1 = convert_current_to(vcur.as_ref(), frm.as_ref())?;

                // current(vfs) → new variant frame mapping.
                let map3 = Box::new(CmpMap::new(map1, map, Composition::Series)?);
                let map3 = Mapping::simplified(map3.as_ref())?;

                // The new variant frame is a copy of the parent frame with its
                // own variants cleared and Domain set to the variant name.
                let mut vfrm = self.frames[icur].snapshot();
                vfrm.set_domain(&myname);

                let saved_cur = vfs.current_idx;
                vfs.add_frame(FrameRef::Current, map3, vfrm)?;
                vfs.current_idx = saved_cur;
            }
        }

        self.frame_variants[icur] = Some(vfs);

        // Select the variant. C calls `astSetVariant` only when a Mapping was
        // supplied; for a pure rename the renamed variant is already current,
        // so re-selecting it by its new name is an equivalent no-op.
        self.set_variant(&myname)?;

        let _ = is_new;
        Ok(())
    }

    /// Select a named variant of the current Frame, re-mapping the FrameSet so
    /// its transform reflects the chosen variant. Mirrors C `SetVariant`
    /// (frameset.c:9894). Errors (`Attin`) if the current Frame has no variant
    /// with the given name.
    pub fn set_variant(&mut self, name: &str) -> AstResult<()> {
        let icur = self.current_idx;
        let owner = self.get_var_frm(icur + 1) - 1; // 0-based variant owner

        let myvar = name.trim().to_uppercase();

        // No Variants FrameSet → only the frame's own (default) variant exists;
        // C raises AST__ATTIN for any other name (testframeset Error 24).
        let Some(vfs) = &self.frame_variants[owner] else {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!("setVariant: unknown Frame variant '{myvar}' requested"),
            ));
        };

        // Find the variant frame whose Domain matches the requested name.
        let mut target_1based = 0usize;
        for i in 1..=vfs.nframe() {
            if vfs
                .frame(FrameRef::Index(i))?
                .domain()
                .eq_ignore_ascii_case(&myvar)
            {
                target_1based = i;
                break;
            }
        }
        if target_1based == 0 {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "setVariant: unknown Frame variant '{myvar}' requested - available variants are '{}'",
                    self.all_variants()
                ),
            ));
        }

        // Already selected: nothing to do.
        if target_1based == vfs.current_idx + 1 {
            return Ok(());
        }

        // Build the correction Mapping from the old variant to the new variant,
        // expressed in current-Frame coordinates (frameset.c:9998-10032):
        //   map1 = current(this) -> current(vfs)   [old variant]
        //   map2 = current(vfs)  -> target(vfs)    [old variant -> new variant]
        //   map3 = target(vfs)   -> current(this)  [new variant]
        // The composite maps current-Frame coords through the variant change
        // and back, so `remap_frame(Current, ..)` updates the node Mapping
        // exactly as C recomposes it.
        let frm = self.frames[icur].snapshot();
        let vcur = vfs.current().snapshot();
        let map1 = convert_current_to(frm.as_ref(), vcur.as_ref())?;
        let map2 = vfs.mapping(FrameRef::Current, FrameRef::Index(target_1based))?;
        let vnew = vfs.frame(FrameRef::Index(target_1based))?.snapshot();
        let map3 = convert_current_to(vnew.as_ref(), frm.as_ref())?;

        let map45 = CmpMap::new(
            Box::new(CmpMap::new(map1, map2, Composition::Series)?),
            map3,
            Composition::Series,
        )?;
        let correction = Mapping::simplified(&map45)?;

        // Apply the correction to the current frame's node and mark the new
        // variant current within the Variants FrameSet.
        self.remap_frame(FrameRef::Current, correction)?;
        if let Some(vfs) = &mut self.frame_variants[owner] {
            vfs.current_idx = target_1based - 1;
        }
        Ok(())
    }

    /// Name of the currently-selected variant of the current Frame. Mirrors C
    /// `GetVariant` (frameset.c:6041): the Domain of the (mirror-resolved)
    /// Variants FrameSet's current frame, or the current Frame's own Domain
    /// when it has no variants.
    pub fn variant(&self) -> String {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        match &self.frame_variants[owner] {
            Some(vfs) => vfs.current().domain().to_string(),
            None => self.frames[self.current_idx].domain().to_string(),
        }
    }

    /// Whether the current Frame has an explicitly-set variant (i.e. owns a
    /// Variants FrameSet). Mirrors C `TestVariant` (frameset.c:11010).
    pub fn test_variant(&self) -> bool {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        self.frame_variants[owner].is_some()
    }

    /// Clear all variants from the current Frame, reverting to its single
    /// default calibration. Mirrors C `ClearVariant` (frameset.c:2905), which
    /// sets the Frame's Variants FrameSet pointer to NULL.
    pub fn clear_variant(&mut self) {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        self.frame_variants[owner] = None;
    }

    /// Space-separated list of all variant names available for the current
    /// Frame. Mirrors C `GetAllVariants` (frameset.c:6864): the Domains of the
    /// (mirror-resolved) Variants FrameSet's member frames, or just the current
    /// Frame's Domain when it has no variants.
    pub fn all_variants(&self) -> String {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        match &self.frame_variants[owner] {
            Some(vfs) => {
                let mut names = Vec::with_capacity(vfs.nframe());
                for i in 1..=vfs.nframe() {
                    if let Ok(f) = vfs.frame(FrameRef::Index(i)) {
                        names.push(f.domain().to_string());
                    }
                }
                names.join(" ")
            }
            None => self.frames[self.current_idx].domain().to_string(),
        }
    }

    /// Make the current Frame mirror the variants of another Frame. Mirrors C
    /// `astMirrorVariants` (frameset.c:7468): the current Frame's variant
    /// access then forwards to `which`'s. Passing the current frame itself (or
    /// the same index) disables mirroring.
    pub fn mirror_variants(&mut self, which: FrameRef) -> AstResult<()> {
        let icur = self.current_idx;
        let target = self.resolve(which)?; // 0-based
        self.varfrm[icur] = if target == icur { 0 } else { target + 1 };
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
            FrameRef::AllFrames => Err(AstError::new_tracked(
                ErrorCode::Frmin,
                "AST__ALLFRAMES does not identify a single frame",
            )),
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
                // Forward direction — return a clone of the stored mapping.
                // Clone rather than re-simplify: the edge mapping is already
                // in its intended form, and re-running simplify on every
                // access is wasteful and can corrupt mappings the simplify
                // engine handles imperfectly (e.g. the TranMap-carrying
                // SpecFluxFrame conversion, whose inverse it can drop).
                return Ok(edge.mapping.clone_box());
            }
            if edge.from == to && edge.to == from {
                // Reverse direction — clone and invert.
                let mut m = edge.mapping.clone_box();
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

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
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

    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let domainlist = if domains.is_empty() {
            " ".to_string()
        } else {
            domains.join(",")
        };
        frameset_convert_to(self, to, &domainlist)
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

/// Mapping from frame `from` to frame `to`, where the two are calibration
/// variants of the same Frame (used by the variants API). Mirrors C's
/// "temporarily match Domains, then astConvert" idiom (frameset.c:9998-10010):
/// the variant frames are copies of the parent frame and may differ only in
/// Domain (the variant name), so the Domains are forced equal before
/// conversion and the resulting base→current Mapping is returned.
fn convert_current_to(from: &dyn Frame, to: &dyn Frame) -> AstResult<DynMapping> {
    let mut a = from.snapshot();
    let mut b = to.snapshot();
    // Force matching Domains so domain gating does not block the conversion.
    let dom = b.domain().to_string();
    a.set_domain(&dom);
    let cvt = crate::frames::find_frame::convert_frames(a.as_ref(), b.as_ref(), &dom)?.ok_or_else(
        || {
            AstError::new(
                ErrorCode::Inter,
                "addVariant/setVariant: cannot convert between variant frames",
            )
        },
    )?;
    let _ = &mut b;
    cvt.mapping(FrameRef::Base, FrameRef::Current)
}

/// `FrameSet::ConvertX` override (frameset.c:3201).
///
/// Find a conversion route between `from` (a FrameSet) and `to` (a Frame or
/// FrameSet) by trying *every* member frame of each as a candidate alignment
/// endpoint, not just the current frames. For each `(from_member, to_member)`
/// pair the per-pair alignment is found via [`convert_frames`], the
/// `from`-current→member Mapping is prefixed, the `to`-member→current Mapping
/// is appended, the result is simplified, and a score favouring both-direction
/// Mappings (forward+inverse=3, forward=2, inverse=1) selects the best route.
/// The returned 2-frame FrameSet maps `from`'s current Frame to `to`'s current
/// Frame.
///
/// The search order for each set is base first, then the remaining frames in
/// index order, then the current frame last (frameset.c:3308-3348). FrameSet
/// inversion is already materialised as a base/current index swap, so no
/// separate reversed-order branch is needed.
///
/// Divergence from C: `ConvertX` also sets the *input* FrameSets' Base
/// attribute to record the route used (frameset.c:3502-3503). The Frame trait
/// takes `&self`/`&dyn Frame` and cannot mutate the inputs; that side-effect is
/// an optimisation hint, not part of the conversion result, so it is omitted.
fn frameset_convert_to(
    from: &FrameSet,
    to: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<FrameSet>> {
    // Treat `to` as a FrameSet when it is one; otherwise as a single frame.
    let to_fs = to.as_any().downcast_ref::<FrameSet>();

    // Build the base-first / others / current-last search order (1-based).
    fn search_order(nframe: usize, base: usize, current: usize) -> Vec<usize> {
        let mut order = Vec::with_capacity(nframe);
        order.push(base);
        for i in 1..=nframe {
            if i != base && i != current {
                order.push(i);
            }
        }
        if current != base {
            order.push(current);
        }
        order
    }

    let from_current = from.current_idx() + 1;
    let from_order = search_order(from.nframe(), from.base_idx() + 1, from_current);

    let (to_nframe, to_current) = match to_fs {
        Some(fs) => (fs.nframe(), fs.current_idx() + 1),
        None => (1, 1),
    };
    let to_order = match to_fs {
        Some(fs) => search_order(fs.nframe(), fs.base_idx() + 1, to_current),
        None => vec![1],
    };
    let _ = to_nframe;

    // Comma-separated domain fields; an empty list yields one blank field
    // that matches any domain.
    let fields: Vec<String> = if domainlist.is_empty() {
        vec![String::new()]
    } else {
        domainlist.split(',').map(|s| s.to_string()).collect()
    };

    let mut best_score = -1i32;
    let mut result_map: Option<DynMapping> = None;

    'search: for field in &fields {
        let domain = field.trim();
        for &to_idx in &to_order {
            // The selected `to` member frame (or `to` itself when not a set).
            let to_member: DynFrame = match to_fs {
                Some(fs) => fs.frame(FrameRef::Index(to_idx))?.snapshot(),
                None => to.snapshot(),
            };

            for &from_idx in &from_order {
                let from_member = from.frame(FrameRef::Index(from_idx))?.snapshot();

                // Per-pair alignment between the two selected member frames.
                let Some(cvt) = crate::frames::find_frame::convert_frames(
                    from_member.as_ref(),
                    to_member.as_ref(),
                    domain,
                )?
                else {
                    continue;
                };
                let mut map: DynMapping = cvt.mapping(FrameRef::Base, FrameRef::Current)?;

                // Prefix from-current → from-member.
                if from_idx != from_current {
                    let pre = from.mapping(FrameRef::Current, FrameRef::Index(from_idx))?;
                    map = Box::new(CmpMap::new(pre, map, Composition::Series)?);
                }

                // Append to-member → to-current.
                if let Some(fs) = to_fs
                    && to_idx != to_current
                {
                    let post = fs.mapping(FrameRef::Index(to_idx), FrameRef::Current)?;
                    map = Box::new(CmpMap::new(map, post, Composition::Series)?);
                }

                let map = Mapping::simplified(map.as_ref())?;

                let score: i32 =
                    if map.has_forward() { 2 } else { 0 } + if map.has_inverse() { 1 } else { 0 };

                if score > best_score {
                    best_score = score;
                    result_map = Some(map);
                    if best_score >= 3 {
                        break 'search;
                    }
                }
            }
        }
    }

    let Some(result_map) = result_map else {
        return Ok(None);
    };

    // Result: from-current Frame → to-current Frame, linked by the best route.
    let to_cur_frame: DynFrame = match to_fs {
        Some(fs) => fs.current().snapshot(),
        None => to.snapshot(),
    };
    let mut result_fs = FrameSet::new(from.current().snapshot());
    result_fs.add_frame(FrameRef::Base, result_map, to_cur_frame)?;
    Ok(Some(result_fs))
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

        // Base — 1-based base frame index. C writes it as an active value
        // only when the Base attribute is explicitly set (frameset.c:12554,
        // `set = (this->base != -INT_MAX)`); otherwise it emits the default
        // (frame 1) as a commented-out line.
        if self.base_set {
            node.push_int(
                "Base",
                (self.base_idx + 1) as i64,
                Some("Index of base Frame".to_string()),
            );
        } else {
            node.push_unset(
                "Base",
                NodeValue::Int(self.base_idx as i64 + 1),
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

        // Frm{i+1} — each frame as a nested object. C writes a frame's Variants
        // FrameSet inside its own block as object `Vrnts` (frame.c:14679);
        // since Rust holds variants FrameSet-side, inject it here.
        for (i, frame) in self.frames.iter().enumerate() {
            let key = format!("Frm{}", i + 1);
            let mut frame_node = FrameToNode::to_node(frame.as_ref())?;
            if let Some(vfs) = &self.frame_variants[i] {
                let vnode = Serializable::to_node(vfs)?;
                frame_node.push_object("Vrnts", vnode, Some("Variant Frames".to_string()));
            }
            let comment = format!("Frame number {}", i + 1);
            node.push_object(&key, frame_node, Some(comment));
        }

        // VarFrm{i+1} — per-frame variant-mirror indices (1-based; 0 = owns).
        for (i, &vf) in self.varfrm.iter().enumerate() {
            if vf != 0 {
                let key = format!("VarFrm{}", i + 1);
                node.push_int(
                    &key,
                    vf as i64,
                    Some(format!(
                        "Variants for Frame {} are stored in Frame {vf}",
                        i + 1
                    )),
                );
            }
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

        // C reads `astReadInt("base", -INT_MAX)` and treats any value < 1 as
        // unset (frameset.c:13209). `node.get_int` skips commented-out
        // (Unset) defaults, so an absent or commented Base reads as unset.
        let base_active = node.get_int("Base", 0);
        let base_set = base_active >= 1;
        let base = if base_set { base_active as usize } else { 1 };
        let current = node.get_int("Currnt", 1) as usize;
        let nnode = node.get_int("Nnode", nframe as i64) as usize;

        // Read frame-to-node mapping (Nod{i}, defaults to identity).
        let mut node_map: Vec<usize> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("Nod{}", i + 1);
            let nod = node.get_int(&key, (i + 1) as i64) as usize;
            node_map.push(nod - 1); // convert to 0-based
        }

        // Read frames, extracting each frame's optional Variants FrameSet
        // (written by C inside the member Frame's block as object `Vrnts`,
        // frame.c:14679).
        let mut frames: Vec<DynFrame> = Vec::with_capacity(nframe);
        let mut frame_variants: Vec<Option<FrameSet>> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("Frm{}", i + 1);
            let frame_node = node.get_object(&key).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("FrameSet: missing {key}"))
            })?;
            frames.push(frame_from_node(frame_node)?);
            let vrnts = match frame_node.get_object("Vrnts") {
                Some(vn) => Some(FrameSet::from_node(vn)?),
                None => None,
            };
            frame_variants.push(vrnts);
        }

        // Read the per-frame variant-mirror indices (VarFrm{i}, default 0).
        let mut varfrm: Vec<usize> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("VarFrm{}", i + 1);
            varfrm.push(node.get_int(&key, 0) as usize);
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
            base_set,
            frame_variants,
            varfrm,
            nnode,
            node_map,
        })
    }
}
