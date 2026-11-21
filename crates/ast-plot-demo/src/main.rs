// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Opens the demo's window.

use ast_plot_demo::app::DemoApp;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AST Plot demo",
        options,
        Box::new(|_cc| Ok(Box::<DemoApp>::default())),
    )
}
