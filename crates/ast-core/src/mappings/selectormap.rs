//! SelectorMap: a Mapping that locates positions within one of a set of
//! alternate Regions.
//!
//! See [`SelectorMap`].

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::{ast_equal, is_bad};
use super::{DynMapping, Mapping, UnitMap};
use crate::mappings::SimplifiedRecord;
use crate::regions::{
    BoxRegion, Circle, CmpRegion, DynRegion, Ellipse, Interval, NullRegion, PointList, Polygon,
    Prism,
};

/// A Mapping that locates positions within one of a set of alternate
/// Regions.
///
/// A SelectorMap is a Mapping that identifies which Region contains a given
/// input position.
///
/// A SelectorMap encapsulates a number of Regions that all have the same
/// number of axes and represent the same coordinate Frame.
/// The number of inputs of the SelectorMap equals the number of axes spanned
/// by one of the encapsulated Regions.
/// All SelectorMaps have a single output, and SelectorMaps do not define an
/// inverse transformation.
///
/// For each input position, the forward transformation searches through the
/// encapsulated Regions, in the order supplied when the SelectorMap was
/// created, until a Region is found which contains the input position.
/// The output is the index of that Region in the list, starting at 1 for the
/// first Region.
/// If an input position is not contained within any Region, the output is
/// zero.
/// An input position with a bad value on any axis gives
/// [`badval`](SelectorMap::badval).
///
/// If a compound Mapping contains a SelectorMap in series with its own
/// inverse, the two adjacent SelectorMaps are replaced by a [`UnitMap`] when
/// the compound Mapping is simplified.
///
/// SelectorMaps are often used with [`SwitchMap`](super::SwitchMap)s.
///
/// ```
/// use ast_core::frames::BasicFrameBuilder;
/// use ast_core::mappings::{Mapping, SelectorMapBuilder};
/// use ast_core::regions::{CircleBuilder, DynRegion};
///
/// let circle = |x: f64| -> DynRegion {
///     Box::new(
///         CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![x, 0.0])
///             .unwrap()
///             .radius(1.0)
///             .build()
///             .unwrap(),
///     )
/// };
/// let map = SelectorMapBuilder::new(vec![circle(0.0), circle(5.0)])
///     .unwrap()
///     .badval(-1.0)
///     .build();
/// assert_eq!((map.nin(), map.nout()), (2, 1));
/// let pts = ast_core::coords::PointSet::from_rows([
///     [0.5, 0.0],
///     [5.5, 0.0],
///     [9.0, 0.0],
///     [ast_core::constants::BAD_VALUE, 0.0],
/// ]);
/// let out = map.transform(&pts, true).unwrap();
/// assert_eq!(out.axis(0), [1.0, 2.0, 0.0, -1.0]);
/// ```
pub struct SelectorMap {
    regions: Vec<DynRegion>,
    badval: f64,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

/// Builder for [`SelectorMap`].
///
/// The Regions are given to [`new`](SelectorMapBuilder::new), and the value
/// returned for bad input positions to [`badval`](SelectorMapBuilder::badval).
pub struct SelectorMapBuilder {
    regions: Vec<DynRegion>,
    badval: f64,
}

impl SelectorMapBuilder {
    /// Start a SelectorMap that searches `regions`, in order.
    ///
    /// All the Regions must relate to the same coordinate Frame, whose number
    /// of axes is the number of inputs of the SelectorMap.
    /// The SelectorMap takes ownership of the Regions.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Bdpar` if `regions` is empty.
    /// - `ErrorCode::Badni` if a Region does not refer to the same coordinate
    ///   Frame as the first.
    #[doc(alias = "astSelectorMap")]
    pub fn new(regions: Vec<DynRegion>) -> AstResult<Self> {
        // The checks follow selectormap.c astInitSelectorMap.
        if regions.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SelectorMap: at least one Region is required",
            ));
        }
        let first = regions[0].get_region_frame();
        for (i, reg) in regions.iter().enumerate().skip(1) {
            if !reg.get_region_frame().equal(first.as_ref()) {
                return Err(AstError::new(
                    ErrorCode::Badni,
                    format!(
                        "SelectorMap: Region number {} does not refer to the same \
                         coordinate Frame as the first Region",
                        i + 1
                    ),
                ));
            }
        }
        Ok(Self {
            regions,
            badval: BAD_VALUE,
        })
    }

    /// Set the value the forward transformation returns for an input position
    /// that has a bad value on any axis.
    ///
    /// Default: [`BAD_VALUE`].
    pub fn badval(mut self, val: f64) -> Self {
        self.badval = val;
        self
    }

    /// Build the [`SelectorMap`].
    pub fn build(self) -> SelectorMap {
        SelectorMap {
            regions: self.regions,
            badval: self.badval,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }
}

impl SelectorMap {
    /// The encapsulated Regions, in the order they are searched.
    pub fn regions(&self) -> &[DynRegion] {
        &self.regions
    }

    /// The value the forward transformation returns for an input position that
    /// has a bad value on any axis.
    pub fn badval(&self) -> f64 {
        self.badval
    }

    /// The intrinsic number of axes (the regions' shared `naxes()`).
    fn intrinsic_nin(&self) -> usize {
        self.regions[0].naxes()
    }

    /// C's "attempt to simplify the SelectorMap on its own"
    /// (`selectormap.c:693-714`): simplify each encapsulated Region and, if any
    /// of them reduced, build a new SelectorMap from the results. `None` when
    /// none of them did, which is C's `simp == 0`.
    ///
    /// A simplified Mapping that is still a known concrete Region type is
    /// retained; otherwise the original Region is kept for that slot, since a
    /// SelectorMap holds Regions and not Mappings.
    fn reduced_children(&self) -> AstResult<Option<DynMapping>> {
        let mut changed = false;
        let mut simplified_regions: Vec<DynRegion> = Vec::with_capacity(self.regions.len());
        for reg in &self.regions {
            let simp: DynMapping = (reg.as_ref() as &dyn Mapping).simplify()?;
            let was_changed = !simp.equal(reg.as_ref() as &dyn Mapping)
                || simp.is_inverted() != reg.is_inverted();
            let new_reg: DynRegion = if let Some(r) = simp.as_any().downcast_ref::<Circle>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<BoxRegion>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Ellipse>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Polygon>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Interval>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<NullRegion>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<CmpRegion>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<PointList>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Prism>() {
                Box::new(r.clone())
            } else {
                // Simplification produced a non-Region — keep original.
                simplified_regions.push(reg.clone());
                continue;
            };
            if was_changed {
                changed = true;
            }
            simplified_regions.push(new_reg);
        }

        if !changed {
            return Ok(None);
        }

        Ok(Some(Box::new(SelectorMap {
            regions: simplified_regions,
            badval: self.badval,
            inverted: self.inverted,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })))
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
            simplified: self.simplified,
            ident: self.ident.clone(),
            id: Default::default(),
            mapping_flags: self.mapping_flags.clone(),
        }
    }
}

impl Mapping for SelectorMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

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

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn has_forward(&self) -> bool {
        // Intrinsic forward is defined; intrinsic inverse is not.
        // When inverted, the effective forward is the intrinsic inverse.
        !self.inverted
    }

    fn has_inverse(&self) -> bool {
        // Intrinsic inverse is undefined; only the inverted form has an
        // effective inverse direction (the intrinsic forward).
        self.inverted
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        // C passes (1, 0) to astInitMapping (selectormap.c).
        (true, false)
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

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let effective_fwd = forward != self.inverted;
        let npoint = pts.npoint();
        let ncoord = self.intrinsic_nin();

        if !effective_fwd {
            // Inverse not defined — return all BAD.
            let mut out = PointSet::zeros(npoint, ncoord);
            for j in 0..ncoord {
                out.axis_mut(j).fill(BAD_VALUE);
            }
            return Ok(out);
        }

        let mut out = PointSet::zeros(npoint, 1);

        // Initial classification: -1 sentinel = bad input, 0 = unassigned.
        let class = out.axis_mut(0);
        for j in 0..ncoord {
            for (c, &v) in class.iter_mut().zip(pts.axis(j)) {
                if is_bad(v) {
                    *c = -1.0;
                }
            }
        }

        // Test points against each region in turn. Negate + open the region
        // so its forward transform marks interior points as BAD; that's the
        // standard AST trick for region membership in a Mapping context.
        let mut working = pts.clone();

        for ireg in 0..self.regions.len() {
            let reg = &self.regions[ireg];
            let mut neg_reg = reg.negated_copy();
            let was_closed = neg_reg.is_closed();
            neg_reg.set_closed(!was_closed);

            let transformed = (neg_reg.as_ref() as &dyn Mapping).transform(&working, true)?;

            for i in 0..npoint {
                if out.axis(0)[i] == 0.0 && is_bad(transformed.axis(0)[i]) {
                    out.axis_mut(0)[i] = (ireg + 1) as f64;
                }
            }

            // Subsequent iterations test against the already-pruned point set
            // (interior points are now BAD and won't match further regions).
            working = transformed;
        }

        // Replace -1 sentinel with badval.
        for i in 0..npoint {
            if out.axis(0)[i] == -1.0 {
                out.axis_mut(0)[i] = self.badval;
            }
        }

        Ok(out)
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let result = self.transform(&PointSet::from_points(&pts), forward)?;
        out.copy_from(&result.as_points());
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C opens its MapMerge by simplifying the encapsulated Regions and
        // rebuilding the SelectorMap around them (selectormap.c:693-714), and
        // only considers a neighbor when that made no change (:725-727).
        super::simplify::self_reduce_map_merge(self, self.reduced_children(), ctx, where_, series)
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

        if self_inv == other_inv {
            return None;
        }
        if self.regions.len() != other_sel.regions.len() {
            return None;
        }
        if !ast_equal(self.badval, other_sel.badval) {
            return None;
        }
        for (a, b) in self.regions.iter().zip(other_sel.regions.iter()) {
            if !(a.as_ref() as &dyn Mapping).equal(b.as_ref() as &dyn Mapping) {
                return None;
            }
        }

        Some(Box::new(UnitMap::new(self.intrinsic_nin())))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_sel) = other.as_any().downcast_ref::<SelectorMap>() else {
            return false;
        };
        // selectormap.c Equal compares the number of inputs, the Regions and
        // the bad value, not the Invert flag. Inverting swaps the numbers of
        // inputs and outputs, so only a one-dimensional SelectorMap can equal
        // its inverted copy.
        if self.nin() != other_sel.nin() {
            return false;
        }
        if self.regions.len() != other_sel.regions.len() {
            return false;
        }
        if !ast_equal(self.badval, other_sel.badval) {
            return false;
        }
        for (a, b) in self.regions.iter().zip(other_sel.regions.iter()) {
            if !(a.as_ref() as &dyn Mapping).equal(b.as_ref() as &dyn Mapping) {
                return false;
            }
        }
        true
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl crate::serial::Serializable for SelectorMap {
    fn class_name(&self) -> &'static str {
        "SelectorMap"
    }

    fn to_node(&self) -> AstResult<crate::serial::Node> {
        use crate::serial::{Node, RegionToNode, write_mapping_base};

        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Region identification Mapping".to_string());
        write_mapping_base(&mut node, self);

        // The first Region's Frame is the Frame of them all, so the second
        // and later Regions are written without theirs (selectormap.c Dump
        // clears RegionFS); the loader gives them the first one's.
        for (i, reg) in self.regions.iter().enumerate() {
            let key = format!("Reg{}", i + 1);
            let reg_node = if i == 0 {
                RegionToNode::to_node(reg.as_ref())?
            } else {
                let mut frameless = reg.clone();
                frameless.set_region_fs_flag(false);
                RegionToNode::to_node(frameless.as_ref())?
            };
            node.push_object(&key, reg_node, Some("Region of input space".to_string()));
        }

        if !is_bad(self.badval) {
            node.push_double(
                "BadVal",
                self.badval,
                Some("Output value for bad input positions".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &crate::serial::Node) -> AstResult<Self> {
        use crate::serial::{read_mapping_base, region_from_node};

        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("SelectorMap");
        let base = read_mapping_base(whole);

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

        // All but the first Region may carry a dummy FrameSet (dumped
        // frame-less to save space); re-associate them with the first
        // Region's frame. Mirrors C `selectormap.c` Load (1788-1795).
        let first_frame = regions[0].get_region_frame();
        for reg in regions.iter_mut().skip(1) {
            if reg.has_dummy_fs() {
                reg.set_region_fs(first_frame.as_ref());
            }
        }

        let badval = node.get_double("BadVal", BAD_VALUE);

        let mut sm = SelectorMapBuilder::new(regions)
            .map_err(|e| AstError::new(ErrorCode::Lderr, format!("SelectorMap: {e}")))?
            .badval(badval)
            .build();

        if base.inverted {
            sm.invert();
        }

        sm.set_has_been_simplified(base.has_been_simplified);
        Ok(sm)
    }
}
