// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The page the provided backends model, and its style attributes.
//!
//! Graphics coordinates run from 0 to 1 on each axis of a square page, 200
//! mm on a side unless a backend says otherwise. Characters are 4 mm high at
//! a text size of 1, and each is 0.6 times its height wide.
// C: grf_log.c, the "virtual viewport" its GScales, GQch, GTxExt and GAttr
// answer from. The arithmetic mixes float and double as the C does: every
// local C declares `float` is `f32` here, in C's order of operations.

use super::{GrfAttr, GrfPrimitive, HJust, Justification, TextBox, VJust};
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};

/// The side of the page, in mm, that C's logging backend models.
pub(crate) const DEFAULT_PAGE_MM: f64 = 200.0;
const WORLD_X_MIN: f64 = 0.0;
const WORLD_X_MAX: f64 = 1.0;
const WORLD_Y_MIN: f64 = 0.0;
const WORLD_Y_MAX: f64 = 1.0;
/// The character height, in mm, at a text size of 1.
pub(crate) const DEFAULT_CHR_HT: f64 = 4.0;

/// How the page measures the width of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextMetrics {
    /// Every character 0.6 times the height wide, as C's logging backend
    /// measures it.
    Fixed,
    /// Each character as wide as Helvetica draws it, a font size being the
    /// character height; Arial and Liberation Sans have the same widths.
    Helvetica,
}

/// The width of `text` on the page, in mm, for characters `chr_ht` mm high.
///
/// With [`TextMetrics::Fixed`] each character of the text is 0.6 times the
/// height wide, an accented one as wide as any other.
// C: grf_log.c:231, `strlen( text ) * chr_ht * 0.6`, which counts bytes, the
// same for ASCII text; C predates Unicode, so the port counts characters.
pub(crate) fn text_width_mm(text: &str, chr_ht: f64, metrics: TextMetrics) -> f32 {
    match metrics {
        TextMetrics::Fixed => (text.chars().count() as f64 * chr_ht * 0.6) as f32,
        TextMetrics::Helvetica => {
            let em: u32 = text.chars().map(helvetica_width).sum();
            (f64::from(em) / 1000.0 * chr_ht) as f32
        }
    }
}

/// Helvetica's advance widths for printable ASCII, in thousandths of the
/// font size, from Adobe's Helvetica font metrics, `' '` first.
const HELVETICA_ASCII: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278,
    278, // ' ' to '/'
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // '0' to '9'
    278, 278, 584, 584, 584, 556, 1015, // ':' to '@'
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // 'A' to 'M'
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // 'N' to 'Z'
    278, 278, 278, 469, 556, 333, // '[' to '`'
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // 'a' to 'm'
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // 'n' to 'z'
    334, 260, 334, 584, // '{' to '~'
];

/// The advance width of `c` in Helvetica, in thousandths of the font size.
///
/// Printable ASCII, Latin-1 and the few symbols axis labels use take
/// Helvetica's widths; any other character is taken as 556, the width of a
/// digit and of most lower-case letters.
fn helvetica_width(c: char) -> u32 {
    let code = c as u32;
    if (0x20..0x7f).contains(&code) {
        return u32::from(HELVETICA_ASCII[(code - 0x20) as usize]);
    }
    match c {
        '\u{a0}' | '·' => 278,
        '°' => 400,
        '±' | '×' | '÷' | '¬' | '\u{2212}' => 584,
        '²' | '³' | '¹' | '´' | '¨' | '¯' | '¸' => 333,
        '©' | '®' => 737,
        '¼' | '½' | '¾' => 834,
        'À'..='Å' | 'È'..='Ë' | 'Ý' | 'Þ' | '\u{212b}' => 667,
        'Æ' => 1000,
        'Ç' | 'Ð' | 'Ñ' | 'Ù'..='Ü' => 722,
        'Ò'..='Ö' | 'Ø' => 778,
        'Ì'..='Ï' | 'ì'..='ï' => 278,
        'ß' | 'ø' => 611,
        'æ' => 889,
        'ç' | 'ý' | 'ÿ' => 500,
        '′' => 191,
        '″' => 355,
        _ => 556,
    }
}

/// The modelled page and the style attributes set on it.
#[derive(Debug, Clone)]
pub(crate) struct VirtualPage {
    /// The side of the square page in mm.
    page_mm: f64,
    /// How text widths are measured.
    metrics: TextMetrics,
    /// Each attribute, indexed by primitive (text, line, mark).
    style: [f64; 3],
    width: [f64; 3],
    size: [f64; 3],
    font: [f64; 3],
    colour: [f64; 3],
}

impl VirtualPage {
    /// A page with every attribute of every primitive 1.
    // C: astGrfLogInit.
    pub(crate) fn new() -> Self {
        Self::with_side_mm(DEFAULT_PAGE_MM, TextMetrics::Fixed)
    }

    /// A page `page_mm` mm square measuring text with `metrics`, with every
    /// attribute of every primitive 1.
    pub(crate) fn with_side_mm(page_mm: f64, metrics: TextMetrics) -> Self {
        Self {
            page_mm,
            metrics,
            style: [1.0; 3],
            width: [1.0; 3],
            size: [1.0; 3],
            font: [1.0; 3],
            colour: [1.0; 3],
        }
    }

    fn values_mut(&mut self, attr: GrfAttr) -> &mut [f64; 3] {
        match attr {
            GrfAttr::Style => &mut self.style,
            GrfAttr::Width => &mut self.width,
            GrfAttr::Size => &mut self.size,
            GrfAttr::Font => &mut self.font,
            GrfAttr::Colour => &mut self.colour,
        }
    }

    /// The value of an attribute of a primitive.
    pub(crate) fn get(&self, attr: GrfAttr, prim: GrfPrimitive) -> f64 {
        let values = match attr {
            GrfAttr::Style => &self.style,
            GrfAttr::Width => &self.width,
            GrfAttr::Size => &self.size,
            GrfAttr::Font => &self.font,
            GrfAttr::Colour => &self.colour,
        };
        values[prim as usize]
    }

    /// Query and set an attribute, returning its previous value and
    /// whether a value was set. A value of `BAD_VALUE` queries, as C's does.
    // C: astGAttr (grf_log.c:364).
    pub(crate) fn attr(
        &mut self,
        attr: GrfAttr,
        value: Option<f64>,
        prim: GrfPrimitive,
    ) -> (f64, Option<f64>) {
        let slot = &mut self.values_mut(attr)[prim as usize];
        let old = *slot;
        let set = value.filter(|&v| v != BAD_VALUE);
        if let Some(v) = set {
            *slot = v;
        }
        (old, set)
    }

    /// The scales from graphics coordinates to mm on the page.
    // C: astGScales (grf_log.c:161).
    pub(crate) fn scales(&self) -> [f32; 2] {
        [
            (self.page_mm / (WORLD_X_MAX - WORLD_X_MIN)) as f32,
            (self.page_mm / (WORLD_Y_MAX - WORLD_Y_MIN)) as f32,
        ]
    }

    /// The character height in mm: the default height times the text
    /// size, a size at or below zero counting as 1.
    // C: ChrHtMM (grf_log.c:152).
    fn chr_ht_mm(&self) -> f64 {
        let mut scale = self.size[GrfPrimitive::Text as usize];
        if scale <= 0.0 {
            scale = 1.0;
        }
        DEFAULT_CHR_HT * scale
    }

    /// The character height in graphics coordinates, `[chv, chh]`.
    // C: astGQch (grf_log.c:169), after its call of astGScales.
    pub(crate) fn char_height(&self) -> [f32; 2] {
        let [alpha, beta] = self.scales();
        let ht = self.chr_ht_mm();
        let chv = if alpha != 0.0 {
            (ht / f64::from(alpha).abs()) as f32
        } else {
            0.0
        };
        let chh = if beta != 0.0 {
            (ht / f64::from(beta).abs()) as f32
        } else {
            0.0
        };
        [chv, chh]
    }

    /// The box `text` would occupy, or `None` for empty text, whose box is
    /// all zeros and for which C does not consult the scales.
    // C: astGTxExt (grf_log.c:183), after its call of astGScales.
    pub(crate) fn text_extent(
        &self,
        text: &str,
        x: f32,
        y: f32,
        just: Justification,
        up: [f32; 2],
    ) -> AstResult<Option<TextBox>> {
        if text.is_empty() {
            return Ok(None);
        }
        // C accepts only T, C and B vertically, and takes anything else,
        // such as M, as C.
        let vertical = match just.vertical {
            VJust::Top => VJust::Top,
            VJust::Baseline => VJust::Baseline,
            VJust::Centre | VJust::Bottom => VJust::Centre,
        };
        let [alpha, beta] = self.scales();
        let [mut upx, mut upy] = up;
        if alpha < 0.0 {
            upx = -upx;
        }
        if beta < 0.0 {
            upy = -upy;
        }
        let mut ux = alpha * upx;
        let mut uy = beta * upy;
        let uplen = crate::math::sqrt(f64::from(ux * ux + uy * uy)) as f32;
        if uplen > 0.0 {
            ux /= uplen;
            uy /= uplen;
        } else {
            return Err(AstError::new(
                ErrorCode::Grfer,
                "astGTxExt: Zero length up-vector supplied.",
            ));
        }
        let vx = uy;
        let vy = -ux;

        let chr_ht = self.chr_ht_mm();
        let phys_hu = chr_ht as f32;
        let phys_hd = (-0.2 * chr_ht) as f32;
        let phys_w = text_width_mm(text, chr_ht, self.metrics);
        let padded_w = phys_w + 0.2_f32 * phys_hu;

        let cy_mm = match vertical {
            VJust::Baseline => 0.5_f32 * (phys_hu + phys_hd),
            VJust::Top => -0.5_f32 * (phys_hu - phys_hd),
            _ => 0.5_f32 * phys_hd,
        };
        let cx_mm = match just.horizontal {
            HJust::Left => 0.5_f32 * padded_w,
            HJust::Right => -0.5_f32 * padded_w,
            HJust::Centre => 0.0,
        };

        let xc = x + (cx_mm * vx + cy_mm * ux) / alpha;
        let yc = y + (cx_mm * vy + cy_mm * uy) / beta;

        let half_w_mm = 0.5_f32 * padded_w;
        let half_h_mm = 0.5_f32 * (phys_hu - phys_hd);

        let vdx = (half_w_mm * vx) / alpha;
        let vdy = (half_w_mm * vy) / beta;
        let udx = (half_h_mm * ux) / alpha;
        let udy = (half_h_mm * uy) / beta;

        Ok(Some(TextBox {
            x: [
                xc - vdx - udx,
                xc + vdx - udx,
                xc + vdx + udx,
                xc - vdx + udx,
            ],
            y: [
                yc - vdy - udy,
                yc + vdy - udy,
                yc + vdy + udy,
                yc - vdy + udy,
            ],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A character that is not ASCII is as wide as one that is.
    #[test]
    fn text_width_counts_characters_not_bytes() {
        for metrics in [TextMetrics::Fixed, TextMetrics::Helvetica] {
            assert_eq!(
                text_width_mm("é", 4.0, metrics),
                text_width_mm("e", 4.0, metrics)
            );
            assert_eq!(
                text_width_mm("Ångström", 4.0, metrics),
                text_width_mm("Angstrom", 4.0, metrics)
            );
        }
    }
}
