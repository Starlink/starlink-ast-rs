//! Per-axis display properties for celestial coordinate axes.
//!
//! `SkyAxis` is an internal helper used by `SkyFrame` to control how
//! coordinate values are displayed (sexagesimal HMS or DMS) and parsed.

// This module is a building block for SkyFrame (Task 4). All items will
// have callers once SkyFrame lands; suppress dead_code until then.
#![allow(dead_code)]

use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable};

/// Per-axis display properties for a sky coordinate axis. Extends the
/// Axis class as C's SkyAxis does (skyaxis.h): the Axis attributes live in
/// `axis`, except the Format, which a SkyAxis keeps itself (C's
/// `skyformat`, skyaxis.c `SetAxisFormat`), so `axis.format` is never set.
#[derive(Debug, Clone)]
pub(crate) struct SkyAxis {
    /// The Axis-class attributes.
    pub axis: super::axis::Axis,
    /// Format as time (hours) rather than angle (degrees).
    pub as_time: Option<bool>,
    /// The SkyAxis Format string (C's `skyformat`), when set.
    pub format: Option<String>,
    /// The IsLatitude attribute, when set. A SkyFrame never sets it on its
    /// own axes; SubFrame sets it on an axis it picks out (skyframe.c:10282).
    pub is_latitude: Option<bool>,
    /// The CentreZero attribute, when set: normalise to [-pi,pi] (true) or
    /// [0,2pi] (false).
    pub centre_zero: Option<bool>,
}

impl SkyAxis {
    /// A SkyAxis with no attribute set, as C's `astSkyAxis` makes one.
    pub fn new() -> Self {
        Self {
            axis: super::axis::Axis::default(),
            as_time: None,
            format: None,
            is_latitude: None,
            centre_zero: None,
        }
    }

    /// AsTime, set or default: true only for an axis explicitly marked as a
    /// longitude (skyaxis.c:4619).
    pub fn as_time_value(&self) -> bool {
        self.as_time.unwrap_or(self.is_latitude == Some(false))
    }

    /// IsLatitude, set or default (false; skyaxis.c:4631).
    pub fn is_latitude_value(&self) -> bool {
        self.is_latitude.unwrap_or(false)
    }

    /// A value normalised as skyaxis.c `AxisNorm` does: into -pi..pi when
    /// the range is centred on zero, else 0..2pi; a bad value is unchanged.
    pub(crate) fn axis_norm(&self, value: f64) -> f64 {
        if value == crate::constants::BAD_VALUE {
            value
        } else if self.centre_zero_value() {
            crate::pal::drange(value)
        } else {
            crate::pal::dranrm(value)
        }
    }

    /// CentreZero, set or default (IsLatitude; skyaxis.c:4641).
    pub fn centre_zero_value(&self) -> bool {
        self.centre_zero.unwrap_or(self.is_latitude_value())
    }

    /// The Digits used for this axis: its own, else the one the Frame lends
    /// it (frame.c `MAKE_GET`).
    pub fn digits_value(&self, frame_digits: i32) -> i32 {
        self.axis.digits.unwrap_or(frame_digits)
    }

    /// The Format, set or default. Mirrors `GetAxisFormat` (skyaxis.c:2888).
    pub fn format_value(&self, frame_digits: i32) -> String {
        match &self.format {
            Some(f) => f.clone(),
            None => default_dhms_format(self.digits_value(frame_digits), self.as_time_value()),
        }
    }

    /// A value formatted as `SkyAxis::AxisFormat` does (skyaxis.c:809).
    pub fn format_axis_value(&self, frame_digits: i32, value: f64) -> AstResult<String> {
        sky_format(
            &self.format_value(frame_digits),
            self.digits_value(frame_digits),
            value,
        )
    }

    /// The Unit, set or default: `"rad"` for a `%`-style Format, else the
    /// one the Format describes. Mirrors `GetAxisUnit` (skyaxis.c:3288).
    pub fn unit_value(&self, frame_digits: i32) -> String {
        if let Some(u) = &self.axis.unit {
            return u.clone();
        }
        let fmt = self.format_value(frame_digits);
        if fmt.starts_with('%') {
            "rad".to_string()
        } else {
            dhms_unit(&fmt, self.digits_value(frame_digits)).0
        }
    }

    /// The NormUnit. Mirrors `GetAxisNormUnit` (skyaxis.c:3101): a
    /// description of the axis values is returned unchanged when the Unit is
    /// the default or has been set to that description; anything else is a
    /// units expression, normalised as the Axis class does.
    pub fn norm_unit_value(&self, frame_digits: i32) -> String {
        let fmt = self.format_value(frame_digits);
        if !fmt.starts_with('%') {
            let (description, describes_values) = dhms_unit(&fmt, self.digits_value(frame_digits));
            if describes_values
                && self
                    .axis
                    .unit
                    .as_ref()
                    .is_none_or(|unit| *unit == description)
            {
                return description;
            }
        }
        crate::units::norm_unit(&self.unit_value(frame_digits))
    }

    /// The Label, set or default. Mirrors `GetAxisLabel` (skyaxis.c:3012).
    pub fn label_value(&self) -> String {
        if let Some(l) = &self.axis.label {
            return l.clone();
        }
        let as_time = self.as_time_value();
        match (self.is_latitude, as_time) {
            (None, true) => "Angle on sky expressed as time",
            (None, false) => "Angle on sky",
            (Some(true), true) => "Sky latitude expressed as time",
            (Some(true), false) => "Sky latitude",
            (Some(false), true) => "Sky longitude expressed as time",
            (Some(false), false) => "Sky longitude",
        }
        .to_string()
    }

    /// The Symbol, set or default. Mirrors `GetAxisSymbol` (skyaxis.c:3205).
    pub fn symbol_value(&self) -> String {
        if let Some(s) = &self.axis.symbol {
            return s.clone();
        }
        let latitude = match self.is_latitude {
            Some(lat) => lat,
            None => !self.as_time_value(),
        };
        if latitude { "delta" } else { "alpha" }.to_string()
    }

    /// The Direction, set or default: an axis formatted as a time plots in
    /// reverse. Mirrors `GetAxisDirection` (skyaxis.c:2820).
    pub fn direction_value(&self) -> bool {
        self.axis.direction.unwrap_or(!self.as_time_value())
    }

    /// The Top, set or default: pi/2 for a latitude axis, else no limit.
    /// Mirrors `GetAxisTop` (skyaxis.c:2755).
    pub fn top_value(&self) -> f64 {
        self.axis.top.unwrap_or(if self.is_latitude_value() {
            std::f64::consts::FRAC_PI_2
        } else {
            f64::MAX
        })
    }

    /// The Bottom, set or default, as [`top_value`](Self::top_value) is.
    /// Mirrors `GetAxisBottom` (skyaxis.c:2647).
    pub fn bottom_value(&self) -> f64 {
        self.axis.bottom.unwrap_or(if self.is_latitude_value() {
            -std::f64::consts::FRAC_PI_2
        } else {
            -f64::MAX
        })
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
            is_latitude: self.is_latitude_value(),
            centre_zero: self.centre_zero_value(),
        }
    }
}

impl Default for SkyAxis {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Sexagesimal Format strings
// ---------------------------------------------------------------------------

/// Radians per hour, as C computes it with `palDtf2r( 1, 0, 0.0, ... )`
/// (skyaxis.c:3521), written out as `eraTf2a`'s arithmetic on ERFA's
/// `ERFA_DS2R`; the two are bit-identical.
// The literal is ERFA's, copied verbatim from erfam.h.
#[allow(clippy::excessive_precision)]
const HR2RAD: f64 = (60.0 * (60.0 * 1.0 + 0.0) + 0.0) * 7.272205216643039903848712e-5;

/// Radians per degree, as C computes it with `palDaf2r( 1, 0, 0.0, ... )`
/// (skyaxis.c:3522), written out as `eraAf2a`'s arithmetic on ERFA's
/// `ERFA_DAS2R`; the two are bit-identical.
// The literal is ERFA's, copied verbatim from erfam.h.
#[allow(clippy::excessive_precision)]
const DEG2RAD: f64 = (60.0 * (60.0 * 1.0 + 0.0) + 0.0) * 4.848136811095359935899141e-6;

/// Length of C's `DHmsFormat` result buffer
/// (`AST__SKYAXIS_DHMSFORMAT_BUFF_LEN`, skyaxis.h:216), which caps the
/// number of decimal places so that the result cannot overflow it.
const DHMSFORMAT_BUFF_LEN: usize = 70;

/// Field separator of a SkyAxis Format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DHmsSep {
    /// A character between the fields: `':'` (`i`) or `' '` (`b`).
    Char(char),
    /// Letters after each field, as in `12h34m56s` (`l`, and `g`; see
    /// [`parse_dhms_format`]).
    Letters,
}

/// A SkyAxis Format string, parsed. Mirrors the outputs of C
/// `ParseDHmsFormat` (skyaxis.c:3536).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DHmsFields {
    pub sep: DHmsSep,
    /// Prefix a positive value with `+`.
    pub plus: bool,
    /// Pad the leading field with zeros.
    pub lead_zero: bool,
    /// Format as a time (hours) rather than an angle (degrees).
    pub as_time: bool,
    /// A degrees/hours field is present.
    pub dh: bool,
    /// A minutes field is present.
    pub min: bool,
    /// A seconds field is present.
    pub sec: bool,
    /// Decimal places after the final field.
    pub ndp: usize,
}

/// Parse a SkyAxis Format string, with `digs` the axis Digits value used by
/// a `.*` precision. Mirrors C `ParseDHmsFormat` (skyaxis.c:3536).
///
/// C turns the `g` separator (graphical escape sequences) into `l` unless
/// `astEscapes` is on. The port has no escape sequences, which is C's
/// default, so `g` is always `l` here.
pub(crate) fn parse_dhms_format(fmt: &str, digs: i32) -> DHmsFields {
    let mut as_time: Option<bool> = None;
    let mut f = DHmsFields {
        sep: DHmsSep::Char(':'),
        plus: false,
        lead_zero: false,
        as_time: false,
        dh: false,
        min: false,
        sec: false,
        ndp: 0,
    };
    let bytes = fmt.as_bytes();
    let mut decpos: Option<usize> = None;
    for (i, &c) in bytes.iter().enumerate() {
        match c {
            b'+' => f.plus = true,
            b'Z' | b'z' => f.lead_zero = true,
            b'I' | b'i' => f.sep = DHmsSep::Char(':'),
            b'B' | b'b' => f.sep = DHmsSep::Char(' '),
            b'L' | b'l' | b'G' | b'g' => f.sep = DHmsSep::Letters,
            b'T' | b't' => {
                if as_time.is_none() {
                    as_time = Some(true);
                }
            }
            b'H' | b'h' => {
                f.dh = true;
                as_time = Some(true);
            }
            b'D' | b'd' => {
                f.dh = true;
                as_time = Some(false);
            }
            b'M' | b'm' => f.min = true,
            b'S' | b's' => f.sec = true,
            b'.' => decpos = Some(i),
            _ => {}
        }
    }
    f.as_time = as_time.unwrap_or(false);

    // Degrees (or hours) is the default field, and a seconds field needs a
    // minutes field when a degrees/hours field is present.
    if !f.min && !f.sec {
        f.dh = true;
    }
    if f.dh && !f.min {
        f.sec = false;
    }

    // Decimal places: an integer after the last '.', or '*' for as many as
    // "digs" digits of precision leave after the fields present.
    if let Some(decpos) = decpos.filter(|&d| d + 1 < bytes.len()) {
        let rest = &fmt[decpos + 1..];
        if let Some(ndpval) = scan_c_int(rest) {
            if ndpval >= 0 {
                f.ndp = ndpval as usize;
            }
        } else if rest.starts_with('*') {
            let mut ndp = if f.as_time {
                let n = digs.max(2);
                if f.dh { n - 2 } else { n }
            } else {
                let n = digs.max(3);
                if f.dh { n - 3 } else { n }
            };
            if f.min {
                ndp -= 2;
            }
            if f.sec {
                ndp -= 2;
            }
            f.ndp = ndp.max(0) as usize;
        }
    }
    f
}

/// The integer C's `sscanf( text, "%d", &n )` reads from the start of
/// `text`: optional white space, an optional sign and at least one digit.
fn scan_c_int(text: &str) -> Option<i64> {
    let t = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let n = digits.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    // C's "%d" is undefined beyond int range; saturate rather than wrap.
    let v = digits[..n].parse::<i64>().unwrap_or(i64::MAX);
    Some(if neg { -v } else { v })
}

/// The default Format of a SkyAxis whose Format is unset, from its Digits
/// and AsTime values. Mirrors C `GetAxisFormat` (skyaxis.c:2888).
pub(crate) fn default_dhms_format(digits: i32, as_time: bool) -> String {
    if as_time {
        match digits {
            d if d <= 2 => "h".to_string(),
            3 | 4 => "hm".to_string(),
            5 | 6 => "hms".to_string(),
            d => format!("hms.{}", d - 6),
        }
    } else {
        match digits {
            d if d <= 3 => "d".to_string(),
            4 | 5 => "dm".to_string(),
            6 | 7 => "dms".to_string(),
            d => format!("dms.{}", d - 7),
        }
    }
}

/// The Unit a SkyAxis Format implies, with whether it describes the axis
/// values (`"hh:mm:ss.s"`, `"minutes of time"`) rather than naming a unit
/// (`"degrees"`). Mirrors C `DHmsUnit` (skyaxis.c:2279) with `output` set,
/// the form every caller in the port needs; `descr` is its `descr` flag.
pub(crate) fn dhms_unit(fmt: &str, digs: i32) -> (String, bool) {
    // Decimal places shown before an ellipsis replaces the rest.
    const MAXDP: usize = 6;

    let f = parse_dhms_format(fmt, digs);
    match f.sep {
        DHmsSep::Letters => return (String::new(), false),
        DHmsSep::Char(_) if f.dh && !f.min && !f.sec => {
            let u = if f.as_time { "hours" } else { "degrees" };
            return (u.to_string(), false);
        }
        DHmsSep::Char(_) if !f.dh && f.min && !f.sec => {
            let u = if f.as_time {
                "minutes of time"
            } else {
                "arcminutes"
            };
            return (u.to_string(), f.as_time);
        }
        DHmsSep::Char(_) if !f.dh && !f.min && f.sec => {
            let u = if f.as_time {
                "seconds of time"
            } else {
                "arcseconds"
            };
            return (u.to_string(), f.as_time);
        }
        DHmsSep::Char(_) => {}
    }
    let DHmsSep::Char(sep) = f.sep else {
        unreachable!("letter separators returned above")
    };

    let mut out = String::new();
    let mut dpchar = 'd';
    if f.dh {
        out.push_str(if f.as_time { "hh" } else { "ddd" });
        dpchar = if f.as_time { 'h' } else { 'd' };
    }
    if f.min {
        if f.dh {
            out.push(sep);
        }
        out.push_str("mm");
        dpchar = 'm';
    }
    if f.sec {
        if f.dh || f.min {
            out.push(sep);
        }
        out.push_str("ss");
        dpchar = 's';
    }
    if f.ndp > 0 {
        out.push('.');
        for dp in 0..f.ndp {
            if dp < MAXDP {
                out.push(dpchar);
            } else {
                // After MAXDP places, the last three become an ellipsis.
                out.truncate(out.len() - 3);
                out.push_str("...");
                break;
            }
        }
    }
    (out, true)
}

/// Format `value` (radians) with a SkyAxis Format string, `digs` being the
/// axis Digits value. Mirrors C `DHmsFormat` (skyaxis.c:1561).
pub(crate) fn dhms_format(fmt: &str, digs: i32, value: f64) -> String {
    if value == crate::constants::BAD_VALUE {
        return "<bad>".to_string();
    }
    let f = parse_dhms_format(fmt, digs);
    let ndp = if f.ndp + 11 > DHMSFORMAT_BUFF_LEN {
        DHMSFORMAT_BUFF_LEN - 11
    } else {
        f.ndp
    };

    // C turns a "minus zero" into zero before testing the sign.
    let value = if value == 0.0 { 0.0 } else { value };
    let mut positive = value >= 0.0;
    let absvalue = if positive { value } else { -value };
    let unit = if f.as_time { HR2RAD } else { DEG2RAD };

    // Split into fields, each the whole part of what remains (C's modf).
    let mut fract = absvalue / unit;
    let mut idh = 0.0;
    if f.dh {
        idh = fract.trunc();
        fract -= idh;
    }
    let mut imin = 0.0;
    if f.min {
        let v = fract * 60.0;
        imin = v.trunc();
        fract = v - imin;
    }
    let mut isec = 0.0;
    if f.sec {
        if !f.min {
            fract *= 60.0;
        }
        let v = fract * 60.0;
        isec = v.trunc();
        fract = v - isec;
    }

    // Truncate the final field to "ndp" places, then round up if the value
    // is at least the one whose textual form ends in 5 at the next place,
    // compared in radians so that a formatted value reads back unchanged.
    let shift = 10f64.powi(ndp as i32);
    let mut ifract = (fract * shift).floor();
    let mut test = (0.5 + ifract) / shift;
    if f.sec {
        test = (isec + test) / 60.0;
    }
    if f.min {
        test = (imin + test) / 60.0;
    } else if f.sec {
        test /= 60.0;
    }
    if f.dh {
        test += idh;
    }
    test *= unit;
    if absvalue >= test {
        ifract += 1.0;
    }

    // Carry a rounded-up fraction into the fields that are present.
    if ifract / shift >= 1.0 {
        ifract = 0.0;
        if f.sec {
            isec += 1.0;
            if (isec + 0.5).floor() > 59.5 && f.min {
                isec = 0.0;
                imin += 1.0;
                if (imin + 0.5).floor() > 59.5 && f.dh {
                    imin = 0.0;
                    idh += 1.0;
                }
            }
        } else if f.min {
            imin += 1.0;
            if (imin + 0.5).floor() > 59.5 && f.dh {
                imin = 0.0;
                idh += 1.0;
            }
        } else {
            idh += 1.0;
        }
    }

    // A value that formats as zero in every field has no sign.
    let zero = |v: f64| (v + 0.5).floor() < 0.5;
    if !positive
        && (!f.dh || zero(idh))
        && (!f.min || zero(imin))
        && (!f.sec || zero(isec))
        && zero(ifract)
    {
        positive = true;
    }

    let letters = f.sep == DHmsSep::Letters;
    let mut out = String::new();
    if !positive {
        out.push('-');
    } else if f.plus {
        out.push('+');
    }
    if f.dh {
        let width = if f.lead_zero {
            if f.as_time { 2 } else { 3 }
        } else {
            1
        };
        out.push_str(&format!("{idh:0width$.0}"));
        if (f.min || f.sec) && letters {
            out.push(if f.as_time { 'h' } else { 'd' });
        }
    }
    if f.min {
        if let DHmsSep::Char(sep) = f.sep {
            if f.dh {
                out.push(sep);
            }
        }
        let width = if f.dh || f.lead_zero { 2 } else { 1 };
        out.push_str(&format!("{imin:0width$.0}"));
        if f.sec && letters {
            out.push('m');
        }
    }
    if f.sec {
        if let DHmsSep::Char(sep) = f.sep {
            if f.dh || f.min {
                out.push(sep);
            }
        }
        let width = if f.dh || f.min || f.lead_zero { 2 } else { 1 };
        out.push_str(&format!("{isec:0width$.0}"));
    }
    if ndp > 0 {
        out.push('.');
        out.push_str(&format!("{ifract:0ndp$.0}"));
    }
    if letters {
        out.push(if f.sec {
            's'
        } else if f.min {
            'm'
        } else if f.as_time {
            'h'
        } else {
            'd'
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl SkyAxis {
    /// Write this SkyAxis as C's Dump does (skyaxis.c:4750, after the Axis
    /// class Dump, axis.c): the Axis attributes that are set, the `IsA Axis`
    /// marker when there were any, then the SkyAxis attributes that are set.
    pub(crate) fn to_node_dump(&self) -> Node {
        let mut node = Node::new("SkyAxis");
        node.class_comment = Some("Celestial coordinate axis".to_string());
        self.axis.write_set_attributes(&mut node);
        if !node.items.is_empty() {
            node.push_isa("Axis", Some("Coordinate axis".to_string()));
        }

        if let Some(format) = &self.format {
            node.push_string("Format", format, Some("Format specifier".to_string()));
        }
        if let Some(lat) = self.is_latitude {
            node.push_int(
                "IsLat",
                i64::from(lat),
                Some(if lat {
                    "Latitude axis (not longitude)".to_string()
                } else {
                    "Longitude axis (not latitude)".to_string()
                }),
            );
        }
        if let Some(cz) = self.centre_zero {
            node.push_int(
                "CnZer",
                i64::from(cz),
                Some(if cz {
                    "Display axis values in range -PI -> +PI".to_string()
                } else {
                    "Display axis values in range 0 -> 2.PI".to_string()
                }),
            );
        }
        if let Some(at) = self.as_time {
            node.push_int(
                "AsTime",
                i64::from(at),
                Some(if at {
                    "Display values as times (not angles)".to_string()
                } else {
                    "Display values as angles (not times)".to_string()
                }),
            );
        }
        node
    }
}

impl Serializable for SkyAxis {
    fn class_name(&self) -> &'static str {
        "SkyAxis"
    }

    fn to_node(&self) -> AstResult<Node> {
        Ok(self.to_node_dump())
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("SkyAxis");
        let mut axis = SkyAxis::new();
        if node.get("IsLat").is_some() {
            axis.is_latitude = Some(node.get_int("IsLat", 0) != 0);
        }

        // Each attribute is set only if present in the node. A SkyAxis
        // stores its Format itself, so its Format is the SkyAxis part's
        // (C's Axis-level Format is never set on a SkyAxis).
        if node.get("CnZer").is_some() {
            axis.centre_zero = Some(node.get_int("CnZer", 0) != 0);
        }
        if node.get("AsTime").is_some() {
            axis.as_time = Some(node.get_int("AsTime", 0) != 0);
        }
        axis.format = node.get_string_if_set("Format");
        axis.axis = super::axis::Axis::read(whole);
        axis.axis.format = None;

        Ok(axis)
    }
}

/// A value formatted with the Format `fmt` of a SkyAxis whose Digits is
/// `digits`, as `SkyAxis::AxisFormat` does (skyaxis.c:809): with
/// `DHmsFormat`, except that a `%`-style Format goes to the Axis class's
/// formatter, which reads the Axis's own Format rather than the SkyAxis one
/// and so always uses its default, `%1.<Digits>G` (axis.c:731, :2673).
pub(crate) fn sky_format(fmt: &str, digits: i32, value: f64) -> AstResult<String> {
    if crate::mappings::helpers::is_bad(value) {
        return Ok("<bad>".to_string());
    }
    if fmt.starts_with('%') {
        crate::frames::axis_format::format_value(&format!("%1.{digits}G"), digits, value)
    } else {
        Ok(dhms_format(fmt, digits, value))
    }
}

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

/// The separators a sky coordinate may use (skyaxis.c `sep_list`), whether
/// each says the value is an angle (1) or a time (2), and which field it
/// ends when it is a suffix (1 to 3), zero meaning it does not say.
const SEP_LIST: &[u8] = b" :hHdDmM'sS\"";
const ANGLE_OR_TIME_LIST: [u8; 12] = [0, 0, 2, 2, 1, 1, 0, 0, 1, 0, 0, 1];
const FIELD_ID_LIST: [usize; 12] = [0, 0, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3];

/// C's `isspace` in the "C" locale.
fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The length of the run of ASCII digits at the start of `s`.
fn digit_run(s: &[u8]) -> usize {
    s.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// Read a sky coordinate in radians from the start of `text`, as
/// `SkyAxis::AxisUnformat` does (skyaxis.c:4068) for an axis whose Format is
/// `fmt` and whose Digits is `digits`. Returns the value and the number of
/// characters read, which is zero, with the value unchanged at zero, when
/// nothing could be read.
///
/// The Format says which fields are expected when the string does not
/// identify them, and whether the value is a time or an angle; separators
/// such as `h`, `m` and `d` in the string can override both. A `%`-style
/// Format goes to the Axis class's reader instead.
pub(crate) fn unformat_sky_axis(text: &str, fmt: &str, digits: i32) -> AstResult<(f64, usize)> {
    if fmt.starts_with('%') {
        return Ok(super::basic::axis_unformat(text));
    }
    let parsed = parse_dhms_format(fmt, digits);
    let mut as_time = parsed.as_time;
    let (dh, min) = (parsed.dh, parsed.min);
    let fieldvalue: [f64; 3] = [1.0, 1.0 / 60.0, 1.0 / 3600.0];

    let string = text.as_bytes();
    let at = |i: usize| string.get(i).copied().unwrap_or(0);

    let mut s = 0;
    while c_isspace(at(s)) {
        s += 1;
    }
    let string_start = s;

    // An optional sign, surrounded by optional white space (" %1[+-] %n").
    let mut positive = true;
    {
        let mut k = s;
        while c_isspace(at(k)) {
            k += 1;
        }
        if at(k) == b'+' || at(k) == b'-' {
            positive = at(k) == b'+';
            k += 1;
            while c_isspace(at(k)) {
                k += 1;
            }
            s = k;
        }
    }

    let mut angle_or_time = 0u8;
    let mut last_sep = 0u8;
    let mut next_id = 0usize;
    let mut nfield = 0usize;
    let mut sep_used = 0u8;
    let mut suffix_sep = false;
    let mut sep_len = 0usize;
    let mut field = [0.0f64; 3];
    let mut field_start = [0usize; 3];
    let mut field_id = [0usize; 3];
    let mut nc = 0usize;

    for ifield in 0..3 {
        field[ifield] = 0.0;
        if ifield > 0 && !suffix_sep {
            s += sep_len;
        }
        field_start[ifield] = s;

        // A sequence of digits, with an optional decimal point.
        let rest = &string[s.min(string.len())..];
        let mut decimal = true;
        let mut exponent = false;
        let d1 = digit_run(rest);
        let mut n = 0;
        if d1 > 0 && rest.get(d1) == Some(&b'.') && digit_run(&rest[d1 + 1..]) > 0 {
            n = d1 + 1 + digit_run(&rest[d1 + 1..]);
        } else if d1 > 0 && rest.get(d1) == Some(&b'.') {
            n = d1 + 1;
        } else if rest.first() == Some(&b'.') && digit_run(&rest[1..]) > 0 {
            n = 1 + digit_run(&rest[1..]);
        } else if d1 > 0 {
            n = d1;
            decimal = false;
        }
        let matched = n > 0;
        if matched {
            // An exponent, with or without a sign.
            let e = &rest[n..];
            if matches!(e.first(), Some(b'E' | b'e')) {
                let signed = matches!(e.get(1), Some(b'+' | b'-'));
                let start = if signed { 2 } else { 1 };
                let digs = digit_run(&e[start.min(e.len())..]);
                if digs > 0 {
                    n += start + digs;
                    exponent = true;
                }
            }
        }

        // Read the number from exactly those characters, then any white
        // space ("%<n>lf %n").
        let mut nread = 0;
        if matched {
            let number = std::str::from_utf8(&rest[..n]).unwrap_or("");
            if let Ok(v) = number.parse::<f64>() {
                field[ifield] = v;
                let mut k = n;
                while c_isspace(rest.get(k).copied().unwrap_or(0)) {
                    k += 1;
                }
                nread = k;
                s += nread;
            }
        }
        nc = s;

        // The separator after the field; a space by default, which is
        // valid if white space followed the number.
        let mut sep = b' ';
        let mut good_sep = nread > n;
        sep_len = 0;
        let c = at(s);
        if c != 0 && SEP_LIST.contains(&c) {
            sep = c;
            good_sep = true;
            sep_len = 1;
            while c_isspace(at(s + sep_len)) {
                sep_len += 1;
            }
        }
        let sep_index = SEP_LIST.iter().position(|&b| b == sep).unwrap_or(0);
        let sep_field_id = FIELD_ID_LIST[sep_index];
        let sep_angle_or_time = ANGLE_OR_TIME_LIST[sep_index];

        if good_sep {
            if sep_field_id != 0 {
                good_sep = sep_field_id >= next_id && sep_field_id > ifield;
            } else {
                good_sep = sep_used == 0 || sep == sep_used;
                if sep_used == 0 {
                    sep_used = sep;
                }
            }
        }
        if good_sep {
            if angle_or_time == 0 {
                angle_or_time = sep_angle_or_time;
            } else {
                good_sep = sep_angle_or_time == 0 || sep_angle_or_time == angle_or_time;
            }
        }
        if good_sep {
            nread += sep_len;
        }
        suffix_sep = good_sep && sep_field_id != 0;
        if suffix_sep {
            field_id[ifield] = sep_field_id;
            next_id = sep_field_id + 1;
            s += sep_len;
            nc = s;
        } else {
            field_id[ifield] = next_id;
            if next_id != 0 {
                next_id += 1;
            }
        }

        let mut last = false;
        if nread == 0 && (ifield == 0 || c_isspace(last_sep)) {
            last = true;
        } else {
            nfield += 1;
        }
        last = last || decimal || exponent || !good_sep || next_id > 3;
        if last {
            break;
        }
        last_sep = sep;
    }

    // Number the fields that no separator identified, working back from
    // the last, from the fields the Format says are expected.
    let mut next_id = nfield
        + if dh {
            0
        } else if min {
            1
        } else {
            2
        };
    if next_id > 3 {
        next_id = 3;
    }
    for ifield in (0..nfield).rev() {
        if field_id[ifield] != 0 {
            next_id = field_id[ifield] - 1;
        } else {
            field_id[ifield] = next_id;
            next_id = next_id.saturating_sub(1);
        }
    }

    if nfield == 0 {
        // "<bad>", with optional white space anywhere.
        let mut k = 0;
        let skip = |k: &mut usize| {
            while c_isspace(at(*k)) {
                *k += 1;
            }
        };
        skip(&mut k);
        let mut ok = at(k) == b'<';
        if ok {
            k += 1;
            for &want in b"bad" {
                skip(&mut k);
                if at(k).to_ascii_lowercase() == want {
                    k += 1;
                } else {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            skip(&mut k);
            ok = at(k) == b'>';
            if ok {
                k += 1;
                skip(&mut k);
            }
        }
        return Ok(if ok {
            (crate::constants::BAD_VALUE, k)
        } else {
            (0.0, 0)
        });
    }

    if angle_or_time != 0 {
        as_time = angle_or_time == 2;
    }

    // Fields after the first must be below 60, unless only their decimal
    // places round them up to it.
    for ifield in 1..nfield {
        if field[ifield] >= 60.0 {
            let start = field_start[ifield];
            let n = digit_run(&string[start.min(string.len())..]);
            if n > 0 {
                let whole = std::str::from_utf8(&string[start..start + n])
                    .ok()
                    .and_then(|t| t.parse::<f64>().ok());
                if whole.is_some_and(|v| v >= 60.0) {
                    let nchar = nc - string_start;
                    let shown = &string[string_start..string_start + nchar];
                    let len = shown
                        .iter()
                        .rposition(|&b| !c_isspace(b))
                        .map_or(0, |i| i + 1);
                    return Err(AstError::new_tracked(
                        ErrorCode::Unfer,
                        format!(
                            "Invalid {}{} value in sky coordinate \"{}\".",
                            if as_time { "" } else { "arc" },
                            if field_id[ifield] == 2 {
                                "minutes"
                            } else {
                                "seconds"
                            },
                            String::from_utf8_lossy(&shown[..len])
                        ),
                    ));
                }
            }
        }
    }

    let unit = if as_time { HR2RAD } else { DEG2RAD };
    let mut value = 0.0;
    for ifield in 0..nfield {
        value += field[ifield] * fieldvalue[field_id[ifield] - 1] * unit;
    }
    if !positive {
        value = -value;
    }
    Ok((value, nc))
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

/// Whether the angle `val` lies between `lo` and `hi`, including the limits
/// when `closed`, after moving it by whole turns into the range. Mirrors
/// skyaxis.c `AxisIn`.
pub(crate) fn axis_in(lo: f64, hi: f64, val: f64, closed: bool) -> bool {
    let turn = 2.0 * std::f64::consts::PI;
    let mut val = val;
    if closed {
        while val > hi {
            val -= turn;
        }
        while val < lo {
            val += turn;
        }
        val <= hi
    } else {
        while val >= hi {
            val -= turn;
        }
        while val <= lo {
            val += turn;
        }
        val < hi
    }
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
    fn defaults_follow_is_latitude() {
        // skyaxis.c:4619-4642: with nothing set, an axis is neither a
        // latitude nor formatted as a time, and is not centred on zero.
        let axis = SkyAxis::new();
        assert!(!axis.as_time_value());
        assert!(!axis.centre_zero_value());
        let mut lon = SkyAxis::new();
        lon.is_latitude = Some(false);
        assert!(lon.as_time_value());
        assert!(!lon.centre_zero_value());
        let mut lat = SkyAxis::new();
        lat.is_latitude = Some(true);
        assert!(!lat.as_time_value());
        assert!(lat.centre_zero_value());
    }

    #[test]
    fn explicit_overrides() {
        let mut axis = SkyAxis::new();
        axis.as_time = Some(false);
        axis.centre_zero = Some(true);
        assert!(!axis.as_time_value());
        assert!(axis.centre_zero_value());
    }

    // ----------------------------------------------------------------
    // HMS formatting
    // ----------------------------------------------------------------

    #[test]
    fn format_hms_zero() {
        assert_eq!(dhms_format("hms.1", 7, 0.0), "0:00:00.0");
    }

    #[test]
    fn format_hms_6h() {
        // PI/2 radians = 6 hours
        assert_eq!(dhms_format("hms.1", 7, PI / 2.0), "6:00:00.0");
    }

    #[test]
    fn format_hms_12h() {
        // PI radians = 12 hours
        assert_eq!(dhms_format("hms.1", 7, PI), "12:00:00.0");
    }

    #[test]
    fn format_hms_18h() {
        // 3*PI/2 radians = 18 hours
        assert_eq!(dhms_format("hms.1", 7, 3.0 * PI / 2.0), "18:00:00.0");
    }

    #[test]
    fn format_hms_precision_0() {
        assert_eq!(dhms_format("hms", 7, 0.0), "0:00:00");
    }

    #[test]
    fn format_hms_precision_3() {
        assert_eq!(dhms_format("hms.3", 7, PI / 2.0), "6:00:00.000");
    }

    #[test]
    fn format_hms_fractional() {
        // 12h 30m 0s = (12.5/24) * 2*PI = 12.5 * PI / 12
        let val = 12.5 * PI / 12.0;
        assert_eq!(dhms_format("hms.1", 7, val), "12:30:00.0");
    }

    // ----------------------------------------------------------------
    // DMS formatting
    // ----------------------------------------------------------------

    #[test]
    fn format_dms_45deg() {
        // PI/4 = 45 degrees
        assert_eq!(dhms_format("dms", 7, PI / 4.0), "45:00:00");
    }

    #[test]
    fn format_dms_neg45deg() {
        assert_eq!(dhms_format("dms", 7, -PI / 4.0), "-45:00:00");
    }

    #[test]
    fn format_dms_90deg() {
        assert_eq!(dhms_format("dms", 7, PI / 2.0), "90:00:00");
    }

    #[test]
    fn format_dms_zero() {
        assert_eq!(dhms_format("dms", 7, 0.0), "0:00:00");
    }

    #[test]
    fn format_dms_1arcsec() {
        // 1 arcsecond = PI/648000 radians
        assert_eq!(dhms_format("dms", 7, ARCSEC), "0:00:01");
    }

    #[test]
    fn format_dms_neg1arcsec() {
        assert_eq!(dhms_format("dms", 7, -ARCSEC), "-0:00:01");
    }

    #[test]
    fn format_dms_precision_3() {
        assert_eq!(dhms_format("dms.3", 7, PI / 4.0), "45:00:00.000");
    }

    #[test]
    fn format_dms_180deg() {
        assert_eq!(dhms_format("dms", 7, PI), "180:00:00");
    }

    #[test]
    fn format_dms_fractional() {
        // 45.5 degrees = PI * 45.5 / 180
        let val = 45.5 * PI / 180.0;
        assert_eq!(dhms_format("dms", 7, val), "45:30:00");
    }

    // ----------------------------------------------------------------
    // BAD_VALUE handling
    // ----------------------------------------------------------------

    #[test]
    fn format_bad_value() {
        assert_eq!(dhms_format("hms.1", 7, BAD_VALUE), "<bad>");
        assert_eq!(dhms_format("dms", 7, BAD_VALUE), "<bad>");
    }

    // ----------------------------------------------------------------
    // Unformat HMS
    // ----------------------------------------------------------------

    #[test]
    fn unformat_hms_12h30m() {
        // "12:30:00" as time -> 12.5 hours -> 12.5 * PI / 12
        let expected = 12.5 * PI / 12.0;
        let (val, nc) = unformat_sky_axis("12:30:00", "hms", 7).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 8);
    }

    #[test]
    fn unformat_hms_6h() {
        let expected = PI / 2.0;
        let (val, _nc) = unformat_sky_axis("6:00:00.0", "hms", 7).unwrap();
        assert!((val - expected).abs() < TOL);
    }

    #[test]
    fn unformat_hms_0h() {
        let (val, _nc) = unformat_sky_axis("0:00:00", "hms", 7).unwrap();
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
        let (val, nc) = unformat_sky_axis("+45:30:15.5", "dms", 7).unwrap();
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
        let (val, nc) = unformat_sky_axis("-0:00:01", "dms", 7).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 8);
    }

    #[test]
    fn unformat_dms_90deg() {
        let expected = PI / 2.0;
        let (val, _nc) = unformat_sky_axis("+90:00:00", "dms", 7).unwrap();
        assert!((val - expected).abs() < TOL);
    }

    #[test]
    fn unformat_decimal_degrees() {
        // "180.0" as degrees -> PI
        let (val, nc) = unformat_sky_axis("180.0", "dms", 7).unwrap();
        assert!((val - PI).abs() < TOL);
        assert_eq!(nc, 5);
    }

    #[test]
    fn unformat_decimal_hours() {
        // "6.0" as time -> 6 hours -> PI/2
        let expected = PI / 2.0;
        let (val, _nc) = unformat_sky_axis("6.0", "hms", 7).unwrap();
        assert!((val - expected).abs() < TOL);
    }

    #[test]
    fn unformat_with_leading_whitespace() {
        let expected = PI / 2.0;
        let (val, nc) = unformat_sky_axis("  6:00:00", "hms", 7).unwrap();
        assert!((val - expected).abs() < TOL);
        assert_eq!(nc, 9); // 2 spaces + 7 chars
    }

    #[test]
    fn unformat_space_separated() {
        // "12 30 00" as time -> 12.5 hours
        let expected = 12.5 * PI / 12.0;
        let (val, nc) = unformat_sky_axis("12 30 00", "hms", 7).unwrap();
        assert!(
            (val - expected).abs() < TOL,
            "got {val}, expected {expected}"
        );
        assert_eq!(nc, 8);
    }

    #[test]
    fn unformat_empty_string() {
        // skyaxis.c:4530: nothing read is not an error.
        assert_eq!(unformat_sky_axis("", "hms", 7).unwrap(), (0.0, 0));
        assert_eq!(unformat_sky_axis("   ", "dms", 7).unwrap(), (0.0, 0));
    }

    #[test]
    fn unformat_no_seconds() {
        // "12:30" as time -> 12h 30m 0s = 12.5 hours
        let expected = 12.5 * PI / 12.0;
        let (val, nc) = unformat_sky_axis("12:30", "hms", 7).unwrap();
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
            let formatted = dhms_format("hms.3", 7, orig);
            let (parsed, _nc) = unformat_sky_axis(&formatted, "hms", 7).unwrap();
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
            let formatted = dhms_format("dms.3", 7, orig);
            let (parsed, _nc) = unformat_sky_axis(&formatted, "dms", 7).unwrap();
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
        let result = dhms_format("hms.1", 7, rad);
        assert_eq!(result, "6:00:00.0");
    }

    #[test]
    fn format_dms_rounding_overflow() {
        // 44:59:59.999... degrees should round to 45:00:00 at precision 0.
        let deg = 44.0 + 59.0 / 60.0 + 59.8 / 3600.0;
        let rad = deg * PI / 180.0;
        let result = dhms_format("dms", 7, rad);
        assert_eq!(result, "45:00:00");
    }

    #[test]
    fn format_hms_24h_no_wrap() {
        // 23h 59m 59.96s at precision 1 rounds the seconds up and carries to
        // 24:00:00.0 — C's eraA2tf does NOT range-reduce, so it is not wrapped
        // back to 0 (ERFA a2tf note 3).
        let hours = 23.0 + 59.0 / 60.0 + 59.96 / 3600.0;
        let rad = hours * PI / 12.0;
        let result = dhms_format("hms.1", 7, rad);
        assert_eq!(result, "24:00:00.0");
    }

    #[test]
    fn format_hms_over_24h_verbatim() {
        // A value exceeding 24h formats verbatim (e.g. an offset SkyRef).
        let hours = 458.0 + 21.0 / 60.0 + 58.4 / 3600.0;
        let rad = hours * PI / 12.0;
        assert_eq!(dhms_format("hms.1", 7, rad), "458:21:58.4");
    }

    #[test]
    fn format_hms_negative_signed() {
        // Negative value carries a leading "-" (eraA2tf sign), not a wrap.
        assert_eq!(dhms_format("hms.1", 7, -PI / 2.0), "-6:00:00.0");
    }
}
