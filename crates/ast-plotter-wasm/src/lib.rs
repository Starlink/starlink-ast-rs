// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The work of the AST FITS plotter page, in Rust.
//!
//! [`plot`] follows the steps of `fplottest.f`, the Fortran program behind the
//! page the C library's authors host: it reads a FITS header through a
//! FitsChan, writes the coordinate system back out as FITS-WCS cards and as
//! Native text, and draws an annotated grid of it as SVG.
//! `tools/oracle_fixtures/fits_plotter_probe.c` runs the same steps on the C
//! library, and the crate's tests compare the two.

mod examples;

pub use examples::{Example, parse_examples};

use starlink_ast::attributes::Attributes;
use starlink_ast::error::AstError;
use starlink_ast::frames::{Frame, FrameRef};
use starlink_ast::grf::{SvgGrf, SvgOptions};
use starlink_ast::plot::{Draw, Plot};
use starlink_ast::{AnyObject, Channel, Encoding, FitsChan, NativeChannel};

/// The smallest image side [`plot`] draws, in pixels.
pub const MIN_SIZE: u32 = 16;
/// The largest image side [`plot`] draws, in pixels.
pub const MAX_SIZE: u32 = 4096;

/// What to plot.
#[derive(Debug, Clone)]
pub struct PlotRequest {
    /// The FITS header, one card per line.
    pub header: String,
    /// Plot attribute settings, as a comma-separated list.
    pub plot_settings: String,
    /// FitsChan attribute settings, used when reading the header.
    pub fitschan_settings: String,
    /// The image width in pixels, from 16 to 4096.
    pub width: u32,
    /// The image height in pixels, from 16 to 4096.
    pub height: u32,
    /// The version character of the axes to plot: `' '` for the primary
    /// axes, `'$'` for pixel coordinates, or the letter of alternate axes.
    pub version: char,
    /// Draw as the page shows the grid: as PGPLOT lays it out, on a centred
    /// square with a margin for the labels and text sized to it, and labels
    /// drawn over a cleared box. Off, the SVG
    /// is the C library's logging backend's, byte for byte.
    pub page_look: bool,
}

/// The three outputs. Each is the output text, or the messages that say why
/// it could not be made, one per line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotResult {
    /// The annotated grid, as an SVG document.
    pub svg: Result<String, String>,
    /// The header rewritten as FITS-WCS, one 80-character card per line,
    /// after the cards the read left behind.
    pub fits: Result<String, String>,
    /// The coordinate system as Native text.
    pub ast: Result<String, String>,
}

/// The message lines of an error.
fn messages(e: &AstError) -> String {
    std::iter::once(e.message())
        .chain(e.context().iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The same failure for every output.
fn all(message: String) -> PlotResult {
    PlotResult {
        svg: Err(message.clone()),
        fits: Err(message.clone()),
        ast: Err(message),
    }
}

/// `NAXISn` as `fplottest.f` reads it: the integer at the start of the
/// card's value field, or 100 if the keyword is absent or holds none.
fn naxis(fc: &mut FitsChan, key: &str) -> Result<i32, AstError> {
    fc.clear_card()?;
    let Some(card) = fc.find_fits(key, true)? else {
        return Ok(100);
    };
    let value = card.get(10..).unwrap_or("").trim_start();
    let digits = value
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '+' || c == '-')))
        .last()
        .map_or(0, |(i, c)| i + c.len_utf8());
    Ok(value[..digits].parse().unwrap_or(100))
}

/// Reads the header: the FitsChan after the read, the object read and the
/// plot box.
fn read(request: &PlotRequest) -> Result<(FitsChan, Option<AnyObject>, [f64; 4]), AstError> {
    let mut fc = FitsChan::new()?;
    fc.set_attributes(&request.fitschan_settings)?;
    for line in request.header.lines() {
        fc.put_fits(line, false)?;
    }
    let pbox = [
        0.5,
        0.5,
        f64::from(naxis(&mut fc, "NAXIS1")?) + 0.5,
        f64::from(naxis(&mut fc, "NAXIS2")?) + 0.5,
    ];
    fc.clear_card()?;
    let object = fc.read()?;
    Ok((fc, object, pbox))
}

/// The FITS-WCS cards: written at the start of the FitsChan, which is
/// rewound rather than emptied, then every card listed.
fn fits_text(fc: &mut FitsChan, object: &AnyObject) -> Result<String, String> {
    let mut run = || -> Result<Option<String>, AstError> {
        fc.clear_card()?;
        fc.set_encoding(Encoding::FitsWcs);
        if fc.write(object)? == 0 {
            return Ok(None);
        }
        fc.clear_card()?;
        let mut text = String::new();
        while let Some(card) = fc.find_fits("%f", true)? {
            text.push_str(&card);
            text.push('\n');
        }
        Ok(Some(text))
    };
    match run() {
        Ok(Some(text)) => Ok(text),
        Ok(None) => Err("Failed to create new FITS headers".to_string()),
        Err(e) => Err(messages(&e)),
    }
}

/// The object as Native text.
fn ast_text(object: &AnyObject) -> Result<String, String> {
    let mut channel = NativeChannel::with_string_sink();
    match channel.write(object) {
        Ok(0) => Err("Failed to write FrameSet to standard output".to_string()),
        Ok(_) => Ok(channel.into_string()),
        Err(e) => Err(messages(&e)),
    }
}

/// Makes current the Frame the version character names.
fn select_version(object: &mut AnyObject, version: char) -> Result<(), String> {
    if version == ' ' {
        return Ok(());
    }
    let not_found = || {
        format!(
            "The supplied FITS headers do not define a coordinate system with the \
             requested version character '{version}'."
        )
    };
    // A FitsChan read gives a FrameSet; anything else has no other axes.
    let AnyObject::FrameSet(fs) = object else {
        return Err(not_found());
    };
    if version == '$' {
        return fs.set_current(FrameRef::Base).map_err(|e| messages(&e));
    }
    let ident = version.to_string();
    for i in 1..=fs.nframe() {
        if fs
            .frame(FrameRef::Index(i))
            .map_err(|e| messages(&e))?
            .ident()
            == ident
        {
            return fs.set_current(FrameRef::Index(i)).map_err(|e| messages(&e));
        }
    }
    Err(not_found())
}

/// `fplottest.f`'s graphics box: the unit square of its equal-aspect PGPLOT
/// window less 5% on each side, then narrowed on one axis to the plot box's
/// aspect ratio, all in single precision as the Fortran is.
fn graphics_box(pbox: [f64; 4]) -> [f32; 4] {
    let mut g = [0.0f32, 0.0, 1.0, 1.0];
    let range = g[2] - g[0];
    g[0] += 0.05 * range;
    g[2] -= 0.05 * range;
    let range = g[3] - g[1];
    g[1] += 0.05 * range;
    g[3] -= 0.05 * range;
    let mut asp = ((pbox[3] - pbox[1]) as f32 / (pbox[2] - pbox[0]) as f32).abs();
    if !(0.2..=5.0).contains(&asp) {
        asp = 1.0;
    }
    if asp > 1.0 {
        let delta = (asp - 1.0) * (g[2] - g[0]) / (2.0 * asp);
        g[2] -= delta;
        g[0] += delta;
    } else {
        let delta = (1.0 - asp) * (g[3] - g[1]) / 2.0;
        g[3] -= delta;
        g[1] += delta;
    }
    g
}

/// The annotated grid as SVG.
fn svg_text(request: &PlotRequest, frame: &dyn Frame, pbox: [f64; 4]) -> Result<String, String> {
    let sizes = MIN_SIZE..=MAX_SIZE;
    if !sizes.contains(&request.width) || !sizes.contains(&request.height) {
        return Err(format!(
            "The image size {} by {} pixels is outside {MIN_SIZE} to {MAX_SIZE} pixels.",
            request.width, request.height
        ));
    }
    let mut options = SvgOptions::default();
    options.width = request.width;
    options.height = request.height;
    options.erase_under_text = request.page_look;
    options.equal_aspect = request.page_look;
    let run = || -> Result<Vec<u8>, AstError> {
        let mut grf = SvgGrf::new(Vec::new(), options)?;
        let mut plot = Plot::new(frame, graphics_box(pbox), pbox)?;
        plot.set_attributes(&request.plot_settings)?;
        plot.draw_grid(&mut grf)?;
        grf.finish()
    };
    let bytes = run().map_err(|e| messages(&e))?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Reads the header and makes the three outputs, as `fplottest.f` does.
///
/// A failure to read the header, or a header with no coordinate system,
/// fails every output; otherwise each output fails or succeeds on its own.
pub fn plot(request: &PlotRequest) -> PlotResult {
    let (mut fc, object, pbox) = match read(request) {
        Ok(read) => read,
        Err(e) => return all(messages(&e)),
    };
    let Some(mut object) = object else {
        return all("!!! No object read from file!!!".to_string());
    };
    let fits = fits_text(&mut fc, &object);
    let ast = ast_text(&object);
    let svg = select_version(&mut object, request.version).and_then(|()| {
        let frame = object
            .as_frame()
            .ok_or_else(|| "The object read from the header is not a Frame.".to_string())?;
        svg_text(request, frame, pbox)
    });
    PlotResult { svg, fits, ast }
}
