// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A window that draws the coordinate grid of a Starlink AST Plot through
//! egui.
//!
//! The user picks one of the grid test cases, each a FITS header with a
//! plot box and some Plot attributes, edits the attributes, and sees the
//! grid drawn with egui's painter and measured with egui's fonts.
//! Run it with `cargo run -p ast-plot-demo --release`.

pub mod app;
pub mod cases;
pub mod grf;
pub mod settings;
