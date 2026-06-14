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
    /// Return the canonical string representation (as used in serialisation).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FluxDen => "FLXDN",
            Self::FluxDenW => "FLXDNW",
            Self::SBright => "SFCBR",
            Self::SBrightW => "SFCBRW",
        }
    }

    /// Parse from string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "FLXDN" | "FLUXDEN" => Some(Self::FluxDen),
            "FLXDNW" | "FLUXDENW" => Some(Self::FluxDenW),
            "SFCBR" | "SBRIGHT" => Some(Self::SBright),
            "SFCBRW" | "SBRIGHTW" => Some(Self::SBrightW),
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

    /// Human-readable label for this system.
    pub fn label(&self) -> &'static str {
        match self {
            Self::FluxDen => "Flux Density",
            Self::FluxDenW => "Flux Density",
            Self::SBright => "Surface Brightness",
            Self::SBrightW => "Surface Brightness",
        }
    }

    /// Symbol for this system.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::FluxDen => "FlxDn",
            Self::FluxDenW => "FlxDnW",
            Self::SBright => "SfcBr",
            Self::SBrightW => "SfcBrW",
        }
    }

    /// Whether this system measures per unit frequency (true) or per unit wavelength (false).
    pub fn is_per_frequency(&self) -> bool {
        matches!(self, Self::FluxDen | Self::SBright)
    }

    /// Whether this system is a surface-brightness system (`SBright` /
    /// `SBrightW`) rather than a flux-density system (`FluxDen` /
    /// `FluxDenW`). Mirrors the `sb_in`/`sb_out` flags in
    /// `fluxframe.c::MakeFluxMapping`.
    pub fn is_surface_brightness(&self) -> bool {
        matches!(self, Self::SBright | Self::SBrightW)
    }

    /// The corresponding flux-density system, collapsing a surface-brightness
    /// system onto its flux-density counterpart (`SBright` → `FluxDen`,
    /// `SBrightW` → `FluxDenW`). Mirrors the `sys_in`/`sys_out` mapping in
    /// `fluxframe.c::MakeFluxMapping` (lines 2086-2112).
    pub fn density_flux_system(&self) -> Self {
        match self {
            Self::SBright => Self::FluxDen,
            Self::SBrightW => Self::FluxDenW,
            other => *other,
        }
    }

    /// The spectral [`SpecSystem`] describing this flux system's density
    /// axis: frequency for "per Hz" systems, wavelength for "per Angstrom"
    /// systems. Mirrors C `DensitySystem` (fluxframe.c:537).
    pub fn density_spec_system(&self) -> super::specframe::SpecSystem {
        if self.is_per_frequency() {
            super::specframe::SpecSystem::Freq
        } else {
            super::specframe::SpecSystem::Wavelen
        }
    }

    /// The default unit for this flux system's density axis: "Hz" for "per
    /// frequency" systems, "Angstrom" for "per wavelength" systems. Mirrors
    /// C `DensityUnit` (fluxframe.c:602).
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

/// A coordinate frame for flux (brightness) measurements.
///
/// `FluxFrame` is a 1-axis Frame representing measured flux or surface
/// brightness. It supports flux density per unit frequency or per unit
/// wavelength. All flux values are assumed to be measured at the same
/// spectral position (given by the `spec_val` attribute).
///
/// Construct via [`FluxFrame::new`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, FluxFrame, FluxSystem};
///
/// let ff = FluxFrame::new(FluxSystem::FluxDen);
/// assert_eq!(ff.naxes(), 1);
/// assert_eq!(ff.system(), FluxSystem::FluxDen);
/// ```
#[derive(Debug, Clone)]
pub struct FluxFrame {
    // Frame-level attributes
    title: Option<String>,
    domain: Option<String>,

    // FluxFrame-specific
    system: FluxSystem,
    /// Whether `system` was set explicitly (mirrors C `astTestSystem`).
    system_set: bool,
    /// Per-instance axis Unit string (e.g. "Jy"). `None` means the system
    /// default units are used (mirrors C `astTestUnit`/`astGetUnit`).
    unit: Option<String>,
    spec_val: Option<f64>, // spectral position at which flux is measured
    /// Optional encapsulated SpecFrame defining the coordinate system of
    /// `spec_val` (mirrors C `this->specframe`, fluxframe.c). When `None`,
    /// SpecVal is interpreted directly in the FluxFrame's density system
    /// (the default `GetSpecFrame` path). Required for a meaningful
    /// cross-density-system flux conversion.
    specframe: Option<Box<super::specframe::SpecFrame>>,

    // Object-level attributes
    act_unt: Option<bool>,
    ident: Option<String>,

    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
}

impl FluxFrame {
    /// Create a new `FluxFrame` with the given flux coordinate system.
    ///
    /// Defaults: domain = "FLUX".
    pub fn new(system: FluxSystem) -> Self {
        Self {
            title: None,
            // None = use the default "FLUX"; mirrors astTestDomain.
            domain: None,
            system,
            // Match the SpecFrame convention: when the constructor is
            // called with the default system (FluxDen here), treat it
            // as unset; otherwise mark it as explicitly set so it can
            // act as a template-side discriminator.
            system_set: !matches!(system, FluxSystem::FluxDen),
            unit: None,
            spec_val: None,
            specframe: None,
            act_unt: None,
            ident: None,
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
        }
    }

    /// Whether `System` has been explicitly set on this FluxFrame.
    /// Mirrors C `astTestSystem`.
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear the explicit-set state of System.
    pub fn clear_system(&mut self) {
        self.system_set = false;
    }

    /// Frame Title accessor. Returns the empty string when unset.
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }

    /// Whether Title has been explicitly set. Mirrors `astTestTitle`.
    pub fn test_title(&self) -> bool {
        self.title.is_some()
    }

    /// Clear the Title attribute.
    pub fn clear_title(&mut self) {
        self.title = None;
    }

    // --- FluxFrame-specific attributes ---

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Return the flux coordinate system.
    ///
    /// Mirrors C `GetSystem` (fluxframe.c:1514): when System has not been set
    /// explicitly but a Unit *has*, the default System is the first flux
    /// system whose default units are convertible from the set Unit (scanned
    /// in declaration order). Otherwise the stored System (default FluxDen)
    /// is returned.
    pub fn system(&self) -> FluxSystem {
        if !self.system_set {
            if let Some(ref u) = self.unit {
                for sys in [
                    FluxSystem::FluxDen,
                    FluxSystem::FluxDenW,
                    FluxSystem::SBright,
                    FluxSystem::SBrightW,
                ] {
                    if crate::units::unit_mapper(u, sys.default_units()).is_some() {
                        return sys;
                    }
                }
            }
        }
        self.system
    }

    /// The stored System value, ignoring any unit-derived default. Used by
    /// serialisation and the `test_system` set-ness check.
    pub(crate) fn stored_system(&self) -> FluxSystem {
        self.system
    }

    /// Set the flux coordinate system.
    pub fn set_system(&mut self, sys: FluxSystem) {
        self.system = sys;
        self.system_set = true;
    }

    /// Return the explicitly-set axis Unit string, or `None` for the system
    /// default. Mirrors `astTestUnit`/`astGetUnit`.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// Set the axis Unit string (e.g. `"Jy"`).
    pub fn set_unit(&mut self, unit: &str) {
        self.unit = Some(unit.to_string());
    }

    /// Whether the axis Unit has been explicitly set.
    pub fn test_unit(&self) -> bool {
        self.unit.is_some()
    }

    /// Clear the axis Unit, reverting to the system default.
    pub fn clear_unit(&mut self) {
        self.unit = None;
    }

    /// The effective axis Unit string: the explicitly-set value, or the
    /// (unit-derived or stored) system's default units. Mirrors C `GetUnit`
    /// (fluxframe.c:1650).
    pub fn unit_or_default(&self) -> &str {
        match self.unit.as_deref() {
            Some(u) => u,
            None => self.system().default_units(),
        }
    }

    /// Return the spectral position at which flux values are measured.
    ///
    /// Returns `BAD_VALUE` if not set.
    pub fn spec_val(&self) -> f64 {
        self.spec_val.unwrap_or(BAD_VALUE)
    }

    /// Set the spectral position at which flux values are measured.
    pub fn set_spec_val(&mut self, val: f64) {
        if is_bad(val) {
            self.spec_val = None;
        } else {
            self.spec_val = Some(val);
        }
    }

    /// Clear the spectral position.
    pub fn clear_spec_val(&mut self) {
        self.spec_val = None;
    }

    /// The encapsulated SpecFrame defining the coordinate system of SpecVal,
    /// if one has been attached.
    pub fn spec_frame(&self) -> Option<&super::specframe::SpecFrame> {
        self.specframe.as_deref()
    }

    /// Attach a SpecFrame defining the coordinate system of SpecVal. Mirrors
    /// setting `this->specframe` in C. Required for a meaningful
    /// cross-density-system flux conversion (otherwise SpecVal is interpreted
    /// in the density system and cross-system pairs do not align).
    pub fn set_spec_frame(&mut self, sf: super::specframe::SpecFrame) {
        self.specframe = Some(Box::new(sf));
    }

    /// The SpecFrame defining this FluxFrame's spectral (SpecVal) coordinate
    /// system. Mirrors C `GetSpecFrame` (fluxframe.c:1211): returns a clone of
    /// the encapsulated SpecFrame when present, otherwise a default SpecFrame
    /// whose System/Unit are the FluxFrame's density system (FREQ in Hz or
    /// WAVELEN in Angstrom).
    fn get_spec_frame(&self) -> super::specframe::SpecFrame {
        if let Some(sf) = self.specframe.as_deref() {
            return sf.clone();
        }
        let mut sf = super::specframe::SpecFrame::new(self.system.density_spec_system());
        sf.set_unit(self.system.density_unit());
        sf
    }

    /// Generate the Mapping that converts flux values from `self` (target) to
    /// `result`, taking account of the differing flux systems and the
    /// spectral position (SpecVal). Mirrors C `MakeFluxMapping`
    /// (fluxframe.c:1971).
    ///
    /// Returns `Ok(Some(map))` on success (forward = target → result),
    /// `Ok(None)` when the two FluxFrames are too unrelated to convert
    /// (exactly one is a surface-brightness system, or the SpecVal positions
    /// differ / are absent for a cross-density conversion).
    ///
    /// The full chain mirrors C: a units Mapping from the target's user units
    /// to the target system's default units (`map1`), the density-system zoom
    /// (`map2`), and a units Mapping from the result system's default units to
    /// the result's user units (`map3`), composed in series and simplified.
    pub fn make_flux_mapping(&self, result: &FluxFrame) -> AstResult<Option<DynMapping>> {
        use crate::mappings::{CmpMap, Composition, Mapping, ZoomMap};

        let rsys_in = self.system();
        let rsys_out = result.system();

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
            // FluxFrame (`sfin1`/`sfout1`), plus standardised copies in the
            // density systems (`sfin2`/`sfout2`). Mirrors fluxframe.c:2148-2157.
            let sfin1 = self.get_spec_frame();
            let mut sfin2 = sfin1.clone();
            sfin2.set_system(sys_in.density_spec_system());
            sfin2.set_unit(sys_in.density_unit());

            let sfout1 = result.get_spec_frame();
            let mut sfout2 = sfout1.clone();
            sfout2.set_system(sys_out.density_spec_system());
            sfout2.set_unit(sys_out.density_unit());

            // smap: standardised result system → standardised target system
            // (fluxframe.c:2163, astConvert(sfout2, sfin2)).
            let smap = sfout2.make_spec_mapping(&sfin2)?;
            let smap = smap.simplified()?;

            // Standardise both SpecVals into the result density system and
            // require them equal (fluxframe.c:2172-2206).
            let specval_in_raw = self.spec_val();
            let specval_out_raw = result.spec_val();
            if is_bad(specval_in_raw) || is_bad(specval_out_raw) {
                return Ok(None);
            }

            // smap_in: target's SpecFrame → standardised target system; then
            // smap (inverse) maps the standardised target value back into the
            // standardised result system.
            let smap_in = sfin1.make_spec_mapping(&sfin2)?;
            let specval2 = transform1(smap_in.as_ref(), specval_in_raw, true)?;
            let specval_in = transform1(smap.as_ref(), specval2, false)?;

            // smap_out: result's SpecFrame → standardised result system.
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
                zm.factor()
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
        Ok(Some(chain.simplified()?))
    }

    /// Convert from `self`'s flux system to `result`'s, applying the
    /// [`make_flux_mapping`](Self::make_flux_mapping) Mapping. Returns `None`
    /// when no conversion exists. This is the FluxFrame analogue of
    /// `astConvert`'s mapping for the 1-D flux axis.
    pub fn convert_flux_to(&self, result: &FluxFrame) -> AstResult<Option<DynMapping>> {
        self.make_flux_mapping(result)
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
    /// Mirrors C's `astTestDomain` for `FluxFrame`.
    pub fn test_domain(&self) -> bool {
        self.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default `"FLUX"`.
    pub fn clear_domain(&mut self) {
        self.domain = None;
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
    fn naxes(&self) -> usize {
        1
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    fn get_active_unit(&self) -> bool {
        true // FluxFrame always uses active units (frame.c:9882).
    }

    fn set_active_unit(&mut self, _value: bool) {}

    fn test_active_unit(&self) -> bool {
        false
    }

    fn effective_label(&self, _axis: usize) -> String {
        self.system.label().to_string()
    }

    fn effective_unit(&self, _axis: usize) -> String {
        self.unit_or_default().to_string()
    }

    fn effective_symbol(&self, _axis: usize) -> String {
        self.system.symbol().to_string()
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("FluxFrame has 1 axis, got axis={axis}"),
            ));
        }
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        Ok(format!("{value:.7}"))
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("FluxFrame has 1 axis, got axis={axis}"),
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
        // Flux coordinates are unbounded — no normalisation needed.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        // FluxFrame attribute changes don't affect coordinate transformations
        // in the simple case. Return UnitMap.
        Ok(Box::new(UnitMap::new(1)))
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); FluxFrame
        // does not override it. Within one flux System the conversion is a
        // UnitMap regardless of SpecVal, so two same-System FluxFrames are
        // equal even if their SpecVal differs (oracle-verified). A
        // cross-System pair needs MakeFluxMapping; while that is unported,
        // convert_frames yields no conversion, so the frames compare
        // unequal — which matches the oracle for genuine cross-System
        // pairs.
        super::find_frame::frames_equal(self, other)
    }

    fn system_string(&self) -> Option<&str> {
        Some(self.system.as_str())
    }

    fn domain(&self) -> &str {
        self.domain.as_deref().unwrap_or("FLUX")
    }

    fn test_domain(&self) -> bool {
        FluxFrame::test_domain(self)
    }

    fn ident(&self) -> &str {
        FluxFrame::ident(self)
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
        // FluxFrame is 1-axis: permutations are degenerate. C defaults
        // Permute to 1 (true) for FluxFrame because the parent Frame
        // class default applies (fluxframe.c does not override).
        self.permute_opt.unwrap_or(true)
    }
    fn set_permute(&mut self, v: bool) {
        self.permute_opt = Some(v);
    }
    fn test_permute(&self) -> bool {
        self.permute_opt.is_some()
    }

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (fluxframe.c:2280 / 2888 / 2470).
    // -----------------------------------------------------------------

    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        fluxframe_overlay(template, template_axes, self)
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

/// Mirrors C `Overlay` for FluxFrame (fluxframe.c:2470). Only
/// attributes the template FluxFrame has explicitly set are
/// transferred onto the result. When `result` is itself a FluxFrame
/// and overlaying System would change the coordinate type, the
/// result's Title is cleared first so the new system's defaults take
/// effect.
fn fluxframe_overlay(
    template: &dyn Frame,
    _template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    let template_flux = match template.as_any().downcast_ref::<FluxFrame>() {
        Some(f) => f,
        None => return Ok(()),
    };

    if let Some(result_flux) = result.as_any_mut().downcast_mut::<FluxFrame>() {
        let old_system = result_flux.system;
        let new_system = template_flux.system;

        // fluxframe.c:2570-2590 — if the System will change AND the
        // template explicitly sets it, clear Title.
        if new_system != old_system && template_flux.test_system() {
            result_flux.clear_title();
        }

        // Parent Frame::Overlay copies (Domain / Title / SpecVal /
        // System).
        if template_flux.test_domain() {
            result_flux.set_domain(template_flux.domain());
        }
        if template_flux.test_title() {
            if let Some(ref t) = template_flux.title {
                result_flux.set_title(t);
            }
        }
        if template_flux.test_system() {
            result_flux.set_system(template_flux.system);
        }
        // Axis Unit is overlaid by the parent Frame/Axis Overlay when set on
        // the template (this is what lets astConvert apply a units scaling
        // between e.g. a "Jy" FluxFrame and a default-unit one).
        if template_flux.test_unit() {
            if let Some(u) = template_flux.unit() {
                result_flux.set_unit(u);
            }
        }
        // SpecVal: copied when the template has it set
        // (fluxframe.c:2636 OVERLAY(SpecVal)).
        if let Some(v) = template_flux.spec_val {
            result_flux.set_spec_val(v);
        }
        return Ok(());
    }

    // Result is not a FluxFrame — fluxframe.c:2602-2611 temporarily
    // clears System and AlignSystem before invoking parent_overlay.
    // Reproduce the observable effect: copy only Frame-level
    // attributes that exist on the result type.
    if template_flux.test_domain() {
        if let Some(bf) = result
            .as_any_mut()
            .downcast_mut::<super::basic::BasicFrame>()
        {
            bf.set_domain(template_flux.domain().to_string());
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
            fluxframe_overlay(t, template_axes, &mut result)?;
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

    // parent_subframe branch (fluxframe.c:3072+). Delegate to the
    // default Frame::SubFrame path which produces a BasicFrame-shaped
    // result.
    super::find_frame::default_sub_frame(target, template, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for FluxFrame {
    fn class_name(&self) -> &'static str {
        "FluxFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of flux coordinate system".to_string());

        // --- Frame-level attributes (before IsA Frame) ---

        // Title (if set and non-empty).
        if let Some(ref title) = self.title {
            if !title.is_empty() {
                node.push_string(
                    "Title",
                    title,
                    Some("Title of coordinate system".to_string()),
                );
            }
        }

        // Naxes = 1 (always).
        node.push_int("Naxes", 1, Some("Number of coordinate axes".to_string()));

        // Domain (if set and != "FLUX", which is the default).
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() && domain != "FLUX" {
                node.push_string(
                    "Domain",
                    domain,
                    Some("Coordinate system domain".to_string()),
                );
            }
        }

        // System (the stored value, independent of any unit-derived default).
        node.push_string(
            "System",
            self.stored_system().as_str(),
            Some("Coordinate system type".to_string()),
        );

        // Per-axis Unit string, when explicitly set, lives in the Ax1 Axis
        // child node (mirrors the C Frame/Axis dump's active `Unit` line).
        if self.test_unit() {
            let u = self.unit_or_default();
            let mut axis_node = Node::new("Axis");
            axis_node.class_comment = Some("Coordinate axis".to_string());
            axis_node.push_string("Unit", u, Some("Axis units".to_string()));
            node.push_object("Ax1", axis_node, Some("Axis number 1".to_string()));
        }

        // ActUnt (if explicitly set to false).
        if let Some(false) = self.act_unt {
            node.push_int(
                "ActUnt",
                0,
                Some("Unit strings do not affect alignment".to_string()),
            );
        }

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- FluxFrame-specific attributes (after IsA Frame) ---

        // SpecVal (SpcVl — spectral position at which flux is measured).
        if let Some(spec_val) = self.spec_val {
            if !is_bad(spec_val) {
                node.push_double("SpcVl", spec_val, Some("Spectral position".to_string()));
            }
        }

        // The encapsulated SpecFrame (SpcFr), only when one is attached
        // (fluxframe.c:3828).
        if let Some(sf) = self.specframe.as_deref() {
            let sf_node = Serializable::to_node(sf)?;
            node.push_object("SpcFr", sf_node, Some("SpcVl coord system".to_string()));
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Read System.
        let sys_str = node.get_string("System", "FLXDN");
        let system = FluxSystem::parse(&sys_str).unwrap_or(FluxSystem::FluxDen);

        let mut ff = FluxFrame::new(system);

        // Title.
        let title = node.get_string("Title", "");
        if !title.is_empty() {
            ff.set_title(&title);
        }

        // Domain.
        let domain = node.get_string("Domain", "");
        if !domain.is_empty() {
            ff.domain = Some(domain);
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

        // ActUnt.
        if node.get_int("ActUnt", 1) == 0 {
            ff.act_unt = Some(false);
        }

        // Ident.
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            ff.ident = Some(ident);
        }

        // Per-axis Unit string from the Ax1 Axis child node.
        if let Some(axis_node) = node.get_object("Ax1") {
            let unit = axis_node.get_string("Unit", "");
            if !unit.is_empty() {
                ff.set_unit(&unit);
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
        f.set_system(FluxSystem::SBright);
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
        assert_eq!(r.system(), FluxSystem::SBright);
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
        // SubFrame returns None for cross-system because the full
        // MakeFluxMapping conversion is not yet ported. Match should
        // therefore fail rather than return a fake mapping.
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
        assert_eq!(r.system(), FluxSystem::SBright);
    }

    #[test]
    fn overlay_template_copies_set_system() {
        let mut template = FluxFrame::new(FluxSystem::FluxDen);
        template.set_system(FluxSystem::SBright);
        assert!(template.test_system());
        let target = FluxFrame::new(FluxSystem::FluxDen);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system(), FluxSystem::SBright);
    }

    #[test]
    fn overlay_template_skips_unset_system() {
        // Default-constructed template (FluxDen) is unset; overlaying
        // it onto an SBright target must leave the target's system.
        let template = FluxFrame::new(FluxSystem::FluxDen);
        assert!(!template.test_system());
        let target = FluxFrame::new(FluxSystem::SBright);
        let mut result: DynFrame = Box::new(target);
        result.overlay_template(&template, &[0]).unwrap();
        let r = result.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system(), FluxSystem::SBright);
    }

    #[test]
    fn pick_axes_snapshot_full_returns_fluxframe() {
        let target = FluxFrame::new(FluxSystem::SBright);
        let snap = target.pick_axes_snapshot(&[0]).unwrap();
        let r = snap.as_any().downcast_ref::<FluxFrame>().unwrap();
        assert_eq!(r.system(), FluxSystem::SBright);
    }
}
