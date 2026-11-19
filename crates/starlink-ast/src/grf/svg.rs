// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A backend that draws to SVG.

use std::io::Write;

use super::page::VirtualPage;
use super::{
    Grf, GrfAttr, GrfCap, GrfPrimitive, HJust, Justification, TextBox, VJust, check_points,
    io_error,
};
use crate::error::AstResult;

/// Radians to degrees, as the C library writes the constant.
// C: grf_log.c:66.
#[allow(clippy::excessive_precision)]
const R2D: f64 = 57.29577951308232;

/// The colours of colour indices 0 to 15, PLplot's colour map 0. Any other
/// index draws in colour 1.
const CMAP0: [&str; 16] = [
    "#000000", "#ff0000", "#ffff00", "#00ff00", "#7fffd4", "#ffc0cb", "#f5deb3", "#808080",
    "#a52a2a", "#0000ff", "#8a2be2", "#00ffff", "#f0e68c", "#fa8072", "#c0c0c0", "#ffffff",
];

/// Options for [`SvgGrf`].
///
/// Start from the default and set the fields wanted, so that code keeps
/// working as options are added:
///
/// ```
/// use starlink_ast::grf::SvgOptions;
///
/// let mut options = SvgOptions::default();
/// options.width = 1024;
/// options.height = 768;
/// ```
// Only Debug and Clone are derived, so that options of any type can be
// added without removing a derive.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SvgOptions {
    /// The width of the image in pixels; graphics x coordinates 0 to 1 span
    /// it. The default is 720.
    pub width: u32,
    /// The height of the image in pixels; graphics y coordinates 0 to 1
    /// span it, upwards. The default is 540.
    pub height: u32,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            width: 720,
            height: 540,
        }
    }
}

/// A [`Grf`] that draws to SVG, on a black background.
///
/// Graphics coordinates 0 to 1 span the image on each axis, with y upwards.
/// Lines and markers are drawn in the colour their colour attribute
/// indexes in PLplot's 16-colour map (any other index draws in red),
/// markers as small filled circles of every type, and text in a
/// sans-serif font 14 pixels high at a text size of 1.
/// Text is rotated by a multiple of 5 degrees: its angle is truncated
/// towards zero to a multiple of 5, and a negative angle whose whole
/// degrees are not that multiple goes 5 degrees further, so 94 degrees
/// draws at 90 and -12 at -15.
/// Line styles and fonts are not drawn.
/// Queries are answered from the page described in the [module
/// documentation](super), so a Plot lays out its drawing for a 200 mm
/// page whatever the image size.
///
/// Text is written into the SVG as given, without escaping, so text that
/// holds `<` or `&` makes the SVG invalid.
///
/// The SVG is complete only once [`finish`](SvgGrf::finish) has written
/// its closing tag.
///
/// # Examples
///
/// ```
/// use starlink_ast::grf::{Grf, SvgGrf, SvgOptions};
///
/// let mut grf = SvgGrf::new(Vec::new(), SvgOptions::default()).unwrap();
/// grf.line(&[0.0, 1.0], &[0.0, 1.0]).unwrap();
/// let svg = String::from_utf8(grf.finish().unwrap()).unwrap();
/// assert!(svg.contains("points=\"0,540 720,0 \""));
/// ```
// C: the SVG output of grf_log.c, with its page size from
// astGrfLogSetSvg. The look of the output is C's; later options will make
// the background, colours and font metrics selectable.
#[derive(Debug)]
pub struct SvgGrf<W: Write> {
    out: W,
    width: i32,
    height: i32,
    page: VirtualPage,
}

impl<W: Write> SvgGrf<W> {
    /// A backend that writes SVG to `out`, starting with the SVG header and
    /// background, with every attribute of every primitive 1.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when `out` cannot be written, or a page
    ///   dimension does not fit in an `i32`.
    // C: astGrfLogInit and astGrfLogSetSvg (grf_log.c:93, :105).
    pub fn new(mut out: W, options: SvgOptions) -> AstResult<Self> {
        let to_i32 = |v: u32| {
            i32::try_from(v).map_err(|_| {
                crate::error::AstError::new(
                    crate::error::ErrorCode::Grfer,
                    format!("SVG page dimension {v} is too large."),
                )
            })
        };
        let (width, height) = (to_i32(options.width)?, to_i32(options.height)?);
        write!(
            out,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <svg xmlns=\"http://www.w3.org/2000/svg\" \
             width=\"{width}\" height=\"{height}\" \
             viewBox=\"0 0 {width} {height}\">\n\
             <rect width=\"100%\" height=\"100%\" fill=\"black\"/>\n"
        )
        .map_err(io_error)?;
        Ok(Self {
            out,
            width,
            height,
            page: VirtualPage::new(),
        })
    }

    /// Write the closing tag of the SVG, flush it, and return the writer.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the writer cannot be written or flushed.
    // C: astGrfLogClose (grf_log.c:120).
    pub fn finish(mut self) -> AstResult<W> {
        self.out.write_all(b"</svg>\n").map_err(io_error)?;
        self.out.flush().map_err(io_error)?;
        Ok(self.out)
    }

    /// The pixel column of graphics x coordinate `wx`.
    // C: SvgX (grf_log.c:131), `(int) round( wx * (float)svg_w )`.
    fn svg_x(&self, wx: f32) -> i32 {
        f64::from(wx * self.width as f32).round() as i32
    }

    /// The pixel row of graphics y coordinate `wy`.
    // C: SvgY (grf_log.c:135).
    fn svg_y(&self, wy: f32) -> i32 {
        let h = self.height as f32;
        f64::from(h - wy * h).round() as i32
    }

    /// The colour a primitive is drawn in.
    // C: SvgColour (grf_log.c:139).
    fn colour(&self, prim: GrfPrimitive) -> &'static str {
        let idx = self.page.get(GrfAttr::Colour, prim).round() as i32;
        usize::try_from(idx)
            .ok()
            .and_then(|i| CMAP0.get(i))
            .copied()
            .unwrap_or(CMAP0[1])
    }

    /// The font size of text, in pixels.
    // C: SvgFontSize (grf_log.c:145).
    fn font_size(&self) -> i32 {
        let mut scale = self.page.get(GrfAttr::Size, GrfPrimitive::Text);
        if scale <= 0.0 {
            scale = 1.0;
        }
        (14.0 * scale).round() as i32
    }

    fn write(&mut self, text: &str) -> AstResult<()> {
        self.out.write_all(text.as_bytes()).map_err(io_error)
    }
}

impl<W: Write> Grf for SvgGrf<W> {
    // C: the SVG half of astGLine (grf_log.c:334).
    fn line(&mut self, x: &[f32], y: &[f32]) -> AstResult<()> {
        check_points("line", x, y)?;
        if x.len() > 1 {
            let mut w = self.page.get(GrfAttr::Width, GrfPrimitive::Line);
            if w < 1.0 {
                w = 1.0;
            }
            let mut svg = format!(
                "<polyline fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" points=\"",
                self.colour(GrfPrimitive::Line),
                w.round() as i32
            );
            for (&px, &py) in x.iter().zip(y) {
                svg.push_str(&format!("{},{} ", self.svg_x(px), self.svg_y(py)));
            }
            svg.push_str("\"/>\n");
            self.write(&svg)?;
        }
        Ok(())
    }

    // C: the SVG half of astGMark (grf_log.c:353); every marker type is a
    // circle.
    fn mark(&mut self, x: &[f32], y: &[f32], _marker: i32) -> AstResult<()> {
        check_points("mark", x, y)?;
        let fill = self.colour(GrfPrimitive::Mark);
        let mut svg = String::new();
        for (&px, &py) in x.iter().zip(y) {
            svg.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"2\" fill=\"{fill}\"/>\n",
                self.svg_x(px),
                self.svg_y(py)
            ));
        }
        self.write(&svg)
    }

    // C: the SVG half of astGText (grf_log.c:291). The angle is truncated
    // towards zero to a multiple of 5 degrees, and a negative angle whose
    // own truncation differs from that is moved 5 degrees further, so -12
    // draws at -15 and -10.5 at -10.
    fn text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        let (sx, sy) = (self.svg_x(x), self.svg_y(y));
        let angle = -crate::math::atan2(f64::from(up[0]), f64::from(up[1])) * R2D;
        let anchor = match just.horizontal {
            HJust::Left => "start",
            HJust::Right => "end",
            HJust::Centre => "middle",
        };
        let baseline = match just.vertical {
            VJust::Top => "hanging",
            VJust::Baseline => "alphabetic",
            VJust::Centre | VJust::Bottom => "central",
        };
        let mut svg = format!(
            "<text x=\"{sx}\" y=\"{sy}\" font-family=\"sans-serif\" font-size=\"{}\" \
             fill=\"{}\" text-anchor=\"{anchor}\" dominant-baseline=\"{baseline}\"",
            self.font_size(),
            self.colour(GrfPrimitive::Text)
        );
        if angle.abs() > 0.01 {
            let mut iangle = (angle / 5.0) as i32 * 5;
            if angle < 0.0 && iangle != angle as i32 {
                iangle -= 5;
            }
            svg.push_str(&format!(" transform=\"rotate({iangle},{sx},{sy})\""));
        }
        svg.push_str(&format!(">{text}</text>\n"));
        self.write(&svg)
    }

    fn text_extent(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<TextBox> {
        Ok(self
            .page
            .text_extent(text, x, y, just, up)?
            .unwrap_or(TextBox {
                x: [0.0; 4],
                y: [0.0; 4],
            }))
    }

    fn char_height(&mut self) -> AstResult<[f32; 2]> {
        Ok(self.page.char_height())
    }

    fn scales(&mut self) -> AstResult<[f32; 2]> {
        Ok(self.page.scales())
    }

    fn attr(&mut self, attr: GrfAttr, value: Option<f64>, prim: GrfPrimitive) -> AstResult<f64> {
        Ok(self.page.attr(attr, value, prim).0)
    }

    fn cap(&mut self, cap: GrfCap, _value: i32) -> bool {
        cap == GrfCap::Scales
    }

    fn begin_buffer(&mut self) -> AstResult<()> {
        Ok(())
    }

    fn end_buffer(&mut self) -> AstResult<()> {
        Ok(())
    }

    fn flush(&mut self) -> AstResult<()> {
        self.out.flush().map_err(io_error)
    }
}
