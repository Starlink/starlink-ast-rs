//! Per-axis display properties for celestial coordinate axes.
//!
//! `SkyAxis` is an internal helper used by `SkyFrame` to control how
//! coordinate values are displayed (sexagesimal HMS or DMS) and parsed.

// This module is a building block for SkyFrame (Task 4). All items will
// have callers once SkyFrame lands; suppress dead_code until then.
#![allow(dead_code)]

use std::f64::consts::PI;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::is_bad;
use crate::serial::{Node, Serializable};

/// Per-axis display properties for a sky coordinate axis.
#[derive(Debug, Clone)]
pub(crate) struct SkyAxis {
    /// Format as time (hours) rather than angle (degrees).
    pub as_time: Option<bool>,
    /// This is a latitude axis (constrains to +/-90 degrees).
    pub is_latitude: bool,
    /// Normalize to [-pi,pi] (true) or [0,2pi] (false).
    pub centre_zero: Option<bool>,
}

impl SkyAxis {
    /// Create a longitude axis with default properties.
    ///
    /// Defaults: `as_time` = true (hours), `centre_zero` = false ([0,2pi]).
    pub fn longitude() -> Self {
        Self {
            as_time: None,
            is_latitude: false,
            centre_zero: None,
        }
    }

    /// Create a latitude axis with default properties.
    ///
    /// Defaults: `as_time` = false (degrees), `centre_zero` = true ([-pi,pi]).
    pub fn latitude() -> Self {
        Self {
            as_time: None,
            is_latitude: true,
            centre_zero: None,
        }
    }

    /// Return the effective `as_time` setting.
    ///
    /// If not explicitly set, longitude axes default to time (hours) and
    /// latitude axes default to angle (degrees).
    pub fn effective_as_time(&self) -> bool {
        self.as_time.unwrap_or(!self.is_latitude)
    }

    /// Return the effective `centre_zero` setting.
    ///
    /// If not explicitly set, latitude axes default to true ([-pi,pi]) and
    /// longitude axes default to false ([0,2pi]).
    pub fn effective_centre_zero(&self) -> bool {
        self.centre_zero.unwrap_or(self.is_latitude)
    }

    /// The kind of Axis this is, as [`Frame::get_axis`] reports it.
    ///
    /// Resolves the attributes that drive the `astAxis*` methods to their
    /// effective values, which is what C's accessors return when the Axis is
    /// interrogated through the fetched pointer.
    ///
    /// [`Frame::get_axis`]: super::Frame::get_axis
    pub fn kind(&self) -> super::AxisKind {
        super::AxisKind::Sky {
            is_latitude: self.is_latitude,
            centre_zero: self.effective_centre_zero(),
        }
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for SkyAxis {
    fn class_name(&self) -> &'static str {
        "SkyAxis"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Celestial coordinate axis".to_string());

        if let Some(cz) = self.centre_zero {
            if cz {
                node.push_int("CnZer", 1, Some("Centre zero for axis range".to_string()));
            }
        }

        if let Some(at) = self.as_time {
            node.push_int(
                "AsTime",
                if at { 1 } else { 0 },
                Some("Format as time".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let is_lat = node.get_int("IsLat", 0) != 0;

        let mut axis = if is_lat {
            SkyAxis::latitude()
        } else {
            SkyAxis::longitude()
        };

        // CnZer: only set if present in the node.
        if node.get("CnZer").is_some() {
            axis.centre_zero = Some(node.get_int("CnZer", 0) != 0);
        }

        // AsTime: only set if present in the node.
        if node.get("AsTime").is_some() {
            axis.as_time = Some(node.get_int("AsTime", 0) != 0);
        }

        Ok(axis)
    }
}

/// Format a value in radians as sexagesimal (HMS or DMS).
///
/// - `as_time=true`: output `HH:MM:SS.sss` (24h = 2pi radians)
/// - `as_time=false`: output `+DD:MM:SS.sss` or `-DD:MM:SS.sss`
/// - `precision`: decimal places on the seconds field
///
/// If `value` is `BAD_VALUE` or NaN, returns `"<bad>"`.
/// Normalise a right-ascension (or any 2π-cyclic angle) into `[0, 2π)`.
///
/// Mirrors `eraAnp` (SOFA "Angle, Normalize, Positive"): the canonical
/// range used by SLALIB / palGaleq for celestial RA values stored in
/// `SkyFrame` and `SpecFrame::ref_ra`. Values that come out of an
/// `atan2`-based pipeline (e.g. our SlaMap's Cartesian round-trip)
/// land in `(-π, π]`; without this normalisation they don't match
/// the C library's f64 storage bit-for-bit even when the angle is
/// physically identical.
pub(crate) fn normalise_ra(angle: f64) -> f64 {
    let two_pi = 2.0 * std::f64::consts::PI;
    let r = angle.rem_euclid(two_pi);
    // rem_euclid can return exactly two_pi for tiny negative inputs
    // due to FP rounding; clamp to strictly < 2π to match `eraAnp`.
    if r >= two_pi { 0.0 } else { r }
}

/// Normalise a declination-style angle into `(-π, π]`.
///
/// Mirrors `eraAnpm` (SOFA "Angle, Normalize, Plus/Minus pi"). Used by
/// `SpecFrame::set_ref_pos` for `RefDec` so the stored value matches C
/// after a coordinate-system conversion.
pub(crate) fn normalise_dec(angle: f64) -> f64 {
    // Mirror `eraAnpm` exactly: `w = fmod(a, 2π); if |w| >= π { w -= sign(2π, a) }`.
    // `fmod` is symmetric (truncates toward zero), so for an angle already in
    // (-π, π) it returns the input bit-for-bit — unlike `rem_euclid`, whose
    // always-non-negative result forces a small negative angle through
    // `2π − |a|` and then back, losing low-order bits (~26 ULP on a ~0.01 rad
    // declination). The bit-exact pass-through matters for RefDec round-trips.
    let two_pi = 2.0 * std::f64::consts::PI;
    let w = angle % two_pi;
    if w.abs() >= std::f64::consts::PI {
        w - two_pi.copysign(angle)
    } else {
        w
    }
}

pub(crate) fn format_sky(value: f64, as_time: bool, precision: usize) -> String {
    if is_bad(value) {
        return "<bad>".to_string();
    }

    if as_time {
        format_hms(value, precision)
    } else {
        format_dms(value, precision)
    }
}

/// Format radians as hours:minutes:seconds.
fn format_hms(value: f64, precision: usize) -> String {
    // Convert radians to hours. C's astFormat decomposes via palDr2tf →
    // eraA2tf/eraD2tf, which does NOT range-reduce the angle (ERFA a2tf
    // note 3: "the absolute value of angle may exceed 2pi"). A value > 24h
    // therefore formats verbatim (e.g. an offset SkyRef of 458:21:58.4),
    // and a negative value carries a leading "-".
    let hours = value * 12.0 / PI;
    let sign = if hours < 0.0 { "-" } else { "" };
    let abs_hours = hours.abs();

    // Determine the rounding limit for seconds based on precision.
    let sec_limit = round_limit(precision);

    let h = abs_hours.floor() as u32;
    let rem = (abs_hours - h as f64) * 60.0;
    let mut m = rem.floor() as u32;
    let mut s = (rem - m as f64) * 60.0;

    // Handle rounding overflow: if seconds round up to >= 60, carry.
    if s >= sec_limit {
        s = 0.0;
        m += 1;
    }
    let mut h_out = h;
    if m >= 60 {
        m = 0;
        h_out += 1;
    }

    if precision == 0 {
        format!("{sign}{h_out}:{m:02}:{s:02.0}")
    } else {
        // Total width = 2 (min digits) + 1 (decimal point) + precision.
        let width = 3 + precision;
        format!("{sign}{h_out}:{m:02}:{s:0width$.precision$}")
    }
}

/// Format radians as degrees:minutes:seconds.
///
/// A leading `-` is emitted for negative values; positive values carry no
/// sign, matching C `astFormat` for a `d`/`dms` axis (sla DR2AF returns a
/// sign character of `+` or `-`, but the AST sexagesimal formatter only
/// prints it when negative).
fn format_dms(value: f64, precision: usize) -> String {
    // Convert radians to degrees.
    let deg = value * 180.0 / PI;

    let sign = if deg < 0.0 { "-" } else { "" };
    let abs_deg = deg.abs();

    // Determine the rounding limit for seconds based on precision.
    let sec_limit = round_limit(precision);

    let d = abs_deg.floor() as u32;
    let rem = (abs_deg - d as f64) * 60.0;
    let mut m = rem.floor() as u32;
    let mut s = (rem - m as f64) * 60.0;

    // Handle rounding overflow.
    if s >= sec_limit {
        s = 0.0;
        m += 1;
    }
    let mut d_out = d;
    if m >= 60 {
        m = 0;
        d_out += 1;
    }

    if precision == 0 {
        format!("{sign}{d_out}:{m:02}:{s:02.0}")
    } else {
        let width = 3 + precision;
        format!("{sign}{d_out}:{m:02}:{s:0width$.precision$}")
    }
}

/// Compute the threshold at which seconds should be rounded up to 60.
///
/// For precision=0, seconds >= 59.5 round to 60. For precision=1,
/// seconds >= 59.95 round to 60, etc.
fn round_limit(precision: usize) -> f64 {
    60.0 - 0.5 * 10.0_f64.powi(-(precision as i32))
}

/// Parse a sexagesimal string back to radians.
///
/// Accepts:
/// - `HH:MM:SS.ss` or `+/-DD:MM:SS.ss` (colon-separated)
/// - `HH MM SS.ss` or `+/-DD MM SS.ss` (space-separated)
/// - `DD.ddd` or `HH.hhh` (decimal degrees/hours)
///
/// Returns `(value_in_radians, characters_consumed)`.
pub(crate) fn unformat_sky(text: &str, as_time: bool) -> AstResult<(f64, usize)> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    // Skip leading whitespace.
    let mut pos = 0;
    while pos < len && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }

    if pos >= len {
        return Err(AstError::new_tracked(
            ErrorCode::Unfer,
            "empty string cannot be parsed as a sky coordinate",
        ));
    }

    // Parse optional sign.
    let negative = match bytes[pos] {
        b'-' => {
            pos += 1;
            true
        }
        b'+' => {
            pos += 1;
            false
        }
        _ => false,
    };

    // Parse first number (integer or decimal).
    let (first, first_end, first_is_int) = parse_number(text, pos)?;
    pos = first_end;

    // Check for separator (colon or space+digit).
    let (value_in_units, final_pos) = if pos < len && bytes[pos] == b':' {
        // Colon-separated sexagesimal.
        let (val, end) = parse_sexagesimal_fields(text, first, pos)?;
        (val, end)
    } else if pos < len
        && bytes[pos] == b' '
        && first_is_int
        && pos + 1 < len
        && bytes[pos + 1].is_ascii_digit()
    {
        // Space-separated sexagesimal.
        let (val, end) = parse_sexagesimal_fields(text, first, pos)?;
        (val, end)
    } else {
        // Decimal degrees/hours (the first number is the whole value).
        (first, pos)
    };

    // Convert to radians.
    let sign = if negative { -1.0 } else { 1.0 };
    let radians = if as_time {
        sign * value_in_units * PI / 12.0
    } else {
        sign * value_in_units * PI / 180.0
    };

    Ok((radians, final_pos))
}

/// Parse a number (integer or decimal) starting at `pos` in `text`.
///
/// Returns `(value, end_position, is_integer)`.
fn parse_number(text: &str, start: usize) -> AstResult<(f64, usize, bool)> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut pos = start;
    let mut has_dot = false;

    if pos >= len || (!bytes[pos].is_ascii_digit() && bytes[pos] != b'.') {
        return Err(AstError::new_tracked(
            ErrorCode::Unfer,
            format!(
                "expected a number at position {} in '{}'",
                start,
                &text[start..]
            ),
        ));
    }

    while pos < len {
        if bytes[pos].is_ascii_digit() {
            pos += 1;
        } else if bytes[pos] == b'.' && !has_dot {
            has_dot = true;
            pos += 1;
        } else {
            break;
        }
    }

    let num_str = &text[start..pos];
    let value: f64 = num_str.parse().map_err(|_| {
        AstError::new_tracked(
            ErrorCode::Unfer,
            format!("cannot parse '{}' as a number", num_str),
        )
    })?;

    Ok((value, pos, !has_dot))
}

/// Parse minutes and seconds fields after the first number and separator.
///
/// `first` is the already-parsed hours/degrees value.
/// `sep_pos` is the position of the separator (`:` or space).
///
/// Returns `(total_in_hours_or_degrees, end_position)`.
fn parse_sexagesimal_fields(text: &str, first: f64, sep_pos: usize) -> AstResult<(f64, usize)> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    // Skip the separator.
    let pos = sep_pos + 1;

    // Parse minutes.
    let (minutes, mut pos, _) = parse_number(text, pos)?;

    // Optionally parse seconds.
    let seconds = if pos < len && (bytes[pos] == b':' || bytes[pos] == b' ') {
        let sep_char = bytes[pos];
        // For space separator, verify next char is a digit.
        if sep_char == b' ' {
            if pos + 1 < len && (bytes[pos + 1].is_ascii_digit() || bytes[pos + 1] == b'.') {
                pos += 1;
                let (s, end, _) = parse_number(text, pos)?;
                pos = end;
                s
            } else {
                0.0
            }
        } else {
            // Colon separator.
            pos += 1;
            let (s, end, _) = parse_number(text, pos)?;
            pos = end;
            s
        }
    } else {
        0.0
    };

    let total = first.floor() + minutes / 60.0 + seconds / 3600.0;
    Ok((total, pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::BAD_VALUE;
    use std::f64::consts::PI;

    const ARCSEC: f64 = PI / 648000.0;

    /// Tolerance for floating-point comparisons (~1e-10 radians).
    const TOL: f64 = 1.0e-10;

    // ----------------------------------------------------------------
    // SkyAxis defaults
    // ----------------------------------------------------------------

    #[test]
    fn normalise_dec_passes_small_angles_through_unchanged() {
        // eraAnpm returns a small in-range angle bit-for-bit (fmod is
        // symmetric); rem_euclid would round-trip through ~2π and lose ULPs.
        for &a in &[-0.012636183784438945_f64, 0.5, -0.5, 1.0, -1.0] {
            assert_eq!(normalise_dec(a), a, "angle {a} should pass through");
        }
    }

    #[test]
    fn normalise_dec_wraps_into_plus_minus_pi() {
        // Matches eraAnpm: range is -pi <= a < pi.
        let two_pi = 2.0 * PI;
        assert!((normalise_dec(1.5 * PI) - (-0.5 * PI)).abs() < TOL);
        assert!((normalise_dec(-1.5 * PI) - (0.5 * PI)).abs() < TOL);
        assert!((normalise_dec(two_pi + 0.3) - 0.3).abs() < TOL);
    }

    #[test]
    fn longitude_defaults() {
        let axis = SkyAxis::longitude();
        assert!(!axis.is_latitude);
        assert!(axis.as_time.is_none());
        assert!(axis.centre_zero.is_none());
        // Effective defaults: as_time=true, centre_zero=false
        assert!(axis.effective_as_time());
        assert!(!axis.effective_centre_zero());
    }

    #[test]
    fn latitude_defaults() {
        let axis = SkyAxis::latitude();
        assert!(axis.is_latitude);
        assert!(axis.as_time.is_none());
        assert!(axis.centre_zero.is_none());
        // Effective defaults: as_time=false, centre_zero=true
        assert!(!axis.effective_as_time());
        assert!(axis.effective_centre_zero());
    }

    #[test]
    fn explicit_overrides() {
        let mut axis = SkyAxis::longitude();
        axis.as_time = Some(false);
        axis.centre_zero = Some(true);
        assert!(!axis.effective_as_time());
        assert!(axis.effective_centre_zero());
    }

    // ----------------------------------------------------------------
    // HMS formatting
    // ----------------------------------------------------------------

    #[test]
    fn format_hms_zero() {
        assert_eq!(format_sky(0.0, true, 1), "0:00:00.0");
    }

    #[test]
    fn format_hms_6h() {
        // PI/2 radians = 6 hours
        assert_eq!(format_sky(PI / 2.0, true, 1), "6:00:00.0");
    }

    #[test]
    fn format_hms_12h() {
        // PI radians = 12 hours
        assert_eq!(format_sky(PI, true, 1), "12:00:00.0");
    }

    #[test]
    fn format_hms_18h() {
        // 3*PI/2 radians = 18 hours
        assert_eq!(format_sky(3.0 * PI / 2.0, true, 1), "18:00:00.0");
    }

    #[test]
    fn format_hms_precision_0() {
        assert_eq!(format_sky(0.0, true, 0), "0:00:00");
    }

    #[test]
    fn format_hms_precision_3() {
        assert_eq!(format_sky(PI / 2.0, true, 3), "6:00:00.000");
    }

    #[test]
    fn format_hms_fractional() {
        // 12h 30m 0s = (12.5/24) * 2*PI = 12.5 * PI / 12
        let val = 12.5 * PI / 12.0;
        assert_eq!(format_sky(val, true, 1), "12:30:00.0");
    }

    // ----------------------------------------------------------------
    // DMS formatting
    // ----------------------------------------------------------------

    #[test]
    fn format_dms_45deg() {
        // PI/4 = 45 degrees
        assert_eq!(format_sky(PI / 4.0, false, 0), "45:00:00");
    }

    #[test]
    fn format_dms_neg45deg() {
        assert_eq!(format_sky(-PI / 4.0, false, 0), "-45:00:00");
    }

    #[test]
    fn format_dms_90deg() {
        assert_eq!(format_sky(PI / 2.0, false, 0), "90:00:00");
    }

    #[test]
    fn format_dms_zero() {
        assert_eq!(format_sky(0.0, false, 0), "0:00:00");
    }

    #[test]
    fn format_dms_1arcsec() {
        // 1 arcsecond = PI/648000 radians
        assert_eq!(format_sky(ARCSEC, false, 0), "0:00:01");
    }

    #[test]
    fn format_dms_neg1arcsec() {
        assert_eq!(format_sky(-ARCSEC, false, 0), "-0:00:01");
    }

    #[test]
    fn format_dms_precision_3() {
        assert_eq!(format_sky(PI / 4.0, false, 3), "45:00:00.000");
    }

    #[test]
    fn format_dms_180deg() {
        assert_eq!(format_sky(PI, false, 0), "180:00:00");
    }

    #[test]
    fn format_dms_fractional() {
        // 45.5 degrees = PI * 45.5 / 180
        let val = 45.5 * PI / 180.0;
        assert_eq!(format_sky(val, false, 0), "45:30:00");
    }

    // ----------------------------------------------------------------
    // BAD_VALUE handling
    // ----------------------------------------------------------------

    #[test]
    fn format_bad_value() {
        assert_eq!(format_sky(BAD_VALUE, true, 1), "<bad>");
        assert_eq!(format_sky(BAD_VALUE, false, 0), "<bad>");
    }

    #[test]
    fn format_nan() {
        assert_eq!(format_sky(f64::NAN, true, 1), "<bad>");
        assert_eq!(format_sky(f64::NAN, false, 0), "<bad>");
    }

    // ----------------------------------------------------------------
    // Unformat HMS
    // ----------------------------------------------------------------

    #[test]
    fn unformat_hms_12h30m() {
        // "12:30:00" as time -> 12.5 hours -> 12.5 * PI / 12
        let expected = 12.5 * PI / 12.0;
        let (val, nc) = unformat_sky("12:30:00", true).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 8);
    }

    #[test]
    fn unformat_hms_6h() {
        let expected = PI / 2.0;
        let (val, _nc) = unformat_sky("6:00:00.0", true).unwrap();
        assert!((val - expected).abs() < TOL);
    }

    #[test]
    fn unformat_hms_0h() {
        let (val, _nc) = unformat_sky("0:00:00", true).unwrap();
        assert!(val.abs() < TOL);
    }

    // ----------------------------------------------------------------
    // Unformat DMS
    // ----------------------------------------------------------------

    #[test]
    fn unformat_dms_45_30_15_5() {
        // +45:30:15.5 -> 45 + 30/60 + 15.5/3600 = 45.50430555... degrees
        let expected_deg = 45.0 + 30.0 / 60.0 + 15.5 / 3600.0;
        let expected = expected_deg * PI / 180.0;
        let (val, nc) = unformat_sky("+45:30:15.5", false).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 11);
    }

    #[test]
    fn unformat_dms_neg1arcsec() {
        // "-0:00:01" -> -1 arcsecond
        let expected = -ARCSEC;
        let (val, nc) = unformat_sky("-0:00:01", false).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 8);
    }

    #[test]
    fn unformat_dms_90deg() {
        let expected = PI / 2.0;
        let (val, _nc) = unformat_sky("+90:00:00", false).unwrap();
        assert!((val - expected).abs() < TOL);
    }

    #[test]
    fn unformat_decimal_degrees() {
        // "180.0" as degrees -> PI
        let (val, nc) = unformat_sky("180.0", false).unwrap();
        assert!((val - PI).abs() < TOL);
        assert_eq!(nc, 5);
    }

    #[test]
    fn unformat_decimal_hours() {
        // "6.0" as time -> 6 hours -> PI/2
        let expected = PI / 2.0;
        let (val, _nc) = unformat_sky("6.0", true).unwrap();
        assert!((val - expected).abs() < TOL);
    }

    #[test]
    fn unformat_with_leading_whitespace() {
        let expected = PI / 2.0;
        let (val, nc) = unformat_sky("  6:00:00", true).unwrap();
        assert!((val - expected).abs() < TOL);
        assert_eq!(nc, 9); // 2 spaces + 7 chars
    }

    #[test]
    fn unformat_space_separated() {
        // "12 30 00" as time -> 12.5 hours
        let expected = 12.5 * PI / 12.0;
        let (val, nc) = unformat_sky("12 30 00", true).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 8);
    }

    #[test]
    fn unformat_empty_string() {
        assert!(unformat_sky("", true).is_err());
        assert!(unformat_sky("   ", false).is_err());
    }

    #[test]
    fn unformat_no_seconds() {
        // "12:30" as time -> 12h 30m 0s = 12.5 hours
        let expected = 12.5 * PI / 12.0;
        let (val, nc) = unformat_sky("12:30", true).unwrap();
        assert!((val - expected).abs() < TOL);
        assert_eq!(nc, 5);
    }

    // ----------------------------------------------------------------
    // Round-trip tests
    // ----------------------------------------------------------------

    #[test]
    fn roundtrip_hms() {
        let values = [0.0, PI / 6.0, PI / 4.0, PI / 2.0, PI, 3.0 * PI / 2.0];
        for &orig in &values {
            let formatted = format_sky(orig, true, 3);
            let (parsed, _nc) = unformat_sky(&formatted, true).unwrap();
            // Normalize both to [0, 2*PI) for comparison.
            let orig_norm = orig.rem_euclid(2.0 * PI);
            let parsed_norm = parsed.rem_euclid(2.0 * PI);
            assert!(
                (orig_norm - parsed_norm).abs() < 1.0e-6,
                "HMS round-trip failed for {orig}: formatted='{}', parsed={parsed}",
                formatted
            );
        }
    }

    #[test]
    fn roundtrip_dms() {
        let values = [
            0.0,
            PI / 6.0,
            PI / 4.0,
            PI / 2.0,
            -PI / 6.0,
            -PI / 4.0,
            -PI / 2.0,
        ];
        for &orig in &values {
            let formatted = format_sky(orig, false, 3);
            let (parsed, _nc) = unformat_sky(&formatted, false).unwrap();
            assert!(
                (orig - parsed).abs() < 1.0e-6,
                "DMS round-trip failed for {orig}: formatted='{}', parsed={parsed}",
                formatted
            );
        }
    }

    // ----------------------------------------------------------------
    // Rounding overflow
    // ----------------------------------------------------------------

    #[test]
    fn format_hms_rounding_overflow() {
        // A value very close to 6h 0m 0s from below:
        // 5h 59m 59.999...s should round to 6:00:00.0 at precision 1.
        let hours = 5.0 + 59.0 / 60.0 + 59.999 / 3600.0;
        let rad = hours * PI / 12.0;
        let result = format_sky(rad, true, 1);
        assert_eq!(result, "6:00:00.0");
    }

    #[test]
    fn format_dms_rounding_overflow() {
        // 44:59:59.999... degrees should round to 45:00:00 at precision 0.
        let deg = 44.0 + 59.0 / 60.0 + 59.8 / 3600.0;
        let rad = deg * PI / 180.0;
        let result = format_sky(rad, false, 0);
        assert_eq!(result, "45:00:00");
    }

    #[test]
    fn format_hms_24h_no_wrap() {
        // 23h 59m 59.96s at precision 1 rounds the seconds up and carries to
        // 24:00:00.0 — C's eraA2tf does NOT range-reduce, so it is not wrapped
        // back to 0 (ERFA a2tf note 3).
        let hours = 23.0 + 59.0 / 60.0 + 59.96 / 3600.0;
        let rad = hours * PI / 12.0;
        let result = format_sky(rad, true, 1);
        assert_eq!(result, "24:00:00.0");
    }

    #[test]
    fn format_hms_over_24h_verbatim() {
        // A value exceeding 24h formats verbatim (e.g. an offset SkyRef).
        let hours = 458.0 + 21.0 / 60.0 + 58.4 / 3600.0;
        let rad = hours * PI / 12.0;
        assert_eq!(format_sky(rad, true, 1), "458:21:58.4");
    }

    #[test]
    fn format_hms_negative_signed() {
        // Negative value carries a leading "-" (eraA2tf sign), not a wrap.
        assert_eq!(format_sky(-PI / 2.0, true, 1), "-6:00:00.0");
    }
}
