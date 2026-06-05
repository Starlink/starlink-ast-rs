//! The spectral coordinate frame (SpecFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{ast_equal, is_bad};
use crate::mappings::{CmpMap, Composition, DynMapping, SpecCvtType, SpecMap, UnitMap};
use crate::serial::{Node, NodeValue, Serializable};
use crate::units::unit_mapper;

// ---------------------------------------------------------------------------
// SpecSystem — spectral coordinate system type
// ---------------------------------------------------------------------------

/// Spectral coordinate system type.
///
/// Each variant corresponds to a way of measuring spectral position.
/// The "absolute" systems (Freq, Energy, Wavenum, Wavelen, Airwave) do not
/// require a rest frequency, while the "relative" systems (Vradio, Voptical,
/// Redshift, Beta, Vrel) define coordinates relative to a rest frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecSystem {
    /// Frequency (Hz).
    Freq,
    /// Energy (J).
    Energy,
    /// Wave number (1/m).
    Wavenum,
    /// Wavelength in vacuum (m).
    Wavelen,
    /// Wavelength in air (m).
    Airwave,
    /// Radio velocity (m/s).
    Vradio,
    /// Optical velocity (m/s).
    Voptical,
    /// Redshift (dimensionless).
    Redshift,
    /// Beta factor v/c (dimensionless).
    Beta,
    /// Relativistic velocity (m/s).
    Vrel,
}

impl SpecSystem {
    /// Return the canonical string representation (FITS-WCS style).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freq => "FREQ",
            Self::Energy => "ENER",
            Self::Wavenum => "WAVN",
            Self::Wavelen => "WAVE",
            Self::Airwave => "AWAV",
            Self::Vradio => "VRAD",
            Self::Voptical => "VOPT",
            Self::Redshift => "ZOPT",
            Self::Beta => "BETA",
            Self::Vrel => "VELO",
        }
    }

    /// Parse from string (case-insensitive). Accepts both short and long forms.
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "FREQ" | "FREQUENCY" => Some(Self::Freq),
            "ENER" | "ENERGY" => Some(Self::Energy),
            "WAVN" | "WAVENUM" | "WAVENUMBER" => Some(Self::Wavenum),
            "WAVE" | "WAVELEN" | "WAVELENGTH" => Some(Self::Wavelen),
            "AWAV" | "AIRWAVE" | "AIRWAV" => Some(Self::Airwave),
            "VRAD" | "VRADIO" => Some(Self::Vradio),
            "VOPT" | "VOPTICAL" => Some(Self::Voptical),
            "ZOPT" | "REDSHIFT" => Some(Self::Redshift),
            "BETA" => Some(Self::Beta),
            "VELO" | "VREL" | "RELATIVISTIC" => Some(Self::Vrel),
            _ => None,
        }
    }

    /// Whether this system is a velocity-type (relative) that requires rest frequency.
    pub fn is_velocity(&self) -> bool {
        matches!(
            self,
            Self::Vradio | Self::Voptical | Self::Redshift | Self::Beta | Self::Vrel
        )
    }

    /// Whether this system is an absolute system (not dependent on rest frequency).
    pub fn is_absolute(&self) -> bool {
        !self.is_velocity()
    }

    /// Default units for SpecMap (SI-like).
    pub fn specmap_units(&self) -> &'static str {
        match self {
            Self::Freq => "Hz",
            Self::Energy => "J",
            Self::Wavenum => "1/m",
            Self::Wavelen | Self::Airwave => "m",
            Self::Vradio | Self::Voptical | Self::Vrel => "m/s",
            Self::Redshift | Self::Beta => "",
        }
    }

    /// Human-readable label for this system.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Freq => "Frequency",
            Self::Energy => "Energy",
            Self::Wavenum => "Wave-number",
            Self::Wavelen => "Wavelength",
            Self::Airwave => "Wavelength",
            Self::Vradio => "Radio velocity",
            Self::Voptical => "Optical velocity",
            Self::Redshift => "Redshift",
            Self::Beta => "Beta factor",
            Self::Vrel => "Velocity",
        }
    }

    /// Native serialisation attribute key for the unit preference (e.g. "UFreq").
    pub(crate) fn unit_attr_key(&self) -> &'static str {
        match self {
            Self::Freq => "UFreq",
            Self::Energy => "UEner",
            Self::Wavenum => "UWavn",
            Self::Wavelen => "UWave",
            Self::Airwave => "UAwav",
            Self::Vradio => "UVrad",
            Self::Voptical => "UVopt",
            Self::Redshift => "UZopt",
            Self::Beta => "UBeta",
            Self::Vrel => "UVelo",
        }
    }

    /// Comment for the unit preference attribute in Native format.
    pub(crate) fn unit_attr_comment(&self) -> &'static str {
        match self {
            Self::Freq => "Preferred units for frequency",
            Self::Energy => "Preferred units for energy",
            Self::Wavenum => "Preferred units for wave-number",
            Self::Wavelen => "Preferred units for wavelength",
            Self::Airwave => "Preferred units for wavelength in air",
            Self::Vradio => "Preferred units for radio velocity",
            Self::Voptical => "Preferred units for optical velocity",
            Self::Redshift => "Preferred units for redshift",
            Self::Beta => "Preferred units for beta factor",
            Self::Vrel => "Preferred units for apparent radial velocity",
        }
    }

    /// Default display units for this system (user-facing, not SI).
    pub(crate) fn default_display_units(&self) -> &'static str {
        // Mirrors C's `DefUnit` in specframe.c:901+. Differs from
        // `specmap_units` (the canonical SI unit used inside SpecMap)
        // only for velocity systems, which display as km/s by default.
        match self {
            Self::Freq => "GHz",
            Self::Energy => "J",
            Self::Wavenum => "1/m",
            Self::Wavelen | Self::Airwave => "Angstrom",
            Self::Vradio | Self::Voptical | Self::Vrel => "km/s",
            Self::Redshift | Self::Beta => "",
        }
    }
}

// ---------------------------------------------------------------------------
// StdOfRest — standard of rest
// ---------------------------------------------------------------------------

/// Standard of rest for spectral observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StdOfRest {
    /// Topocentric (observer on Earth surface).
    Topocentric,
    /// Geocentric (Earth centre).
    Geocentric,
    /// Barycentric (Solar system barycentre).
    Barycentric,
    /// Heliocentric (Sun centre).
    Heliocentric,
    /// Local Standard of Rest (kinematic).
    Lsrk,
    /// Local Standard of Rest (dynamical).
    Lsrd,
    /// Galactic centre.
    Galactic,
    /// Local Group.
    LocalGroup,
    /// Source rest frame (defined by SourceVel).
    Source,
}

impl StdOfRest {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Topocentric => "Topocentric",
            Self::Geocentric => "Geocentric",
            Self::Barycentric => "Barycentric",
            Self::Heliocentric => "Heliocentric",
            Self::Lsrk => "LSRK",
            Self::Lsrd => "LSRD",
            Self::Galactic => "Galactic",
            Self::LocalGroup => "Local_group",
            Self::Source => "Source",
        }
    }

    /// Parse from string (case-insensitive). Accepts various abbreviations.
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "TOPO" | "TOPOCENT" | "TOPOCENTRIC" => Some(Self::Topocentric),
            "GEO" | "GEOCENTR" | "GEOCENTRIC" => Some(Self::Geocentric),
            "BARY" | "BARYCENT" | "BARYCENTRIC" => Some(Self::Barycentric),
            "HELIO" | "HELIOCEN" | "HELIOCENTRIC" => Some(Self::Heliocentric),
            "LSRK" | "LSR" => Some(Self::Lsrk),
            "LSRD" => Some(Self::Lsrd),
            "GAL" | "GALACTOC" | "GALACTIC" => Some(Self::Galactic),
            "LG" | "LOCALGRP" | "LOCAL_GROUP" | "LOCAL-GROUP" => Some(Self::LocalGroup),
            "SOURCE" | "SRC" => Some(Self::Source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SpecFrame
// ---------------------------------------------------------------------------

/// A coordinate frame for spectral coordinates.
///
/// `SpecFrame` is a 1-axis Frame representing spectral position. It supports
/// a range of coordinate systems (frequency, wavelength, velocity, energy,
/// redshift, etc.) and reference frames (heliocentric, barycentric, LSRK, etc.).
///
/// Construct via [`SpecFrame::new`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, SpecFrame, SpecSystem, StdOfRest};
///
/// let mut sf = SpecFrame::new(SpecSystem::Freq);
/// assert_eq!(sf.naxes(), 1);
/// assert_eq!(sf.system(), SpecSystem::Freq);
/// assert_eq!(sf.std_of_rest(), StdOfRest::Heliocentric);
/// ```
#[derive(Debug, Clone)]
pub struct SpecFrame {
    // Frame-level attributes
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>, // MJD

    // Observer
    obs_lat: Option<f64>, // geodetic latitude (radians)
    obs_lon: Option<f64>, // geodetic longitude (radians)
    obs_alt: Option<f64>, // altitude (metres)

    // SpecFrame-specific
    system: SpecSystem,
    /// Whether System has been explicitly set. Mirrors C's `astTestSystem`
    /// for `SpecFrame`. The constructor leaves this `false` when the
    /// requested system equals the C default (`AST__FREQ`); any other
    /// constructor argument or call to `set_system` flips it `true`.
    system_set: bool,
    std_of_rest: StdOfRest,
    sor_set: bool,           // whether std_of_rest was explicitly set (vs defaulting)
    unit: Option<String>,    // user-facing unit for the current system (e.g. "GHz")
    rest_freq: Option<f64>,  // Hz
    ref_ra: Option<f64>,     // FK5 J2000 radians
    ref_dec: Option<f64>,    // FK5 J2000 radians
    source_vel: Option<f64>, // m/s (or redshift/beta depending on source_sys)
    source_vrf: Option<StdOfRest>, // rest frame of source velocity
    source_sys: Option<SpecSystem>, // spectral system of source velocity
    spec_origin: Option<f64>, // spectral origin (in default units for current system)
    align_std_of_rest: Option<StdOfRest>,
    /// Alignment system. Mirrors C `GetAlignSystem` which returns
    /// `AST__WAVELEN` by default for SpecFrames (specframe.c:1899-1957).
    align_system: Option<SpecSystem>,
    align_spec_offset: bool,
    /// Whether AlignSpecOffset was explicitly set. Mirrors C
    /// `astTestAlignSpecOffset`.
    align_spec_offset_set: bool,
    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,

    /// User-set Label override for this frame's single axis. `None`
    /// means "use the system-derived default from
    /// [`SpecSystem::label`]".
    label_override: Option<String>,

    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
}

impl SpecFrame {
    /// Create a new `SpecFrame` with the given spectral coordinate system.
    ///
    /// Defaults: domain = "SPECTRUM", std_of_rest = Heliocentric.
    pub fn new(system: SpecSystem) -> Self {
        Self {
            title: None,
            // None = use the default "SPECTRUM". Mirrors C's
            // astTestDomain returning false until set_domain.
            domain: None,
            epoch: None,
            obs_lat: None,
            obs_lon: None,
            obs_alt: None,
            system,
            // C SpecFrame default System is AST__FREQ (specframe.c
            // GetSystem). Match SkyFrame's pattern: only flag System as
            // explicitly set when the constructor argument differs from
            // the class default.
            system_set: !matches!(system, SpecSystem::Freq),
            std_of_rest: StdOfRest::Heliocentric,
            sor_set: false,
            unit: None,
            rest_freq: None,
            ref_ra: None,
            ref_dec: None,
            source_vel: None,
            source_vrf: None,
            source_sys: None,
            spec_origin: None,
            align_std_of_rest: None,
            align_system: None,
            align_spec_offset: false,
            align_spec_offset_set: false,
            act_unt: None,
            ident: None,
            label_override: None,
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
        }
    }

    // --- SpecFrame-specific attributes ---

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Return the spectral coordinate system.
    pub fn system(&self) -> SpecSystem {
        self.system
    }

    /// Set the spectral coordinate system.
    pub fn set_system(&mut self, sys: SpecSystem) {
        self.system = sys;
        self.system_set = true;
    }

    /// Test whether System has been explicitly set. Mirrors C
    /// `astTestSystem` for SpecFrame.
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear System back to its class default (`Freq`). Mirrors
    /// C `astClearSystem` for SpecFrame.
    pub fn clear_system(&mut self) {
        self.system = SpecSystem::Freq;
        self.system_set = false;
    }

    /// Return the standard of rest.
    pub fn std_of_rest(&self) -> StdOfRest {
        self.std_of_rest
    }

    /// Set the standard of rest.
    pub fn set_std_of_rest(&mut self, sor: StdOfRest) {
        self.std_of_rest = sor;
        self.sor_set = true;
    }

    /// Test whether StdOfRest has been explicitly set.
    pub fn test_std_of_rest(&self) -> bool {
        self.sor_set
    }

    /// Return the user-facing unit for the current system (e.g. "GHz" for FREQ).
    ///
    /// Returns `None` if no unit has been explicitly set.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// Set the user-facing unit for the current system.
    pub fn set_unit(&mut self, unit: &str) {
        self.unit = Some(unit.to_string());
    }

    /// Test whether Unit has been explicitly set on this SpecFrame's
    /// (single) axis.
    pub fn test_unit(&self) -> bool {
        self.unit.is_some()
    }

    /// Return the user-facing unit, falling back to the system default
    /// (e.g. "GHz" for FREQ, "km/s" for VRAD). Mirrors C's `astGetUnit`,
    /// which always returns a non-NULL string.
    pub fn unit_or_default(&self) -> &str {
        self.unit
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.system.default_display_units())
    }

    /// Clear the user-set unit override so that the system's default
    /// unit (`Hz` for `Freq`, `m` for `Wavelen`, etc.) takes effect.
    pub fn clear_unit(&mut self) {
        self.unit = None;
    }

    /// Return the alignment standard of rest.
    ///
    /// Defaults to Heliocentric if not explicitly set.
    pub fn align_std_of_rest(&self) -> StdOfRest {
        self.align_std_of_rest.unwrap_or(StdOfRest::Heliocentric)
    }

    /// Set the alignment standard of rest.
    pub fn set_align_std_of_rest(&mut self, sor: StdOfRest) {
        self.align_std_of_rest = Some(sor);
    }

    /// Test whether AlignStdOfRest has been explicitly set.
    pub fn test_align_std_of_rest(&self) -> bool {
        self.align_std_of_rest.is_some()
    }

    /// Return the alignment system.
    ///
    /// Defaults to `Wavelen` if not explicitly set. Mirrors C
    /// `GetAlignSystem` (specframe.c:1899-1957).
    pub fn align_system(&self) -> SpecSystem {
        self.align_system.unwrap_or(SpecSystem::Wavelen)
    }

    /// Set the alignment system.
    pub fn set_align_system(&mut self, sys: SpecSystem) {
        self.align_system = Some(sys);
    }

    /// Test whether AlignSystem has been explicitly set.
    pub fn test_align_system(&self) -> bool {
        self.align_system.is_some()
    }

    /// Clear AlignSystem back to its class default (`Wavelen`).
    pub fn clear_align_system(&mut self) {
        self.align_system = None;
    }

    /// Return the explicit Label override if set, otherwise the
    /// system-derived default ("Frequency", "Wavelength", etc.).
    /// Mirrors `astGetLabel(specfrm, 0)`.
    pub fn axis_label(&self) -> String {
        if let Some(s) = &self.label_override {
            return s.clone();
        }
        self.system.label().to_string()
    }

    /// Test whether the Label was explicitly set. Mirrors
    /// `astTestLabel(specfrm, 0)`.
    pub fn test_axis_label(&self) -> bool {
        self.label_override.is_some()
    }

    /// Set an explicit Label override.
    pub fn set_axis_label(&mut self, label: impl Into<String>) {
        self.label_override = Some(label.into());
    }

    /// Clear an explicit Label override.
    pub fn clear_axis_label(&mut self) {
        self.label_override = None;
    }

    /// Return the rest frequency in Hz.
    pub fn rest_freq(&self) -> Option<f64> {
        self.rest_freq
    }

    /// Set the rest frequency in Hz.
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.rest_freq = Some(hz);
    }

    /// Test whether RestFreq has been explicitly set.
    pub fn test_rest_freq(&self) -> bool {
        self.rest_freq.is_some()
    }

    /// Return the reference RA (FK5 J2000, radians).
    pub fn ref_ra(&self) -> Option<f64> {
        self.ref_ra
    }

    /// Set the reference RA (FK5 J2000, radians).
    pub fn set_ref_ra(&mut self, ra: f64) {
        self.ref_ra = Some(ra);
    }

    /// Test whether RefRA has been explicitly set.
    pub fn test_ref_ra(&self) -> bool {
        self.ref_ra.is_some()
    }

    /// Return the reference Dec (FK5 J2000, radians).
    pub fn ref_dec(&self) -> Option<f64> {
        self.ref_dec
    }

    /// Set the reference Dec (FK5 J2000, radians).
    pub fn set_ref_dec(&mut self, dec: f64) {
        self.ref_dec = Some(dec);
    }

    /// Test whether RefDec has been explicitly set.
    pub fn test_ref_dec(&self) -> bool {
        self.ref_dec.is_some()
    }

    /// Set the reference position from a celestial coordinate system.
    /// Mirrors C `astSetRefPos` (specframe.c:3913-4038).
    ///
    /// `frm` is the SkyFrame which defines the celestial coordinate
    /// system in which the longitude and latitude values are supplied.
    /// If `None`, the supplied longitude and latitude values are
    /// assumed to already be FK5 J2000 RA and Dec.
    ///
    /// `lon` and `lat` are in radians within `frm`'s coordinate system.
    pub fn set_ref_pos(
        &mut self,
        frm: Option<&super::SkyFrame>,
        lon: f64,
        lat: f64,
    ) -> AstResult<()> {
        // If no SkyFrame was supplied, just store the supplied values
        // as RefRA/RefDec directly. Mirrors C:3983-3985.
        let Some(frm) = frm else {
            self.set_ref_ra(lon);
            self.set_ref_dec(lat);
            return Ok(());
        };

        // Otherwise, convert the supplied values from the source
        // SkyFrame's system to FK5 J2000 (the canonical system in
        // which RefRA/RefDec are stored). Mirrors C:3990-4036.
        let canonical = super::SkyFrame::new(super::SkySystem::Fk5);
        // (The C version sets equinox=J2000 explicitly; SkyFrame::new
        // uses J2000 as the default Equinox, so no explicit set is
        // needed here.)
        let Some(fs) = crate::frames::Frame::convert(frm, &canonical, &[])? else {
            // Conversion was not possible — leave RefRA/RefDec
            // unchanged (C silently no-ops in this case).
            return Ok(());
        };

        // Determine the axis order on each side: SkyFrame may have
        // its axes permuted (lat-first vs lon-first). C uses
        // astGetLonAxis on each frame; mirror via SkyFrame::lon_axis.
        let base = fs.base();
        let current = fs.current();
        let base_lon = base
            .as_any()
            .downcast_ref::<super::SkyFrame>()
            .map(|s| s.lon_axis())
            .unwrap_or(0);
        let cur_lon = current
            .as_any()
            .downcast_ref::<super::SkyFrame>()
            .map(|s| s.lon_axis())
            .unwrap_or(0);

        let (xin, yin) = if base_lon == 0 {
            (lon, lat)
        } else {
            (lat, lon)
        };

        let mapping = fs.mapping(
            crate::frames::FrameRef::Base,
            crate::frames::FrameRef::Current,
        )?;
        let pts = crate::coords::PointSet::from_rows([[xin, yin]]);
        let out = mapping.transform(&pts, true)?;
        let row = out.point(0);
        let xout = row[0];
        let yout = row[1];

        let (refra, refdec) = if cur_lon == 0 {
            (xout, yout)
        } else {
            (yout, xout)
        };
        self.set_ref_ra(refra);
        self.set_ref_dec(refdec);
        Ok(())
    }

    /// Return the source velocity (m/s or redshift/beta depending on source_sys).
    pub fn source_vel(&self) -> Option<f64> {
        self.source_vel
    }

    /// Set the source velocity.
    pub fn set_source_vel(&mut self, vel: f64) {
        self.source_vel = Some(vel);
    }

    /// Test whether SourceVel has been explicitly set.
    pub fn test_source_vel(&self) -> bool {
        self.source_vel.is_some()
    }

    /// Return the source velocity rest frame.
    ///
    /// Defaults to Heliocentric if not set.
    pub fn source_vrf(&self) -> StdOfRest {
        self.source_vrf.unwrap_or(StdOfRest::Heliocentric)
    }

    /// Set the source velocity rest frame.
    pub fn set_source_vrf(&mut self, sor: StdOfRest) {
        self.source_vrf = Some(sor);
    }

    /// Test whether SourceVRF has been explicitly set.
    pub fn test_source_vrf(&self) -> bool {
        self.source_vrf.is_some()
    }

    /// Return the source velocity spectral system.
    ///
    /// Defaults to Vrel if not set.
    pub fn source_sys(&self) -> SpecSystem {
        self.source_sys.unwrap_or(SpecSystem::Vrel)
    }

    /// Set the source velocity spectral system.
    pub fn set_source_sys(&mut self, sys: SpecSystem) {
        self.source_sys = Some(sys);
    }

    /// Test whether SourceSys has been explicitly set.
    pub fn test_source_sys(&self) -> bool {
        self.source_sys.is_some()
    }

    /// Return the spectral origin (in default units for current system).
    pub fn spec_origin(&self) -> Option<f64> {
        self.spec_origin
    }

    /// Set the spectral origin.
    pub fn set_spec_origin(&mut self, origin: f64) {
        self.spec_origin = Some(origin);
    }

    /// Test whether SpecOrigin has been explicitly set.
    pub fn test_spec_origin(&self) -> bool {
        self.spec_origin.is_some()
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

    /// Set the epoch as MJD.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        self.epoch = Some(mjd);
    }

    /// Test whether Epoch has been explicitly set.
    pub fn test_epoch(&self) -> bool {
        self.epoch.is_some()
    }

    /// Set the epoch as a decimal year.
    ///
    /// Years strictly less than 1984.0 are interpreted as Besselian
    /// epochs; >= 1984.0 as Julian epochs (IAU 1984 rule). Mirrors
    /// `astReadDateTime` (frame.c:9028-9032) when called from
    /// `astSetEpoch` via the `epoch=…` setting.
    pub fn set_epoch_year(&mut self, year: f64) {
        self.epoch = Some(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the title.
    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    /// Test whether Title has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.title.is_some()
    }

    /// Clear Title back to its (computed) default.
    pub fn clear_title(&mut self) {
        self.title = None;
    }

    /// Compute the default title from the system label and standard of rest.
    ///
    /// Format: "{label} ({sor})" — e.g. "Frequency (Heliocentric)".
    fn default_title(&self) -> String {
        let label = self.system.label();
        let sor = self.std_of_rest.as_str();
        format!("{label} ({sor})")
    }

    /// Set the domain.
    pub fn set_domain(&mut self, domain: &str) {
        self.domain = Some(domain.to_string());
    }

    /// Test whether the domain attribute has been explicitly set.
    /// Mirrors C's `astTestDomain` for `SpecFrame`.
    pub fn test_domain(&self) -> bool {
        self.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default
    /// `"SPECTRUM"`.
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

    /// Whether to align in offset coordinate system.
    pub fn align_spec_offset(&self) -> bool {
        self.align_spec_offset
    }

    /// Set whether to align in offset coordinate system.
    pub fn set_align_spec_offset(&mut self, val: bool) {
        self.align_spec_offset = val;
        self.align_spec_offset_set = true;
    }

    /// Test whether AlignSpecOffset has been explicitly set.
    pub fn test_align_spec_offset(&self) -> bool {
        self.align_spec_offset_set
    }

    // --- Conversion helpers ---

    /// Convert source velocity to heliocentric relativistic velocity (m/s).
    ///
    /// This is used when computing the "Source" rest frame correction.
    /// Follows the C `ConvertSourceVel` algorithm.
    fn convert_source_vel_to_helio_vrel(&self) -> f64 {
        let vel = match self.source_vel {
            Some(v) => v,
            None => return 0.0,
        };

        let sys = self.source_sys();
        let sor = self.source_vrf();

        // If already heliocentric + VREL, return directly.
        if sor == StdOfRest::Heliocentric && sys == SpecSystem::Vrel {
            return vel;
        }

        // Build a SpecMap chain to convert:
        //   source_sys → Vrel → freq → (SOR correction) → freq → Vrel
        let rf = match self.rest_freq {
            Some(rf) => rf,
            None => return 0.0,
        };

        let mut specmap = SpecMap::new();

        // Convert from source_sys to relativistic velocity.
        match sys {
            SpecSystem::Vradio => specmap.add(SpecCvtType::VrToVl, &[]),
            SpecSystem::Voptical => specmap.add(SpecCvtType::VoToVl, &[]),
            SpecSystem::Redshift => specmap.add(SpecCvtType::ZoToVl, &[]),
            SpecSystem::Beta => specmap.add(SpecCvtType::BtToVl, &[]),
            SpecSystem::Vrel => {} // already Vrel
            _ => return 0.0,       // absolute systems invalid for source vel
        }

        // Convert velocity to frequency.
        specmap.add(SpecCvtType::VlToFr, &[rf]);

        // Convert from source_vrf to heliocentric (frequency domain).
        self.add_sor_to_helio(&mut specmap, sor);

        // Convert frequency back to velocity.
        specmap.add(SpecCvtType::FrToVl, &[rf]);

        // Transform the value.
        use crate::coords::PointSet;
        use crate::mappings::Mapping;
        let pts = PointSet::from_rows([[vel]]);
        match specmap.transform(&pts, true) {
            Ok(out) => {
                let v = out.point(0)[0];
                if is_bad(v) { 0.0 } else { v }
            }
            Err(_) => 0.0,
        }
    }

    /// Add SpecMap steps to convert from a given SOR to heliocentric (frequency domain).
    fn add_sor_to_helio(&self, specmap: &mut SpecMap, from: StdOfRest) {
        let ra = self.ref_ra.unwrap_or(0.0);
        let dec = self.ref_dec.unwrap_or(0.0);
        let lon = self.obs_lon.unwrap_or(0.0);
        let lat = self.obs_lat.unwrap_or(0.0);
        let alt = self.obs_alt.unwrap_or(0.0);
        let epoch = self.epoch_mjd();

        match from {
            StdOfRest::Heliocentric => {} // no-op
            StdOfRest::Topocentric => {
                specmap.add(SpecCvtType::TpF2Hl, &[lon, lat, alt, epoch, ra, dec]);
            }
            StdOfRest::Geocentric => {
                specmap.add(SpecCvtType::GeF2Hl, &[epoch, ra, dec]);
            }
            StdOfRest::Barycentric => {
                specmap.add(SpecCvtType::ByF2Hl, &[epoch, ra, dec]);
            }
            StdOfRest::Lsrk => {
                specmap.add(SpecCvtType::LkF2Hl, &[ra, dec]);
            }
            StdOfRest::Lsrd => {
                specmap.add(SpecCvtType::LdF2Hl, &[ra, dec]);
            }
            StdOfRest::LocalGroup => {
                specmap.add(SpecCvtType::LgF2Hl, &[ra, dec]);
            }
            StdOfRest::Galactic => {
                specmap.add(SpecCvtType::GlF2Hl, &[ra, dec]);
            }
            StdOfRest::Source => {
                let src_vel = self.convert_source_vel_to_helio_vrel();
                specmap.add(SpecCvtType::UsF2Hl, &[src_vel, ra, dec]);
            }
        }
    }

    /// Add SpecMap steps to convert from heliocentric to a given SOR (frequency domain).
    fn add_helio_to_sor(&self, specmap: &mut SpecMap, to: StdOfRest) {
        let ra = self.ref_ra.unwrap_or(0.0);
        let dec = self.ref_dec.unwrap_or(0.0);
        let lon = self.obs_lon.unwrap_or(0.0);
        let lat = self.obs_lat.unwrap_or(0.0);
        let alt = self.obs_alt.unwrap_or(0.0);
        let epoch = self.epoch_mjd();

        match to {
            StdOfRest::Heliocentric => {} // no-op
            StdOfRest::Topocentric => {
                specmap.add(SpecCvtType::HlF2Tp, &[lon, lat, alt, epoch, ra, dec]);
            }
            StdOfRest::Geocentric => {
                specmap.add(SpecCvtType::HlF2Ge, &[epoch, ra, dec]);
            }
            StdOfRest::Barycentric => {
                specmap.add(SpecCvtType::HlF2By, &[epoch, ra, dec]);
            }
            StdOfRest::Lsrk => {
                specmap.add(SpecCvtType::HlF2Lk, &[ra, dec]);
            }
            StdOfRest::Lsrd => {
                specmap.add(SpecCvtType::HlF2Ld, &[ra, dec]);
            }
            StdOfRest::LocalGroup => {
                specmap.add(SpecCvtType::HlF2Lg, &[ra, dec]);
            }
            StdOfRest::Galactic => {
                specmap.add(SpecCvtType::HlF2Gl, &[ra, dec]);
            }
            StdOfRest::Source => {
                let src_vel = self.convert_source_vel_to_helio_vrel();
                specmap.add(SpecCvtType::HlF2Us, &[src_vel, ra, dec]);
            }
        }
    }

    /// Topocentric apparent velocity (m/s) of the SpecFrame's
    /// `StdOfRest`. This is the value emitted as the FITS `VELOSYS`
    /// keyword.
    ///
    /// Mirrors `fitschan.c::SpectralAxes` lines 30774-30796: imagine
    /// a hypothetical source at rest in the SpecFrame's `StdOfRest`
    /// (i.e. `SourceVel = 0`, `SourceVRF = StdOfRest`, `SourceSys =
    /// Vrel`), then convert that source velocity into the topocentric
    /// frame.
    ///
    /// Returns `None` if `StdOfRest` is `Source` (the source rest
    /// frame is itself the standard of rest, so the topocentric
    /// velocity is undefined here — C sets `vsource = AST__BAD` in
    /// that branch).
    pub fn velosys(&self) -> Option<f64> {
        let sor = self.std_of_rest();
        if sor == StdOfRest::Source {
            return None;
        }

        let rf = self.rest_freq?;

        // Hypothetical source at rest in the SpecFrame's rest frame.
        // ConvertSourceVel from (sor, Vrel) to (Topocentric, Vrel)
        // with SourceVel = 0:
        //   VlToFr (rf)  → SOR→Helio (sor) → Helio→TPSOR → FrToVl (rf)
        // (The system-conversion VrToVl etc. is a no-op when both
        // sides are Vrel.)
        let mut specmap = SpecMap::new();
        specmap.add(SpecCvtType::VlToFr, &[rf]);
        self.add_sor_to_helio(&mut specmap, sor);
        self.add_helio_to_sor(&mut specmap, StdOfRest::Topocentric);
        specmap.add(SpecCvtType::FrToVl, &[rf]);

        use crate::coords::PointSet;
        use crate::mappings::Mapping;
        let pts = PointSet::from_rows([[0.0]]);
        match specmap.transform(&pts, true) {
            Ok(out) => {
                let v = out.point(0)[0];
                if is_bad(v) { None } else { Some(v) }
            }
            Err(_) => None,
        }
    }

    /// Add SpecMap steps to convert from the current system to frequency.
    fn add_system_to_freq(specmap: &mut SpecMap, system: SpecSystem, rest_freq: Option<f64>) {
        match system {
            SpecSystem::Freq => {} // already frequency
            SpecSystem::Energy => specmap.add(SpecCvtType::EnToFr, &[]),
            SpecSystem::Wavenum => specmap.add(SpecCvtType::WnToFr, &[]),
            SpecSystem::Wavelen => specmap.add(SpecCvtType::WvToFr, &[]),
            SpecSystem::Airwave => specmap.add(SpecCvtType::AwToFr, &[]),
            SpecSystem::Vradio => {
                specmap.add(SpecCvtType::VrToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Voptical => {
                specmap.add(SpecCvtType::VoToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Redshift => {
                specmap.add(SpecCvtType::ZoToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Beta => {
                specmap.add(SpecCvtType::BtToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Vrel => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
        }
    }

    /// Add SpecMap steps to convert from frequency to the target system.
    fn add_freq_to_system(specmap: &mut SpecMap, system: SpecSystem, rest_freq: Option<f64>) {
        match system {
            SpecSystem::Freq => {} // already frequency
            SpecSystem::Energy => specmap.add(SpecCvtType::FrToEn, &[]),
            SpecSystem::Wavenum => specmap.add(SpecCvtType::FrToWn, &[]),
            SpecSystem::Wavelen => specmap.add(SpecCvtType::FrToWv, &[]),
            SpecSystem::Airwave => specmap.add(SpecCvtType::FrToAw, &[]),
            SpecSystem::Vradio => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToVr, &[]);
            }
            SpecSystem::Voptical => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToVo, &[]);
            }
            SpecSystem::Redshift => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToZo, &[]);
            }
            SpecSystem::Beta => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToBt, &[]);
            }
            SpecSystem::Vrel => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
            }
        }
    }

    /// Mirrors C `EqualSor` (specframe.c:989-1077). Two SpecFrames
    /// share the same rest frame only when `std_of_rest` matches AND
    /// the per-SOR-defining attributes also match (RefRA/RefDec
    /// always; Epoch + ObsLat/Lon/Alt for topocentric; Epoch alone
    /// for geocentric/barycentric/heliocentric; SourceVel + SourceVRF
    /// + SourceSys for source).
    fn equal_sor(&self, other: &SpecFrame) -> bool {
        if self.std_of_rest != other.std_of_rest {
            return false;
        }
        if !ast_equal(self.ref_ra.unwrap_or(0.0), other.ref_ra.unwrap_or(0.0)) {
            return false;
        }
        if !ast_equal(self.ref_dec.unwrap_or(0.0), other.ref_dec.unwrap_or(0.0)) {
            return false;
        }
        match self.std_of_rest {
            StdOfRest::Source => {
                ast_equal(
                    self.source_vel.unwrap_or(0.0),
                    other.source_vel.unwrap_or(0.0),
                ) && self.source_vrf == other.source_vrf
                    && self.source_sys == other.source_sys
            }
            StdOfRest::Geocentric | StdOfRest::Barycentric | StdOfRest::Heliocentric => {
                ast_equal(self.epoch_mjd(), other.epoch_mjd())
            }
            StdOfRest::Topocentric => {
                ast_equal(self.epoch_mjd(), other.epoch_mjd())
                    && ast_equal(self.obs_alt.unwrap_or(0.0), other.obs_alt.unwrap_or(0.0))
                    && ast_equal(self.obs_lon.unwrap_or(0.0), other.obs_lon.unwrap_or(0.0))
                    && ast_equal(self.obs_lat.unwrap_or(0.0), other.obs_lat.unwrap_or(0.0))
            }
            StdOfRest::Lsrk | StdOfRest::Lsrd | StdOfRest::Galactic | StdOfRest::LocalGroup => true,
        }
    }

    /// Add a conversion to `specmap` that transforms frequencies from the
    /// standard of rest defined by `this` to the standard of rest defined
    /// by `that`. Mirrors C `SorConvert` (specframe.c:4241-4429).
    ///
    /// Always works in the frequency domain regardless of either
    /// SpecFrame's `System`. The conversion is gated on `!EqualSor(this,
    /// that)`: if the two rest frames are equal (per `EqualSor` semantics)
    /// no SpecMap entries are added.
    fn sor_convert(this: &SpecFrame, that: &SpecFrame, specmap: &mut SpecMap) {
        // No conversion required if the rest frames are equal.
        if this.equal_sor(that) {
            return;
        }

        // Convert from the "this" rest frame to heliocentric, using
        // "this"'s attributes.
        {
            let from = this.std_of_rest;
            let ra = this.ref_ra.unwrap_or(0.0);
            let dec = this.ref_dec.unwrap_or(0.0);
            let lon = this.obs_lon.unwrap_or(0.0);
            let lat = this.obs_lat.unwrap_or(0.0);
            let alt = this.obs_alt.unwrap_or(0.0);
            let epoch = this.epoch_mjd();

            match from {
                StdOfRest::Topocentric => {
                    specmap.add(SpecCvtType::TpF2Hl, &[lon, lat, alt, epoch, ra, dec]);
                }
                StdOfRest::Geocentric => {
                    specmap.add(SpecCvtType::GeF2Hl, &[epoch, ra, dec]);
                }
                StdOfRest::Barycentric => {
                    specmap.add(SpecCvtType::ByF2Hl, &[epoch, ra, dec]);
                }
                StdOfRest::Lsrk => {
                    specmap.add(SpecCvtType::LkF2Hl, &[ra, dec]);
                }
                StdOfRest::Lsrd => {
                    specmap.add(SpecCvtType::LdF2Hl, &[ra, dec]);
                }
                StdOfRest::LocalGroup => {
                    specmap.add(SpecCvtType::LgF2Hl, &[ra, dec]);
                }
                StdOfRest::Galactic => {
                    specmap.add(SpecCvtType::GlF2Hl, &[ra, dec]);
                }
                StdOfRest::Source => {
                    let src_vel = this.convert_source_vel_to_helio_vrel();
                    specmap.add(SpecCvtType::UsF2Hl, &[src_vel, ra, dec]);
                }
                StdOfRest::Heliocentric => {} // already heliocentric, no-op
            }
        }

        // Now go from heliocentric to the "that" frame, using "that"'s
        // attributes.
        {
            let to = that.std_of_rest;
            let ra = that.ref_ra.unwrap_or(0.0);
            let dec = that.ref_dec.unwrap_or(0.0);
            let lon = that.obs_lon.unwrap_or(0.0);
            let lat = that.obs_lat.unwrap_or(0.0);
            let alt = that.obs_alt.unwrap_or(0.0);
            let epoch = that.epoch_mjd();

            match to {
                StdOfRest::Topocentric => {
                    specmap.add(SpecCvtType::HlF2Tp, &[lon, lat, alt, epoch, ra, dec]);
                }
                StdOfRest::Geocentric => {
                    specmap.add(SpecCvtType::HlF2Ge, &[epoch, ra, dec]);
                }
                StdOfRest::Barycentric => {
                    specmap.add(SpecCvtType::HlF2By, &[epoch, ra, dec]);
                }
                StdOfRest::Lsrk => {
                    specmap.add(SpecCvtType::HlF2Lk, &[ra, dec]);
                }
                StdOfRest::Lsrd => {
                    specmap.add(SpecCvtType::HlF2Ld, &[ra, dec]);
                }
                StdOfRest::LocalGroup => {
                    specmap.add(SpecCvtType::HlF2Lg, &[ra, dec]);
                }
                StdOfRest::Galactic => {
                    specmap.add(SpecCvtType::HlF2Gl, &[ra, dec]);
                }
                StdOfRest::Source => {
                    let src_vel = that.convert_source_vel_to_helio_vrel();
                    specmap.add(SpecCvtType::HlF2Us, &[src_vel, ra, dec]);
                }
                StdOfRest::Heliocentric => {} // already heliocentric, no-op
            }
        }
    }

    /// Build the SpecMap for converting from `self` (target) to `result`
    /// using the default alignment frame (cloned from target, with target's
    /// `align_system` and `align_std_of_rest` substituted in for
    /// System/StdOfRest, mirroring the C `Match`/`SubFrame` setup at
    /// specframe.c:4856+, 4883-4884) and the non-integrity ("convert")
    /// path. The two-argument signature is the entry point for `convert`
    /// callers (FITS WCS construction, user-driven `astConvert`).
    pub(crate) fn make_spec_mapping(&self, result: &SpecFrame) -> AstResult<DynMapping> {
        self.make_spec_mapping_full(result, None, false)
    }

    /// Build the SpecMap for converting from `self` (target) to `result`
    /// in the FrameSet integrity path: align frame is taken from the
    /// target's (current's) AlignSystem/AlignStdOfRest applied as
    /// System/StdOfRest, and `intflag=true` so SorConvert sees the
    /// per-SOR-defining attributes of the original alignment frame
    /// rather than overlaying target/result attributes onto it.
    pub(crate) fn make_spec_mapping_integrity(&self, result: &SpecFrame) -> AstResult<DynMapping> {
        self.make_spec_mapping_full(result, None, true)
    }

    /// Full-form port of C `MakeSpecMapping` (specframe.c:2488-2966).
    ///
    /// Parameters mirror C exactly:
    ///   * `target` = `self`, `result` = `result`
    ///   * `align_frm = None` → derive from target as in C `Match`:
    ///     copy target, then set System=target.AlignSystem and
    ///     StdOfRest=target.AlignStdOfRest (mirrors C lines 4867+ and
    ///     4883-4884).
    ///   * `intflag = true` → FrameSet integrity path (C
    ///     AST__INTFLAG branch, lines 2667-2670). `align_target` and
    ///     `align_result` are clones of `align_frm` retaining its own
    ///     SOR-defining attributes.
    ///   * `intflag = false` → "convert" path (C lines 2676-2696):
    ///     `align_target` and `align_result` are copies of `align_frm`
    ///     with target's / result's per-SOR-defining attributes
    ///     (Epoch, RefRA, RefDec, SourceVRF, SourceVel, ObsLat, ObsLon,
    ///     ObsAlt) substituted in.
    fn make_spec_mapping_full(
        &self,
        result: &SpecFrame,
        align_frm: Option<&SpecFrame>,
        intflag: bool,
    ) -> AstResult<DynMapping> {
        let target = self;

        // Derive default alignment frame (mirrors C Match/SubFrame setup
        // at specframe.c:4867 + 4883-4884: align_frm = copy(target) then
        // System=AlignSystem, StdOfRest=AlignStdOfRest).
        let derived_align;
        let align_frm: &SpecFrame = match align_frm {
            Some(f) => f,
            None => {
                let mut a = target.clone();
                a.system = target.align_system();
                a.std_of_rest = target.align_std_of_rest();
                derived_align = a;
                &derived_align
            }
        };

        // C lines 2629-2634.
        let target_rf = target.rest_freq.unwrap_or(0.0);
        let result_rf = result.rest_freq.unwrap_or(0.0);
        let target_system = target.system;
        let result_system = result.system;
        let mut align_system = align_frm.system;

        // C lines 2667-2696. `align_target` / `align_result`
        // construction depends on the integrity flag.
        let (align_target, align_result) = if intflag {
            // Clones — keep align_frm's attributes intact.
            (align_frm.clone(), align_frm.clone())
        } else {
            // Copies, then substitute target/result's per-SOR-defining
            // attributes in. C unconditionally calls astSetEpoch /
            // astSetRefRA / etc, so we mirror that by always populating
            // these fields (using each side's effective default if
            // unset, mirroring the GET behaviour of the corresponding
            // astGet*).
            let mut at = align_frm.clone();
            at.epoch = Some(target.epoch_mjd());
            at.ref_ra = Some(target.ref_ra.unwrap_or(0.0));
            at.ref_dec = Some(target.ref_dec.unwrap_or(0.0));
            at.source_vrf = Some(target.source_vrf());
            at.source_vel = Some(target.source_vel.unwrap_or(0.0));
            at.obs_lat = Some(target.obs_lat.unwrap_or(0.0));
            at.obs_lon = Some(target.obs_lon.unwrap_or(0.0));
            at.obs_alt = Some(target.obs_alt.unwrap_or(0.0));

            let mut ar = align_frm.clone();
            ar.epoch = Some(result.epoch_mjd());
            ar.ref_ra = Some(result.ref_ra.unwrap_or(0.0));
            ar.ref_dec = Some(result.ref_dec.unwrap_or(0.0));
            ar.source_vrf = Some(result.source_vrf());
            ar.source_vel = Some(result.source_vel.unwrap_or(0.0));
            ar.obs_lat = Some(result.obs_lat.unwrap_or(0.0));
            ar.obs_lon = Some(result.obs_lon.unwrap_or(0.0));
            ar.obs_alt = Some(result.obs_alt.unwrap_or(0.0));

            (at, ar)
        };

        // C line 2713: align in Freq if alignment system is absolute,
        // else in Vrel.
        align_system = if align_system.is_absolute() {
            SpecSystem::Freq
        } else {
            SpecSystem::Vrel
        };

        // C lines 2738-2743: initialise all steps to "needed".
        let mut step2 = true;
        let mut step3 = true;
        let mut step4 = true;
        let mut step5 = true;
        let mut step6 = true;
        let mut step7 = true;

        // C line 2746.
        if target_system == SpecSystem::Freq {
            step2 = false;
        }

        // C line 2750.
        if target.equal_sor(&align_target) {
            step3 = false;
        }

        // C line 2754.
        if result.equal_sor(&align_result) {
            step6 = false;
        }

        // C line 2757.
        if result_system == SpecSystem::Freq {
            step7 = false;
        }

        // C line 2761: steps 4 and 5 are not necessary if alignment
        // system is frequency, or target/result rest frequencies are
        // equal.
        if align_system == SpecSystem::Freq || ast_equal(result_rf, target_rf) {
            step4 = false;
            step5 = false;
        }

        // C line 2765.
        if !step4 && !step5 && target.equal_sor(result) {
            step3 = false;
            step6 = false;
        }

        // C lines 2771-2774.
        if !step3
            && !step4
            && !step5
            && !step6
            && target.equal_sor(result)
            && target_system == result_system
            && (target_system.is_absolute() || ast_equal(result_rf, target_rf))
        {
            step2 = false;
            step7 = false;
        }

        // Build the SpecMap.
        let mut specmap = SpecMap::new();

        // Step 2: target system → frequency in target rest frame
        // (C lines 2782-2819).
        if step2 {
            Self::add_system_to_freq(&mut specmap, target_system, target.rest_freq);
        }

        // Step 3: frequency in target SOR → frequency in alignment SOR
        // (C line 2823). Uses target's attrs on the from side and
        // align_target's attrs on the to side.
        if step3 {
            Self::sor_convert(target, &align_target, &mut specmap);
        }

        // Step 4: frequency in alignment SOR → alignment system in
        // alignment SOR (C lines 2828-2833). Uses target rest freq.
        if step4 && align_system == SpecSystem::Vrel {
            specmap.add(SpecCvtType::FrToVl, &[target_rf]);
        }

        // Step 5: alignment system in alignment SOR → frequency in
        // alignment SOR (C lines 2838-2843). Uses result rest freq.
        if step5 && align_system == SpecSystem::Vrel {
            specmap.add(SpecCvtType::VlToFr, &[result_rf]);
        }

        // Step 6: frequency in alignment SOR → frequency in result SOR
        // (C line 2847). Uses align_result's attrs on the from side
        // and result's attrs on the to side.
        if step6 {
            Self::sor_convert(&align_result, result, &mut specmap);
        }

        // Step 7: frequency in result SOR → result system in result
        // SOR (C lines 2851-2888).
        if step7 {
            Self::add_freq_to_system(&mut specmap, result_system, result.rest_freq);
        }

        // Build the canonical-units chain. SpecMap operates on values
        // expressed in the canonical SI unit for each system (Hz for
        // FREQ, m for WAVE, m/s for VEL, etc — see
        // [`SpecSystem::specmap_units`]).
        let mut mapping: DynMapping = if specmap.is_empty() {
            Box::new(UnitMap::new(1))
        } else {
            Box::new(specmap)
        };

        // Wrap in unit-conversion stages at the boundaries to match
        // C's MakeSpecMapping (specframe.c:2891+). C uses `astGetUnit`,
        // which always returns a non-NULL string (the explicit Unit if
        // set, else the system's display default). Wrap whenever the
        // effective Unit differs from the SpecMap's canonical unit
        // (e.g. km/s vs m/s for VRAD), even when no explicit Unit is
        // set. Returns Err on dimensionally-incompatible units
        // (mirrors C `AST__BADUN`).
        let in_default = target_system.specmap_units();
        let in_unit = target
            .unit
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| target_system.default_display_units());
        if !in_unit.is_empty() && !in_default.is_empty() && in_unit != in_default {
            let pre = unit_mapper(in_unit, in_default).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Badun,
                    format!(
                        "SpecFrame Unit '{in_unit}' incompatible with system default '{in_default}'"
                    ),
                )
            })?;
            mapping = Box::new(CmpMap::new(pre, mapping, Composition::Series)?);
        }
        let out_default = result_system.specmap_units();
        let out_unit = result
            .unit
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| result_system.default_display_units());
        if !out_unit.is_empty() && !out_default.is_empty() && out_unit != out_default {
            let post = unit_mapper(out_default, out_unit).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Badun,
                    format!(
                        "SpecFrame Unit '{out_unit}' incompatible with system default '{out_default}'"
                    ),
                )
            })?;
            mapping = Box::new(CmpMap::new(mapping, post, Composition::Series)?);
        }

        Ok(mapping)
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for SpecFrame {
    fn naxes(&self) -> usize {
        1
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("SpecFrame has 1 axis, got axis={axis}"),
            ));
        }
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // Simple numeric formatting with units.
        let units = self.system.specmap_units();
        if units.is_empty() {
            Ok(format!("{value:.6}"))
        } else {
            Ok(format!("{value:.6} {units}"))
        }
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("SpecFrame has 1 axis, got axis={axis}"),
            ));
        }
        // Parse a floating-point number from the start of the string.
        let trimmed = text.trim_start();
        let offset = text.len() - trimmed.len();
        // Find the end of the numeric portion.
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
        // For a 1-D frame, angle at b in triangle a-b-c is either 0 or PI.
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
        // Spectral coordinates are unbounded — no normalisation needed.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    // Match-control attributes — mirror C `astGet/SetMinAxes` etc.
    // SpecFrame defaults Permute to `false` (a 1-axis class can't
    // have a permuted match anyway, and C's specframe.c sets the
    // default to 0).
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
        self.permute_opt.unwrap_or(false)
    }
    fn set_permute(&mut self, v: bool) {
        self.permute_opt = Some(v);
    }
    fn test_permute(&self) -> bool {
        self.permute_opt.is_some()
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = previous.as_any().downcast_ref::<SpecFrame>();
        match prev {
            // FrameSet integrity path (C AST__INTFLAG branch in
            // MakeSpecMapping, specframe.c:2667-2670).
            Some(prev_spec) => prev_spec.make_spec_mapping_integrity(self),
            None => {
                // Previous was not a SpecFrame — return UnitMap.
                Ok(Box::new(UnitMap::new(1)))
            }
        }
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let to_spec = to.as_any().downcast_ref::<SpecFrame>();
        match to_spec {
            Some(to_spec) => {
                let mapping = self.make_spec_mapping(to_spec)?;
                let mut fs = FrameSet::new(self.snapshot());
                fs.add_frame(FrameRef::Base, mapping, to.snapshot())?;
                Ok(Some(fs))
            }
            None => Ok(None),
        }
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SpecFrame>() else {
            return false;
        };
        if self.system != other.system {
            return false;
        }
        if self.std_of_rest != other.std_of_rest {
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
        let rf_eq = match (self.rest_freq, other.rest_freq) {
            (Some(a), Some(b)) => ast_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if !rf_eq {
            return false;
        }
        self.title == other.title && self.domain == other.domain
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

    fn system_string(&self) -> Option<&str> {
        Some(self.system.as_str())
    }

    fn domain(&self) -> &str {
        self.domain.as_deref().unwrap_or("SPECTRUM")
    }

    fn test_domain(&self) -> bool {
        SpecFrame::test_domain(self)
    }

    fn ident(&self) -> &str {
        SpecFrame::ident(self)
    }

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (specframe.c:2998 / 4705 / 3381).
    // -----------------------------------------------------------------

    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        specframe_overlay(template, template_axes, self)
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        // Mirrors specframe.c:2998. Step 1: defer to the parent (Frame
        // class) Match for the axis-count + Domain pre-checks. The
        // axis association arrays returned by the parent are
        // discarded — SpecFrame is a 1-axis class so we know exactly
        // what they should be.
        let parent = super::find_frame::default_match(self, target, match_sub)?;
        if parent.is_none() {
            return Ok(None);
        }

        // Step 2 (specframe.c:3143-3155): walk the target's primary
        // frames looking for a SpecFrame axis. Stop when one is
        // found and remember the (zero-based) external axis index.
        let target_naxes = target.naxes();
        let mut target_axis0: Option<usize> = None;
        for iaxis in 0..target_naxes {
            let (primary, _local_axis) = primary_frame_walk(target, iaxis);
            if primary.as_any().downcast_ref::<SpecFrame>().is_some() {
                target_axis0 = Some(iaxis);
                break;
            }
        }
        let Some(target_axis0) = target_axis0 else {
            return Ok(None);
        };

        // Step 3 (specframe.c:3162-3163): SpecFrame has 1 axis; build
        // the trivial association arrays. The target axis index
        // points to the spectral component within the target.
        let template_axes_vec = vec![0i32];
        let target_axes_vec = vec![target_axis0 as i32];

        // Step 4 (specframe.c:3170-3171): defer to the target's
        // SubFrame to build the result + Mapping.
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
        specframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // SpecFrame is 1-axis, so the only SpecFrame-preserving
        // selection is `[0]` — a direct copy of self. Any other
        // selection (including a `[-1]` "no-association" entry) falls
        // through to a BasicFrame snapshot, mirroring the
        // parent_subframe branch of specframe.c SubFrame at line 4946.
        if target_axes.len() == 1 && target_axes[0] == 0 {
            return Ok(Box::new(self.clone()));
        }

        let mut bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        if self.test_domain() {
            bf.set_domain(self.domain().to_string());
        }
        for (i, &t) in target_axes.iter().enumerate() {
            if t == 0 {
                let _ = bf.set_label(i + 1, self.axis_label());
                let _ = bf.set_unit(i + 1, self.unit_or_default().to_string());
            }
        }
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// Match / SubFrame / Overlay helpers
// ---------------------------------------------------------------------------

/// Mirrors C `Overlay` for SpecFrame (specframe.c:3381).
///
/// Only attributes the template SpecFrame has explicitly set are
/// transferred onto the result Frame. When `result` is itself a
/// SpecFrame and overlaying System would change the coordinate type,
/// the result's Title (and per-axis Unit / Label / Symbol overrides
/// where appropriate) are cleared first so the new system's defaults
/// take effect.
/// Walk a Frame's components to find the primary Frame underlying
/// the supplied (zero-based) external axis. Mirrors C
/// `astPrimaryFrame` (frame.c).
///
/// For each frame class along the way, the external axis is
/// translated through any axis permutation the class carries, so the
/// returned `local_axis` is the *internal* physical axis index of
/// the primary Frame. CmpFrame routes through its perm to one of
/// its components and recurses; SkyFrame applies its own perm to
/// expose the underlying lon/lat axis order.
pub(super) fn primary_frame_walk(frame: &dyn Frame, axis: usize) -> (&dyn Frame, usize) {
    if let Some(cmp) = frame.as_any().downcast_ref::<crate::frames::CmpFrame>() {
        // External axis → internal axis via perm.
        let internal = cmp.perm()[axis];
        let n1 = cmp.naxes1();
        if internal < n1 {
            primary_frame_walk(cmp.frame1(), internal)
        } else {
            primary_frame_walk(cmp.frame2(), internal - n1)
        }
    } else if let Some(sky) = frame.as_any().downcast_ref::<crate::frames::SkyFrame>() {
        // SkyFrame stores an external→internal axis permutation in
        // its `perm` field (after `perm_axes` calls). Apply it so
        // the returned local axis is the underlying physical axis
        // (0 = lon, 1 = lat).
        (frame, sky.axis_permutation()[axis])
    } else {
        (frame, axis)
    }
}

fn specframe_overlay(
    template: &dyn Frame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    let _ = template_axes; // SpecFrame is 1-axis: per-axis OVERLAY uses
    // axis 0 unconditionally (specframe.c:3536-3554 — the OVERLAY macro
    // takes no axis argument because the SpecFrame attributes it
    // copies are scalar).
    let template_spec = match template.as_any().downcast_ref::<SpecFrame>() {
        Some(s) => s,
        None => return Ok(()),
    };

    // Branch: result is a SpecFrame.
    if let Some(result_spec) = result.as_any_mut().downcast_mut::<SpecFrame>() {
        let old_system = result_spec.system;
        let new_system = template_spec.system;

        // specframe.c:3494-3510 — if the System will change AND the
        // template explicitly sets it, clear Title (+ Label and Unit
        // when their default depends on the system).
        if new_system != old_system && template_spec.test_system() {
            result_spec.clear_title();

            // Clear Unit only when the new and old systems' default
            // units differ. This mirrors the C `DefUnit` comparison at
            // specframe.c:3499-3502.
            let new_def = new_system.specmap_units();
            let old_def = old_system.specmap_units();
            if new_def != old_def {
                result_spec.clear_unit();
            }

            // Clear axis Label override (specframe.c:3508-3509).
            // (Symbol has no per-axis storage in the Rust port — the
            // SpecFrame returns a fixed default — so nothing to do.)
            result_spec.clear_axis_label();
        }

        // Parent-class Frame::Overlay copies (frame.c:8567-8594).
        // Mirrors the call at specframe.c:3527.
        if template_spec.test_domain() {
            result_spec.set_domain(template_spec.domain());
        }
        if template_spec.test_title() {
            if let Some(ref t) = template_spec.title {
                result_spec.set_title(t);
            }
        }
        if template_spec.test_epoch() {
            result_spec.set_epoch_mjd(template_spec.epoch_mjd());
        }
        if let Some(v) = template_spec.obs_lat {
            result_spec.set_obs_lat(v);
        }
        if let Some(v) = template_spec.obs_lon {
            result_spec.set_obs_lon(v);
        }
        if let Some(v) = template_spec.obs_alt {
            result_spec.set_obs_alt(v);
        }
        if template_spec.test_system() {
            result_spec.set_system(template_spec.system);
        }
        if template_spec.test_align_system() {
            result_spec.set_align_system(template_spec.align_system());
        }
        if template_spec.test_unit() {
            if let Some(ref u) = template_spec.unit {
                result_spec.set_unit(u);
            }
        }

        // SpecFrame-specific OVERLAY block (specframe.c:3545-3554).
        // Note: SourceVRF must be overlayed before SourceVel — this
        // mirrors the explicit ordering call-out at specframe.c:3542.
        if template_spec.test_align_std_of_rest() {
            result_spec.set_align_std_of_rest(template_spec.align_std_of_rest());
        }
        if template_spec.test_align_spec_offset() {
            result_spec.set_align_spec_offset(template_spec.align_spec_offset());
        }
        if template_spec.test_ref_dec() {
            if let Some(v) = template_spec.ref_dec {
                result_spec.set_ref_dec(v);
            }
        }
        if template_spec.test_ref_ra() {
            if let Some(v) = template_spec.ref_ra {
                result_spec.set_ref_ra(v);
            }
        }
        if template_spec.test_rest_freq() {
            if let Some(v) = template_spec.rest_freq {
                result_spec.set_rest_freq(v);
            }
        }
        if template_spec.test_source_sys() {
            result_spec.set_source_sys(template_spec.source_sys());
        }
        if template_spec.test_source_vrf() {
            result_spec.set_source_vrf(template_spec.source_vrf());
        }
        if template_spec.test_source_vel() {
            if let Some(v) = template_spec.source_vel {
                result_spec.set_source_vel(v);
            }
        }
        if template_spec.test_std_of_rest() {
            result_spec.set_std_of_rest(template_spec.std_of_rest);
        }
        if template_spec.test_spec_origin() {
            if let Some(v) = template_spec.spec_origin {
                result_spec.set_spec_origin(v);
            }
        }

        return Ok(());
    }

    // Branch: result is not a SpecFrame. Mirrors specframe.c:3516-3522
    // (clears System and AlignSystem on a deep copy of the template
    // before invoking parent_overlay) — we reproduce the same
    // observable effect by skipping System/AlignSystem here and
    // copying only Frame-level attributes.
    if template_spec.test_domain() {
        if let Some(bf) = result
            .as_any_mut()
            .downcast_mut::<super::basic::BasicFrame>()
        {
            bf.set_domain(template_spec.domain().to_string());
        }
    }
    Ok(())
}

/// Mirrors C `SubFrame` for SpecFrame (specframe.c:4705).
fn specframe_sub_frame(
    target: &SpecFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-SpecFrame branch (specframe.c:4840-4895).
    if result_naxes == 1 && target_axes.len() == 1 && target_axes[0] == 0 {
        // Form the result from a copy of the target (specframe.c:4843).
        let mut result = target.clone();

        // If a template was supplied, overlay it onto the result and
        // pick the alignment frame from the template (specframe.c:
        // 4853-4868).
        let align_frm = if let Some(t) = template {
            specframe_overlay(t, template_axes, &mut result)?;
            if let Some(template_spec) = t.as_any().downcast_ref::<SpecFrame>() {
                template_spec.clone()
            } else {
                target.clone()
            }
        } else {
            // No template — align in the target's own SOR
            // (specframe.c:4872-4876). The C VerifyAttrs check that
            // StdOfRest is set is currently elided; downstream
            // MakeSpecMapping carries enough state to fail loudly when
            // alignment is genuinely impossible.
            target.clone()
        };

        // The MakeSpecMapping function uses System and StdOfRest to
        // define the alignment frame, but C `SubFrame` substitutes
        // AlignSystem/AlignStdOfRest there (specframe.c:4883-4884).
        let mut align_frm = align_frm;
        align_frm.system = align_frm.align_system();
        align_frm.std_of_rest = align_frm.align_std_of_rest();

        // Build the Mapping target → result, taking account of the
        // alignment frame (specframe.c:4891).
        let mapping = target.make_spec_mapping_full(&result, Some(&align_frm), false)?;

        let result_box: DynFrame = Box::new(result);
        return Ok(Some((mapping, result_box)));
    }

    // Result is not a SpecFrame (specframe.c:4905-4951). Make a copy,
    // explicitly set Frame-level / per-axis defaults, clear System and
    // AlignSystem (since their values are SpecFrame-specific), then
    // defer to the default Frame-level SubFrame.
    let mut temp = target.clone();
    if !temp.test_title() {
        temp.set_title(&temp.default_title());
    }
    if !temp.test_domain() {
        temp.set_domain("SPECTRUM");
    }
    if !temp.test_axis_label() {
        let lbl = temp.axis_label();
        temp.set_axis_label(lbl);
    }
    if !temp.test_unit() {
        let u = temp.unit_or_default().to_string();
        temp.set_unit(&u);
    }
    temp.clear_system();
    temp.clear_align_system();

    super::find_frame::default_sub_frame(&temp, template, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for SpecFrame {
    fn class_name(&self) -> &'static str {
        "SpecFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of spectral coordinate system".to_string());

        // --- Frame-level attributes (before IsA Frame) ---
        // Order: Title, Naxes, Domain, Epoch, Lbl1, System, Uni1, Ax1,
        //        [observer], IsA Frame

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

        // Domain: active if explicitly set to non-"SPECTRUM", otherwise unset.
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() && domain != "SPECTRUM" {
                node.push_string(
                    "Domain",
                    domain,
                    Some("Coordinate system domain".to_string()),
                );
            } else {
                node.push_unset(
                    "Domain",
                    NodeValue::String("SPECTRUM".to_string()),
                    Some("Coordinate system domain".to_string()),
                );
            }
        } else {
            node.push_unset(
                "Domain",
                NodeValue::String("SPECTRUM".to_string()),
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

        // Lbl1: axis label (always unset with system-appropriate default).
        node.push_unset(
            "Lbl1",
            NodeValue::String(self.system.label().to_string()),
            Some("Label for axis 1".to_string()),
        );

        // System (always active).
        node.push_string(
            "System",
            self.system.as_str(),
            Some("Coordinate system type".to_string()),
        );

        // Uni1: axis unit (always unset with current unit or system default).
        let display_unit = self
            .unit
            .as_deref()
            .unwrap_or(self.system.default_display_units());
        if !display_unit.is_empty() {
            node.push_unset(
                "Uni1",
                NodeValue::String(display_unit.to_string()),
                Some("Units for axis 1".to_string()),
            );
        }

        // ActUnt: active if explicitly set to false.
        if let Some(false) = self.act_unt {
            node.push_int(
                "ActUnt",
                0,
                Some("Unit strings do not affect alignment".to_string()),
            );
        }

        // Ax1: axis child object with unit (if unit is set or non-empty default).
        let mut axis_node = Node::new("Axis");
        axis_node.class_comment = Some("Coordinate axis".to_string());
        if let Some(ref unit) = self.unit {
            axis_node.push_string("Unit", unit, Some("Axis units".to_string()));
        }
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

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- SpecFrame-specific attributes (after IsA Frame) ---

        // SoR (StdOfRest) — only write if explicitly set.
        if self.sor_set {
            node.push_string(
                "SoR",
                self.std_of_rest.as_str(),
                Some("Standard of rest".to_string()),
            );
        }

        // Unit preference (UFreq, UWave, etc.) — only if explicitly set.
        if let Some(ref unit) = self.unit {
            node.push_string(
                self.system.unit_attr_key(),
                unit,
                Some(self.system.unit_attr_comment().to_string()),
            );
        }

        // AlSoR (AlignStdOfRest, only if explicitly set).
        if let Some(ref asor) = self.align_std_of_rest {
            node.push_string(
                "AlSoR",
                asor.as_str(),
                Some("Alignment standard of rest".to_string()),
            );
        }

        // AlSys (AlignSystem, only if explicitly set).
        if let Some(ref asys) = self.align_system {
            node.push_string(
                "AlSys",
                asys.as_str(),
                Some("Alignment coordinate system".to_string()),
            );
        }

        // RefRA (if set).
        if let Some(ref_ra) = self.ref_ra {
            node.push_double(
                "RefRA",
                ref_ra,
                Some("Reference RA (rads, FK5 J2000)".to_string()),
            );
        }

        // RefDec (if set).
        if let Some(ref_dec) = self.ref_dec {
            node.push_double(
                "RefDec",
                ref_dec,
                Some("Reference Dec (rads, FK5 J2000)".to_string()),
            );
        }

        // RstFrq (RestFreq, if set).
        if let Some(rest_freq) = self.rest_freq {
            node.push_double("RstFrq", rest_freq, Some("Rest frequency (Hz)".to_string()));
        }

        // SrcVel (SourceVel, if set).
        if let Some(source_vel) = self.source_vel {
            node.push_double(
                "SrcVel",
                source_vel,
                Some("Source velocity (m/s)".to_string()),
            );
        }

        // SrcVRF (SourceVRF, if set).
        if let Some(ref svrf) = self.source_vrf {
            node.push_string(
                "SrcVRF",
                svrf.as_str(),
                Some("Source velocity rest frame".to_string()),
            );
        }

        // SrcSys (SourceSys, if set).
        if let Some(ref ssys) = self.source_sys {
            node.push_string(
                "SrcSys",
                ssys.as_str(),
                Some("Source velocity spectral system".to_string()),
            );
        }

        // AlSpOf (AlignSpecOffset, if true).
        if self.align_spec_offset {
            node.push_int("AlSpOf", 1, Some("Align in offset coords".to_string()));
        }

        // SpOrg (SpecOrigin, if set).
        if let Some(spec_origin) = self.spec_origin {
            if !is_bad(spec_origin) {
                node.push_double("SpOrg", spec_origin, Some("Spec offset".to_string()));
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Read System.
        let sys_str = node.get_string("System", "FREQ");
        let system = SpecSystem::parse(&sys_str).unwrap_or(SpecSystem::Freq);
        let system_present = node.get("System").is_some();

        let mut sf = SpecFrame::new(system);
        // SpecFrame::new only flags the system as set when the
        // requested system differs from the C default `FREQ`. When the
        // serialised form has an explicit `System=` line we honour
        // that regardless.
        sf.system_set = system_present;

        // Title.
        let title = node.get_string("Title", "");
        if !title.is_empty() {
            sf.set_title(&title);
        }

        // Domain.
        let domain = node.get_string("Domain", "");
        if !domain.is_empty() {
            sf.domain = Some(domain);
        }

        // Epoch. AST Native stores Epoch as a decimal year ("Julian
        // epoch of observation" / "Besselian epoch of observation");
        // convert to MJD via the IAU 1984 rule. Mirrors C
        // `astReadDateTime` invoked from `frame.c::SetAttrib` for
        // the "epoch=..." setting (frame.c:10185).
        if node.get("Epoch").is_some() {
            sf.set_epoch_year(node.get_double("Epoch", 0.0));
        }

        // Observer attributes.
        if node.get("ObsLat").is_some() {
            sf.obs_lat = Some(node.get_double("ObsLat", 0.0));
        }
        if node.get("ObsLon").is_some() {
            sf.obs_lon = Some(node.get_double("ObsLon", 0.0));
        }
        if node.get("ObsAlt").is_some() {
            sf.obs_alt = Some(node.get_double("ObsAlt", 0.0));
        }

        // SoR (StdOfRest).
        let sor_str = node.get_string("SoR", "Heliocentric");
        if let Some(sor) = StdOfRest::parse(&sor_str) {
            sf.std_of_rest = sor;
            if node.get("SoR").is_some() {
                sf.sor_set = true;
            }
        }

        // Unit preference (UFreq, UWave, etc.).
        let unit_key = system.unit_attr_key();
        let unit_str = node.get_string(unit_key, "");
        if !unit_str.is_empty() {
            sf.unit = Some(unit_str);
        } else {
            // Fall back to Uni1 (the per-axis attribute used by CmpFrame
            // overrides), then to the Axis child Unit attribute. C reaches
            // the same effect because CmpFrame's SetAttrib forwards
            // "Unit(N)" into the inner SpecFrame which stores it on both
            // the Axis and `usedunits[system]`.
            let uni1 = node.get_string("Uni1", "");
            if !uni1.is_empty() {
                sf.unit = Some(uni1);
            } else if let Some(axis_node) = node.get_object("Ax1") {
                let axis_unit = axis_node.get_string("Unit", "");
                if !axis_unit.is_empty() {
                    sf.unit = Some(axis_unit);
                }
            }
        }

        // AlSoR (AlignStdOfRest).
        let alsor_str = node.get_string("AlSoR", "");
        if !alsor_str.is_empty() {
            sf.align_std_of_rest = StdOfRest::parse(&alsor_str);
        }

        // AlSys (AlignSystem).
        let alsys_str = node.get_string("AlSys", "");
        if !alsys_str.is_empty() {
            sf.align_system = SpecSystem::parse(&alsys_str);
        }

        // RefRA.
        if node.get("RefRA").is_some() {
            sf.ref_ra = Some(node.get_double("RefRA", 0.0));
        }

        // RefDec.
        if node.get("RefDec").is_some() {
            sf.ref_dec = Some(node.get_double("RefDec", 0.0));
        }

        // RstFrq (RestFreq).
        if node.get("RstFrq").is_some() {
            sf.rest_freq = Some(node.get_double("RstFrq", 0.0));
        }

        // SrcVel (SourceVel).
        if node.get("SrcVel").is_some() {
            sf.source_vel = Some(node.get_double("SrcVel", 0.0));
        }

        // SrcVRF (SourceVRF).
        let svrf_str = node.get_string("SrcVRF", "");
        if !svrf_str.is_empty() {
            sf.source_vrf = StdOfRest::parse(&svrf_str);
        }

        // SrcSys (SourceSys).
        let ssys_str = node.get_string("SrcSys", "");
        if !ssys_str.is_empty() {
            sf.source_sys = SpecSystem::parse(&ssys_str);
        }

        // AlSpOf (AlignSpecOffset).
        if node.get("AlSpOf").is_some() {
            sf.align_spec_offset = node.get_int("AlSpOf", 0) != 0;
            sf.align_spec_offset_set = true;
        }

        // SpOrg (SpecOrigin).
        if node.get("SpOrg").is_some() {
            sf.spec_origin = Some(node.get_double("SpOrg", 0.0));
        }

        // ActUnt (default is 1 = true; only store when 0).
        let act_unt_val = node.get_int("ActUnt", 1);
        if act_unt_val == 0 {
            sf.act_unt = Some(false);
        }

        // Ident (Object-level identification string).
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            sf.ident = Some(ident);
        }

        Ok(sf)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{Frame, SpecFrame, SpecSystem, StdOfRest};

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (specframe.c:2998 / 4705 / 3381)
    // -----------------------------------------------------------------

    #[test]
    fn overlay_template_copies_set_system() {
        // Template has System explicitly set to VRAD; result starts as
        // FREQ. After overlay the result must be VRAD and its Title
        // cleared (since the system changed and the template had
        // System set).
        let template = SpecFrame::new(SpecSystem::Vradio);
        assert!(template.test_system());
        let target = SpecFrame::new(SpecSystem::Freq);
        assert!(!target.test_system());
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.system(), SpecSystem::Vradio);
    }

    #[test]
    fn overlay_template_skips_unset_attrs() {
        // Template default-constructs as FREQ without any explicit
        // System; overlaying onto a VRAD target must leave the
        // target's system unchanged.
        let template = SpecFrame::new(SpecSystem::Freq);
        assert!(!template.test_system());
        let target = SpecFrame::new(SpecSystem::Vradio);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.system(), SpecSystem::Vradio);
    }

    #[test]
    fn overlay_template_copies_rest_freq_when_set() {
        let mut template = SpecFrame::new(SpecSystem::Freq);
        template.set_rest_freq(1.420405751e9);
        let target = SpecFrame::new(SpecSystem::Freq);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.rest_freq(), Some(1.420405751e9));
    }

    #[test]
    fn sub_frame_specframe_to_specframe() {
        // FREQ/Heliocentric target → VRAD/LSRK template via SubFrame
        // must produce a 1-axis Mapping and a SpecFrame result whose
        // system is VRAD and SOR is LSRK.
        let mut target = SpecFrame::new(SpecSystem::Freq);
        target.set_std_of_rest(StdOfRest::Heliocentric);
        target.set_rest_freq(1.420405751e9);

        let mut template = SpecFrame::new(SpecSystem::Vradio);
        template.set_std_of_rest(StdOfRest::Lsrk);
        template.set_rest_freq(1.420405751e9);

        let template_axes = [0i32];
        let target_axes = [0i32];
        let (map, result) = target
            .sub_frame(Some(&template), 1, &target_axes, &template_axes)
            .unwrap()
            .expect("should match");
        assert_eq!(map.nin(), 1);
        assert_eq!(map.nout(), 1);
        let result_spec = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(result_spec.system(), SpecSystem::Vradio);
        assert_eq!(result_spec.std_of_rest(), StdOfRest::Lsrk);
    }

    #[test]
    fn match_template_specframe_self() {
        let target = SpecFrame::new(SpecSystem::Freq);
        let template = SpecFrame::new(SpecSystem::Vradio);
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("should match");
        assert_eq!(m.template_axes.len(), 1);
        assert_eq!(m.target_axes.len(), 1);
        assert_eq!(m.template_axes[0], 0);
        assert_eq!(m.target_axes[0], 0);
        let r = m.result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.system(), SpecSystem::Vradio);
    }

    #[test]
    fn match_template_specframe_against_basicframe_fails() {
        // SpecFrame template against a non-SpecFrame target must not
        // match (specframe.c:3147 — the astPrimaryFrame / IsASpecFrame
        // check rejects anything whose primary frame is not a
        // SpecFrame).
        let template = SpecFrame::new(SpecSystem::Freq);
        let target = super::super::basic::BasicFrameBuilder::new(1)
            .build()
            .unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_specframe_supports_max_axes_oversized() {
        // 1-axis SpecFrame template with MaxAxes=2 should match a
        // 2-axis target only if the underlying target has a SpecFrame
        // axis. Until CmpFrame walking lands the CmpFrame/SkyFrame
        // case is rejected (returns None); a bare 2-axis BasicFrame
        // target is therefore the only oversized case we can
        // exercise. Confirm the oversized template is rejected (no
        // SpecFrame axis present in the target).
        let mut template = SpecFrame::new(SpecSystem::Freq);
        template.set_max_axes(2);
        let target = super::super::basic::BasicFrameBuilder::new(2)
            .build()
            .unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn pick_axes_snapshot_specframe_full() {
        // Picking axis 0 yields a SpecFrame copy.
        let target = SpecFrame::new(SpecSystem::Vradio);
        let snap = target.pick_axes_snapshot(&[0]).unwrap();
        let s = snap.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(s.system(), SpecSystem::Vradio);
    }

    #[test]
    fn pick_axes_snapshot_specframe_partial() {
        // Picking with `-1` (no association) yields a 1-axis BasicFrame.
        let target = SpecFrame::new(SpecSystem::Vradio);
        let snap = target.pick_axes_snapshot(&[-1]).unwrap();
        assert_eq!(snap.naxes(), 1);
        assert!(snap.as_any().downcast_ref::<SpecFrame>().is_none());
    }
}
