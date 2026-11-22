// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The JavaScript interface, through wasm-bindgen.

use wasm_bindgen::prelude::*;

use crate::{PlotRequest, PlotResult};

/// The three outputs of [`plot`]: each output's text, or the messages that
/// say why it could not be made, whichever there is; the other is
/// `undefined`.
#[wasm_bindgen]
pub struct PlotOutput {
    result: PlotResult,
}

#[wasm_bindgen]
impl PlotOutput {
    /// The annotated grid as an SVG document.
    pub fn svg(&self) -> Option<String> {
        self.result.svg.clone().ok()
    }

    /// Why the grid could not be drawn, one message per line.
    pub fn svg_error(&self) -> Option<String> {
        self.result.svg.clone().err()
    }

    /// The header rewritten as FITS-WCS, one card per line.
    pub fn fits(&self) -> Option<String> {
        self.result.fits.clone().ok()
    }

    /// Why the FITS-WCS cards could not be written, one message per line.
    pub fn fits_error(&self) -> Option<String> {
        self.result.fits.clone().err()
    }

    /// The coordinate system as Native text.
    pub fn ast(&self) -> Option<String> {
        self.result.ast.clone().ok()
    }

    /// Why the Native text could not be written, one message per line.
    pub fn ast_error(&self) -> Option<String> {
        self.result.ast.clone().err()
    }
}

/// Reads a FITS header and makes the plotter's three outputs.
///
/// `version` is the version character of the axes to plot, its first
/// character used: empty or a space for the primary axes, `$` for pixel
/// coordinates, or the letter of alternate axes. `page_look` draws on the
/// centred square PGPLOT would draw on, with text sized to it, as the page
/// shows it.
#[wasm_bindgen]
pub fn plot(
    header: String,
    plot_settings: String,
    fitschan_settings: String,
    width: u32,
    height: u32,
    version: String,
    page_look: bool,
) -> PlotOutput {
    let request = PlotRequest {
        header,
        plot_settings,
        fitschan_settings,
        width,
        height,
        version: version.chars().next().unwrap_or(' '),
        page_look,
    };
    PlotOutput {
        result: crate::plot(&request),
    }
}

/// Panics, so that the page's smoke test can check it recovers from a trap.
#[cfg(feature = "smoke-test")]
#[wasm_bindgen]
pub fn __trap_for_test() {
    panic!("trap requested by the smoke test");
}
