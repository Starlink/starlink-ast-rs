//! Mapping with specified forward and inverse transformations.
//!
//! A `TranMap` combines the forward transformation of one Mapping with
//! the inverse transformation of another Mapping, ignoring the unused
//! transformation in each (indeed the unused transformation need not exist).
//!
//! When the forward transformation of the TranMap is used, the actual
//! transformation applied is the forward transformation of the first
//! Mapping supplied when the TranMap was constructed. When the inverse
//! transformation is used, the actual transformation applied is the
//! inverse transformation of the second Mapping.

use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{
    MappingToNode, Node, Serializable, mapping_from_node, read_mapping_base, write_mapping_base,
};

use super::cmpmap::{CmpMap, Composition};
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that combines the forward transformation of one Mapping
/// with the inverse transformation of another.
///
/// The forward transformation uses `map1`'s forward direction.
/// The inverse transformation uses `map2`'s inverse direction.
///
/// `inv_a`/`inv_b` record each child's Invert flag at the time the TranMap
/// was constructed or loaded. The effective transform direction for each
/// child is computed at runtime based on these recorded states.
pub struct TranMap {
    map1: DynMapping,
    map2: DynMapping,
    inverted: bool,
    do_not_simplify: bool,
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
    /// Create a new `TranMap`.
    ///
    /// - `map1` defines the forward transformation.
    /// - `map2` defines the inverse transformation.
    ///
    /// Requirements:
    /// - `map1.nin() == map2.nin()` and `map1.nout() == map2.nout()`
    /// - `map1` must have a forward transformation defined.
    /// - `map2` must have an inverse transformation defined.
    pub fn new(map1: DynMapping, map2: DynMapping) -> AstResult<Self> {
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

        Ok(Self {
            map1,
            map2,
            inverted: false,
            do_not_simplify: false,
            tran_forward: true,
            tran_inverse: true,
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
            do_not_simplify: self.do_not_simplify,
            tran_forward: self.tran_forward,
            tran_inverse: self.tran_inverse,
            inv_a: self.inv_a,
            inv_b: self.inv_b,
        }
    }
}

impl Mapping for TranMap {
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

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
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

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let effective_fwd = forward != self.inverted;

        if effective_fwd {
            let fwd1 = !self.map1_invert_changed();
            self.map1.transform_into(pts, fwd1, out)
        } else {
            let fwd2 = self.map2_invert_changed();
            self.map2.transform_into(pts, fwd2, out)
        }
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        // Step 1: If inverted, create a normalized TranMap with swapped,
        // inverted component mappings and clear the inverted flag.
        if self.inverted {
            let mut new_map1 = self.map2.clone();
            let mut new_map2 = self.map1.clone();
            new_map1.invert();
            new_map2.invert();
            let normalized = TranMap::new(new_map1, new_map2)?;
            return normalized.simplified();
        }

        // Step 2: Simplify each component independently.
        let simp1 = self.map1.simplified()?;
        let simp2 = self.map2.simplified()?;

        // Detect whether simplification changed either component. We check
        // both structural equality AND whether the inverted state differs,
        // because a ZoomMap(0.5,inverted) is equal() to ZoomMap(2.0) but has
        // a different representation that should be canonicalized.
        let changed1 =
            simp1.is_inverted() != self.map1.is_inverted() || !simp1.equal(self.map1.as_ref());
        let changed2 =
            simp2.is_inverted() != self.map2.is_inverted() || !simp2.equal(self.map2.as_ref());

        if changed1 || changed2 {
            // Construct a new TranMap from simplified components.
            let new_tm = TranMap::new(simp1, simp2)?;
            return Ok(Box::new(new_tm));
        }

        // Step 3: If both components define both directions, check if they
        // are equal (forward of map1 composed with inverse of map2 = UnitMap).
        if self.map1.has_forward()
            && self.map1.has_inverse()
            && self.map2.has_forward()
            && self.map2.has_inverse()
        {
            // Check if map1 and map2 are equal — if so, the TranMap can be
            // replaced by map1 alone (which has both forward and inverse).
            if self.map1.equal(self.map2.as_ref()) {
                return Ok(self.map1.clone());
            }

            // Also try: form CmpMap(map1, inv(map2), series) and see if it
            // simplifies to a UnitMap.
            let mut map2_inv = self.map2.clone();
            map2_inv.invert();
            let cmp = CmpMap::new(self.map1.clone(), map2_inv, Composition::Series)?;
            let simp_cmp = cmp.simplified()?;
            if simp_cmp.as_any().is::<UnitMap>() {
                // Components are effectively equal — replace with map1.
                return Ok(self.map1.clone());
            }
        }

        // No simplification possible.
        Ok(Box::new(self.clone()))
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
        let simp_fwd = cmp_fwd.simplified().ok()?;

        // Combine inverse mappings: other_inv ; self_inv (reverse order for inverse)
        let cmp_inv = CmpMap::new(other_inv_map, self_inv_map, Composition::Series).ok()?;
        let simp_inv = cmp_inv.simplified().ok()?;

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
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_tm) = other.as_any().downcast_ref::<TranMap>() else {
            return false;
        };

        // Check dimensions match.
        if self.nin() != other_tm.nin() || self.nout() != other_tm.nout() {
            return false;
        }

        // If both have the same inversion state, compare components directly.
        if self.inverted == other_tm.inverted {
            self.map1.equal(other_tm.map1.as_ref()) && self.map2.equal(other_tm.map2.as_ref())
        } else {
            // Different inversion states: an inverted TranMap(A, B) is
            // equivalent to TranMap(inv(B), inv(A)). So compare:
            // self.map1 with inv(other.map2) and self.map2 with inv(other.map1).
            let mut other_map1_inv = other_tm.map1.clone();
            other_map1_inv.invert();
            let mut other_map2_inv = other_tm.map2.clone();
            other_map2_inv.invert();

            self.map1.equal(other_map2_inv.as_ref()) && self.map2.equal(other_map1_inv.as_ref())
        }
    }

    fn has_forward(&self) -> bool {
        self.tran_forward
    }

    fn has_inverse(&self) -> bool {
        self.tran_inverse
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
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
        let base = read_mapping_base(node);

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

        let mut tm = Self {
            map1: map_a,
            map2: map_b,
            inverted: false,
            do_not_simplify: false,
            tran_forward: true,
            tran_inverse: true,
            inv_a,
            inv_b,
        };
        tm.tran_forward = base.has_forward;
        tm.tran_inverse = base.has_inverse;
        tm.set_do_not_simplify(base.do_not_simplify);
        if base.inverted {
            tm.invert();
        }
        Ok(tm)
    }
}
