//! Cartesian-to-spherical coordinate mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad, point_has_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

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
    unit_radius: bool,
    polar_long: f64,
    inverted: bool,
}

impl SphMap {
    /// Create a new `SphMap` with default attributes.
    ///
    /// Default: `unit_radius = false`, `polar_long = 0.0`.
    pub fn new() -> Self {
        Self {
            unit_radius: false,
            polar_long: 0.0,
            inverted: false,
        }
    }

    /// Whether input Cartesian vectors are assumed to have unit length.
    pub fn unit_radius(&self) -> bool {
        self.unit_radius
    }

    /// Set whether input Cartesian vectors are assumed to have unit length.
    pub fn set_unit_radius(&mut self, unit_radius: bool) {
        self.unit_radius = unit_radius;
    }

    /// The longitude value returned at either pole.
    pub fn polar_long(&self) -> f64 {
        self.polar_long
    }

    /// Set the longitude value returned at either pole (radians).
    pub fn set_polar_long(&mut self, polar_long: f64) {
        self.polar_long = polar_long;
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
    fn nin(&self) -> usize {
        if self.inverted { 2 } else { 3 }
    }

    fn nout(&self) -> usize {
        if self.inverted { 3 } else { 2 }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        let ncoord_out = if fwd { 2 } else { 3 };
        let mut out = PointSet::zeros(pts.npoint(), ncoord_out);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if fwd {
            // Forward: Cartesian (x, y, z) -> Spherical (lon, lat)
            let polarlong = self.polar_long;
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if point_has_bad(inp) {
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
                let outp = out.point_mut(i);
                if point_has_bad(inp) {
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

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
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
        if !ast_equal(self.polar_long, other_sph.polar_long) {
            return None;
        }

        // Determine which direction each map is applied in.
        // SphMap forward: 3D Cartesian → 2D spherical (cart→sph), nin=3, nout=2
        // SphMap inverse: 2D spherical → 3D Cartesian (sph→cart), nin=2, nout=3
        //
        // self_inv == true means self is applied in its INVERSE direction (sph→cart)
        // self_inv == false means self is applied FORWARD (cart→sph)
        //
        // Cancellation rules (matching C sphmap.c:748-761):
        //
        // - Inverse→forward (sph→cart then cart→sph): always cancels.
        //   palDcs2c always produces unit vectors, so the intermediate
        //   3D vector is guaranteed to lie on the unit sphere.
        //
        // - Forward→inverse (cart→sph then sph→cart): cancels only if
        //   UnitRadius is set on the forward map. Without it, magnitude
        //   information is lost in the forward step and the round-trip
        //   is not identity for non-unit inputs.
        if !self_inv {
            // Self is forward (cart→sph), other is inverse (sph→cart).
            // Pattern: forward→inverse needs UnitRadius on the forward map.
            if !self.unit_radius {
                return None;
            }
        }
        // If self is inverse (sph→cart) and other is forward (cart→sph):
        // No UnitRadius check needed — palDcs2c always produces unit vectors,
        // so the intermediate 3D vector is guaranteed on the unit sphere.

        // UnitMap dimension: the "input" side of the cancelling pair.
        // If self is inverse (sph->cart, input is 2D), pair input is 2D.
        // If self is forward (cart->sph, input is 3D), pair input is 3D.
        let nin_pair = if self_inv { 2 } else { 3 };
        Some(Box::new(UnitMap::new(nin_pair)))
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SphMap>() else {
            return false;
        };
        self.inverted == other.inverted
            && self.unit_radius == other.unit_radius
            && ast_equal(self.polar_long, other.polar_long)
    }
}

impl Serializable for SphMap {
    fn class_name(&self) -> &'static str {
        "SphMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Cartesian to Spherical mapping".to_string());
        write_mapping_base(&mut node, self);
        if self.unit_radius {
            node.push_int(
                "UntRd",
                1,
                Some("All input vectors have unit length".to_string()),
            );
        }
        if self.polar_long != 0.0 {
            node.push_double(
                "PlrLg",
                self.polar_long,
                Some("Polar longitude (rad.s)".to_string()),
            );
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (_nin, _nout, inverted) = read_mapping_base(node);
        let unit_radius = node.get_int("UntRd", 0) != 0;
        let polar_long_raw = node.get_double("PlrLg", f64::MIN);
        let polar_long = if is_bad(polar_long_raw) {
            0.0
        } else {
            polar_long_raw
        };
        let mut m = SphMap {
            unit_radius,
            polar_long,
            inverted: false,
        };
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
