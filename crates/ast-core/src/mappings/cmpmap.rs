//! Compound mapping (series and parallel composition).
//!
//! `CmpMap` composes two mappings either:
//!
//! - **Series**: apply `first`, then `second`. Requires `first.nout() ==
//!   second.nin()`.
//! - **Parallel**: apply `first` and `second` side-by-side to disjoint
//!   coordinate ranges, then concatenate their outputs.

use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{
    MappingToNode, Node, Serializable, mapping_from_node, read_mapping_base, write_mapping_base,
};

use super::{DynMapping, Mapping};

/// Determines whether two mappings are composed in series or in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    /// Apply `first` then `second`.  `first.nout()` must equal `second.nin()`.
    Series,
    /// Apply `first` and `second` to separate coordinate ranges and
    /// concatenate the results.
    Parallel,
}

/// A mapping formed by composing two component mappings.
///
/// See [`Composition`] for the two modes.
///
/// `inv_a`/`inv_b` record each child's Invert flag at the time the CmpMap
/// was constructed or loaded.  Children are stored with their native Invert
/// state; the effective transform direction for each child is computed at
/// runtime as `forward XOR (inv_a != child.is_inverted())`.
pub struct CmpMap {
    first: DynMapping,
    second: DynMapping,
    kind: Composition,
    inverted: bool,
    do_not_simplify: bool,
    /// Whether the forward transform is defined.
    pub(crate) tran_forward: bool,
    /// Whether the inverse transform is defined.
    pub(crate) tran_inverse: bool,
    /// Recorded Invert flag for first child (snapshot at construction/load).
    inv_a: bool,
    /// Recorded Invert flag for second child (snapshot at construction/load).
    inv_b: bool,
}

impl CmpMap {
    /// Create a new `CmpMap`.
    ///
    /// - `Series`: `first.nout()` must equal `second.nin()`.
    /// - `Parallel`: no dimension constraint.
    pub fn new(first: DynMapping, second: DynMapping, kind: Composition) -> AstResult<Self> {
        if kind == Composition::Series && first.nout() != second.nin() {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!(
                    "series CmpMap: first.nout()={} but second.nin()={}",
                    first.nout(),
                    second.nin()
                ),
            ));
        }
        let inv_a = first.is_inverted();
        let inv_b = second.is_inverted();
        Ok(Self {
            first,
            second,
            kind,
            inverted: false,
            do_not_simplify: false,
            tran_forward: true,
            tran_inverse: true,
            inv_a,
            inv_b,
        })
    }

    /// The first component mapping.
    pub(crate) fn first(&self) -> &dyn Mapping {
        self.first.as_ref()
    }

    /// The second component mapping.
    pub(crate) fn second(&self) -> &dyn Mapping {
        self.second.as_ref()
    }

    /// The composition mode (series or parallel).
    pub(crate) fn composition(&self) -> Composition {
        self.kind
    }

    /// Recorded Invert flag for first child.
    pub(crate) fn inv_a(&self) -> bool {
        self.inv_a
    }

    /// Recorded Invert flag for second child.
    pub(crate) fn inv_b(&self) -> bool {
        self.inv_b
    }

    /// Whether the first child's current Invert differs from the recorded
    /// snapshot — if so, the effective transform direction must flip.
    fn first_invert_changed(&self) -> bool {
        self.inv_a != self.first.is_inverted()
    }

    /// Whether the second child's current Invert differs from the recorded
    /// snapshot.
    fn second_invert_changed(&self) -> bool {
        self.inv_b != self.second.is_inverted()
    }

    /// Effective Nin of first child when the CmpMap is used in direction `fwd`.
    fn effective_first_nin(&self, fwd: bool) -> usize {
        let fwd1 = fwd != self.first_invert_changed();
        if fwd1 {
            self.first.nin()
        } else {
            self.first.nout()
        }
    }

    /// Effective Nout of first child when the CmpMap is used in direction `fwd`.
    fn effective_first_nout(&self, fwd: bool) -> usize {
        let fwd1 = fwd != self.first_invert_changed();
        if fwd1 {
            self.first.nout()
        } else {
            self.first.nin()
        }
    }

    /// Effective Nin of second child when the CmpMap is used in direction `fwd`.
    fn effective_second_nin(&self, fwd: bool) -> usize {
        let fwd2 = fwd != self.second_invert_changed();
        if fwd2 {
            self.second.nin()
        } else {
            self.second.nout()
        }
    }

    /// Effective Nout of second child when the CmpMap is used in direction `fwd`.
    fn effective_second_nout(&self, fwd: bool) -> usize {
        let fwd2 = fwd != self.second_invert_changed();
        if fwd2 {
            self.second.nout()
        } else {
            self.second.nin()
        }
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    // ------------------------------------------------------------------ //
    // Series helpers                                                       //
    // ------------------------------------------------------------------ //

    fn series_transform(&self, pts: &PointSet, fwd: bool) -> AstResult<PointSet> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();
        if fwd {
            let mid = self.first.transform(pts, fwd1)?;
            self.second.transform(&mid, fwd2)
        } else {
            let mid = self.second.transform(pts, fwd2)?;
            self.first.transform(&mid, fwd1)
        }
    }

    fn series_transform_into(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();
        if fwd {
            let mid = self.first.transform(pts, fwd1)?;
            self.second.transform_into(&mid, fwd2, out)
        } else {
            let mid = self.second.transform(pts, fwd2)?;
            self.first.transform_into(&mid, fwd1, out)
        }
    }

    // ------------------------------------------------------------------ //
    // Parallel helpers                                                     //
    // ------------------------------------------------------------------ //

    fn parallel_transform(&self, pts: &PointSet, fwd: bool) -> AstResult<PointSet> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();

        let npoint = pts.npoint();
        let split = self.effective_first_nin(fwd);

        let pts_a = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., ..split]).to_owned());
        let pts_b = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., split..]).to_owned());

        let out_a = self.first.transform(&pts_a, fwd1)?;
        let out_b = self.second.transform(&pts_b, fwd2)?;

        let ncols_a = out_a.ncoord();
        let ncols_b = out_b.ncoord();
        let mut combined = PointSet::zeros(npoint, ncols_a + ncols_b);
        {
            let mut view = combined.view_mut();
            view.slice_mut(ndarray::s![.., ..ncols_a])
                .assign(&out_a.view());
            view.slice_mut(ndarray::s![.., ncols_a..])
                .assign(&out_b.view());
        }
        Ok(combined)
    }

    fn parallel_transform_into(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.parallel_transform(pts, fwd)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }
}

// Manual Debug impl because DynMapping (Box<dyn Mapping>) is not Debug.
impl fmt::Debug for CmpMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CmpMap")
            .field("kind", &self.kind)
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for CmpMap {
    fn clone(&self) -> Self {
        CmpMap {
            first: self.first.clone(),
            second: self.second.clone(),
            kind: self.kind,
            inverted: self.inverted,
            do_not_simplify: self.do_not_simplify,
            tran_forward: self.tran_forward,
            tran_inverse: self.tran_inverse,
            inv_a: self.inv_a,
            inv_b: self.inv_b,
        }
    }
}

impl Mapping for CmpMap {
    fn nin(&self) -> usize {
        // The "forward nin" of the CmpMap (before considering self.inverted)
        // is determined by the children in the CmpMap's nominal forward direction.
        let fwd_nin = match self.kind {
            Composition::Series => self.effective_first_nin(true),
            Composition::Parallel => {
                self.effective_first_nin(true) + self.effective_second_nin(true)
            }
        };
        let fwd_nout = match self.kind {
            Composition::Series => self.effective_second_nout(true),
            Composition::Parallel => {
                self.effective_first_nout(true) + self.effective_second_nout(true)
            }
        };
        if self.inverted { fwd_nout } else { fwd_nin }
    }

    fn nout(&self) -> usize {
        let fwd_nin = match self.kind {
            Composition::Series => self.effective_first_nin(true),
            Composition::Parallel => {
                self.effective_first_nin(true) + self.effective_second_nin(true)
            }
        };
        let fwd_nout = match self.kind {
            Composition::Series => self.effective_second_nout(true),
            Composition::Parallel => {
                self.effective_first_nout(true) + self.effective_second_nout(true)
            }
        };
        if self.inverted { fwd_nin } else { fwd_nout }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform(pts, fwd),
            Composition::Parallel => self.parallel_transform(pts, fwd),
        }
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform_into(pts, fwd, out),
            Composition::Parallel => self.parallel_transform_into(pts, fwd, out),
        }
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }
        crate::mappings::simplify::simplify_mapping(self)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_cmp) = other.as_any().downcast_ref::<CmpMap>() else {
            return false;
        };
        if self.kind != other_cmp.kind {
            return false;
        }
        let series = self.kind == Composition::Series;
        // Flatten both into canonical component lists, accounting for
        // tree structure differences like (A;(B;C)) vs ((A;B);C).
        let Ok(list_self) = super::simplify::flatten(self, false, series) else {
            return false;
        };
        let Ok(list_other) = super::simplify::flatten(other_cmp, false, series) else {
            return false;
        };
        if list_self.len() != list_other.len() {
            return false;
        }
        list_self
            .iter()
            .zip(list_other.iter())
            .all(|((m1, inv1), (m2, inv2))| inv1 == inv2 && m1.equal(m2.as_ref()))
    }

    fn map_split(&self, first_out: usize, nout_wanted: usize) -> Option<DynMapping> {
        if self.kind != Composition::Parallel {
            return None;
        }
        let na = self.effective_first_nout(!self.inverted);
        let nb = self.effective_second_nout(!self.inverted);

        if first_out + nout_wanted > na + nb {
            return None;
        }

        // Whether each child is effectively inverted relative to the
        // CmpMap's forward direction.
        let eff_inv_a = self.inverted != self.first_invert_changed();
        let eff_inv_b = self.inverted != self.second_invert_changed();

        if first_out == 0 && nout_wanted == na {
            let mut m = self.first.simplified().ok()?;
            if eff_inv_a {
                m.invert();
            }
            Some(m)
        } else if first_out == na && nout_wanted == nb {
            let mut m = self.second.simplified().ok()?;
            if eff_inv_b {
                m.invert();
            }
            Some(m)
        } else if first_out < na && first_out + nout_wanted <= na {
            if eff_inv_a {
                let mut sub = self.first.map_split(first_out, nout_wanted)?;
                sub.invert();
                Some(sub)
            } else {
                self.first.map_split(first_out, nout_wanted)
            }
        } else if first_out >= na && first_out + nout_wanted <= na + nb {
            if eff_inv_b {
                let mut sub = self.second.map_split(first_out - na, nout_wanted)?;
                sub.invert();
                Some(sub)
            } else {
                self.second.map_split(first_out - na, nout_wanted)
            }
        } else {
            None
        }
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn has_forward(&self) -> bool {
        self.tran_forward
    }

    fn has_inverse(&self) -> bool {
        self.tran_inverse
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for CmpMap {
    fn class_name(&self) -> &'static str {
        "CmpMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compound Mapping".to_string());
        write_mapping_base(&mut node, self);

        // Series is the default (1); only write for parallel (0).
        if self.kind == Composition::Parallel {
            node.push_int(
                "Series",
                0,
                Some("Component Mappings applied in parallel".to_string()),
            );
        }

        // Component invert flags (only if non-zero).
        if self.inv_a {
            node.push_int(
                "InvA",
                1,
                Some("First Mapping used in inverse direction".to_string()),
            );
        }
        if self.inv_b {
            node.push_int(
                "InvB",
                1,
                Some("Second Mapping used in inverse direction".to_string()),
            );
        }

        // Component mappings as nested objects.
        let node_a = self.first.to_node()?;
        let node_b = self.second.to_node()?;
        node.push_object("MapA", node_a, Some("First component Mapping".to_string()));
        node.push_object("MapB", node_b, Some("Second component Mapping".to_string()));

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        let is_series = node.get_int("Series", 1) != 0;
        let kind = if is_series {
            Composition::Series
        } else {
            Composition::Parallel
        };

        let inv_a = node.get_int("InvA", 0) != 0;
        let inv_b = node.get_int("InvB", 0) != 0;

        let node_a = node
            .get_object("MapA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpMap: missing MapA"))?;
        let node_b = node
            .get_object("MapB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpMap: missing MapB"))?;

        let map_a = mapping_from_node(node_a)?;
        let map_b = mapping_from_node(node_b)?;

        // Children keep their native Invert state as serialized.
        // inv_a/inv_b record the expected child state; the effective
        // transform direction is computed at runtime via
        // first_invert_changed() / second_invert_changed().
        let mut cmp = Self {
            first: map_a,
            second: map_b,
            kind,
            inverted: false,
            do_not_simplify: false,
            tran_forward: true,
            tran_inverse: true,
            inv_a,
            inv_b,
        };
        cmp.tran_forward = base.has_forward;
        cmp.tran_inverse = base.has_inverse;
        cmp.set_do_not_simplify(base.do_not_simplify);
        if base.inverted {
            cmp.invert();
        }
        Ok(cmp)
    }
}
