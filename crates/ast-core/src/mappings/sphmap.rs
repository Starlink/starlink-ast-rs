//! Cartesian-to-spherical coordinate mapping.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::AstResult;
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad, point_has_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// A mapping that converts between 3-D Cartesian coordinates and
/// 2-D spherical coordinates (longitude, latitude).
///
/// The forward transformation converts Cartesian `(x, y, z)` to
/// spherical `(longitude, latitude)` in radians.  The inverse
/// transformation converts `(longitude, latitude)` back to a unit
/// Cartesian vector `(x, y, z)`.
///
/// # Attributes
///
/// - **`unit_radius`** (default `false`): if `true`, input Cartesian
///   vectors are assumed to already have unit length, enabling
///   simplification of a forward-then-inverse `CmpMap` pair into a
///   `UnitMap`.
///
/// - **`polar_long`** (default `0.0`): the longitude value returned
///   when the input Cartesian vector lies exactly at either pole
///   (i.e. `x == 0` and `y == 0`).
#[derive(Debug, Clone)]
pub struct SphMap {
    /// `None` while unset (C's -1), reading as false.
    unit_radius: Option<bool>,
    polar_long: f64,
    /// Tracks whether `polar_long` was explicitly set by the user (or read
    /// from a serialised representation with `set=1`). Mirrors the C
    /// library's `TestPolarLong` flag so byte-exact round-trip distinguishes
    /// "default value 0.0 (commented in dump)" from "explicitly set to 0.0
    /// (uncommented in dump)".
    polar_long_set: bool,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl SphMap {
    /// Create a new `SphMap` with default attributes.
    ///
    /// Default: `unit_radius = false`, `polar_long = 0.0`.
    pub fn new() -> Self {
        Self {
            unit_radius: None,
            polar_long: 0.0,
            polar_long_set: false,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// Whether input Cartesian vectors are assumed to have unit length.
    pub fn unit_radius(&self) -> bool {
        self.unit_radius.unwrap_or(false)
    }

    /// Set whether input Cartesian vectors are assumed to have unit length.
    pub fn set_unit_radius(&mut self, unit_radius: bool) {
        // `UnitRadius` is read by SphMap's own MapMerge, which cancels an
        // inverse-then-forward pair only when the first SphMap has it set
        // (sphmap.c:771), so a record left in place could decline the
        // cancellation the new value allows (sphmap.c:1394-1396,
        // `astClearIsSimple` at mapping.h:449-472).
        if Some(unit_radius) != self.unit_radius {
            self.simplified.clear();
        }
        self.unit_radius = Some(unit_radius);
    }

    /// Whether UnitRadius is set, `false` included (sphmap.c
    /// `TestUnitRadius`).
    pub fn test_unit_radius(&self) -> bool {
        self.unit_radius.is_some()
    }

    /// Unset UnitRadius, so that it reads as `false` (sphmap.c
    /// `ClearUnitRadius`, which also discards both simplified records).
    pub fn clear_unit_radius(&mut self) {
        self.simplified.clear();
        self.unit_radius = None;
    }

    /// The longitude value returned at either pole.
    pub fn polar_long(&self) -> f64 {
        self.polar_long
    }

    /// The value C keeps in the `polarlong` component, which is the raw value
    /// including the `AST__BAD` unset sentinel (`sphmap.c:1436`). The port stores
    /// what the getter reports plus a set flag, so the guard on
    /// [`set_polar_long`](Self::set_polar_long) has to rebuild it.
    fn stored_polar_long(&self) -> f64 {
        if self.polar_long_set {
            self.polar_long
        } else {
            BAD_VALUE
        }
    }

    /// Set the longitude value returned at either pole (radians).
    /// `BAD_VALUE` is C's unset value, so it unsets the attribute, which then
    /// reads as 0.0 (sphmap.c:1437-1441).
    pub fn set_polar_long(&mut self, polar_long: f64) {
        // `PolarLong` is read by the same MapMerge, which requires the pair
        // to agree on it (sphmap.c:762-763, :932); the record goes for the same
        // reason (sphmap.c:1434-1436).
        if polar_long != self.stored_polar_long() {
            self.simplified.clear();
        }
        self.polar_long_set = polar_long != BAD_VALUE;
        self.polar_long = if self.polar_long_set { polar_long } else { 0.0 };
    }

    /// Whether PolarLong is set (sphmap.c `TestPolarLong`).
    pub fn test_polar_long(&self) -> bool {
        self.polar_long_set
    }

    /// Unset PolarLong, so that it reads as 0.0 (sphmap.c `ClearPolarLong`,
    /// which also discards both simplified records).
    pub fn clear_polar_long(&mut self) {
        self.simplified.clear();
        self.polar_long_set = false;
        self.polar_long = 0.0;
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
}

impl Default for SphMap {
    fn default() -> Self {
        Self::new()
    }
}

impl Mapping for SphMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        if self.inverted { 2 } else { 3 }
    }

    fn nout(&self) -> usize {
        if self.inverted { 3 } else { 2 }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    /// NaN-as-bad fast path. Drops the per-point `point_has_bad` gates.
    /// The forward pole-detection branch (`|x| < mxerr && |y| < mxerr`)
    /// uses ordered comparisons against `mxerr`, which is `NaN` when any
    /// of x/y/z is NaN — every such comparison is false, so NaN inputs
    /// fall through to `pal::dcc2s` which returns `(NaN, NaN)`
    /// (atan2 / sqrt propagate NaN cleanly). Inverse direction just calls
    /// `pal::dcs2c` directly; same story.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        if fwd {
            let polarlong = self.polar_long;
            let pin = pts.axis_slices();
            let pout = out.axes_mut();
            for i in 0..pts.npoint() {
                let x = pin[0][i];
                let y = pin[1][i];
                let z = pin[2][i];
                let mxerr = (1000.0 * z).abs() * f64::EPSILON;
                if x.abs() < mxerr && y.abs() < mxerr {
                    // Pole branch: only entered when x,y,z are all finite
                    // (NaN comparisons against mxerr are false).
                    if z < 0.0 {
                        pout[0][i] = polarlong;
                        pout[1][i] = -std::f64::consts::FRAC_PI_2;
                    } else if z > 0.0 {
                        pout[0][i] = polarlong;
                        pout[1][i] = std::f64::consts::FRAC_PI_2;
                    } else {
                        pout[0][i] = f64::NAN;
                        pout[1][i] = f64::NAN;
                    }
                } else {
                    let v = [x, y, z];
                    let (lon, lat) = pal::dcc2s(&v);
                    pout[0][i] = lon;
                    pout[1][i] = lat;
                }
            }
        } else {
            let pin = pts.axis_slices();
            let pout = out.axes_mut();
            for i in 0..pts.npoint() {
                let v = pal::dcs2c(pin[0][i], pin[1][i]);
                pout[0][i] = v[0];
                pout[1][i] = v[1];
                pout[2][i] = v[2];
            }
        }
        Ok(())
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform(self, &pts, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);

        if fwd {
            // Forward: Cartesian (x, y, z) -> Spherical (lon, lat)
            let polarlong = self.polar_long;
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let mut outp = out.point_mut(i);
                if point_has_bad(&inp) {
                    outp.fill(BAD_VALUE);
                } else {
                    let x = inp[0];
                    let y = inp[1];
                    let z = inp[2];

                    // Check for pole: x and y are effectively zero
                    // relative to z.  Use the same criterion as the C
                    // library: |x| < mxerr && |y| < mxerr where
                    // mxerr = |1000 * z| * DBL_EPSILON.
                    // When z is also zero, mxerr is 0 and `0 < 0` is
                    // false, so the zero-vector falls through to
                    // palDcc2s (which returns (0, 0)).
                    let mxerr = (1000.0 * z).abs() * f64::EPSILON;
                    if x.abs() < mxerr && y.abs() < mxerr {
                        if z < 0.0 {
                            outp[0] = polarlong;
                            outp[1] = -std::f64::consts::FRAC_PI_2;
                        } else if z > 0.0 {
                            outp[0] = polarlong;
                            outp[1] = std::f64::consts::FRAC_PI_2;
                        } else {
                            // z == 0.0 (all three zero) — should not
                            // be reachable since mxerr would be 0, but
                            // kept for safety to match C behavior.
                            outp[0] = BAD_VALUE;
                            outp[1] = BAD_VALUE;
                        }
                    } else {
                        let v = [x, y, z];
                        let (lon, lat) = pal::dcc2s(&v);
                        outp[0] = lon;
                        outp[1] = lat;
                    }
                }
            }
        } else {
            // Inverse: Spherical (lon, lat) -> Cartesian (x, y, z)
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let mut outp = out.point_mut(i);
                if point_has_bad(&inp) {
                    outp.fill(BAD_VALUE);
                } else {
                    let lon = inp[0];
                    let lat = inp[1];
                    let v = pal::dcs2c(lon, lat);
                    outp[0] = v[0];
                    outp[1] = v[1];
                    outp[2] = v[2];
                }
            }
        }
        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::simplify::sph_map_merge(self, ctx, where_, series)
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
        // Only handle inverse cancellation: opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_sph = other.as_any().downcast_ref::<SphMap>()?;

        // Determine which direction each map is applied in.
        // SphMap forward: 3D Cartesian → 2D spherical (cart→sph), nin=3, nout=2
        // SphMap inverse: 2D spherical → 3D Cartesian (sph→cart), nin=2, nout=3
        //
        // self_inv == true means self is applied in its INVERSE direction (sph→cart)
        // self_inv == false means self is applied FORWARD (cart→sph)
        //
        // Cancellation rules — mirror C sphmap.c MapMerge (lines 753-768):
        //
        // - Inverse→forward (sph→cart then cart→sph): cancels only if
        //   PolarLong is equal on both — the spherical interpretation
        //   on each side must match for the round-trip to be identity.
        //   Result: 2-axis UnitMap (input side).
        //
        // - Forward→inverse (cart→sph then sph→cart): cancels only if
        //   UnitRadius is set on the *forward* map. The intermediate
        //   cart→sph drops magnitude; round-trip is identity in
        //   3-vector space iff the input was already on the unit sphere.
        //   No PolarLong check is required: the cartesian round-trip
        //   ignores the spherical-coord interpretation. Result: 3-axis
        //   UnitMap (input side).
        let nin_pair = if self_inv {
            // self is inverse (sph→cart), other is forward (cart→sph).
            // C path: invert_list[imap1]==1 && invert_list[imap2]==0.
            // Requires PolarLong equality.
            if !ast_equal(self.polar_long, other_sph.polar_long) {
                return None;
            }
            2
        } else {
            // self is forward (cart→sph), other is inverse (sph→cart).
            // C path: invert_list[imap1]==0 && invert_list[imap2]==1.
            // Requires UnitRadius on the forward (self) map. C does
            // NOT check PolarLong here.
            if !self.unit_radius() {
                return None;
            }
            3
        };
        Some(Box::new(UnitMap::new(nin_pair)))
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SphMap>() else {
            return false;
        };
        // sphmap.c Equal compares the stored values, so an unset attribute
        // differs from one set to its default.
        self.inverted == other.inverted
            && self.unit_radius == other.unit_radius
            && ast_equal(self.stored_polar_long(), other.stored_polar_long())
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
}

impl Serializable for SphMap {
    fn class_name(&self) -> &'static str {
        "SphMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Cartesian to Spherical mapping".to_string());
        crate::serial::write_mapping_base(&mut node, self);
        let unit_radius = self.unit_radius.unwrap_or(false);
        node.write_int(
            "UntRd",
            self.unit_radius.is_some(),
            false,
            i64::from(unit_radius),
            if unit_radius {
                "All input vectors have unit length"
            } else {
                "Input vectors do not all have unit length"
            },
        );
        node.write_double(
            "PlrLg",
            self.polar_long_set,
            true,
            self.polar_long,
            "Polar longitude (rad.s)",
        );
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("SphMap");
        use crate::serial::NodeValue;
        let base = read_mapping_base(whole);
        let unit_radius = node.get("UntRd").map(|_| node.get_int("UntRd", 0) != 0);
        // Distinguish "uncommented PlrLg = …" (explicitly set in source) from
        // "#   PlrLg = …" (commented default) — only the former counts as set.
        // `get_any` unwraps `Unset(inner)`, so we have to walk `items` directly.
        let polar_long_set = node
            .items
            .iter()
            .find(|i| i.key == "PlrLg")
            .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)));
        let polar_long_raw = node.get_double("PlrLg", f64::MIN);
        let polar_long = if is_bad(polar_long_raw) {
            0.0
        } else {
            polar_long_raw
        };
        let mut m = SphMap {
            unit_radius,
            polar_long,
            polar_long_set,
            inverted: false,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        };
        if base.inverted {
            m.invert();
        }
        // C assigns the structure component and only then re-applies it through
        // the setter, so the set changes nothing and the setter's guarded
        // `astClearIsSimple` does not fire (sphmap.c:2040-2046): the struct literal
        // above already holds the values read from the cards.
        if let Some(unit_radius) = unit_radius {
            m.set_unit_radius(unit_radius);
        }
        if polar_long_set {
            m.set_polar_long(polar_long);
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use std::f64::consts::{FRAC_PI_2, PI};

    const TOL: f64 = 1.0e-10;

    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!(
            (a - b).abs() < tol,
            "expected {a} ~= {b} (diff = {})",
            (a - b).abs()
        );
    }

    #[test]
    fn forward_x_axis() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[1.0, 0.0, 0.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], 0.0, TOL); // lon
        assert_close(out.point(0)[1], 0.0, TOL); // lat
    }

    #[test]
    fn forward_y_axis() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[0.0, 1.0, 0.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], FRAC_PI_2, TOL); // lon = pi/2
        assert_close(out.point(0)[1], 0.0, TOL); // lat = 0
    }

    #[test]
    fn forward_north_pole() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[0.0, 0.0, 1.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], 0.0, TOL); // polar_long default = 0
        assert_close(out.point(0)[1], FRAC_PI_2, TOL);
    }

    #[test]
    fn forward_south_pole() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[0.0, 0.0, -1.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], 0.0, TOL);
        assert_close(out.point(0)[1], -FRAC_PI_2, TOL);
    }

    #[test]
    fn forward_pole_custom_polar_long() {
        let mut m = SphMap::new();
        m.set_polar_long(1.5);
        let pts = PointSet::from_rows([[0.0, 0.0, 1.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], 1.5, TOL);
        assert_close(out.point(0)[1], FRAC_PI_2, TOL);
    }

    #[test]
    fn inverse_origin() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[0.0, 0.0]]);
        let out = m.transform(&pts, false).unwrap();
        assert_close(out.point(0)[0], 1.0, TOL);
        assert_close(out.point(0)[1], 0.0, TOL);
        assert_close(out.point(0)[2], 0.0, TOL);
    }

    #[test]
    fn inverse_pi_half() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[FRAC_PI_2, 0.0]]);
        let out = m.transform(&pts, false).unwrap();
        assert_close(out.point(0)[0], 0.0, TOL);
        assert_close(out.point(0)[1], 1.0, TOL);
        assert_close(out.point(0)[2], 0.0, TOL);
    }

    #[test]
    fn round_trip_forward_inverse() {
        let m = SphMap::new();
        // Forward: unit vector -> spherical
        let pts_cart = PointSet::from_rows([[0.6, 0.8, 0.0]]);
        let sph = m.transform(&pts_cart, true).unwrap();
        // Inverse: spherical -> unit vector
        let cart_back = m.transform(&sph, false).unwrap();
        assert_close(cart_back.point(0)[0], 0.6, TOL);
        assert_close(cart_back.point(0)[1], 0.8, TOL);
        assert_close(cart_back.point(0)[2], 0.0, TOL);
    }

    #[test]
    fn non_unit_input_normalized() {
        // (2, 0, 0) should map to (0, 0) — same direction as (1, 0, 0)
        let m = SphMap::new();
        let pts = PointSet::from_rows([[2.0, 0.0, 0.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], 0.0, TOL);
        assert_close(out.point(0)[1], 0.0, TOL);
    }

    #[test]
    fn bad_propagation_forward() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[BAD_VALUE, 1.0, 0.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_eq!(out.point(0)[0], BAD_VALUE);
        assert_eq!(out.point(0)[1], BAD_VALUE);
    }

    #[test]
    fn bad_propagation_inverse() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[0.5, BAD_VALUE]]);
        let out = m.transform(&pts, false).unwrap();
        assert_eq!(out.point(0)[0], BAD_VALUE);
        assert_eq!(out.point(0)[1], BAD_VALUE);
        assert_eq!(out.point(0)[2], BAD_VALUE);
    }

    #[test]
    fn dimensions_not_inverted() {
        let m = SphMap::new();
        assert_eq!(m.nin(), 3);
        assert_eq!(m.nout(), 2);
        assert!(!m.is_inverted());
    }

    #[test]
    fn dimensions_inverted() {
        let mut m = SphMap::new();
        m.invert();
        assert_eq!(m.nin(), 2);
        assert_eq!(m.nout(), 3);
        assert!(m.is_inverted());
    }

    #[test]
    fn unit_radius_attribute() {
        let mut m = SphMap::new();
        assert!(!m.unit_radius());
        m.set_unit_radius(true);
        assert!(m.unit_radius());
    }

    #[test]
    fn forward_negative_x() {
        let m = SphMap::new();
        let pts = PointSet::from_rows([[-1.0, 0.0, 0.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], PI, TOL);
        assert_close(out.point(0)[1], 0.0, TOL);
    }

    #[test]
    fn zero_vector_returns_origin() {
        // A zero vector is not at a pole (z is also 0), so it falls
        // through to palDcc2s which returns (0, 0).
        let m = SphMap::new();
        let pts = PointSet::from_rows([[0.0, 0.0, 0.0]]);
        let out = m.transform(&pts, true).unwrap();
        assert_close(out.point(0)[0], 0.0, TOL);
        assert_close(out.point(0)[1], 0.0, TOL);
    }
}
