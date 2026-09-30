//! The attributes C's Frame class stores for a whole Frame (frame.c), shared
//! by every Rust Frame class, and the Frame class's part of the Native dump.
//!
//! Each attribute is `None` until set. Defaults that depend on the class (the
//! Title, Domain and Epoch of a SkyFrame, say) are supplied by the owning
//! class, as C's subclasses over-ride the Frame class's Get methods.

use crate::serial::{Node, NodeValue};

/// Frame-class attributes. Mirrors the instance variables of C's `AstFrame`
/// (frame.h) other than the axes, the permutation and the System values,
/// which are class-specific.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FrameAttrs {
    pub title: Option<String>,
    pub domain: Option<String>,
    /// Epoch as a TDB Modified Julian Date.
    pub epoch: Option<f64>,
    pub digits: Option<i32>,
    /// Observer's geodetic latitude, radians.
    pub obs_lat: Option<f64>,
    /// Observer's geodetic longitude, radians.
    pub obs_lon: Option<f64>,
    /// Observer's geodetic altitude, metres.
    pub obs_alt: Option<f64>,
    /// TAI-UTC, seconds.
    pub dtai: Option<f64>,
    /// UT1-UTC, seconds.
    pub dut1: Option<f64>,
    pub match_end: Option<bool>,
    pub min_axes: Option<usize>,
    pub max_axes: Option<usize>,
    pub permute: Option<bool>,
    pub preserve_axes: Option<bool>,
    pub active_unit: Option<bool>,
    /// The Object-class Ident attribute.
    pub ident: Option<String>,
}

impl FrameAttrs {
    /// Copy the attributes set on `template` onto `self`, as the attribute
    /// part of C's `Frame::Overlay` does (frame.c:8580-8600): Dtai, Dut1,
    /// Digits, Domain, Epoch, Title, ObsLat, ObsLon and ObsAlt.
    pub(crate) fn overlay_from(&mut self, template: &FrameAttrs) {
        macro_rules! overlay {
            ($($field:ident),*) => {$(
                if template.$field.is_some() {
                    self.$field.clone_from(&template.$field);
                }
            )*};
        }
        overlay!(
            dtai, dut1, digits, domain, epoch, title, obs_lat, obs_lon, obs_alt
        );
    }
}

/// A Domain as C stores it: white space removed and letters upper-cased
/// (frame.c `CleanDomain`, applied by `SetDomain`).
pub(crate) fn clean_domain(domain: &str) -> String {
    domain
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The epoch C's Frame class uses when none is set, J2000.0 (frame.c,
/// `GetEpoch`).
pub(crate) fn default_epoch_mjd() -> f64 {
    crate::pal::epj2d(2000.0)
}

/// The default Symbol of an axis: the Domain if one is set, else `"x"`,
/// followed by the 1-based internal axis number, with white space replaced by
/// underscores and the whole limited to 50 characters. Mirrors C's
/// `GetDefaultSymbol` (frame.c:5419).
pub(crate) fn default_symbol(set_domain: Option<&str>, internal_axis: usize) -> String {
    const SYMBOL_BUFF_LEN: usize = 50;
    let number = (internal_axis + 1).to_string();
    let prefix: String = set_domain
        .unwrap_or("x")
        .chars()
        .take(SYMBOL_BUFF_LEN - number.len())
        .collect();
    format!("{prefix}{number}")
        .chars()
        .map(|c| if c.is_ascii_whitespace() { '_' } else { c })
        .collect()
}

/// What C's `Frame::Dump` reads from a Frame, through its (possibly
/// over-ridden) methods, to write the Frame class's part of the dump.
///
/// Every per-axis method takes a 0-based *internal* axis index, because the
/// dump is written in unpermuted order.
pub(crate) trait FrameDumpSource {
    /// The Frame-class attributes.
    fn attrs(&self) -> &FrameAttrs;
    /// Number of axes.
    fn dump_naxes(&self) -> usize;
    /// The permutation, `perm[external] = internal`.
    fn dump_perm(&self) -> Vec<usize>;
    /// Whether the Frame is a plain Frame (or a CmpFrame, or a FrameSet whose
    /// current Frame is plain), which hides some unset values. Mirrors
    /// `isFrame` in frame.c:14290.
    fn dump_is_frame(&self) -> bool;
    /// The Title, set or default.
    fn dump_title(&self) -> String;
    /// The Domain, set or default.
    fn dump_domain(&self) -> String;
    /// The Epoch as an MJD, set or default.
    fn dump_epoch(&self) -> f64;
    /// The System: whether it is set, and its name.
    fn dump_system(&self) -> (bool, String);
    /// The AlignSystem: whether it is set, and its name.
    fn dump_align_system(&self) -> (bool, String);
    /// Label, Symbol, Unit and Format of an internal axis, set or default.
    fn dump_label(&self, axis: usize) -> String;
    /// See [`dump_label`](Self::dump_label).
    fn dump_symbol(&self, axis: usize) -> String;
    /// See [`dump_label`](Self::dump_label).
    fn dump_unit(&self, axis: usize) -> String;
    /// See [`dump_label`](Self::dump_label).
    fn dump_format(&self, axis: usize) -> String;
    /// The axis's own Digits, if set.
    fn dump_axis_digits(&self, axis: usize) -> Option<i32>;
    /// The Direction of an internal axis, set or default.
    fn dump_direction(&self, axis: usize) -> bool;
    /// Bottom and Top of an internal axis, set or default.
    fn dump_bottom(&self, axis: usize) -> f64;
    /// See [`dump_bottom`](Self::dump_bottom).
    fn dump_top(&self, axis: usize) -> f64;
    /// PreserveAxes, Permute, MinAxes, MaxAxes and MatchEnd, set or default.
    fn dump_preserve_axes(&self) -> bool;
    /// See [`dump_preserve_axes`](Self::dump_preserve_axes).
    fn dump_permute(&self) -> bool;
    /// See [`dump_preserve_axes`](Self::dump_preserve_axes).
    fn dump_min_axes(&self) -> usize;
    /// See [`dump_preserve_axes`](Self::dump_preserve_axes).
    fn dump_max_axes(&self) -> usize;
    /// See [`dump_preserve_axes`](Self::dump_preserve_axes).
    fn dump_match_end(&self) -> bool;
    /// ActiveUnit, when C's `astTestActiveUnit` reports it set.
    fn dump_active_unit(&self) -> Option<bool>;
}

/// Write the Frame class's part of a dump, from Title to the permutation, as
/// C's `Frame::Dump` does (frame.c:14250-14650). The Axis objects, written
/// after it, are the caller's.
pub(crate) fn write_frame_cards(node: &mut Node, f: &impl FrameDumpSource) {
    let a = f.attrs();
    let naxes = f.dump_naxes();
    let is_frame = f.dump_is_frame();

    // Each card is written active when set; otherwise as a comment when
    // "helpful", and not at all when not.
    fn string(node: &mut Node, key: &str, set: bool, helpful: bool, v: String, c: String) {
        if set {
            node.push_string(key, &v, Some(c));
        } else if helpful {
            node.push_unset(key, NodeValue::String(v), Some(c));
        }
    }
    fn int(node: &mut Node, key: &str, set: bool, helpful: bool, v: i64, c: String) {
        if set {
            node.push_int(key, v, Some(c));
        } else if helpful {
            node.push_unset(key, NodeValue::Int(v), Some(c));
        }
    }
    fn double(node: &mut Node, key: &str, set: bool, helpful: bool, v: f64, c: String) {
        if set {
            node.push_double(key, v, Some(c));
        } else if helpful {
            node.push_unset(key, NodeValue::Double(v), Some(c));
        }
    }

    string(
        node,
        "Title",
        a.title.is_some(),
        true,
        f.dump_title(),
        "Title of coordinate system".into(),
    );
    int(
        node,
        "Naxes",
        true,
        true,
        naxes as i64,
        "Number of coordinate axes".into(),
    );
    let domain = f.dump_domain();
    let helpful = !domain.is_empty();
    string(
        node,
        "Domain",
        a.domain.is_some(),
        helpful,
        domain,
        "Coordinate system domain".into(),
    );

    // The Epoch is written as a Besselian year before 1984 and a Julian year
    // from then on (frame.c:14338-14343).
    let mjd = f.dump_epoch();
    let bessyr = mjd < crate::pal::epj2d(1984.0);
    let year = if bessyr {
        crate::pal::epb(mjd)
    } else {
        crate::pal::epj(mjd)
    };
    let comment = if bessyr {
        "Besselian epoch of observation"
    } else {
        "Julian epoch of observation"
    };
    double(
        node,
        "Epoch",
        a.epoch.is_some(),
        !is_frame,
        year,
        comment.into(),
    );

    for axis in 0..naxes {
        string(
            node,
            &format!("Lbl{}", axis + 1),
            false,
            true,
            f.dump_label(axis),
            format!("Label for axis {}", axis + 1),
        );
    }
    for axis in 0..naxes {
        string(
            node,
            &format!("Sym{}", axis + 1),
            false,
            false,
            f.dump_symbol(axis),
            format!("Symbol for axis {}", axis + 1),
        );
    }

    let (set, system) = f.dump_system();
    string(
        node,
        "System",
        set,
        !is_frame,
        system,
        "Coordinate system type".into(),
    );
    let (set, align) = f.dump_align_system();
    string(
        node,
        "AlSys",
        set,
        false,
        align,
        "Alignment coordinate system".into(),
    );

    for axis in 0..naxes {
        let unit = f.dump_unit(axis);
        let comment = match crate::units::unit_label(&unit) {
            Some(lab) if lab != unit => format!("Units for axis {} ({lab})", axis + 1),
            _ => format!("Units for axis {}", axis + 1),
        };
        let helpful = !unit.is_empty();
        string(
            node,
            &format!("Uni{}", axis + 1),
            false,
            helpful,
            unit,
            comment,
        );
    }

    // Per-axis Digits, then the Frame's (shown when any axis has its own),
    // then each Format (shown for an axis with its own Digits).
    let frame_digits = a.digits.unwrap_or(7);
    let mut any_axis_digits = false;
    for axis in 0..naxes {
        let own = f.dump_axis_digits(axis);
        any_axis_digits |= own.is_some();
        int(
            node,
            &format!("Dig{}", axis + 1),
            false,
            own.is_some(),
            own.unwrap_or(frame_digits) as i64,
            format!("Individual precision for axis {}", axis + 1),
        );
    }
    int(
        node,
        "Digits",
        a.digits.is_some(),
        any_axis_digits,
        frame_digits as i64,
        "Default formatting precision".into(),
    );
    for axis in 0..naxes {
        string(
            node,
            &format!("Fmt{}", axis + 1),
            false,
            f.dump_axis_digits(axis).is_some(),
            f.dump_format(axis),
            format!("Format specifier for axis {}", axis + 1),
        );
    }

    for axis in 0..naxes {
        let dir = f.dump_direction(axis);
        let comment = if dir {
            format!("Plot axis {} in conventional direction", axis + 1)
        } else {
            format!("Plot axis {} in reverse direction", axis + 1)
        };
        int(
            node,
            &format!("Dir{}", axis + 1),
            false,
            !dir,
            i64::from(dir),
            comment,
        );
    }
    for axis in 0..naxes {
        let v = f.dump_bottom(axis);
        double(
            node,
            &format!("Bot{}", axis + 1),
            false,
            v != -f64::MAX,
            v,
            "Lowest legal axis value".into(),
        );
    }
    for axis in 0..naxes {
        let v = f.dump_top(axis);
        double(
            node,
            &format!("Top{}", axis + 1),
            false,
            v != f64::MAX,
            v,
            "Highest legal axis value".into(),
        );
    }

    let v = f.dump_preserve_axes();
    int(
        node,
        "Presrv",
        a.preserve_axes.is_some(),
        false,
        i64::from(v),
        if v {
            "Preserve target axes"
        } else {
            "Don't preserve target axes"
        }
        .into(),
    );
    let v = f.dump_permute();
    int(
        node,
        "Permut",
        a.permute.is_some(),
        false,
        i64::from(v),
        if v {
            "Axes may be permuted to match"
        } else {
            "Axes may not be permuted match"
        }
        .into(),
    );
    int(
        node,
        "MinAx",
        a.min_axes.is_some(),
        false,
        f.dump_min_axes() as i64,
        "Minimum number of axes to match".into(),
    );
    int(
        node,
        "MaxAx",
        a.max_axes.is_some(),
        false,
        f.dump_max_axes() as i64,
        "Maximum number of axes to match".into(),
    );
    let v = f.dump_match_end();
    int(
        node,
        "MchEnd",
        a.match_end.is_some(),
        false,
        i64::from(v),
        if v {
            "Match final target axes"
        } else {
            "Match initial target axes"
        }
        .into(),
    );

    let obs = [
        ("ObsLat", a.obs_lat, "Observers geodetic latitude (rads)"),
        ("ObsLon", a.obs_lon, "Observers geodetic longitude (rads)"),
        ("ObsAlt", a.obs_alt, "Observers geodetic altitude (metres)"),
        ("Dtai", a.dtai, "TAI-UTC in seconds"),
        ("Dut1", a.dut1, "UT1-UTC in seconds"),
    ];
    for (key, value, comment) in obs {
        if let Some(v) = value {
            node.push_double(key, v, Some(comment.to_string()));
        }
    }

    if let Some(v) = f.dump_active_unit() {
        node.push_int(
            "ActUnt",
            i64::from(v),
            Some(
                if v {
                    "Unit strings affects alignment"
                } else {
                    "Unit strings do not affect alignment"
                }
                .to_string(),
            ),
        );
    }

    for (ext, &internal) in f.dump_perm().iter().enumerate() {
        if internal != ext {
            node.push_int(
                &format!("Prm{}", ext + 1),
                (internal + 1) as i64,
                Some(format!(
                    "Axis {} permuted to use internal axis {}",
                    ext + 1,
                    internal + 1
                )),
            );
        }
    }
}

/// Read the Frame-class attributes from a Frame node. Mirrors the attribute
/// part of C's `astLoadFrame` (frame.c).
pub(crate) fn read_frame_attrs(node: &Node) -> FrameAttrs {
    let bool_of = |key: &str| node.get(key).map(|_| node.get_int(key, 0) != 0);
    let usize_of = |key: &str| node.get(key).map(|_| node.get_int(key, 0) as usize);
    let f64_of = |key: &str| node.get(key).map(|_| node.get_double(key, 0.0));
    FrameAttrs {
        title: node.get_string_if_set("Title"),
        domain: node.get_string_if_set("Domain"),
        // The Epoch is stored as a year, Besselian before 1984 and Julian
        // from then on (frame.c, `astLoadFrame`).
        epoch: f64_of("Epoch").map(crate::frames::date_time::year_to_mjd_iau1984),
        digits: node.get("Digits").map(|_| node.get_int("Digits", 7) as i32),
        obs_lat: f64_of("ObsLat"),
        obs_lon: f64_of("ObsLon"),
        obs_alt: f64_of("ObsAlt"),
        dtai: f64_of("Dtai"),
        dut1: f64_of("Dut1"),
        match_end: bool_of("MchEnd"),
        min_axes: usize_of("MinAx"),
        max_axes: usize_of("MaxAx"),
        permute: bool_of("Permut"),
        preserve_axes: bool_of("Presrv"),
        active_unit: bool_of("ActUnt"),
        ident: node.get_string_if_set("Ident"),
    }
}

// ---------------------------------------------------------------------------
// `Frame` trait implementations over `FrameAttrs`
// ---------------------------------------------------------------------------
//
// Used inside `impl Frame for X`, where `X` has an `attrs: FrameAttrs` field.
// Each macro implements one group of attributes; a class whose C counterpart
// over-rides a group writes that group by hand instead. The class supplies
// its defaults through the inherent hooks the macros name:
// `frame_default_title`, `frame_default_domain`, `frame_default_epoch` and
// `frame_default_permute`.

/// Title, from `self.attrs.title` and `self.frame_default_title()`.
macro_rules! frame_attrs_title {
    () => {
        fn title(&self) -> String {
            self.attrs
                .title
                .clone()
                .unwrap_or_else(|| self.frame_default_title())
        }
        fn set_title(&mut self, title: &str) -> $crate::error::AstResult<()> {
            self.attrs.title = Some(title.to_string());
            Ok(())
        }
        fn test_title(&self) -> bool {
            self.attrs.title.is_some()
        }
        fn clear_title(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.title = None;
            Ok(())
        }
    };
}

/// Domain, from `self.attrs.domain` and `self.frame_default_domain()`.
macro_rules! frame_attrs_domain {
    () => {
        fn domain(&self) -> &str {
            self.attrs
                .domain
                .as_deref()
                .unwrap_or_else(|| self.frame_default_domain())
        }
        fn set_domain(&mut self, domain: &str) -> $crate::error::AstResult<()> {
            self.attrs.domain = Some($crate::frames::frame_attrs::clean_domain(domain));
            Ok(())
        }
        fn test_domain(&self) -> bool {
            self.attrs.domain.is_some()
        }
        fn clear_domain(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.domain = None;
            Ok(())
        }
    };
}

/// Epoch, from `self.attrs.epoch` and `self.frame_default_epoch()`.
macro_rules! frame_attrs_epoch {
    () => {
        fn epoch_mjd(&self) -> f64 {
            self.attrs
                .epoch
                .unwrap_or_else(|| self.frame_default_epoch())
        }
        fn set_epoch_mjd(&mut self, mjd: f64) -> $crate::error::AstResult<()> {
            self.attrs.epoch = Some(mjd);
            Ok(())
        }
        fn test_epoch(&self) -> bool {
            self.attrs.epoch.is_some()
        }
        fn clear_epoch(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.epoch = None;
            Ok(())
        }
    };
}

/// Digits, from `self.attrs.digits`, default 7.
macro_rules! frame_attrs_digits {
    () => {
        fn digits(&self) -> i32 {
            self.attrs.digits.unwrap_or(7)
        }
        fn set_digits(&mut self, value: i32) -> $crate::error::AstResult<()> {
            self.attrs.digits = Some(value.max(1));
            Ok(())
        }
        fn test_digits(&self) -> bool {
            self.attrs.digits.is_some()
        }
        fn clear_digits(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.digits = None;
            Ok(())
        }
    };
}

/// One optional f64 attribute of `self.attrs` with a fixed default.
macro_rules! frame_attrs_f64 {
    ($field:ident, $get:ident, $set:ident, $test:ident, $clear:ident, $default:expr) => {
        fn $get(&self) -> f64 {
            self.attrs.$field.unwrap_or($default)
        }
        fn $set(&mut self, value: f64) -> $crate::error::AstResult<()> {
            self.attrs.$field = Some(value);
            Ok(())
        }
        fn $test(&self) -> bool {
            self.attrs.$field.is_some()
        }
        fn $clear(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.$field = None;
            Ok(())
        }
    };
}

/// ObsLat, ObsLon, ObsAlt, Dtai and Dut1 from `self.attrs`, with C's
/// defaults (0, and `BAD_VALUE` for Dtai).
macro_rules! frame_attrs_observer {
    () => {
        frame_attrs_f64!(
            obs_lat,
            obs_lat,
            set_obs_lat,
            test_obs_lat,
            clear_obs_lat,
            0.0
        );
        frame_attrs_f64!(
            obs_lon,
            obs_lon,
            set_obs_lon,
            test_obs_lon,
            clear_obs_lon,
            0.0
        );
        frame_attrs_f64!(
            obs_alt,
            obs_alt,
            set_obs_alt,
            test_obs_alt,
            clear_obs_alt,
            0.0
        );
        frame_attrs_f64!(
            dtai,
            dtai,
            set_dtai,
            test_dtai,
            clear_dtai,
            $crate::constants::BAD_VALUE
        );
        frame_attrs_f64!(dut1, dut1, set_dut1, test_dut1, clear_dut1, 0.0);
    };
}

/// Ident, from `self.attrs.ident`; setting a blank Ident clears it.
macro_rules! frame_attrs_ident {
    () => {
        fn ident(&self) -> &str {
            self.attrs.ident.as_deref().unwrap_or("")
        }
        fn set_ident(&mut self, ident: &str) -> $crate::error::AstResult<()> {
            self.attrs.ident = (!ident.is_empty()).then(|| ident.to_string());
            Ok(())
        }
        fn clear_ident(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.ident = None;
            Ok(())
        }
        fn test_ident(&self) -> bool {
            self.attrs.ident.is_some()
        }
    };
}

/// MinAxes, MaxAxes, MatchEnd, PreserveAxes and Permute from `self.attrs`.
/// MinAxes and MaxAxes default to the number of axes, MatchEnd and
/// PreserveAxes to false, and Permute to `self.frame_default_permute()`.
macro_rules! frame_attrs_match {
    () => {
        frame_attrs_match!(@body |s| s.naxes(), |s| s.naxes());
    };
    // Explicit MinAxes and MaxAxes defaults, for a class that over-rides
    // them (CmpFrame, cmpframe.c `GetMinAxes`, `GetMaxAxes`).
    (min = $min:expr, max = $max:expr) => {
        frame_attrs_match!(@body |_s| $min, |_s| $max);
    };
    (@body |$smin:ident| $min:expr, |$smax:ident| $max:expr) => {
        fn min_axes(&self) -> usize {
            self.attrs.min_axes.unwrap_or_else(|| {
                let $smin = self;
                $min
            })
        }
        fn set_min_axes(&mut self, n: usize) -> $crate::error::AstResult<()> {
            self.attrs.min_axes = Some(n);
            Ok(())
        }
        fn test_min_axes(&self) -> bool {
            self.attrs.min_axes.is_some()
        }
        fn clear_min_axes(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.min_axes = None;
            Ok(())
        }
        fn max_axes(&self) -> usize {
            self.attrs.max_axes.unwrap_or_else(|| {
                let $smax = self;
                $max
            })
        }
        fn set_max_axes(&mut self, n: usize) -> $crate::error::AstResult<()> {
            self.attrs.max_axes = Some(n);
            Ok(())
        }
        fn test_max_axes(&self) -> bool {
            self.attrs.max_axes.is_some()
        }
        fn clear_max_axes(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.max_axes = None;
            Ok(())
        }
        fn match_end(&self) -> bool {
            self.attrs.match_end.unwrap_or(false)
        }
        fn set_match_end(&mut self, v: bool) -> $crate::error::AstResult<()> {
            self.attrs.match_end = Some(v);
            Ok(())
        }
        fn test_match_end(&self) -> bool {
            self.attrs.match_end.is_some()
        }
        fn clear_match_end(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.match_end = None;
            Ok(())
        }
        fn preserve_axes(&self) -> bool {
            self.attrs.preserve_axes.unwrap_or(false)
        }
        fn set_preserve_axes(&mut self, v: bool) -> $crate::error::AstResult<()> {
            self.attrs.preserve_axes = Some(v);
            Ok(())
        }
        fn test_preserve_axes(&self) -> bool {
            self.attrs.preserve_axes.is_some()
        }
        fn clear_preserve_axes(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.preserve_axes = None;
            Ok(())
        }
        fn permute(&self) -> bool {
            self.attrs
                .permute
                .unwrap_or_else(|| self.frame_default_permute())
        }
        fn set_permute(&mut self, v: bool) -> $crate::error::AstResult<()> {
            self.attrs.permute = Some(v);
            Ok(())
        }
        fn test_permute(&self) -> bool {
            self.attrs.permute.is_some()
        }
        fn clear_permute(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.permute = None;
            Ok(())
        }
    };
}

/// Forward every Frame-class attribute to `self.<path>`, which is itself a
/// Frame. Used by the Regions, which forward to their FrameSet (whose
/// setters apply FrameSet integrity) as C's Region class does (region.c).
macro_rules! forward_frame_attrs {
    ($($path:ident).+) => {
        forward_frame_attrs!(@all_but_domain [$($path).+]);
        fn domain(&self) -> &str {
            $crate::frames::Frame::domain(&self.$($path).+)
        }
        forward_frame_attrs!(@set [$($path).+] set_domain(domain: &str));
        forward_frame_attrs!(@get [$($path).+] test_domain -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_domain);
    };
    // Every attribute but Domain, for a class that supplies its own Domain
    // default.
    (@all_but_domain [$($path:ident).+]) => {
        forward_frame_attrs!(@get [$($path).+] title -> String);
        forward_frame_attrs!(@set [$($path).+] set_title(title: &str));
        forward_frame_attrs!(@get [$($path).+] test_title -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_title);
        forward_frame_attrs!(@get [$($path).+] epoch_mjd -> f64);
        forward_frame_attrs!(@set [$($path).+] set_epoch_mjd(mjd: f64));
        forward_frame_attrs!(@get [$($path).+] test_epoch -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_epoch);
        forward_frame_attrs!(@get [$($path).+] digits -> i32);
        forward_frame_attrs!(@set [$($path).+] set_digits(value: i32));
        forward_frame_attrs!(@get [$($path).+] test_digits -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_digits);
        forward_frame_attrs!(@get [$($path).+] obs_lat -> f64);
        forward_frame_attrs!(@set [$($path).+] set_obs_lat(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_obs_lat -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_obs_lat);
        forward_frame_attrs!(@get [$($path).+] obs_lon -> f64);
        forward_frame_attrs!(@set [$($path).+] set_obs_lon(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_obs_lon -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_obs_lon);
        forward_frame_attrs!(@get [$($path).+] obs_alt -> f64);
        forward_frame_attrs!(@set [$($path).+] set_obs_alt(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_obs_alt -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_obs_alt);
        forward_frame_attrs!(@get [$($path).+] dtai -> f64);
        forward_frame_attrs!(@set [$($path).+] set_dtai(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_dtai -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_dtai);
        forward_frame_attrs!(@get [$($path).+] dut1 -> f64);
        forward_frame_attrs!(@set [$($path).+] set_dut1(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_dut1 -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_dut1);
        forward_frame_attrs!(@get [$($path).+] min_axes -> usize);
        forward_frame_attrs!(@set [$($path).+] set_min_axes(n: usize));
        forward_frame_attrs!(@get [$($path).+] test_min_axes -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_min_axes);
        forward_frame_attrs!(@get [$($path).+] max_axes -> usize);
        forward_frame_attrs!(@set [$($path).+] set_max_axes(n: usize));
        forward_frame_attrs!(@get [$($path).+] test_max_axes -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_max_axes);
        forward_frame_attrs!(@get [$($path).+] match_end -> bool);
        forward_frame_attrs!(@set [$($path).+] set_match_end(v: bool));
        forward_frame_attrs!(@get [$($path).+] test_match_end -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_match_end);
        forward_frame_attrs!(@get [$($path).+] preserve_axes -> bool);
        forward_frame_attrs!(@set [$($path).+] set_preserve_axes(v: bool));
        forward_frame_attrs!(@get [$($path).+] test_preserve_axes -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_preserve_axes);
        forward_frame_attrs!(@get [$($path).+] permute -> bool);
        forward_frame_attrs!(@set [$($path).+] set_permute(v: bool));
        forward_frame_attrs!(@get [$($path).+] test_permute -> bool);
        forward_frame_attrs!(@clear [$($path).+] clear_permute);
        fn get_active_unit(&self) -> bool {
            $crate::frames::Frame::get_active_unit(&self.$($path).+)
        }
        fn set_active_unit(&mut self, value: bool) {
            $crate::frames::Frame::set_active_unit(&mut self.$($path).+, value)
        }
        fn test_active_unit(&self) -> bool {
            $crate::frames::Frame::test_active_unit(&self.$($path).+)
        }
    };
    (@get [$($path:ident).+] $name:ident -> $ty:ty) => {
        fn $name(&self) -> $ty {
            $crate::frames::Frame::$name(&self.$($path).+)
        }
    };
    (@set [$($path:ident).+] $name:ident($arg:ident: $ty:ty)) => {
        fn $name(&mut self, $arg: $ty) -> $crate::error::AstResult<()> {
            $crate::frames::Frame::$name(&mut self.$($path).+, $arg)
        }
    };
    (@clear [$($path:ident).+] $name:ident) => {
        fn $name(&mut self) -> $crate::error::AstResult<()> {
            $crate::frames::Frame::$name(&mut self.$($path).+)
        }
    };
}

/// The Frame-class attributes of a Region: every one forwarded to its
/// FrameSet, as C's Region class does (region.c), except the Object-class
/// Ident, which is the Region's own.
macro_rules! region_frame_attrs {
    () => {
        forward_frame_attrs!(data.frameset);
        fn ident(&self) -> &str {
            self.data.ident.as_deref().unwrap_or("")
        }
        fn set_ident(&mut self, ident: &str) -> $crate::error::AstResult<()> {
            self.data.ident = (!ident.is_empty()).then(|| ident.to_string());
            Ok(())
        }
        fn clear_ident(&mut self) -> $crate::error::AstResult<()> {
            self.data.ident = None;
            Ok(())
        }
        fn test_ident(&self) -> bool {
            self.data.ident.is_some()
        }
    };
}

/// The Mapping-side Ident of a Region, the same value as its Frame side.
macro_rules! region_mapping_ident {
    () => {
        fn ident(&self) -> &str {
            self.data.ident.as_deref().unwrap_or("")
        }
        fn set_ident(&mut self, ident: &str) {
            self.data.ident = (!ident.is_empty()).then(|| ident.to_string());
        }
        fn clear_ident(&mut self) {
            self.data.ident = None;
        }
        fn test_ident(&self) -> bool {
            self.data.ident.is_some()
        }
    };
}

/// An attribute a CmpFrame shares with its components `$f1` and `$f2`, as C's CmpFrame
/// over-rides it (cmpframe.c `GetEpoch`, `SetEpoch`, `ClearEpoch` and
/// the like for ObsLat, ObsLon, ObsAlt, Dtai and Dut1): the value set on
/// the CmpFrame, else the one set on the first component, else the one set
/// on the second, else the first component's default. Setting or clearing
/// it acts on the CmpFrame and on both components; testing it asks the
/// CmpFrame alone.
macro_rules! cmp_frame_shared_attr {
    // Components held by value, reached through the `Frame` trait so that
    // their own inherent methods do not shadow it.
    (concrete $f1:ident, $f2:ident; $field:ident, $get:ident, $set:ident, $test:ident, $clear:ident) => {
        fn $get(&self) -> f64 {
            use $crate::frames::Frame as F;
            if let Some(v) = self.attrs.$field {
                v
            } else if F::$test(&self.$f1) {
                F::$get(&self.$f1)
            } else if F::$test(&self.$f2) {
                F::$get(&self.$f2)
            } else {
                F::$get(&self.$f1)
            }
        }
        fn $set(&mut self, v: f64) -> $crate::error::AstResult<()> {
            use $crate::frames::Frame as F;
            self.attrs.$field = Some(v);
            F::$set(&mut self.$f1, v)?;
            F::$set(&mut self.$f2, v)
        }
        fn $test(&self) -> bool {
            self.attrs.$field.is_some()
        }
        fn $clear(&mut self) -> $crate::error::AstResult<()> {
            use $crate::frames::Frame as F;
            self.attrs.$field = None;
            F::$clear(&mut self.$f1)?;
            F::$clear(&mut self.$f2)
        }
    };
    ($f1:ident, $f2:ident; $field:ident, $get:ident, $set:ident, $test:ident, $clear:ident) => {
        fn $get(&self) -> f64 {
            if let Some(v) = self.attrs.$field {
                v
            } else if self.$f1.$test() {
                self.$f1.$get()
            } else if self.$f2.$test() {
                self.$f2.$get()
            } else {
                self.$f1.$get()
            }
        }
        fn $set(&mut self, v: f64) -> $crate::error::AstResult<()> {
            self.attrs.$field = Some(v);
            self.$f1.$set(v)?;
            self.$f2.$set(v)
        }
        fn $test(&self) -> bool {
            self.attrs.$field.is_some()
        }
        fn $clear(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.$field = None;
            self.$f1.$clear()?;
            self.$f2.$clear()
        }
    };
}
