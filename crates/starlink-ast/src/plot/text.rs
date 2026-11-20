// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Text: justified and rotated strings, with the escape sequences that
//! change their appearance part way through.
//!
//! An escape sequence starts with `%` and ends with `+`; between them a
//! letter says what to change and a number how much. A Plot draws the
//! pieces of a string between escape sequences itself, moving and
//! restyling each, unless the backend reports that it interprets escape
//! sequences, in which case the whole string is passed on.
// C: plot.c Text (:26313), DrawText (:9743), JustMB (:20373), RightVector
// (:24708), UpdateConcat (:26907), InterpEscape (:20059), astFindEscape
// (:12086), HasEscapes (:19484), PushGat (:24460), PopGat (:24313), ChrLen
// (:6300), GText (:15273), GTxExt (:15384), GQch (:15166).
//
// C computes much of this in float with double intermediates wherever a
// double literal or a sqrt enters an expression; the port casts at the same
// places, since the boxes it finds decide where labels go.

use super::Plot;
use super::elements::PlotElement;
use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::grf::{Grf, GrfAttr, GrfCap, GrfPrimitive, Justification};
use crate::mappings::Mapping;
use crate::math::sqrt;
use crate::serial::native_write::format_double_g;

/// The kinds of escape sequence.
// C: GRF__ESPER .. GRF__ESPOP (grf.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Escape {
    /// `%%`, a percent sign.
    Percent,
    /// `%^n+`, raise the baseline.
    Sup,
    /// `%vn+`, lower the baseline.
    Sub,
    /// `%>n+`, leave a gap.
    Gap,
    /// `%<n+`, move back.
    Back,
    /// `%sn+`, change the size.
    Size,
    /// `%wn+`, change the width.
    Width,
    /// `%fn+`, change the font.
    Font,
    /// `%cn+`, change the colour.
    Colour,
    /// `%g+`, go to the position marked by `%h+`.
    Go,
    /// `%h+`, mark the position.
    Mark,
    /// `%tn+`, change the style.
    Style,
    /// `%-`, push the attributes.
    Push,
    /// `%+`, pop them.
    Pop,
}

/// Saved text attributes, pushed by `%-` and popped by `%+`.
// C: AstGat (plot.h).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gat {
    rise: f32,
    size: f64,
    width: f64,
    font: f64,
    style: f64,
    col: f64,
}

/// The box a string occupies, as C's DrawText returns it: corners bottom
/// left, top left, top right, bottom right, and the distance from the
/// baseline down to the bottom of the box.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextBounds {
    pub(crate) xbn: [f32; 4],
    pub(crate) ybn: [f32; 4],
    pub(crate) drop: f32,
}

/// C's `isspace` in the "C" locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The length of a string without its trailing white space.
// C: ChrLen (plot.c:6300).
pub(crate) fn chr_len(text: &str) -> usize {
    let b = text.as_bytes();
    let mut ret = b.len();
    while ret > 0 && is_space(b[ret - 1]) {
        ret -= 1;
    }
    ret
}

/// C's `sscanf( s, "%d%n", ... )`: an optional sign after white space,
/// then at least one digit; the value (clamped to C's long range, then
/// truncated to int) and the number of characters read.
fn scan_int(s: &[u8]) -> Option<(i32, usize)> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let start = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add(i64::from(s[i] - b'0'));
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if neg { v.saturating_neg() } else { v };
    Some((v as i32, i))
}

/// Look for an escape sequence in `text`: whether one starts it, its kind
/// and value (-1 for a `+` that resets), and the number of characters to
/// skip: the escape sequence's own if one starts the text, else those
/// before the first escape sequence, else the whole text.
// C: astFindEscape (plot.c:12086). The digits must be followed by "+", but
// C's sscanf succeeds when they are not, and C then skips the character
// after the digits as if it were the "+" (c-library-quirks.md). Where the
// digits end the string, C's count runs past its end; the port stops at
// the end.
pub(crate) fn find_escape(text: &str) -> (bool, Escape, i32, usize) {
    let t = text.as_bytes();
    let mut kind = Escape::Percent;
    let mut value = 0;
    let mut perc = None;
    let mut b = None;
    let mut a = 0;
    while let Some(off) = t[a..].iter().position(|&c| c == b'%') {
        let p = a + off;
        perc = Some(p);
        a = p + 1;
        let next = t.get(a).copied().unwrap_or(0);
        kind = match next {
            b'%' => {
                kind = Escape::Percent;
                b = Some(a + 1);
                break;
            }
            b'^' => Escape::Sup,
            b'v' => Escape::Sub,
            b'>' => Escape::Gap,
            b'<' => Escape::Back,
            b's' => Escape::Size,
            b'w' => Escape::Width,
            b'f' => Escape::Font,
            b'c' => Escape::Colour,
            b'g' => Escape::Go,
            b'h' => Escape::Mark,
            b't' => Escape::Style,
            b'-' => {
                kind = Escape::Push;
                b = Some(a + 1);
                break;
            }
            b'+' => {
                kind = Escape::Pop;
                b = Some(a + 1);
                break;
            }
            _ => continue,
        };
        a += 1;
        if a < t.len() {
            if t[a] == b'+' {
                value = -1;
                b = Some(a + 1);
                break;
            } else if let Some((v, nd)) = scan_int(&t[a..]) {
                value = v;
                b = Some((a + nd + 1).min(t.len()));
                break;
            }
        }
    }
    match (b, perc) {
        (Some(b), Some(0)) => (true, kind, value, b),
        (Some(_), Some(p)) => (false, kind, value, p),
        _ => (false, kind, value, t.len()),
    }
}

/// Whether `text` holds an escape sequence.
// C: HasEscapes (plot.c:19484).
pub(crate) fn has_escapes(text: &str) -> bool {
    let (found, _, _, nc) = find_escape(text);
    found || nc < text.len()
}

/// A justification as C's two characters.
fn just_code(just: [u8; 2]) -> Justification {
    Justification::from_code(std::str::from_utf8(&just).unwrap_or("CC")).unwrap_or(
        Justification::new(crate::grf::VJust::Centre, crate::grf::HJust::Centre),
    )
}

/// Extend the limits of a string's box, in multiples of the up-vector
/// (`alpha`) and right-vector (`beta`) from `(x0, y0)`, by the box
/// `(xbn, ybn)`, and move the concatenation point `(x, y)` along the
/// baseline by the box's width.
// C: UpdateConcat (plot.c:26907).
#[allow(clippy::too_many_arguments)]
fn update_concat(
    xbn: &[f32; 4],
    ybn: &[f32; 4],
    ux: f32,
    uy: f32,
    rx: f32,
    ry: f32,
    x: &mut f32,
    y: &mut f32,
    x0: f32,
    y0: f32,
    limits: &mut [f32; 4],
) {
    let denom = ux * ry - uy * rx;
    if denom == 0.0 {
        return;
    }
    let xc = (f64::from(xbn[0] + xbn[1] + xbn[2] + xbn[3]) / 4.0) as f32;
    let yc = (f64::from(ybn[0] + ybn[1] + ybn[2] + ybn[3]) / 4.0) as f32;
    let (mut alo, mut ahi, mut blo, mut bhi) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for ic in 0..4 {
        let dx = xbn[ic] - xc;
        let dy = ybn[ic] - yc;
        let alpha = (dx * ry - dy * rx) / denom;
        let beta = (dy * ux - dx * uy) / denom;
        if alpha < alo {
            alo = alpha;
        }
        if alpha > ahi {
            ahi = alpha;
        }
        if beta < blo {
            blo = beta;
        }
        if beta > bhi {
            bhi = beta;
        }
        // C compares the floats with double constants.
        if f64::from(alpha) < 1.0e-4 {
            if f64::from(beta) < 1.0e-4 {
                *x -= xbn[ic];
                *y -= ybn[ic];
            } else if f64::from(beta) > -1.0e-4 {
                *x += xbn[ic];
                *y += ybn[ic];
            }
        }
    }
    let dx = xc - x0;
    let dy = yc - y0;
    let alpha = (dx * ry - dy * rx) / denom;
    let beta = (dy * ux - dx * uy) / denom;
    alo += alpha;
    ahi += alpha;
    blo += beta;
    bhi += beta;
    if alo < limits[0] {
        limits[0] = alo;
    }
    if ahi > limits[1] {
        limits[1] = ahi;
    }
    if blo < limits[2] {
        limits[2] = blo;
    }
    if bhi > limits[3] {
        limits[3] = bhi;
    }
}

/// `sqrt( dx*dx + dy*dy )` of floats, as C stores it in a float.
fn flen(dx: f32, dy: f32) -> f32 {
    sqrt(f64::from(dx * dx + dy * dy)) as f32
}

impl Plot {
    /// The heights of text with vertical and horizontal baselines, asked of
    /// the backend once per drawing.
    // C: GQch (plot.c:15166).
    pub(crate) fn g_qch(&mut self, grf: &mut dyn Grf) -> AstResult<(f32, f32)> {
        if self.draw.chh != BAD_VALUE && self.draw.chv != BAD_VALUE {
            return Ok((self.draw.chv as f32, self.draw.chh as f32));
        }
        let [chv, chh] = grf.char_height()?;
        if chh == 0.0 || chv == 0.0 {
            self.draw.chh = 1.0;
            self.draw.chv = 1.0;
            return Err(AstError::new(
                ErrorCode::Grfer,
                format!(
                    "astGQch: Returned text heights are {} and {} but zero is illegal!",
                    format_double_g(f64::from(chv), 6),
                    format_double_g(f64::from(chh), 6)
                ),
            ));
        }
        self.draw.chh = f64::from(chh);
        self.draw.chv = f64::from(chv);
        Ok((chv, chh))
    }

    /// Draw a string, unless the Plot draws in invisible ink.
    // C: GText (plot.c:15273).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn g_text(
        &mut self,
        grf: &mut dyn Grf,
        text: &str,
        x: f32,
        y: f32,
        just: [u8; 2],
        upx: f32,
        upy: f32,
    ) -> AstResult<()> {
        if !self.invisible() {
            grf.text(text, x, y, just_code(just), [upx, upy])?;
        }
        Ok(())
    }

    /// The box a string drawn by `g_text` occupies.
    // C: GTxExt (plot.c:15384).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn g_tx_ext(
        &mut self,
        grf: &mut dyn Grf,
        text: &str,
        x: f32,
        y: f32,
        just: [u8; 2],
        upx: f32,
        upy: f32,
    ) -> AstResult<([f32; 4], [f32; 4])> {
        let b = grf.text_extent(text, x, y, just_code(just), [upx, upy])?;
        Ok((b.x, b.y))
    }

    /// Set a backend text attribute, unless `value` is bad.
    fn g_attr_set(&mut self, grf: &mut dyn Grf, attr: GrfAttr, value: f64) -> AstResult<()> {
        if value != BAD_VALUE {
            self.g_attr(grf, attr, Some(value), GrfPrimitive::Text)?;
        }
        Ok(())
    }

    /// The up-vector `(ux, uy)` scaled to the height of normal text, and
    /// the vector along the baseline of the same length on the page.
    // C: RightVector (plot.c:24708).
    fn right_vector(&mut self, grf: &mut dyn Grf, ux: f32, uy: f32) -> AstResult<[f32; 4]> {
        let (alpha, beta) = self.g_scales(grf)?;
        let (mut ux, mut uy) = (ux * alpha, uy * beta);
        let l = flen(ux, uy);
        if f64::from(l) <= 0.0 {
            ux = 0.0;
            uy = 1.0;
        } else {
            ux /= l;
            uy /= l;
        }
        let (chv, chh) = self.g_qch(grf)?;
        let a = ux / (chv * alpha);
        let b = uy / (chh * beta);
        let nl = (1.0 / sqrt(f64::from(a * a + b * b))) as f32;
        ux *= nl;
        uy *= nl;
        let (mut rx, mut ry) = (uy, -ux);
        ux /= alpha;
        uy /= beta;
        rx /= alpha;
        ry /= beta;
        Ok([ux, uy, rx, ry])
    }

    /// The justification to draw with, "M" replaced by "B" for a backend
    /// that cannot justify by the bottom of the text, the reference point
    /// moved to suit, and the left end of the baseline.
    // C: JustMB (plot.c:20373).
    #[allow(clippy::too_many_arguments)]
    fn just_mb(
        &mut self,
        grf: &mut dyn Grf,
        esc: bool,
        text: &str,
        x: &mut f32,
        y: &mut f32,
        upx: f32,
        upy: f32,
        just: [u8; 2],
        unit: [f32; 4],
    ) -> AstResult<([u8; 2], f32, f32)> {
        let [uxu, uyu, rxu, ryu] = unit;
        let c0 = just[0].to_ascii_uppercase();
        let c1 = just[1].to_ascii_uppercase();
        let mut result = [
            if matches!(c0, b'T' | b'C' | b'B' | b'M') {
                c0
            } else {
                b'C'
            },
            if matches!(c1, b'L' | b'C' | b'R') {
                c1
            } else {
                b'C'
            },
        ];
        let mut bounds = None;
        if result[0] == b'M' && !self.g_cap(grf, GrfCap::MJust, 1) {
            let b = self.render_text(grf, false, esc, text, *x, *y, *b"BL", upx, upy)?;
            *x += *x - b.xbn[0];
            *y += *y - b.ybn[0];
            result[0] = b'B';
            bounds = Some(b);
        }
        if result == *b"BL" {
            return Ok((result, *x, *y));
        }
        let b = match bounds {
            Some(b) => b,
            None => self.render_text(grf, false, esc, text, *x, *y, *b"BL", upx, upy)?,
        };
        let width = flen(b.xbn[0] - b.xbn[3], b.ybn[0] - b.ybn[3]);
        let height = flen(b.xbn[0] - b.xbn[1], b.ybn[0] - b.ybn[1]);
        let mut f: f32 = match result[1] {
            b'C' => 0.5,
            b'R' => 1.0,
            _ => 0.0,
        };
        f *= width;
        let mut x0 = *x - f * rxu;
        let mut y0 = *y - f * ryu;
        f = match result[0] {
            b'T' => height - b.drop,
            b'C' => (0.5 * f64::from(height) - f64::from(b.drop)) as f32,
            b'M' => -b.drop,
            _ => 0.0,
        };
        x0 -= f * uxu;
        y0 -= f * uyu;
        Ok((result, x0, y0))
    }

    /// Save the backend's text attributes and the baseline's rise.
    // C: PushGat (plot.c:24460).
    fn push_gat(&mut self, grf: &mut dyn Grf, rise: f32) -> AstResult<()> {
        let q = |p: &mut Plot, grf: &mut dyn Grf, a| p.g_attr(grf, a, None, GrfPrimitive::Text);
        let size = q(self, grf, GrfAttr::Size)?;
        let width = q(self, grf, GrfAttr::Width)?;
        let font = q(self, grf, GrfAttr::Font)?;
        let style = q(self, grf, GrfAttr::Style)?;
        let col = q(self, grf, GrfAttr::Colour)?;
        self.draw.gat.push(Gat {
            rise,
            size,
            width,
            font,
            style,
            col,
        });
        Ok(())
    }

    /// Restore the attributes saved last, returning the saved rise, or
    /// `None` if nothing is saved.
    // C: PopGat (plot.c:24313).
    fn pop_gat(&mut self, grf: &mut dyn Grf) -> AstResult<Option<f32>> {
        let Some(gat) = self.draw.gat.pop() else {
            return Ok(None);
        };
        self.g_attr_set(grf, GrfAttr::Size, gat.size)?;
        self.g_attr_set(grf, GrfAttr::Width, gat.width)?;
        self.g_attr_set(grf, GrfAttr::Colour, gat.col)?;
        self.g_attr_set(grf, GrfAttr::Font, gat.font)?;
        self.g_attr_set(grf, GrfAttr::Style, gat.style)?;
        Ok(Some(gat.rise))
    }

    /// Act on an escape sequence: move the concatenation point `(x, y)` or
    /// change the backend's text attributes. `vectors` are the up and right
    /// vectors, `normal` the normal size, style, width, colour and font.
    // C: InterpEscape (plot.c:20059).
    #[allow(clippy::too_many_arguments)]
    fn interp_escape(
        &mut self,
        grf: &mut dyn Grf,
        kind: Escape,
        mut value: f64,
        x: &mut f32,
        y: &mut f32,
        vectors: [f32; 4],
        rise: &mut f32,
        normal: [f64; 5],
    ) -> AstResult<()> {
        let [ux, uy, rx, ry] = vectors;
        let [nsize, nstyle, nwidth, ncol, nfont] = normal;
        let (ux, uy, rxd, ryd) = (f64::from(ux), f64::from(uy), f64::from(rx), f64::from(ry));
        let mv = |p: &mut f32, d: f64| *p = (f64::from(*p) + d) as f32;
        match kind {
            Escape::Sup => {
                if value > 0.0 {
                    mv(x, 0.01 * ux * (value - f64::from(*rise)));
                    mv(y, 0.01 * uy * (value - f64::from(*rise)));
                    *rise = value as f32;
                } else {
                    mv(x, -(0.01 * ux * f64::from(*rise)));
                    mv(y, -(0.01 * uy * f64::from(*rise)));
                    *rise = 0.0;
                }
            }
            Escape::Sub => {
                if value > 0.0 {
                    mv(x, -(0.01 * ux * (value + f64::from(*rise))));
                    mv(y, -(0.01 * uy * (value + f64::from(*rise))));
                    *rise = -value as f32;
                } else {
                    mv(x, -(0.01 * ux * f64::from(*rise)));
                    mv(y, -(0.01 * uy * f64::from(*rise)));
                    *rise = 0.0;
                }
            }
            Escape::Gap => {
                mv(x, 0.01 * rxd * value);
                mv(y, 0.01 * ryd * value);
            }
            Escape::Back => {
                mv(x, -(0.01 * rxd * value));
                mv(y, -(0.01 * ryd * value));
            }
            Escape::Mark => self.draw.hmark = (*x, *y),
            Escape::Go => {
                let (hx, hy) = self.draw.hmark;
                if hx != f32::MAX && hy != f32::MAX {
                    let t1 = (*x - hx) * rx + (*y - hy) * ry;
                    let t2 = rx * rx + ry * ry;
                    *x -= rx * t1 / t2;
                    *y -= ry * t1 / t2;
                }
            }
            Escape::Size => {
                if value < 0.0 {
                    value = 100.0;
                }
                self.g_attr_set(grf, GrfAttr::Size, 0.01 * value * nsize)?;
            }
            Escape::Width => {
                if value < 0.0 {
                    value = 100.0;
                }
                self.g_attr_set(grf, GrfAttr::Width, 0.01 * value * nwidth)?;
            }
            Escape::Font => {
                if value < 0.0 {
                    value = nfont;
                }
                self.g_attr_set(grf, GrfAttr::Font, value)?;
            }
            Escape::Colour => {
                if value < 0.0 {
                    value = ncol;
                }
                self.g_attr_set(grf, GrfAttr::Colour, value)?;
            }
            Escape::Style => {
                if value < 0.0 {
                    value = nstyle;
                }
                self.g_attr_set(grf, GrfAttr::Style, value)?;
            }
            Escape::Push => self.push_gat(grf, *rise)?,
            Escape::Pop => {
                let new_rise = match self.pop_gat(grf)? {
                    Some(r) => r,
                    None => {
                        self.g_attr_set(grf, GrfAttr::Size, nsize)?;
                        self.g_attr_set(grf, GrfAttr::Width, nwidth)?;
                        self.g_attr_set(grf, GrfAttr::Colour, ncol)?;
                        self.g_attr_set(grf, GrfAttr::Font, nfont)?;
                        self.g_attr_set(grf, GrfAttr::Style, nstyle)?;
                        0.0
                    }
                };
                mv(x, -(0.01 * ux * f64::from(*rise - new_rise)));
                mv(y, -(0.01 * uy * f64::from(*rise - new_rise)));
                *rise = new_rise;
            }
            Escape::Percent => {}
        }
        Ok(())
    }

    /// Draw (if `ink`) a string at `(x, y)` justified by `just` with
    /// up-vector `(upx, upy)`, interpreting escape sequences if `esc` and
    /// the backend does not, and return the box it occupies, or `None` if
    /// there is nothing to draw.
    // C: DrawText (plot.c:9743), whose box C leaves unset in that case.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_text(
        &mut self,
        grf: &mut dyn Grf,
        ink: bool,
        esc: bool,
        text: &str,
        x: f32,
        y: f32,
        just: [u8; 2],
        upx: f32,
        upy: f32,
    ) -> AstResult<TextBounds> {
        self.render_text_opt(grf, ink, esc, text, x, y, just, upx, upy)?
            .ok_or_else(|| {
                AstError::new(
                    ErrorCode::Inter,
                    "render_text: nothing to measure".to_string(),
                )
            })
    }

    /// `render_text`, giving `None` where C draws nothing.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_text_opt(
        &mut self,
        grf: &mut dyn Grf,
        ink: bool,
        esc: bool,
        text: &str,
        x: f32,
        y: f32,
        just: [u8; 2],
        upx: f32,
        upy: f32,
    ) -> AstResult<Option<TextBounds>> {
        if text.is_empty()
            || f64::from(x) == BAD_VALUE
            || f64::from(y) == BAD_VALUE
            || (upx == 0.0 && upy == 0.0)
        {
            return Ok(None);
        }
        let ux = if self.xrev { -upx } else { upx };
        let uy = if self.yrev { -upy } else { upy };
        let [ux, uy, rx, ry] = self.right_vector(grf, ux, uy)?;
        let rlen = flen(rx, ry);
        let (rxu, ryu) = (rx / rlen, ry / rlen);
        let ulen = flen(ux, uy);
        let (uxu, uyu) = (ux / ulen, uy / ulen);
        let (mut lx, mut ly) = (x, y);
        let (lj, x0, y0) = self.just_mb(
            grf,
            esc,
            text,
            &mut lx,
            &mut ly,
            upx,
            upy,
            just,
            [uxu, uyu, rxu, ryu],
        )?;
        // alpha_lo, alpha_hi, beta_lo, beta_hi.
        let mut limits = [f32::MAX, -f32::MAX, f32::MAX, -f32::MAX];
        let grfcap = self.g_cap(grf, GrfCap::Escapes, i32::from(esc));
        self.draw.hmark = (f32::MAX, f32::MAX);
        if esc && has_escapes(text) && !grfcap {
            let mut rise = 0.0f32;
            let q = |p: &mut Plot, grf: &mut dyn Grf, a| p.g_attr(grf, a, None, GrfPrimitive::Text);
            let nsize = q(self, grf, GrfAttr::Size)?;
            let nwidth = q(self, grf, GrfAttr::Width)?;
            let nfont = q(self, grf, GrfAttr::Font)?;
            let nstyle = q(self, grf, GrfAttr::Style)?;
            let ncol = q(self, grf, GrfAttr::Colour)?;
            let (mut cx, mut cy) = (x0, y0);
            let mut a = 0;
            while a < text.len() {
                let rest = &text[a..];
                let (mut got, kind, value, mut nc) = find_escape(rest);
                let mut start = a;
                if got && kind == Escape::Percent {
                    got = false;
                    start += 1;
                    a += 1;
                    nc = 1;
                }
                if got {
                    self.interp_escape(
                        grf,
                        kind,
                        f64::from(value),
                        &mut cx,
                        &mut cy,
                        [ux, uy, rx, ry],
                        &mut rise,
                        [nsize, nstyle, nwidth, ncol, nfont],
                    )?;
                } else {
                    let sub = &text[start..start + nc];
                    if lj == *b"BL" {
                        lx = cx;
                        ly = cy;
                    } else {
                        let hm = self.draw.hmark;
                        let b = self.render_text(grf, false, esc, sub, cx, cy, *b"BL", upx, upy)?;
                        self.draw.hmark = hm;
                        let mut width = flen(b.xbn[0] - b.xbn[3], b.ybn[0] - b.ybn[3]);
                        let mut height = flen(b.xbn[0] - b.xbn[1], b.ybn[0] - b.ybn[1]);
                        if lj[1] == b'C' {
                            width = (f64::from(width) * 0.5) as f32;
                        } else if lj[1] != b'R' {
                            width = 0.0;
                        }
                        lx = cx + rxu * width;
                        ly = cy + ryu * width;
                        if lj[0] == b'T' {
                            height -= b.drop;
                            lx += height * uxu;
                            ly += height * uyu;
                        } else if lj[0] == b'C' {
                            height = (0.5 * f64::from(height) - f64::from(b.drop)) as f32;
                            lx += height * uxu;
                            ly += height * uyu;
                        } else if lj[0] == b'M' {
                            lx -= b.drop * uxu;
                            ly -= b.drop * uyu;
                        }
                    }
                    if ink {
                        self.g_text(grf, sub, lx, ly, lj, upx, upy)?;
                    }
                    let (txbn, tybn) = self.g_tx_ext(grf, sub, lx, ly, lj, upx, upy)?;
                    update_concat(
                        &txbn,
                        &tybn,
                        ux,
                        uy,
                        rx,
                        ry,
                        &mut cx,
                        &mut cy,
                        x0,
                        y0,
                        &mut limits,
                    );
                }
                a += nc;
            }
            self.g_attr_set(grf, GrfAttr::Size, nsize)?;
            self.g_attr_set(grf, GrfAttr::Width, nwidth)?;
            self.g_attr_set(grf, GrfAttr::Colour, ncol)?;
            self.g_attr_set(grf, GrfAttr::Font, nfont)?;
            self.g_attr_set(grf, GrfAttr::Style, nstyle)?;
        } else {
            if ink {
                self.g_text(grf, text, lx, ly, lj, upx, upy)?;
            }
            let (txbn, tybn) = self.g_tx_ext(grf, text, lx, ly, lj, upx, upy)?;
            update_concat(
                &txbn,
                &tybn,
                ux,
                uy,
                rx,
                ry,
                &mut lx,
                &mut ly,
                x0,
                y0,
                &mut limits,
            );
        }
        let [alo, ahi, blo, bhi] = limits;
        let xbn = [
            x0 + alo * ux + blo * rx,
            x0 + ahi * ux + blo * rx,
            x0 + ahi * ux + bhi * rx,
            x0 + alo * ux + bhi * rx,
        ];
        let ybn = [
            y0 + alo * uy + blo * ry,
            y0 + ahi * uy + blo * ry,
            y0 + ahi * uy + bhi * ry,
            y0 + alo * uy + bhi * ry,
        ];
        let drop = -alo * ulen;
        if ink && !self.draw.boxp_freeze {
            for i in 0..4 {
                let d = &mut self.draw;
                d.boxp_lbnd[0] = super::draw::fmin(xbn[i], d.boxp_lbnd[0]);
                d.boxp_ubnd[0] = super::draw::fmax(xbn[i], d.boxp_ubnd[0]);
                d.boxp_lbnd[1] = super::draw::fmin(ybn[i], d.boxp_lbnd[1]);
                d.boxp_ubnd[1] = super::draw::fmax(ybn[i], d.boxp_ubnd[1]);
            }
        }
        Ok(Some(TextBounds { xbn, ybn, drop }))
    }

    // C: Text (plot.c:26313).
    pub(crate) fn text_impl(
        &mut self,
        grf: &mut dyn Grf,
        text: &str,
        pos: &[f64],
        up: [f32; 2],
        just: Justification,
    ) -> AstResult<()> {
        let naxes = Mapping::nin(self);
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astText(Plot): Number of axes ({naxes}) in the base Frame of the supplied \
                     Plot is invalid - this number should be 2."
                ),
            ));
        }
        let _escapes = crate::escapes::set_escapes(true);
        self.reset_boxp();
        self.reset_grf();
        self.grf_attrs(grf, PlotElement::Strings, true, GrfPrimitive::Text)?;
        let ncoord = Mapping::nout(self);
        if pos.len() < ncoord {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!(
                    "astText(Plot): {} values given for the position of the text, but the \
                     current Frame has {ncoord} axes.",
                    pos.len()
                ),
            ));
        }
        let phys = PointSet::from_rows([&pos[..ncoord]]);
        let graph = self.trans_with(None, &phys, false, false)?;
        let (gx, gy) = (graph.axis(0)[0], graph.axis(1)[0]);
        let ltext = &text[..chr_len(text)];
        if gx != BAD_VALUE && gy != BAD_VALUE {
            let esc = self.escape();
            self.render_text_opt(
                grf,
                true,
                esc,
                ltext,
                gx as f32,
                gy as f32,
                just.code(),
                up[0],
                up[1],
            )?;
        }
        self.grf_attrs(grf, PlotElement::Strings, false, GrfPrimitive::Text)
    }
}
