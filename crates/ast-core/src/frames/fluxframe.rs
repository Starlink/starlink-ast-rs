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
    spec_val: Option<f64>, // spectral position at which flux is measured

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
            spec_val: None,
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
    pub fn system(&self) -> FluxSystem {
        self.system
    }

    /// Set the flux coordinate system.
    pub fn set_system(&mut self, sys: FluxSystem) {
        self.system = sys;
        self.system_set = true;
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
        self.system.default_units().to_string()
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

        // Build the conversion Mapping. The full FluxFrame ↔
        // FluxFrame conversion (MakeFluxMapping) requires the
        // SpecFrame defining the spectral position, which we don't
        // wire up in this port. For same-system targets the result
        // is identity; for different systems we return None to
        // signal "not yet supported" — same behaviour as the
        // existing FluxFrame::convert. This deliberately does not
        // paper over the gap with a fake mapping.
        if target.system == result.system {
            let map: DynMapping = Box::new(UnitMap::new(1));
            return Ok(Some((map, Box::new(result))));
        }
        return Ok(None);
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

        // System.
        node.push_string(
            "System",
            self.system.as_str(),
            Some("Coordinate system type".to_string()),
        );

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

        // ActUnt.
        if node.get_int("ActUnt", 1) == 0 {
            ff.act_unt = Some(false);
        }

        // Ident.
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            ff.ident = Some(ident);
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
