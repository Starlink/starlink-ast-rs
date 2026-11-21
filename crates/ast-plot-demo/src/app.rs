// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The demo's window: a case picker and an attribute field above the
//! case's grid, drawn to fill the rest of the window.

use eframe::egui::{self, CentralPanel, Color32, ComboBox, Rect, Shape, TextEdit, TopBottomPanel};
use starlink_ast::plot::Draw;

use crate::cases::{DemoCase, build_plot, cases, graphics_box, plot_box};
use crate::grf::EguiGrf;

/// The smallest drawing rectangle, in points on each side, drawn in.
const MIN_SIDE: f32 = 100.0;

/// The shapes of the grid of `case`, with the attributes `attr`, drawn to
/// fill `rect`, with color index 1 `foreground` and color index 0 `background`.
///
/// # Errors
///
/// A message for the user when `rect` is too small to draw in, or the case
/// cannot be built or drawn with these attributes.
pub fn draw_case(
    ctx: &egui::Context,
    rect: Rect,
    case: &DemoCase,
    attr: &str,
    foreground: Color32,
    background: Color32,
) -> Result<Vec<Shape>, String> {
    if rect.width() < MIN_SIDE || rect.height() < MIN_SIDE {
        return Err("The window is too small to draw in.".to_owned());
    }
    let gbox = graphics_box(rect.width(), rect.height(), plot_box(case));
    let mut grf = EguiGrf::new(ctx, rect, foreground, background);
    build_plot(case, attr, gbox)
        .and_then(|mut plot| plot.draw_grid(&mut grf))
        .map_err(|e| e.message().to_owned())?;
    Ok(grf.into_shapes())
}

/// What a drawing was made for: a change to any of it draws again.
#[derive(Debug, PartialEq)]
struct DrawingKey {
    case: usize,
    attr: String,
    /// The drawing rectangle, rounded to whole points.
    rect: [i32; 4],
    /// The screen scale, since egui's text layout is only good for the
    /// scale it was made at.
    pixels_per_point: u32,
    foreground: Color32,
    background: Color32,
}

impl DrawingKey {
    fn new(
        case: usize,
        attr: &str,
        rect: Rect,
        pixels_per_point: f32,
        foreground: Color32,
        background: Color32,
    ) -> Self {
        Self {
            case,
            attr: attr.to_owned(),
            rect: [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(|v| v.round() as i32),
            pixels_per_point: pixels_per_point.to_bits(),
            foreground,
            background,
        }
    }
}

struct Drawing {
    key: DrawingKey,
    result: Result<Vec<Shape>, String>,
}

/// The demo application.
pub struct DemoApp {
    cases: Vec<DemoCase>,
    selected: usize,
    /// The attribute field's text, applied on Enter or when it loses focus.
    edit: String,
    /// The attributes the drawing uses.
    applied: String,
    drawing: Option<Drawing>,
}

impl Default for DemoApp {
    fn default() -> Self {
        Self::with_case(None)
    }
}

impl DemoApp {
    /// The demo with the built-in cases, after `first` if it is given, and
    /// the first case selected.
    pub fn with_case(first: Option<DemoCase>) -> Self {
        let cases: Vec<DemoCase> = first.into_iter().chain(cases()).collect();
        let attr = cases[0].attr.to_owned();
        Self {
            cases,
            selected: 0,
            edit: attr.clone(),
            applied: attr,
            drawing: None,
        }
    }

    fn reset_attributes(&mut self) {
        self.edit = self.cases[self.selected].attr.to_owned();
        self.applied = self.edit.clone();
    }
}

impl eframe::App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let before = self.selected;
                ComboBox::from_label("Case")
                    .selected_text(self.cases[self.selected].name)
                    .show_ui(ui, |ui| {
                        for (i, case) in self.cases.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, i, case.name);
                        }
                    });
                // The button is drawn on every frame, so the row keeps its
                // layout on the frame a new case is picked.
                let reset = ui.button("Reset").clicked();
                if self.selected != before || reset {
                    self.reset_attributes();
                }
                ui.label("Attributes");
                let field = TextEdit::singleline(&mut self.edit)
                    .desired_width(f32::INFINITY)
                    .hint_text("Name=value, separated by commas");
                if ui.add(field).lost_focus() {
                    self.applied = self.edit.clone();
                }
            });
        });
        CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            let visuals = ui.visuals();
            let (foreground, background) = (visuals.text_color(), visuals.panel_fill);
            let key = DrawingKey::new(
                self.selected,
                &self.applied,
                rect,
                ctx.pixels_per_point(),
                foreground,
                background,
            );
            if self.drawing.as_ref().is_none_or(|d| d.key != key) {
                let case = &self.cases[self.selected];
                let result = draw_case(ctx, rect, case, &self.applied, foreground, background);
                self.drawing = Some(Drawing { key, result });
            }
            match self.drawing.as_ref().map(|d| &d.result) {
                Some(Ok(shapes)) => ui.painter().extend(shapes.iter().cloned()),
                Some(Err(message)) => {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
                None => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::cases;
    use eframe::egui::{self, Color32, Pos2, RawInput, Rect, Shape};

    fn run<R>(f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0))),
            ..Default::default()
        };
        let mut f = Some(f);
        let mut result = None;
        let _ = ctx.run(input, |ctx| result = Some((f.take().unwrap())(ctx)));
        result.unwrap()
    }

    fn draw(rect: Rect, attr: &str) -> Result<Vec<Shape>, String> {
        let case = cases()[0];
        run(|ctx| draw_case(ctx, rect, &case, attr, Color32::WHITE, Color32::BLACK))
    }

    #[test]
    fn draw_case_refuses_a_tiny_rectangle() {
        let too_small = Err("The window is too small to draw in.".to_owned());
        assert_eq!(
            draw(Rect::from_min_size(Pos2::ZERO, egui::Vec2::ZERO), ""),
            too_small
        );
        let narrow = Rect::from_min_size(Pos2::ZERO, egui::vec2(99.0, 500.0));
        assert_eq!(draw(narrow, ""), too_small);
    }

    #[test]
    fn draw_case_reports_a_bad_attribute() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let err = draw(rect, "nosuch=1").unwrap_err();
        assert!(err.contains("nosuch"), "{err}");
    }

    #[test]
    fn draw_case_survives_a_huge_text_size() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        for attr in ["size=inf", "size=1000", "Size(title)=300"] {
            assert!(draw(rect, attr).is_ok(), "{attr}");
        }
    }

    #[test]
    fn a_new_screen_scale_draws_again() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let key = |ppp| DrawingKey::new(0, "", rect, ppp, Color32::WHITE, Color32::BLACK);
        assert_eq!(key(1.0), key(1.0));
        assert_ne!(key(1.0), key(2.0));
    }

    #[test]
    fn a_command_line_case_comes_first_and_is_selected() {
        let mut mine = cases()[3];
        mine.name = "mine";
        mine.attr = "Grid=1";
        let app = DemoApp::with_case(Some(mine));
        assert_eq!(app.cases.len(), 21);
        assert_eq!(app.cases[0].name, "mine");
        assert_eq!(app.selected, 0);
        assert_eq!(
            (app.edit.as_str(), app.applied.as_str()),
            ("Grid=1", "Grid=1")
        );
    }

    #[test]
    fn draw_case_draws_a_grid() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let shapes = draw(rect, cases()[0].attr).unwrap();
        assert!(!shapes.is_empty());
    }
}
