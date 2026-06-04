//! The basic (Cartesian) coordinate frame.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::{ast_equal, is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};
use crate::serial::{Node, NodeValue, Serializable};

use super::{DynFrame, Frame, FrameRef, FrameSet};

/// A basic coordinate frame with Euclidean (Cartesian) geometry.
///
/// `BasicFrame` is the simplest concrete `Frame` implementation. It
/// represents an N-dimensional Cartesian coordinate system with no
/// special axis semantics. All geometry methods use straight-line
/// Euclidean distance.
///
/// Construct via [`BasicFrameBuilder`].
#[derive(Debug, Clone)]
pub struct BasicFrame {
    naxes: usize,
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>,
    labels: Vec<Option<String>>,
    units: Vec<Option<String>>,
    symbols: Vec<Option<String>>,
    format_strs: Vec<Option<String>>,
    bottoms: Vec<Option<f64>>,
    tops: Vec<Option<f64>>,
    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,
}

/// Builder for [`BasicFrame`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{BasicFrameBuilder, Frame};
///
/// let frame = BasicFrameBuilder::new(2)
///     .title("Sky Position")
///     .domain("SKY")
///     .label(1, "X")
///     .label(2, "Y")
///     .build()
///     .unwrap();
///
/// assert_eq!(frame.naxes(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct BasicFrameBuilder {
    naxes: usize,
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>,
    labels: Vec<Option<String>>,
    units: Vec<Option<String>>,
    symbols: Vec<Option<String>>,
    format_strs: Vec<Option<String>>,
    bottoms: Vec<Option<f64>>,
    tops: Vec<Option<f64>>,
    act_unt: Option<bool>,
    ident: Option<String>,
}

impl BasicFrameBuilder {
    /// Create a builder for a frame with `naxes` axes.
    pub fn new(naxes: usize) -> Self {
        Self {
            naxes,
            title: None,
            domain: None,
            epoch: None,
            labels: vec![None; naxes],
            units: vec![None; naxes],
            symbols: vec![None; naxes],
            format_strs: vec![None; naxes],
            bottoms: vec![None; naxes],
            tops: vec![None; naxes],
            act_unt: None,
            ident: None,
        }
    }

    /// Set the frame title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the frame domain.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// Set the frame epoch.
    pub fn epoch(mut self, epoch: f64) -> Self {
        self.epoch = Some(epoch);
        self
    }

    /// Set the label for a 1-based axis.
    pub fn label(mut self, axis: usize, label: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.labels[axis - 1] = Some(label.into());
        }
        self
    }

    /// Set the unit for a 1-based axis.
    pub fn unit(mut self, axis: usize, unit: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.units[axis - 1] = Some(unit.into());
        }
        self
    }

    /// Set the symbol for a 1-based axis.
    pub fn symbol(mut self, axis: usize, symbol: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.symbols[axis - 1] = Some(symbol.into());
        }
        self
    }

    /// Set the format string for a 1-based axis.
    pub fn format_str(mut self, axis: usize, fmt: impl Into<String>) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.format_strs[axis - 1] = Some(fmt.into());
        }
        self
    }

    /// Set the bottom limit for a 1-based axis.
    pub fn bottom(mut self, axis: usize, bottom: f64) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.bottoms[axis - 1] = Some(bottom);
        }
        self
    }

    /// Set the top limit for a 1-based axis.
    pub fn top(mut self, axis: usize, top: f64) -> Self {
        if axis >= 1 && axis <= self.naxes {
            self.tops[axis - 1] = Some(top);
        }
        self
    }

    /// Build the [`BasicFrame`].
    ///
    /// Returns an error if `naxes` is zero.
    pub fn build(self) -> AstResult<BasicFrame> {
        if self.naxes == 0 {
            return Err(AstError::new_tracked(
                ErrorCode::Naxin,
                "number of axes must be at least 1",
            ));
        }
        Ok(BasicFrame {
            naxes: self.naxes,
            title: self.title,
            domain: self.domain,
            epoch: self.epoch,
            labels: self.labels,
            units: self.units,
            symbols: self.symbols,
            format_strs: self.format_strs,
            bottoms: self.bottoms,
            tops: self.tops,
            act_unt: self.act_unt,
            ident: self.ident,
        })
    }
}

impl BasicFrame {
    /// Validate that `axis` is in 1..=naxes, returning an error if not.
    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        if axis < 1 || axis > self.naxes {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..={}", axis, self.naxes),
            ));
        }
        Ok(())
    }

    // --- Frame-level attributes ---

    /// Return the frame title.
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }

    /// Set the frame title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    /// Test whether the title attribute has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.title.is_some()
    }

    /// Clear the title attribute, reverting to the default.
    pub fn clear_title(&mut self) {
        self.title = None;
    }

    /// Return the frame domain.
    pub fn domain(&self) -> &str {
        self.domain.as_deref().unwrap_or("")
    }

    /// Set the frame domain.
    pub fn set_domain(&mut self, domain: impl Into<String>) {
        self.domain = Some(domain.into());
    }

    /// Test whether the domain attribute has been explicitly set.
    pub fn test_domain(&self) -> bool {
        self.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default.
    pub fn clear_domain(&mut self) {
        self.domain = None;
    }

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    /// Used by the FITS-WCS reader to label alternate-plane frames
    /// with their version letter (`'A'..'Z'`).
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Return the frame epoch as a decimal year, or `None` if not
    /// set. `BasicFrame` stores the epoch as a year (matching the
    /// `Epoch = …` value emitted in native serialization); use
    /// `set_epoch_year` / `epoch_year` for the explicit-units API.
    pub fn epoch_year(&self) -> Option<f64> {
        self.epoch
    }

    /// Backwards-compatible accessor; equivalent to `epoch_year`.
    pub fn epoch(&self) -> Option<f64> {
        self.epoch
    }

    /// Set the frame epoch as a decimal year (the unit used in AST
    /// native serialization). The value is stored as-is.
    pub fn set_epoch_year(&mut self, year: f64) {
        self.epoch = Some(year);
    }

    /// Test whether the epoch attribute has been explicitly set.
    pub fn test_epoch(&self) -> bool {
        self.epoch.is_some()
    }

    /// Clear the epoch attribute.
    pub fn clear_epoch(&mut self) {
        self.epoch = None;
    }

    // --- Per-axis attributes (1-based) ---

    /// Return the label for the given 1-based axis.
    pub fn label(&self, axis: usize) -> AstResult<&str> {
        self.validate_axis(axis)?;
        Ok(self.labels[axis - 1].as_deref().unwrap_or(""))
    }

    /// Set the label for the given 1-based axis.
    pub fn set_label(&mut self, axis: usize, label: impl Into<String>) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.labels[axis - 1] = Some(label.into());
        Ok(())
    }

    /// Test whether the label for the given 1-based axis has been explicitly set.
    pub fn test_label(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.labels[axis - 1].is_some())
    }

    /// Clear the label for the given 1-based axis.
    pub fn clear_label(&mut self, axis: usize) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.labels[axis - 1] = None;
        Ok(())
    }

    /// Return the unit for the given 1-based axis.
    pub fn unit(&self, axis: usize) -> AstResult<&str> {
        self.validate_axis(axis)?;
        Ok(self.units[axis - 1].as_deref().unwrap_or(""))
    }

    /// Set the unit for the given 1-based axis.
    pub fn set_unit(&mut self, axis: usize, unit: impl Into<String>) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.units[axis - 1] = Some(unit.into());
        Ok(())
    }

    /// Test whether the unit for the given 1-based axis has been explicitly set.
    pub fn test_unit(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.units[axis - 1].is_some())
    }

    /// Clear the unit for the given 1-based axis.
    pub fn clear_unit(&mut self, axis: usize) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.units[axis - 1] = None;
        Ok(())
    }

    /// Return the symbol for the given 1-based axis.
    ///
    /// If no symbol has been set, falls back to the label; if no label
    /// either, returns an empty string.
    pub fn symbol(&self, axis: usize) -> AstResult<&str> {
        self.validate_axis(axis)?;
        let idx = axis - 1;
        if let Some(s) = &self.symbols[idx] {
            Ok(s.as_str())
        } else if let Some(l) = &self.labels[idx] {
            Ok(l.as_str())
        } else {
            Ok("")
        }
    }

    /// Set the symbol for the given 1-based axis.
    pub fn set_symbol(&mut self, axis: usize, symbol: impl Into<String>) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.symbols[axis - 1] = Some(symbol.into());
        Ok(())
    }

    /// Test whether the symbol for the given 1-based axis has been explicitly set.
    pub fn test_symbol(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.symbols[axis - 1].is_some())
    }

    /// Clear the symbol for the given 1-based axis.
    pub fn clear_symbol(&mut self, axis: usize) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.symbols[axis - 1] = None;
        Ok(())
    }

    /// Return the format string for the given 1-based axis.
    pub fn format_str(&self, axis: usize) -> AstResult<&str> {
        self.validate_axis(axis)?;
        Ok(self.format_strs[axis - 1].as_deref().unwrap_or(""))
    }

    /// Set the format string for the given 1-based axis.
    pub fn set_format_str(&mut self, axis: usize, fmt: impl Into<String>) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.format_strs[axis - 1] = Some(fmt.into());
        Ok(())
    }

    /// Test whether the format string for the given 1-based axis has been explicitly set.
    pub fn test_format_str(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.format_strs[axis - 1].is_some())
    }

    /// Clear the format string for the given 1-based axis.
    pub fn clear_format_str(&mut self, axis: usize) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.format_strs[axis - 1] = None;
        Ok(())
    }

    /// Return the bottom limit for the given 1-based axis.
    ///
    /// Defaults to `-f64::MAX`.
    pub fn bottom(&self, axis: usize) -> AstResult<f64> {
        self.validate_axis(axis)?;
        Ok(self.bottoms[axis - 1].unwrap_or(-f64::MAX))
    }

    /// Set the bottom limit for the given 1-based axis.
    pub fn set_bottom(&mut self, axis: usize, bottom: f64) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.bottoms[axis - 1] = Some(bottom);
        Ok(())
    }

    /// Test whether the bottom limit for the given 1-based axis has been explicitly set.
    pub fn test_bottom(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.bottoms[axis - 1].is_some())
    }

    /// Clear the bottom limit for the given 1-based axis.
    pub fn clear_bottom(&mut self, axis: usize) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.bottoms[axis - 1] = None;
        Ok(())
    }

    /// Return the top limit for the given 1-based axis.
    ///
    /// Defaults to `f64::MAX`.
    pub fn top(&self, axis: usize) -> AstResult<f64> {
        self.validate_axis(axis)?;
        Ok(self.tops[axis - 1].unwrap_or(f64::MAX))
    }

    /// Set the top limit for the given 1-based axis.
    pub fn set_top(&mut self, axis: usize, top: f64) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.tops[axis - 1] = Some(top);
        Ok(())
    }

    /// Test whether the top limit for the given 1-based axis has been explicitly set.
    pub fn test_top(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.tops[axis - 1].is_some())
    }

    /// Clear the top limit for the given 1-based axis.
    pub fn clear_top(&mut self, axis: usize) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.tops[axis - 1] = None;
        Ok(())
    }
}

impl Frame for BasicFrame {
    fn naxes(&self) -> usize {
        self.naxes
    }

    fn system_string(&self) -> Option<&str> {
        Some("Cartesian")
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // If format_str is set and parseable as a precision, use it.
        let fmt = &self.format_strs[axis - 1];
        if let Some(s) = fmt {
            if let Ok(prec) = s.parse::<usize>() {
                return Ok(format!("{value:.prec$}"));
            }
        }
        // Default float formatting.
        Ok(format!("{value}"))
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;
        // Skip leading whitespace.
        let trimmed = text.trim_start();
        let leading_ws = text.len() - trimmed.len();

        if trimmed.is_empty() {
            return Err(AstError::new_tracked(
                ErrorCode::Unfer,
                "empty string cannot be parsed as a coordinate value",
            ));
        }

        // Find the longest prefix that parses as an f64.
        let mut best_len = 0;
        let mut best_val = None;
        // Try progressively longer prefixes of trimmed.
        for end in 1..=trimmed.len() {
            // Only try boundaries that could end a number (not mid-exponent).
            let candidate = &trimmed[..end];
            if let Ok(v) = candidate.parse::<f64>() {
                best_len = end;
                best_val = Some(v);
            }
        }

        match best_val {
            Some(v) => Ok((v, leading_ws + best_len)),
            None => Err(AstError::new_tracked(
                ErrorCode::Unfer,
                format!("cannot parse '{}' as a coordinate value", trimmed),
            )),
        }
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) {
            return Ok(BAD_VALUE);
        }
        let sum: f64 = a
            .iter()
            .zip(b.iter())
            .map(|(&ai, &bi)| (ai - bi) * (ai - bi))
            .sum();
        Ok(sum.sqrt())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) || point_has_bad(c) {
            return Ok(BAD_VALUE);
        }
        // Vectors BA and BC.
        let n = self.naxes;
        let mut dot = 0.0;
        let mut mag_ba = 0.0;
        let mut mag_bc = 0.0;
        for i in 0..n {
            let ba = a[i] - b[i];
            let bc = c[i] - b[i];
            dot += ba * bc;
            mag_ba += ba * ba;
            mag_bc += bc * bc;
        }
        let denom = (mag_ba * mag_bc).sqrt();
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(cos_angle.acos())
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        if point_has_bad(from) || point_has_bad(to) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; self.naxes]);
        }
        let total = self.distance(from, to)?;
        if total == 0.0 || is_bad(total) {
            // Can't determine direction; return from point.
            return Ok(from.to_vec());
        }
        let scale = dist / total;
        let result: Vec<f64> = from
            .iter()
            .zip(to.iter())
            .map(|(&f, &t)| f + scale * (t - f))
            .collect();
        Ok(result)
    }

    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        // No-op for Cartesian frames.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, self.naxes)?;
        fn permute_slice<T: Clone>(v: &mut [T], perm: &[usize]) {
            let old = v.to_vec();
            for (i, &p) in perm.iter().enumerate() {
                v[i] = old[p - 1].clone();
            }
        }
        permute_slice(&mut self.labels, perm);
        permute_slice(&mut self.units, perm);
        permute_slice(&mut self.symbols, perm);
        permute_slice(&mut self.format_strs, perm);
        permute_slice(&mut self.bottoms, perm);
        permute_slice(&mut self.tops, perm);
        Ok(())
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes)))
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<BasicFrame>() else {
            return false;
        };
        if self.naxes != other.naxes {
            return false;
        }
        if self.title != other.title || self.domain != other.domain {
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
        self.labels == other.labels
            && self.units == other.units
            && self.symbols == other.symbols
            && self.format_strs == other.format_strs
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        if self.naxes() == to.naxes() {
            let mut fs = FrameSet::new(self.snapshot());
            fs.add_frame(
                FrameRef::Base,
                Box::new(UnitMap::new(self.naxes)),
                to.snapshot(),
            )?;
            Ok(Some(fs))
        } else {
            Ok(None)
        }
    }

    fn domain(&self) -> &str {
        BasicFrame::domain(self)
    }

    fn test_domain(&self) -> bool {
        BasicFrame::test_domain(self)
    }

    fn ident(&self) -> &str {
        BasicFrame::ident(self)
    }
}

impl Serializable for BasicFrame {
    fn class_name(&self) -> &'static str {
        "Frame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Coordinate system description".to_string());

        // Title (active if set, commented default otherwise).
        if let Some(ref title) = self.title {
            if !title.is_empty() {
                node.push_string(
                    "Title",
                    title,
                    Some("Title of coordinate system".to_string()),
                );
            }
        } else {
            let default_title = format!("{}-d coordinate system", self.naxes);
            node.push_unset(
                "Title",
                NodeValue::String(default_title),
                Some("Title of coordinate system".to_string()),
            );
        }

        // Naxes (always active).
        node.push_int(
            "Naxes",
            self.naxes as i64,
            Some("Number of coordinate axes".to_string()),
        );

        // Domain (active if set and non-empty).
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() {
                node.push_string(
                    "Domain",
                    domain,
                    Some("Coordinate system domain".to_string()),
                );
            }
        }

        // Epoch (active if set).
        if let Some(epoch) = self.epoch {
            node.push_double(
                "Epoch",
                epoch,
                Some("Julian epoch of observation".to_string()),
            );
        }

        // Per-axis Label (always commented/unset).
        // Shows the explicit label if set, otherwise default "Axis N".
        for i in 0..self.naxes {
            let label_val = match &self.labels[i] {
                Some(label) => label.clone(),
                None => format!("Axis {}", i + 1),
            };
            let key = format!("Lbl{}", i + 1);
            let comment = format!("Label for axis {}", i + 1);
            node.push_unset(&key, NodeValue::String(label_val), Some(comment));
        }

        // Per-axis Unit (commented/unset — only emitted when set and non-empty).
        for i in 0..self.naxes {
            if let Some(ref unit) = self.units[i] {
                if !unit.is_empty() {
                    let key = format!("Uni{}", i + 1);
                    let comment = format!("Units for axis {}", i + 1);
                    node.push_unset(&key, NodeValue::String(unit.clone()), Some(comment));
                }
            }
        }

        // ActUnt: active if explicitly set to false.
        if let Some(false) = self.act_unt {
            node.push_int(
                "ActUnt",
                0,
                Some("Unit strings do not affect alignment".to_string()),
            );
        }

        // Per-axis Axis child objects.
        for i in 0..self.naxes {
            let mut axis_node = Node::new("Axis");
            axis_node.class_comment = Some("Coordinate axis".to_string());

            // Label.
            if let Some(ref label) = self.labels[i] {
                axis_node.push_string("Label", label, Some("Axis Label".to_string()));
            }

            // Symbol.
            if let Some(ref symbol) = self.symbols[i] {
                axis_node.push_string("Symbol", symbol, Some("Axis symbol".to_string()));
            }

            // Unit.
            if let Some(ref unit) = self.units[i] {
                axis_node.push_string("Unit", unit, Some("Axis units".to_string()));
            }

            // Format.
            if let Some(ref fmt) = self.format_strs[i] {
                axis_node.push_string("Format", fmt, Some("Format specifier".to_string()));
            }

            let key = format!("Ax{}", i + 1);
            let comment = format!("Axis number {}", i + 1);
            node.push_object(&key, axis_node, Some(comment));
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let naxes = node.get_int("Naxes", 0) as usize;
        if naxes == 0 {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "Frame: Naxes is 0 or missing",
            ));
        }

        let mut builder = BasicFrameBuilder::new(naxes);

        // Title.
        let title = node.get_string("Title", "");
        if !title.is_empty() {
            builder = builder.title(title);
        }

        // Domain.
        let domain = node.get_string("Domain", "");
        if !domain.is_empty() {
            builder = builder.domain(domain);
        }

        // Epoch.
        if node.get("Epoch").is_some() {
            let epoch = node.get_double("Epoch", 0.0);
            builder = builder.epoch(epoch);
        }

        // ActUnt (default is 1 = true; only store when 0).
        let act_unt_val = node.get_int("ActUnt", 1);
        if act_unt_val == 0 {
            builder.act_unt = Some(false);
        }

        // Ident (Object-level identification string).
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            builder.ident = Some(ident);
        }

        // Per-axis attributes from Axis child objects.
        for i in 0..naxes {
            let key = format!("Ax{}", i + 1);
            if let Some(axis_node) = node.get_object(&key) {
                let label = axis_node.get_string("Label", "");
                if !label.is_empty() {
                    builder = builder.label(i + 1, label);
                }

                let symbol = axis_node.get_string("Symbol", "");
                if !symbol.is_empty() {
                    builder = builder.symbol(i + 1, symbol);
                }

                let unit = axis_node.get_string("Unit", "");
                if !unit.is_empty() {
                    builder = builder.unit(i + 1, unit);
                }

                let fmt = axis_node.get_string("Format", "");
                if !fmt.is_empty() {
                    builder = builder.format_str(i + 1, fmt);
                }
            }
        }

        builder.build()
    }
}
