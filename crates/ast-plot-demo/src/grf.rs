// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A graphics backend that draws through egui's painter.
//!
//! Graphics coordinates are egui points, x to the right from the left edge
//! of the drawing rectangle and y upward from its bottom edge, so equal
//! distances on both axes are equal on the screen and
//! [`scales`](Grf::scales) is `[1, 1]`.
//! Text is laid out with egui's own fonts, so the boxes the Plot asks for
//! are the boxes egui draws the text in.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, FontId, Galley, Pos2, Rect, Shape, Stroke, Vec2,
    epaint::{StrokeKind, TextShape},
};
use starlink_ast::error::{AstError, AstResult, ErrorCode};
use starlink_ast::grf::{Grf, GrfAttr, GrfCap, GrfPrimitive, HJust, Justification, TextBox, VJust};

/// The font size, in points, of text of size 1.
pub const BASE_FONT_SIZE: f32 = 12.0;
/// The width, in points, of lines of width 1.
pub const BASE_LINE_WIDTH: f32 = 1.0;
/// The size, in points, of markers of size 1.
pub const BASE_MARKER_SIZE: f32 = 6.0;
/// The smallest and largest font sizes, in points, that text is drawn at,
/// whatever its size; egui cannot lay out a glyph larger than its font
/// texture.
pub const FONT_SIZE_RANGE: (f32, f32) = (1.0, 200.0);

/// PGPLOT's colors 2 to 15.
const PGPLOT_COLOURS: [Color32; 14] = [
    Color32::from_rgb(0xFF, 0x00, 0x00),
    Color32::from_rgb(0x00, 0xFF, 0x00),
    Color32::from_rgb(0x00, 0x00, 0xFF),
    Color32::from_rgb(0x00, 0xFF, 0xFF),
    Color32::from_rgb(0xFF, 0x00, 0xFF),
    Color32::from_rgb(0xFF, 0xFF, 0x00),
    Color32::from_rgb(0xFF, 0x80, 0x00),
    Color32::from_rgb(0x80, 0xFF, 0x00),
    Color32::from_rgb(0x00, 0xFF, 0x80),
    Color32::from_rgb(0x00, 0x80, 0xFF),
    Color32::from_rgb(0x80, 0x00, 0xFF),
    Color32::from_rgb(0xFF, 0x00, 0x80),
    Color32::from_rgb(0x55, 0x55, 0x55),
    Color32::from_rgb(0xAA, 0xAA, 0xAA),
];

/// A [`Grf`] that collects egui shapes for a drawing rectangle.
///
/// It must be used inside an egui pass, such as an app's `update`, where
/// egui's fonts are ready.
pub struct EguiGrf {
    ctx: egui::Context,
    /// The screen position of graphics coordinates (0, 0).
    origin: Pos2,
    foreground: Color32,
    background: Color32,
    shapes: Vec<Shape>,
    /// Style, width, size, font and color, for text, lines and markers.
    attrs: [[f64; 5]; 3],
}

fn attr_index(attr: GrfAttr) -> usize {
    match attr {
        GrfAttr::Style => 0,
        GrfAttr::Width => 1,
        GrfAttr::Size => 2,
        GrfAttr::Font => 3,
        GrfAttr::Colour => 4,
    }
}

fn prim_index(prim: GrfPrimitive) -> usize {
    match prim {
        GrfPrimitive::Text => 0,
        GrfPrimitive::Line => 1,
        GrfPrimitive::Mark => 2,
    }
}

fn grf_error(message: String) -> AstError {
    AstError::new(ErrorCode::Grfer, message)
}

fn check_lengths(x: &[f32], y: &[f32]) -> AstResult<()> {
    if x.len() == y.len() {
        Ok(())
    } else {
        Err(grf_error(format!(
            "EguiGrf: {} x coordinates were given with {} y coordinates.",
            x.len(),
            y.len()
        )))
    }
}

/// A multiple at or below zero counts as 1.
fn positive(v: f64) -> f32 {
    if v > 0.0 { v as f32 } else { 1.0 }
}

impl EguiGrf {
    /// A backend drawing in `rect`, with color index 1 `foreground` and color index 0
    /// `background`.
    pub fn new(ctx: &egui::Context, rect: Rect, foreground: Color32, background: Color32) -> Self {
        Self {
            ctx: ctx.clone(),
            origin: rect.left_bottom(),
            foreground,
            background,
            shapes: Vec::new(),
            attrs: [[1.0; 5]; 3],
        }
    }

    /// The shapes drawn so far, in screen coordinates.
    pub fn into_shapes(self) -> Vec<Shape> {
        self.shapes
    }

    fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.origin.x + x, self.origin.y - y)
    }

    fn get(&self, attr: GrfAttr, prim: GrfPrimitive) -> f64 {
        self.attrs[prim_index(prim)][attr_index(attr)]
    }

    fn colour(&self, prim: GrfPrimitive) -> Color32 {
        match self.get(GrfAttr::Colour, prim).round() {
            0.0 => self.background,
            c if (2.0..=15.0).contains(&c) => PGPLOT_COLOURS[c as usize - 2],
            _ => self.foreground,
        }
    }

    fn stroke(&self, prim: GrfPrimitive) -> Stroke {
        let width = BASE_LINE_WIDTH * positive(self.get(GrfAttr::Width, prim));
        Stroke::new(width, self.colour(prim))
    }

    fn font(&self) -> FontId {
        let multiple = self.get(GrfAttr::Size, GrfPrimitive::Text);
        let size = if multiple.is_finite() && multiple > 0.0 {
            (BASE_FONT_SIZE * multiple as f32).clamp(FONT_SIZE_RANGE.0, FONT_SIZE_RANGE.1)
        } else {
            BASE_FONT_SIZE
        };
        FontId::proportional(size)
    }

    /// The text's galley, its box, and the direction it reads, in graphics
    /// coordinates; `None` for empty text.
    fn layout(
        &self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<Option<(Arc<Galley>, TextBox, Vec2)>> {
        let len = up[0].hypot(up[1]);
        if len == 0.0 {
            return Err(grf_error(
                "EguiGrf: the up vector of text has zero length.".to_owned(),
            ));
        }
        if text.is_empty() {
            return Ok(None);
        }
        let galley = self
            .ctx
            .fonts(|f| f.layout_no_wrap(text.to_owned(), self.font(), Color32::PLACEHOLDER));
        let (w, h) = (galley.size().x, galley.size().y);
        // The baseline's distance below the top of the text.
        let ascent = galley
            .rows
            .first()
            .and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y))
            .unwrap_or(h);
        let n = Vec2::new(up[0] / len, up[1] / len);
        let d = Vec2::new(n.y, -n.x);
        let top = match just.vertical {
            VJust::Top => 0.0,
            VJust::Centre => 0.5 * h,
            VJust::Baseline => ascent,
            VJust::Bottom => h,
        };
        let left = match just.horizontal {
            HJust::Left => 0.0,
            HJust::Centre => 0.5 * w,
            HJust::Right => w,
        };
        let c0 = Vec2::new(x, y) - left * d + (top - h) * n;
        let c1 = c0 + w * d;
        let c2 = c1 + h * n;
        let c3 = c0 + h * n;
        let tb = TextBox {
            x: [c0.x, c1.x, c2.x, c3.x],
            y: [c0.y, c1.y, c2.y, c3.y],
        };
        Ok(Some((galley, tb, d)))
    }

    fn mark_at(&mut self, c: Pos2, marker: i32) {
        let stroke = self.stroke(GrfPrimitive::Mark);
        let colour = self.colour(GrfPrimitive::Mark);
        let s = BASE_MARKER_SIZE * positive(self.get(GrfAttr::Size, GrfPrimitive::Mark));
        let r = 0.5 * s;
        let arm = |angle: f32| {
            let v = r * Vec2::angled(angle);
            Shape::line_segment([c - v, c + v], stroke)
        };
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_6};
        match marker {
            1 => self
                .shapes
                .push(Shape::circle_filled(c, stroke.width.max(1.0), colour)),
            2 => self.shapes.extend([arm(0.0), arm(FRAC_PI_2)]),
            3 => self
                .shapes
                .extend([arm(FRAC_PI_2), arm(FRAC_PI_6), arm(-FRAC_PI_6)]),
            5 => self.shapes.extend([arm(FRAC_PI_4), arm(-FRAC_PI_4)]),
            6 => self.shapes.push(Shape::rect_stroke(
                Rect::from_center_size(c, Vec2::splat(s)),
                0.0,
                stroke,
                StrokeKind::Middle,
            )),
            _ => self.shapes.push(Shape::circle_stroke(c, r, stroke)),
        }
    }
}

impl Grf for EguiGrf {
    fn line(&mut self, x: &[f32], y: &[f32]) -> AstResult<()> {
        check_lengths(x, y)?;
        if x.len() < 2 {
            return Ok(());
        }
        let points: Vec<Pos2> = x
            .iter()
            .zip(y)
            .map(|(&x, &y)| self.to_screen(x, y))
            .collect();
        let stroke = self.stroke(GrfPrimitive::Line);
        let w = stroke.width;
        match self.get(GrfAttr::Style, GrfPrimitive::Line).round() {
            2.0 | 3.0 | 5.0 => {
                self.shapes
                    .extend(Shape::dashed_line(&points, stroke, 6.0 * w, 4.0 * w))
            }
            4.0 => self
                .shapes
                .extend(Shape::dashed_line(&points, stroke, w, 3.0 * w)),
            _ => self.shapes.push(Shape::line(points, stroke)),
        }
        Ok(())
    }

    fn mark(&mut self, x: &[f32], y: &[f32], marker: i32) -> AstResult<()> {
        check_lengths(x, y)?;
        for (&x, &y) in x.iter().zip(y) {
            let c = self.to_screen(x, y);
            self.mark_at(c, marker);
        }
        Ok(())
    }

    fn text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<()> {
        if let Some((galley, tb, d)) = self.layout(text, x, y, just, up)? {
            let pos = self.to_screen(tb.x[3], tb.y[3]);
            // egui turns text clockwise on a screen whose y runs down.
            let angle = (-d.y).atan2(d.x);
            let colour = self.colour(GrfPrimitive::Text);
            self.shapes
                .push(TextShape::new(pos, galley, colour).with_angle(angle).into());
        }
        Ok(())
    }

    fn text_extent(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<TextBox> {
        Ok(self.layout(text, x, y, just, up)?.map_or(
            TextBox {
                x: [0.0; 4],
                y: [0.0; 4],
            },
            |(_, tb, _)| tb,
        ))
    }

    fn char_height(&mut self) -> AstResult<[f32; 2]> {
        let font = self.font();
        let r = self.ctx.fonts(|f| f.row_height(&font));
        Ok([r, r])
    }

    fn scales(&mut self) -> AstResult<[f32; 2]> {
        Ok([1.0, 1.0])
    }

    fn attr(&mut self, attr: GrfAttr, value: Option<f64>, prim: GrfPrimitive) -> AstResult<f64> {
        let slot = &mut self.attrs[prim_index(prim)][attr_index(attr)];
        let old = *slot;
        if let Some(v) = value {
            *slot = v;
        }
        Ok(old)
    }

    fn cap(&mut self, cap: GrfCap, _value: i32) -> bool {
        cap == GrfCap::Scales
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::{build_plot, cases, graphics_box, plot_box};
    use eframe::egui::{self, Color32, Pos2, RawInput, Rect, Shape, epaint::ColorMode};
    use starlink_ast::error::ErrorCode;
    use starlink_ast::grf::{Grf, GrfAttr, GrfPrimitive, HJust, Justification, VJust};
    use starlink_ast::plot::Draw;

    const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));

    /// Run `f` inside an egui pass on an 800 by 600 screen, with
    /// `ppp` physical pixels per point, on a backend over the whole screen
    /// with a white foreground and a black background.
    fn with_grf<R>(ppp: f32, f: impl FnOnce(EguiGrf) -> R) -> R {
        let ctx = egui::Context::default();
        let mut input = RawInput {
            screen_rect: Some(SCREEN),
            ..Default::default()
        };
        input
            .viewports
            .entry(input.viewport_id)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        let mut f = Some(f);
        let mut result = None;
        let _ = ctx.run(input, |ctx| {
            let grf = EguiGrf::new(ctx, SCREEN, Color32::WHITE, Color32::BLACK);
            result = Some((f.take().unwrap())(grf));
        });
        result.unwrap()
    }

    fn just(v: VJust, h: HJust) -> Justification {
        Justification::new(v, h)
    }

    fn mean(b: &starlink_ast::grf::TextBox) -> (f32, f32) {
        (b.x.iter().sum::<f32>() / 4.0, b.y.iter().sum::<f32>() / 4.0)
    }

    fn side(b: &starlink_ast::grf::TextBox, i: usize, j: usize) -> f32 {
        (b.x[j] - b.x[i]).hypot(b.y[j] - b.y[i])
    }

    #[test]
    fn attr_returns_the_previous_value() {
        with_grf(1.0, |mut g| {
            let line = GrfPrimitive::Line;
            assert_eq!(g.attr(GrfAttr::Colour, Some(3.0), line).unwrap(), 1.0);
            assert_eq!(g.attr(GrfAttr::Colour, None, line).unwrap(), 3.0);
            assert_eq!(
                g.attr(GrfAttr::Colour, None, GrfPrimitive::Text).unwrap(),
                1.0
            );
        });
    }

    #[test]
    fn color_one_is_the_foreground() {
        let shapes = with_grf(1.0, |mut g| {
            g.line(&[10.0, 20.0], &[10.0, 20.0]).unwrap();
            g.into_shapes()
        });
        let [Shape::Path(path)] = shapes.as_slice() else {
            panic!("expected one path, got {shapes:?}");
        };
        assert_eq!(path.stroke.color, ColorMode::Solid(Color32::WHITE));
    }

    #[test]
    fn empty_text_has_a_zero_box() {
        let b = with_grf(1.0, |mut g| {
            let j = just(VJust::Centre, HJust::Centre);
            g.text_extent("", 100.0, 200.0, j, [0.0, 1.0]).unwrap()
        });
        assert_eq!(b.x, [0.0; 4]);
        assert_eq!(b.y, [0.0; 4]);
    }

    #[test]
    fn zero_up_vector_is_an_error() {
        let err = with_grf(1.0, |mut g| {
            let j = just(VJust::Centre, HJust::Centre);
            g.text_extent("a", 100.0, 200.0, j, [0.0, 0.0]).unwrap_err()
        });
        assert_eq!(err.code(), ErrorCode::Grfer);
    }

    #[test]
    fn mismatched_coordinates_are_an_error() {
        with_grf(1.0, |mut g| {
            assert_eq!(
                g.line(&[0.0, 1.0], &[0.0]).unwrap_err().code(),
                ErrorCode::Grfer
            );
            assert_eq!(
                g.mark(&[0.0, 1.0], &[0.0], 1).unwrap_err().code(),
                ErrorCode::Grfer
            );
        });
    }

    #[test]
    fn centered_text_is_centered_at_any_angle() {
        with_grf(1.0, |mut g| {
            let j = just(VJust::Centre, HJust::Centre);
            let flat = g.text_extent("Label", 100.0, 200.0, j, [0.0, 1.0]).unwrap();
            let (w, h) = (side(&flat, 0, 1), side(&flat, 0, 3));
            assert!(w > h && h > 0.0);
            for up in [[0.0, 1.0], [-1.0, 0.0], [1.0, 1.0], [0.0, -1.0]] {
                let b = g.text_extent("Label", 100.0, 200.0, j, up).unwrap();
                let (mx, my) = mean(&b);
                assert!(
                    (mx - 100.0).abs() < 1e-3 && (my - 200.0).abs() < 1e-3,
                    "{up:?}"
                );
                assert!((side(&b, 0, 1) - w).abs() < 1e-3, "{up:?}");
                assert!((side(&b, 0, 3) - h).abs() < 1e-3, "{up:?}");
            }
        });
    }

    #[test]
    fn justification_places_the_box() {
        with_grf(1.0, |mut g| {
            let up = [0.0, 1.0];
            let b = g
                .text_extent(
                    "Label",
                    100.0,
                    200.0,
                    just(VJust::Baseline, HJust::Left),
                    up,
                )
                .unwrap();
            assert_eq!(b.x[0], 100.0);
            assert!(b.y[0] < 200.0 && b.y[3] > 200.0);
            let b = g
                .text_extent("Label", 100.0, 200.0, just(VJust::Top, HJust::Right), up)
                .unwrap();
            assert_eq!((b.x[2], b.y[2]), (100.0, 200.0));
        });
    }

    #[test]
    fn drawn_text_starts_at_its_box() {
        let (b, shapes) = with_grf(1.0, |mut g| {
            let (j, up) = (just(VJust::Baseline, HJust::Left), [-1.0, 0.0]);
            let b = g.text_extent("Label", 100.0, 200.0, j, up).unwrap();
            g.text("Label", 100.0, 200.0, j, up).unwrap();
            (b, g.into_shapes())
        });
        let [Shape::Text(t)] = shapes.as_slice() else {
            panic!("expected one text, got {shapes:?}");
        };
        // Graphics y runs up from the bottom of the screen.
        let corner = Pos2::new(b.x[3], SCREEN.bottom() - b.y[3]);
        assert!((t.pos - corner).length() < 1e-3, "{:?} {corner:?}", t.pos);
        assert!((t.angle + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn char_height_is_in_points_at_any_scale() {
        let one = with_grf(1.0, |mut g| g.char_height().unwrap());
        let two = with_grf(2.0, |mut g| g.char_height().unwrap());
        assert!((one[0] - two[0]).abs() <= 1.0, "{one:?} {two:?}");
        let big = with_grf(1.0, |mut g| {
            g.attr(GrfAttr::Size, Some(2.0), GrfPrimitive::Text)
                .unwrap();
            g.char_height().unwrap()
        });
        let ratio = big[1] / one[1];
        assert!((1.8..=2.2).contains(&ratio), "{ratio}");
    }

    #[test]
    fn every_case_draws_through_egui_grf() {
        for c in cases() {
            let shapes = with_grf(1.0, |mut g| {
                let gbox = graphics_box(800.0, 600.0, plot_box(&c));
                let mut plot = build_plot(&c, c.attr, gbox).unwrap();
                plot.draw_grid(&mut g).unwrap();
                g.into_shapes()
            });
            assert!(
                shapes.iter().any(|s| matches!(s, Shape::Text(_))),
                "{}",
                c.name
            );
        }
    }
}
