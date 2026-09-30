//! The celestial coordinate frame (SkyFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::sky_convert::{SkyRefIs, SkySystem, make_offset_mapping, make_sky_mapping};
use crate::frames::skyaxis::{SkyAxis, format_sky, unformat_sky};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, LineDef, SkyLineDef};
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, Composition, DynMapping, PermMap, UnitMap};
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
    /// Tracks whether `system` was set via `set_system` or loaded
    /// from a native node (mirrors C `astTestSystem` for SkyFrame).
    system_set: bool,
    equinox: Option<f64>, // MJD of mean equinox
    projection: Option<String>,
    neglon: bool,
    /// Tracks whether `neglon` was explicitly set (mirrors C
    /// `astTestNegLon`).
    neglon_set: bool,
    sky_tol: Option<f64>, // smallest significant distance (arcseconds)
    sky_ref: [f64; 2],    // offset origin/pole [lon, lat] (radians)
    /// Per-axis "explicitly set" flags for `sky_ref` (mirrors C
    /// `astTestSkyRef`).
    sky_ref_set: [bool; 2],
    sky_ref_p: [f64; 2], // offset meridian point [lon, lat]
    /// Per-axis "explicitly set" flags for `sky_ref_p` (mirrors C
    /// `astTestSkyRefP`).
    sky_ref_p_set: [bool; 2],
    sky_ref_is: SkyRefIs,
    sky_ref_is_set: bool,
    align_offset: bool,
    /// Tracks whether `align_offset` was explicitly set (mirrors C
    /// `astTestAlignOffset`).
    align_offset_set: bool,
    align_system: Option<SkySystem>,

    // Per-axis properties
    axes: [SkyAxis; 2], // [longitude, latitude] (physical storage)

    /// Per-axis Label override (physical storage, lon then lat).
    /// `None` means "use the system-derived default from
    /// `default_labels`".
    label_overrides: [Option<String>; 2],

    /// Per-axis Symbol override (physical storage, lon then lat).
    /// `None` means "use the system-derived default symbol".
    symbol_overrides: [Option<String>; 2],

    /// Axis permutation: perm[ext_axis] = physical_axis (0-based).
    /// Physical 0 = longitude, physical 1 = latitude.
    /// After swap: perm = [1, 0] means external axis 0 → latitude.
    perm: [usize; 2],

    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,

    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    // None = "use the trait default" (which returns 2 for SkyFrame's
    // naxes-based attributes, false/false/false for the booleans
    // overridden below).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
}

impl SkyFrame {
    /// Create a new `SkyFrame` with no explicit coordinate system —
    /// the equivalent of C's `astSkyFrame("")`.
    ///
    /// `system_set` is left `false`, so when this SkyFrame is used as
    /// a `find_frame` template, the System attribute acts as "any"
    /// and is inherited from the matched target. The struct-level
    /// `system` field defaults to `Icrs` (AST's `AST__ICRS` default),
    /// but is treated as unset until [`SkyFrame::set_system`] is
    /// called. Use [`SkyFrame::new`] when you know the system at
    /// construction time and want it marked as set.
    pub fn unset_system() -> Self {
        // Re-use SkyFrame::new(Icrs) — that branch already leaves
        // `system_set = false` for ICRS. Mirrors C `astSkyFrame("")`.
        Self::new(SkySystem::Icrs)
    }

    /// Create a new `SkyFrame` with the given coordinate system.
    ///
    /// The default domain is `"SKY"`, the default epoch is J2000.0
    /// (MJD 51544.5), and the default equinox is determined by the
    /// coordinate system.
    ///
    /// Passing `SkySystem::Icrs` leaves `system_set = false` (the
    /// SkyFrame default per AST), which makes the resulting frame
    /// behave as "system unspecified" when used as a `find_frame`
    /// template. Use [`SkyFrame::unset_system`] for a self-documenting
    /// call site when that's the intent.
    pub fn new(system: SkySystem) -> Self {
        Self {
            title: None,
            // None = use the default "SKY". Mirrors C's astTestDomain
            // returning false until set_domain is called.
            domain: None,
            epoch: None,
            obs_lat: None,
            obs_lon: None,
            obs_alt: None,
            dtai: None,
            dut1: None,
            system,
            // ICRS is the SkyFrame default (mirrors C: AST__ICRS is the
            // default System). Only mark the attribute as explicitly
            // set when the caller picks something else; the matching
            // serialisation treats ICRS as the "unset default".
            system_set: !matches!(system, SkySystem::Icrs),
            equinox: None,
            projection: None,
            neglon: false,
            neglon_set: false,
            sky_tol: None,
            sky_ref: [0.0, 0.0],
            sky_ref_set: [false, false],
            sky_ref_p: [0.0, std::f64::consts::FRAC_PI_2],
            sky_ref_p_set: [false, false],
            sky_ref_is: SkyRefIs::Ignored,
            sky_ref_is_set: false,
            align_offset: false,
            align_offset_set: false,
            align_system: None,
            axes: [SkyAxis::longitude(), SkyAxis::latitude()],
            label_overrides: [None, None],
            symbol_overrides: [None, None],
            perm: [0, 1],
            act_unt: None,
            ident: None,
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
        }
    }

    /// External-to-internal axis permutation. `axis_permutation()[i]`
    /// returns the (zero-based) physical axis index (0 = longitude,
    /// 1 = latitude) currently exposed at external axis `i`. Mirrors
    /// the Frame `perm` array used by C `astPrimaryFrame`.
    #[inline]
    pub fn axis_permutation(&self) -> [usize; 2] {
        self.perm
    }

    /// Position of longitude in coordinate arrays (0-based). Mirrors
    /// `astGetLonAxis(skyframe)`.
    #[inline]
    pub fn lon_axis(&self) -> usize {
        if self.perm[0] == 0 { 0 } else { 1 }
    }

    /// Position of latitude in coordinate arrays (0-based). Mirrors
    /// `astGetLatAxis(skyframe)`.
    #[inline]
    pub fn lat_axis(&self) -> usize {
        if self.perm[0] == 1 { 0 } else { 1 }
    }

    /// Position of longitude in coordinate arrays (0-based).
    #[inline]
    fn lon_pos(&self) -> usize {
        self.lon_axis()
    }

    /// Position of latitude in coordinate arrays (0-based).
    #[inline]
    fn lat_pos(&self) -> usize {
        self.lat_axis()
    }

    // --- Per-axis Label / Symbol (1-based external API) ---

    /// Resolve a 1-based external axis index into 0-based physical
    /// storage (lon=0, lat=1) using the permutation. Returns
    /// `Err(Axiin)` if `axis` is out of range.
    fn physical_axis(&self, axis: usize) -> AstResult<usize> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..=2", axis),
            ));
        }
        Ok(self.perm[axis - 1])
    }

    /// Effective `AsTime` for the physical axis `phys` (0 = longitude,
    /// 1 = latitude), mirroring C `astGetAsTime` (skyframe.c:2547-2566).
    ///
    /// If the axis has an explicit `AsTime` override it is honoured.
    /// Otherwise the longitude axis is formatted as a time only when the
    /// system is equatorial (involves the Earth's equator); the latitude
    /// axis and all non-equatorial longitudes default to angle (degrees).
    fn effective_as_time(&self, phys: usize) -> bool {
        if let Some(set) = self.axes[phys].as_time {
            return set;
        }
        if self.axes[phys].is_latitude {
            false
        } else {
            is_equatorial(self.system)
        }
    }

    /// Return the explicit Label override for `axis` (1-based) if
    /// set, falling back to the system-derived default. Mirrors
    /// `astGetLabel(skyfrm, axis)`.
    pub fn axis_label(&self, axis: usize) -> AstResult<String> {
        let phys = self.physical_axis(axis)?;
        if let Some(s) = &self.label_overrides[phys] {
            return Ok(s.clone());
        }
        let (lon, lat) = self.default_labels();
        Ok(if phys == 0 {
            lon.to_string()
        } else {
            lat.to_string()
        })
    }

    /// Set the Label for `axis` (1-based). Mirrors `astSetLabel`.
    pub fn set_axis_label(&mut self, axis: usize, label: impl Into<String>) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.label_overrides[phys] = Some(label.into());
        Ok(())
    }

    /// Test whether the Label for `axis` (1-based) was explicitly
    /// set. Mirrors `astTestLabel(skyfrm, axis)`.
    pub fn test_axis_label(&self, axis: usize) -> AstResult<bool> {
        let phys = self.physical_axis(axis)?;
        Ok(self.label_overrides[phys].is_some())
    }

    /// Clear an explicit Label for `axis` (1-based).
    pub fn clear_axis_label(&mut self, axis: usize) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.label_overrides[phys] = None;
        Ok(())
    }

    /// Default per-axis Symbol from the current celestial system.
    /// Returns the C library's two-letter / four-letter convention
    /// (e.g. `("RA", "DEC")` for FK5, `("GLON", "GLAT")` for
    /// galactic). Used as the fallback when no override is set.
    fn default_symbols(&self) -> (&'static str, &'static str) {
        // Matches C `astGetSymbol` (skyframe.c:4230-4256).
        match self.system {
            SkySystem::Galactic => ("l", "b"),
            SkySystem::Ecliptic | SkySystem::Helioecliptic => ("Lambda", "Beta"),
            SkySystem::Supergalactic => ("SGL", "SGB"),
            SkySystem::Azel => ("Az", "El"),
            SkySystem::Unknown => ("Lon", "Lat"),
            // FK4/FK5/ICRS/J2000/GAPPT/FK4NoE all use RA/Dec.
            _ => ("RA", "Dec"),
        }
    }

    /// Return the explicit Symbol override for `axis` (1-based) if
    /// set, falling back to the system-derived default. Mirrors
    /// `astGetSymbol(skyfrm, axis)`.
    pub fn axis_symbol(&self, axis: usize) -> AstResult<String> {
        let phys = self.physical_axis(axis)?;
        if let Some(s) = &self.symbol_overrides[phys] {
            return Ok(s.clone());
        }
        let (lon, lat) = self.default_symbols();
        let base = if phys == 0 { lon } else { lat };
        // In an offset coordinate system C prepends "D" (delta) to the
        // default symbol — DRA, DDec, etc. (skyframe.c GetSymbol:108).
        Ok(if self.is_offset() {
            format!("D{base}")
        } else {
            base.to_string()
        })
    }

    /// Set the Symbol for `axis` (1-based). Mirrors `astSetSymbol`.
    pub fn set_axis_symbol(&mut self, axis: usize, symbol: impl Into<String>) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.symbol_overrides[phys] = Some(symbol.into());
        Ok(())
    }

    /// Test whether the Symbol for `axis` (1-based) was explicitly
    /// set. Mirrors `astTestSymbol`.
    pub fn test_axis_symbol(&self, axis: usize) -> AstResult<bool> {
        let phys = self.physical_axis(axis)?;
        Ok(self.symbol_overrides[phys].is_some())
    }

    // --- SkyFrame-specific attributes ---

    /// Return the celestial coordinate system.
    pub fn system(&self) -> SkySystem {
        self.system
    }

    /// Set the celestial coordinate system.
    pub fn set_system(&mut self, sys: SkySystem) {
        self.system = sys;
        self.system_set = true;
    }

    /// Test whether the `System` attribute has been explicitly set.
    /// Mirrors C `astTestSystem` for `SkyFrame`.
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear the explicit-set flag on `System` so the attribute
    /// reverts to "not set" while keeping the current default value.
    /// Mirrors C `astClearSystem` for `SkyFrame`.
    pub fn clear_system(&mut self) {
        self.system_set = false;
    }

    /// Return the alignment coordinate system.
    ///
    /// If not explicitly set, returns ICRS (mirrors C
    /// `GetAlignSystem` for `SkyFrame`, skyframe.c:4281).
    pub fn align_system(&self) -> SkySystem {
        self.align_system.unwrap_or(SkySystem::Icrs)
    }

    /// Set the alignment coordinate system.
    pub fn set_align_system(&mut self, sys: SkySystem) {
        self.align_system = Some(sys);
    }

    /// Test whether the `AlignSystem` attribute has been explicitly set.
    pub fn test_align_system(&self) -> bool {
        self.align_system.is_some()
    }

    /// Return the equinox as MJD.
    ///
    /// If not explicitly set, returns the default for the current system.
    pub fn equinox_mjd(&self) -> f64 {
        self.equinox
            .unwrap_or_else(|| self.system.default_equinox_mjd())
    }

    /// Return the equinox as a decimal year (Besselian if MJD <
    /// J1984, Julian otherwise — mirrors `frame.c::Dump`).
    pub fn equinox_year(&self) -> f64 {
        let mjd = self.equinox_mjd();
        if mjd < pal::epj2d(1984.0) {
            pal::epb(mjd)
        } else {
            pal::epj(mjd)
        }
    }

    /// Set the equinox as MJD.
    pub fn set_equinox_mjd(&mut self, mjd: f64) {
        self.equinox = Some(mjd);
    }

    /// Return the epoch as MJD.
    ///
    /// If not explicitly set, the default depends on the System (C
    /// `GetEpoch` SkyFrame override, skyframe.c:3210-3225): B1950.0 for
    /// FK4 / FK4-no-E, J2000.0 for every other system.
    pub fn epoch_mjd(&self) -> f64 {
        match self.epoch {
            Some(e) => e,
            None => match self.system {
                SkySystem::Fk4 | SkySystem::Fk4NoE => pal::epb2d(1950.0),
                _ => pal::epj2d(2000.0),
            },
        }
    }

    /// Return the epoch as a decimal year (Besselian if MJD <
    /// J1984, Julian otherwise — mirrors `frame.c::Dump`).
    pub fn epoch_year(&self) -> f64 {
        let mjd = self.epoch_mjd();
        if mjd < pal::epj2d(1984.0) {
            pal::epb(mjd)
        } else {
            pal::epj(mjd)
        }
    }

    /// Whether the epoch attribute has been explicitly set
    /// (mirrors `astTestEpoch`).
    pub fn test_epoch(&self) -> bool {
        self.epoch.is_some()
    }

    /// Lowest valid value for the given 1-based external axis.
    ///
    /// Mirrors the SkyFrame `GetBottom` override (skyframe.c:2907-2989):
    /// the latitude axis is clamped to `-π/2`; the longitude axis has no
    /// lower limit (`-f64::MAX`). SkyFrame does not store explicit
    /// per-axis Bottom values, so these defaults are always returned.
    pub fn bottom(&self, axis: usize) -> AstResult<f64> {
        // physical_axis == 1 is the latitude axis (C's `axis_p == 1`).
        if self.physical_axis(axis)? == 1 {
            Ok(-std::f64::consts::FRAC_PI_2)
        } else {
            Ok(-f64::MAX)
        }
    }

    /// Highest valid value for the given 1-based external axis.
    ///
    /// Mirrors the SkyFrame `GetTop` override (skyframe.c:3234-3316):
    /// the latitude axis is clamped to `+π/2`; the longitude axis has no
    /// upper limit (`f64::MAX`). SkyFrame does not store explicit
    /// per-axis Top values, so these defaults are always returned.
    pub fn top(&self, axis: usize) -> AstResult<f64> {
        if self.physical_axis(axis)? == 1 {
            Ok(std::f64::consts::FRAC_PI_2)
        } else {
            Ok(f64::MAX)
        }
    }

    /// Clear the explicit `Epoch` value so the attribute reverts to
    /// its default. Mirrors C `astClearEpoch`.
    pub fn clear_epoch(&mut self) {
        self.epoch = None;
    }

    /// Clear the explicit `Equinox` value so the attribute reverts
    /// to its default. Mirrors C `astClearEquinox`.
    pub fn clear_equinox(&mut self) {
        self.equinox = None;
    }

    /// Set the epoch as MJD.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        self.epoch = Some(mjd);
    }

    /// Set the epoch from a bare decimal year (e.g. a FITS `EPOCH`
    /// keyword or the value stored in a native-encoded `Epoch` node).
    ///
    /// Applies the IAU 1984 convention to resolve the year to an MJD,
    /// matching what C does when the value flows through
    /// `astSet(skyframe, "Epoch=...")` → `astReadDateTime`.
    pub fn set_epoch_year(&mut self, year: f64) {
        self.set_epoch_mjd(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the equinox from a bare decimal year (e.g. a FITS
    /// `EQUINOX` keyword or the value stored in a native-encoded
    /// `Eqnox` node).
    ///
    /// Applies the IAU 1984 convention to resolve the year to an MJD,
    /// matching what C does when the value flows through
    /// `astSet(skyframe, "Equinox=...")` → `astReadDateTime`.
    pub fn set_equinox_year(&mut self, year: f64) {
        self.set_equinox_mjd(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Return whether longitude increases in the negative direction.
    pub fn neglon(&self) -> bool {
        self.neglon
    }

    /// Set whether longitude increases in the negative direction.
    pub fn set_neglon(&mut self, neg: bool) {
        self.neglon = neg;
        self.neglon_set = true;
    }

    /// Test whether the `NegLon` attribute has been explicitly set.
    pub fn test_neglon(&self) -> bool {
        self.neglon_set
    }

    /// Return the sky reference point `[lon, lat]` in radians.
    pub fn sky_ref(&self) -> [f64; 2] {
        self.sky_ref
    }

    /// Set the sky reference point.
    pub fn set_sky_ref(&mut self, lon: f64, lat: f64) {
        self.sky_ref = [lon, lat];
        self.sky_ref_set = [true, true];
    }

    /// Test whether axis `axis` (0-based) of `SkyRef` has been
    /// explicitly set.
    pub fn test_sky_ref(&self, axis: usize) -> bool {
        self.sky_ref_set.get(axis).copied().unwrap_or(false)
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

    /// Test whether `SkyRefIs` has been explicitly set.
    pub fn test_sky_ref_is(&self) -> bool {
        self.sky_ref_is_set
    }

    /// Return the sky reference meridian point `[lon, lat]` in radians.
    pub fn sky_ref_p(&self) -> [f64; 2] {
        self.sky_ref_p
    }

    /// Set the sky reference meridian point.
    pub fn set_sky_ref_p(&mut self, lon: f64, lat: f64) {
        self.sky_ref_p = [lon, lat];
        self.sky_ref_p_set = [true, true];
    }

    /// Test whether axis `axis` (0-based) of `SkyRefP` has been
    /// explicitly set.
    pub fn test_sky_ref_p(&self, axis: usize) -> bool {
        self.sky_ref_p_set.get(axis).copied().unwrap_or(false)
    }

    /// Return the sky tolerance in arcseconds.
    ///
    /// The smallest angular separation regarded as significant:
    /// system-conversion chains whose effect everywhere on the sphere
    /// is below this tolerance are replaced by a UnitMap.
    pub fn sky_tol(&self) -> f64 {
        // C default: 0.001 arcsec (skyframe.c:11549).
        self.sky_tol.unwrap_or(0.001)
    }

    /// Set the sky tolerance in arcseconds.
    pub fn set_sky_tol(&mut self, tol: f64) {
        self.sky_tol = Some(tol);
    }

    /// Test whether `SkyTol` has been explicitly set.
    pub fn test_sky_tol(&self) -> bool {
        self.sky_tol.is_some()
    }

    /// Test whether `Equinox` has been explicitly set.
    pub fn test_equinox(&self) -> bool {
        self.equinox.is_some()
    }

    /// Return whether alignment uses offset coordinates.
    pub fn align_offset(&self) -> bool {
        self.align_offset
    }

    /// Set whether alignment uses offset coordinates.
    pub fn set_align_offset(&mut self, ao: bool) {
        self.align_offset = ao;
        self.align_offset_set = true;
    }

    /// Test whether `AlignOffset` has been explicitly set.
    pub fn test_align_offset(&self) -> bool {
        self.align_offset_set
    }

    /// Return the projection description, if set.
    pub fn projection(&self) -> Option<&str> {
        self.projection.as_deref()
    }

    /// Set the projection description.
    pub fn set_projection(&mut self, proj: &str) {
        self.projection = Some(proj.to_string());
    }

    /// Test whether `Projection` has been explicitly set.
    pub fn test_projection(&self) -> bool {
        self.projection.is_some()
    }

    /// Test whether `Title` has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.title.is_some()
    }

    /// Clear the title attribute.
    pub fn clear_title(&mut self) {
        self.title = None;
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

    /// Test whether the domain attribute has been explicitly set.
    /// Mirrors C's `astTestDomain` for `SkyFrame`.
    pub fn test_domain(&self) -> bool {
        self.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default `"SKY"`.
    pub fn clear_domain(&mut self) {
        self.domain = None;
    }

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
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

    /// Local Apparent Sidereal Time, in radians, at this SkyFrame's
    /// Epoch and observer location.
    ///
    /// Mirrors `skyframe.c::CalcLAST`: builds a TDB-MJD TimeFrame and a
    /// LAST-MJD TimeFrame, both stamped with the SkyFrame's
    /// `obs_lon`/`obs_lat`/`obs_alt`/`dut1`/`dtai`, then transforms the
    /// SkyFrame's Epoch through the TDB→LAST conversion and folds the
    /// fractional day into radians.
    ///
    /// Returns 0.0 when the longitude/latitude are unset — the AzEl
    /// pipeline only consults LAST when the observer is known anyway,
    /// so this matches the C behaviour where a missing ObsLon defaults
    /// to 0 and the resulting LAST is meaningless but harmless.
    pub(crate) fn compute_last(&self) -> AstResult<f64> {
        compute_last(
            self.epoch_mjd(),
            self.obs_lon.unwrap_or(0.0),
            self.obs_lat.unwrap_or(0.0),
            self.obs_alt.unwrap_or(0.0),
            self.dut1.unwrap_or(0.0),
            self.dtai,
        )
    }

    /// Diurnal aberration vector magnitude, in radians, at this
    /// SkyFrame's observer location.
    ///
    /// Mirrors `skyframe.c::GetDiurab` (line 3735):
    /// `diurab = 2π · uau · SOLSID / C` with `uau` from
    /// `palGeoc(obs_lat, obs_alt)`. The constants `SOLSID = 1.00273790935`
    /// (sidereal/solar ratio) and `C = 173.14463331` (speed of light in
    /// AU/day) are taken verbatim from skyframe.c:381-382.
    ///
    /// Returns 0.0 when neither obs_lat nor obs_alt is set — matches the
    /// C library's behaviour that uau is computed from default zeros.
    pub(crate) fn compute_diurab(&self) -> f64 {
        const SOLSID: f64 = 1.00273790935;
        const C: f64 = 173.14463331;
        let lat = self.obs_lat.unwrap_or(0.0);
        let alt = self.obs_alt.unwrap_or(0.0);
        let (uau, _vau) = pal::geoc(lat, alt);
        2.0 * std::f64::consts::PI * uau * SOLSID / C
    }

    // --- Private helpers for serialisation defaults ---

    /// Default title for the current system and equinox.
    /// Is an offset coordinate system in use? Mirrors C `GetTitle`/`GetLabel`
    /// (skyframe.c): SkyRef set on either axis and SkyRefIs != IGNORED.
    fn is_offset(&self) -> bool {
        (self.test_sky_ref(1) || self.test_sky_ref(2)) && self.sky_ref_is != SkyRefIs::Ignored
    }

    fn default_title(&self) -> String {
        // Mirrors C `SkyFrame::GetTitle` (skyframe.c:4404). "coordinates" vs
        // "offsets" depending on whether an offset system is in use.
        let offset = self.is_offset();
        let word = if offset { "offsets" } else { "coordinates" };
        let mut base = match self.system {
            SkySystem::Fk5 => {
                let eq = fmt_decimal_yr(pal::epj(self.equinox_mjd()));
                format!("FK5 equatorial {word}; mean equinox J{eq}")
            }
            SkySystem::Fk4 | SkySystem::Fk4NoE => {
                let eq = fmt_decimal_yr(pal::epb(self.equinox_mjd()));
                let ep = fmt_decimal_yr(pal::epb(self.epoch_mjd()));
                let stem = if matches!(self.system, SkySystem::Fk4NoE) {
                    format!("FK4 equatorial {word}; no E-terms")
                } else {
                    format!("FK4 equatorial {word}")
                };
                format!("{stem}; mean equinox B{eq}; epoch B{ep}")
            }
            SkySystem::J2000 => format!("J2000 equatorial {word}"),
            SkySystem::Icrs => format!("ICRS {word}"),
            SkySystem::Azel => format!("Horizon (Azimuth/Elevation) {word}"),
            SkySystem::Gappt => {
                // The doubled "; ; epoch" is a verbatim quirk of C's GetTitle
                // (skyframe.c:4568, the format string contains "; ; epoch").
                let ep = fmt_decimal_yr(pal::epj(self.epoch_mjd()));
                format!("Geocentric apparent equatorial {word}; ; epoch J{ep}")
            }
            SkySystem::Ecliptic => {
                let eq = fmt_decimal_yr(pal::epj(self.equinox_mjd()));
                format!("Ecliptic {word}; mean equinox J{eq}")
            }
            SkySystem::Helioecliptic => {
                let ep = fmt_decimal_yr(pal::epj(self.epoch_mjd()));
                format!("Helio-ecliptic {word}; mean equinox J2000; epoch J{ep}")
            }
            SkySystem::Galactic => format!("IAU (1958) galactic {word}"),
            SkySystem::Supergalactic => format!("De Vaucouleurs supergalactic {word}"),
            SkySystem::Unknown => format!("Spherical {word}"),
        };

        // Append either the offset origin/pole description or the projection,
        // mirroring C (skyframe.c:4629): only one is included.
        if offset {
            let word = if self.sky_ref_is == SkyRefIs::Pole {
                "pole"
            } else {
                "origin"
            };
            // C formats external axes 0 and 1 in order via astFormat with
            // their astGetSkyRef values. sky_ref is stored in internal order,
            // so external axis e (0-based) reads sky_ref at the internal slot
            // it maps to (invperm), and is formatted with that external axis —
            // identical to how the SRef cards' comments are produced below.
            let invperm = [self.lon_axis(), self.lat_axis()];
            let internal_of_ext =
                |ext0: usize| invperm.iter().position(|&e| e == ext0).unwrap_or(ext0);
            let v0 = self
                .format(1, self.sky_ref[internal_of_ext(0)])
                .unwrap_or_default();
            let v1 = self
                .format(2, self.sky_ref[internal_of_ext(1)])
                .unwrap_or_default();
            base.push_str(&format!("; {word} at {v0} {v1}"));
        } else if let Some(ref proj) = self.projection {
            let proj = proj.trim_end();
            if !proj.is_empty() {
                base.push_str(&format!("; {proj} projection"));
            }
        }
        base
    }

    /// Default axis labels for the current system. Mirrors C
    /// `SkyFrame::GetLabel` (skyframe.c:3564): the per-system longitude /
    /// latitude names, with " offset" appended on each when an offset
    /// coordinate system is in use.
    fn default_labels(&self) -> (String, String) {
        let (lon, lat) = match self.system {
            SkySystem::Galactic => ("Galactic longitude", "Galactic latitude"),
            SkySystem::Ecliptic => ("Ecliptic longitude", "Ecliptic latitude"),
            SkySystem::Helioecliptic => ("Helio-ecliptic longitude", "Helio-ecliptic latitude"),
            SkySystem::Supergalactic => ("Supergalactic longitude", "Supergalactic latitude"),
            SkySystem::Azel => ("Azimuth", "Elevation"),
            SkySystem::Unknown => ("Longitude", "Latitude"),
            // Equatorial systems (FK4/FK4-no-E/FK5/J2000/ICRS/GAPPT).
            _ => ("Right ascension", "Declination"),
        };
        if self.is_offset() {
            (format!("{lon} offset"), format!("{lat} offset"))
        } else {
            (lon.to_string(), lat.to_string())
        }
    }

    /// Default axis units for the current system. The longitude axis is shown
    /// as a time (hh:mm:ss) only for equatorial systems — those involving the
    /// Earth's equator (FK4, FK4-no-E, ICRS, FK5, J2000, GAPPT), per C's
    /// `GetAsTime` + `IsEquatorial` (skyframe.c:2495,2565). All other systems
    /// (Galactic, Ecliptic, Supergalactic, Helio-ecliptic, AzEl, Unknown)
    /// default to degrees on both axes.
    fn default_units(&self) -> (&'static str, &'static str) {
        let (lon, lat) = self.default_axis_units();
        (lon.text, lat.text)
    }

    /// [`default_units`](Self::default_units), each with whether it
    /// describes the formatted axis values rather than naming a unit.
    ///
    /// C derives the default Unit from the axis Format with `DHmsUnit`
    /// (skyaxis.c:2279), which flags a description (its `descr` argument)
    /// when the Format has more than one field, or a single minutes or
    /// seconds field of a time. The equatorial defaults come from the
    /// multi-field Formats `hms.1` and `dms`; the others from the
    /// single-field decimal Format `d.4` (`GetFormat`, skyframe.c:3394),
    /// which names a unit.
    fn default_axis_units(&self) -> (SkyAxisUnit, SkyAxisUnit) {
        if self.system.is_equatorial() {
            (
                SkyAxisUnit {
                    text: "hh:mm:ss.s",
                    describes_values: true,
                },
                SkyAxisUnit {
                    text: "ddd:mm:ss",
                    describes_values: true,
                },
            )
        } else {
            let degrees = SkyAxisUnit {
                text: "degrees",
                describes_values: false,
            };
            (degrees, degrees)
        }
    }
}

/// A SkyFrame axis's default Unit. See [`SkyFrame::default_axis_units`].
#[derive(Clone, Copy)]
struct SkyAxisUnit {
    text: &'static str,
    describes_values: bool,
}

impl Frame for SkyFrame {
    fn naxes(&self) -> usize {
        2
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        // Apply the SkyFrame's external->internal axis permutation so
        // the returned axis is the underlying physical axis
        // (0 = lon, 1 = lat).
        (self, self.axis_permutation()[axis])
    }

    fn direction(&self, axis: usize) -> bool {
        // C `SkyFrame::GetDirection` (skyframe.c:2828-2905): all sky
        // systems are left-handed, so the longitude axis is reversed
        // (direction 0) and the latitude axis is not (direction 1). The
        // axis index is permuted to its internal physical axis first
        // (0 = longitude, 1 = latitude).
        let phys = self.perm[axis - 1];
        phys != 0
    }

    fn get_active_unit(&self) -> bool {
        false // SkyFrame always reports 0 (frame.c:4651, SkyAxis present).
    }

    fn set_active_unit(&mut self, _value: bool) {}

    fn test_active_unit(&self) -> bool {
        false
    }

    fn effective_label(&self, axis: usize) -> String {
        self.axis_label(axis).unwrap_or_default()
    }

    fn effective_unit(&self, axis: usize) -> String {
        // SkyFrame has no per-axis Unit override; the effective unit is
        // the system default for the physical (lon/lat) axis.
        let phys = self.physical_axis(axis).unwrap_or(0);
        let (u_lon, u_lat) = self.default_units();
        if phys == 0 { u_lon } else { u_lat }.to_string()
    }

    fn internal_unit(&self, _axis: usize) -> String {
        // SkyFrame axes are angles stored internally in radians. Mirrors
        // C `astGetInternalUnit` for a SkyAxis returning "rad".
        "rad".to_string()
    }

    /// Mirrors C `SkyFrame::GetNormUnit` (skyframe.c:4349), which asks the
    /// SkyAxis, and `SkyAxis::GetAxisNormUnit` (skyaxis.c:3095). A Unit
    /// that describes the axis values, such as `"hh:mm:ss.s"`, has no
    /// normalised form and is returned unchanged; this has to be decided
    /// here rather than by the units parser, which reads the spaces in a
    /// description such as `"hh mm ss"` as multiplication. A Unit that
    /// names a unit, such as `"degrees"`, is normalised.
    ///
    /// C also normalises a Unit that has been set on the axis, and the
    /// `"rad"` a `%`-style Format implies. Neither can arise here: this
    /// SkyFrame has no Unit or Format attribute, so its Unit is always the
    /// default for its System.
    fn norm_unit(&self, axis: usize) -> String {
        let phys = self.physical_axis(axis).unwrap_or(0);
        let (lon, lat) = self.default_axis_units();
        let unit = if phys == 0 { lon } else { lat };
        if unit.describes_values {
            unit.text.to_string()
        } else {
            crate::units::norm_unit(unit.text)
        }
    }

    fn effective_symbol(&self, axis: usize) -> String {
        self.axis_symbol(axis).unwrap_or_default()
    }

    fn test_axis_label(&self, axis: usize) -> bool {
        // Defer to the inherent per-axis label-override test.
        SkyFrame::test_axis_label(self, axis).unwrap_or(false)
    }

    fn test_axis_unit(&self, _axis: usize) -> bool {
        // SkyFrame has no per-axis Unit override; the unit is always
        // the system default, so `astTestUnit` is effectively false.
        false
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..=2", axis),
            ));
        }
        let phys = self.perm[axis - 1];
        let as_time = self.effective_as_time(phys);

        // For equatorial systems the axes behave as standard RA/Dec and are
        // formatted sexagesimally. For non-equatorial systems (galactic,
        // ecliptic, supergalactic, unknown) C `GetFormat` (skyframe.c:3536)
        // builds a decimal format `d.<digits-3>` (or `h.<digits-2>` when
        // AsTime is set), i.e. decimal degrees/hours. Digits defaults to 7.
        if !is_equatorial(self.system) {
            let digits = 7i32;
            return Ok(if as_time {
                let ndp = (digits - 2).max(0) as usize;
                let hours = (value * 12.0 / std::f64::consts::PI).rem_euclid(24.0);
                format!("{hours:.ndp$}")
            } else {
                let ndp = (digits - 3).max(0) as usize;
                let deg = value * 180.0 / std::f64::consts::PI;
                format!("{deg:.ndp$}")
            });
        }

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
        let as_time = self.effective_as_time(phys);
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

    fn resolve_points(
        &self,
        point1: &[f64],
        point2: &[f64],
        input: &crate::coords::PointSet,
    ) -> AstResult<crate::coords::PointSet> {
        // Faithful port of `skyframe.c::ResolvePoints` (8399). Unlike the
        // planar base-Frame implementation, this resolves each vector using
        // spherical (great-circle) geometry: the parallel component is the
        // signed arc distance measured along the great circle through
        // `point1`->`point2`, and the perpendicular component is the signed
        // arc distance from that great circle to the point.
        let npoint = input.npoint();
        let mut d1v = vec![BAD_VALUE; npoint];
        let mut d2v = vec![BAD_VALUE; npoint];

        // Axis permutation → (longitude,latitude) order. If the axes have been
        // swapped, the sign of the returned perpendicular distances flips
        // (skyframe.c:8575).
        let perm = self.perm;
        let sign = if perm[0] == 0 { -1.0 } else { 1.0 };

        // Permute the two basis-vector endpoints into (lon,lat) order.
        let mut p1 = [0.0f64; 2];
        let mut p2 = [0.0f64; 2];
        p1[perm[0]] = point1[0];
        p1[perm[1]] = point1[1];
        p2[perm[0]] = point2[0];
        p2[perm[1]] = point2[1];

        // Unit 3-vectors for the basis endpoints.
        let v1 = pal::dcs2c(p1[0], p1[1]);
        let v2 = pal::dcs2c(p2[0], p2[1]);

        // Unit normal to the great-circle plane through v1 and v2. Undefined
        // (bad results) when the endpoints coincide or are antipodal.
        let (n1, vmod) = pal::dvn(&pal::dvxv(&v2, &v1));
        if vmod > 0.0 {
            // v5 is 90 degrees from v1 along the great circle, towards v2.
            let v5 = pal::dvxv(&v1, &n1);

            for ip in 0..npoint {
                let point = input.point(ip);
                let (px, py) = (point[0], point[1]);
                if is_bad(px) || is_bad(py) {
                    continue;
                }

                // Permute the outlying point into (lon,lat) order.
                let mut p3 = [0.0f64; 2];
                p3[perm[0]] = px;
                p3[perm[1]] = py;
                let v3 = pal::dcs2c(p3[0], p3[1]);

                // Unit normal to the great circle through v3 perpendicular to
                // n1. Undefined when v3 lies on the pole of the basis circle.
                let (n2, vmod2) = pal::dvn(&pal::dvxv(&v3, &n1));
                if vmod2 <= 0.0 {
                    continue;
                }

                // Point of closest approach: intersection of the two great
                // circles = n1 x n2 (already unit length).
                let v4 = pal::dvxv(&n1, &n2);

                // Parallel distance (signed, -PI..PI).
                d1v[ip] = pal::dvdv(&v4, &v5).atan2(pal::dvdv(&v4, &v1));
                // Perpendicular distance (signed by axis order).
                d2v[ip] = sign * pal::dvdv(&v3, &n1).atan2(pal::dvdv(&v3, &v4));
            }
        }

        crate::coords::PointSet::from_columns(&[&d1v, &d2v])
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if a.iter().chain(b.iter()).chain(c.iter()).any(|&v| is_bad(v)) {
            return Ok(BAD_VALUE);
        }
        // The angle is undefined when either end point coincides with
        // the vertex (skyframe.c:1182-1189).
        if (a[0] == b[0] && a[1] == b[1]) || (c[0] == b[0] && c[1] == b[1]) {
            return Ok(BAD_VALUE);
        }
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        // Bearings of A and C as seen *from the vertex B*; the result
        // is their signed difference, corrected from "angle from
        // north" to "angle from axis 2" when the axes are permuted
        // (skyframe.c:1193-1209).
        let anga = pal::dbear(b[lp], b[bp], a[lp], a[bp]);
        let angc = pal::dbear(b[lp], b[bp], c[lp], c[bp]);
        let mut result = angc - anga;
        if self.perm[0] != 0 {
            result = std::f64::consts::FRAC_PI_2 - result;
        }
        Ok(pal::drange(result))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        if from.iter().chain(to.iter()).any(|&v| is_bad(v)) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; 2]);
        }

        let lp = self.lon_pos();
        let bp = self.lat_pos();

        let v1 = pal::dcs2c(from[lp], from[bp]);
        let v2 = pal::dcs2c(to[lp], to[bp]);

        // C reverses the vector order (palDvxv(v2, v1) — skyframe.c:7697)
        // and rotates by +offset; keep that exact form so results match
        // bit-for-bit even where rounding noise dominates the cross
        // product (nearly coincident points).
        let axis_vec = pal::dvxv(&v2, &v1);
        let (axis_norm, axis_len) = pal::dvn(&axis_vec);

        // Coincident or diametrically opposite points define no great
        // circle. C generates output only when the offset is an exact
        // multiple of pi (rotating v1 by cos(offset)); otherwise the
        // result is bad (skyframe.c:7709-7727).
        if axis_len == 0.0 {
            if dist.sin() == 0.0 {
                let scale = dist.cos();
                let v3 = [v1[0] * scale, v1[1] * scale, v1[2] * scale];
                // C constrains the longitude to 0..2*pi, which palDcc2s does
                // not do itself (skyframe.c:7718-7720).
                let (lon, lat) = pal::dcc2s(&v3);
                let mut result = vec![0.0; 2];
                result[lp] = pal::dranrm(lon);
                result[bp] = lat;
                return Ok(result);
            }
            return Ok(vec![BAD_VALUE; 2]);
        }

        let rot_vec = [
            axis_norm[0] * dist,
            axis_norm[1] * dist,
            axis_norm[2] * dist,
        ];

        let rmat = pal::dav2m(&rot_vec);
        let v_out = pal::dmxv(&rmat, &v1);
        // C constrains the longitude to 0..2*pi (skyframe.c:7744-7746).
        let (lon, lat) = pal::dcc2s(&v_out);

        let mut result = vec![0.0; 2];
        result[lp] = pal::dranrm(lon);
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

    /// Every SkyFrame axis carries a SkyAxis, so the `astAx*` methods get
    /// celestial wrapping. Which SkyAxis depends on the axis permutation.
    /// Mirrors the Axis array a SkyFrame inherits from Frame, reached through
    /// `astGetAxis` (frame.c:5309) after `astValidateAxis` permutes.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.axes[self.perm[axis - 1]].kind()
    }

    /// Offset along a great circle at a given bearing. Mirrors C
    /// `SkyFrame::Offset2` (skyframe.c:7937): builds the reference
    /// vectors via Shcal, offsets via Shapp, normalises, and returns the
    /// bearing of the great circle at the end point.
    fn offset2(&self, point1: &[f64], angle: f64, offset: f64) -> AstResult<(Vec<f64>, f64)> {
        let perm = self.perm;

        // Bad inputs give all-BAD output and a BAD returned angle.
        if is_bad(point1[0]) || is_bad(point1[1]) || is_bad(angle) || is_bad(offset) {
            return Ok((vec![BAD_VALUE, BAD_VALUE], BAD_VALUE));
        }

        let piby2 = std::f64::consts::FRAC_PI_2;

        // Apply the axis permutation to obtain (longitude, latitude) order.
        let mut p1 = [0.0f64; 2];
        p1[perm[0]] = point1[0];
        p1[perm[1]] = point1[1];

        // If the axes are permuted, convert the supplied angle into a
        // position angle (measured from north).
        let pa = if perm[0] == 0 { angle } else { piby2 - angle };

        // Shcal: reference vector R0 and the point 90 degrees along the
        // great circle R3.
        let (r0, r3) = shcal(p1[0], p1[1], pa);

        // Shapp: the new position offset by `offset` along the great circle.
        let mut p2 = shapp(offset, &r0, &r3, p1[0]);

        // Normalise the result. `p2` is in physical (lon, lat) order;
        // `norm` reads/writes lon at lon_pos and lat at lat_pos, so map
        // through the permutation and back.
        {
            let mut ext = vec![0.0; 2];
            ext[self.lon_pos()] = p2[0];
            ext[self.lat_pos()] = p2[1];
            self.norm(&mut ext)?;
            p2[0] = ext[self.lon_pos()];
            p2[1] = ext[self.lat_pos()];
        }

        // Compute the bearing of the great circle at the required point.
        let sina1 = p2[0].sin();
        let cosa1 = p2[0].cos();
        let sinb1 = p2[1].sin();
        let cosb1 = p2[1].cos();

        // Q1: point in the meridian of p2, 90 deg further north.
        let q1 = [-sinb1 * cosa1, -sinb1 * sina1, cosb1];
        // Q2: point on the equator, 90 deg west of p2.
        let q2 = [-sina1, cosa1, 0.0];
        // Q3: point 90 deg away from p2 along the great circle.
        let cosoff = offset.cos();
        let sinoff = offset.sin();
        let q3 = [
            -sinoff * r0[0] + cosoff * r3[0],
            -sinoff * r0[1] + cosoff * r3[1],
            -sinoff * r0[2] + cosoff * r3[2],
        ];

        let pa_end = pal::dvdv(&q3, &q2).atan2(pal::dvdv(&q3, &q1));
        let result = if perm[0] == 0 { pa_end } else { piby2 - pa_end };
        let result = pal::dranrm(result);

        // Undo the axis permutation on the output coordinates.
        let mut point2 = vec![0.0; 2];
        point2[0] = p2[perm[0]];
        point2[1] = p2[perm[1]];

        Ok((point2, result))
    }

    /// Extend a box to include the effect of the celestial poles. Mirrors
    /// C `SkyFrame::NormBox` (skyframe.c:7244): tests whether the box (via
    /// the supplied region-membership predicate) includes either pole and,
    /// if so, expands the longitude range to 0..2*PI and pins the latitude
    /// limit to the pole.
    ///
    /// C tests its error status after transforming the two poles and treats a
    /// failed transformation as finding neither pole, because the values it
    /// would otherwise read were never written (skyframe.c:7338-7342). Here the
    /// predicate returns a `Result`, so a failure propagates instead and the
    /// limits are left as they arrived — the same outcome without the read.
    fn norm_box(
        &self,
        lbnd: &mut [f64],
        ubnd: &mut [f64],
        inside: &dyn Fn(&[f64]) -> AstResult<bool>,
    ) -> AstResult<()> {
        let perm = self.perm;
        let piby2 = std::f64::consts::FRAC_PI_2;

        // Obtain the sky longitude and latitude limits, allowing for any
        // axis permutation.
        let mut lb = [0.0f64; 2];
        let mut ub = [0.0f64; 2];
        lb[perm[0]] = lbnd[0];
        lb[perm[1]] = lbnd[1];
        ub[perm[0]] = ubnd[0];
        ub[perm[1]] = ubnd[1];

        // Coordinates of the north and south poles, in external axis order.
        let (npole, spole) = if perm[0] == 0 {
            ([0.0, piby2], [0.0, -piby2])
        } else {
            ([piby2, 0.0], [-piby2, 0.0])
        };

        // Test whether the box includes each pole.
        let north_in = inside(&npole)?;
        let south_in = inside(&spole)?;

        // If the box includes the north pole...
        if north_in {
            let t = if !is_bad(ub[1]) && !is_bad(lb[1]) {
                let t = pal::drange(ub[1]);
                let t2 = pal::drange(lb[1]);
                if t2 < t { t2 } else { t }
            } else {
                BAD_VALUE
            };
            lb[1] = t;
            ub[1] = piby2;
            lb[0] = 0.0;
            ub[0] = 2.0 * std::f64::consts::PI;
        }

        // If the box includes the south pole...
        if south_in {
            let t = if !is_bad(ub[1]) && !is_bad(lb[1]) {
                let t = pal::drange(ub[1]);
                let t2 = pal::drange(lb[1]);
                if t2 > t { t2 } else { t }
            } else {
                BAD_VALUE
            };
            lb[1] = -piby2;
            ub[1] = t;
            lb[0] = 0.0;
            ub[0] = 2.0 * std::f64::consts::PI;
        }

        // Return the modified limits, undoing the axis permutation.
        lbnd[0] = lb[perm[0]];
        lbnd[1] = lb[perm[1]];
        ubnd[0] = ub[perm[0]];
        ubnd[1] = ub[perm[1]];
        Ok(())
    }

    // --- Great-circle line geometry (skyframe.c LineDef/LineOffset/
    // LineCrossing/LineContains) ---
    //
    // These override the planar Frame defaults with spherical geometry. A line
    // is a great-circle arc; positions are held as 3-D unit vectors in the
    // returned `LineDef`'s `sky` field. All four require a `SkyLineDef` built
    // by `line_def`.

    fn line_def(&self, start: &[f64], end: &[f64]) -> AstResult<Option<LineDef>> {
        if is_bad(start[0]) || is_bad(start[1]) || is_bad(end[0]) || is_bad(end[1]) {
            return Ok(None);
        }

        let perm = self.perm;

        // Permute external axis values into physical (longitude, latitude)
        // order before converting to Cartesian.
        let mut p1 = [0.0; 2];
        let mut p2 = [0.0; 2];
        p1[perm[0]] = start[0];
        p1[perm[1]] = start[1];
        p2[perm[0]] = end[0];
        p2[perm[1]] = end[1];

        let sv = pal::dcs2c(p1[0], p1[1]);
        let ev = pal::dcs2c(p2[0], p2[1]);

        // Great-circle length in radians. `palDvdv` gives the raw dot product;
        // correct for palDcs2c rounding so the vectors are treated as unit
        // length before taking the arc-cosine.
        let dot = pal::dvdv(&sv, &ev);
        let ls = sv[0] * sv[0] + sv[1] * sv[1] + sv[2] * sv[2];
        let le = ev[0] * ev[0] + ev[1] * ev[1] + ev[2] * ev[2];
        let coslength = dot / (ls * le).sqrt();
        let length = if coslength < -1.0 {
            std::f64::consts::PI
        } else if coslength > 1.0 {
            0.0
        } else {
            coslength.acos()
        };

        // Pole of the great circle, oriented so it lies to the left of the
        // line travelling start→end. Undefined for a zero-length or 180° arc,
        // where an arbitrary nearby direction is used.
        let pole_raw = if length == 0.0 || length > std::f64::consts::PI - 5.0e-11 {
            let temp = pal::dcs2c(p1[0] + 0.01, p1[1] + 0.01);
            pal::dvxv(&temp, &sv)
        } else {
            pal::dvxv(&ev, &sv)
        };
        let (q, _len) = pal::dvn(&pole_raw);

        // Quarter point: 90° along the great circle from the start.
        let dir = pal::dvxv(&sv, &q);

        // Normalised external-order end points (mirrors C start_2d/end_2d).
        let mut start_2d = [start[0], start[1]];
        let mut end_2d = [end[0], end[1]];
        self.norm(&mut start_2d)?;
        self.norm(&mut end_2d)?;

        Ok(Some(LineDef {
            length,
            infinite: false,
            start: start_2d,
            end: end_2d,
            dir: [0.0, 0.0],
            q: [0.0, 0.0],
            sky: Some(SkyLineDef {
                start: sv,
                end: ev,
                dir,
                q,
            }),
        }))
    }

    fn line_offset(&self, line: &LineDef, par: f64, prp: f64) -> AstResult<[f64; 2]> {
        let sl = line.sky.as_ref().ok_or_else(|| {
            AstError::new(ErrorCode::Inter, "line_offset: line is not a SkyFrame line")
        })?;

        // Move a distance `par` along the great circle from the start.
        let c = par.cos();
        let s = par.sin();
        let nx = c * sl.start[0] + s * sl.dir[0];
        let ny = c * sl.start[1] + s * sl.dir[1];
        let nz = c * sl.start[2] + s * sl.dir[2];

        // Move a distance `prp` at right angles, towards the pole.
        let v = if prp != 0.0 {
            let c2 = prp.cos();
            let s2 = prp.sin();
            [
                c2 * nx + s2 * sl.q[0],
                c2 * ny + s2 * sl.q[1],
                c2 * nz + s2 * sl.q[2],
            ]
        } else {
            [nx, ny, nz]
        };

        let (lon, lat) = pal::dcc2s(&v);
        let p = [lon, lat];
        let perm = self.perm;
        Ok([p[perm[0]], p[perm[1]]])
    }

    fn line_crossing(&self, l1: &LineDef, l2: &LineDef) -> AstResult<(bool, [f64; 2], f64)> {
        let sl1 = l1.sky.as_ref().ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "line_crossing: line 1 is not a SkyFrame line",
            )
        })?;
        let sl2 = l2.sky.as_ref().ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "line_crossing: line 2 is not a SkyFrame line",
            )
        })?;

        // The two great circles intersect along the direction perpendicular to
        // both poles.
        let temp = pal::dvxv(&sl1.q, &sl2.q);
        let (mut b, _len) = pal::dvn(&temp);

        let mut dist = 0.0;
        let mut result = false;

        // See if the intersection point lies within both arcs; the `dist`
        // side-effect on the `l1` test mirrors C's LineIncludes writing through
        // its `dist` pointer whenever it is called.
        if sky_line_includes(sl2, l2.length, l2.infinite, &b).0 {
            let (r1, t2) = sky_line_includes(sl1, l1.length, l1.infinite, &b);
            dist = t2;
            if r1 {
                result = true;
            }
        }
        if !result {
            b = [-b[0], -b[1], -b[2]];
            if sky_line_includes(sl2, l2.length, l2.infinite, &b).0 {
                let (r1, t2) = sky_line_includes(sl1, l1.length, l1.infinite, &b);
                dist = t2;
                if r1 {
                    result = true;
                }
            }
        }

        let (lon, lat) = pal::dcc2s(&b);
        let p = [lon, lat];
        let perm = self.perm;
        Ok((result, [p[perm[0]], p[perm[1]]], dist))
    }

    fn line_contains(&self, l: &LineDef, point: &[f64]) -> AstResult<bool> {
        let sl = l.sky.as_ref().ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "line_contains: line is not a SkyFrame line",
            )
        })?;
        if is_bad(point[0]) || is_bad(point[1]) {
            return Ok(false);
        }

        let perm = self.perm;
        let mut p1 = [0.0; 2];
        p1[perm[0]] = point[0];
        p1[perm[1]] = point[1];
        let b = pal::dcs2c(p1[0], p1[1]);

        // The point of closest approach must lie within the arc, and the point
        // must be within a small angle of the great circle (90° from the pole).
        if sky_line_includes(sl, l.length, l.infinite, &b).0 {
            let t1 = pal::dvdv(&sl.q, &b);
            let mut t2 = 1.0e-7 * l.length;
            if t2 < 1.0e-10 {
                t2 = 1.0e-10;
            }
            if t1.abs() <= t2 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, 2)?;
        let old = self.perm;
        self.perm[0] = old[perm[0] - 1];
        self.perm[1] = old[perm[1] - 1];
        Ok(())
    }

    // Match-control attributes — mirror C `astGet/SetMinAxes` etc.
    // SkyFrame defaults Permute to false (mirrors skyframe.c at the
    // SetPermute line); the others inherit the trait default
    // (`naxes()` for cardinality, `false` for booleans).
    fn min_axes(&self) -> usize {
        self.min_axes_opt.unwrap_or(2)
    }
    fn set_min_axes(&mut self, n: usize) {
        self.min_axes_opt = Some(n);
    }
    fn test_min_axes(&self) -> bool {
        self.min_axes_opt.is_some()
    }
    fn max_axes(&self) -> usize {
        self.max_axes_opt.unwrap_or(2)
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
    /// Permute defaults to `true` (`1`). SkyFrame does not override the
    /// Permute getter, so it inherits the parent Frame default
    /// (`astMAKE_GET(Frame,Permute,int,0,…)` returns `1` when unset —
    /// frame.c:13459-13460). This default is load-bearing for the
    /// `astConvert`/Match path: it lets axis-swapped SkyFrames (e.g.
    /// CTYPE1=DEC, CTYPE2=RA) align (skyframe.c:6826
    /// `match = !swap || astGetPermute(template)`).
    fn permute(&self) -> bool {
        self.permute_opt.unwrap_or(true)
    }
    fn set_permute(&mut self, v: bool) {
        self.permute_opt = Some(v);
    }
    fn test_permute(&self) -> bool {
        self.permute_opt.is_some()
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = previous.as_any().downcast_ref::<SkyFrame>();
        match prev {
            Some(prev_sky) => {
                // When both systems are Unknown, C `MakeSkyMapping`
                // (skyframe.c:5975) returns a UnitMap and returns before the
                // `SkyOffsetMap` section, so no offset rotation is ever
                // applied. Two Unknown SkyFrames are assumed identical; this
                // keeps a bare Title/SkyRefIs change from breaking FrameSet
                // integrity. Mirror that early return here — applying the
                // offset maps below would wrongly rotate an Unknown offset
                // Frame.
                if prev_sky.system == SkySystem::Unknown && self.system == SkySystem::Unknown {
                    return Ok(wrap_with_axis_swap(
                        Box::new(UnitMap::new(2)),
                        prev_sky,
                        self,
                    ));
                }

                let prev_last = prev_sky.compute_last()?;
                let self_last = self.compute_last()?;
                let mut mapping: DynMapping = make_sky_mapping(
                    prev_sky.system,
                    prev_sky.equinox_mjd(),
                    prev_sky.epoch_mjd(),
                    self.system,
                    self.equinox_mjd(),
                    self.epoch_mjd(),
                    self.system, // align in current system
                    prev_sky.obs_lat.unwrap_or(0.0),
                    prev_last,
                    prev_sky.compute_diurab(),
                    self.obs_lat.unwrap_or(0.0),
                    self_last,
                    self.compute_diurab(),
                )?;

                // Replace the system-conversion chain by a UnitMap when
                // its effect everywhere on the sphere is below the
                // frames' SkyTol (skyframe.c:6546 TestSlaUnit).
                if crate::frames::sky_convert::is_effectively_unit(
                    mapping.as_ref(),
                    prev_sky.sky_tol().min(self.sky_tol()),
                ) {
                    mapping = Box::new(UnitMap::new(2));
                }

                // Prepend source offset removal (offset→standard).
                if let Some(off_map) = make_offset_mapping(
                    prev_sky.sky_ref,
                    prev_sky.sky_ref_p,
                    prev_sky.sky_ref_is,
                    prev_sky.sky_ref_set[0] || prev_sky.sky_ref_set[1],
                )? {
                    mapping = Box::new(CmpMap::new(off_map, mapping, Composition::Series)?);
                }

                // Append target offset application (standard→offset = inverse).
                if let Some(mut off_map) = make_offset_mapping(
                    self.sky_ref,
                    self.sky_ref_p,
                    self.sky_ref_is,
                    self.sky_ref_set[0] || self.sky_ref_set[1],
                )? {
                    off_map.invert();
                    mapping = Box::new(CmpMap::new(mapping, off_map, Composition::Series)?);
                }

                // Wrap in PermMap swap when either Frame's external axis
                // 0 is its physical latitude axis (perm = [lat, lon]).
                // Mirrors C `SubFrame` (skyframe.c:10082-10117): the
                // make_sky_mapping output is in internal (lon, lat)
                // order, but the FrameSet's chain feeds and consumes
                // values in each Frame's external order. Without these
                // swaps the delta is applied to the wrong axes.
                Ok(wrap_with_axis_swap(mapping, prev_sky, self))
            }
            None => {
                // Previous was not a SkyFrame -- return UnitMap.
                Ok(Box::new(UnitMap::new(2)))
            }
        }
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549) and SkyFrame
        // does not override it: same class, astConvert succeeds, mapping
        // simplifies to a UnitMap. Two SkyFrames whose System/Epoch/Equinox/
        // SkyRef/NegLon differ produce a non-UnitMap conversion and so
        // compare unequal, exactly as the old attribute checks intended.
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
        SkyFrame::domain(self)
    }

    fn clear_domain(&mut self) {
        SkyFrame::clear_domain(self);
    }

    fn test_domain(&self) -> bool {
        SkyFrame::test_domain(self)
    }

    fn ident(&self) -> &str {
        SkyFrame::ident(self)
    }

    fn set_ident(&mut self, ident: &str) {
        SkyFrame::set_ident(self, ident);
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (skyframe.c:6604 / 9869 / 8010).
    // -----------------------------------------------------------------

    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        skyframe_overlay(template, template_axes, self)
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
        // Mirrors skyframe.c:6604. Step 1: defer to the parent (Frame
        // class) Match for axis-count + Domain pre-checks. We only
        // need the boolean from that call; the returned objects are
        // discarded just as the C does (the axis-association arrays
        // are reconstructed below).
        let parent = super::find_frame::default_match(self, target, match_sub)?;
        if parent.is_none() {
            return Ok(None);
        }

        let target_naxes = target.naxes();

        // Step 2: locate two axes in the target that are derived from
        // a single underlying SkyFrame (in either physical order).
        // Mirrors skyframe.c:6759-6803: walk astPrimaryFrame on each
        // target axis and check whether the resulting primary Frame
        // is a SkyFrame; the two SkyFrame axes need not be adjacent
        // in the target's external axis order.
        let mut target_sky_owned: Option<&SkyFrame> = None;
        let mut target_axis0: Option<usize> = None;
        let mut target_axis1: Option<usize> = None;
        let mut iaxis0: usize = 0;
        let mut iaxis1: usize = 0;
        for iaxis in 0..target_naxes {
            let (primary, local_axis) = crate::frames::specframe::primary_frame_walk(target, iaxis);
            if let Some(sky) = primary.as_any().downcast_ref::<SkyFrame>() {
                if target_axis0.is_none() {
                    target_sky_owned = Some(sky);
                    target_axis0 = Some(iaxis);
                    iaxis0 = local_axis;
                } else {
                    // Both SkyFrame axes must come from the same
                    // primary instance.
                    if !std::ptr::eq(target_sky_owned.unwrap() as *const _, sky as *const _) {
                        return Ok(None);
                    }
                    target_axis1 = Some(iaxis);
                    iaxis1 = local_axis;
                    break;
                }
            }
        }
        let (Some(target_axis0), Some(target_axis1), Some(_target_sky)) =
            (target_axis0, target_axis1, target_sky_owned)
        else {
            return Ok(None);
        };
        // Sanity check: the underlying primary axes must be 0 and 1
        // in either order (skyframe.c:6800-6802).
        if !((iaxis0 == 0 && iaxis1 == 1) || (iaxis0 == 1 && iaxis1 == 0)) {
            return Ok(None);
        }

        // Step 3: axis-swap detection (skyframe.c:6813-6827).
        // swap1: does the template's permutation reverse axis 0?
        let swap1 = self.perm[0] != 0;
        // swap2: are the underlying primary axes reversed in the target?
        let swap2 = iaxis0 != 0;
        let swap = swap1 != swap2;
        if swap && !self.permute() {
            return Ok(None);
        }

        // Step 4: build the association arrays (skyframe.c:6831-6849).
        let preserve_axes = self.preserve_axes();
        let (template_axes_vec, target_axes_vec) = if preserve_axes {
            (
                vec![if swap { 1 } else { 0 }, if swap { 0 } else { 1 }],
                vec![target_axis0 as i32, target_axis1 as i32],
            )
        } else {
            (
                vec![0i32, 1],
                if swap {
                    vec![target_axis1 as i32, target_axis0 as i32]
                } else {
                    vec![target_axis0 as i32, target_axis1 as i32]
                },
            )
        };

        // Step 5: defer to the target's SubFrame to build the result
        // and Mapping (skyframe.c:6856-6857).
        let Some((map, result)) =
            target.sub_frame(Some(self), 2, &target_axes_vec, &template_axes_vec)?
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
        skyframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // Whole-frame extraction: a copy with the requested axis
        // permutation (mirrors the SkyFrame branch of skyframe.c
        // SubFrame at line 10029).
        if target_axes.len() == 2
            && ((target_axes[0] == 0 && target_axes[1] == 1)
                || (target_axes[0] == 1 && target_axes[1] == 0))
        {
            let mut snap = self.clone();
            // Apply axis permutation: target_axes is zero-based.
            if target_axes[0] != 0 {
                let _ = snap.perm_axes(&[2, 1]);
            }
            return Ok(Box::new(snap));
        }

        // Partial / oversized selection: produce a BasicFrame whose
        // per-axis attributes follow the picked SkyFrame axes.
        // Mirrors the parent_subframe branch of skyframe.c SubFrame
        // (line 10128), but only the metadata-carrying portion: the
        // Mapping is built by the caller (default_sub_frame).
        let mut bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        if self.test_domain() {
            bf.set_domain(self.domain().to_string());
        }
        for (i, &t) in target_axes.iter().enumerate() {
            if t < 0 || (t as usize) >= 2 {
                continue;
            }
            // Use the SkyFrame's effective per-axis label/unit/symbol
            // for axis `t` (1-based) -> result axis `i + 1`.
            let _ = bf.set_label(i + 1, self.axis_label(t as usize + 1)?);
            let (u_lon, u_lat) = self.default_units();
            let unit = if t == 0 { u_lon } else { u_lat };
            let _ = bf.set_unit(i + 1, unit.to_string());
            let _ = bf.set_symbol(i + 1, self.axis_symbol(t as usize + 1)?);
            // C's SubFrame attaches the target's own Axis object to the result
            // (frame.c:11023), so the picked axis stays a SkyAxis. skyframe.c
            // SubFrame first pins down IsLatitude and CentreZero on it
            // (skyframe.c:10280): a latitude axis gets both set, a longitude
            // axis takes CentreZero from the SkyFrame's NegLon. The picked axis
            // therefore keeps wrapping its values, and its presence forces the
            // result's ActiveUnit to 0 (frame.c:4669).
            let is_latitude = t as usize == self.lat_pos();
            bf.set_axis_kind(
                i + 1,
                AxisKind::Sky {
                    is_latitude,
                    centre_zero: if is_latitude { true } else { self.neglon },
                },
            );
        }
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// Match / SubFrame / Overlay helpers
// ---------------------------------------------------------------------------

/// Compute Local Apparent Sidereal Time, in radians, at the given
/// epoch and observer location.
///
/// Mirrors `skyframe.c::CalcLAST`: builds a TDB-MJD TimeFrame and a
/// LAST-MJD TimeFrame, applies the observer parameters to both,
/// Format an epoch expressed in decimal years, mirroring C's
/// `astFmtDecimalYr(year, 9)` (frame.c:4189): `sprintf("%#.9g")` followed by
/// trailing-zero removal, but keeping one zero after the decimal point so the
/// result always reads like a year (e.g. "2000.0", "1999.80005"). The `#`
/// flag forces a decimal point; `format_double_g` already strips trailing
/// zeros and the point, so we re-append ".0" when no point remains.
fn fmt_decimal_yr(year: f64) -> String {
    let s = crate::serial::native_write::format_double_g(year, 9);
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

/// transforms the (split) epoch through TDB→LAST and converts the
/// fractional day to radians.
///
/// Splitting the epoch into integer + fractional parts via TimeOrigin
/// preserves precision; mirrors the C `(int) epoch` trick at
/// skyframe.c:1303-1308.
fn compute_last(
    epoch_mjd: f64,
    obs_lon: f64,
    obs_lat: f64,
    obs_alt: f64,
    dut1: f64,
    dtai: Option<f64>,
) -> AstResult<f64> {
    use crate::frames::{TimeFrame, TimeScale, TimeSystem};

    let origin = epoch_mjd.floor();
    let epoch_off = epoch_mjd - origin;

    let mut tdb = TimeFrame::new(TimeSystem::Mjd);
    tdb.set_timescale(TimeScale::Tdb);
    tdb.set_time_origin(origin);
    tdb.set_obs_lon(obs_lon);
    tdb.set_obs_lat(obs_lat);
    tdb.set_obs_alt(obs_alt);
    tdb.set_dut1(dut1);
    if let Some(d) = dtai {
        tdb.set_dtai(d);
    }

    let mut last_tf = TimeFrame::new(TimeSystem::Mjd);
    last_tf.set_timescale(TimeScale::Last);
    last_tf.set_time_origin(origin);
    last_tf.set_obs_lon(obs_lon);
    last_tf.set_obs_lat(obs_lat);
    last_tf.set_obs_alt(obs_alt);
    last_tf.set_dut1(dut1);
    if let Some(d) = dtai {
        last_tf.set_dtai(d);
    }

    let fs = match tdb.convert_to(&last_tf, &[])? {
        Some(fs) => fs,
        None => {
            return Err(AstError::new(
                ErrorCode::Inter,
                "compute_last: TimeFrame TDB->LAST conversion failed",
            ));
        }
    };
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    let pts = crate::coords::PointSet::from_columns(&[&[epoch_off][..]])?;
    let out = mapping.transform(&pts, true)?;
    let last_off_mjd = out.point(0)[0];

    let frac = last_off_mjd - last_off_mjd.floor();
    Ok(frac * 2.0 * std::f64::consts::PI)
}

/// Determine whether a great-circle segment includes a point known to lie on
/// its great circle. Returns `(included, dist)` where `dist` is the distance in
/// radians from the line's start to the point, measured forwards along the
/// line. The end point is not part of the segment. Mirrors C `LineIncludes`
/// (skyframe.c:5659).
fn sky_line_includes(
    sl: &SkyLineDef,
    length: f64,
    infinite: bool,
    point: &[f64; 3],
) -> (bool, f64) {
    // Unsigned distance of the point from the start, 0 - 180 degs.
    let t1 = pal::dvdv(&sl.start, point);
    let mut t2 = t1.acos();

    // If the point is more than 90 degrees from the quarter point it is in the
    // reverse half of the great circle; convert to the forward distance.
    let t3 = pal::dvdv(&sl.dir, point);
    if t3 < -1.0e-10 {
        t2 = 2.0 * std::f64::consts::PI - t2;
    }

    let result = if infinite {
        true
    } else if length > 0.0 {
        t2 < length
    } else {
        t2 == 0.0
    };
    (result, t2)
}

/// Mirrors C `Overlay` for SkyFrame (skyframe.c:8010).
///
/// `result` is the Frame receiving the new attribute values; `template`
/// is the SkyFrame supplying them. Only attributes the template has
/// explicitly set are transferred. When `result` is itself a SkyFrame
/// and the System or SkyRef changes, axis-level state that no longer
/// applies (Title and per-axis Label/Unit/Symbol overrides) is cleared
/// first so the new system's defaults take effect.
fn skyframe_overlay(
    template: &dyn Frame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    let template_sky = match template.as_any().downcast_ref::<SkyFrame>() {
        Some(s) => s,
        None => return Ok(()),
    };

    // Branch 1: result is a SkyFrame.
    if let Some(result_sky) = result.as_any_mut().downcast_mut::<SkyFrame>() {
        let old_system = result_sky.system;
        let new_system = template_sky.system;
        let skyref_changed = template_sky.sky_ref[0] != result_sky.sky_ref[0]
            || template_sky.sky_ref[1] != result_sky.sky_ref[1];

        // If the System will change (and is set on the template), or
        // SkyRef differs, the result Frame's Title is cleared
        // (skyframe.c:8117-8120).
        if (new_system != old_system && template_sky.test_system()) || skyref_changed {
            result_sky.clear_title();

            // If the equatorial-ness changes, clear per-axis overrides
            // so the new system's defaults take over (skyframe.c:
            // 8124-8140).
            if is_equatorial(new_system) != is_equatorial(old_system) || skyref_changed {
                for axis in 0..2 {
                    result_sky.label_overrides[axis] = None;
                    result_sky.symbol_overrides[axis] = None;
                    // Format / AsTime / Direction / Unit live on the
                    // SkyAxis. Reset the per-axis state to its
                    // default by replacing the axis. Latitude vs
                    // longitude is determined by the physical slot.
                    result_sky.axes[axis] = if axis == 0 {
                        super::skyaxis::SkyAxis::longitude()
                    } else {
                        super::skyaxis::SkyAxis::latitude()
                    };
                }
            }
        }

        // Parent Frame attributes (Domain, Title, Epoch, observer
        // attrs) — mirror Frame::Overlay (frame.c:8567-8574). Title
        // copy comes after the optional clear above.
        if template_sky.test_domain() {
            result_sky.set_domain(template_sky.domain());
        }
        if template_sky.test_title() {
            result_sky.set_title(template_sky.title());
        }
        if template_sky.test_epoch() {
            result_sky.set_epoch_mjd(template_sky.epoch_mjd());
        }
        if let Some(v) = template_sky.obs_lat {
            result_sky.set_obs_lat(v);
        }
        if let Some(v) = template_sky.obs_lon {
            result_sky.set_obs_lon(v);
        }
        if let Some(v) = template_sky.obs_alt {
            result_sky.set_obs_alt(v);
        }
        if let Some(v) = template_sky.dtai {
            result_sky.set_dtai(v);
        }
        if let Some(v) = template_sky.dut1 {
            result_sky.set_dut1(v);
        }

        // System / AlignSystem (frame.c:8580-8594).
        if template_sky.test_system() {
            result_sky.set_system(template_sky.system);
        }
        if template_sky.test_align_system() {
            result_sky.set_align_system(template_sky.align_system());
        }

        // SkyFrame-specific OVERLAY block (skyframe.c:8198-8205).
        if template_sky.test_equinox() {
            result_sky.set_equinox_mjd(template_sky.equinox_mjd());
        }
        if template_sky.test_projection() {
            if let Some(p) = template_sky.projection() {
                result_sky.set_projection(p);
            }
        }
        if template_sky.test_neglon() {
            result_sky.set_neglon(template_sky.neglon());
        }
        if template_sky.test_sky_tol() {
            result_sky.set_sky_tol(template_sky.sky_tol());
        }
        if template_sky.test_align_offset() {
            result_sky.set_align_offset(template_sky.align_offset());
        }
        if template_sky.test_sky_ref_is() {
            result_sky.set_sky_ref_is(template_sky.sky_ref_is());
        }

        // OVERLAY2 macro: per-axis SkyRef / SkyRefP entries indexed
        // by template_axes (skyframe.c:8204-8205).
        let (tax0, tax1) = if !template_axes.is_empty() {
            (
                template_axes[0],
                if template_axes.len() > 1 {
                    template_axes[1]
                } else {
                    -1
                },
            )
        } else {
            (0, 1)
        };
        for (result_axis, &t_axis) in [tax0, tax1].iter().enumerate() {
            if t_axis < 0 || (t_axis as usize) >= 2 {
                continue;
            }
            let t = t_axis as usize;
            if template_sky.test_sky_ref(t) {
                let mut sr = result_sky.sky_ref;
                sr[result_axis] = template_sky.sky_ref[t];
                result_sky.sky_ref = sr;
                result_sky.sky_ref_set[result_axis] = true;
            }
            if template_sky.test_sky_ref_p(t) {
                let mut sr = result_sky.sky_ref_p;
                sr[result_axis] = template_sky.sky_ref_p[t];
                result_sky.sky_ref_p = sr;
                result_sky.sky_ref_p_set[result_axis] = true;
            }
        }

        return Ok(());
    }

    // Branch 2: result is not a SkyFrame. Skip System/AlignSystem and
    // any SkyFrame-specific axis attributes; only Frame-level
    // attributes flow through (skyframe.c:8146-8154 — which
    // temporarily clears System/AlignSystem on the template before
    // calling parent_overlay; we achieve the same by skipping them
    // here and routing non-system attributes through the BasicFrame's
    // overlay below).
    if template_sky.test_domain() {
        // Use the result's `set_domain` via type-specific dispatch.
        if let Some(bf) = result
            .as_any_mut()
            .downcast_mut::<super::basic::BasicFrame>()
        {
            bf.set_domain(template_sky.domain().to_string());
        }
    }
    Ok(())
}

/// Wrap an internal-axis-order (lon, lat) → (lon, lat) mapping with
/// PermMap swaps so it operates on each SkyFrame's external axis
/// order. Mirrors C `SubFrame` (skyframe.c:10082-10117): when the
/// target's external axis 0 is its physical latitude axis, prepend a
/// `[lat,lon]→[lon,lat]` PermMap; when the result's external axis 0
/// is its physical latitude axis, append a `[lon,lat]→[lat,lon]`
/// PermMap. Both checks are independent because the two SkyFrames may
/// have different perms (rare, but supported).
///
/// Identity-fast when neither needs swapping. The `PermMap::new` and
/// `CmpMap::new` calls below are infallible for a fixed 2-axis perm
/// against an already-2-input/2-output mapping; the `expect` messages
/// document the invariant.
fn wrap_with_axis_swap(mapping: DynMapping, target: &SkyFrame, result: &SkyFrame) -> DynMapping {
    let target_swap = target.axis_permutation()[0] != 0;
    let result_swap = result.axis_permutation()[0] != 0;
    if !target_swap && !result_swap {
        return mapping;
    }
    let mut out = mapping;
    if target_swap {
        let pre = PermMap::new(&[2, 1], &[2, 1], &[]).expect("trivial 2-axis PermMap construction");
        out = Box::new(
            CmpMap::new(Box::new(pre), out, Composition::Series)
                .expect("compose 2-axis PermMap with 2-axis mapping"),
        );
    }
    if result_swap {
        let post =
            PermMap::new(&[2, 1], &[2, 1], &[]).expect("trivial 2-axis PermMap construction");
        out = Box::new(
            CmpMap::new(out, Box::new(post), Composition::Series)
                .expect("compose 2-axis mapping with 2-axis PermMap"),
        );
    }
    out
}

/// Returns true if `system` is one of the equatorial SkyFrame systems
/// (FK4, FK4-no-E, FK5, ICRS, J2000, GAPPT). Mirrors C `IsEquatorial`
/// (skyframe.c).
fn is_equatorial(system: SkySystem) -> bool {
    matches!(
        system,
        SkySystem::Fk4
            | SkySystem::Fk4NoE
            | SkySystem::Fk5
            | SkySystem::Icrs
            | SkySystem::J2000
            | SkySystem::Gappt
    )
}

/// Mirrors C `SubFrame` for SkyFrame (skyframe.c:9869).
fn skyframe_sub_frame(
    target: &SkyFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-SkyFrame branch (skyframe.c:10003-10117).
    if result_naxes == 2
        && target_axes.len() == 2
        && ((target_axes[0] == 0 && target_axes[1] == 1)
            || (target_axes[0] == 1 && target_axes[1] == 0))
    {
        // Form the result from a copy of the target (skyframe.c:10029)
        // and apply the requested axis permutation.
        let mut result = target.clone();
        if target_axes[0] != 0 {
            result.perm_axes(&[2, 1])?;
        }

        // If a template was supplied, overlay it onto the result and
        // pick the alignment system from the template
        // (skyframe.c:10036-10042). We call the overlay helper
        // directly on `&mut result` to avoid round-tripping through a
        // DynFrame box.
        let align_sys = if let Some(t) = template {
            skyframe_overlay(t, template_axes, &mut result)?;
            if let Some(template_sky) = t.as_any().downcast_ref::<SkyFrame>() {
                template_sky.align_system()
            } else {
                target.align_system()
            }
        } else {
            target.align_system()
        };

        // Build the Mapping that takes account of changes between
        // target and result systems (skyframe.c:10071-10074). The
        // AlignOffset / AST__INTFLAG short-circuit (10055-10065) is
        // currently unused by the Rust callers — they always go
        // through MakeSkyMapping.
        let target_last = target.compute_last()?;
        let result_last = result.compute_last()?;
        let mut map: DynMapping = make_sky_mapping(
            target.system,
            target.equinox_mjd(),
            target.epoch_mjd(),
            result.system,
            result.equinox_mjd(),
            result.epoch_mjd(),
            align_sys,
            target.obs_lat.unwrap_or(0.0),
            target_last,
            target.compute_diurab(),
            result.obs_lat.unwrap_or(0.0),
            result_last,
            result.compute_diurab(),
        )?;

        // Replace the system-conversion chain by a UnitMap when its
        // effect everywhere on the sphere is below the frames' SkyTol
        // (skyframe.c:6546 TestSlaUnit).
        if crate::frames::sky_convert::is_effectively_unit(
            map.as_ref(),
            target.sky_tol().min(result.sky_tol()),
        ) {
            map = Box::new(UnitMap::new(2));
        }

        // Prepend source offset removal (offset→standard).
        if let Some(off_map) = make_offset_mapping(
            target.sky_ref,
            target.sky_ref_p,
            target.sky_ref_is,
            target.sky_ref_set[0] || target.sky_ref_set[1],
        )? {
            map = Box::new(CmpMap::new(off_map, map, Composition::Series)?);
        }

        // Append target offset application (standard→offset = inverse).
        if let Some(mut off_map) = make_offset_mapping(
            result.sky_ref,
            result.sky_ref_p,
            result.sky_ref_is,
            result.sky_ref_set[0] || result.sky_ref_set[1],
        )? {
            off_map.invert();
            map = Box::new(CmpMap::new(map, off_map, Composition::Series)?);
        }

        // Coordinate-order swaps (skyframe.c:10082-10116). The
        // Mapping built above operates in (longitude, latitude) order
        // — if either the target or the result presents axes in a
        // different external order, prepend / append a 2×2 PermMap.
        // target_swap: axis-0 of the target points at latitude.
        let target_swap = target.perm[0] != 0;
        // result_swap: was the target presented in a different order
        // than the result requires? When target_axes selects axis 1
        // first, the resulting coordinate pair flows through
        // longitude, latitude; result_swap mirrors the C expression
        // `target_swap != (target_axes[0] != 0)`.
        let result_swap = target_swap != (target_axes[0] != 0);
        if target_swap || result_swap {
            let perm_inperm: [i64; 2] = [2, 1];
            let perm_outperm: [i64; 2] = [2, 1];
            let permmap = crate::mappings::PermMap::new(&perm_inperm, &perm_outperm, &[])?;
            if target_swap {
                let pm = Box::new(permmap.clone());
                map = Box::new(CmpMap::new(pm, map, Composition::Series)?);
            }
            if result_swap {
                let pm = Box::new(permmap);
                map = Box::new(CmpMap::new(map, pm, Composition::Series)?);
            }
        }

        let result_box: DynFrame = Box::new(result);
        return Ok(Some((map, result_box)));
    }

    // Result is not a SkyFrame (skyframe.c:10128-10199). Build a
    // BasicFrame snapshot whose per-axis attributes follow the picked
    // SkyFrame axes, then defer to the default Frame-level
    // SubFrame to construct the Mapping.
    super::find_frame::default_sub_frame(target, template, target_axes, template_axes)
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

        // Axis labels. Per C `Frame::Dump` (frame.c:14345-14358) the
        // aggregate `Lbl{n}` is always written unset (commented) with
        // `helpful = 1`, but its *value* comes from `astGetLabel`, which
        // returns the explicit per-axis Label override when one is set.
        // The card stays commented; the override round-trips through the
        // Axis sub-object's own active `Label` card (emitted below).
        //
        // Per-axis attributes are written in INTERNAL (unpermuted) axis
        // order — the permutation is recorded separately as `Prm{n}` and
        // replayed on read. SkyFrame internal axis 0 is always longitude
        // and axis 1 always latitude, so we read the overrides by physical
        // axis directly.
        let (lon_label, lat_label) = self.default_labels();
        let lbl1 = self.label_overrides[0].clone().unwrap_or(lon_label);
        let lbl2 = self.label_overrides[1].clone().unwrap_or(lat_label);
        node.push_unset(
            "Lbl1",
            NodeValue::String(lbl1),
            Some("Label for axis 1".to_string()),
        );
        node.push_unset(
            "Lbl2",
            NodeValue::String(lbl2),
            Some("Label for axis 2".to_string()),
        );

        // System. Mirrors C `Frame::Dump` (frame.c:14372-14400), which
        // writes the System line via `astWriteString(..., set, ...)` where
        // `set = astTestSystem(this)` — i.e. an *explicitly set* System is
        // always written as an active card (regardless of its value),
        // while an unset System is emitted as a commented default (and
        // suppressed entirely when this Frame is a CmpFrame component).
        // Gate on `system_set`, not on the value being ICRS.
        if self.system_set {
            node.push_string(
                "System",
                self.system.dump_str(),
                Some("Coordinate system type".to_string()),
            );
        } else {
            node.push_unset(
                "System",
                NodeValue::String(self.system.dump_str().to_string()),
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
        // Internal-order, like Lbl above: Uni1 = lon_unit, Uni2 = lat_unit.
        let (lon_unit, lat_unit) = self.default_units();
        node.push_unset(
            "Uni1",
            NodeValue::String(lon_unit.to_string()),
            Some("Units for axis 1".to_string()),
        );
        node.push_unset(
            "Uni2",
            NodeValue::String(lat_unit.to_string()),
            Some("Units for axis 2".to_string()),
        );

        // Dir1: longitude axis plots in reverse direction. Internal axis
        // 0 is longitude, so this is always Dir1 (unset default).
        node.push_unset(
            "Dir1",
            NodeValue::Int(0),
            Some("Plot axis 1 in reverse direction".to_string()),
        );

        // Bot{N}/Top{N}: per-axis legal range. The latitude axis has
        // [-π/2, π/2]; the longitude axis has [-DBL_MAX, DBL_MAX].
        // Emitted in INTERNAL axis order (like Lbl/Uni): internal axis 1
        // is always latitude, so these are always Bot2/Top2.
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

        // ActUnt: Frame's Dump gates the ActUnt write on astTestActiveUnit
        // (frame.c:14616). SkyFrame overrides TestActiveUnit to always
        // return 0 (skyframe.c:10433), so C never emits an ActUnt token
        // for a SkyFrame. Gating on test_active_unit() (always false for
        // SkyFrame) reproduces that "never emit" behaviour by construction,
        // even when act_unt was populated by from_node reading a token.
        if self.test_active_unit() {
            let v = self.act_unt.unwrap_or(false);
            node.push_int(
                "ActUnt",
                if v { 1 } else { 0 },
                Some(if v {
                    "Unit strings affects alignment".to_string()
                } else {
                    "Unit strings do not affect alignment".to_string()
                }),
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
                Some("Observers geodetic altitude (metres)".to_string()),
            );
        }
        if let Some(dtai) = self.dtai {
            node.push_double("Dtai", dtai, Some("TAI-UTC in seconds".to_string()));
        }
        if let Some(dut1) = self.dut1 {
            node.push_double("Dut1", dut1, Some("UT1-UTC in seconds".to_string()));
        }

        // Axis permutation array `Prm{n}`: written just before the Axis
        // Objects, mirroring C `Frame::Dump` (frame.c:14623-14641). For
        // each external axis `n` the value is the 1-based internal axis
        // it maps to (`perm[n-1] + 1`). C writes it via
        // `astWriteInt(..., set, helpful=0, ...)` where `set =
        // (perm[axis] != axis)`; an unset, non-helpful attribute is
        // suppressed entirely at the default Full=0 verbosity. So we
        // emit `Prm{n}` only when the axis is actually permuted.
        for (ext, &internal) in self.perm.iter().enumerate() {
            if internal != ext {
                let ival = (internal + 1) as i64;
                node.push_int(
                    &format!("Prm{}", ext + 1),
                    ival,
                    Some(format!(
                        "Axis {} permuted to use internal axis {}",
                        ext + 1,
                        ival
                    )),
                );
            }
        }

        // Per-axis SkyAxis children. An explicit per-axis Label / Symbol
        // override is stored on the Frame but, like C, round-trips as an
        // active `Label` / `Symbol` card *inside* the Axis sub-object
        // (the Frame-level `Lbl{n}` aggregate stays commented). C writes
        // these via the Axis class Dump (axis.c); when any active card is
        // present the Axis emits the `IsA Axis` marker. Axes are in
        // internal order (0 = lon, 1 = lat).
        for (i, axis) in self.axes.iter().enumerate() {
            let mut axis_node = Serializable::to_node(axis)?;
            if let Some(label) = &self.label_overrides[i] {
                axis_node.push_string("Label", label, Some("Axis Label".to_string()));
            }
            if let Some(symbol) = &self.symbol_overrides[i] {
                axis_node.push_string("Symbol", symbol, Some("Axis symbol".to_string()));
            }
            // Emit the `IsA Axis` marker when the SkyAxis carries active
            // (non-comment) cards, matching C's Axis Dump.
            if axis_node
                .items
                .iter()
                .any(|it| !matches!(it.value, NodeValue::Unset(_)) && it.key != "IsA")
            {
                axis_node.push_isa("Axis", Some("Coordinate axis".to_string()));
            }
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
                SkyRefIs::Origin => "Rotated to put origin at ref. pos.",
                SkyRefIs::Pole => "Rotated to put pole at ref. pos.",
            };
            node.push_string("SRefIs", srefis_str, Some(comment.to_string()));
        }

        // SRef1, SRef2 (sky_ref longitude/latitude).
        //
        // Mirrors C SkyFrame Dump (skyframe.c:12040-12051): each axis is
        // written when its SkyRef value has been explicitly set
        // (`TestSkyRef`), regardless of magnitude — so a reference position
        // of exactly (0, 0) still emits. The comment is
        // `Ref. pos. <Symbol> <Format>`, taking the value in the SkyFrame's
        // external axis order (`SRef<axis_p+1>`) with the symbol/format from
        // the corresponding axis.
        if self.test_sky_ref(0)
            || self.test_sky_ref(1)
            || self.sky_ref[0] != 0.0
            || self.sky_ref[1] != 0.0
            || self.sky_ref_is != SkyRefIs::Ignored
        {
            // Mirrors C `SkyFrame::Dump` (skyframe.c:12040-12051): the
            // SRef keyword is written in INTERNAL axis order, looping
            // `axis_p` over internal axes and resolving the external axis
            // via the inverse permutation — `SRef{axis_p+1}` carries the
            // value, symbol and format of external axis `invperm[axis_p]`.
            // `sky_ref` is stored in internal order, so `sky_ref[axis_p]`
            // is already the right value; only the symbol/format must use
            // the resolved external axis.
            let invperm = [self.lon_axis(), self.lat_axis()]; // invperm[internal] = external (0-based)
            for (axis_p, &ext0) in invperm.iter().enumerate() {
                let ext_axis = ext0 + 1; // 1-based external axis
                let dval = self.sky_ref[axis_p];
                let symbol = self.axis_symbol(ext_axis).unwrap_or_default();
                let fmt = self.format(ext_axis, dval).unwrap_or_default();
                node.push_double(
                    &format!("SRef{}", axis_p + 1),
                    dval,
                    Some(format!("Ref. pos. {symbol} {fmt}")),
                );
            }
        }

        // SRefP1, SRefP2 (sky_ref_p longitude/latitude). Mirrors C
        // (skyframe.c:12059-12068): looped over internal axes with `helpful=0`
        // (so only an explicitly-set value is written), with the comment
        // `Ref. north <symbol> <fmt>` resolved on the external axis.
        let invperm = [self.lon_axis(), self.lat_axis()];
        for (axis_p, &ext0) in invperm.iter().enumerate() {
            if !self.sky_ref_p_set[axis_p] {
                continue;
            }
            let ext_axis = ext0 + 1;
            let dval = self.sky_ref_p[axis_p];
            let symbol = self.axis_symbol(ext_axis).unwrap_or_default();
            let fmt = self.format(ext_axis, dval).unwrap_or_default();
            node.push_double(
                &format!("SRefP{}", axis_p + 1),
                dval,
                Some(format!("Ref. north {symbol} {fmt}")),
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
        // If the System node was explicitly present, mark it as set.
        if node.get("System").is_some() {
            sky.system_set = true;
        }

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

        if node.get("NegLon").is_some() {
            sky.neglon = node.get_int("NegLon", 0) != 0;
            sky.neglon_set = true;
        }

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
            sky.sky_ref_set[0] = true;
        }
        if node.get("SRef2").is_some() {
            sky.sky_ref[1] = node.get_double("SRef2", 0.0);
            sky.sky_ref_set[1] = true;
        }

        // SRefP1, SRefP2.
        if node.get("SRefP1").is_some() {
            sky.sky_ref_p[0] = node.get_double("SRefP1", 0.0);
            sky.sky_ref_p_set[0] = true;
        }
        if node.get("SRefP2").is_some() {
            sky.sky_ref_p[1] = node.get_double("SRefP2", 0.0);
            sky.sky_ref_p_set[1] = true;
        }

        if node.get("AlOff").is_some() {
            sky.align_offset = node.get_int("AlOff", 0) != 0;
            sky.align_offset_set = true;
        }

        // AlSys (alignment system).
        let alsys_str = node.get_string("AlSys", "");
        if !alsys_str.is_empty() {
            sky.align_system = SkySystem::parse(&alsys_str);
        }

        // ActUnt: stored only when the token is present, with its value
        // (frame.c:14614). Absent => unset (default false).
        if node.get("ActUnt").is_some() {
            sky.act_unt = Some(node.get_int("ActUnt", 0) != 0);
        }

        // Ident (Object-level identification string).
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            sky.ident = Some(ident);
        }

        // Read SkyAxis children. Axes are stored in internal (physical)
        // order (0 = lon, 1 = lat). An explicit per-axis Label / Symbol is
        // carried as an active card inside the Axis sub-object (mirroring
        // C's Axis Dump); restore it into the Frame-level override arrays.
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
                    let label = axis_node.get_string("Label", "");
                    if !label.is_empty() {
                        sky.label_overrides[i] = Some(label);
                    }
                    let symbol = axis_node.get_string("Symbol", "");
                    if !symbol.is_empty() {
                        sky.symbol_overrides[i] = Some(symbol);
                    }
                }
            }
        }

        // Axis permutation `Prm{n}` (1-based internal axis for each
        // external axis). Mirrors C `Frame::Dump`/load. Apply last so it
        // is not clobbered by earlier per-axis state, and only when it
        // differs from the identity. `perm_axes` takes a 1-based
        // external→internal mapping, which is exactly the `Prm{n}` values.
        let prm: Vec<usize> = (1..=2)
            .map(|n| node.get_int(&format!("Prm{n}"), n as i64) as usize)
            .collect();
        if prm != [1, 2] {
            Frame::perm_axes(&mut sky, &prm)?;
        }

        Ok(sky)
    }
}

/// Calculate the reference vectors used by `Offset2`. Mirrors C `Shcal`
/// (skyframe.c:9766): returns `(r0, r3)` where `r0` is the 3-vector of the
/// sky position `(a0, b0)` and `r3` is the 3-vector 90 degrees away along
/// the great circle passing through `r0` at position angle `angle`
/// (measured from north to east). The XY plane is the equator, Z is north,
/// X towards longitude 0, Y towards longitude 90 degrees.
fn shcal(a0: f64, b0: f64, angle: f64) -> ([f64; 3], [f64; 3]) {
    let sina0 = a0.sin();
    let cosa0 = a0.cos();
    let sinb0 = b0.sin();
    let cosb0 = b0.cos();
    let sinpa = angle.sin();
    let cospa = angle.cos();

    // R0: the given point.
    let r0 = [cosb0 * cosa0, cosb0 * sina0, sinb0];

    // R1: the point in the meridian of R0, 90 deg further north.
    let r1 = [-sinb0 * cosa0, -sinb0 * sina0, cosb0];

    // R2: the point on the equator, 90 deg west of R0.
    let r2 = [-sina0, cosa0, 0.0];

    // R3: the point 90 deg away from R0 along the required great circle.
    let r3 = [
        cospa * r1[0] + sinpa * r2[0],
        cospa * r1[1] + sinpa * r2[1],
        cospa * r1[2] + sinpa * r2[2],
    ];

    (r0, r3)
}

/// Use the vectors from `shcal` to find the sky position offset by `dist`
/// along the great circle. Mirrors C `Shapp` (skyframe.c:9680). Returns
/// `[lon, lat]` in radians; a point at a pole is assigned the reference
/// longitude `a0`.
fn shapp(dist: f64, r0: &[f64; 3], r3: &[f64; 3], a0: f64) -> [f64; 2] {
    let sindst = dist.sin();
    let cosdst = dist.cos();

    // R4: the required position, a linear sum of R0 and R3.
    let r4 = [
        cosdst * r0[0] + sindst * r3[0],
        cosdst * r0[1] + sindst * r3[1],
        cosdst * r0[2] + sindst * r3[2],
    ];

    let lon = if r4[0] != 0.0 || r4[1] != 0.0 {
        r4[1].atan2(r4[0])
    } else {
        a0
    };

    let z = r4[2].clamp(-1.0, 1.0);
    let lat = z.asin();

    [lon, lat]
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
        assert_close(sky.sky_tol(), 0.001, TOL);
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
        sky.set_epoch_mjd(53000.0);
        assert_close(sky.epoch_mjd(), 53000.0, TOL);
    }

    #[test]
    fn set_equinox() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_equinox_mjd(53000.0);
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
        // PI/4 = 45 degrees. C `astFormat` emits no leading '+' for
        // positive sexagesimal values.
        let s = sky.format(2, PI / 4.0).unwrap();
        assert_eq!(s, "45:00:00");
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
    // Frame trait: resolve_points (spherical / great-circle geometry)
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_points_along_equator() {
        // Basis vector runs east along the equator from (0,0) to (0.1,0). A
        // point just north of it resolves to a parallel component equal to its
        // longitude arc and a perpendicular component equal to its latitude
        // arc. Per skyframe.c:8457, a positive perpendicular means rotation
        // from the basis vector to the point matches the lon->lat sense, so a
        // northward point is positive.
        let sky = SkyFrame::new(SkySystem::Fk5);
        let pts = crate::coords::PointSet::from_rows([[0.05, 0.02]].iter().map(|p| &p[..]));
        let res = sky.resolve_points(&[0.0, 0.0], &[0.1, 0.0], &pts).unwrap();
        let comp = res.point(0);
        assert_close(comp[0], 0.05, 1.0e-6);
        assert_close(comp[1], 0.02, 1.0e-6);
    }

    #[test]
    fn resolve_points_differs_from_planar() {
        // At high declination the great-circle perpendicular component is
        // markedly smaller than the naive planar delta-latitude, because a
        // parallel of latitude is a small circle. This divergence is what the
        // planar base-Frame implementation gets wrong for sky coordinates.
        let sky = SkyFrame::new(SkySystem::Fk5);
        let lat = 1.2_f64; // ~68.75 deg
        let pts =
            crate::coords::PointSet::from_rows([[0.001, lat + 1.0e-5]].iter().map(|p| &p[..]));
        let res = sky
            .resolve_points(&[0.0, lat], &[0.002, lat], &pts)
            .unwrap();
        let comp = res.point(0);
        // The spherical perpendicular differs from the planar 1e-5 by more
        // than the sagitta of the small-circle chord over this longitude span.
        assert!(comp[1].abs() < 1.0e-5, "prp={:.3e}", comp[1]);
        assert!(comp[1].is_finite());
    }

    #[test]
    fn resolve_points_bad_input() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let pts = crate::coords::PointSet::from_rows([[BAD_VALUE, 0.02]].iter().map(|p| &p[..]));
        let res = sky.resolve_points(&[0.0, 0.0], &[0.1, 0.0], &pts).unwrap();
        let comp = res.point(0);
        assert_eq!(comp[0], BAD_VALUE);
        assert_eq!(comp[1], BAD_VALUE);
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

    #[test]
    fn angle_is_signed_and_matches_oracle() {
        // Oracle values from the reference C library (astAngle on a
        // FK5 SkyFrame): the angle at b is signed, measured from the
        // direction of north (positive towards east).
        let sky = SkyFrame::new(SkySystem::Fk5);
        let north = [0.0, 0.1];
        let b = [0.0, 0.0];
        let east = [0.1, 0.0];
        let west = [-0.1, 0.0];
        let half_pi = std::f64::consts::FRAC_PI_2;
        assert!((sky.angle(&north, &b, &east).unwrap() - half_pi).abs() < 1e-9);
        assert!((sky.angle(&north, &b, &west).unwrap() + half_pi).abs() < 1e-9);
        assert!((sky.angle(&east, &b, &north).unwrap() + half_pi).abs() < 1e-9);

        // General-position oracle value.
        let p = [0.3, 0.4];
        let q = [0.25, -0.2];
        let t = [0.6, 0.1];
        assert!(
            (sky.angle(&p, &q, &t).unwrap() - 0.796037127730).abs() < 1e-9,
            "got {}",
            sky.angle(&p, &q, &t).unwrap()
        );
    }

    #[test]
    fn angle_coincident_points_return_bad() {
        // C returns AST__BAD when a or c coincides with the vertex b.
        let sky = SkyFrame::new(SkySystem::Fk5);
        let b = [0.0, 0.0];
        let east = [0.1, 0.0];
        assert_eq!(sky.angle(&b, &b, &east).unwrap(), BAD_VALUE);
        assert_eq!(sky.angle(&east, &b, &b).unwrap(), BAD_VALUE);
    }

    #[test]
    fn angle_respects_axis_permutation() {
        // With the axes swapped (axis 1 = latitude) C applies a
        // pi/2 - result correction (skyframe.c:1205). Oracle values
        // from the reference library via astPermAxes.
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        Frame::perm_axes(&mut sky, &[2, 1]).unwrap();
        // Same geometry as the unpermuted N/b/E case, coords (lat, lon).
        let north = [0.1, 0.0];
        let b = [0.0, 0.0];
        let east = [0.0, 0.1];
        assert!(sky.angle(&north, &b, &east).unwrap().abs() < 1e-9);

        let p = [0.4, 0.3];
        let q = [-0.2, 0.25];
        let t = [0.1, 0.6];
        assert!(
            (sky.angle(&p, &q, &t).unwrap() - 0.774759199065).abs() < 1e-9,
            "got {}",
            sky.angle(&p, &q, &t).unwrap()
        );
    }

    // -----------------------------------------------------------------------
    // SkyTol
    // -----------------------------------------------------------------------

    #[test]
    fn sky_tol_default_is_one_milliarcsec() {
        // C default: 0.001 arcsec (skyframe.c:11549).
        let sky = SkyFrame::new(SkySystem::Fk5);
        assert!((sky.sky_tol() - 0.001).abs() < 1e-15);
    }

    #[test]
    fn convert_collapses_chain_within_sky_tol() {
        // Oracle: astConvert(ICRS, FK5 J2000) with SkyTol=100 on both
        // frames simplifies to a UnitMap (the ~23 mas frame bias is
        // within tolerance); at the default 1 mas it does not.
        let mut icrs = SkyFrame::new(SkySystem::Icrs);
        icrs.set_epoch_year(2000.0);
        icrs.set_sky_tol(100.0);
        let mut fk5 = SkyFrame::new(SkySystem::Fk5);
        fk5.set_epoch_year(2000.0);
        fk5.set_sky_tol(100.0);

        let fs = Frame::convert_to(&icrs, &fk5, &[]).unwrap().unwrap();
        let map = fs
            .mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .simplify()
            .unwrap();
        assert!(
            map.as_any().is::<crate::mappings::UnitMap>(),
            "expected UnitMap within SkyTol, got {map:?}"
        );

        let mut icrs2 = SkyFrame::new(SkySystem::Icrs);
        icrs2.set_epoch_year(2000.0);
        let mut fk52 = SkyFrame::new(SkySystem::Fk5);
        fk52.set_epoch_year(2000.0);
        let fs2 = Frame::convert_to(&icrs2, &fk52, &[]).unwrap().unwrap();
        let map2 = fs2
            .mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .simplify()
            .unwrap();
        assert!(
            !map2.as_any().is::<crate::mappings::UnitMap>(),
            "the 23 mas ICRS-FK5 bias exceeds the default 1 mas SkyTol"
        );
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

        let fs = fk5.convert_to(&gal, &[]).unwrap();
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
    fn convert_skyframe_to_blank_domain_frame_is_unitmap() {
        // `astConvert(SkyFrame, Frame[Domain=""])` succeeds with a
        // UnitMap: a blank-domain plain Frame matches any Frame, and the
        // generic ConvertX driver casts the SkyFrame down to a plain
        // Frame for the alignment, leaving the axes unchanged. Confirmed
        // against the C oracle (ast_tester/testconvert.c errors 4/8 fire
        // if no FrameSet, 3/7 if not a UnitMap).
        use crate::frames::{BasicFrameBuilder, Frame};
        let sky = SkyFrame::new(SkySystem::Fk5);
        let basic = BasicFrameBuilder::new(2).build().unwrap();

        let fs = sky
            .convert_to(&basic, &[])
            .unwrap()
            .expect("SkyFrame -> blank-domain Frame converts");
        let map = fs
            .mapping(super::FrameRef::Base, super::FrameRef::Current)
            .unwrap();
        assert!(
            map.as_any().is::<UnitMap>(),
            "SkyFrame -> blank-domain Frame is a UnitMap"
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

        let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

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

        let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

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

        let fs_fwd = src.convert_to(&dst, &[]).unwrap().unwrap();

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

    #[test]
    fn skyframe_set_ident_round_trip() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        assert_eq!(sky.ident(), "");
        sky.set_ident("A");
        assert_eq!(sky.ident(), "A");
        sky.set_ident("");
        assert_eq!(sky.ident(), "");
    }

    // -----------------------------------------------------------------------
    // Match / SubFrame / Overlay (skyframe.c:6604 / 9869 / 8010)
    // -----------------------------------------------------------------------

    #[test]
    fn overlay_template_copies_set_system() {
        // Template has System explicitly set to Galactic; result starts
        // life as FK5. After overlay the result must be Galactic and
        // its Title cleared (since the system changed and the template
        // had System set).
        let mut template = SkyFrame::new(SkySystem::Galactic);
        // SkyFrame::new for Galactic auto-sets system_set = true.
        assert!(template.test_system());
        let target = SkyFrame::new(SkySystem::Fk5);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0, 1]).unwrap();
        let result_sky = result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(result_sky.system(), SkySystem::Galactic);
        // Set a known title on the template; after overlay it should
        // copy through.
        template.set_title("custom");
        let mut r2: DynFrame = Box::new(SkyFrame::new(SkySystem::Fk5));
        r2.overlay_template(&template, &[0, 1]).unwrap();
        let r2_sky = r2.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(r2_sky.title(), "custom");
    }

    #[test]
    fn overlay_template_skips_unset_attrs() {
        // Template default-constructs as ICRS without any other
        // attributes set. Overlaying onto a Galactic target must NOT
        // overwrite the target's system (template ICRS isn't marked as
        // explicitly set by `SkyFrame::new(Icrs)`).
        let template = SkyFrame::new(SkySystem::Icrs);
        assert!(!template.test_system());
        let target = SkyFrame::new(SkySystem::Galactic);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0, 1]).unwrap();
        let r = result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(r.system(), SkySystem::Galactic);
    }

    #[test]
    fn sub_frame_skyframe_to_skyframe() {
        // FK5 target → Galactic template via SubFrame must produce a
        // 2-axis Mapping and a Galactic result Frame.
        let target = SkyFrame::new(SkySystem::Fk5);
        let template = SkyFrame::new(SkySystem::Galactic);
        let template_axes = [0i32, 1];
        let target_axes = [0i32, 1];
        let (map, result) = target
            .sub_frame(Some(&template), 2, &target_axes, &template_axes)
            .unwrap()
            .expect("should match");
        assert_eq!(map.nin(), 2);
        assert_eq!(map.nout(), 2);
        let result_sky = result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(result_sky.system(), SkySystem::Galactic);
    }

    #[test]
    fn match_template_skyframe_self() {
        let target = SkyFrame::new(SkySystem::Fk5);
        let template = SkyFrame::new(SkySystem::Galactic);
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("should match");
        // Both axes accounted for.
        assert_eq!(m.template_axes.len(), 2);
        assert_eq!(m.target_axes.len(), 2);
        // Result must be a SkyFrame in Galactic.
        let r = m.result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(r.system(), SkySystem::Galactic);
    }

    #[test]
    fn match_template_skyframe_swap_blocked_when_permute_false() {
        // Target has its axes swapped; with Permute explicitly cleared
        // to false the swap-detection logic (skyframe.c:6826
        // `match = !swap || astGetPermute(template)`) refuses the match.
        let mut target = SkyFrame::new(SkySystem::Fk5);
        target.perm_axes(&[2, 1]).unwrap();
        let mut template = SkyFrame::new(SkySystem::Galactic);
        template.set_permute(false);
        assert!(!template.permute());
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_skyframe_permute_defaults_true() {
        // Permute defaults to 1 (true) — SkyFrame inherits the parent
        // Frame default (frame.c:13459-13460); it does not override the
        // getter. An axis-swapped target therefore matches by default.
        let template = SkyFrame::new(SkySystem::Galactic);
        assert!(template.permute());
        let mut target = SkyFrame::new(SkySystem::Fk5);
        target.perm_axes(&[2, 1]).unwrap();
        assert!(
            template.match_template(&target, false).unwrap().is_some(),
            "axis-swapped SkyFrame matches with the default Permute=true"
        );
    }

    #[test]
    fn match_template_skyframe_swap_allowed_when_permute_true() {
        let mut target = SkyFrame::new(SkySystem::Fk5);
        target.perm_axes(&[2, 1]).unwrap();
        let mut template = SkyFrame::new(SkySystem::Galactic);
        template.set_permute(true);
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("should match with permute on");
        assert_eq!(m.template_axes.len(), 2);
    }

    #[test]
    fn pick_axes_snapshot_skyframe_full() {
        // Picking both axes in canonical order yields a SkyFrame copy.
        let target = SkyFrame::new(SkySystem::Galactic);
        let snap = target.pick_axes_snapshot(&[0, 1]).unwrap();
        let s = snap.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(s.system(), SkySystem::Galactic);
    }

    #[test]
    fn pick_axes_snapshot_skyframe_partial() {
        // Picking just one axis yields a 1-axis BasicFrame (no longer
        // a SkyFrame).
        let target = SkyFrame::new(SkySystem::Galactic);
        let snap = target.pick_axes_snapshot(&[1]).unwrap();
        assert_eq!(snap.naxes(), 1);
        assert!(snap.as_any().downcast_ref::<SkyFrame>().is_none());
    }

    #[test]
    fn cast_skyframe_to_basic_frame() {
        let sf = SkyFrame::new(crate::frames::SkySystem::Fk5);
        let bf = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let casted = sf.cast(&bf).unwrap();
        assert!(casted.is_some());
        let casted = casted.unwrap();
        assert!(
            casted
                .as_any()
                .downcast_ref::<crate::frames::BasicFrame>()
                .is_some()
        );
        assert_eq!(casted.naxes(), 2);
    }

    #[test]
    fn cast_skyframe_preserves_per_axis_metadata() {
        // C astCast copies each source Axis into the basic Frame
        // (cmpframe.c:1335-1338) so per-axis Label/Unit/Symbol carry
        // through. Set explicit Label and Symbol overrides on a
        // SkyFrame, cast it down to a plain Frame, and confirm both the
        // overrides and the system-derived Unit survive on the result.
        let mut sf = SkyFrame::new(crate::frames::SkySystem::Galactic);
        sf.set_axis_label(1, "MyLon").unwrap();
        sf.set_axis_label(2, "MyLat").unwrap();
        sf.set_axis_symbol(1, "ML").unwrap();
        sf.set_axis_symbol(2, "MB").unwrap();

        let template = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let casted = sf.cast(&template).unwrap().unwrap();
        let out = casted
            .as_any()
            .downcast_ref::<crate::frames::BasicFrame>()
            .expect("cast result should be a BasicFrame");

        assert_eq!(out.naxes(), 2);
        // Explicit per-axis Label overrides survive.
        assert_eq!(out.label(1).unwrap(), "MyLon");
        assert_eq!(out.label(2).unwrap(), "MyLat");
        // Explicit per-axis Symbol overrides survive.
        assert_eq!(out.symbol(1).unwrap(), "ML");
        assert_eq!(out.symbol(2).unwrap(), "MB");
        // System-derived per-axis Unit survives (galactic -> degrees).
        assert_eq!(out.unit(1).unwrap(), "degrees");
        assert_eq!(out.unit(2).unwrap(), "degrees");
    }
}
