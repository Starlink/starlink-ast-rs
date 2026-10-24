//! The attributes C's Axis class stores for one axis of a Frame (axis.c):
//! Label, Symbol, Unit, Digits, Format, Direction, Top and Bottom.
//!
//! Each is `None` until set. The defaults are supplied by the Frame that owns
//! the axis, because C's Frame methods substitute their own (`"Axis 1"` for a
//! Label, the Frame's Digits for an axis without its own) rather than the
//! Axis class's.

use crate::serial::Node;

/// The Digits of an Axis object that has none set (axis.c `GetAxisDigits`).
pub(crate) const AXIS_DIGITS: i32 = 7;

/// The values an Axis object's `astGetAxis<attribute>` methods give for the
/// attributes that are not set, which its class decides.
pub(crate) struct AxisValues {
    pub label: String,
    pub symbol: String,
    pub unit: String,
    pub format: String,
    pub direction: bool,
    pub top: f64,
    pub bottom: f64,
}

/// Axis-class attributes of one axis. Mirrors the instance variables of C's
/// `AstAxis` (axis.h).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Axis {
    pub label: Option<String>,
    pub symbol: Option<String>,
    pub unit: Option<String>,
    pub digits: Option<i32>,
    pub format: Option<String>,
    pub direction: Option<bool>,
    pub top: Option<f64>,
    pub bottom: Option<f64>,
}

impl Axis {
    /// The Object and Axis parts of the Axis object's dump (object.c and
    /// axis.c `Dump`), each value active when set and otherwise the one
    /// `values` gives, as the class's `astGetAxis<attribute>` returns it.
    /// Label and Symbol are helpful; the rest are not. The Frame that owns
    /// the axis writes it with Full -1 unless Full is 1, so at Full 0 only
    /// the set values appear (frame.c `Dump`).
    pub(crate) fn write_parts(&self, node: &mut Node, values: &AxisValues) {
        crate::frames::frame_attrs::write_object_part(
            node,
            &crate::object_id::ObjectId::default(),
            None,
            None,
            true,
        );
        node.write_string(
            "Label",
            self.label.is_some(),
            true,
            self.label.as_deref().unwrap_or(&values.label),
            "Axis Label",
        );
        node.write_string(
            "Symbol",
            self.symbol.is_some(),
            true,
            self.symbol.as_deref().unwrap_or(&values.symbol),
            "Axis symbol",
        );
        let unit = self.unit.as_deref().unwrap_or(&values.unit);
        // C appends the unit's full name only when it differs from the unit
        // string (axis.c, `lab && strcmp( lab, sval )`).
        let comment = match crate::units::unit_label(unit) {
            Some(lab) if lab != unit => format!("Axis units ({lab})"),
            _ => "Axis units".to_string(),
        };
        node.write_string("Unit", self.unit.is_some(), false, unit, &comment);
        node.write_int(
            "Digits",
            self.digits.is_some(),
            false,
            i64::from(self.digits.unwrap_or(AXIS_DIGITS)),
            "Default formatting precision",
        );
        node.write_string(
            "Format",
            self.format.is_some(),
            false,
            self.format.as_deref().unwrap_or(&values.format),
            "Format specifier",
        );
        let dir = self.direction.unwrap_or(values.direction);
        node.write_int(
            "Dirn",
            self.direction.is_some(),
            false,
            i64::from(dir),
            if dir {
                "Plot in conventional direction (hint)"
            } else {
                "Plot in reverse direction (hint)"
            },
        );
        node.write_double(
            "Top",
            self.top.is_some(),
            false,
            self.top.unwrap_or(values.top),
            "Maximum legal axis value",
        );
        node.write_double(
            "Bottom",
            self.bottom.is_some(),
            false,
            self.bottom.unwrap_or(values.bottom),
            "Minimum legal axis value",
        );
    }

    /// The dump of a plain Axis object, whose unset values are the Axis
    /// class's defaults.
    pub(crate) fn to_dump_node(&self) -> Node {
        let mut node = Node::new("Axis");
        node.class_comment = Some("Coordinate axis".to_string());
        let digits = self.digits.unwrap_or(AXIS_DIGITS);
        self.write_parts(
            &mut node,
            &AxisValues {
                label: "Coordinate axis".to_string(),
                symbol: "x".to_string(),
                unit: String::new(),
                format: format!("%1.{digits}G"),
                direction: true,
                top: f64::MAX,
                bottom: -f64::MAX,
            },
        );
        node
    }

    /// Read the attributes present in an Axis node. Mirrors C's
    /// `astLoadAxis` (axis.c), which reads each with its unset default.
    pub(crate) fn read(whole: &Node) -> Self {
        // The Axis class's part of the description (astReadClassData).
        let node = &whole.section("Axis");
        Axis {
            label: node.get_string_if_set("Label"),
            symbol: node.get_string_if_set("Symbol"),
            unit: node.get_string_if_set("Unit"),
            digits: node.get("Digits").map(|_| node.get_int("Digits", 7) as i32),
            format: node.get_string_if_set("Format"),
            direction: node.get("Dirn").map(|_| node.get_int("Dirn", 1) != 0),
            top: node.get("Top").map(|_| node.get_double("Top", f64::MAX)),
            bottom: node
                .get("Bottom")
                .map(|_| node.get_double("Bottom", -f64::MAX)),
        }
    }

    /// Apply the per-axis items of a Frame dump for axis `n` (1-based),
    /// `Lbl<n>`, `Sym<n>`, `Uni<n>`, `Dir<n>`, `Top<n>`, `Bot<n>` and
    /// `Dig<n>`, through the Axis setters, as `astLoadFrame` does after
    /// reading the Axis object (frame.c:15226). Dump writes these commented
    /// out, so only a hand-written description sets them. `Fmt<n>` is
    /// returned rather than applied, since a SkyAxis keeps its own Format.
    pub(crate) fn apply_frame_axis_keys(&mut self, node: &Node, n: usize) -> Option<String> {
        if let Some(v) = node.get_string_if_set(&format!("Lbl{n}")) {
            self.label = Some(v);
        }
        if let Some(v) = node.get_string_if_set(&format!("Sym{n}")) {
            self.symbol = Some(v);
        }
        if let Some(v) = node.get_string_if_set(&format!("Uni{n}")) {
            self.unit = Some(v);
        }
        let key = format!("Dir{n}");
        if node.get(&key).is_some() {
            self.direction = Some(node.get_int(&key, 1) != 0);
        }
        let key = format!("Top{n}");
        if node.get(&key).is_some() {
            self.top = Some(node.get_double(&key, f64::MAX));
        }
        let key = format!("Bot{n}");
        if node.get(&key).is_some() {
            self.bottom = Some(node.get_double(&key, -f64::MAX));
        }
        // SetAxisDigits keeps at least one digit (axis.c:2617).
        let key = format!("Dig{n}");
        if node.get(&key).is_some() {
            self.digits = Some((node.get_int(&key, 7) as i32).max(1));
        }
        node.get_string_if_set(&format!("Fmt{n}"))
    }

    /// Copy the attributes set on `template` onto this axis. Mirrors C's
    /// `Axis::AxisOverlay` (axis.c:1226).
    pub(crate) fn overlay_from(&mut self, template: &Axis) {
        if template.digits.is_some() {
            self.digits = template.digits;
        }
        if template.direction.is_some() {
            self.direction = template.direction;
        }
        if template.label.is_some() {
            self.label.clone_from(&template.label);
        }
        if template.symbol.is_some() {
            self.symbol.clone_from(&template.symbol);
        }
        if template.unit.is_some() {
            self.unit.clone_from(&template.unit);
        }
        if template.format.is_some() {
            self.format.clone_from(&template.format);
        }
    }
}

/// The Axis object on one axis of a plain Frame: a plain Axis, or a SkyAxis
/// that SubFrame or a loaded dump attached (frame.c:11023), as C's Frame
/// holds Axis objects of either class and dispatches to them through the
/// Axis vtab.
#[derive(Debug, Clone)]
pub(crate) enum FrameAxis {
    /// A plain Axis (axis.c).
    Plain(Axis),
    /// A SkyAxis (skyaxis.c).
    Sky(super::skyaxis::SkyAxis),
}

impl Default for FrameAxis {
    fn default() -> Self {
        FrameAxis::Plain(Axis::default())
    }
}

impl FrameAxis {
    /// The Axis-class attributes.
    pub(crate) fn axis(&self) -> &Axis {
        match self {
            FrameAxis::Plain(a) => a,
            FrameAxis::Sky(s) => &s.axis,
        }
    }

    /// The Axis-class attributes, to change.
    pub(crate) fn axis_mut(&mut self) -> &mut Axis {
        match self {
            FrameAxis::Plain(a) => a,
            FrameAxis::Sky(s) => &mut s.axis,
        }
    }

    /// The Format that has been set: the Axis one, or a SkyAxis's own
    /// (skyaxis.c `TestAxisFormat`).
    pub(crate) fn format_slot(&self) -> &Option<String> {
        match self {
            FrameAxis::Plain(a) => &a.format,
            FrameAxis::Sky(s) => &s.format,
        }
    }

    /// The Format that has been set, to change.
    pub(crate) fn format_slot_mut(&mut self) -> &mut Option<String> {
        match self {
            FrameAxis::Plain(a) => &mut a.format,
            FrameAxis::Sky(s) => &mut s.format,
        }
    }

    /// The Digits used for the axis: its own, else the Frame's.
    pub(crate) fn digits_value(&self, frame_digits: i32) -> i32 {
        self.axis().digits.unwrap_or(frame_digits)
    }

    /// `astGetAxisFormat` with the Frame's Digits lent: axis.c
    /// `GetDefaultFormat` (`"%1.<Digits>G"`) or skyaxis.c `GetAxisFormat`.
    pub(crate) fn format_value(&self, frame_digits: i32) -> String {
        match self {
            FrameAxis::Plain(a) => a
                .format
                .clone()
                .unwrap_or_else(|| format!("%1.{}G", self.digits_value(frame_digits))),
            FrameAxis::Sky(s) => s.format_value(frame_digits),
        }
    }

    /// `astGetAxisUnit`: blank unless set, or the SkyAxis one.
    pub(crate) fn unit_value(&self, frame_digits: i32) -> String {
        match self {
            FrameAxis::Plain(a) => a.unit.clone().unwrap_or_default(),
            FrameAxis::Sky(s) => s.unit_value(frame_digits),
        }
    }

    /// `astGetAxisNormUnit`, or `None` for a plain Axis, whose NormUnit the
    /// Frame derives from the Unit it reports (frame.c:13149).
    pub(crate) fn sky_norm_unit(&self, frame_digits: i32) -> Option<String> {
        match self {
            FrameAxis::Plain(_) => None,
            FrameAxis::Sky(s) => Some(s.norm_unit_value(frame_digits)),
        }
    }

    /// `astGetAxisDirection`.
    pub(crate) fn direction_value(&self) -> bool {
        match self {
            FrameAxis::Plain(a) => a.direction.unwrap_or(true),
            FrameAxis::Sky(s) => s.direction_value(),
        }
    }

    /// `astGetAxisTop`.
    pub(crate) fn top_value(&self) -> f64 {
        match self {
            FrameAxis::Plain(a) => a.top.unwrap_or(f64::MAX),
            FrameAxis::Sky(s) => s.top_value(),
        }
    }

    /// `astGetAxisBottom`.
    pub(crate) fn bottom_value(&self) -> f64 {
        match self {
            FrameAxis::Plain(a) => a.bottom.unwrap_or(-f64::MAX),
            FrameAxis::Sky(s) => s.bottom_value(),
        }
    }

    /// The kind of Axis, as `Frame::get_axis` reports it.
    pub(crate) fn kind(&self) -> super::AxisKind {
        match self {
            FrameAxis::Plain(_) => super::AxisKind::Plain,
            FrameAxis::Sky(s) => s.kind(),
        }
    }

    /// The Axis object's dump: axis.c `Dump`, or skyaxis.c `Dump`.
    pub(crate) fn to_dump_node(&self) -> Node {
        match self {
            FrameAxis::Plain(a) => a.to_dump_node(),
            FrameAxis::Sky(s) => s.to_node_dump(),
        }
    }

    /// Read an Axis object of either class from a dump.
    pub(crate) fn read(node: &Node) -> crate::error::AstResult<Self> {
        use crate::serial::Serializable;
        Ok(if node.class == "SkyAxis" {
            FrameAxis::Sky(super::skyaxis::SkyAxis::from_node(node)?)
        } else {
            FrameAxis::Plain(Axis::read(node))
        })
    }

    /// Apply the per-axis items of a Frame dump for axis `n` (1-based),
    /// the Format going to a SkyAxis's own Format.
    /// See [`Axis::apply_frame_axis_keys`].
    pub(crate) fn apply_frame_axis_keys(&mut self, node: &Node, n: usize) {
        if let Some(f) = self.axis_mut().apply_frame_axis_keys(node, n) {
            *self.format_slot_mut() = Some(f);
        }
    }

    /// Overlay the attributes set on the template Axis object. Mirrors
    /// `astAxisOverlay`, dispatched on the template's class: axis.c:1226 for
    /// the Axis attributes, then, for a SkyAxis onto a SkyAxis,
    /// skyaxis.c:1370 for its Format, AsTime, IsLatitude and Centerzero.
    pub(crate) fn overlay_from(&mut self, template: &FrameAxis) {
        self.axis_mut().overlay_from(template.axis());
        if let (FrameAxis::Sky(r), FrameAxis::Sky(t)) = (self, template) {
            if t.format.is_some() {
                r.format.clone_from(&t.format);
            }
            if t.as_time.is_some() {
                r.as_time = t.as_time;
            }
            if t.is_latitude.is_some() {
                r.is_latitude = t.is_latitude;
            }
            if t.centre_zero.is_some() {
                r.centre_zero = t.centre_zero;
            }
        }
    }
}

/// Whether `val` lies between `lo` and `hi`, including the limits when
/// `closed`. Mirrors axis.c `AxisIn`.
pub(crate) fn axis_in(lo: f64, hi: f64, val: f64, closed: bool) -> bool {
    if closed {
        lo <= val && val <= hi
    } else {
        lo < val && val < hi
    }
}
