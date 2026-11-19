// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Plot's own attributes: those that control how a coordinate grid is drawn
//! and annotated.
// C: plot.c, the Get/Set/Test/Clear functions each attribute's astMAKE_*
// and MAKE_* macros generate (plot.c:2407-4168), and the hand-written
// GetLabelUnits (:14638), SetLogPlot (:25507), ClearLogPlot (:6965) and
// ToggleLogLin (:29493). An unset value is `None` where C stores -1, -9999
// or AST__BAD; setting the value C uses for "unset" (AST__BAD for a
// floating-point attribute) leaves the attribute unset, as in C.

use super::Plot;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{FrameRef, SkyFrame, SkySystem};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, MathMap, UnitMap, WinMap};

/// An edge of the plotting area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    /// The left edge.
    Left,
    /// The top edge.
    Top,
    /// The right edge.
    Right,
    /// The bottom edge.
    Bottom,
}

impl Edge {
    /// The edge's name in a saved Plot: `left`, `top`, `right` or `bottom`.
    pub fn name(&self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Top => "top",
            Edge::Right => "right",
            Edge::Bottom => "bottom",
        }
    }
}

/// Where a Plot puts the numerical labels of a coordinate grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Labelling {
    /// Round the edges of the plotting area, where possible.
    Exterior,
    /// Along curves inside the plotting area.
    Interior,
}

impl Labelling {
    /// The value's name in a saved Plot: `exterior` or `interior`.
    pub fn name(&self) -> &'static str {
        match self {
            Labelling::Exterior => "exterior",
            Labelling::Interior => "interior",
        }
    }
}

/// What the gaps between the axis labels and title of a grid are measured
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextGapType {
    /// From the edge of the box enclosing the numerical labels.
    Box,
    /// From the edge of the plotting area.
    Plot,
}

impl TextGapType {
    /// The value's name in a saved Plot: `box` or `plot`.
    pub fn name(&self) -> &'static str {
        match self {
            TextGapType::Box => "box",
            TextGapType::Plot => "plot",
        }
    }
}

/// The storage of Plot's own attributes, `None` when unset.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlotAttrs {
    pub(crate) tol: Option<f64>,
    pub(crate) grid: Option<bool>,
    pub(crate) tick_all: Option<bool>,
    pub(crate) force_exterior: Option<bool>,
    pub(crate) invisible: Option<bool>,
    pub(crate) border: Option<bool>,
    pub(crate) clip: Option<i32>,
    pub(crate) clip_op: Option<bool>,
    pub(crate) draw_title: Option<bool>,
    pub(crate) escape: Option<bool>,
    pub(crate) title_gap: Option<f64>,
    pub(crate) text_gap_type: Option<TextGapType>,
    pub(crate) labelling: Option<Labelling>,
    pub(crate) draw_axes: [Option<bool>; 3],
    pub(crate) abbrev: [Option<bool>; 3],
    pub(crate) label_up: [Option<bool>; 3],
    pub(crate) num_lab: [Option<bool>; 3],
    pub(crate) text_lab: [Option<bool>; 3],
    pub(crate) label_units: [Option<bool>; 3],
    pub(crate) log_plot: [Option<bool>; 3],
    pub(crate) log_ticks: [Option<bool>; 3],
    pub(crate) log_label: [Option<bool>; 3],
    pub(crate) label_at: [Option<f64>; 3],
    pub(crate) centre: [Option<f64>; 3],
    pub(crate) gap: [Option<f64>; 3],
    pub(crate) log_gap: [Option<f64>; 3],
    pub(crate) num_lab_gap: [Option<f64>; 3],
    pub(crate) text_lab_gap: [Option<f64>; 3],
    pub(crate) maj_tick_len: [Option<f64>; 3],
    pub(crate) min_tick_len: [Option<f64>; 3],
    pub(crate) min_tick: [Option<i32>; 3],
    pub(crate) edge: [Option<Edge>; 3],
}

/// MinTick as C stores it: a value below 1 is 1.
fn at_least_one(value: i32) -> Option<i32> {
    Some(value.max(1))
}

/// A floating-point attribute value as C stores it: the bad value is
/// "unset".
fn stored(value: f64) -> Option<f64> {
    (value != BAD_VALUE).then_some(value)
}

/// Set, test and clear for a Plot attribute that is one flag; its getter,
/// which carries the attribute's documentation, is written out.
macro_rules! flag_attr_rest {
    ($get:ident, $set:ident, $test:ident, $clear:ident, $field:ident) => {
        #[doc = concat!("Set [`", stringify!($get), "`](Plot::", stringify!($get), ").")]
        pub fn $set(&mut self, value: bool) {
            self.attrs.$field = Some(value);
        }
        #[doc = concat!("Whether [`", stringify!($get), "`](Plot::", stringify!($get), ") is set.")]
        pub fn $test(&self) -> bool {
            self.attrs.$field.is_some()
        }
        #[doc = concat!("Clear [`", stringify!($get), "`](Plot::", stringify!($get), ").")]
        pub fn $clear(&mut self) {
            self.attrs.$field = None;
        }
    };
}

/// Set, test and clear for a per-axis attribute; its getter, which carries
/// the attribute's documentation, is written out.
macro_rules! axis_attr_rest {
    ($get:ident, $set:ident, $test:ident, $clear:ident, $field:ident, $ty:ty, $name:literal) => {
        axis_attr_rest!($get, $set, $test, $clear, $field, $ty, $name, Some);
    };
    ($get:ident, $set:ident, $test:ident, $clear:ident, $field:ident, $ty:ty, $name:literal,
     $store:expr) => {
        #[doc = concat!("Set [`", stringify!($get), "`](Plot::", stringify!($get), ").")]
        #[doc = ""]
        #[doc = "# Errors"]
        #[doc = ""]
        #[doc = "- `ErrorCode::Axiin` when `axis` is not 1 or 2."]
        pub fn $set(&mut self, axis: usize, value: $ty) -> AstResult<()> {
            let i = self.attr_axis("Set", $name, axis)?;
            let store: fn($ty) -> Option<$ty> = $store;
            self.attrs.$field[i] = store(value);
            Ok(())
        }
        #[doc = concat!("Whether [`", stringify!($get), "`](Plot::", stringify!($get), ") is set.")]
        #[doc = ""]
        #[doc = "# Errors"]
        #[doc = ""]
        #[doc = "- `ErrorCode::Axiin` when `axis` is not 1 or 2."]
        pub fn $test(&self, axis: usize) -> AstResult<bool> {
            let i = self.attr_axis("Test", $name, axis)?;
            Ok(self.attrs.$field[i].is_some())
        }
        #[doc = concat!("Clear [`", stringify!($get), "`](Plot::", stringify!($get), ").")]
        #[doc = ""]
        #[doc = "# Errors"]
        #[doc = ""]
        #[doc = "- `ErrorCode::Axiin` when `axis` is not 1 or 2."]
        pub fn $clear(&mut self, axis: usize) -> AstResult<()> {
            let i = self.attr_axis("Clear", $name, axis)?;
            self.attrs.$field[i] = None;
            Ok(())
        }
    };
}

impl Plot {
    /// The 0-based index of a 1-based axis of a per-axis attribute.
    // C: the axis check of the MAKE_GET etc. macros (plot.c:929), against
    // astGetNin, which is 2 for a Plot.
    fn attr_axis(&self, op: &str, name: &str, axis: usize) -> AstResult<usize> {
        let nin = Mapping::nin(self);
        if axis < 1 || axis > nin {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "ast{op}{name}(Plot): Index ({axis}) is invalid for attribute {name} - it \
                     should be in the range 1 to {nin}."
                ),
            ));
        }
        Ok(axis - 1)
    }

    /// The plotting tolerance: the largest distance, as a fraction of the
    /// diagonal of the plotting area, by which a drawn curve may depart
    /// from the true one. A value outside 1.0e-7 to 1.0 is clamped into
    /// that range.
    ///
    /// The default is 0.01.
    #[doc(alias = "Tol")]
    pub fn tol(&self) -> f64 {
        self.attrs.tol.unwrap_or(0.01)
    }

    /// Set [`tol`](Plot::tol).
    pub fn set_tol(&mut self, value: f64) {
        self.attrs.tol = Some(value.clamp(1.0e-7, 1.0));
    }

    /// Whether [`tol`](Plot::tol) is set.
    pub fn test_tol(&self) -> bool {
        self.attrs.tol.is_some()
    }

    /// Clear [`tol`](Plot::tol).
    pub fn clear_tol(&mut self) {
        self.attrs.tol = None;
    }

    /// Whether a grid drawn by the Plot covers the plotting area with curves of constant coordinate value, rather than marking only its edges with ticks.
    ///
    /// The default is `false`.
    #[doc(alias = "Grid")]
    pub fn grid(&self) -> bool {
        self.attrs.grid.unwrap_or(false)
    }
    flag_attr_rest!(grid, set_grid, test_grid, clear_grid, grid);
    /// Whether tick marks are drawn on all edges of the plotting area, rather than only the labelled ones.
    ///
    /// The default is `true`.
    #[doc(alias = "TickAll")]
    pub fn tick_all(&self) -> bool {
        self.attrs.tick_all.unwrap_or(true)
    }
    flag_attr_rest!(
        tick_all,
        set_tick_all,
        test_tick_all,
        clear_tick_all,
        tick_all
    );
    /// Whether exterior labelling is used even where the Plot judges it unsuitable.
    ///
    /// The default is `false`.
    pub fn force_exterior(&self) -> bool {
        self.attrs.force_exterior.unwrap_or(false)
    }
    flag_attr_rest!(
        force_exterior,
        set_force_exterior,
        test_force_exterior,
        clear_force_exterior,
        force_exterior
    );
    /// Whether graphics are drawn in invisible ink, so that only their extent, as reported by a bounding box, is found.
    ///
    /// The default is `false`.
    #[doc(alias = "Invisible")]
    pub fn invisible(&self) -> bool {
        self.attrs.invisible.unwrap_or(false)
    }
    flag_attr_rest!(
        invisible,
        set_invisible,
        test_invisible,
        clear_invisible,
        invisible
    );
    /// Whether a grid is drawn with a border round the regions of valid coordinates.
    ///
    /// The default is `true`.
    #[doc(alias = "Border")]
    pub fn border(&self) -> bool {
        self.attrs.border.unwrap_or(true)
    }
    flag_attr_rest!(border, set_border, test_border, clear_border, border);
    /// Whether a point is kept by the clipping region when any one of its axes is within bounds, rather than all of them.
    ///
    /// The default is `false`.
    #[doc(alias = "ClipOp")]
    pub fn clip_op(&self) -> bool {
        self.attrs.clip_op.unwrap_or(false)
    }
    flag_attr_rest!(clip_op, set_clip_op, test_clip_op, clear_clip_op, clip_op);
    /// Whether a grid is drawn with the current Frame's title.
    ///
    /// The default is `true`.
    #[doc(alias = "DrawTitle")]
    pub fn draw_title(&self) -> bool {
        self.attrs.draw_title.unwrap_or(true)
    }
    flag_attr_rest!(
        draw_title,
        set_draw_title,
        test_draw_title,
        clear_draw_title,
        draw_title
    );
    /// Whether escape sequences in text change its appearance, rather than being drawn as text.
    ///
    /// The default is `true`.
    #[doc(alias = "Escape")]
    pub fn escape(&self) -> bool {
        self.attrs.escape.unwrap_or(true)
    }
    flag_attr_rest!(escape, set_escape, test_escape, clear_escape, escape);

    /// Which graphics are clipped at the edges of the plotting area, as the
    /// sum of 1 for curves and 2 for markers.
    ///
    /// The default is 1.
    #[doc(alias = "Clip")]
    pub fn clip(&self) -> i32 {
        self.attrs.clip.unwrap_or(1)
    }

    /// Set [`clip`](Plot::clip).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` when `value` is not between 0 and 3.
    pub fn set_clip(&mut self, value: i32) -> AstResult<()> {
        if !(0..=3).contains(&value) {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!("astSetClip(Plot): Invalid value {value} supplied for Clip attribute"),
            ));
        }
        self.attrs.clip = Some(value);
        Ok(())
    }

    /// Whether [`clip`](Plot::clip) is set.
    pub fn test_clip(&self) -> bool {
        self.attrs.clip.is_some()
    }

    /// Clear [`clip`](Plot::clip).
    pub fn clear_clip(&mut self) {
        self.attrs.clip = None;
    }

    /// The gap between the top of a grid and its title, as a fraction of
    /// the smaller dimension of the plotting area.
    ///
    /// The default is 0.05.
    #[doc(alias = "TitleGap")]
    pub fn title_gap(&self) -> f64 {
        self.attrs.title_gap.unwrap_or(0.05)
    }

    /// Set [`title_gap`](Plot::title_gap).
    pub fn set_title_gap(&mut self, value: f64) {
        self.attrs.title_gap = stored(value);
    }

    /// Whether [`title_gap`](Plot::title_gap) is set.
    pub fn test_title_gap(&self) -> bool {
        self.attrs.title_gap.is_some()
    }

    /// Clear [`title_gap`](Plot::title_gap).
    pub fn clear_title_gap(&mut self) {
        self.attrs.title_gap = None;
    }

    /// What the gaps between a grid's labels and title are measured from.
    ///
    /// The default is [`TextGapType::Box`].
    #[doc(alias = "TextGapType")]
    pub fn text_gap_type(&self) -> TextGapType {
        self.attrs.text_gap_type.unwrap_or(TextGapType::Box)
    }

    /// Set [`text_gap_type`](Plot::text_gap_type).
    pub fn set_text_gap_type(&mut self, value: TextGapType) {
        self.attrs.text_gap_type = Some(value);
    }

    /// Whether [`text_gap_type`](Plot::text_gap_type) is set.
    pub fn test_text_gap_type(&self) -> bool {
        self.attrs.text_gap_type.is_some()
    }

    /// Clear [`text_gap_type`](Plot::text_gap_type).
    pub fn clear_text_gap_type(&mut self) {
        self.attrs.text_gap_type = None;
    }

    /// Where a grid's numerical labels are put.
    ///
    /// The default is [`Labelling::Exterior`].
    #[doc(alias = "Labelling")]
    pub fn labelling(&self) -> Labelling {
        self.attrs.labelling.unwrap_or(Labelling::Exterior)
    }

    /// Set [`labelling`](Plot::labelling).
    pub fn set_labelling(&mut self, value: Labelling) {
        self.attrs.labelling = Some(value);
    }

    /// Whether [`labelling`](Plot::labelling) is set.
    pub fn test_labelling(&self) -> bool {
        self.attrs.labelling.is_some()
    }

    /// Clear [`labelling`](Plot::labelling).
    pub fn clear_labelling(&mut self) {
        self.attrs.labelling = None;
    }

    /// Whether a curve is drawn through the tick marks of an axis when they are inside the plotting area.
    ///
    /// The default is `true`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "DrawAxes")]
    pub fn draw_axes(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "DrawAxes", axis)?;
        Ok(self.attrs.draw_axes[i].unwrap_or(true))
    }
    axis_attr_rest!(
        draw_axes,
        set_draw_axes,
        test_draw_axes,
        clear_draw_axes,
        draw_axes,
        bool,
        "DrawAxes",
        Some
    );
    /// Whether the numerical labels of an axis leave out the leading fields they share with their neighbours.
    ///
    /// The default is `true`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "Abbrev")]
    pub fn abbreviate(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "Abbrev", axis)?;
        Ok(self.attrs.abbrev[i].unwrap_or(true))
    }
    axis_attr_rest!(
        abbreviate,
        set_abbreviate,
        test_abbreviate,
        clear_abbreviate,
        abbrev,
        bool,
        "Abbrev",
        Some
    );
    /// Whether the numerical labels of an axis are drawn upright, rather than along the axis.
    ///
    /// The default is `false`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LabelUp")]
    pub fn label_up(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "LabelUp", axis)?;
        Ok(self.attrs.label_up[i].unwrap_or(false))
    }
    axis_attr_rest!(
        label_up,
        set_label_up,
        test_label_up,
        clear_label_up,
        label_up,
        bool,
        "LabelUp",
        Some
    );
    /// Whether an axis has numerical labels.
    ///
    /// The default is `true`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "NumLab")]
    pub fn num_lab(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "NumLab", axis)?;
        Ok(self.attrs.num_lab[i].unwrap_or(true))
    }
    axis_attr_rest!(
        num_lab,
        set_num_lab,
        test_num_lab,
        clear_num_lab,
        num_lab,
        bool,
        "NumLab",
        Some
    );
    /// Whether an axis has a descriptive label.
    ///
    /// The default is `true`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "TextLab")]
    pub fn text_lab(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "TextLab", axis)?;
        Ok(self.attrs.text_lab[i].unwrap_or(true))
    }
    axis_attr_rest!(
        text_lab,
        set_text_lab,
        test_text_lab,
        clear_text_lab,
        text_lab,
        bool,
        "TextLab",
        Some
    );
    /// The value on the other axis at which an axis's numerical labels are put when labelling is interior, `BAD_VALUE` meaning the Plot chooses.
    ///
    /// The default is `BAD_VALUE`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LabelAt")]
    pub fn label_at(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "LabelAt", axis)?;
        Ok(self.attrs.label_at[i].unwrap_or(BAD_VALUE))
    }
    axis_attr_rest!(
        label_at,
        set_label_at,
        test_label_at,
        clear_label_at,
        label_at,
        f64,
        "LabelAt",
        stored
    );
    /// A value on an axis at which a major tick mark is put, all others being a whole number of gaps from it; `BAD_VALUE` means the Plot chooses.
    ///
    /// The default is `BAD_VALUE`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    pub fn tick_centre(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "Centre", axis)?;
        Ok(self.attrs.centre[i].unwrap_or(BAD_VALUE))
    }
    axis_attr_rest!(
        tick_centre,
        set_tick_centre,
        test_tick_centre,
        clear_tick_centre,
        centre,
        f64,
        "Centre",
        stored
    );
    /// The gap between major tick marks on an axis, in the axis's coordinates; `BAD_VALUE` means the Plot chooses.
    ///
    /// The default is `BAD_VALUE`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "Gap")]
    pub fn tick_gap(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "Gap", axis)?;
        Ok(self.attrs.gap[i].unwrap_or(BAD_VALUE))
    }
    axis_attr_rest!(
        tick_gap,
        set_tick_gap,
        test_tick_gap,
        clear_tick_gap,
        gap,
        f64,
        "Gap",
        stored
    );
    /// The ratio between major tick marks on an axis whose ticks are spaced logarithmically; `BAD_VALUE` means the Plot chooses.
    ///
    /// The default is `BAD_VALUE`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LogGap")]
    pub fn log_gap(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "LogGap", axis)?;
        Ok(self.attrs.log_gap[i].unwrap_or(BAD_VALUE))
    }
    axis_attr_rest!(
        log_gap,
        set_log_gap,
        test_log_gap,
        clear_log_gap,
        log_gap,
        f64,
        "LogGap",
        stored
    );
    /// The gap between an axis's numerical labels and the edge or curve they label, as a fraction of the smaller dimension of the plotting area.
    ///
    /// The default is `0.01`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "NumLabGap")]
    pub fn num_lab_gap(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "NumLabGap", axis)?;
        Ok(self.attrs.num_lab_gap[i].unwrap_or(0.01))
    }
    axis_attr_rest!(
        num_lab_gap,
        set_num_lab_gap,
        test_num_lab_gap,
        clear_num_lab_gap,
        num_lab_gap,
        f64,
        "NumLabGap",
        stored
    );
    /// The length of an axis's major tick marks, as a fraction of the smaller dimension of the plotting area; a negative length draws them outwards.
    ///
    /// The default is `0.015`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "MajTickLen")]
    pub fn maj_tick_len(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "MajTickLen", axis)?;
        Ok(self.attrs.maj_tick_len[i].unwrap_or(0.015))
    }
    axis_attr_rest!(
        maj_tick_len,
        set_maj_tick_len,
        test_maj_tick_len,
        clear_maj_tick_len,
        maj_tick_len,
        f64,
        "MajTickLen",
        stored
    );
    /// The length of an axis's minor tick marks, as a fraction of the smaller dimension of the plotting area; a negative length draws them outwards.
    ///
    /// The default is `0.007`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "MinTickLen")]
    pub fn min_tick_len(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "MinTickLen", axis)?;
        Ok(self.attrs.min_tick_len[i].unwrap_or(0.007))
    }
    axis_attr_rest!(
        min_tick_len,
        set_min_tick_len,
        test_min_tick_len,
        clear_min_tick_len,
        min_tick_len,
        f64,
        "MinTickLen",
        stored
    );
    /// The number of divisions between an axis's major tick marks, one fewer minor tick marks being drawn; a value below 1 is taken as 1.
    ///
    /// The default is `1`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "MinTick")]
    pub fn min_tick(&self, axis: usize) -> AstResult<i32> {
        let i = self.attr_axis("Get", "MinTick", axis)?;
        Ok(self.attrs.min_tick[i].unwrap_or(1))
    }
    axis_attr_rest!(
        min_tick,
        set_min_tick,
        test_min_tick,
        clear_min_tick,
        min_tick,
        i32,
        "MinTick",
        at_least_one
    );

    /// The edge of the plotting area on which an axis's labels are put
    /// when labelling is exterior.
    ///
    /// The default is [`Edge::Bottom`] for axis 1 and [`Edge::Left`] for
    /// axis 2.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "Edge")]
    pub fn edge(&self, axis: usize) -> AstResult<Edge> {
        let i = self.attr_axis("Get", "Edge", axis)?;
        Ok(self.attrs.edge[i].unwrap_or(if i == 0 { Edge::Bottom } else { Edge::Left }))
    }
    axis_attr_rest!(edge, set_edge, test_edge, clear_edge, edge, Edge, "Edge");

    /// The gap between an axis's descriptive label and the edge of the
    /// plotting area or of the box enclosing the numerical labels, as a
    /// fraction of the smaller dimension of the plotting area.
    ///
    /// The default is 0.07 when [`text_gap_type`](Plot::text_gap_type) is
    /// [`TextGapType::Plot`], and 0.01 otherwise.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "TextLabGap")]
    pub fn text_lab_gap(&self, axis: usize) -> AstResult<f64> {
        let i = self.attr_axis("Get", "TextLabGap", axis)?;
        Ok(
            self.attrs.text_lab_gap[i].unwrap_or(match self.text_gap_type() {
                TextGapType::Plot => 0.07,
                TextGapType::Box => 0.01,
            }),
        )
    }

    /// Set [`text_lab_gap`](Plot::text_lab_gap).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    pub fn set_text_lab_gap(&mut self, axis: usize, value: f64) -> AstResult<()> {
        let i = self.attr_axis("Set", "TextLabGap", axis)?;
        self.attrs.text_lab_gap[i] = stored(value);
        Ok(())
    }

    /// Whether [`text_lab_gap`](Plot::text_lab_gap) is set.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    pub fn test_text_lab_gap(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Test", "TextLabGap", axis)?;
        Ok(self.attrs.text_lab_gap[i].is_some())
    }

    /// Clear [`text_lab_gap`](Plot::text_lab_gap).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    pub fn clear_text_lab_gap(&mut self, axis: usize) -> AstResult<()> {
        let i = self.attr_axis("Clear", "TextLabGap", axis)?;
        self.attrs.text_lab_gap[i] = None;
        Ok(())
    }

    /// Whether an axis's descriptive label includes its units.
    ///
    /// The default is `false` for an axis of a SkyFrame in an ICRS, FK4,
    /// FK5, apparent geocentric, ecliptic, galactic or supergalactic system,
    /// whose units the numerical labels show, and `true` otherwise.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LabelUnits")]
    // C: GetLabelUnits (plot.c:14638).
    pub fn label_units(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "LabelUnits", axis)?;
        if let Some(v) = self.attrs.label_units[i] {
            return Ok(v);
        }
        let current = self.frameset().current();
        let (prim, _) = current.primary_frame(i);
        let celestial = prim.as_any().downcast_ref::<SkyFrame>().is_some_and(|s| {
            matches!(
                s.system(),
                SkySystem::Icrs
                    | SkySystem::Fk4
                    | SkySystem::Fk4NoE
                    | SkySystem::Fk5
                    | SkySystem::Gappt
                    | SkySystem::Ecliptic
                    | SkySystem::Galactic
                    | SkySystem::Supergalactic
            )
        });
        Ok(!celestial)
    }
    axis_attr_rest!(
        label_units,
        set_label_units,
        test_label_units,
        clear_label_units,
        label_units,
        bool,
        "LabelUnits"
    );

    /// Whether the tick marks of an axis are spaced logarithmically.
    ///
    /// The default is [`log_plot`](Plot::log_plot) for the axis.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LogTicks")]
    pub fn log_ticks(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "LogTicks", axis)?;
        match self.attrs.log_ticks[i] {
            Some(v) => Ok(v),
            None => self.log_plot(axis),
        }
    }
    axis_attr_rest!(
        log_ticks,
        set_log_ticks,
        test_log_ticks,
        clear_log_ticks,
        log_ticks,
        bool,
        "LogTicks"
    );

    /// Whether the numerical labels of an axis are in the form `10^x`.
    ///
    /// The default is [`log_ticks`](Plot::log_ticks) for the axis.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LogLabel")]
    pub fn log_label(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "LogLabel", axis)?;
        match self.attrs.log_label[i] {
            Some(v) => Ok(v),
            None => self.log_ticks(axis),
        }
    }
    axis_attr_rest!(
        log_label,
        set_log_label,
        test_log_label,
        clear_log_label,
        log_label,
        bool,
        "LogLabel"
    );

    /// Whether an axis of the plotting area is mapped logarithmically onto
    /// the plot box, rather than linearly.
    ///
    /// Setting it changes the Mapping from graphics coordinates to the
    /// base Frame of the Frame or FrameSet the Plot was built from. It can
    /// only be set when the plot box's range on the axis does not include
    /// zero; otherwise setting it has no effect and leaves it unset.
    ///
    /// The default is `false`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "LogPlot")]
    pub fn log_plot(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Get", "LogPlot", axis)?;
        Ok(self.attrs.log_plot[i].unwrap_or(false))
    }

    /// Set [`log_plot`](Plot::log_plot), remapping the axis.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    /// - Any error from building or applying the new Mapping.
    // C: SetLogPlot (plot.c:25507).
    pub fn set_log_plot(&mut self, axis: usize, value: bool) -> AstResult<()> {
        let i = self.log_plot_axis("Set", axis)?;
        let oldval = self.log_plot(axis)?;
        if value != oldval {
            if self.toggle_log_lin(axis, oldval)? {
                self.attrs.log_plot[i] = Some(!oldval);
            }
        } else {
            self.attrs.log_plot[i] = Some(oldval);
        }
        Ok(())
    }

    /// Whether [`log_plot`](Plot::log_plot) is set.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    pub fn test_log_plot(&self, axis: usize) -> AstResult<bool> {
        let i = self.attr_axis("Test", "LogPlot", axis)?;
        Ok(self.attrs.log_plot[i].is_some())
    }

    /// Clear [`log_plot`](Plot::log_plot), remapping the axis if its value
    /// changes.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    /// - Any error from building or applying the new Mapping.
    // C: ClearLogPlot (plot.c:6965).
    pub fn clear_log_plot(&mut self, axis: usize) -> AstResult<()> {
        let i = self.log_plot_axis("Clear", axis)?;
        let Some(oldval) = self.attrs.log_plot[i] else {
            return Ok(());
        };
        self.attrs.log_plot[i] = None;
        let newval = self.log_plot(axis)?;
        if oldval != newval && !self.toggle_log_lin(axis, oldval)? {
            self.attrs.log_plot[i] = Some(oldval);
        }
        Ok(())
    }

    /// The 0-based index of an axis of LogPlot, which C checks against 2
    /// rather than the number of inputs.
    fn log_plot_axis(&self, op: &str, axis: usize) -> AstResult<usize> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "ast{op}LogPlot(Plot): Index ({axis}) is invalid for attribute LogPlot - it \
                     should be in the range 1 to 2."
                ),
            ));
        }
        Ok(axis - 1)
    }

    /// Remap the graphics Frame so that `axis` becomes linearly related to
    /// the plot box if `islog`, or logarithmically if not. Returns whether
    /// it could: not when the plot box's range on the axis includes zero.
    // C: ToggleLogLin (plot.c:29493).
    fn toggle_log_lin(&mut self, axis: usize, islog: bool) -> AstResult<bool> {
        let [xlo, ylo, xhi, yhi] = self.graphics_box();
        let [xrev, yrev] = self.reversed();
        let bbox = self.plot_box();
        let (g1, g2, b1, b2) = if axis == 1 {
            let (g1, g2) = if xrev { (xhi, xlo) } else { (xlo, xhi) };
            (g1, g2, bbox[0], bbox[2])
        } else {
            let (g1, g2) = if yrev { (yhi, ylo) } else { (ylo, yhi) };
            (g1, g2, bbox[1], bbox[3])
        };
        if !(b1 != BAD_VALUE && b2 != BAD_VALUE && g1 != g2 && b1 != b2 && b1 * b2 > 0.0) {
            return Ok(false);
        }
        let mut linmap = WinMap::from_corners(&[g1], &[g2], &[b1], &[b2])?;
        let log10 = crate::math::log10;
        let g17 = |v: f64| crate::frames::axis_format::sprintf_double("%.17g", v);
        let c = log10(b1 / b2);
        let a = (g1 - g2) / c;
        let (fwd, inv) = if b1 > 0.0 {
            let b = (g2 * log10(b1) - g1 * log10(b2)) / c;
            (
                format!("b=pow(10,(g-{})/{})", g17(b), g17(a)),
                format!("g={}*log10(b)+{}", g17(a), g17(b)),
            )
        } else {
            let b = (g2 * log10(-b1) - g1 * log10(-b2)) / c;
            (
                format!("b=-pow(10,(g-{})/{})", g17(b), g17(a)),
                format!("g={}*log10(-b)+{}", g17(a), g17(b)),
            )
        };
        let mut logmap = MathMap::new(1, 1, &[fwd.as_str()], &[inv.as_str()])?;
        logmap.set_simp_fi(true);
        logmap.set_simp_if(true);
        let remap1: DynMapping = if islog {
            linmap.invert();
            Box::new(CmpMap::new(
                Box::new(logmap),
                Box::new(linmap),
                Composition::Series,
            )?)
        } else {
            logmap.invert();
            Box::new(CmpMap::new(
                Box::new(linmap),
                Box::new(logmap),
                Composition::Series,
            )?)
        };
        let unit: DynMapping = Box::new(UnitMap::new(1));
        let remap2 = if axis == 1 {
            CmpMap::new(remap1, unit, Composition::Parallel)?
        } else {
            CmpMap::new(unit, remap1, Composition::Parallel)?
        };
        self.frameset_mut()
            .remap_frame(FrameRef::Base, Box::new(remap2))?;
        Ok(true)
    }
}
