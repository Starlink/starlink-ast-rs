//! Shared helpers for the integration test binaries.
//!
//! Each area program that needs these declares `mod common;` once in its
//! `main.rs`, and its test files reach them through `crate::common`. Declaring
//! an item `pub` but leaving it unused in a given program trips `dead_code`,
//! so the module is annotated to allow it.
#![allow(dead_code)]

/// Compare two native (`.ast`) serializations: byte-equal, except that a
/// number may differ in value by up to `max_ulps` ULP. This absorbs the
/// last-bit libm drift that differs between platforms (and between the Rust
/// port and the C oracle); every other byte, whitespace included, must match
/// ([`line_tokens_equivalent`]).
pub fn native_strings_equivalent(got: &str, want: &str, max_ulps: u64) -> bool {
    if got == want {
        return true;
    }
    if got.ends_with('\n') != want.ends_with('\n') {
        return false;
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

/// [`native_strings_equivalent`] with AST's own `astEQUAL` tolerance on
/// numeric tokens instead of a fixed ULP budget.
pub fn native_strings_astequal(got: &str, want: &str) -> bool {
    if got == want {
        return true;
    }
    if got.ends_with('\n') != want.ends_with('\n') {
        return false;
    }
    let mut g = got.lines();
    let mut w = want.lines();
    loop {
        match (g.next(), w.next()) {
            (None, None) => return true,
            (Some(gl), Some(wl)) if gl == wl => continue,
            (Some(gl), Some(wl)) => {
                if !line_tokens_astequal(gl, wl) {
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
/// gap and not a C artifact. Callers comparing a serialized object against a
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

/// A line as alternating runs of whitespace and of everything else, in order.
fn runs(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut ws = None;
    for (i, c) in line.char_indices() {
        let w = c.is_whitespace();
        if ws.is_some_and(|prev| prev != w) {
            out.push(&line[start..i]);
            start = i;
        }
        ws = Some(w);
    }
    if start < line.len() {
        out.push(&line[start..]);
    }
    out
}

/// Whether two lines match under `close`: the same runs of whitespace and of
/// other text, byte for byte, except that a token both sides read as a number
/// may be written differently when its value differs and `close` accepts the
/// difference. The same value written another way (`1` and `1.0`) is a
/// formatting difference and fails. Whitespace may differ only to absorb
/// such a number's change of width in a fixed-width line, as an 80-column
/// FITS card does: some number in the line must differ, and the whitespace
/// must change in total by exactly the opposite of the numbers' width change.
fn line_matches(got: &str, want: &str, close: impl Fn(f64, f64) -> bool) -> bool {
    if got == want {
        return true;
    }
    let (g, w) = (runs(got), runs(want));
    if g.len() != w.len() {
        return false;
    }
    let (mut numbers_differ, mut ws_differ) = (false, false);
    let (mut num_width, mut ws_width) = (0isize, 0isize);
    for (g, w) in g.iter().zip(&w) {
        if g == w {
            continue;
        }
        let width = g.len() as isize - w.len() as isize;
        if g.trim().is_empty() && w.trim().is_empty() {
            ws_differ = true;
            ws_width += width;
            continue;
        }
        match (g.parse::<f64>(), w.parse::<f64>()) {
            (Ok(gv), Ok(wv)) if gv.to_bits() != wv.to_bits() && close(gv, wv) => {
                numbers_differ = true;
                num_width += width;
            }
            _ => return false,
        }
    }
    !ws_differ || (numbers_differ && ws_width == -num_width)
}

/// [`line_matches`] with numbers within `max_ulps` ULP.
pub fn line_tokens_equivalent(got: &str, want: &str, max_ulps: u64) -> bool {
    line_matches(got, want, |a, b| ulp_close(a, b, max_ulps))
}

/// [`line_matches`] with numbers within AST's `astEQUAL` tolerance.
pub fn line_tokens_astequal(got: &str, want: &str) -> bool {
    line_matches(got, want, ast_equal_close)
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

/// Whether the C values the oracle tests compare against were captured on this
/// platform. They come from the reference library built on macOS, where the
/// port, calling the same libm, matches them bit for bit. Setting
/// `AST_ORACLE_TOLERANT` makes macOS compare as other platforms do, to check
/// the tolerant comparisons locally.
pub fn on_capture_platform() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("AST_ORACLE_TOLERANT").is_none()
}

/// Relative tolerance for a C value on any other platform, where libm rounds
/// some last bits differently. The largest difference the Linux CI run of
/// 2026-10-15 showed across 24 oracle tests was 3.6e-14 relative; 1e-13 keeps
/// a margin while staying far below any algorithmic difference.
pub const LIBM_RTOL: f64 = 1.0e-13;

/// Absolute floor for [`LIBM_RTOL`], for values near zero that are the small
/// difference of O(1) quantities: the same CI run's largest was 3.8e-15.
pub const LIBM_ATOL: f64 = 1.0e-14;

/// Whether `got` matches the C value `want`: exactly on the platform the C
/// values were captured on, and to within libm rounding
/// ([`LIBM_RTOL`], [`LIBM_ATOL`]) elsewhere. A bad value matches only a bad
/// value, and NaN matches nothing, as with `==`.
pub fn c_value_eq(got: f64, want: f64) -> bool {
    if got == want {
        return true;
    }
    !on_capture_platform() && within_libm_rounding(got, want)
}

/// The comparison [`c_value_eq`] makes off the capture platform: within
/// [`LIBM_RTOL`] and [`LIBM_ATOL`], with a bad value or NaN matching nothing.
pub fn within_libm_rounding(got: f64, want: f64) -> bool {
    let bad = ast_core::constants::BAD_VALUE;
    if !got.is_finite() || !want.is_finite() || got == bad || want == bad {
        return got == want;
    }
    (got - want).abs() <= LIBM_ATOL + LIBM_RTOL * want.abs()
}

/// [`c_value_eq`] element by element, with equal lengths.
pub fn c_values_eq(got: &[f64], want: &[f64]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| c_value_eq(*g, *w))
}

/// Assert [`c_values_eq`], reporting both sides and `what` on failure.
#[track_caller]
pub fn assert_c_values(got: &[f64], want: &[f64], what: &str) {
    assert!(
        c_values_eq(got, want),
        "{what}: got {got:?}, C gives {want:?}"
    );
}

/// Assert [`c_text_eq`], reporting both texts and `what` on failure.
#[track_caller]
pub fn assert_c_text(got: &str, want: &str, what: &str) {
    assert!(
        c_text_eq(got, want),
        "{what}:\n--- got\n{got}\n--- C\n{want}"
    );
}

/// Whether the text `got` (a dump or report) matches the C text `want`:
/// exactly on the capture platform, and elsewhere line by line with every
/// whitespace-separated numeric token compared by [`c_value_eq`] and every
/// other token exactly.
pub fn c_text_eq(got: &str, want: &str) -> bool {
    if got == want {
        return true;
    }
    if on_capture_platform() {
        return false;
    }
    let mut g = got.lines();
    let mut w = want.lines();
    loop {
        match (g.next(), w.next()) {
            (None, None) => return true,
            (Some(gl), Some(wl)) if gl == wl => continue,
            (Some(gl), Some(wl)) => {
                let mut gt = gl.split_whitespace();
                let mut wt = wl.split_whitespace();
                loop {
                    match (gt.next(), wt.next()) {
                        (None, None) => break,
                        (Some(a), Some(b)) if a == b => continue,
                        (Some(a), Some(b)) => {
                            let num =
                                |t: &str| t.trim_matches(|c| c == ',' || c == '"').parse::<f64>();
                            match (num(a), num(b)) {
                                (Ok(x), Ok(y)) if c_value_eq(x, y) => continue,
                                _ => return false,
                            }
                        }
                        _ => return false,
                    }
                }
            }
            _ => return false,
        }
    }
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
/// collision history, so identical KeyMaps can serialize differently".
pub fn is_map_sz_card(line: &str) -> bool {
    line.trim_start()
        .trim_start_matches('#')
        .trim_start()
        .starts_with("MapSz =")
}

/// The identity transformation the C test programs register for IntraMap
/// fixtures (`TestIntraTran` in `ast_tester/simplify.c`, `roundtrip.c` and
/// `ast_astequal.c`): each output coordinate copies the input coordinate of
/// the same index, bad where there is none.
struct TestIntraTran {
    name: &'static str,
    flags: ast_core::mappings::IntraFlags,
    purpose: &'static str,
}

impl ast_core::mappings::IntraTransform for TestIntraTran {
    fn name(&self) -> &str {
        self.name
    }
    fn nin(&self) -> Option<usize> {
        Some(1)
    }
    fn nout(&self) -> Option<usize> {
        Some(1)
    }
    fn flags(&self) -> ast_core::mappings::IntraFlags {
        self.flags
    }
    fn purpose(&self) -> &str {
        self.purpose
    }
    fn author(&self) -> &str {
        "AST test suite"
    }
    fn contact(&self) -> &str {
        "starlink-ast"
    }
    fn transform(
        &self,
        _map: &ast_core::mappings::IntraMap,
        input: &ast_core::coords::Points<'_>,
        _forward: bool,
        out: &mut ast_core::coords::PointsMut<'_>,
    ) -> ast_core::error::AstResult<()> {
        let ncoord_in = input.ncoord();
        for ip in 0..input.npoint() {
            let row = input.point(ip).to_vec();
            for (ic, v) in out.point_mut(ip).iter_mut().enumerate() {
                *v = if ic < ncoord_in {
                    row[ic]
                } else {
                    ast_core::constants::BAD_VALUE
                };
            }
        }
        Ok(())
    }
}

/// The four transformations the C test programs register for the IntraMap
/// simplify fixtures, with the same names, flags and descriptions.
pub fn test_intra_transforms() -> Vec<std::sync::Arc<dyn ast_core::mappings::IntraTransform>> {
    use ast_core::mappings::IntraFlags;
    let both = IntraFlags {
        simp_fi: true,
        simp_if: true,
        ..IntraFlags::default()
    };
    [
        (
            "simplifyidentity",
            both,
            "Identity IntraMap for simplify fixtures",
        ),
        (
            "simplifyidentity2",
            both,
            "Second identity IntraMap for simplify fixtures",
        ),
        (
            "nosimpfi",
            IntraFlags {
                simp_if: true,
                ..IntraFlags::default()
            },
            "IntraMap without SIMPFI for simplify fixtures",
        ),
        (
            "nosimpif",
            IntraFlags {
                simp_fi: true,
                ..IntraFlags::default()
            },
            "IntraMap without SIMPIF for simplify fixtures",
        ),
    ]
    .into_iter()
    .map(|(name, flags, purpose)| {
        std::sync::Arc::new(TestIntraTran {
            name,
            flags,
            purpose,
        }) as std::sync::Arc<dyn ast_core::mappings::IntraTransform>
    })
    .collect()
}

/// Read Native text with the C test programs' IntraMap transformations
/// registered, so the IntraMap fixtures load as they do for C's harnesses.
pub fn from_native_string(text: &str) -> ast_core::error::AstResult<ast_core::AnyObject> {
    let mut registry = ast_core::mappings::IntraRegistry::new();
    for t in test_intra_transforms() {
        registry.register(t)?;
    }
    ast_core::from_native_string_with(text, &registry)
}

/// The Native dump a Channel with no attributes set writes for `obj`:
/// comments included and nested objects indented, as the reference
/// fixtures were written.
pub fn native_dump(obj: &ast_core::AnyObject) -> ast_core::error::AstResult<String> {
    use ast_core::{Channel, NativeChannel};
    let mut ch = NativeChannel::with_string_sink();
    ch.write(obj)?;
    Ok(ch.into_string())
}
