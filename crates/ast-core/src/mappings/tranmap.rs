//! Mapping with specified forward and inverse transformations.
//!
//! See [`TranMap`].

use std::fmt;

use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{
    MappingToNode, Node, Serializable, mapping_from_node, read_mapping_base, write_mapping_base,
};

use super::cmpmap::{CmpMap, Composition};
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Mapping with specified forward and inverse transformations.
///
/// A TranMap is a Mapping which combines the forward transformation of a
/// supplied Mapping with the inverse transformation of another supplied
/// Mapping, ignoring the unused transformation in each Mapping (indeed the
/// unused transformation need not exist).
///
/// When the forward transformation of the TranMap is referred to, the
/// transformation actually used is the forward transformation of the first
/// Mapping supplied when the TranMap was constructed.
/// Likewise, when the inverse transformation of the TranMap is referred to,
/// the transformation actually used is the inverse transformation of the
/// second Mapping supplied when the TranMap was constructed.
///
/// ```
/// use ast_core::mappings::{Mapping, TranMap, ZoomMap};
///
/// // Forward multiplies by 2, inverse divides by 4.
/// let map = TranMap::new(
///     Box::new(ZoomMap::new(1, 2.0).unwrap()),
///     Box::new(ZoomMap::new(1, 4.0).unwrap()),
/// )
/// .unwrap();
/// assert_eq!(map.tran1(&[3.0], true).unwrap(), vec![6.0]);
/// assert_eq!(map.tran1(&[8.0], false).unwrap(), vec![2.0]);
/// ```
// `inv_a`/`inv_b` record each child's Invert flag at the time the TranMap
// was constructed or loaded. The effective transform direction for each
// child is computed at runtime based on these recorded states.
pub struct TranMap {
    map1: DynMapping,
    map2: DynMapping,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// Whether the forward transform is defined.
    tran_forward: bool,
    /// Whether the inverse transform is defined.
    tran_inverse: bool,
    /// Recorded Invert flag for first child (snapshot at construction/load).
    inv_a: bool,
    /// Recorded Invert flag for second child (snapshot at construction/load).
    inv_b: bool,
}

impl TranMap {
    /// Mutable access to the two component Mappings, `map1` (forward) and
    /// `map2` (inverse), as C's `astDecompose` hands them out.
    pub(crate) fn components_mut(&mut self) -> (&mut DynMapping, &mut DynMapping) {
        (&mut self.map1, &mut self.map2)
    }

    /// Create a TranMap from `map1`, which defines the forward
    /// transformation, and `map2`, which defines the inverse transformation.
    ///
    /// The TranMap takes ownership of both Mappings.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Intrd` if `map1` has no forward transformation or
    ///   `map2` has no inverse transformation.
    /// - `ErrorCode::Innco` if the two Mappings have different numbers of
    ///   inputs or of outputs.
    #[doc(alias = "astTranMap")]
    pub fn new(map1: DynMapping, map2: DynMapping) -> AstResult<Self> {
        // The forward transformation is map1's and the inverse map2's, so C
        // requires both to exist (tranmap.c astInitTranMap).
        if !map1.has_forward() {
            return Err(AstError::new(
                ErrorCode::Intrd,
                "TranMap: the first Mapping is not able to transform coordinates in \
                 the forward direction",
            ));
        }
        if !map2.has_inverse() {
            return Err(AstError::new(
                ErrorCode::Intrd,
                "TranMap: the second Mapping is not able to transform coordinates in \
                 the inverse direction",
            ));
        }
        // Validate dimensions match.
        if map1.nin() != map2.nin() {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!(
                    "TranMap: map1.nin()={} but map2.nin()={}",
                    map1.nin(),
                    map2.nin()
                ),
            ));
        }
        if map1.nout() != map2.nout() {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!(
                    "TranMap: map1.nout()={} but map2.nout()={}",
                    map1.nout(),
                    map2.nout()
                ),
            ));
        }

        let inv_a = map1.is_inverted();
        let inv_b = map2.is_inverted();

        // The forward transformation applies map1 forwards and the inverse
        // applies map2 backwards, so each direction is defined only when
        // the corresponding component direction is (tranmap.c
        // GetTranForward / GetTranInverse).
        let tran_forward = map1.has_forward();
        let tran_inverse = map2.has_inverse();

        Ok(Self {
            map1,
            map2,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            tran_forward,
            tran_inverse,
            inv_a,
            inv_b,
        })
    }

    /// The first component mapping (defines forward transformation).
    #[allow(dead_code)]
    pub(crate) fn map1(&self) -> &dyn Mapping {
        self.map1.as_ref()
    }

    /// The second component mapping (defines inverse transformation).
    #[allow(dead_code)]
    pub(crate) fn map2(&self) -> &dyn Mapping {
        self.map2.as_ref()
    }

    /// Recorded Invert flag for first child.
    #[allow(dead_code)]
    pub(crate) fn inv_a(&self) -> bool {
        self.inv_a
    }

    /// Recorded Invert flag for second child.
    #[allow(dead_code)]
    pub(crate) fn inv_b(&self) -> bool {
        self.inv_b
    }

    /// Whether the first child's current Invert differs from the recorded
    /// snapshot — if so, the effective transform direction must flip.
    fn map1_invert_changed(&self) -> bool {
        self.inv_a != self.map1.is_inverted()
    }

    /// Whether the second child's current Invert differs from the recorded
    /// snapshot.
    fn map2_invert_changed(&self) -> bool {
        self.inv_b != self.map2.is_inverted()
    }

    /// Intrinsic nin (before considering self.inverted).
    fn intrinsic_nin(&self) -> usize {
        self.map1.nin()
    }

    /// Intrinsic nout (before considering self.inverted).
    fn intrinsic_nout(&self) -> usize {
        self.map1.nout()
    }
}

// Manual Debug impl because DynMapping (Box<dyn Mapping>) is not Debug.
impl fmt::Debug for TranMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TranMap")
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for TranMap {
    fn clone(&self) -> Self {
        TranMap {
            map1: self.map1.clone(),
            map2: self.map2.clone(),
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            id: Default::default(),
            mapping_flags: self.mapping_flags.clone(),
            tran_forward: self.tran_forward,
            tran_inverse: self.tran_inverse,
            inv_a: self.inv_a,
            inv_b: self.inv_b,
        }
    }
}

impl Mapping for TranMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// tranmap.c `RemoveRegions`: remove the Regions from both components;
    /// two UnitMaps become one.
    fn mapping_without_regions(&self) -> Option<DynMapping> {
        let new1 = self.map1.mapping_without_regions();
        let new2 = self.map2.mapping_without_regions();
        if new1.is_none() && new2.is_none() {
            return None;
        }
        let map1 = new1.unwrap_or_else(|| self.map1.clone_box());
        let map2 = new2.unwrap_or_else(|| self.map2.clone_box());
        if map1.as_any().is::<UnitMap>() && map2.as_any().is::<UnitMap>() {
            // Both components act on the TranMap's own inputs, so the result
            // has its number of inputs (C gives the sum of the two, fixed
            // upstream).
            return Some(Box::new(UnitMap::new(map1.nin())));
        }
        let mut new = self.clone();
        new.map1 = map1;
        new.map2 = map2;
        Some(Box::new(new))
    }

    fn nin(&self) -> usize {
        if self.inverted {
            self.intrinsic_nout()
        } else {
            self.intrinsic_nin()
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.intrinsic_nin()
        } else {
            self.intrinsic_nout()
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        // Determine the effective direction, accounting for the TranMap's inversion.
        let effective_fwd = forward != self.inverted;

        if effective_fwd {
            // Use map1's forward transformation.
            // The effective direction for map1 is forward, but we need to
            // account for any change in map1's invert flag since construction.
            let fwd1 = !self.map1_invert_changed();
            self.map1.transform(pts, fwd1)
        } else {
            // Use map2's inverse transformation.
            // The effective direction for map2 is inverse (false), but we need
            // to account for any change in map2's invert flag since construction.
            let fwd2 = self.map2_invert_changed();
            self.map2.transform(pts, fwd2)
        }
    }

    fn transform_into(&self, pts: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let effective_fwd = forward != self.inverted;

        if effective_fwd {
            let fwd1 = !self.map1_invert_changed();
            self.map1.transform_into(pts, fwd1, out)
        } else {
            let fwd2 = self.map2_invert_changed();
            self.map2.transform_into(pts, fwd2, out)
        }
    }

    /// NaN-as-bad pass-through: delegate to whichever component map's
    /// `transform_into_nan` is selected for this direction. If a child
    /// hasn't overridden the trait method it falls back to its own
    /// `transform_into` (BAD-checking), which is correct but skips the
    /// per-stage win for that child.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        out: PointsMut<'_>,
    ) -> AstResult<()> {
        let effective_fwd = forward != self.inverted;

        if effective_fwd {
            let fwd1 = !self.map1_invert_changed();
            self.map1.transform_into_nan(pts, fwd1, out)
        } else {
            let fwd2 = self.map2_invert_changed();
            self.map2.transform_into_nan(pts, fwd2, out)
        }
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // "Simplify the TranMap on its own" (`tranmap.c:834-897`) is an
        // either/or over three cases, and the applied inversion decides which
        // of them can run.
        let applied_inv = ctx.entry(where_).1;

        // Case 1: an inverted TranMap equals a forward one whose components are
        // inverted and swapped, so replace it with that and clear the invert
        // flag (`tranmap.c:837-850`). C stops there — an inverted TranMap never
        // reaches the other two cases in the same pass.
        if applied_inv {
            let mut new_map1 = self.map2.clone();
            let mut new_map2 = self.map1.clone();
            new_map1.invert();
            new_map2.invert();
            let normalized = TranMap::new(new_map1, new_map2).ok()?;
            ctx.set_pair_single(where_, (Box::new(normalized), false, false));
            return Some(where_);
        }

        // Case 2: simplify both encapsulated Mappings and replace the TranMap
        // when either reduced (`tranmap.c:851-863`). `astSimplify` returns the
        // object it was given when it cannot reduce it, recording *that* object
        // (`mapping.c:24763`), so both components end up recorded either way and
        // only a real reduction is reported as a change. Owned values make the
        // record an explicit rebuild rather than a side effect on a shared
        // pointer.
        let simp1 = self.map1.simplify().ok()?;
        let simp2 = self.map2.simplify().ok()?;
        let reduced = simp1.is_inverted() != self.map1.is_inverted()
            || !simp1.equal(self.map1.as_ref())
            || simp2.is_inverted() != self.map2.is_inverted()
            || !simp2.equal(self.map2.as_ref());

        if reduced {
            let tm = TranMap::new(simp1, simp2).ok()?;
            ctx.set_pair_single(where_, (Box::new(tm), false, false));
            return Some(where_);
        }

        // Case 3: when both components are defined in both directions, compose
        // the first with the inverse of the second; if that reduces to a UnitMap
        // the two are equal, so either one can stand for the whole TranMap
        // (`tranmap.c:864-887`).
        if self.map1.has_forward()
            && self.map1.has_inverse()
            && self.map2.has_forward()
            && self.map2.has_inverse()
        {
            let mut map2_inv = self.map2.clone();
            map2_inv.invert();
            let cmp = CmpMap::new(self.map1.clone(), map2_inv, Composition::Series).ok()?;
            let simp_cmp = <dyn Mapping>::simplify(&cmp).ok()?;
            if simp_cmp.as_any().is::<UnitMap>() {
                // C copies the component as it was when the TranMap was created
                // and leaves the list's invert flag alone (`tranmap.c:881-886`).
                // The list holds every entry in its forward state with the
                // intrinsic flag beside it, so an inverted component is stored
                // the way [`simplify::default_map_merge`] stores a merge result.
                let mut m1 = self.map1.clone();
                let orig = m1.is_inverted();
                if orig {
                    m1.invert();
                }
                ctx.set_pair_single(where_, (m1, orig, orig));
                return Some(where_);
            }
        }

        // Nothing changed above, so look for a neighbouring TranMap to merge
        // with, in series only (`tranmap.c:898-903`).
        super::simplify::default_map_merge(ctx, where_, series)
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        // TranMap merging only works in series.
        if !series {
            return None;
        }

        // The other mapping must also be a TranMap.
        let other_tm = other.as_any().downcast_ref::<TranMap>()?;

        // Determine the effective forward/inverse mappings for self,
        // accounting for the external inversion flag.
        let (self_fwd, self_inv_map) = if self_inv {
            // Inverted: forward uses map2's inverse, inverse uses map1's forward
            let mut sf = self.map2.clone();
            sf.invert();
            let mut si = self.map1.clone();
            si.invert();
            (sf, si)
        } else {
            (self.map1.clone(), self.map2.clone())
        };

        // Determine the effective forward/inverse mappings for other.
        let (other_fwd, other_inv_map) = if other_inv {
            let mut of = other_tm.map2.clone();
            of.invert();
            let mut oi = other_tm.map1.clone();
            oi.invert();
            (of, oi)
        } else {
            (other_tm.map1.clone(), other_tm.map2.clone())
        };

        // Check dimension compatibility for series composition.
        // Forward chain: self_fwd then other_fwd.
        if self_fwd.nout() != other_fwd.nin() {
            return None;
        }

        // Combine forward mappings: self_fwd ; other_fwd
        let cmp_fwd = CmpMap::new(self_fwd, other_fwd, Composition::Series).ok()?;
        let mut simp_fwd = <dyn Mapping>::simplify(&cmp_fwd).ok()?;

        // Combine inverse mappings: other_inv ; self_inv (reverse order for inverse)
        let cmp_inv = CmpMap::new(other_inv_map, self_inv_map, Composition::Series).ok()?;
        let mut simp_inv = <dyn Mapping>::simplify(&cmp_inv).ok()?;

        // C reaches both through `astSimplify` (tranmap.c:951, 955), which
        // records the result as simplified.
        simp_fwd.set_has_been_simplified(true);
        simp_inv.set_has_been_simplified(true);

        // Check if any simplification happened (pointer comparison analog).
        let fwd_simplified = !simp_fwd.equal(&cmp_fwd as &dyn Mapping);
        let inv_simplified = !simp_inv.equal(&cmp_inv as &dyn Mapping);

        if fwd_simplified || inv_simplified {
            // Build a new TranMap from the simplified forward/inverse.
            let result = TranMap::new(simp_fwd, simp_inv).ok()?;
            Some(Box::new(result))
        } else {
            None
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        // tranmap.c Rate asks the component that supplies the forward
        // transformation: the first, in its recorded orientation, or for an
        // inverted TranMap the second, in the opposite one.
        let (component, invert) = if self.inverted {
            (&self.map2, !self.inv_b)
        } else {
            (&self.map1, self.inv_a)
        };
        if component.is_inverted() == invert {
            component.rate(at, ax1, ax2)
        } else {
            let mut component = component.clone_box();
            component.invert();
            component.rate(at, ax1, ax2)
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_tm) = other.as_any().downcast_ref::<TranMap>() else {
            return false;
        };

        // Check dimensions match.
        if self.nin() != other_tm.nin() || self.nout() != other_tm.nout() {
            return false;
        }

        // tranmap.c Equal compares the components in the orientations each
        // TranMap recorded when it was made. With the same Invert flag the
        // components must match pairwise; with different ones, an inverted
        // TranMap(A, B) is TranMap(inv B, inv A).
        let oriented = |map: &DynMapping, invert: bool, flip: bool| -> DynMapping {
            let mut m = map.clone_box();
            if m.is_inverted() != (invert != flip) {
                m.invert();
            }
            m
        };
        let a1 = oriented(&self.map1, self.inv_a, false);
        let a2 = oriented(&self.map2, self.inv_b, false);
        if self.inverted == other_tm.inverted {
            let b1 = oriented(&other_tm.map1, other_tm.inv_a, false);
            let b2 = oriented(&other_tm.map2, other_tm.inv_b, false);
            a1.equal(b1.as_ref()) && a2.equal(b2.as_ref())
        } else {
            let b1 = oriented(&other_tm.map1, other_tm.inv_a, true);
            let b2 = oriented(&other_tm.map2, other_tm.inv_b, true);
            a1.equal(b2.as_ref()) && a2.equal(b1.as_ref())
        }
    }

    fn has_forward(&self) -> bool {
        if self.inverted {
            self.tran_inverse
        } else {
            self.tran_forward
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            self.tran_forward
        } else {
            self.tran_inverse
        }
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
        // Mirrors tranmap.c::MapSplit (~line 1017). The C body first calls
        // the parent (default) impl for the full-input case, then a
        // per-class fallback that splits both the forward and inverse
        // mappings, verifies they recover the same input set, and builds
        // a new TranMap from the pair.
        // An input out of range is AST__AXIIN from the parent's MapSplit.
        if let Some(r) = crate::mappings::helpers::base_map_split(self, in_axes)? {
            return Ok(Some(r));
        }

        // Pick the forward and inverse component mappings, with their
        // construction-snapshot Invert flags. Mirrors C lines 1115-1125.
        let (fmap_child, finv, imap_child, iinv) = if !self.inverted {
            (
                self.map1.as_ref(),
                self.inv_a(),
                self.map2.as_ref(),
                self.inv_b(),
            )
        } else {
            (
                self.map2.as_ref(),
                !self.inv_b(),
                self.map1.as_ref(),
                !self.inv_a(),
            )
        };

        // Build snapshot-oriented views of each child. C does this by
        // mutating the child's Invert flag in place and restoring it
        // afterwards; in Rust we clone+invert when the child's current
        // state differs from the snapshot.
        let fmap_view: DynMapping = if fmap_child.is_inverted() != finv {
            let mut tmp = fmap_child.clone_box();
            tmp.invert();
            tmp
        } else {
            fmap_child.clone_box()
        };
        let imap_view: DynMapping = if imap_child.is_inverted() != iinv {
            let mut tmp = imap_child.clone_box();
            tmp.invert();
            tmp
        } else {
            imap_child.clone_box()
        };

        // Split the forward mapping by the requested inputs (0-based, to
        // match this protected method's encoding).
        let Some((out, rfmap)) = fmap_view.map_split_zero_based(in_axes)? else {
            return Ok(None);
        };

        // Invert the inverse-mapping view so its forward direction matches
        // the original-forward semantics, then split by the recovered output
        // indices. C: astInvert(imap); ires = astMapSplit(imap, nout, out).
        let mut imap_view_inv = imap_view;
        imap_view_inv.invert();
        let Some((ires, rimap_inv_view)) = imap_view_inv.map_split_zero_based(&out)? else {
            return Ok(None);
        };

        // Verify the recovered inputs match the requested inputs in order
        // (C lines 1151-1158). If not, the TranMap can't be split this way.
        if ires.len() != in_axes.len() || ires.iter().zip(in_axes).any(|(a, b)| a != b) {
            return Ok(None);
        }

        // Invert the inverse-side sub-mapping back to the inverse-mapping
        // orientation (C: astInvert(rimap)).
        let mut rimap = rimap_inv_view;
        rimap.invert();

        // Build the new TranMap.
        let new_tm = TranMap::new(rfmap, rimap)?;
        Ok(Some((out, Box::new(new_tm))))
    }
}

impl Serializable for TranMap {
    fn class_name(&self) -> &'static str {
        "TranMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compound Transformation Mapping".to_string());
        write_mapping_base(&mut node, self);

        // The component Invert flags, each set when true.
        node.write_int(
            "InvA",
            self.inv_a,
            false,
            i64::from(self.inv_a),
            if self.inv_a {
                "First Mapping used in inverse direction"
            } else {
                "First Mapping used in forward direction"
            },
        );
        node.write_int(
            "InvB",
            self.inv_b,
            false,
            i64::from(self.inv_b),
            if self.inv_b {
                "Second Mapping used in inverse direction"
            } else {
                "Second Mapping used in forward direction"
            },
        );

        // Component mappings as nested objects.
        let node_a = self.map1.to_node()?;
        let node_b = self.map2.to_node()?;
        node.push_object(
            "MapA",
            node_a,
            Some("Mapping for forward transformation".to_string()),
        );
        node.push_object(
            "MapB",
            node_b,
            Some("Mapping for inverse transformation".to_string()),
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("TranMap");
        let base = read_mapping_base(whole);

        let inv_a = node.get_int("InvA", 0) != 0;
        let inv_b = node.get_int("InvB", 0) != 0;

        let node_a = node
            .get_object("MapA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "TranMap: missing MapA"))?;
        let node_b = node
            .get_object("MapB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "TranMap: missing MapB"))?;

        let map_a = mapping_from_node(node_a)?;
        let map_b = mapping_from_node(node_b)?;

        // Definedness is dynamic in C (GetTranForward/GetTranInverse
        // consult the components; the dumped base flags are always 1 for
        // TranMap), so recompute from the loaded components rather than
        // trusting the file. inv_a/inv_b give each component's invert
        // state as applied, which may differ from its serialized state.
        let tran_forward = if map_a.is_inverted() == inv_a {
            map_a.has_forward()
        } else {
            map_a.has_inverse()
        };
        let tran_inverse = if map_b.is_inverted() == inv_b {
            map_b.has_inverse()
        } else {
            map_b.has_forward()
        };
        let mut tm = Self {
            map1: map_a,
            map2: map_b,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            tran_forward,
            tran_inverse,
            inv_a,
            inv_b,
        };
        if base.inverted {
            tm.invert();
        }

        tm.set_has_been_simplified(base.has_been_simplified);
        Ok(tm)
    }
}
