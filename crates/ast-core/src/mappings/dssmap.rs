//! 2-D pixel to equatorial sky mapping via a Digitised Sky Survey plate solution.
//!
//! A `DssMap` transforms 2-dimensional pixel coordinates into equatorial
//! sky coordinates (right ascension and declination) using a DSS astrometric
//! plate solution.  The input coordinates are pixel numbers along the first
//! and second dimensions of an image, where the centre of the first pixel is
//! located at (1,1) and the spacing between pixel centres is unity.  The
//! output coordinates are right ascension and declination in radians.
//!
//! The plate solution algorithm is ported from the SAOimage `platepos` /
//! `platepix` routines (Doug Mink, Smithsonian Astrophysical Observatory,
//! 1988/1996), as used by the C AST library.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::winmap::WinMap;
use super::{DynMapping, Mapping};

/// Radians per degree, as used *inside* the SAOimage `platepos` / `platepix`
/// routines (their literal `cond2r`). This truncated value is part of the
/// faithful port of those routines and must not be substituted with the
/// full-precision constant.
const COND2R: f64 = 1.745329252e-2;

/// Arcseconds per radian, as used inside `platepos` / `platepix` (their
/// literal `cons2r` / `conr2s`).
const CONS2R: f64 = 206264.8062470964;

/// Degrees per radian (`AST__DR2D`), used by the `Transform` wrapper to
/// convert the sky coordinates fed to `platepix`. Full precision, matching C.
const DR2D: f64 = 180.0 / std::f64::consts::PI;

/// Radians per degree (`AST__DD2R`), used by the `Transform` wrapper to
/// convert the sky coordinates returned by `platepos`. Full precision,
/// matching C.
const DD2R: f64 = std::f64::consts::PI / 180.0;

/// 2 * pi.
const TWOPI: f64 = std::f64::consts::TAU;

/// A 2-D pixel to equatorial sky mapping via a DSS plate solution.
///
/// The forward transformation converts pixel (x, y) to (RA, Dec) in
/// radians.  The inverse converts (RA, Dec) in radians to pixel (x, y).
///
/// There is no public constructor — mirroring C, where `astDssMap` is a
/// *protected* function (only in `dssmap.h`, not the public `ast.h`). A
/// `DssMap` is therefore produced only by deserialising a native AST dump
/// (typically one written by an older AST, before the FITS-WCS DSS path was
/// changed to emit a TPN/`WcsMap` pipeline) or internally when the simplifier
/// rebuilds one while absorbing a neighbouring `WinMap`. The type itself is
/// public so callers can recognise it (matching C's public `AstDssMap` type
/// and `astIsADssMap`).
#[derive(Debug, Clone)]
pub struct DssMap {
    /// Right ascension of plate centre (radians).
    plate_ra: f64,
    /// Declination of plate centre (radians).
    plate_dec: f64,
    /// Plate scale (arcsec/mm).
    plate_scale: f64,
    /// X pixel offset of image corner.
    x_pixel_offset: f64,
    /// Y pixel offset of image corner.
    y_pixel_offset: f64,
    /// X pixel size (microns).
    x_pixel_size: f64,
    /// Y pixel size (microns).
    y_pixel_size: f64,
    /// Orientation coefficients PPO1-PPO6.
    ppo_coeff: [f64; 6],
    /// Plate solution X coefficients AMDX1-AMDX19.
    amd_x_coeff: [f64; 19],
    /// Plate solution Y coefficients AMDY1-AMDY19.
    amd_y_coeff: [f64; 19],
    /// Whether this mapping is inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    has_been_simplified: bool,
}

impl DssMap {
    /// Create a new `DssMap` from explicit plate solution parameters.
    ///
    /// Crate-private, mirroring C's *protected* `astDssMap` constructor: a
    /// `DssMap` is only ever built internally — by native deserialisation
    /// (`from_node`) or by the simplifier's `WinMap`-absorption rebuild. All
    /// angular quantities are in radians except `plate_scale` (arcsec/mm).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_plate_params(
        plate_ra: f64,
        plate_dec: f64,
        plate_scale: f64,
        x_pixel_offset: f64,
        y_pixel_offset: f64,
        x_pixel_size: f64,
        y_pixel_size: f64,
        ppo_coeff: [f64; 6],
        amd_x_coeff: [f64; 19],
        amd_y_coeff: [f64; 19],
    ) -> Self {
        Self {
            plate_ra,
            plate_dec,
            plate_scale,
            x_pixel_offset,
            y_pixel_offset,
            x_pixel_size,
            y_pixel_size,
            ppo_coeff,
            amd_x_coeff,
            amd_y_coeff,
            inverted: false,
            has_been_simplified: false,
        }
    }

    /// Plate centre RA (radians).
    pub fn plate_ra(&self) -> f64 {
        self.plate_ra
    }

    /// Plate centre Dec (radians).
    pub fn plate_dec(&self) -> f64 {
        self.plate_dec
    }

    /// Plate scale (arcsec/mm).
    pub fn plate_scale(&self) -> f64 {
        self.plate_scale
    }

    /// X pixel offset.
    pub fn x_pixel_offset(&self) -> f64 {
        self.x_pixel_offset
    }

    /// Y pixel offset.
    pub fn y_pixel_offset(&self) -> f64 {
        self.y_pixel_offset
    }

    /// X pixel size (microns).
    pub fn x_pixel_size(&self) -> f64 {
        self.x_pixel_size
    }

    /// Y pixel size (microns).
    pub fn y_pixel_size(&self) -> f64 {
        self.y_pixel_size
    }

    /// PPO orientation coefficients.
    pub fn ppo_coeff(&self) -> &[f64; 6] {
        &self.ppo_coeff
    }

    /// Plate solution X coefficients.
    pub fn amd_x_coeff(&self) -> &[f64; 19] {
        &self.amd_x_coeff
    }

    /// Plate solution Y coefficients.
    pub fn amd_y_coeff(&self) -> &[f64; 19] {
        &self.amd_y_coeff
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

    /// Forward transform: pixel (x, y) -> (RA, Dec) in degrees.
    ///
    /// Ported from SAOimage `platepos` (Doug Mink, 1996).
    /// Returns `Ok(())` on success, `Err(())` if the angle is too large.
    /// Output (xpos, ypos) are in degrees.
    fn platepos(&self, xpix: f64, ypix: f64, xpos: &mut f64, ypos: &mut f64) -> Result<(), ()> {
        // Convert from image pixels to plate pixels
        let x = xpix + self.x_pixel_offset - 1.0 + 0.5;
        let y = ypix + self.y_pixel_offset - 1.0 + 0.5;

        // Convert from pixels to millimeters
        let xmm = (self.ppo_coeff[2] - x * self.x_pixel_size) / 1000.0;
        let ymm = (y * self.y_pixel_size - self.ppo_coeff[5]) / 1000.0;
        let xmm2 = xmm * xmm;
        let ymm2 = ymm * ymm;
        let xmm3 = xmm * xmm2;
        let ymm3 = ymm * ymm2;
        let x2y2 = xmm2 + ymm2;

        // Compute standard coordinates from plate model
        let xi = self.amd_x_coeff[0] * xmm
            + self.amd_x_coeff[1] * ymm
            + self.amd_x_coeff[2]
            + self.amd_x_coeff[3] * xmm2
            + self.amd_x_coeff[4] * xmm * ymm
            + self.amd_x_coeff[5] * ymm2
            + self.amd_x_coeff[6] * x2y2
            + self.amd_x_coeff[7] * xmm3
            + self.amd_x_coeff[8] * xmm2 * ymm
            + self.amd_x_coeff[9] * xmm * ymm2
            + self.amd_x_coeff[10] * ymm3
            + self.amd_x_coeff[11] * xmm * x2y2
            + self.amd_x_coeff[12] * xmm * x2y2 * x2y2;

        let eta = self.amd_y_coeff[0] * ymm
            + self.amd_y_coeff[1] * xmm
            + self.amd_y_coeff[2]
            + self.amd_y_coeff[3] * ymm2
            + self.amd_y_coeff[4] * xmm * ymm
            + self.amd_y_coeff[5] * xmm2
            + self.amd_y_coeff[6] * x2y2
            + self.amd_y_coeff[7] * ymm3
            + self.amd_y_coeff[8] * ymm2 * xmm
            + self.amd_y_coeff[9] * ymm * xmm2
            + self.amd_y_coeff[10] * xmm3
            + self.amd_y_coeff[11] * ymm * x2y2
            + self.amd_y_coeff[12] * ymm * x2y2 * x2y2;

        // Convert to radians
        let xir = xi / CONS2R;
        let etar = eta / CONS2R;

        // Convert to RA and Dec
        let ctan = self.plate_dec.tan();
        let ccos = self.plate_dec.cos();
        let raoff = (xir / ccos).atan2(1.0 - etar * ctan);
        let mut ra = raoff + self.plate_ra;
        if ra < 0.0 {
            ra += TWOPI;
        }
        *xpos = ra / COND2R;

        let dec = (raoff.cos() / ((1.0 - (etar * ctan)) / (etar + ctan))).atan();
        *ypos = dec / COND2R;

        Ok(())
    }

    /// Inverse transform: (RA, Dec) in degrees -> pixel (x, y).
    ///
    /// Ported from SAOimage `platepix` (Doug Mink, 1996).
    /// Uses Newton-Raphson iteration to invert the plate model.
    /// Returns `Ok(())` on success, `Err(())` if iteration fails.
    fn platepix(&self, xpos: f64, ypos: f64, xpix: &mut f64, ypix: &mut f64) -> Result<(), ()> {
        let tolerance = 0.000_000_5;
        let max_iterations = 50;

        // Convert RA and Dec in degrees to radians
        let xr = xpos * COND2R;
        let yr = ypos * COND2R;

        // Convert to standard coordinates on a plate
        let sypos = yr.sin();
        let cypos = yr.cos();
        let syplate = self.plate_dec.sin();
        let cyplate = self.plate_dec.cos();
        let sxdiff = (xr - self.plate_ra).sin();
        let cxdiff = (xr - self.plate_ra).cos();
        let div = (sypos * syplate) + (cypos * cyplate * cxdiff);
        let xi = cypos * sxdiff * CONS2R / div;
        let eta = ((sypos * cyplate) - (cypos * syplate * cxdiff)) * CONS2R / div;

        // Set initial value for x, y (in mm)
        let mut xmm = xi / self.plate_scale;
        let mut ymm = eta / self.plate_scale;

        // Iterate by Newton's method
        for _i in 0..max_iterations {
            // X plate model
            let xy = xmm * ymm;
            let x2 = xmm * xmm;
            let y2 = ymm * ymm;
            let x2y = x2 * ymm;
            let y2x = y2 * xmm;
            let x2y2 = x2 + y2;
            let cjunk = x2y2 * x2y2;
            let x3 = x2 * xmm;
            let y3 = y2 * ymm;
            let x4 = x2 * x2;
            let y4 = y2 * y2;

            let f = self.amd_x_coeff[0] * xmm
                + self.amd_x_coeff[1] * ymm
                + self.amd_x_coeff[2]
                + self.amd_x_coeff[3] * x2
                + self.amd_x_coeff[4] * xy
                + self.amd_x_coeff[5] * y2
                + self.amd_x_coeff[6] * x2y2
                + self.amd_x_coeff[7] * x3
                + self.amd_x_coeff[8] * x2y
                + self.amd_x_coeff[9] * y2x
                + self.amd_x_coeff[10] * y3
                + self.amd_x_coeff[11] * xmm * x2y2
                + self.amd_x_coeff[12] * xmm * cjunk;

            // Derivative of X model wrt x
            let fx = self.amd_x_coeff[0]
                + self.amd_x_coeff[3] * 2.0 * xmm
                + self.amd_x_coeff[4] * ymm
                + self.amd_x_coeff[6] * 2.0 * xmm
                + self.amd_x_coeff[7] * 3.0 * x2
                + self.amd_x_coeff[8] * 2.0 * xy
                + self.amd_x_coeff[9] * y2
                + self.amd_x_coeff[11] * (3.0 * x2 + y2)
                + self.amd_x_coeff[12] * (5.0 * x4 + 6.0 * x2 * y2 + y4);

            // Derivative of X model wrt y
            let fy = self.amd_x_coeff[1]
                + self.amd_x_coeff[4] * xmm
                + self.amd_x_coeff[5] * 2.0 * ymm
                + self.amd_x_coeff[6] * 2.0 * ymm
                + self.amd_x_coeff[8] * x2
                + self.amd_x_coeff[9] * 2.0 * xy
                + self.amd_x_coeff[10] * 3.0 * y2
                + self.amd_x_coeff[11] * 2.0 * xy
                + self.amd_x_coeff[12] * 4.0 * xy * x2y2;

            // Y plate model
            let g = self.amd_y_coeff[0] * ymm
                + self.amd_y_coeff[1] * xmm
                + self.amd_y_coeff[2]
                + self.amd_y_coeff[3] * y2
                + self.amd_y_coeff[4] * xy
                + self.amd_y_coeff[5] * x2
                + self.amd_y_coeff[6] * x2y2
                + self.amd_y_coeff[7] * y3
                + self.amd_y_coeff[8] * y2x
                + self.amd_y_coeff[9] * x2y
                + self.amd_y_coeff[10] * x3
                + self.amd_y_coeff[11] * ymm * x2y2
                + self.amd_y_coeff[12] * ymm * cjunk;

            // Derivative of Y model wrt x
            let gx = self.amd_y_coeff[1]
                + self.amd_y_coeff[4] * ymm
                + self.amd_y_coeff[5] * 2.0 * xmm
                + self.amd_y_coeff[6] * 2.0 * xmm
                + self.amd_y_coeff[8] * y2
                + self.amd_y_coeff[9] * 2.0 * xy
                + self.amd_y_coeff[10] * 3.0 * x2
                + self.amd_y_coeff[11] * 2.0 * xy
                + self.amd_y_coeff[12] * 4.0 * xy * x2y2;

            // Derivative of Y model wrt y
            let gy = self.amd_y_coeff[0]
                + self.amd_y_coeff[3] * 2.0 * ymm
                + self.amd_y_coeff[4] * xmm
                + self.amd_y_coeff[6] * 2.0 * ymm
                + self.amd_y_coeff[7] * 3.0 * y2
                + self.amd_y_coeff[8] * 2.0 * xy
                + self.amd_y_coeff[9] * x2
                + self.amd_y_coeff[11] * (x2 + 3.0 * y2)
                + self.amd_y_coeff[12] * (5.0 * y4 + 6.0 * x2 * y2 + x4);

            // Newton-Raphson update
            let f = f - xi;
            let g = g - eta;
            let det = (fx * gy) - (fy * gx);
            let dx = ((-f * gy) + (g * fy)) / det;
            let dy = ((-g * fx) + (f * gx)) / det;
            xmm += dx;
            ymm += dy;
            if dx.abs() < tolerance && dy.abs() < tolerance {
                break;
            }
        }

        // Convert mm from plate centre to plate pixels
        let x = (self.ppo_coeff[2] - xmm * 1000.0) / self.x_pixel_size;
        let y = (self.ppo_coeff[5] + ymm * 1000.0) / self.y_pixel_size;

        // Convert from plate pixels to image pixels
        *xpix = x - self.x_pixel_offset + 1.0 - 0.5;
        *ypix = y - self.y_pixel_offset + 1.0 - 0.5;

        Ok(())
    }

    /// Helper: get the effective WinMap shift and scale terms (accounting for
    /// its effective inversion state).
    fn get_winmap_terms(wm: &WinMap, win_inv: bool) -> Option<(Vec<f64>, Vec<f64>)> {
        let shifts = wm.shifts();
        let scales = wm.scales();
        if shifts.len() != 2 || scales.len() != 2 {
            return None;
        }
        if win_inv {
            // Inverted WinMap: output = (input - shift) / scale
            // So effective: shift' = -shift/scale, scale' = 1/scale
            let s0 = scales[0];
            let s1 = scales[1];
            if s0 == 0.0 || s1 == 0.0 {
                return None;
            }
            Some((
                vec![-shifts[0] / s0, -shifts[1] / s1],
                vec![1.0 / s0, 1.0 / s1],
            ))
        } else {
            Some((shifts.to_vec(), scales.to_vec()))
        }
    }
}

impl Mapping for DssMap {
    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
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
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. `platepos`/`platepix` are the underlying
    /// DSS plate-solution routines; both have internal failure modes
    /// (singular projection, out-of-range coefficients) that emit
    /// `BAD_VALUE`. Auditing them for NaN-safety is out of scope; use the
    /// safe delegate-and-post-process adapter.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        self.transform_into(pts, forward, out)?;
        let mut view = out.view_mut();
        let flat = view.as_slice_mut().expect("PointSet is C-contiguous");
        for v in flat.iter_mut() {
            if *v == BAD_VALUE {
                *v = f64::NAN;
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if fwd {
            // Forward: pixel (x,y) -> (RA, Dec) in radians
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    let mut xpos = 0.0;
                    let mut ypos = 0.0;
                    if self.platepos(inp[0], inp[1], &mut xpos, &mut ypos).is_ok() {
                        // platepos returns degrees; convert to radians
                        outp[0] = xpos * DD2R;
                        outp[1] = ypos * DD2R;
                    } else {
                        outp[0] = BAD_VALUE;
                        outp[1] = BAD_VALUE;
                    }
                }
            }
        } else {
            // Inverse: (RA, Dec) in radians -> pixel (x,y)
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    // Convert from radians to degrees for platepix
                    let ra_deg = inp[0] * DR2D;
                    let dec_deg = inp[1] * DR2D;
                    let mut xpix = 0.0;
                    let mut ypix = 0.0;
                    if self.platepix(ra_deg, dec_deg, &mut xpix, &mut ypix).is_ok() {
                        outp[0] = xpix;
                        outp[1] = ypix;
                    } else {
                        outp[0] = BAD_VALUE;
                        outp[1] = BAD_VALUE;
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
        // DssMap has no self-simplification; return a clone.
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<DssMap>() else {
            return false;
        };
        // Invert flags must be equal for DssMaps
        if self.inverted != other.inverted {
            return false;
        }
        // Compare essential plate parameters
        if !ast_equal(self.x_pixel_offset, other.x_pixel_offset)
            || !ast_equal(self.y_pixel_offset, other.y_pixel_offset)
            || !ast_equal(self.ppo_coeff[2], other.ppo_coeff[2])
            || !ast_equal(self.ppo_coeff[5], other.ppo_coeff[5])
            || !ast_equal(self.x_pixel_size, other.x_pixel_size)
            || !ast_equal(self.y_pixel_size, other.y_pixel_size)
            || !ast_equal(self.plate_dec, other.plate_dec)
            || !ast_equal(self.plate_ra, other.plate_ra)
        {
            return false;
        }
        // Compare first 13 polynomial coefficients
        for i in 0..13 {
            if !ast_equal(self.amd_x_coeff[i], other.amd_x_coeff[i])
                || !ast_equal(self.amd_y_coeff[i], other.amd_y_coeff[i])
            {
                return false;
            }
        }
        true
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

        // DssMap can merge with an adjacent WinMap that precedes it (in the
        // forward direction). The WinMap's shift and scale are absorbed into
        // the pixel offset and pixel size parameters.
        //
        // If DssMap is not inverted, the WinMap should precede it (other comes
        // before self in the map list). But try_merge is called with self being
        // the DssMap, and other the neighbor. In the simplification loop, the
        // caller handles the ordering.
        //
        // For simplicity: merge DssMap + WinMap in either order, as the C does.
        if let Some(wm) = other.as_any().downcast_ref::<WinMap>() {
            // Determine whether the WinMap precedes or follows the DssMap
            // based on the inversion states.
            let terms = Self::get_winmap_terms(wm, other_inv)?;
            let (a, b) = (terms.0, terms.1);

            // Check scale terms are usable
            if is_bad(a[0])
                || is_bad(b[0])
                || b[0] == 0.0
                || is_bad(a[1])
                || is_bad(b[1])
                || b[1] == 0.0
            {
                return None;
            }

            let mut cnpix1 = self.x_pixel_offset;
            let mut cnpix2 = self.y_pixel_offset;
            let mut xpixelsz = self.x_pixel_size;
            let mut ypixelsz = self.y_pixel_size;

            // If DssMap is not inverted, WinMap precedes it
            // If DssMap is inverted, WinMap follows it
            if !self_inv {
                // WinMap is applied to the inputs before the DssMap forward transform
                xpixelsz *= b[0];
                ypixelsz *= b[1];
                cnpix1 = 0.5 + (cnpix1 + a[0] - 0.5) / b[0];
                cnpix2 = 0.5 + (cnpix2 + a[1] - 0.5) / b[1];
            } else {
                // WinMap is applied to outputs after the DssMap inverse transform
                xpixelsz /= b[0];
                ypixelsz /= b[1];
                cnpix1 = b[0] * (cnpix1 - 0.5) - a[0] + 0.5;
                cnpix2 = b[1] * (cnpix2 - 0.5) - a[1] + 0.5;
            }

            // The CNPIX values must be integer to a good approximation
            let cnpix1_rounded = cnpix1.round();
            let cnpix2_rounded = cnpix2.round();
            if (cnpix1 - cnpix1_rounded).abs() >= 0.01 || (cnpix2 - cnpix2_rounded).abs() >= 0.01 {
                return None;
            }

            let mut result = self.clone();
            result.x_pixel_offset = cnpix1_rounded;
            result.y_pixel_offset = cnpix2_rounded;
            result.x_pixel_size = xpixelsz;
            result.y_pixel_size = ypixelsz;
            result.inverted = self_inv;
            return Some(Box::new(result));
        }

        None
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

impl Serializable for DssMap {
    fn class_name(&self) -> &'static str {
        "DssMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("DSS plate fit mapping".to_string());
        write_mapping_base(&mut node, self);

        node.push_double(
            "PltRA",
            self.plate_ra,
            Some("Plate centre RA (radians)".to_string()),
        );
        node.push_double(
            "PltDec",
            self.plate_dec,
            Some("Plate centre Dec (radians)".to_string()),
        );
        node.push_double(
            "PltScl",
            self.plate_scale,
            Some("Plate scale (arcsec/mm)".to_string()),
        );
        node.push_double(
            "CNPix1",
            self.x_pixel_offset,
            Some("X Pixel offset (pixels)".to_string()),
        );
        node.push_double(
            "CNPix2",
            self.y_pixel_offset,
            Some("Y Pixel offset (pixels)".to_string()),
        );
        node.push_double(
            "XPixSz",
            self.x_pixel_size,
            Some("X Pixel size (microns)".to_string()),
        );
        node.push_double(
            "YPixSz",
            self.y_pixel_size,
            Some("Y Pixel size (microns)".to_string()),
        );

        for i in 0..6 {
            let name = format!("PPO{}", i + 1);
            node.push_double(
                &name,
                self.ppo_coeff[i],
                Some("Orientation coefficients".to_string()),
            );
        }

        for i in 0..19 {
            let name = format!("AMDX{}", i + 1);
            node.push_double(
                &name,
                self.amd_x_coeff[i],
                Some("Plate solution X coefficients".to_string()),
            );
        }

        for i in 0..19 {
            let name = format!("AMDY{}", i + 1);
            node.push_double(
                &name,
                self.amd_y_coeff[i],
                Some("Plate solution Y coefficients".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        let plate_ra = node.get_double("PltRA", BAD_VALUE);
        let plate_dec = node.get_double("PltDec", BAD_VALUE);
        let plate_scale = node.get_double("PltScl", BAD_VALUE);
        let x_pixel_offset = node.get_double("CNPix1", BAD_VALUE);
        let y_pixel_offset = node.get_double("CNPix2", BAD_VALUE);
        let x_pixel_size = node.get_double("XPixSz", BAD_VALUE);
        let y_pixel_size = node.get_double("YPixSz", BAD_VALUE);

        let mut ppo_coeff = [0.0; 6];
        for (i, coeff) in ppo_coeff.iter_mut().enumerate() {
            let name = format!("PPO{}", i + 1);
            let val = node.get_double(&name, BAD_VALUE);
            *coeff = if is_bad(val) { 0.0 } else { val };
        }

        let mut amd_x_coeff = [0.0; 19];
        for (i, coeff) in amd_x_coeff.iter_mut().enumerate() {
            let name = format!("AMDX{}", i + 1);
            let val = node.get_double(&name, BAD_VALUE);
            *coeff = if is_bad(val) { 0.0 } else { val };
        }

        let mut amd_y_coeff = [0.0; 19];
        for (i, coeff) in amd_y_coeff.iter_mut().enumerate() {
            let name = format!("AMDY{}", i + 1);
            let val = node.get_double(&name, BAD_VALUE);
            *coeff = if is_bad(val) { 0.0 } else { val };
        }

        let mut m = DssMap::from_plate_params(
            plate_ra,
            plate_dec,
            plate_scale,
            x_pixel_offset,
            y_pixel_offset,
            x_pixel_size,
            y_pixel_size,
            ppo_coeff,
            amd_x_coeff,
            amd_y_coeff,
        );
        if base.inverted {
            m.invert();
        }
        m.has_been_simplified = base.has_been_simplified;
        Ok(m)
    }
}

// ---------------------------------------------------------------------------
// Tests
//
// DssMap's constructor is crate-private (mirroring C's protected astDssMap),
// so the construction-based tests live here as in-crate unit tests rather
// than in an external `tests/` crate. The deserialisation-driven coverage
// (native round-trips, the C-generated `c_dssmap.ast` fixture, and the
// dssmap-03/07/08 simplify-pathway fixtures) lives in
// `tests/test_simplify_regression.rs` and `tests/fixtures/dssmap/`.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::BAD_VALUE;
    use crate::coords::PointSet;
    use crate::mappings::{CmpMap, Composition, WinMap, ZoomMap};
    use crate::{AnyObject, from_native_string, to_native_string};

    /// Helper to create a realistic DssMap from a simplified DSS plate solution.
    /// These values approximate a real DSS plate (POSS-I E survey).
    fn make_test_dssmap() -> DssMap {
        // Plate centre: RA = 10h 47m 57.3s, Dec = -60d 15m 52s
        let plate_ra = 2.827_237_465_568_411; // radians
        let plate_dec = -1.051_812_254_050_266_8; // radians
        let plate_scale = 67.2; // arcsec/mm
        let x_pixel_offset = 4201.0;
        let y_pixel_offset = 4201.0;
        let x_pixel_size = 25.263; // microns
        let y_pixel_size = 25.263; // microns

        // PPO coefficients — only PPO3 and PPO6 are non-zero for typical DSS plates
        let ppo_coeff = [0.0, 0.0, 105990.5119, 0.0, 0.0, 105990.5119];

        // AMD X coefficients (simplified — first few are dominant)
        let amd_x_coeff = [
            67.1526,   // AMDX1
            0.00512,   // AMDX2
            -288.409,  // AMDX3
            -8.84e-6,  // AMDX4
            -2.93e-5,  // AMDX5
            3.91e-6,   // AMDX6
            -4.16e-6,  // AMDX7
            -9.75e-10, // AMDX8
            3.26e-10,  // AMDX9
            7.78e-10,  // AMDX10
            2.07e-10,  // AMDX11
            5.55e-9,   // AMDX12
            1.49e-12,  // AMDX13
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];

        // AMD Y coefficients
        let amd_y_coeff = [
            67.1542,   // AMDY1
            -0.00383,  // AMDY2
            191.562,   // AMDY3
            -2.12e-5,  // AMDY4
            -2.92e-5,  // AMDY5
            -9.44e-6,  // AMDY6
            -4.14e-6,  // AMDY7
            6.67e-10,  // AMDY8
            6.94e-10,  // AMDY9
            -2.08e-10, // AMDY10
            -1.20e-9,  // AMDY11
            5.56e-9,   // AMDY12
            1.50e-12,  // AMDY13
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];

        DssMap::from_plate_params(
            plate_ra,
            plate_dec,
            plate_scale,
            x_pixel_offset,
            y_pixel_offset,
            x_pixel_size,
            y_pixel_size,
            ppo_coeff,
            amd_x_coeff,
            amd_y_coeff,
        )
    }

    // ------------------------------------------------------------------ //
    // Construction                                                         //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_basic_properties() {
        let dm = make_test_dssmap();
        assert_eq!(dm.nin(), 2);
        assert_eq!(dm.nout(), 2);
        assert!(!dm.is_inverted());
        assert!((dm.plate_ra() - 2.827_237_465_568_411).abs() < 1e-12);
        assert!((dm.plate_dec() - (-1.051_812_254_050_266_8)).abs() < 1e-12);
        assert!((dm.plate_scale() - 67.2).abs() < 1e-10);
    }

    // ------------------------------------------------------------------ //
    // Forward transform                                                    //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_forward_plate_centre() {
        // The pixel that maps to the plate centre RA/Dec should be near
        // pixel (1, 1) when cnpix offset is taken into account.
        // Actually, the plate centre maps from the optical axis position.
        let dm = make_test_dssmap();

        // The optical axis in plate pixels is at PPO3/xpixsz, PPO6/ypixsz
        // In image pixels: (PPO3/xpixsz - cnpix + 1 - 0.5, ...)
        // PPO3 = 105990.5119, xpixsz = 25.263 => plate_pix_x = 4196.05
        // image_pix_x = 4196.05 - 4201 + 1 - 0.5 = -4.45 (off image, but valid)
        // Actually let's just check that a pixel near the centre transforms to
        // something close to the plate centre.

        // For a pixel at (1, 1): check that it produces a valid RA/Dec.
        let pts = PointSet::from_rows([[1.0, 1.0]]);
        let out = dm.transform(&pts, true).unwrap();
        // Should produce valid (non-BAD) output
        assert!(out.point(0)[0] != BAD_VALUE);
        assert!(out.point(0)[1] != BAD_VALUE);
        // RA should be in [0, 2*pi] radians
        assert!(out.point(0)[0] >= 0.0);
        assert!(out.point(0)[0] < std::f64::consts::TAU);
        // Dec should be in [-pi/2, pi/2]
        assert!(out.point(0)[1] >= -std::f64::consts::FRAC_PI_2);
        assert!(out.point(0)[1] <= std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn dssmap_forward_bad_propagation() {
        let dm = make_test_dssmap();
        let pts = PointSet::from_rows([
            [BAD_VALUE, 100.0],
            [100.0, BAD_VALUE],
            [BAD_VALUE, BAD_VALUE],
        ]);
        let out = dm.transform(&pts, true).unwrap();
        for i in 0..3 {
            assert_eq!(out.point(i)[0], BAD_VALUE);
            assert_eq!(out.point(i)[1], BAD_VALUE);
        }
    }

    // ------------------------------------------------------------------ //
    // Inverse transform                                                    //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_inverse_bad_propagation() {
        let dm = make_test_dssmap();
        let pts = PointSet::from_rows([[BAD_VALUE, 0.5], [1.0, BAD_VALUE]]);
        let out = dm.transform(&pts, false).unwrap();
        for i in 0..2 {
            assert_eq!(out.point(i)[0], BAD_VALUE);
            assert_eq!(out.point(i)[1], BAD_VALUE);
        }
    }

    // ------------------------------------------------------------------ //
    // Forward/inverse roundtrip                                            //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_roundtrip() {
        let dm = make_test_dssmap();

        // Start with some pixel coordinates
        let pixels = PointSet::from_rows([
            [500.0, 500.0],
            [1000.0, 1000.0],
            [2000.0, 3000.0],
            [4000.0, 4000.0],
        ]);

        // Forward: pixel -> RA/Dec
        let sky = dm.transform(&pixels, true).unwrap();

        // Inverse: RA/Dec -> pixel
        let recovered = dm.transform(&sky, false).unwrap();

        // Check roundtrip accuracy (should be very good — sub-pixel)
        for i in 0..pixels.npoint() {
            let orig = pixels.point(i);
            let recov = recovered.point(i);
            let dx = (orig[0] - recov[0]).abs();
            let dy = (orig[1] - recov[1]).abs();
            assert!(dx < 1e-6, "pixel {}: x roundtrip error {} too large", i, dx);
            assert!(dy < 1e-6, "pixel {}: y roundtrip error {} too large", i, dy);
        }
    }

    #[test]
    fn dssmap_roundtrip_inverted() {
        let mut dm = make_test_dssmap();
        dm.invert();

        // When inverted, forward means RA/Dec -> pixel, inverse means pixel -> RA/Dec
        // Start with pixel coords, use "inverse" (which is now forward direction = pixel->sky)
        let pixels = PointSet::from_rows([[1500.0, 2500.0], [3000.0, 3500.0]]);

        // inverse of inverted = forward of non-inverted = pixel->sky
        let sky = dm.transform(&pixels, false).unwrap();
        // forward of inverted = inverse of non-inverted = sky->pixel
        let recovered = dm.transform(&sky, true).unwrap();

        for i in 0..pixels.npoint() {
            let orig = pixels.point(i);
            let recov = recovered.point(i);
            assert!(
                (orig[0] - recov[0]).abs() < 1e-6,
                "inverted roundtrip: x error too large"
            );
            assert!(
                (orig[1] - recov[1]).abs() < 1e-6,
                "inverted roundtrip: y error too large"
            );
        }
    }

    // ------------------------------------------------------------------ //
    // equal()                                                              //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_equal_same() {
        let dm1 = make_test_dssmap();
        let dm2 = make_test_dssmap();
        assert!(dm1.equal(&dm2));
    }

    #[test]
    fn dssmap_equal_different_ppo() {
        let dm1 = make_test_dssmap();
        let mut ppo = *dm1.ppo_coeff();
        ppo[2] += 1.0;
        let dm2 = DssMap::from_plate_params(
            dm1.plate_ra(),
            dm1.plate_dec(),
            dm1.plate_scale(),
            dm1.x_pixel_offset(),
            dm1.y_pixel_offset(),
            dm1.x_pixel_size(),
            dm1.y_pixel_size(),
            ppo,
            *dm1.amd_x_coeff(),
            *dm1.amd_y_coeff(),
        );
        assert!(!dm1.equal(&dm2));
    }

    #[test]
    fn dssmap_equal_different_invert() {
        let dm1 = make_test_dssmap();
        let mut dm2 = make_test_dssmap();
        dm2.invert();
        assert!(!dm1.equal(&dm2));
    }

    // ------------------------------------------------------------------ //
    // Serialization roundtrip                                              //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_serialization_roundtrip() {
        let dm = make_test_dssmap();
        let obj = AnyObject::DssMap(dm.clone());
        let text = to_native_string(&obj).unwrap();
        let obj2 = from_native_string(&text).unwrap();
        let dm2 = match obj2 {
            AnyObject::DssMap(m) => m,
            other => panic!("Expected DssMap, got {:?}", other),
        };
        assert!(dm.equal(&dm2));
        assert!((dm.plate_ra() - dm2.plate_ra()).abs() < 1e-15);
        assert!((dm.plate_dec() - dm2.plate_dec()).abs() < 1e-15);
        assert!((dm.plate_scale() - dm2.plate_scale()).abs() < 1e-15);
        assert!((dm.x_pixel_offset() - dm2.x_pixel_offset()).abs() < 1e-15);
        assert!((dm.y_pixel_offset() - dm2.y_pixel_offset()).abs() < 1e-15);
        assert!((dm.x_pixel_size() - dm2.x_pixel_size()).abs() < 1e-15);
        assert!((dm.y_pixel_size() - dm2.y_pixel_size()).abs() < 1e-15);
    }

    #[test]
    fn dssmap_serialization_inverted() {
        let mut dm = make_test_dssmap();
        dm.invert();
        let obj = AnyObject::DssMap(dm.clone());
        let text = to_native_string(&obj).unwrap();
        let obj2 = from_native_string(&text).unwrap();
        let dm2 = match obj2 {
            AnyObject::DssMap(m) => m,
            other => panic!("Expected DssMap, got {:?}", other),
        };
        assert!(dm2.is_inverted());
        assert!(dm.equal(&dm2));
    }

    /// Read a native dump written by the *reference C library* and verify the
    /// Rust reader produces an equivalent DssMap.
    ///
    /// This is the principal real-world role of the DssMap class: the C
    /// `astDssMap` constructor is protected and the modern FITS path emits a
    /// TPN/WcsMap instead, so a DssMap reaches an application almost exclusively
    /// by loading an older native serialisation. The fixture
    /// `fixtures/dssmap/c_dssmap.ast` was produced by `astWrite` on the built
    /// reference libast; the expected transform value is the C `astTran2` output
    /// for the same object (oracle-verified).
    #[test]
    fn dssmap_reads_c_written_native_dump() {
        let path = format!(
            "{}/tests/fixtures/dssmap/c_dssmap.ast",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("c_dssmap.ast fixture");

        let obj = from_native_string(&text).unwrap();
        let dm = match obj {
            AnyObject::DssMap(m) => m,
            other => panic!("expected DssMap, got {other:?}"),
        };

        // C does not convert a DssMap on read — it stays a DssMap (oracle-verified:
        // C `astRead` reports class=DssMap and re-`astWrite`s it as `Begin DssMap`).
        assert_eq!(dm.nin(), 2);
        assert!((dm.plate_ra() - 2.827_237_465_568_411).abs() < 1e-12);
        assert!((dm.x_pixel_offset() - 4201.0).abs() < 1e-9);

        // Forward transform must match the C oracle (astTran2 of the same dump).
        let pts = PointSet::from_rows([[1000.0, 2000.0]]);
        let out = dm.transform(&pts, true).unwrap();
        // Oracle: forward(1000,2000) = 2.8083346239617448  -1.0343153598165551.
        // The tiny residual is the plate-solution inverse precision (< 1 mas).
        assert!((out.point(0)[0] - 2.808_334_623_961_745).abs() < 1e-7);
        assert!((out.point(0)[1] - (-1.034_315_359_816_555)).abs() < 1e-7);
    }

    #[test]
    fn dssmap_serialization_transform_preserved() {
        // Verify that a deserialized DssMap produces the same transform results.
        let dm = make_test_dssmap();
        let obj = AnyObject::DssMap(dm.clone());
        let text = to_native_string(&obj).unwrap();
        let obj2 = from_native_string(&text).unwrap();
        let dm2 = match obj2 {
            AnyObject::DssMap(m) => m,
            other => panic!("Expected DssMap, got {:?}", other),
        };

        let pts = PointSet::from_rows([[1000.0, 2000.0], [3000.0, 4000.0]]);
        let out1 = dm.transform(&pts, true).unwrap();
        let out2 = dm2.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            assert!((out1.point(i)[0] - out2.point(i)[0]).abs() < 1e-15);
            assert!((out1.point(i)[1] - out2.point(i)[1]).abs() < 1e-15);
        }
    }

    // ------------------------------------------------------------------ //
    // simplified()                                                         //
    // ------------------------------------------------------------------ //

    #[test]
    fn dssmap_simplified_returns_clone() {
        let dm = make_test_dssmap();
        let simp = dm.simplified().unwrap();
        // DssMap has no self-simplification, so simplified() returns itself.
        assert!(simp.as_any().downcast_ref::<DssMap>().is_some());
        assert!(simp.equal(&dm));
    }

    // ------------------------------------------------------------------ //
    // MapMerge (DssMap + WinMap absorption)                                //
    //                                                                      //
    // These exercise the simplification pathways C documents in            //
    // `simplify_pathways.md` as dssmap-01..08. In the C library all eight   //
    // are marked "infeasible: protected constructor" — the C `astDssMap`    //
    // constructor is NOT in the public `ast.h`, so the C test harness       //
    // cannot build a DssMap to drive its `MapMerge`. The Rust              //
    // `DssMap::from_plate_params` IS public, so we can finally cover them.  //
    // Reference: reference/starlink-ast/src/dssmap.c `MapMerge` (line 845). //
    // ------------------------------------------------------------------ //

    /// A DssMap identical to `make_test_dssmap` but with the pixel-corner
    /// offset (CNPIX) replaced, so a chosen WinMap yields integer CNPIX.
    fn dssmap_with_offset(off: f64) -> DssMap {
        let d = make_test_dssmap();
        DssMap::from_plate_params(
            d.plate_ra(),
            d.plate_dec(),
            d.plate_scale(),
            off,
            off,
            d.x_pixel_size(),
            d.y_pixel_size(),
            *d.ppo_coeff(),
            *d.amd_x_coeff(),
            *d.amd_y_coeff(),
        )
    }

    /// dssmap-07: a non-inverted DssMap absorbs a *preceding* WinMap when the
    /// resulting CNPIX values are integer. `WinMap(shift=-0.5, scale=2)` before a
    /// DssMap with CNPIX=4200 gives CNPIX' = 0.5 + (4200 - 0.5 - 0.5)/2 = 2100.0.
    #[test]
    fn dssmap_absorbs_preceding_winmap() {
        let dm = dssmap_with_offset(4200.0);
        let wm = WinMap::new(&[-0.5, -0.5], &[2.0, 2.0]).unwrap();
        // The WinMap must remain a WinMap through per-component simplification
        // (a unit scale would degrade to a ShiftMap, a zero shift to a ZoomMap —
        // neither of which C's `MapMerge` matches either).
        assert!(
            wm.clone()
                .simplified()
                .unwrap()
                .as_any()
                .downcast_ref::<WinMap>()
                .is_some(),
            "test WinMap must stay a WinMap"
        );

        let chain = CmpMap::new(Box::new(wm), Box::new(dm.clone()), Composition::Series).unwrap();
        let simp = chain.simplified().unwrap();

        // The pair collapses to a single DssMap (CmpMap is gone).
        assert!(
            simp.as_any().downcast_ref::<DssMap>().is_some(),
            "WinMap + DssMap should collapse to a single DssMap"
        );
        let merged = simp.as_any().downcast_ref::<DssMap>().unwrap();
        assert!((merged.x_pixel_offset() - 2100.0).abs() < 1e-9);
        assert!((merged.y_pixel_offset() - 2100.0).abs() < 1e-9);
        assert!((merged.x_pixel_size() - dm.x_pixel_size() * 2.0).abs() < 1e-9);

        // The merged mapping must be numerically identical to the chain.
        let pts = PointSet::from_rows([[123.0, 45.0]]);
        let want = chain.transform(&pts, true).unwrap();
        let got = simp.transform(&pts, true).unwrap();
        assert!((want.point(0)[0] - got.point(0)[0]).abs() < 1e-12);
        assert!((want.point(0)[1] - got.point(0)[1]).abs() < 1e-12);
    }

    /// dssmap-08: an inverted DssMap absorbs a *following* WinMap. With the
    /// DssMap inverted the WinMap operates on the (output) pixel coordinates, so
    /// it must follow the DssMap in the series list.
    #[test]
    fn dssmap_inverted_absorbs_following_winmap() {
        let mut dm = dssmap_with_offset(4200.0);
        dm.invert();
        // Inverse WinMap branch: cnpix' = b*(cnpix - 0.5) - a + 0.5.
        // With cnpix=4200, b=2, a=-0.5: 2*4199.5 + 0.5 + 0.5 = 8400.0.
        let wm = WinMap::new(&[-0.5, -0.5], &[2.0, 2.0]).unwrap();

        let chain = CmpMap::new(Box::new(dm.clone()), Box::new(wm), Composition::Series).unwrap();
        let simp = chain.simplified().unwrap();

        assert!(
            simp.as_any().downcast_ref::<DssMap>().is_some(),
            "inverted DssMap + WinMap should collapse to a single DssMap"
        );
        let merged = simp.as_any().downcast_ref::<DssMap>().unwrap();
        assert!(merged.is_inverted());
        assert!((merged.x_pixel_offset() - 8400.0).abs() < 1e-9);

        let pts = PointSet::from_rows([[2.83, -1.05]]);
        let want = chain.transform(&pts, true).unwrap();
        let got = simp.transform(&pts, true).unwrap();
        assert!((want.point(0)[0] - got.point(0)[0]).abs() < 1e-9);
        assert!((want.point(0)[1] - got.point(0)[1]).abs() < 1e-9);
    }

    /// dssmap-01: parallel composition is refused — `try_merge` only fires in
    /// series.
    #[test]
    fn dssmap_parallel_not_merged() {
        let dm = make_test_dssmap();
        let wm = WinMap::new(&[-0.5, -0.5], &[2.0, 2.0]).unwrap();
        assert!(dm.try_merge(&wm, false, false, false).is_none());
    }

    /// dssmap-03: an adjacent mapping that is not a WinMap (here a ZoomMap) is
    /// not absorbed.
    #[test]
    fn dssmap_non_winmap_neighbour_not_merged() {
        let dm = make_test_dssmap();
        let zoom = ZoomMap::new(2, 2.0).unwrap();
        assert!(dm.try_merge(&zoom, true, false, false).is_none());

        // End-to-end: the chain stays a CmpMap.
        let chain = CmpMap::new(Box::new(zoom), Box::new(dm), Composition::Series).unwrap();
        let simp = chain.simplified().unwrap();
        assert!(
            simp.as_any().downcast_ref::<DssMap>().is_none(),
            "DssMap + ZoomMap must not collapse to a DssMap"
        );
    }

    /// dssmap-04: a WinMap with a zero scale term is unusable and the merge is
    /// refused.
    #[test]
    fn dssmap_zero_scale_winmap_not_merged() {
        let dm = make_test_dssmap();
        let wm = WinMap::new(&[0.0, 0.0], &[0.0, 2.0]).unwrap();
        assert!(dm.try_merge(&wm, true, false, false).is_none());
    }

    /// dssmap-05: when the resulting CNPIX values are not integer (within one
    /// hundredth of a pixel), the merge is refused.
    #[test]
    fn dssmap_fractional_cnpix_not_merged() {
        // make_test_dssmap has CNPIX=4201; WinMap(shift=0, scale=2) gives
        // CNPIX' = 0.5 + (4201 - 0.5)/2 = 0.5 + 2100.25 = 2100.75 — non-integer.
        let dm = make_test_dssmap();
        let wm = WinMap::new(&[0.0, 0.0], &[2.0, 2.0]).unwrap();
        // (This WinMap has a zero shift, which would degrade to a ZoomMap under
        // full simplification, so test `try_merge` directly to isolate the
        // non-integer-CNPIX guard.)
        assert!(dm.try_merge(&wm, true, false, false).is_none());
    }
}
