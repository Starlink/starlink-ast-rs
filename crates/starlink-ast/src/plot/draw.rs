// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Drawing a Plot through a graphics backend.
//!
//! The [`Draw`] trait holds the methods that draw: each takes the
//! [`Grf`] to draw through, so a Plot holds its configuration and not its
//! output. [`Plot`] implements it, and a 3-D Plot will implement it again by
//! overriding the methods it draws differently.
// C: the vtab entries plot3d.c overrides (Border, BoundingBox, Clip, Curve,
// GenCurve, Grid, GridLine, Mark, PolyCurve, SetTickValues, Text), which are
// Plot's extension points (2026-10-30-plot-design.md, "Plot's extension
// points"). Where one of them calls another through the vtab, the port calls
// the trait method.
//
// C keeps the state of a drawing in thread-static globals. The port keeps
// them in a `DrawState` held by each Plot, so that a drawing method called
// from inside another (Grid calls astBorder) shares them as in C:
//
// - the cached character heights and axis scales that RESET_GRF clears
//   (Grf_chh, Grf_chv, Grf_alpha, Grf_beta), cleared at the start of each
//   public drawing method as C clears them;
// - the polyline buffer (Poly_x, Poly_y, Poly_xp, Poly_yp, Poly_np), which
//   Fpoly empties before each drawing method returns;
// - the bounding box of what a grid draws (Box_lbnd, Box_ubnd) and of what
//   the last drawing method drew (Boxp_lbnd, Boxp_ubnd, Boxp_freeze);
// - the nesting level and saved values of astGrfAttrs;
// - the curve tracer's globals (Crv_*), and the description of the last
//   curve drawn (Curve_data).
//
// The parameters C's Map1 to Map5 read from globals are passed to the curve
// tracer as a `CurveMap`, and Crv's static structure, shared between its
// recursive calls, is a `CrvShared`. C's globals are shared by every Plot in
// a thread; the port's belong to one Plot, so a bounding box is that of the
// last drawing on the same Plot.

use super::Plot;
use super::curve::CurveData;
use super::elements::PlotElement;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::grf::{Grf, GrfAttr, GrfCap, GrfPrimitive, Justification};
use crate::serial::native_write::format_double_g;

/// The methods that draw a Plot through a graphics backend.
pub trait Draw {
    /// Draw a curve along which one axis of the current Frame varies and
    /// the others stay at the values of `start`.
    ///
    /// The curve starts at `start`, a position in the current Frame, and
    /// runs a distance `length` along the 1-based `axis`, which may be
    /// negative to go towards smaller values. It is drawn in the style of
    /// the grid lines of the other axis (so a curve along axis 1 is drawn
    /// as [`Grid2`](crate::plot::PlotElement::Grid2) is), and clipped as
    /// the Plot's [`clip`](Plot::clip) attribute says.
    ///
    /// Nothing is drawn if any value of `start`, or `length`, is bad.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    /// - `ErrorCode::Axiin` when `axis` is not an axis of the current
    ///   Frame.
    /// - `ErrorCode::Cvbrk` when the curve has more than 1000 breaks.
    /// - Any error the backend reports.
    #[doc(alias = "astGridLine")]
    fn draw_grid_line(
        &mut self,
        grf: &mut dyn Grf,
        axis: usize,
        start: &[f64],
        length: f64,
    ) -> AstResult<()>;

    /// Draw the border of the regions of the plotting area where the
    /// current Frame's coordinates are valid, in the style of
    /// [`Border`](crate::plot::PlotElement::Border).
    ///
    /// The border follows the edges of the plotting area where coordinates
    /// are valid there, and the boundary between valid and invalid
    /// coordinates inside it, traced to within the Plot's
    /// [`tol`](Plot::tol). Returns whether any invalid coordinates were
    /// found.
    ///
    /// A current Frame of more than two axes is drawn through its first
    /// two.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the base Frame, or the current Frame, has
    ///   fewer or more than two axes.
    /// - `ErrorCode::Inter` when the current Frame is a Region, whose
    ///   border cannot be drawn yet.
    /// - `ErrorCode::Cvbrk` when an edge has more than 1000 breaks.
    /// - Any error the backend reports.
    #[doc(alias = "astBorder")]
    fn draw_border(&mut self, grf: &mut dyn Grf) -> AstResult<bool>;

    /// Draw `text` at `pos`, a position in the current Frame, justified
    /// about it by `just`, with its up direction along `up`, a vector in
    /// graphics coordinates, in the style of
    /// [`Strings`](crate::plot::PlotElement::Strings).
    ///
    /// Trailing white space is ignored. Unless the Plot's
    /// [`escape`](Plot::escape) attribute is cleared, escape sequences in
    /// the text change its appearance part way through: `%^50+` raises the
    /// baseline by half a character height and `%v50+` lowers it, `%>30+`
    /// and `%<30+` move along it, `%s150+`, `%w200+`, `%f2+`, `%c3+` and
    /// `%t2+` change the size, width, font, colour and style, `%h+` marks a
    /// position that `%g+` returns to, `%-` and `%+` push and pop the
    /// attributes, and `%%` is a percent sign; a `+` in place of the
    /// number restores the normal value.
    ///
    /// Nothing is drawn where `pos` has no graphics position, for empty
    /// text, or for a zero up-vector.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    /// - `ErrorCode::Ncpin` when `pos` has fewer values than the current
    ///   Frame has axes.
    /// - Any error the backend reports.
    #[doc(alias = "astText")]
    fn draw_text(
        &mut self,
        grf: &mut dyn Grf,
        text: &str,
        pos: &[f64],
        up: [f32; 2],
        just: Justification,
    ) -> AstResult<()>;
}

impl Draw for Plot {
    fn draw_grid_line(
        &mut self,
        grf: &mut dyn Grf,
        axis: usize,
        start: &[f64],
        length: f64,
    ) -> AstResult<()> {
        self.grid_line_impl(grf, axis, start, length)
    }

    fn draw_border(&mut self, grf: &mut dyn Grf) -> AstResult<bool> {
        self.border_impl(grf)
    }

    fn draw_text(
        &mut self,
        grf: &mut dyn Grf,
        text: &str,
        pos: &[f64],
        up: [f32; 2],
        just: Justification,
    ) -> AstResult<()> {
        self.text_impl(grf, text, pos, up, just)
    }
}

/// The state C keeps in globals while a Plot draws.
#[derive(Debug, Clone)]
pub(crate) struct DrawState {
    /// Character heights, `AST__BAD` until asked for.
    // C: Grf_chh, Grf_chv.
    pub(crate) chh: f64,
    pub(crate) chv: f64,
    /// The backend's axis scales, zero until asked for.
    // C: Grf_alpha, Grf_beta.
    pub(crate) alpha: f32,
    pub(crate) beta: f32,
    /// The bounding box of what a grid draws.
    // C: Box_lbnd, Box_ubnd, which start at FLT_MAX and FLT_MIN; FLT_MIN is
    // the smallest positive float, not the most negative.
    pub(crate) box_lbnd: [f32; 2],
    pub(crate) box_ubnd: [f32; 2],
    /// The bounding box of what the last drawing method drew, and whether
    /// it is frozen.
    // C: Boxp_lbnd, Boxp_ubnd, Boxp_freeze.
    pub(crate) boxp_lbnd: [f32; 2],
    pub(crate) boxp_ubnd: [f32; 2],
    pub(crate) boxp_freeze: bool,
    /// The polyline being built, and those finished but not yet drawn.
    // C: Poly_x, Poly_y, Poly_n; Poly_xp, Poly_yp, Poly_np, Poly_npoly.
    pub(crate) poly_x: Vec<f32>,
    pub(crate) poly_y: Vec<f32>,
    pub(crate) polys: Vec<(Vec<f32>, Vec<f32>)>,
    /// The nesting level of `grf_attrs`, and the backend values it saved
    /// at level one (style, width, size, font, colour; `None` where it set
    /// nothing).
    // C: grfattrs_nesting, grfattrs_attrs.
    pub(crate) grfattrs_nesting: i32,
    pub(crate) grfattrs_saved: [Option<f64>; 5],
    /// The description of the last curve `draw_grid_line` drew.
    // C: Curve_data.
    pub(crate) curve_data: CurveData,
    /// The position marked by a `%h+` escape sequence, `f32::MAX` if none.
    // C: the Plot's hmarkx and hmarky.
    pub(crate) hmark: (f32, f32),
    /// The text attributes saved by `%-` escape sequences.
    // C: the Plot's gat and ngat.
    pub(crate) gat: Vec<super::text::Gat>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            chh: crate::constants::BAD_VALUE,
            chv: crate::constants::BAD_VALUE,
            alpha: 0.0,
            beta: 0.0,
            box_lbnd: [f32::MAX, f32::MAX],
            box_ubnd: [f32::MIN_POSITIVE, f32::MIN_POSITIVE],
            boxp_lbnd: [f32::MAX, f32::MAX],
            boxp_ubnd: [f32::MIN_POSITIVE, f32::MIN_POSITIVE],
            boxp_freeze: false,
            poly_x: Vec::new(),
            poly_y: Vec::new(),
            polys: Vec::new(),
            grfattrs_nesting: 0,
            grfattrs_saved: [None; 5],
            curve_data: CurveData::default(),
            hmark: (f32::MAX, f32::MAX),
            gat: Vec::new(),
        }
    }
}

/// C's `astMIN` of two floats: the first unless the second is smaller.
pub(crate) fn fmin(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// C's `astMAX` of two floats: the first unless the second is larger.
pub(crate) fn fmax(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

impl Plot {
    /// Forget the backend values cached during a drawing.
    // C: RESET_GRF.
    pub(crate) fn reset_grf(&mut self) {
        self.draw.chh = crate::constants::BAD_VALUE;
        self.draw.chv = crate::constants::BAD_VALUE;
        self.draw.alpha = 0.0;
        self.draw.beta = 0.0;
    }

    /// Start a new bounding box for what a drawing method draws, unless it
    /// is frozen.
    pub(crate) fn reset_boxp(&mut self) {
        if !self.draw.boxp_freeze {
            self.draw.boxp_lbnd = [f32::MAX, f32::MAX];
            self.draw.boxp_ubnd = [f32::MIN_POSITIVE, f32::MIN_POSITIVE];
        }
    }

    /// Extend the drawn bounding box by the points `(x[i], y[i])`.
    fn extend_boxp(&mut self, x: &[f32], y: &[f32]) {
        if self.draw.boxp_freeze {
            return;
        }
        for (&x, &y) in x.iter().zip(y) {
            self.draw.boxp_lbnd[0] = fmin(x, self.draw.boxp_lbnd[0]);
            self.draw.boxp_ubnd[0] = fmax(x, self.draw.boxp_ubnd[0]);
            self.draw.boxp_lbnd[1] = fmin(y, self.draw.boxp_lbnd[1]);
            self.draw.boxp_ubnd[1] = fmax(y, self.draw.boxp_ubnd[1]);
        }
    }

    /// Draw a polyline, unless the Plot draws in invisible ink.
    // C: GLine (plot.c:14957).
    pub(crate) fn g_line(&mut self, grf: &mut dyn Grf, x: &[f32], y: &[f32]) -> AstResult<()> {
        if !self.invisible() {
            grf.line(x, y)?;
        }
        self.extend_boxp(x, y);
        Ok(())
    }

    /// Whether the backend has a capability.
    // C: GCap (plot.c:14358).
    pub(crate) fn g_cap(&self, grf: &mut dyn Grf, cap: GrfCap, value: i32) -> bool {
        grf.cap(cap, value)
    }

    /// Set (unless `value` is `None`) and return a backend attribute.
    // C: GAttr (plot.c:14044), whose callers pass AST__BAD for no new value
    // and NULL for no old value; a call with neither does nothing, which
    // the callers here do not make.
    pub(crate) fn g_attr(
        &mut self,
        grf: &mut dyn Grf,
        attr: GrfAttr,
        value: Option<f64>,
        prim: GrfPrimitive,
    ) -> AstResult<f64> {
        grf.attr(attr, value, prim)
    }

    /// The scales from graphics coordinates to equal distances on the
    /// page, asked of the backend once per drawing.
    // C: GScales (plot.c:14242).
    pub(crate) fn g_scales(&mut self, grf: &mut dyn Grf) -> AstResult<(f32, f32)> {
        if self.draw.alpha != 0.0 && self.draw.beta != 0.0 {
            return Ok((self.draw.alpha, self.draw.beta));
        }
        if self.g_cap(grf, GrfCap::Scales, 1) {
            let [alpha, beta] = grf.scales()?;
            if alpha == 0.0 || beta == 0.0 {
                self.draw.alpha = 1.0;
                self.draw.beta = 1.0;
                return Err(AstError::new(
                    ErrorCode::Grfer,
                    format!(
                        "astGScales: Returned axis scales are {} and {} but zero is illegal!",
                        format_double_g(f64::from(alpha), 6),
                        format_double_g(f64::from(beta), 6)
                    ),
                ));
            }
            self.draw.alpha = alpha;
            self.draw.beta = beta;
        } else {
            self.draw.alpha = if self.xrev { -1.0 } else { 1.0 };
            self.draw.beta = if self.yrev { -1.0 } else { 1.0 };
        }
        Ok((self.draw.alpha, self.draw.beta))
    }

    /// Establish the backend attributes for drawing `element` with the
    /// primitive `prim` from the Plot's style attributes (`set`), or put
    /// back the values they replaced (`!set`). Calls nest: only the
    /// outermost pair changes anything.
    // C: astGrfAttrs (plot.c:19022).
    pub(crate) fn grf_attrs(
        &mut self,
        grf: &mut dyn Grf,
        element: PlotElement,
        set: bool,
        prim: GrfPrimitive,
    ) -> AstResult<()> {
        if set {
            self.draw.grfattrs_nesting += 1;
        } else {
            self.draw.grfattrs_nesting -= 1;
        }
        if prim == GrfPrimitive::Line {
            self.fpoly(grf)?;
        }
        if set && self.draw.grfattrs_nesting == 1 {
            let values = [
                (
                    GrfAttr::Style,
                    self.test_style(element),
                    self.style(element) as f64,
                ),
                (
                    GrfAttr::Width,
                    self.test_width(element),
                    self.width(element),
                ),
                (GrfAttr::Size, self.test_size(element), self.size(element)),
                (
                    GrfAttr::Font,
                    self.test_font(element),
                    self.font(element) as f64,
                ),
                (
                    GrfAttr::Colour,
                    self.test_colour(element),
                    self.colour(element) as f64,
                ),
            ];
            for (k, (attr, is_set, value)) in values.into_iter().enumerate() {
                self.draw.grfattrs_saved[k] = if is_set {
                    Some(self.g_attr(grf, attr, Some(value), prim)?)
                } else {
                    None
                };
            }
        }
        if !set && self.draw.grfattrs_nesting == 0 {
            let attrs = [
                GrfAttr::Style,
                GrfAttr::Width,
                GrfAttr::Size,
                GrfAttr::Font,
                GrfAttr::Colour,
            ];
            for (k, attr) in attrs.into_iter().enumerate() {
                if let Some(value) = self.draw.grfattrs_saved[k] {
                    self.g_attr(grf, attr, Some(value), prim)?;
                }
            }
        }
        Ok(())
    }
}
