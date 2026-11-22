// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The page the provided backends model, and its style attributes.
//!
//! Graphics coordinates run from 0 to 1 on each axis of a page 200 mm
//! square. Characters are 4 mm high at a text size of 1, and each is 0.6
//! times its height wide.
// C: grf_log.c, the "virtual viewport" its GScales, GQch, GTxExt and GAttr
// answer from. The arithmetic mixes float and double as the C does: every
// local C declares `float` is `f32` here, in C's order of operations.

use super::{GrfAttr, GrfPrimitive, HJust, Justification, TextBox, VJust};
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};

const PAGE_WIDTH_MM: f64 = 200.0;
const PAGE_HEIGHT_MM: f64 = 200.0;
const WORLD_X_MIN: f64 = 0.0;
const WORLD_X_MAX: f64 = 1.0;
const WORLD_Y_MIN: f64 = 0.0;
const WORLD_Y_MAX: f64 = 1.0;
/// The character height, in mm, at a text size of 1.
const DEFAULT_CHR_HT: f64 = 4.0;

/// The width of `text` on the page, in mm, for characters `chr_ht` mm high.
///
/// Each character of the text is 0.6 times the height wide, an accented
/// one as wide as any other.
// C: grf_log.c:231, `strlen( text ) * chr_ht * 0.6`, which counts bytes, the
// same for ASCII text; C predates Unicode, so the port counts characters.
// Kept as one function so that real font metrics can replace it.
pub(crate) fn text_width_mm(text: &str, chr_ht: f64) -> f32 {
    (text.chars().count() as f64 * chr_ht * 0.6) as f32
}

/// The modelled page and the style attributes set on it.
#[derive(Debug, Clone)]
pub(crate) struct VirtualPage {
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
        Self {
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
            (PAGE_WIDTH_MM / (WORLD_X_MAX - WORLD_X_MIN)) as f32,
            (PAGE_HEIGHT_MM / (WORLD_Y_MAX - WORLD_Y_MIN)) as f32,
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
        let phys_w = text_width_mm(text, chr_ht);
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
        assert_eq!(text_width_mm("é", 4.0), text_width_mm("e", 4.0));
        assert_eq!(
            text_width_mm("Ångström", 4.0),
            text_width_mm("Angstrom", 4.0)
        );
    }
}
