//! The celestial coordinate frame (SkyFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::sky_convert::{SkyRefIs, SkySystem, make_offset_mapping, make_sky_mapping};
use crate::frames::skyaxis::{SkyAxis, format_sky, unformat_sky};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{ast_equal, is_bad};
use crate::mappings::{CmpMap, Composition, DynMapping, UnitMap};
use crate::pal;
use crate::serial::{Node, NodeValue, Serializable};

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
    sky_ref_is_set: bool,
    align_offset: bool,
    align_system: Option<SkySystem>,

    // Per-axis properties
    axes: [SkyAxis; 2], // [longitude, latitude] (physical storage)

    /// Axis permutation: perm[ext_axis] = physical_axis (0-based).
    /// Physical 0 = longitude, physical 1 = latitude.
    /// After swap: perm = [1, 0] means external axis 0 → latitude.
    perm: [usize; 2],

    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,
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
            sky_ref_is_set: false,
            align_offset: false,
            align_system: None,
            axes: [SkyAxis::longitude(), SkyAxis::latitude()],
            perm: [0, 1],
            act_unt: None,
            ident: None,
        }
    }

    /// Position of longitude in coordinate arrays (0-based).
    #[inline]
    fn lon_pos(&self) -> usize {
        if self.perm[0] == 0 { 0 } else { 1 }
    }

    /// Position of latitude in coordinate arrays (0-based).
    #[inline]
    fn lat_pos(&self) -> usize {
        if self.perm[0] == 1 { 0 } else { 1 }
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

    /// Whether the epoch attribute has been explicitly set
    /// (mirrors `astTestEpoch`).
    pub fn test_epoch(&self) -> bool {
        self.epoch.is_some()
    }

    /// Set the epoch as MJD.
    pub fn set_epoch(&mut self, mjd: f64) {
        self.epoch = Some(mjd);
    }

    /// Set the epoch from a bare decimal year (e.g. a FITS `EPOCH`
    /// keyword or the value stored in a native-encoded `Epoch` node).
    ///
    /// Applies the IAU 1984 convention to resolve the year to an MJD,
    /// matching what C does when the value flows through
    /// `astSet(skyframe, "Epoch=...")` → `astReadDateTime`.
    pub fn set_epoch_year(&mut self, year: f64) {
        self.set_epoch(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the equinox from a bare decimal year (e.g. a FITS
    /// `EQUINOX` keyword or the value stored in a native-encoded
    /// `Eqnox` node).
    ///
    /// Applies the IAU 1984 convention to resolve the year to an MJD,
    /// matching what C does when the value flows through
    /// `astSet(skyframe, "Equinox=...")` → `astReadDateTime`.
    pub fn set_equinox_year(&mut self, year: f64) {
        self.set_equinox(crate::frames::date_time::year_to_mjd_iau1984(year));
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
        self.sky_ref_is_set = true;
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

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
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

    // --- Private helpers for serialisation defaults ---

    /// Default title for the current system and equinox.
    fn default_title(&self) -> String {
        let base = match self.system {
            SkySystem::Fk5 => {
                let eq = self.equinox_epoch_string();
                format!("FK5 equatorial coordinates; mean equinox J{eq}")
            }
            SkySystem::Fk4 | SkySystem::Fk4NoE => {
                let eq = self.equinox_epoch_string();
                // C library (`skyframe.c` GetTitle) appends the Epoch as a
                // Besselian year alongside the equinox for FK4 / FK4-no-E.
                let ep = self.epoch_epoch_string_besselian();
                let stem = if matches!(self.system, SkySystem::Fk4NoE) {
                    "FK4 equatorial coordinates; no E-terms"
                } else {
                    "FK4 equatorial coordinates"
                };
                format!("{stem}; mean equinox B{eq}; epoch B{ep}")
            }
            SkySystem::Galactic => "IAU (1958) galactic coordinates".to_string(),
            SkySystem::Supergalactic => "Supergalactic coordinates".to_string(),
            SkySystem::Ecliptic => "Ecliptic coordinates".to_string(),
            SkySystem::Icrs => "ICRS coordinates".to_string(),
            SkySystem::J2000 => "J2000 equatorial coordinates".to_string(),
            _ => "Sky coordinates".to_string(),
        };
        if let Some(ref proj) = self.projection {
            format!("{base}; {proj} projection")
        } else {
            base
        }
    }

    /// Format the SkyFrame's epoch as a Besselian-year display string, used
    /// for FK4 / FK4-no-E titles (matches C's `astFmtDecimalYr(palEpb(epoch))`).
    /// When `Epoch` has not been explicitly set, mirrors the C
    /// `GetEpoch` SkyFrame override: B1950.0 for FK4 / FK4-no-E, otherwise
    /// J2000.0 (here unused — only FK4 titles call this).
    fn epoch_epoch_string_besselian(&self) -> String {
        let epoch_mjd = self.epoch.unwrap_or_else(|| {
            if matches!(self.system, SkySystem::Fk4 | SkySystem::Fk4NoE) {
                pal::epb2d(1950.0)
            } else {
                pal::epj2d(2000.0)
            }
        });
        let bep = pal::epb(epoch_mjd);
        if bep == bep.trunc() {
            format!("{:.1}", bep)
        } else {
            format!("{}", bep)
        }
    }

    /// Format the equinox as a display string (e.g. "2000.0" or "1950.0").
    fn equinox_epoch_string(&self) -> String {
        let eqnox_mjd = self
            .equinox
            .unwrap_or_else(|| self.system.default_equinox_mjd());
        let jep = pal::epj(eqnox_mjd);
        let epoch_val = if jep < 1984.0 {
            pal::epb(eqnox_mjd)
        } else {
            jep
        };
        // Format: if integer, show as "NNNN.0"; otherwise full precision.
        if epoch_val == epoch_val.trunc() {
            format!("{:.1}", epoch_val)
        } else {
            format!("{}", epoch_val)
        }
    }

    /// Default axis labels for the current system.
    fn default_labels(&self) -> (&'static str, &'static str) {
        match self.system {
            SkySystem::Galactic => ("Galactic longitude", "Galactic latitude"),
            SkySystem::Ecliptic => ("Ecliptic longitude", "Ecliptic latitude"),
            SkySystem::Supergalactic => ("Supergalactic longitude", "Supergalactic latitude"),
            _ => ("Right ascension", "Declination"),
        }
    }

    /// Default axis units for the current system.
    fn default_units(&self) -> (&'static str, &'static str) {
        match self.system {
            SkySystem::Galactic | SkySystem::Ecliptic | SkySystem::Supergalactic => {
                ("degrees", "degrees")
            }
            _ => ("hh:mm:ss.s", "ddd:mm:ss"),
        }
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
        let phys = self.perm[axis - 1];
        let as_time = self.axes[phys].effective_as_time();
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
        let phys = self.perm[axis - 1];
        let as_time = self.axes[phys].effective_as_time();
        unformat_sky(text, as_time)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        if is_bad(a[0]) || is_bad(a[1]) || is_bad(b[0]) || is_bad(b[1]) {
            return Ok(BAD_VALUE);
        }
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        Ok(pal::dsep(a[lp], a[bp], b[lp], b[bp]))
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if a.iter().chain(b.iter()).chain(c.iter()).any(|&v| is_bad(v)) {
            return Ok(BAD_VALUE);
        }
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        let ab = pal::dbear(a[lp], a[bp], b[lp], b[bp]);
        let cb = pal::dbear(c[lp], c[bp], b[lp], b[bp]);
        Ok(pal::drange(ab - cb))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        if from.iter().chain(to.iter()).any(|&v| is_bad(v)) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; 2]);
        }

        let lp = self.lon_pos();
        let bp = self.lat_pos();

        let v1 = pal::dcs2c(from[lp], from[bp]);
        let v2 = pal::dcs2c(to[lp], to[bp]);

        let axis_vec = pal::dvxv(&v1, &v2);
        let (axis_norm, axis_len) = pal::dvn(&axis_vec);

        if axis_len < 1e-20 {
            return Ok(from.to_vec());
        }

        let rot_vec = [
            -axis_norm[0] * dist,
            -axis_norm[1] * dist,
            -axis_norm[2] * dist,
        ];

        let rmat = pal::dav2m(&rot_vec);
        let v_out = pal::dmxv(&rmat, &v1);
        let (lon, lat) = pal::dcc2s(&v_out);

        let mut result = vec![0.0; 2];
        result[lp] = lon;
        result[bp] = lat;
        Ok(result)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        if coords.len() < 2 {
            return Ok(());
        }
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        if is_bad(coords[lp]) || is_bad(coords[bp]) {
            return Ok(());
        }

        let mut lat = pal::drange(coords[bp]);
        let mut lon = coords[lp];

        if lat > std::f64::consts::FRAC_PI_2 {
            lat = std::f64::consts::PI - lat;
            lon += std::f64::consts::PI;
        } else if lat < -std::f64::consts::FRAC_PI_2 {
            lat = -std::f64::consts::PI - lat;
            lon += std::f64::consts::PI;
        }

        if self.neglon {
            lon = pal::drange(lon);
        } else {
            lon = pal::dranrm(lon);
        }

        coords[lp] = lon;
        coords[bp] = lat;
        Ok(())
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, 2)?;
        let old = self.perm;
        self.perm[0] = old[perm[0] - 1];
        self.perm[1] = old[perm[1] - 1];
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = previous.as_any().downcast_ref::<SkyFrame>();
        match prev {
            Some(prev_sky) => {
                let mut mapping: DynMapping = make_sky_mapping(
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
                )?;

                // Prepend source offset removal (offset→standard).
                if let Some(off_map) =
                    make_offset_mapping(prev_sky.sky_ref, prev_sky.sky_ref_p, prev_sky.sky_ref_is)?
                {
                    mapping = Box::new(CmpMap::new(off_map, mapping, Composition::Series)?);
                }

                // Append target offset application (standard→offset = inverse).
                if let Some(mut off_map) =
                    make_offset_mapping(self.sky_ref, self.sky_ref_p, self.sky_ref_is)?
                {
                    off_map.invert();
                    mapping = Box::new(CmpMap::new(mapping, off_map, Composition::Series)?);
                }

                Ok(mapping)
            }
            None => {
                // Previous was not a SkyFrame -- return UnitMap.
                Ok(Box::new(UnitMap::new(2)))
            }
        }
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SkyFrame>() else {
            return false;
        };
        if self.system != other.system {
            return false;
        }
        let epoch_eq = match (self.epoch, other.epoch) {
            (Some(a), Some(b)) => ast_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if !epoch_eq {
            return false;
        }
        let equinox_eq = match (self.equinox, other.equinox) {
            (Some(a), Some(b)) => ast_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if !equinox_eq {
            return false;
        }
        self.sky_ref_is == other.sky_ref_is
            && ast_equal(self.sky_ref[0], other.sky_ref[0])
            && ast_equal(self.sky_ref[1], other.sky_ref[1])
            && self.neglon == other.neglon
            && self.title == other.title
            && self.domain == other.domain
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let to_sky = to.as_any().downcast_ref::<SkyFrame>();
        match to_sky {
            Some(to_sky) => {
                let mut mapping: DynMapping = make_sky_mapping(
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

                // Prepend source offset removal (offset→standard).
                if let Some(off_map) =
                    make_offset_mapping(self.sky_ref, self.sky_ref_p, self.sky_ref_is)?
                {
                    mapping = Box::new(CmpMap::new(off_map, mapping, Composition::Series)?);
                }

                // Append target offset application (standard→offset = inverse).
                if let Some(mut off_map) =
                    make_offset_mapping(to_sky.sky_ref, to_sky.sky_ref_p, to_sky.sky_ref_is)?
                {
                    off_map.invert();
                    mapping = Box::new(CmpMap::new(mapping, off_map, Composition::Series)?);
                }

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

    fn domain(&self) -> &str {
        SkyFrame::domain(self)
    }

    fn ident(&self) -> &str {
        SkyFrame::ident(self)
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for SkyFrame {
    fn class_name(&self) -> &'static str {
        "SkyFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of celestial coordinate system".to_string());

        // --- Object-level attributes (before IsA Object) ---
        // Ident: written if explicitly set.
        if let Some(ref ident) = self.ident {
            node.push_string(
                "Ident",
                ident,
                Some("Permanent Object identification string".to_string()),
            );
        }

        // IsA Object marker (only emitted when Ident is present).
        if self.ident.is_some() {
            node.push_isa("Object", Some("AST Object".to_string()));
        }

        // --- Frame-level attributes (before IsA Frame) ---
        // Order: Title, Naxes, Domain, Epoch, Lbl1, Lbl2, System,
        //        Uni1, Uni2, Dir1, Bot2, Top2, [observer], Axes

        // Title: active if explicitly set, otherwise unset with default.
        if let Some(ref title) = self.title {
            if !title.is_empty() {
                node.push_string(
                    "Title",
                    title,
                    Some("Title of coordinate system".to_string()),
                );
            }
        } else {
            node.push_unset(
                "Title",
                NodeValue::String(self.default_title()),
                Some("Title of coordinate system".to_string()),
            );
        }

        // Naxes = 2 (always active).
        node.push_int("Naxes", 2, Some("Number of coordinate axes".to_string()));

        // Domain: active if explicitly set to non-"SKY", otherwise unset "SKY".
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() && domain != "SKY" {
                node.push_string(
                    "Domain",
                    domain,
                    Some("Coordinate system domain".to_string()),
                );
            } else {
                node.push_unset(
                    "Domain",
                    NodeValue::String("SKY".to_string()),
                    Some("Coordinate system domain".to_string()),
                );
            }
        } else {
            node.push_unset(
                "Domain",
                NodeValue::String("SKY".to_string()),
                Some("Coordinate system domain".to_string()),
            );
        }

        // Epoch: store MJD internally; convert to Besselian / Julian year on
        // output (matches `frame.c` line 14328: `bessyr = epoch < palEpj2d(1984.0)`).
        // The default MJD depends on the SkyFrame's System — B1950.0 for
        // FK4 / FK4-no-E, J2000.0 elsewhere.
        let epoch_mjd = self.epoch.unwrap_or_else(|| {
            if matches!(self.system, SkySystem::Fk4 | SkySystem::Fk4NoE) {
                pal::epb2d(1950.0)
            } else {
                pal::epj2d(2000.0)
            }
        });
        let bessyr = epoch_mjd < pal::epj2d(1984.0);
        let epoch_year = if bessyr {
            pal::epb(epoch_mjd)
        } else {
            pal::epj(epoch_mjd)
        };
        let epoch_comment = if bessyr {
            "Besselian epoch of observation"
        } else {
            "Julian epoch of observation"
        };
        if self.epoch.is_some() {
            node.push_double("Epoch", epoch_year, Some(epoch_comment.to_string()));
        } else {
            node.push_unset(
                "Epoch",
                NodeValue::Double(epoch_year),
                Some(epoch_comment.to_string()),
            );
        }

        // Axis labels (always unset with system-appropriate defaults).
        let (lbl1, lbl2) = self.default_labels();
        node.push_unset(
            "Lbl1",
            NodeValue::String(lbl1.to_string()),
            Some("Label for axis 1".to_string()),
        );
        node.push_unset(
            "Lbl2",
            NodeValue::String(lbl2.to_string()),
            Some("Label for axis 2".to_string()),
        );

        // System. C library defaults SkyFrame's System to ICRS; for ICRS we
        // emit commented (default), for any other system uncommented (explicit).
        if matches!(self.system, SkySystem::Icrs) {
            node.push_unset(
                "System",
                NodeValue::String(self.system.as_str().to_string()),
                Some("Coordinate system type".to_string()),
            );
        } else {
            node.push_string(
                "System",
                self.system.as_str(),
                Some("Coordinate system type".to_string()),
            );
        }

        // AlSys (alignment coordinate system, if set).
        if let Some(align_sys) = self.align_system {
            node.push_string(
                "AlSys",
                align_sys.as_str(),
                Some("Alignment coordinate system".to_string()),
            );
        }

        // Axis units (always unset with system-appropriate defaults).
        let (uni1, uni2) = self.default_units();
        node.push_unset(
            "Uni1",
            NodeValue::String(uni1.to_string()),
            Some("Units for axis 1".to_string()),
        );
        node.push_unset(
            "Uni2",
            NodeValue::String(uni2.to_string()),
            Some("Units for axis 2".to_string()),
        );

        // Dir1: longitude axis plots in reverse direction (always unset).
        node.push_unset(
            "Dir1",
            NodeValue::Int(0),
            Some("Plot axis 1 in reverse direction".to_string()),
        );

        // Bot2/Top2: latitude axis limits (always unset).
        node.push_unset(
            "Bot2",
            NodeValue::Double(-std::f64::consts::FRAC_PI_2),
            Some("Lowest legal axis value".to_string()),
        );
        node.push_unset(
            "Top2",
            NodeValue::Double(std::f64::consts::FRAC_PI_2),
            Some("Highest legal axis value".to_string()),
        );

        // ActUnt: active if explicitly set to false.
        if let Some(false) = self.act_unt {
            node.push_int(
                "ActUnt",
                0,
                Some("Unit strings do not affect alignment".to_string()),
            );
        }

        // Observer attributes (active only if explicitly set).
        if let Some(obs_lat) = self.obs_lat {
            node.push_double(
                "ObsLat",
                obs_lat,
                Some("Observers geodetic latitude (rads)".to_string()),
            );
        }
        if let Some(obs_lon) = self.obs_lon {
            node.push_double(
                "ObsLon",
                obs_lon,
                Some("Observers geodetic longitude (rads)".to_string()),
            );
        }
        if let Some(obs_alt) = self.obs_alt {
            node.push_double(
                "ObsAlt",
                obs_alt,
                Some("Observers geodetic altitude (m)".to_string()),
            );
        }
        if let Some(dtai) = self.dtai {
            node.push_double("Dtai", dtai, Some("TAI-UTC (s)".to_string()));
        }
        if let Some(dut1) = self.dut1 {
            node.push_double("Dut1", dut1, Some("UT1-UTC (s)".to_string()));
        }

        // Per-axis SkyAxis children.
        for (i, axis) in self.axes.iter().enumerate() {
            let axis_node = Serializable::to_node(axis)?;
            let key = format!("Ax{}", i + 1);
            let comment = format!("Axis number {}", i + 1);
            node.push_object(&key, axis_node, Some(comment));
        }

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- SkyFrame-specific attributes (after IsA Frame) ---

        // Proj (if set).
        if let Some(ref proj) = self.projection {
            node.push_string(
                "Proj",
                proj,
                Some("Description of sky projection".to_string()),
            );
        }

        // NegLon (if true).
        if self.neglon {
            node.push_int(
                "NegLon",
                1,
                Some("Display negative longitude values".to_string()),
            );
        }

        // SkyTol: active if explicitly set, otherwise unset with default 0.001.
        if let Some(sky_tol) = self.sky_tol {
            node.push_double(
                "SkyTol",
                sky_tol,
                Some("Smallest significant separation [arc-sec]".to_string()),
            );
        } else {
            node.push_unset(
                "SkyTol",
                NodeValue::Double(0.001),
                Some("Smallest significant separation [arc-sec]".to_string()),
            );
        }

        // Equinox as decimal epoch (Besselian if < 1984, Julian otherwise).
        // C library treats Equinox as "helpful" (commented default emitted)
        // only when the system is FK4, FK4-no-E, FK5 or Ecliptic; for other
        // systems an unset Equinox is omitted entirely.
        let eq_helpful = matches!(
            self.system,
            SkySystem::Fk4 | SkySystem::Fk4NoE | SkySystem::Fk5 | SkySystem::Ecliptic
        );
        if self.equinox.is_some() || eq_helpful {
            let eqnox_mjd = self
                .equinox
                .unwrap_or_else(|| self.system.default_equinox_mjd());
            let jep = pal::epj(eqnox_mjd);
            let bessyr = jep < 1984.0;
            let epoch_val = if bessyr { pal::epb(eqnox_mjd) } else { jep };
            let comment = if bessyr {
                "Besselian epoch of mean equinox"
            } else {
                "Julian epoch of mean equinox"
            };
            if self.equinox.is_some() {
                node.push_double("Eqnox", epoch_val, Some(comment.to_string()));
            } else {
                node.push_unset(
                    "Eqnox",
                    NodeValue::Double(epoch_val),
                    Some(comment.to_string()),
                );
            }
        }

        // SRefIs (write if explicitly set, even if Ignored).
        if self.sky_ref_is_set || self.sky_ref_is != SkyRefIs::Ignored {
            let srefis_str = match self.sky_ref_is {
                SkyRefIs::Pole => "Pole",
                SkyRefIs::Origin => "Origin",
                SkyRefIs::Ignored => "Ignored",
            };
            let comment = match self.sky_ref_is {
                SkyRefIs::Ignored => "Not rotated (ref. pos. is ignored)",
                SkyRefIs::Origin => "Rotated to put ref. pos. at origin",
                SkyRefIs::Pole => "Rotated to put ref. pos. at pole",
            };
            node.push_string("SRefIs", srefis_str, Some(comment.to_string()));
        }

        // SRef1, SRef2 (sky_ref longitude/latitude) — write if non-zero.
        if self.sky_ref[0] != 0.0 || self.sky_ref[1] != 0.0 || self.sky_ref_is != SkyRefIs::Ignored
        {
            let (lon_sym, lat_sym) = match self.system {
                SkySystem::Fk5
                | SkySystem::Fk4
                | SkySystem::Fk4NoE
                | SkySystem::Icrs
                | SkySystem::J2000 => ("RA", "Dec"),
                SkySystem::Galactic => ("Gal. long.", "Gal. lat."),
                SkySystem::Supergalactic => ("Supergal. long.", "Supergal. lat."),
                SkySystem::Ecliptic => ("Ecl. long.", "Ecl. lat."),
                _ => ("Longitude", "Latitude"),
            };
            let lon_fmt = self.format(1, self.sky_ref[0]).unwrap_or_default();
            let lat_fmt = self.format(2, self.sky_ref[1]).unwrap_or_default();
            node.push_double(
                "SRef1",
                self.sky_ref[0],
                Some(format!("Ref. pos. {lon_sym} {lon_fmt}")),
            );
            node.push_double(
                "SRef2",
                self.sky_ref[1],
                Some(format!("Ref. pos. {lat_sym} {lat_fmt}")),
            );
        }

        // SRefP1, SRefP2 (sky_ref_p longitude/latitude).
        // Only write if SRefIs != Ignored (SRefP is only meaningful when offset coords are active).
        if self.sky_ref_is != SkyRefIs::Ignored {
            if self.sky_ref_p[0] != 0.0 {
                node.push_double(
                    "SRefP1",
                    self.sky_ref_p[0],
                    Some("Sky reference meridian longitude".to_string()),
                );
            }
            node.push_double(
                "SRefP2",
                self.sky_ref_p[1],
                Some("Sky reference meridian latitude".to_string()),
            );
        }

        // AlOff (if true).
        if self.align_offset {
            node.push_int(
                "AlOff",
                1,
                Some("Align offset coordinate system".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Read System. Default is ICRS to match the C library
        // (`skyframe.c`: AST__ICRS is the default System).
        let sys_str = node.get_string("System", "ICRS");
        let system = SkySystem::parse(&sys_str).unwrap_or(SkySystem::Icrs);

        let mut sky = SkyFrame::new(system);

        // Title.
        let title = node.get_string("Title", "");
        if !title.is_empty() {
            sky.set_title(&title);
        }

        // Domain.
        let domain = node.get_string("Domain", "");
        if !domain.is_empty() {
            sky.domain = Some(domain);
        }

        // Epoch and Equinox are stored as decimal years on disk and
        // resolved to MJD via the SkyFrame's year-aware setters
        // (which apply the IAU 1984 rule). Mirrors `frame.c`
        // astLoadFrame, which delegates to astReadDateTime.
        if node.get("Epoch").is_some() {
            sky.set_epoch_year(node.get_double("Epoch", 0.0));
        }
        if node.get("Eqnox").is_some() {
            sky.set_equinox_year(node.get_double("Eqnox", 0.0));
        }

        // Observer attributes.
        if node.get("ObsLat").is_some() {
            sky.obs_lat = Some(node.get_double("ObsLat", 0.0));
        }
        if node.get("ObsLon").is_some() {
            sky.obs_lon = Some(node.get_double("ObsLon", 0.0));
        }
        if node.get("ObsAlt").is_some() {
            sky.obs_alt = Some(node.get_double("ObsAlt", 0.0));
        }
        if node.get("Dtai").is_some() {
            sky.dtai = Some(node.get_double("Dtai", 0.0));
        }
        if node.get("Dut1").is_some() {
            sky.dut1 = Some(node.get_double("Dut1", 0.0));
        }

        // SkyFrame-specific.
        let proj = node.get_string("Proj", "");
        if !proj.is_empty() {
            sky.projection = Some(proj);
        }

        sky.neglon = node.get_int("NegLon", 0) != 0;

        if node.get("SkyTol").is_some() {
            sky.sky_tol = Some(node.get_double("SkyTol", 0.0));
        }

        // SRefIs.
        if node.get("SRefIs").is_some() {
            sky.sky_ref_is_set = true;
        }
        let srefis_str = node.get_string("SRefIs", "Ignored");
        sky.sky_ref_is = match srefis_str.to_ascii_uppercase().as_str() {
            "POLE" => SkyRefIs::Pole,
            "ORIGIN" => SkyRefIs::Origin,
            _ => SkyRefIs::Ignored,
        };

        // SRef1, SRef2.
        if node.get("SRef1").is_some() {
            sky.sky_ref[0] = node.get_double("SRef1", 0.0);
        }
        if node.get("SRef2").is_some() {
            sky.sky_ref[1] = node.get_double("SRef2", 0.0);
        }

        // SRefP1, SRefP2.
        if node.get("SRefP1").is_some() {
            sky.sky_ref_p[0] = node.get_double("SRefP1", 0.0);
        }
        if node.get("SRefP2").is_some() {
            sky.sky_ref_p[1] = node.get_double("SRefP2", 0.0);
        }

        sky.align_offset = node.get_int("AlOff", 0) != 0;

        // AlSys (alignment system).
        let alsys_str = node.get_string("AlSys", "");
        if !alsys_str.is_empty() {
            sky.align_system = SkySystem::parse(&alsys_str);
        }

        // ActUnt (default is 1 = true; only store when 0).
        let act_unt_val = node.get_int("ActUnt", 1);
        if act_unt_val == 0 {
            sky.act_unt = Some(false);
        }

        // Ident (Object-level identification string).
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            sky.ident = Some(ident);
        }

        // Read SkyAxis children.
        for i in 0..2 {
            let key = format!("Ax{}", i + 1);
            if let Some(axis_node) = node.get_object(&key) {
                if axis_node.class == "SkyAxis" {
                    let mut axis = SkyAxis::from_node(axis_node)?;
                    // When IsLat is absent, infer from position (axis 1 = lon, axis 2 = lat).
                    if axis_node.get("IsLat").is_none() {
                        axis.is_latitude = i == 1;
                    }
                    sky.axes[i] = axis;
                }
            }
        }

        Ok(sky)
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

    // -----------------------------------------------------------------------
    // Offset coordinate integration (convert / map_from)
    // -----------------------------------------------------------------------

    #[test]
    fn convert_with_origin_offset() {
        // Source frame has offset coordinates; target is standard FK5.
        let mut src = SkyFrame::new(SkySystem::Fk5);
        src.set_sky_ref(1.0, 0.5);
        src.set_sky_ref_p(1.0, 0.5 + 0.1);
        src.set_sky_ref_is(SkyRefIs::Origin);

        let dst = SkyFrame::new(SkySystem::Fk5);

        let fs = src.convert(&dst, &[]).unwrap().unwrap();

        // (0, 0) in offset should map to sky_ref in standard.
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = fs.transform(&pts, true).unwrap();
        let p = out.point(0);

        // Compare with wrapping tolerance.
        let lon_diff = (p[0] - 1.0).rem_euclid(2.0 * PI);
        let lon_err = lon_diff.min(2.0 * PI - lon_diff);
        assert!(lon_err < 1.0e-10, "lon mismatch: {:.2e}", lon_err);
        assert_close(p[1], 0.5, 1.0e-10);
    }

    #[test]
    fn convert_to_offset_frame() {
        // Source is standard FK5; target has offset coordinates.
        let src = SkyFrame::new(SkySystem::Fk5);

        let mut dst = SkyFrame::new(SkySystem::Fk5);
        dst.set_sky_ref(1.0, 0.5);
        dst.set_sky_ref_p(1.0, 0.5 + 0.1);
        dst.set_sky_ref_is(SkyRefIs::Origin);

        let fs = src.convert(&dst, &[]).unwrap().unwrap();

        // sky_ref in standard should map to (0, 0) in offset.
        let pts = crate::coords::PointSet::from_rows([[1.0, 0.5]]);
        let out = fs.transform(&pts, true).unwrap();
        let p = out.point(0);

        // The longitude at (0,0) may come out as 0 or +-2pi.
        let lon_diff = p[0].rem_euclid(2.0 * PI);
        let lon_err = lon_diff.min(2.0 * PI - lon_diff);
        assert!(lon_err < 1.0e-10, "lon mismatch: {:.2e}", lon_err);
        assert_close(p[1], 0.0, 1.0e-10);
    }

    #[test]
    fn convert_between_offset_frames_round_trip() {
        // Both source and target have offsets; round-trip through convert.
        let mut src = SkyFrame::new(SkySystem::Fk5);
        src.set_sky_ref(0.5, 0.3);
        src.set_sky_ref_p(0.5, 0.3 + 0.1);
        src.set_sky_ref_is(SkyRefIs::Origin);

        let mut dst = SkyFrame::new(SkySystem::Fk5);
        dst.set_sky_ref(2.0, -0.4);
        dst.set_sky_ref_p(2.0, -0.4 + 0.1);
        dst.set_sky_ref_is(SkyRefIs::Origin);

        let fs_fwd = src.convert(&dst, &[]).unwrap().unwrap();

        // Pick a point in source offset coords, convert to dst offset,
        // then convert back.
        let test_lon = 0.1;
        let test_lat = 0.2;
        let pts = crate::coords::PointSet::from_rows([[test_lon, test_lat]]);
        let mid = fs_fwd.transform(&pts, true).unwrap();
        let back = fs_fwd.transform(&mid, false).unwrap();
        let p = back.point(0);

        assert_close(p[0], test_lon, 1.0e-9);
        assert_close(p[1], test_lat, 1.0e-9);
    }

    #[test]
    fn map_from_with_offsets() {
        // Test map_from with offset source.
        let mut prev = SkyFrame::new(SkySystem::Fk5);
        prev.set_sky_ref(1.0, 0.5);
        prev.set_sky_ref_p(1.0, 0.5 + 0.1);
        prev.set_sky_ref_is(SkyRefIs::Origin);

        let cur = SkyFrame::new(SkySystem::Fk5);

        let mapping = cur.map_from(&prev).unwrap();

        // (0,0) in prev (offset) should map to sky_ref in standard.
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = mapping.transform(&pts, true).unwrap();
        let p = out.point(0);

        let lon_diff = (p[0] - 1.0).rem_euclid(2.0 * PI);
        let lon_err = lon_diff.min(2.0 * PI - lon_diff);
        assert!(lon_err < 1.0e-10, "lon mismatch: {:.2e}", lon_err);
        assert_close(p[1], 0.5, 1.0e-10);
    }
}
