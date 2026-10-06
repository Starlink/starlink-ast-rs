//! The spectral coordinate frame (SpecFrame).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame};
use crate::mappings::helpers::{ast_equal, is_bad};
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, SpecCvtType, SpecMap, UnitMap};
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
    /// System label with a capitalised first letter, matching C's
    /// `SystemLabel` (specframe.c) as used by both GetLabel and GetTitle.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Freq => "Frequency",
            Self::Energy => "Energy",
            Self::Wavenum => "Wave-number",
            Self::Wavelen => "Wavelength",
            Self::Airwave => "Wavelength in air",
            Self::Vradio => "Radio velocity",
            Self::Voptical => "Optical velocity",
            Self::Redshift => "Redshift",
            Self::Beta => "Beta factor",
            Self::Vrel => "Apparent radial velocity",
        }
    }

    /// All systems in C's `AstSystemType` index order (specframe.h:78-87).
    /// Used to dump the `UsedUnits` block (`U<system>`) in the same order C
    /// iterates its `usedunits[]` array.
    pub(crate) const ALL: [Self; 10] = [
        Self::Freq,
        Self::Energy,
        Self::Wavenum,
        Self::Wavelen,
        Self::Airwave,
        Self::Vradio,
        Self::Voptical,
        Self::Redshift,
        Self::Beta,
        Self::Vrel,
    ];

    /// Position of this system in [`Self::ALL`] — the index into a
    /// `used_units` array.
    pub(crate) fn used_units_index(&self) -> usize {
        Self::ALL.iter().position(|s| s == self).unwrap()
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
    /// The Frame-class attributes.
    attrs: super::frame_attrs::FrameAttrs,
    // Frame-level attributes

    // Observer

    // SpecFrame-specific
    system: SpecSystem,
    /// Whether System has been explicitly set. Mirrors C's `astTestSystem`,
    /// which tracks set-ness independently of the value
    /// (`this->system != AST__BADSYSTEM`, frame.c:13822). When unset, C
    /// `GetSystem` returns the class default `AST__WAVELEN`
    /// (specframe.c:2015). A SpecFrame built via [`SpecFrame::new`] has
    /// System explicitly set (the caller chose it, equivalent to C
    /// `astSpecFrame("System=...")`); [`SpecFrame::clear_system`] reverts
    /// to the unset state in which `system` holds the WAVELEN default.
    system_set: bool,
    std_of_rest: StdOfRest,
    sor_set: bool, // whether std_of_rest was explicitly set (vs defaulting)
    /// The Axis-class attributes of the single axis. Its Unit is the
    /// user-facing unit for the current system (e.g. "GHz").
    axis: super::axis::Axis,
    /// Preferred unit recorded per spectral system, mirroring C's
    /// `usedunits[]` (specframe.c SetUnit). Indexed by `SpecSystem::ALL`
    /// position. A unit set while in a given system is remembered here and
    /// re-instated if the SpecFrame later returns to that system.
    used_units: [Option<String>; 10],
    rest_freq: Option<f64>,         // Hz
    ref_ra: Option<f64>,            // FK5 J2000 radians
    ref_dec: Option<f64>,           // FK5 J2000 radians
    source_vel: Option<f64>,        // m/s (or redshift/beta depending on source_sys)
    source_vrf: Option<StdOfRest>,  // rest frame of source velocity
    source_sys: Option<SpecSystem>, // spectral system of source velocity
    spec_origin: Option<f64>,       // spectral origin (in default units for current system)
    align_std_of_rest: Option<StdOfRest>,
    /// Alignment system. Mirrors C `GetAlignSystem` which returns
    /// `AST__WAVELEN` by default for SpecFrames (specframe.c:1899-1957).
    align_system: Option<SpecSystem>,
    align_spec_offset: bool,
    /// Whether AlignSpecOffset was explicitly set. Mirrors C
    /// `astTestAlignSpecOffset`.
    align_spec_offset_set: bool,
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
}

impl SpecFrame {
    /// The Frame-class attributes, for a subclass's cast into a plain Frame.
    pub(crate) fn frame_attrs(&self) -> &super::frame_attrs::FrameAttrs {
        &self.attrs
    }

    // Defaults of the Frame-class attributes, for the `frame_attrs_*`
    // macros. Permute is the Frame class's default: SpecFrame does not
    // over-ride it (frame.c:13572).
    fn frame_default_title(&self) -> String {
        self.default_title()
    }
    fn frame_default_domain(&self) -> &'static str {
        "SPECTRUM"
    }
    fn frame_default_epoch(&self) -> f64 {
        super::frame_attrs::default_epoch_mjd()
    }
    fn frame_default_permute(&self) -> bool {
        true
    }

    /// Create a new `SpecFrame` with the given spectral coordinate system.
    ///
    /// Defaults: domain = "SPECTRUM", std_of_rest = Heliocentric.
    pub fn new(system: SpecSystem) -> Self {
        Self {
            attrs: super::frame_attrs::FrameAttrs::default(),
            // None = use the default "SPECTRUM". Mirrors C's
            // astTestDomain returning false until set_domain.
            system,
            // The caller supplied an explicit System, which is equivalent
            // to C `astSpecFrame("System=...")` invoking astSetSystem — so
            // astTestSystem is true regardless of the value. C's unset
            // default (AST__WAVELEN, specframe.c:2015) is only reachable
            // via clear_system().
            system_set: true,
            std_of_rest: StdOfRest::Heliocentric,
            sor_set: false,
            axis: super::axis::Axis::default(),
            used_units: Default::default(),
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
        }
    }

    /// A SpecFrame with no attribute set, as `astSpecFrame(" ")` makes one:
    /// its System is wavelength by default rather than by being set.
    pub fn unset_system() -> Self {
        Self {
            system_set: false,
            ..Self::new(SpecSystem::Wavelen)
        }
    }

    // --- SpecFrame-specific attributes ---

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.attrs.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.attrs.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Return the spectral coordinate system.
    pub fn system(&self) -> SpecSystem {
        self.system
    }

    /// Set the spectral coordinate system.
    pub fn set_system(&mut self, sys: SpecSystem) {
        let old = self.system;
        self.system = sys;
        self.system_set = true;
        if old != sys {
            // Changing System changes the default Units. If a unit was
            // previously used with the new system, re-instate it; otherwise
            // clear the current unit so the new system's default applies.
            // Mirrors C ClearSystem/SetSystem (specframe.c:628-636). Set the
            // field directly so we don't re-record into used_units.
            self.axis.unit = self.used_units[sys.used_units_index()].clone();
            // A set SpecOrigin keeps referring to the same spectral
            // position: convert it to the new system (specframe.c:643,
            // OriginSystem).
            self.rescale_spec_origin(|sf| {
                sf.system = old;
            });
            // Attributes with System-specific defaults (specframe.c:4117).
            self.axis.label = None;
            self.axis.symbol = None;
            self.attrs.title = None;
        }
    }

    /// Test whether System has been explicitly set. Mirrors C
    /// `astTestSystem` for SpecFrame.
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear System back to its class default. Mirrors C
    /// `astClearSystem` for SpecFrame: after clearing, `astGetSystem`
    /// returns the SpecFrame default `AST__WAVELEN` (specframe.c:2015).
    pub fn clear_system(&mut self) {
        // specframe.c `ClearSystem`: when the default System differs from
        // the old one, restore the Unit last used with it (or clear the
        // Unit), clear the attributes with System-specific defaults, and
        // re-express a set SpecOrigin.
        let old = self.system;
        self.system = SpecSystem::Wavelen;
        self.system_set = false;
        if old != self.system {
            self.axis.unit = self.used_units[self.system.used_units_index()].clone();
            self.axis.label = None;
            self.axis.symbol = None;
            self.attrs.title = None;
            self.rescale_spec_origin(|sf| {
                sf.system = old;
            });
        }
    }

    /// Return the standard of rest.
    pub fn std_of_rest(&self) -> StdOfRest {
        self.std_of_rest
    }

    /// Set the standard of rest.
    pub fn set_std_of_rest(&mut self, sor: StdOfRest) {
        let old = self.std_of_rest;
        let changed = old != sor;
        self.std_of_rest = sor;
        self.sor_set = true;
        // Mirrors OriginStdOfRest (specframe.c:3191): a set SpecOrigin
        // keeps referring to the same spectral position.
        if changed {
            self.rescale_spec_origin(|sf| {
                sf.std_of_rest = old;
            });
        }
    }

    /// Test whether StdOfRest has been explicitly set.
    pub fn test_std_of_rest(&self) -> bool {
        self.sor_set
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

    /// Set the Unit of the single axis and record it as the one used with
    /// the current System, as C's `SetUnit` does (specframe.c:4167).
    pub(crate) fn store_unit(&mut self, unit: &str) {
        self.used_units[self.system.used_units_index()] = Some(unit.to_string());
        self.axis.unit = Some(unit.to_string());
    }

    /// Return the user-facing unit, falling back to the system default
    /// (e.g. "GHz" for FREQ, "km/s" for VRAD). Mirrors C's `astGetUnit`,
    /// which always returns a non-NULL string.
    pub fn unit_or_default(&self) -> &str {
        self.axis
            .unit
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.system.default_display_units())
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

    /// Clear AlignStdOfRest.
    pub fn clear_align_std_of_rest(&mut self) {
        self.align_std_of_rest = None;
    }

    /// Clear StdOfRest, re-expressing a set SpecOrigin as heliocentric
    /// first (specframe.c:650).
    pub fn clear_std_of_rest(&mut self) {
        self.set_std_of_rest(StdOfRest::Heliocentric);
        self.sor_set = false;
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

    /// RestFreq in Hz, set or default (1e14 Hz; specframe.c:6020).
    pub fn rest_freq(&self) -> f64 {
        self.rest_freq.unwrap_or(1.0e14)
    }

    /// Clear RestFreq.
    pub fn clear_rest_freq(&mut self) {
        self.rest_freq = None;
    }

    /// Set the rest frequency in Hz.
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.rest_freq = Some(hz);
    }

    /// Test whether RestFreq has been explicitly set.
    pub fn test_rest_freq(&self) -> bool {
        self.rest_freq.is_some()
    }

    /// RefRA (FK5 J2000, radians), set or default (0).
    pub fn ref_ra(&self) -> f64 {
        self.ref_ra.unwrap_or(0.0)
    }

    /// Clear RefRA.
    pub fn clear_ref_ra(&mut self) {
        self.ref_ra = None;
    }

    /// Set the reference RA (FK5 J2000, radians).
    pub fn set_ref_ra(&mut self, ra: f64) {
        self.ref_ra = Some(ra);
    }

    /// Test whether RefRA has been explicitly set.
    pub fn test_ref_ra(&self) -> bool {
        self.ref_ra.is_some()
    }

    /// RefDec (FK5 J2000, radians), set or default (0).
    pub fn ref_dec(&self) -> f64 {
        self.ref_dec.unwrap_or(0.0)
    }

    /// Clear RefDec.
    pub fn clear_ref_dec(&mut self) {
        self.ref_dec = None;
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

        // Otherwise, find the Mapping from the supplied SkyFrame to the
        // FK5 J2000 SkyFrame in which RefRA/RefDec are stored. C uses
        // `astFindFrame(frm, skyframe, "")` (specframe.c:4000-4003), NOT
        // `astConvert`: FindFrame uses the FK5/J2000 template's own
        // settings to drive the alignment (Match/SubFrame), so an
        // equinox/epoch-only difference within FK5 collapses to a
        // UnitMap. `astConvert` instead aligns through AlignSystem
        // (ICRS), which leaves a sub-SkyTol SlaMap residue that perturbs
        // the stored RefRA at the f32 level and breaks the downstream
        // Geocentric/Heliocentric SpecMap cancellation.
        let mut canonical = super::SkyFrame::new(super::SkySystem::Fk5);
        // C builds the template as `astSkyFrame("system=FK5,equinox=J2000")`
        // (specframe.c:3994), which leaves Equinox *set*. Set-ness is
        // load-bearing: `Match`/`SubFrame` only impose the template's Equinox
        // on the result when the template has tested it, so leaving it at the
        // (numerically identical) J2000 default would let a B1950 source
        // frame carry its own equinox through the alignment, collapsing the
        // conversion to a near-identity.
        canonical.set_equinox_year(2000.0);
        let Some(fs) = crate::frames::Frame::find_frame_template(frm, &canonical, " ")? else {
            // Alignment was not possible — leave RefRA/RefDec unchanged
            // (C silently no-ops in this case).
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
        // RefRA must lie in [0, 2π); RefDec must lie in (-π, π]. AST
        // (and SLALIB / palGaleq, via eraAnp / eraAnpm) normalises to
        // those ranges; our SlaMap goes through Cartesian and ends up
        // in (-π, π] for the longitude. Re-apply the standard ranges
        // so RefRA matches C's f64 representation bit-for-bit —
        // without this, a galactic source with RA ~ 332° comes out as
        // -28° in f64, and the f32 cast inside SpecMap's LSRK fcorr
        // (see c-library-quirks.md "SpecMap velocity routines cast
        // RefRA/RefDec to float") produces a slightly different
        // velocity, drifting DELTAV by ~30 mm/s.
        self.set_ref_ra(crate::frames::skyaxis::normalise_ra(refra));
        self.set_ref_dec(crate::frames::skyaxis::normalise_dec(refdec));
        Ok(())
    }

    /// SourceVel, set or default (0): a velocity in m/s, or a redshift or
    /// beta factor, according to SourceSys.
    pub fn source_vel(&self) -> f64 {
        self.source_vel.unwrap_or(0.0)
    }

    /// Clear SourceVel.
    pub fn clear_source_vel(&mut self) {
        self.source_vel = None;
    }

    /// Whether SourceSys is a velocity, which C's `SourceVel` attribute
    /// gives in km/s (specframe.c `GetAttrib`).
    fn source_vel_is_velocity(&self) -> bool {
        matches!(
            self.source_sys(),
            SpecSystem::Vrel | SpecSystem::Vradio | SpecSystem::Voptical
        )
    }

    /// SourceVel as C's attribute reports it: km/s for a velocity, else the
    /// redshift or beta factor.
    pub fn source_vel_cur(&self) -> f64 {
        if self.source_vel_is_velocity() {
            self.source_vel() * 1.0e-3
        } else {
            self.source_vel()
        }
    }

    /// Set SourceVel from a value as C's attribute reads it: km/s for a
    /// velocity, else the redshift or beta factor.
    pub fn set_source_vel_cur(&mut self, value: f64) {
        let v = if self.source_vel_is_velocity() {
            value * 1.0e3
        } else {
            value
        };
        self.set_source_vel(v);
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

    /// Set the source velocity rest frame, re-expressing a set SourceVel in
    /// it (specframe.c:6134). The Source rest frame is not allowed.
    pub fn set_source_vrf(&mut self, sor: StdOfRest) -> AstResult<()> {
        if sor == StdOfRest::Source {
            return Err(AstError::new(
                ErrorCode::Attin,
                "astSetSourceVRF(SpecFrame): Bad value given for SourceVRF attribute.",
            ));
        }
        if self.test_source_vel() {
            self.source_vel = Some(self.convert_source_vel(sor, self.source_sys()));
        }
        self.source_vrf = Some(sor);
        Ok(())
    }

    /// Clear SourceVRF, re-expressing a set SourceVel as heliocentric
    /// (specframe.c:6126).
    pub fn clear_source_vrf(&mut self) {
        if self.test_source_vel() {
            self.source_vel =
                Some(self.convert_source_vel(StdOfRest::Heliocentric, self.source_sys()));
        }
        self.source_vrf = None;
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

    /// Set the source velocity spectral system, re-expressing a set
    /// SourceVel in it (specframe.c:6201). Only a velocity, a redshift or a
    /// beta factor is allowed.
    pub fn set_source_sys(&mut self, sys: SpecSystem) -> AstResult<()> {
        if !matches!(
            sys,
            SpecSystem::Vrel
                | SpecSystem::Beta
                | SpecSystem::Vradio
                | SpecSystem::Redshift
                | SpecSystem::Voptical
        ) {
            return Err(AstError::new(
                ErrorCode::Attin,
                "astSetSourceSys(SpecFrame): Bad value given for SourceSys attribute.",
            ));
        }
        if self.test_source_vel() {
            self.source_vel = Some(self.convert_source_vel(self.source_vrf(), sys));
        }
        self.source_sys = Some(sys);
        Ok(())
    }

    /// Clear SourceSys, re-expressing a set SourceVel as relativistic
    /// velocity (specframe.c:6194).
    pub fn clear_source_sys(&mut self) {
        if self.test_source_vel() {
            self.source_vel = Some(self.convert_source_vel(self.source_vrf(), SpecSystem::Vrel));
        }
        self.source_sys = None;
    }

    /// Test whether SourceSys has been explicitly set.
    pub fn test_source_sys(&self) -> bool {
        self.source_sys.is_some()
    }

    /// SpecOrigin in the default units of the current System, set or
    /// default (0).
    pub fn spec_origin(&self) -> f64 {
        self.spec_origin.unwrap_or(0.0)
    }

    /// Clear SpecOrigin.
    pub fn clear_spec_origin(&mut self) {
        self.spec_origin = None;
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
        self.attrs.epoch.unwrap_or(51544.5)
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
        self.attrs.epoch = Some(mjd);
    }

    /// Test whether Epoch has been explicitly set.
    pub fn test_epoch(&self) -> bool {
        self.attrs.epoch.is_some()
    }

    /// Set the epoch as a decimal year.
    ///
    /// Years strictly less than 1984.0 are interpreted as Besselian
    /// epochs; >= 1984.0 as Julian epochs (IAU 1984 rule). Mirrors
    /// `astReadDateTime` (frame.c:9028-9032) when called from
    /// `astSetEpoch` via the `epoch=…` setting.
    pub fn set_epoch_year(&mut self, year: f64) {
        self.attrs.epoch = Some(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the title.
    pub fn set_title(&mut self, title: &str) {
        self.attrs.title = Some(title.to_string());
    }

    /// Test whether Title has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.attrs.title.is_some()
    }

    /// Clear Title back to its (computed) default.
    pub fn clear_title(&mut self) {
        self.attrs.title = None;
    }

    /// Compute the default title from the system label and standard of rest.
    ///
    /// Format: "{label} ({sor})" — e.g. "Frequency (Heliocentric)".
    fn default_title(&self) -> String {
        // Mirrors C `SpecFrame::GetTitle` (specframe.c:2022): the capitalised
        // system label, the standard of rest in parentheses only when
        // explicitly set, and (for non-absolute systems) the rest frequency
        // in GHz when set or when defaults are usable (UseDefs defaults true).
        let mut title = self.system.label().to_string();
        if self.sor_set {
            title.push_str(&format!(" ({})", self.std_of_rest.as_str()));
        }
        if !self.system.is_absolute() {
            // C uses astGetRestFreq, whose default is 1.0E14 Hz.
            let rf_ghz = self.rest_freq.unwrap_or(1.0e14) * 1.0e-9;
            let rf_str = crate::serial::native_write::format_double_g(rf_ghz, 6);
            title.push_str(&format!(", rest frequency = {rf_str} GHz"));
        }
        title
    }

    /// Set the domain.
    pub fn set_domain(&mut self, domain: &str) {
        self.attrs.domain = Some(domain.to_string());
    }

    /// Test whether the domain attribute has been explicitly set.
    /// Mirrors C's `astTestDomain` for `SpecFrame`.
    pub fn test_domain(&self) -> bool {
        self.attrs.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default
    /// `"SPECTRUM"`.
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

    /// Set TAI-UTC (seconds).
    pub fn set_dtai(&mut self, dtai: f64) {
        self.attrs.dtai = Some(dtai);
    }

    /// Set UT1-UTC (seconds).
    pub fn set_dut1(&mut self, dut1: f64) {
        self.attrs.dut1 = Some(dut1);
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

    /// Clear AlignSpecOffset.
    pub fn clear_align_spec_offset(&mut self) {
        self.align_spec_offset = false;
        self.align_spec_offset_set = false;
    }

    // --- Conversion helpers ---

    /// SourceVel re-expressed in the rest frame `new_sor` and the system
    /// `new_sys`. Mirrors C `ConvertSourceVel` (specframe.c:745): an unset
    /// SourceVel and RestFreq take their defaults (0 and 1e14 Hz), and the
    /// conversion goes through relativistic velocity and frequency.
    fn convert_source_vel(&self, new_sor: StdOfRest, new_sys: SpecSystem) -> f64 {
        let vel = self.source_vel();
        let sor = self.source_vrf();
        let sys = self.source_sys();
        if sor == new_sor && sys == new_sys {
            return vel;
        }
        let mut from = self.clone();
        from.set_std_of_rest(sor);
        let mut to = self.clone();
        to.set_std_of_rest(new_sor);
        let mut specmap = SpecMap::new();
        match sys {
            SpecSystem::Vradio => specmap.add(SpecCvtType::VrToVl, &[]),
            SpecSystem::Voptical => specmap.add(SpecCvtType::VoToVl, &[]),
            SpecSystem::Redshift => specmap.add(SpecCvtType::ZoToVl, &[]),
            SpecSystem::Beta => specmap.add(SpecCvtType::BtToVl, &[]),
            _ => {}
        }
        let rf = self.rest_freq();
        specmap.add(SpecCvtType::VlToFr, &[rf]);
        Self::sor_convert(&from, &to, &mut specmap);
        specmap.add(SpecCvtType::FrToVl, &[rf]);
        match new_sys {
            SpecSystem::Vradio => specmap.add(SpecCvtType::VlToVr, &[]),
            SpecSystem::Voptical => specmap.add(SpecCvtType::VlToVo, &[]),
            SpecSystem::Redshift => specmap.add(SpecCvtType::VlToZo, &[]),
            SpecSystem::Beta => specmap.add(SpecCvtType::VlToBt, &[]),
            _ => {}
        }
        use crate::coords::PointSet;
        use crate::mappings::Mapping;
        let pts = PointSet::from_rows([[vel]]);
        match specmap.transform(&pts, true) {
            Ok(out) => out.point(0)[0],
            Err(_) => 0.0,
        }
    }

    /// SourceVel as heliocentric relativistic velocity, the form the Source
    /// rest frame conversions use (specframe.c `SorConvert`).
    fn convert_source_vel_to_helio_vrel(&self) -> f64 {
        self.convert_source_vel(StdOfRest::Heliocentric, SpecSystem::Vrel)
    }

    /// Add SpecMap steps to convert from a given SOR to heliocentric (frequency domain).
    fn add_sor_to_helio(&self, specmap: &mut SpecMap, from: StdOfRest) {
        let ra = self.ref_ra.unwrap_or(0.0);
        let dec = self.ref_dec.unwrap_or(0.0);
        let lon = self.attrs.obs_lon.unwrap_or(0.0);
        let lat = self.attrs.obs_lat.unwrap_or(0.0);
        let alt = self.attrs.obs_alt.unwrap_or(0.0);
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
        let lon = self.attrs.obs_lon.unwrap_or(0.0);
        let lat = self.attrs.obs_lat.unwrap_or(0.0);
        let alt = self.attrs.obs_alt.unwrap_or(0.0);
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

        // Mirror C `astGetRestFreq` default of 1.0E14 Hz when RestFreq
        // has not been set (specframe.c:6020). The actual value is
        // immaterial to the result because the chain is VlToFr → SOR
        // convert → FrToVl with the same `rf` on both ends — the rest
        // frequency cancels out — but a finite value is required for
        // SpecMap to evaluate.
        let rf = self.rest_freq.unwrap_or(1.0e14);

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

    /// Add SpecMap steps to convert from the current system to frequency
    /// (step 2 of `MakeSpecMapping`, specframe.c:2782). A velocity system
    /// goes through apparent radial velocity using `rest_freq`.
    fn add_system_to_freq(specmap: &mut SpecMap, system: SpecSystem, rest_freq: f64) {
        match system {
            SpecSystem::Freq => return,
            SpecSystem::Energy => return specmap.add(SpecCvtType::EnToFr, &[]),
            SpecSystem::Wavenum => return specmap.add(SpecCvtType::WnToFr, &[]),
            SpecSystem::Wavelen => return specmap.add(SpecCvtType::WvToFr, &[]),
            SpecSystem::Airwave => return specmap.add(SpecCvtType::AwToFr, &[]),
            SpecSystem::Vradio => specmap.add(SpecCvtType::VrToVl, &[]),
            SpecSystem::Voptical => specmap.add(SpecCvtType::VoToVl, &[]),
            SpecSystem::Redshift => specmap.add(SpecCvtType::ZoToVl, &[]),
            SpecSystem::Beta => specmap.add(SpecCvtType::BtToVl, &[]),
            SpecSystem::Vrel => {}
        }
        specmap.add(SpecCvtType::VlToFr, &[rest_freq]);
    }

    /// Add SpecMap steps to convert from frequency to the given system
    /// (step 7 of `MakeSpecMapping`, specframe.c:2851).
    fn add_freq_to_system(specmap: &mut SpecMap, system: SpecSystem, rest_freq: f64) {
        match system {
            SpecSystem::Freq => return,
            SpecSystem::Energy => return specmap.add(SpecCvtType::FrToEn, &[]),
            SpecSystem::Wavenum => return specmap.add(SpecCvtType::FrToWn, &[]),
            SpecSystem::Wavelen => return specmap.add(SpecCvtType::FrToWv, &[]),
            SpecSystem::Airwave => return specmap.add(SpecCvtType::FrToAw, &[]),
            _ => {}
        }
        specmap.add(SpecCvtType::FrToVl, &[rest_freq]);
        match system {
            SpecSystem::Vradio => specmap.add(SpecCvtType::VlToVr, &[]),
            SpecSystem::Voptical => specmap.add(SpecCvtType::VlToVo, &[]),
            SpecSystem::Redshift => specmap.add(SpecCvtType::VlToZo, &[]),
            SpecSystem::Beta => specmap.add(SpecCvtType::VlToBt, &[]),
            _ => {}
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
                    && ast_equal(
                        self.attrs.obs_alt.unwrap_or(0.0),
                        other.attrs.obs_alt.unwrap_or(0.0),
                    )
                    && ast_equal(
                        self.attrs.obs_lon.unwrap_or(0.0),
                        other.attrs.obs_lon.unwrap_or(0.0),
                    )
                    && ast_equal(
                        self.attrs.obs_lat.unwrap_or(0.0),
                        other.attrs.obs_lat.unwrap_or(0.0),
                    )
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
            let lon = this.attrs.obs_lon.unwrap_or(0.0);
            let lat = this.attrs.obs_lat.unwrap_or(0.0);
            let alt = this.attrs.obs_alt.unwrap_or(0.0);
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
            let lon = that.attrs.obs_lon.unwrap_or(0.0);
            let lat = that.attrs.obs_lat.unwrap_or(0.0);
            let alt = that.attrs.obs_alt.unwrap_or(0.0);
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

    /// Set SpecOrigin from a value in the SpecFrame's current units, as
    /// C's `SpecOrigin` attribute reads a number with no units
    /// (specframe.c:3893, `ToUnits`).
    pub fn set_spec_origin_cur(&mut self, value: f64) -> AstResult<()> {
        let def = self.system.default_display_units();
        let cur = Frame::unit(self, 1)?;
        let map = unit_mapper(&cur, def).ok_or_else(|| {
            AstError::new(
                ErrorCode::Badun,
                format!(
                    "astSetSpecOrigin(SpecFrame): Cannot convert the supplied attribute value from units of {cur} to {def}."
                ),
            )
        })?;
        let pts = crate::coords::PointSet::from_rows([[value]]);
        let v = crate::mappings::Mapping::transform(map.as_ref(), &pts, true)?.point(0)[0];
        self.set_spec_origin(v);
        Ok(())
    }

    /// The SpecOrigin value converted from its storage basis (the
    /// default units of the current System) to the frame's current
    /// units. Returns 0.0 when unset. Mirrors `GetSpecOriginCur`
    /// (specframe.c:2129).
    pub fn spec_origin_cur(&self) -> AstResult<f64> {
        let Some(origin) = self.spec_origin else {
            return Ok(0.0);
        };
        if origin == 0.0 || is_bad(origin) {
            return Ok(origin);
        }
        let def = self.system.default_display_units();
        let cur = self
            .axis
            .unit
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.system.default_display_units());
        if cur == def || def.is_empty() {
            return Ok(origin);
        }
        let map = unit_mapper(def, cur).ok_or_else(|| {
            AstError::new(
                ErrorCode::Badun,
                format!("SpecFrame: the current units ({cur}) are not suitable for a SpecFrame"),
            )
        })?;
        let pts = PointSet::from_columns(&[&[origin]])?;
        let out = map.transform(&pts, true)?;
        Ok(out.point(0)[0])
    }

    /// Convert a stored SpecOrigin between two attribute states of
    /// this frame (used when System or StdOfRest changes). Mirrors
    /// `OriginSystem` / `OriginStdOfRest` (specframe.c:3191-3380):
    /// the conversion runs between copies with Unit, SpecOrigin and
    /// AlignSpecOffset cleared, so it operates in the default units
    /// of each system.
    fn rescale_spec_origin<F>(&mut self, make_old: F)
    where
        F: FnOnce(&mut SpecFrame),
    {
        let Some(origin) = self.spec_origin else {
            return;
        };
        if is_bad(origin) {
            return;
        }
        let mut sf1 = self.clone();
        sf1.axis.unit = None;
        sf1.spec_origin = None;
        sf1.align_spec_offset = false;
        sf1.align_spec_offset_set = false;
        let mut sf2 = sf1.clone();
        make_old(&mut sf2);
        let Ok(Some(map)) = sf2.make_spec_mapping_full(&sf1, None, false, false) else {
            return;
        };
        let Ok(pts) = PointSet::from_columns(&[&[origin]]) else {
            return;
        };
        if let Ok(out) = map.transform(&pts, true) {
            let v = out.point(0)[0];
            if !is_bad(v) {
                self.spec_origin = Some(v);
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
        self.make_spec_mapping_full(result, None, false, true)?
            .ok_or_else(|| {
                AstError::new(
                    ErrorCode::Badun,
                    "astMatch(SpecFrame): no Mapping between the units",
                )
            })
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
        report: bool,
    ) -> AstResult<Option<DynMapping>> {
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

        // C lines 2629-2634: RestFreq with its default of 1.0E14 Hz.
        let target_rf = target.rest_freq();
        let result_rf = result.rest_freq();
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
            at.attrs.epoch = Some(target.epoch_mjd());
            at.ref_ra = Some(target.ref_ra.unwrap_or(0.0));
            at.ref_dec = Some(target.ref_dec.unwrap_or(0.0));
            at.source_vrf = Some(target.source_vrf());
            at.source_vel = Some(target.source_vel.unwrap_or(0.0));
            at.attrs.obs_lat = Some(target.attrs.obs_lat.unwrap_or(0.0));
            at.attrs.obs_lon = Some(target.attrs.obs_lon.unwrap_or(0.0));
            at.attrs.obs_alt = Some(target.attrs.obs_alt.unwrap_or(0.0));

            let mut ar = align_frm.clone();
            ar.attrs.epoch = Some(result.epoch_mjd());
            ar.ref_ra = Some(result.ref_ra.unwrap_or(0.0));
            ar.ref_dec = Some(result.ref_dec.unwrap_or(0.0));
            ar.source_vrf = Some(result.source_vrf());
            ar.source_vel = Some(result.source_vel.unwrap_or(0.0));
            ar.attrs.obs_lat = Some(result.attrs.obs_lat.unwrap_or(0.0));
            ar.attrs.obs_lon = Some(result.attrs.obs_lon.unwrap_or(0.0));
            ar.attrs.obs_alt = Some(result.attrs.obs_alt.unwrap_or(0.0));

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
        if align_system == SpecSystem::Freq || result_rf == target_rf {
            step4 = false;
            step5 = false;
        }

        // C line 2765.
        if !step4 && !step5 && target.equal_sor(result) {
            step3 = false;
            step6 = false;
        }

        // C lines 2771-2774: only a relative system with an unchanged rest
        // frequency can skip the conversion to and from frequency.
        if !step3
            && !step4
            && !step5
            && !step6
            && target.equal_sor(result)
            && target_system == result_system
            && !target_system.is_absolute()
            && result_rf == target_rf
        {
            step2 = false;
            step7 = false;
        }

        // Build the SpecMap.
        let mut specmap = SpecMap::new();

        // Step 2: target system → frequency in target rest frame
        // (C lines 2782-2819).
        if step2 {
            Self::add_system_to_freq(&mut specmap, target_system, target_rf);
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
            Self::add_freq_to_system(&mut specmap, result_system, result_rf);
        }

        // Steps 1 and 8: the SpecMap works in the default units of each
        // system, so bracket it with Mappings from the target's Unit and to
        // the result's (specframe.c:2891-2934).
        let specmap: DynMapping = if specmap.is_empty() {
            Box::new(UnitMap::new(1))
        } else {
            Box::new(specmap)
        };
        let in_unit = target.unit_or_default();
        let out_unit = result.unit_or_default();
        let umap1 = unit_mapper(in_unit, target_system.specmap_units());
        let umap2 = unit_mapper(result_system.specmap_units(), out_unit);
        let mut matched = true;
        let mut mapping: DynMapping = match (umap1, umap2) {
            (Some(umap1), Some(umap2)) => {
                let map1 = CmpMap::new(umap1, specmap, Composition::Series)?;
                Box::new(CmpMap::new(Box::new(map1), umap2, Composition::Series)?)
            }
            (umap1, _) => {
                // Without a units Mapping the Frames only match when no
                // spectral conversion is needed and the units are the same.
                let simplified = specmap.simplify()?;
                if !simplified.as_any().is::<UnitMap>() || in_unit != out_unit {
                    matched = false;
                    if report {
                        let (unit, system) = if umap1.is_none() {
                            (in_unit, target_system)
                        } else {
                            (out_unit, result_system)
                        };
                        return Err(AstError::new(
                            ErrorCode::Badun,
                            format!(
                                "astMatch(SpecFrame): Inappropriate units ({unit}) specified for a {} axis.",
                                system.label()
                            ),
                        ));
                    }
                }
                simplified
            }
        };

        // Step 0: offset to absolute value in the target system —
        // prepend a ShiftMap by the target origin (specframe.c:2937-2945).
        let target_origin = target.spec_origin_cur()?;
        if target_origin != 0.0 && !is_bad(target_origin) {
            let sm = ShiftMap::new(vec![target_origin]);
            mapping = Box::new(CmpMap::new(Box::new(sm), mapping, Composition::Series)?);
        }

        // Step 9: absolute value to offset in the result system. When
        // both frames align in the offset system, the new zero point is
        // the transformed target origin; otherwise the result frame's
        // own origin (specframe.c:2947-2972).
        let result_origin = if target.align_spec_offset() && result.align_spec_offset() {
            let pts = PointSet::from_columns(&[&[0.0]])?;
            let out = mapping.transform(&pts, true)?;
            out.point(0)[0]
        } else {
            result.spec_origin_cur()?
        };
        if result_origin != 0.0 && !is_bad(result_origin) {
            let sm = ShiftMap::new(vec![-result_origin]);
            mapping = Box::new(CmpMap::new(mapping, Box::new(sm), Composition::Series)?);
        }

        if !matched {
            return Ok(None);
        }
        mapping.simplify().map(Some)
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for SpecFrame {
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

    fn get_active_unit(&self) -> bool {
        true // SpecFrame always uses active units (frame.c:9882).
    }

    fn set_active_unit(&mut self, _value: bool) {}

    fn test_active_unit(&self) -> bool {
        false
    }

    /// Mirrors C `SpecFrame::GetLabel` (specframe.c:1520): a Label that
    /// has been set, else the System's name, followed by the SpecOrigin
    /// when there is one.
    fn label(&self, axis: usize) -> AstResult<String> {
        if let Some(label) = &self.ax(axis)?.label {
            return Ok(label.clone());
        }
        let mut label = self.system.label().to_string();
        let origin = self.spec_origin_cur()?;
        if origin != 0.0 {
            label.push_str(&format!(" offset from {}", self.format(axis, origin)?));
        }
        Ok(super::frame_attrs::relabel_for_unit(
            label,
            self.system.default_display_units(),
            self.axis.unit.as_deref(),
        ))
    }

    /// Mirrors C `SpecFrame::GetSymbol` (specframe.c:1770): a Symbol that
    /// has been set, else the System's FITS code.
    fn symbol(&self, axis: usize) -> AstResult<String> {
        if let Some(symbol) = &self.ax(axis)?.symbol {
            return Ok(symbol.clone());
        }
        Ok(super::frame_attrs::relabel_for_unit(
            self.system.as_str().to_string(),
            self.system.default_display_units(),
            self.axis.unit.as_deref(),
        ))
    }

    /// Mirrors C `SpecFrame::GetUnit` (specframe.c:2220): a Unit that has
    /// been set, else the System's default unit.
    fn unit(&self, axis: usize) -> AstResult<String> {
        Ok(self
            .ax(axis)?
            .unit
            .clone()
            .unwrap_or_else(|| self.system.default_display_units().to_string()))
    }

    /// Mirrors C `SpecFrame::SetUnit` (specframe.c:4167), which also records
    /// the Unit as the one used with the current System, so that it comes
    /// back when the System does.
    fn set_unit(&mut self, axis: usize, unit: &str) -> AstResult<()> {
        self.ax(axis)?;
        let old = self.unit_or_default().to_string();
        self.store_unit(unit);
        // The Frame class's SetUnit follows (frame.c:10775).
        if self.get_active_unit() {
            let new = unit.trim_end().to_string();
            super::frame_attrs::new_unit(&mut self.axis, &old, &new);
        }
        Ok(())
    }

    /// Mirrors C `SpecFrame::ClearUnit` (specframe.c:691), which also
    /// forgets the Unit used with the current System.
    fn clear_unit(&mut self, axis: usize) -> AstResult<()> {
        self.ax(axis)?;
        self.used_units[self.system.used_units_index()] = None;
        // The Frame class's ClearUnit follows (frame.c:2527).
        if let Some(old) = self.axis.unit.take() {
            if self.get_active_unit() {
                let new = self.unit_or_default().to_string();
                super::frame_attrs::new_unit(&mut self.axis, &old, &new);
            }
        }
        Ok(())
    }

    fn test_unit(&self, axis: usize) -> AstResult<bool> {
        Ok(self.ax(axis)?.unit.is_some())
    }

    axis_attrs_base_get!(format, digits, direction, top, bottom);
    axis_attrs_store!(label, symbol, format, digits, direction, top, bottom);

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        Ok(super::cartesian_distance(&a[..1], &b[..1]))
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
        Ok(super::cartesian_offset(&from[..1], &to[..1], dist))
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

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); SpecFrame
        // does not override it. A difference in System/StdOfRest/Epoch/
        // RestFreq yields a non-UnitMap spectral conversion and so the
        // frames compare unequal.
        super::find_frame::frames_equal(self, other)
    }

    fn system_string(&self) -> Option<&str> {
        Some(self.system.as_str())
    }

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (specframe.c:2998 / 4705 / 3381).
    // -----------------------------------------------------------------

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        specframe_overlay(self, template_axes, result)
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
        let t = template.downcast_ref::<SpecFrame>().or_else(|| {
            template
                .downcast_ref::<crate::frames::DSBSpecFrame>()
                .map(|d| d.spec())
        });
        if let Some(t) = t {
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
        spec_match(self, target, match_sub)
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
pub(crate) fn primary_frame_walk(frame: &dyn Frame, axis: usize) -> (&dyn Frame, usize) {
    frame.primary_frame(axis)
}

/// SpecFrame `Match` (specframe.c:2998), with the template passed as a
/// Frame so that DSBSpecFrame, a SpecFrame in C, can use it as its parent
/// method and still hand itself to the target's SubFrame.
pub(crate) fn spec_match(
    template: &dyn Frame,
    target: &dyn Frame,
    match_sub: bool,
) -> AstResult<Option<super::find_frame::MatchResult>> {
    // Mirrors specframe.c:2998. Step 1: defer to the parent (Frame
    // class) Match for the axis-count + Domain pre-checks. The
    // axis association arrays returned by the parent are
    // discarded — SpecFrame is a 1-axis class so we know exactly
    // what they should be.
    let parent = super::find_frame::default_match(template, target, match_sub)?;
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
        // Mirrors C `astIsASpecFrame(frame0)` (specframe.c:3147),
        // which is true for SpecFrame and any subclass (DSBSpecFrame).
        // The Rust DSBSpecFrame is a distinct struct embedding a
        // SpecFrame, so test both class identities.
        let is_specframe = primary.as_any().downcast_ref::<SpecFrame>().is_some()
            || primary
                .as_any()
                .downcast_ref::<crate::frames::DSBSpecFrame>()
                .is_some();
        if is_specframe {
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
        target.sub_frame(Some(template), 1, &target_axes_vec, &template_axes_vec)?
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

/// `frame` as a SpecFrame, when it is one or is a DSBSpecFrame: the Rust
/// equivalent of C's `astIsASpecFrame`, which is true for the subclass too.
pub(crate) fn as_spec_frame(frame: &dyn Frame) -> Option<&SpecFrame> {
    frame.as_any().downcast_ref::<SpecFrame>().or_else(|| {
        frame
            .as_any()
            .downcast_ref::<crate::frames::DSBSpecFrame>()
            .map(|d| d.spec())
    })
}

/// `frame` as a mutable SpecFrame, when it is one or is a DSBSpecFrame.
pub(crate) fn as_spec_frame_mut(frame: &mut dyn Frame) -> Option<&mut SpecFrame> {
    if frame.as_any().is::<SpecFrame>() {
        return frame.as_any_mut().downcast_mut::<SpecFrame>();
    }
    frame
        .as_any_mut()
        .downcast_mut::<crate::frames::DSBSpecFrame>()
        .map(|d| d.spec_mut())
}

/// C's SpecFrame `Overlay` (specframe.c:3381), a method of the template.
///
/// When `result` is a SpecFrame (or DSBSpecFrame) whose System the template
/// sets to a new value, its Title, Label and Symbol are cleared, and its Unit
/// when the two Systems' default units differ. The Frame-class part follows,
/// which sets the System on a SpecFrame result only, and then the SpecFrame
/// attributes.
fn specframe_overlay(
    template_spec: &SpecFrame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    if let Some(result_spec) = as_spec_frame_mut(result) {
        let old_system = result_spec.system;
        let new_system = template_spec.system;
        if new_system != old_system && template_spec.test_system() {
            result_spec.clear_title();
            if new_system.default_display_units() != old_system.default_display_units() {
                result_spec.clear_unit(1)?;
            }
            result_spec.axis.label = None;
            result_spec.axis.symbol = None;
        }
    }

    super::overlay::frame_overlay(template_spec, template_axes, result)?;

    if let Some(result_spec) = as_spec_frame_mut(result) {
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
            result_spec.set_source_sys(template_spec.source_sys())?;
        }
        if template_spec.test_source_vrf() {
            result_spec.set_source_vrf(template_spec.source_vrf())?;
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
        // MakeSpecMapping reports why two SpecFrames cannot be aligned
        // only when the template is itself a SpecFrame (specframe.c:4865).
        let mut report = false;
        let align_frm = if let Some(t) = template {
            t.overlay(template_axes, &mut result)?;
            if let Some(template_spec) = as_spec_frame(t) {
                report = true;
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
        let intflag = target.frame_flags() & super::INTFLAG != 0;
        let Some(mapping) =
            target.make_spec_mapping_full(&result, Some(&align_frm), intflag, report)?
        else {
            return Ok(None);
        };

        let result_box: DynFrame = Box::new(result);
        return Ok(Some((mapping, result_box)));
    }

    // Result is not a SpecFrame (specframe.c:4905-4951). Make a copy,
    // explicitly set Frame-level / per-axis defaults, clear System and
    // AlignSystem (since their values are SpecFrame-specific), then
    // defer to the default Frame-level SubFrame.
    let mut temp = target.clone();
    super::find_frame::set_class_defaults(&mut temp)?;
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

        // --- Object-level attributes (before the frame attributes) ---
        // Ident is written if explicitly set, followed by an `IsA Object`
        // marker. Mirrors the Object Dump that precedes the Frame Dump (the
        // `IDENT`/`IsA Object` block in the reference native dumps).
        if let Some(ref ident) = self.attrs.ident {
            node.push_string(
                "Ident",
                ident,
                Some("Permanent Object identification string".to_string()),
            );
            node.push_isa("Object", Some("AST Object".to_string()));
        }

        // The Frame class's part, read through the SpecFrame's getters
        // (frame.c:14250).
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

        // Ax1: the Axis, with the attributes that are set (axis.c Dump).
        let mut axis_node = Node::new("Axis");
        axis_node.class_comment = Some("Coordinate axis".to_string());
        self.axis.write_set_attributes(&mut axis_node);
        node.push_object("Ax1", axis_node, Some("Axis number 1".to_string()));

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- SpecFrame-specific attributes (after IsA Frame) ---

        // SoR (StdOfRest): C writes it with helpful=1 (specframe.c Dump),
        // so a set value is active and an unset one is a commented default
        // (the default StdOfRest is Heliocentric).
        if self.sor_set {
            node.push_string(
                "SoR",
                self.std_of_rest.as_str(),
                Some("Standard of rest".to_string()),
            );
        } else {
            node.push_unset(
                "SoR",
                NodeValue::String(self.std_of_rest.as_str().to_string()),
                Some("Standard of rest".to_string()),
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

        // UsedUnits block: one `U<system>` card per system that has a
        // recorded used unit, in C's AstSystemType index order
        // (specframe.c:6681-6692, after AlSpOf and before SpOrg). C's
        // SetUnit keeps `usedunits[current]` in step with the current Unit,
        // so the current system's entry reflects `self.axis.unit`.
        for sys in SpecSystem::ALL {
            let stored = if sys == self.system {
                self.axis.unit.clone()
            } else {
                self.used_units[sys.used_units_index()].clone()
            };
            if let Some(unit) = stored {
                node.push_string(
                    sys.unit_attr_key(),
                    &unit,
                    Some(sys.unit_attr_comment().to_string()),
                );
            }
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
        // Read System. The unset default is WAVELEN (specframe.c:2015),
        // so an absent `System=` line decodes to a WAVELEN frame with
        // system_set false.
        let sys_str = node.get_string("System", SpecSystem::Wavelen.as_str());
        let system = SpecSystem::parse(&sys_str).unwrap_or(SpecSystem::Wavelen);
        let system_present = node.get("System").is_some();

        let mut sf = SpecFrame::new(system);
        // SpecFrame::new flags System as set; honour the serialised
        // set-ness instead (a commented `System=` line means unset).
        sf.system_set = system_present;

        // The Frame-class attributes (frame.c `astLoadFrame`), then the
        // GeoLat and GeoLon items older versions of AST wrote in place of
        // ObsLat and ObsLon, read only when those are absent (specframe.c:7097).
        sf.attrs = super::frame_attrs::read_frame_attrs(node);
        if sf.attrs.obs_lat.is_none() && node.get("GeoLat").is_some() {
            sf.attrs.obs_lat = Some(node.get_double("GeoLat", 0.0));
        }
        if sf.attrs.obs_lon.is_none() && node.get("GeoLon").is_some() {
            sf.attrs.obs_lon = Some(node.get_double("GeoLon", 0.0));
        }

        // SoR (StdOfRest).
        let sor_str = node.get_string("SoR", "Heliocentric");
        if let Some(sor) = StdOfRest::parse(&sor_str) {
            sf.std_of_rest = sor;
            if node.get("SoR").is_some() {
                sf.sor_set = true;
            }
        }

        // UsedUnits block: restore every `U<system>` card into the per-system
        // map (C astLoad populates `usedunits[]`). Card presence — not
        // emptiness — marks a set unit, so a dimensionless "" round-trips.
        for sys in SpecSystem::ALL {
            if node.get(sys.unit_attr_key()).is_some() {
                sf.used_units[sys.used_units_index()] =
                    Some(node.get_string(sys.unit_attr_key(), ""));
            }
        }

        // The current Unit comes from the current system's used unit if one
        // was recorded, else the per-axis `Uni1` (CmpFrame override
        // forwarding) or the Axis child Unit card. C's SetSystem re-instates
        // `usedunits[system]` as the current Unit; a system with no recorded
        // used unit (e.g. a20070718's VRAD, which only carries UFreq) keeps
        // the system default.
        let axis_unit_card = node.get_object("Ax1").and_then(|ax| {
            ax.get("Unit")
                .map(|_| ax.get_string("Unit", "").to_string())
        });
        if let Some(axis_node) = node.get_object("Ax1") {
            sf.axis = super::axis::Axis::read(axis_node);
        }
        if let Some(u) = &sf.used_units[system.used_units_index()] {
            sf.axis.unit = Some(u.clone());
        } else if node.get("Uni1").is_some() {
            sf.axis.unit = Some(node.get_string("Uni1", ""));
        } else if let Some(u) = axis_unit_card {
            sf.axis.unit = Some(u);
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
        assert!(target.test_system());
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0], result.as_mut()).unwrap();
        let r = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.system(), SpecSystem::Vradio);
    }

    #[test]
    fn overlay_template_skips_unset_attrs() {
        // A template whose System has been cleared (System unset,
        // defaulting to WAVELEN) must not overlay System onto the
        // result. Overlaying onto a VRAD target leaves it VRAD.
        let mut template = SpecFrame::new(SpecSystem::Freq);
        template.clear_system();
        assert!(!template.test_system());
        let target = SpecFrame::new(SpecSystem::Vradio);
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0], result.as_mut()).unwrap();
        let r = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.system(), SpecSystem::Vradio);
    }

    #[test]
    fn overlay_template_copies_rest_freq_when_set() {
        let mut template = SpecFrame::new(SpecSystem::Freq);
        template.set_rest_freq(1.420405751e9);
        let target = SpecFrame::new(SpecSystem::Freq);
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0], result.as_mut()).unwrap();
        let r = result.as_any().downcast_ref::<SpecFrame>().unwrap();
        assert_eq!(r.rest_freq(), 1.420405751e9);
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
        template.set_max_axes(2).unwrap();
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

    #[test]
    fn convert_through_generic_driver_transfers_system() {
        // Regression for the value-derived System set-tracking bug: a
        // SpecFrame built with an explicit System (VRAD here, m/s) must
        // report System set, so the generic ConvertX Match/Overlay path
        // overlays its System onto the common alignment frame. Before the
        // fix, `SpecFrame::new(Freq)` reported System unset, the result
        // SpecFrame's Hz unit was checked against the WAVELEN default, and
        // a hard `AST__BADUN` (Unit 'Hz' incompatible with 'm/s')
        // resulted.
        //
        // Ground truth from the C oracle (libast astConvert on the same
        // two SpecFrames, RestFreq=1.0E9 Hz, StdOfRest=Topocentric):
        //   VRAD 1000 m/s -> FREQ 999996664.359048 Hz
        use crate::frames::find_frame::convert_frames;

        let mut vrad = SpecFrame::new(SpecSystem::Vradio);
        vrad.set_unit(1, "m/s").unwrap();
        vrad.set_rest_freq(1.0e9);
        vrad.set_std_of_rest(StdOfRest::Topocentric);
        assert!(vrad.test_system());

        let mut freq = SpecFrame::new(SpecSystem::Freq);
        freq.set_unit(1, "Hz").unwrap();
        freq.set_rest_freq(1.0e9);
        freq.set_std_of_rest(StdOfRest::Topocentric);
        assert!(freq.test_system());

        let fs = convert_frames(&vrad, &freq, "")
            .expect("convert must not error with BADUN")
            .expect("a VRAD->FREQ conversion must exist");

        let map = fs
            .mapping(
                crate::frames::FrameRef::Base,
                crate::frames::FrameRef::Current,
            )
            .unwrap();
        let pts = PointSet::from_rows([[1000.0]]);
        let out = map.transform(&pts, true).unwrap();
        let f_hz = out.point(0)[0];
        assert!(
            (f_hz - 999_996_664.359_048).abs() < 1.0e-3,
            "VRAD 1000 m/s -> FREQ should be 999996664.359048 Hz, got {f_hz}"
        );
    }

    #[test]
    fn set_ref_pos_converts_from_fk4_b1950() {
        // RefRA/RefDec are stored in FK5 J2000, so a position supplied in
        // FK4 B1950 must actually be precessed. Golden values are the
        // `REFRA_B` / `REFDEC_B` cards of the `fits-pc-alt` wcsconv
        // reference, whose alternate plane describes RA/Dec = 180/45 deg
        // in FK4 B1950.
        let mut fk4 = crate::frames::SkyFrame::new(crate::frames::SkySystem::Fk4);
        fk4.set_equinox_year(1950.0);
        fk4.set_epoch_year(1950.0);

        let mut spec = SpecFrame::new(SpecSystem::Wavelen);
        spec.set_ref_pos(Some(&fk4), 180.0_f64.to_radians(), 45.0_f64.to_radians())
            .unwrap();

        let ra = spec.ref_ra();
        let dec = spec.ref_dec();
        assert!(
            (ra - 3.152_747_005_821_885).abs() < 1.0e-12,
            "RefRA should be 3.1527470058218849, got {ra}"
        );
        assert!(
            (dec - 0.780_540_387_333_539_5).abs() < 1.0e-12,
            "RefDec should be 0.78054038733353948, got {dec}"
        );
    }
}
