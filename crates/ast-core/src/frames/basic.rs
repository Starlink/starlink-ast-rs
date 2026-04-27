//! The basic (Cartesian) coordinate frame.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};

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

    /// Return the frame epoch, or `None` if not set.
    pub fn epoch(&self) -> Option<f64> {
        self.epoch
    }

    /// Set the frame epoch.
    pub fn set_epoch(&mut self, epoch: f64) {
        self.epoch = Some(epoch);
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

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes)))
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
}
