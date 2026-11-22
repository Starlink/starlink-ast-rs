// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A backend that draws to SVG.

use std::io::Write;

use super::page::{DEFAULT_CHR_HT, TextMetrics, VirtualPage};
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
    /// Fill each text's box with the background before drawing the text,
    /// so lines drawn earlier stop short of labels, as the C library's
    /// PGPLOT and PLplot backends draw them. The default is `false`, as the
    /// C library's logging backend draws.
    pub erase_under_text: bool,
    /// Draw as PGPLOT draws after `PGWNAD(0, 1, 0, 1)` on its default
    /// viewport, rather than stretching graphics coordinates 0 to 1 over the
    /// image's width and height separately: they span the largest square
    /// centred inside a margin of four character heights, which leaves room
    /// for a Plot's labels outside its graphics box, and text and markers are
    /// drawn at the size a Plot lays them out for, characters 1/40 of the
    /// image's shorter side high at a text size of 1. Text turns the way its
    /// up vector points, at its exact angle, rather than the other way in
    /// 5-degree steps, and is drawn in Helvetica, or Arial or Liberation
    /// Sans, and measured with their widths, so the pieces of a label a Plot
    /// lays out side by side sit as close as their letters. The default is `false`, as the C library's logging
    /// backend draws.
    pub equal_aspect: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            width: 720,
            height: 540,
            erase_under_text: false,
            equal_aspect: false,
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
/// Text is turned as the C library's logging backend turns it, the
/// opposite way to its up vector, by a multiple of 5 degrees: an up vector
/// 94 degrees anticlockwise from upright turns it 90 degrees clockwise, and
/// one 12 degrees clockwise turns it 15 anticlockwise.
/// [`SvgOptions::equal_aspect`] turns text the way its up vector points.
/// Line styles and fonts are not drawn.
/// Queries are answered from the page described in the [module
/// documentation](super), so a Plot lays out its drawing for a 200 mm
/// page whatever the image size.
///
/// Text is written into the SVG with `&`, `<`, `>` and `"` escaped.
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
    erase_under_text: bool,
    /// Where graphics coordinates are drawn, with `equal_aspect`.
    square: Option<Square>,
}

/// The square graphics coordinates 0 to 1 span with `equal_aspect`, as
/// PGPLOT lays it out: `pgopen.f` sets the character height to 1 and calls
/// `PGVSTD`, whose viewport is the view surface less 4 character heights all
/// round, and `PGWNAD` then shrinks that viewport to the largest centred
/// square. At height 1 a character is 1/40 of the view surface's shorter
/// side (`pgsch.f`).
#[derive(Debug, Clone, Copy)]
struct Square {
    /// The square's side in pixels.
    side: f32,
    /// The pixel column of its left edge.
    x0: f32,
    /// The pixel row of its top edge.
    y0: f32,
    /// The pixels of a character's height at text size 1.
    char_height: f32,
}

impl Square {
    fn new(width: f32, height: f32) -> Self {
        let char_height = width.min(height) / 40.0;
        let margin = 4.0 * char_height;
        let side = (width - 2.0 * margin).min(height - 2.0 * margin);
        Self {
            side,
            x0: (width - side) / 2.0,
            y0: (height - side) / 2.0,
            char_height,
        }
    }
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
        let square = options
            .equal_aspect
            .then(|| Square::new(width as f32, height as f32));
        // The page's side in mm is what makes a character as high on it as
        // the square draws one.
        let page = square.map_or_else(VirtualPage::new, |sq| {
            VirtualPage::with_side_mm(
                DEFAULT_CHR_HT * f64::from(sq.side / sq.char_height),
                TextMetrics::Helvetica,
            )
        });
        Ok(Self {
            out,
            width,
            height,
            page,
            erase_under_text: options.erase_under_text,
            square,
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
        match self.square {
            Some(sq) => f64::from(sq.x0 + wx * sq.side).round() as i32,
            None => f64::from(wx * self.width as f32).round() as i32,
        }
    }

    /// The pixel row of graphics y coordinate `wy`.
    // C: SvgY (grf_log.c:135).
    fn svg_y(&self, wy: f32) -> i32 {
        match self.square {
            Some(sq) => f64::from(sq.y0 + sq.side - wy * sq.side).round() as i32,
            None => {
                let h = self.height as f32;
                f64::from(h - wy * h).round() as i32
            }
        }
    }

    /// The pixels per text-size unit: 14, as C draws, or PGPLOT's character
    /// height with `equal_aspect`.
    fn text_scale(&self) -> f64 {
        self.square.map_or(14.0, |sq| f64::from(sq.char_height))
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
        (self.text_scale() * scale).round() as i32
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
        // 2 pixels, as C draws, scaled with text under equal_aspect.
        let radius = (2.0 * self.text_scale() / 14.0).round() as i32;
        let mut svg = String::new();
        for (&px, &py) in x.iter().zip(y) {
            svg.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{radius}\" fill=\"{fill}\"/>\n",
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
        if self.erase_under_text
            && let Some(tbox) = self.page.text_extent(text, x, y, just, up)?
        {
            let mut poly = String::from("<polygon fill=\"black\" points=\"");
            for i in 0..4 {
                poly.push_str(&format!(
                    "{},{} ",
                    self.svg_x(tbox.x[i]),
                    self.svg_y(tbox.y[i])
                ));
            }
            poly.push_str("\"/>\n");
            self.write(&poly)?;
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
        // Drawing as PGPLOT does, the fonts the page measures text with.
        let family = if self.square.is_some() {
            "Helvetica, Arial, 'Liberation Sans', sans-serif"
        } else {
            "sans-serif"
        };
        let mut svg = format!(
            "<text x=\"{sx}\" y=\"{sy}\" font-family=\"{family}\" font-size=\"{}\" \
             fill=\"{}\" text-anchor=\"{anchor}\" dominant-baseline=\"{baseline}\"",
            self.font_size(),
            self.colour(GrfPrimitive::Text)
        );
        if angle.abs() > 0.01 {
            // `angle` is anticlockwise with y upwards, and SVG turns
            // clockwise with y downwards, so C's SVG turns text the wrong
            // way. Drawing as PGPLOT does, the text turns the right way, at
            // the angle its extent was laid out at, to a tenth of a degree.
            let rotate = if self.square.is_some() {
                format!("{}", (-angle * 10.0).round() / 10.0)
            } else {
                let mut iangle = (angle / 5.0) as i32 * 5;
                if angle < 0.0 && iangle != angle as i32 {
                    iangle -= 5;
                }
                iangle.to_string()
            };
            svg.push_str(&format!(" transform=\"rotate({rotate},{sx},{sy})\""));
        }
        svg.push_str(&format!(">{}</text>\n", xml_escape(text)));
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

    fn flush(&mut self) -> AstResult<()> {
        self.out.flush().map_err(io_error)
    }
}

/// Text with the characters XML gives a meaning escaped.
// C: grf_log.c writes text as given, so a label holding `<` or `&` makes its
// SVG invalid (c-library-quirks.md).
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters an XML document cannot hold, even as a reference.
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}
