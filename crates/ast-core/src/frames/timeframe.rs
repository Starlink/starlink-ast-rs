//! The time coordinate frame (TimeFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame};
use crate::mappings::helpers::{ast_equal, is_bad};
use crate::mappings::{DynMapping, TimeCvtType, TimeMap, UnitMap};
use crate::serial::{Node, NodeValue, Serializable};

// ---------------------------------------------------------------------------
// TimeSystem — time coordinate system type
// ---------------------------------------------------------------------------

/// Time coordinate system type.
///
/// Each variant corresponds to a way of expressing a time coordinate value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeSystem {
    /// Modified Julian Date (days).
    Mjd,
    /// Julian Date (days).
    Jd,
    /// Julian Epoch (years).
    Jepoch,
    /// Besselian Epoch (years).
    Bepoch,
}

impl TimeSystem {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mjd => "MJD",
            Self::Jd => "JD",
            Self::Jepoch => "JEPOCH",
            Self::Bepoch => "BEPOCH",
        }
    }

    /// Parse from string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "MJD" => Some(Self::Mjd),
            "JD" => Some(Self::Jd),
            "JEPOCH" | "JEP" => Some(Self::Jepoch),
            "BEPOCH" | "BEP" => Some(Self::Bepoch),
            _ => None,
        }
    }

    /// Default units for this time system.
    pub fn default_units(&self) -> &'static str {
        match self {
            Self::Mjd | Self::Jd => "d",
            Self::Jepoch | Self::Bepoch => "yr",
        }
    }

    /// Human-readable label for this system.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Mjd => "Modified Julian Date",
            Self::Jd => "Julian Date",
            Self::Jepoch => "Julian Epoch",
            Self::Bepoch => "Besselian Epoch",
        }
    }
}

// ---------------------------------------------------------------------------
// TimeScale — time scale
// ---------------------------------------------------------------------------

/// Time scale for time coordinate frames.
///
/// The first group (UT1, GMST, LAST, LMST) defines time in terms of
/// the orientation of the earth. The second group (TAI, UTC, TT, TDB,
/// TCB, TCG, LT) defines time in terms of atomic processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeScale {
    /// International Atomic Time.
    Tai,
    /// Coordinated Universal Time.
    Utc,
    /// Universal Time (UT1).
    Ut1,
    /// Greenwich Mean Sidereal Time.
    Gmst,
    /// Local Apparent Sidereal Time.
    Last,
    /// Local Mean Sidereal Time.
    Lmst,
    /// Terrestrial Time.
    Tt,
    /// Barycentric Dynamical Time.
    Tdb,
    /// Barycentric Coordinate Time.
    Tcb,
    /// Geocentric Coordinate Time.
    Tcg,
    /// Local Time (UTC + offset).
    Lt,
}

impl TimeScale {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tai => "TAI",
            Self::Utc => "UTC",
            Self::Ut1 => "UT1",
            Self::Gmst => "GMST",
            Self::Last => "LAST",
            Self::Lmst => "LMST",
            Self::Tt => "TT",
            Self::Tdb => "TDB",
            Self::Tcb => "TCB",
            Self::Tcg => "TCG",
            Self::Lt => "LT",
        }
    }

    /// Parse from string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "TAI" => Some(Self::Tai),
            "UTC" => Some(Self::Utc),
            "UT1" => Some(Self::Ut1),
            "GMST" => Some(Self::Gmst),
            "LAST" => Some(Self::Last),
            "LMST" => Some(Self::Lmst),
            "TT" => Some(Self::Tt),
            "TDB" => Some(Self::Tdb),
            "TCB" => Some(Self::Tcb),
            "TCG" => Some(Self::Tcg),
            "LT" => Some(Self::Lt),
            _ => None,
        }
    }

    /// Whether this timescale is in the "earth rotation" group
    /// (requires DUT1 for conversion to/from the atomic group).
    pub fn is_earth_rotation(&self) -> bool {
        matches!(self, Self::Ut1 | Self::Gmst | Self::Last | Self::Lmst)
    }
}

// ---------------------------------------------------------------------------
// TimeFrame
// ---------------------------------------------------------------------------

/// A coordinate frame for time coordinates.
///
/// `TimeFrame` is a 1-axis Frame representing time position. It supports
/// a range of coordinate systems (MJD, JD, Julian epoch, Besselian epoch)
/// and time scales (TAI, UTC, TT, TDB, UT1, GMST, LAST, LMST, TCB, TCG, LT).
///
/// Construct via [`TimeFrame::new`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, TimeFrame, TimeSystem, TimeScale};
///
/// let tf = TimeFrame::new(TimeSystem::Mjd);
/// assert_eq!(tf.naxes(), 1);
/// assert_eq!(tf.system(), TimeSystem::Mjd);
/// assert_eq!(tf.timescale(), TimeScale::Tai);
/// ```
#[derive(Debug, Clone)]
pub struct TimeFrame {
    // Frame-level attributes
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>, // MJD

    // Observer
    obs_lat: Option<f64>, // geodetic latitude (radians)
    obs_lon: Option<f64>, // geodetic longitude (radians)
    obs_alt: Option<f64>, // altitude (metres)
    dut1: Option<f64>,    // UT1-UTC (seconds)
    dtai: Option<f64>,    // TAI-UTC (seconds)

    // TimeFrame-specific
    system: TimeSystem,
    /// Whether `system` was set explicitly (mirrors C `astTestSystem`).
    system_set: bool,
    timescale: TimeScale,
    /// Whether `timescale` was set explicitly. Mirrors `astTestTimeScale`.
    timescale_set: bool,
    time_origin: Option<f64>, // in default units for current system
    lt_offset: Option<f64>,   // Local Time offset from UTC (hours)
    align_timescale: Option<TimeScale>,
    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,

    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
}

impl TimeFrame {
    /// Create a new `TimeFrame` with the given time coordinate system.
    ///
    /// Defaults: domain = "TIME", timescale = TAI (or TT for Bepoch).
    pub fn new(system: TimeSystem) -> Self {
        let timescale = if system == TimeSystem::Bepoch {
            TimeScale::Tt
        } else {
            TimeScale::Tai
        };
        Self {
            title: None,
            // None = use the default "TIME"; mirrors astTestDomain.
            domain: None,
            epoch: None,
            obs_lat: None,
            obs_lon: None,
            obs_alt: None,
            dut1: None,
            dtai: None,
            system,
            // The caller supplied an explicit System, equivalent to C
            // `astTimeFrame("System=...")` invoking astSetSystem — so
            // astTestSystem is true regardless of the value. C tracks
            // set-ness independently of the value (`this->system !=
            // AST__BADSYSTEM`, frame.c:13822); the unset default is
            // AST__MJD (timeframe.c:2911), reachable only via
            // clear_system().
            system_set: true,
            timescale,
            timescale_set: false,
            time_origin: None,
            lt_offset: None,
            align_timescale: None,
            act_unt: None,
            ident: None,
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
        }
    }

    /// Whether `System` has been explicitly set on this TimeFrame.
    /// Mirrors C `astTestSystem`.
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear System back to its class default. Mirrors C
    /// `astClearSystem`: after clearing, `astGetSystem` returns the
    /// TimeFrame default `AST__MJD` (timeframe.c:2911).
    pub fn clear_system(&mut self) {
        self.system = TimeSystem::Mjd;
        self.system_set = false;
    }

    /// Whether `TimeScale` has been explicitly set.
    pub fn test_timescale(&self) -> bool {
        self.timescale_set
    }

    /// Frame Title accessor. Returns the empty string when unset.
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }

    /// Whether Title has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.title.is_some()
    }

    /// Clear the Title attribute.
    pub fn clear_title(&mut self) {
        self.title = None;
    }

    /// Whether `TimeOrigin` has been explicitly set.
    pub fn test_time_origin(&self) -> bool {
        self.time_origin.is_some()
    }

    /// Whether `LtOffset` has been explicitly set.
    pub fn test_lt_offset(&self) -> bool {
        self.lt_offset.is_some()
    }

    /// Whether `AlignTimeScale` has been explicitly set.
    pub fn test_align_timescale(&self) -> bool {
        self.align_timescale.is_some()
    }

    // --- TimeFrame-specific attributes ---

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Return the time coordinate system.
    pub fn system(&self) -> TimeSystem {
        self.system
    }

    /// Set the time coordinate system.
    pub fn set_system(&mut self, sys: TimeSystem) {
        self.system = sys;
        self.system_set = true;
    }

    /// Return the time scale.
    pub fn timescale(&self) -> TimeScale {
        self.timescale
    }

    /// Set the time scale.
    ///
    /// Note: Besselian epoch system only supports TT timescale.
    pub fn set_timescale(&mut self, ts: TimeScale) {
        self.timescale = ts;
        self.timescale_set = true;
    }

    /// Return the alignment time scale.
    ///
    /// Defaults to UT1 if the current timescale is an earth-rotation scale
    /// (UT1, GMST, LAST, LMST), otherwise defaults to TAI.
    pub fn align_timescale(&self) -> TimeScale {
        self.align_timescale.unwrap_or_else(|| {
            if self.timescale.is_earth_rotation() {
                TimeScale::Ut1
            } else {
                TimeScale::Tai
            }
        })
    }

    /// Set the alignment time scale.
    pub fn set_align_timescale(&mut self, ts: TimeScale) {
        self.align_timescale = Some(ts);
    }

    /// Return the time origin (in default units for current system).
    pub fn time_origin(&self) -> f64 {
        self.time_origin.unwrap_or(0.0)
    }

    /// Set the time origin (in default units for current system).
    pub fn set_time_origin(&mut self, origin: f64) {
        self.time_origin = Some(origin);
    }

    /// Clear the time origin.
    pub fn clear_time_origin(&mut self) {
        self.time_origin = None;
    }

    /// Return the Local Time offset from UTC (hours).
    pub fn lt_offset(&self) -> f64 {
        self.lt_offset.unwrap_or(0.0)
    }

    /// Set the Local Time offset from UTC (hours).
    pub fn set_lt_offset(&mut self, hours: f64) {
        self.lt_offset = Some(hours);
    }

    /// Return the epoch as MJD.
    ///
    /// Defaults to J2000.0 (MJD 51544.5) if not set.
    pub fn epoch_mjd(&self) -> f64 {
        self.epoch.unwrap_or(51544.5)
    }

    /// Return the epoch as a decimal year (Besselian if MJD < J1984,
    /// Julian otherwise — mirrors `frame.c::Dump`).
    pub fn epoch_year(&self) -> f64 {
        let mjd = self.epoch_mjd();
        if mjd < crate::pal::epj2d(1984.0) {
            crate::pal::epb(mjd)
        } else {
            crate::pal::epj(mjd)
        }
    }

    /// Whether the Epoch attribute has been explicitly set. Mirrors
    /// `astTestEpoch`.
    pub fn test_epoch(&self) -> bool {
        self.epoch.is_some()
    }

    /// Set the epoch as MJD.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        self.epoch = Some(mjd);
    }

    /// Set the epoch as a decimal year (IAU 1984 rule:
    /// year < 1984 ⇒ Besselian, ≥ 1984 ⇒ Julian).
    pub fn set_epoch_year(&mut self, year: f64) {
        self.epoch = Some(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the title.
    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    /// Set the domain.
    pub fn set_domain(&mut self, domain: &str) {
        self.domain = Some(domain.to_string());
    }

    /// Test whether the domain attribute has been explicitly set.
    /// Mirrors C's `astTestDomain` for `TimeFrame`.
    pub fn test_domain(&self) -> bool {
        self.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default `"TIME"`.
    pub fn clear_domain(&mut self) {
        self.domain = None;
    }

    /// Set the observer geodetic latitude (radians).
    pub fn set_obs_lat(&mut self, lat: f64) {
        self.obs_lat = Some(lat);
    }

    /// Set the observer geodetic longitude (radians).
    pub fn set_obs_lon(&mut self, lon: f64) {
        self.obs_lon = Some(lon);
    }

    /// Set the observer altitude (metres).
    pub fn set_obs_alt(&mut self, alt: f64) {
        self.obs_alt = Some(alt);
    }

    /// Return the DUT1 value (UT1-UTC in seconds).
    pub fn get_dut1(&self) -> f64 {
        self.dut1.unwrap_or(0.0)
    }

    /// Set the DUT1 value (UT1-UTC in seconds).
    pub fn set_dut1(&mut self, dut1: f64) {
        self.dut1 = Some(dut1);
    }

    /// Return the DTAI value (TAI-UTC in seconds).
    pub fn get_dtai(&self) -> f64 {
        self.dtai.unwrap_or(BAD_VALUE)
    }

    /// Set the DTAI value (TAI-UTC in seconds).
    pub fn set_dtai(&mut self, dtai: f64) {
        self.dtai = Some(dtai);
    }

    // --- Serialisation helpers ---

    /// Return the default title: "SystemLabel [TimeScale]".
    fn default_title(&self) -> String {
        format!("{} [{}]", self.system.label(), self.timescale.as_str())
    }

    /// Return a human-readable description for a unit abbreviation.
    fn unit_description(units: &str) -> &'static str {
        match units {
            "d" => "day",
            "yr" => "year",
            "s" => "second",
            _ => "",
        }
    }

    // --- Conversion helpers ---

    /// Convert a time_origin value from the given system to MJD.
    ///
    /// This mirrors the C `ToMJD` function.
    fn to_mjd(system: TimeSystem, val: f64) -> f64 {
        if is_bad(val) {
            return BAD_VALUE;
        }
        match system {
            TimeSystem::Mjd => val,
            TimeSystem::Jd => {
                // JD -> MJD: subtract 2400000.5
                val - 2_400_000.5
            }
            TimeSystem::Jepoch => {
                // Julian epoch to MJD via PAL
                crate::pal::epj2d(val)
            }
            TimeSystem::Bepoch => {
                // Besselian epoch to MJD via PAL
                crate::pal::epb2d(val)
            }
        }
    }

    /// Build the TimeMap chain for converting from `self` (source) to `result` (target).
    ///
    /// Follows the C `MakeMap` algorithm:
    /// 1. Convert source system → MJD (if not already MJD)
    /// 2. Convert source timescale → TAI (the common intermediate)
    /// 3. Convert TAI → target timescale
    /// 4. Convert MJD → target system (if not already MJD)
    fn make_time_mapping(&self, result: &TimeFrame) -> AstResult<DynMapping> {
        let sys1 = self.system;
        let sys2 = result.system;
        let ts1 = self.timescale;
        let ts2 = result.timescale;
        let off1 = self.time_origin();
        let off2 = result.time_origin();

        // Simple case: same system, timescale, and origin.
        if ts1 == ts2 && sys1 == sys2 && ast_equal(off1, off2) {
            return Ok(Box::new(UnitMap::new(1)));
        }

        // Simple case: same system and timescale, only offset differs.
        if ts1 == ts2 && sys1 == sys2 {
            let shift = off1 - off2;
            let mut tm = TimeMap::new();
            tm.add(TimeCvtType::MjdToMjd, &[off1, off2]);
            // But that only works for MJD. For other systems we need to
            // go through the full path. Actually MjdToMjd just adds the
            // shift in-place, regardless of system since the unit is the same.
            // The C code handles this via a ShiftMap. We can use MjdToMjd
            // which adds args[0]-args[1] to the value.
            if shift == 0.0 {
                return Ok(Box::new(UnitMap::new(1)));
            }
            return Ok(Box::new(tm));
        }

        // Full conversion path.
        let mut timemap = TimeMap::new();

        // The MJD offset corresponding to the source origin.
        let mjd_off1 = Self::to_mjd(sys1, off1);

        // Step 1: Convert source system → MJD (if needed).
        // args = [source_offset, mjd_offset]
        match sys1 {
            TimeSystem::Mjd => {} // already MJD
            TimeSystem::Jd => {
                timemap.add(TimeCvtType::JdToMjd, &[off1, mjd_off1]);
            }
            TimeSystem::Jepoch => {
                timemap.add(TimeCvtType::JepToMjd, &[off1, mjd_off1]);
            }
            TimeSystem::Bepoch => {
                timemap.add(TimeCvtType::BepToMjd, &[off1, mjd_off1]);
            }
        }

        // Step 2 & 3: Convert between timescales (via TAI as intermediate).
        if ts1 != ts2 {
            // Args for most conversions: [mjd_offset, -obs_lon, obs_lat, obs_alt, dtai]
            // Note: Frame stores longitude as +ve eastwards, but TimeMap needs +ve westwards.
            let obs_lon = -(self.obs_lon.unwrap_or(0.0));
            let obs_lat = self.obs_lat.unwrap_or(0.0);
            let obs_alt = self.obs_alt.unwrap_or(0.0);
            let dtai = self.get_dtai();
            let args = [mjd_off1, obs_lon, obs_lat, obs_alt, dtai];
            let args_ut = [self.get_dut1()];
            let args_lt = [self.lt_offset()]; // LTOffset in hours (TimeMap divides by 24)
            let args_tai = [mjd_off1, dtai];

            // Convert source timescale → TAI.
            match ts1 {
                TimeScale::Tai => {} // already TAI
                TimeScale::Utc => {
                    timemap.add(TimeCvtType::UtcToTai, &args_tai);
                }
                TimeScale::Tt => {
                    timemap.add(TimeCvtType::TtToTai, &args[..1]);
                }
                TimeScale::Tdb => {
                    timemap.add(TimeCvtType::TdbToTt, &args);
                    timemap.add(TimeCvtType::TtToTai, &args[..1]);
                }
                TimeScale::Tcg => {
                    timemap.add(TimeCvtType::TcgToTt, &args[..1]);
                    timemap.add(TimeCvtType::TtToTai, &args[..1]);
                }
                TimeScale::Tcb => {
                    timemap.add(TimeCvtType::TcbToTdb, &args[..1]);
                    timemap.add(TimeCvtType::TdbToTt, &args);
                    timemap.add(TimeCvtType::TtToTai, &args[..1]);
                }
                TimeScale::Lt => {
                    timemap.add(TimeCvtType::LtToUtc, &args_lt);
                    timemap.add(TimeCvtType::UtcToTai, &args_tai);
                }
                TimeScale::Ut1 => {
                    timemap.add(TimeCvtType::UtToUtc, &args_ut);
                    timemap.add(TimeCvtType::UtcToTai, &args_tai);
                }
                TimeScale::Gmst => {
                    timemap.add(TimeCvtType::GmstToUt, &args[..1]);
                    timemap.add(TimeCvtType::UtToUtc, &args_ut);
                    timemap.add(TimeCvtType::UtcToTai, &args_tai);
                }
                TimeScale::Last => {
                    timemap.add(TimeCvtType::LastToLmst, &args[..3]);
                    timemap.add(TimeCvtType::LmstToGmst, &args[..3]);
                    timemap.add(TimeCvtType::GmstToUt, &args[..1]);
                    timemap.add(TimeCvtType::UtToUtc, &args_ut);
                    timemap.add(TimeCvtType::UtcToTai, &args_tai);
                }
                TimeScale::Lmst => {
                    timemap.add(TimeCvtType::LmstToGmst, &args[..3]);
                    timemap.add(TimeCvtType::GmstToUt, &args[..1]);
                    timemap.add(TimeCvtType::UtToUtc, &args_ut);
                    timemap.add(TimeCvtType::UtcToTai, &args_tai);
                }
            }

            // Convert TAI → target timescale.
            // Use result's observer properties for target-side conversions.
            let obs_lon2 = -(result.obs_lon.unwrap_or(0.0));
            let obs_lat2 = result.obs_lat.unwrap_or(0.0);
            let obs_alt2 = result.obs_alt.unwrap_or(0.0);
            let dtai2 = result.get_dtai();
            let args2 = [mjd_off1, obs_lon2, obs_lat2, obs_alt2, dtai2];
            let args_ut2 = [result.get_dut1()];
            let args_lt2 = [result.lt_offset()]; // LTOffset in hours (TimeMap divides by 24)
            let args_tai2 = [mjd_off1, dtai2];

            match ts2 {
                TimeScale::Tai => {} // already TAI
                TimeScale::Utc => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai2);
                }
                TimeScale::Tt => {
                    timemap.add(TimeCvtType::TaiToTt, &args2[..1]);
                }
                TimeScale::Tdb => {
                    timemap.add(TimeCvtType::TaiToTt, &args2[..1]);
                    timemap.add(TimeCvtType::TtToTdb, &args2);
                }
                TimeScale::Tcg => {
                    timemap.add(TimeCvtType::TaiToTt, &args2[..1]);
                    timemap.add(TimeCvtType::TtToTcg, &args2[..1]);
                }
                TimeScale::Tcb => {
                    timemap.add(TimeCvtType::TaiToTt, &args2[..1]);
                    timemap.add(TimeCvtType::TtToTdb, &args2);
                    timemap.add(TimeCvtType::TdbToTcb, &args2[..1]);
                }
                TimeScale::Lt => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai2);
                    timemap.add(TimeCvtType::UtcToLt, &args_lt2);
                }
                TimeScale::Ut1 => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai2);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut2);
                }
                TimeScale::Gmst => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai2);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut2);
                    timemap.add(TimeCvtType::UtToGmst, &args2[..1]);
                }
                TimeScale::Last => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai2);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut2);
                    timemap.add(TimeCvtType::UtToGmst, &args2[..1]);
                    timemap.add(TimeCvtType::GmstToLmst, &args2[..3]);
                    timemap.add(TimeCvtType::LmstToLast, &args2[..3]);
                }
                TimeScale::Lmst => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai2);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut2);
                    timemap.add(TimeCvtType::UtToGmst, &args2[..1]);
                    timemap.add(TimeCvtType::GmstToLmst, &args2[..3]);
                }
            }
        }

        // Step 4: Convert MJD → target system (if needed).
        // args = [mjd_offset, target_offset]
        match sys2 {
            TimeSystem::Mjd => {
                // If the offsets differ, add MjdToMjd to shift.
                if !ast_equal(mjd_off1, off2) {
                    timemap.add(TimeCvtType::MjdToMjd, &[mjd_off1, off2]);
                }
            }
            TimeSystem::Jd => {
                timemap.add(TimeCvtType::MjdToJd, &[mjd_off1, off2]);
            }
            TimeSystem::Jepoch => {
                timemap.add(TimeCvtType::MjdToJep, &[mjd_off1, off2]);
            }
            TimeSystem::Bepoch => {
                timemap.add(TimeCvtType::MjdToBep, &[mjd_off1, off2]);
            }
        }

        // If the timemap is empty, return a UnitMap.
        if timemap.is_empty() {
            return Ok(Box::new(UnitMap::new(1)));
        }

        Ok(Box::new(timemap))
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for TimeFrame {
    fn naxes(&self) -> usize {
        1
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    fn effective_label(&self, _axis: usize) -> String {
        self.system.label().to_string()
    }

    fn effective_unit(&self, _axis: usize) -> String {
        self.system.default_units().to_string()
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("TimeFrame has 1 axis, got axis={axis}"),
            ));
        }
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // The parent (non-date) Format default is `%1.7G` — 7 *significant*
        // digits, not 7 decimal places (frame.c GetDefaultFormat with the
        // default Digits=7). The `iso.N[T]` calendar-date Format is not yet
        // ported (it needs the pal calendar conversions); see
        // docs/issues/frame-mapping-c-port-gaps.md (6f).
        const DIGITS: usize = 7;
        Ok(crate::frames::axis_format::format_value(
            "%1.7G", DIGITS, value,
        ))
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("TimeFrame has 1 axis, got axis={axis}"),
            ));
        }
        // Parse a floating-point number from the start of the string.
        let trimmed = text.trim_start();
        let offset = text.len() - trimmed.len();
        let mut end = 0;
        for (i, c) in trimmed.char_indices() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                end = i + c.len_utf8();
            } else if end > 0 {
                break;
            }
        }
        if end == 0 {
            return Ok((BAD_VALUE, 0));
        }
        match trimmed[..end].parse::<f64>() {
            Ok(val) => Ok((val, offset + end)),
            Err(_) => Ok((BAD_VALUE, 0)),
        }
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        let va = a[0];
        let vb = b[0];
        if is_bad(va) || is_bad(vb) {
            return Ok(BAD_VALUE);
        }
        Ok((va - vb).abs())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        let va = a[0];
        let vb = b[0];
        let vc = c[0];
        if is_bad(va) || is_bad(vb) || is_bad(vc) {
            return Ok(BAD_VALUE);
        }
        let ab = va - vb;
        let cb = vc - vb;
        if ab * cb >= 0.0 {
            Ok(0.0)
        } else {
            Ok(std::f64::consts::PI)
        }
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        let vf = from[0];
        let vt = to[0];
        if is_bad(vf) || is_bad(vt) || is_bad(dist) {
            return Ok(vec![BAD_VALUE]);
        }
        let d = vt - vf;
        if d == 0.0 {
            Ok(vec![vf + dist])
        } else {
            let sign = d.signum();
            Ok(vec![vf + sign * dist])
        }
    }

    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        // Time coordinates are unbounded — no normalisation needed.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = previous.as_any().downcast_ref::<TimeFrame>();
        match prev {
            Some(prev_time) => prev_time.make_time_mapping(self),
            None => {
                // Previous was not a TimeFrame — return UnitMap.
                Ok(Box::new(UnitMap::new(1)))
            }
        }
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); TimeFrame
        // does not override it. A difference in System/TimeScale/Epoch/
        // TimeOrigin yields a non-UnitMap time conversion and so the
        // frames compare unequal.
        super::find_frame::frames_equal(self, other)
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
        self.domain.as_deref().unwrap_or("TIME")
    }

    fn test_domain(&self) -> bool {
        TimeFrame::test_domain(self)
    }

    fn ident(&self) -> &str {
        TimeFrame::ident(self)
    }

    // -----------------------------------------------------------------
    // Match-control attributes (mirror C `astGet/SetMinAxes` etc.).
    // -----------------------------------------------------------------

    fn min_axes(&self) -> usize {
        self.min_axes_opt.unwrap_or(1)
    }
    fn set_min_axes(&mut self, n: usize) {
        self.min_axes_opt = Some(n);
    }
    fn test_min_axes(&self) -> bool {
        self.min_axes_opt.is_some()
    }
    fn max_axes(&self) -> usize {
        self.max_axes_opt.unwrap_or(1)
    }
    fn set_max_axes(&mut self, n: usize) {
        self.max_axes_opt = Some(n);
    }
    fn test_max_axes(&self) -> bool {
        self.max_axes_opt.is_some()
    }
    fn match_end(&self) -> bool {
        self.match_end_opt.unwrap_or(false)
    }
    fn set_match_end(&mut self, v: bool) {
        self.match_end_opt = Some(v);
    }
    fn test_match_end(&self) -> bool {
        self.match_end_opt.is_some()
    }
    fn preserve_axes(&self) -> bool {
        self.preserve_axes_opt.unwrap_or(false)
    }
    fn set_preserve_axes(&mut self, v: bool) {
        self.preserve_axes_opt = Some(v);
    }
    fn test_preserve_axes(&self) -> bool {
        self.preserve_axes_opt.is_some()
    }
    fn permute(&self) -> bool {
        // TimeFrame is 1-axis: permutations are degenerate. C
        // inherits the parent Frame default of 1 (true).
        self.permute_opt.unwrap_or(true)
    }
    fn set_permute(&mut self, v: bool) {
        self.permute_opt = Some(v);
    }
    fn test_permute(&self) -> bool {
        self.permute_opt.is_some()
    }

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (timeframe.c).
    // -----------------------------------------------------------------

    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        timeframe_overlay(template, template_axes, self)
    }

    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // A specialised leaf frame can only be cast down to a plain
        // Frame (BasicFrame). Same-class is handled by the direct Match
        // (no cast needed), so only the BasicFrame target matters here.
        if other
            .as_any()
            .downcast_ref::<crate::frames::BasicFrame>()
            .is_some()
        {
            Ok(Some(Box::new(crate::frames::reduce_to_basic_frame(self)?)))
        } else {
            Ok(None)
        }
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        // Step 1: parent (Frame class) Match for cardinality + Domain.
        let parent = super::find_frame::default_match(self, target, match_sub)?;
        if parent.is_none() {
            return Ok(None);
        }

        // Step 2: walk astPrimaryFrame on each target axis looking
        // for a TimeFrame.
        let target_naxes = target.naxes();
        let mut target_axis0: Option<usize> = None;
        for iaxis in 0..target_naxes {
            let (primary, _) = crate::frames::specframe::primary_frame_walk(target, iaxis);
            if primary.as_any().downcast_ref::<TimeFrame>().is_some() {
                target_axis0 = Some(iaxis);
                break;
            }
        }
        let Some(target_axis0) = target_axis0 else {
            return Ok(None);
        };

        // Step 3: 1-axis associations.
        let template_axes_vec = vec![0i32];
        let target_axes_vec = vec![target_axis0 as i32];

        // Step 4: defer to target's SubFrame.
        let Some((map, result)) =
            target.sub_frame(Some(self), 1, &target_axes_vec, &template_axes_vec)?
        else {
            return Ok(None);
        };

        Ok(Some(super::find_frame::MatchResult {
            template_axes: template_axes_vec,
            target_axes: target_axes_vec,
            map,
            result,
        }))
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        timeframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        if target_axes.len() == 1 && target_axes[0] == 0 {
            return Ok(Box::new(self.clone()));
        }
        let mut bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        if self.test_domain() {
            bf.set_domain(self.domain().to_string());
        }
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// Match / SubFrame / Overlay helpers
// ---------------------------------------------------------------------------

/// Mirrors C `Overlay` for TimeFrame (timeframe.c). Only attributes
/// the template TimeFrame has explicitly set are transferred onto the
/// result. When `result` is itself a TimeFrame and overlaying System
/// would change the coordinate type, the result's Title is cleared
/// first so the new system's defaults take effect.
fn timeframe_overlay(
    template: &dyn Frame,
    _template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    let template_time = match template.as_any().downcast_ref::<TimeFrame>() {
        Some(t) => t,
        None => return Ok(()),
    };

    if let Some(result_time) = result.as_any_mut().downcast_mut::<TimeFrame>() {
        let old_system = result_time.system;
        let new_system = template_time.system;

        // If System changes (and template explicitly set it), clear
        // Title so the new system's default takes effect.
        if new_system != old_system && template_time.test_system() {
            result_time.clear_title();
        }

        // Frame-level numeric/observer attributes transferred by the
        // parent Frame Overlay (frame.c:8567-8575): Dtai, Dut1, Epoch,
        // ObsLat, ObsLon, ObsAlt — each only when set on the template.
        // Domain and Title are transferred by the dedicated blocks below.
        if let Some(v) = template_time.dtai {
            result_time.set_dtai(v);
        }
        if let Some(v) = template_time.dut1 {
            result_time.set_dut1(v);
        }
        if let Some(v) = template_time.epoch {
            result_time.set_epoch_mjd(v);
        }
        if let Some(v) = template_time.obs_lat {
            result_time.set_obs_lat(v);
        }
        if let Some(v) = template_time.obs_lon {
            result_time.set_obs_lon(v);
        }
        if let Some(v) = template_time.obs_alt {
            result_time.set_obs_alt(v);
        }
        if template_time.test_domain() {
            result_time.set_domain(template_time.domain());
        }
        if template_time.test_title() {
            if let Some(ref t) = template_time.title {
                result_time.set_title(t);
            }
        }
        if template_time.test_system() {
            result_time.set_system(template_time.system);
        }
        if template_time.test_timescale() {
            result_time.set_timescale(template_time.timescale);
        }
        if template_time.test_align_timescale() {
            if let Some(ts) = template_time.align_timescale {
                result_time.set_align_timescale(ts);
            }
        }
        if template_time.test_time_origin() {
            if let Some(v) = template_time.time_origin {
                result_time.set_time_origin(v);
            }
        }
        if template_time.test_lt_offset() {
            if let Some(v) = template_time.lt_offset {
                result_time.set_lt_offset(v);
            }
        }
        return Ok(());
    }

    // Result is not a TimeFrame — copy only Frame-level attributes
    // that have a setter on the result type.
    if template_time.test_domain() {
        if let Some(bf) = result
            .as_any_mut()
            .downcast_mut::<super::basic::BasicFrame>()
        {
            bf.set_domain(template_time.domain().to_string());
        }
    }
    Ok(())
}

/// Mirrors C `SubFrame` for TimeFrame.
fn timeframe_sub_frame(
    target: &TimeFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-TimeFrame branch.
    if result_naxes == 1 && target_axes.len() == 1 && target_axes[0] == 0 {
        // Result is a copy of the target, with template attributes
        // overlaid.
        let mut result = target.clone();
        if let Some(t) = template {
            timeframe_overlay(t, template_axes, &mut result)?;
        }

        // Build the conversion Mapping using TimeFrame's existing
        // make_time_mapping infrastructure.
        let map: DynMapping = target.make_time_mapping(&result)?;
        return Ok(Some((map, Box::new(result))));
    }

    // parent_subframe branch — produce a BasicFrame-shaped result
    // via the default Frame::SubFrame.
    super::find_frame::default_sub_frame(target, template, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for TimeFrame {
    fn class_name(&self) -> &'static str {
        "TimeFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of time coordinate system".to_string());

        // --- Frame-level attributes (before IsA Frame) ---
        // Order: Title, Naxes, Domain, Epoch, Lbl1, System, Uni1, Ax1, IsA Frame

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

        // Naxes = 1 (always active).
        node.push_int("Naxes", 1, Some("Number of coordinate axes".to_string()));

        // Domain: active if explicitly set to non-"TIME", otherwise unset "TIME".
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() && domain != "TIME" {
                node.push_string(
                    "Domain",
                    domain,
                    Some("Coordinate system domain".to_string()),
                );
            } else {
                node.push_unset(
                    "Domain",
                    NodeValue::String("TIME".to_string()),
                    Some("Coordinate system domain".to_string()),
                );
            }
        } else {
            node.push_unset(
                "Domain",
                NodeValue::String("TIME".to_string()),
                Some("Coordinate system domain".to_string()),
            );
        }

        // Epoch: stored internally as MJD, written as Besselian /
        // Julian decimal year per the IAU 1984 rule (mirrors
        // `frame.c::Dump` lines 14325-14336).
        let epoch_mjd = self.epoch.unwrap_or(51544.5);
        let bessyr = epoch_mjd < crate::pal::epj2d(1984.0);
        let epoch_year = if bessyr {
            crate::pal::epb(epoch_mjd)
        } else {
            crate::pal::epj(epoch_mjd)
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

        // Axis label (always unset with system-appropriate default).
        node.push_unset(
            "Lbl1",
            NodeValue::String(self.system.label().to_string()),
            Some("Label for axis 1".to_string()),
        );

        // System: active when explicitly set, otherwise written commented
        // with the class default value. Mirrors `frame.c::Dump`
        // (`set = TestSystem`; value = GetSystem which returns AST__MJD
        // for an unset TimeFrame, timeframe.c:2911).
        if self.system_set {
            node.push_string(
                "System",
                self.system.as_str(),
                Some("Coordinate system type".to_string()),
            );
        } else {
            node.push_unset(
                "System",
                NodeValue::String(TimeSystem::Mjd.as_str().to_string()),
                Some("Coordinate system type".to_string()),
            );
        }

        // Axis units (always unset with system-appropriate default).
        let units = self.system.default_units();
        let unit_desc = Self::unit_description(units);
        let uni_comment = format!("Units for axis 1 ({unit_desc})");
        node.push_unset(
            "Uni1",
            NodeValue::String(units.to_string()),
            Some(uni_comment),
        );

        // Per-axis Axis child object (empty Axis).
        let mut axis_node = Node::new("Axis");
        axis_node.class_comment = Some("Coordinate axis".to_string());
        node.push_object("Ax1", axis_node, Some("Axis number 1".to_string()));

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

        // Dut1 (if set).
        if let Some(dut1) = self.dut1 {
            node.push_double("Dut1", dut1, Some("UT1-UTC (s)".to_string()));
        }

        // Dtai (if set).
        if let Some(dtai) = self.dtai {
            node.push_double("Dtai", dtai, Some("TAI-UTC (s)".to_string()));
        }

        // ActUnt: active if explicitly set to false.
        if let Some(false) = self.act_unt {
            node.push_int(
                "ActUnt",
                0,
                Some("Unit strings do not affect alignment".to_string()),
            );
        }

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- TimeFrame-specific attributes (after IsA Frame) ---

        // TmScl (TimeScale): active when explicitly set, otherwise
        // written commented with the default value. Mirrors
        // `timeframe.c::Dump` lines 6799-6800 (`set = TestTimeScale`;
        // value = GetTimeScale, which is TT for BEPOCH else TAI — the
        // value `self.timescale` already holds when unset).
        if self.timescale_set {
            node.push_string(
                "TmScl",
                self.timescale.as_str(),
                Some("Time scale".to_string()),
            );
        } else {
            node.push_unset(
                "TmScl",
                NodeValue::String(self.timescale.as_str().to_string()),
                Some("Time scale".to_string()),
            );
        }

        // ATmScl (AlignTimeScale, only if explicitly set).
        if let Some(ref ats) = self.align_timescale {
            node.push_string(
                "ATmScl",
                ats.as_str(),
                Some("Alignment time scale".to_string()),
            );
        }

        // TmOrg (TimeOrigin, if set).
        if let Some(time_origin) = self.time_origin {
            if !is_bad(time_origin) {
                node.push_double("TmOrg", time_origin, Some("Time offset".to_string()));
            }
        }

        // LTOff (LTOffset, if set).
        if let Some(lt_offset) = self.lt_offset {
            node.push_double(
                "LTOff",
                lt_offset,
                Some("Local Time offset from UTC".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Read System. The unset default is MJD (timeframe.c:2911), so an
        // absent `System=` line decodes to an MJD frame with system_set
        // false.
        let sys_str = node.get_string("System", "MJD");
        let system = TimeSystem::parse(&sys_str).unwrap_or(TimeSystem::Mjd);
        let system_present = node.get("System").is_some();

        let mut tf = TimeFrame::new(system);
        // TimeFrame::new flags System as set; honour the serialised
        // set-ness instead (a commented `System=` line means unset).
        tf.system_set = system_present;

        // Title.
        let title = node.get_string("Title", "");
        if !title.is_empty() {
            tf.set_title(&title);
        }

        // Domain.
        let domain = node.get_string("Domain", "");
        if !domain.is_empty() {
            tf.domain = Some(domain);
        }

        // Epoch. AST Native stores the value as a decimal year
        // (Julian / Besselian); resolve via the IAU 1984 rule. See
        // `frame.c::SetAttrib` (line 10185) where the string flow
        // delegates to `astReadDateTime`.
        if node.get("Epoch").is_some() {
            tf.set_epoch_year(node.get_double("Epoch", 0.0));
        }

        // Observer attributes.
        if node.get("ObsLat").is_some() {
            tf.obs_lat = Some(node.get_double("ObsLat", 0.0));
        }
        if node.get("ObsLon").is_some() {
            tf.obs_lon = Some(node.get_double("ObsLon", 0.0));
        }
        if node.get("ObsAlt").is_some() {
            tf.obs_alt = Some(node.get_double("ObsAlt", 0.0));
        }

        // Dut1.
        if node.get("Dut1").is_some() {
            tf.dut1 = Some(node.get_double("Dut1", 0.0));
        }

        // Dtai.
        if node.get("Dtai").is_some() {
            tf.dtai = Some(node.get_double("Dtai", 0.0));
        }

        // TmScl (TimeScale). Use set_timescale to flip the
        // timescale_set flag so test_timescale reports true after
        // round-trip.
        let ts_str = node.get_string("TmScl", "");
        if !ts_str.is_empty() {
            if let Some(ts) = TimeScale::parse(&ts_str) {
                tf.set_timescale(ts);
            }
        }

        // ATmScl (AlignTimeScale).
        let ats_str = node.get_string("ATmScl", "");
        if !ats_str.is_empty() {
            tf.align_timescale = TimeScale::parse(&ats_str);
        }

        // TmOrg (TimeOrigin).
        if node.get("TmOrg").is_some() {
            tf.time_origin = Some(node.get_double("TmOrg", 0.0));
        }

        // LTOff (LTOffset).
        if node.get("LTOff").is_some() {
            tf.lt_offset = Some(node.get_double("LTOff", 0.0));
        }

        // ActUnt (default is 1 = true; only store when 0).
        let act_unt_val = node.get_int("ActUnt", 1);
        if act_unt_val == 0 {
            tf.act_unt = Some(false);
        }

        // Ident (Object-level identification string).
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            tf.ident = Some(ident);
        }

        Ok(tf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_set_tracking_independent_of_value() {
        // A frame built with an explicit System (even the default MJD)
        // reports System set, mirroring C `astTimeFrame("System=MJD")`.
        let f = TimeFrame::new(TimeSystem::Mjd);
        assert!(f.test_system());
        assert_eq!(f.system(), TimeSystem::Mjd);
        // Clearing reverts to the unset MJD default.
        let mut f = TimeFrame::new(TimeSystem::Jd);
        f.clear_system();
        assert!(!f.test_system());
        assert_eq!(f.system(), TimeSystem::Mjd);
    }

    #[test]
    fn match_template_timeframe_self() {
        let template = TimeFrame::new(TimeSystem::Jd);
        let target = TimeFrame::new(TimeSystem::Jd);
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("self-match");
        assert_eq!(m.template_axes, vec![0]);
        assert_eq!(m.target_axes, vec![0]);
    }

    #[test]
    fn match_template_timeframe_against_basicframe_fails() {
        let template = TimeFrame::new(TimeSystem::Jd);
        let target = crate::frames::BasicFrameBuilder::new(1).build().unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn sub_frame_timeframe_to_timeframe_identity() {
        let target = TimeFrame::new(TimeSystem::Jd);
        let template = TimeFrame::new(TimeSystem::Jd);
        let (map, result) = target
            .sub_frame(Some(&template), 1, &[0], &[0])
            .unwrap()
            .expect("identity sub_frame");
        assert_eq!(map.nin(), 1);
        assert_eq!(map.nout(), 1);
        let r = result.as_any().downcast_ref::<TimeFrame>().unwrap();
        assert_eq!(r.system(), TimeSystem::Jd);
    }

    #[test]
    fn overlay_template_copies_set_system() {
        let mut template = TimeFrame::new(TimeSystem::Mjd);
        template.set_system(TimeSystem::Jepoch);
        assert!(template.test_system());
        let target = TimeFrame::new(TimeSystem::Mjd);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<TimeFrame>().unwrap();
        assert_eq!(r.system(), TimeSystem::Jepoch);
    }

    #[test]
    fn overlay_template_skips_unset_system() {
        let mut template = TimeFrame::new(TimeSystem::Mjd);
        template.clear_system();
        assert!(!template.test_system());
        let target = TimeFrame::new(TimeSystem::Jd);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<TimeFrame>().unwrap();
        assert_eq!(r.system(), TimeSystem::Jd);
    }

    #[test]
    fn pick_axes_snapshot_full_returns_timeframe() {
        let target = TimeFrame::new(TimeSystem::Jd);
        let snap = target.pick_axes_snapshot(&[0]).unwrap();
        let r = snap.as_any().downcast_ref::<TimeFrame>().unwrap();
        assert_eq!(r.system(), TimeSystem::Jd);
    }
}
