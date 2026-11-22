// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a Plot.
//!
//! A per-axis attribute given without an index (`Gap=10`) is set on every
//! axis, and read from the first. A per-element attribute (`Colour(Axes)`)
//! names its element by any unambiguous abbreviation, and a group of
//! elements (`Axes`) sets each member; without an element it is set on every
//! single element, and read from the border (Style, Width) or the axis
//! labels (Font, Colour, Size).
//!
//! The attributes whose defaults depend on what a grid draws (Grid, Border,
//! Labelling, Edge, Gap, Centre and the rest) read, when not set, as the
//! value a grid would use: getting one works the grid out without drawing,
//! through a logging graphics backend with no output, which is what the C
//! library linked with its logging graphics module gives.
//! [`Plot::get_attribute_with_grf`] gets them through another backend.
//!
//! Any other name goes to the Plot as a FrameSet.
// C: plot.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, whose parent is
// FrameSet; the GetUsed* functions run astGrid with Ink cleared.

use super::frameset::{frameset_clear, frameset_get, frameset_set, frameset_test};
use super::{
    axis_double_value, axis_int_value, axis_name, double_value, fmt_double, fmt_int, int_value,
    keyword_value, scan_whole,
};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::grf::{Grf, LogGrf};
use crate::mappings::Mapping;
use crate::plot::{Edge, Labelling, Plot, PlotElement, TextGapType, UsedValues};

/// The names of the graphical elements, in the order of [`PlotElement`].
// C: GrfLabels (plot.c).
const GRF_LABELS: &str = "Border Curves Title Markers Strings Axis1 Axis2 Axis3 NumLab1 NumLab2 \
                          NumLab3 TextLab1 TextLab2 TextLab3 Ticks1 Ticks2 Ticks3 Grid1 Grid2 \
                          Grid3 Axes NumLab TextLab Grid Ticks";

/// C's `Ustrncmp( a, b, n ) == 0` in plot.c: the first `n` characters of
/// `a` and `b` agree, ignoring case, where a string that ends before `n`
/// agrees only with another that ends at the same place.
fn ustrn_eq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        match (a.get(i), b.get(i)) {
            (None, None) => return true,
            (Some(x), Some(y)) if x.eq_ignore_ascii_case(y) => {}
            _ => return false,
        }
    }
    true
}

/// The index in the space-separated `list` of the option `test` names, by
/// a unique abbreviation or in full; an error naming `text` if it names
/// none or several.
// C: plot.c FullForm.
pub(crate) fn full_form(
    list: &str,
    test: &str,
    text: &str,
    method: &str,
    class: &str,
) -> AstResult<usize> {
    let len = super::chr_len(test);
    let mut found = None;
    let mut nmatch = 0;
    for (i, option) in list.split(' ').filter(|o| !o.is_empty()).enumerate() {
        if ustrn_eq(test.as_bytes(), option.as_bytes(), len) {
            found = Some(i);
            if super::chr_len(option) == len {
                nmatch = 1;
                break;
            }
            nmatch += 1;
        }
    }
    let shown = (&test[..len], &text[..super::chr_len(text)]);
    match (nmatch, found) {
        (1, Some(i)) => Ok(i),
        (0, _) | (_, None) => Err(AstError::new(
            ErrorCode::Opt,
            format!(
                "{method}({class}): Option '{}' is unknown in '{}'.",
                shown.0, shown.1
            ),
        )),
        _ => Err(AstError::new(
            ErrorCode::Opt,
            format!(
                "{method}({class}): Option '{}' is ambiguous in '{}'.",
                shown.0, shown.1
            ),
        )),
    }
}

/// The per-element attributes: name in a setting, and the setter, getter,
/// test and clear, by `which`.
#[derive(Clone, Copy, PartialEq)]
enum StyleAttr {
    Style,
    Font,
    Colour,
    Width,
    Size,
}

impl StyleAttr {
    /// The attribute a name starts with, and the name as C's FullForm error
    /// quotes it on a set (C writes "Colour" or "Color" as given).
    fn from_name(name: &str) -> Option<(Self, &'static str)> {
        Some(match name {
            "style" => (StyleAttr::Style, "Style"),
            "font" => (StyleAttr::Font, "Font"),
            "colour" => (StyleAttr::Colour, "Colour"),
            "color" => (StyleAttr::Colour, "Color"),
            "width" => (StyleAttr::Width, "Width"),
            "size" => (StyleAttr::Size, "Size"),
            _ => return None,
        })
    }

    /// Whether the value is an integer (Style, Font, Colour) or a real.
    fn is_int(self) -> bool {
        !matches!(self, StyleAttr::Width | StyleAttr::Size)
    }

    /// The element an unindexed get or test reads.
    fn default_element(self) -> PlotElement {
        match self {
            StyleAttr::Style | StyleAttr::Width => PlotElement::Border,
            _ => PlotElement::TextLab,
        }
    }

    fn set(self, plot: &mut Plot, element: PlotElement, value: f64) -> AstResult<()> {
        match self {
            StyleAttr::Style => plot.set_style(element, value as i32),
            StyleAttr::Font => plot.set_font(element, value as i32),
            StyleAttr::Colour => plot.set_colour(element, value as i32),
            StyleAttr::Width => return plot.set_width(element, value),
            StyleAttr::Size => return plot.set_size(element, value),
        }
        Ok(())
    }

    fn get(self, plot: &Plot, element: PlotElement) -> String {
        match self {
            StyleAttr::Style => fmt_int(plot.style(element)),
            StyleAttr::Font => fmt_int(plot.font(element)),
            StyleAttr::Colour => fmt_int(plot.colour(element)),
            StyleAttr::Width => fmt_double(plot.width(element)),
            StyleAttr::Size => fmt_double(plot.size(element)),
        }
    }

    fn test(self, plot: &Plot, element: PlotElement) -> bool {
        match self {
            StyleAttr::Style => plot.test_style(element),
            StyleAttr::Font => plot.test_font(element),
            StyleAttr::Colour => plot.test_colour(element),
            StyleAttr::Width => plot.test_width(element),
            StyleAttr::Size => plot.test_size(element),
        }
    }

    fn clear(self, plot: &mut Plot, element: PlotElement) {
        match self {
            StyleAttr::Style => plot.clear_style(element),
            StyleAttr::Font => plot.clear_font(element),
            StyleAttr::Colour => plot.clear_colour(element),
            StyleAttr::Width => plot.clear_width(element),
            StyleAttr::Size => plot.clear_size(element),
        }
    }
}

/// The single elements, which an unindexed set or clear acts on.
fn single_elements() -> &'static [PlotElement] {
    &PlotElement::ALL[..20]
}

/// The element a label names, by FullForm over C's element names.
fn element(label: &str, text: &str, method: &str) -> AstResult<PlotElement> {
    full_form(GRF_LABELS, label, text, method, "Plot").map(|i| PlotElement::ALL[i])
}

/// The style attribute and element label of a name of the form
/// `"style(%20[^()])"`, as a get, test or clear is given it.
fn style_name(name: &str) -> Option<(StyleAttr, &str)> {
    let open = name.find('(')?;
    let (attr, _) = StyleAttr::from_name(&name[..open])?;
    let scan = scan_whole(name, &format!("{}(%20[^()])%n", &name[..open]), 1)?;
    Some((attr, &name[open + 1..open + 1 + scan.text(0).len()]))
}

/// The per-axis attributes a setting without an index sets on every axis.
type AxisSetter<T> = fn(&mut Plot, usize, T) -> AstResult<()>;
/// A setter of a flag of the Plot as a whole.
type FlagSetter = fn(&mut Plot, bool);
/// A test of an attribute of the Plot as a whole.
type FlagTest = fn(&Plot) -> bool;
/// A clear of an attribute of the Plot as a whole.
type Clearer = fn(&mut Plot);

/// The integer per-axis attributes, by name.
fn int_axis_setter(name: &str) -> Option<AxisSetter<i32>> {
    Some(match name {
        "drawaxes" => |p, a, v| p.set_draw_axes(a, v != 0),
        "abbrev" => |p, a, v| p.set_abbreviate(a, v != 0),
        "labelup" => |p, a, v| p.set_label_up(a, v != 0),
        "logplot" => |p, a, v| p.set_log_plot(a, v != 0),
        "logticks" => |p, a, v| p.set_log_ticks(a, v != 0),
        "loglabel" => |p, a, v| p.set_log_label(a, v != 0),
        "numlab" => |p, a, v| p.set_num_lab(a, v != 0),
        "mintick" => |p, a, v| p.set_min_tick(a, v),
        "textlab" => |p, a, v| p.set_text_lab(a, v != 0),
        "labelunits" => |p, a, v| p.set_label_units(a, v != 0),
        _ => return None,
    })
}

/// The floating point per-axis attributes, by name. LabelAt and Centre
/// take an index always.
fn double_axis_setter(name: &str) -> Option<(AxisSetter<f64>, bool)> {
    Some(match name {
        "labelat" => (|p: &mut Plot, a, v| p.set_label_at(a, v), false),
        "centre" => (|p: &mut Plot, a, v| p.set_tick_centre(a, v), false),
        "gap" => (|p: &mut Plot, a, v| p.set_tick_gap(a, v), true),
        "loggap" => (|p: &mut Plot, a, v| p.set_log_gap(a, v), true),
        "numlabgap" => (|p: &mut Plot, a, v| p.set_num_lab_gap(a, v), true),
        "textlabgap" => (|p: &mut Plot, a, v| p.set_text_lab_gap(a, v), true),
        "majticklen" => (|p: &mut Plot, a, v| p.set_maj_tick_len(a, v), true),
        "minticklen" => (|p: &mut Plot, a, v| p.set_min_tick_len(a, v), true),
        _ => return None,
    })
}

/// C's order of the per-axis arms in SetAttrib does not matter: each
/// matches a distinct name. These are the names of both tables.
const INT_AXIS: [&str; 10] = [
    "drawaxes",
    "abbrev",
    "labelup",
    "logplot",
    "logticks",
    "loglabel",
    "numlab",
    "mintick",
    "textlab",
    "labelunits",
];
const DOUBLE_AXIS: [&str; 8] = [
    "labelat",
    "centre",
    "gap",
    "loggap",
    "numlabgap",
    "textlabgap",
    "majticklen",
    "minticklen",
];

/// The number of axes an unindexed setting is applied to.
fn nax(plot: &Plot) -> usize {
    Mapping::nin(plot)
}

/// Plot's `SetAttrib`.
pub(crate) fn plot_set(plot: &mut Plot, setting: &str) -> AstResult<()> {
    if let Some(v) = double_value(setting, "tol") {
        plot.set_tol(v);
        return Ok(());
    }
    let flags: [(&str, FlagSetter); 8] = [
        ("grid", Plot::set_grid),
        ("tickall", Plot::set_tick_all),
        ("forceexterior", Plot::set_force_exterior),
        ("invisible", Plot::set_invisible),
        ("border", Plot::set_border),
        ("clipop", Plot::set_clip_op),
        ("drawtitle", Plot::set_draw_title),
        ("escape", Plot::set_escape),
    ];
    for (name, set) in flags {
        if let Some(v) = int_value(setting, name) {
            set(plot, v != 0);
            return Ok(());
        }
    }
    if let Some(v) = int_value(setting, "clip") {
        return plot.set_clip(v);
    }
    if let Some(v) = int_value(setting, "grf") {
        plot.set_grf(v != 0);
        return Ok(());
    }
    for name in INT_AXIS {
        let set = int_axis_setter(name).expect("a named setter");
        if let Some(v) = int_value(setting, name) {
            for axis in 1..=nax(plot) {
                set(plot, axis, v)?;
            }
            return Ok(());
        }
        if let Some((axis, v)) = axis_int_value(setting, name) {
            return set(plot, usize::try_from(axis).unwrap_or(0), v);
        }
    }
    if let Some(scan) = scan_whole(setting, "edge(%d)= %n%*s %n", 1) {
        let value = &setting[scan.count(1)..];
        let edge = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
            [full_form("left right top bottom", value, setting, "astSet", "Plot")?];
        return plot.set_edge(usize::try_from(scan.int(0)).unwrap_or(0), edge);
    }
    for name in DOUBLE_AXIS {
        let (set, unindexed) = double_axis_setter(name).expect("a named setter");
        if unindexed && let Some(v) = double_value(setting, name) {
            for axis in 1..=nax(plot) {
                set(plot, axis, v)?;
            }
            return Ok(());
        }
        if let Some((axis, v)) = axis_double_value(setting, name) {
            return set(plot, usize::try_from(axis).unwrap_or(0), v);
        }
    }
    for name in ["style", "font", "colour", "color", "width", "size"] {
        let (attr, quoted) = StyleAttr::from_name(name).expect("a style name");
        let unindexed = if attr.is_int() {
            int_value(setting, name).map(f64::from)
        } else {
            double_value(setting, name)
        };
        if let Some(v) = unindexed {
            for &element in single_elements() {
                attr.set(plot, element, v)?;
            }
            return Ok(());
        }
        let conv = if attr.is_int() { "%d" } else { "%lg" };
        if let Some(scan) = scan_whole(setting, &format!("{name}(%20[^()])= {conv} %n"), 2) {
            let element = element(scan.text(0), quoted, "astSet")?;
            let v = if attr.is_int() {
                f64::from(scan.int(1))
            } else {
                scan.double(1)
            };
            return attr.set(plot, element, v);
        }
    }
    if let Some(v) = double_value(setting, "titlegap") {
        plot.set_title_gap(v);
        return Ok(());
    }
    if let Some(v) = keyword_value(setting, "labelling") {
        let i = full_form("exterior interior", v, setting, "astSet", "Plot")?;
        plot.set_labelling([Labelling::Exterior, Labelling::Interior][i]);
        return Ok(());
    }
    if let Some(v) = keyword_value(setting, "textgaptype") {
        let i = full_form("box plot", v, setting, "astSet", "Plot")?;
        plot.set_text_gap_type([TextGapType::Box, TextGapType::Plot][i]);
        return Ok(());
    }
    frameset_set(plot, setting)
}

/// A 1-based axis index checked as C's GetUsed functions check it.
fn used_axis_index(plot: &Plot, axis: i32, name: &str) -> AstResult<usize> {
    let nin = nax(plot).min(3);
    match usize::try_from(axis) {
        Ok(a) if a >= 1 && a <= nin => Ok(a),
        _ => Err(AstError::new(
            ErrorCode::Axiin,
            format!(
                "astGetUsed{name}(Plot): Index ({axis}) is invalid for attribute {name} - it \
                 should be in the range 1 to {nin}."
            ),
        )),
    }
}

/// The values a grid uses, worked out through `grf` on a copy of the Plot.
// C's GetUsed* functions run astGrid on the Plot itself. A grid that fails
// returns before clearing the LogTicks and LogLabel values it set while it
// worked, and the Plot keeps them; the port's grid puts them back
// (plot/grid.rs), and a copy leaves the Plot as it was either way.
fn used(plot: &Plot, grf: &mut dyn Grf) -> AstResult<UsedValues> {
    plot.clone().used_values(grf)
}

/// Plot's `GetAttrib`, with the backend that used values are worked out
/// through.
pub(crate) fn plot_get_with(plot: &Plot, name: &str, grf: &mut dyn Grf) -> AstResult<String> {
    let flag = |v: bool| fmt_int(i32::from(v));
    // An axis named by an index, or axis 1 for the unindexed form of a
    // per-axis attribute.
    let axis_of = |attr: &str, unindexed: bool| -> Option<i32> {
        if unindexed && name == attr {
            return Some(1);
        }
        axis_name(name, attr)
    };
    match name {
        "tol" => return Ok(fmt_double(plot.tol())),
        "grid" => {
            return Ok(flag(if plot.test_grid() {
                plot.grid()
            } else {
                used(plot, grf)?.grid
            }));
        }
        "tickall" => return Ok(flag(plot.tick_all())),
        "forceexterior" => return Ok(flag(plot.force_exterior())),
        "invisible" => return Ok(flag(plot.invisible())),
        "border" => {
            return Ok(flag(if plot.test_border() {
                plot.border()
            } else {
                used(plot, grf)?.border
            }));
        }
        "clipop" => return Ok(flag(plot.clip_op())),
        "clip" => return Ok(fmt_int(plot.clip())),
        "grf" => return Ok(flag(plot.grf())),
        "drawtitle" => return Ok(flag(plot.draw_title())),
        "escape" => return Ok(flag(plot.escape())),
        "titlegap" => return Ok(fmt_double(plot.title_gap())),
        "labelling" => {
            let l = if plot.test_labelling() {
                plot.labelling()
            } else {
                used(plot, grf)?.labelling
            };
            return Ok(l.name().to_string());
        }
        "textgaptype" => return Ok(plot.text_gap_type().name().to_string()),
        _ => {}
    }
    // Per-axis values a grid chooses when they are not set. C checks the
    // index in its GetUsed function, which names itself in the error.
    macro_rules! used_axis {
        ($attr:literal, $name:literal, $unindexed:expr, $test:ident, $get:ident, $field:ident,
         $fmt:expr) => {
            if let Some(axis) = axis_of($attr, $unindexed) {
                let axis = used_axis_index(plot, axis, $name)?;
                let value = if plot.$test(axis)? {
                    plot.$get(axis)?
                } else {
                    used(plot, grf)?.$field[axis - 1]
                };
                return Ok($fmt(value));
            }
        };
    }
    used_axis!(
        "labelat",
        "LabelAt",
        false,
        test_label_at,
        label_at,
        label_at,
        fmt_double
    );
    used_axis!(
        "centre",
        "Centre",
        false,
        test_tick_centre,
        tick_centre,
        centre,
        fmt_double
    );
    used_axis!("gap", "Gap", true, test_tick_gap, tick_gap, gap, fmt_double);
    used_axis!(
        "loggap",
        "LogGap",
        true,
        test_log_gap,
        log_gap,
        log_gap,
        fmt_double
    );
    used_axis!(
        "loglabel",
        "LogLabel",
        true,
        test_log_label,
        log_label,
        log_label,
        flag
    );
    used_axis!(
        "logticks",
        "LogTicks",
        true,
        test_log_ticks,
        log_ticks,
        log_ticks,
        flag
    );
    used_axis!(
        "mintick",
        "MinTick",
        true,
        test_min_tick,
        min_tick,
        min_tick,
        fmt_int
    );
    used_axis!(
        "textlab",
        "TextLab",
        true,
        test_text_lab,
        text_lab,
        text_lab,
        flag
    );
    used_axis!(
        "labelunits",
        "LabelUnits",
        true,
        test_label_units,
        label_units,
        label_units,
        flag
    );
    used_axis!(
        "majticklen",
        "MajTickLen",
        true,
        test_maj_tick_len,
        maj_tick_len,
        maj_tick_len,
        fmt_double
    );
    used_axis!("edge", "Edge", false, test_edge, edge, edge, |e: Edge| e
        .name()
        .to_string());
    // Per-axis values that are their own.
    macro_rules! plain_axis {
        ($attr:literal, $get:ident, $fmt:expr) => {
            if let Some(axis) = axis_of($attr, true) {
                return Ok($fmt(plot.$get(usize::try_from(axis).unwrap_or(0))?));
            }
        };
    }
    plain_axis!("numlabgap", num_lab_gap, fmt_double);
    plain_axis!("textlabgap", text_lab_gap, fmt_double);
    plain_axis!("labelup", label_up, flag);
    plain_axis!("logplot", log_plot, flag);
    plain_axis!("numlab", num_lab, flag);
    plain_axis!("drawaxes", draw_axes, flag);
    plain_axis!("abbrev", abbreviate, flag);
    plain_axis!("minticklen", min_tick_len, fmt_double);
    if let Some((attr, _)) = StyleAttr::from_name(name) {
        return Ok(attr.get(plot, attr.default_element()));
    }
    if let Some((attr, label)) = style_name(name) {
        return Ok(attr.get(plot, element(label, name, "astGet")?));
    }
    frameset_get(plot, name)
}

/// Plot's `GetAttrib`, with used values worked out through a logging
/// backend that writes nowhere.
pub(crate) fn plot_get(plot: &Plot, name: &str) -> AstResult<String> {
    plot_get_with(plot, name, &mut LogGrf::new(std::io::sink()))
}

/// Plot's `TestAttrib`.
pub(crate) fn plot_test(plot: &Plot, name: &str) -> AstResult<bool> {
    let simple: [(&str, FlagTest); 14] = [
        ("tol", Plot::test_tol),
        ("grid", Plot::test_grid),
        ("tickall", Plot::test_tick_all),
        ("forceexterior", Plot::test_force_exterior),
        ("invisible", Plot::test_invisible),
        ("border", Plot::test_border),
        ("clipop", Plot::test_clip_op),
        ("clip", Plot::test_clip),
        ("grf", Plot::test_grf),
        ("drawtitle", Plot::test_draw_title),
        ("escape", Plot::test_escape),
        ("titlegap", Plot::test_title_gap),
        ("labelling", Plot::test_labelling),
        ("textgaptype", Plot::test_text_gap_type),
    ];
    if let Some((_, test)) = simple.iter().find(|(n, _)| *n == name) {
        return Ok(test(plot));
    }
    type AxisTest = fn(&Plot, usize) -> AstResult<bool>;
    // C's TestAttrib has no arm for Centre, which goes on to the FrameSet
    // (c-library-quirks.md).
    let axes: [(&str, AxisTest, bool); 18] = [
        ("drawaxes", Plot::test_draw_axes, true),
        ("abbrev", Plot::test_abbreviate, true),
        ("edge", Plot::test_edge, false),
        ("labelat", Plot::test_label_at, false),
        ("gap", Plot::test_tick_gap, true),
        ("loggap", Plot::test_log_gap, true),
        ("numlabgap", Plot::test_num_lab_gap, true),
        ("textlabgap", Plot::test_text_lab_gap, true),
        ("labelup", Plot::test_label_up, true),
        ("logplot", Plot::test_log_plot, true),
        ("logticks", Plot::test_log_ticks, true),
        ("loglabel", Plot::test_log_label, true),
        ("numlab", Plot::test_num_lab, true),
        ("mintick", Plot::test_min_tick, true),
        ("textlab", Plot::test_text_lab, true),
        ("labelunits", Plot::test_label_units, true),
        ("majticklen", Plot::test_maj_tick_len, true),
        ("minticklen", Plot::test_min_tick_len, true),
    ];
    for (attr, test, unindexed) in axes {
        if unindexed && name == attr {
            return test(plot, 1);
        }
        if let Some(axis) = axis_name(name, attr) {
            return test(plot, usize::try_from(axis).unwrap_or(0));
        }
    }
    if let Some((attr, _)) = StyleAttr::from_name(name) {
        return Ok(attr.test(plot, attr.default_element()));
    }
    if let Some((attr, label)) = style_name(name) {
        return Ok(attr.test(plot, element(label, name, "astTest")?));
    }
    frameset_test(plot, name)
}

/// Plot's `ClearAttrib`.
pub(crate) fn plot_clear(plot: &mut Plot, name: &str) -> AstResult<()> {
    let simple: [(&str, Clearer); 14] = [
        ("tol", Plot::clear_tol),
        ("grid", Plot::clear_grid),
        ("tickall", Plot::clear_tick_all),
        ("forceexterior", Plot::clear_force_exterior),
        ("invisible", Plot::clear_invisible),
        ("border", Plot::clear_border),
        ("clipop", Plot::clear_clip_op),
        ("clip", Plot::clear_clip),
        ("grf", Plot::clear_grf),
        ("drawtitle", Plot::clear_draw_title),
        ("escape", Plot::clear_escape),
        ("titlegap", Plot::clear_title_gap),
        ("labelling", Plot::clear_labelling),
        ("textgaptype", Plot::clear_text_gap_type),
    ];
    if let Some((_, clear)) = simple.iter().find(|(n, _)| *n == name) {
        clear(plot);
        return Ok(());
    }
    type AxisClear = fn(&mut Plot, usize) -> AstResult<()>;
    let axes: [(&str, AxisClear, bool); 19] = [
        ("drawaxes", Plot::clear_draw_axes, true),
        ("abbrev", Plot::clear_abbreviate, true),
        ("edge", Plot::clear_edge, false),
        ("labelat", Plot::clear_label_at, false),
        ("centre", Plot::clear_tick_centre, false),
        ("gap", Plot::clear_tick_gap, true),
        ("loggap", Plot::clear_log_gap, true),
        ("numlabgap", Plot::clear_num_lab_gap, true),
        ("textlabgap", Plot::clear_text_lab_gap, true),
        ("labelup", Plot::clear_label_up, true),
        ("logplot", Plot::clear_log_plot, true),
        ("logticks", Plot::clear_log_ticks, true),
        ("loglabel", Plot::clear_log_label, true),
        ("numlab", Plot::clear_num_lab, true),
        ("mintick", Plot::clear_min_tick, true),
        ("textlab", Plot::clear_text_lab, true),
        ("labelunits", Plot::clear_label_units, true),
        ("majticklen", Plot::clear_maj_tick_len, true),
        ("minticklen", Plot::clear_min_tick_len, true),
    ];
    for (attr, clear, unindexed) in axes {
        if unindexed && name == attr {
            for axis in 1..=nax(plot) {
                clear(plot, axis)?;
            }
            return Ok(());
        }
        if let Some(axis) = axis_name(name, attr) {
            return clear(plot, usize::try_from(axis).unwrap_or(0));
        }
    }
    if let Some((attr, _)) = StyleAttr::from_name(name) {
        for &element in single_elements() {
            attr.clear(plot, element);
        }
        return Ok(());
    }
    if let Some((attr, label)) = style_name(name) {
        attr.clear(plot, element(label, name, "astClear")?);
        return Ok(());
    }
    frameset_clear(plot, name)
}
