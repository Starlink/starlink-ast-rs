// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The drawing interface between a Plot and a graphics system.
//!
//! A Plot draws annotated coordinate grids, curves, text and markers by
//! calling the methods of a [`Grf`], which a graphics backend implements.
//! The interface is deliberately small and low level: polylines, markers,
//! text with its bounding box, character height and scale queries, and
//! per-primitive style attributes.
//! Everything a Plot needs to know about the graphics system, such as how
//! large text is and how far apart labels must be, it asks through these
//! methods, so any backend that answers them consistently can be drawn on.
//!
//! Coordinates are in the backend's "graphics" coordinate system, the one a
//! Plot's graphics box is given in, as `f32`.
//! Attribute values are `f64`.
//!
//! Two backends are provided:
//!
//! - [`LogGrf`] draws nothing and writes a line of text for each call, which
//!   is useful for testing a Plot and for seeing what it asks of a backend.
//! - [`SvgGrf`] draws to SVG.
//!
//! Both model the same page: graphics coordinates from 0 to 1 on each axis
//! of a page 200 mm square, with characters 4 mm high at a text size of 1
//! and each character of text 0.6 times its height wide.
// C: `grf.h`, which the trait mirrors one method per function, and
// `grf_log.c`, which the backends reproduce.

mod log;
mod page;
mod svg;

pub use log::LogGrf;
pub use svg::{SvgGrf, SvgOptions};

use crate::error::{AstError, AstResult, ErrorCode};

/// A style attribute of a graphics primitive.
// C: the GRF__STYLE .. GRF__COLOUR codes of grf.h.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrfAttr {
    /// The line style, as an integer code whose meaning the backend defines.
    Style,
    /// The line width, as a multiple of the backend's default width.
    Width,
    /// The size of text or markers, as a multiple of the default size.
    Size,
    /// The text font, as an integer code whose meaning the backend defines.
    Font,
    /// The colour, as an integer index into the backend's colour table.
    Colour,
}

/// A kind of graphics primitive, each of which has its own attributes.
// C: GRF__TEXT, GRF__LINE, GRF__MARK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrfPrimitive {
    /// Text.
    Text,
    /// Lines.
    Line,
    /// Markers.
    Mark,
}

/// A capability a backend may or may not have.
// C: GRF__ESC, GRF__MJUST, GRF__SCALES.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrfCap {
    /// The backend interprets the escape sequences a Plot can embed in text
    /// to change its appearance, such as superscripts.
    Escapes,
    /// The backend supports [`VJust::Bottom`] justification, at the bottom
    /// of the text's bounding box including descenders.
    MJust,
    /// The backend answers [`Grf::scales`].
    Scales,
}

/// Vertical justification of text: which part of the text lies at the
/// reference point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VJust {
    /// The bottom of the text, including descenders (C's code `M`).
    Bottom,
    /// The baseline of the text (`B`).
    Baseline,
    /// The middle of the text (`C`).
    Centre,
    /// The top of the text (`T`).
    Top,
}

/// Horizontal justification of text: which part of the text lies at the
/// reference point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HJust {
    /// The left end of the text (`L`).
    Left,
    /// The middle of the text (`C`).
    Centre,
    /// The right end of the text (`R`).
    Right,
}

/// How text is placed relative to its reference point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Justification {
    /// The vertical part.
    pub vertical: VJust,
    /// The horizontal part.
    pub horizontal: HJust,
}

impl Justification {
    /// A justification from its vertical and horizontal parts.
    pub fn new(vertical: VJust, horizontal: HJust) -> Self {
        Self {
            vertical,
            horizontal,
        }
    }

    /// The justification as the two characters the C library uses: one of
    /// `M`, `B`, `C`, `T`, then one of `L`, `C`, `R`.
    pub fn code(&self) -> [u8; 2] {
        let v = match self.vertical {
            VJust::Bottom => b'M',
            VJust::Baseline => b'B',
            VJust::Centre => b'C',
            VJust::Top => b'T',
        };
        let h = match self.horizontal {
            HJust::Left => b'L',
            HJust::Centre => b'C',
            HJust::Right => b'R',
        };
        [v, h]
    }

    /// The justification a two-character C code names.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when `code` is not one of `M`, `B`, `C`, `T`
    ///   followed by one of `L`, `C`, `R`.
    pub fn from_code(code: &str) -> AstResult<Self> {
        let invalid = || {
            AstError::new(
                ErrorCode::Grfer,
                format!("Illegal text justification \"{code}\" supplied."),
            )
        };
        let &[v, h] = code.as_bytes() else {
            return Err(invalid());
        };
        let vertical = match v {
            b'M' => VJust::Bottom,
            b'B' => VJust::Baseline,
            b'C' => VJust::Centre,
            b'T' => VJust::Top,
            _ => return Err(invalid()),
        };
        let horizontal = match h {
            b'L' => HJust::Left,
            b'C' => HJust::Centre,
            b'R' => HJust::Right,
            _ => return Err(invalid()),
        };
        Ok(Self::new(vertical, horizontal))
    }
}

/// The corners of the box that text occupies, in graphics coordinates:
/// `(x[i], y[i])` is the bottom left corner of the text for `i` 0, then
/// bottom right, top right and top left, as the text reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBox {
    /// The x coordinates of the corners.
    pub x: [f32; 4],
    /// The y coordinates of the corners.
    pub y: [f32; 4],
}

/// A graphics backend, through which a Plot draws.
///
/// Each method corresponds to one function of the C library's graphics
/// interface, and a Plot calls them as the C library does, so a backend
/// written against this trait draws what the C library would draw on the
/// same graphics system.
///
/// A backend holds its own style state: [`attr`](Grf::attr) sets the style
/// of a primitive, and later drawing of that primitive uses it.
/// A Plot sets the styles it needs before each part of a drawing and puts
/// the previous ones back afterwards, through `attr` alone.
///
/// Methods return an error for a failure of the graphics system, such as
/// an output stream that cannot be written, which the Plot passes on.
pub trait Grf {
    /// Draw a polyline through the points `(x[i], y[i])`.
    ///
    /// Nothing is drawn for fewer than two points.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when `x` and `y` differ in length, or the
    ///   graphics system fails.
    fn line(&mut self, x: &[f32], y: &[f32]) -> AstResult<()>;

    /// Draw a marker of type `marker` at each point `(x[i], y[i])`.
    ///
    /// The marker types are the backend's own, conventionally PGPLOT's.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when `x` and `y` differ in length, or the
    ///   graphics system fails.
    fn mark(&mut self, x: &[f32], y: &[f32], marker: i32) -> AstResult<()>;

    /// Draw `text` at the reference point `(x, y)`, justified as `just`
    /// about it, with its up direction along `up`, a vector in graphics
    /// coordinates.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<()>;

    /// The box that [`text`](Grf::text) with the same arguments would
    /// draw the text in, without drawing it.
    ///
    /// The box of empty text has every corner at zero.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when `up` has zero length, or the graphics
    ///   system fails.
    fn text_extent(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<TextBox>;

    /// The height of text in graphics coordinates, as `[along x, along y]`:
    /// the first is the height of text whose up direction is along the
    /// x axis, measured in x, the second that of text whose up direction is
    /// along the y axis, measured in y.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn char_height(&mut self) -> AstResult<[f32; 2]>;

    /// The scales from graphics coordinates to equal physical distances on
    /// the page, as `[alpha, beta]`: a distance `d` in x is `alpha * d`
    /// physically, and one in y is `beta * d`.
    ///
    /// Only the ratio matters, and a negative value means the axis runs
    /// the other way on the page.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn scales(&mut self) -> AstResult<[f32; 2]>;

    /// Query and set the attribute `attr` of the primitive `prim`.
    ///
    /// Returns the value the attribute had, and sets it to `value` unless
    /// `value` is `None` (or `BAD_VALUE`, as in the C library), so one call
    /// can both query and set.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn attr(&mut self, attr: GrfAttr, value: Option<f64>, prim: GrfPrimitive) -> AstResult<f64>;

    /// Whether the backend has the capability `cap`.
    ///
    /// For [`GrfCap::Escapes`], `value` tells a backend that has the
    /// capability whether text it is given will carry escape sequences.
    fn cap(&mut self, cap: GrfCap, value: i32) -> bool;

    /// Start buffering output, so that what follows appears at once.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn begin_buffer(&mut self) -> AstResult<()>;

    /// End buffering started by [`begin_buffer`](Grf::begin_buffer).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn end_buffer(&mut self) -> AstResult<()>;

    /// Bring the output up to date.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the graphics system fails.
    fn flush(&mut self) -> AstResult<()>;
}

/// An I/O error from a backend's output, as a graphics error.
pub(crate) fn io_error(err: std::io::Error) -> AstError {
    AstError::new(
        ErrorCode::Grfer,
        format!("Cannot write graphics output: {err}"),
    )
}

/// Check that a point list has as many x values as y values.
pub(crate) fn check_points(method: &str, x: &[f32], y: &[f32]) -> AstResult<()> {
    if x.len() == y.len() {
        return Ok(());
    }
    Err(AstError::new(
        ErrorCode::Grfer,
        format!(
            "{method}: {} x values were given with {} y values.",
            x.len(),
            y.len()
        ),
    ))
}
