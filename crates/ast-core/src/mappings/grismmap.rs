//! 1-D spectral dispersion mapping (grism/grating/prism).
//!
//! A `GrismMap` transforms 1-dimensional coordinates using the spectral
//! dispersion equation described in FITS-WCS paper III "Representation of
//! spectral coordinates in FITS".  This describes the dispersion produced
//! by gratings, prisms and grisms.
//!
//! When initially created, the forward transformation of a GrismMap
//! transforms input "grism parameter" values into output wavelength
//! values.  The "grism parameter" is a dimensionless value linearly
//! related to position on the detector, defined as the offset from the
//! camera axis intersection measured in units of the effective focal length.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A 1-D spectral dispersion mapping for grisms, gratings and prisms.
///
/// The forward transformation converts the "grism parameter" (a
/// dimensionless detector position offset) into wavelength.  The inverse
/// converts wavelength back to the grism parameter.
///
/// The physical model and equations are from FITS-WCS Paper III.
#[derive(Debug, Clone)]
pub struct GrismMap {
    /// Refractive index at reference wavelength (default 1.0).
    grism_nr: f64,
    /// Rate of change of refractive index with wavelength (default 0.0).
    grism_nrp: f64,
    /// Reference wavelength (default 5000.0).
    grism_wave_r: f64,
    /// Angle of incidence on the grating, radians (default 0.0).
    grism_alpha: f64,
    /// Grating ruling density (default 0.0).
    grism_g: f64,
    /// Diffraction order (default 0).
    grism_m: i32,
    /// Angle between grating normal and dispersion plane, radians (default 0.0).
    grism_eps: f64,
    /// Angle between detector normal and reference ray, radians (default 0.0).
    grism_theta: f64,
    /// Derived constant k1.
    k1: f64,
    /// Derived constant k2.
    k2: f64,
    /// Derived constant k3.
    k3: f64,
    inverted: bool,
    do_not_simplify: bool,
}

impl GrismMap {
    /// Create a new `GrismMap` with the given physical parameters.
    ///
    /// All parameters use the default values from the C library:
    /// - `nr`: refractive index at reference wavelength (default 1.0)
    /// - `nrp`: dn/dlambda at reference wavelength (default 0.0)
    /// - `wave_r`: reference wavelength (default 5000.0)
    /// - `alpha`: angle of incidence on grating in radians (default 0.0)
    /// - `g`: grating ruling density (default 0.0)
    /// - `m`: diffraction order (default 0)
    /// - `eps`: angle between grating normal and dispersion plane in radians (default 0.0)
    /// - `theta`: angle between detector normal and reference ray in radians (default 0.0)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        nr: f64,
        nrp: f64,
        wave_r: f64,
        alpha: f64,
        g: f64,
        m: i32,
        eps: f64,
        theta: f64,
    ) -> Self {
        let mut map = Self {
            grism_nr: nr,
            grism_nrp: nrp,
            grism_wave_r: wave_r,
            grism_alpha: alpha,
            grism_g: g,
            grism_m: m,
            grism_eps: eps,
            grism_theta: theta,
            k1: 0.0,
            k2: 0.0,
            k3: 0.0,
            inverted: false,
            do_not_simplify: false,
        };
        map.update_constants();
        map
    }

    /// Create a `GrismMap` with all default attribute values.
    pub fn default_map() -> Self {
        Self::new(1.0, 0.0, 5000.0, 0.0, 0.0, 0, 0.0, 0.0)
    }

    /// The refractive index at the reference wavelength.
    pub fn grism_nr(&self) -> f64 {
        self.grism_nr
    }

    /// The rate of change of refractive index with wavelength.
    pub fn grism_nrp(&self) -> f64 {
        self.grism_nrp
    }

    /// The reference wavelength.
    pub fn grism_wave_r(&self) -> f64 {
        self.grism_wave_r
    }

    /// The angle of incidence of incoming light on the grating.
    pub fn grism_alpha(&self) -> f64 {
        self.grism_alpha
    }

    /// The grating ruling density.
    pub fn grism_g(&self) -> f64 {
        self.grism_g
    }

    /// The diffraction order.
    pub fn grism_m(&self) -> i32 {
        self.grism_m
    }

    /// The angle between grating normal and dispersion plane.
    pub fn grism_eps(&self) -> f64 {
        self.grism_eps
    }

    /// The angle between detector normal and reference ray.
    pub fn grism_theta(&self) -> f64 {
        self.grism_theta
    }

    /// Recalculate the derived constants k1, k2, k3.
    ///
    /// Called whenever any attribute changes.  Matches the C library's
    /// `UpdateConstants` function.
    fn update_constants(&mut self) {
        let nr = self.grism_nr;
        let nrp = self.grism_nrp;
        let wave_r = self.grism_wave_r;
        let alpha = self.grism_alpha;
        let g = self.grism_g;
        let m = self.grism_m as f64;
        let eps = self.grism_eps;
        let theta = self.grism_theta;

        let coseps = eps.cos();
        let sinalpha = alpha.sin();

        self.k1 = sinalpha * (nr - nrp * wave_r);

        if coseps != 0.0 {
            self.k2 = (g * m / coseps) - nrp * sinalpha;

            let sinbeta_r = g * m * wave_r / coseps - nr * sinalpha;
            if !(-1.0..=1.0).contains(&sinbeta_r) {
                self.k3 = BAD_VALUE;
            } else {
                self.k3 = sinbeta_r.asin() + theta;
            }
        } else {
            self.k2 = BAD_VALUE;
            self.k3 = BAD_VALUE;
        }
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

    /// Try to merge this GrismMap with a ZoomMap that follows it.
    ///
    /// The forward GrismMap produces wavelength output.  Multiplying
    /// wavelength by a zoom factor z is equivalent to adjusting the
    /// physical parameters:
    ///   - `g /= z` (ruling density scales inversely with wavelength unit)
    ///   - `nrp /= z` (dn/dlambda scales inversely)
    ///   - `wave_r *= z` (reference wavelength scales with wavelength unit)
    pub(crate) fn merge_with_zoom(
        &self,
        zoom_factor: f64,
        invert_result: bool,
    ) -> Option<DynMapping> {
        if zoom_factor == 0.0 {
            return None;
        }

        let new_g = self.grism_g / zoom_factor;
        let new_nrp = self.grism_nrp / zoom_factor;
        let new_wave_r = self.grism_wave_r * zoom_factor;

        let mut result = GrismMap::new(
            self.grism_nr,
            new_nrp,
            new_wave_r,
            self.grism_alpha,
            new_g,
            self.grism_m,
            self.grism_eps,
            self.grism_theta,
        );

        if invert_result {
            result.invert();
        }

        Some(Box::new(result))
    }
}

impl Mapping for GrismMap {
    fn nin(&self) -> usize {
        1
    }

    fn nout(&self) -> usize {
        1
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 1);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        // If any derived constants are BAD, all output is BAD.
        if is_bad(self.k1) || is_bad(self.k2) || is_bad(self.k3) {
            for i in 0..pts.npoint() {
                out.point_mut(i)[0] = BAD_VALUE;
            }
            return Ok(());
        }

        if fwd {
            // Forward: grism parameter → wavelength
            for i in 0..pts.npoint() {
                let value_in = pts.point(i)[0];
                if is_bad(value_in) || self.k2 == 0.0 {
                    out.point_mut(i)[0] = BAD_VALUE;
                } else {
                    out.point_mut(i)[0] = (self.k1 + (value_in.atan() + self.k3).sin()) / self.k2;
                }
            }
        } else {
            // Inverse: wavelength → grism parameter
            for i in 0..pts.npoint() {
                let value_in = pts.point(i)[0];
                if is_bad(value_in) {
                    out.point_mut(i)[0] = BAD_VALUE;
                } else {
                    let sinbeta = self.k2 * value_in - self.k1;
                    if !(-1.0..=1.0).contains(&sinbeta) {
                        out.point_mut(i)[0] = BAD_VALUE;
                    } else {
                        out.point_mut(i)[0] = (sinbeta.asin() - self.k3).tan();
                    }
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<GrismMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        ast_equal(self.grism_nr, other.grism_nr)
            && ast_equal(self.grism_nrp, other.grism_nrp)
            && ast_equal(self.grism_wave_r, other.grism_wave_r)
            && ast_equal(self.grism_alpha, other.grism_alpha)
            && ast_equal(self.grism_g, other.grism_g)
            && self.grism_m == other.grism_m
            && ast_equal(self.grism_eps, other.grism_eps)
            && ast_equal(self.grism_theta, other.grism_theta)
            && ast_equal(self.k1, other.k1)
            && ast_equal(self.k2, other.k2)
            && ast_equal(self.k3, other.k3)
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

        // Case 1: GrismMap + GrismMap (inverse cancellation)
        if let Some(other_grism) = other.as_any().downcast_ref::<GrismMap>() {
            // Check that all floating-point attributes match AND that M
            // values differ AND that directions are opposite.
            // This matches the C library's CanMerge logic exactly.
            if ast_equal(self.grism_nr, other_grism.grism_nr)
                && ast_equal(self.grism_nrp, other_grism.grism_nrp)
                && ast_equal(self.grism_wave_r, other_grism.grism_wave_r)
                && ast_equal(self.grism_alpha, other_grism.grism_alpha)
                && ast_equal(self.grism_g, other_grism.grism_g)
                && self.grism_m != other_grism.grism_m
                && ast_equal(self.grism_eps, other_grism.grism_eps)
                && ast_equal(self.grism_theta, other_grism.grism_theta)
                && self_inv != other_inv
            {
                return Some(Box::new(UnitMap::new(1)));
            }
            return None;
        }

        // GrismMap + ZoomMap merges are handled by the simplify engine's
        // special-case code (try_merge_grism_zoom) because the try_merge
        // API cannot distinguish forward from reverse calls, and the merge
        // is only valid in specific orderings.

        None
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

impl Serializable for GrismMap {
    fn class_name(&self) -> &'static str {
        "GrismMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        use crate::serial::NodeValue;

        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map 1-d coordinates using a spectral disperser".to_string());
        write_mapping_base(&mut node, self);

        // Helper: write a double attribute, using push_unset if at default.
        let write_attr = |node: &mut Node, key: &str, val: f64, default: f64, comment: &str| {
            if ast_equal(val, default) {
                node.push_unset(key, NodeValue::Double(val), Some(comment.to_string()));
            } else {
                node.push_double(key, val, Some(comment.to_string()));
            }
        };

        write_attr(
            &mut node,
            "GrmNR",
            self.grism_nr,
            1.0,
            "Refractive index at the ref. wavelength",
        );
        write_attr(
            &mut node,
            "GrmNRP",
            self.grism_nrp,
            0.0,
            "Rate of change of refractive index",
        );
        write_attr(
            &mut node,
            "GrmWR",
            self.grism_wave_r,
            5000.0,
            "Ref. wavelength",
        );
        write_attr(
            &mut node,
            "GrmAlp",
            self.grism_alpha,
            0.0,
            "Angle of incidence of incoming light",
        );
        write_attr(
            &mut node,
            "GrmG",
            self.grism_g,
            0.0,
            "Grating ruling density",
        );

        // GrmM — integer attribute, default 0
        let m_val = self.grism_m as f64;
        if self.grism_m == 0 {
            node.push_unset(
                "GrmM",
                NodeValue::Double(m_val),
                Some("The interference order".to_string()),
            );
        } else {
            node.push_double("GrmM", m_val, Some("The interference order".to_string()));
        }

        write_attr(
            &mut node,
            "GrmEps",
            self.grism_eps,
            0.0,
            "Angle between grating normal and dispersion plane",
        );
        write_attr(
            &mut node,
            "GrmTh",
            self.grism_theta,
            0.0,
            "Angle between detector normal and reference ray",
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        let nr = node.get_double("GrmNR", 1.0);
        let nrp = node.get_double("GrmNRP", 0.0);
        let wave_r = node.get_double("GrmWR", 5000.0);
        let alpha = node.get_double("GrmAlp", 0.0);
        let g = node.get_double("GrmG", 0.0);
        let m = node.get_double("GrmM", 0.0) as i32;
        let eps = node.get_double("GrmEps", 0.0);
        let theta = node.get_double("GrmTh", 0.0);

        let mut map = GrismMap::new(nr, nrp, wave_r, alpha, g, m, eps, theta);
        map.do_not_simplify = base.do_not_simplify;
        if base.inverted {
            map.invert();
        }
        Ok(map)
    }
}
