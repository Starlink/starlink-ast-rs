// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A backend that logs each drawing call.

use std::io::Write;

use super::page::VirtualPage;
use super::{Grf, GrfAttr, GrfCap, GrfPrimitive, Justification, TextBox, check_points, io_error};
use crate::error::AstResult;
use crate::frames::axis_format::sprintf_double;

/// A [`Grf`] that draws nothing and writes one line of text for each
/// drawing call, describing it.
///
/// It answers queries from the page described in the [module
/// documentation](super), so a Plot lays out a drawing on it as it would on
/// a real page of that size.
/// Its log is line for line what the C library's logging graphics module
/// writes for the same calls, which makes it suited to testing a Plot.
///
/// Call [`finish`](LogGrf::finish) when drawing is done, to flush the log
/// and get the writer back.
///
/// # Examples
///
/// ```
/// use starlink_ast::grf::{Grf, LogGrf};
///
/// let mut grf = LogGrf::new(Vec::new());
/// grf.line(&[0.1, 0.9], &[0.2, 0.8]).unwrap();
/// let log = String::from_utf8(grf.finish().unwrap()).unwrap();
/// assert_eq!(log, "GLine from=(0.100,0.200) to=(0.900,0.800)\n");
/// ```
// C: the call log of `grf_log.c`.
#[derive(Debug)]
pub struct LogGrf<W: Write> {
    out: W,
    page: VirtualPage,
}

/// `%.3f` of a graphics coordinate, as C prints a `float`.
fn f3(v: f32) -> String {
    sprintf_double("%.3f", f64::from(v))
}

fn attr_name(attr: GrfAttr) -> &'static str {
    match attr {
        GrfAttr::Style => "STYLE",
        GrfAttr::Width => "WIDTH",
        GrfAttr::Size => "SIZE",
        GrfAttr::Font => "FONT",
        GrfAttr::Colour => "COLOUR",
    }
}

fn prim_name(prim: GrfPrimitive) -> &'static str {
    match prim {
        GrfPrimitive::Text => "TEXT",
        GrfPrimitive::Line => "LINE",
        GrfPrimitive::Mark => "MARK",
    }
}

impl<W: Write> LogGrf<W> {
    /// A backend that writes its log to `out`, with every attribute of
    /// every primitive 1.
    pub fn new(out: W) -> Self {
        Self {
            out,
            page: VirtualPage::new(),
        }
    }

    /// Flush the log and return the writer.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Grfer` when the writer cannot be flushed.
    pub fn finish(mut self) -> AstResult<W> {
        self.out.flush().map_err(io_error)?;
        Ok(self.out)
    }

    fn write(&mut self, line: &str) -> AstResult<()> {
        self.out.write_all(line.as_bytes()).map_err(io_error)
    }

    /// Log the scales, as C's GQch and GTxExt do through their call of
    /// GScales.
    fn log_scales(&mut self) -> AstResult<[f32; 2]> {
        let [alpha, beta] = self.page.scales();
        self.write(&format!("GScales alpha={} beta={}\n", f3(alpha), f3(beta)))?;
        Ok([alpha, beta])
    }
}

impl<W: Write> Grf for LogGrf<W> {
    // C: astGLine logs only a line whose ends differ.
    fn line(&mut self, x: &[f32], y: &[f32]) -> AstResult<()> {
        check_points("line", x, y)?;
        let n = x.len();
        if n > 1 && (x[0] != x[n - 1] || y[0] != y[n - 1]) {
            self.write(&format!(
                "GLine from=({},{}) to=({},{})\n",
                f3(x[0]),
                f3(y[0]),
                f3(x[n - 1]),
                f3(y[n - 1])
            ))?;
        }
        Ok(())
    }

    // C: astGMark logs the first point only.
    fn mark(&mut self, x: &[f32], y: &[f32], marker: i32) -> AstResult<()> {
        check_points("mark", x, y)?;
        if !x.is_empty() {
            self.write(&format!(
                "GMark type={marker} at=({},{})\n",
                f3(x[0]),
                f3(y[0])
            ))?;
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
        let [jv, jh] = just.code();
        self.write(&format!(
            "GText \"{text}\" x={} y={} just={}{} up=({},{})\n",
            f3(x),
            f3(y),
            jv as char,
            jh as char,
            f3(up[0]),
            f3(up[1])
        ))
    }

    fn text_extent(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<TextBox> {
        if !text.is_empty() {
            self.log_scales()?;
        }
        let bx = self
            .page
            .text_extent(text, x, y, just, up)?
            .unwrap_or(TextBox {
                x: [0.0; 4],
                y: [0.0; 4],
            });
        let [jv, jh] = just.code();
        self.write(&format!(
            "GTxExt \"{text}\" x={} y={} just={}{} box=({},{})-({},{})-({},{})-({},{})\n",
            f3(x),
            f3(y),
            jv as char,
            jh as char,
            f3(bx.x[0]),
            f3(bx.y[0]),
            f3(bx.x[1]),
            f3(bx.y[1]),
            f3(bx.x[2]),
            f3(bx.y[2]),
            f3(bx.x[3]),
            f3(bx.y[3])
        ))?;
        Ok(bx)
    }

    fn char_height(&mut self) -> AstResult<[f32; 2]> {
        self.log_scales()?;
        let [chv, chh] = self.page.char_height();
        self.write(&format!(
            "GQch chv={} chh={}\n",
            sprintf_double("%.6f", f64::from(chv)),
            sprintf_double("%.6f", f64::from(chh))
        ))?;
        Ok([chv, chh])
    }

    fn scales(&mut self) -> AstResult<[f32; 2]> {
        self.log_scales()
    }

    // C: astGAttr logs only when it sets a value.
    fn attr(&mut self, attr: GrfAttr, value: Option<f64>, prim: GrfPrimitive) -> AstResult<f64> {
        let (old, set) = self.page.attr(attr, value, prim);
        if let Some(v) = set {
            self.write(&format!(
                "GAttr {}={} prim={}\n",
                attr_name(attr),
                sprintf_double("%.4f", v),
                prim_name(prim)
            ))?;
        }
        Ok(old)
    }

    fn cap(&mut self, cap: GrfCap, _value: i32) -> bool {
        cap == GrfCap::Scales
    }

    fn flush(&mut self) -> AstResult<()> {
        self.write("GFlush\n")?;
        self.out.flush().map_err(io_error)
    }
}
