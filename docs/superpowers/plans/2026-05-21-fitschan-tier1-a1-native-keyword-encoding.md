# FitsChan Tier-1 A1 — keyword-card NATIVE encoding decoder

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the keyword-card variant of NATIVE encoding so `polco_native`, `polco2_native`, `specflux_native`, and `timeplot_native` wcsconv tests pass.

**Architecture:** Add a new `crates/ast-core/src/fitschan/enc_native.rs` that — when a FitsChan in NATIVE encoding contains keyword cards of the form `BEGAST_xx`, `ENDAST_xx`, `ISA_xx`, `<NAME>_xx` (where `xx` is two characters from `_ABCDEFGHIJKLMNOPQRSTUVWXYZ`) — translates those cards back into the same line-oriented text format that `serial::native_read::parse_native` already understands (`Begin Class`, `Key = value`, `IsA Class`, `End Class`). The existing parser then takes over with no further FITS knowledge. This mirrors the C library's structure where `fitschan.c::Read` (line 25699) routes NATIVE-encoded reads through the inherited Channel reader using a keyword-card source function.

**Tech Stack:** Rust 2024, existing FitsChan infrastructure (`CardList`, `FitsCard`, `FitsValue`), existing `serial::native_read::parse_native`. No new dependencies.

---

## Background — encoding format

The C library defines (`fitschan.c::CreateKeyword`, `fitschan.c:8254`, and `fitschan.c::FindAst`, `fitschan.c:17865`) a 2-character suffix alphabet:

```c
#define SEQ_CHARS "_ABCDEFGHIJKLMNOPQRSTUVWXYZ"   // 27 characters
```

Every NATIVE-encoded keyword has its base name followed by exactly two suffix characters drawn from `SEQ_CHARS` (giving 27 × 27 = 729 sequence values per base name). Example sample (`polco.head`):

```
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
CURRNT_A=                    2 / Index of current Frame
BEGAST_B= 'Frame   '           / Coordinate system description
DOMAIN_A= 'GRID    '           / Coordinate system domain
ISA_A   = 'Frame   '           / Coordinate system description (IsA marker)
BEGAST_C= 'Axis    '           / Coordinate axis
SYMBOL_A= 'g1      '           / Axis symbol
ENDAST_A= 'Axis    '           / End of object definition
...
```

Card → native text mapping:

| Keyword pattern              | Value type | Native text emission                                              |
|------------------------------|------------|-------------------------------------------------------------------|
| `BEGAST<xx>` = `'<Class>'`   | string     | `Begin <Class>`                                                   |
| `ISA<xx>` = `'<Class>'`      | string     | `IsA <Class>`                                                     |
| `ENDAST<xx>` = `'<Class>'`   | string     | `End <Class>`                                                     |
| `<NAME>_<xx>` = `'<value>'`  | non-empty string | `<name> = "<value>"`  (lowercased name)                     |
| `<NAME>_<xx>` = `'        '` | empty string | `<name> =`  (introduces nested object — next BEGAST is a child) |
| `<NAME>_<xx>` = `<int>`      | int        | `<name> = <int>`                                                  |
| `<NAME>_<xx>` = `<float>`    | float      | `<name> = <float>`                                                |

Long strings ending in `&` continue on subsequent `CONTINUE` cards. This pattern already exists in the test fixtures (e.g. `polco.head` line 12: `TITLE_A = 'Data grid indices; first pixel at (1&'/...` followed by `CONTINUE  '",1)     "'`).

The `parse_native` Rust parser (`crates/ast-core/src/serial/native_read.rs`) already accepts:
- `Begin <Class>` / `End <Class>`
- `<key> = <value>` scalar attributes
- `<key> =` (empty value) as a marker that the next non-blank line is `Begin <ChildClass>` of a nested object
- `IsA <Class>` inheritance markers
- `# ... # comment` lines (skipped) and blank lines (skipped)

So our job in `enc_native.rs` is purely a card-to-text projection — no parsing logic.

---

## File structure

- **Create:** `crates/ast-core/src/fitschan/enc_native.rs` — keyword-card NATIVE decoder.
- **Modify:** `crates/ast-core/src/fitschan/mod.rs` — register the module; route `Encoding::Native` reads through `enc_native::cards_to_native_text` when keyword-card NATIVE is detected, otherwise fall back to existing `extract_native_text`.
- **Modify:** `crates/ast-core/src/fitschan/guess_encoding.rs` — detect `BEGAST<xx>` keyword as a NATIVE-encoding signature in addition to the existing COMMENT-card markers.
- **Create:** `crates/ast-core/tests/test_fitschan_native_keyword.rs` — focused integration tests for keyword-card NATIVE input.
- (Test side-effect: `cargo test -p ast-core --test test_wcsconv` should report 4 more passes.)

---

## Task 1: Failing integration test for keyword-card NATIVE input

**Files:**
- Create: `crates/ast-core/tests/test_fitschan_native_keyword.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_fitschan_native_keyword.rs` with:

```rust
//! Tests for the keyword-card variant of NATIVE encoding (BEGAST/ENDAST cards).
//!
//! The C library accepts NATIVE-encoded headers in two storage forms:
//!   1. COMMENT-card variant: native text inside `COMMENT  AST +++  AST` markers
//!   2. Keyword-card variant: BEGAST_xx / ENDAST_xx / data-keyword cards
//! Variant 2 is what `polco.head`, `polco2.head`, `specflux.head`,
//! `timeplot.head` use. This test exercises that path end-to-end.

use std::path::PathBuf;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name)
}

fn read_fixture(name: &str) -> AnyObject {
    let text = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("read {name}: {e}"));
    let mut fc = FitsChan::from_header(&text)
        .unwrap_or_else(|e| panic!("from_header {name}: {e}"));
    fc.guess_encoding();
    fc.read()
        .unwrap_or_else(|e| panic!("read {name}: {e}"))
        .unwrap_or_else(|| panic!("read {name}: returned None"))
}

#[test]
fn polco_loads() {
    let _obj = read_fixture("polco.head");
    // Just verify it loads without error. Identity / value checks are out
    // of scope for the encoding-decoder test.
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_native_keyword polco_loads -- --nocapture
```

Expected: FAIL with a panic message like
`read polco.head: read error: expected 'Begin', got: AST            Beginning of AST data for FrameSet object`.

- [ ] **Step 3: Commit the failing test**

```bash
git add crates/ast-core/tests/test_fitschan_native_keyword.rs
git commit -m "test(fitschan): expected-failure test for keyword-card NATIVE input"
```

This commit is intentionally a red-test commit; it will turn green at the end of Task 9.

---

## Task 2: Module skeleton with SEQ_CHARS table and suffix predicate

**Files:**
- Create: `crates/ast-core/src/fitschan/enc_native.rs`
- Modify: `crates/ast-core/src/fitschan/mod.rs` — add `mod enc_native;`

- [ ] **Step 1: Write a failing unit test**

Append to `enc_native.rs` (creating the file):

```rust
//! Keyword-card NATIVE encoding decoder.
//!
//! Translates BEGAST_xx / ENDAST_xx / ISA_xx / <NAME>_xx FITS keyword cards
//! into the line-oriented text format consumed by
//! [`crate::serial::native_read::parse_native`].
//!
//! Mirrors C library `fitschan.c::FindAst` (line 17820) and
//! `fitschan.c::CreateKeyword` (line 8209).

/// Characters used in the 2-character sequence suffix on every NATIVE
/// keyword. Matches `SEQ_CHARS` in `fitschan.c` (line 1377).
pub(crate) const SEQ_CHARS: &str = "_ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Returns true if `c` is a valid sequence-suffix character.
pub(crate) fn is_seq_char(c: char) -> bool {
    SEQ_CHARS.contains(c)
}

/// Returns true if `s` is exactly two SEQ_CHARS characters.
pub(crate) fn is_two_char_seq_suffix(s: &str) -> bool {
    s.len() == 2 && s.chars().all(is_seq_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_chars_recognises_underscore_and_letters() {
        assert!(is_seq_char('_'));
        assert!(is_seq_char('A'));
        assert!(is_seq_char('Z'));
        assert!(!is_seq_char('a'));
        assert!(!is_seq_char('1'));
        assert!(!is_seq_char('0'));
    }

    #[test]
    fn two_char_seq_suffix_examples() {
        assert!(is_two_char_seq_suffix("_A"));
        assert!(is_two_char_seq_suffix("AB"));
        assert!(is_two_char_seq_suffix("__"));
        assert!(is_two_char_seq_suffix("ZZ"));
        assert!(!is_two_char_seq_suffix("A"));
        assert!(!is_two_char_seq_suffix("ABC"));
        assert!(!is_two_char_seq_suffix("a_"));
        assert!(!is_two_char_seq_suffix("0_"));
    }
}
```

In `crates/ast-core/src/fitschan/mod.rs`, add the module declaration alongside the existing encoding modules. Find the line near the top:

```rust
mod enc_aips;
mod enc_class;
mod enc_dss;
mod enc_iraf;
mod enc_pc;
```

Insert in alphabetical order:

```rust
mod enc_aips;
mod enc_class;
mod enc_dss;
mod enc_iraf;
mod enc_native;
mod enc_pc;
```

- [ ] **Step 2: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests -- --nocapture
```

Expected: 2 PASS.

- [ ] **Step 3: Verify clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

Both must pass. If fmt fails, `cargo fmt --all` then re-run check.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_native.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "feat(fitschan): enc_native module skeleton with SEQ_CHARS table"
```

---

## Task 3: Classify a NATIVE keyword

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs`

- [ ] **Step 1: Write the failing tests**

Append to `enc_native.rs`, above the existing `#[cfg(test)] mod tests`:

```rust
/// Classification of a keyword card under the NATIVE keyword-card encoding.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeywordKind {
    /// `BEGAST<xx>` — opens an object block; value is the class name.
    Begin,
    /// `ENDAST<xx>` — closes an object block; value is the class name.
    End,
    /// `ISA<xx>` — inheritance marker; value is the parent class name.
    IsA,
    /// `<NAME>_<xx>` — data attribute; the inner string is the lowercased
    /// base name (without the 2-char suffix) ready to emit as a native key.
    Data(String),
    /// Any other keyword (SIMPLE, NAXIS, COMMENT, CONTINUE, ...) — not part
    /// of NATIVE keyword-card encoding.
    Other,
}

/// Classify a FITS keyword as a NATIVE encoding marker, data card, or other.
pub(crate) fn classify(keyword: &str) -> KeywordKind {
    let len = keyword.len();
    if len < 3 {
        return KeywordKind::Other;
    }
    // The trailing 2 characters must both be SEQ_CHARS to be a NATIVE card.
    let (head, suffix) = keyword.split_at(len - 2);
    if !is_two_char_seq_suffix(suffix) {
        return KeywordKind::Other;
    }
    match head {
        "BEGAST" => KeywordKind::Begin,
        "ENDAST" => KeywordKind::End,
        "ISA" => KeywordKind::IsA,
        _ => {
            if head.is_empty() {
                KeywordKind::Other
            } else {
                KeywordKind::Data(head.to_ascii_lowercase())
            }
        }
    }
}
```

Append the following tests inside the existing `mod tests`:

```rust
    #[test]
    fn classify_recognises_begast() {
        assert_eq!(classify("BEGAST_A"), KeywordKind::Begin);
        assert_eq!(classify("BEGAST_Z"), KeywordKind::Begin);
        assert_eq!(classify("BEGASTAA"), KeywordKind::Begin);
    }

    #[test]
    fn classify_recognises_endast_and_isa() {
        assert_eq!(classify("ENDAST_A"), KeywordKind::End);
        assert_eq!(classify("ISA_A"), KeywordKind::IsA);
        assert_eq!(classify("ISAAB"), KeywordKind::IsA);
    }

    #[test]
    fn classify_recognises_data_card() {
        assert_eq!(classify("NFRAME_A"), KeywordKind::Data("nframe".to_string()));
        assert_eq!(classify("DOMAIN_B"), KeywordKind::Data("domain".to_string()));
        assert_eq!(classify("FRAMEA_A"), KeywordKind::Data("framea".to_string()));
    }

    #[test]
    fn classify_rejects_non_native_keywords() {
        assert_eq!(classify("SIMPLE"), KeywordKind::Other);
        assert_eq!(classify("NAXIS"), KeywordKind::Other);
        assert_eq!(classify("COMMENT"), KeywordKind::Other);
        assert_eq!(classify("CONTINUE"), KeywordKind::Other);
        assert_eq!(classify("CTYPE1"), KeywordKind::Other); // suffix is "E1", '1' not in SEQ_CHARS
        assert_eq!(classify(""), KeywordKind::Other);
        assert_eq!(classify("AB"), KeywordKind::Other);
    }
```

- [ ] **Step 2: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests -- --nocapture
```

Expected: 4 new PASSes (6 total).

- [ ] **Step 3: Verify clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_native.rs
git commit -m "feat(fitschan): classify NATIVE keyword cards (BEGAST/ENDAST/ISA/data)"
```

---

## Task 4: Detect keyword-card NATIVE encoding presence

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs`

- [ ] **Step 1: Write the failing test**

Append to `enc_native.rs` (above the `#[cfg(test)]`):

```rust
use crate::fitschan::card::FitsValue;
use crate::fitschan::card_list::CardList;

/// Returns true if the card list contains at least one `BEGAST<xx>` keyword,
/// indicating keyword-card NATIVE encoding.
///
/// Mirrors the C library check at `fitschan.c::GetEncoding` line 12566:
/// `astKeyFields( this, "BEGAST%2f", 0, NULL, NULL )`.
pub(crate) fn has_keyword_native(cards: &CardList) -> bool {
    cards
        .iter()
        .any(|card| matches!(classify(&card.keyword), KeywordKind::Begin))
}
```

Inside the `mod tests` block, add helpers and a test. First check what `CardList::iter` looks like; if it doesn't exist, use the public iteration method that does. The current `CardList` API in `card_list.rs` has methods like `find_next`, `position`, `set_position`. If iteration over all cards is not exposed, add a public-in-crate iterator method to `card_list.rs` first; otherwise use the existing API.

Run this command to confirm what iteration helper exists:

```bash
grep -nE "fn (iter|cards|all|each)" crates/ast-core/src/fitschan/card_list.rs
```

If `iter()` returning `impl Iterator<Item = &FitsCard>` exists, use it. Otherwise: add it as a `pub(crate) fn iter(&self) -> impl Iterator<Item = &FitsCard> + '_ { self.cards.iter() }` (look at the field name in `card_list.rs` — likely `cards` or `entries`) and re-run.

Now add the test in `mod tests`:

```rust
    use crate::fitschan::FitsChan;

    fn cards_from(text: &str) -> CardList {
        let fc = FitsChan::from_header(text).expect("from_header");
        fc.cards_for_test().clone()
    }

    #[test]
    fn detects_keyword_native_when_begast_present() {
        let header = "\
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
ENDAST_A= 'FrameSet'           / End of object definition
END
";
        let cards = cards_from(header);
        assert!(has_keyword_native(&cards));
    }

    #[test]
    fn rejects_pure_wcs_header() {
        let header = "\
SIMPLE  =                    T / file conforms to FITS standard
NAXIS   =                    2 / number of data axes
CTYPE1  = 'RA---TAN'           / coord type
CRPIX1  =                100.0 / reference pixel
CRVAL1  =                  0.0 / reference value
END
";
        let cards = cards_from(header);
        assert!(!has_keyword_native(&cards));
    }
```

The test uses a `cards_for_test()` accessor on `FitsChan`. If it doesn't exist, add a `#[cfg(test)] pub(crate) fn cards_for_test(&self) -> &CardList { &self.cards }` inside `impl FitsChan` in `mod.rs`. Look for an existing similar test accessor first (`grep -n "for_test\|cards()" crates/ast-core/src/fitschan/mod.rs`) and reuse if found.

- [ ] **Step 2: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests -- --nocapture
```

Expected: 2 new PASSes (8 total).

- [ ] **Step 3: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_native.rs crates/ast-core/src/fitschan/mod.rs crates/ast-core/src/fitschan/card_list.rs
git commit -m "feat(fitschan): detect keyword-card NATIVE encoding via BEGAST presence"
```

(`card_list.rs` and `mod.rs` are only added to the commit if you needed to add the iteration accessor or the test cards accessor in this task.)

---

## Task 5: Format a single attribute value as native text

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs`

The `parse_native` parser accepts these value formats on a `Key = Value` line:
- Quoted string: `"text"`  (with embedded `\"` for literal quote characters; native_read.rs splits on `=` then trims)
- Plain integer: `42`, `-5`
- Plain float: `1.5`, `-3.14e10`, `1.0E+5`

Empty string (`'        '` in FITS) is special: the C library treats it as "an Object data item follows" — we emit `key =` with no value.

- [ ] **Step 1: Write the failing tests**

Append to `enc_native.rs` (above `#[cfg(test)]`):

```rust
/// Result of formatting a card value for native-text emission.
pub(crate) enum FormattedValue {
    /// The card has a non-empty scalar value; emit `<key> = <text>`.
    Scalar(String),
    /// The card has an empty string value; emit `<key> =` and expect a
    /// child object to follow on subsequent cards (a `BEGAST<xx>` card).
    ObjectMarker,
}

/// Format a `FitsValue` for emission as native-text scalar.
///
/// Returns `None` if the value type is not representable in NATIVE encoding
/// (Comment, Logical, Undefined) — such cards should be ignored by the
/// keyword-card decoder.
pub(crate) fn format_value(value: &FitsValue) -> Option<FormattedValue> {
    match value {
        FitsValue::String(s) => {
            let trimmed = s.trim_end();
            if trimmed.is_empty() {
                Some(FormattedValue::ObjectMarker)
            } else {
                // Quote the string for native text. Embedded double-quotes
                // are escaped with a backslash; this matches the convention
                // accepted by `parse_native`.
                let escaped = trimmed.replace('\\', "\\\\").replace('"', "\\\"");
                Some(FormattedValue::Scalar(format!("\"{escaped}\"")))
            }
        }
        FitsValue::Int(n) => Some(FormattedValue::Scalar(n.to_string())),
        FitsValue::Float(f) => Some(FormattedValue::Scalar(format_native_float(*f))),
        // Logical, Comment, Undefined, Continue are not native scalars.
        _ => None,
    }
}

/// Format a float for NATIVE text emission. Mirrors the C library's choice
/// of `%.17g` (full double precision) so a NATIVE round-trip is value-exact.
fn format_native_float(x: f64) -> String {
    if x.is_nan() {
        // Native format does not have a NaN literal; the AST library uses
        // the BAD_VALUE sentinel for missing data and never serialises NaN.
        // Emit a placeholder that downstream parsing will reject loudly.
        return "<bad>".to_string();
    }
    let s = format!("{x:.17e}");
    // Compress to a more compact form when round-tripping is preserved.
    // Match parse_native's tolerance for both `1.5` and `1.5e0` styles.
    s
}
```

In `mod tests`, add:

```rust
    #[test]
    fn format_value_string_non_empty() {
        let v = FitsValue::String("FrameSet".to_string());
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => assert_eq!(s, "\"FrameSet\""),
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_string_empty_is_object_marker() {
        let v = FitsValue::String("        ".to_string());
        assert!(matches!(format_value(&v), Some(FormattedValue::ObjectMarker)));
    }

    #[test]
    fn format_value_int() {
        let v = FitsValue::Int(42);
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => assert_eq!(s, "42"),
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_float_basic() {
        let v = FitsValue::Float(1.5);
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => {
                // Just confirm it's parseable by Rust as 1.5.
                let parsed: f64 = s.parse().expect("parses");
                assert_eq!(parsed, 1.5);
            }
            other => panic!("expected Scalar, got {other:?}"),
        }
    }
```

If `FormattedValue` doesn't derive `Debug`, add `#[derive(Debug)]` so the panic message in the first test compiles.

- [ ] **Step 2: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests -- --nocapture
```

Expected: 4 new PASSes (12 total).

- [ ] **Step 3: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_native.rs
git commit -m "feat(fitschan): format card values as NATIVE-text scalars"
```

---

## Task 6: Walk cards into a native-text string

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs`

This is the main pass. Sequentially walk the card list. For each card:
- `Begin` keyword → emit ` Begin <Class>` line; class comes from the unquoted string value (trimmed).
- `End` keyword → emit ` End <Class>` line.
- `IsA` keyword → emit `    IsA <Class>` line.
- `Data` keyword + scalar value → emit `    <key> = <scalar>` line.
- `Data` keyword + object marker → emit `    <key> =` line.
- `Other` keyword (SIMPLE, NAXIS, COMMENT, …) → skip.

Indentation is cosmetic — `parse_native` ignores leading whitespace.

- [ ] **Step 1: Write the failing tests**

Append to `enc_native.rs`:

```rust
use crate::error::{AstError, AstResult, ErrorCode};

/// Translate a card list with keyword-card NATIVE encoding into the
/// line-oriented text format consumed by [`crate::serial::native_read::parse_native`].
///
/// The card list is walked in order. Cards that are not part of NATIVE
/// keyword-card encoding (SIMPLE, NAXIS, COMMENT, etc.) are silently
/// skipped.
///
/// Returns an error only if the cards encode an unsupported value type
/// that cannot be represented in native text (e.g. a Logical value where
/// a string was expected). Returns an empty string if the card list
/// contains no NATIVE keywords.
pub(crate) fn cards_to_native_text(cards: &CardList) -> AstResult<String> {
    let mut out = String::new();
    for card in cards.iter() {
        let kind = classify(&card.keyword);
        match kind {
            KeywordKind::Begin => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" Begin ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::End => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" End ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::IsA => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str("    IsA ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::Data(name) => match format_value(&card.value) {
                Some(FormattedValue::Scalar(text)) => {
                    out.push_str("    ");
                    out.push_str(&name);
                    out.push_str(" = ");
                    out.push_str(&text);
                    out.push('\n');
                }
                Some(FormattedValue::ObjectMarker) => {
                    out.push_str("    ");
                    out.push_str(&name);
                    out.push_str(" =\n");
                }
                None => {
                    return Err(AstError::new(
                        ErrorCode::Rderr,
                        format!(
                            "NATIVE data card '{}' has unsupported value type",
                            card.keyword
                        ),
                    ));
                }
            },
            KeywordKind::Other => continue,
        }
    }
    Ok(out)
}

fn expect_class_name(value: &FitsValue, keyword: &str) -> AstResult<String> {
    match value {
        FitsValue::String(s) => Ok(s.trim().to_string()),
        _ => Err(AstError::new(
            ErrorCode::Rderr,
            format!(
                "NATIVE keyword '{keyword}' must have string value, got {value:?}"
            ),
        )),
    }
}
```

In `mod tests`:

```rust
    #[test]
    fn cards_to_native_text_round_trips_a_minimal_frameset() {
        let header = "\
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
ENDAST_A= 'FrameSet'           / End of object definition
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(text.contains("Begin FrameSet"));
        assert!(text.contains("nframe = 2"));
        assert!(text.contains("End FrameSet"));
    }

    #[test]
    fn cards_to_native_text_emits_object_marker_for_empty_string() {
        let header = "\
BEGAST_A= 'FrameSet'
FRAMEA_A= '        '           / First component Frame
BEGAST_B= 'Frame   '
ENDAST_A= 'Frame   '
ENDAST_B= 'FrameSet'
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        // The empty-string FRAMEA_A must produce a child-object marker.
        assert!(text.contains("framea =\n"));
        // The next emitted line is `Begin Frame`.
        let pos_marker = text.find("framea =\n").expect("marker present");
        let after = &text[pos_marker + "framea =\n".len()..];
        assert!(
            after.trim_start().starts_with("Begin Frame"),
            "expected child Begin after object marker, got: {after}"
        );
    }
```

- [ ] **Step 2: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests -- --nocapture
```

Expected: 2 new PASSes (14 total).

- [ ] **Step 3: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_native.rs
git commit -m "feat(fitschan): translate keyword-card NATIVE to native text stream"
```

---

## Task 7: CONTINUE-card support for long string values

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs`

A long string value ending in `&` continues on subsequent `CONTINUE` cards. The native-text representation joins them into a single quoted string.

Example (from `polco.head`):

```
TITLE_A = 'Data grid indices; first pixel at (1&'/ Title of coordinate system
CONTINUE  '",1)     "'
```

Joined value: `Data grid indices; first pixel at (1",1)     `.

The C library logic is at `fitschan.c:18024-18069`. The convention: drop the trailing `&` from each continued segment and concatenate, then unquote.

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn cards_to_native_text_joins_continue_cards() {
        let header = "\
BEGAST_A= 'Frame   '
TITLE_A = 'first part &'        / first part
CONTINUE  '   second part'      / continued
ENDAST_A= 'Frame   '
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(
            text.contains("title = \"first part    second part\""),
            "expected joined string, got: {text}"
        );
    }
```

(The expected value `"first part    second part"` reflects the `&` getting dropped and the next segment's leading whitespace surviving the join — match the C library's exact behaviour: strip `&`, concatenate.)

- [ ] **Step 2: Run the test to confirm it fails**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests::cards_to_native_text_joins_continue_cards -- --nocapture
```

Expected: FAIL — current code treats CONTINUE as `KeywordKind::Other` and skips it, leaving `title = "first part &"` in the output.

- [ ] **Step 3: Refactor `cards_to_native_text` to peek ahead for CONTINUE cards**

Replace the body of `cards_to_native_text` with an indexed walk so we can look ahead. Replace the existing `for card in cards.iter()` loop with:

```rust
pub(crate) fn cards_to_native_text(cards: &CardList) -> AstResult<String> {
    let mut out = String::new();
    let card_vec: Vec<&FitsCard> = cards.iter().collect();
    let mut i = 0;
    while i < card_vec.len() {
        let card = card_vec[i];
        let kind = classify(&card.keyword);
        match kind {
            KeywordKind::Begin => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" Begin ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::End => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" End ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::IsA => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str("    IsA ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::Data(name) => {
                let (formatted, advance) =
                    format_value_with_continues(&card.value, &card_vec[i + 1..])?;
                match formatted {
                    Some(FormattedValue::Scalar(text)) => {
                        out.push_str("    ");
                        out.push_str(&name);
                        out.push_str(" = ");
                        out.push_str(&text);
                        out.push('\n');
                    }
                    Some(FormattedValue::ObjectMarker) => {
                        out.push_str("    ");
                        out.push_str(&name);
                        out.push_str(" =\n");
                    }
                    None => {
                        return Err(AstError::new(
                            ErrorCode::Rderr,
                            format!(
                                "NATIVE data card '{}' has unsupported value type",
                                card.keyword
                            ),
                        ));
                    }
                }
                i += advance;
            }
            KeywordKind::Other => {}
        }
        i += 1;
    }
    Ok(out)
}

/// Format a card value, consuming any trailing CONTINUE cards if the value
/// is a string ending in `&`. Returns the formatted value plus the number
/// of additional CONTINUE cards consumed.
fn format_value_with_continues(
    value: &FitsValue,
    rest: &[&FitsCard],
) -> AstResult<(Option<FormattedValue>, usize)> {
    let FitsValue::String(s) = value else {
        return Ok((format_value(value), 0));
    };
    let trimmed = s.trim_end();
    if trimmed.is_empty() {
        return Ok((Some(FormattedValue::ObjectMarker), 0));
    }
    if !trimmed.ends_with('&') {
        return Ok((format_value(value), 0));
    }
    // Strip the trailing &, then walk CONTINUE cards.
    let mut joined = trimmed.trim_end_matches('&').to_string();
    let mut consumed = 0usize;
    for next in rest {
        if next.keyword != "CONTINUE" {
            break;
        }
        consumed += 1;
        // CONTINUE cards parse with value = FitsValue::Continue(text).
        let seg = match &next.value {
            FitsValue::Continue(s) => s,
            FitsValue::String(s) => s,
            _ => {
                return Err(AstError::new(
                    ErrorCode::Rderr,
                    "CONTINUE card without string value",
                ));
            }
        };
        let seg_trimmed = seg.trim_end();
        let next_continues = seg_trimmed.ends_with('&');
        let seg_text = seg_trimmed.trim_end_matches('&');
        joined.push_str(seg_text);
        if !next_continues {
            break;
        }
    }
    let escaped = joined.replace('\\', "\\\\").replace('"', "\\\"");
    Ok((Some(FormattedValue::Scalar(format!("\"{escaped}\""))), consumed))
}
```

(The `let-else` for `FitsValue::String` requires that variant to be `String(String)`. If the actual variant uses a different inner type, adjust the destructure to match.)

- [ ] **Step 4: Run all enc_native tests**

```bash
cargo test -p ast-core --lib fitschan::enc_native::tests -- --nocapture
```

Expected: 15 PASS (including the new CONTINUE-handling test).

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_native.rs
git commit -m "feat(fitschan): join CONTINUE cards for long NATIVE string values"
```

---

## Task 8: Wire keyword-card NATIVE into the FitsChan read dispatch

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`

In the `Channel for FitsChan::read` impl, the `Encoding::Native` branch currently calls `extract_native_text()` (COMMENT-card only). Extend it to first check whether the card list contains keyword-card NATIVE; if so, route through `enc_native::cards_to_native_text`.

- [ ] **Step 1: Locate the read dispatch**

```bash
grep -n "Encoding::Native" crates/ast-core/src/fitschan/mod.rs
```

You should find a block in the `read()` implementation around line 508.

- [ ] **Step 2: Modify the Native branch**

Replace this block (current code, around lines 508-516):

```rust
            Encoding::Native => {
                let text = match self.extract_native_text() {
                    Some(t) => t,
                    None => return Ok(None),
                };
                let node = serial::native_read::parse_native(&text)?;
                let obj = serial::any_object_from_node(&node)?;
                Ok(Some(obj))
            }
```

with:

```rust
            Encoding::Native => {
                // Two storage variants: keyword-card (BEGAST_xx / ENDAST_xx)
                // and COMMENT-card (`COMMENT  AST +++  AST` markers).
                // The keyword-card variant takes precedence when present.
                let text = if enc_native::has_keyword_native(&self.cards) {
                    enc_native::cards_to_native_text(&self.cards)?
                } else {
                    match self.extract_native_text() {
                        Some(t) => t,
                        None => return Ok(None),
                    }
                };
                if text.trim().is_empty() {
                    return Ok(None);
                }
                let node = serial::native_read::parse_native(&text)?;
                let obj = serial::any_object_from_node(&node)?;
                Ok(Some(obj))
            }
```

If `enc_native` is not in scope at this `impl` block, add `use super::enc_native;` near the top of the file or qualify with the full path.

- [ ] **Step 3: Run the integration test from Task 1**

```bash
cargo test -p ast-core --test test_fitschan_native_keyword polco_loads -- --nocapture
```

Expected: PASS (the polco fixture now decodes successfully).

If it still fails: read the new error message — most likely a value-type mismatch in `expect_class_name` / `format_value`, or a missing case for an attribute type our parser cannot accept. Diagnose and adjust `enc_native.rs` (do not weaken the parser).

- [ ] **Step 4: Run the existing FitsChan tests to confirm no regression**

```bash
cargo test -p ast-core --test test_fitschan
cargo test -p ast-core --test test_fitschan_encodings
cargo test -p ast-core --test test_fitschan_wcs
cargo test -p ast-core --test test_fitschan_wcs_write
```

All must remain green.

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/mod.rs
git commit -m "feat(fitschan): route keyword-card NATIVE reads through enc_native"
```

---

## Task 9: Add tests for the remaining three fixtures

**Files:**
- Modify: `crates/ast-core/tests/test_fitschan_native_keyword.rs`

- [ ] **Step 1: Add tests for polco2, specflux, timeplot**

Append to `test_fitschan_native_keyword.rs`:

```rust
#[test]
fn polco2_loads() {
    let _obj = read_fixture("polco2.head");
}

#[test]
fn specflux_loads() {
    let _obj = read_fixture("specflux.head");
}

#[test]
fn timeplot_loads() {
    let _obj = read_fixture("timeplot.head");
}
```

- [ ] **Step 2: Run them**

```bash
cargo test -p ast-core --test test_fitschan_native_keyword -- --nocapture
```

Expected: 4 PASS.

If a fixture fails: read the error and decide if it's a missing CONTINUE edge case, an unsupported value type that the C library accepts, or a fixture-specific quirk. Add a focused unit test in `enc_native.rs` reproducing the issue, fix it, then this integration test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_fitschan_native_keyword.rs
git commit -m "test(fitschan): cover polco2, specflux, timeplot keyword-card NATIVE inputs"
```

---

## Task 10: Extend `guess_encoding` to detect BEGAST keyword

**Files:**
- Modify: `crates/ast-core/src/fitschan/guess_encoding.rs`

Currently `has_native_markers` only looks for COMMENT-card `AST +++` markers. The C library (`fitschan.c::GetEncoding` line 12566) also returns NATIVE when `astKeyFields( this, "BEGAST%2f", 0, NULL, NULL )` matches. The wcsconv test invokes `fc.guess_encoding()` for `.head` inputs without an explicit encoding, so this is the trigger that sets `Encoding::Native` before `read()` is called.

- [ ] **Step 1: Locate has_native_markers**

```bash
grep -n "has_native_markers\|fn guess_encoding" crates/ast-core/src/fitschan/guess_encoding.rs
```

- [ ] **Step 2: Add the BEGAST check**

Modify `has_native_markers` (around line 109). Find the function:

```rust
fn has_native_markers(cards: &CardList) -> bool {
    // existing: scan COMMENT cards for "AST +++"
    ...
}
```

Augment to also return true when `enc_native::has_keyword_native` does:

```rust
fn has_native_markers(cards: &CardList) -> bool {
    if super::enc_native::has_keyword_native(cards) {
        return true;
    }
    // existing COMMENT-card scan continues unchanged.
    ...
}
```

(Adjust `super::` path to match the actual module hierarchy. If `guess_encoding` is at `fitschan/guess_encoding.rs` and `enc_native` is at `fitschan/enc_native.rs`, then `super::enc_native::has_keyword_native` is the correct path from inside `guess_encoding.rs`.)

- [ ] **Step 3: Add a unit test**

In the existing `#[cfg(test)] mod tests` block in `guess_encoding.rs`:

```rust
    #[test]
    fn keyword_native_detected_as_native_encoding() {
        let header = "\
BEGAST_A= 'FrameSet'
NFRAME_A=                    2
ENDAST_A= 'FrameSet'
END
";
        let cards = card_list_from_header(header);
        assert_eq!(guess_encoding(&cards), Encoding::Native);
    }
```

(If `card_list_from_header` is not the existing helper name, look at how nearby tests construct a `CardList` and use that pattern.)

- [ ] **Step 4: Run the tests**

```bash
cargo test -p ast-core --lib fitschan::guess_encoding::tests -- --nocapture
```

Expected: existing tests still pass + 1 new PASS.

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/guess_encoding.rs
git commit -m "feat(fitschan): guess_encoding recognises BEGAST keyword cards as NATIVE"
```

---

## Task 11: Verify the wcsconv suite gain and full workspace green

**Files:** none (test-only verification)

- [ ] **Step 1: Run the wcsconv suite**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: `=== wcsconv results: 108 passed, 84 failed, 0 skipped (of 192 total) ===` (104 → 108, +4 from `polco_native`, `polco2_native`, `specflux_native`, `timeplot_native`).

If only some of the four pass, capture which still fail:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "polco|specflux|timeplot"
```

A still-failing case needs additional investigation in `enc_native.rs` (likely a value-formatting edge case not yet covered).

- [ ] **Step 2: Run full workspace tests for regressions**

```bash
cargo test --workspace 2>&1 | tail -30
```

All previously-passing tests must remain green. If anything regresses, read the failure and decide whether `enc_native` is being entered for inputs where it shouldn't be, or whether `guess_encoding` is misclassifying a borderline case.

- [ ] **Step 3: Clippy clean across the workspace**

```bash
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 4: Commit (verification commit, no code changes)**

If no further code changes were needed, no commit is necessary — Task 9's commit was the last one. Otherwise, commit any small fix-ups with a focused message.

---

## Self-review checklist (run before declaring done)

- [ ] **Spec coverage:** A1 in the roadmap (`docs/superpower/specs/2026-05-21-fitschan-wcsconv-roadmap.md`) calls for keyword-card NATIVE encoding via a new `enc_native.rs`. This plan delivers that across Tasks 2–8, with verification in Tasks 1, 9, 11.
- [ ] **No placeholders:** every step contains code or commands. No "TBD", no "handle edge cases", no unspecified types.
- [ ] **Type consistency:** `KeywordKind` (Task 3), `FormattedValue` (Task 5), `cards_to_native_text` (Tasks 6 & 7), `has_keyword_native` (Task 4) — names used in later tasks match introductions.
- [ ] **Frequent commits:** 9 commits (1 per task plus 1 red-test commit at the start).
- [ ] **Done criteria:** wcsconv suite at 108/192, full workspace green, clippy/fmt clean.

---

## What this plan does NOT cover

- **A2** (WCSAXES/NAXIS axis-count fallback for `car3_native`) — separate plan.
- **A3** (axes-swap CmpMap dimension mismatch) — separate plan.
- **B1** (CLASS reader NAXIS fallback for `jan_fits-class_*`) — separate plan.
- **Write side** (encoding an AST object as keyword-card NATIVE) — not needed for the wcsconv tests in this campaign; the roadmap's NATIVE write tests already use the COMMENT-card variant. A future plan can add the write side if a use case appears.
