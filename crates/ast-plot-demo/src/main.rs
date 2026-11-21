// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Opens the demo's window, on the FITS header the command line names, if
//! any; `ast-plot-demo --help` says how.

use ast_plot_demo::app::DemoApp;
use ast_plot_demo::cli::{USAGE, case_from_args};
use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let first = match case_from_args(&args, |path| std::fs::read_to_string(path)) {
        Ok(first) => first,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AST Plot demo",
        options,
        Box::new(move |_cc| Ok(Box::new(DemoApp::with_case(first)))),
    )
}
