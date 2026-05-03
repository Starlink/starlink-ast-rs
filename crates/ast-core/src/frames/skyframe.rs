//! The celestial coordinate frame (SkyFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::sky_convert::{SkyRefIs, SkySystem, make_sky_mapping};
use crate::frames::skyaxis::{SkyAxis, format_sky, unformat_sky};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::is_bad;
use crate::mappings::{DynMapping, UnitMap};
use crate::pal;

/// A celestial coordinate frame.
///
/// `SkyFrame` represents a coordinate frame on the celestial sphere.
/// It supports a range of coordinate systems (FK4, FK5, ICRS, Galactic,
/// etc.) and provides spherical geometry for distance, angle, offset,
/// and normalisation operations.
///
/// All angles are in radians. Axis indices are 1-based (axis 1 =
/// longitude, axis 2 = latitude).
///
/// Construct via [`SkyFrame::new`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, SkyFrame, SkySystem};
///
/// let sky = SkyFrame::new(SkySystem::Fk5);
/// assert_eq!(sky.naxes(), 2);
/// assert_eq!(sky.system(), SkySystem::Fk5);
/// ```
#[derive(Debug, Clone)]
pub struct SkyFrame {
    // Frame-level
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>, // MJD

    // Observer (Frame trait in C)
    obs_lat: Option<f64>, // geodetic latitude (radians)
    obs_lon: Option<f64>, // geodetic longitude (radians)
    obs_alt: Option<f64>, // altitude (metres)
    dtai: Option<f64>,    // TAI-UTC (seconds)
    dut1: Option<f64>,    // UT1-UTC (seconds)

    // SkyFrame-specific
    system: SkySystem,
    equinox: Option<f64>, // MJD of mean equinox
    projection: Option<String>,
    neglon: bool,
    sky_tol: Option<f64>, // smallest significant distance (radians)
    sky_ref: [f64; 2],    // offset origin/pole [lon, lat] (radians)
    sky_ref_p: [f64; 2],  // offset meridian point [lon, lat]
    sky_ref_is: SkyRefIs,
    align_offset: bool,

    // Per-axis properties
    axes: [SkyAxis; 2], // [longitude, latitude]
}

impl SkyFrame {
    /// Create a new `SkyFrame` with the given coordinate system.
    ///
    /// The default domain is `"SKY"`, the default epoch is J2000.0
    /// (MJD 51544.5), and the default equinox is determined by the
    /// coordinate system.
    pub fn new(system: SkySystem) -> Self {
        Self {
            title: None,
            domain: Some("SKY".to_string()),
            epoch: None,
            obs_lat: None,
            obs_lon: None,
            obs_alt: None,
            dtai: None,
            dut1: None,
            system,
            equinox: None,
            projection: None,
            neglon: false,
            sky_tol: None,
            sky_ref: [0.0, 0.0],
            sky_ref_p: [0.0, std::f64::consts::FRAC_PI_2],
            sky_ref_is: SkyRefIs::Ignored,
            align_offset: false,
            axes: [SkyAxis::longitude(), SkyAxis::latitude()],
        }
    }

    // --- SkyFrame-specific attributes ---

    /// Return the celestial coordinate system.
    pub fn system(&self) -> SkySystem {
        self.system
    }

    /// Set the celestial coordinate system.
    pub fn set_system(&mut self, sys: SkySystem) {
        self.system = sys;
    }

    /// Return the equinox as MJD.
    ///
    /// If not explicitly set, returns the default for the current system.
    pub fn equinox_mjd(&self) -> f64 {
        self.equinox
            .unwrap_or_else(|| self.system.default_equinox_mjd())
    }

    /// Set the equinox as MJD.
    pub fn set_equinox(&mut self, mjd: f64) {
        self.equinox = Some(mjd);
    }

    /// Return the epoch as MJD.
    ///
    /// If not explicitly set, returns J2000.0 (MJD 51544.5).
    pub fn epoch_mjd(&self) -> f64 {
        self.epoch.unwrap_or(51544.5)
    }

    /// Set the epoch as MJD.
    pub fn set_epoch(&mut self, mjd: f64) {
        self.epoch = Some(mjd);
    }

    /// Return whether longitude increases in the negative direction.
    pub fn neglon(&self) -> bool {
        self.neglon
    }

    /// Set whether longitude increases in the negative direction.
    pub fn set_neglon(&mut self, neg: bool) {
        self.neglon = neg;
    }

    /// Return the sky reference point `[lon, lat]` in radians.
    pub fn sky_ref(&self) -> [f64; 2] {
        self.sky_ref
    }

    /// Set the sky reference point.
    pub fn set_sky_ref(&mut self, lon: f64, lat: f64) {
        self.sky_ref = [lon, lat];
    }

    /// Return the interpretation of the sky reference point.
    pub fn sky_ref_is(&self) -> SkyRefIs {
        self.sky_ref_is
    }

    /// Set the interpretation of the sky reference point.
    pub fn set_sky_ref_is(&mut self, mode: SkyRefIs) {
        self.sky_ref_is = mode;
    }

    /// Return the sky reference meridian point `[lon, lat]` in radians.
    pub fn sky_ref_p(&self) -> [f64; 2] {
        self.sky_ref_p
    }

    /// Set the sky reference meridian point.
    pub fn set_sky_ref_p(&mut self, lon: f64, lat: f64) {
        self.sky_ref_p = [lon, lat];
    }

    /// Return the sky tolerance in radians.
    ///
    /// If not explicitly set, returns 0.0.
    pub fn sky_tol(&self) -> f64 {
        self.sky_tol.unwrap_or(0.0)
    }

    /// Set the sky tolerance in radians.
    pub fn set_sky_tol(&mut self, tol: f64) {
        self.sky_tol = Some(tol);
    }

    /// Return whether alignment uses offset coordinates.
    pub fn align_offset(&self) -> bool {
        self.align_offset
    }

    /// Set whether alignment uses offset coordinates.
    pub fn set_align_offset(&mut self, ao: bool) {
        self.align_offset = ao;
    }

    /// Return the projection description, if set.
    pub fn projection(&self) -> Option<&str> {
        self.projection.as_deref()
    }

    /// Set the projection description.
    pub fn set_projection(&mut self, proj: &str) {
        self.projection = Some(proj.to_string());
    }

    // --- Frame-level attributes ---

    /// Return the frame title.
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }

    /// Set the frame title.
    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    /// Return the frame domain.
    pub fn domain(&self) -> &str {
        self.domain.as_deref().unwrap_or("SKY")
    }

    /// Set the frame domain.
    pub fn set_domain(&mut self, domain: &str) {
        self.domain = Some(domain.to_string());
    }

    // --- Observer attributes ---

    /// Set the observer geodetic latitude in radians.
    pub fn set_obs_lat(&mut self, lat: f64) {
        self.obs_lat = Some(lat);
    }

    /// Set the observer geodetic longitude in radians.
    pub fn set_obs_lon(&mut self, lon: f64) {
        self.obs_lon = Some(lon);
    }

    /// Set the observer altitude in metres.
    pub fn set_obs_alt(&mut self, alt: f64) {
        self.obs_alt = Some(alt);
    }

    /// Set TAI-UTC in seconds.
    pub fn set_dtai(&mut self, dtai: f64) {
        self.dtai = Some(dtai);
    }

    /// Set UT1-UTC in seconds.
    pub fn set_dut1(&mut self, dut1: f64) {
        self.dut1 = Some(dut1);
    }
}

impl Frame for SkyFrame {
    fn naxes(&self) -> usize {
        2
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..=2", axis),
            ));
        }
        let idx = axis - 1;
        let as_time = self.axes[idx].effective_as_time();
        // Default precision: 1 decimal place for time, 0 for angle.
        let precision = if as_time { 1 } else { 0 };
        Ok(format_sky(value, as_time, precision))
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..=2", axis),
            ));
        }
        let idx = axis - 1;
        let as_time = self.axes[idx].effective_as_time();
        unformat_sky(text, as_time)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        if is_bad(a[0]) || is_bad(a[1]) || is_bad(b[0]) || is_bad(b[1]) {
            return Ok(BAD_VALUE);
        }
        Ok(pal::dsep(a[0], a[1], b[0], b[1]))
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        // Angle at point b in triangle a-b-c.
        if a.iter().chain(b.iter()).chain(c.iter()).any(|&v| is_bad(v)) {
            return Ok(BAD_VALUE);
        }
        let ab = pal::dbear(a[0], a[1], b[0], b[1]);
        let cb = pal::dbear(c[0], c[1], b[0], b[1]);
        Ok(pal::drange(ab - cb))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        // Walk along great circle from `from` toward `to` by `dist` radians.
        if from.iter().chain(to.iter()).any(|&v| is_bad(v)) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; 2]);
        }

        // Convert to 3-vectors.
        let v1 = pal::dcs2c(from[0], from[1]);
        let v2 = pal::dcs2c(to[0], to[1]);

        // Cross product gives rotation axis.
        let axis_vec = pal::dvxv(&v1, &v2);
        let (axis_norm, axis_len) = pal::dvn(&axis_vec);

        if axis_len < 1e-20 {
            // Points coincident or antipodal -- offset along any great circle.
            return Ok(from.to_vec());
        }

        // Scale the rotation axis by the distance.  PAL uses passive
        // (frame-rotation) convention, so negate to get active rotation
        // of the vector toward the destination.
        let rot_vec = [
            -axis_norm[0] * dist,
            -axis_norm[1] * dist,
            -axis_norm[2] * dist,
        ];

        // Convert to rotation matrix and apply.
        let rmat = pal::dav2m(&rot_vec);
        let v_out = pal::dmxv(&rmat, &v1);
        let (lon, lat) = pal::dcc2s(&v_out);

        Ok(vec![lon, lat])
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        if coords.len() < 2 {
            return Ok(());
        }
        if is_bad(coords[0]) || is_bad(coords[1]) {
            return Ok(());
        }

        // Normalize latitude to [-pi/2, pi/2], folding through poles.
        let mut lat = pal::drange(coords[1]); // [-pi, pi]
        let mut lon = coords[0];

        if lat > std::f64::consts::FRAC_PI_2 {
            lat = std::f64::consts::PI - lat;
            lon += std::f64::consts::PI;
        } else if lat < -std::f64::consts::FRAC_PI_2 {
            lat = -std::f64::consts::PI - lat;
            lon += std::f64::consts::PI;
        }

        // Normalize longitude.
        if self.neglon {
            lon = pal::drange(lon); // [-pi, pi]
        } else {
            lon = pal::dranrm(lon); // [0, 2pi]
        }

        coords[0] = lon;
        coords[1] = lat;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = previous.as_any().downcast_ref::<SkyFrame>();
        match prev {
            Some(prev_sky) => make_sky_mapping(
                prev_sky.system,
                prev_sky.equinox_mjd(),
                prev_sky.epoch_mjd(),
                self.system,
                self.equinox_mjd(),
                self.epoch_mjd(),
                self.system, // align in current system
                prev_sky.obs_lat.unwrap_or(0.0),
                0.0, // LAST=0 for non-AZEL (full LAST deferred)
                self.obs_lat.unwrap_or(0.0),
                0.0,
            ),
            None => {
                // Previous was not a SkyFrame -- return UnitMap.
                Ok(Box::new(UnitMap::new(2)))
            }
        }
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let to_sky = to.as_any().downcast_ref::<SkyFrame>();
        match to_sky {
            Some(to_sky) => {
                let mapping = make_sky_mapping(
                    self.system,
                    self.equinox_mjd(),
                    self.epoch_mjd(),
                    to_sky.system,
                    to_sky.equinox_mjd(),
                    to_sky.epoch_mjd(),
                    SkySystem::Fk5, // align through FK5
                    self.obs_lat.unwrap_or(0.0),
                    0.0,
                    to_sky.obs_lat.unwrap_or(0.0),
                    0.0,
                )?;
                let mut fs = FrameSet::new(self.snapshot());
                fs.add_frame(FrameRef::Base, mapping, to.snapshot())?;
                Ok(Some(fs))
            }
            None => Ok(None),
        }
    }

    fn obs_lat(&self) -> Option<f64> {
        self.obs_lat
    }

    fn obs_lon(&self) -> Option<f64> {
        self.obs_lon
    }

    fn obs_alt(&self) -> Option<f64> {
        self.obs_alt
    }

    fn dtai(&self) -> Option<f64> {
        self.dtai
    }

    fn dut1(&self) -> Option<f64> {
        self.dut1
    }

    fn system_string(&self) -> Option<&str> {
        Some(self.system.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::Mapping;
    use std::f64::consts::{FRAC_PI_2, PI};

    const TOL: f64 = 1.0e-10;

    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!(
            (a - b).abs() < tol,
            "expected {a} ~= {b} (diff = {:.2e})",
            (a - b).abs()
        );
    }

    // -----------------------------------------------------------------------
    // Construction and defaults
    // -----------------------------------------------------------------------

    #[test]
    fn new_defaults() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        assert_eq!(sky.naxes(), 2);
        assert_eq!(sky.system(), SkySystem::Fk5);
        assert_eq!(sky.domain(), "SKY");
        assert_close(sky.epoch_mjd(), 51544.5, TOL);
        assert_close(sky.equinox_mjd(), 51544.5, TOL);
        assert!(!sky.neglon());
        assert_eq!(sky.sky_ref_is(), SkyRefIs::Ignored);
        assert!(!sky.align_offset());
        assert_close(sky.sky_ref()[0], 0.0, TOL);
        assert_close(sky.sky_ref()[1], 0.0, TOL);
        assert_close(sky.sky_ref_p()[0], 0.0, TOL);
        assert_close(sky.sky_ref_p()[1], FRAC_PI_2, TOL);
        assert_close(sky.sky_tol(), 0.0, TOL);
        assert!(sky.projection().is_none());
        assert_eq!(sky.title(), "");
    }

    #[test]
    fn fk4_default_equinox() {
        let sky = SkyFrame::new(SkySystem::Fk4);
        let b1950_mjd = pal::epb2d(1950.0);
        assert_close(sky.equinox_mjd(), b1950_mjd, 1.0e-6);
    }

    // -----------------------------------------------------------------------
    // Attribute setters
    // -----------------------------------------------------------------------

    #[test]
    fn set_system() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_system(SkySystem::Galactic);
        assert_eq!(sky.system(), SkySystem::Galactic);
    }

    #[test]
    fn set_epoch() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_epoch(53000.0);
        assert_close(sky.epoch_mjd(), 53000.0, TOL);
    }

    #[test]
    fn set_equinox() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_equinox(53000.0);
        assert_close(sky.equinox_mjd(), 53000.0, TOL);
    }

    #[test]
    fn set_neglon() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_neglon(true);
        assert!(sky.neglon());
    }

    #[test]
    fn set_sky_ref() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref(1.0, 0.5);
        assert_close(sky.sky_ref()[0], 1.0, TOL);
        assert_close(sky.sky_ref()[1], 0.5, TOL);
    }

    #[test]
    fn set_sky_ref_is() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_is(SkyRefIs::Origin);
        assert_eq!(sky.sky_ref_is(), SkyRefIs::Origin);
    }

    #[test]
    fn set_sky_ref_p() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_p(1.0, 0.5);
        assert_close(sky.sky_ref_p()[0], 1.0, TOL);
        assert_close(sky.sky_ref_p()[1], 0.5, TOL);
    }

    #[test]
    fn set_sky_tol() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_tol(1.0e-6);
        assert_close(sky.sky_tol(), 1.0e-6, TOL);
    }

    #[test]
    fn set_align_offset() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_align_offset(true);
        assert!(sky.align_offset());
    }

    #[test]
    fn set_projection() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_projection("TAN");
        assert_eq!(sky.projection(), Some("TAN"));
    }

    #[test]
    fn set_observer() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_obs_lat(0.5);
        sky.set_obs_lon(1.0);
        sky.set_obs_alt(4200.0);
        sky.set_dtai(37.0);
        sky.set_dut1(0.3);
        assert_close(sky.obs_lat().unwrap(), 0.5, TOL);
        assert_close(sky.obs_lon().unwrap(), 1.0, TOL);
        assert_close(sky.obs_alt().unwrap(), 4200.0, TOL);
        assert_close(sky.dtai().unwrap(), 37.0, TOL);
        assert_close(sky.dut1().unwrap(), 0.3, TOL);
    }

    // -----------------------------------------------------------------------
    // Frame trait: format / unformat
    // -----------------------------------------------------------------------

    #[test]
    fn format_longitude_hms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // PI/2 = 6h
        let s = sky.format(1, FRAC_PI_2).unwrap();
        assert_eq!(s, "6:00:00.0");
    }

    #[test]
    fn format_latitude_dms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // PI/4 = 45 degrees
        let s = sky.format(2, PI / 4.0).unwrap();
        assert_eq!(s, "+45:00:00");
    }

    #[test]
    fn format_bad_axis() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        assert!(sky.format(0, 0.0).is_err());
        assert!(sky.format(3, 0.0).is_err());
    }

    #[test]
    fn unformat_hms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let (val, _nc) = sky.unformat(1, "6:00:00.0").unwrap();
        assert_close(val, FRAC_PI_2, 1.0e-6);
    }

    #[test]
    fn unformat_dms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let (val, _nc) = sky.unformat(2, "+45:00:00").unwrap();
        assert_close(val, PI / 4.0, 1.0e-6);
    }

    // -----------------------------------------------------------------------
    // Frame trait: distance
    // -----------------------------------------------------------------------

    #[test]
    fn distance_same_point() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let d = sky.distance(&[1.0, 0.5], &[1.0, 0.5]).unwrap();
        assert_close(d, 0.0, TOL);
    }

    #[test]
    fn distance_poles() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let d = sky.distance(&[0.0, FRAC_PI_2], &[0.0, -FRAC_PI_2]).unwrap();
        assert_close(d, PI, TOL);
    }

    #[test]
    fn distance_bad_value() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let d = sky.distance(&[BAD_VALUE, 0.0], &[1.0, 0.5]).unwrap();
        assert_eq!(d, BAD_VALUE);
    }

    // -----------------------------------------------------------------------
    // Frame trait: angle
    // -----------------------------------------------------------------------

    #[test]
    fn angle_bad_value() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let ang = sky
            .angle(&[BAD_VALUE, 0.0], &[0.0, 0.0], &[1.0, 0.0])
            .unwrap();
        assert_eq!(ang, BAD_VALUE);
    }

    // -----------------------------------------------------------------------
    // Frame trait: offset
    // -----------------------------------------------------------------------

    #[test]
    fn offset_zero_distance() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let result = sky.offset(&[1.0, 0.5], &[2.0, 0.3], 0.0).unwrap();
        // Zero distance should return the start point.
        assert_close(result[0], 1.0, TOL);
        assert_close(result[1], 0.5, TOL);
    }

    #[test]
    fn offset_full_distance() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let from = [0.0, 0.0];
        let to = [FRAC_PI_2, 0.0];
        let sep = sky.distance(&from, &to).unwrap();
        let result = sky.offset(&from, &to, sep).unwrap();
        assert_close(result[0], FRAC_PI_2, 1.0e-8);
        assert_close(result[1], 0.0, 1.0e-8);
    }

    #[test]
    fn offset_bad_value() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let result = sky.offset(&[BAD_VALUE, 0.0], &[1.0, 0.0], 0.5).unwrap();
        assert_eq!(result[0], BAD_VALUE);
        assert_eq!(result[1], BAD_VALUE);
    }

    // -----------------------------------------------------------------------
    // Frame trait: norm
    // -----------------------------------------------------------------------

    #[test]
    fn norm_no_op() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut coords = [1.0, 0.5];
        sky.norm(&mut coords).unwrap();
        assert_close(coords[0], 1.0, TOL);
        assert_close(coords[1], 0.5, TOL);
    }

    #[test]
    fn norm_wraps_longitude() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // Default neglon=false, so longitude normalizes to [0, 2pi].
        let mut coords = [-0.5, 0.3];
        sky.norm(&mut coords).unwrap();
        assert_close(coords[0], 2.0 * PI - 0.5, 1.0e-8);
        assert_close(coords[1], 0.3, TOL);
    }

    #[test]
    fn norm_neglon_wraps_to_minus_pi_pi() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_neglon(true);
        let mut coords = [4.0, 0.3];
        sky.norm(&mut coords).unwrap();
        // 4.0 radians normalized to [-pi, pi].
        let expected_lon = pal::drange(4.0);
        assert_close(coords[0], expected_lon, 1.0e-8);
    }

    #[test]
    fn norm_folds_latitude_through_pole() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // Latitude > pi/2 folds through the north pole.
        let mut coords = [0.0, FRAC_PI_2 + 0.1];
        sky.norm(&mut coords).unwrap();
        // Latitude should become pi - (pi/2 + 0.1) = pi/2 - 0.1
        assert_close(coords[1], FRAC_PI_2 - 0.1, 1.0e-8);
        // Longitude shifts by pi.
        assert_close(coords[0], PI, 1.0e-8);
    }

    #[test]
    fn norm_bad_value_no_op() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut coords = [BAD_VALUE, 0.5];
        sky.norm(&mut coords).unwrap();
        assert_eq!(coords[0], BAD_VALUE);
        assert_close(coords[1], 0.5, TOL);
    }

    // -----------------------------------------------------------------------
    // Frame trait: convert
    // -----------------------------------------------------------------------

    #[test]
    fn convert_fk5_to_galactic() {
        let fk5 = SkyFrame::new(SkySystem::Fk5);
        let gal = SkyFrame::new(SkySystem::Galactic);

        let fs = fk5.convert(&gal, &[]).unwrap();
        assert!(fs.is_some(), "FK5 -> Galactic conversion should succeed");

        let fs = fs.unwrap();
        // The FrameSet should be usable as a mapping.
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = fs.transform(&pts, true).unwrap();
        let p = out.point(0);
        // RA=0, Dec=0 in FK5 should give a specific Galactic coordinate.
        // Just verify we get something reasonable (l, b).
        assert!(p[0].is_finite());
        assert!(p[1].is_finite());
    }

    #[test]
    fn convert_incompatible() {
        use crate::frames::{BasicFrameBuilder, Frame};
        let sky = SkyFrame::new(SkySystem::Fk5);
        let basic = BasicFrameBuilder::new(2).build().unwrap();

        let result = sky.convert(&basic, &[]).unwrap();
        assert!(
            result.is_none(),
            "SkyFrame -> BasicFrame should return None"
        );
    }

    // -----------------------------------------------------------------------
    // Frame trait: system_string
    // -----------------------------------------------------------------------

    #[test]
    fn system_string() {
        let sky = SkyFrame::new(SkySystem::Galactic);
        assert_eq!(sky.system_string(), Some("GALACTIC"));
    }

    // -----------------------------------------------------------------------
    // Frame trait: map_from
    // -----------------------------------------------------------------------

    #[test]
    fn map_from_same() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let prev = sky.clone();
        let mapping = sky.map_from(&prev).unwrap();
        // Same system, equinox, epoch -> UnitMap.
        assert!(mapping.as_any().is::<UnitMap>());
    }

    // -----------------------------------------------------------------------
    // Clone / Send
    // -----------------------------------------------------------------------

    #[test]
    fn clone_is_independent() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_title("original");
        let mut clone = sky.clone();
        clone.set_title("clone");
        assert_eq!(sky.title(), "original");
        assert_eq!(clone.title(), "clone");
    }

    fn _assert_send<T: Send>() {}

    #[test]
    fn skyframe_is_send() {
        _assert_send::<SkyFrame>();
    }
}
