//! Shared helpers for the integration test binaries.
//!
//! Integration tests each compile as a separate crate, so code shared
//! between them lives here and is pulled in with `mod common;`. Declaring an
//! item `pub` but leaving it unused in a given binary trips `dead_code`, so
//! the module is annotated to allow it.
#![allow(dead_code)]

/// Compare two native (`.ast`) serialisations for equivalence: byte-equal
/// after trimming, or equal modulo ≤`max_ulps`-ULP differences in any numeric
/// token. This absorbs the last-bit libm drift that differs between platforms
/// (and between the Rust port and the C oracle).
pub fn native_strings_equivalent(got: &str, want: &str, max_ulps: u64) -> bool {
    if got.trim() == want.trim() {
        return true;
    }
    let mut g = got.lines();
    let mut w = want.lines();
    loop {
        match (g.next(), w.next()) {
            (None, None) => return true,
            (Some(gl), Some(wl)) if gl == wl => continue,
            (Some(gl), Some(wl)) => {
                if !line_tokens_equivalent(gl, wl, max_ulps) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Drop the `IsSimp` attribute line from a native dump.
pub fn strip_is_simp(s: &str) -> String {
    s.lines()
        .filter(|l| !l.trim_start().starts_with("IsSimp ="))
        .collect::<Vec<_>>()
        .join("\n")
}

/// True for a keyword-card line holding an `IsSimp` tag (`ISSIMP<xx>=`, where
/// `<xx>` is a two-character sequence number).
///
/// Where the tag lands inside a compound Mapping became a property of the
/// Mapping tree alone at upstream `e40e71b4`, so a difference here is a port
/// gap and not a C artifact. Callers comparing a serialised object against a
/// C-generated reference use this to drop the tag cards from both sides while
/// pinning how many each side carries.
///
/// `INVERT<xx>` used to be dropped alongside it, for the `WcsNative` shared
/// PermMap quirk. Upstream `f54ff1b7` fixed that, and the port's shape was
/// already the corrected one, so `Invert` cards are compared like any other.
/// See `docs/issues/c-library-quirks.md`.
pub fn is_issimp_tag_card(line: &str) -> bool {
    // The `.ast` text dump writes the same tag as an indented `IsSimp = 1`.
    if line.trim_start().starts_with("IsSimp =") {
        return true;
    }
    let key = line.split(['=', ' ']).next().unwrap_or("");
    key.starts_with("ISSIMP") && key.len() == 8
}

/// Compare two whitespace-tokenised lines for equivalence: tokens must be
/// byte-equal, except numeric tokens that parse as `f64` may differ by up to
/// `max_ulps` ULP.
pub fn line_tokens_equivalent(got: &str, want: &str, max_ulps: u64) -> bool {
    if got == want {
        return true;
    }
    let mut gt = got.split_whitespace();
    let mut wt = want.split_whitespace();
    loop {
        match (gt.next(), wt.next()) {
            (None, None) => return true,
            (Some(g), Some(w)) if g == w => continue,
            (Some(g), Some(w)) => {
                let (Ok(gv), Ok(wv)) = (g.parse::<f64>(), w.parse::<f64>()) else {
                    return false;
                };
                if !ulp_close(gv, wv, max_ulps) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Compare two whitespace-tokenised lines using AST's `astEQUAL` tolerance on
/// numeric tokens (non-numeric tokens must be byte-equal). This is the
/// floating-point-equivalence comparison the C oracle applies via `astEqual`:
/// looser than [`line_tokens_equivalent`]'s fixed ULP budget, so it absorbs
/// platform-libm differences that are amplified by cancellation (e.g. an
/// observatory altitude derived from geocentric coordinates), while still
/// rejecting genuine (≳1e-8 relative) discrepancies.
pub fn line_tokens_astequal(got: &str, want: &str) -> bool {
    if got == want {
        return true;
    }
    let mut gt = got.split_whitespace();
    let mut wt = want.split_whitespace();
    loop {
        match (gt.next(), wt.next()) {
            (None, None) => return true,
            (Some(g), Some(w)) if g == w => continue,
            (Some(g), Some(w)) => {
                let (Ok(gv), Ok(wv)) = (g.parse::<f64>(), w.parse::<f64>()) else {
                    return false;
                };
                if !ast_equal_close(gv, wv) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Port of C `astEQUAL` (object.h): `|a-b| <= max(1e5*(|a|+|b|)*DBL_EPSILON,
/// 1e-12)`. The near-zero floor (`1e-12`) matches AST's handling of ulp-scale
/// residuals that drift between macOS and Linux math libraries.
pub fn ast_equal_close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() {
        return false;
    }
    let tol = (1.0e5 * (a.abs() + b.abs()) * f64::EPSILON).max(1.0e-12);
    (a - b).abs() <= tol
}

/// True when `a` and `b` are within `max_ulps` units in the last place.
/// Differing signs (including ±0 with opposite sign bits) are never close.
pub fn ulp_close(a: f64, b: f64, max_ulps: u64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= max_ulps
}

/// True for a KeyMap `MapSz` card line.
///
/// `MapSz` records the size C's chained hash table had grown to when the
/// KeyMap was dumped, which depends on the insertion order and on which keys
/// collided (`keymap.c:10510`, `keymap.c:2578`). Entries here live in a
/// `HashMap`, so there is no bucket table to record and the card is
/// reconstructed at dump time from the key set alone. The two values agree
/// for every population the oracle probe measured, but the card is excluded
/// from byte-exact comparisons against C-generated references because the
/// agreement is arithmetic rather than structural.
/// See `docs/issues/c-library-quirks.md`, under "KeyMap `MapSz` records
/// collision history, so identical KeyMaps can serialise differently".
pub fn is_map_sz_card(line: &str) -> bool {
    line.trim_start()
        .trim_start_matches('#')
        .trim_start()
        .starts_with("MapSz =")
}
