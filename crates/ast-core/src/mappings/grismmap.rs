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
use crate::mappings::SimplifiedRecord;

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
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// Per-attribute "was this set by the user / source" flags, mirroring
    /// the C library's `Test*` predicates so we can choose commented vs
    /// uncommented dump output to match byte-exact.
    set_flags: GrismSetFlags,
}

#[derive(Debug, Clone, Copy, Default)]
struct GrismSetFlags {
    nr: bool,
    nrp: bool,
    wave_r: bool,
    alpha: bool,
    g: bool,
    m: bool,
    eps: bool,
    theta: bool,
}

/// C's `MAKE_SET` guards its `astClearIsSimple` on the incoming value differing
/// from the structure component (`grismmap.c:255`), and that component holds the
/// raw value including the unset sentinel. The port stores the value the getter
/// would report plus a separate set flag, so a guard has to rebuild what C would
/// be comparing against.
fn stored(set: bool, value: f64) -> f64 {
    if set { value } else { BAD_VALUE }
}

/// [`stored`] for `GrismM`, whose unset sentinel is `INT_MAX`
/// (`grismmap.c:1930`).
fn stored_int(set: bool, value: i32) -> i32 {
    if set { value } else { i32::MAX }
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
        // `new` is the user-facing constructor: any value supplied here is
        // by definition explicit, so every attribute starts "set". Matches
        // the C library where `astGrismMap` parses every parameter and the
        // resulting Test* predicates are all true.
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
            simplified: SimplifiedRecord::default(),
            ident: None,
            set_flags: GrismSetFlags {
                nr: true,
                nrp: true,
                wave_r: true,
                alpha: true,
                g: true,
                m: true,
                eps: true,
                theta: true,
            },
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

    /// Set the refractive index at the reference wavelength — C's
    /// `astSetGrismNR`.
    ///
    /// GrismMap's setters come from a class-local `MAKE_SET`
    /// (`grismmap.c:236-262`) rather than `astMAKE_SET1`. It opens with the same
    /// `astClearIsSimple`, so both simplified records are discarded
    /// (`mapping.h:449-472`), and it ends by recalculating the derived constants
    /// the transformation uses.
    ///
    /// C stores the value raw and reads it back through a getter that reports
    /// the default in place of the `AST__BAD` unset sentinel
    /// (`grismmap.c:1733-1737`). The port stores what that getter would report
    /// and carries the sentinel test as a flag, so passing `BAD_VALUE` here
    /// restores the default and leaves the attribute untested, exactly as
    /// `astSetGrismNR( map, AST__BAD )` does.
    pub fn set_grism_nr(&mut self, value: f64) {
        if value != stored(self.set_flags.nr, self.grism_nr) {
            self.simplified.clear();
        }
        self.set_flags.nr = value != BAD_VALUE;
        self.grism_nr = if value == BAD_VALUE { 1.0 } else { value };
        self.update_constants();
    }

    /// Set the rate of change of refractive index with wavelength — C's
    /// `astSetGrismNRP` (`grismmap.c:1775-1779`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes.
    pub fn set_grism_nrp(&mut self, value: f64) {
        if value != stored(self.set_flags.nrp, self.grism_nrp) {
            self.simplified.clear();
        }
        self.set_flags.nrp = value != BAD_VALUE;
        self.grism_nrp = if value == BAD_VALUE { 0.0 } else { value };
        self.update_constants();
    }

    /// Set the reference wavelength — C's `astSetGrismWaveR`
    /// (`grismmap.c:1814-1818`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes.
    pub fn set_grism_wave_r(&mut self, value: f64) {
        if value != stored(self.set_flags.wave_r, self.grism_wave_r) {
            self.simplified.clear();
        }
        self.set_flags.wave_r = value != BAD_VALUE;
        self.grism_wave_r = if value == BAD_VALUE { 5000.0 } else { value };
        self.update_constants();
    }

    /// Set the angle of incidence of incoming light on the grating — C's
    /// `astSetGrismAlpha` (`grismmap.c:1852-1856`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes.
    pub fn set_grism_alpha(&mut self, value: f64) {
        if value != stored(self.set_flags.alpha, self.grism_alpha) {
            self.simplified.clear();
        }
        self.set_flags.alpha = value != BAD_VALUE;
        self.grism_alpha = if value == BAD_VALUE { 0.0 } else { value };
        self.update_constants();
    }

    /// Set the grating ruling density — C's `astSetGrismG`
    /// (`grismmap.c:1892-1896`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes.
    pub fn set_grism_g(&mut self, value: f64) {
        if value != stored(self.set_flags.g, self.grism_g) {
            self.simplified.clear();
        }
        self.set_flags.g = value != BAD_VALUE;
        self.grism_g = if value == BAD_VALUE { 0.0 } else { value };
        self.update_constants();
    }

    /// Set the diffraction order — C's `astSetGrismM`
    /// (`grismmap.c:1930-1934`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes. This attribute's unset
    /// sentinel is `INT_MAX` rather than `AST__BAD` (`grismmap.c:1930`), so
    /// `i32::MAX` is what restores the default here.
    pub fn set_grism_m(&mut self, value: i32) {
        if value != stored_int(self.set_flags.m, self.grism_m) {
            self.simplified.clear();
        }
        self.set_flags.m = value != i32::MAX;
        self.grism_m = if value == i32::MAX { 0 } else { value };
        self.update_constants();
    }

    /// Set the angle between grating normal and dispersion plane — C's
    /// `astSetGrismEps` (`grismmap.c:1970-1974`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes.
    pub fn set_grism_eps(&mut self, value: f64) {
        if value != stored(self.set_flags.eps, self.grism_eps) {
            self.simplified.clear();
        }
        self.set_flags.eps = value != BAD_VALUE;
        self.grism_eps = if value == BAD_VALUE { 0.0 } else { value };
        self.update_constants();
    }

    /// Set the angle between detector normal and reference ray — C's
    /// `astSetGrismTheta` (`grismmap.c:2011-2015`).
    ///
    /// Discards both simplified records and recalculates the derived constants,
    /// as [`set_grism_nr`](Self::set_grism_nr) describes.
    pub fn set_grism_theta(&mut self, value: f64) {
        if value != stored(self.set_flags.theta, self.grism_theta) {
            self.simplified.clear();
        }
        self.set_flags.theta = value != BAD_VALUE;
        self.grism_theta = if value == BAD_VALUE { 0.0 } else { value };
        self.update_constants();
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

        // Clone preserves the source's set_flags; we then only flip flags
        // for the three attributes that the merge math actually touches:
        // GrmNRP, GrmWR, GrmG. The others (GrmNR, GrmAlpha, GrmM, GrmEps,
        // GrmTheta) keep their original explicit/default state.
        let mut result = self.clone();
        result.grism_g = self.grism_g / zoom_factor;
        result.grism_nrp = self.grism_nrp / zoom_factor;
        result.grism_wave_r = self.grism_wave_r * zoom_factor;
        result.set_flags.g = true;
        result.set_flags.nrp = true;
        result.set_flags.wave_r = true;
        result.update_constants();

        if invert_result {
            result.invert();
        }
        // C builds this merged GrismMap the same way -- astCopy, then a setter
        // per adjusted attribute (grismmap.c:573-592) -- and each of those
        // setters discards both simplified records (grismmap.c:255,
        // `astClearIsSimple` at mapping.h:449-472).
        result.simplified.clear();

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

    /// NaN-as-bad fast path. Per-point `is_bad` gates are dropped; NaN
    /// inputs propagate through `atan`/`sin`/`asin`/`tan` cleanly. The
    /// inverse's domain check `if !(-1.0..=1.0).contains(&sinbeta)` is
    /// kept (it's a real domain failure for `asin`, not a BAD-input
    /// gate), but emits NaN instead of `BAD_VALUE` — which is also what
    /// `(k2 * NaN - k1)` already produces for NaN inputs, so the branch
    /// fires correctly on both genuine domain failures (`|sinbeta| > 1`)
    /// and NaN inputs.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if is_bad(self.k1) || is_bad(self.k2) || is_bad(self.k3) {
            for i in 0..pts.npoint() {
                out.point_mut(i)[0] = f64::NAN;
            }
            return Ok(());
        }

        if fwd {
            // Forward division-by-zero guard: k2 is a constant, not per-
            // point; if it's zero every output is NaN.
            if self.k2 == 0.0 {
                for i in 0..pts.npoint() {
                    out.point_mut(i)[0] = f64::NAN;
                }
                return Ok(());
            }
            for i in 0..pts.npoint() {
                let value_in = pts.point(i)[0];
                out.point_mut(i)[0] = (self.k1 + (value_in.atan() + self.k3).sin()) / self.k2;
            }
        } else {
            for i in 0..pts.npoint() {
                let value_in = pts.point(i)[0];
                let sinbeta = self.k2 * value_in - self.k1;
                out.point_mut(i)[0] = if !(-1.0..=1.0).contains(&sinbeta) {
                    f64::NAN
                } else {
                    (sinbeta.asin() - self.k3).tan()
                };
            }
        }
        Ok(())
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

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::simplify::grism_map_merge(self, ctx, where_, series)
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
        // special-case code (`grism_zoom_merge_at`) because the try_merge
        // API cannot distinguish forward from reverse calls, and the merge
        // is only valid in specific orderings.

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

impl Serializable for GrismMap {
    fn class_name(&self) -> &'static str {
        "GrismMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        use crate::serial::NodeValue;

        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map 1-d coordinates using a spectral disperser".to_string());
        write_mapping_base(&mut node, self);

        // Helper: write a double attribute, picking commented vs uncommented
        // by the per-attribute "set" flag (mirrors C's `Test*` predicate).
        let write_attr = |node: &mut Node, key: &str, val: f64, set: bool, comment: &str| {
            if set {
                node.push_double(key, val, Some(comment.to_string()));
            } else {
                node.push_unset(key, NodeValue::Double(val), Some(comment.to_string()));
            }
        };

        write_attr(
            &mut node,
            "GrmNR",
            self.grism_nr,
            self.set_flags.nr,
            "Refractive index at the ref. wavelength",
        );
        write_attr(
            &mut node,
            "GrmNRP",
            self.grism_nrp,
            self.set_flags.nrp,
            "Rate of change of refractive index",
        );
        write_attr(
            &mut node,
            "GrmWR",
            self.grism_wave_r,
            self.set_flags.wave_r,
            "Ref. wavelength",
        );
        write_attr(
            &mut node,
            "GrmAlp",
            self.grism_alpha,
            self.set_flags.alpha,
            "Angle of incidence of incoming light",
        );
        write_attr(
            &mut node,
            "GrmG",
            self.grism_g,
            self.set_flags.g,
            "Grating ruling density",
        );

        // GrmM — integer attribute. Same set-tracking logic.
        let m_val = self.grism_m as f64;
        if self.set_flags.m {
            node.push_double("GrmM", m_val, Some("The interference order".to_string()));
        } else {
            node.push_unset(
                "GrmM",
                NodeValue::Double(m_val),
                Some("The interference order".to_string()),
            );
        }

        write_attr(
            &mut node,
            "GrmEps",
            self.grism_eps,
            self.set_flags.eps,
            "Angle between grating normal and dispersion plane",
        );
        write_attr(
            &mut node,
            "GrmTh",
            self.grism_theta,
            self.set_flags.theta,
            "Angle between detector normal and reference ray",
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("GrismMap");
        use crate::serial::NodeValue;
        let base = read_mapping_base(whole);
        let nr = node.get_double("GrmNR", 1.0);
        let nrp = node.get_double("GrmNRP", 0.0);
        let wave_r = node.get_double("GrmWR", 5000.0);
        let alpha = node.get_double("GrmAlp", 0.0);
        let g = node.get_double("GrmG", 0.0);
        let m = node.get_double("GrmM", 0.0) as i32;
        let eps = node.get_double("GrmEps", 0.0);
        let theta = node.get_double("GrmTh", 0.0);

        // Detect which attributes were uncommented (explicit) in the source.
        // `get_any` unwraps `Unset(inner)` so it can't distinguish commented
        // from uncommented; walk `items` directly.
        let is_set = |key: &str| {
            node.items
                .iter()
                .find(|i| i.key == key)
                .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)))
        };
        let set_flags = GrismSetFlags {
            nr: is_set("GrmNR"),
            nrp: is_set("GrmNRP"),
            wave_r: is_set("GrmWR"),
            alpha: is_set("GrmAlp"),
            g: is_set("GrmG"),
            m: is_set("GrmM"),
            eps: is_set("GrmEps"),
            theta: is_set("GrmTh"),
        };

        let mut map = GrismMap::new(nr, nrp, wave_r, alpha, g, m, eps, theta);
        map.set_flags = set_flags;
        if base.inverted {
            map.invert();
        }
        map.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        // C's loader re-applies each attribute whose `Test*` predicate reports it
        // set through the private setter (`grismmap.c:2548-2571`), which discards
        // the simplified record the parent loader installed from the `IsSimp`
        // card. The routing mirrors a quirk rather than a design decision, so it
        // is revisited when the pin moves: see `docs/issues/c-library-quirks.md`,
        // "The `IsSimp` card does not survive being read back, for twelve
        // classes".
        if set_flags.nr {
            map.set_grism_nr(nr);
        }
        if set_flags.nrp {
            map.set_grism_nrp(nrp);
        }
        if set_flags.wave_r {
            map.set_grism_wave_r(wave_r);
        }
        if set_flags.alpha {
            map.set_grism_alpha(alpha);
        }
        if set_flags.g {
            map.set_grism_g(g);
        }
        if set_flags.m {
            map.set_grism_m(m);
        }
        if set_flags.eps {
            map.set_grism_eps(eps);
        }
        if set_flags.theta {
            map.set_grism_theta(theta);
        }
        Ok(map)
    }
}
