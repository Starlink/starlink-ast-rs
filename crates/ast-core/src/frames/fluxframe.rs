//! The flux coordinate frame (FluxFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{ast_equal, is_bad};
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
    spec_val: Option<f64>, // spectral position at which flux is measured

    // Object-level attributes
    act_unt: Option<bool>,
    ident: Option<String>,
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
            spec_val: None,
            act_unt: None,
            ident: None,
        }
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

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let to_flux = to.as_any().downcast_ref::<FluxFrame>();
        match to_flux {
            Some(to_flux) => {
                if self.system == to_flux.system {
                    // Same system — conversion is identity.
                    let mut fs = FrameSet::new(self.snapshot());
                    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(1)), to.snapshot())?;
                    Ok(Some(fs))
                } else {
                    // Different flux systems. Full conversion requires the
                    // spectral position and is complex. For now, return None
                    // (incompatible).
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<FluxFrame>() else {
            return false;
        };
        if self.system != other.system {
            return false;
        }
        let spec_eq = match (self.spec_val, other.spec_val) {
            (Some(a), Some(b)) => ast_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if !spec_eq {
            return false;
        }
        self.title == other.title && self.domain == other.domain
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
