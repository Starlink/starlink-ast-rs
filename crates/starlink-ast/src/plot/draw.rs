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
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;
use crate::grf::{Grf, GrfAttr, GrfCap, GrfPrimitive, Justification};
use crate::mappings::Mapping;
use crate::serial::native_write::format_double_g;

/// The methods that draw a Plot through a graphics backend.
pub trait Draw {
    /// Draw an annotated coordinate grid of the current Frame: grid lines
    /// or tick marks along each axis, numerical labels at the major ticks,
    /// a descriptive label for each axis, a title and a border, each as the
    /// Plot's attributes ask, with what they leave unset chosen to suit the
    /// coordinates, as [`used_values`](Plot::used_values) reports.
    ///
    /// A current Frame of more than two axes is drawn through its first
    /// two.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the base Frame, or the current Frame, does
    ///   not have two axes.
    /// - `ErrorCode::Vsmal`, `ErrorCode::Zerax`, `ErrorCode::Attin` or
    ///   `ErrorCode::Plfmt` when no usable ticks or labels can be found for
    ///   an axis.
    /// - Any error the backend reports.
    #[doc(alias = "astGrid")]
    fn draw_grid(&mut self, grf: &mut dyn Grf) -> AstResult<()>;

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
    /// When the current Frame is a Region, the boundary inside the
    /// plotting area is the edge of the Region. A Region that can trace its
    /// own boundary is drawn along it, part by part where it splits into
    /// parts that do not overlap, and the result is then false. Those that
    /// cannot are a PointList, a NullRegion, a Prism, the STC Regions, an
    /// Interval with an open or excluded limit, a Region defined in a Frame
    /// that is not 2-D, and a CmpRegion whose first component cannot. A
    /// CmpRegion whose second component cannot trace its boundary draws no
    /// border along that component, as the C library's tracing of it is
    /// undefined.
    ///
    /// A current Frame of more than two axes is drawn through its first
    /// two.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the base Frame, or the current Frame, has
    ///   fewer or more than two axes.
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

    /// Draw the geodesic curve from `start` to `finish`, two positions in
    /// the current Frame, in the style of
    /// [`Curves`](crate::plot::PlotElement::Curves).
    ///
    /// The curve is the shortest path between the two positions in the
    /// current Frame (a great circle on the sky, a straight line in a plain
    /// Frame), traced to within the Plot's [`tol`](Plot::tol) and broken
    /// where its graphics positions are undefined. It is clipped at the
    /// edges of the plotting area when the Plot's [`clip`](Plot::clip)
    /// attribute includes 1, for curves, and by any clip region.
    ///
    /// Nothing is drawn if any value of `start` or `finish` is bad.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    /// - `ErrorCode::Ncpin` when `start` or `finish` has fewer values than
    ///   the current Frame has axes.
    /// - `ErrorCode::Cvbrk` when the curve has more than 1000 breaks.
    /// - Any error the backend reports.
    #[doc(alias = "astCurve")]
    fn draw_curve(&mut self, grf: &mut dyn Grf, start: &[f64], finish: &[f64]) -> AstResult<()>;

    /// Draw a curve through `positions`, one axis of the PointSet per axis
    /// of the current Frame, made of the geodesic between each position and
    /// the next, in the style of [`Curves`](crate::plot::PlotElement::Curves).
    ///
    /// Each segment is drawn as [`draw_curve`](Draw::draw_curve) draws a
    /// curve, and the segments are joined into one line. A segment with a
    /// bad value at either end is left out, breaking the line there.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D, or when
    ///   `positions` does not have one axis per axis of the current Frame.
    /// - `ErrorCode::Cvbrk` when a segment has more than 1000 breaks.
    /// - Any error the backend reports.
    #[doc(alias = "astPolyCurve")]
    fn draw_poly_curve(&mut self, grf: &mut dyn Grf, positions: &PointSet) -> AstResult<()>;

    /// Draw the curve that `map` describes, in the style of
    /// [`Curves`](crate::plot::PlotElement::Curves).
    ///
    /// `map` has one input, a distance along the curve from 0 at its start
    /// to 1 at its end, and one output per axis of the current Frame, the
    /// position at that distance. The curve is traced to within the Plot's
    /// [`tol`](Plot::tol), broken where `map` gives bad positions or their
    /// graphics positions are undefined, and clipped as
    /// [`draw_curve`](Draw::draw_curve) clips.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    /// - The error transforming through `map` gives when it does not have
    ///   one input and one output per axis of the current Frame.
    /// - `ErrorCode::Cvbrk` when the curve has more than 1000 breaks.
    /// - Any error the backend reports.
    #[doc(alias = "astGenCurve")]
    fn draw_gen_curve(&mut self, grf: &mut dyn Grf, map: &dyn Mapping) -> AstResult<()>;

    /// Draw a marker of type `marker` at each of `positions`, one axis of
    /// the PointSet per axis of the current Frame, in the style of
    /// [`Markers`](crate::plot::PlotElement::Markers).
    ///
    /// What each marker type looks like is up to the backend. Positions
    /// with no graphics position are left out, and so, when the Plot's
    /// [`clip`](Plot::clip) attribute includes 2, for markers, are those
    /// outside the plotting area; any clip region also removes markers.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    /// - The error transforming `positions` gives when it does not have one
    ///   axis per axis of the current Frame.
    /// - Any error the backend reports.
    #[doc(alias = "astMark")]
    fn draw_mark(&mut self, grf: &mut dyn Grf, positions: &PointSet, marker: i32) -> AstResult<()>;

    /// Clip what the Plot draws to a box in one of its Frames: positions
    /// whose coordinates in Frame `frame` fall outside `lbnd` to `ubnd` (one
    /// value per axis of that Frame) are not drawn.
    ///
    /// A bad bound leaves that side of the box open. Whether a position is
    /// clipped when it is outside the bounds on any axis, or only when it is
    /// outside them on every axis, is set by the Plot's
    /// [`clip_op`](Plot::clip_op) attribute. The box replaces any set
    /// before; it is kept with the Plot when it is saved, and it applies to
    /// lines, markers and text alike, whatever the Plot's
    /// [`clip`](Plot::clip) attribute, which controls clipping at the edges
    /// of the plotting area.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    /// - `ErrorCode::Frmin` when `frame` is not a Frame of the Plot.
    /// - `ErrorCode::Clpax` when `lbnd` or `ubnd` does not have one value
    ///   per axis of the Frame.
    ///
    /// The Plot is left unchanged if there is an error.
    #[doc(alias = "astClip")]
    fn set_clip_region(&mut self, frame: FrameRef, lbnd: &[f64], ubnd: &[f64]) -> AstResult<()>;

    /// Remove the clip region [`set_clip_region`](Draw::set_clip_region)
    /// set, if any.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    #[doc(alias = "astClip")]
    fn clear_clip_region(&mut self) -> AstResult<()>;

    /// The bounds of a box in graphics coordinates that encloses what the
    /// last drawing method called on this Plot drew, as `(lbnd, ubnd)`.
    ///
    /// A drawing method that drew nothing leaves lower bounds of `f32::MAX`
    /// and upper bounds of `f32::MIN_POSITIVE`. Lines and markers drawn in
    /// invisible ink count, as if they had been drawn.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Plot's base Frame is not 2-D.
    #[doc(alias = "astBoundingBox")]
    fn bounding_box(&self) -> AstResult<([f32; 2], [f32; 2])>;
}

impl Draw for Plot {
    fn draw_grid(&mut self, grf: &mut dyn Grf) -> AstResult<()> {
        self.begin_drawing();
        self.grid_impl(grf, true)
    }

    fn draw_grid_line(
        &mut self,
        grf: &mut dyn Grf,
        axis: usize,
        start: &[f64],
        length: f64,
    ) -> AstResult<()> {
        self.begin_drawing();
        self.grid_line_impl(grf, axis, start, length)
    }

    fn draw_border(&mut self, grf: &mut dyn Grf) -> AstResult<bool> {
        self.begin_drawing();
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
        self.begin_drawing();
        self.text_impl(grf, text, pos, up, just)
    }

    fn draw_curve(&mut self, grf: &mut dyn Grf, start: &[f64], finish: &[f64]) -> AstResult<()> {
        self.begin_drawing();
        self.curve_impl(grf, start, finish)
    }

    fn draw_poly_curve(&mut self, grf: &mut dyn Grf, positions: &PointSet) -> AstResult<()> {
        self.begin_drawing();
        self.poly_curve_impl(grf, positions)
    }

    fn draw_gen_curve(&mut self, grf: &mut dyn Grf, map: &dyn Mapping) -> AstResult<()> {
        self.begin_drawing();
        self.gen_curve_impl(grf, map)
    }

    fn draw_mark(&mut self, grf: &mut dyn Grf, positions: &PointSet, marker: i32) -> AstResult<()> {
        self.begin_drawing();
        self.mark_impl(grf, positions, marker)
    }

    // C: Clip (plot.c:7030), with a Frame.
    fn set_clip_region(&mut self, frame: FrameRef, lbnd: &[f64], ubnd: &[f64]) -> AstResult<()> {
        self.check_base_2d("astClip")?;
        // C: astValidateFrameIndex.
        let nframe = self.frameset.nframe();
        let index = match frame {
            FrameRef::Base => self.frameset.base_index(),
            FrameRef::Current => self.frameset.current_index(),
            FrameRef::Index(i) if (1..=nframe).contains(&i) => i,
            FrameRef::Index(i) => {
                return Err(AstError::new(
                    ErrorCode::Frmin,
                    format!(
                        "astClip(Plot): Invalid Frame index ({i}) given. This value should be in \
                         the range 1 to {nframe} (or AST__CURRENT or AST__BASE)."
                    ),
                ));
            }
            FrameRef::AllFrames => {
                return Err(AstError::new(
                    ErrorCode::Frmin,
                    "astClip(Plot): AST__ALLFRAMES does not identify a single Frame.",
                ));
            }
        };
        let naxes = self.frameset.frame(FrameRef::Index(index))?.naxes();
        for (name, bnd) in [("lower", lbnd), ("upper", ubnd)] {
            if bnd.len() != naxes {
                return Err(AstError::new(
                    ErrorCode::Clpax,
                    format!(
                        "astClip(Plot): {} values were supplied for the {name} bounds of the \
                         clipping volume, but the clipping Frame has {naxes} axes.",
                        bnd.len()
                    ),
                ));
            }
        }
        let or_open = |v: f64, limit: f64| {
            if v == crate::constants::BAD_VALUE {
                limit
            } else {
                v
            }
        };
        self.clip = Some(super::ClipRegion {
            frame: index as i32,
            lbnd: lbnd.iter().map(|&v| or_open(v, -f64::MAX)).collect(),
            ubnd: ubnd.iter().map(|&v| or_open(v, f64::MAX)).collect(),
        });
        Ok(())
    }

    // C: Clip (plot.c:7030), with AST__NOFRAME.
    fn clear_clip_region(&mut self) -> AstResult<()> {
        self.check_base_2d("astClip")?;
        self.clip = None;
        Ok(())
    }

    // C: BoundingBox (plot.c:5228).
    fn bounding_box(&self) -> AstResult<([f32; 2], [f32; 2])> {
        self.check_base_2d("astBoundingBox")?;
        Ok((self.draw.boxp_lbnd, self.draw.boxp_ubnd))
    }
}

/// The state C keeps in globals while a Plot draws.
#[derive(Debug)]
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
    /// The tick marks the last grid drew.
    pub(crate) ticks: super::axes::DrawnTicks,
}

// C: astCopy of a Plot (plot.c Copy) starts the copy with an empty stack of
// pushed text attributes, and copies the rest.
impl Clone for DrawState {
    fn clone(&self) -> Self {
        Self {
            chh: self.chh,
            chv: self.chv,
            alpha: self.alpha,
            beta: self.beta,
            box_lbnd: self.box_lbnd,
            box_ubnd: self.box_ubnd,
            boxp_lbnd: self.boxp_lbnd,
            boxp_ubnd: self.boxp_ubnd,
            boxp_freeze: self.boxp_freeze,
            poly_x: self.poly_x.clone(),
            poly_y: self.poly_y.clone(),
            polys: self.polys.clone(),
            grfattrs_nesting: self.grfattrs_nesting,
            grfattrs_saved: self.grfattrs_saved,
            curve_data: self.curve_data.clone(),
            hmark: self.hmark,
            gat: Vec::new(),
            ticks: self.ticks.clone(),
        }
    }
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
            ticks: Default::default(),
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
    /// Start a public drawing method with no style settings nested and no
    /// polylines waiting, as C's globals are after any drawing that ends
    /// without an error.
    // C's nesting count and polyline buffer are left as they were by a
    // drawing that stops on an error; the port does not carry them on.
    pub(crate) fn begin_drawing(&mut self) {
        self.draw.grfattrs_nesting = 0;
        self.draw.poly_x.clear();
        self.draw.poly_y.clear();
        self.draw.polys.clear();
    }

    /// Hand the drawing state to `copy`, a copy of this Plot with fewer
    /// axes that a drawing method draws through. What C keeps in globals
    /// moves with the drawing; what C keeps in the Plot (the pushed text
    /// attributes, the `%h+` mark and the drawn ticks) stays here, the copy
    /// holding its own, with no pushed attributes, as C's copy does.
    // C: Fset2D returns astCopy of the Plot when it must reduce the current
    // Frame.
    pub(crate) fn lend_draw_state(&mut self, copy: &mut Plot) {
        let mut d = std::mem::take(&mut self.draw);
        self.draw.gat = std::mem::take(&mut d.gat);
        self.draw.hmark = d.hmark;
        self.draw.ticks = d.ticks.clone();
        copy.draw = d;
    }

    /// Take back the drawing state lent with `lend_draw_state`, keeping
    /// this Plot's own fields.
    pub(crate) fn return_draw_state(&mut self, copy: &mut Plot) {
        let mut d = std::mem::take(&mut copy.draw);
        d.gat = std::mem::take(&mut self.draw.gat);
        d.hmark = self.draw.hmark;
        d.ticks = std::mem::take(&mut self.draw.ticks);
        self.draw = d;
    }

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

    /// Draw markers, unless the Plot draws in invisible ink.
    // C: GMark (plot.c:15060).
    pub(crate) fn g_mark(
        &mut self,
        grf: &mut dyn Grf,
        x: &[f32],
        y: &[f32],
        marker: i32,
    ) -> AstResult<()> {
        if !self.invisible() {
            grf.mark(x, y, marker)?;
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
