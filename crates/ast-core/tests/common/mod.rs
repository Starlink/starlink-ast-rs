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
