# FitsChan Phase 1: Card Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement FITS card data model (parsing/formatting 80-character cards), card collection with iteration/search, FITS double formatter, and FitsChan struct with public card manipulation API and Channel trait.

**Architecture:** A new `fitschan` module with card.rs (FitsCard + FitsValue + parse/format), card_list.rs (CardList collection), format.rs (FITS double formatter), and mod.rs (FitsChan struct implementing Channel). Study the C implementation at `reference/starlink-ast/src/fitschan.c` for the exact FITS card parsing rules and double formatting algorithm.

**Tech Stack:** Rust 2024 edition. No new dependencies. Standard FITS format rules (80-char cards, keyword in cols 1-8, etc.).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/fitschan/mod.rs` | FitsChan struct, Channel impl, public API, Encoding enum |
| `crates/ast-core/src/fitschan/card.rs` | FitsCard, FitsValue enum, parse/format 80-char cards |
| `crates/ast-core/src/fitschan/card_list.rs` | CardList (Vec-based collection with position tracking) |
| `crates/ast-core/src/fitschan/format.rs` | FITS double formatter (nine-run/zero-run trimming) |
| `crates/ast-core/src/lib.rs` | Add `pub mod fitschan;` and re-exports |
| `crates/ast-core/tests/test_fitschan.rs` | FitsChan integration tests |

---

### Task 1: FitsValue and FitsCard (Parsing)

**Files:**
- Create: `crates/ast-core/src/fitschan/card.rs`

Implement the FITS card data model — parsing 80-character FITS
header cards into structured data. Study `fitschan.c` `CnvType`
function (line 7444) for type detection rules.

- [ ] **Step 1: Define FitsValue enum and FitsCard struct**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FitsValue {
    Int(i64),
    Float(f64),
    String(String),
    Logical(bool),
    ComplexFloat(f64, f64),
    ComplexInt(i64, i64),
    Comment,         // COMMENT/HISTORY/blank keyword
    Undef,           // keyword with no value
    Continue(String), // CONTINUE long string
}

#[derive(Debug, Clone)]
pub struct FitsCard {
    pub keyword: String,
    pub value: FitsValue,
    pub comment: Option<String>,
    pub(crate) used: bool,
}
```

- [ ] **Step 2: Implement FitsCard::parse**

Parse an 80-character FITS header card string following FITS
standard rules:

```rust
impl FitsCard {
    pub fn parse(card: &str) -> AstResult<Self> { ... }
}
```

Parsing rules (from FITS standard + C implementation):
- Pad/truncate to 80 chars
- Cols 1-8: keyword (trimmed, uppercased)
- If keyword is COMMENT, HISTORY, or blank: value is Comment,
  rest of card is the comment text
- If keyword is CONTINUE: value is Continue(string from cols 11-80)
- Cols 9-10: must be "= " for value cards
- Value area (cols 11-80): parse type:
  - Quoted string: starts with `'`, ends with `'`, right-padded
  - Logical: `T` or `F` (usually at col 30)
  - Integer: all digits (possibly negative)
  - Float: contains `.` or `E`/`e`
  - Complex: `(real, imag)` in parens
  - Undefined: blank value area
- Comment separator: first `/` after value (outside quotes)

- [ ] **Step 3: Implement FitsCard::format**

Format a FitsCard back to an 80-character string:

```rust
impl FitsCard {
    pub fn format(&self) -> String { ... }
}
```

Rules:
- Keyword: left-justified, padded to 8 chars
- Value indicator: "= " in cols 9-10 (except COMMENT/HISTORY)
- String values: single-quoted, padded to at least 8 chars
- Numeric values: right-justified in cols 11-30
- Logical: T or F at col 30
- Comment: ` / ` + comment text, space-padded to 80 chars

- [ ] **Step 4: Write unit tests**

Tests in `#[cfg(test)]` module:
- Parse integer card: `"NAXIS   =                    2 / Number of axes"`
- Parse float card: `"CRVAL1  =     266.416666666667 / Reference value"`
- Parse string card: `"CTYPE1  = 'RA---TAN'           / Axis type"`
- Parse logical card: `"SIMPLE  =                    T / FITS standard"`
- Parse comment card: `"COMMENT This is a comment"`
- Parse undefined card: `"BLANK   =                      / No value"`
- Format → parse round-trip for each type
- Edge cases: long strings, empty comments, negative numbers

- [ ] **Step 5: Verify and commit**

```bash
git commit -m "Add FitsCard and FitsValue: FITS header card parsing/formatting"
```

---

### Task 2: CardList (Collection)

**Files:**
- Create: `crates/ast-core/src/fitschan/card_list.rs`

Ordered collection of FITS cards with position tracking and
search capabilities.

- [ ] **Step 1: Implement CardList struct**

```rust
pub struct CardList {
    cards: Vec<FitsCard>,
    position: usize,
}

impl CardList {
    pub fn new() -> Self;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;

    // Insertion
    pub fn push(&mut self, card: FitsCard);
    pub fn put(&mut self, card: FitsCard, overwrite: bool);

    // Search
    pub fn find(&self, keyword: &str) -> Option<&FitsCard>;
    pub fn find_next(&mut self, keyword: &str) -> Option<&FitsCard>;
    pub fn get_int(&self, keyword: &str) -> Option<i64>;
    pub fn get_float(&self, keyword: &str) -> Option<f64>;
    pub fn get_string(&self, keyword: &str) -> Option<&str>;
    pub fn get_logical(&self, keyword: &str) -> Option<bool>;

    // Deletion
    pub fn delete(&mut self, keyword: &str) -> bool;
    pub fn delete_at_position(&mut self) -> bool;
    pub fn clear(&mut self);

    // Position/iteration
    pub fn rewind(&mut self);
    pub fn position(&self) -> usize;
    pub fn set_position(&mut self, pos: usize);
    pub fn at_eof(&self) -> bool;

    // Iteration
    pub fn iter(&self) -> impl Iterator<Item = &FitsCard>;
    pub fn cards(&self) -> &[FitsCard];
}
```

`put` with `overwrite=true`: replace existing card with same
keyword. With `overwrite=false`: insert at current position.

`find_next`: search forward from current position, advance
position past the found card. Supports wildcards (`%` for
single char, `*` for any).

- [ ] **Step 2: Implement from_header (bulk parse)**

```rust
impl CardList {
    pub fn from_header(text: &str) -> AstResult<Self> {
        // Split text into 80-char chunks or by newlines
        // Parse each as a FitsCard
        // Collect into CardList
    }
}
```

Accept both:
- Multi-line text (one card per line, may be < 80 chars)
- Concatenated 80-char blocks (no newlines)

- [ ] **Step 3: Write tests and commit**

```bash
git commit -m "Add CardList: FITS header card collection with search/iteration"
```

---

### Task 3: FITS Double Formatter

**Files:**
- Create: `crates/ast-core/src/fitschan/format.rs`

The specialized FITS double formatter with nine-run/zero-run
trimming. Study `RoundFString` in `fitschan.c` line 26266.

- [ ] **Step 1: Implement format_fits_double**

```rust
/// Format a double for a FITS card value field.
///
/// Applies FITS-specific rules:
/// - Uses specified number of significant digits
/// - Nine-run trimming (1.99999... → 2.0)
/// - Zero-run trimming
/// - Right-justified in field width
pub fn format_fits_double(value: f64, digits: usize) -> String;
```

Algorithm (from C `RoundFString`):
1. Format with `digits` significant figures using `%.*G` style
2. Detect nine-runs: if last N digits are all 9s and N exceeds
   guard digits, round up (increment preceding digit, replace 9s
   with 0s)
3. Detect zero-runs: trim trailing zeros that exceed guard digits
4. Handle special cases: exact zero, integer values, very large/small

- [ ] **Step 2: Write tests**

Test against known C output:
- `format_fits_double(3.14159265358979, 8)` → specific string
- `format_fits_double(1.9999999, 8)` → "2" (nine-run)
- `format_fits_double(1.0000001, 8)` → preserved (significant)
- `format_fits_double(0.0, 8)` → "0"
- `format_fits_double(1e-15, 8)` → scientific notation
- `format_fits_double(266.416666666667, 8)` → matches C

- [ ] **Step 3: Commit**

```bash
git commit -m "Add FITS double formatter with nine-run/zero-run trimming"
```

---

### Task 4: FitsChan Struct and Public API

**Files:**
- Create: `crates/ast-core/src/fitschan/mod.rs`
- Modify: `crates/ast-core/src/lib.rs`

The main FitsChan struct with public card manipulation API.

- [ ] **Step 1: Create mod.rs with FitsChan struct and Encoding enum**

```rust
pub mod card;
pub mod card_list;
pub mod format;

use crate::channel::{Channel, ChannelConfig};
use crate::error::AstResult;
use crate::serial::AnyObject;
use card_list::CardList;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Native,
    FitsWcs,
    FitsPc,
    FitsIraf,
    FitsAips,
    FitsAipsPP,
    FitsClass,
    Dss,
}

pub struct FitsChan {
    config: ChannelConfig,
    cards: CardList,
    encoding: Encoding,
    warnings: Vec<String>,
    fits_digits: usize,
    cd_matrix: bool,
    // ... other config attrs with defaults
}
```

- [ ] **Step 2: Implement constructors and public card API**

```rust
impl FitsChan {
    pub fn new() -> Self;
    pub fn from_header(text: &str) -> AstResult<Self>;

    pub fn put_fits(&mut self, card: &str, overwrite: bool) -> AstResult<()>;
    pub fn put_cards(&mut self, cards: &str) -> AstResult<()>;
    pub fn find_fits(&mut self, name: &str, inc: bool) -> Option<String>;
    pub fn del_fits(&mut self) -> AstResult<()>;
    pub fn empty_fits(&mut self);
    pub fn ncard(&self) -> usize;

    pub fn get_fits_i(&self, name: &str) -> Option<i64>;
    pub fn get_fits_f(&self, name: &str) -> Option<f64>;
    pub fn get_fits_s(&self, name: &str) -> Option<String>;
    pub fn get_fits_l(&self, name: &str) -> Option<bool>;
    pub fn set_fits_i(&mut self, name: &str, value: i64, comment: &str, overwrite: bool);
    pub fn set_fits_f(&mut self, name: &str, value: f64, comment: &str, overwrite: bool);
    pub fn set_fits_s(&mut self, name: &str, value: &str, comment: &str, overwrite: bool);
    pub fn set_fits_l(&mut self, name: &str, value: bool, comment: &str, overwrite: bool);

    pub fn set_encoding(&mut self, enc: Encoding);
    pub fn encoding(&self) -> Encoding;
    pub fn set_fits_digits(&mut self, digits: usize);
}
```

- [ ] **Step 3: Implement Channel trait (Native encoding only)**

```rust
impl Channel for FitsChan {
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        match self.encoding {
            Encoding::Native => {
                // Collect all cards into text, delegate to native parser
                let text = self.cards_as_native_text();
                // ... parse using native_read
            }
            _ => Err(AstError::new(ErrorCode::Nodef,
                "FITS-WCS read not yet implemented"))
        }
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<()> {
        match self.encoding {
            Encoding::Native => {
                // Render to native text, store as cards
                // ... render using native_write, store each line as a card
            }
            _ => Err(AstError::new(ErrorCode::Nodef,
                "FITS-WCS write not yet implemented"))
        }
    }

    fn warnings(&self) -> &[String] { &self.warnings }
    fn config(&self) -> &ChannelConfig { &self.config }
    fn config_mut(&mut self) -> &mut ChannelConfig { &mut self.config }
}
```

For Native encoding in FitsChan: each card is a text line of the
AST native format (Begin/End blocks stored as COMMENT cards with
AST markers). Study C's approach — Native in FitsChan wraps each
line as `COMMENT AST ...` cards.

- [ ] **Step 4: Add to lib.rs**

```rust
pub mod fitschan;
pub use fitschan::{FitsChan, Encoding};
```

- [ ] **Step 5: Commit**

```bash
git commit -m "Add FitsChan struct with public card API and Channel trait (Native only)"
```

---

### Task 5: Integration Tests

**Files:**
- Create: `crates/ast-core/tests/test_fitschan.rs`

- [ ] **Step 1: Write tests**

Tests covering:
- `FitsChan::from_header`: parse a real FITS header (use sip.head)
- `put_fits` / `find_fits` round-trip
- `get_fits_i/f/s/l` for each type
- `set_fits_*` then `get_fits_*` round-trip
- `del_fits` removes a card
- `empty_fits` clears all
- `ncard` returns correct count
- Card iteration via find_fits with wildcards
- FitsChan with Native encoding: write a ZoomMap, read back
- Parse real FITS headers: load sip.head fixture, verify keywords

Copy `reference/starlink-ast/ast_tester/sip.head` to
`crates/ast-core/tests/fixtures/fits/headers/sip.head`.

- [ ] **Step 2: Run and commit**

```bash
git commit -m "Add FitsChan integration tests"
```

---

### Task 6: Final Verification

- [ ] **Step 1: Clippy and fmt**
- [ ] **Step 2: Full test suite**
- [ ] **Step 3: Commit fixes**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. FitsCard parses all FITS value types (int, float, string, logical, comment)
5. FitsCard formats back to valid 80-char cards
6. CardList supports put/find/delete/iterate
7. FITS double formatter handles nine-run/zero-run
8. FitsChan public API (putFits, getFits, findFits, delFits) works
9. FitsChan with Native encoding reads/writes AST objects
10. Real FITS headers (sip.head) parse correctly
11. All existing tests still pass
