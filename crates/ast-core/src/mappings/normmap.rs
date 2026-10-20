//! Mapping that normalizes coordinates using a supplied Frame.
//!
//! See [`NormMap`].

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{BasicFrame, DynFrame, Frame};
use crate::serial::{
    FrameToNode, Node, Serializable, frame_from_node, read_mapping_base, write_mapping_base,
};

use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Normalize coordinates using a supplied Frame.
///
/// A NormMap is a Mapping which normalizes coordinate values using the
/// [`norm`](crate::frames::Frame::norm) method of a supplied Frame.
/// The number of inputs and outputs of a NormMap are both equal to the number
/// of axes in the Frame.
///
/// The forward and inverse transformations of a NormMap are both defined but
/// are identical: they do not form a real inverse pair, in that the inverse
/// transformation does not undo the normalization but reapplies it.
/// However, simplification replaces neighboring pairs of forward and inverse
/// NormMaps by a single [`UnitMap`], so long as the Frames encapsulated by
/// the two NormMaps are equal (the same class and the same attribute values).
/// This means, for instance, that a CmpMap containing a NormMap still cancels
/// with its own inverse.
/// Simplification also merges consecutive identical NormMaps, rebuilds a
/// NormMap whose Frame simplifies around the simplified Frame, and replaces
/// one encapsulating a basic Frame, whose normalization does nothing, by a
/// [`UnitMap`].
///
/// ```
/// use ast_core::frames::{SkyFrame, SkySystem};
/// use ast_core::mappings::{Mapping, NormMap};
///
/// let map = NormMap::new(Box::new(SkyFrame::new(SkySystem::Fk5)));
/// let (lon, lat) = map.tran2(&[-0.5], &[0.25], true).unwrap();
/// assert!((lon[0] - (2.0 * std::f64::consts::PI - 0.5)).abs() < 1e-15);
/// assert_eq!(lat[0], 0.25);
/// ```
pub struct NormMap {
    /// The frame whose `norm()` method is called.
    frame: DynFrame,
    /// Whether this NormMap is inverted.
    inverted: bool,
    /// Whether simplification is inhibited.
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl NormMap {
    /// Create a NormMap which normalizes coordinates with `frame`.
    ///
    /// The number of inputs and outputs equals the number of axes in the
    /// Frame.
    /// The NormMap takes ownership of the Frame.
    #[doc(alias = "astNormMap")]
    pub fn new(frame: DynFrame) -> Self {
        Self {
            frame,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// The Frame used to normalize coordinates.
    pub fn frame(&self) -> &dyn Frame {
        self.frame.as_ref()
    }

    /// The number of axes (same for nin and nout in the intrinsic direction).
    fn naxes(&self) -> usize {
        self.frame.naxes()
    }
}

// Manual Debug impl because DynFrame (Box<dyn Frame>) uses the trait's Debug.
impl fmt::Debug for NormMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NormMap")
            .field("naxes", &self.naxes())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for NormMap {
    fn clone(&self) -> Self {
        NormMap {
            frame: self.frame.snapshot(),
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            id: Default::default(),
            mapping_flags: self.mapping_flags.clone(),
        }
    }
}

impl Mapping for NormMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// normmap.c `RemoveRegions`: remove the Regions from the Frame whose
    /// normalisation is applied.
    fn mapping_without_regions(&self) -> Option<DynMapping> {
        let frame = self.frame.frame_without_regions()?;
        let mut new = self.clone();
        new.frame = frame;
        Some(Box::new(new))
    }

    fn nin(&self) -> usize {
        // NormMap has same nin and nout regardless of inversion,
        // since both directions do the same thing.
        self.naxes()
    }

    fn nout(&self) -> usize {
        self.naxes()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let _ = forward;
        let naxes = self.naxes();

        let pin = pts.axis_slices();
        let pout = out.axis_list_mut();
        let mut point = vec![0.0; naxes];
        for ipoint in 0..pts.npoint() {
            // C normalises every point, bad values included, and leaves each
            // Frame's Norm to decide what a bad value means (normmap.c
            // Transform): a SkyFrame still wraps the longitude of a point
            // whose latitude is bad. Both directions normalise.
            for (v, axis) in point.iter_mut().zip(pin) {
                *v = axis[ipoint];
            }
            self.frame.norm(&mut point)?;
            for (axis, &v) in pout.iter_mut().zip(&point) {
                axis[ipoint] = if v.is_nan() { BAD_VALUE } else { v };
            }
        }

        Ok(())
    }

    /// NaN-as-bad fast path, the same as `transform_into`: every
    /// `Frame::norm` treats a NaN coordinate as it treats a bad one.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let _ = forward;
        let naxes = self.naxes();
        let pin = pts.axis_slices();
        let pout = out.axis_list_mut();
        let mut point = vec![0.0; naxes];
        for ipoint in 0..pts.npoint() {
            for (v, axis) in point.iter_mut().zip(pin) {
                *v = axis[ipoint];
            }
            self.frame.norm(&mut point)?;
            for (axis, &v) in pout.iter_mut().zip(&point) {
                axis[ipoint] = v;
            }
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C's MapMerge tries two replacements before it looks at neighbours,
        // in this order. Anything that neither replaces falls through to the
        // neighbour steps, which `NormMap::try_merge` performs.
        //
        // The order is load-bearing: C reaches the second test with
        // `class = astGetClass( smap )`, the class of the *simplified* Frame,
        // so a Frame that reduces to a basic Frame is rebuilt on this
        // iteration and becomes a UnitMap on the next one. Collapsing the two
        // into a single pass yields a UnitMap where C yields a NormMap.
        //
        // An error from the Frame simplify abandons the merge, as C's status
        // check does.
        let simplified_frame = self.frame.simplify_frame().ok()?;
        let reduced: Option<DynMapping> = if let Some(frame) = simplified_frame {
            // The encapsulated Frame reduced: rebuild the NormMap around it
            // (normmap.c:619-631).
            Some(Box::new(NormMap::new(frame)))
        } else if self.frame.as_any().is::<BasicFrame>() {
            // A basic Frame's astNorm does nothing, so the NormMap is a UnitMap
            // (normmap.c:632-640).
            Some(Box::new(UnitMap::new(self.naxes())))
        } else {
            None
        };

        super::simplify::self_reduce_map_merge(self, reduced, ctx, where_, series)
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        // NormMap merging only works in series.
        if !series {
            return None;
        }

        // The other mapping must also be a NormMap.
        let other_nm = other.as_any().downcast_ref::<NormMap>()?;

        // Case 1: Inverse cancellation.
        // A NormMap with its inverse (opposite invert flags) cancels to UnitMap
        // if the frames are equal.
        if self_inv != other_inv {
            if self.frame.equal(other_nm.frame.as_ref()) {
                return Some(Box::new(UnitMap::new(self.naxes())));
            }
            return None;
        }

        // Case 2: Duplicate elimination.
        // Adjacent NormMaps with same invert flags and equal frames →
        // the pair reduces to a single NormMap (since norm is idempotent).
        if self.frame.equal(other_nm.frame.as_ref()) {
            let mut result = self.clone();
            result.inverted = self_inv;
            return Some(Box::new(result));
        }

        None
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, _at, ax1, ax2)?;
        // The C implementation returns 1.0 for matching axes, 0.0 otherwise.
        // This is because norm() is treated as approximately identity for
        // rate-of-change purposes (it only affects values at wrap boundaries).
        if ax1 == ax2 { Ok(1.0) } else { Ok(0.0) }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_nm) = other.as_any().downcast_ref::<NormMap>() else {
            return false;
        };

        // Check the Invert flags are equal.
        if self.inverted != other_nm.inverted {
            return false;
        }

        // Check the encapsulated Frames are equal.
        self.frame.equal(other_nm.frame.as_ref())
    }

    fn has_forward(&self) -> bool {
        true
    }

    fn has_inverse(&self) -> bool {
        true
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

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }

    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C allocates the returned output indices with astMalloc, which
        // gives NULL for none, so an empty selection gives no split.
        if in_axes.is_empty() {
            return Ok(None);
        }
        // normmap.c MapSplit: the parent's MapSplit first (an input out of
        // range is AST__AXIIN, and a full selection splits whole), then a
        // NormMap of the selected axes of the encapsulated Frame, picked as
        // `astPickAxes` picks them, so a repeated axis is AST__SELIN. The new
        // NormMap is not inverted: a NormMap's transformation is the same in
        // both directions. Output indices equal the input indices.
        if let Some(r) = crate::mappings::helpers::base_map_split(self, in_axes)? {
            return Ok(Some(r));
        }
        // pick_axes is 1-based at the public API.
        let axes_1based: Vec<usize> = in_axes.iter().map(|&i| i + 1).collect();
        let (sub_frame, _pick_map) = crate::frames::pick_axes(self.frame.as_ref(), &axes_1based)?;
        Ok(Some((in_axes.to_vec(), Box::new(NormMap::new(sub_frame)))))
    }
}

impl Serializable for NormMap {
    fn class_name(&self) -> &'static str {
        "NormMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Normalise axis values".to_string());
        write_mapping_base(&mut node, self);

        // Frame — nested object.
        let frame_node = FrameToNode::to_node(self.frame.as_ref())?;
        node.push_object(
            "Frame",
            frame_node,
            Some("Frame defining the normalisation".to_string()),
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("NormMap");
        let base = read_mapping_base(whole);

        // Read the encapsulated Frame.
        let frame_node = node
            .get_object("Frame")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "NormMap: missing Frame"))?;
        let frame = frame_from_node(frame_node)?;

        let mut nm = Self {
            frame,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        };

        if base.inverted {
            nm.invert();
        }
        nm.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(nm)
    }
}
