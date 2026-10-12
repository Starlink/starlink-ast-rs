//! The attributes C's Frame class stores for a whole Frame (frame.c), shared
//! by every Rust Frame class, and the Frame class's part of the Native dump.
//!
//! Each attribute is `None` until set. Defaults that depend on the class (the
//! Title, Domain and Epoch of a SkyFrame, say) are supplied by the owning
//! class, as C's subclasses over-ride the Frame class's Get methods.

use crate::error::{AstError, AstResult, ErrorCode};
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
    /// The Object-class ID attribute, which a clone does not inherit.
    pub id: crate::object_id::ObjectId,
    /// The Object-class UseDefs attribute.
    pub use_defs: Option<bool>,
    /// C's frame flags (frame.h `flags`, `astGetFrameFlags`): transient
    /// markers such as [`INTFLAG`](super::INTFLAG), not an attribute, so
    /// never dumped.
    pub flags: u32,
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
    /// The number of axes the Frame class's own structure holds, C's
    /// `this->naxes`, which decides whether Naxes is written active and how
    /// many `Prm` cards there are (frame.c Dump). A CmpFrame keeps its axes
    /// in its components and holds none.
    fn dump_own_naxes(&self) -> usize {
        self.dump_naxes()
    }
    /// Whether the Frame is a plain Frame (or a CmpFrame, or a FrameSet whose
    /// current Frame is plain), which hides some unset values. Mirrors
    /// `isFrame` in frame.c:14290.
    fn dump_is_frame(&self) -> bool;
    /// The Title, set or default.
    fn dump_title(&self) -> AstResult<String>;
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
pub(crate) fn write_frame_cards(node: &mut Node, f: &impl FrameDumpSource) -> AstResult<()> {
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
        f.dump_title()?,
        "Title of coordinate system".into(),
    );
    let own_naxes = f.dump_own_naxes();
    int(
        node,
        "Naxes",
        own_naxes != 0,
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

    for (ext, &internal) in f.dump_perm().iter().enumerate().take(own_naxes) {
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
    Ok(())
}

/// Record as read the per-axis items `astLoadFrame` reads from a Frame part
/// of a description (frame.c): each axis's `Ax`, its permutation `Prm`, and
/// the `Lbl`, `Sym`, `Fmt`, `Uni`, `Dir`, `Top`, `Bot` and `Dig` that older
/// versions wrote in the Frame part, for as many axes as `Naxes` gives.
pub(crate) fn mark_frame_axes_read(frame_part: &Node) {
    let naxes = frame_part.get_int("Naxes", 0).max(0);
    for i in 1..=naxes {
        for key in [
            "Ax", "Lbl", "Sym", "Fmt", "Uni", "Dir", "Top", "Bot", "Dig", "Prm",
        ] {
            frame_part.mark_read(&format!("{key}{i}"));
        }
    }
}

/// Read the Frame-class attributes from a Frame node. Mirrors the attribute
/// part of C's `astLoadFrame` (frame.c).
/// Write the Object part of a dump: ID when set, Ident when given, UseDefs
/// when set, then the `IsA Object` marker when any was written (object.c
/// `Dump`, where an unset ID or Ident is blank and so not helpful, and
/// UseDefs is never written unset).
pub(crate) fn write_object_part(
    node: &mut Node,
    id: &crate::object_id::ObjectId,
    ident: Option<&str>,
    use_defs: Option<bool>,
) {
    if let Some(id) = id.as_option() {
        node.push_string("ID", id, Some("Object identification string".to_string()));
    }
    if let Some(ident) = ident {
        node.push_string(
            "Ident",
            ident,
            Some("Permanent Object identification string".to_string()),
        );
    }
    if let Some(use_defs) = use_defs {
        node.push_int(
            "UseDfs",
            i64::from(use_defs),
            Some(if use_defs {
                "Default attribute values can be used".to_string()
            } else {
                "Default values cannot be used".to_string()
            }),
        );
    }
    if id.test() || ident.is_some() || use_defs.is_some() {
        node.push_isa("Object", Some("AST Object".to_string()));
    }
}

/// Read the System item `key` (`System` or `AlSys`), naming the attribute
/// `attr` in the error. A name the class does not know is AST__ATTIN, as
/// `astLoadFrame` reports it through the class's `astSystemCode`
/// (frame.c:15392).
pub(crate) fn read_system_item<T>(
    node: &Node,
    key: &str,
    attr: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> AstResult<Option<T>> {
    let Some(name) = node.get_string_if_set(key) else {
        return Ok(None);
    };
    parse(&name).map(Some).ok_or_else(|| {
        AstError::new(
            ErrorCode::Attin,
            format!("astRead(Channel): Invalid {attr} description \"{name}\"."),
        )
    })
}

pub(crate) fn read_frame_attrs(whole: &Node) -> FrameAttrs {
    // The Object and Frame classes' parts of the description, which their
    // loaders read in C (object.c, frame.c `astLoadFrame`).
    let object = whole.section("Object");
    let node = &whole.section("Frame");
    // astLoadFrame reads the number of axes, which each class takes from
    // its own description.
    node.mark_read("Naxes");
    let bool_of = |key: &str| node.get(key).map(|_| node.get_int(key, 0) != 0);
    let f64_of = |key: &str| node.get(key).map(|_| node.get_double(key, 0.0));
    // astLoadFrame passes each value it reads through the class's setter,
    // so the setters' constraints apply: Digits is at least 1, the axis
    // counts are not negative, and a MaxAxes below MinAxes lowers MinAxes
    // (frame.c `ConsistentMaxAxes`).
    let min_axes = node
        .get("MinAx")
        .map(|_| node.get_int("MinAx", 0).max(0) as usize);
    let max_axes = node
        .get("MaxAx")
        .map(|_| node.get_int("MaxAx", 0).max(0) as usize);
    let min_axes = match (min_axes, max_axes) {
        (Some(min), Some(max)) if min > max => Some(max),
        _ => min_axes,
    };
    FrameAttrs {
        title: node.get_string_if_set("Title"),
        domain: node.get_string_if_set("Domain"),
        // The Epoch is stored as a year, Besselian before 1984 and Julian
        // from then on (frame.c, `astLoadFrame`).
        epoch: f64_of("Epoch").map(crate::frames::date_time::year_to_mjd_iau1984),
        digits: node
            .get("Digits")
            .map(|_| (node.get_int("Digits", 7) as i32).max(1)),
        obs_lat: f64_of("ObsLat"),
        obs_lon: f64_of("ObsLon"),
        obs_alt: f64_of("ObsAlt"),
        dtai: f64_of("Dtai"),
        dut1: f64_of("Dut1"),
        match_end: bool_of("MchEnd"),
        min_axes,
        max_axes,
        permute: bool_of("Permut"),
        preserve_axes: bool_of("Presrv"),
        active_unit: bool_of("ActUnt"),
        ident: object.get_string_if_set("Ident"),
        id: crate::object_id::ObjectId::from_object_part(&object),
        use_defs: object
            .get("UseDfs")
            .map(|_| object.get_int("UseDfs", 1) != 0),
        // A loaded Frame starts with no flags (frame.c:15179).
        flags: 0,
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
        fn title(&self) -> $crate::error::AstResult<String> {
            match &self.attrs.title {
                Some(t) => Ok(t.clone()),
                None => self.frame_default_title(),
            }
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
            // A bad Epoch is no Epoch (frame.c `TestEpoch` is `!= AST__BAD`).
            self.attrs.epoch = (mjd != $crate::constants::BAD_VALUE).then_some(mjd);
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

/// Ident, from `self.attrs.ident`. A blank Ident is set, as in C.
macro_rules! frame_attrs_ident {
    () => {
        fn ident(&self) -> &str {
            self.attrs.ident.as_deref().unwrap_or("")
        }
        fn set_ident(&mut self, ident: &str) -> $crate::error::AstResult<()> {
            self.attrs.ident = Some(ident.to_string());
            Ok(())
        }
        fn clear_ident(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.ident = None;
            Ok(())
        }
        fn test_ident(&self) -> bool {
            self.attrs.ident.is_some()
        }

        fn id(&self) -> &str {
            self.attrs.id.get()
        }
        fn set_id(&mut self, id: &str) -> $crate::error::AstResult<()> {
            self.attrs.id.set(id);
            Ok(())
        }
        fn clear_id(&mut self) -> $crate::error::AstResult<()> {
            self.attrs.id.clear();
            Ok(())
        }
        fn test_id(&self) -> bool {
            self.attrs.id.test()
        }
        fn stored_use_defs(&self) -> Option<bool> {
            self.attrs.use_defs
        }
        fn store_use_defs(&mut self, value: Option<bool>) {
            self.attrs.use_defs = value;
        }
    };
}

/// MinAxes, MaxAxes, MatchEnd, PreserveAxes and Permute from `self.attrs`.
/// MinAxes and MaxAxes default to the number of axes, MatchEnd and
/// PreserveAxes to false, and Permute to `self.frame_default_permute()`.
macro_rules! frame_attrs_match {
    () => {
        // An unset MinAxes defaults to Naxes, but no more than a set
        // MaxAxes, and an unset MaxAxes to Naxes, but no less than a set
        // MinAxes (frame.c `DefaultMinAxes`, `DefaultMaxAxes`).
        frame_attrs_match!(
            @body
            |s| s.attrs.max_axes.map_or(s.naxes(), |max| s.naxes().min(max)),
            |s| s.attrs.min_axes.map_or(s.naxes(), |min| s.naxes().max(min))
        );
        fn set_frame_flags(&mut self, flags: u32) {
            self.attrs.flags = flags;
        }
    };
    // Explicit MinAxes and MaxAxes defaults, for a class that over-rides
    // them (CmpFrame, cmpframe.c `GetMinAxes`, `GetMaxAxes`).
    (min = $min:expr, max = $max:expr) => {
        frame_attrs_match!(@body |_s| $min, |_s| $max);
    };
    (@body |$smin:ident| $min:expr, |$smax:ident| $max:expr) => {
        fn frame_flags(&self) -> u32 {
            self.attrs.flags
        }
        fn min_axes(&self) -> usize {
            self.attrs.min_axes.unwrap_or_else(|| {
                let $smin = self;
                $min
            })
        }
        fn set_min_axes(&mut self, n: usize) -> $crate::error::AstResult<()> {
            // A set MaxAxes below the new MinAxes is raised to it
            // (frame.c `ConsistentMinAxes`).
            if self.attrs.max_axes.is_some_and(|max| max < n) {
                self.attrs.max_axes = Some(n);
            }
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
            // A set MinAxes above the new MaxAxes is lowered to it
            // (frame.c `ConsistentMaxAxes`).
            if self.attrs.min_axes.is_some_and(|min| min > n) {
                self.attrs.min_axes = Some(n);
            }
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
    // A Region: getters read its FrameSet, while setters and clears go
    // through the Region, which remaps itself only when it is Adaptive
    // (region.c `SetAttrib`, `ClearAttrib`).
    (region $($path:ident).+) => {
        forward_frame_attrs!(@all_but_domain region [$($path).+]);
        fn domain(&self) -> &str {
            $crate::frames::Frame::domain(&self.$($path).+)
        }
        forward_frame_attrs!(@set region [$($path).+] set_domain(domain: &str));
        forward_frame_attrs!(@get [$($path).+] test_domain -> bool);
        forward_frame_attrs!(@clear region [$($path).+] clear_domain);
    };
    // Every attribute but Domain, for a class that supplies its own Domain
    // default.
    (@all_but_domain $($mode:ident)? [$($path:ident).+]) => {
        forward_frame_attrs!(@get [$($path).+] title -> $crate::error::AstResult<String>);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_title(title: &str));
        forward_frame_attrs!(@get [$($path).+] test_title -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_title);
        forward_frame_attrs!(@get [$($path).+] epoch_mjd -> f64);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_epoch_mjd(mjd: f64));
        forward_frame_attrs!(@get [$($path).+] test_epoch -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_epoch);
        forward_frame_attrs!(@get [$($path).+] digits -> i32);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_digits(value: i32));
        forward_frame_attrs!(@get [$($path).+] test_digits -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_digits);
        forward_frame_attrs!(@get [$($path).+] obs_lat -> f64);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_obs_lat(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_obs_lat -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_obs_lat);
        forward_frame_attrs!(@get [$($path).+] obs_lon -> f64);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_obs_lon(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_obs_lon -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_obs_lon);
        forward_frame_attrs!(@get [$($path).+] obs_alt -> f64);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_obs_alt(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_obs_alt -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_obs_alt);
        forward_frame_attrs!(@get [$($path).+] dtai -> f64);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_dtai(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_dtai -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_dtai);
        forward_frame_attrs!(@get [$($path).+] dut1 -> f64);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_dut1(v: f64));
        forward_frame_attrs!(@get [$($path).+] test_dut1 -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_dut1);
        forward_frame_attrs!(@get [$($path).+] min_axes -> usize);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_min_axes(n: usize));
        forward_frame_attrs!(@get [$($path).+] test_min_axes -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_min_axes);
        forward_frame_attrs!(@get [$($path).+] max_axes -> usize);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_max_axes(n: usize));
        forward_frame_attrs!(@get [$($path).+] test_max_axes -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_max_axes);
        forward_frame_attrs!(@get [$($path).+] match_end -> bool);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_match_end(v: bool));
        forward_frame_attrs!(@get [$($path).+] test_match_end -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_match_end);
        forward_frame_attrs!(@get [$($path).+] preserve_axes -> bool);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_preserve_axes(v: bool));
        forward_frame_attrs!(@get [$($path).+] test_preserve_axes -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_preserve_axes);
        forward_frame_attrs!(@get [$($path).+] permute -> bool);
        forward_frame_attrs!(@set $($mode)? [$($path).+] set_permute(v: bool));
        forward_frame_attrs!(@get [$($path).+] test_permute -> bool);
        forward_frame_attrs!(@clear $($mode)? [$($path).+] clear_permute);
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
    (@set region [$($path:ident).+] $name:ident($arg:ident: $ty:ty)) => {
        fn $name(&mut self, $arg: $ty) -> $crate::error::AstResult<()> {
            let apply = |f: &mut dyn $crate::frames::Frame, _: usize| f.$name($arg);
            $crate::regions::data::set_region_frame_attr(
                self,
                &$crate::regions::data::FrameSetting::new(None, &apply),
            )
        }
    };
    (@clear region [$($path:ident).+] $name:ident) => {
        fn $name(&mut self) -> $crate::error::AstResult<()> {
            let apply = |f: &mut dyn $crate::frames::Frame, _: usize| f.$name();
            $crate::regions::data::set_region_frame_attr(
                self,
                &$crate::regions::data::FrameSetting::clearing(None, &apply),
            )
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
        region_frame_attrs!(@common);
        fn use_defs(&self) -> bool {
            self.data
                .use_defs
                .unwrap_or_else(|| self.data.frameset.current().use_defs())
        }
    };
    // A class that finds an unset UseDefs elsewhere defines `use_defs`.
    (own_use_defs) => {
        region_frame_attrs!(@common);
    };
    (@common) => {
        forward_frame_attrs!(region data.frameset);
        forward_axis_attrs!(region data.frameset);
        // The Frame methods region.c overrides to act on the current Frame
        // of the Region's FrameSet (region.c:4700-4830).
        fn offset2(&self, point1: &[f64], angle: f64, offset: f64)
        -> $crate::error::AstResult<(Vec<f64>, f64)> {
            self.data.frameset.current().offset2(point1, angle, offset)
        }
        fn resolve(
            &self,
            point1: &[f64],
            point2: &[f64],
            point3: &[f64],
        ) -> $crate::error::AstResult<(Vec<f64>, f64, f64)> {
            self.data.frameset.current().resolve(point1, point2, point3)
        }
        fn resolve_points(
            &self,
            point1: &[f64],
            point2: &[f64],
            input: &$crate::coords::PointSet,
        ) -> $crate::error::AstResult<$crate::coords::PointSet> {
            self.data.frameset.current().resolve_points(point1, point2, input)
        }
        fn intersect(&self, a1: &[f64], a2: &[f64], b1: &[f64], b2: &[f64])
        -> $crate::error::AstResult<Vec<f64>> {
            self.data.frameset.current().intersect(a1, a2, b1, b2)
        }
        fn line_def(&self, start: &[f64], end: &[f64])
        -> $crate::error::AstResult<Option<$crate::frames::LineDef>> {
            self.data.frameset.current().line_def(start, end)
        }
        fn line_contains(&self, l: &$crate::frames::LineDef, point: &[f64])
        -> $crate::error::AstResult<bool> {
            self.data.frameset.current().line_contains(l, point)
        }
        fn line_crossing(&self, l1: &$crate::frames::LineDef, l2: &$crate::frames::LineDef)
        -> $crate::error::AstResult<(bool, [f64; 2], f64)> {
            self.data.frameset.current().line_crossing(l1, l2)
        }
        fn line_offset(&self, line: &$crate::frames::LineDef, par: f64, prp: f64)
        -> $crate::error::AstResult<[f64; 2]> {
            self.data.frameset.current().line_offset(line, par, prp)
        }
        fn norm_box(
            &self,
            lbnd: &mut [f64],
            ubnd: &mut [f64],
            inside: &dyn Fn(&[f64]) -> $crate::error::AstResult<bool>,
        ) -> $crate::error::AstResult<()> {
            self.data.frameset.current().norm_box(lbnd, ubnd, inside)
        }
        fn sub_frame(
            &self,
            template: Option<&dyn $crate::frames::Frame>,
            result_naxes: usize,
            target_axes: &[i32],
            template_axes: &[i32],
        ) -> $crate::error::AstResult<
            Option<($crate::mappings::DynMapping, $crate::frames::DynFrame)>,
        > {
            self.data
                .frameset
                .current()
                .sub_frame(template, result_naxes, target_axes, template_axes)
        }
        fn system_string(&self) -> $crate::error::AstResult<Option<&str>> {
            self.data.frameset.current().system_string()
        }
        fn ax_angle(&self, a: &[f64], b: &[f64], axis: usize) -> $crate::error::AstResult<f64> {
            self.validate_frame_axis(axis, "astAxAngle")?;
            self.data.frameset.current().ax_angle(a, b, axis)
        }
        fn ax_distance(&self, axis: usize, v1: f64, v2: f64) -> $crate::error::AstResult<f64> {
            self.validate_frame_axis(axis, "astAxDistance")?;
            self.data.frameset.current().ax_distance(axis, v1, v2)
        }
        fn ax_norm(&self, axis: usize, oper: i32, values: &mut [f64])
        -> $crate::error::AstResult<()> {
            self.validate_frame_axis(axis, "astAxNorm")?;
            self.data.frameset.current().ax_norm(axis, oper, values)
        }
        fn ax_offset(&self, axis: usize, v1: f64, dist: f64) -> $crate::error::AstResult<f64> {
            self.validate_frame_axis(axis, "astAxOffset")?;
            self.data.frameset.current().ax_offset(axis, v1, dist)
        }
        fn validate_frame_axis(&self, axis: usize, method: &str) -> $crate::error::AstResult<()> {
            self.data.frameset.current().validate_frame_axis(axis, method)
        }
        // region.c `IsUnitFrame`: a Region is never equivalent to a UnitMap.
        fn is_unit_frame(&self) -> bool {
            false
        }
        // region.c `PermAxes`: the FrameSet's axes are permuted.
        fn perm_axes(&mut self, perm: &[usize]) -> $crate::error::AstResult<()> {
            self.data.perm_axes(perm)
        }
        // region.c `PickAxes`: a Region of the picked axes where the Region's
        // class can form one, otherwise the picked current-Frame axes.
        fn pick_axes_snapshot(&self, target_axes: &[i32])
        -> $crate::error::AstResult<$crate::frames::DynFrame> {
            if target_axes.iter().any(|&a| a < 0) {
                return self.data.frameset.current().pick_axes_snapshot(target_axes);
            }
            let axes: Vec<usize> = target_axes.iter().map(|&a| a as usize).collect();
            Ok(match $crate::regions::pick_axes_region(self, &axes)? {
                $crate::regions::PickedAxes::Region(region) => region,
                $crate::regions::PickedAxes::Frame(frame) => frame,
            })
        }
        // region.c `NormalPoints` (6591): the current Frame of the Region's
        // FrameSet normalises the points.
        fn norm_points(
            &self,
            points: &$crate::coords::PointSet,
            contig: bool,
        ) -> $crate::error::AstResult<$crate::coords::PointSet> {
            self.data.frameset.current().norm_points(points, contig)
        }
        // region.c `Overlay` (7463): the current Frame of the Region's
        // FrameSet is the template.
        fn overlay(
            &self,
            template_axes: &[i32],
            result: &mut dyn $crate::frames::Frame,
        ) -> $crate::error::AstResult<()> {
            self.data.frameset.overlay(template_axes, result)
        }
        fn axis_overlay_source(
            &self,
            axis: usize,
        ) -> Option<$crate::frames::overlay::AxisOverlaySource> {
            self.data.frameset.axis_overlay_source(axis)
        }
        fn apply_axis_overlay(
            &mut self,
            axis: usize,
            source: &$crate::frames::overlay::AxisOverlaySource,
        ) {
            self.data.frameset.apply_axis_overlay(axis, source);
        }
        fn overlay_system_from(
            &mut self,
            template: &dyn std::any::Any,
        ) -> $crate::error::AstResult<()> {
            self.data.frameset.overlay_system_from(template)
        }
        fn ident(&self) -> &str {
            self.data.ident.as_deref().unwrap_or("")
        }
        fn set_ident(&mut self, ident: &str) -> $crate::error::AstResult<()> {
            self.data.ident = Some(ident.to_string());
            Ok(())
        }
        fn clear_ident(&mut self) -> $crate::error::AstResult<()> {
            self.data.ident = None;
            Ok(())
        }
        fn test_ident(&self) -> bool {
            self.data.ident.is_some()
        }

        fn id(&self) -> &str {
            self.data.id.get()
        }
        fn set_id(&mut self, id: &str) -> $crate::error::AstResult<()> {
            self.data.id.set(id);
            Ok(())
        }
        fn clear_id(&mut self) -> $crate::error::AstResult<()> {
            self.data.id.clear();
            Ok(())
        }
        fn test_id(&self) -> bool {
            self.data.id.test()
        }
        // UseDefs, as C's attribute access gives it: setting, clearing and
        // testing it act on the encapsulated Frame, and an unset value comes
        // from it (region.c `GetUseDefs`). A value stored on the Region
        // itself, which only a dump can supply, takes precedence.
        fn stored_use_defs(&self) -> Option<bool> {
            self.data
                .use_defs
                .or_else(|| self.data.frameset.current().stored_use_defs())
        }
        fn store_use_defs(&mut self, value: Option<bool>) {
            self.data.use_defs = value;
        }
        fn set_use_defs(&mut self, value: bool) -> $crate::error::AstResult<()> {
            self.data
                .frameset
                .frame_mut($crate::frames::FrameRef::Current)?
                .set_use_defs(value)
        }
        fn clear_use_defs(&mut self) -> $crate::error::AstResult<()> {
            self.data
                .frameset
                .frame_mut($crate::frames::FrameRef::Current)?
                .clear_use_defs()
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
            self.data.ident = Some(ident.to_string());
        }
        fn clear_ident(&mut self) {
            self.data.ident = None;
        }
        fn test_ident(&self) -> bool {
            self.data.ident.is_some()
        }

        fn id(&self) -> &str {
            self.data.id.get()
        }
        fn set_id(&mut self, id: &str) {
            self.data.id.set(id);
        }
        fn clear_id(&mut self) {
            self.data.id.clear();
        }
        fn test_id(&self) -> bool {
            self.data.id.test()
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

/// Check a 1-based axis index against a Frame's axis count, as C's
/// `astValidateAxis` does.
pub(crate) fn validate_axis(axis: usize, naxes: usize) -> AstResult<()> {
    if axis == 0 || axis > naxes {
        return Err(AstError::new_tracked(
            ErrorCode::Axiin,
            format!("axis index {axis} is out of range 1..={naxes}"),
        ));
    }
    Ok(())
}

/// C's `NewUnit` (frame.c:7583), run when an axis Unit changes from
/// `old_units` to `new_units` with ActiveUnit set: a set Label and Symbol
/// are rewritten for the new Unit through `astUnitMapper` (so that `Dist`
/// becomes `log( Dist )` for a change from `m` to `log(m)`), and a set
/// Format is cleared when the Unit actually changes.
pub(crate) fn new_unit(axis: &mut super::axis::Axis, old_units: &str, new_units: &str) {
    for text in [&mut axis.label, &mut axis.symbol] {
        if let Some(current) = text.as_deref() {
            if let Some((_, Some(rewritten))) =
                crate::units::unit_mapper_label(old_units, new_units, current)
            {
                *text = Some(rewritten);
            }
        }
    }
    if old_units != new_units {
        axis.format = None;
    }
}

/// A default Label or Symbol rewritten for a Unit that has been set, as the
/// `GetLabel` and `GetSymbol` of SpecFrame, TimeFrame and FluxFrame do
/// (specframe.c:1520, timeframe.c:2579, fluxframe.c:1107): through the
/// label `astUnitMapper` returns for a change from the System's default
/// units, so that a Unit of `"log(Hz)"` makes `"Frequency"` into
/// `"log( Frequency )"`. Unchanged when no Unit is set or no label results.
pub(crate) fn relabel_for_unit(label: String, default_unit: &str, unit: Option<&str>) -> String {
    let Some(unit) = unit else {
        return label;
    };
    match crate::units::unit_mapper_label(default_unit, unit, &label) {
        Some((_, Some(new_label))) => new_label,
        _ => label,
    }
}

/// Set, test and clear for the named per-axis attributes, as C's Frame
/// does them through `astGetAxis` (frame.c `MAKE_SET`, `MAKE_TEST`,
/// `MAKE_CLEAR`). Used inside `impl Frame for X`, where `X` has inherent
/// `ax(axis)` and `ax_mut(axis)` returning the `Axis` of a 1-based external
/// axis. Each name is one of `label`, `symbol`, `unit`, `format`, `digits`,
/// `direction`, `top` and `bottom`.
macro_rules! axis_attrs_store {
    ($($attr:ident),+ $(,)?) => {
        $(axis_attrs_store!(@one $attr);)+
    };
    (@one label) => {
        axis_attrs_store!(@str set_label, test_label, clear_label, label);
    };
    (@one symbol) => {
        axis_attrs_store!(@str set_symbol, test_symbol, clear_symbol, symbol);
    };
    (@one unit) => {
        // frame.c `SetUnit`: trailing spaces are dropped, and with
        // ActiveUnit set the change of Unit is applied to the Label,
        // Symbol and Format (`NewUnit`).
        fn set_unit(&mut self, axis: usize, value: &str) -> $crate::error::AstResult<()> {
            let value = value.trim_end();
            if $crate::frames::Frame::get_active_unit(self) {
                let old = $crate::frames::Frame::unit(self, axis)?;
                $crate::frames::frame_attrs::new_unit(self.ax_mut(axis)?, &old, value);
            }
            self.ax_mut(axis)?.unit = Some(value.to_string());
            Ok(())
        }
        fn test_unit(&self, axis: usize) -> $crate::error::AstResult<bool> {
            Ok(self.ax(axis)?.unit.is_some())
        }
        // frame.c `ClearUnit`: the change from the set Unit to the default
        // is applied as `SetUnit` applies one.
        fn clear_unit(&mut self, axis: usize) -> $crate::error::AstResult<()> {
            let Some(old) = self.ax_mut(axis)?.unit.take() else {
                return Ok(());
            };
            if $crate::frames::Frame::get_active_unit(self) {
                let new = $crate::frames::Frame::unit(self, axis)?;
                $crate::frames::frame_attrs::new_unit(self.ax_mut(axis)?, &old, &new);
            }
            Ok(())
        }
    };
    (@one format) => {
        axis_attrs_store!(@str set_format_str, test_format_str, clear_format_str, format);
    };
    (@one digits) => {
        // C constrains an axis's Digits to at least 1 (axis.c:2617).
        fn set_axis_digits(&mut self, axis: usize, value: i32) -> $crate::error::AstResult<()> {
            self.ax_mut(axis)?.digits = Some(value.max(1));
            Ok(())
        }
        axis_attrs_store!(@test_clear test_axis_digits, clear_axis_digits, digits);
    };
    (@one direction) => {
        axis_attrs_store!(@val set_direction, test_direction, clear_direction, direction, bool);
    };
    (@one top) => {
        axis_attrs_store!(@bad_unset set_top, test_top, clear_top, top);
    };
    (@one bottom) => {
        axis_attrs_store!(@bad_unset set_bottom, test_bottom, clear_bottom, bottom);
    };
    (@str $set:ident, $test:ident, $clear:ident, $field:ident) => {
        fn $set(&mut self, axis: usize, value: &str) -> $crate::error::AstResult<()> {
            self.ax_mut(axis)?.$field = Some(value.to_string());
            Ok(())
        }
        axis_attrs_store!(@test_clear $test, $clear, $field);
    };
    (@val $set:ident, $test:ident, $clear:ident, $field:ident, $ty:ty) => {
        fn $set(&mut self, axis: usize, value: $ty) -> $crate::error::AstResult<()> {
            self.ax_mut(axis)?.$field = Some(value);
            Ok(())
        }
        axis_attrs_store!(@test_clear $test, $clear, $field);
    };
    // An attribute whose C TEST is `!= AST__BAD` (axis.c Top, Bottom), so
    // that setting the bad value clears it.
    (@bad_unset $set:ident, $test:ident, $clear:ident, $field:ident) => {
        fn $set(&mut self, axis: usize, value: f64) -> $crate::error::AstResult<()> {
            self.ax_mut(axis)?.$field = (value != $crate::constants::BAD_VALUE).then_some(value);
            Ok(())
        }
        axis_attrs_store!(@test_clear $test, $clear, $field);
    };
    (@test_clear $test:ident, $clear:ident, $field:ident) => {
        fn $test(&self, axis: usize) -> $crate::error::AstResult<bool> {
            Ok(self.ax(axis)?.$field.is_some())
        }
        fn $clear(&mut self, axis: usize) -> $crate::error::AstResult<()> {
            self.ax_mut(axis)?.$field = None;
            Ok(())
        }
    };
}

/// The Frame class's getters for the named per-axis attributes, for a class
/// whose C counterpart does not over-ride them (frame.c `MAKE_GET`), with the
/// Frame's Digits standing in for an axis that has none. Same storage
/// contract as [`axis_attrs_store`]. Each name is one of `unit`, `format`,
/// `digits`, `direction`, `top` and `bottom`.
macro_rules! axis_attrs_base_get {
    ($($attr:ident),+ $(,)?) => {
        $(axis_attrs_base_get!(@one $attr);)+
    };
    (@one unit) => {
        fn unit(&self, axis: usize) -> $crate::error::AstResult<String> {
            Ok(self.ax(axis)?.unit.clone().unwrap_or_default())
        }
    };
    (@one format) => {
        // axis.c `GetDefaultFormat`: "%1.<Digits>G".
        fn format_str(&self, axis: usize) -> $crate::error::AstResult<String> {
            match &self.ax(axis)?.format {
                Some(f) => Ok(f.clone()),
                None => Ok(format!(
                    "%1.{}G",
                    $crate::frames::Frame::axis_digits(self, axis)?
                )),
            }
        }
    };
    (@one digits) => {
        fn axis_digits(&self, axis: usize) -> $crate::error::AstResult<i32> {
            Ok(self
                .ax(axis)?
                .digits
                .unwrap_or_else(|| $crate::frames::Frame::digits(self)))
        }
    };
    (@one direction) => {
        fn direction(&self, axis: usize) -> $crate::error::AstResult<bool> {
            Ok(self.ax(axis)?.direction.unwrap_or(true))
        }
    };
    (@one top) => {
        fn top(&self, axis: usize) -> $crate::error::AstResult<f64> {
            Ok(self.ax(axis)?.top.unwrap_or(f64::MAX))
        }
    };
    (@one bottom) => {
        fn bottom(&self, axis: usize) -> $crate::error::AstResult<f64> {
            Ok(self.ax(axis)?.bottom.unwrap_or(-f64::MAX))
        }
    };
}

/// Forward every per-axis attribute to `self.<path>`, itself a Frame.
macro_rules! forward_axis_attrs {
    ($($path:ident).+) => {
        forward_axis_attrs!(@get [$($path).+] label -> String);
        forward_axis_attrs!(@rest [$($path).+]);
    };
    // A Region: getters read its FrameSet, while setters and clears go
    // through the Region, which remaps itself only when it is Adaptive.
    (region $($path:ident).+) => {
        forward_axis_attrs!(@get [$($path).+] label -> String);
        forward_axis_attrs!(@rest region [$($path).+]);
    };
    // Every attribute but the Label getter, for a class that supplies its
    // own Label default.
    (@all_but_label [$($path:ident).+]) => {
        forward_axis_attrs!(@rest [$($path).+]);
    };
    (@rest $($mode:ident)? [$($path:ident).+]) => {
        forward_axis_attrs!(@get [$($path).+] axis_object_top -> f64);
        forward_axis_attrs!(@get [$($path).+] axis_object_bottom -> f64);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_label(&str));
        forward_axis_attrs!(@get [$($path).+] test_label -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_label);
        forward_axis_attrs!(@get [$($path).+] symbol -> String);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_symbol(&str));
        forward_axis_attrs!(@get [$($path).+] test_symbol -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_symbol);
        forward_axis_attrs!(@get [$($path).+] unit -> String);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_unit(&str));
        forward_axis_attrs!(@get [$($path).+] test_unit -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_unit);
        forward_axis_attrs!(@get [$($path).+] format_str -> String);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_format_str(&str));
        forward_axis_attrs!(@get [$($path).+] test_format_str -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_format_str);
        forward_axis_attrs!(@get [$($path).+] axis_digits -> i32);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_axis_digits(i32));
        forward_axis_attrs!(@get [$($path).+] test_axis_digits -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_axis_digits);
        forward_axis_attrs!(@get [$($path).+] direction -> bool);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_direction(bool));
        forward_axis_attrs!(@get [$($path).+] test_direction -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_direction);
        forward_axis_attrs!(@get [$($path).+] top -> f64);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_top(f64));
        forward_axis_attrs!(@get [$($path).+] test_top -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_top);
        forward_axis_attrs!(@get [$($path).+] bottom -> f64);
        forward_axis_attrs!(@set $($mode)? [$($path).+] set_bottom(f64));
        forward_axis_attrs!(@get [$($path).+] test_bottom -> bool);
        forward_axis_attrs!(@clear $($mode)? [$($path).+] clear_bottom);
    };
    (@get [$($path:ident).+] $name:ident -> $ty:ty) => {
        fn $name(&self, axis: usize) -> $crate::error::AstResult<$ty> {
            $crate::frames::Frame::$name(&self.$($path).+, axis)
        }
    };
    (@set region [$($path:ident).+] $name:ident($ty:ty)) => {
        fn $name(&mut self, axis: usize, value: $ty) -> $crate::error::AstResult<()> {
            let apply = |f: &mut dyn $crate::frames::Frame, a: usize| f.$name(a, value);
            $crate::regions::data::set_region_frame_attr(
                self,
                &$crate::regions::data::FrameSetting::new(Some(axis), &apply),
            )
        }
    };
    (@clear region [$($path:ident).+] $name:ident) => {
        fn $name(&mut self, axis: usize) -> $crate::error::AstResult<()> {
            let apply = |f: &mut dyn $crate::frames::Frame, a: usize| f.$name(a);
            $crate::regions::data::set_region_frame_attr(
                self,
                &$crate::regions::data::FrameSetting::clearing(Some(axis), &apply),
            )
        }
    };
    (@set [$($path:ident).+] $name:ident($ty:ty)) => {
        fn $name(&mut self, axis: usize, value: $ty) -> $crate::error::AstResult<()> {
            $crate::frames::Frame::$name(&mut self.$($path).+, axis, value)
        }
    };
    (@clear [$($path:ident).+] $name:ident) => {
        fn $name(&mut self, axis: usize) -> $crate::error::AstResult<()> {
            $crate::frames::Frame::$name(&mut self.$($path).+, axis)
        }
    };
}

/// Call `get` on the component Frame `f` of a compound Frame whose Digits
/// is `digits`, lending it that Digits when it has none of its own, as the
/// getters in cmpframe.c `MAKE_GET` do by setting and then clearing it.
pub(crate) fn with_lent_digits<R>(
    digits: i32,
    f: &dyn super::Frame,
    get: impl FnOnce(&dyn super::Frame) -> AstResult<R>,
) -> AstResult<R> {
    if f.test_digits() || f.digits() == digits {
        return get(f);
    }
    let mut lent = f.snapshot();
    lent.set_digits(digits)?;
    get(lent.as_ref())
}

/// The per-axis attributes of a CmpFrame, and of a SpecFluxFrame, which is
/// one in C, as cmpframe.c's `MAKE_GET`, `MAKE_SET`, `MAKE_TEST` and
/// `MAKE_CLEAR` do them: each is routed to the component that holds the
/// axis, a getter lends the CmpFrame's Digits to a component that has none,
/// and a plain Frame component's default Label and Symbol are numbered by
/// the CmpFrame's axis. Top and Bottom, which C reaches through
/// `astGetAxis` and so through the component's own Axis, come from the
/// component's Axis object, and Digits(axis), which C reads through the
/// attribute string of the component (cmpframe.c `GetAttrib`), from the
/// component.
///
/// Used inside `impl Frame for X`, where `X` has inherent
/// `axis_component(axis)` and `axis_component_mut(axis)` returning the
/// component and its axis for a 1-based external axis.
macro_rules! cmp_frame_axis_attrs {
    () => {
        fn label(&self, axis: usize) -> $crate::error::AstResult<String> {
            let (f, a) = self.axis_component(axis)?;
            if f.as_any().is::<$crate::frames::BasicFrame>() && !f.test_label(a)? {
                return Ok(format!("Axis {axis}"));
            }
            $crate::frames::frame_attrs::with_lent_digits(
                $crate::frames::Frame::digits(self),
                f,
                |f| f.label(a),
            )
        }
        fn symbol(&self, axis: usize) -> $crate::error::AstResult<String> {
            let (f, a) = self.axis_component(axis)?;
            if f.as_any().is::<$crate::frames::BasicFrame>() && !f.test_symbol(a)? {
                return Ok(format!("x{axis}"));
            }
            $crate::frames::frame_attrs::with_lent_digits(
                $crate::frames::Frame::digits(self),
                f,
                |f| f.symbol(a),
            )
        }
        cmp_frame_axis_attrs!(@lent unit -> String);
        cmp_frame_axis_attrs!(@lent format_str -> String);
        cmp_frame_axis_attrs!(@lent direction -> bool);
        cmp_frame_axis_attrs!(@get axis_digits -> i32);
        fn top(&self, axis: usize) -> $crate::error::AstResult<f64> {
            let (f, a) = self.axis_component(axis)?;
            f.axis_object_top(a)
        }
        fn bottom(&self, axis: usize) -> $crate::error::AstResult<f64> {
            let (f, a) = self.axis_component(axis)?;
            f.axis_object_bottom(a)
        }
        cmp_frame_axis_attrs!(@get axis_object_top -> f64);
        cmp_frame_axis_attrs!(@get axis_object_bottom -> f64);
        cmp_frame_axis_attrs!(@rest set_label(&str), test_label, clear_label);
        cmp_frame_axis_attrs!(@rest set_symbol(&str), test_symbol, clear_symbol);
        cmp_frame_axis_attrs!(@rest set_unit(&str), test_unit, clear_unit);
        cmp_frame_axis_attrs!(@rest set_format_str(&str), test_format_str, clear_format_str);
        cmp_frame_axis_attrs!(@rest set_axis_digits(i32), test_axis_digits, clear_axis_digits);
        cmp_frame_axis_attrs!(@rest set_direction(bool), test_direction, clear_direction);
        cmp_frame_axis_attrs!(@rest set_top(f64), test_top, clear_top);
        cmp_frame_axis_attrs!(@rest set_bottom(f64), test_bottom, clear_bottom);
    };
    (@lent $name:ident -> $ty:ty) => {
        fn $name(&self, axis: usize) -> $crate::error::AstResult<$ty> {
            let (f, a) = self.axis_component(axis)?;
            $crate::frames::frame_attrs::with_lent_digits(
                $crate::frames::Frame::digits(self),
                f,
                |f| f.$name(a),
            )
        }
    };
    (@get $name:ident -> $ty:ty) => {
        fn $name(&self, axis: usize) -> $crate::error::AstResult<$ty> {
            let (f, a) = self.axis_component(axis)?;
            f.$name(a)
        }
    };
    (@rest $set:ident($ty:ty), $test:ident, $clear:ident) => {
        fn $set(&mut self, axis: usize, value: $ty) -> $crate::error::AstResult<()> {
            let (f, a) = self.axis_component_mut(axis)?;
            f.$set(a, value)
        }
        fn $test(&self, axis: usize) -> $crate::error::AstResult<bool> {
            let (f, a) = self.axis_component(axis)?;
            f.$test(a)
        }
        fn $clear(&mut self, axis: usize) -> $crate::error::AstResult<()> {
            let (f, a) = self.axis_component_mut(axis)?;
            f.$clear(a)
        }
    };
}

/// The Frame part of a subclass's dump, read through the `Frame` trait as
/// C's `Frame::Dump` reads it through the virtual getters, so that each
/// class's own defaults appear (frame.c:14250). The class supplies its
/// stored Frame attributes, whether its System and AlignSystem are set and
/// their names, and the permutation.
pub(crate) struct TraitDump<'a> {
    /// The Frame being dumped.
    pub frame: &'a dyn super::Frame,
    /// Its stored Frame attributes.
    pub attrs: &'a FrameAttrs,
    /// Whether System is set, and its name.
    pub system: (bool, String),
    /// Whether AlignSystem is set, and its name.
    pub align_system: (bool, String),
    /// The permutation, `perm[external] = internal`.
    pub perm: Vec<usize>,
    /// Whether the Frame counts as a plain Frame in C's `isFrame` sense
    /// (frame.c Dump): true for a CmpFrame.
    pub is_frame: bool,
    /// C's `this->naxes`: the axes the Frame class's own structure holds,
    /// none for a CmpFrame.
    pub own_naxes: usize,
}

impl TraitDump<'_> {
    /// The 1-based external axis of an internal one.
    fn ext(&self, internal: usize) -> usize {
        self.perm
            .iter()
            .position(|&p| p == internal)
            .unwrap_or(internal)
            + 1
    }
}

impl FrameDumpSource for TraitDump<'_> {
    fn attrs(&self) -> &FrameAttrs {
        self.attrs
    }
    fn dump_naxes(&self) -> usize {
        self.frame.naxes()
    }
    fn dump_perm(&self) -> Vec<usize> {
        self.perm.clone()
    }
    fn dump_is_frame(&self) -> bool {
        self.is_frame
    }
    fn dump_own_naxes(&self) -> usize {
        self.own_naxes
    }
    fn dump_title(&self) -> AstResult<String> {
        self.frame.title()
    }
    fn dump_domain(&self) -> String {
        self.frame.domain().to_string()
    }
    fn dump_epoch(&self) -> f64 {
        self.frame.epoch_mjd()
    }
    fn dump_system(&self) -> (bool, String) {
        self.system.clone()
    }
    fn dump_align_system(&self) -> (bool, String) {
        self.align_system.clone()
    }
    fn dump_label(&self, axis: usize) -> String {
        self.frame.label(self.ext(axis)).unwrap_or_default()
    }
    fn dump_symbol(&self, axis: usize) -> String {
        self.frame.symbol(self.ext(axis)).unwrap_or_default()
    }
    fn dump_unit(&self, axis: usize) -> String {
        self.frame.unit(self.ext(axis)).unwrap_or_default()
    }
    fn dump_format(&self, axis: usize) -> String {
        self.frame.format_str(self.ext(axis)).unwrap_or_default()
    }
    fn dump_axis_digits(&self, axis: usize) -> Option<i32> {
        let ext = self.ext(axis);
        if self.frame.test_axis_digits(ext).unwrap_or(false) {
            self.frame.axis_digits(ext).ok()
        } else {
            None
        }
    }
    fn dump_direction(&self, axis: usize) -> bool {
        self.frame.direction(self.ext(axis)).unwrap_or(true)
    }
    fn dump_bottom(&self, axis: usize) -> f64 {
        self.frame.bottom(self.ext(axis)).unwrap_or(-f64::MAX)
    }
    fn dump_top(&self, axis: usize) -> f64 {
        self.frame.top(self.ext(axis)).unwrap_or(f64::MAX)
    }
    fn dump_preserve_axes(&self) -> bool {
        self.frame.preserve_axes()
    }
    fn dump_permute(&self) -> bool {
        self.frame.permute()
    }
    fn dump_min_axes(&self) -> usize {
        self.frame.min_axes()
    }
    fn dump_max_axes(&self) -> usize {
        self.frame.max_axes()
    }
    fn dump_match_end(&self) -> bool {
        self.frame.match_end()
    }
    // frame.c:14729 asks the class's own astTestActiveUnit and
    // astGetActiveUnit, so a class that over-rides them, as SkyFrame and
    // TimeFrame do, writes what they report.
    fn dump_active_unit(&self) -> Option<bool> {
        self.frame
            .test_active_unit()
            .then(|| self.frame.get_active_unit())
    }
}
