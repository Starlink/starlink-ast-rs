//! The flux coordinate frame (FluxFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame};
use crate::mappings::helpers::is_bad;
use crate::mappings::{DynMapping, UnitMap};
use crate::serial::{Node, Serializable};

// ---------------------------------------------------------------------------
// FluxSystem — flux coordinate system type
// ---------------------------------------------------------------------------

/// Flux coordinate system type.
///
/// Each variant corresponds to a way of measuring signal level (flux density
/// or surface brightness), either per unit frequency or per unit wavelength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FluxSystem {
    /// Flux density per unit frequency (e.g., Jy, W/m^2/Hz).
    FluxDen,
    /// Flux density per unit wavelength (e.g., W/m^2/Angstrom).
    FluxDenW,
    /// Surface brightness per unit frequency (e.g., W/m^2/Hz/arcmin^2).
    SBright,
    /// Surface brightness per unit wavelength (e.g., W/m^2/Angstrom/arcmin^2).
    SBrightW,
}

impl FluxSystem {
    /// Return the canonical string representation (as used in serialization).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FluxDen => "FLXDN",
            Self::FluxDenW => "FLXDNW",
            Self::SBright => "SFCBR",
            Self::SBrightW => "SFCBRW",
        }
    }

    /// Parse a flux system name, the one [`as_str`](Self::as_str) gives,
    /// ignoring case and trailing white space.
    /// `"SRCBR"` is also accepted for [`SBrightW`](Self::SBrightW).
    // C: fluxframe.c `SystemCode`, which matches with `astChrMatch` and
    // accepts "SRCBR", the only name it read for SFCBRW before upstream
    // b4457da9.
    pub fn parse(s: &str) -> Option<Self> {
        match crate::mappings::helpers::chr_match_key(s).as_str() {
            "FLXDN" => Some(Self::FluxDen),
            "FLXDNW" => Some(Self::FluxDenW),
            "SFCBR" => Some(Self::SBright),
            "SFCBRW" | "SRCBR" => Some(Self::SBrightW),
            _ => None,
        }
    }

    /// Default units for this flux system.
    pub fn default_units(&self) -> &'static str {
        match self {
            Self::FluxDen => "W/m^2/Hz",
            Self::FluxDenW => "W/m^2/Angstrom",
            Self::SBright => "W/m^2/Hz/arcmin**2",
            Self::SBrightW => "W/m^2/Angstrom/arcmin**2",
        }
    }

    /// Human-readable label for this system, which is the default Label and
    /// starts the default Title.
    // C: fluxframe.c `SystemLabel` (3212), with its first letter
    // capitalized as C's default Label and Title have it.
    pub fn label(&self) -> &'static str {
        match self {
            Self::FluxDen => "Flux density",
            Self::FluxDenW => "Flux wavelength density",
            Self::SBright => "Surface brightness",
            Self::SBrightW => "Surface brightness (per wavelength)",
        }
    }

    /// Every system, in the order of C's `AST__FLUXDEN` to `AST__SBRIGHTW`.
    pub(crate) const ALL: [Self; 4] =
        [Self::FluxDen, Self::FluxDenW, Self::SBright, Self::SBrightW];

    /// Position in [`Self::ALL`].
    pub(crate) fn index(&self) -> usize {
        Self::ALL.iter().position(|s| s == self).unwrap_or(0)
    }

    /// The name of the `UsedUnits` dump card for this system: `U` and the
    /// System string with all but its first letter in lower case
    /// (fluxframe.c Dump).
    fn used_units_key(&self) -> String {
        let name = self.as_str();
        format!("U{}{}", &name[..1], name[1..].to_ascii_lowercase())
    }

    /// Symbol for this system.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::FluxDen => "S_nu",
            Self::FluxDenW => "S_lambda",
            Self::SBright => "mu_nu",
            Self::SBrightW => "mu_lambda",
        }
    }

    /// Whether this system measures per unit frequency (true) or per unit wavelength (false).
    pub fn is_per_frequency(&self) -> bool {
        matches!(self, Self::FluxDen | Self::SBright)
    }

    /// Whether this system is a surface-brightness system (`SBright` /
    /// `SBrightW`) rather than a flux-density system (`FluxDen` /
    /// `FluxDenW`).
    // C: the `sb_in`/`sb_out` flags in fluxframe.c `MakeFluxMapping`.
    pub fn is_surface_brightness(&self) -> bool {
        matches!(self, Self::SBright | Self::SBrightW)
    }

    /// The corresponding flux-density system, collapsing a surface-brightness
    /// system onto its flux-density counterpart (`SBright` → `FluxDen`,
    /// `SBrightW` → `FluxDenW`).
    // C: the `sys_in`/`sys_out` mapping in fluxframe.c `MakeFluxMapping`
    // (2086-2112).
    pub fn density_flux_system(&self) -> Self {
        match self {
            Self::SBright => Self::FluxDen,
            Self::SBrightW => Self::FluxDenW,
            other => *other,
        }
    }

    /// The spectral [`SpecSystem`](crate::frames::SpecSystem) describing
    /// this flux system's density axis: frequency for "per Hz" systems,
    /// wavelength for "per Angstrom" systems.
    // C: fluxframe.c `DensitySystem` (537).
    pub fn density_spec_system(&self) -> super::specframe::SpecSystem {
        if self.is_per_frequency() {
            super::specframe::SpecSystem::Freq
        } else {
            super::specframe::SpecSystem::Wavelen
        }
    }

    /// The default unit for this flux system's density axis: "Hz" for "per
    /// frequency" systems, "Angstrom" for "per wavelength" systems.
    // C: fluxframe.c `DensityUnit` (602).
    pub fn density_unit(&self) -> &'static str {
        if self.is_per_frequency() {
            "Hz"
        } else {
            "Angstrom"
        }
    }
}

// ---------------------------------------------------------------------------
// FluxFrame
// ---------------------------------------------------------------------------

/// A measured flux description.
///
/// A FluxFrame is a specialized form of one-dimensional Frame which
/// represents various systems used to represent the signal level in an
/// observation.
/// The particular system is its [`system`](FluxFrame::system), qualified as
/// necessary by other attributes such as the units.
///
/// All flux values are assumed to be measured at the same frequency or
/// wavelength, the [`spec_val`](FluxFrame::spec_val), so this class is more
/// appropriate for use with images than with spectra.
///
/// The axis attributes of the Frame trait take axis index 1, the only axis.
/// A FluxFrame always uses active units (see
/// [`set_active_unit`](Frame::set_active_unit)), so a difference in the
/// Unit of two FluxFrames is taken into account when they are converted.
/// Its Domain defaults to `"FLUX"`, and its Title to the System's
/// [`label`](FluxSystem::label), followed by `" at = <SpecVal> <unit>"`
/// when a SpecVal is known, as in `"Flux density at = 1000 GHz"`.
///
/// # Attributes
///
/// In addition to the attributes common to all Frames, a FluxFrame has:
///
/// - [`spec_val`](FluxFrame::spec_val): the spectral position at which the
///   flux values are measured.
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, FluxFrame, FluxSystem};
///
/// let ff = FluxFrame::new(FluxSystem::FluxDen);
/// assert_eq!(ff.naxes(), 1);
/// assert_eq!(ff.system().unwrap(), FluxSystem::FluxDen);
/// ```
#[derive(Debug, Clone)]
pub struct FluxFrame {
    /// The Frame-class attributes.
    attrs: super::frame_attrs::FrameAttrs,
    // Frame-level attributes

    // FluxFrame-specific
    system: FluxSystem,
    /// Whether `system` was set explicitly (mirrors C `astTestSystem`).
    system_set: bool,
    /// AlignSystem, when set; the default is flux density
    /// (fluxframe.c `GetAlignSystem`).
    align_system: Option<FluxSystem>,
    /// The Unit last used with each System, in [`FluxSystem::ALL`] order,
    /// as C's `usedunits` array records it (fluxframe.c `SetUnit`).
    used_units: [Option<String>; 4],
    /// The Axis-class attributes of the single axis. A Unit of `None`
    /// means the system default units are used.
    axis: super::axis::Axis,
    spec_val: Option<f64>, // spectral position at which flux is measured
    /// Optional encapsulated SpecFrame defining the coordinate system of
    /// `spec_val` (mirrors C `this->specframe`, fluxframe.c). When `None`,
    /// SpecVal is interpreted directly in the FluxFrame's density system
    /// (the default `GetSpecFrame` path). Required for a meaningful
    /// cross-density-system flux conversion.
    specframe: Option<Box<super::specframe::SpecFrame>>,
    /// The SpecVal used when none is set, given when the FluxFrame is made
    /// (C's `defspecval`), or `BAD_VALUE`.
    def_spec_val: f64,
    // Object-level attributes

    // Match-control attributes (used by `astFindFrame` / `astMatch`).
}

impl FluxFrame {
    // Defaults of the Frame-class attributes, for the `frame_attrs_*`
    // macros.

    /// The System label, capitalized, followed by the SpecVal when one is
    /// set (fluxframe.c `GetTitle`).
    fn frame_default_title(&self) -> AstResult<String> {
        let mut title = self.system()?.label().to_string();
        if let Some(first) = title.get(..1) {
            title = first.to_ascii_uppercase() + &title[1..];
        }
        let specval = self.spec_val();
        if specval != BAD_VALUE {
            let sf = self.get_spec_frame()?;
            let sv = Frame::format(&sf, 1, specval)?;
            let su = Frame::unit(&sf, 1)?;
            title.push_str(&format!(" at = {sv} {su}"));
        }
        Ok(title)
    }
    fn frame_default_domain(&self) -> &'static str {
        "FLUX"
    }
    fn frame_default_epoch(&self) -> f64 {
        super::frame_attrs::default_epoch_mjd()
    }
    fn frame_default_permute(&self) -> bool {
        true
    }

    /// Create a FluxFrame in the flux system `system`, with no SpecVal.
    ///
    /// `system` is set as the System unless it is
    /// [`FluxDen`](FluxSystem::FluxDen), the default, which leaves the System
    /// unset, as [`unset_system`](FluxFrame::unset_system) does, so that it
    /// follows a Unit that is set.
    /// [`with_spec_val`](FluxFrame::with_spec_val) also gives the spectral
    /// position.
    ///
    /// When two FluxFrames are converted with
    /// [`convert_to`](Frame::convert_to), account is taken of the flux
    /// systems they represent, of their AlignSystem and other attributes,
    /// and of any difference in their Units.
    #[doc(alias = "astFluxFrame")]
    pub fn new(system: FluxSystem) -> Self {
        Self {
            attrs: super::frame_attrs::FrameAttrs::default(),
            // None = use the default "FLUX"; mirrors astTestDomain.
            system,
            // C's astFluxFrame has no System argument, so FLXDN, the
            // default, is left unset, as `astFluxFrame( specval, specfrm,
            // "" )` leaves it; any other System is set, as an options
            // string "System=..." sets it.
            system_set: !matches!(system, FluxSystem::FluxDen),
            axis: super::axis::Axis::default(),
            spec_val: None,
            specframe: None,
            def_spec_val: BAD_VALUE,
            align_system: None,
            used_units: Default::default(),
        }
    }

    /// A FluxFrame with no attribute set and no SpecVal.
    #[doc(alias = "astFluxFrame")]
    pub fn unset_system() -> Self {
        Self::new(FluxSystem::FluxDen)
    }

    /// Create a FluxFrame measured at the spectral position `spec_val`,
    /// given in the spectral system and units of `spec_frame`.
    ///
    /// `spec_val` becomes the default [`spec_val`](FluxFrame::spec_val)
    /// rather than a set one, and the System is unset, so it follows a Unit
    /// when one is set.
    /// `spec_val` may be `BAD_VALUE` if the spectral position is unknown,
    /// but then [`convert_to`](Frame::convert_to) may find no conversion to
    /// another FluxFrame.
    /// With no `spec_frame`, `spec_val` is a frequency in Hz for a "per Hz"
    /// System and a wavelength in Angstrom for a "per Angstrom" one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{Frame, FluxFrame, FluxSystem, SpecFrame, SpecSystem};
    ///
    /// let ff = FluxFrame::with_spec_val(1000.0, Some(SpecFrame::new(SpecSystem::Freq)));
    /// assert_eq!(ff.spec_val(), 1000.0);
    /// assert!(!ff.test_spec_val());
    /// assert_eq!(ff.system().unwrap(), FluxSystem::FluxDen);
    /// assert_eq!(ff.title().unwrap(), "Flux density at = 1000 GHz");
    /// ```
    #[doc(alias = "astFluxFrame")]
    // C: fluxframe.c `astInitFluxFrame`.
    pub fn with_spec_val(spec_val: f64, spec_frame: Option<super::specframe::SpecFrame>) -> Self {
        let mut f = Self::unset_system();
        f.def_spec_val = spec_val;
        f.specframe = spec_frame.map(Box::new);
        f
    }

    /// The flux system in which to align FluxFrames: the AlignSystem
    /// attribute, set or default.
    ///
    /// Default: [`FluxSystem::FluxDen`].
    pub fn align_system(&self) -> FluxSystem {
        self.align_system.unwrap_or(FluxSystem::FluxDen)
    }

    /// Set AlignSystem; see [`align_system`](FluxFrame::align_system).
    pub fn set_align_system(&mut self, sys: FluxSystem) {
        self.align_system = Some(sys);
    }

    /// Whether AlignSystem is set; see
    /// [`align_system`](FluxFrame::align_system).
    pub fn test_align_system(&self) -> bool {
        self.align_system.is_some()
    }

    /// Clear AlignSystem; see [`align_system`](FluxFrame::align_system).
    pub fn clear_align_system(&mut self) {
        self.align_system = None;
    }

    /// Whether SpecVal is set; see [`spec_val`](FluxFrame::spec_val).
    pub fn test_spec_val(&self) -> bool {
        self.spec_val.is_some()
    }

    /// Whether the System is set; see [`system`](FluxFrame::system).
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear the System; see [`system`](FluxFrame::system).
    ///
    /// When this changes the System, the Unit last used with the new System
    /// is re-instated, and the Label, Symbol and Title are cleared.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badun` when the System before or after follows from a
    ///   Unit that fits no flux system.
    pub fn clear_system(&mut self) -> AstResult<()> {
        let old = self.system()?;
        self.system_set = false;
        self.system_changed(old)
    }

    /// The Title, set or default; see [`Frame::title`] and, for the
    /// default, [`FluxFrame`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badun` when the Title is not set and the System follows
    ///   from a Unit that fits no flux system.
    pub fn title(&self) -> AstResult<String> {
        Frame::title(self)
    }

    /// Whether the Title is set; see [`Frame::title`].
    pub fn test_title(&self) -> bool {
        self.attrs.title.is_some()
    }

    /// Clear the Title; see [`Frame::title`].
    pub fn clear_title(&mut self) {
        self.attrs.title = None;
    }

    // --- FluxFrame-specific attributes ---

    /// The Ident, or an empty string when it is not set; see
    /// [`Frame::ident`].
    pub fn ident(&self) -> &str {
        self.attrs.ident.as_deref().unwrap_or("")
    }

    /// Set the Ident; see [`Frame::ident`].
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.attrs.ident = Some(s);
    }

    /// The spectral system the flux is a density in: frequency for a "per
    /// Hz" System, wavelength for a "per Angstrom" one.
    ///
    /// # Errors
    ///
    /// Passes on the error of [`system`](FluxFrame::system).
    // C: fluxframe.c `GetDensitySystem`.
    pub fn density_system(&self) -> AstResult<super::specframe::SpecSystem> {
        Ok(self.system()?.density_spec_system())
    }

    /// The default unit of the spectral system the flux is a density in:
    /// "Hz" or "Angstrom".
    ///
    /// # Errors
    ///
    /// Passes on the error of [`system`](FluxFrame::system).
    // C: fluxframe.c `GetDensityUnit`.
    pub fn density_unit(&self) -> AstResult<&'static str> {
        Ok(self.system()?.density_unit())
    }

    /// The flux system: the System attribute, set or default.
    ///
    /// The systems, with the default Unit of each in parentheses, are
    /// [`FluxDen`](FluxSystem::FluxDen), `"FLXDN"`, flux per unit frequency
    /// (W/m^2/Hz); [`FluxDenW`](FluxSystem::FluxDenW), `"FLXDNW"`, flux per
    /// unit wavelength (W/m^2/Angstrom); [`SBright`](FluxSystem::SBright),
    /// `"SFCBR"`, surface brightness in frequency units
    /// (W/m^2/Hz/arcmin**2); and [`SBrightW`](FluxSystem::SBrightW),
    /// `"SFCBRW"`, surface brightness in wavelength units
    /// (W/m^2/Angstrom/arcmin**2).
    /// [`FluxSystem::parse`] reads a System name.
    ///
    /// Default: with a Unit set and no System, the first of these systems
    /// whose default Unit can be converted to the Unit; with neither set,
    /// FLXDN, with the Unit W/m^2/Hz.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badun` when the System is not set and the Unit fits no
    ///   flux system.
    // C: fluxframe.c `GetSystem` (1514).
    pub fn system(&self) -> AstResult<FluxSystem> {
        if self.system_set {
            return Ok(self.system);
        }
        let Some(u) = self.axis.unit.as_deref() else {
            return Ok(FluxSystem::FluxDen);
        };
        for sys in [
            FluxSystem::FluxDen,
            FluxSystem::FluxDenW,
            FluxSystem::SBright,
            FluxSystem::SBrightW,
        ] {
            if crate::units::unit_mapper(u, sys.default_units()).is_some() {
                return Ok(sys);
            }
        }
        Err(AstError::new(
            ErrorCode::Badun,
            format!(
                "astGetSystem(FluxFrame): The current units ({u}) cannot be used with any of \
                 the supported flux systems."
            ),
        ))
    }

    /// Set the System; see [`system`](FluxFrame::system).
    ///
    /// When this changes the System, the Unit last used with the new System
    /// is re-instated (or the Unit cleared if none was), and the Label,
    /// Symbol and Title are cleared, since their defaults depend on it.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badun` when the old System follows from a Unit that
    ///   fits no flux system, as [`system`](FluxFrame::system) reports.
    pub fn set_system(&mut self, sys: FluxSystem) -> AstResult<()> {
        let old = self.system()?;
        self.system = sys;
        self.system_set = true;
        self.system_changed(old)
    }

    /// After a change of System, re-instate the Unit last used with the new
    /// one, or clear the Unit, and clear the Label, Symbol and Title, whose
    /// defaults follow the System. Mirrors fluxframe.c `SetSystem` and
    /// `ClearSystem` (fluxframe.c:2733, :331).
    fn system_changed(&mut self, old: FluxSystem) -> AstResult<()> {
        let new = self.system()?;
        if new != old {
            self.axis.unit = self.used_units[new.index()].clone();
            self.axis.label = None;
            self.axis.symbol = None;
            self.attrs.title = None;
        }
        Ok(())
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

    /// Clear the Unit of the single axis.
    pub(crate) fn clear_axis_unit(&mut self) {
        self.axis.unit = None;
    }

    /// The axis Unit: the one set, or the System's default unit.
    // C: fluxframe.c `GetUnit` (1650).
    pub fn unit_or_default(&self) -> &str {
        match self.axis.unit.as_deref() {
            Some(u) => u,
            // With no Unit set, the System is always known.
            None => self
                .system()
                .expect("the System of a FluxFrame with no Unit")
                .default_units(),
        }
    }

    /// The spectral position at which flux values are measured: the SpecVal
    /// attribute, set or default.
    ///
    /// A frequency, wavelength or other spectral position, in the spectral
    /// system and units of the [`spec_frame`](FluxFrame::spec_frame).
    /// It is used when determining the Mapping between FluxFrames.
    ///
    /// Default: the value given to
    /// [`with_spec_val`](FluxFrame::with_spec_val), or `BAD_VALUE`.
    #[doc(alias = "SpecVal")]
    pub fn spec_val(&self) -> f64 {
        self.spec_val.unwrap_or(self.def_spec_val)
    }

    /// Set SpecVal; see [`spec_val`](FluxFrame::spec_val).
    /// `BAD_VALUE` clears it.
    pub fn set_spec_val(&mut self, val: f64) {
        if is_bad(val) {
            self.spec_val = None;
        } else {
            self.spec_val = Some(val);
        }
    }

    /// Clear SpecVal; see [`spec_val`](FluxFrame::spec_val).
    pub fn clear_spec_val(&mut self) {
        self.spec_val = None;
    }

    /// The SpecFrame giving the spectral system and units of the SpecVal, if
    /// one has been attached.
    /// With none, the SpecVal is a frequency in Hz for a "per Hz" System and
    /// a wavelength in Angstrom for a "per Angstrom" one.
    pub fn spec_frame(&self) -> Option<&super::specframe::SpecFrame> {
        self.specframe.as_deref()
    }

    /// Attach the SpecFrame giving the spectral system and units of the
    /// SpecVal; see [`spec_frame`](FluxFrame::spec_frame).
    pub fn set_spec_frame(&mut self, sf: super::specframe::SpecFrame) {
        self.specframe = Some(Box::new(sf));
    }

    /// The SpecFrame defining this FluxFrame's spectral (SpecVal) coordinate
    /// system. Mirrors C `GetSpecFrame` (fluxframe.c:1211): returns a clone of
    /// the encapsulated SpecFrame when present, otherwise a default SpecFrame
    /// whose System/Unit are the FluxFrame's density system (FREQ in Hz or
    /// WAVELEN in Angstrom).
    fn get_spec_frame(&self) -> AstResult<super::specframe::SpecFrame> {
        if let Some(sf) = self.specframe.as_deref() {
            return Ok(sf.clone());
        }
        // The System as C's astGetSystem gives it, which may follow from the
        // Unit (fluxframe.c:1235 `GetDensitySystem`).
        let system = self.system()?;
        let mut sf = super::specframe::SpecFrame::new(system.density_spec_system());
        sf.store_unit(system.density_unit());
        Ok(sf)
    }

    /// Generate the Mapping that converts flux values from `self` (target) to
    /// `result`, taking account of the differing flux systems and the
    /// spectral position (SpecVal).
    ///
    /// Returns `Ok(Some(map))` on success (forward = target → result),
    /// `Ok(None)` when the two FluxFrames are too unrelated to convert
    /// (exactly one is a surface-brightness system, or the SpecVal positions
    /// differ / are absent for a cross-density conversion).
    ///
    /// The Mapping is a units Mapping from the target's units to the target
    /// system's default units, the change of density system, and a units
    /// Mapping from the result system's default units to the result's units,
    /// composed in series and simplified.
    ///
    /// # Errors
    ///
    /// Passes on the error of [`system`](FluxFrame::system) on either
    /// FluxFrame, or of building or simplifying the Mapping.
    // C: fluxframe.c `MakeFluxMapping` (1971).
    pub fn make_flux_mapping(&self, result: &FluxFrame) -> AstResult<Option<DynMapping>> {
        use crate::mappings::{CmpMap, Composition, Mapping, ZoomMap};

        let rsys_in = self.system()?;
        let rsys_out = result.system()?;

        // Collapse surface-brightness systems onto their flux-density
        // counterparts and record the surface-brightness flags
        // (fluxframe.c:2086-2112).
        let sys_in = rsys_in.density_flux_system();
        let sys_out = rsys_out.density_flux_system();
        let sb_in = rsys_in.is_surface_brightness();
        let sb_out = rsys_out.is_surface_brightness();

        // If exactly one of the systems is surface brightness, no Mapping can
        // be formed (fluxframe.c:2129).
        if sb_in != sb_out {
            return Ok(None);
        }

        // Determine the density-system zoom factor.
        let zoom = if sys_in == sys_out {
            // Same flux-density system → identity (fluxframe.c:2134).
            1.0
        } else {
            // Build the SpecFrames encapsulated in (or defaulted for) each
            // FluxFrame (`sfin1`/`sfout1`), plus standardized copies in the
            // density systems (`sfin2`/`sfout2`). Mirrors fluxframe.c:2148-2157.
            let sfin1 = self.get_spec_frame()?;
            let mut sfin2 = sfin1.clone();
            sfin2.set_system(sys_in.density_spec_system())?;
            sfin2.set_unit(1, sys_in.density_unit())?;

            let sfout1 = result.get_spec_frame()?;
            let mut sfout2 = sfout1.clone();
            sfout2.set_system(sys_out.density_spec_system())?;
            sfout2.set_unit(1, sys_out.density_unit())?;

            // smap: standardized result system → standardized target system
            // (fluxframe.c:2163, astConvert(sfout2, sfin2)).
            let smap = sfout2.make_spec_mapping(&sfin2)?;
            let smap = smap.simplify()?;

            // Standardize both SpecVals into the result density system and
            // require them equal (fluxframe.c:2172-2206).
            let specval_in_raw = self.spec_val();
            let specval_out_raw = result.spec_val();
            if is_bad(specval_in_raw) || is_bad(specval_out_raw) {
                return Ok(None);
            }

            // smap_in: target's SpecFrame → standardized target system; then
            // smap (inverse) maps the standardized target value back into the
            // standardized result system.
            let smap_in = sfin1.make_spec_mapping(&sfin2)?;
            let specval2 = transform1(smap_in.as_ref(), specval_in_raw, true)?;
            let specval_in = transform1(smap.as_ref(), specval2, false)?;

            // smap_out: result's SpecFrame → standardized result system.
            let smap_out = sfout1.make_spec_mapping(&sfout2)?;
            let specval_out = transform1(smap_out.as_ref(), specval_out_raw, true)?;

            if is_bad(specval_in) || is_bad(specval_out) {
                return Ok(None);
            }
            // C checks astEQUALS(specval_in, specval_out, 1.0E8) — a relative
            // tolerance of 1e8 * DBL_EPSILON ≈ 2.2e-8.
            let tol = 1.0e8 * f64::EPSILON * specval_in.abs().max(specval_out.abs()).max(1.0);
            if (specval_in - specval_out).abs() > tol {
                return Ok(None);
            }

            // Read the zoom from the simplified Mapping: UnitMap → 1, ZoomMap
            // → its factor, else differentiate at the mean SpecVal
            // (fluxframe.c:2208-2222).
            if smap.as_any().is::<UnitMap>() {
                1.0
            } else if let Some(zm) = smap.as_any().downcast_ref::<ZoomMap>() {
                zm.zoom()
            } else {
                let mid = 0.5 * (specval_in + specval_out);
                let r = smap.rate(&[mid], 1, 1)?;
                if is_bad(r) {
                    return Ok(None);
                }
                r
            }
        };

        // map2: the flux scale factor is |zoom| (fluxframe.c:2239: fabs(zoom)).
        let map2: DynMapping = Box::new(ZoomMap::new(1, zoom.abs())?);

        // map1: target user units → target system default units; map3: result
        // system default units → result user units (fluxframe.c:2078/2247).
        // Any incompatible-units failure means no conversion (C requires all
        // three maps non-NULL).
        let in_default = rsys_in.default_units();
        let out_default = rsys_out.default_units();
        let target_unit = self.unit_or_default();
        let result_unit = result.unit_or_default();

        let map1 = match unit_or_unit_map(target_unit, in_default)? {
            Some(m) => m,
            None => return Ok(None),
        };
        let map3 = match unit_or_unit_map(out_default, result_unit)? {
            Some(m) => m,
            None => return Ok(None),
        };

        // Compose map1 ∘ map2 ∘ map3 in series and simplify.
        let chain = CmpMap::new(map1, map2, Composition::Series)?;
        let chain = CmpMap::new(Box::new(chain), map3, Composition::Series)?;
        Ok(Some(<dyn Mapping>::simplify(&chain)?))
    }

    /// The same as [`make_flux_mapping`](FluxFrame::make_flux_mapping).
    ///
    /// # Errors
    ///
    /// Passes on the error of [`make_flux_mapping`](FluxFrame::make_flux_mapping).
    pub fn convert_flux_to(&self, result: &FluxFrame) -> AstResult<Option<DynMapping>> {
        self.make_flux_mapping(result)
    }

    /// Set the Title; see [`Frame::title`].
    pub fn set_title(&mut self, title: &str) {
        self.attrs.title = Some(title.to_string());
    }

    /// Set the Domain, removing white space and upper-casing letters; see
    /// [`Frame::domain`].
    pub fn set_domain(&mut self, domain: &str) {
        self.attrs.domain = Some(super::frame_attrs::clean_domain(domain));
    }

    /// Whether the Domain is set; see [`Frame::domain`].
    pub fn test_domain(&self) -> bool {
        self.attrs.domain.is_some()
    }

    /// Clear the Domain, so that it is `"FLUX"` by default; see
    /// [`Frame::domain`].
    pub fn clear_domain(&mut self) {
        self.attrs.domain = None;
    }
}

/// Build a units-conversion Mapping from `from` to `to`, mirroring C
/// `astUnitMapper`: identical strings yield a `UnitMap`, dimensionally
/// compatible strings yield the conversion Mapping, and incompatible strings
/// yield `None` (so the caller can decline the conversion, as C does when
/// `map1`/`map3` come back NULL).
fn unit_or_unit_map(from: &str, to: &str) -> AstResult<Option<DynMapping>> {
    if from == to {
        return Ok(Some(Box::new(crate::mappings::UnitMap::new(1))));
    }
    Ok(crate::units::unit_mapper(from, to))
}

/// Transform a single scalar through a 1-D Mapping in the given direction,
/// returning `BAD_VALUE` if the mapping produces a bad output. Used by
/// [`FluxFrame::make_flux_mapping`] to mirror C's `astTran1` calls.
fn transform1(map: &dyn crate::mappings::Mapping, value: f64, forward: bool) -> AstResult<f64> {
    let pts = crate::coords::PointSet::from_rows([[value]]);
    let out = map.transform(&pts, forward)?;
    Ok(out.point(0)[0])
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for FluxFrame {
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

    /// Always true: a FluxFrame always uses active units, and reports the
    /// flag as not set.
    fn get_active_unit(&self) -> bool {
        true // FluxFrame always uses active units (frame.c:9882).
    }

    /// Does nothing: a FluxFrame always uses active units; see
    /// [`get_active_unit`](FluxFrame::get_active_unit).
    fn set_active_unit(&mut self, _value: bool) {}

    fn test_active_unit(&self) -> bool {
        false
    }

    /// The Label, set or default; see [`Frame::label`].
    ///
    /// The default is the System's [`label`](FluxSystem::label), rewritten
    /// for a Unit set to a function of the default unit, so `"log(W/m^2/Hz)"`
    /// gives `"log( Flux density )"`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1.
    /// - Passes on the error of [`system`](FluxFrame::system).
    // C: fluxframe.c `GetLabel` (1107).
    fn label(&self, axis: usize) -> AstResult<String> {
        if let Some(label) = &self.ax(axis)?.label {
            return Ok(label.clone());
        }
        // GetSystem, which takes the System from a set Unit when the System
        // is not set.
        let system = self.system()?;
        Ok(super::frame_attrs::relabel_for_unit(
            system.label().to_string(),
            system.default_units(),
            self.axis.unit.as_deref(),
        ))
    }

    /// The Symbol, set or default; see [`Frame::symbol`].
    ///
    /// The default is the System's [`symbol`](FluxSystem::symbol), rewritten
    /// for a Unit as the [`label`](FluxFrame::label) is.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1.
    /// - Passes on the error of [`system`](FluxFrame::system).
    // C: fluxframe.c `GetSymbol`.
    fn symbol(&self, axis: usize) -> AstResult<String> {
        if let Some(symbol) = &self.ax(axis)?.symbol {
            return Ok(symbol.clone());
        }
        let system = self.system()?;
        Ok(super::frame_attrs::relabel_for_unit(
            system.symbol().to_string(),
            system.default_units(),
            self.axis.unit.as_deref(),
        ))
    }

    /// The Unit, set or default: the System's default unit, as
    /// [`system`](FluxFrame::system) lists; see [`Frame::unit`].
    // C: fluxframe.c `GetUnit` (1650).
    fn unit(&self, axis: usize) -> AstResult<String> {
        self.ax(axis)?;
        Ok(self.unit_or_default().to_string())
    }

    /// Set the Unit; see [`unit`](FluxFrame::unit).
    ///
    /// A Unit that suits the System is recorded as the one used with it,
    /// which [`set_system`](FluxFrame::set_system) re-instates; one that
    /// does not clears the System, which then follows from the Unit.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1.
    /// - `ErrorCode::Badun` when the System is not set and the Unit fits no
    ///   flux system.
    // C: fluxframe.c `SetUnit` (2803).
    fn set_unit(&mut self, axis: usize, unit: &str) -> AstResult<()> {
        // The Frame class's SetUnit (frame.c:10775).
        let unit = unit.trim_end();
        if self.get_active_unit() {
            let old = Frame::unit(self, axis)?;
            super::frame_attrs::new_unit(&mut self.axis, &old, unit);
        }
        self.ax_mut(axis)?.unit = Some(unit.to_string());
        let system = self.system()?;
        if crate::units::unit_mapper(system.default_units(), unit).is_some() {
            self.used_units[system.index()] = Some(unit.to_string());
        } else {
            self.system_set = false;
        }
        Ok(())
    }

    /// Clear the Unit, also forgetting the Unit used with the current
    /// System; see [`unit`](FluxFrame::unit).
    // C: fluxframe.c `ClearUnit` (404).
    fn clear_unit(&mut self, axis: usize) -> AstResult<()> {
        self.ax(axis)?;
        let system = self.system()?;
        self.used_units[system.index()] = None;
        // The Frame class's ClearUnit (frame.c:2527).
        if let Some(old) = self.axis.unit.take() {
            if self.get_active_unit() {
                let new = Frame::unit(self, axis)?;
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
        super::assert_points_len("distance", self.naxes(), &[a, b]);
        Ok(super::cartesian_distance(&a[..1], &b[..1]))
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        super::assert_points_len("angle", self.naxes(), &[a, b, c]);
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
        super::assert_points_len("offset", self.naxes(), &[from, to]);
        Ok(super::cartesian_offset(&from[..1], &to[..1], dist))
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        super::assert_points_len("norm", self.naxes(), &[&*coords]);
        // Flux coordinates are unbounded — no normalization needed.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); FluxFrame
        // does not override it. Within one flux System the conversion is a
        // UnitMap regardless of SpecVal, so two same-System FluxFrames are
        // equal even if their SpecVal differs (oracle-verified). A
        // cross-System pair converts, when it converts at all, with a
        // Mapping that is not a UnitMap, so the frames compare unequal.
        super::find_frame::frames_equal(self, other)
    }

    fn system_string(&self) -> AstResult<Option<&str>> {
        Ok(Some(self.system()?.as_str()))
    }

    // -----------------------------------------------------------------
    // Match-control attributes (mirror C `astGet/SetMinAxes` etc.).
    // -----------------------------------------------------------------

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (fluxframe.c:2280 / 2888 / 2470).
    // -----------------------------------------------------------------

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        fluxframe_overlay(self, template_axes, result)
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
        if let Some(t) = template.downcast_ref::<FluxFrame>() {
            if t.test_system() {
                self.set_system(t.system)?;
            }
            if let Some(sys) = t.align_system {
                self.align_system = Some(sys);
            }
        }
        Ok(())
    }

    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // A specialized leaf frame can only be cast down to a plain
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
        // Mirrors fluxframe.c:2280. Step 1: parent (Frame class)
        // Match handles axis-count and Domain pre-checks.
        let parent = super::find_frame::default_match(self, target, match_sub)?;
        if parent.is_none() {
            return Ok(None);
        }

        // Step 2 (fluxframe.c:2425-2438): walk astPrimaryFrame on
        // each target axis looking for a FluxFrame.
        let target_naxes = target.naxes();
        let mut target_axis0: Option<usize> = None;
        for iaxis in 0..target_naxes {
            let (primary, _) = crate::frames::specframe::primary_frame_walk(target, iaxis);
            if primary.as_any().downcast_ref::<FluxFrame>().is_some() {
                target_axis0 = Some(iaxis);
                break;
            }
        }
        let Some(target_axis0) = target_axis0 else {
            return Ok(None);
        };

        // Step 3 (fluxframe.c:2442-2444): build the axis
        // associations.
        let template_axes_vec = vec![0i32];
        let target_axes_vec = vec![target_axis0 as i32];

        // Step 4 (fluxframe.c:2451-2452): defer to target's SubFrame.
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
        fluxframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // FluxFrame is 1-axis; pick `[0]` returns a clone, anything
        // else falls through to a BasicFrame snapshot (mirrors the
        // parent_subframe branch of fluxframe.c SubFrame).
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

/// C's FluxFrame `Overlay` (fluxframe.c:2470), a method of the template.
///
/// When `result` is a FluxFrame whose System the template sets to a new
/// value, its Title, Label and Symbol are cleared, and its Unit when the
/// two Systems' default units differ; it always takes the template's
/// default SpecVal and SpecFrame. The Frame-class part follows, which sets
/// the System on a FluxFrame result only, and then SpecVal.
fn fluxframe_overlay(
    template_flux: &FluxFrame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    if let Some(result_flux) = result.as_any_mut().downcast_mut::<FluxFrame>() {
        // The effective Systems, which may follow from the Unit
        // (fluxframe.c:2561 `astGetSystem`).
        let old_system = result_flux.system()?;
        let new_system = template_flux.system()?;
        if new_system != old_system && template_flux.test_system() {
            result_flux.clear_title();
            if new_system.default_units() != old_system.default_units() {
                result_flux.clear_unit(1)?;
            }
            result_flux.axis.label = None;
            result_flux.axis.symbol = None;
        }
        result_flux.def_spec_val = template_flux.def_spec_val;
        result_flux.specframe.clone_from(&template_flux.specframe);
    }

    super::overlay::frame_overlay(template_flux, template_axes, result)?;

    if let Some(result_flux) = result.as_any_mut().downcast_mut::<FluxFrame>() {
        if let Some(v) = template_flux.spec_val {
            result_flux.set_spec_val(v);
        }
    }
    Ok(())
}

/// Mirrors C `SubFrame` for FluxFrame (fluxframe.c:2888).
fn fluxframe_sub_frame(
    target: &FluxFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-FluxFrame branch (fluxframe.c:3019-3062).
    if result_naxes == 1 && target_axes.len() == 1 && target_axes[0] == 0 {
        // Result is a copy of the target.
        let mut result = target.clone();

        // If a template was supplied, overlay it onto the result.
        if let Some(t) = template {
            t.overlay(template_axes, &mut result)?;
        }

        // Build the conversion Mapping via MakeFluxMapping
        // (fluxframe.c SubFrame calls MakeFluxMapping with align_sys).
        // Returns None when the two systems are too unrelated to convert
        // (surface-brightness mismatch, or a cross-density pair whose
        // SpecVal positions are absent / don't align).
        match target.make_flux_mapping(&result)? {
            Some(map) => return Ok(Some((map, Box::new(result)))),
            None => return Ok(None),
        }
    }

    // Result is not a FluxFrame (fluxframe.c:3072): set explicitly the
    // attributes whose defaults come from this class, and clear the
    // System, which only this class understands, on a copy, then select the
    // axes as the Frame class does.
    let mut temp = target.clone();
    super::find_frame::set_class_defaults(&mut temp)?;
    temp.clear_system()?;
    temp.clear_align_system();
    super::find_frame::default_sub_frame(&temp, template, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

impl Serializable for FluxFrame {
    fn class_name(&self) -> &'static str {
        "FluxFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of flux values".to_string());

        // The Object class's part: Ident, when set (object.c Dump).
        super::frame_attrs::write_object_part(
            &mut node,
            &self.attrs.id,
            self.attrs.ident.as_deref(),
            self.attrs.use_defs,
            crate::frames::Frame::use_defs(self),
        );

        // The Frame class's part, read through this class's getters
        // (frame.c:14250), then the Axis (frame.c:14777).
        super::frame_attrs::write_frame_cards(
            &mut node,
            &super::frame_attrs::TraitDump {
                frame: self,
                attrs: &self.attrs,
                system: (self.system_set, self.system()?.as_str().to_string()),
                align_system: (
                    self.align_system.is_some(),
                    self.align_system().as_str().to_string(),
                ),
                perm: vec![0],
                is_frame: false,
                own_naxes: 1,
                mapping: None,
            },
        )?;
        let axis_node = self.axis.to_dump_node();
        node.push_full_capped_object("Ax1", axis_node, Some("Axis number 1".to_string()));

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- FluxFrame-specific attributes (after IsA Frame) ---

        // SpecVal (SpcVl), the spectral position at which flux is measured,
        // unless it is bad: active when set, otherwise the default.
        let spec_val = self.spec_val();
        if !is_bad(spec_val) {
            node.write_double(
                "SpcVl",
                self.spec_val.is_some(),
                false,
                spec_val,
                "Spectral position",
            );
        }

        // The encapsulated SpecFrame (SpcFr), only when one is attached
        // (fluxframe.c:3828).
        if let Some(sf) = self.specframe.as_deref() {
            let sf_node = Serializable::to_node(sf)?;
            node.push_object("SpcFr", sf_node, Some("SpcVl coord system".to_string()));
        }

        // The default SpecVal (fluxframe.c:3838).
        if !is_bad(self.def_spec_val) {
            node.push_double(
                "DfSpc",
                self.def_spec_val,
                Some("Default spectral position".to_string()),
            );
        }

        // UsedUnits (fluxframe.c:3840): the Unit last used with each System.
        for sys in FluxSystem::ALL {
            if let Some(u) = &self.used_units[sys.index()] {
                let label = sys.label();
                let lower = format!("{}{}", label[..1].to_ascii_lowercase(), &label[1..]);
                node.push_string(
                    &sys.used_units_key(),
                    u,
                    Some(format!("Preferred units for {lower}")),
                );
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let frame = &whole.section("Frame");
        let node = &whole.section("FluxFrame");
        let mut ff = FluxFrame::unset_system();
        ff.attrs = super::frame_attrs::read_frame_attrs(whole);
        if let Some(system) =
            super::frame_attrs::read_system_item(frame, "System", "System", FluxSystem::parse)?
        {
            ff.system = system;
            ff.system_set = true;
        }
        if let Some(system) =
            super::frame_attrs::read_system_item(frame, "AlSys", "AlignSystem", FluxSystem::parse)?
        {
            ff.align_system = Some(system);
        }
        if let Some(axis_node) = frame.get_object("Ax1") {
            ff.axis = super::axis::Axis::read(axis_node);
        }
        if let Some(f) = ff.axis.apply_frame_axis_keys(frame, 1) {
            ff.axis.format = Some(f);
        }

        // SpecVal (SpcVl).
        if node.get("SpcVl").is_some() {
            let sv = node.get_double("SpcVl", BAD_VALUE);
            if !is_bad(sv) {
                ff.spec_val = Some(sv);
            }
        }

        // The encapsulated SpecFrame (SpcFr).
        if let Some(sf_node) = node.get_object("SpcFr") {
            let frame = crate::serial::frame_from_node(sf_node)?;
            if let Some(sf) = frame.as_any().downcast_ref::<super::specframe::SpecFrame>() {
                ff.specframe = Some(Box::new(sf.clone()));
            }
        }

        // The default SpecVal (fluxframe.c:4222).
        ff.def_spec_val = node.get_double("DfSpc", BAD_VALUE);

        // UsedUnits (fluxframe.c:4232).
        for sys in FluxSystem::ALL {
            let key = sys.used_units_key();
            if node.get(&key).is_some() {
                ff.used_units[sys.index()] = Some(node.get_string(&key, ""));
            }
        }

        Ok(ff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_default_is_fluxden_unset() {
        // FluxFrame::new(FluxDen) must report system as not-explicitly-
        // set (matches the SpecFrame pattern).
        let f = FluxFrame::new(FluxSystem::FluxDen);
        assert!(!f.test_system());
        // After explicit set, test_system flips.
        let mut f = FluxFrame::new(FluxSystem::FluxDen);
        f.set_system(FluxSystem::SBright).unwrap();
        assert!(f.test_system());
    }

    #[test]
    fn match_template_fluxframe_self() {
        let template = FluxFrame::new(FluxSystem::SBright);
        let target = FluxFrame::new(FluxSystem::SBright);
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("FluxFrame ↔ FluxFrame match");
        assert_eq!(m.template_axes, vec![0]);
        assert_eq!(m.target_axes, vec![0]);
        let r = m.result.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system().unwrap(), FluxSystem::SBright);
    }

    #[test]
    fn match_template_fluxframe_against_basicframe_fails() {
        // FluxFrame template requires a FluxFrame target.
        let template = FluxFrame::new(FluxSystem::SBright);
        let target = crate::frames::BasicFrameBuilder::new(1).build().unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_fluxframe_different_system_returns_none() {
        // Surface brightness and flux density do not convert
        // (MakeFluxMapping, fluxframe.c:1971; testflux.c Error 29), so the
        // match fails.
        let template = FluxFrame::new(FluxSystem::SBright);
        let target = FluxFrame::new(FluxSystem::FluxDenW);
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn sub_frame_fluxframe_to_fluxframe_identity() {
        let target = FluxFrame::new(FluxSystem::SBright);
        let template = FluxFrame::new(FluxSystem::SBright);
        let (map, result) = target
            .sub_frame(Some(&template), 1, &[0], &[0])
            .unwrap()
            .expect("identity sub_frame");
        assert_eq!(map.nin(), 1);
        assert_eq!(map.nout(), 1);
        let r = result.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system().unwrap(), FluxSystem::SBright);
    }

    #[test]
    fn overlay_template_copies_set_system() {
        let mut template = FluxFrame::new(FluxSystem::FluxDen);
        template.set_system(FluxSystem::SBright).unwrap();
        assert!(template.test_system());
        let target = FluxFrame::new(FluxSystem::FluxDen);
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0], result.as_mut()).unwrap();
        let r = result.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system().unwrap(), FluxSystem::SBright);
    }

    #[test]
    fn overlay_template_skips_unset_system() {
        // Default-constructed template (FluxDen) is unset; overlaying
        // it onto an SBright target must leave the target's system.
        let template = FluxFrame::new(FluxSystem::FluxDen);
        assert!(!template.test_system());
        let target = FluxFrame::new(FluxSystem::SBright);
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0], result.as_mut()).unwrap();
        let r = result.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system().unwrap(), FluxSystem::SBright);
    }

    #[test]
    fn pick_axes_snapshot_full_returns_fluxframe() {
        let target = FluxFrame::new(FluxSystem::SBright);
        let snap = target.pick_axes_snapshot(&[0]).unwrap();
        let r = snap.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system().unwrap(), FluxSystem::SBright);
    }
}
