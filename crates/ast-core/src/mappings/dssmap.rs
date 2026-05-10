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

/// Radians per degree.
const COND2R: f64 = 1.745329252e-2;

/// Arcseconds per radian.
const CONS2R: f64 = 206264.8062470964;

/// 2 * pi.
const TWOPI: f64 = std::f64::consts::TAU;

/// A 2-D pixel to equatorial sky mapping via a DSS plate solution.
///
/// The forward transformation converts pixel (x, y) to (RA, Dec) in
/// radians.  The inverse converts (RA, Dec) in radians to pixel (x, y).
///
/// This mapping cannot be created directly via a simple constructor -- it is
/// produced either by deserialisation from a native AST dump, or by reading
/// a DSS-encoded FitsChan (not yet implemented in the Rust API).
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
    /// Do not simplify flag.
    do_not_simplify: bool,
}

impl DssMap {
    /// Create a new `DssMap` from explicit plate solution parameters.
    ///
    /// This is the direct constructor used for deserialization and testing.
    /// All angular quantities in radians except `plate_scale` (arcsec/mm).
    #[allow(clippy::too_many_arguments)]
    pub fn from_plate_params(
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
            do_not_simplify: false,
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
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
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
                        outp[0] = xpos * COND2R;
                        outp[1] = ypos * COND2R;
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
                    let ra_deg = inp[0] / COND2R;
                    let dec_deg = inp[1] / COND2R;
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
        if self.do_not_simplify {
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
        m.do_not_simplify = base.do_not_simplify;
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}
