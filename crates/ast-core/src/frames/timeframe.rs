//! The time coordinate frame (TimeFrame).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame};
use crate::mappings::helpers::{ast_equals, is_bad};
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, ShiftMap, TimeCvtType, TimeMap, UnitMap,
};
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

    /// Whether converting to this timescale depends on the observer's
    /// position (timeframe.c `CLOCK_SCALE`).
    pub(crate) fn is_clock_scale(&self) -> bool {
        matches!(self, Self::Lmst | Self::Last | Self::Tdb | Self::Tcb)
    }

    /// Whether converting to this timescale needs DUT1
    /// (timeframe.c `DUT1_SCALE`).
    pub(crate) fn is_dut1_scale(&self) -> bool {
        matches!(self, Self::Lmst | Self::Last | Self::Gmst | Self::Ut1)
    }

    /// The group of timescales this one is in, of the three between which
    /// conversion needs DTAI (timeframe.c `DTAI_SCALE`).
    pub(crate) fn dtai_group(&self) -> u8 {
        match self {
            Self::Lmst | Self::Last | Self::Gmst | Self::Ut1 | Self::Utc | Self::Lt => 1,
            Self::Tai | Self::Tt => 2,
            _ => 3,
        }
    }

    /// Whether converting to UTC needs LTOffset (timeframe.c
    /// `LTOFFSET_SCALE`).
    pub(crate) fn is_ltoffset_scale(&self) -> bool {
        matches!(self, Self::Lt)
    }
}

/// Whether two DTAI values are the same, as C's `EQUAL( a, b, 1.0E-6 )`
/// compares them in `MakeTimeMapping`: an absolute tolerance, with the
/// unset value (bad) equal only to itself.
fn dtai_equal(a: f64, b: f64) -> bool {
    if a == BAD_VALUE || b == BAD_VALUE {
        return a == b;
    }
    (a - b).abs() <= 1.0e-6
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
    /// The Frame-class attributes.
    attrs: super::frame_attrs::FrameAttrs,
    // Frame-level attributes

    // Observer

    // TimeFrame-specific
    system: TimeSystem,
    /// Whether `system` was set explicitly (mirrors C `astTestSystem`).
    system_set: bool,
    timescale: TimeScale,
    /// Whether `timescale` was set explicitly. Mirrors `astTestTimeScale`.
    timescale_set: bool,
    time_origin: Option<f64>, // in default units for current system
    /// The Axis-class attributes of the single axis.
    axis: super::axis::Axis,
    lt_offset: Option<f64>, // Local Time offset from UTC (hours)
    align_timescale: Option<TimeScale>,
    /// AlignSystem, when set; the default is MJD (timeframe.c:2858).
    align_system: Option<TimeSystem>,
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
}

impl TimeFrame {
    // Defaults of the Frame-class attributes, for the `frame_attrs_*`
    // macros.
    fn frame_default_title(&self) -> String {
        self.default_title()
    }
    fn frame_default_domain(&self) -> &'static str {
        "TIME"
    }
    /// The TimeOrigin as a TDB MJD when one is set, else J2000.0
    /// (timeframe.c `GetEpoch`).
    fn frame_default_epoch(&self) -> f64 {
        if self.time_origin.is_some() {
            if let Ok(mjd) = self.epoch_from_time_origin() {
                return mjd;
            }
        }
        super::frame_attrs::default_epoch_mjd()
    }
    fn frame_default_permute(&self) -> bool {
        true
    }

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
            attrs: super::frame_attrs::FrameAttrs::default(),
            // None = use the default "TIME"; mirrors astTestDomain.
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
            axis: super::axis::Axis::default(),
            lt_offset: None,
            align_timescale: None,
            align_system: None,
        }
    }

    /// A TimeFrame with no attribute set, as `astTimeFrame(" ")` makes one:
    /// its System is MJD by default rather than by being set.
    pub fn unset_system() -> Self {
        Self {
            system_set: false,
            ..Self::new(TimeSystem::Mjd)
        }
    }

    /// AlignSystem, set or default (MJD). Mirrors C `GetAlignSystem`
    /// (timeframe.c:2858).
    pub fn align_system(&self) -> TimeSystem {
        self.align_system.unwrap_or(TimeSystem::Mjd)
    }

    /// Set AlignSystem.
    pub fn set_align_system(&mut self, sys: TimeSystem) {
        self.align_system = Some(sys);
    }

    /// Whether AlignSystem is set.
    pub fn test_align_system(&self) -> bool {
        self.align_system.is_some()
    }

    /// Clear AlignSystem.
    pub fn clear_align_system(&mut self) {
        self.align_system = None;
    }

    /// Clear AlignTimeScale.
    pub fn clear_align_timescale(&mut self) {
        self.align_timescale = None;
    }

    /// Clear LTOffset.
    pub fn clear_lt_offset(&mut self) {
        self.lt_offset = None;
    }

    /// TimeOrigin in the TimeFrame's current units, as C's `TimeOrigin`
    /// attribute reports it (timeframe.c `GetTimeOriginCur`).
    pub fn time_origin_cur(&self) -> f64 {
        self.get_time_origin_cur()
    }

    /// Set TimeOrigin from a value in the TimeFrame's current units, as C's
    /// `TimeOrigin` attribute reads a number with no units (timeframe.c:4748,
    /// `ToUnits`).
    pub fn set_time_origin_cur(&mut self, value: f64) -> AstResult<()> {
        let def = self.system.default_units();
        let cur = self.unit_or_default().to_string();
        let Some(map) = crate::units::unit_mapper(&cur, def) else {
            return Err(AstError::new(
                ErrorCode::Badun,
                format!(
                    "astSetTimeOrigin(TimeFrame): Cannot convert the supplied attribute value from units of {cur} to {def}."
                ),
            ));
        };
        let v = transform_scalar(map.as_ref(), value, true)?;
        self.set_time_origin(v);
        Ok(())
    }

    /// Whether `System` has been explicitly set on this TimeFrame.
    /// Mirrors C `astTestSystem`.
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear System back to its class default. Mirrors C
    /// `astClearSystem`: after clearing, `astGetSystem` returns the
    /// TimeFrame default `AST__MJD` (timeframe.c:2911).
    ///
    /// As with [`Self::set_system`], the stored `TimeOrigin` is re-expressed
    /// into the new (default) system (C `OriginSystem`). If the old system was
    /// Besselian epoch, Unit and TimeScale are also cleared, since those are
    /// constrained while System is Besselian epoch.
    pub fn clear_system(&mut self) {
        let oldsys = self.system;
        self.system = TimeSystem::Mjd;
        self.system_set = false;

        if self.system != oldsys {
            self.origin_system(oldsys);
            self.axis.label = None;
            self.axis.symbol = None;
            self.clear_title();
            if oldsys == TimeSystem::Bepoch {
                self.axis.unit = None;
                self.clear_timescale();
            }
        }
    }

    /// Whether `TimeScale` has been explicitly set.
    pub fn test_timescale(&self) -> bool {
        self.timescale_set
    }

    /// Frame Title accessor. Returns the empty string when unset.
    pub fn title(&self) -> String {
        Frame::title(self)
    }

    /// Whether Title has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.attrs.title.is_some()
    }

    /// Clear the Title attribute.
    pub fn clear_title(&mut self) {
        self.attrs.title = None;
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
        self.attrs.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.attrs.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Return the time coordinate system.
    pub fn system(&self) -> TimeSystem {
        self.system
    }

    /// Set the time coordinate system.
    ///
    /// `TimeOrigin` records an absolute instant, so changing the system
    /// re-expresses the stored origin into the new system (C `SetSystem`
    /// → `OriginSystem`). Besselian epoch additionally forces Unit="yr" and
    /// TimeScale=TT, applied (like C) before the system change so the origin is
    /// first brought onto the TT timescale in the old system.
    pub fn set_system(&mut self, sys: TimeSystem) {
        // Besselian epoch is defined in terms of TT and is always expressed in
        // years; force those before changing the system (C SetSystem).
        if sys == TimeSystem::Bepoch {
            self.axis.unit = Some("yr".to_string());
            self.set_timescale(TimeScale::Tt);
        }

        let oldsys = self.system;
        self.system = sys;
        self.system_set = true;

        if oldsys != sys {
            // Re-express the stored origin from the old system to the new one.
            self.origin_system(oldsys);
            // Attributes with System-specific defaults (timeframe.c:4844).
            self.axis.label = None;
            self.axis.symbol = None;
            self.clear_title();
        }
    }

    /// Return the time scale.
    pub fn timescale(&self) -> TimeScale {
        self.timescale
    }

    /// Set the time scale.
    ///
    /// `TimeOrigin` records an absolute instant, so changing the timescale
    /// re-expresses the stored origin into the new timescale (C `SetTimeScale`
    /// → `OriginScale`). The re-expression happens before the new timescale is
    /// stored, so it converts from the current timescale to `ts`.
    ///
    /// Note: Besselian epoch system only supports TT timescale.
    pub fn set_timescale(&mut self, ts: TimeScale) {
        self.origin_scale(ts);
        self.timescale = ts;
        self.timescale_set = true;
    }

    /// Clear the time scale, reverting to the system default (TT for Besselian
    /// epoch, otherwise TAI).
    ///
    /// Mirrors C `astClearTimeScale`: unless the System is Besselian epoch
    /// (whose timescale is always TT), the stored `TimeOrigin` is re-expressed
    /// into the default TAI timescale before the timescale is cleared.
    pub fn clear_timescale(&mut self) {
        if self.system != TimeSystem::Bepoch {
            self.origin_scale(TimeScale::Tai);
        }
        self.timescale = if self.system == TimeSystem::Bepoch {
            TimeScale::Tt
        } else {
            TimeScale::Tai
        };
        self.timescale_set = false;
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

    /// The Axis-class attributes of the single axis, after checking the
    /// 1-based `axis`.
    fn ax(&self, axis: usize) -> AstResult<&super::axis::Axis> {
        super::frame_attrs::validate_axis(axis, 1)?;
        Ok(&self.axis)
    }

    /// The Axis-class attributes of the single axis, to change.
    fn ax_mut(&mut self, axis: usize) -> AstResult<&mut super::axis::Axis> {
        super::frame_attrs::validate_axis(axis, 1)?;
        Ok(&mut self.axis)
    }

    /// Return the effective axis Unit string: the explicitly-set value, or
    /// the current system's default units.
    pub fn unit_or_default(&self) -> &str {
        self.axis
            .unit
            .as_deref()
            .unwrap_or(self.system.default_units())
    }

    /// The Format that has been set, or `""`, which no date format matches,
    /// as the numeric default C would report does not.
    fn set_format(&self) -> &str {
        self.axis.format.as_deref().unwrap_or("")
    }

    /// The axis label. Mirrors C `GetLabel` (timeframe.c:2579): a date Format
    /// overrides the system label with `"Date/Time"` (when a time field is
    /// present) or `"Date"` (date only); otherwise the system label is used,
    /// and — when a non-zero TimeOrigin is set — the origin is appended as an
    /// absolute `iso.0` date/time (` offset from <date>`, with a trailing
    /// ` 00:00:00` time-of-day stripped). A Local Time timescale appends the
    /// `(UTC±offset)` suffix.
    fn default_axis_label(&self) -> String {
        let (is_date, ndp, _sep) = date_format(self.set_format());
        let mut result = if is_date {
            if ndp >= 0 {
                "Date/Time".to_string()
            } else {
                "Date".to_string()
            }
        } else {
            self.system.label().to_string()
        };

        // Append the TimeOrigin offset as an absolute iso.0 date/time when a
        // non-zero origin is set (C timeframe.c:2670-2699).
        if !is_date && self.time_origin() != 0.0 {
            if let Ok(mut date) = self.format_iso0_zero() {
                // Strip a trailing "00:00:00" time-of-day, keeping the space
                // that precedes it — C only nulls the last 8 characters
                // (timeframe.c:2696-2698).
                if let Some(stripped) = date.strip_suffix("00:00:00") {
                    date = stripped.to_string();
                }
                result.push_str(" offset from ");
                result.push_str(&date);
            }
        }
        if !is_date {
            result = super::frame_attrs::relabel_for_unit(
                result,
                self.system.default_units(),
                self.axis.unit.as_deref(),
            );
        }

        // Local Time, whatever the label (C timeframe.c:2726-2734): indicate the offset from UTC (C timeframe.c:2726-2734).
        if self.timescale == TimeScale::Lt {
            let ltoff = self.lt_offset();
            if ltoff >= 0.0 {
                result = format!("{result} (UTC+{})", fmt_g(ltoff));
            } else {
                result = format!("{result} (UTC-{})", fmt_g(-ltoff));
            }
        }

        result
    }

    /// Return the Local Time offset from UTC (hours).
    pub fn lt_offset(&self) -> f64 {
        self.lt_offset.unwrap_or(0.0)
    }

    /// Set the Local Time offset from UTC (hours).
    pub fn set_lt_offset(&mut self, hours: f64) {
        self.lt_offset = Some(hours);
    }

    /// Return the epoch as an absolute TDB MJD.
    ///
    /// Mirrors C `timeframe.c::GetEpoch` (2458):
    /// - if the Epoch attribute is set, return it;
    /// - otherwise, if TimeOrigin is set, return the TimeOrigin converted to
    ///   an absolute TDB MJD (earth-rotation timescales are routed through
    ///   UT1 assuming DUT1 == 0, then UTC → TDB, since the default Epoch is
    ///   not critical and DUT1 may be unknown);
    /// - otherwise, the default J2000.0 (MJD 51544.5).
    pub fn epoch_mjd(&self) -> f64 {
        if let Some(e) = self.attrs.epoch {
            return e;
        }
        if self.time_origin.is_some() {
            if let Ok(mjd) = self.epoch_from_time_origin() {
                return mjd;
            }
        }
        51544.5
    }

    /// Convert the set TimeOrigin to an absolute TDB MJD. Mirrors the
    /// TimeOrigin branch of C `GetEpoch` (timeframe.c:2521-2568).
    ///
    /// The conversion maps are built with *zero* offsets (the TimeOrigin is
    /// the absolute value being transformed, not an axis offset), matching
    /// the `MakeMap(..., 0.0, 0.0, ...)` calls in C.
    fn epoch_from_time_origin(&self) -> AstResult<f64> {
        let origin = self.time_origin();
        let mut sys = self.system;
        let mut ts = self.timescale;
        let mut u = sys.default_units();
        let mut oldval = origin;

        // Epoch is a TDB value. Earth-rotation timescales (UT1/GMST/LAST/
        // LMST) cannot be converted to TDB without DUT1, so C first maps the
        // stored value to UT1 and then treats UT1 == UTC (i.e. DUT1 == 0).
        // We force DUT1 to zero on a clone for that hop.
        let zero_dut1 = if ts.is_earth_rotation() {
            let mut z = self.clone();
            z.attrs.dut1 = Some(0.0);
            Some(z)
        } else {
            None
        };

        if let Some(ref z) = zero_dut1 {
            let to_ut1 = z.make_map(sys, TimeSystem::Mjd, ts, TimeScale::Ut1, 0.0, 0.0, u, "d")?;
            oldval = transform_scalar(to_ut1.as_ref(), origin, true)?;
            // Treat the UT1 result as UTC (DUT1 == 0) for the TDB step.
            ts = TimeScale::Utc;
            sys = TimeSystem::Mjd;
            u = "d";
        }

        // Convert to TDB.
        let obs = zero_dut1.as_ref().unwrap_or(self);
        let map = obs.make_map(sys, TimeSystem::Mjd, ts, TimeScale::Tdb, 0.0, 0.0, u, "d")?;
        transform_scalar(map.as_ref(), oldval, true)
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
        self.attrs.epoch.is_some()
    }

    /// Set the epoch as MJD.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        self.attrs.epoch = Some(mjd);
    }

    /// Set the epoch as a decimal year (IAU 1984 rule:
    /// year < 1984 ⇒ Besselian, ≥ 1984 ⇒ Julian).
    pub fn set_epoch_year(&mut self, year: f64) {
        self.attrs.epoch = Some(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the title.
    pub fn set_title(&mut self, title: &str) {
        self.attrs.title = Some(title.to_string());
    }

    /// Set the domain.
    pub fn set_domain(&mut self, domain: &str) {
        self.attrs.domain = Some(domain.to_string());
    }

    /// Test whether the domain attribute has been explicitly set.
    /// Mirrors C's `astTestDomain` for `TimeFrame`.
    pub fn test_domain(&self) -> bool {
        self.attrs.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default `"TIME"`.
    pub fn clear_domain(&mut self) {
        self.attrs.domain = None;
    }

    /// Set the observer geodetic latitude (radians).
    pub fn set_obs_lat(&mut self, lat: f64) {
        self.attrs.obs_lat = Some(lat);
    }

    /// Set the observer geodetic longitude (radians).
    pub fn set_obs_lon(&mut self, lon: f64) {
        self.attrs.obs_lon = Some(lon);
    }

    /// Set the observer altitude (metres).
    pub fn set_obs_alt(&mut self, alt: f64) {
        self.attrs.obs_alt = Some(alt);
    }

    /// Return the DUT1 value (UT1-UTC in seconds).
    pub fn get_dut1(&self) -> f64 {
        self.attrs.dut1.unwrap_or(0.0)
    }

    /// Set the DUT1 value (UT1-UTC in seconds).
    pub fn set_dut1(&mut self, dut1: f64) {
        self.attrs.dut1 = Some(dut1);
    }

    /// Return the DTAI value (TAI-UTC in seconds).
    pub fn get_dtai(&self) -> f64 {
        self.attrs.dtai.unwrap_or(BAD_VALUE)
    }

    /// Set the DTAI value (TAI-UTC in seconds).
    pub fn set_dtai(&mut self, dtai: f64) {
        self.attrs.dtai = Some(dtai);
    }

    // --- Serialisation helpers ---

    /// Return the default title: "SystemLabel [TimeScale]".
    fn default_title(&self) -> String {
        // Mirrors C `GetTitle` (timeframe.c:3107). Start with the system
        // label (already capitalised), append the timescale in brackets when
        // it is *set* (and the system is not BEPOCH, which is TT-only), and
        // append the origin as an absolute iso.0 date/time when a non-zero
        // TimeOrigin is set and the Format is not itself a date string.
        let mut title = self.system.label().to_string();

        if self.system != TimeSystem::Bepoch && self.timescale_set {
            title.push_str(" [");
            title.push_str(self.timescale.as_str());
            // For Local Time, include the offset from UTC.
            if self.timescale == TimeScale::Lt {
                let ltoff = self.lt_offset();
                if ltoff >= 0.0 {
                    title.push_str(&format!(" (UTC+{})", fmt_g(ltoff)));
                } else {
                    title.push_str(&format!(" (UTC-{})", fmt_g(-ltoff)));
                }
            }
            title.push(']');
        }

        // Append "offset from <date>" when a non-zero origin is set and the
        // Format does not already render an (absolute) date string.
        let orig = self.get_time_origin_cur();
        if orig != 0.0 && !date_format(self.set_format()).0 {
            // Format the origin (axis value 0.0) as an absolute iso.0 time.
            if let Ok(date) = self.format_iso0_zero() {
                title.push_str(" offset from ");
                title.push_str(&date);
            }
        }

        title
    }

    /// The TimeOrigin expressed in the current axis units. Mirrors C
    /// `GetTimeOriginCur` (timeframe.c): the stored origin (in the system's
    /// default units) converted to the per-instance Unit.
    fn get_time_origin_cur(&self) -> f64 {
        let result = self.time_origin();
        if result == 0.0 {
            return result;
        }
        let def = self.system.default_units();
        let cur = self.unit_or_default();
        if cur == def {
            return result;
        }
        match crate::units::unit_mapper(def, cur) {
            Some(map) => transform_scalar(map.as_ref(), result, true).unwrap_or(result),
            None => result,
        }
    }

    /// Format the axis value 0.0 (i.e. the TimeOrigin) as an absolute
    /// `iso.0` date/time string, regardless of the frame's own Format.
    /// Mirrors C `GetTitle`'s temporary `astSetFormat(this, 0, "iso.0")`.
    ///
    /// `iso.0` has `ndp == 0`, so a time field is shown and the MJD is *not*
    /// rounded to the nearest day (rounding only happens for date-only
    /// formats with `ndp < 0`).
    fn format_iso0_zero(&self) -> AstResult<String> {
        let mjd = self.value_to_mjd(0.0)?;
        let (iy, im, id, fd, _j) = crate::pal::djcl(mjd);
        let (_sign, ihmsf) = crate::pal::dd2tf(0, fd);
        Ok(format!(
            "{iy:4}-{im:02}-{id:02} {:02}:{:02}:{:02}",
            ihmsf[0], ihmsf[1], ihmsf[2]
        ))
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

    /// Re-express the stored `TimeOrigin` from the current timescale into
    /// `newts`. Mirrors C `OriginScale` (timeframe.c:4258).
    ///
    /// Called *before* the new timescale is stored, so the current
    /// `self.timescale` is the timescale the origin currently refers to. A
    /// no-op when no origin is set or the timescale is unchanged.
    fn origin_scale(&mut self, newts: TimeScale) {
        let Some(oldval) = self.time_origin else {
            return;
        };
        let oldts = self.timescale;
        if newts == oldts {
            return;
        }
        let sys = self.system;
        let u = sys.default_units();
        if let Ok(map) = self.make_map(sys, sys, oldts, newts, 0.0, 0.0, u, u) {
            if let Ok(newval) = transform_scalar(map.as_ref(), oldval, true) {
                self.time_origin = Some(newval);
            }
        }
    }

    /// Re-express the stored `TimeOrigin` from `oldsys` into the current
    /// system. Mirrors C `OriginSystem` (timeframe.c:4347).
    ///
    /// Called *after* `self.system` has been updated to the new system. A
    /// no-op when no origin is set or the system is unchanged.
    fn origin_system(&mut self, oldsys: TimeSystem) {
        let Some(oldval) = self.time_origin else {
            return;
        };
        let newsys = self.system;
        if oldsys == newsys {
            return;
        }
        let ts = self.timescale;
        let oldu = oldsys.default_units();
        let newu = newsys.default_units();
        if let Ok(map) = self.make_map(oldsys, newsys, ts, ts, 0.0, 0.0, oldu, newu) {
            if let Ok(newval) = transform_scalar(map.as_ref(), oldval, true) {
                self.time_origin = Some(newval);
            }
        }
    }

    /// The Mapping from `self` (the target) to `result`, aligned in the
    /// AlignTimeScale of `align_frm`. A port of `MakeTimeMapping`
    /// (timeframe.c:3838): when the two Frames differ in a property that one
    /// of the conversions to or from the alignment timescale depends on (the
    /// clock position, DUT1, DTAI or LTOffset), the conversion is made in
    /// two parts, the first with the target's properties and the second
    /// with the result's. Otherwise the alignment timescale does not affect
    /// the result and one conversion, with the target's properties, is made.
    fn make_time_mapping(
        &self,
        result: &TimeFrame,
        align_frm: &TimeFrame,
    ) -> AstResult<DynMapping> {
        let (sys1, ts1, off1, u1, ltoff1) = (
            self.system,
            self.timescale,
            self.time_origin(),
            self.unit_or_default(),
            self.lt_offset(),
        );
        let (sys2, ts2, off2, u2, ltoff2) = (
            result.system,
            result.timescale,
            result.time_origin(),
            result.unit_or_default(),
            result.lt_offset(),
        );
        let align_ts = align_frm.align_timescale();

        let clkdiff = self.obs_lon() != result.obs_lon()
            || self.obs_lat() != result.obs_lat()
            || self.obs_alt() != result.obs_alt();
        let taclk = ts1.is_clock_scale() || align_ts.is_clock_scale();
        let arclk = align_ts.is_clock_scale() || ts2.is_clock_scale();
        let dut1diff = self.get_dut1() != result.get_dut1();
        let tadut = ts1.is_dut1_scale() != align_ts.is_dut1_scale();
        let ardut = align_ts.is_dut1_scale() != ts2.is_dut1_scale();
        let dtaidiff = !dtai_equal(self.get_dtai(), result.get_dtai());
        let tadtai = ts1.dtai_group() != align_ts.dtai_group();
        let ardtai = align_ts.dtai_group() != ts2.dtai_group();
        let ltodiff = ltoff1 != ltoff2;
        let talto = ts1.is_ltoffset_scale() != align_ts.is_ltoffset_scale();
        let arlto = align_ts.is_ltoffset_scale() != ts2.is_ltoffset_scale();

        if (!clkdiff || !(taclk || arclk))
            && (!ltodiff || !(talto || arlto))
            && (!dut1diff || !(tadut || ardut))
            && (!dtaidiff || !(tadtai || ardtai))
        {
            return self.make_map(sys1, sys2, ts1, ts2, off1, off2, u1, u2);
        }

        // Through MJD in the alignment timescale, with an origin equal to
        // the MJD equivalent of the target's.
        let align_off = Self::to_mjd(sys1, off1);
        let align_unit = TimeSystem::Mjd.default_units();
        let map1 = self.make_map(
            sys1,
            TimeSystem::Mjd,
            ts1,
            align_ts,
            off1,
            align_off,
            u1,
            align_unit,
        )?;
        let map2 = result.make_map(
            TimeSystem::Mjd,
            sys2,
            align_ts,
            ts2,
            align_off,
            off2,
            align_unit,
            u2,
        )?;
        let joined: DynMapping = Box::new(CmpMap::new(map1, map2, Composition::Series)?);
        joined.simplify()
    }

    /// Make a Mapping between the stated systems, timescales, origins and
    /// units. Mirrors C `timeframe.c::MakeMap` (3505).
    ///
    /// `off1`/`off2` are axis offsets in the default units of `sys1`/`sys2`.
    /// `unit1`/`unit2` are the input/output unit strings. The returned
    /// Mapping transforms a value in (`sys1`, `ts1`, `off1`, `unit1`) to one
    /// in (`sys2`, `ts2`, `off2`, `unit2`).
    ///
    /// Every timescale conversion uses the observer, DUT1, DTAI and
    /// LTOffset of `self`, as C's uses those of its `this` Frame.
    #[allow(clippy::too_many_arguments)]
    fn make_map(
        &self,
        sys1: TimeSystem,
        sys2: TimeSystem,
        ts1: TimeScale,
        ts2: TimeScale,
        off1: f64,
        off2: f64,
        unit1: &str,
        unit2: &str,
    ) -> AstResult<DynMapping> {
        let du1 = sys1.default_units();
        let du2 = sys2.default_units();

        // Simple case: same system and timescale.
        if ts1 == ts2 && sys1 == sys2 {
            // Same offset too: identity in the default units; only a unit
            // conversion (if any) remains.
            if ast_equals(off1, off2, 1.0e3) {
                if unit1 == unit2 {
                    return Ok(Box::new(UnitMap::new(1)));
                }
                return unit_mapper_or_err(unit1, unit2);
            }

            // Offsets differ. Transform the offset difference from the
            // system default units into the output units, then apply it as
            // a ShiftMap, bracketed by an input→output unit conversion.
            let mut shift = off1 - off2;
            if du1 != unit2 {
                let umap = unit_mapper_or_err(du1, unit2)?;
                shift = transform_scalar(umap.as_ref(), shift, true)?;
            }
            let shift_map: DynMapping = Box::new(ShiftMap::new(vec![shift]));
            if unit1 != unit2 {
                let umap = unit_mapper_or_err(unit1, unit2)?;
                return Ok(Box::new(CmpMap::new(umap, shift_map, Composition::Series)?));
            }
            return Ok(shift_map);
        }

        // Full path: input units → default-in units, TimeMap, default-out
        // units → output units.
        let umap1 = unit_mapper_or_err(unit1, du1)?;
        let umap2 = unit_mapper_or_err(du2, unit2)?;

        let mut timemap = TimeMap::new();
        let mjd_off1 = Self::to_mjd(sys1, off1);
        self.build_timemap(&mut timemap, sys1, sys2, ts1, ts2, off1, off2, mjd_off1);

        let timemap: DynMapping = if timemap.is_empty() {
            Box::new(UnitMap::new(1))
        } else {
            Box::new(timemap)
        };

        let chain = CmpMap::new(umap1, timemap, Composition::Series)?;
        let chain = CmpMap::new(Box::new(chain), umap2, Composition::Series)?;
        <dyn Mapping>::simplify(&chain)
    }

    /// Append the System/TimeScale conversion steps of C's `MakeMap`
    /// (timeframe.c:3600-3790) to `timemap`, using the observer, DUT1, DTAI
    /// and LTOffset of `self` throughout.
    #[allow(clippy::too_many_arguments)]
    fn build_timemap(
        &self,
        timemap: &mut TimeMap,
        sys1: TimeSystem,
        sys2: TimeSystem,
        ts1: TimeScale,
        ts2: TimeScale,
        _off1: f64,
        off2: f64,
        mjd_off1: f64,
    ) {
        let off1 = _off1;
        // Step 1: Convert source system → MJD (if needed).
        match sys1 {
            TimeSystem::Mjd => {}
            TimeSystem::Jd => timemap.add(TimeCvtType::JdToMjd, &[off1, mjd_off1]),
            TimeSystem::Jepoch => timemap.add(TimeCvtType::JepToMjd, &[off1, mjd_off1]),
            TimeSystem::Bepoch => timemap.add(TimeCvtType::BepToMjd, &[off1, mjd_off1]),
        }

        // Step 2 & 3: timescale conversion via TAI.
        if ts1 != ts2 {
            // Source-side observer (negate longitude: Frame stores +ve east,
            // TimeMap needs +ve west).
            let obs_lon = -(self.attrs.obs_lon.unwrap_or(0.0));
            let obs_lat = self.attrs.obs_lat.unwrap_or(0.0);
            let obs_alt = self.attrs.obs_alt.unwrap_or(0.0);
            let dtai = self.get_dtai();
            let args = [mjd_off1, obs_lon, obs_lat, obs_alt, dtai];
            let args_ut = [self.get_dut1()];
            let args_lt = [self.lt_offset()];
            let args_tai = [mjd_off1, dtai];

            match ts1 {
                TimeScale::Tai => {}
                TimeScale::Utc => timemap.add(TimeCvtType::UtcToTai, &args_tai),
                TimeScale::Tt => timemap.add(TimeCvtType::TtToTai, &args[..1]),
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

            match ts2 {
                TimeScale::Tai => {}
                TimeScale::Utc => timemap.add(TimeCvtType::TaiToUtc, &args_tai),
                TimeScale::Tt => timemap.add(TimeCvtType::TaiToTt, &args[..1]),
                TimeScale::Tdb => {
                    timemap.add(TimeCvtType::TaiToTt, &args[..1]);
                    timemap.add(TimeCvtType::TtToTdb, &args);
                }
                TimeScale::Tcg => {
                    timemap.add(TimeCvtType::TaiToTt, &args[..1]);
                    timemap.add(TimeCvtType::TtToTcg, &args[..1]);
                }
                TimeScale::Tcb => {
                    timemap.add(TimeCvtType::TaiToTt, &args[..1]);
                    timemap.add(TimeCvtType::TtToTdb, &args);
                    timemap.add(TimeCvtType::TdbToTcb, &args[..1]);
                }
                TimeScale::Lt => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai);
                    timemap.add(TimeCvtType::UtcToLt, &args_lt);
                }
                TimeScale::Ut1 => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut);
                }
                TimeScale::Gmst => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut);
                    timemap.add(TimeCvtType::UtToGmst, &args[..1]);
                }
                TimeScale::Last => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut);
                    timemap.add(TimeCvtType::UtToGmst, &args[..1]);
                    timemap.add(TimeCvtType::GmstToLmst, &args[..3]);
                    timemap.add(TimeCvtType::LmstToLast, &args[..3]);
                }
                TimeScale::Lmst => {
                    timemap.add(TimeCvtType::TaiToUtc, &args_tai);
                    timemap.add(TimeCvtType::UtcToUt, &args_ut);
                    timemap.add(TimeCvtType::UtToGmst, &args[..1]);
                    timemap.add(TimeCvtType::GmstToLmst, &args[..3]);
                }
            }
        }

        // Step 4: Convert MJD → target system (if needed).
        match sys2 {
            TimeSystem::Mjd => {
                if mjd_off1 != off2 {
                    timemap.add(TimeCvtType::MjdToMjd, &[mjd_off1, off2]);
                }
            }
            TimeSystem::Jd => timemap.add(TimeCvtType::MjdToJd, &[mjd_off1, off2]),
            TimeSystem::Jepoch => timemap.add(TimeCvtType::MjdToJep, &[mjd_off1, off2]),
            TimeSystem::Bepoch => timemap.add(TimeCvtType::MjdToBep, &[mjd_off1, off2]),
        }
    }

    /// Convert a coordinate `value` in this frame (its System, TimeScale,
    /// TimeOrigin and Unit) to an absolute MJD in days. Mirrors the
    /// `MakeMap(this, sys, AST__MJD, ts, ts, off, 0.0, unit, "d", ...)` call
    /// at the top of C `Format` (timeframe.c:1701).
    fn value_to_mjd(&self, value: f64) -> AstResult<f64> {
        let map = self.make_map(
            self.system,
            TimeSystem::Mjd,
            self.timescale,
            self.timescale,
            self.time_origin(),
            0.0,
            self.unit_or_default(),
            "d",
        )?;
        transform_scalar(map.as_ref(), value, true)
    }

    /// Convert an absolute MJD in days (in timescale `ts1`) to a coordinate
    /// value in this frame's System/TimeScale/TimeOrigin/Unit. Mirrors the
    /// `MakeMap(this, AST__MJD, sys, ts1, ts2, 0.0, off, "d", unit, ...)` call
    /// in C `Unformat` (timeframe.c:6241); `ts1` is TT for a Besselian-epoch
    /// input string, else the frame's own TimeScale.
    fn mjd_to_value(&self, mjd: f64, ts1: TimeScale) -> AstResult<f64> {
        let map = self.make_map(
            TimeSystem::Mjd,
            self.system,
            ts1,
            self.timescale,
            0.0,
            self.time_origin(),
            "d",
            self.unit_or_default(),
        )?;
        transform_scalar(map.as_ref(), mjd, true)
    }
}

/// Build a units-conversion Mapping or report an incompatible-units error
/// (mirrors C `astUnitMapper` returning NULL → AST__INTGES error path).
fn unit_mapper_or_err(unit1: &str, unit2: &str) -> AstResult<DynMapping> {
    crate::units::unit_mapper(unit1, unit2).ok_or_else(|| {
        AstError::new(
            ErrorCode::Badun,
            format!("TimeFrame: cannot convert between units \"{unit1}\" and \"{unit2}\""),
        )
    })
}

/// Format a value like C's `%g`: the shorter of `%e`/`%f` with up to 6
/// significant digits and no trailing zeros. Used for the LT offset in the
/// default Title.
fn fmt_g(v: f64) -> String {
    crate::frames::axis_format::format_value("%g", 6, v)
}

/// Transform a single scalar through `map` in the given direction.
fn transform_scalar(map: &dyn Mapping, value: f64, forward: bool) -> AstResult<f64> {
    let pts = PointSet::from_rows([[value]]);
    let out = map.transform(&pts, forward)?;
    Ok(out.point(0)[0])
}

/// Read a leading floating-point number from `text`, returning the value and
/// the number of characters consumed (including leading and trailing
/// whitespace around the number). This is the numeric fallback used by
/// [`TimeFrame::unformat`], matching the parent Frame's numeric unformat.
/// Returns `(BAD_VALUE, 0)` when no number is present.
fn numeric_unformat(text: &str) -> (f64, usize) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let num_start = i;
    let mut j = i;
    if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
        j += 1;
    }
    let mut digits = 0;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if j < bytes.len() && bytes[j] == b'.' {
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return (BAD_VALUE, 0);
    }
    if j < bytes.len() && (bytes[j] == b'e' || bytes[j] == b'E') {
        let mut k = j + 1;
        if k < bytes.len() && (bytes[k] == b'+' || bytes[k] == b'-') {
            k += 1;
        }
        let exp_start = k;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k > exp_start {
            j = k;
        }
    }
    let val = match text[num_start..j].parse::<f64>() {
        Ok(v) => v,
        Err(_) => return (BAD_VALUE, 0),
    };
    // Consume trailing whitespace, as the parent astUnformat does.
    let mut end = j;
    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    (val, end)
}

/// Decide whether a Format string requests a date/time rendering, and with
/// what precision and separator. Mirrors C `DateFormat` (timeframe.c:813).
///
/// Returns `(is_date, ndp, sep)`:
/// - `is_date` is true when the (whitespace-trimmed) format string begins
///   with `"iso"`.
/// - `ndp` is the number of decimal places of seconds: `-1` means a date
///   only (no time field), `>= 0` (from `"iso.N"`) means a time field with N
///   fractional-second digits.
/// - `sep` is the date/time separator: `'T'` when the format ends with a
///   `T`/`t` after `iso.N`, otherwise a space.
fn date_format(fmt: &str) -> (bool, i32, char) {
    let trimmed = fmt.trim_start();
    if !trimmed.starts_with("iso") {
        return (false, -1, ' ');
    }
    // Try to parse "iso.N" with an optional trailing separator character.
    if let Some(rest) = trimmed.strip_prefix("iso.") {
        // Read the integer N from the start of `rest`.
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            if let Ok(ndp) = digits.parse::<i32>() {
                let after = &rest[digits.len()..];
                let sep = match after.chars().next() {
                    Some('T') | Some('t') => 'T',
                    _ => ' ',
                };
                return (true, ndp, sep);
            }
        }
    }
    // Plain "iso" (or "iso." with no integer): date only.
    (true, -1, ' ')
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for TimeFrame {
    /// A TimeFrame always has active units and reports ActiveUnit as unset
    /// (timeframe.c `GetActiveUnit`, `TestActiveUnit`).
    fn get_active_unit(&self) -> bool {
        true
    }
    fn set_active_unit(&mut self, _value: bool) {}
    fn test_active_unit(&self) -> bool {
        false
    }

    frame_attrs_title!();
    frame_attrs_domain!();
    frame_attrs_epoch!();
    frame_attrs_digits!();
    frame_attrs_observer!();
    frame_attrs_ident!();
    frame_attrs_match!();

    fn naxes(&self) -> usize {
        1
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    /// Mirrors C `TimeFrame::GetLabel` (timeframe.c:2579).
    fn label(&self, axis: usize) -> AstResult<String> {
        Ok(match &self.ax(axis)?.label {
            Some(label) => label.clone(),
            None => self.default_axis_label(),
        })
    }

    /// Mirrors C `TimeFrame::GetSymbol` (timeframe.c:2741).
    fn symbol(&self, axis: usize) -> AstResult<String> {
        if let Some(symbol) = &self.ax(axis)?.symbol {
            return Ok(symbol.clone());
        }
        let symbol = match self.system {
            TimeSystem::Mjd => "MJD",
            TimeSystem::Jd => "JD",
            TimeSystem::Bepoch => "BEP",
            TimeSystem::Jepoch => "JEP",
        };
        Ok(super::frame_attrs::relabel_for_unit(
            symbol.to_string(),
            self.system.default_units(),
            self.axis.unit.as_deref(),
        ))
    }

    /// Mirrors C `TimeFrame::GetUnit` (timeframe.c:3253).
    fn unit(&self, axis: usize) -> AstResult<String> {
        Ok(match &self.ax(axis)?.unit {
            Some(unit) => unit.clone(),
            None => self.system.default_units().to_string(),
        })
    }

    /// Mirrors C `TimeFrame::SetUnit` (timeframe.c:4958): a Besselian
    /// epoch is in years and takes no other Unit.
    fn set_unit(&mut self, axis: usize, unit: &str) -> AstResult<()> {
        self.ax(axis)?;
        if self.system == TimeSystem::Bepoch && unit != "yr" {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetUnit(TimeFrame): Supplied Unit ({unit}) cannot be used because \
                     the TimeFrame represents Besselian Epoch which is defined in units \
                     of years (yr)."
                ),
            ));
        }
        // The Frame class's SetUnit follows (frame.c:10775).
        let unit = unit.trim_end();
        if self.get_active_unit() {
            let old = Frame::unit(self, axis)?;
            super::frame_attrs::new_unit(&mut self.axis, &old, unit);
        }
        self.axis.unit = Some(unit.to_string());
        Ok(())
    }

    axis_attrs_base_get!(format, digits, direction, top, bottom);
    axis_attrs_store!(label, symbol, format, digits, direction, top, bottom);
    fn test_unit(&self, axis: usize) -> AstResult<bool> {
        Ok(self.ax(axis)?.unit.is_some())
    }
    fn clear_unit(&mut self, axis: usize) -> AstResult<()> {
        // The Frame class's ClearUnit (frame.c:2527).
        if let Some(old) = self.ax_mut(axis)?.unit.take() {
            if self.get_active_unit() {
                let new = Frame::unit(self, axis)?;
                super::frame_attrs::new_unit(&mut self.axis, &old, &new);
            }
        }
        Ok(())
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

        // If the Format string does not request a date, use the default
        // numeric format. The parent (non-date) Format default is `%1.7G`:
        // 7 *significant* digits (frame.c GetDefaultFormat, default Digits=7).
        let (is_date, ndp, sep) = date_format(self.set_format());
        if !is_date {
            let digits = self.axis_digits(1)?.max(1) as usize;
            return Ok(crate::frames::axis_format::format_value(
                &self.format_str(1)?,
                digits,
                value,
            ));
        }

        // Date branch (timeframe.c:1694-1736). Convert the value to an
        // absolute MJD in days, then split into calendar fields.
        let mut mjd = self.value_to_mjd(value)?;

        // If no time fields will be produced, round to the nearest day.
        if ndp < 0 {
            mjd = mjd.round();
        }

        let (iy, im, id, fd, _j) = crate::pal::djcl(mjd);
        // "%4d-%2.2d-%2.2d" — the year is space-padded to width 4.
        let mut out = format!("{iy:4}-{im:02}-{id:02}");

        if ndp >= 0 {
            let (_sign, ihmsf) = crate::pal::dd2tf(ndp, fd);
            let (h, m, s, frac) = (ihmsf[0], ihmsf[1], ihmsf[2], ihmsf[3]);
            if ndp > 0 {
                let width = ndp as usize;
                out.push_str(&format!("{sep}{h:02}:{m:02}:{s:02}.{frac:0width$}"));
            } else {
                out.push_str(&format!("{sep}{h:02}:{m:02}:{s:02}"));
            }
        }

        Ok(out)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("TimeFrame has 1 axis, got axis={axis}"),
            ));
        }

        // First attempt a plain numeric read (C calls the parent Frame
        // unformat with any date Format temporarily cleared). We replicate
        // the parent's float scan directly.
        let (val1, nc1) = numeric_unformat(text);

        // Now attempt to read an ISO date/time. astReadDateTime requires the
        // whole string to parse, but astUnformat allows trailing text, so we
        // find the longest leading substring that yields a non-bad value by
        // truncating from the full (trailing-whitespace-trimmed) length down.
        let l = text.trim_end().len();
        let mut mjd = BAD_VALUE;
        let mut lt = 0usize;
        for cut in (1..=l).rev() {
            // Only cut on a char boundary.
            if !text.is_char_boundary(cut) {
                continue;
            }
            if let Some(m) = crate::frames::date_time::read_date_time(&text[..cut]) {
                mjd = m;
                lt = cut;
                break;
            }
        }
        // If the whole non-blank start parsed, include any trailing
        // whitespace in the consumed-character count.
        if lt >= l {
            lt = text.len();
        }

        // If no date/time could be read, or the numeric read was at least as
        // long, return the numeric value (assumed already in the frame's
        // system and units).
        if is_bad(mjd) || nc1 >= l {
            return Ok((val1, nc1));
        }

        // A date/time was read and is the better match. Determine the input
        // timescale: a Besselian-epoch string ("B...") is TT, otherwise the
        // frame's own TimeScale.
        let first = text.trim_start().chars().next();
        let ts1 = if matches!(first, Some('B') | Some('b')) {
            TimeScale::Tt
        } else {
            self.timescale
        };
        let value = self.mjd_to_value(mjd, ts1)?;
        Ok((value, lt))
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        Ok(super::cartesian_distance(&a[..1], &b[..1]))
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
        Ok(super::cartesian_offset(&from[..1], &to[..1], dist))
    }

    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        // Time coordinates are unbounded — no normalisation needed.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); TimeFrame
        // does not override it. A difference in System/TimeScale/Epoch/
        // TimeOrigin yields a non-UnitMap time conversion and so the
        // frames compare unequal.
        super::find_frame::frames_equal(self, other)
    }

    fn system_string(&self) -> Option<&str> {
        Some(self.system.as_str())
    }

    // -----------------------------------------------------------------
    // Match-control attributes (mirror C `astGet/SetMinAxes` etc.).
    // -----------------------------------------------------------------

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (timeframe.c).
    // -----------------------------------------------------------------

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        timeframe_overlay(self, template_axes, result)
    }

    fn axis_overlay_source(&self, axis: usize) -> Option<super::overlay::AxisOverlaySource> {
        (axis == 1).then(|| {
            super::overlay::AxisOverlaySource(super::axis::FrameAxis::Plain(self.axis.clone()))
        })
    }

    fn apply_axis_overlay(&mut self, axis: usize, source: &super::overlay::AxisOverlaySource) {
        if axis == 1 {
            let mut target = super::axis::FrameAxis::Plain(self.axis.clone());
            source.apply_to(&mut target);
            if let super::axis::FrameAxis::Plain(a) = target {
                self.axis = a;
            }
        }
    }

    fn overlay_system_from(&mut self, template: &dyn std::any::Any) -> AstResult<()> {
        if let Some(t) = template.downcast_ref::<TimeFrame>() {
            if t.test_system() {
                self.set_system(t.system);
            }
            if t.test_align_system() {
                self.set_align_system(t.align_system());
            }
        }
        Ok(())
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
            Ok(Some(Box::new(crate::frames::cast_to_basic_frame(
                self,
                &self.attrs,
            )?)))
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
        // Otherwise a plain Frame holding a copy of the Axis object on each
        // axis selected from this one (frame.c:11017 `astSetAxis`).
        let mut bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        for (i, &t) in target_axes.iter().enumerate() {
            if t == 0 {
                bf.set_axis_object(i + 1, super::axis::FrameAxis::Plain(self.axis.clone()));
            }
        }
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// Match / SubFrame / Overlay helpers
// ---------------------------------------------------------------------------

/// C's TimeFrame `Overlay` (timeframe.c:4440), a method of the template.
///
/// When `result` is a TimeFrame whose System the template sets to a new
/// value, its Title, Label and Symbol are cleared. The Frame-class part
/// follows, which sets the System on a TimeFrame result only, and then the
/// TimeFrame attributes in C's order: TimeOrigin before TimeScale, so
/// setting TimeScale re-expresses the template's origin as if it were in
/// the result's old timescale.
fn timeframe_overlay(
    template_time: &TimeFrame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    if let Some(result_time) = result.as_any_mut().downcast_mut::<TimeFrame>() {
        if template_time.system != result_time.system && template_time.test_system() {
            result_time.clear_title();
            result_time.axis.label = None;
            result_time.axis.symbol = None;
        }
    }

    super::overlay::frame_overlay(template_time, template_axes, result)?;

    if let Some(result_time) = result.as_any_mut().downcast_mut::<TimeFrame>() {
        if let Some(ts) = template_time.align_timescale {
            result_time.set_align_timescale(ts);
        }
        if let Some(v) = template_time.lt_offset {
            result_time.set_lt_offset(v);
        }
        if let Some(v) = template_time.time_origin {
            result_time.set_time_origin(v);
        }
        if template_time.test_timescale() {
            result_time.set_timescale(template_time.timescale);
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

        // The alignment Frame, whose AlignTimeScale is used, is the
        // template when it is a TimeFrame, and otherwise the target
        // (timeframe.c:5266-5281).
        let mut align_frm = target;
        if let Some(t) = template {
            t.overlay(template_axes, &mut result)?;
            if let Some(t) = t.as_any().downcast_ref::<TimeFrame>() {
                align_frm = t;
            }
        }
        let map: DynMapping = target.make_time_mapping(&result, align_frm)?;
        return Ok(Some((map, Box::new(result))));
    }

    // Result is not a TimeFrame (timeframe.c:5325): set explicitly the
    // attributes whose defaults come from this class, and clear the
    // System, which only this class understands, on a copy, then select the
    // axes as the Frame class does.
    let mut temp = target.clone();
    super::find_frame::set_class_defaults(&mut temp)?;
    temp.clear_system();
    temp.clear_align_system();
    super::find_frame::default_sub_frame(&temp, template, target_axes, template_axes)
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

        // The Object class's part: Ident, when set (object.c Dump).
        if let Some(ident) = self.attrs.ident.as_deref().filter(|s| !s.is_empty()) {
            node.push_string(
                "Ident",
                ident,
                Some("Permanent Object identification string".to_string()),
            );
            node.push_isa("Object", Some("AST Object".to_string()));
        }

        // The Frame class's part, read through this class's getters
        // (frame.c:14250), then the Axis (frame.c:14777).
        super::frame_attrs::write_frame_cards(
            &mut node,
            &super::frame_attrs::TraitDump {
                frame: self,
                attrs: &self.attrs,
                system: (self.system_set, self.system.as_str().to_string()),
                align_system: (
                    self.align_system.is_some(),
                    self.align_system().as_str().to_string(),
                ),
                perm: vec![0],
                is_frame: false,
                own_naxes: 1,
            },
        );
        let mut axis_node = Node::new("Axis");
        axis_node.class_comment = Some("Coordinate axis".to_string());
        self.axis.write_set_attributes(&mut axis_node);
        node.push_object("Ax1", axis_node, Some("Axis number 1".to_string()));

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

        // The Frame-class attributes (frame.c `astLoadFrame`), then the
        // ClLat and ClLon items older versions of AST wrote in place of
        // ObsLat and ObsLon, read only when those are absent (timeframe.c:7248).
        tf.attrs = super::frame_attrs::read_frame_attrs(node);
        if tf.attrs.obs_lat.is_none() && node.get("ClLat").is_some() {
            tf.attrs.obs_lat = Some(node.get_double("ClLat", 0.0));
        }
        if tf.attrs.obs_lon.is_none() && node.get("ClLon").is_some() {
            tf.attrs.obs_lon = Some(node.get_double("ClLon", 0.0));
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

        if node.get("AlSys").is_some() {
            tf.align_system = TimeSystem::parse(&node.get_string("AlSys", ""));
        }

        // The Axis attributes live in the Ax1 Axis child node.
        if let Some(axis_node) = node.get_object("Ax1") {
            tf.axis = super::axis::Axis::read(axis_node);
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
        template.overlay(&[0], result.as_mut()).unwrap();
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
        template.overlay(&[0], result.as_mut()).unwrap();
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
