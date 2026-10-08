//! 2-D pincushion/barrel distortion mapping.
//!
//! A `PcdMap` transforms 2-dimensional positions to correct for the
//! radial distortion introduced by some cameras and telescopes.  The
//! forward transformation applies the distortion:
//!
//!   RD = R * (1 + C * R^2)
//!
//! where R is the undistorted radial distance from the distortion
//! centre, RD is the distorted radial distance, and C is the
//! distortion coefficient.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, NodeValue, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::perm::PermMap;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// A 2-D pincushion/barrel distortion mapping.
///
/// The forward transformation applies radial distortion characterised
/// by a single coefficient `disc`:
///
/// ```text
/// dx = x - cen[0]
/// dy = y - cen[1]
/// f = 1 + disc * (dx^2 + dy^2)
/// x_out = dx * f + cen[0]
/// y_out = dy * f + cen[1]
/// ```
///
/// Positive `disc` gives pincushion distortion; negative gives barrel
/// distortion; zero gives no distortion (identity).
///
/// The inverse is an approximate iterative solution (from SLA_UNPCD)
/// that is accurate to negligible levels for astrometric purposes.
#[derive(Debug, Clone)]
pub struct PcdMap {
    /// The distortion coefficient as C's getter reports it, 0.0 in place of the
    /// `AST__BAD` unset sentinel (`pcdmap.c:2584-2585`).
    disc: f64,
    /// Whether `Disco` holds a supplied value — C's `TestDisco`
    /// (`pcdmap.c:2587`), which the dump writes as the card's set flag.
    disc_set: bool,
    /// The distortion centre as C's getter reports it, 0.0 per axis in place of
    /// the `AST__BAD` unset sentinel (`pcdmap.c:2638-2639`).
    pcdcen: [f64; 2],
    /// Whether each `PcdCen` axis holds a supplied value — C's `TestPcdCen`
    /// (`pcdmap.c:2637`).
    pcdcen_set: [bool; 2],
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl PcdMap {
    /// Create a new `PcdMap` with the given distortion coefficient and centre.
    ///
    /// `disc` is the distortion coefficient (positive = pincushion,
    /// negative = barrel, zero = no distortion).
    /// `pcdcen` is the `[x, y]` centre of the distortion.
    /// `AST__BAD` for either leaves that attribute unset, as C's initialiser
    /// stores it raw and its getter then reports the default
    /// (`pcdmap.c:96-98`).
    pub fn new(disc: f64, pcdcen: [f64; 2]) -> Self {
        let disc_set = disc != BAD_VALUE;
        let pcdcen_set = [pcdcen[0] != BAD_VALUE, pcdcen[1] != BAD_VALUE];
        Self {
            disc: if disc_set { disc } else { 0.0 },
            disc_set,
            pcdcen: [
                if pcdcen_set[0] { pcdcen[0] } else { 0.0 },
                if pcdcen_set[1] { pcdcen[1] } else { 0.0 },
            ],
            pcdcen_set,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// The distortion coefficient.
    pub fn disc(&self) -> f64 {
        self.disc
    }

    /// The value C keeps in the `disco` component, which is the raw value
    /// including the `AST__BAD` unset sentinel (`pcdmap.c:2586`). The port stores
    /// what the getter reports plus a set flag, so the guards below has to
    /// rebuild it.
    fn stored_disc(&self) -> f64 {
        if self.disc_set { self.disc } else { BAD_VALUE }
    }

    /// [`stored_disc`](Self::stored_disc) for one axis of `PcdCen`
    /// (`pcdmap.c:506`). `i` is 0-based.
    fn stored_pcdcen(&self, i: usize) -> f64 {
        if self.pcdcen_set[i] {
            self.pcdcen[i]
        } else {
            BAD_VALUE
        }
    }

    /// Set the distortion coefficient — C's `astSetDisco`.
    ///
    /// A zero coefficient is what PcdMap's own `MapMerge` tests for when it
    /// replaces the PcdMap with a `UnitMap`, in the section C marks "without
    /// reference to the neighbouring Mappings in the list"
    /// (`pcdmap.c:1418-1422`), so a conclusion an earlier simplification reached
    /// no longer holds. Both simplified records are discarded (`pcdmap.c:2586`,
    /// the convention at `mapping.h:449-472`).
    ///
    /// C stores the value raw and reads it back through a getter that reports
    /// 0.0 in place of the `AST__BAD` unset sentinel (`pcdmap.c:2584-2585`).
    /// The port stores what that getter would report and carries the sentinel
    /// test as a flag, so passing `BAD_VALUE` here restores the default and
    /// leaves the attribute untested, exactly as `astSetDisco( map, AST__BAD )`
    /// does.
    pub fn set_disc(&mut self, disc: f64) {
        if disc != self.stored_disc() {
            self.simplified.clear();
        }
        self.disc_set = disc != BAD_VALUE;
        self.disc = if self.disc_set { disc } else { 0.0 };
    }

    /// Set the distortion centre on one axis — C's `astSetPcdCen`.
    ///
    /// `axis` is 1-based. An index outside 1..=2 is rejected with
    /// `ErrorCode::Axiin` and changes nothing, as C's class-local `MAKE_SET`
    /// does (`pcdmap.c:490-494`); that macro then discards both simplified
    /// records before storing (`pcdmap.c:506-507`, the convention at
    /// `mapping.h:449-472`).
    ///
    /// `BAD_VALUE` restores the default and leaves the axis untested, as the
    /// sentinel does in C (`pcdmap.c:2637-2639`).
    pub fn set_pcd_cen(&mut self, axis: usize, value: f64) -> AstResult<()> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {axis} is out of range 1..=2"),
            ));
        }
        let i = axis - 1;
        if value != self.stored_pcdcen(i) {
            self.simplified.clear();
        }
        self.pcdcen_set[i] = value != BAD_VALUE;
        self.pcdcen[i] = if self.pcdcen_set[i] { value } else { 0.0 };
        Ok(())
    }

    /// The distortion centre.
    pub fn pcdcen(&self) -> [f64; 2] {
        self.pcdcen
    }

    /// Whether `Disco` is set (pcdmap.c `TestDisco`).
    pub fn test_disc(&self) -> bool {
        self.disc_set
    }

    /// Unset `Disco`, so that it reads as 0.0 (pcdmap.c `ClearDisco`, which
    /// also discards both simplified records).
    pub fn clear_disc(&mut self) {
        self.simplified.clear();
        self.disc_set = false;
        self.disc = 0.0;
    }

    /// Whether `PcdCen` is set on `axis` (1-based), as C's `astTestPcdCen`
    /// reports; an index outside 1..=2 is `ErrorCode::Axiin`.
    pub fn test_pcd_cen(&self, axis: usize) -> AstResult<bool> {
        Ok(self.pcdcen_set[Self::pcd_cen_index(axis, "astTestPcdCen")?])
    }

    /// Unset `PcdCen` on `axis` (1-based), so that it reads as 0.0, as C's
    /// `astClearPcdCen` does, discarding both simplified records; an index
    /// outside 1..=2 is `ErrorCode::Axiin` and changes nothing.
    pub fn clear_pcd_cen(&mut self, axis: usize) -> AstResult<()> {
        let i = Self::pcd_cen_index(axis, "astClearPcdCen")?;
        self.simplified.clear();
        self.pcdcen_set[i] = false;
        self.pcdcen[i] = 0.0;
        Ok(())
    }

    /// The 0-based index of a 1-based `PcdCen` axis, or C's AST__AXIIN.
    fn pcd_cen_index(axis: usize, method: &str) -> AstResult<usize> {
        if (1..=2).contains(&axis) {
            Ok(axis - 1)
        } else {
            Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "{method}(PcdMap): Index ({axis}) is invalid for attribute PcdCen - it \
                     should be in the range 1 to 2."
                ),
            ))
        }
    }

    /// Get the effective distortion coefficient, accounting for inversion state.
    ///
    /// When the PcdMap is inverted, the disc value is still the same
    /// (the forward/inverse roles of the transform are swapped, not
    /// the coefficient itself).
    fn effective_disc(&self) -> f64 {
        self.disc
    }

    /// Get the effective centre, accounting for inversion state.
    fn effective_pcdcen(&self) -> [f64; 2] {
        self.pcdcen
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }

    /// Check if a PermMap simply swaps axes 0 and 1 (both directions).
    pub(crate) fn perm_is_swap(pm: &PermMap) -> bool {
        let outperm = pm.outperm();
        let inperm = pm.inperm();
        if outperm.len() != 2 || inperm.len() != 2 {
            return false;
        }
        outperm[0] == 2 && outperm[1] == 1 && inperm[0] == 2 && inperm[1] == 1
    }
}

impl Mapping for PcdMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. Pure arithmetic (polynomial in dx,dy for
    /// forward; rational function for inverse); NaN propagates through
    /// both branches without per-point gating.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let disco = self.effective_disc();
        let cen0 = self.effective_pcdcen()[0];
        let cen1 = self.effective_pcdcen()[1];

        if fwd {
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                let dx = inp[0] - cen0;
                let dy = inp[1] - cen1;
                let f = 1.0 + disco * (dx * dx + dy * dy);
                outp[0] = dx * f + cen0;
                outp[1] = dy * f + cen1;
            }
        } else {
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                let dxp = inp[0] - cen0;
                let dyp = inp[1] - cen1;
                let cr2 = disco * (dxp * dxp + dyp * dyp);
                let a = (2.0 * cr2 + 1.0) / (3.0 * cr2 + 1.0);
                let cr2a2 = cr2 * a * a;
                let f = (2.0 * cr2a2 * a + 1.0) / (3.0 * cr2a2 + 1.0);
                outp[0] = dxp * f + cen0;
                outp[1] = dyp * f + cen1;
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let fwd = self.effective_forward(forward);
        let disco = self.effective_disc();
        let cen0 = self.effective_pcdcen()[0];
        let cen1 = self.effective_pcdcen()[1];

        if fwd {
            // Forward: apply distortion
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    let dx = inp[0] - cen0;
                    let dy = inp[1] - cen1;
                    let f = 1.0 + disco * (dx * dx + dy * dy);
                    outp[0] = dx * f + cen0;
                    outp[1] = dy * f + cen1;
                }
            }
        } else {
            // Inverse: remove distortion (SLA_UNPCD approximation)
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    let dxp = inp[0] - cen0;
                    let dyp = inp[1] - cen1;
                    let cr2 = disco * (dxp * dxp + dyp * dyp);
                    let a = (2.0 * cr2 + 1.0) / (3.0 * cr2 + 1.0);
                    let cr2a2 = cr2 * a * a;
                    let f = (2.0 * cr2a2 * a + 1.0) / (3.0 * cr2a2 + 1.0);
                    outp[0] = dxp * f + cen0;
                    outp[1] = dyp * f + cen1;
                }
            }
        }
        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PcdMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        ast_equal(self.disc, other.disc)
            && ast_equal(self.pcdcen[0], other.pcdcen[0])
            && ast_equal(self.pcdcen[1], other.pcdcen[1])
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::simplify::pcd_map_merge(self, ctx, where_, series)
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

        // Case 1: PcdMap + PcdMap (inverse cancellation)
        if let Some(other_pcd) = other.as_any().downcast_ref::<PcdMap>() {
            // They cancel if they have the same disc and centre but opposite
            // effective inversion states.
            if ast_equal(self.disc, other_pcd.disc)
                && ast_equal(self.pcdcen[0], other_pcd.pcdcen[0])
                && ast_equal(self.pcdcen[1], other_pcd.pcdcen[1])
                && self_inv != other_inv
            {
                return Some(Box::new(UnitMap::new(2)));
            }
            return None;
        }

        // Case 2: PcdMap + UnitMap → PcdMap unchanged
        if other.as_any().downcast_ref::<UnitMap>().is_some() {
            let mut result = self.clone();
            // Normalize: apply the effective inversion state
            result.inverted = self_inv;
            return Some(Box::new(result));
        }

        None
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

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for PcdMap {
    fn class_name(&self) -> &'static str {
        "PcdMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Apply pincushion distortion".to_string());
        write_mapping_base(&mut node, self);
        // C writes all three cards, set or not, and an unset card carries the
        // default the getter reports for information only (pcdmap.c:2705-2721).
        let comments = [
            "Distortion centre on first axis",
            "Distortion centre on second axis",
        ];
        for (i, comment) in comments.iter().enumerate() {
            let key = format!("PcdCn{i}");
            if self.pcdcen_set[i] {
                node.push_double(&key, self.pcdcen[i], Some((*comment).to_string()));
            } else {
                node.push_unset(
                    &key,
                    NodeValue::Double(self.pcdcen[i]),
                    Some((*comment).to_string()),
                );
            }
        }
        let disco_comment = Some("Distortion coefficient".to_string());
        if self.disc_set {
            node.push_double("Disco", self.disc, disco_comment);
        } else {
            node.push_unset("Disco", NodeValue::Double(self.disc), disco_comment);
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("PcdMap");
        let base = read_mapping_base(whole);
        let disc = node.get_double("Disco", BAD_VALUE);
        let pcdcen0 = node.get_double("PcdCn0", BAD_VALUE);
        let pcdcen1 = node.get_double("PcdCn1", BAD_VALUE);
        let mut m = PcdMap::new(disc, [pcdcen0, pcdcen1]);
        if base.inverted {
            m.invert();
        }
        m.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        // C assigns the structure component and only then re-applies it through
        // the setter, so the set changes nothing and the setter's guarded
        // `astClearIsSimple` does not fire (pcdmap.c:3192-3205): `PcdMap::new` above
        // was already given the values read from the cards.
        if node.get("PcdCn0").is_some() {
            m.set_pcd_cen(1, pcdcen0)?;
        }
        if node.get("PcdCn1").is_some() {
            m.set_pcd_cen(2, pcdcen1)?;
        }
        if node.get("Disco").is_some() {
            m.set_disc(disc);
        }
        Ok(m)
    }
}
