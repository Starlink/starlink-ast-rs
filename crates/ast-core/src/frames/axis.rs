//! The attributes C's Axis class stores for one axis of a Frame (axis.c):
//! Label, Symbol, Unit, Digits, Format, Direction, Top and Bottom.
//!
//! Each is `None` until set. The defaults are supplied by the Frame that owns
//! the axis, because C's Frame methods substitute their own (`"Axis 1"` for a
//! Label, the Frame's Digits for an axis without its own) rather than the
//! Axis class's.

use crate::serial::Node;

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
    /// Write the attributes that are set, as C's `Axis::Dump` (axis.c:2900)
    /// does when the enclosing Frame dumps its axes with `Full = -1`, the
    /// setting `Frame::Dump` uses for them (frame.c:14660).
    pub(crate) fn write_set_attributes(&self, node: &mut Node) {
        if let Some(label) = &self.label {
            node.push_string("Label", label, Some("Axis Label".to_string()));
        }
        if let Some(symbol) = &self.symbol {
            node.push_string("Symbol", symbol, Some("Axis symbol".to_string()));
        }
        if let Some(unit) = &self.unit {
            // C appends the unit's full name only when it differs from the
            // unit string (axis.c, `lab && strcmp( lab, sval )`).
            let comment = match crate::units::unit_label(unit) {
                Some(lab) if lab != unit => format!("Axis units ({lab})"),
                _ => "Axis units".to_string(),
            };
            node.push_string("Unit", unit, Some(comment));
        }
        if let Some(digits) = self.digits {
            node.push_int(
                "Digits",
                digits as i64,
                Some("Default formatting precision".to_string()),
            );
        }
        if let Some(format) = &self.format {
            node.push_string("Format", format, Some("Format specifier".to_string()));
        }
        if let Some(dir) = self.direction {
            node.push_int(
                "Dirn",
                i64::from(dir),
                Some(if dir {
                    "Plot in conventional direction (hint)".to_string()
                } else {
                    "Plot in reverse direction (hint)".to_string()
                }),
            );
        }
        if let Some(top) = self.top {
            node.push_double("Top", top, Some("Maximum legal axis value".to_string()));
        }
        if let Some(bottom) = self.bottom {
            node.push_double(
                "Bottom",
                bottom,
                Some("Minimum legal axis value".to_string()),
            );
        }
    }

    /// Read the attributes present in an Axis node. Mirrors C's
    /// `astLoadAxis` (axis.c), which reads each with its unset default.
    pub(crate) fn read(node: &Node) -> Self {
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
