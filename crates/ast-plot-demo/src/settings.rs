// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Plot attributes set from a string in the C library's `astSet` syntax:
//! `Name=value` settings separated by commas, as in
//! `Grid=1,Labelling=interior,Colour(axes)=3`.
//!
//! Names are case-insensitive. An axis attribute given without an axis is
//! set on every axis, and a style attribute (`Style`, `Font`, `Colour`,
//! `Width`, `Size`) given without an element is set on every element, as
//! the C library does.
//! The names and values accepted are those the grid test cases use and the
//! obvious ones beside them, not the C library's full set of spellings.

use std::str::FromStr;

use starlink_ast::error::{AstError, AstResult, ErrorCode};
use starlink_ast::frames::{CmpFrame, Frame, SpecFrame, SpecSystem};
use starlink_ast::mappings::Mapping;
use starlink_ast::plot::{Edge, Labelling, Plot, PlotElement, TextGapType};

/// The per-axis attributes, set on every axis of the base Frame when no
/// axis is given.
const PER_AXIS: [&str; 19] = [
    "drawaxes",
    "abbrev",
    "labelat",
    "centre",
    "gap",
    "loggap",
    "numlabgap",
    "textlabgap",
    "labelup",
    "logplot",
    "logticks",
    "loglabel",
    "numlab",
    "mintick",
    "textlab",
    "labelunits",
    "majticklen",
    "minticklen",
    "edge",
];

/// The style attributes, which take a graphical element.
const STYLES: [&str; 5] = ["style", "font", "colour", "width", "size"];

/// Apply the comma-separated settings in `settings` to `plot`, in order.
///
/// # Errors
///
/// - `ErrorCode::Badat` for an attribute or element name not known here.
/// - `ErrorCode::Attin` for a setting with no `=` or a value that does not
///   parse.
/// - Any error the Plot's setter gives.
pub fn apply_settings(plot: &mut Plot, settings: &str) -> AstResult<()> {
    for setting in settings.split(',').filter(|s| !s.trim().is_empty()) {
        let setting = setting.trim();
        let Some((name, value)) = setting.split_once('=') else {
            return Err(bad_value(setting));
        };
        let base = name.split('(').next().unwrap_or(name).trim().to_lowercase();
        if STYLES.contains(&base.as_str()) {
            set_style(plot, setting, name.trim(), value.trim())?;
        } else {
            set_attr(plot, setting, name.trim(), value.trim())?;
        }
    }
    Ok(())
}

fn unknown(setting: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astSet(Plot): unknown attribute \"{setting}\"."),
    )
}

fn bad_value(setting: &str) -> AstError {
    AstError::new(
        ErrorCode::Attin,
        format!("astSet(Plot): invalid value in \"{setting}\"."),
    )
}

/// A name as `Name(qualifier)`, split into its lower-case name and its
/// qualifier, if any.
fn split_name(name: &str) -> (String, Option<&str>) {
    match name.split_once('(') {
        Some((n, rest)) => (
            n.trim().to_lowercase(),
            Some(rest.trim_end_matches(')').trim()),
        ),
        None => (name.to_lowercase(), None),
    }
}

fn parse<T: FromStr>(setting: &str, value: &str) -> AstResult<T> {
    value.parse().map_err(|_| bad_value(setting))
}

fn edge_named(setting: &str, value: &str) -> AstResult<Edge> {
    match value.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('l') => Ok(Edge::Left),
        Some('t') => Ok(Edge::Top),
        Some('r') => Ok(Edge::Right),
        Some('b') => Ok(Edge::Bottom),
        _ => Err(bad_value(setting)),
    }
}

fn set_attr(plot: &mut Plot, setting: &str, name: &str, value: &str) -> AstResult<()> {
    let (name, axis) = split_name(name);
    let axis = match axis {
        Some(a) => Some(parse::<usize>(setting, a).map_err(|_| unknown(setting))?),
        None if PER_AXIS.contains(&name.as_str()) => {
            for a in 1..=Mapping::nin(plot) {
                set_axis_attr(plot, setting, &name, a, value)?;
            }
            return Ok(());
        }
        None => None,
    };
    if let Some(a) = axis {
        return set_axis_attr(plot, setting, &name, a, value);
    }
    let flag = || parse::<i32>(setting, value).map(|v| v != 0);
    let num = || parse::<f64>(setting, value);
    match name.as_str() {
        "tol" => plot.set_tol(num()?),
        "grid" => plot.set_grid(flag()?),
        "tickall" => plot.set_tick_all(flag()?),
        "forceexterior" => plot.set_force_exterior(flag()?),
        "invisible" => plot.set_invisible(flag()?),
        "border" => plot.set_border(flag()?),
        "clipop" => plot.set_clip_op(flag()?),
        "clip" => plot.set_clip(parse(setting, value)?)?,
        "drawtitle" => plot.set_draw_title(flag()?),
        "escape" => plot.set_escape(flag()?),
        "titlegap" => plot.set_title_gap(num()?),
        "labelling" => {
            let v = value.to_lowercase();
            let labelling = if !v.is_empty() && "interior".starts_with(&v) {
                Labelling::Interior
            } else if !v.is_empty() && "exterior".starts_with(&v) {
                Labelling::Exterior
            } else {
                return Err(bad_value(setting));
            };
            plot.set_labelling(labelling);
        }
        "textgaptype" => plot.set_text_gap_type(match value.to_lowercase().as_str() {
            "plot" => TextGapType::Plot,
            "box" => TextGapType::Box,
            _ => return Err(bad_value(setting)),
        }),
        "title" => plot.set_title(value)?,
        "system" => set_component_system(plot, setting, value)?,
        _ => return Err(unknown(setting)),
    }
    Ok(())
}

fn set_axis_attr(
    plot: &mut Plot,
    setting: &str,
    name: &str,
    a: usize,
    value: &str,
) -> AstResult<()> {
    let flag = || parse::<i32>(setting, value).map(|v| v != 0);
    let num = || parse::<f64>(setting, value);
    let int = || parse::<i32>(setting, value);
    match name {
        "drawaxes" => plot.set_draw_axes(a, flag()?),
        "abbrev" => plot.set_abbreviate(a, flag()?),
        "labelat" => plot.set_label_at(a, num()?),
        "centre" => plot.set_tick_centre(a, num()?),
        "gap" => plot.set_tick_gap(a, num()?),
        "loggap" => plot.set_log_gap(a, num()?),
        "numlabgap" => plot.set_num_lab_gap(a, num()?),
        "textlabgap" => plot.set_text_lab_gap(a, num()?),
        "labelup" => plot.set_label_up(a, flag()?),
        "logplot" => plot.set_log_plot(a, flag()?),
        "logticks" => plot.set_log_ticks(a, flag()?),
        "loglabel" => plot.set_log_label(a, flag()?),
        "numlab" => plot.set_num_lab(a, flag()?),
        "mintick" => plot.set_min_tick(a, int()?),
        "textlab" => plot.set_text_lab(a, flag()?),
        "labelunits" => plot.set_label_units(a, flag()?),
        "majticklen" => plot.set_maj_tick_len(a, num()?),
        "minticklen" => plot.set_min_tick_len(a, num()?),
        "edge" => plot.set_edge(a, edge_named(setting, value)?),
        "format" => plot.set_format_str(a, value),
        "bottom" => plot.set_bottom(a, num()?),
        "digits" => plot.set_axis_digits(a, int()?),
        _ => Err(unknown(setting)),
    }
}

fn set_style(plot: &mut Plot, setting: &str, name: &str, value: &str) -> AstResult<()> {
    let (attr, element) = split_name(name);
    let elements: Vec<PlotElement> = match element {
        // The C library sets every single element.
        None => PlotElement::ALL[..20].to_vec(),
        Some(e) => vec![
            PlotElement::ALL
                .into_iter()
                .find(|p| p.name().eq_ignore_ascii_case(e))
                .ok_or_else(|| unknown(setting))?,
        ],
    };
    for element in elements {
        match attr.as_str() {
            "style" => plot.set_style(element, parse(setting, value)?),
            "font" => plot.set_font(element, parse(setting, value)?),
            "colour" => plot.set_colour(element, parse(setting, value)?),
            "width" => plot.set_width(element, parse(setting, value)?)?,
            "size" => plot.set_size(element, parse(setting, value)?)?,
            _ => return Err(unknown(setting)),
        }
    }
    Ok(())
}

/// Set the System of a Plot whose current Frame is a CmpFrame with a
/// SpecFrame first, as the C library's `astSet` of `System` does through a
/// Plot: the spectral component changes, and the Plot's Mapping follows.
fn set_component_system(plot: &mut Plot, setting: &str, value: &str) -> AstResult<()> {
    let system = SpecSystem::parse(value).ok_or_else(|| bad_value(setting))?;
    let mut found = true;
    plot.frameset_mut().with_integrity(|f| {
        match f
            .as_any_mut()
            .downcast_mut::<CmpFrame>()
            .and_then(|cmp| cmp.frame1_mut().as_any_mut().downcast_mut::<SpecFrame>())
        {
            Some(spec) => spec.set_system(system),
            None => {
                found = false;
                Ok(())
            }
        }
    })?;
    if found { Ok(()) } else { Err(unknown(setting)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::{build_plot, cases, graphics_box, plot_box};
    use starlink_ast::error::ErrorCode;
    use starlink_ast::plot::{Labelling, Plot, PlotElement};

    fn plot() -> Plot {
        let case = &cases()[0];
        build_plot(case, "", graphics_box(800.0, 600.0, plot_box(case))).unwrap()
    }

    #[test]
    fn settings_reach_the_typed_setters() {
        let mut plot = plot();
        apply_settings(&mut plot, "grid=1,labelling=int,Colour(axes)=3,width=2").unwrap();
        assert!(plot.grid());
        assert_eq!(plot.labelling(), Labelling::Interior);
        assert_eq!(plot.colour(PlotElement::Axis1), 3);
        assert_eq!(plot.width(PlotElement::Title), 2.0);
    }

    #[test]
    fn unknown_attribute_is_an_error() {
        let mut plot = plot();
        let mut code = |s| apply_settings(&mut plot, s).unwrap_err().code();
        assert_eq!(code("nosuch=1"), ErrorCode::Badat);
        assert_eq!(code("colour(nosuch)=2"), ErrorCode::Badat);
    }

    #[test]
    fn bad_value_is_an_error() {
        let mut plot = plot();
        let mut code = |s| apply_settings(&mut plot, s).unwrap_err().code();
        assert_eq!(code("grid=yes"), ErrorCode::Attin);
        assert_eq!(code("tol"), ErrorCode::Attin);
    }
}
