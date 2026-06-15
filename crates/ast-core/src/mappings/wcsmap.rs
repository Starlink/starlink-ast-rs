//! FITS-WCS sky projection mapping.

use std::cell::RefCell;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};
use crate::wcslib::{PrjParams, WcsProjection};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::perm::PermMap;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// Maximum number of projection parameters per axis (from wcslib).
const MXPAR: usize = 100;

/// A mapping that applies a FITS-WCS sky projection.
///
/// `WcsMap` transforms between native spherical coordinates (longitude, latitude
/// in radians) and projected coordinates (x, y), using the wcslib projection
/// library.  It supports all 31 FITS-WCS projection types defined in the
/// [`WcsProjection`] enum.
///
/// Axes other than the longitude and latitude axes are passed through unchanged.
///
/// # Example
///
/// ```
/// use ast_core::mappings::{Mapping, WcsMap, WcsProjection};
/// use ast_core::coords::PointSet;
///
/// // Create a TAN (gnomonic) projection with 2 axes,
/// // longitude on axis 1, latitude on axis 2.
/// let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
///
/// // Transform a point near the pole (in radians).
/// let lon = 0.1_f64;
/// let lat = 1.4_f64;
/// let pts = PointSet::from_rows([[lon, lat]]);
/// let out = wcs.transform(&pts, true).unwrap();
/// // Inverse should recover the original.
/// let back = wcs.transform(&out, false).unwrap();
/// assert!((back.point(0)[0] - lon).abs() < 1e-10);
/// assert!((back.point(0)[1] - lat).abs() < 1e-10);
/// ```
#[derive(Debug)]
pub struct WcsMap {
    /// The projection type requested at construction time.
    ///
    /// For NCP and GLS this stores the original deprecated code, while
    /// the underlying `PrjParams` uses the equivalent SIN or SFL.
    proj: WcsProjection,
    /// 0-based longitude axis index.
    lonax: usize,
    /// 0-based latitude axis index.
    latax: usize,
    /// Total number of coordinate axes (>= 2).
    ncoord: usize,
    /// Per-axis projection parameters.
    /// `params[0]` = latitude axis (wcslib `p`), `params[1]` = longitude axis (wcslib `p2`).
    /// Each inner vec is indexed by parameter number `m`.
    params: Vec<Vec<Option<f64>>>,
    /// wcslib C struct (interior mutability because `transform` takes `&self`
    /// but the C code mutates internal state on first use after param changes).
    prj: RefCell<PrjParams>,
    /// Whether the mapping is inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    has_been_simplified: bool,
}

impl WcsMap {
    /// Create a new `WcsMap`.
    ///
    /// - `ncoord`: total number of coordinate axes (must be >= 2).
    /// - `proj`: the FITS-WCS projection type.
    /// - `lonax`: 1-based index of the longitude axis.
    /// - `latax`: 1-based index of the latitude axis.
    ///
    /// Returns an error if the arguments are invalid or the projection
    /// cannot be initialised.
    pub fn new(ncoord: usize, proj: WcsProjection, lonax: usize, latax: usize) -> AstResult<Self> {
        if ncoord < 2 {
            return Err(AstError::new(
                ErrorCode::Wcsnc,
                format!("WcsMap ncoord must be >= 2, got {ncoord}"),
            ));
        }
        if lonax == 0 || lonax > ncoord {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap lonax {lonax} out of range 1..={ncoord}"),
            ));
        }
        if latax == 0 || latax > ncoord {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap latax {latax} out of range 1..={ncoord}"),
            ));
        }
        if lonax == latax {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap lonax and latax must differ, both are {lonax}"),
            ));
        }

        // For NCP and GLS, create the underlying projection as SIN/SFL
        // with appropriate default parameters.
        // For Unknown, we don't need a PrjParams — it acts as identity.
        let underlying = match proj {
            WcsProjection::Ncp => WcsProjection::Sin,
            WcsProjection::Gls => WcsProjection::Sfl,
            WcsProjection::Unknown => {
                // Unknown projection has no wcslib implementation.
                // It acts as an identity mapping and simplifies to UnitMap.
                let params = vec![Vec::new(), Vec::new()];
                // The `prj` field must hold *some* PrjParams, but it is never
                // consulted for an Unknown WcsMap: `transform_into`/
                // `transform_into_nan` short-circuit to an identity copy,
                // `simplified` returns a UnitMap, and `nat_lon`/`nat_lat`
                // return 0 directly. CAR is the cheapest inert placeholder.
                let prj = PrjParams::new(WcsProjection::Car)?;
                return Ok(Self {
                    proj,
                    lonax: lonax - 1,
                    latax: latax - 1,
                    ncoord,
                    params,
                    prj: RefCell::new(prj),
                    inverted: false,
                    has_been_simplified: false,
                });
            }
            other => other,
        };

        let prj = PrjParams::new(underlying)?;

        // Initialise empty parameter arrays for 2 axes (lat=0, lon=1).
        let params = vec![Vec::new(), Vec::new()];

        Ok(Self {
            proj,
            lonax: lonax - 1,
            latax: latax - 1,
            ncoord,
            params,
            prj: RefCell::new(prj),
            inverted: false,
            has_been_simplified: false,
        })
    }

    /// Set a projection parameter.
    ///
    /// - `axis`: 1-based axis index. Must be either `lonax()` or `latax()`.
    /// - `m`: parameter number (0-based, must be < 100).
    /// - `value`: the parameter value.
    pub fn set_pv(&mut self, axis: usize, m: usize, value: f64) -> AstResult<()> {
        if m >= MXPAR {
            return Err(AstError::new(
                ErrorCode::Wcspa,
                format!(
                    "WcsMap PV parameter index {m} out of range (max {})",
                    MXPAR - 1
                ),
            ));
        }

        // Determine which wcslib axis this corresponds to.
        // axis is 1-based.  latax is 0-based internally.
        let prj_axis = if axis == 0 || axis > self.ncoord {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap set_pv axis {axis} out of range 1..={}", self.ncoord),
            ));
        } else if axis - 1 == self.latax {
            0 // latitude -> p storage
        } else if axis - 1 == self.lonax {
            1 // longitude -> p2 storage
        } else {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!(
                    "WcsMap set_pv axis {axis} is neither longitude ({}) nor latitude ({})",
                    self.lonax + 1,
                    self.latax + 1,
                ),
            ));
        };

        // Grow the parameter vector if needed.
        let pv = &mut self.params[prj_axis];
        if m >= pv.len() {
            pv.resize(m + 1, None);
        }
        pv[m] = Some(value);

        // Push to wcslib.
        self.prj.borrow_mut().set_param(prj_axis, m, value);

        Ok(())
    }

    /// Return the FITS-WCS projection type.
    pub fn projection(&self) -> WcsProjection {
        self.proj
    }

    /// Get the PV parameter value for the given 1-based axis and
    /// 0-based parameter index. Returns `None` if the parameter has
    /// not been set, or `axis` is neither the longitude nor latitude
    /// axis. Inverse of [`set_pv`].
    pub fn get_pv(&self, axis: usize, m: usize) -> Option<f64> {
        let prj_axis = if axis == 0 || axis > self.ncoord {
            return None;
        } else if axis - 1 == self.latax {
            0
        } else if axis - 1 == self.lonax {
            1
        } else {
            return None;
        };
        self.params[prj_axis].get(m).copied().flatten()
    }

    /// Return the 1-based longitude axis index.
    pub fn lonax(&self) -> usize {
        self.lonax + 1
    }

    /// Return the 1-based latitude axis index.
    pub fn latax(&self) -> usize {
        self.latax + 1
    }

    /// Native longitude of the fiducial point (radians).
    ///
    /// For zenithal projections (TAN, SIN, STG, ARC, etc.) this is 0.
    /// The underlying wcslib struct stores this in degrees; this method
    /// converts to radians (the AST convention).
    pub fn nat_lon(&self) -> f64 {
        // An Unknown (AST__WCSBAD) projection has no real reference point;
        // C `GetNatLon` is a fixed 0 for every projection type (wcsmap.c),
        // so return 0 explicitly rather than relying on the inert CAR
        // placeholder's reference point.
        if self.proj == WcsProjection::Unknown {
            return 0.0;
        }
        let (phi0, _theta0) = self.prj.borrow().reference_point();
        phi0.to_radians()
    }

    /// Native latitude of the fiducial point (radians).
    ///
    /// For zenithal projections (TAN, SIN, STG, ARC, etc.) this is pi/2.
    /// For cylindrical projections (CAR, MER, CEA, etc.) this is 0.
    /// The underlying wcslib struct stores this in degrees; this method
    /// converts to radians (the AST convention).
    pub fn nat_lat(&self) -> f64 {
        // An Unknown (AST__WCSBAD) projection has no real reference point; C
        // `GetNatLat` reads `FindPrjData(AST__WCSBAD)->theta0`, which is 0
        // (wcsmap.c:716). Return 0 explicitly rather than relying on the inert
        // CAR placeholder (whose reference point is also 0, but only by
        // coincidence).
        if self.proj == WcsProjection::Unknown {
            return 0.0;
        }
        let (_phi0, theta0) = self.prj.borrow().reference_point();
        theta0.to_radians()
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

    /// Rebuild PrjParams from stored parameters (used by clone and simplified).
    fn rebuild_prj(&self) -> AstResult<PrjParams> {
        let underlying = match self.proj {
            WcsProjection::Ncp => WcsProjection::Sin,
            WcsProjection::Gls => WcsProjection::Sfl,
            // Unknown (AST__WCSBAD) has no wcslib projection. The stored
            // PrjParams is never consulted (transform is identity, simplify is
            // a UnitMap, nat_lon/nat_lat return 0), so CAR is an inert
            // placeholder kept only so this field is populated.
            WcsProjection::Unknown => WcsProjection::Car,
            other => other,
        };
        let mut prj = PrjParams::new(underlying)?;
        for (prj_axis, pv) in self.params.iter().enumerate() {
            for (m, val) in pv.iter().enumerate() {
                if let Some(v) = val {
                    prj.set_param(prj_axis, m, *v);
                }
            }
        }
        Ok(prj)
    }

    /// Create a copy of this WcsMap with different axis assignments.
    ///
    /// Used by the simplify engine to absorb a PermMap into a WcsMap
    /// by remapping which axes carry the longitude/latitude.
    /// `new_lonax` and `new_latax` are 0-based.
    pub(crate) fn with_permuted_axes(
        &self,
        new_ncoord: usize,
        new_lonax: usize,
        new_latax: usize,
    ) -> Option<Self> {
        if new_lonax == new_latax || new_lonax >= new_ncoord || new_latax >= new_ncoord {
            return None;
        }
        let prj = self.rebuild_prj().ok()?;
        Some(Self {
            proj: self.proj,
            lonax: new_lonax,
            latax: new_latax,
            ncoord: new_ncoord,
            params: self.params.clone(),
            prj: RefCell::new(prj),
            inverted: self.inverted,
            has_been_simplified: self.has_been_simplified,
        })
    }

    /// 0-based longitude axis index (crate-internal).
    pub(crate) fn lonax_0based(&self) -> usize {
        self.lonax
    }

    /// 0-based latitude axis index (crate-internal).
    pub(crate) fn latax_0based(&self) -> usize {
        self.latax
    }

    /// Compare projection parameters between two WcsMaps.
    fn params_equal(&self, other: &WcsMap) -> bool {
        for axis in 0..2 {
            let p1 = self.params.get(axis).map(|v| v.as_slice()).unwrap_or(&[]);
            let p2 = other.params.get(axis).map(|v| v.as_slice()).unwrap_or(&[]);
            let max_len = p1.len().max(p2.len());
            for m in 0..max_len {
                let v1 = p1.get(m).copied().flatten().unwrap_or(0.0);
                let v2 = p2.get(m).copied().flatten().unwrap_or(0.0);
                if !ast_equal(v1, v2) {
                    return false;
                }
            }
        }
        true
    }
}

impl Clone for WcsMap {
    fn clone(&self) -> Self {
        let prj = self
            .rebuild_prj()
            .expect("WcsMap::clone: failed to rebuild PrjParams");
        Self {
            proj: self.proj,
            lonax: self.lonax,
            latax: self.latax,
            ncoord: self.ncoord,
            params: self.params.clone(),
            prj: RefCell::new(prj),
            inverted: self.inverted,
            has_been_simplified: self.has_been_simplified,
        }
    }
}

impl Mapping for WcsMap {
    fn nin(&self) -> usize {
        self.ncoord
    }

    fn nout(&self) -> usize {
        self.ncoord
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
        // Inversion invalidates the "already simplified" flag, since the
        // inverse may simplify further (C astSetInvert clears IsSimple).
        self.has_been_simplified = false;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), self.ncoord);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. Same logic as `transform_into`, but the
    /// is_bad branch and projection-`Err` branch emit `f64::NAN` instead
    /// of `BAD_VALUE` so downstream NaN-aware stages receive the protocol-
    /// canonical sentinel rather than a finite -1.8e308 that would
    /// produce numerical garbage through their arithmetic.
    ///
    /// The is_bad gate at the top is *kept* because the underlying
    /// projection routines (`wcslib`-port `prj.forward`/`reverse`) don't
    /// handle `BAD_VALUE` or `NaN` inputs gracefully — they may return
    /// Err, NaN, or garbage depending on which trig path they take. Gating
    /// here is cheap (one comparison per point per axis) and keeps the
    /// projection math on the well-tested finite-input path.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        if self.proj == WcsProjection::Unknown {
            for i in 0..pts.npoint() {
                out.point_mut(i).copy_from_slice(pts.point(i));
            }
            return Ok(());
        }

        let fwd = self.effective_forward(forward);
        let mut prj = self.prj.borrow_mut();

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            outp.copy_from_slice(inp);

            let v1 = inp[self.lonax];
            let v2 = inp[self.latax];

            if is_bad(v1) || is_bad(v2) {
                outp[self.lonax] = f64::NAN;
                outp[self.latax] = f64::NAN;
                continue;
            }

            if fwd {
                let lon_deg = v1.to_degrees();
                let lat_deg = v2.to_degrees();
                match prj.forward(lon_deg, lat_deg) {
                    Ok((x, y)) => {
                        outp[self.lonax] = x.to_radians();
                        outp[self.latax] = y.to_radians();
                    }
                    Err(_) => {
                        outp[self.lonax] = f64::NAN;
                        outp[self.latax] = f64::NAN;
                    }
                }
            } else {
                let x_deg = v1.to_degrees();
                let y_deg = v2.to_degrees();
                match prj.reverse(x_deg, y_deg) {
                    Ok((phi, theta)) => {
                        outp[self.lonax] = phi.to_radians();
                        outp[self.latax] = theta.to_radians();
                    }
                    Err(_) => {
                        outp[self.lonax] = f64::NAN;
                        outp[self.latax] = f64::NAN;
                    }
                }
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        // Unknown/blank projection acts as identity — pass through all coords.
        if self.proj == WcsProjection::Unknown {
            for i in 0..pts.npoint() {
                out.point_mut(i).copy_from_slice(pts.point(i));
            }
            return Ok(());
        }

        let fwd = self.effective_forward(forward);
        let mut prj = self.prj.borrow_mut();

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);

            // Copy all axes through first.
            outp.copy_from_slice(inp);

            let v1 = inp[self.lonax];
            let v2 = inp[self.latax];

            if is_bad(v1) || is_bad(v2) {
                outp[self.lonax] = BAD_VALUE;
                outp[self.latax] = BAD_VALUE;
                continue;
            }

            if fwd {
                // Forward: native spherical (lon, lat in radians) -> projected (x, y)
                let lon_deg = v1.to_degrees();
                let lat_deg = v2.to_degrees();
                match prj.forward(lon_deg, lat_deg) {
                    Ok((x, y)) => {
                        outp[self.lonax] = x.to_radians();
                        outp[self.latax] = y.to_radians();
                    }
                    Err(_) => {
                        outp[self.lonax] = BAD_VALUE;
                        outp[self.latax] = BAD_VALUE;
                    }
                }
            } else {
                // Inverse: projected (x, y) -> native spherical (lon, lat in radians)
                let x_deg = v1.to_degrees();
                let y_deg = v2.to_degrees();
                match prj.reverse(x_deg, y_deg) {
                    Ok((phi, theta)) => {
                        outp[self.lonax] = phi.to_radians();
                        outp[self.latax] = theta.to_radians();
                    }
                    Err(_) => {
                        outp[self.lonax] = BAD_VALUE;
                        outp[self.latax] = BAD_VALUE;
                    }
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified {
            return Ok(Box::new(self.clone()));
        }

        // Unknown/blank projection simplifies to identity.
        if self.proj == WcsProjection::Unknown {
            Ok(Box::new(UnitMap::new(self.ncoord)))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors wcsmap.c::MapSplit (~3342). C first defers to the parent
        // (basic-Mapping) MapSplit, which only succeeds when every input is
        // selected (`nin == mapnin`). If that fails, the WcsMap-specific
        // override runs and handles partial input selections by case:
        //
        //   - both celestial axes kept       -> smaller WcsMap
        //   - neither celestial axis kept    -> UnitMap
        //   - only one celestial axis kept   -> PermMap that drops that axis
        //                                       (yields BAD on inverse) — only
        //                                       valid when nin > 1
        //
        // Validation note: project convention rejects out-of-range or
        // duplicate input indices up front (UnitMap/ZoomMap/ShiftMap/etc. set
        // this precedent in Tasks 1.1-1.4). C's parent path errors on
        // out-of-range and rejects duplicates implicitly, while the WcsMap
        // override only checks the in-range half. The Rust gate is stricter
        // but never accepts a case C would reject.
        let mapnin = self.nin();
        let nin = in_axes.len();

        // Full-set path: delegate to the shared helper, which mirrors C's
        // parent (basic-Mapping) MapSplit. For an identity permutation it
        // returns `self`; for any other permutation it builds PermMap+self
        // and simplifies (the simplifier may absorb the perm back into a
        // single WcsMap with permuted axes).
        if let Some(result) = crate::mappings::helpers::full_set_split(self, in_axes) {
            return Some(result);
        }

        // Partial-set path: WcsMap-specific override.
        crate::mappings::helpers::validate_in_axes(in_axes, mapnin)?;
        let lonax = self.lonax;
        let latax = self.latax;
        let mut ilon: Option<usize> = None;
        let mut ilat: Option<usize> = None;
        let mut ilatlon: usize = 0;
        let mut result: Vec<usize> = Vec::with_capacity(nin);
        for (i, &iin) in in_axes.iter().enumerate() {
            if iin == lonax {
                ilon = Some(i);
                ilatlon = i;
            } else if iin == latax {
                ilat = Some(i);
                ilatlon = i;
            }
            result.push(iin);
        }

        match (ilon, ilat) {
            // Both celestial axes selected: returned mapping is a smaller
            // WcsMap with the same projection / params and Invert flag, with
            // the longitude/latitude axes remapped to their positions in the
            // kept-input ordering. Mirrors C's astWcsMap+CopyPV+SetInvert.
            (Some(ilon), Some(ilat)) => {
                let new_wcs = self.with_permuted_axes(nin, ilon, ilat)?;
                Some((result, Box::new(new_wcs)))
            }
            // Neither celestial axis selected: returned mapping is a UnitMap
            // over the kept pass-through axes. Mirrors C's astUnitMap branch.
            (None, None) => Some((result, Box::new(UnitMap::new(nin)))),
            // Only one celestial axis selected: drop it. The C code treats
            // this as nin > 1 only — for nin == 1 (the lone kept input is
            // the celestial), it frees the result and returns NULL.
            _ => {
                if nin <= 1 {
                    return None;
                }
                // Drop the celestial position from `result` (length becomes
                // nin - 1, matching the new mapping's nout).
                result.remove(ilatlon);

                // Build a PermMap with `nin` inputs and `nin - 1` outputs.
                // Forward: skip the celestial axis at position `ilatlon`.
                // Inverse: yield BAD at position `ilatlon` (C: INT_MAX, which
                // AST C's PermMap treats as out-of-range -> AST__BAD; our
                // PermMap encodes BAD with a 0 entry).
                let mut inperm: Vec<i64> = vec![0; nin];
                let mut outperm: Vec<i64> = vec![0; nin - 1];
                for i in 0..ilatlon {
                    inperm[i] = (i + 1) as i64;
                    outperm[i] = (i + 1) as i64;
                }
                inperm[ilatlon] = 0; // BAD on inverse for the dropped axis
                for i in (ilatlon + 1)..nin {
                    inperm[i] = i as i64;
                    outperm[i - 1] = (i + 1) as i64;
                }
                let pm = PermMap::new(&inperm, &outperm, &[]).ok()?;
                Some((result, Box::new(pm)))
            }
        }
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
        // Only handle inverse cancellation: same WcsMap in opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_wcs = other.as_any().downcast_ref::<WcsMap>()?;
        if self.proj == other_wcs.proj
            && self.lonax == other_wcs.lonax
            && self.latax == other_wcs.latax
            && self.ncoord == other_wcs.ncoord
            && self.params_equal(other_wcs)
        {
            // WcsMap always has nin == nout == ncoord.
            Some(Box::new(UnitMap::new(self.ncoord)))
        } else {
            None
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<WcsMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        self.proj == other.proj
            && self.lonax == other.lonax
            && self.latax == other.latax
            && self.ncoord == other.ncoord
            && self.params_equal(other)
    }

    fn has_been_simplified(&self) -> bool {
        self.has_been_simplified
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.has_been_simplified = flag;
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

/// Return the human-readable description for a projection type (for Native
/// format comments).
fn projection_description(proj: WcsProjection) -> &'static str {
    // Verbatim from the C projection table (wcsmap.c:685-716); the native
    // dump comment is "<desc> projection".
    match proj {
        WcsProjection::Azp => "zenithal perspective",
        WcsProjection::Szp => "slant zenithal perspective",
        WcsProjection::Tan => "gnomonic",
        WcsProjection::Stg => "stereographic",
        WcsProjection::Sin => "orthographic",
        WcsProjection::Arc => "zenithal equidistant",
        WcsProjection::Zpn => "zenithal polynomial",
        WcsProjection::Zea => "zenithal equal area",
        WcsProjection::Air => "Airy",
        WcsProjection::Cyp => "cylindrical perspective",
        WcsProjection::Cea => "cylindrical equal area",
        WcsProjection::Car => "Cartesian",
        WcsProjection::Mer => "Mercator",
        WcsProjection::Sfl => "Sanson-Flamsteed",
        WcsProjection::Par => "parabolic",
        WcsProjection::Mol => "Mollweide",
        WcsProjection::Ait => "Hammer-Aitoff",
        WcsProjection::Cop => "conical perspective",
        WcsProjection::Coe => "conical equal area",
        WcsProjection::Cod => "conical equidistant",
        WcsProjection::Coo => "conical orthomorphic",
        WcsProjection::Bon => "Bonne's equal area",
        WcsProjection::Pco => "polyconic",
        WcsProjection::Tsc => "tangential spherical cube",
        WcsProjection::Csc => "cobe quadrilateralized spherical cube",
        WcsProjection::Qsc => "quadrilateralized spherical cube",
        WcsProjection::Ncp => "AIPS north celestial pole",
        WcsProjection::Gls => "sinusoidal",
        WcsProjection::Tpn => "gnomonic polynomial",
        WcsProjection::Hpx => "HEALPix",
        WcsProjection::Xph => "polar HEALPix",
        WcsProjection::Unknown => "<null>",
    }
}

impl Serializable for WcsMap {
    fn class_name(&self) -> &'static str {
        "WcsMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("FITS-WCS sky projection".to_string());

        // Mapping base attributes.
        write_mapping_base(&mut node, self);

        // Type — 3-letter FITS projection code, always written. The comment
        // is "<desc> projection" with the first character upper-cased,
        // mirroring C `WcsMap::Dump` (wcsmap.c:5325-5326): the projection
        // descriptions themselves are lower-case in the C table.
        let descr = projection_description(self.proj);
        let mut comment = format!("{descr} projection");
        if let Some(first) = comment.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        node.push_string("Type", self.proj.code(), Some(comment));

        // PV parameters.
        // params[0] = latitude axis (wcslib p), params[1] = longitude axis (wcslib p2).
        // In the Native format, axis indices are 1-based and refer to the
        // WcsMap's lonax/latax.
        for (prj_axis, pv) in self.params.iter().enumerate() {
            // Map prj_axis back to the 1-based WcsMap axis.
            let axis_1based = if prj_axis == 0 {
                self.latax + 1
            } else {
                self.lonax + 1
            };
            for (m, val) in pv.iter().enumerate() {
                if let Some(v) = val {
                    node.push_double(
                        &format!("PV{axis_1based}_{m}"),
                        *v,
                        Some(format!("Projection parameter {m} for axis {axis_1based}")),
                    );
                }
            }
        }

        // WcsAx1 / WcsAx2 — only written when non-default.
        if self.lonax != 0 {
            node.push_int(
                "WcsAx1",
                (self.lonax + 1) as i64,
                Some("Index of celestial longitude axis".to_string()),
            );
        }
        if self.latax != 1 {
            node.push_int(
                "WcsAx2",
                (self.latax + 1) as i64,
                Some("Index of celestial latitude axis".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        // Read projection type.
        // If the type is blank or unrecognised, fall back to Unknown (which
        // acts as identity and simplifies to UnitMap).
        let type_str = node.get_string("Type", "");
        let proj = WcsProjection::from_code(type_str.trim()).unwrap_or(WcsProjection::Unknown);

        // Read axis indices (1-based, defaults 1 and 2).
        let wcsax1 = node.get_int("WcsAx1", 1) as usize;
        let wcsax2 = node.get_int("WcsAx2", 2) as usize;

        let mut wcs = WcsMap::new(base.nin, proj, wcsax1, wcsax2)?;

        // Scan items for PV{i}_{m} keys.
        for item in &node.items {
            if let Some(rest) = item.key.strip_prefix("PV") {
                if let Some((axis_str, m_str)) = rest.split_once('_') {
                    if let (Ok(axis), Ok(m)) = (axis_str.parse::<usize>(), m_str.parse::<usize>()) {
                        let value = match &item.value {
                            crate::serial::NodeValue::Double(v) => *v,
                            crate::serial::NodeValue::Int(v) => *v as f64,
                            _ => continue,
                        };
                        wcs.set_pv(axis, m, value)?;
                    }
                }
            }
        }

        if base.inverted {
            wcs.invert();
        }

        wcs.set_has_been_simplified(base.has_been_simplified);

        Ok(wcs)
    }
}
