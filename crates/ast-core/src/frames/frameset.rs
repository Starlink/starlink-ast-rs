//! The FrameSet: a graph of Frames connected by Mappings.

use std::collections::VecDeque;
use std::fmt;

use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap, UnitMap, ViaSimplify};
use crate::serial::{
    FrameToNode, MappingToNode, Node, NodeValue, Serializable, frame_from_node, mapping_from_node,
};

use super::{AxisKind, DynFrame, Frame, FrameSearch, simplify_frame_via_mapping};

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
    /// every frame in the set.
    /// It does not identify a single frame and cannot be passed to
    /// [`FrameSet::frame`], `set_base`, etc.
    // C: `AST__ALLFRAMES` (frameset.h:465).
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
    /// The Invert attribute of the Mapping object C stores for this link,
    /// which C's separate per-link flag (`invert[]`, written as `Inv<n>`)
    /// overrides when the link is used: `mapping` is that object with its
    /// Invert set to the flag. The two differ only after the link has been
    /// reversed, as merging a FrameSet does.
    object_invert: bool,
}

impl Edge {
    /// A link whose Mapping object is stored as it is used.
    fn new(from: usize, to: usize, mapping: DynMapping) -> Self {
        let object_invert = mapping.is_inverted();
        Edge {
            from,
            to,
            mapping,
            object_invert,
        }
    }
}

impl fmt::Debug for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Edge")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish()
    }
}

/// A set of inter-related coordinate systems.
///
/// A FrameSet consists of one or more [`Frame`]s, which describe coordinate
/// systems, connected by [`Mapping`]s, which describe how the coordinate
/// systems are inter-related.
/// It gives a Mapping between any pair of its Frames
/// ([`mapping`](FrameSet::mapping)), that is, a conversion between any two
/// of the coordinate systems it describes.
/// The Frames are numbered from 1 in the order they are added
/// ([`add_frame`](FrameSet::add_frame)), and a [`FrameRef`] names one.
///
/// Every FrameSet has a *base* Frame and a *current* Frame, which may be the
/// same.
/// Any Frame may be made either ([`set_base`](FrameSet::set_base),
/// [`set_current`](FrameSet::set_current)); by default the first Frame
/// added is the base Frame and the last one added is the current Frame.
/// The base Frame describes the "native" coordinate system of whatever the
/// FrameSet calibrates, such as the pixel coordinates of an image, and the
/// current Frame the "apparent" coordinate system in which it should be
/// viewed.
/// Any further Frames are a library of alternative coordinate systems,
/// which may be selected by making them current.
///
/// # As a Frame
///
/// Where a Frame is needed, a FrameSet presents its current Frame, so it
/// can be used in place of its current Frame in most situations.
/// Every Frame attribute read through the [`Frame`] trait is that of the
/// current Frame: its number of axes, Title, Domain, Epoch, Digits, System,
/// observer's position, MinAxes, MaxAxes, MatchEnd, PreserveAxes, Permute
/// and ActiveUnit flag, and the per-axis Label, Symbol, Unit, Format,
/// Digits, Direction, Top and Bottom.
/// Its Ident is its own.
/// The attributes a class adds, such as a SkyFrame's Equinox, are read
/// through [`current`](FrameSet::current), and the set of them may change
/// when a new current Frame is selected.
///
/// Setting or clearing a Frame attribute through the [`Frame`] trait sets
/// or clears it on the current Frame, and keeps the FrameSet's integrity:
/// when the change alters the coordinate system the current Frame
/// describes, as changing a SkyFrame's System or a SpecFrame's Unit does,
/// the Mapping from the old coordinate system to the new one is found and
/// the current Frame is remapped through it
/// ([`remap_frame`](FrameSet::remap_frame)), so that the coordinates every
/// other Frame gives a position are unchanged and those of the current
/// Frame follow its new definition.
/// [`with_integrity`](FrameSet::with_integrity) makes any change to the
/// current Frame in this way, including one to an attribute the Frame trait
/// does not have.
/// [`frame_mut`](FrameSet::frame_mut) is the escape hatch: a change made
/// through it leaves the Mappings as they are, so the Frame is relabeled
/// rather than converted.
///
/// # As a Mapping
///
/// Where a Mapping is needed, a FrameSet is the Mapping from its base Frame
/// to its current Frame, so it converts "native" coordinates to "apparent"
/// ones and back.
/// Its [`nin`](Mapping::nin) is the number of axes of its base Frame and its
/// [`nout`](Mapping::nout) that of its current Frame, and
/// [`has_forward`](Mapping::has_forward),
/// [`has_inverse`](Mapping::has_inverse) and
/// [`is_linear`](Mapping::is_linear) are those of the Mapping between them,
/// so selecting a new base or current Frame may change them.
/// Inverting a FrameSet ([`invert`](Mapping::invert)) exchanges its base
/// and current Frames, which reverses the Mapping and may change its other
/// properties, and inverting it again exchanges them back;
/// [`is_inverted`](Mapping::is_inverted) tells whether it is inverted.
///
/// A Region may be added to a FrameSet as a Frame.
/// The Mapping between two Frames then includes the masking effect of any
/// Region on the path between them, the two Frames themselves included.
/// A Region inside a CmpFrame has no such effect here, since a CmpFrame is
/// not a Mapping.
///
/// # Attributes
///
/// In addition to the attributes of its current Frame, a FrameSet has:
///
/// - [`all_variants`](FrameSet::all_variants): the variant Mappings stored
///   with the current Frame.
/// - [`set_base`](FrameSet::set_base): the base Frame.
/// - [`set_current`](FrameSet::set_current): the current Frame.
/// - [`nframe`](FrameSet::nframe): the number of Frames.
/// - [`variant`](FrameSet::variant): the variant Mapping in use by the
///   current Frame.
///
/// # Examples
///
/// ```
/// use ast_core::frames::{BasicFrameBuilder, Frame, FrameRef, FrameSet};
/// use ast_core::mappings::{Mapping, ZoomMap};
///
/// let pixel = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
/// let mm = BasicFrameBuilder::new(2).domain("MM").build().unwrap();
/// let mut fs = FrameSet::new(Box::new(pixel));
/// fs.add_frame(FrameRef::Base, Box::new(ZoomMap::new(2, 0.5).unwrap()), Box::new(mm))
///     .unwrap();
/// assert_eq!(fs.nframe(), 2);
/// assert_eq!(fs.domain(), "MM");
/// assert_eq!(fs.tran2(&[4.0], &[6.0], true).unwrap(), (vec![2.0], vec![3.0]));
///
/// fs.set_current(FrameRef::Index(1)).unwrap();
/// assert_eq!(fs.domain(), "PIXEL");
/// ```
pub struct FrameSet {
    frames: Vec<DynFrame>,
    edges: Vec<Edge>,
    base_idx: usize,
    current_idx: usize,
    /// Whether the Base attribute has been explicitly set (mirrors C's
    /// `base != -INT_MAX`, frameset.c:12554). Controls whether `Base` is
    /// serialized as an active value or a commented-out default: a FrameSet
    /// assembled from a FITS header has Base explicitly set (C `astSetBase`,
    /// fitschan.c:12084), whereas one built by `astAddFrame` leaves it
    /// defaulted. When `false`, `base_idx` still holds the effective base,
    /// [`default_base_idx`](FrameSet::default_base_idx); it is simply written
    /// as the implicit default.
    base_set: bool,
    /// Whether the Current attribute has been set (C's `current !=
    /// -INT_MAX`). When `false`, `current_idx` holds the default,
    /// [`default_current_idx`](FrameSet::default_current_idx), and Current is
    /// written as a commented-out default. Adding a Frame sets it, as
    /// `astAddFrame` calls `astSetCurrent`.
    current_set: bool,
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
    /// test surface). Serialization still matches C exactly (the `Vrnts` object
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
    /// The FrameSet's own `Ident`, `None` when unset. C keeps this on Object
    /// and a FrameSet reads its own rather than its current Frame's
    /// (`frameset.c:2759`, `:4841`, `:10830` all pass `this`).
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// The FrameSet's own `UseDefs`, `None` when unset.
    use_defs: Option<bool>,
    /// The Invert attribute. `base_idx`/`current_idx` (and their `_set`
    /// flags) are the effective base and current Frames, already exchanged
    /// when this is true, as C's `GetBase`/`GetCurrent` exchange its stored
    /// `base`/`current`; a dump writes the stored ones.
    inverted: bool,
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
                    object_invert: e.object_invert,
                })
                .collect(),
            base_idx: self.base_idx,
            current_idx: self.current_idx,
            base_set: self.base_set,
            current_set: self.current_set,
            frame_variants: self.frame_variants.clone(),
            varfrm: self.varfrm.clone(),
            nnode: self.nnode,
            node_map: self.node_map.clone(),
            ident: self.ident.clone(),
            id: Default::default(),
            mapping_flags: self.mapping_flags.clone(),
            use_defs: self.use_defs,
            inverted: self.inverted,
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
    /// This FrameSet with the Regions removed from every Mapping and every
    /// Frame, or `None` when it holds none (frameset.c `RemoveRegions`).
    fn without_regions(&self) -> Option<FrameSet> {
        let maps: Vec<Option<DynMapping>> = self
            .edges
            .iter()
            .map(|e| e.mapping.mapping_without_regions())
            .collect();
        let frames: Vec<Option<DynFrame>> = self
            .frames
            .iter()
            .map(|f| f.frame_without_regions())
            .collect();
        if maps.iter().all(Option::is_none) && frames.iter().all(Option::is_none) {
            return None;
        }
        let mut new = self.clone();
        for (edge, map) in new.edges.iter_mut().zip(maps) {
            if let Some(map) = map {
                edge.mapping = map;
            }
        }
        for (slot, frame) in new.frames.iter_mut().zip(frames) {
            if let Some(frame) = frame {
                *slot = frame;
            }
        }
        Some(new)
    }

    // ------------------------------------------------------------------ //
    // Construction                                                        //
    // ------------------------------------------------------------------ //

    /// Create a FrameSet holding one Frame, which is both its base and its
    /// current Frame; further Frames are added with
    /// [`add_frame`](FrameSet::add_frame).
    ///
    /// Given a FrameSet, the new FrameSet instead holds its Frames and
    /// Mappings, with the same Base, Current and Invert attributes; its other
    /// attributes, such as [`ident`](Frame::ident), are not taken.
    #[doc(alias = "astFrameSet")]
    pub fn new(mut base: DynFrame) -> Self {
        // C: frameset.c `astInitFrameSet` takes the arrays of a FrameSet
        // argument and copies only its `base`, `current` and a set Invert.
        if let Some(old) = base.as_mut().as_any_mut().downcast_mut::<FrameSet>() {
            return Self::taking_frames_of(old);
        }
        Self {
            frames: vec![base],
            edges: Vec::new(),
            base_idx: 0,
            current_idx: 0,
            base_set: false,
            current_set: false,
            frame_variants: vec![None],
            varfrm: vec![0],
            nnode: 1,
            node_map: vec![0],
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            use_defs: None,
            inverted: false,
        }
    }

    /// A FrameSet holding the Frames and Mappings of `old`, which is left
    /// empty, with its Base, Current and Invert.
    fn taking_frames_of(old: &mut FrameSet) -> Self {
        let mapping_flags = crate::mappings::MappingFlags {
            invert_zero: old.mapping_flags.invert_zero,
            ..Default::default()
        };
        Self {
            frames: std::mem::take(&mut old.frames),
            edges: std::mem::take(&mut old.edges),
            base_idx: old.base_idx,
            current_idx: old.current_idx,
            base_set: old.base_set,
            current_set: old.current_set,
            frame_variants: std::mem::take(&mut old.frame_variants),
            varfrm: std::mem::take(&mut old.varfrm),
            nnode: old.nnode,
            node_map: std::mem::take(&mut old.node_map),
            ident: None,
            id: Default::default(),
            mapping_flags,
            use_defs: None,
            inverted: old.inverted,
        }
    }

    // ------------------------------------------------------------------ //
    // Frame access                                                        //
    // ------------------------------------------------------------------ //

    /// The number of Frames in the FrameSet: the Nframe attribute.
    ///
    /// It changes as Frames are added or removed, and is always at least 1.
    #[doc(alias = "Nframe")]
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

    /// The base Frame, the one the Base attribute selects; see
    /// [`set_base`](FrameSet::set_base).
    pub fn base(&self) -> &dyn Frame {
        self.frames[self.base_idx].as_ref()
    }

    /// The current Frame, the one the Current attribute selects; see
    /// [`set_current`](FrameSet::set_current).
    pub fn current(&self) -> &dyn Frame {
        self.frames[self.current_idx].as_ref()
    }

    /// A Frame of the FrameSet.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    #[doc(alias = "astGetFrame")]
    pub fn frame(&self, which: FrameRef) -> AstResult<&dyn Frame> {
        let idx = self.resolve(which)?;
        Ok(self.frames[idx].as_ref())
    }

    /// A Frame of the FrameSet, to change: the escape hatch from the
    /// FrameSet's integrity.
    ///
    /// A change made through it leaves the FrameSet's Mappings as they are.
    /// Use [`with_integrity`](FrameSet::with_integrity) to change the
    /// current Frame and keep the Mappings consistent with it; see
    /// [`FrameSet`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    #[doc(alias = "astGetFrame")]
    pub fn frame_mut(&mut self, which: FrameRef) -> AstResult<&mut dyn Frame> {
        let idx = self.resolve(which)?;
        Ok(self.frames[idx].as_mut())
    }

    // ------------------------------------------------------------------ //
    // Base / current selection                                             //
    // ------------------------------------------------------------------ //

    /// Select the base Frame: set the Base attribute.
    ///
    /// The Base attribute is the index of the Frame regarded as the base
    /// Frame; [`base`](FrameSet::base) gives that Frame.
    /// Inverting the FrameSet exchanges its Base and Current.
    ///
    /// Default: 1, the first Frame added.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    #[doc(alias = "Base")]
    pub fn set_base(&mut self, which: FrameRef) -> AstResult<()> {
        let idx = self.resolve(which)?;
        self.base_idx = idx;
        self.base_set = true;
        Ok(())
    }

    /// Select the current Frame: set the Current attribute.
    ///
    /// The Current attribute is the index of the Frame regarded as the
    /// current Frame; [`current`](FrameSet::current) gives that Frame.
    /// Inverting the FrameSet exchanges its Base and Current.
    ///
    /// Default: [`nframe`](FrameSet::nframe), the most recent Frame added.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    #[doc(alias = "Current")]
    pub fn set_current(&mut self, which: FrameRef) -> AstResult<()> {
        let idx = self.resolve(which)?;
        self.current_idx = idx;
        self.current_set = true;
        Ok(())
    }

    /// The Base attribute: the 1-based index of the base Frame; see
    /// [`set_base`](FrameSet::set_base).
    ///
    /// While the FrameSet is inverted it is the index of the Frame that was
    /// current before inversion.
    #[doc(alias = "Base")]
    pub fn base_index(&self) -> usize {
        self.base_idx + 1
    }

    /// The Current attribute: the 1-based index of the current Frame; see
    /// [`set_current`](FrameSet::set_current).
    ///
    /// While the FrameSet is inverted it is the index of the Frame that was
    /// the base Frame before inversion.
    #[doc(alias = "Current")]
    pub fn current_index(&self) -> usize {
        self.current_idx + 1
    }

    /// Make the first Frame whose Domain is `domain` the base Frame: set the
    /// Base attribute by Domain name.
    ///
    /// Frames are searched in index order.
    /// The Domains are compared ignoring case and leading and trailing white
    /// space.
    /// Every name is a Domain here, including "Base" and "Current"; select
    /// those Frames with [`FrameRef::Base`] and [`FrameRef::Current`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` when no Frame has the Domain.
    #[doc(alias = "Base")]
    pub fn set_base_by_domain(&mut self, domain: &str) -> AstResult<()> {
        let idx = self.domain_index(domain, "Base")?;
        self.set_base(FrameRef::Index(idx))
    }

    /// Make the first Frame whose Domain is `domain` the current Frame: set
    /// the Current attribute by Domain name.
    ///
    /// Frames are searched in index order.
    /// The Domains are compared ignoring case and leading and trailing white
    /// space.
    /// Every name is a Domain here, including "Base" and "Current"; select
    /// those Frames with [`FrameRef::Base`] and [`FrameRef::Current`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` when no Frame has the Domain.
    #[doc(alias = "Current")]
    pub fn set_current_by_domain(&mut self, domain: &str) -> AstResult<()> {
        let idx = self.domain_index(domain, "Current")?;
        self.set_current(FrameRef::Index(idx))
    }

    /// The 1-based index of the first Frame whose Domain matches `domain`.
    // C: frameset.c `SetAttrib` reads the value with `%*s` after skipping
    // leading white space, then compares each Frame's Domain with
    // `astChrMatch`, which ignores case and trailing white space.
    fn domain_index(&self, domain: &str, attrib: &str) -> AstResult<usize> {
        let want = crate::mappings::helpers::chr_match_key(
            domain.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']),
        );
        self.frames
            .iter()
            .position(|f| crate::mappings::helpers::chr_match_key(f.domain()) == want)
            .map(|i| i + 1)
            .ok_or_else(|| {
                AstError::new(
                    ErrorCode::Attin,
                    format!(
                        "astSetAttrib(FrameSet): Invalid index value for {attrib} Frame \"{domain}\"."
                    ),
                )
            })
    }

    /// Whether the Base attribute is set; see
    /// [`set_base`](FrameSet::set_base).
    pub fn test_base(&self) -> bool {
        self.base_set
    }

    /// Clear the Base attribute, so that the first Frame is the base Frame,
    /// or the last Frame while the FrameSet is inverted; see
    /// [`set_base`](FrameSet::set_base).
    pub fn clear_base(&mut self) {
        self.base_set = false;
        self.base_idx = self.default_base_idx();
    }

    /// Whether the Current attribute is set; see
    /// [`set_current`](FrameSet::set_current).
    pub fn test_current(&self) -> bool {
        self.current_set
    }

    /// Clear the Current attribute, so that the last Frame is the current
    /// Frame, or the first Frame while the FrameSet is inverted; see
    /// [`set_current`](FrameSet::set_current).
    pub fn clear_current(&mut self) {
        self.current_set = false;
        self.current_idx = self.default_current_idx();
    }

    // C's GetBase and GetCurrent give an unset stored `base` the default 1
    // and an unset stored `current` the default Nframe, and exchange the two
    // while the FrameSet is inverted. `base_idx` and `current_idx` hold the
    // effective (already exchanged) Frames, so their defaults exchange too.

    /// The 0-based base Frame used when Base is not set.
    fn default_base_idx(&self) -> usize {
        if self.inverted {
            self.frames.len() - 1
        } else {
            0
        }
    }

    /// The 0-based current Frame used when Current is not set.
    fn default_current_idx(&self) -> usize {
        if self.inverted {
            0
        } else {
            self.frames.len() - 1
        }
    }

    /// Move an unset Base or Current to its default after the number of
    /// Frames changes, as C's defaults follow Nframe.
    fn track_defaults(&mut self) {
        if !self.base_set {
            self.base_idx = self.default_base_idx();
        }
        if !self.current_set {
            self.current_idx = self.default_current_idx();
        }
    }

    // ------------------------------------------------------------------ //
    // Graph mutation                                                       //
    // ------------------------------------------------------------------ //

    /// Add a Frame to the FrameSet to define a new coordinate system,
    /// derived from the one the `anchor` Frame describes.
    ///
    /// `map` converts coordinates from the `anchor` Frame's coordinate
    /// system to the new one with its forward transformation, and back with
    /// its inverse.
    /// The new Frame becomes the current Frame.
    /// Any class of Frame may be added, including a Region.
    ///
    /// Two cases are special:
    ///
    /// - When `new_frame` is itself a FrameSet, the two FrameSets are
    ///   merged, as [`add_frameset`](FrameSet::add_frameset) does: `map`
    ///   converts from the `anchor` Frame to that FrameSet's current Frame,
    ///   which becomes the current Frame.
    /// - When `anchor` is [`FrameRef::AllFrames`], `map` is ignored and the
    ///   axes of `new_frame` are appended to every Frame: each Frame is
    ///   replaced by a [`CmpFrame`](crate::frames::CmpFrame) of itself and
    ///   `new_frame`, and each Mapping by a parallel
    ///   [`CmpMap`](crate::mappings::CmpMap) of itself and a
    ///   [`UnitMap`](crate::mappings::UnitMap) with as many axes as
    ///   `new_frame`, simplified.
    ///
    /// A FrameSet given as `map` is used as the Mapping from its base Frame
    /// to its current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `anchor` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe).
    /// - `ErrorCode::Ncpin` when the [`nin`](Mapping::nin) of `map` is not
    ///   the number of axes of the `anchor` Frame, or its
    ///   [`nout`](Mapping::nout) is not that of `new_frame`.
    /// - Passes on the error of building or simplifying the Mappings when
    ///   appending axes.
    #[doc(alias = "astAddFrame")]
    // C: frameset.c `AddFrame`; the AST__ALLFRAMES case is at 1336.
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
        // C uses a FrameSet given as the Mapping as the Mapping from its
        // base Frame to its current Frame (frameset.c AddFrame).
        let map = match map.as_ref().as_any().downcast_ref::<FrameSet>() {
            Some(fs) => fs.mapping(FrameRef::Base, FrameRef::Current)?,
            None => map,
        };
        let anchor_idx = self.resolve(anchor)?;
        // C's astAddFrame checks the Mapping's Nin against the anchor Frame
        // and its Nout against the new Frame (frameset.c AddFrame).
        let naxes = self.frames[anchor_idx].naxes();
        if map.nin() != naxes {
            return Err(AstError::new_tracked(
                ErrorCode::Ncpin,
                format!(
                    "astAddFrame(FrameSet): Bad number of Mapping input coordinate values \
                     ({}); it should accept {naxes}.",
                    map.nin()
                ),
            ));
        }
        if map.nout() != new_frame.naxes() {
            return Err(AstError::new_tracked(
                ErrorCode::Ncpin,
                format!(
                    "astAddFrame(FrameSet): Bad number of Mapping output coordinate values \
                     ({}); it should generate {}.",
                    map.nout(),
                    new_frame.naxes()
                ),
            ));
        }
        // C merges a FrameSet given as the new Frame.
        if let Some(other) = new_frame.as_ref().as_any().downcast_ref::<FrameSet>() {
            return self.add_frameset(anchor, map, other.clone());
        }
        let anchor_node = self.node_map[anchor_idx];
        let new_node = self.nnode;
        self.nnode += 1;
        self.frames.push(new_frame);
        self.node_map.push(new_node);
        self.frame_variants.push(None);
        self.varfrm.push(0);
        self.edges.push(Edge::new(anchor_node, new_node, map));
        self.current_idx = self.frames.len() - 1;
        self.current_set = true;
        self.track_defaults();
        Ok(())
    }

    /// Merge `other` into the FrameSet.
    ///
    /// The Frames of `other` are appended, in order, keeping the Mappings
    /// between them, and `map` relates the `anchor` Frame to the current
    /// Frame of `other`, which becomes the current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `anchor` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    /// - `ErrorCode::Ncpin` when the [`nin`](Mapping::nin) of `map` is not
    ///   the number of axes of the `anchor` Frame, or its
    ///   [`nout`](Mapping::nout) is not that of the current Frame of
    ///   `other`.
    #[doc(alias = "astAddFrame")]
    // C: the FrameSet branch of frameset.c `AddFrame` (1360-1620): the
    // Frames and nodes of `other` are appended, its tree is re-rooted at
    // its current Frame by reversing each link on the path from that Frame
    // to its first node, and the new link is made from `anchor`.
    pub fn add_frameset(
        &mut self,
        anchor: FrameRef,
        map: DynMapping,
        other: FrameSet,
    ) -> AstResult<()> {
        let anchor_idx = self.resolve(anchor)?;
        if map.nin() != self.frames[anchor_idx].naxes() || map.nout() != other.current().naxes() {
            return Err(AstError::new_tracked(
                ErrorCode::Ncpin,
                "astAddFrame(FrameSet): the Mapping does not match the Frames it connects",
            ));
        }
        let anchor_node = self.node_map[anchor_idx];
        let nframe0 = self.frames.len();
        let node_zero = self.nnode;
        let current = other.current_idx + nframe0;
        let FrameSet {
            frames,
            edges,
            frame_variants,
            varfrm,
            nnode,
            node_map,
            ..
        } = other;
        self.frames.extend(frames);
        self.frame_variants.extend(frame_variants);
        self.varfrm.extend(
            varfrm
                .into_iter()
                .map(|v| if v > 0 { v + nframe0 } else { 0 }),
        );
        self.node_map
            .extend(node_map.into_iter().map(|n| n + node_zero));
        let mut new_edges: Vec<Edge> = edges
            .into_iter()
            .map(|e| Edge {
                from: e.from + node_zero,
                to: e.to + node_zero,
                mapping: e.mapping,
                object_invert: e.object_invert,
            })
            .collect();

        // Reverse each link on the path from the current Frame's node to the
        // first node, inverting its Mapping, so the current node is free to
        // take the new link.
        let current_node = self.node_map[current];
        let mut node = current_node;
        while node != node_zero {
            let k = new_edges
                .iter()
                .position(|e| e.to == node)
                .expect("every node but the first has a parent link");
            let mut e = new_edges.remove(k);
            let parent = e.from;
            e.mapping.invert();
            new_edges.push(Edge {
                from: node,
                to: parent,
                mapping: e.mapping,
                object_invert: e.object_invert,
            });
            node = parent;
        }
        new_edges.push(Edge::new(anchor_node, current_node, map));
        self.edges.extend(new_edges);
        // C keeps each node's link at the node's own index.
        self.edges.sort_by_key(|e| e.to);
        self.nnode += nnode;
        self.current_idx = current;
        self.current_set = true;
        self.track_defaults();
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
            let mapping = <dyn Mapping>::simplify(&cmp)?;
            new_edges.push(Edge::new(edge.from, edge.to, mapping));
        }
        self.edges = new_edges;
        Ok(())
    }

    /// Remove a Frame from the FrameSet.
    ///
    /// The other Frames are renumbered from 1 if necessary, but are
    /// otherwise unchanged, and so are the relationships between them, even
    /// those that depended on the Frame removed.
    /// Removing the base or current Frame clears the Base or Current
    /// attribute, so that another Frame takes its role by default; removing
    /// any other Frame leaves the base and current Frames the same.
    /// Any Frame that mirrored the removed Frame's variant Mappings no
    /// longer mirrors.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Remin` when the FrameSet has only one Frame.
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    /// - Passes on the error of simplifying the Mappings joined where the
    ///   Frame was.
    #[doc(alias = "astRemoveFrame")]
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
        // it is set (frameset.c RemoveFrame, `astTestBase`): if the removed
        // frame *was* the base, clear the attribute so the default applies;
        // if it was below the base, decrement. Current is treated the same
        // way. An unset attribute then takes its default, which is the
        // first or last Frame, so it moves when the last Frame does.
        if self.base_set {
            if self.base_idx == idx {
                self.base_set = false;
            } else if self.base_idx > idx {
                self.base_idx -= 1;
            }
        }
        if self.current_set {
            if self.current_idx == idx {
                self.current_set = false;
            } else if self.current_idx > idx {
                self.current_idx -= 1;
            }
        }
        self.track_defaults();

        self.tidy_nodes()?;
        Ok(())
    }

    /// Modify a Frame's relationship to the other Frames in the FrameSet.
    ///
    /// `map` converts coordinates from the Frame's original coordinate
    /// system to the new one with its forward transformation, and back with
    /// its inverse.
    /// This might be needed when the FrameSet calibrates an image that is
    /// then re-binned: the image's Frame has undergone a coordinate
    /// transformation, which is communicated to the FrameSet this way.
    /// The relationships between the other Frames are unchanged.
    /// It cannot change the number of axes of a Frame: add a new Frame and
    /// remove the old one for that.
    /// For a change of axis order, [`perm_axes`](FrameSet::perm_axes) may be
    /// simpler.
    ///
    /// The Frame's variant Mappings, except the one in use, are removed
    /// (see [`variant`](FrameSet::variant)), even when the remapping then
    /// fails; a Frame that mirrors another's variants keeps the mirror.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    /// - `ErrorCode::Ncpin` when the [`nin`](Mapping::nin) or
    ///   [`nout`](Mapping::nout) of `map` is not the Frame's number of axes.
    /// - Passes on the error of simplifying the Mappings joined by the
    ///   remapping.
    #[doc(alias = "astRemapFrame")]
    // C: frameset.c `RemapFrame` (8486). The old node keeps its place in
    // the graph and the Frame moves to a new node linked to it by `map`;
    // `tidy_nodes` then collapses any node left redundant.
    pub fn remap_frame(&mut self, which: FrameRef, map: DynMapping) -> AstResult<()> {
        let idx = self.resolve(which)?;
        // C clears the Variant of the Frame before remapping it, since its
        // variant Mappings are stored against the node it leaves; a Frame
        // that only mirrors another's variants keeps the mirror
        // (frameset.c RemapFrame).
        self.frame_variants[idx] = None;
        self.remap_node(idx, map)
    }

    /// Move the Frame at 0-based `idx` to a new node linked to its old one
    /// by `map`, keeping its variant Mappings.
    fn remap_node(&mut self, idx: usize, map: DynMapping) -> AstResult<()> {
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
        self.edges.push(Edge::new(old_node, new_node, map));
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
            // C `TidyNodes` (frameset.c:11191) calls `astSimplify` on the
            // combined Mapping, which sets the IsSimple flag (→ `IsSimp = 1`
            // in the native dump). Use `simplify()` (not the internal
            // `simplified()`) so the stored edge carries that flag.
            let simplified = (combined.as_ref() as &dyn Mapping).simplify()?;

            // Remove the two edges (highest index first) and insert
            // the combined edge.
            let (a, b) = if e1 > e2 { (e1, e2) } else { (e2, e1) };
            self.edges.remove(a);
            self.edges.remove(b);
            self.edges.push(Edge::new(mid_a, mid_b, simplified));

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

    /// A Mapping that converts coordinates between two Frames of the
    /// FrameSet.
    ///
    /// Its forward transformation converts from the coordinate system of
    /// `from` to that of `to`, and its inverse back.
    /// It includes the masking effect of any Region on the path between the
    /// two Frames, the two Frames themselves included.
    /// For the same Frame at both ends, it is a
    /// [`UnitMap`](crate::mappings::UnitMap), unless that Frame is a Region.
    /// A Mapping can always be made, but it may not be able to perform the
    /// conversion: [`has_forward`](Mapping::has_forward) and
    /// [`has_inverse`](Mapping::has_inverse) tell.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `from` or `to` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    /// - Passes on the error of composing the Mappings on the path.
    #[doc(alias = "astGetMapping")]
    // C: frameset.c `GetMapping` and `Span` (10263); the same-Frame case is
    // at 5436-5450. The path is found breadth-first, and a Frame on it that
    // is not equivalent to a UnitMap (`is_unit_frame`) is included as a
    // Mapping at its node.
    pub fn mapping(&self, from: FrameRef, to: FrameRef) -> AstResult<DynMapping> {
        let from_idx = self.resolve(from)?;
        let to_idx = self.resolve(to)?;
        if from_idx == to_idx {
            if let Some(map) = self.frame_mapping(self.node_map[from_idx]) {
                return Ok(map);
            }
            let naxes = self.frames[from_idx].naxes();
            return Ok(Box::new(UnitMap::new(naxes)));
        }
        let from_node = self.node_map[from_idx];
        let to_node = self.node_map[to_idx];
        let path = self.find_path(from_node, to_node)?;
        self.compose_path(&path)
    }

    /// The Mapping the Frame at `node` contributes to a path through it, if
    /// it is not equivalent to a UnitMap (frameset.c `Span`). Only a Region
    /// is such a Frame here, and a Region is its own Mapping.
    fn frame_mapping(&self, node: usize) -> Option<DynMapping> {
        let idx = self.node_map.iter().position(|&n| n == node)?;
        let frame = self.frames[idx].as_ref();
        if frame.is_unit_frame() {
            return None;
        }
        let region = crate::regions::any_as_region(frame.as_any())
            .expect("a Frame that is not a unit Frame is a Region");
        Some(region.clone_box())
    }

    // ------------------------------------------------------------------ //
    // Integrity                                                           //
    // ------------------------------------------------------------------ //

    /// Change the current Frame with `f`, keeping the FrameSet's integrity.
    ///
    /// After `f` has run, the Mapping from the current Frame's old
    /// coordinate system to its new one is found and, unless it is a
    /// [`UnitMap`](crate::mappings::UnitMap), the current Frame is remapped
    /// through it with [`remap_frame`](FrameSet::remap_frame), which also
    /// drops its variant Mappings but the one in use.
    /// Setting a Frame attribute through the [`Frame`] trait on a FrameSet
    /// does this; see [`FrameSet`].
    ///
    /// # Errors
    ///
    /// Passes on the error of `f`, of finding or simplifying the Mapping
    /// between the old and new Frame, or of the remapping.
    // C: frameset.c `RestoreIntegrity`, which calls `astRemapFrame` (8486)
    // and so `TidyNodes` (11076); simplifying the joined Mappings there is
    // where inverse pairs from a round trip such as ICRS -> FK5 -> ICRS
    // cancel.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        let snap = self.frames[self.current_idx].snapshot();
        f(self.frames[self.current_idx].as_mut())?;
        let delta = self.frames[self.current_idx].map_from(snap.as_ref())?;
        let delta = delta.simplify()?;
        // C remaps only when the Mapping is not a UnitMap (frameset.c
        // RestoreIntegrity). The boxed Mapping is tested, not the box.
        if delta.as_ref().as_any().is::<UnitMap>() {
            return Ok(());
        }
        self.remap_frame(FrameRef::Current, delta)?;
        Ok(())
    }

    // ------------------------------------------------------------------ //
    // find_frame                                                          //
    // ------------------------------------------------------------------ //

    /// Find a coordinate system with specified characteristics in this
    /// FrameSet.
    ///
    /// The search of [`FrameSearch::find_frame_template`](crate::frames::FrameSearch::find_frame_template) with this FrameSet as
    /// the target, which describes it in full.
    /// Each entry of `domains` is taken in turn, and within it `template` is
    /// matched against the FrameSet's Frames in the order current Frame,
    /// base Frame, then the others in the order they were added; the first
    /// match whose Frame has a Domain matching the entry is accepted.
    /// An empty entry, or an empty slice, accepts any Domain.
    ///
    /// The result is a FrameSet of two Frames: the base Frame of this
    /// FrameSet, and the Frame found, related by the Mapping from this
    /// FrameSet's base Frame through the Frame matched to the Frame found.
    /// When a match is found, the Frame matched becomes this FrameSet's
    /// current Frame, so [`current_index`](FrameSet::current_index) tells
    /// which it was.
    ///
    /// # Errors
    ///
    /// Passes on the error of [`FrameSearch::find_frame_template`](crate::frames::FrameSearch::find_frame_template) on the Frames
    /// searched, or of [`add_frame`](FrameSet::add_frame) building the
    /// result.
    #[doc(alias = "astFindFrame")]
    pub fn find_frame(
        &mut self,
        template: &dyn Frame,
        domains: &[&str],
    ) -> AstResult<Option<FrameSet>> {
        let domainlist = super::find_frame::domain_list(domains);
        FrameSearch::find_frame_template(self, template, &domainlist)
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
    /// variant in use.
    ///
    /// `map` converts coordinates from the current Frame to the new variant
    /// of it.
    /// With no `map`, the variant in use is renamed `name` and no variant is
    /// added.
    /// The variant named becomes the one in use, as
    /// [`set_variant`](FrameSet::set_variant) makes it.
    /// `name` is stored with leading and trailing white space removed and
    /// letters in upper case.
    /// See [`variant`](FrameSet::variant) for what variants are.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Mirro` when the current Frame mirrors another's
    ///   variants ([`mirror_variants`](FrameSet::mirror_variants)).
    /// - `ErrorCode::Bdvnm` when the current Frame already has a variant
    ///   named `name`.
    /// - Passes on the error of finding the Mapping from the current Frame
    ///   to its variants, or of selecting the variant.
    #[doc(alias = "astAddVariant")]
    // C: frameset.c `AddVariant` (1645).
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
                vfs.frame_mut(FrameRef::Current)?.set_domain(&myname)?;
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
                let map3 = <dyn Mapping>::simplify(map3.as_ref())?;

                // The new variant frame is a copy of the parent frame with its
                // own variants cleared and Domain set to the variant name.
                let mut vfrm = self.frames[icur].snapshot();
                vfrm.set_domain(&myname)?;

                let saved_cur = vfs.current_idx;
                vfs.add_frame(FrameRef::Current, map3, vfrm)?;
                vfs.current_idx = saved_cur;
                vfs.current_set = true;
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

    /// Select the variant Mapping the current Frame uses; see
    /// [`variant`](FrameSet::variant).
    ///
    /// `name` is compared with leading and trailing white space removed,
    /// ignoring case.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` when the current Frame has no variant named
    ///   `name`; with no variants, its only one is named by its Domain.
    /// - Passes on the error of finding the Mappings between the variants
    ///   and the current Frame, or of the remapping.
    // C: frameset.c `SetVariant` (9894).
    pub fn set_variant(&mut self, name: &str) -> AstResult<()> {
        let icur = self.current_idx;
        let owner = self.get_var_frm(icur + 1) - 1; // 0-based variant owner

        let myvar = name.trim().to_uppercase();

        // With no Variants FrameSet the only variant is the current Frame's
        // own, named by its Domain; selecting it does nothing.
        let Some(vfs) = &self.frame_variants[owner] else {
            if myvar == self.frames[icur].domain() {
                return Ok(());
            }
            return Err(AstError::new(
                ErrorCode::Attin,
                format!("astSetVariant(FrameSet): Unknown Frame variant '{myvar}' requested."),
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
                    "astSetVariant(FrameSet): Unknown Frame variant '{myvar}' requested - available variants are '{}'.",
                    self.all_variants()
                ),
            ));
        }

        // Already selected: nothing to do.
        if target_1based == vfs.current_idx + 1 {
            return Ok(());
        }

        // Build the Mapping from the old variant to the new one, in the
        // coordinates of the variant owner, which for a mirror is not the
        // current Frame (frameset.c:9998-10032):
        //   map1 = owner        -> current(vfs)   [old variant]
        //   map2 = current(vfs) -> target(vfs)    [old variant -> new variant]
        //   map3 = target(vfs)  -> owner          [new variant]
        let frm = self.frames[owner].snapshot();
        let vcur = vfs.current().snapshot();
        let map1 = convert_current_to(frm.as_ref(), vcur.as_ref())?;
        let map2 = vfs.mapping(FrameRef::Current, FrameRef::Index(target_1based))?;
        let vnew = vfs.frame(FrameRef::Index(target_1based))?.snapshot();
        let map3 = convert_current_to(vnew.as_ref(), frm.as_ref())?;

        let map5: DynMapping = Box::new(CmpMap::new(
            Box::new(CmpMap::new(map1, map2, Composition::Series)?),
            map3,
            Composition::Series,
        )?);

        // C replaces the Mapping that generates the owner's node, from its
        // parent node, with that Mapping followed by `map5`, simplified, so
        // every Frame derived from the owner's node moves with it.
        let inode = self.node_map[owner];
        if inode == 0 {
            // The root node has no parent Mapping; C indexes `map[-1]` there
            // (docs/issues/c-library-quirks.md). The port moves the owner to
            // a new node instead, so Frames derived from it do not move.
            let correction = <dyn Mapping>::simplify(map5.as_ref())?;
            self.remap_node(owner, correction)?;
        } else {
            let parent = self.find_path(inode, 0)?[1];
            let map0 = self.edge_mapping(parent, inode)?;
            let k = self
                .edges
                .iter()
                .position(|e| {
                    (e.from, e.to) == (parent, inode) || (e.from, e.to) == (inode, parent)
                })
                .expect("a node on a path has a link to its parent");
            // C sets the Invert of the link's Mapping object to the link's
            // flag, makes the CmpMap, which records that direction as its
            // `InvA`, simplifies it, and then restores the object's own Invert
            // (frameset.c `SetVariant`, `astSetInvert( map0, inv )`). The
            // CmpMap therefore holds the object in its own orientation, which
            // is what a component the simplification leaves alone is written
            // with.
            let mut map6 = CmpMap::new(map0, map5, Composition::Series)?;
            map6.set_first_orientation(self.edges[k].object_invert);
            let new_map = <dyn Mapping>::simplify(&map6)?;
            // C records the Invert of the simplified Mapping as the link's
            // flag, as `Edge::new` does.
            self.edges[k] = Edge::new(parent, inode, new_map);
        }
        if let Some(vfs) = &mut self.frame_variants[owner] {
            vfs.current_idx = target_1based - 1;
            vfs.current_set = true;
        }
        Ok(())
    }

    /// The name of the variant Mapping in use by the current Frame: the
    /// Variant attribute.
    ///
    /// By default each Frame is connected to the others by one Mapping,
    /// which only [`remap_frame`](FrameSet::remap_frame) changes.
    /// A Frame may instead have several variant Mappings, each with a name,
    /// and selecting one with [`set_variant`](FrameSet::set_variant) remaps
    /// the current Frame to use it.
    ///
    /// For instance, an image made by co-adding images of two parts of the
    /// sky has two sky positions in each pixel, so its WCS could hold two
    /// SkyFrames.
    /// Software often finds a SkyFrame by its Domain, `"SKY"`, rather than
    /// its class, so the two cannot be told apart by giving them different
    /// Domains; instead one SkyFrame can be added with two variant Mappings,
    /// and the variant selected decides which part of the sky it describes.
    /// Any class of Frame can have variants.
    ///
    /// A Frame is added as usual with [`add_frame`](FrameSet::add_frame),
    /// whose Mapping is the variant named by the Frame's Domain;
    /// [`add_variant`](FrameSet::add_variant) with no Mapping renames it,
    /// and with a Mapping adds a further variant.
    /// Each Frame has its own variants, and to work with a Frame's variants
    /// it must be the current Frame.
    /// [`mirror_variants`](FrameSet::mirror_variants) lets another Frame
    /// share them.
    /// [`clear_variant`](FrameSet::clear_variant) removes all but the one in
    /// use, as remapping the Frame does,
    /// [`test_variant`](FrameSet::test_variant) tells whether the current
    /// Frame has variants, and [`all_variants`](FrameSet::all_variants)
    /// lists them.
    ///
    /// Default: the Domain of the current Frame, when it has no variants.
    #[doc(alias = "Variant")]
    // C: frameset.c `GetVariant` (6041): the Domain of the current Frame
    // of the (mirror-resolved) Variants FrameSet.
    pub fn variant(&self) -> String {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        match &self.frame_variants[owner] {
            Some(vfs) => vfs.current().domain().to_string(),
            None => self.frames[self.current_idx].domain().to_string(),
        }
    }

    /// Whether the current Frame has variant Mappings, its own or those of a
    /// Frame it mirrors; see [`variant`](FrameSet::variant).
    // C: frameset.c `TestVariant` (11010).
    pub fn test_variant(&self) -> bool {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        self.frame_variants[owner].is_some()
    }

    /// Remove the variant Mappings of the current Frame, or of the Frame it
    /// mirrors, except the one in use; see [`variant`](FrameSet::variant).
    // C: frameset.c `ClearVariant` (2905).
    pub fn clear_variant(&mut self) {
        let owner = self.get_var_frm(self.current_idx + 1) - 1;
        self.frame_variants[owner] = None;
    }

    /// The names of all the variant Mappings of the current Frame,
    /// separated by spaces: the read-only AllVariants attribute.
    ///
    /// With no variants it is the Domain of the current Frame; see
    /// [`variant`](FrameSet::variant).
    #[doc(alias = "AllVariants")]
    // C: frameset.c `GetAllVariants` (6864).
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

    /// Make the current Frame mirror the variant Mappings of another Frame.
    ///
    /// All access to the variants of the current Frame is then forwarded to
    /// `which`: selecting a variant applies to `which` rather than the
    /// current Frame, and [`variant`](FrameSet::variant) reports that of
    /// `which`.
    /// This propagates the effect of variant Mappings around a FrameSet:
    /// a Frame added by connecting it to one with several variant Mappings
    /// can mirror it, so that a variant can be selected through either.
    /// Mirrors can be chained: a mirror of a mirror of `A` mirrors `A`.
    /// While mirroring, the current Frame's own variants are ignored and
    /// [`add_variant`](FrameSet::add_variant) fails.
    /// Giving the current Frame itself ends any mirroring.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Frmin` when `which` is an index outside 1 to
    ///   [`nframe`](FrameSet::nframe), or is [`FrameRef::AllFrames`].
    #[doc(alias = "astMirrorVariants")]
    // C: frameset.c `MirrorVariants` (7468).
    pub fn mirror_variants(&mut self, which: FrameRef) -> AstResult<()> {
        let icur = self.current_idx;
        let target = self.resolve(which)?; // 0-based
        self.varfrm[icur] = if target == icur { 0 } else { target + 1 };
        Ok(())
    }

    /// Permute the axes of the current Frame, and remap it so that each
    /// coordinate value stays with its axis.
    ///
    /// `perm` lists the 1-based axes in their new order; see
    /// [`Frame::perm_axes`].
    /// The current Frame is remapped through a
    /// [`PermMap`](crate::mappings::PermMap), as
    /// [`remap_frame`](FrameSet::remap_frame) does, so its variant Mappings
    /// but the one in use are removed.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Prmin` when `perm` is not a permutation of the current
    ///   Frame's axes.
    /// - Passes on the error of permuting the Frame or of the remapping.
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

        // Keep the coordinate values with their permuted axes by remapping
        // the current Frame through the PermMap (frameset.c `PermAxes`).
        self.remap_frame(FrameRef::Current, Box::new(pm))
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

        // C's `Span` lists, for each node, its Frame's Mapping (when the Frame
        // is not a unit Frame) and then the edge to the next node, ending
        // with the last node's Frame; `GetMapping` joins them in series in
        // that order.
        let mut steps: Vec<DynMapping> = Vec::new();
        for pair in path.windows(2) {
            steps.extend(self.frame_mapping(pair[0]));
            steps.push(self.edge_mapping(pair[0], pair[1])?);
        }
        steps.extend(self.frame_mapping(path[path.len() - 1]));
        let mut steps = steps.into_iter();
        let mut result = steps.next().expect("a path has at least one edge");
        for next in steps {
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

/// A Frame-class attribute of a FrameSet: read from its current Frame, and
/// set or cleared on it with FrameSet integrity restored afterwards, as C's
/// FrameSet does for every Frame attribute (frameset.c, the attribute
/// access macros and `RestoreIntegrity`).
macro_rules! frameset_attr {
    (@get $name:ident -> $ty:ty) => {
        fn $name(&self) -> $ty {
            self.current().$name()
        }
    };
    (@set $name:ident($arg:ident: $ty:ty)) => {
        fn $name(&mut self, $arg: $ty) -> AstResult<()> {
            self.with_integrity(|f| f.$name($arg))
        }
    };
    (@clear $name:ident) => {
        fn $name(&mut self) -> AstResult<()> {
            self.with_integrity(|f| f.$name())
        }
    };
    // A per-axis attribute: the axis index comes first.
    (@axis $get:ident: $ty:ty, $set:ident($arg:ty), $test:ident, $clear:ident) => {
        fn $get(&self, axis: usize) -> AstResult<$ty> {
            self.current().$get(axis)
        }
        fn $set(&mut self, axis: usize, value: $arg) -> AstResult<()> {
            self.with_integrity(|f| f.$set(axis, value))
        }
        fn $test(&self, axis: usize) -> AstResult<bool> {
            self.current().$test(axis)
        }
        fn $clear(&mut self, axis: usize) -> AstResult<()> {
            self.with_integrity(|f| f.$clear(axis))
        }
    };
    ($get:ident: $ty:ty, $set:ident, $test:ident, $clear:ident) => {
        frameset_attr!(@get $get -> $ty);
        frameset_attr!(@set $set(value: $ty));
        frameset_attr!(@get $test -> bool);
        frameset_attr!(@clear $clear);
    };
}

impl Frame for FrameSet {
    fn axis_perm(&self) -> Vec<usize> {
        self.current().axis_perm()
    }

    fn title(&self) -> AstResult<String> {
        self.current().title()
    }
    fn set_title(&mut self, title: &str) -> AstResult<()> {
        self.with_integrity(|f| f.set_title(title))
    }
    frameset_attr!(@get test_title -> bool);
    frameset_attr!(@clear clear_title);
    fn domain(&self) -> &str {
        self.current().domain()
    }
    fn set_domain(&mut self, domain: &str) -> AstResult<()> {
        self.with_integrity(|f| f.set_domain(domain))
    }
    frameset_attr!(@get test_domain -> bool);
    frameset_attr!(@clear clear_domain);
    frameset_attr!(epoch_mjd: f64, set_epoch_mjd, test_epoch, clear_epoch);
    frameset_attr!(digits: i32, set_digits, test_digits, clear_digits);
    frameset_attr!(@axis label: String, set_label(&str), test_label, clear_label);
    frameset_attr!(@axis symbol: String, set_symbol(&str), test_symbol, clear_symbol);
    frameset_attr!(@axis unit: String, set_unit(&str), test_unit, clear_unit);
    frameset_attr!(@axis format_str: String, set_format_str(&str), test_format_str, clear_format_str);
    frameset_attr!(@axis axis_digits: i32, set_axis_digits(i32), test_axis_digits, clear_axis_digits);
    frameset_attr!(@axis direction: bool, set_direction(bool), test_direction, clear_direction);
    frameset_attr!(@axis top: f64, set_top(f64), test_top, clear_top);
    frameset_attr!(@axis bottom: f64, set_bottom(f64), test_bottom, clear_bottom);
    fn axis_object_top(&self, axis: usize) -> AstResult<f64> {
        self.current().axis_object_top(axis)
    }
    fn axis_object_bottom(&self, axis: usize) -> AstResult<f64> {
        self.current().axis_object_bottom(axis)
    }
    frameset_attr!(obs_lat: f64, set_obs_lat, test_obs_lat, clear_obs_lat);
    frameset_attr!(obs_lon: f64, set_obs_lon, test_obs_lon, clear_obs_lon);
    frameset_attr!(obs_alt: f64, set_obs_alt, test_obs_alt, clear_obs_alt);
    frameset_attr!(dtai: f64, set_dtai, test_dtai, clear_dtai);
    frameset_attr!(dut1: f64, set_dut1, test_dut1, clear_dut1);
    frameset_attr!(min_axes: usize, set_min_axes, test_min_axes, clear_min_axes);
    frameset_attr!(max_axes: usize, set_max_axes, test_max_axes, clear_max_axes);
    frameset_attr!(match_end: bool, set_match_end, test_match_end, clear_match_end);
    frameset_attr!(preserve_axes: bool, set_preserve_axes, test_preserve_axes, clear_preserve_axes);
    frameset_attr!(permute: bool, set_permute, test_permute, clear_permute);

    fn get_active_unit(&self) -> bool {
        self.current().get_active_unit()
    }
    fn set_active_unit(&mut self, value: bool) {
        let _ = self.with_integrity(|f| {
            f.set_active_unit(value);
            Ok(())
        });
    }
    fn test_active_unit(&self) -> bool {
        self.current().test_active_unit()
    }

    // Ident is the FrameSet's own Object-class attribute, not its current
    // Frame's (object.c).
    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }
    fn set_ident(&mut self, ident: &str) -> AstResult<()> {
        self.ident = Some(ident.to_string());
        Ok(())
    }
    fn clear_ident(&mut self) -> AstResult<()> {
        self.ident = None;
        Ok(())
    }
    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) -> AstResult<()> {
        self.id.set(id);
        Ok(())
    }
    fn clear_id(&mut self) -> AstResult<()> {
        self.id.clear();
        Ok(())
    }
    fn test_id(&self) -> bool {
        self.id.test()
    }
    // UseDefs, as C's attribute access gives it: setting, clearing and
    // testing it act on the current Frame, as for every Frame attribute,
    // and an unset value comes from the current Frame (frameset.c
    // `GetUseDefs`). A value stored on the FrameSet itself, which only a
    // dump can supply, takes precedence and is written back out.
    fn stored_use_defs(&self) -> Option<bool> {
        self.use_defs.or_else(|| self.current().stored_use_defs())
    }
    fn store_use_defs(&mut self, value: Option<bool>) {
        self.use_defs = value;
    }
    fn use_defs(&self) -> bool {
        self.use_defs.unwrap_or_else(|| self.current().use_defs())
    }
    // UseDefs changes no coordinates, so integrity is left alone, as C's
    // leaves the Mappings untouched.
    fn set_use_defs(&mut self, value: bool) -> AstResult<()> {
        self.frame_mut(FrameRef::Current)?.set_use_defs(value)
    }
    fn clear_use_defs(&mut self) -> AstResult<()> {
        self.frame_mut(FrameRef::Current)?.clear_use_defs()
    }

    fn naxes(&self) -> usize {
        self.current().naxes()
    }

    /// The current Frame's axes are matched.
    // C: `FrameSet::MatchAxesX` (frameset.c:7398).
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.current().match_axes_x(frm1)
    }

    /// The current Frame's primary Frame.
    // C: frameset.c `PrimaryFrame`.
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        self.current().primary_frame(axis)
    }

    // C: frameset.c `IsUnitFrame`.
    fn is_unit_frame(&self) -> bool {
        self.current().is_unit_frame()
    }

    /// The same as [`FrameSet::perm_axes`].
    // C: frameset.c `PermAxes`.
    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        FrameSet::perm_axes(self, perm)
    }

    /// Resolved in the current Frame, so on great circles for a SkyFrame.
    // C: frameset.c `Resolve`.
    fn resolve(
        &self,
        point1: &[f64],
        point2: &[f64],
        point3: &[f64],
    ) -> AstResult<(Vec<f64>, f64, f64)> {
        self.current().resolve(point1, point2, point3)
    }

    // C: frameset.c `ResolvePoints`.
    fn resolve_points(
        &self,
        point1: &[f64],
        point2: &[f64],
        input: &crate::coords::PointSet,
    ) -> AstResult<crate::coords::PointSet> {
        self.current().resolve_points(point1, point2, input)
    }

    // C: frameset.c `Intersect`.
    fn intersect(&self, a1: &[f64], a2: &[f64], b1: &[f64], b2: &[f64]) -> AstResult<Vec<f64>> {
        self.current().intersect(a1, a2, b1, b2)
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

    fn offset2(&self, point1: &[f64], angle: f64, offset: f64) -> AstResult<(Vec<f64>, f64)> {
        self.current().offset2(point1, angle, offset)
    }

    /// A FrameSet's axes are its current Frame's axes.
    // C: `FrameSet::GetAxis` (frameset.c:4948).
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.current().get_axis(axis)
    }

    fn norm_box(
        &self,
        lbnd: &mut [f64],
        ubnd: &mut [f64],
        inside: &dyn Fn(&[f64]) -> AstResult<bool>,
    ) -> AstResult<()> {
        self.current().norm_box(lbnd, ubnd, inside)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.current().norm(coords)
    }

    /// The current Frame makes the grid.
    // C: frameset.c `FrameGrid`.
    fn frame_grid(
        &self,
        size: i32,
        lbnd: &[f64],
        ubnd: &[f64],
    ) -> AstResult<crate::coords::PointSet> {
        self.current().frame_grid(size, lbnd, ubnd)
    }

    /// The current Frame answers, as its Axis is the FrameSet's.
    // C: frame.c `GetInternalUnit` reads the Axis that frameset.c
    // `GetAxis` takes from the current Frame.
    fn internal_unit(&self, axis: usize) -> AstResult<String> {
        self.current().internal_unit(axis)
    }

    /// The current Frame answers, as its Axis is the FrameSet's.
    // C: frame.c `GetNormUnit` reads the Axis that frameset.c `GetAxis`
    // takes from the current Frame, and the Unit through frameset.c
    // `GetUnit`.
    fn norm_unit(&self, axis: usize) -> AstResult<String> {
        self.current().norm_unit(axis)
    }

    /// The current Frame normalizes the points.
    // C: frameset.c `NormalPoints`.
    fn normal_points(
        &self,
        points: &crate::coords::PointSet,
        contig: bool,
    ) -> AstResult<crate::coords::PointSet> {
        self.current().normal_points(points, contig)
    }

    fn frame_without_regions(&self) -> Option<DynFrame> {
        self.without_regions().map(|fs| Box::new(fs) as DynFrame)
    }

    fn snapshot(&self) -> DynFrame {
        // A copy of a FrameSet is a FrameSet, as C `astCopy` gives
        // (`object.c:1174`): the class has to survive, because a FrameSet used
        // as a Frame keeps its internal Mappings and is dumped as a FrameSet.
        // Callers that want the Frame-level state a FrameSet presents — an
        // integrity delta, or the Frame a Region encapsulates — take
        // `current().snapshot()` explicitly.
        Box::new(self.clone())
    }

    /// A FrameSet simplifies through its Mapping side.
    /// That reduction can hand back something that is no longer a Frame,
    /// which the helper's conversion drops.
    // C: there is one `Simplify` slot (`frameset.c:10111`); the case that is
    // no longer a Frame is the one `astIsAFrame` guards (`cmpframe.c:9024`).
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        // The Frame-level state a FrameSet presents is its current Frame's, so
        // a delta against an earlier FrameSet is a delta between the two
        // current Frames.
        if let Some(previous) = previous.as_any().downcast_ref::<FrameSet>() {
            return self.current().map_from(previous.current());
        }
        self.current().map_from(previous)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<FrameSet>() else {
            return false;
        };
        if self.frames.len() != other.frames.len() {
            return false;
        }
        // C compares the stored Base and Current, so a set index differs
        // from an unset one even when they select the same Frame.
        if (
            self.base_idx,
            self.base_set,
            self.current_idx,
            self.current_set,
        ) != (
            other.base_idx,
            other.base_set,
            other.current_idx,
            other.current_set,
        ) {
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

    fn system_string(&self) -> AstResult<Option<&str>> {
        self.current().system_string()
    }

    // ------------------------------------------------------------------
    // C forwards every remaining Frame virtual to the current frame
    // (frameset.c:6260-6350); without these, trait defaults (constant
    // values) would silently apply.
    // ------------------------------------------------------------------

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        // C `FrameSet::Match` (frameset.c:7173) calls `astMatch` on the
        // current Frame, so the cast retry applies to it.
        crate::frames::find_frame::match_with_subclass(self.current(), target, match_sub)
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

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        // frameset.c `Overlay`: the current Frame is the template.
        self.current().overlay(template_axes, result)
    }

    fn axis_overlay_source(&self, axis: usize) -> Option<super::overlay::AxisOverlaySource> {
        self.current().axis_overlay_source(axis)
    }

    fn apply_axis_overlay(&mut self, axis: usize, source: &super::overlay::AxisOverlaySource) {
        self.frames[self.current_idx].apply_axis_overlay(axis, source);
    }

    fn overlay_system_from(&mut self, template: &dyn std::any::Any) -> AstResult<()> {
        self.frames[self.current_idx].overlay_system_from(template)
    }
}

/// `FrameSet::FindFrame` override (frameset.c:3790).
///
/// Iterate the FrameSet's frames in priority order — current first,
/// base next, then everything else — calling
/// `FrameSearch::find_frame_template` on each. The first successful match
/// wins. The resulting Mapping is prefixed by the FrameSet's
/// base→matched-frame Mapping and simplified, then packaged as a
/// 2-frame FrameSet (base = self.base, current = matched-frame's
/// result), returned with the 1-based index of the Frame matched, which
/// [`FrameSearch::find_frame_template`](crate::frames::FrameSearch::find_frame_template) makes the target's current Frame
/// (frameset.c:4022).
///
/// `domainlist` is the standard comma-separated, case-insensitive,
/// whitespace-stripped priority list. Each field (including the
/// blank one introduced by adjacent commas) is tried in order.
pub(crate) fn frameset_find_frame(
    target: &FrameSet,
    template: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<(FrameSet, usize)>> {
    let nframe = target.nframe();
    let current_idx = target.current_idx() + 1; // 1-based
    let base_idx = target.base_idx() + 1;

    // Build search order: current, then base (if different), then
    // every other frame in 1..=nframe (frameset.c `FindFrame`, 3936-3947).
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
            let Some(found) =
                crate::frames::find_frame::find_frame(selected, template, domain_field)?
            else {
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
            let simplified = <dyn Mapping>::simplify(combined.as_ref())?;

            // Build the result 2-frame FrameSet.
            let mut result_fs = FrameSet::new(target.base().snapshot());
            result_fs.add_frame(FrameRef::Base, simplified, result_frame)?;
            return Ok(Some((result_fs, target_idx_1based)));
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
    a.set_domain(&dom)?;
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

/// `FrameSet::ConvertX` (frameset.c:3204). C reaches it from frameset.c
/// `Convert` when `from` is a FrameSet, and from frame.c `Convert`, which
/// dispatches `astConvertX` on the class of `to`, when `to` is one.
///
/// Find a conversion route between `from` and `to` by trying *every* member
/// frame of each as a candidate alignment endpoint, not just the current
/// frames; either may be a plain Frame, which is its own single member, as C
/// treats it. For each `(from_member, to_member)` pair the per-pair alignment
/// is found via [`convert_frames`], the `from`-current→member Mapping is
/// prefixed, the `to`-member→current Mapping is appended, the result is
/// simplified, and a score favoring both-direction Mappings
/// (forward+inverse=3, forward=2, inverse=1) selects the best route. The
/// returned 2-frame FrameSet maps `from`'s current Frame to `to`'s current
/// Frame.
///
/// The search order for each set is base first, then the remaining frames in
/// ascending index order, or descending for an inverted FrameSet, then the
/// current frame last (frameset.c:3308-3348).
///
/// The indices of the `from` and `to` Frames used are returned with the
/// result; [`FrameSearch::convert_to`](crate::frames::FrameSearch::convert_to) sets the Base of a FrameSet argument to them,
/// as `ConvertX` does (frameset.c:3502-3503).
pub(crate) fn frameset_convert_x(
    to: &dyn Frame,
    from: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<crate::frames::find_frame::ConvertRoute>> {
    let from_fs = from.as_any().downcast_ref::<FrameSet>();
    let to_fs = to.as_any().downcast_ref::<FrameSet>();

    // The 1-based search order: the base Frame, then the others in
    // ascending index order (descending while the FrameSet is inverted),
    // then the current Frame.
    fn search_order(fs: &FrameSet) -> Vec<usize> {
        let (nframe, base, current) = (fs.nframe(), fs.base_index(), fs.current_index());
        let others = (1..=nframe).filter(|&i| i != base && i != current);
        let mut order = Vec::with_capacity(nframe);
        order.push(base);
        if fs.is_inverted() {
            order.extend(others.rev());
        } else {
            order.extend(others);
        }
        if current != base {
            order.push(current);
        }
        order
    }

    let (from_current, from_order) = match from_fs {
        Some(fs) => (fs.current_index(), search_order(fs)),
        None => (1, vec![1]),
    };
    let (to_current, to_order) = match to_fs {
        Some(fs) => (fs.current_index(), search_order(fs)),
        None => (1, vec![1]),
    };

    // Comma-separated domain fields; an empty list yields one blank field
    // that matches any domain.
    let fields: Vec<String> = if domainlist.is_empty() {
        vec![String::new()]
    } else {
        domainlist.split(',').map(|s| s.to_string()).collect()
    };

    let mut result_map: Option<DynMapping> = None;
    let (mut iframe_from, mut iframe_to) = (0, 0);

    // Each field is scored afresh, and the first field that gives any
    // conversion ends the search (frameset.c:3366-3378).
    'search: for field in &fields {
        let domain = field.trim();
        let mut best_score = -1i32;
        for &to_idx in &to_order {
            // The selected `to` member frame (or `to` itself when not a set).
            // The member itself, not a copy, takes part, as in C: a
            // SkyFrame member keeps the LAST it computes for the conversion.
            let to_member: &dyn Frame = match to_fs {
                Some(fs) => fs.frame(FrameRef::Index(to_idx))?,
                None => to,
            };

            for &from_idx in &from_order {
                let from_member: &dyn Frame = match from_fs {
                    Some(fs) => fs.frame(FrameRef::Index(from_idx))?,
                    None => from,
                };

                // Per-pair alignment between the two selected member frames,
                // through C's virtual `astConvert`, so a Region member takes
                // part as its Frame (frameset.c:3410).
                let Some(cvt) = crate::frames::find_frame::convert_frames_or_regions(
                    from_member,
                    to_member,
                    domain,
                )?
                else {
                    continue;
                };
                let mut map: DynMapping = cvt.mapping(FrameRef::Base, FrameRef::Current)?;

                // Prefix from-current → from-member.
                if let Some(fs) = from_fs
                    && from_idx != from_current
                {
                    let pre = fs.mapping(FrameRef::Current, FrameRef::Index(from_idx))?;
                    map = Box::new(CmpMap::new(pre, map, Composition::Series)?);
                }

                // Append to-member → to-current.
                if let Some(fs) = to_fs
                    && to_idx != to_current
                {
                    let post = fs.mapping(FrameRef::Index(to_idx), FrameRef::Current)?;
                    map = Box::new(CmpMap::new(map, post, Composition::Series)?);
                }

                let map = <dyn Mapping>::simplify(map.as_ref())?;

                let score: i32 =
                    if map.has_forward() { 2 } else { 0 } + if map.has_inverse() { 1 } else { 0 };

                if score > best_score {
                    best_score = score;
                    result_map = Some(map);
                    iframe_from = from_idx;
                    iframe_to = to_idx;
                    if best_score >= 3 {
                        break 'search;
                    }
                }
            }
        }
        if result_map.is_some() {
            break;
        }
    }

    let Some(result_map) = result_map else {
        return Ok(None);
    };

    // Result: from-current Frame → to-current Frame, linked by the best route.
    // C's result shares these Frames with the arguments; an owned result
    // holds copies, taken after the search so that they carry the LAST the
    // search cached in the arguments' Frames.
    let to_cur_frame: DynFrame = match to_fs {
        Some(fs) => fs.current().snapshot(),
        None => to.snapshot(),
    };
    let from_cur_frame: DynFrame = match from_fs {
        Some(fs) => fs.current().snapshot(),
        None => from.snapshot(),
    };
    let mut result_fs = FrameSet::new(from_cur_frame);
    result_fs.add_frame(FrameRef::Base, result_map, to_cur_frame)?;
    Ok(Some(crate::frames::find_frame::ConvertRoute {
        fs: result_fs,
        from_frame: iframe_from,
        to_frame: iframe_to,
    }))
}

// ====================================================================== //
// Mapping trait implementation                                            //
// ====================================================================== //

impl Mapping for FrameSet {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }

    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }

    fn clear_id(&mut self) {
        self.id.clear();
    }

    fn test_id(&self) -> bool {
        self.id.test()
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// Whether the base-to-current Mapping is linear.
    // C: frameset.c `GetIsLinear`.
    fn is_linear(&self) -> bool {
        self.mapping(FrameRef::Base, FrameRef::Current)
            .is_ok_and(|map| map.is_linear())
    }

    /// Split the base-to-current Mapping.
    // C: frameset.c `MapSplit`.
    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        self.mapping(FrameRef::Base, FrameRef::Current)?
            .map_split_zero_based(in_axes)
    }

    /// Remove the Regions from every Mapping and every Frame.
    // C: frameset.c `RemoveRegions`.
    fn mapping_without_regions(&self) -> Option<DynMapping> {
        self.without_regions().map(|fs| Box::new(fs) as DynMapping)
    }

    /// The input values are formatted by the base Frame and the output values
    /// by the current Frame, the other way round for the inverse.
    // C: frameset.c `ReportPoints`.
    fn report_points(&self, forward: bool, inp: &PointSet, out: &PointSet) {
        let (from, to) = if forward {
            (self.base(), self.current())
        } else {
            (self.current(), self.base())
        };
        crate::mappings::report::write_report(&super::frame_report_lines(from, to, inp, out));
    }

    // C's `Frame` overrides `astDoNotSimplify` to zero (frame.c:3511-3554): a
    // set `Ident` is a user label on a coordinate system, not a request to keep
    // a Mapping intact, so a named Frame is still simplified.
    fn do_not_simplify(&self) -> bool {
        false
    }

    fn nin(&self) -> usize {
        self.base().naxes()
    }

    fn nout(&self) -> usize {
        self.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        std::mem::swap(&mut self.base_idx, &mut self.current_idx);
        std::mem::swap(&mut self.base_set, &mut self.current_set);
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let map = if forward {
            self.mapping(FrameRef::Base, FrameRef::Current)?
        } else {
            self.mapping(FrameRef::Current, FrameRef::Base)?
        };
        map.transform(pts, true)
    }

    fn transform_into(&self, pts: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let map = if forward {
            self.mapping(FrameRef::Base, FrameRef::Current)?
        } else {
            self.mapping(FrameRef::Current, FrameRef::Base)?
        };
        map.transform_into(pts, true, out)
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // C FrameSet::Simplify (frameset.c:10111) returns a *copy of
        // the FrameSet* with each internal edge mapping individually
        // simplified — not the composed base→current mapping.
        let mut copy = self.clone();
        // C's `simpler`, raised by any edge whose `astSimplify` handed back a
        // different Mapping (`tmp != map`, frameset.c:10208), and the whole
        // decision between returning the copy and returning the original
        // (`astClone( simpler ? new : this )`, frameset.c:10226).
        let mut simpler = false;
        for edge in &mut copy.edges {
            let (simplified, changed) = edge.mapping.simplify_reporting_change()?;
            edge.mapping = simplified;
            simpler |= changed;
        }
        // `astClone( simpler ? new : this )` (frameset.c:10226): when no edge
        // reduced, the copy — stamps and all — is discarded and the original
        // returned.
        Ok(if simpler {
            (Box::new(copy) as DynMapping, true)
        } else {
            (self.clone_box(), false)
        })
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
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
        // C compares the stored Base and Current, so a set index differs
        // from an unset one even when they select the same Frame.
        if (
            self.base_idx,
            self.base_set,
            self.current_idx,
            self.current_set,
        ) != (
            other.base_idx,
            other.base_set,
            other.current_idx,
            other.current_set,
        ) {
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
// Serialization                                                          //
// ====================================================================== //

impl Serializable for FrameSet {
    fn class_name(&self) -> &'static str {
        "FrameSet"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Set of inter-related coordinate systems".to_string());

        // Object-level attributes come first, before the commented Frame proxy
        // header: C dumps the class chain from the top down (`object.c:1697`),
        // and writes the `IsA Object` marker only when something preceded it.
        // The value is the FrameSet's own, not the current Frame's
        // (`frameset.c:4841`).
        super::frame_attrs::write_object_part(
            &mut node,
            &self.id,
            self.ident.as_deref(),
            self.use_defs,
            crate::frames::Frame::use_defs(self),
        );
        // The Mapping and Frame parts, read through the FrameSet's getters,
        // which give the current Frame's values (frame.c Dump on the
        // FrameSet). The FrameSet's own Frame structure holds no attribute
        // values and no axes, so everything is unset, and there are no Prm
        // items or Axis objects.
        let current = self.current();
        let current_node = FrameToNode::to_node(current)?;
        let frame_part = current_node.section("Frame");
        let system_of = |key: &str| match frame_part.get_any(key) {
            Some(NodeValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let is_frame = current
            .as_any()
            .downcast_ref::<super::BasicFrame>()
            .is_some();
        super::frame_attrs::write_frame_cards(
            &mut node,
            &super::frame_attrs::TraitDump {
                frame: self,
                attrs: &super::frame_attrs::FrameAttrs::default(),
                system: (false, system_of("System")),
                align_system: (false, system_of("AlSys")),
                perm: self.axis_perm(),
                is_frame,
                own_naxes: 0,
                mapping: Some(crate::serial::MappingPart::of_frame(
                    &self.mapping_flags,
                    Mapping::nin(self),
                    Mapping::nout(self),
                    self.inverted,
                    self.has_forward(),
                    self.has_inverse(),
                )),
            },
        )?;

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // Nframe — number of frames (always).
        let nframe = self.frames.len();
        node.push_int(
            "Nframe",
            nframe as i64,
            Some("Number of Frames in FrameSet".to_string()),
        );

        // C writes its stored `base` and `current`, which an inverted
        // FrameSet's effective ones are exchanged from.
        let ((base_idx, base_set), (current_idx, current_set)) = if self.inverted {
            (
                (self.current_idx, self.current_set),
                (self.base_idx, self.base_set),
            )
        } else {
            (
                (self.base_idx, self.base_set),
                (self.current_idx, self.current_set),
            )
        };

        // Base — 1-based base frame index. C writes it as an active value
        // only when the Base attribute is explicitly set (frameset.c:12554,
        // `set = (this->base != -INT_MAX)`); otherwise it emits the default
        // (frame 1) as a commented-out line.
        if base_set {
            node.push_int(
                "Base",
                (base_idx + 1) as i64,
                Some("Index of base Frame".to_string()),
            );
        } else {
            node.push_unset(
                "Base",
                NodeValue::Int(base_idx as i64 + 1),
                Some("Index of base Frame".to_string()),
            );
        }

        // Currnt — 1-based current frame index, active only when the
        // Current attribute is set (frameset.c:12561), otherwise its default
        // commented out.
        if current_set {
            node.push_int(
                "Currnt",
                (current_idx + 1) as i64,
                Some("Index of current Frame".to_string()),
            );
        } else {
            node.push_unset(
                "Currnt",
                NodeValue::Int(current_idx as i64 + 1),
                Some("Index of current Frame".to_string()),
            );
        }

        // Nnode, set when the tree has nodes no Frame is at.
        node.write_int(
            "Nnode",
            self.nnode != nframe,
            false,
            self.nnode as i64,
            "Number of nodes in FrameSet",
        );

        // Nod{i}, the node of each Frame, set when not the identity.
        for (i, &nod) in self.node_map.iter().enumerate() {
            let val = nod + 1;
            node.write_int(
                &format!("Nod{}", i + 1),
                val != i + 1,
                false,
                val as i64,
                &format!("Frame {} is associated with node {}", i + 1, val),
            );
        }

        // VFr{i}, the 1-based Frame whose variants each Frame uses, set when
        // it is another Frame's (0 when the Frame holds its own).
        for (i, &vf) in self.varfrm.iter().enumerate() {
            node.write_int(
                &format!("VFr{}", i + 1),
                vf > 0,
                false,
                vf as i64,
                &format!("Frame {} inherits variants from Frame {vf}", i + 1),
            );
        }

        // Lnk{node} = parent for each edge (1-based node indices), and the
        // link's Inv{node}, set when the inverse Mapping is used.
        for edge in &self.edges {
            let key = format!("Lnk{}", edge.to + 1);
            let comment = format!(
                "Node {} is derived from node {}",
                edge.to + 1,
                edge.from + 1,
            );
            node.push_int(&key, (edge.from + 1) as i64, Some(comment));
            let inv = edge.mapping.is_inverted();
            node.write_int(
                &format!("Inv{}", edge.to + 1),
                inv,
                false,
                i64::from(inv),
                if inv {
                    "The inverse mapping is used"
                } else {
                    "The forward mapping is used"
                },
            );
        }

        // Frm{i+1} — each frame as a nested object. C writes a frame's Variants
        // FrameSet as object `Vrnts`, last in the Frame class's part of its
        // dump, before the `IsA Frame` marker of a Frame subclass
        // (frame.c:14679); since Rust holds variants FrameSet-side, inject it
        // there.
        for (i, frame) in self.frames.iter().enumerate() {
            let key = format!("Frm{}", i + 1);
            let mut frame_node = FrameToNode::to_node(frame.as_ref())?;
            if let Some(vfs) = &self.frame_variants[i] {
                let vnode = Serializable::to_node(vfs)?;
                let end_of_frame_part = frame_node
                    .items
                    .iter()
                    .position(|item| {
                        item.key == "IsA"
                            && matches!(&item.value, NodeValue::String(c) if c == "Frame")
                    })
                    .unwrap_or(frame_node.items.len());
                frame_node.insert_object(
                    end_of_frame_part,
                    "Vrnts",
                    vnode,
                    Some("Variant Frames".to_string()),
                );
            }
            let comment = format!("Frame number {}", i + 1);
            node.push_object(&key, frame_node, Some(comment));
        }

        // Map{node+1} — each edge mapping as a nested object.
        for edge in &self.edges {
            let key = format!("Map{}", edge.to + 1);
            // The object is written with its own Invert attribute; the link's
            // flag is the `Inv<n>` above.
            let map_node = if edge.mapping.is_inverted() == edge.object_invert {
                MappingToNode::to_node(edge.mapping.as_ref())?
            } else {
                let mut object = edge.mapping.clone();
                object.invert();
                MappingToNode::to_node(object.as_ref())?
            };
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
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let object = &whole.section("Object");
        // astLoadFrame reads the FrameSet's own Frame part into Frame fields
        // the FrameSet never uses, its Frame attributes being its current
        // Frame's.
        let _ = super::frame_attrs::read_frame_attrs(whole);
        let frame_part = whole.section("Frame");
        super::frame_attrs::mark_frame_axes_read(&frame_part);
        for key in ["System", "AlSys", "Vrnts"] {
            frame_part.mark_read(key);
        }
        let node = &whole.section("FrameSet");
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
        // Currnt likewise; an unset Current defaults to the last Frame
        // (frameset.c `GetCurrent`).
        let current_active = node.get_int("Currnt", 0);
        let current_set = current_active >= 1;
        let current = if current_set {
            current_active as usize
        } else {
            nframe
        };
        let nnode = node.get_int("Nnode", nframe as i64) as usize;

        // The FrameSet's own Object-level `Ident` (`frameset.c:4841`), absent
        // when unset.
        let ident = object.get_string_if_set("Ident");

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
            let vrnts = match frame_node.section("Frame").get_object("Vrnts") {
                Some(vn) => Some(FrameSet::from_node(vn)?),
                None => None,
            };
            frame_variants.push(vrnts);
        }

        // Read the per-frame variant-mirror indices (VFr{i}, default 0).
        let mut varfrm: Vec<usize> = Vec::with_capacity(nframe);
        for i in 0..nframe {
            let key = format!("VFr{}", i + 1);
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
            let mut mapping = mapping_from_node(map_node)?;
            // The link uses the object with its Invert attribute set to the
            // link's flag.
            let object_invert = mapping.is_inverted();
            let invert = node.get_int(&format!("Inv{}", inode + 1), 0) != 0;
            if invert != object_invert {
                mapping.invert();
            }

            edges.push(Edge {
                from: parent - 1, // 0-based node index
                to: inode,        // 0-based node index
                mapping,
                object_invert,
            });
        }

        // An inverted FrameSet's stored Base and Current are exchanged to
        // give its effective ones, as C's `GetBase`/`GetCurrent` do.
        let inverted = crate::serial::read_mapping_base(whole).inverted;
        let ((base, base_set), (current, current_set)) = if inverted {
            ((current, current_set), (base, base_set))
        } else {
            ((base, base_set), (current, current_set))
        };
        Ok(FrameSet {
            inverted,
            frames,
            edges,
            base_idx: base - 1,
            current_idx: current - 1,
            base_set,
            current_set,
            frame_variants,
            varfrm,
            nnode,
            node_map,
            ident,
            id: crate::object_id::ObjectId::from_object_part(object),
            mapping_flags: crate::serial::read_mapping_flags(whole),
            use_defs: object
                .get("UseDfs")
                .map(|_| object.get_int("UseDfs", 1) != 0),
        })
    }
}
