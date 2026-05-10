//! Mapping which represents differentiation (a single Jacobian element).
//!
//! A `RateMap` is a Mapping which represents a single element of the
//! Jacobian matrix of another Mapping. The Mapping for which the
//! Jacobian is required is specified when the new RateMap is created,
//! and is referred to as the "encapsulated Mapping" below.
//!
//! The number of inputs to a RateMap is the same as the number of inputs
//! to its encapsulated Mapping. The number of outputs from a RateMap
//! is always one. This one output equals the rate of change of a
//! specified output of the encapsulated Mapping with respect to a
//! specified input of the encapsulated Mapping.
//!
//! A RateMap which has not been inverted does not define an inverse
//! transformation. If a RateMap has been inverted then it will define
//! an inverse transformation but not a forward transformation.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{
    MappingToNode, Node, NodeValue, Serializable, mapping_from_node, read_mapping_base,
    write_mapping_base,
};

use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that computes a single element of the Jacobian matrix
/// of an encapsulated Mapping.
///
/// The single output equals the rate of change of a specified output
/// of the encapsulated Mapping with respect to a specified input.
///
/// # Axis indices
///
/// Both `ax1` (output) and `ax2` (input) use 1-based indexing.
///
/// # Transform directions
///
/// When not inverted: forward is defined (computes the rate), inverse is not.
/// When inverted: inverse is defined (computes the rate), forward is not.
pub struct RateMap {
    /// The encapsulated mapping whose derivative is computed.
    map: DynMapping,
    /// 1-based index of the output axis being differentiated.
    ax1: usize,
    /// 1-based index of the input axis being varied.
    ax2: usize,
    /// Whether this RateMap is inverted.
    inverted: bool,
    /// Whether simplification is inhibited.
    do_not_simplify: bool,
    /// Snapshot of the encapsulated mapping's Invert flag at construction
    /// time. Used for serialization compatibility with the C library.
    invert_snapshot: bool,
}

impl RateMap {
    /// Create a new `RateMap`.
    ///
    /// - `map`: The encapsulated mapping to differentiate.
    /// - `ax1`: 1-based index of the output axis being differentiated.
    /// - `ax2`: 1-based index of the input axis being varied.
    ///
    /// The encapsulated mapping must have a forward transformation defined.
    pub fn new(map: DynMapping, ax1: usize, ax2: usize) -> AstResult<Self> {
        // The encapsulated mapping must have a forward transformation.
        if !map.has_forward() {
            return Err(AstError::new(
                ErrorCode::Intrd,
                "RateMap: the encapsulated Mapping has no forward transformation",
            ));
        }

        // Validate axis indices (1-based).
        let nout = map.nout();
        let nin = map.nin();
        if ax1 < 1 || ax1 > nout {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!("RateMap: output axis {ax1} is out of range (1..{nout})"),
            ));
        }
        if ax2 < 1 || ax2 > nin {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!("RateMap: input axis {ax2} is out of range (1..{nin})"),
            ));
        }

        let invert_snapshot = map.is_inverted();

        Ok(Self {
            map,
            ax1,
            ax2,
            inverted: false,
            do_not_simplify: false,
            invert_snapshot,
        })
    }

    /// The encapsulated mapping.
    #[allow(dead_code)]
    pub(crate) fn map(&self) -> &dyn Mapping {
        self.map.as_ref()
    }

    /// 1-based index of the output axis being differentiated.
    #[allow(dead_code)]
    pub fn ax1(&self) -> usize {
        self.ax1
    }

    /// 1-based index of the input axis being varied.
    #[allow(dead_code)]
    pub fn ax2(&self) -> usize {
        self.ax2
    }

    /// Intrinsic nin (before considering self.inverted).
    fn intrinsic_nin(&self) -> usize {
        self.map.nin()
    }

    /// Intrinsic nout (before considering self.inverted) — always 1.
    fn intrinsic_nout(&self) -> usize {
        1
    }
}

// Manual Debug impl because DynMapping (Box<dyn Mapping>) is not Debug.
impl fmt::Debug for RateMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateMap")
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("ax1", &self.ax1)
            .field("ax2", &self.ax2)
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for RateMap {
    fn clone(&self) -> Self {
        RateMap {
            map: self.map.clone(),
            ax1: self.ax1,
            ax2: self.ax2,
            inverted: self.inverted,
            do_not_simplify: self.do_not_simplify,
            invert_snapshot: self.invert_snapshot,
        }
    }
}

impl Mapping for RateMap {
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
        // Determine the effective direction.
        let effective_fwd = forward != self.inverted;

        if !effective_fwd {
            // The RateMap class does not define an inverse transformation
            // (or forward when inverted). Return BAD for all outputs.
            let npoint = pts.npoint();
            let nout = if forward { self.nout() } else { self.nin() };
            let columns: Vec<Vec<f64>> = (0..nout).map(|_| vec![BAD_VALUE; npoint]).collect();
            let col_refs: Vec<&[f64]> = columns.iter().map(|c| c.as_slice()).collect();
            return PointSet::from_columns(&col_refs);
        }

        // Forward (not inverted): compute rate for each point.
        let npoint = pts.npoint();
        let nin = self.intrinsic_nin();
        let mut results = vec![BAD_VALUE; npoint];

        for (ipoint, result) in results.iter_mut().enumerate().take(npoint) {
            // Extract the input position for this point.
            let point = pts.point(ipoint);

            // Check if point has any BAD values.
            let has_bad = point
                .iter()
                .take(nin)
                .any(|&v| crate::mappings::helpers::is_bad(v));
            if has_bad {
                continue; // result stays BAD_VALUE
            }

            // Compute the rate of change using the encapsulated mapping's
            // rate method.
            *result = self.map.rate(point, self.ax1, self.ax2)?;
        }

        PointSet::from_columns(&[&results])
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        let npoint = result.npoint();
        let nout = result.ncoord();
        for i in 0..npoint {
            let src = result.point(i);
            let dst = out.point_mut(i);
            dst[..nout].copy_from_slice(&src[..nout]);
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        // Step 1: Simplify the encapsulated mapping.
        let simp_map = self.map.simplified()?;

        // Check if simplification changed the encapsulated mapping.
        let changed =
            simp_map.is_inverted() != self.map.is_inverted() || !simp_map.equal(self.map.as_ref());

        if changed {
            // Construct a new RateMap around the simplified mapping.
            let mut new_rm = RateMap::new(simp_map, self.ax1, self.ax2)?;
            if self.inverted {
                new_rm.invert();
            }
            return Ok(Box::new(new_rm));
        }

        // No simplification was possible.
        Ok(Box::new(self.clone()))
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        // RateMap merging only works in series.
        if !series {
            return None;
        }

        // The other mapping must also be a RateMap.
        let other_rm = other.as_any().downcast_ref::<RateMap>()?;

        // Can only cancel if the invert flags are opposite.
        if self_inv == other_inv {
            return None;
        }

        // Check that the axis indices match.
        if self.ax1 != other_rm.ax1 || self.ax2 != other_rm.ax2 {
            return None;
        }

        // Check that the encapsulated mappings are equal.
        // We need to compare them with their construction-time invert states.
        if !self.map.equal(other_rm.map.as_ref()) {
            return None;
        }

        // RateMap cancels with its inverse → UnitMap.
        // In the C code, both entries in the map_list are replaced by
        // UnitMap(nax). For try_merge which returns a single merged mapping,
        // we return a UnitMap with the appropriate number of axes.
        //
        // When self_inv=false: self has nin=N, nout=1; other(inv) has nin=1, nout=N
        //   In series: self.nout=1 must equal other.nin=1 ✓
        //   Result: nin=N (from self.nin), nout=N (from other.nout)
        //   But that's not a UnitMap(N) since the dims don't match...
        //   Actually the C code replaces BOTH mappings with UnitMap(nax),
        //   effectively removing the pair. In our try_merge interface, the
        //   returned mapping replaces both. For series: input goes through
        //   self(N→1) then other(1→N). The pair should cancel to UnitMap(N).
        //   But dims don't actually match for series composition (N→1→N ≠ N→N).
        //   The C code works differently — it replaces each mapping individually
        //   in the list with a UnitMap. So each gets UnitMap(nax) where nax
        //   preserves the flow.
        //
        // For our interface (single merged result replacing both), the dims
        // must be consistent: merged.nin = self_effective.nin, merged.nout = other_effective.nout.
        // self_inv=false: self_eff.nin=N, other_eff(inv).nout=N → merged is UnitMap(N)? No,
        //   self_eff.nout=1, other_eff.nin=1, so series is valid (1==1).
        //   merged.nin=N, merged.nout=N → but RateMap(N→1) then inv_RateMap(1→N) is N→N.
        //   However this isn't a UnitMap in general, it's only valid because they cancel.
        // The C code's approach is to replace each with UnitMap individually, which
        // means self becomes UnitMap(N→N? no... UnitMap(1→1) for the nout=1 case.
        //
        // Looking at the C code again: line 793-800:
        //   nax is set to astGetNout(rmap1) or astGetNin(rmap1) depending on direction.
        //   Both map_list entries are replaced with UnitMap(nax).
        // When self_inv=false (forward RateMap at position `where`):
        //   For lower neighbor (cancel=where-1): nax = astGetNout(rmap1) = 1
        //   For upper neighbor (cancel=where+1): nax = astGetNin(rmap1) = N
        // This means: the C code replaces both with UnitMap(nax) which are then
        // simplified out by the CmpMap simplifier.
        //
        // For our try_merge: the merged result must have
        //   nin = effective_nin(self) and nout = effective_nout(other)
        // self_inv=false: eff_nin(self) = N; other_inv=true: eff_nout(other) = N
        // → UnitMap(N)? No, that doesn't work because the middle dimension is 1.
        //
        // Actually, in the simplify engine, try_merge returns a SINGLE mapping that
        // replaces both. The engine expects merged.nin = self_eff.nin and
        // merged.nout = other_eff.nout. For:
        //   self_inv=false: self_eff is RateMap(N→1)
        //   other_inv=true: other_eff is inv(RateMap) = RateMap(1→N)
        //   series: (N→1) then (1→N), but they cancel so result is N→N = UnitMap(N)
        //   Wait, but (N→1→N) doesn't cancel to identity in general.
        //   It only works because RateMap+inv(RateMap) is defined as cancelling.
        //
        // Actually wait — the simplify engine checks dimensions before calling try_merge.
        // For series, it requires self.nout == other.nin (accounting for inversion).
        // self_inv=false: self nout=1; other_inv=true: other nin=nout_intrinsic=1.
        // So series is valid: 1==1. Result: nin=self.nin=N, nout=other.nout=N → UnitMap(N).
        //
        // For self_inv=true: self nin=1; other_inv=false: other nin=N.
        // self_inv=true: self nout=N; other nin=N. Series: N==N ✓.
        // Result: nin=self.nin=1, nout=other.nout=1 → UnitMap(1).
        let result_nax = if self_inv { 1 } else { self.intrinsic_nin() };

        Some(Box::new(UnitMap::new(result_nax)))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_rm) = other.as_any().downcast_ref::<RateMap>() else {
            return false;
        };

        // Check dimensions match.
        if self.nin() != other_rm.nin() || self.nout() != other_rm.nout() {
            return false;
        }

        // For RateMap, Invert flags must be equal (from C Equal function).
        if self.inverted != other_rm.inverted {
            return false;
        }

        // Check axis indices.
        if self.ax1 != other_rm.ax1 || self.ax2 != other_rm.ax2 {
            return false;
        }

        // Check encapsulated mappings are equal.
        self.map.equal(other_rm.map.as_ref())
    }

    fn has_forward(&self) -> bool {
        // When not inverted, forward is defined (rate computation).
        // When inverted, forward is NOT defined.
        !self.inverted
    }

    fn has_inverse(&self) -> bool {
        // When not inverted, inverse is NOT defined.
        // When inverted, inverse IS defined (rate computation).
        self.inverted
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

impl Serializable for RateMap {
    fn class_name(&self) -> &'static str {
        "RateMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Differential Mapping".to_string());
        write_mapping_base(&mut node, self);

        // Input axis (0-based in serialization, only written if non-zero).
        // ax2 is the input axis index (1-based in Rust → 0-based in serial).
        let iin = (self.ax2 - 1) as i64;
        if iin != 0 {
            node.push_int("IIn", iin, Some("Index of Mapping input".to_string()));
        }

        // Output axis (0-based in serialization, only written if non-zero).
        // ax1 is the output axis index (1-based in Rust → 0-based in serial).
        let iout = (self.ax1 - 1) as i64;
        if iout != 0 {
            node.push_int("IOut", iout, Some("Index of Mapping output".to_string()));
        }

        // Encapsulated mapping's invert flag snapshot (only if non-zero).
        if self.invert_snapshot {
            node.push_int(
                "Inv",
                1,
                Some("Mapping used in inverse direction".to_string()),
            );
        }

        // Encapsulated mapping as a nested object.
        let map_node = self.map.to_node()?;
        node.push_object(
            "Map",
            map_node,
            Some("Mapping to be differentiated".to_string()),
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        // Read 0-based axis indices (default 0 = first axis).
        let iin = node.get_int("IIn", 0) as usize;
        let iout = node.get_int("IOut", 0) as usize;

        // Convert to 1-based.
        let ax1 = iout + 1;
        let ax2 = iin + 1;

        // Read the encapsulated mapping's invert flag snapshot.
        // This appears AFTER the IsA marker. Since "Inv" is also used by
        // the mapping base (for has_inverse), we need to look for the
        // RateMap-specific one. It appears after the last IsA item.
        let inv_snapshot = get_int_after_isa(node, "Inv", 0) != 0;

        // Read the encapsulated mapping.
        let map_node = node
            .get_object("Map")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "RateMap: missing Map"))?;
        let mut map = mapping_from_node(map_node)?;

        // If the invert snapshot differs from the child mapping's current
        // invert state, flip the child to match the snapshot.
        if inv_snapshot != map.is_inverted() {
            map.invert();
        }

        let mut rm = Self {
            map,
            ax1,
            ax2,
            inverted: false,
            do_not_simplify: false,
            invert_snapshot: inv_snapshot,
        };

        rm.set_do_not_simplify(base.do_not_simplify);
        if base.inverted {
            rm.invert();
        }
        Ok(rm)
    }
}

/// Look up an integer attribute that appears after the last `IsA` marker
/// in a node's items. Returns `default` if not found.
///
/// This is needed for keys like "Inv" that appear both in the mapping
/// base (before IsA) and in the derived class (after IsA) with different
/// semantics.
fn get_int_after_isa(node: &Node, key: &str, default: i64) -> i64 {
    // Find the position of the last IsA marker.
    let last_isa_pos = node.items.iter().rposition(|item| item.key == "IsA");

    let start = match last_isa_pos {
        Some(pos) => pos + 1,
        None => 0,
    };

    // Search for the key after the last IsA marker.
    for item in &node.items[start..] {
        if item.key == key {
            if let NodeValue::Unset(_) = &item.value {
                continue;
            }
            return match &item.value {
                NodeValue::Int(v) => *v,
                NodeValue::Double(v) => *v as i64,
                _ => default,
            };
        }
    }

    default
}
