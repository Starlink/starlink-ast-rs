// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The graphical elements of a Plot, and the style each is drawn in.
// C: plot.c, the Style, Font, Colour, Width and Size attributes (plot.c:4215
// onwards), the element names GrfLabels (:1717), and the resolution of an
// element group into its members (IdFind, :19545; GetUse*, TestUse*,
// :28808 onwards). C resolves groups only in its string interface; the port
// resolves them for every accessor, with the same rules: setting or
// clearing a group sets or clears each member, a group is set when every
// member is, and a group's value is that of its first set member, else of
// its first member.

use super::Plot;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::Mapping;

/// A kind of graphics a Plot draws, whose style can be set on its own.
///
/// The variants up to [`Grid3`](PlotElement::Grid3) are single elements;
/// the last five name a group of them, one per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlotElement {
    /// The border round the regions of valid coordinates.
    Border,
    /// Curves drawn by the curve-drawing methods.
    Curves,
    /// The title of a grid.
    Title,
    /// Markers.
    Markers,
    /// Text strings drawn by the text-drawing method.
    Strings,
    /// The curve through the tick marks of axis 1.
    Axis1,
    /// The curve through the tick marks of axis 2.
    Axis2,
    /// The curve through the tick marks of axis 3.
    Axis3,
    /// The numerical labels of axis 1.
    NumLab1,
    /// The numerical labels of axis 2.
    NumLab2,
    /// The numerical labels of axis 3.
    NumLab3,
    /// The descriptive label of axis 1.
    TextLab1,
    /// The descriptive label of axis 2.
    TextLab2,
    /// The descriptive label of axis 3.
    TextLab3,
    /// The tick marks of axis 1.
    Ticks1,
    /// The tick marks of axis 2.
    Ticks2,
    /// The tick marks of axis 3.
    Ticks3,
    /// The grid lines of constant axis 1 value.
    Grid1,
    /// The grid lines of constant axis 2 value.
    Grid2,
    /// The grid lines of constant axis 3 value.
    Grid3,
    /// `Axis1` and `Axis2`.
    Axes,
    /// `NumLab1` and `NumLab2`.
    NumLab,
    /// `TextLab1` and `TextLab2`.
    TextLab,
    /// `Grid1` and `Grid2`.
    Grid,
    /// `Ticks1` and `Ticks2`.
    Ticks,
}

/// The number of single elements, which are the first variants.
// C: AST__NPID.
const NPID: usize = 20;

impl PlotElement {
    /// Every element and group, in the order of their C identifiers.
    pub const ALL: [PlotElement; 25] = [
        PlotElement::Border,
        PlotElement::Curves,
        PlotElement::Title,
        PlotElement::Markers,
        PlotElement::Strings,
        PlotElement::Axis1,
        PlotElement::Axis2,
        PlotElement::Axis3,
        PlotElement::NumLab1,
        PlotElement::NumLab2,
        PlotElement::NumLab3,
        PlotElement::TextLab1,
        PlotElement::TextLab2,
        PlotElement::TextLab3,
        PlotElement::Ticks1,
        PlotElement::Ticks2,
        PlotElement::Ticks3,
        PlotElement::Grid1,
        PlotElement::Grid2,
        PlotElement::Grid3,
        PlotElement::Axes,
        PlotElement::NumLab,
        PlotElement::TextLab,
        PlotElement::Grid,
        PlotElement::Ticks,
    ];

    /// The element's name, as the C library spells it in attribute names
    /// such as `Colour(Grid1)`.
    pub fn name(&self) -> &'static str {
        const NAMES: [&str; 25] = [
            "Border", "Curves", "Title", "Markers", "Strings", "Axis1", "Axis2", "Axis3",
            "NumLab1", "NumLab2", "NumLab3", "TextLab1", "TextLab2", "TextLab3", "Ticks1",
            "Ticks2", "Ticks3", "Grid1", "Grid2", "Grid3", "Axes", "NumLab", "TextLab", "Grid",
            "Ticks",
        ];
        NAMES[*self as usize]
    }

    /// The single elements this element stands for in a Plot with `nax`
    /// axes: itself, or a group's members on axes 1 to `nax`.
    // C: IdFind (plot.c:19545).
    fn members(self, nax: usize) -> Vec<usize> {
        let first = match self {
            PlotElement::Axes => PlotElement::Axis1,
            PlotElement::NumLab => PlotElement::NumLab1,
            PlotElement::TextLab => PlotElement::TextLab1,
            PlotElement::Grid => PlotElement::Grid1,
            PlotElement::Ticks => PlotElement::Ticks1,
            single => return vec![single as usize],
        } as usize;
        (0..nax.min(3)).map(|k| first + k).collect()
    }
}

/// The style attributes of each single element, `None` when unset.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ElementStyles {
    pub(crate) style: [Option<i32>; NPID],
    pub(crate) font: [Option<i32>; NPID],
    pub(crate) colour: [Option<i32>; NPID],
    pub(crate) width: [Option<f64>; NPID],
    pub(crate) size: [Option<f64>; NPID],
}

impl Default for ElementStyles {
    fn default() -> Self {
        Self {
            style: [None; NPID],
            font: [None; NPID],
            colour: [None; NPID],
            width: [None; NPID],
            size: [None; NPID],
        }
    }
}

/// An integer style value as C stores it: -1 is "unset".
fn stored_int(value: i32) -> Option<i32> {
    (value != -1).then_some(value)
}

/// A floating-point style value as C stores it: the bad value is "unset".
fn stored_f64(value: f64) -> Option<f64> {
    (value != BAD_VALUE).then_some(value)
}

/// Set, test and clear for a style attribute; its getter, which carries
/// the attribute's documentation, is written out.
macro_rules! style_rest {
    ($get:ident, $set:ident, $test:ident, $clear:ident, $field:ident, i32) => {
        #[doc = concat!("Set [`", stringify!($get), "`](Plot::", stringify!($get), ") for an element, or for each member of a group.")]
        pub fn $set(&mut self, element: PlotElement, value: i32) {
            for id in element.members(Mapping::nin(self)) {
                self.attrs.styles.$field[id] = stored_int(value);
            }
        }
        style_rest!(@tc $get, $test, $clear, $field);
    };
    ($get:ident, $set:ident, $test:ident, $clear:ident, $field:ident, f64, $name:literal, $sep:literal) => {
        #[doc = concat!("Set [`", stringify!($get), "`](Plot::", stringify!($get), ") for an element, or for each member of a group.")]
        #[doc = ""]
        #[doc = "# Errors"]
        #[doc = ""]
        #[doc = "- `ErrorCode::Attin` when `value` is zero."]
        pub fn $set(&mut self, element: PlotElement, value: f64) -> AstResult<()> {
            for id in element.members(Mapping::nin(self)) {
                if value == 0.0 {
                    return Err(AstError::new(
                        ErrorCode::Attin,
                        format!(
                            "astSet{}(Plot):{}Invalid zero value supplied for {}({}) attribute",
                            $name,
                            $sep,
                            $name,
                            PlotElement::ALL[id].name()
                        ),
                    ));
                }
                self.attrs.styles.$field[id] = stored_f64(value);
            }
            Ok(())
        }
        style_rest!(@tc $get, $test, $clear, $field);
    };
    (@tc $get:ident, $test:ident, $clear:ident, $field:ident) => {
        #[doc = concat!("Whether [`", stringify!($get), "`](Plot::", stringify!($get), ") is set for an element, or for every member of a group.")]
        pub fn $test(&self, element: PlotElement) -> bool {
            element
                .members(Mapping::nin(self))
                .iter()
                .all(|&id| self.attrs.styles.$field[id].is_some())
        }
        #[doc = concat!("Clear [`", stringify!($get), "`](Plot::", stringify!($get), ") for an element, or for each member of a group.")]
        pub fn $clear(&mut self, element: PlotElement) {
            for id in element.members(Mapping::nin(self)) {
                self.attrs.styles.$field[id] = None;
            }
        }
    };
}

impl Plot {
    /// The single element whose value a getter reports for `element`: the
    /// element itself, or the first member of a group with `field` set,
    /// else its first member.
    // C: the GetUse* functions (plot.c:28808 onwards).
    fn style_source<T>(&self, element: PlotElement, field: &[Option<T>; NPID]) -> usize {
        let ids = element.members(Mapping::nin(self));
        ids.iter()
            .copied()
            .find(|&id| field[id].is_some())
            .unwrap_or(ids[0])
    }

    /// The line style of an element, as an integer code whose meaning the
    /// graphics backend defines. For a group, the value of its first member
    /// that has one set.
    ///
    /// The default is 1. Setting -1 leaves it unset.
    #[doc(alias = "Style")]
    pub fn style(&self, element: PlotElement) -> i32 {
        let id = self.style_source(element, &self.attrs.styles.style);
        self.attrs.styles.style[id].unwrap_or(1)
    }
    style_rest!(style, set_style, test_style, clear_style, style, i32);

    /// The text font of an element, as an integer code whose meaning the
    /// graphics backend defines. For a group, the value of its first member
    /// that has one set.
    ///
    /// The default is 1. Setting -1 leaves it unset.
    #[doc(alias = "Font")]
    pub fn font(&self, element: PlotElement) -> i32 {
        let id = self.style_source(element, &self.attrs.styles.font);
        self.attrs.styles.font[id].unwrap_or(1)
    }
    style_rest!(font, set_font, test_font, clear_font, font, i32);

    /// The colour of an element, as an index into the graphics backend's
    /// colour table. For a group, the value of its first member that has
    /// one set.
    ///
    /// The default is 1. Setting -1 leaves it unset.
    #[doc(alias = "Colour")]
    pub fn colour(&self, element: PlotElement) -> i32 {
        let id = self.style_source(element, &self.attrs.styles.colour);
        self.attrs.styles.colour[id].unwrap_or(1)
    }
    style_rest!(colour, set_colour, test_colour, clear_colour, colour, i32);

    /// The line width of an element, as a multiple of the graphics
    /// backend's default width. For a group, the value of its first member
    /// that has one set.
    ///
    /// The default is 1.0. Setting `BAD_VALUE` leaves it unset.
    // C's message for a zero Width has no space after the colon; Size's does.
    #[doc(alias = "Width")]
    pub fn width(&self, element: PlotElement) -> f64 {
        let id = self.style_source(element, &self.attrs.styles.width);
        self.attrs.styles.width[id].unwrap_or(1.0)
    }
    style_rest!(
        width,
        set_width,
        test_width,
        clear_width,
        width,
        f64,
        "Width",
        ""
    );

    /// The size of an element's text or markers, as a multiple of the
    /// graphics backend's default size. For a group, the value of its first
    /// member that has one set.
    ///
    /// The default is 1.0. Setting `BAD_VALUE` leaves it unset.
    #[doc(alias = "Size")]
    pub fn size(&self, element: PlotElement) -> f64 {
        let id = self.style_source(element, &self.attrs.styles.size);
        self.attrs.styles.size[id].unwrap_or(1.0)
    }
    style_rest!(
        size, set_size, test_size, clear_size, size, f64, "Size", " "
    );
}
