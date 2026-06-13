//! The basic (Cartesian) coordinate frame.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};
use crate::serial::{Node, NodeValue, Serializable};

use super::{DynFrame, Frame};

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
    digits: Option<i32>,
    axis_digits: Vec<Option<i32>>,
    directions: Vec<Option<bool>>,
    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
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
    digits: Option<i32>,
    axis_digits: Vec<Option<i32>>,
    directions: Vec<Option<bool>>,
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
            digits: None,
            axis_digits: vec![None; naxes],
            directions: vec![None; naxes],
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
            digits: self.digits,
            axis_digits: self.axis_digits,
            directions: self.directions,
            act_unt: self.act_unt,
            ident: self.ident,
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
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

    /// Frame-level Digits, defaulting to 7. Mirrors `astGetDigits`.
    pub fn digits(&self) -> i32 {
        self.digits.unwrap_or(7)
    }

    /// Set frame-level Digits, constrained to >= 1 (frame.c:13138).
    pub fn set_digits(&mut self, value: i32) {
        self.digits = Some(value.max(1));
    }

    /// Test whether frame-level Digits is explicitly set.
    pub fn test_digits(&self) -> bool {
        self.digits.is_some()
    }

    /// Per-axis Digits (1-based), falling back to frame-level Digits.
    pub fn axis_digits(&self, axis: usize) -> AstResult<i32> {
        self.validate_axis(axis)?;
        Ok(self.axis_digits[axis - 1].unwrap_or_else(|| self.digits()))
    }

    /// Set per-axis Digits (1-based), constrained to >= 1.
    pub fn set_axis_digits(&mut self, axis: usize, value: i32) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.axis_digits[axis - 1] = Some(value.max(1));
        Ok(())
    }

    /// Test whether per-axis Digits is explicitly set (1-based).
    pub fn test_axis_digits(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.axis_digits[axis - 1].is_some())
    }

    /// Per-axis Direction Plot hint (1-based); default true.
    pub fn direction(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.directions[axis - 1].unwrap_or(true))
    }

    /// Set per-axis Direction (1-based).
    pub fn set_direction(&mut self, axis: usize, value: bool) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.directions[axis - 1] = Some(value);
        Ok(())
    }
}

/// Match the leading "<bad>" token of `s` using the C scanf format
/// `" < %*1[Bb] %*1[Aa] %*1[Dd] > %n"` (axis.c:1357) and return the
/// `%n` count: the number of bytes consumed up to and including any
/// trailing whitespace. Whitespace is allowed (and skipped) before each
/// of the tokens `<`, `b`, `a`, `d`, `>` and after the closing `>`;
/// matching stops at the first non-whitespace following `>`. Returns
/// `None` if the token does not match (scanf would leave `%n` unset).
fn scan_bad_token(s: &str) -> Option<usize> {
    // The byte at `pos` after skipping any leading whitespace must equal
    // the (case-insensitively matched) expected character. `%*1[Bb]`
    // matches a single B or b; `<` and `>` are literals (a literal in a
    // scanf format does not skip whitespace itself, but the spaces around
    // them do). Returns the byte position just past the matched char.
    fn match_char(s: &str, pos: usize, expected: &[char]) -> Option<usize> {
        let mut p = pos;
        let bytes = s.as_bytes();
        while p < bytes.len() && (bytes[p] as char).is_whitespace() {
            p += 1;
        }
        let c = s[p..].chars().next()?;
        if expected.contains(&c) {
            Some(p + c.len_utf8())
        } else {
            None
        }
    }

    let p = match_char(s, 0, &['<'])?;
    let p = match_char(s, p, &['B', 'b'])?;
    let p = match_char(s, p, &['A', 'a'])?;
    let p = match_char(s, p, &['D', 'd'])?;
    let mut p = match_char(s, p, &['>'])?;

    // Trailing whitespace skipped by the space before `%n` is counted.
    let bytes = s.as_bytes();
    while p < bytes.len() && (bytes[p] as char).is_whitespace() {
        p += 1;
    }
    Some(p)
}

impl Frame for BasicFrame {
    fn naxes(&self) -> usize {
        self.naxes
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // Casting a plain Frame to another plain Frame is a same-class
        // copy (object.c Cast: generation_gap == 0 -> astCopy). Any other
        // target class is below the Frame base, so the cast fails.
        if other.as_any().downcast_ref::<BasicFrame>().is_some() {
            Ok(Some(Box::new(BasicFrame::clone(self))))
        } else {
            Ok(None)
        }
    }

    fn system_string(&self) -> Option<&str> {
        Some("Cartesian")
    }

    fn digits(&self) -> i32 {
        BasicFrame::digits(self)
    }

    fn set_digits(&mut self, value: i32) {
        BasicFrame::set_digits(self, value);
    }

    fn test_digits(&self) -> bool {
        BasicFrame::test_digits(self)
    }

    fn axis_digits(&self, axis: usize) -> i32 {
        BasicFrame::axis_digits(self, axis).unwrap_or(7)
    }

    fn direction(&self, axis: usize) -> bool {
        BasicFrame::direction(self, axis).unwrap_or(true)
    }

    fn get_active_unit(&self) -> bool {
        // C default is false; BasicFrame has no SkyAxis (frame.c:4651).
        self.act_unt.unwrap_or(false)
    }

    fn set_active_unit(&mut self, value: bool) {
        self.act_unt = Some(value);
    }

    fn test_active_unit(&self) -> bool {
        self.act_unt.is_some()
    }

    fn effective_label(&self, axis: usize) -> String {
        match BasicFrame::label(self, axis) {
            Ok(s) if !s.is_empty() => s.to_string(),
            _ => format!("Axis {axis}"),
        }
    }

    fn effective_unit(&self, axis: usize) -> String {
        BasicFrame::unit(self, axis).unwrap_or("").to_string()
    }

    fn effective_symbol(&self, axis: usize) -> String {
        // BasicFrame::symbol already falls back from Symbol to Label;
        // when neither is set, fall back to the default label.
        match BasicFrame::symbol(self, axis) {
            Ok(s) if !s.is_empty() => s.to_string(),
            _ => format!("Axis {axis}"),
        }
    }

    fn effective_format(&self, axis: usize) -> String {
        match BasicFrame::test_format_str(self, axis) {
            Ok(true) => BasicFrame::format_str(self, axis).unwrap_or("").to_string(),
            _ => String::new(),
        }
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        // C AxisFormat returns "<bad>" for the bad value (axis.c:704).
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // Resolve the effective Format string: the per-axis Format if
        // set, else the default "%1.<Digits>G" with the per-axis Digits
        // (frame.c:4260 -> GetDefaultFormat axis.c:1819). A SET but
        // non-empty Format is passed through verbatim — including one with
        // no '%', which C hands straight to sprintf so the value is ignored
        // and the literal string is returned (axis.c:780, ParseAxisFormat
        // axis.c:2127 returns the unaltered copy when no '%' is present).
        let digits = self.axis_digits(axis)?.max(1) as usize;
        let fmt = match &self.format_strs[axis - 1] {
            Some(s) if !s.is_empty() => s.clone(),
            _ => format!("%1.{digits}G"),
        };
        Ok(crate::frames::axis_format::format_value(
            &fmt, digits, value,
        ))
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;

        // Mirror C `astSscanf("%lf %n")`: skip leading whitespace, read
        // the longest numeric prefix, then count any trailing whitespace.
        let leading_ws = text.len() - text.trim_start().len();
        let after_ws = &text[leading_ws..];

        // Longest prefix of `after_ws` parsing as f64.
        let mut best_len = 0;
        let mut best_val: Option<f64> = None;
        for end in 1..=after_ws.len() {
            if after_ws.is_char_boundary(end) {
                if let Ok(v) = after_ws[..end].parse::<f64>() {
                    best_len = end;
                    best_val = Some(v);
                }
            }
        }

        if let Some(v) = best_val {
            // Count trailing whitespace consumed by `%n` after `%lf`.
            let rest = &after_ws[best_len..];
            let trailing_ws = rest.len() - rest.trim_start().len();
            return Ok((v, leading_ws + best_len + trailing_ws));
        }

        // Recognise "<bad>" via the C scanf format
        // `" < %*1[Bb] %*1[Aa] %*1[Dd] > %n"` (axis.c:1357). Each space in
        // that format skips optional whitespace, so the tokens `<`, `b`,
        // `a`, `d`, `>` may be separated by whitespace. The trailing space
        // before `%n` skips trailing whitespace, so `%n` (nread) counts
        // that trailing whitespace too — it stops at the first
        // non-whitespace after `>`.
        if let Some(end) = scan_bad_token(after_ws) {
            return Ok((BAD_VALUE, leading_ws + end));
        }

        // Parse failure: nread = 0, no error (axis.c:1366).
        Ok((BAD_VALUE, 0))
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
        // Frame-class algorithm (frame.c:1230-1320): signed in 2-D,
        // AST__BAD for bad inputs or zero-length vectors.
        Ok(super::cartesian_angle(a, b, c))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        if point_has_bad(from) || point_has_bad(to) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; self.naxes]);
        }
        let total = self.distance(from, to)?;
        if is_bad(total) {
            return Ok(vec![BAD_VALUE; self.naxes]);
        }
        // Coincident points define no direction: C returns bad
        // coordinates unless the requested offset is zero
        // (frame.c:8470-8490).
        if total == 0.0 {
            if dist != 0.0 {
                return Ok(vec![BAD_VALUE; self.naxes]);
            }
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

    // Match-control attributes — mirror C `astGet/SetMinAxes` etc.
    fn min_axes(&self) -> usize {
        self.min_axes_opt.unwrap_or(self.naxes)
    }
    fn set_min_axes(&mut self, n: usize) {
        self.min_axes_opt = Some(n);
    }
    fn test_min_axes(&self) -> bool {
        self.min_axes_opt.is_some()
    }
    fn max_axes(&self) -> usize {
        self.max_axes_opt.unwrap_or(self.naxes)
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
        // Plain Frame's Permute defaults to true (1) — see frame.c.
        self.permute_opt.unwrap_or(true)
    }
    fn set_permute(&mut self, v: bool) {
        self.permute_opt = Some(v);
    }
    fn test_permute(&self) -> bool {
        self.permute_opt.is_some()
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
        permute_slice(&mut self.axis_digits, perm);
        permute_slice(&mut self.directions, perm);
        Ok(())
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes)))
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549): same class,
        // astConvert succeeds, and the mapping simplifies to a UnitMap.
        super::find_frame::frames_equal(self, other)
    }

    fn domain(&self) -> &str {
        BasicFrame::domain(self)
    }

    fn set_domain(&mut self, domain: &str) {
        BasicFrame::set_domain(self, domain.to_string());
    }

    fn test_domain(&self) -> bool {
        BasicFrame::test_domain(self)
    }

    fn ident(&self) -> &str {
        BasicFrame::ident(self)
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        super::find_frame::default_match(self, target, match_sub)
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        _result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        super::find_frame::default_sub_frame(self, template, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        let mut snap = BasicFrameBuilder::new(target_axes.len()).build()?;
        if self.test_domain() {
            snap.set_domain(self.domain());
        }
        if let Some(ref title) = self.title {
            snap.set_title(title.clone());
        }
        for (i, &t) in target_axes.iter().enumerate() {
            if t < 0 || (t as usize) >= self.naxes {
                continue;
            }
            let src = t as usize;
            if let Some(ref l) = self.labels[src] {
                let _ = snap.set_label(i + 1, l.clone());
            }
            if let Some(ref u) = self.units[src] {
                let _ = snap.set_unit(i + 1, u.clone());
            }
            if let Some(ref s) = self.symbols[src] {
                let _ = snap.set_symbol(i + 1, s.clone());
            }
            if let Some(ref f) = self.format_strs[src] {
                let _ = snap.set_format_str(i + 1, f.clone());
            }
        }
        Ok(Box::new(snap))
    }

    fn overlay_template(&mut self, template: &dyn Frame, _template_axes: &[i32]) -> AstResult<()> {
        // Mirrors C `Frame::Overlay`: copy any template attribute that
        // has been explicitly set onto the target. For BasicFrame the
        // only Frame-level attribute we care about here is Domain;
        // per-axis attributes flow through `pick_axes_snapshot`.
        if template.test_domain() {
            self.set_domain(template.domain().to_string());
        }
        // Transfer the ActiveUnit flag (frame.c:8578).
        self.set_active_unit(template.get_active_unit());
        Ok(())
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

        // Digits (frame-level, active if set).
        if let Some(d) = self.digits {
            node.push_int(
                "Digits",
                d as i64,
                Some("Default formatting precision".to_string()),
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

        // ActUnt: written only when explicitly set, with its value
        // (frame.c:14614).
        if let Some(v) = self.act_unt {
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

            // Per-axis Digits.
            if let Some(d) = self.axis_digits[i] {
                axis_node.push_int(
                    "Digits",
                    d as i64,
                    Some("Default formatting precision".to_string()),
                );
            }

            // Direction (Plot hint).
            if let Some(dir) = self.directions[i] {
                axis_node.push_int(
                    "Dirn",
                    if dir { 1 } else { 0 },
                    Some(if dir {
                        "Plot in conventional direction (hint)".to_string()
                    } else {
                        "Plot in reverse direction (hint)".to_string()
                    }),
                );
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

        // Digits (frame-level).
        if node.get("Digits").is_some() {
            builder.digits = Some(node.get_int("Digits", 7) as i32);
        }

        // ActUnt: stored only when the token is present, with its value
        // (frame.c:14614). Absent => unset (default false).
        if node.get("ActUnt").is_some() {
            builder.act_unt = Some(node.get_int("ActUnt", 0) != 0);
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

                if axis_node.get("Digits").is_some() {
                    builder.axis_digits[i] = Some(axis_node.get_int("Digits", 7) as i32);
                }
                if axis_node.get("Dirn").is_some() {
                    builder.directions[i] = Some(axis_node.get_int("Dirn", 1) != 0);
                }
            }
        }

        builder.build()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;

    fn bf(naxes: usize) -> BasicFrame {
        BasicFrameBuilder::new(naxes).build().unwrap()
    }

    #[test]
    fn sub_frame_select_2_of_3_axes() {
        // Pick axes (1, 3) (zero-based 0, 2) from a 3-axis BasicFrame.
        let target = bf(3);
        let target_axes: Vec<i32> = vec![0, 2];
        let template_axes: Vec<i32> = vec![0, 1];
        let (map, frame) = target
            .sub_frame(None, 2, &target_axes, &template_axes)
            .unwrap()
            .unwrap();
        assert_eq!(frame.naxes(), 2);
        assert_eq!(map.nin(), 3);
        assert_eq!(map.nout(), 2);
        let pts = PointSet::from_rows([[10.0_f64, 20.0, 30.0]]);
        let out = map.transform(&pts, true).unwrap();
        let row = out.point(0);
        assert!((row[0] - 10.0).abs() < 1e-12);
        assert!((row[1] - 30.0).abs() < 1e-12);
    }

    #[test]
    fn match_template_basic_2axes_2axes() {
        let template = bf(2);
        let target = bf(2);
        let m = template.match_template(&target, false).unwrap().unwrap();
        assert_eq!(m.template_axes, vec![0, 1]);
        assert_eq!(m.target_axes, vec![0, 1]);
        assert_eq!(m.result.naxes(), 2);
        assert_eq!(m.map.nin(), 2);
        assert_eq!(m.map.nout(), 2);
    }

    #[test]
    fn match_template_basic_axis_mismatch() {
        let template = bf(3);
        let target = bf(2);
        let m = template.match_template(&target, false).unwrap();
        assert!(m.is_none(), "MaxAxes/MinAxes default to template's naxes");
    }

    #[test]
    fn match_template_basic_domain_mismatch() {
        let mut template = bf(2);
        template.set_domain("A");
        let mut target = bf(2);
        target.set_domain("B");
        let m = template.match_template(&target, false).unwrap();
        assert!(m.is_none(), "Domains differ; should fail to match");
    }

    #[test]
    fn digits_direction_round_trip() {
        let mut f = BasicFrameBuilder::new(2).build().unwrap();
        f.set_digits(9);
        f.set_axis_digits(1, 4).unwrap();
        f.set_direction(2, false).unwrap();

        let node = f.to_node().unwrap();
        let back = BasicFrame::from_node(&node).unwrap();
        assert_eq!(back.digits(), 9);
        assert_eq!(back.axis_digits(1).unwrap(), 4);
        assert!(!back.direction(2).unwrap());
    }
}
