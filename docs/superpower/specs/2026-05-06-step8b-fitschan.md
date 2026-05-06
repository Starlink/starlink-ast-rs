# Step 8b: FitsChan — Design Spec

## Summary

Implement FitsChan — the Channel for reading/writing AST objects
from FITS headers. This is the largest and most complex class in
the AST library (45K lines of C), decomposed into 5 implementation
phases. FitsChan handles: FITS card storage and manipulation,
multiple WCS encoding formats (FITS-WCS, FITS-AIPS, DSS, etc.),
conversion between FITS keywords and AST FrameSets, and the
specialised FITS double formatter.

## Phased Delivery

| Phase | Scope | Dependencies |
|-------|-------|-------------|
| 1 | Card infrastructure + FitsChan struct + public API | Channel trait |
| 2 | FITS-WCS Read (decode FITS headers → FrameSet) | Phase 1, all Frame/Mapping types |
| 3 | FITS-WCS Write (encode FrameSet → FITS headers) | Phase 1+2 |
| 4 | Other encodings (IRAF, AIPS, CLASS, PC, DSS) | Phase 1 |
| 5 | WCS conversion test harness (196 test cases) | All phases |

Each phase produces working, testable software independently.

## Module Structure

```
crates/ast-core/src/fitschan/
├── mod.rs              # FitsChan struct, Channel impl, public API
├── card.rs             # FitsCard, FitsValue enum, card parsing
├── card_list.rs        # CardList (ordered collection with iteration)
├── format.rs           # FITS double formatter (nine-run/zero-run)
├── fits_store.rs       # FitsStore intermediate structure
├── wcs_read.rs         # FITS-WCS decoder pipeline
├── wcs_write.rs        # FITS-WCS encoder pipeline
├── wcs_celestial.rs    # Celestial axis decode/encode
├── wcs_spectral.rs     # Spectral axis decode/encode
├── wcs_matrix.rs       # PC/CD/CDELT matrix operations
├── wcs_distortion.rs   # SIP, distorted TAN, -TAB handling
├── enc_dss.rs          # DSS encoding
├── enc_iraf.rs         # FITS-IRAF encoding
├── enc_aips.rs         # FITS-AIPS / FITS-AIPS++ encoding
├── enc_class.rs        # FITS-CLASS encoding
├── enc_pc.rs           # FITS-PC encoding
```

## Phase 1: FITS Card Infrastructure

### FitsCard and FitsValue

```rust
/// A typed value from a FITS header card.
#[derive(Debug, Clone, PartialEq)]
pub enum FitsValue {
    Int(i64),
    Float(f64),
    String(String),
    Logical(bool),
    ComplexFloat(f64, f64),
    ComplexInt(i64, i64),
    Comment(String),
    Undef,
    Continue(String),
}

/// A single FITS header card (keyword + value + comment).
#[derive(Debug, Clone)]
pub struct FitsCard {
    /// Keyword name (max 8 chars, uppercase).
    pub keyword: String,
    /// Typed value.
    pub value: FitsValue,
    /// Comment string (after / separator).
    pub comment: Option<String>,
    /// Whether this card has been "used" by a read operation.
    pub(crate) used: bool,
}
```

### Card Parsing

Parse an 80-character FITS card string into a FitsCard:
```rust
impl FitsCard {
    pub fn parse(card: &str) -> AstResult<Self>;
    pub fn format(&self) -> String;  // → 80-char string
}
```

Parsing rules (FITS standard):
- Cols 1-8: keyword name (right-padded with spaces)
- Cols 9-10: `= ` for value cards (or blank for COMMENT/HISTORY)
- Cols 11-80: value + comment (/ separator)
- String values: single-quoted, right-padded
- Logical: `T` or `F` in column 30
- Numeric: right-justified free-format

### CardList

```rust
/// Ordered collection of FITS header cards with position tracking.
pub struct CardList {
    cards: Vec<FitsCard>,
    position: usize,  // current iteration position
}
```

Operations:
- `put(&mut self, card: FitsCard, overwrite: bool)`
- `find(&self, keyword: &str) -> Option<&FitsCard>`
- `find_next(&mut self, keyword: &str) -> Option<&FitsCard>`
- `delete(&mut self, keyword: &str) -> bool`
- `get_int(&self, keyword: &str) -> Option<i64>`
- `get_float(&self, keyword: &str) -> Option<f64>`
- `get_string(&self, keyword: &str) -> Option<&str>`
- `iter(&self) -> impl Iterator<Item = &FitsCard>`
- `len(&self) -> usize`
- `is_empty(&self) -> bool`

### FITS Double Formatter

The specialised formatter for FITS header values:

```rust
/// Format a double for a FITS card value field.
///
/// Applies FITS-specific rounding rules:
/// - Nine-run trimming (1.99999... → 2.0)
/// - Zero-run trimming
/// - Right-justified in available width
pub fn format_fits_double(value: f64, digits: usize) -> String;
```

### FitsChan Struct

```rust
/// Channel for reading/writing AST objects from FITS headers.
pub struct FitsChan {
    config: ChannelConfig,
    cards: CardList,
    encoding: Encoding,
    warnings: Vec<String>,
    // Configuration attributes
    fits_digits: usize,
    cd_matrix: bool,
    sip_ok: bool,
    poly_tan: bool,
    car_lin: bool,
    tab_ok: bool,
    force_tab: bool,
    def_b1950: bool,
    iwc: bool,
    clean: bool,
    sip_replace: bool,
    fits_axis_order: Option<String>,
    alt_axes: i32,
}

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
```

### FitsChan Public API

```rust
impl FitsChan {
    // Construction
    pub fn new() -> Self;
    pub fn from_header(text: &str) -> AstResult<Self>;
    
    // Card manipulation
    pub fn put_fits(&mut self, card: &str, overwrite: bool) -> AstResult<()>;
    pub fn put_cards(&mut self, cards: &str) -> AstResult<()>;
    pub fn find_fits(&mut self, name: &str, inc: bool) -> Option<String>;
    pub fn del_fits(&mut self) -> AstResult<()>;
    pub fn retain_fits(&mut self);
    pub fn empty_fits(&mut self);
    pub fn ncard(&self) -> usize;
    
    // Typed value access
    pub fn get_fits_i(&self, name: &str) -> Option<i64>;
    pub fn get_fits_f(&self, name: &str) -> Option<f64>;
    pub fn get_fits_s(&self, name: &str) -> Option<&str>;
    pub fn get_fits_l(&self, name: &str) -> Option<bool>;
    pub fn set_fits_i(&mut self, name: &str, value: i64, comment: &str, overwrite: bool);
    pub fn set_fits_f(&mut self, name: &str, value: f64, comment: &str, overwrite: bool);
    pub fn set_fits_s(&mut self, name: &str, value: &str, comment: &str, overwrite: bool);
    pub fn set_fits_l(&mut self, name: &str, value: bool, comment: &str, overwrite: bool);
    pub fn set_fits_u(&mut self, name: &str, comment: &str, overwrite: bool); // undefined
    pub fn set_fits_cm(&mut self, comment: &str, overwrite: bool); // comment card
    pub fn test_fits(&self, name: &str) -> (bool, bool); // (exists, has_value)
    
    // Configuration
    pub fn set_encoding(&mut self, enc: Encoding);
    pub fn encoding(&self) -> Encoding;
    pub fn set_fits_digits(&mut self, digits: usize);
}
```

### Channel Trait Implementation

For Phase 1, `read()` with NATIVE encoding delegates to the
existing native parser. `read()` with any FITS encoding returns
an error until Phase 2 implements the decoder.

`write()` with NATIVE encoding uses the existing native renderer
to write AST-formatted FITS cards. `write()` with FITS encodings
returns an error until Phase 3.

## Phase 2: FITS-WCS Read (Decoder)

### FitsStore Intermediate Structure

A structured holder for extracted FITS WCS keywords, indexed
by axis and alternate WCS version (A-Z):

```rust
pub(crate) struct FitsStore {
    pub naxis: usize,
    pub crpix: Vec<Option<f64>>,      // [axis]
    pub crval: Vec<Option<f64>>,      // [axis]
    pub cdelt: Vec<Option<f64>>,      // [axis]
    pub pc: Vec<Vec<Option<f64>>>,    // [row][col]
    pub cd: Vec<Vec<Option<f64>>>,    // [row][col]
    pub ctype: Vec<Option<String>>,   // [axis]
    pub cunit: Vec<Option<String>>,   // [axis]
    pub cname: Vec<Option<String>>,   // [axis]
    pub pv: Vec<Vec<Option<f64>>>,    // [axis][param]
    pub radesys: Option<String>,
    pub equinox: Option<f64>,
    pub lonpole: Option<f64>,
    pub latpole: Option<f64>,
    pub mjdobs: Option<f64>,
    pub specsys: Option<String>,
    pub restfrq: Option<f64>,
    pub restwav: Option<f64>,
    // ... SIP coefficients, -TAB data, etc.
}
```

### Decoder Pipeline

```
FITS cards → FitsStore → FrameSet
```

1. **Extract to FitsStore**: Scan cards for WCS keywords, populate
   the structured store. Handle alternate axes (CRPIX1A, CRVAL1A).
2. **Detect axis types**: From CTYPE, determine which axes are
   celestial (RA/DEC), spectral (FREQ/WAVE), time, or generic.
3. **Build celestial transform**: CRPIX → ShiftMap, PC/CD →
   MatrixMap, projection → WcsMap, CRVAL → rotation.
4. **Build spectral transform**: Linear or non-linear spectral
   coordinate conversion.
5. **Build other transforms**: Linear transforms for remaining axes.
6. **Compose FrameSet**: Pixel frame (base) → transformation →
   World frame (current). Multiple frames for alternate WCS.

### Key sub-modules

- `wcs_celestial.rs`: Handles RA/DEC axes, projection detection,
  LONPOLE/LATPOLE, SkyFrame construction.
- `wcs_spectral.rs`: Handles FREQ/WAVE/VELO axes, SPECSYS,
  SpecFrame construction.
- `wcs_matrix.rs`: PC/CD/CDELT matrix extraction and validation.
- `wcs_distortion.rs`: SIP polynomial, distorted TAN, -TAB lookup.

## Phase 3: FITS-WCS Write (Encoder)

### Encoder Pipeline

```
FrameSet → FitsStore → FITS cards
```

1. **Decompose FrameSet**: Extract base→current mapping, identify
   celestial/spectral/other axes from the current Frame.
2. **Extract linear part**: Factor out the PC/CD matrix from the
   total transformation. Validate invertibility.
3. **Populate FitsStore**: Set CRPIX, CRVAL, CDELT/CD, CTYPE,
   CUNIT for each axis. Add RADESYS, EQUINOX for celestial.
4. **Handle distortions**: If SIP polynomial detected, write
   AP/BP coefficient cards.
5. **Format cards**: Convert FitsStore to 80-character FITS cards
   using the FITS double formatter.

### Configuration effects

- `cd_matrix = true`: Write CD matrix instead of PC+CDELT
- `fits_digits`: Precision for double values
- `sip_ok`: Enable SIP distortion writing
- `fits_axis_order`: Control axis ordering in output

## Phase 4: Other Encodings

Each encoding is a separate small module implementing the same
FitsStore ↔ FITS cards conversion but with different keyword
conventions:

- **FITS-PC**: Uses PCi_j matrix (legacy format, pre-2002)
- **FITS-IRAF**: IRAF-specific keywords (DISPAXIS, etc.)
- **FITS-AIPS**: AIPS non-standard (VELREF, old CTYPE codes)
- **FITS-AIPS++**: Superset of FITS-AIPS
- **FITS-CLASS**: GILDAS/CLASS spectral survey format
- **DSS**: Digitized Sky Survey plate solutions

Each module provides:
```rust
pub(crate) fn read_from_cards(cards: &CardList) -> Option<FitsStore>;
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList);
```

## Phase 5: WCS Conversion Test Harness

### Data-driven tests from wcsconv_tests.txt

Port the 196-case `wcsconv_tests.txt` as a data-driven test
harness (same pattern as our simplify regression tests):

```
crates/ast-core/tests/
├── test_wcsconv.rs           # Data-driven harness
├── fixtures/fits/
│   ├── wcsconv_tests.txt     # Test manifest
│   ├── headers/              # Input .head files
│   │   ├── timj.head
│   │   ├── sip.head
│   │   └── ...
│   ├── expected/             # Expected output by encoding
│   │   ├── fits-wcs/
│   │   ├── fits-aips/
│   │   ├── native/
│   │   └── ...
│   └── ast/                  # Native AST format baselines
│       ├── timj.ast
│       └── ...
```

Test harness algorithm:
1. Read input (.head or .ast file)
2. Load into FitsChan with specified encoding
3. Read the FrameSet
4. Write to a new FitsChan with target encoding
5. Compare output against reference (string-exact or astEqual)

## FITS Double Formatter Detail

The `format_fits_double` function (Phase 1) implements the C
library's `RoundFString` algorithm:

1. Format with `FitsDigits` significant figures
2. **Nine-run detection**: If the formatted string contains a
   sequence of 9s after the significant digits (e.g., "1.9999..."),
   round up: "1.9999..." → "2.0000..."
3. **Zero-run detection**: If trailing zeros appear after a
   significant part, trim them to fit the FITS card width
4. Right-justify in the available field width (up to 70 chars)

This is NOT the same as our existing `format_double_g` (which
matches C's `%g`). The FITS formatter is specific to FITS card
formatting and has additional rounding rules.

## Testing Strategy

### Phase 1 tests:
- FitsCard::parse for each value type
- FitsCard::format produces valid 80-char cards
- CardList operations (put, find, delete, iterate)
- FITS double formatter matches C output for edge cases
- FitsChan: putFits/getFits/findFits round-trip
- FitsChan with Native encoding: read/write AST objects

### Phase 2 tests:
- Read simple 2D celestial header → FrameSet with SkyFrame
- Read spectral header → FrameSet with SpecFrame
- Read SIP distortion header → correct PolyMap
- Read multiple projections (TAN, SIN, AIT, CAR)
- Read alternate WCS (A suffix keywords)

### Phase 3 tests:
- Write SkyFrame FrameSet → valid FITS-WCS keywords
- Write with cd_matrix=true vs false
- Write spectral FrameSet → SPECSYS etc.
- Round-trip: read → write → read produces equal FrameSet

### Phase 4 tests:
- Each encoding: read sample header → correct FrameSet
- Per-encoding keyword validation

### Phase 5 tests:
- The full 196-case wcsconv_tests.txt regression suite

## What This Spec Does NOT Include

- FITS binary table reading/writing (FitsTable class)
- -TAB lookup table full implementation (deferred)
- Plotting keywords (Plot/Plot3D uses FitsChan attributes)
- The `astShow` / `astShowFits` display functions
