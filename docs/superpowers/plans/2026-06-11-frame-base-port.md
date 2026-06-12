# Frame base C-vs-Rust port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the remaining `frame.c` / `axis.c` functionality flagged in section 6 of `docs/issues/frame-mapping-c-port-gaps.md` — printf Format + Digits, the geometry/axis method family, PrimaryFrame, ActiveUnit adaptation, the matchsub cast-retry, and a generic `astConvertX` driver — into `ast-core`.

**Architecture:** Adopt C's single-driver structure: one generic `convert_frames` (porting `astConvertX`) replaces the per-class `convert` overrides, so all alignment flows through `match_template`/`sub_frame`. New `Frame::cast` models `astCast` for the matchsub retry. Format delegates to a new `axis_format` module that ports `ParseAxisFormat`/`AxisFormat`. The geometry methods are added as `Frame` trait methods with Cartesian base impls.

**Tech Stack:** Rust 2024, `ast-core` crate, the reference C library (`reference/starlink-ast`) for oracle verification.

**Authoritative references:**
- Design spec: `docs/superpowers/specs/2026-06-11-frame-base-port-design.md`.
- C source: `reference/starlink-ast/src/frame.c`, `axis.c`, `cmpframe.c`.
- Acceptance test: `reference/starlink-ast/ast_tester/testconvert.c`.

**Conventions (enforced every commit):**
- `cargo build`, `cargo test -p ast-core`, `cargo fmt`, `cargo clippy --workspace -- -D warnings` all clean before moving on.
- `#![warn(missing_docs)]` is active — every new public item needs a doc comment.
- Mirror C control flow directly; cite the C source line in comments. No heuristics the C lacks; no compute-then-check shortcuts. If a faithful port is genuinely ambiguous, stop and ask.
- **On any conflict between existing Rust behaviour/tests/fixtures and the C library, C wins.** When a Rust test or native-dump fixture disagrees with the C-faithful implementation, regenerate the expected value from the C oracle and fix toward C; never revert the C-faithful code or weaken a test below what C guarantees. (This otherwise bites us during oracle validation later — a Rust-only expectation that "passes" green is a latent divergence.)
- 1-based axis indices on the public API.
- Comments describe the code as it is today; do not reference this plan or porting history.

---

## File Structure

**Created:**
- `crates/ast-core/src/frames/axis_format.rs` — ports `ParseAxisFormat` + the numeric formatting half of `AxisFormat`; a faithful C `%G`/`%E`/`%f`/`%d` formatter and the AST flag parser. One responsibility: turn `(format_string, digits, value)` into a string.
- `crates/ast-core/tests/test_convert.rs` — tier-1 acceptance test ported from `testconvert.c`.
- `crates/ast-core/tests/test_frame_geometry.rs` — tier-1 tests for the geometry/axis method family.
- `crates/ast-core/tests/test_frame_format.rs` — tier-1 tests for Format/Digits/Unformat.

**Modified:**
- `crates/ast-core/src/frames/mod.rs` — new trait methods (`cast`, `digits`/`set_digits`, `direction`, geometry family, `primary_frame`, `get_active_unit`/`set_active_unit`, `convert_to` rename); new `convert_frames`-calling default body; helper free functions.
- `crates/ast-core/src/frames/basic.rs` — Digits/Direction storage, printf Format, C-faithful Unformat, geometry overrides where Cartesian-specific, ActiveUnit storage reconciliation, serialization.
- `crates/ast-core/src/frames/find_frame.rs` — `convert_frames` (astConvertX port), the matchsub-retry wrapper, the ActiveUnit tail in `default_sub_frame`.
- `crates/ast-core/src/frames/{skyframe,specframe,cmpframe,timeframe,dsbspecframe,fluxframe,specfluxframe}.rs` — retire `convert` overrides; add `cast`, `primary_frame`, `get_active_unit` overrides as needed.
- `crates/ast-core/src/frames/specframe.rs` — `primary_frame_walk` delegates to / is replaced by the trait method.
- `crates/ast-core/src/fitschan/fset_to_store/make_int_world.rs`, `celestial_axes.rs` — delete private `primary_frame`, call the trait method.
- All `convert(` call sites (regions, fitschan) — rename to `convert_to(`.
- `docs/issues/frame-mapping-c-port-gaps.md` — mark items fixed; record deferrals.
- `docs/issues/c-library-quirks.md` — record the Format log-escape deferral.
- `PLAN.md` — add `testconvert.c` (ported) / `testframe.c` (deferred) tracker entries.

---

## Commit 1 — printf Format + Digits + Unformat

Implements section 5 of the spec. Self-contained: no dependency on later commits.

### Task 1.1: C `%G` formatter core

**Files:**
- Create: `crates/ast-core/src/frames/axis_format.rs`
- Modify: `crates/ast-core/src/frames/mod.rs` (add `pub(crate) mod axis_format;` near the other `mod` lines, around line 3)

- [ ] **Step 1: Register the module**

In `crates/ast-core/src/frames/mod.rs`, add after the existing `mod basic;` line:

```rust
pub(crate) mod axis_format;
```

- [ ] **Step 2: Write the failing test for `format_g`**

Create `crates/ast-core/src/frames/axis_format.rs` with:

```rust
//! Port of `axis.c`'s `ParseAxisFormat` and the numeric-formatting
//! half of `AxisFormat`. Turns an Axis `Format` string plus a `Digits`
//! precision into the formatted coordinate string, reproducing C
//! `printf` semantics for the `%G` / `%E` / `%f` / `%d` subset that
//! AST uses. The default format is `"%1.<Digits>G"`.

/// Format `value` like C `printf("%1.<prec>G", value)`: `prec`
/// significant digits, `%E` notation when the decimal exponent is
/// `< -4` or `>= prec`, otherwise `%f` notation, with trailing zeros
/// removed and the exponent printed as `E±dd` (>= 2 digits). Mirrors
/// the C library / glibc `%G` conversion (axis.c:626 formats via this).
pub(crate) fn format_g(value: f64, prec: usize) -> String {
    let prec = prec.max(1);
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        // glibc prints "inf"/"nan"; AST never formats these through a
        // good path, but stay deterministic.
        return if value.is_nan() {
            "nan".to_string()
        } else if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }

    let exp = value.abs().log10().floor() as i32;
    if exp < -4 || exp >= prec as i32 {
        format_e(value, prec - 1, true)
    } else {
        // Fixed notation: prec - 1 - exp fractional digits.
        let frac = (prec as i32 - 1 - exp).max(0) as usize;
        let s = format!("{value:.frac$}");
        trim_g_zeros(&s)
    }
}

/// Format like C `printf("%.<mant>E")`: one leading digit, `mant`
/// mantissa fraction digits, exponent `E±dd`. When `strip` is true
/// (the `%G` caller), trailing mantissa zeros are removed.
fn format_e(value: f64, mant: usize, strip: bool) -> String {
    // Rust's `{:e}` gives e.g. "1.234568e3" (lowercase, no sign, no
    // zero-pad). Re-render into C `%E` form.
    let raw = format!("{value:.mant$e}");
    let (mantissa, exp_str) = raw.split_once('e').expect("exp form");
    let exp: i32 = exp_str.parse().expect("exp int");
    let mantissa = if strip {
        trim_g_zeros(mantissa)
    } else {
        mantissa.to_string()
    };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}E{sign}{:02}", exp.abs())
}

/// Remove trailing zeros after a decimal point, then a trailing bare
/// decimal point — the `%G` no-`#` behaviour. Leaves integers and
/// strings without a `.` untouched.
fn trim_g_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    let t = t.strip_suffix('.').unwrap_or(t);
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_matches_c_examples() {
        // Verified against C printf("%1.7G", x).
        assert_eq!(format_g(0.0, 7), "0");
        assert_eq!(format_g(1234.5678, 7), "1234.568");
        assert_eq!(format_g(0.00012345678, 7), "0.00012345678");
        assert_eq!(format_g(123456789.0, 7), "1.234568E+08");
        assert_eq!(format_g(-9.5, 7), "-9.5");
        assert_eq!(format_g(1.0, 7), "1");
    }
}
```

- [ ] **Step 3: Run test to verify it fails to compile then build**

Run: `cargo test -p ast-core --lib axis_format -- --nocapture`
Expected: compiles; `g_matches_c_examples` PASS. If any case differs, build the oracle (Task 1.2) to get the authoritative string before changing the test expectation — do not adjust the expected value to match buggy code.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/axis_format.rs crates/ast-core/src/frames/mod.rs
git commit -m "frames: add C %G coordinate formatter core

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.2: Oracle-verify `format_g` against C printf

**Files:**
- Create (temporary, not committed): `/tmp/gfmt_probe.c`

- [ ] **Step 1: Write a C probe**

```c
#include <stdio.h>
int main(void){
  double v[] = {0.0,1234.5678,0.00012345678,123456789.0,-9.5,1.0,
                9.999999e6, 1e-5, 0.1};
  for(int i=0;i<9;i++) printf("%1.7G\n", v[i]);
  return 0;
}
```

- [ ] **Step 2: Compile and run**

Run: `cc /tmp/gfmt_probe.c -o /tmp/gfmt_probe && /tmp/gfmt_probe`
Expected: the authoritative C strings. Compare each against `format_g(v, 7)`; if any Rust output differs, fix `format_g` (not the test) to match C exactly, then re-run Task 1.1 Step 3.

- [ ] **Step 3: No commit** (verification only; `format_g` fixes, if any, amend Task 1.1's commit or land as a follow-up commit `frames: match C %G edge cases`).

### Task 1.3: `ParseAxisFormat` flag parser + dispatch

**Files:**
- Modify: `crates/ast-core/src/frames/axis_format.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `axis_format.rs`:

```rust
    #[test]
    fn parse_default_and_flags() {
        let p = parse_axis_format("%1.7G", 7);
        assert_eq!(p.spec_prec, 7);
        assert_eq!(p.conv, 'G');
        assert!(!p.integ && !p.log);

        // ".*" precision is replaced by Digits.
        let p = parse_axis_format("%.*G", 5);
        assert_eq!(p.spec_prec, 5);

        // Integer conversion code.
        let p = parse_axis_format("%d", 7);
        assert!(p.integ);
        assert_eq!(p.conv, 'd');

        // '&' log flag.
        let p = parse_axis_format("%&G", 7);
        assert!(p.log);
    }

    #[test]
    fn format_value_dispatch() {
        assert_eq!(format_value("%1.7G", 7, 1234.5678), "1234.568");
        assert_eq!(format_value("%d", 7, 42.9), "42");
        assert_eq!(format_value("%.2f", 7, 3.14159), "3.14");
    }
```

- [ ] **Step 2: Implement the parser and dispatch**

Add to `axis_format.rs` (above the `tests` module):

```rust
/// Parsed view of an Axis `Format` string. Mirrors the flags returned
/// by C `ParseAxisFormat` (axis.c:2015) plus the resolved precision and
/// conversion code needed to format a value.
pub(crate) struct ParsedFormat {
    /// Conversion code (`G`, `g`, `E`, `e`, `f`, `F`, `d`, `i`, ...).
    pub conv: char,
    /// Precision (significant digits for G/g, fraction digits for e/f).
    pub spec_prec: usize,
    /// Format the value as `10**x` using graphical escapes. Parsed but
    /// rendered with the non-Plot delimiter (Plot escapes deferred).
    pub log: bool,
    /// Cast the value to `int` before formatting (integer conv code).
    pub integ: bool,
}

/// Parse an Axis `Format` string into a [`ParsedFormat`], substituting
/// `digits` for a `.*` precision field. Mirrors C `ParseAxisFormat`.
pub(crate) fn parse_axis_format(fmt: &str, digits: usize) -> ParsedFormat {
    let mut log = false;
    let mut integ = false;
    let mut conv = 'G';
    let mut spec_prec = digits;

    if let Some(perc) = fmt.find('%') {
        let rest = &fmt[perc + 1..];
        let mut chars = rest.char_indices().peekable();

        // Flags (printf + AST '&'). Mirrors the flag-scan loop.
        let mut idx_after_flags = 0;
        for (i, c) in rest.char_indices() {
            match c {
                '-' | '+' | ' ' | '#' => {}
                '&' => log = true,
                _ => {
                    idx_after_flags = i;
                    break;
                }
            }
            idx_after_flags = i + c.len_utf8();
        }
        let _ = chars; // (kept for clarity; iteration is index-based)

        let tail = &rest[idx_after_flags..];

        // Precision: look for ".<digits>" or ".*". `.*` => digits.
        if let Some(dot) = tail.find('.') {
            let after = &tail[dot + 1..];
            if after.starts_with('*') {
                spec_prec = digits;
            } else {
                let num: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(p) = num.parse::<usize>() {
                    spec_prec = p;
                }
            }
        }

        // Conversion code: first alphabetic char in the tail.
        if let Some(c) = tail.chars().find(|c| c.is_ascii_alphabetic()) {
            conv = c;
            if matches!(c, 'd' | 'i' | 'u' | 'o' | 'x' | 'X' | 'c') {
                integ = true;
            }
        }
    }

    ParsedFormat { conv, spec_prec, log, integ }
}

/// Format `value` according to an Axis `Format` string and the default
/// `digits` precision. Mirrors the numeric path of C `AxisFormat`
/// (axis.c:626): default `%1.<digits>G`, integer-cast for integer
/// conversion codes, `%G`/`%E`/`%f` rendering. The `&` log flag is
/// parsed; its graphical-escape rendering is deferred (Plot, step 10),
/// so a log format currently falls back to formatting the raw value.
pub(crate) fn format_value(fmt: &str, digits: usize, value: f64) -> String {
    let p = parse_axis_format(fmt, digits);
    if p.integ {
        return format!("{}", value as i64);
    }
    match p.conv {
        'G' | 'g' => format_g(value, p.spec_prec),
        'E' | 'e' => format_e(value, p.spec_prec, false),
        'f' | 'F' => format!("{value:.*}", p.spec_prec),
        // Unknown/unsupported code: fall back to %G with the precision.
        _ => format_g(value, p.spec_prec),
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ast-core --lib axis_format`
Expected: `parse_default_and_flags`, `format_value_dispatch`, `g_matches_c_examples` all PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/axis_format.rs
git commit -m "frames: port ParseAxisFormat flag parsing and format dispatch

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.4: Digits / Direction attributes on Frame trait + BasicFrame

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait getters)
- Modify: `crates/ast-core/src/frames/basic.rs` (storage, accessors)

- [ ] **Step 1: Add trait getters with C defaults**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`, after the `permute` block (around line 216), add:

```rust
    /// `Digits`: default number of significant figures used when
    /// formatting coordinate values. Mirrors C `astGetDigits`
    /// (frame.c:13134) — default 7.
    fn digits(&self) -> i32 {
        7
    }

    /// Set the `Digits` attribute. C constrains the stored value to be
    /// at least 1 (frame.c:13138).
    fn set_digits(&mut self, _value: i32) {}

    /// Test whether `Digits` has been explicitly set.
    fn test_digits(&self) -> bool {
        false
    }

    /// Per-axis `Digits(axis)` (1-based). Falls back to the frame-level
    /// `Digits` when no axis-specific value is set. Mirrors C
    /// `astGetAxisDigits` resolution in `Format` (frame.c:4260).
    fn axis_digits(&self, _axis: usize) -> i32 {
        self.digits()
    }

    /// Per-axis `Direction(axis)` (1-based). Plot hint; default 1.
    /// Mirrors C `astGetAxisDirection` (axis.c) — default 1.
    fn direction(&self, _axis: usize) -> bool {
        true
    }
```

- [ ] **Step 2: Add storage + accessors to BasicFrame**

In `crates/ast-core/src/frames/basic.rs`, add fields to both `struct BasicFrame` and `struct BasicFrameBuilder` (after `tops`):

```rust
    digits: Option<i32>,
    axis_digits: Vec<Option<i32>>,
    directions: Vec<Option<bool>>,
```

In `BasicFrameBuilder::new`, initialise:

```rust
            digits: None,
            axis_digits: vec![None; naxes],
            directions: vec![None; naxes],
```

In `BasicFrameBuilder::build`, copy them into the constructed `BasicFrame`:

```rust
            digits: self.digits,
            axis_digits: self.axis_digits,
            directions: self.directions,
```

Add inherent accessors on `impl BasicFrame` (near the epoch accessors):

```rust
    /// Frame-level Digits, defaulting to 7. Mirrors `astGetDigits`.
    pub fn digits(&self) -> i32 {
        self.digits.unwrap_or(7)
    }

    /// Set frame-level Digits, constrained to >= 1 (frame.c:13138).
    pub fn set_digits(&mut self, value: i32) {
        self.digits = Some(value.max(1));
    }

    /// Test whether frame-level Digits is explicitly set.
    pub fn test_digits(&self) -> bool {
        self.digits.is_some()
    }

    /// Per-axis Digits (1-based), falling back to frame-level Digits.
    pub fn axis_digits(&self, axis: usize) -> AstResult<i32> {
        self.validate_axis(axis)?;
        Ok(self.axis_digits[axis - 1].unwrap_or_else(|| self.digits()))
    }

    /// Set per-axis Digits (1-based), constrained to >= 1.
    pub fn set_axis_digits(&mut self, axis: usize, value: i32) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.axis_digits[axis - 1] = Some(value.max(1));
        Ok(())
    }

    /// Test whether per-axis Digits is explicitly set (1-based).
    pub fn test_axis_digits(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.axis_digits[axis - 1].is_some())
    }

    /// Per-axis Direction Plot hint (1-based); default true.
    pub fn direction(&self, axis: usize) -> AstResult<bool> {
        self.validate_axis(axis)?;
        Ok(self.directions[axis - 1].unwrap_or(true))
    }

    /// Set per-axis Direction (1-based).
    pub fn set_direction(&mut self, axis: usize, value: bool) -> AstResult<()> {
        self.validate_axis(axis)?;
        self.directions[axis - 1] = Some(value);
        Ok(())
    }
```

Override the trait getters in `impl Frame for BasicFrame`:

```rust
    fn digits(&self) -> i32 {
        BasicFrame::digits(self)
    }
    fn set_digits(&mut self, value: i32) {
        BasicFrame::set_digits(self, value);
    }
    fn test_digits(&self) -> bool {
        BasicFrame::test_digits(self)
    }
    fn axis_digits(&self, axis: usize) -> i32 {
        BasicFrame::axis_digits(self, axis).unwrap_or(7)
    }
    fn direction(&self, axis: usize) -> bool {
        BasicFrame::direction(self, axis).unwrap_or(true)
    }
```

Extend `perm_axes` in `impl Frame for BasicFrame` to permute the new per-axis vecs too — add after the existing `permute_slice(&mut self.tops, perm);`:

```rust
        permute_slice(&mut self.axis_digits, perm);
        permute_slice(&mut self.directions, perm);
```

- [ ] **Step 3: Build**

Run: `cargo build -p ast-core`
Expected: compiles clean (no test yet — exercised via Task 1.5/1.7).

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/src/frames/basic.rs
git commit -m "frames: add Digits and Direction attributes

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.5: Wire BasicFrame::format to the printf formatter

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs:471-485` (the `format` method)
- Test: `crates/ast-core/tests/test_frame_format.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_frame_format.rs`:

```rust
//! Frame `Format` / `Unformat` tests — printf-style formatting with the
//! `Digits` attribute and the C `astUnformat` whitespace/error contract
//! (frame.c:4260, axis.c:626 / 1284).

use ast_core::frames::{BasicFrameBuilder, Frame};

#[test]
fn format_default_is_g_7() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    // Default format "%1.7G".
    assert_eq!(f.format(1, 1234.5678).unwrap(), "1234.568");
    assert_eq!(f.format(1, 123456789.0).unwrap(), "1.234568E+08");
}

#[test]
fn format_bad_is_bad_token() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    assert_eq!(f.format(1, ast_core::constants::BAD_VALUE).unwrap(), "<bad>");
}

#[test]
fn format_honours_digits() {
    let mut f = BasicFrameBuilder::new(1).build().unwrap();
    f.set_digits(3);
    // "%1.3G" of 1234.5678 -> "1.23E+03".
    assert_eq!(f.format(1, 1234.5678).unwrap(), "1.23E+03");
}

#[test]
fn format_explicit_format_string() {
    let mut f = BasicFrameBuilder::new(1).build().unwrap();
    f.set_format_str(1, "%.2f").unwrap();
    assert_eq!(f.format(1, 3.14159).unwrap(), "3.14");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_frame_format`
Expected: FAIL — `format_honours_digits` / `format_explicit_format_string` differ (current code uses bare-precision parse + Rust `Display`).

- [ ] **Step 3: Replace the `format` body**

In `crates/ast-core/src/frames/basic.rs`, replace the `fn format` body (lines ~471-485) with:

```rust
    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        // C AxisFormat returns "<bad>" for the bad value (axis.c:704).
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // Resolve the effective Format string: the per-axis Format if
        // set, else the default "%1.<Digits>G" with the per-axis Digits
        // (frame.c:4260 -> GetDefaultFormat axis.c:1819).
        let digits = self.axis_digits(axis)?.max(1) as usize;
        let fmt = match &self.format_strs[axis - 1] {
            Some(s) if !s.is_empty() => s.clone(),
            _ => format!("%1.{digits}G"),
        };
        Ok(crate::frames::axis_format::format_value(&fmt, digits, value))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_frame_format`
Expected: PASS (all four). If `format_honours_digits` differs, oracle-check `%1.3G` of 1234.5678 with the C probe pattern from Task 1.2 and fix the formatter, not the test.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/basic.rs crates/ast-core/tests/test_frame_format.rs
git commit -m "frames: format coordinates with printf Format and Digits

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.6: C-faithful Unformat contract

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs:487-520` (the `unformat` method)
- Test: `crates/ast-core/tests/test_frame_format.rs`

- [ ] **Step 1: Write the failing test**

Append to `test_frame_format.rs`:

```rust
#[test]
fn unformat_reads_number_and_trailing_ws() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    // C `sscanf("%lf %n")` counts trailing whitespace in nread.
    let (v, n) = f.unformat(1, "12.5  ").unwrap();
    assert_eq!(v, 12.5);
    assert_eq!(n, 6);
}

#[test]
fn unformat_failure_is_nread_zero_no_error() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    // C returns nread=0 with NO error on a non-numeric string.
    let (_v, n) = f.unformat(1, "xyz").unwrap();
    assert_eq!(n, 0);
}

#[test]
fn unformat_recognises_bad_token() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    let (v, n) = f.unformat(1, " <bad> ").unwrap();
    assert_eq!(v, ast_core::constants::BAD_VALUE);
    assert!(n > 0);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_frame_format unformat`
Expected: FAIL — current code raises `Unfer` on failure and stops nread at the last numeric char.

- [ ] **Step 3: Replace the `unformat` body**

In `crates/ast-core/src/frames/basic.rs`, replace the `fn unformat` body with a port of `AxisUnformat` (axis.c:1284). Add `use crate::constants::BAD_VALUE;` reliance (already imported at top):

```rust
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;

        // Mirror C `astSscanf("%lf %n")`: skip leading whitespace, read
        // the longest numeric prefix, then count any trailing whitespace.
        let leading_ws = text.len() - text.trim_start().len();
        let after_ws = &text[leading_ws..];

        // Longest prefix of `after_ws` parsing as f64.
        let mut best_len = 0;
        let mut best_val: Option<f64> = None;
        for end in 1..=after_ws.len() {
            if after_ws.is_char_boundary(end) {
                if let Ok(v) = after_ws[..end].parse::<f64>() {
                    best_len = end;
                    best_val = Some(v);
                }
            }
        }

        if let Some(v) = best_val {
            // Count trailing whitespace consumed by `%n` after `%lf`.
            let rest = &after_ws[best_len..];
            let trailing_ws = rest.len() - rest.trim_start().len();
            return Ok((v, leading_ws + best_len + trailing_ws));
        }

        // Recognise "<bad>" (case-insensitive, embedded/edge whitespace)
        // => BAD_VALUE (axis.c:1357).
        let compact: String = after_ws
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if compact.starts_with("<bad>") {
            // nread spans through the closing '>' (+ its trailing ws is
            // not counted by the C `%n` placement; match through '>').
            let upto = after_ws
                .char_indices()
                .scan(0usize, |seen, (i, c)| {
                    if !c.is_whitespace() {
                        *seen += 1;
                    }
                    Some((i, c, *seen))
                })
                .find(|&(_, c, seen)| seen == 5 && c == '>')
                .map(|(i, c, _)| i + c.len_utf8());
            if let Some(end) = upto {
                return Ok((BAD_VALUE, leading_ws + (end - 0)));
            }
        }

        // Parse failure: nread = 0, no error (axis.c:1366).
        Ok((BAD_VALUE, 0))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_frame_format`
Expected: PASS (all unformat + format tests).

- [ ] **Step 5: Check no caller depended on the old `Unfer` error**

Run: `grep -rn "Unfer\|unformat" crates/ast-core/src | grep -v "basic.rs"`
Expected: review hits. If any caller treats an `Err` from `unformat` as "invalid", confirm it now handles `nread == 0` instead. Adjust those callers in this commit if needed (show the diff in the commit). If a caller genuinely needs the error, STOP and ask — do not silently keep both behaviours.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/basic.rs crates/ast-core/tests/test_frame_format.rs
git commit -m "frames: match C astUnformat whitespace and no-error contract

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.7: Serialize Digits / Direction (round-trip)

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs` (`to_node` / `from_node`, plus the in-module `tests` block)

- [ ] **Step 1: Write the failing round-trip test**

`Serializable` is `pub(crate)`, so this must be an in-module unit test, not in the `tests/` crate. Append to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/ast-core/src/frames/basic.rs`:

```rust
    #[test]
    fn digits_direction_round_trip() {
        let mut f = BasicFrameBuilder::new(2).build().unwrap();
        f.set_digits(9);
        f.set_axis_digits(1, 4).unwrap();
        f.set_direction(2, false).unwrap();

        let node = f.to_node().unwrap();
        let back = BasicFrame::from_node(&node).unwrap();
        assert_eq!(back.digits(), 9);
        assert_eq!(back.axis_digits(1).unwrap(), 4);
        assert!(!back.direction(2).unwrap());
    }
```

(`Serializable::to_node` / `from_node` are in scope inside `basic.rs` via the `use ... Serializable` at the top of the file.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core digits_direction_round_trip`
Expected: FAIL — Digits/Dirn not yet emitted/read.

- [ ] **Step 3: Emit in `to_node`**

In `BasicFrame::to_node`, after the Epoch block and before the per-axis Label loop, add the frame-level Digits (token `Digits`, C frame.c:14486 — only active when set):

```rust
        if let Some(d) = self.digits {
            node.push_int("Digits", d as i64, Some("Default formatting precision".to_string()));
        }
```

Inside the per-axis `Axis` child loop (where Label/Symbol/Unit/Format are pushed), add Digits (`Digits`, axis.c:2939) and Direction (`Dirn`, axis.c:2953):

```rust
            if let Some(d) = self.axis_digits[i] {
                axis_node.push_int("Digits", d as i64, Some("Default formatting precision".to_string()));
            }
            if let Some(dir) = self.directions[i] {
                axis_node.push_int(
                    "Dirn",
                    if dir { 1 } else { 0 },
                    Some(if dir {
                        "Plot in conventional direction (hint)".to_string()
                    } else {
                        "Plot in reverse direction (hint)".to_string()
                    }),
                );
            }
```

- [ ] **Step 4: Read in `from_node`**

In `BasicFrame::from_node`, after the Epoch read, add frame Digits:

```rust
        if node.get("Digits").is_some() {
            builder.digits = Some(node.get_int("Digits", 7) as i32);
        }
```

Inside the per-axis `Ax{n}` child read block, add:

```rust
                if axis_node.get("Digits").is_some() {
                    builder.axis_digits[i] = Some(axis_node.get_int("Digits", 7) as i32);
                }
                if axis_node.get("Dirn").is_some() {
                    builder.directions[i] = Some(axis_node.get_int("Dirn", 1) != 0);
                }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p ast-core --test test_frame_format` and `cargo test -p ast-core --lib frames::basic`
Expected: PASS, including any existing serialization regression tests (they must not change output for frames without Digits/Dirn set, since those are only emitted when set).

- [ ] **Step 6: Run the native-format regression suite**

Run: `cargo test -p ast-core --test '*-regression'`
Expected: PASS — confirms no byte-exact native dumps regressed.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/frames/basic.rs crates/ast-core/tests/test_frame_format.rs
git commit -m "frames: serialise Digits and Direction attributes

Generated with AI

Co-Authored-By: SLAC AI"
```

- [ ] **Step 6: Run the native-format regression suite**

Run: `cargo test -p ast-core --test '*-regression'`
Expected: PASS — no byte-exact native dumps regressed (Digits/Dirn only emitted when set).

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/frames/basic.rs
git commit -m "frames: serialise Digits and Direction attributes

Generated with AI

Co-Authored-By: SLAC AI"
```

---

## Commit 2 — PrimaryFrame + geometry/axis method family

Implements sections 6 and 7 of the spec. Depends on nothing in Commit 1; can land in parallel but is sequenced second.

### Task 2.1: Promote `primary_frame` to a Frame trait method

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait method)
- Modify: `crates/ast-core/src/frames/cmpframe.rs` (override)
- Modify: `crates/ast-core/src/frames/skyframe.rs` (override)
- Modify: `crates/ast-core/src/frames/specframe.rs` (`primary_frame_walk` delegates)

- [ ] **Step 1: Add the trait method**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`, after `overlay_template` (around line 354), add:

```rust
    /// Identify the primary (leaf, non-compound) Frame and underlying
    /// axis for a given axis of this Frame. `axis` is 0-based (matching
    /// the existing `primary_frame_walk` callers and C `astPrimaryFrame`
    /// internal convention). Returns the leaf frame and the 0-based axis
    /// within it, undoing all axis permutations and Frame combination.
    /// Mirrors C `astPrimaryFrame` (frame.c:8818). Base impl: `self` is
    /// already primary, so return `(self, axis)`.
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }
```

(Index base note: the current `primary_frame_walk` is 0-based; keep that to avoid churning its callers. Documented above.)

- [ ] **Step 2: Override on CmpFrame**

In `crates/ast-core/src/frames/cmpframe.rs`, add to `impl Frame for CmpFrame`:

```rust
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        // External axis -> internal via perm, then descend into the
        // owning component (cmpframe.c PrimaryFrame).
        let internal = self.perm()[axis];
        let n1 = self.naxes1();
        if internal < n1 {
            self.frame1().primary_frame(internal)
        } else {
            self.frame2().primary_frame(internal - n1)
        }
    }
```

- [ ] **Step 3: Override on SkyFrame**

In `crates/ast-core/src/frames/skyframe.rs`, add to `impl Frame for SkyFrame`:

```rust
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        // Apply the SkyFrame's external->internal axis permutation so
        // the returned axis is the underlying physical axis
        // (0 = lon, 1 = lat).
        (self, self.axis_permutation()[axis])
    }
```

- [ ] **Step 4: Make `primary_frame_walk` delegate**

In `crates/ast-core/src/frames/specframe.rs`, replace the body of `primary_frame_walk` (around line 1951) with a one-line delegation:

```rust
pub(crate) fn primary_frame_walk(frame: &dyn Frame, axis: usize) -> (&dyn Frame, usize) {
    frame.primary_frame(axis)
}
```

- [ ] **Step 5: Build + run existing frame tests**

Run: `cargo build -p ast-core && cargo test -p ast-core --lib frames`
Expected: PASS — behaviour is identical (the trait method reproduces the old walk exactly).

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/src/frames/cmpframe.rs crates/ast-core/src/frames/skyframe.rs crates/ast-core/src/frames/specframe.rs
git commit -m "frames: add PrimaryFrame as a Frame trait method

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.2: Unwind the two fitschan `primary_frame` copies

**Files:**
- Modify: `crates/ast-core/src/fitschan/fset_to_store/make_int_world.rs:53`
- Modify: `crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs:341`

- [ ] **Step 1: Replace the private copy in `make_int_world.rs`**

Delete the local `fn primary_frame` (lines ~52-56) and update its call sites to use the trait method directly. The local fn currently wraps `primary_frame_walk`; replace each `primary_frame(frame, axis)` call with `frame.primary_frame(axis)`.

Run: `grep -n "primary_frame" crates/ast-core/src/fitschan/fset_to_store/make_int_world.rs`
For each call, rewrite `primary_frame(<frame_expr>, <axis>)` → `<frame_expr>.primary_frame(<axis>)`.

- [ ] **Step 2: Replace the private copy in `celestial_axes.rs`**

Delete the local `fn primary_frame` (lines ~341-343) and rewrite its call sites the same way.

Run: `grep -n "primary_frame" crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs`

- [ ] **Step 3: Build**

Run: `cargo build -p ast-core`
Expected: clean (the `crate::frames::specframe::primary_frame_walk` import in those files may now be unused — remove it if clippy flags it).

- [ ] **Step 4: Run the fitschan + wcsconv tests**

Run: `cargo test -p ast-core --test test_fitschan_wcs && cargo test -p ast-core --test '*-regression'`
Expected: PASS — the trait method is behaviourally identical to the deleted copies. If anything differs, the copies were NOT identical; STOP and diff them against the trait method rather than papering over.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/fset_to_store/make_int_world.rs crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs
git commit -m "fitschan: use the PrimaryFrame trait method, drop local copies

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.3: Scalar geometry methods — Offset2, Intersect

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait methods + base impls)
- Test: `crates/ast-core/tests/test_frame_geometry.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/ast-core/tests/test_frame_geometry.rs`:

```rust
//! Frame geometry/axis method family — Cartesian base implementations
//! ported from frame.c (Offset2:8228, Intersect:6243, AxAngle:1324,
//! AxDistance:1466, AxOffset:1744, Resolve:9429, ResolvePoints:9588).

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrameBuilder, Frame};

fn bf(n: usize) -> impl Frame {
    BasicFrameBuilder::new(n).build().unwrap()
}

#[test]
fn offset2_cartesian() {
    let f = bf(2);
    // angle 0 => +axis2 direction; offset 5 => point moves +5 on axis 2.
    let (p2, ret) = f.offset2(&[1.0, 2.0], 0.0, 5.0).unwrap();
    assert!((p2[0] - 1.0).abs() < 1e-12);
    assert!((p2[1] - 7.0).abs() < 1e-12);
    assert!((ret - 0.0).abs() < 1e-12);
}

#[test]
fn offset2_bad_inputs() {
    let f = bf(2);
    let (p2, ret) = f.offset2(&[BAD_VALUE, 2.0], 0.0, 5.0).unwrap();
    assert_eq!(p2[0], BAD_VALUE);
    assert_eq!(p2[1], BAD_VALUE);
    assert_eq!(ret, BAD_VALUE);
}

#[test]
fn intersect_crossing_lines() {
    let f = bf(2);
    // Line a: y=x through (0,0)-(1,1). Line b: y=2-x through (0,2)-(1,1).
    let cross = f.intersect(&[0.0, 0.0], &[1.0, 1.0], &[0.0, 2.0], &[2.0, 0.0]).unwrap();
    assert!((cross[0] - 1.0).abs() < 1e-12);
    assert!((cross[1] - 1.0).abs() < 1e-12);
}

#[test]
fn intersect_parallel_is_bad() {
    let f = bf(2);
    let cross = f.intersect(&[0.0, 0.0], &[1.0, 1.0], &[0.0, 1.0], &[1.0, 2.0]).unwrap();
    assert_eq!(cross[0], BAD_VALUE);
    assert_eq!(cross[1], BAD_VALUE);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_frame_geometry`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Add trait methods + base impls**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame` (after `primary_frame`), add. These have full Cartesian base bodies (not stubs):

```rust
    /// Offset along a geodesic at a given angle in a 2-D Frame. Returns
    /// `(point2, ret_angle)`. Errors if the Frame is not 2-D. Bad inputs
    /// give all-BAD outputs. Mirrors C `astOffset2` (frame.c:8228).
    fn offset2(&self, point1: &[f64], angle: f64, offset: f64) -> AstResult<(Vec<f64>, f64)> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "astOffset2 can only be used with 2-dimensional Frames",
            ));
        }
        if is_bad(point1[0]) || is_bad(point1[1]) || is_bad(angle) || is_bad(offset) {
            return Ok((vec![BAD_VALUE, BAD_VALUE], BAD_VALUE));
        }
        let p2 = vec![
            point1[0] + angle.sin() * offset,
            point1[1] + angle.cos() * offset,
        ];
        Ok((p2, angle))
    }

    /// Point of intersection of two geodesics (straight lines) in a 2-D
    /// Frame. Bad inputs, vertical/parallel degeneracies give all-BAD.
    /// Mirrors C `astIntersect` (frame.c:6243).
    fn intersect(&self, a1: &[f64], a2: &[f64], b1: &[f64], b2: &[f64]) -> AstResult<Vec<f64>> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "astIntersect can only be used with 2-dimensional Frames",
            ));
        }
        let mut cross = vec![BAD_VALUE, BAD_VALUE];
        if [a1[0], a1[1], a2[0], a2[1], b1[0], b1[1], b2[0], b2[1]]
            .iter()
            .any(|&v| is_bad(v))
        {
            return Ok(cross);
        }
        let dxa = a2[0] - a1[0];
        let dxb = b2[0] - b1[0];
        if dxa != 0.0 {
            let ma = (a2[1] - a1[1]) / dxa;
            let ca = a1[1] - a1[0] * ma;
            if dxb != 0.0 {
                let mb = (b2[1] - b1[1]) / dxb;
                let cb = b1[1] - b1[0] * mb;
                if ma != mb {
                    cross[0] = (cb - ca) / (ma - mb);
                    cross[1] = ((ma + mb) * cross[0] + ca + cb) / 2.0;
                }
            } else if b1[1] != b2[1] {
                cross[0] = b1[0];
                cross[1] = ma * b1[0] + ca;
            }
        } else if dxb != 0.0 && a1[1] != a2[1] {
            let mb = (b2[1] - b1[1]) / dxb;
            let cb = b1[1] - b1[0] * mb;
            cross[0] = a1[0];
            cross[1] = mb * a1[0] + cb;
        }
        Ok(cross)
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_frame_geometry`
Expected: PASS (the four offset2/intersect cases).

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/tests/test_frame_geometry.rs
git commit -m "frames: port Offset2 and Intersect (Cartesian base)

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.4: Axis scalar methods — AxDistance, AxOffset, AxAngle, AxNorm

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Test: `crates/ast-core/tests/test_frame_geometry.rs`

- [ ] **Step 1: Write the failing tests**

Append to `test_frame_geometry.rs`:

```rust
#[test]
fn ax_distance_and_offset() {
    let f = bf(2);
    assert!((f.ax_distance(1, 3.0, 8.0).unwrap() - 5.0).abs() < 1e-12);
    assert_eq!(f.ax_distance(1, BAD_VALUE, 8.0).unwrap(), BAD_VALUE);
    assert!((f.ax_offset(1, 3.0, 5.0).unwrap() - 8.0).abs() < 1e-12);
    assert_eq!(f.ax_offset(1, BAD_VALUE, 5.0).unwrap(), BAD_VALUE);
}

#[test]
fn ax_angle_cartesian_axis1() {
    let f = bf(2);
    // From a=(0,0) toward b=(1,0): the line lies along +axis1.
    // C measures the angle from the +axis to line AB via astAngle of a
    // point nudged along the axis. For axis 1 and a horizontal AB the
    // result is +/- PI/2 (rotation sense axis2->axis1).
    let ang = f.ax_angle(&[0.0, 0.0], &[1.0, 0.0], 1).unwrap();
    assert!((ang.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
}

#[test]
fn ax_norm_is_noop_for_cartesian() {
    let f = bf(2);
    let mut vals = vec![1.0, 2.0, 3.0];
    f.ax_norm(1, 0, &mut vals).unwrap();
    assert_eq!(vals, vec![1.0, 2.0, 3.0]);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_frame_geometry`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Add trait methods + base impls**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`, add:

```rust
    /// Signed axis increment from `v1` to `v2` on a 1-based `axis`.
    /// Base (Cartesian): `v2 - v1` with BAD propagation. Mirrors C
    /// `astAxDistance` (frame.c:1466 -> axis.c AxisDistance).
    fn ax_distance(&self, _axis: usize, v1: f64, v2: f64) -> AstResult<f64> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        Ok(if is_bad(v1) || is_bad(v2) { BAD_VALUE } else { v2 - v1 })
    }

    /// Axis value formed by adding `dist` to `v1` on a 1-based `axis`.
    /// Base (Cartesian): `v1 + dist` with BAD propagation. Mirrors C
    /// `astAxOffset` (frame.c:1744 -> axis.c AxisOffset).
    fn ax_offset(&self, _axis: usize, v1: f64, dist: f64) -> AstResult<f64> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        Ok(if is_bad(v1) || is_bad(dist) { BAD_VALUE } else { v1 + dist })
    }

    /// Angle, as seen from `a`, between the positive direction of the
    /// 1-based `axis` and the geodesic from `a` to `b`. Base impl
    /// nudges `a` along the axis and calls `angle`. Mirrors C
    /// `astAxAngle` (frame.c:1324).
    fn ax_angle(&self, a: &[f64], b: &[f64], axis: usize) -> AstResult<f64> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let naxes = self.naxes();
        let mut aa = vec![0.0; naxes];
        let mut mxab = 0.0;
        for i in 0..naxes {
            if is_bad(a[i]) || is_bad(b[i]) {
                return Ok(BAD_VALUE);
            }
            aa[i] = a[i];
            let ab = (a[i] - b[i]).abs();
            if ab > mxab {
                mxab = ab;
            }
        }
        // Offset `aa` slightly in the +axis direction (frame.c:1438).
        let k = axis - 1;
        let eps = 10000.0 * f64::EPSILON;
        if a[k] != 0.0 {
            aa[k] += eps * a[k].abs();
        } else if b[k] != 0.0 {
            aa[k] = eps * b[k].abs();
        } else if mxab != 0.0 {
            aa[k] = eps * mxab;
        } else {
            aa[k] = 1.0;
        }
        self.angle(&aa, a, b)
    }

    /// Normalise an array of axis values on a 1-based `axis`. `oper`
    /// selects the normalisation mode (0 = like `norm`, 1 = smallest
    /// range). Base (Cartesian): values are returned unchanged. Mirrors
    /// C `astAxNorm` (frame.c:1560).
    fn ax_norm(&self, _axis: usize, oper: i32, _values: &mut [f64]) -> AstResult<()> {
        if oper < 0 || oper > 1 {
            return Err(AstError::new(
                ErrorCode::Oprin,
                format!("astAxNorm: invalid operation {oper}"),
            ));
        }
        Ok(())
    }
```

NOTE: confirm `ErrorCode::Oprin` exists (it is generated from `ast_err.msg`). If the variant name differs, run `grep -i "oprin\|OPRIN" crates/ast-core/src/error.rs` (or the generated error file) and use the actual name. If there is no operation-invalid code, use `ErrorCode::Aoprin`/closest match found by that grep — do not invent one.

- [ ] **Step 4: Run tests; oracle-check AxAngle sign**

Run: `cargo test -p ast-core --test test_frame_geometry`
Expected: PASS. The `ax_angle` sign depends on `cartesian_angle`'s 2-D signed convention (axis2->axis1). If the assertion's sign/magnitude is off, build a C probe linking libast (`astAxAngle`) for `a=(0,0)`, `b=(1,0)`, `axis=1` and match it exactly; adjust the test expectation to the C value (the assertion above only checks magnitude == PI/2, which is sign-agnostic, so it should hold either way).

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/tests/test_frame_geometry.rs
git commit -m "frames: port AxDistance, AxOffset, AxAngle, AxNorm (base)

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.5: Resolve + ResolvePoints

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Test: `crates/ast-core/tests/test_frame_geometry.rs`

- [ ] **Step 1: Write the failing tests**

Append to `test_frame_geometry.rs`:

```rust
use ast_core::coords::PointSet;

#[test]
fn resolve_parallel_perp() {
    let f = bf(2);
    // basis (0,0)->(2,0); point3 = (1,3). parallel d1=1, perp d2=3.
    let (p4, d1, d2) = f.resolve(&[0.0, 0.0], &[2.0, 0.0], &[1.0, 3.0]).unwrap();
    assert!((d1 - 1.0).abs() < 1e-12);
    assert!((d2 - 3.0).abs() < 1e-12);
    assert!((p4[0] - 1.0).abs() < 1e-12);
    assert!((p4[1] - 0.0).abs() < 1e-12);
}

#[test]
fn resolve_zero_basis_is_bad() {
    let f = bf(2);
    let (_p4, d1, d2) = f.resolve(&[0.0, 0.0], &[0.0, 0.0], &[1.0, 3.0]).unwrap();
    assert_eq!(d1, BAD_VALUE);
    assert_eq!(d2, BAD_VALUE);
}

#[test]
fn resolve_points_signed_2d() {
    let f = bf(2);
    // basis (0,0)->(1,0). Two test points above and below the basis.
    let inset = PointSet::from_rows([[1.0_f64, 2.0], [1.0, -2.0]]);
    let out = f.resolve_points(&[0.0, 0.0], &[1.0, 0.0], &inset).unwrap();
    // d1 (axis 0) = 1.0 for both; d2 (axis 1) signed: +2 then -2.
    assert!((out.point(0)[0] - 1.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 2.0).abs() < 1e-12);
    assert!((out.point(1)[1] + 2.0).abs() < 1e-12);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_frame_geometry resolve`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Add `resolve` (scalar)**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`:

```rust
    /// Resolve the vector point1->point3 into components parallel and
    /// perpendicular to the basis vector point1->point2. Returns
    /// `(point4, d1, d2)` where point4 is the closest approach, d1 the
    /// signed parallel length, d2 the (non-negative) perpendicular
    /// length. Zero-length basis or bad inputs give BAD. Mirrors C
    /// `astResolve` (frame.c:9429).
    fn resolve(&self, point1: &[f64], point2: &[f64], point3: &[f64])
        -> AstResult<(Vec<f64>, f64, f64)>
    {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let naxes = self.naxes();
        let mut point4 = vec![BAD_VALUE; naxes];
        let mut d1 = BAD_VALUE;
        let mut d2 = BAD_VALUE;
        let ok = (0..naxes).all(|i| {
            !(is_bad(point1[i]) || is_bad(point2[i]) || is_bad(point3[i]))
        });
        if ok {
            let mut dp = 0.0;
            let mut bv = 0.0;
            for i in 0..naxes {
                let c = point2[i] - point1[i];
                dp += c * (point3[i] - point1[i]);
                bv += c * c;
            }
            if bv > 0.0 {
                let bv = bv.sqrt();
                d1 = dp / bv;
                for i in 0..naxes {
                    point4[i] = point1[i] + (d1 / bv) * (point2[i] - point1[i]);
                }
                let mut s = 0.0;
                for i in 0..naxes {
                    let c = point3[i] - point4[i];
                    s += c * c;
                }
                d2 = s.sqrt();
            }
        }
        Ok((point4, d1, d2))
    }

    /// Batch form of `resolve`: resolve every vector point1->in[i] into
    /// `(parallel, perpendicular)` components, returned as a 2-coord
    /// PointSet. For 2-D Frames the perpendicular component is signed
    /// (positive when basis->vector rotation matches axis1->axis2). The
    /// 1-axis case returns `(ax_distance, 0)`. Mirrors C
    /// `astResolvePoints` (frame.c:9588). SkyFrame overrides with
    /// great-circle geometry.
    fn resolve_points(
        &self,
        point1: &[f64],
        point2: &[f64],
        input: &crate::coords::PointSet,
    ) -> AstResult<crate::coords::PointSet> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let nax = self.naxes();
        let npoint = input.npoint();
        let mut d1v = vec![BAD_VALUE; npoint];
        let mut d2v = vec![BAD_VALUE; npoint];

        if nax < 2 {
            for p in 0..npoint {
                let ip = input.point(p)[0];
                d1v[p] = self.ax_distance(1, point1[0], ip)?;
                d2v[p] = 0.0;
            }
            return crate::coords::PointSet::from_columns(&[&d1v, &d2v]);
        }

        // Basis vector + squared length.
        let mut basisv = vec![0.0; nax];
        let mut bv = 0.0;
        let mut ok = true;
        for i in 0..nax {
            if is_bad(point1[i]) || is_bad(point2[i]) {
                ok = false;
                break;
            }
            basisv[i] = point2[i] - point1[i];
            bv += basisv[i] * basisv[i];
        }
        if ok && bv > 0.0 {
            let bv = bv.sqrt();
            for p in 0..npoint {
                let row = input.point(p);
                let mut dp = 0.0;
                let mut good = true;
                for i in 0..nax {
                    let d = row[i] - point1[i];
                    if is_bad(d) {
                        good = false;
                        break;
                    }
                    dp += basisv[i] * d;
                }
                if !good {
                    continue;
                }
                let d1 = dp / bv;
                let c = d1 / bv;
                let d2 = if nax > 2 {
                    let mut s = 0.0;
                    for i in 0..nax {
                        let d = row[i] - (point1[i] + c * basisv[i]);
                        s += d * d;
                    }
                    s.sqrt()
                } else {
                    let x1 = c * basisv[0];
                    let y1 = c * basisv[1];
                    let x2 = row[0] - (point1[0] + x1);
                    let y2 = row[1] - (point1[1] + y1);
                    let mut m = (x2 * x2 + y2 * y2).sqrt();
                    if x1 * y2 - x2 * y1 < 0.0 {
                        m = -m;
                    }
                    m
                };
                d1v[p] = d1;
                d2v[p] = d2;
            }
        }
        crate::coords::PointSet::from_columns(&[&d1v, &d2v])
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_frame_geometry`
Expected: PASS (resolve + resolve_points). If the signed perpendicular sign is off, oracle-check with `astResolvePoints` against libast and match exactly.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/tests/test_frame_geometry.rs
git commit -m "frames: port Resolve and ResolvePoints (signed-2D base)

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.6: MatchAxes / MatchAxesX + IsUnitFrame

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Test: `crates/ast-core/tests/test_frame_geometry.rs`

- [ ] **Step 1: Write the failing test**

Append to `test_frame_geometry.rs`:

```rust
use ast_core::frames::{CmpFrame, SkyFrame, SkySystem, SpecFrame, SpecSystem};

#[test]
fn match_axes_finds_sky_in_cmp() {
    // frm1 = CmpFrame(SpecFrame, SkyFrame) (3 axes); frm2 = SkyFrame.
    // Each frm2 axis should map to the corresponding frm1 SkyFrame axis.
    let sf = SkyFrame::new(SkySystem::Fk5);
    let df = SpecFrame::new(SpecSystem::Freq);
    let cf = CmpFrame::new(Box::new(df), Box::new(sf.clone()));
    let axes = cf.match_axes(&sf).unwrap();
    // frm2 (SkyFrame) has 2 axes; both should resolve to 1-based indices
    // within cf (the SkyFrame occupies axes 2 and 3 -> indices 2,3).
    assert_eq!(axes.len(), 2);
    assert!(axes.iter().all(|&a| a >= 1));
}

#[test]
fn is_unit_frame_base_true() {
    let f = bf(2);
    assert!(f.is_unit_frame());
}
```

NOTE: `CmpFrame::new(DynFrame, DynFrame) -> Self`, `SkyFrame::new(SkySystem)`, `SpecFrame::new(SpecSystem)` (verified). The axis-occupancy assertion (`>= 1`) is intentionally loose; tighten to the exact `[2, 3]` only after oracle-confirming the order with `astMatchAxes` against libast.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_frame_geometry match_axes`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Implement MatchAxes/MatchAxesX + IsUnitFrame**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`:

```rust
    /// Find, for each axis of `frm2`, the corresponding 1-based axis
    /// index within `self` (or 0 if none). Mirrors C `astMatchAxes`
    /// (frame.c:7349), which delegates to `MatchAxesX` with args
    /// swapped so it can be overridden on either class.
    fn match_axes(&self, frm2: &dyn Frame) -> AstResult<Vec<i32>> {
        frm2.match_axes_x(self)
    }

    /// Implementation half of `match_axes` (args swapped). For each axis
    /// of `self`, identify its primary frame and try to match it against
    /// `frm1`, recording the 1-based `frm1` axis index (or 0). Mirrors C
    /// `astMatchAxesX` (frame.c:7438).
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        let nax2 = self.naxes();
        let mut axes = vec![0i32; nax2];

        // C temporarily widens frm1's Match-control attributes so it can
        // probe any axis count. We clone frm1 and set them on the clone
        // (frm1 is `&dyn`, so we snapshot then mutate the snapshot).
        let mut probe = frm1.snapshot();
        probe.set_preserve_axes(true);
        probe.set_max_axes(10000);
        probe.set_min_axes(1);

        for ifirst in 0..nax2 {
            let (pfrm, pax) = self.primary_frame(ifirst);
            if let Some(m) = probe.match_template(pfrm, true)? {
                // m.target_axes maps result axis -> pfrm axis; we need the
                // frm1 (template/probe) axis associated with pfrm axis pax.
                // C reads frm1_axes[pax] + 1.
                if let Some(pos) = m.template_axes.iter().position(|&t| t == pax as i32) {
                    axes[ifirst] = m.target_axes[pos] + 1;
                }
            }
        }
        Ok(axes)
    }

    /// Whether this Frame is equivalent to a UnitMap when treated as a
    /// Mapping. Base Frame is always unit-equivalent. Mirrors C
    /// `astIsUnitFrame` (frame.c:6417).
    fn is_unit_frame(&self) -> bool {
        true
    }
```

NOTE on the `frm1_axes[pax]` mapping: C's `astMatch(frm1, pfrm, ...)` returns `frm1_axes` = template (frm1) axes and `pfrm_axes` = target (pfrm) axes, then stores `frm1_axes[pax] + 1`. Here `probe.match_template(pfrm, ...)` treats `probe` as template and `pfrm` as target, so `m.template_axes` are probe(frm1) axes and `m.target_axes` are pfrm axes. We find the result slot whose pfrm axis == `pax`, then read the probe axis there. Verify this indexing against the C oracle (`astMatchAxes`) for the CmpFrame(Spec,Sky) case before tightening the test; if the mapping is transposed, swap which array is searched vs read.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_frame_geometry`
Expected: PASS (loose assertions). Oracle-confirm the exact `[2,3]` mapping, tighten the test, re-run.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/tests/test_frame_geometry.rs
git commit -m "frames: port MatchAxes/MatchAxesX and IsUnitFrame

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.7: NormBox base (no-op) + mark gaps-doc deferrals

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `docs/issues/frame-mapping-c-port-gaps.md`

- [ ] **Step 1: Add `norm_box` base method**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`:

```rust
    /// Extend a box to include the effect of coordinate-system
    /// singularities. The base Frame has none, so the bounds are left
    /// unchanged. SkyFrame overrides this (pole inclusion). Mirrors C
    /// `astNormBox` (frame.c:7766).
    fn norm_box(&self, _lbnd: &mut [f64], _ubnd: &mut [f64]) -> AstResult<()> {
        Ok(())
    }
```

(The C `reg` Mapping argument used by SkyFrame's override is deferred with the SkyFrame spherical overrides; the base signature omits it. When SkyFrame's NormBox lands, add the `reg` parameter then — record this in the gaps doc.)

- [ ] **Step 2: Record deferrals in the gaps doc**

In `docs/issues/frame-mapping-c-port-gaps.md`, under the section-6 method-family bullet, note: base `Offset2`/`AxAngle`/`AxDistance`/`AxOffset`/`AxNorm`/`Resolve`/`ResolvePoints`/`Intersect`/`NormBox`/`MatchAxes`/`IsUnitFrame` now ported (cite the new trait methods); still deferred: `NormalPoints`/`NormPoints` (PointSet-batch Norm; no core consumer), SkyFrame spherical overrides (great-circle Offset2/Resolve/Intersect/NormBox with the `reg` arg), and the capi shims for all of these.

- [ ] **Step 3: Build + full core test**

Run: `cargo build -p ast-core && cargo test -p ast-core`
Expected: PASS.

- [ ] **Step 4: fmt + clippy**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs docs/issues/frame-mapping-c-port-gaps.md
git commit -m "frames: add NormBox base; record method-family deferrals

Generated with AI

Co-Authored-By: SLAC AI"
```

---

## Commit 3 — Frame::cast + matchsub cast-retry

Implements section 4 of the spec. Depends on Commit 2 (uses `primary_frame` indirectly via overlay, and the geometry methods are unrelated but already landed).

### Task 3.1: Add a `set_domain` trait method

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/frames/basic.rs`

Rationale: the matchsub retry must set the cast result's Domain (C
`astSetDomain(super_this, dom)`), but `dyn Frame` has no domain setter.
Add a minimal trait setter (default no-op) and override it where a
concrete setter exists. `cast` only ever returns a `BasicFrame` or a
`SpecFrame`, so those two overrides are load-bearing; others are
harmless.

- [ ] **Step 1: Add the trait method**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`, near `domain`/`test_domain` (around line 133):

```rust
    /// Set the frame Domain. Default no-op; concrete classes with a
    /// Domain field override this. Mirrors C `astSetDomain`.
    fn set_domain(&mut self, _domain: &str) {}
```

- [ ] **Step 2: Override on BasicFrame**

In `crates/ast-core/src/frames/basic.rs`, in `impl Frame for BasicFrame`, add:

```rust
    fn set_domain(&mut self, domain: &str) {
        BasicFrame::set_domain(self, domain.to_string());
    }
```

- [ ] **Step 3: Override on SpecFrame**

In `crates/ast-core/src/frames/specframe.rs`, in `impl Frame for SpecFrame`, add (confirm `SpecFrame` has an inherent `set_domain`; if it's named differently, use that — `grep -n "fn set_domain" crates/ast-core/src/frames/specframe.rs`):

```rust
    fn set_domain(&mut self, domain: &str) {
        SpecFrame::set_domain(self, domain.to_string());
    }
```

- [ ] **Step 4: Build**

Run: `cargo build -p ast-core`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/src/frames/basic.rs crates/ast-core/src/frames/specframe.rs
git commit -m "frames: add a Frame::set_domain trait method

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 3.2: Add the `cast` trait method + BasicFrame/specialised impls

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait method, default None)
- Modify: `crates/ast-core/src/frames/skyframe.rs`, `specframe.rs`, `timeframe.rs`, `fluxframe.rs`, `dsbspecframe.rs`, `cmpframe.rs`
- Test: in-module unit tests where convenient, or `tests/test_convert.rs` later

- [ ] **Step 1: Add the trait method (default None)**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`:

```rust
    /// Cast `self` down into the class of `other`, returning a copy of
    /// `self`'s ancestral component that matches `other`'s class, or
    /// `None` if `self` is not a subclass of `other`'s class. Used by
    /// the matchsub retry so a specialised template can probe a plainer
    /// target. Mirrors C `astCast` (object.c / cmpframe.c:1245).
    /// Default: not castable.
    fn cast(&self, _other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        Ok(None)
    }
```

- [ ] **Step 2: Add a shared helper for "reduce to a BasicFrame"**

In `crates/ast-core/src/frames/mod.rs` (free function near `pick_axes`), add:

```rust
/// Build a plain `BasicFrame` carrying `frame`'s axis count and the
/// Frame-level + per-axis attributes that survive a cast to the Frame
/// base class. Used by the per-class `cast` impls when `other` is a
/// plain Frame. Mirrors the "create a basic Frame with the same axes
/// and attributes" path of C `Cast` (cmpframe.c:1326-1344, and the
/// Object-level CastCopy reduction for leaf frames).
pub(crate) fn reduce_to_basic_frame(frame: &dyn Frame) -> AstResult<BasicFrame> {
    let n = frame.naxes();
    let mut bf = BasicFrameBuilder::new(n).build()?;
    if frame.test_domain() {
        bf.set_domain(frame.domain().to_string());
    }
    // Per-axis Label/Unit/Symbol via the existing pick path: pick all
    // axes in order, which copies the metadata C transfers via the Axis
    // objects + Overlay.
    let target_axes: Vec<i32> = (0..n as i32).collect();
    let picked = frame.pick_axes_snapshot(&target_axes)?;
    // pick_axes_snapshot returns a frame of the same naxes carrying the
    // copied metadata; if it is already a BasicFrame, use it directly.
    if let Some(b) = picked.as_any().downcast_ref::<BasicFrame>() {
        let mut out = b.clone();
        if frame.test_domain() {
            out.set_domain(frame.domain().to_string());
        }
        return Ok(out);
    }
    Ok(bf)
}
```

NOTE: `reduce_to_basic_frame` leans on `pick_axes_snapshot`, which for
SkyFrame/SpecFrame already returns a `BasicFrame` with per-axis metadata
when the selection is not a whole-frame copy. For the whole-frame case a
SkyFrame's `pick_axes_snapshot` returns a SkyFrame clone (not a
BasicFrame), so the `downcast_ref` fails and we fall back to the bare
`bf` (Domain copied). That matches C closely enough for the matchsub
path (Domain is the only attribute the retry re-fixes); verify against
`testconvert.c` and, if axis metadata turns out to matter, pick axes
one-by-one with `-1`-free indices to force the BasicFrame branch.

- [ ] **Step 3: Implement `cast` on the leaf specialised frames**

For each of SkyFrame, SpecFrame, TimeFrame, FluxFrame, add to their `impl Frame for ...`:

```rust
    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // A specialised leaf frame can only be cast down to a plain
        // Frame (BasicFrame). Same-class is handled by the direct Match
        // (no cast needed), so only the BasicFrame target matters here.
        if other.as_any().downcast_ref::<crate::frames::BasicFrame>().is_some() {
            Ok(Some(Box::new(crate::frames::reduce_to_basic_frame(self)?)))
        } else {
            Ok(None)
        }
    }
```

For SpecFrame, additionally there is no sub-target below it except
DSBSpecFrame casting *up* to SpecFrame (handled in DSBSpecFrame). The
BasicFrame-target branch above is sufficient for SpecFrame itself.

- [ ] **Step 4: Implement `cast` on DSBSpecFrame**

In `crates/ast-core/src/frames/dsbspecframe.rs`, `impl Frame for DSBSpecFrame`:

```rust
    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // DSBSpecFrame -> SpecFrame (truncate to the parent), or
        // -> BasicFrame (plain Frame).
        if other.as_any().downcast_ref::<crate::frames::SpecFrame>().is_some() {
            Ok(Some(Box::new(self.spec().clone())))
        } else if other.as_any().downcast_ref::<crate::frames::BasicFrame>().is_some() {
            Ok(Some(Box::new(crate::frames::reduce_to_basic_frame(self)?)))
        } else {
            Ok(None)
        }
    }
```

NOTE: confirm `self.spec()` returns a `&SpecFrame` (the embedded parent); `grep -n "fn spec\b" crates/ast-core/src/frames/dsbspecframe.rs`. If the accessor differs, use the real one.

- [ ] **Step 5: Implement `cast` on CmpFrame**

In `crates/ast-core/src/frames/cmpframe.rs`, `impl Frame for CmpFrame` (mirrors cmpframe.c:1245):

```rust
    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // If `other` is a CmpFrame, a same-class copy is appropriate
        // (the direct Match path normally handles this; provide the
        // copy for completeness).
        if other.as_any().downcast_ref::<crate::frames::CmpFrame>().is_some() {
            return Ok(Some(Box::new(self.clone())));
        }
        // Otherwise build a plain BasicFrame with the same axes +
        // overlaid attributes, then cast THAT to `other`'s class
        // (cmpframe.c:1326-1347).
        let basic = crate::frames::reduce_to_basic_frame(self)?;
        basic.cast(other)
    }
```

- [ ] **Step 6: Build + targeted test**

Add an in-module unit test to `skyframe.rs`'s `tests` block:

```rust
    #[test]
    fn cast_skyframe_to_basic_frame() {
        let sf = SkyFrame::new(crate::frames::SkySystem::Fk5);
        let bf = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let casted = sf.cast(&bf).unwrap();
        assert!(casted.is_some());
        let casted = casted.unwrap();
        assert!(casted.as_any().downcast_ref::<crate::frames::BasicFrame>().is_some());
        assert_eq!(casted.naxes(), 2);
    }
```

Run: `cargo test -p ast-core --lib frames::skyframe::tests::cast_skyframe_to_basic_frame`
Expected: PASS.

- [ ] **Step 7: Export `reduce_to_basic_frame` visibility**

Confirm `reduce_to_basic_frame` is reachable from the sibling modules via `crate::frames::reduce_to_basic_frame`. It is `pub(crate)` in `mod.rs`, so add nothing further. Build to confirm.

Run: `cargo build -p ast-core`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/src/frames/skyframe.rs crates/ast-core/src/frames/specframe.rs crates/ast-core/src/frames/timeframe.rs crates/ast-core/src/frames/fluxframe.rs crates/ast-core/src/frames/dsbspecframe.rs crates/ast-core/src/frames/cmpframe.rs
git commit -m "frames: port astCast for matchsub down-casting

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 3.3: Wire the matchsub retry into matching

**Files:**
- Modify: `crates/ast-core/src/frames/find_frame.rs`

- [ ] **Step 1: Add the retry wrapper**

In `crates/ast-core/src/frames/find_frame.rs`, add a free function mirroring C `astMatch_` (frame.c:15473):

```rust
/// `astMatch` with the matchsub cast-retry (frame.c:15473). Tries the
/// direct per-class match first; on failure (when `match_sub` is set),
/// casts the template down into the target's class, re-fixes its
/// Domain, and retries via the base Match. This is what lets a
/// specialised template (SkyFrame, CmpFrame, ...) align with a plainer
/// target sharing its Domain.
pub(crate) fn match_with_subclass(
    template: &dyn Frame,
    target: &dyn Frame,
    match_sub: bool,
) -> AstResult<Option<MatchResult>> {
    if let Some(m) = template.match_template(target, match_sub)? {
        return Ok(Some(m));
    }
    if match_sub {
        if let Some(mut super_this) = template.cast(target)? {
            // Re-fix the Domain: the parent class may default it
            // differently (frame.c:15505-15509).
            if target.test_domain() {
                let dom = template.domain();
                if !dom.is_empty() {
                    super_this.set_domain(dom);
                }
            }
            return super_this.match_template(target, match_sub);
        }
    }
    Ok(None)
}
```

NOTE: a `BasicFrame`'s `match_template` routes to `default_match` (the base Match), so calling `super_this.match_template` after the cast is the correct base-Match invocation; this matches C, where the cast result's vtab is the parent class.

- [ ] **Step 2: Route `default_find_frame` through the wrapper**

In `default_find_frame`, replace the direct call:

```rust
    let Some(m) = template.match_template(&*target, false)? else {
        return Ok(None);
    };
```

with:

```rust
    let Some(m) = match_with_subclass(template, &*target, false)? else {
        return Ok(None);
    };
```

(`astFindFrame` calls `astMatch` with matchsub=0 in C — frame.c:3899 — so this preserves the existing FindFrame behaviour while making the wrapper the single match entry point. Confirm the matchsub flag: `grep -n "astMatch( template" reference/starlink-ast/src/frame.c` near FindFrame; if FindFrame uses matchsub=1, pass `true` here instead.)

- [ ] **Step 3: Build + run find_frame tests**

Run: `cargo build -p ast-core && cargo test -p ast-core --test test_find_frame`
Expected: PASS — existing behaviour preserved (matchsub=false means the retry is inert here).

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/find_frame.rs
git commit -m "frames: add the matchsub cast-retry match wrapper

Generated with AI

Co-Authored-By: SLAC AI"
```

---

## Commit 4 — ActiveUnit unit adaptation

Implements section 8 of the spec. Depends on Commit 2 (uses `primary_frame`/match machinery indirectly). **Read this whole commit before starting — it changes an attribute default and touches serialization, so the regression suite is the gate.**

### Background: the C semantics to match
- `astGetActiveUnit` (frame.c:4651): returns **0 if the Frame contains any SkyAxis**; otherwise the stored flag, **defaulting to 0** (false).
- `astSetActiveUnit` (frame.c:9882): stores 0/1; cannot be tested/cleared via the generic attribute API but IS dumped: `Dump` writes `ActUnt` only when `astTestActiveUnit` is true, with the actual value (frame.c:14614).
- SkyFrame forces 0; SpecFrame / FluxFrame force 1.

The current Rust `BasicFrame.act_unt: Option<bool>` defaults to **true** and only serializes `ActUnt 0` when explicitly false — both divergent from C. This commit fixes the default to match C **only if** the regression fixtures confirm it (some Rust fixtures may have been written against the current default).

### Task 4.1: Reconcile get/set/test ActiveUnit with C

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait methods)
- Modify: `crates/ast-core/src/frames/basic.rs`
- Modify: `crates/ast-core/src/frames/skyframe.rs`, `specframe.rs`, `fluxframe.rs` (forced values)

- [ ] **Step 1: Add trait methods**

In `crates/ast-core/src/frames/mod.rs`, inside `trait Frame`:

```rust
    /// Whether unit differences affect alignment in `Match`/`Overlay`.
    /// Mirrors C `astGetActiveUnit` (frame.c:4651): always false when the
    /// Frame has a SkyAxis; otherwise the stored flag, default false.
    fn get_active_unit(&self) -> bool {
        false
    }

    /// Set the ActiveUnit flag. Mirrors C `astSetActiveUnit`
    /// (frame.c:9882). SkyFrame ignores it (forced false); SpecFrame /
    /// FluxFrame force true.
    fn set_active_unit(&mut self, _value: bool) {}

    /// Whether ActiveUnit has been explicitly set (for serialization).
    fn test_active_unit(&self) -> bool {
        false
    }
```

- [ ] **Step 2: Implement on BasicFrame (default false)**

In `crates/ast-core/src/frames/basic.rs`, change `act_unt` handling. Add inherent accessors and trait overrides:

```rust
    fn get_active_unit(&self) -> bool {
        // C default is false; BasicFrame has no SkyAxis.
        self.act_unt.unwrap_or(false)
    }
    fn set_active_unit(&mut self, value: bool) {
        self.act_unt = Some(value);
    }
    fn test_active_unit(&self) -> bool {
        self.act_unt.is_some()
    }
```

- [ ] **Step 3: Force values on Sky/Spec/Flux frames**

In `skyframe.rs` `impl Frame for SkyFrame`:

```rust
    fn get_active_unit(&self) -> bool {
        false // SkyFrame always reports 0 (frame.c:4723).
    }
    fn set_active_unit(&mut self, _value: bool) {}
    fn test_active_unit(&self) -> bool {
        false
    }
```

In `specframe.rs` and `fluxframe.rs` `impl Frame`:

```rust
    fn get_active_unit(&self) -> bool {
        true // SpecFrame/FluxFrame always use active units.
    }
    fn set_active_unit(&mut self, _value: bool) {}
    fn test_active_unit(&self) -> bool {
        false
    }
```

- [ ] **Step 4: Fix BasicFrame serialization to match C**

In `BasicFrame::to_node`, replace the current `ActUnt` block (lines ~832-839) with the C rule — write only when set, with the actual value (frame.c:14614):

```rust
        // ActUnt: written only when explicitly set, with its value.
        if let Some(v) = self.act_unt {
            node.push_int(
                "ActUnt",
                if v { 1 } else { 0 },
                Some(if v {
                    "Unit strings affects alignment".to_string()
                } else {
                    "Unit strings do not affect alignment".to_string()
                }),
            );
        }
```

In `BasicFrame::from_node`, replace the read (lines ~903-907) — only store when the token is present (no defaulting to true):

```rust
        if node.get("ActUnt").is_some() {
            builder.act_unt = Some(node.get_int("ActUnt", 0) != 0);
        }
```

- [ ] **Step 5: Run the regression suite — this is the gate**

Run: `cargo test -p ast-core --test '*-regression' && cargo test -p ast-core`
Expected: PASS. If a native-dump regression now differs, inspect the fixture: C only emits `ActUnt` when set, so a frame that never set it should emit nothing. If a Rust fixture expected `ActUnt 0`/`ActUnt 1` by default, that fixture encoded the OLD Rust default — regenerate it from the C oracle (`astShow` of the same frame) rather than reverting the C-faithful behaviour. If the divergence is in SkyFrame's own dump (skyframe.rs has its own ActUnt block at ~1969), apply the same "only when set" rule there, but note SkyFrame forces 0 so it should generally not emit it.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/mod.rs crates/ast-core/src/frames/basic.rs crates/ast-core/src/frames/skyframe.rs crates/ast-core/src/frames/specframe.rs crates/ast-core/src/frames/fluxframe.rs
git commit -m "frames: reconcile ActiveUnit get/set/test and serialisation with C

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 4.2: ActiveUnit unit-mapping tail in SubFrame

**Files:**
- Modify: `crates/ast-core/src/frames/find_frame.rs` (`default_sub_frame`)
- Test: `crates/ast-core/tests/test_frame_geometry.rs` (or a new `test_active_unit.rs`)

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_active_unit.rs`:

```rust
//! ActiveUnit unit adaptation in SubFrame/Match (frame.c:11062-11108).

use ast_core::frames::{BasicFrameBuilder, Frame};

#[test]
fn active_unit_inserts_scale_for_km_to_m() {
    // Template in metres, target in km, both ActiveUnit -> the match
    // mapping should scale by 1000 (km -> m).
    let mut target = BasicFrameBuilder::new(1).unit(1, "km").build().unwrap();
    target.set_active_unit(true);
    let mut template = BasicFrameBuilder::new(1).unit(1, "m").build().unwrap();
    template.set_active_unit(true);

    let m = template.match_template(&target, false).unwrap().unwrap();
    // Apply forward: 2 km -> 2000 m.
    let pts = ast_core::coords::PointSet::from_rows([[2.0_f64]]);
    let out = m.map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2000.0).abs() < 1e-9);
}

#[test]
fn active_unit_off_ignores_units() {
    // ActiveUnit not set -> units ignored, mapping is identity.
    let target = BasicFrameBuilder::new(1).unit(1, "km").build().unwrap();
    let template = BasicFrameBuilder::new(1).unit(1, "m").build().unwrap();
    let m = template.match_template(&target, false).unwrap().unwrap();
    let pts = ast_core::coords::PointSet::from_rows([[2.0_f64]]);
    let out = m.map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
}
```

NOTE: confirm `BasicFrameBuilder::unit(axis, s)` exists (it does — basic.rs:122). Confirm the km→m scale direction with the C oracle (`astUnitMapper("km","m")`); if the test expects 2000 but C gives 0.002, flip the assertion to match C — do not flip the implementation.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_active_unit`
Expected: FAIL — `active_unit_inserts_scale_for_km_to_m` gives identity (no units tail yet).

- [ ] **Step 3: Add the units tail to `default_sub_frame`**

In `crates/ast-core/src/frames/find_frame.rs`, in `default_sub_frame`, after the `map` is built (the `if unit { UnitMap } else { PermMap }` block) and before `Ok(Some((map, result)))`, insert the ActiveUnit tail (frame.c:11062-11108):

```rust
    // ActiveUnit adaptation: when both template and result use active
    // units, compose a per-axis units Mapping so unit differences become
    // real conversions (frame.c:11062-11108).
    let mut map = map;
    if let Some(t) = template {
        if t.get_active_unit() && result.get_active_unit() {
            let result_naxes = target_axes.len();
            let mut umap: Option<DynMapping> = None;
            let mut all_unit = true;
            let mut matched = true;

            for result_axis in 0..result_naxes {
                // Corresponding target axis (outperm encoding: 1-based,
                // 0 = unassociated).
                let t_axis = target_axes[result_axis];
                let aumap = if t_axis >= 0 && (t_axis as usize) < target.naxes() {
                    let tu = unit_string(target, t_axis as usize + 1);
                    let ru = unit_string(result.as_ref(), result_axis + 1);
                    crate::units::unit_mapper(&tu, &ru)
                } else {
                    None
                };
                match aumap {
                    None => {
                        matched = false;
                        break;
                    }
                    Some(am) => {
                        if !is_unit_map(am.as_ref()) {
                            all_unit = false;
                        }
                        umap = Some(match umap {
                            None => am,
                            Some(prev) => Box::new(crate::mappings::CmpMap::new(
                                prev,
                                am,
                                crate::mappings::Composition::Parallel,
                            )?),
                        });
                    }
                }
            }

            if !matched {
                return Ok(None);
            }
            if !all_unit {
                if let Some(um) = umap {
                    let combined = crate::mappings::CmpMap::new(
                        map,
                        um,
                        crate::mappings::Composition::Series,
                    )?;
                    map = crate::mappings::Mapping::simplified(&combined)?;
                }
            }
        }
    }
```

Add two small helpers at the bottom of `find_frame.rs`:

```rust
/// Effective Unit string for a 1-based axis of a frame, via the public
/// `Frame` surface. Returns "" when the frame exposes no per-axis unit.
fn unit_string(frame: &dyn Frame, axis: usize) -> String {
    // BasicFrame/SkyFrame/etc. expose units through their concrete API;
    // route through the downcast helpers already used elsewhere. The
    // simplest portable access is the format/label path — but units are
    // not on the trait, so downcast to the known carriers.
    use crate::frames::BasicFrame;
    if let Some(b) = frame.as_any().downcast_ref::<BasicFrame>() {
        return b.unit(axis).unwrap_or("").to_string();
    }
    String::new()
}

/// Whether `m` is a UnitMap.
fn is_unit_map(m: &dyn crate::mappings::Mapping) -> bool {
    m.as_any().downcast_ref::<crate::mappings::UnitMap>().is_some()
}
```

NOTE on `unit_string`: the `Frame` trait has no `unit(axis)` accessor today (units live on concrete classes). For Commit 4 the only frames whose `default_sub_frame` runs the active-units tail are plain `BasicFrame`s (Sky forces ActiveUnit off; Spec/Flux have their own SubFrame paths). So the `BasicFrame` downcast covers the live case. If a later consumer needs CmpFrame units here, add a trait `unit(axis)` accessor then — record this limitation in the gaps doc. Confirm `Composition::Parallel`/`Series` variant names with `grep -n "enum Composition" crates/ast-core/src/mappings/*.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_active_unit`
Expected: PASS.

- [ ] **Step 5: Full core test + regression**

Run: `cargo test -p ast-core && cargo test -p ast-core --test '*-regression'`
Expected: PASS — confirm no existing match/subframe behaviour regressed (BasicFrames default ActiveUnit false, so the tail is inert unless a test opts in).

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/find_frame.rs crates/ast-core/tests/test_active_unit.rs
git commit -m "frames: insert unit conversions in SubFrame when ActiveUnit set

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 4.3: ActiveUnit transfer + NewUnit in Overlay

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs` (`overlay_template`)
- Test: `crates/ast-core/tests/test_active_unit.rs`

- [ ] **Step 1: Write the failing test**

Append to `test_active_unit.rs`:

```rust
#[test]
fn overlay_transfers_active_unit() {
    let mut result = BasicFrameBuilder::new(1).build().unwrap();
    let mut template = BasicFrameBuilder::new(1).build().unwrap();
    template.set_active_unit(true);
    result.overlay_template(&template, &[0]).unwrap();
    assert!(result.get_active_unit());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ast-core --test test_active_unit overlay_transfers_active_unit`
Expected: FAIL.

- [ ] **Step 3: Transfer the flag in BasicFrame::overlay_template**

In `crates/ast-core/src/frames/basic.rs`, in `overlay_template`, after the Domain copy, add (frame.c:8578):

```rust
        // Transfer the ActiveUnit flag (frame.c:8578).
        self.set_active_unit(template.get_active_unit());
```

The full `NewUnit` Label-rewrite + Format-clear (frame.c:7564) fires only
when the ActiveUnit flag is set AND a Unit value actually changes during
overlay. BasicFrame's `overlay_template` does not currently copy per-axis
Units (units flow via `pick_axes_snapshot`), so there is no Unit change
here to trigger NewUnit. Record in the gaps doc that the `NewUnit`
Label-rewrite is deferred until per-axis Unit overlay is wired through
the active-units path, and is not reachable on the current BasicFrame
overlay route. (Do NOT fake a Label rewrite that C would not perform.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_active_unit`
Expected: PASS.

- [ ] **Step 5: Record the NewUnit deferral**

In `docs/issues/frame-mapping-c-port-gaps.md`, under the ActiveUnit bullet, note: ActiveUnit get/set/test + the SubFrame units tail + the Overlay flag transfer are ported; the `NewUnit` Label-rewrite/Format-clear (frame.c:7564) is deferred until per-axis Unit overlay is wired (not reachable on the current overlay route).

- [ ] **Step 6: fmt + clippy + commit**

```bash
cargo fmt && cargo clippy --workspace -- -D warnings
git add crates/ast-core/src/frames/basic.rs crates/ast-core/tests/test_active_unit.rs docs/issues/frame-mapping-c-port-gaps.md
git commit -m "frames: transfer ActiveUnit flag in Overlay; record NewUnit deferral

Generated with AI

Co-Authored-By: SLAC AI"
```

---

## Commit 5 — ConvertX driver, retire per-class overrides, testconvert

Implements sections 1, 3 of the spec. Lands LAST — depends on the matchsub retry (Commit 3) and ActiveUnit (Commit 4) being in the Match/SubFrame path. This is the riskiest commit; the discipline below (rename → driver → retire one class at a time → acceptance test) keeps each step verifiable.

### Task 5.1: Rename `convert` → `convert_to` across the trait and call sites

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait method name)
- Modify: every `fn convert` impl and every `.convert(` call site

- [ ] **Step 1: Find every occurrence**

Run: `grep -rn "fn convert\b\|\.convert(" crates/ast-core/src crates/ast-capi/src`
Expected: the trait decl in `mod.rs:83`, the per-class impls (skyframe/specframe/cmpframe/timeframe/dsbspecframe/fluxframe/specfluxframe/basic/frameset), the region delegations, and the fitschan callers. Record the full list.

- [ ] **Step 2: Rename the trait method**

In `crates/ast-core/src/frames/mod.rs`, rename the trait method `fn convert` to `fn convert_to` (keep the same signature). Update its doc comment to describe `astConvert` semantics.

- [ ] **Step 3: Rename all impls and call sites**

Rename `fn convert(` → `fn convert_to(` in each frame impl, and `.convert(` → `.convert_to(` at each call site (regions/*, fitschan/*). Do this mechanically; the bodies are unchanged in this step.

- [ ] **Step 4: Build + full test**

Run: `cargo build -p ast-core && cargo test -p ast-core`
Expected: PASS — pure rename, no behaviour change.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "frames: rename Frame::convert to convert_to

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 5.2: Implement the generic `convert_frames` (astConvertX)

**Files:**
- Modify: `crates/ast-core/src/frames/find_frame.rs`

- [ ] **Step 1: Add `convert_frames`**

In `crates/ast-core/src/frames/find_frame.rs`, add (porting frame.c:3028-3258):

```rust
/// Port of C `astConvertX` (frame.c:3028): negotiate a common
/// coordinate system between `from` and `to`, returning a 2-frame
/// FrameSet (base = `from`, current = `to`) whose Mapping converts
/// between them, or `None` if no conversion is possible. `domainlist`
/// is a comma-separated priority list ("" / blank field permits any).
pub(crate) fn convert_frames(
    from: &dyn Frame,
    to: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<FrameSet>> {
    let mut result_map: Option<DynMapping> = None;

    // Loop over comma-separated domain fields until a match is found.
    let fields: Vec<&str> = domainlist.split(',').collect();
    let mut matched = false;
    for raw_field in fields {
        if matched {
            break;
        }
        // Clean the domain: uppercase, strip whitespace.
        let domain: String = raw_field
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();

        let mut best_score = -1;
        let mut perfect = false;

        for icom in 0..=1 {
            if perfect {
                break;
            }
            // common0 = copy of (icom ? from : to), PreserveAxes=0,
            // Domain set when the field is non-blank.
            let mut common0 = if icom == 1 { from.snapshot() } else { to.snapshot() };
            common0.set_preserve_axes(false);
            if !domain.is_empty() {
                common0.set_domain(&domain);
            }
            let frame1: &dyn Frame = if icom == 1 { to } else { from };
            let frame2: &dyn Frame = if icom == 1 { from } else { to };

            // First match: common0 (template) probes frame1.
            let Some(m1) = match_with_subclass(common0.as_ref(), frame1, true)? else {
                continue;
            };
            let map_a = m1.map; // frame1 -> common1
            let mut common1 = m1.result;
            common1.set_preserve_axes(false);

            // Second match: common1 (template) probes frame2.
            let Some(m2) = match_with_subclass(common1.as_ref(), frame2, true)? else {
                continue;
            };
            let map_b = m2.map; // frame2 -> common2

            // from_map / to_map per icom (the C map1_address/map2_address
            // bookkeeping): for icom==0, frame1=from so map_a=from_map and
            // frame2=to so map_b=to_map; for icom==1 it is reversed.
            let (from_map, mut to_map) = if icom == 1 {
                (map_b, map_a)
            } else {
                (map_a, map_b)
            };

            // Compose from_map then inverted to_map, simplify.
            to_map.invert();
            let composed = crate::mappings::CmpMap::new(
                from_map,
                to_map,
                crate::mappings::Composition::Series,
            )?;
            let map = crate::mappings::Mapping::simplified(&composed)?;

            // Score: 2*forward + inverse; keep the best.
            let score = (if map.has_forward() { 2 } else { 0 })
                + (if map.has_inverse() { 1 } else { 0 });
            if score > best_score {
                matched = true;
                best_score = score;
                perfect = best_score >= 3;
                result_map = Some(map);
            }
        }
    }

    if let Some(map) = result_map {
        let mut fs = FrameSet::new(from.snapshot());
        fs.add_frame(FrameRef::Base, map, to.snapshot())?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}
```

NOTE: confirm `Mapping::has_forward`/`has_inverse` are the right accessors (they are the trait methods used elsewhere) and `Mapping::invert` mutates in place (`grep -n "fn invert" crates/ast-core/src/mappings/mod.rs`). Confirm `DynMapping` and `Composition` are imported in `find_frame.rs`; add `use` lines if needed.

- [ ] **Step 2: Build**

Run: `cargo build -p ast-core`
Expected: clean (not yet wired into `convert_to`).

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/src/frames/find_frame.rs
git commit -m "frames: port astConvertX as the generic convert_frames driver

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 5.3: Make `convert_to` a single trait method; retire overrides one class at a time

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs` (trait default body)
- Modify: each frame class (remove the override)

Order: do BasicFrame first (lowest risk), then SkyFrame, SpecFrame, TimeFrame, DSBSpecFrame, FluxFrame, SpecFluxFrame, CmpFrame. FrameSet and Region keep their overrides (they legitimately delegate to the current/encapsulated frame — verify each still compiles and tests pass; do NOT remove those).

- [ ] **Step 1: Give the trait method a default body**

In `crates/ast-core/src/frames/mod.rs`, change `convert_to` from a required method to one with a default body:

```rust
    /// Find a mapping to convert coordinates from this frame to `to`.
    /// `domains` is a priority-ordered domain list. Returns a 2-frame
    /// FrameSet (base = self, current = `to`) or `None`. Mirrors C
    /// `astConvert` (frame.c:2965 -> ConvertX frame.c:3028).
    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let domainlist = if domains.is_empty() {
            " ".to_string()
        } else {
            domains.join(",")
        };
        find_frame::convert_frames(self, to, &domainlist)
    }
```

- [ ] **Step 2: Remove the BasicFrame override**

In `crates/ast-core/src/frames/basic.rs`, delete the `fn convert_to` override (the old UnitMap stub). Build + test:

Run: `cargo test -p ast-core --lib frames::basic`
Expected: PASS — the generic driver produces the same UnitMap for equal-axis BasicFrames (verify via the existing basic tests; if a test checked the old stub's None-on-mismatch behaviour, the generic driver should also return None when Match fails — confirm).

- [ ] **Step 3: Remove the SkyFrame override**

In `crates/ast-core/src/frames/skyframe.rs`, delete the `fn convert_to` override. Run the SkyFrame convert tests:

Run: `cargo test -p ast-core --lib frames::skyframe`
Expected: PASS. The generic driver routes through `skyframe_sub_frame`, which uses the template's AlignSystem (FK5 by default for two default SkyFrames, so `convert_fk5_to_galactic` etc. should still hold). If `convert_collapses_chain_within_sky_tol` or an offset-frame test now differs, the divergence is real: compare the generic-driver mapping against the C oracle (`astConvert` on the same two SkyFrames) and fix whichever side is wrong — but the expected value is C's, not the old override's.

- [ ] **Step 4: Remove the remaining overrides**

Repeat for `specframe.rs`, `timeframe.rs`, `dsbspecframe.rs`, `fluxframe.rs`, `specfluxframe.rs`, `cmpframe.rs` — delete the `fn convert_to` override in each, running that class's tests after each removal:

Run (after each): `cargo test -p ast-core --lib frames::<class>`
Expected: PASS per class. For any failure, oracle-check against `astConvert` and fix to C — never re-add the override to paper over a difference.

- [ ] **Step 5: Full core + regression + fitschan**

Run: `cargo test -p ast-core && cargo test -p ast-core --test '*-regression' && cargo test -p ast-core --test test_fitschan_wcs`
Expected: PASS. The fitschan/region `.convert_to()` consumers now flow through the generic driver; confirm WCS read/write round-trips are unchanged.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "frames: route all alignment through the generic convert_to driver

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 5.4: Port testconvert.c → test_convert.rs (acceptance gate)

**Files:**
- Create: `crates/ast-core/tests/test_convert.rs`
- Modify: `PLAN.md`

- [ ] **Step 1: Build the C oracle to confirm expected map classes**

Run: `cd reference/starlink-ast && ./configure >/dev/null 2>&1; make libast.a >/dev/null 2>&1; cd -` (or the project's documented libast build; if it fails, consult `crates/ast-ffi-tests` build setup). Then compile `ast_tester/testconvert.c` against it and run:

Run: `cc reference/starlink-ast/ast_tester/testconvert.c -Ireference/starlink-ast/src -Lreference/starlink-ast -last -lm -o /tmp/testconvert && /tmp/testconvert`
Expected: `All astConvert tests passed`. This confirms the reference behaviour (UnitMap vs PermMap per pairing) the Rust test will assert. If the build is involved, the `ast-ffi-tests` crate already links libast — reuse its discovered library path.

- [ ] **Step 2: Write the Rust acceptance test**

Create `crates/ast-core/tests/test_convert.rs`, porting `testconvert.c` using the public Rust API. Header comment cites the C ancestor. The test mirrors each `astConvert` call and asserts the base/current frame equality and the mapping class:

```rust
//! Acceptance test for astConvert/astFindFrame, ported from
//! reference/starlink-ast/ast_tester/testconvert.c. Exercises the
//! matchsub cast-retry (Frame[Domain=SKY] <-> SkyFrame => UnitMap),
//! CmpFrame partition alignment (=> PermMap), domain gating, and
//! astFindFrame MaxAxes/MinAxes widening.

use ast_core::frames::{
    BasicFrameBuilder, CmpFrame, Frame, FrameRef, SkyFrame, SkySystem, SpecFrame, SpecSystem,
};
use ast_core::mappings::{Mapping, PermMap, UnitMap};

fn is_unit(m: &dyn Mapping) -> bool {
    m.as_any().downcast_ref::<UnitMap>().is_some()
}
fn is_perm(m: &dyn Mapping) -> bool {
    m.as_any().downcast_ref::<PermMap>().is_some()
}

#[test]
fn convert_basic_sky_unitmap() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let bf = BasicFrameBuilder::new(2).domain("SKY").build().unwrap();

    // astConvert(bf, sf) => UnitMap, base==bf, current==sf.
    let fs = bf.convert_to(&sf, &[]).unwrap().expect("bf->sf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&bf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&sf));
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    assert!(is_unit(m.as_ref()));

    // astConvert(sf, bf) => UnitMap (the reverse).
    let fs = sf.convert_to(&bf, &[]).unwrap().expect("sf->bf converts");
    assert!(is_unit(
        fs.mapping(FrameRef::Base, FrameRef::Current).unwrap().as_ref()
    ));
}

#[test]
fn convert_domain_gate_blocks_mismatch() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let bf = BasicFrameBuilder::new(2).domain("NOTSKY").build().unwrap();
    assert!(bf.convert_to(&sf, &[]).unwrap().is_none());
    assert!(sf.convert_to(&bf, &[]).unwrap().is_none());
}

#[test]
fn convert_basic_cmp_permmap() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let df = SpecFrame::new(SpecSystem::Freq);
    let cf = CmpFrame::new(Box::new(df), Box::new(sf));
    let bf = BasicFrameBuilder::new(2).domain("SKY").build().unwrap();

    let fs = bf.convert_to(&cf, &[]).unwrap().expect("bf->cf converts");
    assert!(is_perm(
        fs.mapping(FrameRef::Base, FrameRef::Current).unwrap().as_ref()
    ));
}

#[test]
fn find_frame_cmp_needs_widened_axes() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let df = SpecFrame::new(SpecSystem::Freq);
    let cf = CmpFrame::new(Box::new(df), Box::new(sf.clone()));

    // astFindFrame(cf, sf) with default SkyFrame Min/MaxAxes => None.
    assert!(cf.find_frame_template(&sf, " ").unwrap().is_none());

    // Widen SkyFrame Min/MaxAxes => match found.
    let mut sf2 = SkyFrame::new(SkySystem::Fk5);
    sf2.set_max_axes(3);
    sf2.set_min_axes(1);
    assert!(cf.find_frame_template(&sf2, " ").unwrap().is_some());
}
```

NOTE: confirm `FrameSet::frame(FrameRef)` and `FrameSet::mapping(FrameRef, FrameRef)` signatures (frame:166, mapping:479 — both `AstResult`). Confirm `find_frame_template` is the right public entry for `astFindFrame` (it is). The `equal` assertions rely on the interim attribute-based `Frame::equal`; if `bf` (Domain=SKY, 2 axes) does not compare equal to itself after round-tripping through the FrameSet base, assert on `naxes()`/`domain()` instead and note it. Build the C oracle output (Step 1) as the source of truth for any case where the Rust assertion is uncertain.

- [ ] **Step 3: Run the acceptance test**

Run: `cargo test -p ast-core --test test_convert`
Expected: PASS (all four). Any failure that contradicts the C oracle from Step 1 is a real bug in the driver/cast/subframe path — fix it there, do not weaken the test below what C guarantees.

- [ ] **Step 4: Update PLAN.md tracker**

In `PLAN.md`, add a "C → Rust test migration" section (or append if it exists) with:

```markdown
| C test (`ast_tester/`) | Rust counterpart | Status |
|---|---|---|
| testconvert.c | crates/ast-core/tests/test_convert.rs | ported 2026-06-11 |
| testframe.c | (deferred) | NormUnit(1) via capi units layer; orthogonal to Frame-base port |
```

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt && cargo clippy --workspace -- -D warnings
git add crates/ast-core/tests/test_convert.rs PLAN.md
git commit -m "tests: port testconvert.c as the astConvert acceptance gate

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 5.5: Final sweep — gaps doc, quirks doc, full suite

**Files:**
- Modify: `docs/issues/frame-mapping-c-port-gaps.md`, `docs/issues/c-library-quirks.md`

- [ ] **Step 1: Mark item 7 fixed in the gaps doc**

In `docs/issues/frame-mapping-c-port-gaps.md`, update the section-0 item 7 entry and the section-6 bullets to FIXED with dates and the new symbol locations (convert_frames, Frame::cast, axis_format, the geometry methods, primary_frame trait method, ActiveUnit). Keep the explicit deferral list (NormalPoints/NormPoints, SkyFrame spherical overrides, Plot log-escapes, capi shims, NewUnit label-rewrite, testframe.c).

- [ ] **Step 2: Record the Format log-escape quirk**

In `docs/issues/c-library-quirks.md`, add an entry: the printf Format port renders the `&` log flag with C's non-Plot delimiter; the Plot graphical-escape rendering (`astTuneC("exdel")`) is deferred to the Plot delivery step. This is a deliberate, tracked divergence in rendering only (numeric result matches C).

- [ ] **Step 3: Full workspace gate**

Run: `cargo build && cargo test -p ast-core && cargo test -p ast-core --test '*-regression' && cargo fmt --check && cargo clippy --workspace -- -D warnings`
Expected: all PASS / clean.

- [ ] **Step 4: Optional tier-3/4 if submodule present**

Run: `cargo test -p ast-ffi-tests 2>/dev/null || echo "submodule absent; skipped"`
Expected: PASS or skipped notice. (PR-time may skip; nightly runs them.)

- [ ] **Step 5: Commit**

```bash
git add docs/issues/frame-mapping-c-port-gaps.md docs/issues/c-library-quirks.md
git commit -m "docs/issues: mark the Frame base port complete; record deferrals

Generated with AI

Co-Authored-By: SLAC AI"
```

---

## Self-review checklist (run before declaring the plan done)

1. **Spec coverage:** every spec section maps to a commit — §3 ConvertX→Commit 5; §4 cast/matchsub→Commit 3; §5 Format/Digits/Unformat→Commit 1; §6 geometry family→Commit 2; §7 PrimaryFrame→Commit 2; §8 ActiveUnit→Commit 4; testconvert.c→Task 5.4.
2. **C-conflict rule:** wherever a Rust fixture/test conflicts with C (notably the ActiveUnit default and the SkyFrame FK5-vs-AlignSystem convert behaviour), the plan instructs regenerating the expectation from the C oracle and fixing toward C — never reverting the C-faithful code or weakening a test below what C guarantees.
3. **No silent omission:** all deferrals (NormalPoints/NormPoints, SkyFrame spherical overrides, Plot log-escapes, NewUnit label-rewrite, capi shims, testframe.c) are written into the gaps/quirks docs by explicit steps.
4. **Verification:** load-bearing numeric/structural behaviour is checked against a built `libast` oracle (Tasks 1.2, 2.4, 2.5, 2.6, 5.4), not guessed.
