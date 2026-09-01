//! Mapping that normalises coordinates using a supplied Frame.
//!
//! A `NormMap` is a Mapping which normalises coordinate values using the
//! [`norm()`](crate::frames::Frame::norm) method of a supplied Frame.
//! The number of inputs and outputs of a NormMap are both equal to the
//! number of axes in the supplied Frame.
//!
//! The forward and inverse transformations of a NormMap are both defined
//! but are identical (that is, they do not form a real inverse pair in
//! that the inverse transformation does not undo the normalisation,
//! instead it reapplies it). However, the simplification engine will
//! replace neighbouring pairs of forward and inverse NormMaps by a single
//! UnitMap (so long as the Frames encapsulated by the two NormMaps are
//! equal).

use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{BasicFrame, DynFrame, Frame};
use crate::serial::{
    FrameToNode, Node, Serializable, frame_from_node, read_mapping_base, write_mapping_base,
};

use super::helpers::is_bad;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// A mapping that normalises coordinates using the `norm()` method of
/// a stored Frame.
///
/// Both the forward and inverse transformations perform normalisation
/// (they are identical). The simplification engine cancels adjacent
/// forward/inverse pairs to a UnitMap and eliminates duplicate
/// consecutive NormMaps.
///
/// A NormMap encapsulating a basic Frame (whose `norm()` is identity)
/// simplifies to a UnitMap.
pub struct NormMap {
    /// The frame whose `norm()` method is called.
    frame: DynFrame,
    /// Whether this NormMap is inverted.
    inverted: bool,
    /// Whether simplification is inhibited.
    simplified: SimplifiedRecord,
    ident: Option<String>,
}

impl NormMap {
    /// Create a new `NormMap` with the given frame.
    ///
    /// The number of inputs and outputs equals the number of axes in
    /// the frame.
    pub fn new(frame: DynFrame) -> Self {
        Self {
            frame,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
        }
    }

    /// The encapsulated frame.
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
        }
    }
}

impl Mapping for NormMap {
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

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        // Both forward and inverse perform normalisation — they are identical.
        // The `forward` and `self.inverted` flags are irrelevant to the actual
        // computation (both directions call norm), matching the C implementation.
        let _ = forward; // suppress unused warning; both dirs do the same thing

        let naxes = self.naxes();
        let npoint = pts.npoint();
        let mut out = PointSet::zeros(npoint, naxes);

        for ipoint in 0..npoint {
            let inp = pts.point(ipoint);
            let outp = out.point_mut(ipoint);

            // Copy input to output work buffer, checking for BAD values.
            // The C implementation calls astNorm unconditionally (it does not
            // skip BAD points). However, Frame::norm implementations typically
            // handle or pass through BAD values. We follow the C approach:
            // copy input then call norm.
            outp[..naxes].copy_from_slice(&inp[..naxes]);

            // Check if any coordinate is BAD. If so, skip normalisation
            // (the C code copies all coords then calls norm which may or may
            // not handle BAD; to be safe and match test expectations, we
            // skip points with any BAD coordinate).
            let has_bad = outp[..naxes].iter().any(|&v| is_bad(v));
            if !has_bad {
                self.frame.norm(&mut outp[..naxes])?;
            }
        }

        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let _ = forward;
        let naxes = self.naxes();

        for ipoint in 0..pts.npoint() {
            let inp = pts.point(ipoint);
            let outp = out.point_mut(ipoint);

            outp[..naxes].copy_from_slice(&inp[..naxes]);

            let has_bad = outp[..naxes].iter().any(|&v| is_bad(v));
            if !has_bad {
                self.frame.norm(&mut outp[..naxes])?;
            }
        }

        Ok(())
    }

    /// NaN-as-bad fast path. Drops the per-point `is_bad` guard. Every
    /// `Frame::norm` impl in this crate is value-by-value arithmetic (mod
    /// 2π wrapping, range checks, etc.) — NaN propagates through that
    /// cleanly to NaN output. The is_bad guard in `transform_into` is a
    /// "don't bother normalising garbage" optimisation that doesn't affect
    /// correctness for the NaN-protocol caller.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let _ = forward;
        let naxes = self.naxes();
        for ipoint in 0..pts.npoint() {
            let inp = pts.point(ipoint);
            let outp = out.point_mut(ipoint);
            outp[..naxes].copy_from_slice(&inp[..naxes]);
            self.frame.norm(&mut outp[..naxes])?;
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C opens its MapMerge with the replace-with-simpler step
        // (normmap.c:632-640): a basic Frame's astNorm does nothing, so a
        // NormMap over one is a UnitMap. Anything else falls through to the
        // neighbour steps, which `NormMap::try_merge` performs.
        //
        // C tries one thing first: simplify the encapsulated Frame and rebuild
        // the NormMap around the result if it reduced (normmap.c:619-631). That
        // is unported; see PLAN.md §10.17 for what would expose it.
        let reduced: Option<DynMapping> = self
            .frame
            .as_any()
            .downcast_ref::<BasicFrame>()
            .map(|_| Box::new(UnitMap::new(self.naxes())) as DynMapping);

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
        self.ident = if ident.is_empty() {
            None
        } else {
            Some(ident.to_string())
        };
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
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

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors normmap.c::MapSplit (~line 736). The C body first calls
        // the parent (default) impl for the full-input case, then falls
        // back to a per-class implementation for partial selections:
        // pick the requested axes from the encapsulated Frame and build
        // a new NormMap.  Output indices equal the input indices because
        // NormMap is identity per axis.
        if let Some(r) = crate::mappings::helpers::full_set_split(self, in_axes) {
            return Some(r);
        }
        crate::mappings::helpers::validate_in_axes(in_axes, self.nin())?;
        // pick_axes is 1-based at the public API.
        let axes_1based: Vec<usize> = in_axes.iter().map(|&i| i + 1).collect();
        let (sub_frame, _pick_map) =
            crate::frames::pick_axes(self.frame.as_ref(), &axes_1based).ok()?;
        let mut sub = NormMap::new(sub_frame);
        if self.inverted {
            sub.invert();
        }
        Some((in_axes.to_vec(), Box::new(sub)))
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
        let base = read_mapping_base(node);

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
        };

        if base.inverted {
            nm.invert();
        }
        nm.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(nm)
    }
}
