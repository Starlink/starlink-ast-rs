// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Plot test helpers: FITS headers read into FrameSets, and C's attribute
//! setting strings applied through the typed setters.

use std::path::PathBuf;

use starlink_ast::AnyObject;
use starlink_ast::channel::Channel;
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::fitschan::FitsChan;
use starlink_ast::frames::FrameSet;
use starlink_ast::plot::{Edge, Labelling, Plot, PlotElement, TextGapType};

/// The FrameSet a FITS header under `tests/fixtures/wcsconv/inputs` reads as.
pub fn read_head(name: &str) -> FrameSet {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut fc = FitsChan::from_header(&text).unwrap();
    match fc.read().unwrap().unwrap() {
        AnyObject::FrameSet(fs) => fs,
        other => panic!("{name}: expected a FrameSet, got {other:?}"),
    }
}

/// An attribute name as C writes it, split into its lower-case name and
/// its axis, if any.
pub fn split_name(name: &str) -> (String, Option<usize>) {
    match name.split_once('(') {
        Some((n, rest)) => (
            n.to_lowercase(),
            Some(rest.trim_end_matches(')').parse().unwrap()),
        ),
        None => (name.to_lowercase(), None),
    }
}

pub fn edge_named(value: &str) -> Edge {
    match value.chars().next().unwrap().to_ascii_lowercase() {
        'l' => Edge::Left,
        't' => Edge::Top,
        'r' => Edge::Right,
        _ => Edge::Bottom,
    }
}

/// Apply one `Name(axis)=value` setting through the typed setter.
pub fn set_attr(plot: &mut Plot, setting: &str) -> AstResult<()> {
    let (name, value) = setting.split_once('=').unwrap();
    let (name, axis) = split_name(name);
    let flag = || value.parse::<i32>().unwrap() != 0;
    let num = || value.parse::<f64>().unwrap();
    let int = || value.parse::<i32>().unwrap();
    match (name.as_str(), axis) {
        ("tol", None) => plot.set_tol(num()),
        ("grid", None) => plot.set_grid(flag()),
        ("tickall", None) => plot.set_tick_all(flag()),
        ("forceexterior", None) => plot.set_force_exterior(flag()),
        ("invisible", None) => plot.set_invisible(flag()),
        ("border", None) => plot.set_border(flag()),
        ("clipop", None) => plot.set_clip_op(flag()),
        ("clip", None) => plot.set_clip(int())?,
        ("drawtitle", None) => plot.set_draw_title(flag()),
        ("escape", None) => plot.set_escape(flag()),
        ("titlegap", None) => plot.set_title_gap(num()),
        ("labelling", None) => plot.set_labelling(if value.starts_with("int") {
            Labelling::Interior
        } else {
            Labelling::Exterior
        }),
        ("textgaptype", None) => plot.set_text_gap_type(if value == "plot" {
            TextGapType::Plot
        } else {
            TextGapType::Box
        }),
        ("drawaxes", Some(a)) => plot.set_draw_axes(a, flag())?,
        ("abbrev", Some(a)) => plot.set_abbreviate(a, flag())?,
        ("labelat", Some(a)) => plot.set_label_at(a, num())?,
        ("centre", Some(a)) => plot.set_tick_centre(a, num())?,
        ("gap", Some(a)) => plot.set_tick_gap(a, num())?,
        ("loggap", Some(a)) => plot.set_log_gap(a, num())?,
        ("numlabgap", Some(a)) => plot.set_num_lab_gap(a, num())?,
        ("textlabgap", Some(a)) => plot.set_text_lab_gap(a, num())?,
        ("labelup", Some(a)) => plot.set_label_up(a, flag())?,
        ("logplot", Some(a)) => plot.set_log_plot(a, flag())?,
        ("logticks", Some(a)) => plot.set_log_ticks(a, flag())?,
        ("loglabel", Some(a)) => plot.set_log_label(a, flag())?,
        ("numlab", Some(a)) => plot.set_num_lab(a, flag())?,
        ("mintick", Some(a)) => plot.set_min_tick(a, int())?,
        ("textlab", Some(a)) => plot.set_text_lab(a, flag())?,
        ("labelunits", Some(a)) => plot.set_label_units(a, flag())?,
        ("majticklen", Some(a)) => plot.set_maj_tick_len(a, num())?,
        ("minticklen", Some(a)) => plot.set_min_tick_len(a, num())?,
        ("edge", Some(a)) => plot.set_edge(a, edge_named(value))?,
        _ => panic!("no setter for {setting}"),
    }
    Ok(())
}

/// A style attribute name as C writes it, `Colour(Axes)`, split into its
/// lower-case attribute name and its element, `None` for a name C does
/// not know.
pub fn split_style(name: &str) -> (String, Option<PlotElement>) {
    let (attr, element) = name.split_once('(').unwrap();
    let element = element.trim_end_matches(')');
    let found = PlotElement::ALL
        .into_iter()
        .find(|e| e.name().eq_ignore_ascii_case(element));
    (attr.to_lowercase(), found)
}

pub fn set_style_attr(plot: &mut Plot, setting: &str) -> AstResult<()> {
    let (name, value) = setting.split_once('=').unwrap();
    let (attr, element) = split_style(name);
    let Some(element) = element else {
        return Err(starlink_ast::error::AstError::new(
            ErrorCode::Badat,
            format!("unknown element in {name}"),
        ));
    };
    let int = || value.trim().parse::<i32>().unwrap();
    let real = || value.trim().parse::<f64>().unwrap();
    match attr.as_str() {
        "style" => plot.set_style(element, int()),
        "font" => plot.set_font(element, int()),
        "colour" => plot.set_colour(element, int()),
        "width" => return plot.set_width(element, real()),
        "size" => return plot.set_size(element, real()),
        _ => panic!("unknown style attribute {attr}"),
    }
    Ok(())
}

/// Apply an astSet string of Plot attributes, style attributes included.
pub fn apply_settings(plot: &mut Plot, settings: &str) -> AstResult<()> {
    for setting in settings.split(',').filter(|s| !s.is_empty()) {
        let name = setting.split(['(', '=']).next().unwrap().to_lowercase();
        if ["style", "font", "colour", "width", "size"].contains(&name.as_str()) {
            set_style_attr(plot, setting)?;
        } else {
            set_attr(plot, setting)?;
        }
    }
    Ok(())
}
