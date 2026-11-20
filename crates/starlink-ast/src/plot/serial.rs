// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A Plot's Native description: its FrameSet's, then its own attributes,
//! clipping region, graphics box and tick values.
// C: plot.c Dump (:30020) and astLoadPlot_ (:31209).
//
// Dump writes an attribute that is set with its value, and one that is not
// with the value its getter gives, which a reader ignores. Three things it
// does are mirrored here because they decide what a dump holds:
// - Gap and LogGap are written only when set, but with the set flag that
//   the Centre loop before them left behind, that of the last axis's
//   Centre; when that Centre is unset, a Gap or LogGap that is set is
//   written as a default, which is neither shown at Full 0 nor read back.
// - LabelAt, Centre and the Width, Size and gap attributes are written only
//   when their value is not bad.
// - Style, Font, Colour, Width and Size are written for each element whose
//   axis the Plot has.
// The loader reads each attribute and validates it through its setter, as
// C's does, except LogPlot, which it stores as read: the Mapping it implies
// is already in the FrameSet, so C's SetLogPlot, finding the value
// unchanged, toggles nothing.

use super::attrs::{Edge, Labelling, TextGapType};
use super::elements::{NPID, PlotElement, grf_item};
use super::{ClipRegion, Plot, TickValues};
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameSet;
use crate::mappings::Mapping;
use crate::serial::{Node, Serializable};

/// The names C writes for Edge, Labelling and TextGapType, in the order of
/// their values.
// C: xedge, xlbling and xtgaptype (plot.c:1723).
const EDGES: [(&str, Edge); 4] = [
    ("left", Edge::Left),
    ("top", Edge::Top),
    ("right", Edge::Right),
    ("bottom", Edge::Bottom),
];
const LABELLINGS: [(&str, Labelling); 2] = [
    ("exterior", Labelling::Exterior),
    ("interior", Labelling::Interior),
];
const TEXT_GAP_TYPES: [(&str, TextGapType); 2] =
    [("box", TextGapType::Box), ("plot", TextGapType::Plot)];

/// The value named `text` in `list`, compared ignoring case, or C's error
/// naming the component `component`.
// C: FindString (plot.c), with Ustrcmp.
fn find_string<T: Copy>(list: &[(&str, T)], text: &str, component: &str) -> AstResult<T> {
    list.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map(|&(_, value)| value)
        .ok_or_else(|| {
            AstError::new(
                ErrorCode::Opt,
                format!(
                    "astRead(Channel): Illegal value '{text}' supplied for the Plot component \
                     '{component}'."
                ),
            )
        })
}

/// An integer item as C's loader reads it with -1 meaning "unset".
// C reads each integer with sscanf's %d into an int, which keeps the low 32
// bits of a value beyond int's range; the `as i32` casts on these values
// do the same.
fn read_set_int(node: &Node, key: &str) -> Option<i64> {
    let value = node.get_int(key, -1);
    (value != -1).then_some(value)
}

/// A flag item as C's loader reads it: unset when absent or -1, otherwise
/// whether it is non-zero.
fn read_flag(node: &Node, key: &str) -> Option<bool> {
    read_set_int(node, key).map(|v| v != 0)
}

/// A floating-point item as C's loader reads it, with the bad value meaning
/// "unset".
fn read_set_double(node: &Node, key: &str) -> Option<f64> {
    let value = node.get_double(key, BAD_VALUE);
    (value != BAD_VALUE).then_some(value)
}

/// A per-axis attribute read for axes 1 to `nax` from the items `key1`,
/// `key2`, ..., unset on the axes beyond.
fn per_axis<T: Copy>(
    nax: usize,
    key: &str,
    read: impl Fn(&str) -> AstResult<Option<T>>,
) -> AstResult<[Option<T>; 3]> {
    let mut out = [None; 3];
    for (axis, value) in out.iter_mut().enumerate().take(nax) {
        *value = read(&format!("{key}{}", axis + 1))?;
    }
    Ok(out)
}

impl Serializable for Plot {
    fn class_name(&self) -> &'static str {
        "Plot"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Serializable::to_node(&self.frameset)?;
        node.class = "Plot".to_string();
        node.class_comment = Some("Provide facilities for 2D graphical output".to_string());
        node.push_isa(
            "FrameSet",
            Some("Set of inter-related coordinate systems".to_string()),
        );

        let a = &self.attrs;
        // C writes an axis attribute for every input, reading past its
        // arrays of three for a Plot with more (c-library-quirks.md).
        let nax = Mapping::nin(self).min(3);

        node.write_double(
            "Tol",
            a.tol.is_some(),
            false,
            self.tol(),
            "Plotting tolerance",
        );
        let flags: [(&str, Option<bool>, bool, bool, &str); 6] = [
            ("Grid", a.grid, self.grid(), false, "Is a grid required?"),
            (
                "TckAll",
                a.tick_all,
                self.tick_all(),
                true,
                "Put ticks on all edges?",
            ),
            (
                "FrcExt",
                a.force_exterior,
                self.force_exterior(),
                true,
                "Force exterior labelling?",
            ),
            (
                "Invsbl",
                a.invisible,
                self.invisible(),
                true,
                "Use invisible ink?",
            ),
            (
                "Border",
                a.border,
                self.border(),
                false,
                "Draw a border round the grid?",
            ),
            (
                "ClpOp",
                a.clip_op,
                self.clip_op(),
                false,
                "Clip using logical OR?",
            ),
        ];
        for (key, set, value, helpful, comment) in flags {
            node.write_int(key, set.is_some(), helpful, i64::from(value), comment);
        }
        let clip = self.clip();
        node.write_int(
            "Clip",
            a.clip.is_some(),
            false,
            i64::from(clip),
            match clip {
                0 => "Do not clip at plot edges",
                1 => "Clip curves at plot edges",
                2 => "Clip markers at plot edges",
                _ => "Clip markers and curves at plot edges",
            },
        );
        node.write_int(
            "DrwTtl",
            a.draw_title.is_some(),
            true,
            i64::from(self.draw_title()),
            "Add a title to the grid?",
        );
        for axis in 1..=nax {
            node.write_int(
                &format!("DrwAxs{axis}"),
                a.draw_axes[axis - 1].is_some(),
                false,
                i64::from(self.draw_axes(axis)?),
                "Draw axis through ticks?",
            );
        }
        for axis in 1..=nax {
            node.write_int(
                &format!("Abbrv{axis}"),
                a.abbrev[axis - 1].is_some(),
                false,
                i64::from(self.abbreviate(axis)?),
                "Abbreviate numerical axis labels?",
            );
        }
        node.write_int(
            "Escape",
            a.escape.is_some(),
            true,
            i64::from(self.escape()),
            "Interpret escape sequences?",
        );
        for axis in 1..=nax {
            let value = self.label_at(axis)?;
            if value != BAD_VALUE {
                node.write_double(
                    &format!("LblAt{axis}"),
                    a.label_at[axis - 1].is_some(),
                    false,
                    value,
                    "Put numerical labels at",
                );
            }
        }
        for axis in 1..=nax {
            let value = self.tick_centre(axis)?;
            if value != BAD_VALUE {
                node.write_double(
                    &format!("Cen{axis}"),
                    a.centre[axis - 1].is_some(),
                    false,
                    value,
                    "Tick mark origin",
                );
            }
        }
        // C writes Gap and LogGap with the set flag the Centre loop left.
        let stale_set = a.centre[nax - 1].is_some();
        for axis in 1..=nax {
            if a.gap[axis - 1].is_some() {
                let value = self.tick_gap(axis)?;
                if value != BAD_VALUE {
                    node.write_double(
                        &format!("Gap{axis}"),
                        stale_set,
                        false,
                        value,
                        "Difference between ticks",
                    );
                }
            }
        }
        for axis in 1..=nax {
            if a.log_gap[axis - 1].is_some() {
                let value = self.log_gap(axis)?;
                if value != BAD_VALUE {
                    node.write_double(
                        &format!("LgGap{axis}"),
                        stale_set,
                        false,
                        value,
                        "Ratio between ticks",
                    );
                }
            }
        }
        for axis in 1..=nax {
            let value = self.num_lab_gap(axis)?;
            if value != BAD_VALUE {
                node.write_double(
                    &format!("NmGap{axis}"),
                    a.num_lab_gap[axis - 1].is_some(),
                    true,
                    value,
                    "Spacing of numerical labels",
                );
            }
        }
        for axis in 1..=nax {
            let value = self.text_lab_gap(axis)?;
            if value != BAD_VALUE {
                node.write_double(
                    &format!("TxGap{axis}"),
                    a.text_lab_gap[axis - 1].is_some(),
                    true,
                    value,
                    "Spacing of descriptive labels",
                );
            }
        }
        type AxisFlag = fn(&Plot, usize) -> AstResult<i64>;
        // Key, whether each axis is set, value written, helpful, comment.
        type AxisFlagItem<'a> = (&'a str, [bool; 3], AxisFlag, bool, &'a str);
        let set = |field: &[Option<bool>; 3]| field.map(|v| v.is_some());
        let axis_flags: [AxisFlagItem; 5] = [
            (
                "LblUp",
                set(&a.label_up),
                |p, axis| p.label_up(axis).map(i64::from),
                true,
                "Draw numerical labels upright?",
            ),
            (
                "LgPlt",
                a.log_plot.map(|v| v.is_some()),
                // C writes the integer it holds.
                |p, axis| Ok(p.attrs.log_plot[axis - 1].map_or(0, i64::from)),
                true,
                "Map plot axis logarithmically?",
            ),
            (
                "LgTck",
                set(&a.log_ticks),
                |p, axis| p.log_ticks(axis).map(i64::from),
                true,
                "Space ticks logarithmically?",
            ),
            (
                "LgLbl",
                set(&a.log_label),
                |p, axis| p.log_label(axis).map(i64::from),
                true,
                "Scientific notation for labels?",
            ),
            (
                "NmLbl",
                set(&a.num_lab),
                |p, axis| p.num_lab(axis).map(i64::from),
                true,
                "Draw numerical labels?",
            ),
        ];
        for (key, set, get, helpful, comment) in axis_flags {
            for axis in 1..=nax {
                node.write_int(
                    &format!("{key}{axis}"),
                    set[axis - 1],
                    helpful,
                    get(self, axis)?,
                    comment,
                );
            }
        }
        for axis in 1..=nax {
            node.write_int(
                &format!("MnTks{axis}"),
                a.min_tick[axis - 1].is_some(),
                false,
                i64::from(self.min_tick(axis)?),
                "No. of sub-divisions between major tick marks",
            );
        }
        for axis in 1..=nax {
            node.write_int(
                &format!("TxLbl{axis}"),
                a.text_lab[axis - 1].is_some(),
                false,
                i64::from(self.text_lab(axis)?),
                "Draw textual label?",
            );
        }
        for axis in 1..=nax {
            node.write_int(
                &format!("LbUnt{axis}"),
                a.label_units[axis - 1].is_some(),
                false,
                i64::from(self.label_units(axis)?),
                "Add units to axis label?",
            );
        }

        // The style of each element whose axis the Plot has.
        let shown = |id: usize| grf_item(id).1.is_none_or(|ax| ax < nax);
        let s = &a.styles;
        type IntStyle = fn(&Plot, PlotElement) -> i32;
        let int_styles: [(&str, &[Option<i32>; NPID], IntStyle, &str); 3] = [
            ("Style", &s.style, Plot::style, " line style"),
            ("Font", &s.font, Plot::font, " character font"),
            ("Col", &s.colour, Plot::colour, " colour index"),
        ];
        for (key, field, get, what) in int_styles {
            for id in (0..NPID).filter(|&id| shown(id)) {
                node.write_int(
                    &format!("{key}{}", id + 1),
                    field[id].is_some(),
                    false,
                    i64::from(get(self, PlotElement::ALL[id])),
                    &format!("{}{what}", grf_item(id).0),
                );
            }
        }
        type RealStyle = fn(&Plot, PlotElement) -> f64;
        let real_styles: [(&str, &[Option<f64>; NPID], RealStyle, &str); 2] = [
            ("Width", &s.width, Plot::width, " line width"),
            ("Size", &s.size, Plot::size, " character size"),
        ];
        for (key, field, get, what) in real_styles {
            for id in (0..NPID).filter(|&id| shown(id)) {
                let value = get(self, PlotElement::ALL[id]);
                if value != BAD_VALUE {
                    node.write_double(
                        &format!("{key}{}", id + 1),
                        field[id].is_some(),
                        false,
                        value,
                        &format!("{}{what}", grf_item(id).0),
                    );
                }
            }
        }

        let value = self.title_gap();
        if value != BAD_VALUE {
            node.write_double(
                "TtlGap",
                a.title_gap.is_some(),
                true,
                value,
                "Gap between title and edge",
            );
        }
        for axis in 1..=nax {
            let value = self.maj_tick_len(axis)?;
            if value != BAD_VALUE {
                node.write_double(
                    &format!("MjTkLn{axis}"),
                    a.maj_tick_len[axis - 1].is_some(),
                    false,
                    value,
                    "Major tick length",
                );
            }
        }
        for axis in 1..=nax {
            let value = self.min_tick_len(axis)?;
            if value != BAD_VALUE {
                node.write_double(
                    &format!("MnTkLn{axis}"),
                    a.min_tick_len[axis - 1].is_some(),
                    true,
                    value,
                    "Minor tick length",
                );
            }
        }
        node.write_string(
            "Lbling",
            a.labelling.is_some(),
            false,
            self.labelling().name(),
            "Labelling scheme",
        );
        node.write_string(
            "TGapTyp",
            a.text_gap_type.is_some(),
            false,
            self.text_gap_type().name(),
            "Text gap reference position",
        );
        for axis in 1..=nax {
            node.write_string(
                &format!("Edge{axis}"),
                a.edge[axis - 1].is_some(),
                false,
                self.edge(axis)?.name(),
                "Edge used to label an axis",
            );
        }

        if let Some(clip) = &self.clip {
            for (i, &value) in clip.lbnd.iter().enumerate() {
                if value != BAD_VALUE {
                    node.write_double(
                        &format!("ClpLb{}", i + 1),
                        true,
                        false,
                        value,
                        "Lower bound of clipping region",
                    );
                }
            }
            for (i, &value) in clip.ubnd.iter().enumerate() {
                if value != BAD_VALUE {
                    node.write_double(
                        &format!("ClpUb{}", i + 1),
                        true,
                        false,
                        value,
                        "Upper bound of clipping region",
                    );
                }
            }
            node.write_int(
                "ClpAxs",
                true,
                false,
                clip.lbnd.len() as i64,
                "No. of bounds for clipping region",
            );
            node.write_int(
                "ClpFrm",
                true,
                false,
                i64::from(clip.frame),
                "Index of clipping Frame",
            );
        }

        for (key, value, comment) in [
            ("Xlo", self.xlo, "Lower X bound of plotting area"),
            ("Ylo", self.ylo, "Lower Y bound of plotting area"),
            ("Xhi", self.xhi, "Upper X bound of plotting area"),
            ("Yhi", self.yhi, "Upper Y bound of plotting area"),
        ] {
            node.write_double(key, true, true, value, comment);
        }
        node.write_int(
            "Xrev",
            true,
            false,
            i64::from(self.xrev),
            "X axis reversed?",
        );
        node.write_int(
            "Yrev",
            true,
            false,
            i64::from(self.yrev),
            "Y axis reversed?",
        );
        for (key, value, comment) in [
            ("Xb1", self.bbox[0], "Lower X bound of supplied base Frame"),
            ("Yb1", self.bbox[1], "Lower Y bound of supplied base Frame"),
            ("Xb2", self.bbox[2], "Upper X bound of supplied base Frame"),
            ("Yb2", self.bbox[3], "Upper Y bound of supplied base Frame"),
        ] {
            node.write_double(key, true, true, value, comment);
        }

        for (axis, ticks) in self.tick_values.iter().enumerate() {
            let axis = axis + 1;
            if !ticks.major.is_empty() {
                node.write_int(
                    &format!("NMjTk{axis}"),
                    true,
                    true,
                    ticks.major.len() as i64,
                    "",
                );
                for (i, &value) in ticks.major.iter().enumerate() {
                    node.write_double(&format!("MjTk{axis}_{}", i + 1), true, true, value, "");
                }
            }
            if !ticks.minor.is_empty() {
                node.write_int(
                    &format!("NMnTk{axis}"),
                    true,
                    true,
                    ticks.minor.len() as i64,
                    "",
                );
                for (i, &value) in ticks.minor.iter().enumerate() {
                    node.write_double(&format!("MnTk{axis}_{}", i + 1), true, true, value, "");
                }
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let frameset = FrameSet::from_node(node)?;
        let node = &node.section("Plot");
        let nax = Mapping::nin(&frameset).min(3);
        let mut plot = Plot {
            frameset,
            xlo: 0.0,
            ylo: 0.0,
            xhi: 1.0,
            yhi: 1.0,
            xrev: false,
            yrev: false,
            bbox: [BAD_VALUE; 4],
            clip: None,
            tick_values: Default::default(),
            draw: Default::default(),
            attrs: Default::default(),
        };

        let tol = node.get_double("tol", -1.0);
        if tol != -1.0 {
            plot.set_tol(tol);
        }
        let a = &mut plot.attrs;
        a.grid = read_flag(node, "grid");
        a.tick_all = read_flag(node, "tckall");
        a.force_exterior = read_flag(node, "frcext");
        a.invisible = read_flag(node, "invsbl");
        a.border = read_flag(node, "border");
        a.clip_op = read_flag(node, "clpop");
        if let Some(clip) = read_set_int(node, "clip") {
            plot.set_clip(clip as i32)?;
        }
        let a = &mut plot.attrs;
        a.draw_title = read_flag(node, "drwttl");
        let flags = |key: &str| per_axis(nax, key, |k| Ok(read_flag(node, k)));
        let doubles = |key: &str| per_axis(nax, key, |k| Ok(read_set_double(node, k)));
        a.label_up = flags("lblup")?;
        a.log_plot = per_axis(
            nax,
            "lgplt",
            |k| Ok(read_set_int(node, k).map(|v| v as i32)),
        )?;
        a.log_ticks = flags("lgtck")?;
        a.log_label = flags("lglbl")?;
        // A Plot written before DrawAxes and Abbrev took an axis gives one
        // value for both axes.
        a.draw_axes = match read_flag(node, "drwaxs") {
            Some(both) => [Some(both), Some(both), None],
            None => flags("drwaxs")?,
        };
        a.abbrev = match read_flag(node, "abbrv") {
            Some(both) => [Some(both), Some(both), None],
            None => flags("abbrv")?,
        };
        a.escape = read_flag(node, "escape");
        a.label_at = doubles("lblat")?;
        a.centre = doubles("cen")?;
        a.gap = doubles("gap")?;
        a.log_gap = doubles("lggap")?;
        a.num_lab_gap = doubles("nmgap")?;
        a.text_lab_gap = doubles("txgap")?;
        a.num_lab = flags("nmlbl")?;
        a.min_tick = per_axis(nax, "mntks", |k| {
            Ok(read_set_int(node, k).map(|v| (v as i32).max(1)))
        })?;
        a.text_lab = flags("txlbl")?;
        a.label_units = flags("lbunt")?;
        for (key, field) in [
            ("style", &mut a.styles.style),
            ("font", &mut a.styles.font),
            ("col", &mut a.styles.colour),
        ] {
            for (id, value) in field.iter_mut().enumerate() {
                *value = read_set_int(node, &format!("{key}{}", id + 1)).map(|v| v as i32);
            }
        }
        type SetReal = fn(&mut Plot, PlotElement, f64) -> AstResult<()>;
        for (key, set) in [
            ("width", Plot::set_width as SetReal),
            ("size", Plot::set_size),
        ] {
            for (id, &element) in PlotElement::ALL[..NPID].iter().enumerate() {
                if let Some(value) = read_set_double(node, &format!("{key}{}", id + 1)) {
                    set(&mut plot, element, value)?;
                }
            }
        }
        let a = &mut plot.attrs;
        a.title_gap = read_set_double(node, "ttlgap");
        // A Plot written before the tick lengths took an axis gives one
        // value for both axes.
        a.maj_tick_len = match read_set_double(node, "mjtkln") {
            Some(both) => [Some(both), Some(both), None],
            None => doubles("mjtkln")?,
        };
        a.min_tick_len = match read_set_double(node, "mntkln") {
            Some(both) => [Some(both), Some(both), None],
            None => doubles("mntkln")?,
        };
        let text = node.get_string("lbling", " ");
        if text != " " {
            a.labelling = Some(find_string(&LABELLINGS, &text, "Lbling")?);
        }
        let text = node.get_string("tgaptyp", " ");
        if text != " " {
            a.text_gap_type = Some(find_string(&TEXT_GAP_TYPES, &text, "TGapTyp")?);
        }
        a.edge = per_axis(nax, "edge", |k| {
            let text = node.get_string(k, " ");
            if text == " " {
                Ok(None)
            } else {
                find_string(&EDGES, &text, "Edge").map(Some)
            }
        })?;

        let clip_axes = node.get_int("clpaxs", 0).max(0) as usize;
        // C: AST__NOFRAME.
        let clip_frame = node.get_int("clpfrm", -99) as i32;
        if clip_axes > 0 {
            let mut lbnd = Vec::with_capacity(clip_axes);
            let mut ubnd = Vec::with_capacity(clip_axes);
            for axis in 1..=clip_axes {
                lbnd.push(node.get_double(&format!("clplb{axis}"), -f64::MAX));
                ubnd.push(node.get_double(&format!("clpub{axis}"), f64::MAX));
            }
            plot.clip = Some(ClipRegion {
                frame: clip_frame,
                lbnd,
                ubnd,
            });
        }
        plot.xlo = node.get_double("xlo", 0.0);
        plot.xhi = node.get_double("xhi", 1.0);
        plot.ylo = node.get_double("ylo", 0.0);
        plot.yhi = node.get_double("yhi", 1.0);
        plot.xrev = node.get_int("xrev", 0) != 0;
        plot.yrev = node.get_int("yrev", 0) != 0;
        for (i, key) in ["xb1", "yb1", "xb2", "yb2"].into_iter().enumerate() {
            plot.bbox[i] = node.get_double(key, BAD_VALUE);
        }
        for (axis, ticks) in plot.tick_values.iter_mut().enumerate() {
            let axis = axis + 1;
            *ticks = TickValues {
                major: (1..=node.get_int(&format!("nmjtk{axis}"), 0).max(0))
                    .map(|i| node.get_double(&format!("mjtk{axis}_{i}"), BAD_VALUE))
                    .collect(),
                minor: (1..=node.get_int(&format!("nmntk{axis}"), 0).max(0))
                    .map(|i| node.get_double(&format!("mntk{axis}_{i}"), BAD_VALUE))
                    .collect(),
            };
        }
        Ok(plot)
    }
}
