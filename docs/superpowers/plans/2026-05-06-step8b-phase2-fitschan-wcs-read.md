# FitsChan Phase 2: FITS-WCS Read (Decoder) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the FITS-WCS decoder that converts FITS header keywords into AST FrameSets — enabling FitsChan to read WCS information from FITS files.

**Architecture:** A pipeline that extracts WCS keywords from the CardList into a FitsStore intermediate structure, then builds a FrameSet by composing Mappings (WinMap for CRPIX, MatrixMap for PC/CD, WcsMap for projections) and Frames (SkyFrame for celestial, SpecFrame for spectral). Study the C `FsetFromStore()` → `WcsMapFrm()` pipeline at `reference/starlink-ast/src/fitschan.c` for the exact algorithm.

**Tech Stack:** Rust 2024 edition. Uses all existing Frame and Mapping types (WinMap, MatrixMap, WcsMap, SkyFrame, SpecFrame, CmpMap, FrameSet).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/starlink-ast/src/fitschan/fits_store.rs` | FitsStore intermediate structure |
| `crates/starlink-ast/src/fitschan/wcs_read.rs` | Main decoder: FitsStore → FrameSet pipeline |
| `crates/starlink-ast/src/fitschan/wcs_keywords.rs` | Extract keywords from CardList → FitsStore |
| `crates/starlink-ast/src/fitschan/wcs_celestial.rs` | Celestial axis detection + SkyFrame construction |
| `crates/starlink-ast/src/fitschan/wcs_spectral.rs` | Spectral axis detection + SpecFrame construction |
| `crates/starlink-ast/src/fitschan/wcs_matrix.rs` | PC/CD/CDELT matrix extraction |
| `crates/starlink-ast/src/fitschan/mod.rs` | Update FitsChan::read() for FitsWcs encoding |
| `crates/starlink-ast/tests/test_fitschan_wcs.rs` | WCS decoder integration tests |

---

### Task 1: FitsStore Intermediate Structure

**Files:**
- Create: `crates/starlink-ast/src/fitschan/fits_store.rs`

The FitsStore holds extracted WCS keywords in a structured form,
independent of encoding. Study the C code's usage pattern.

- [ ] **Step 1: Define FitsStore**

```rust
/// Intermediate storage for extracted FITS WCS keywords.
///
/// Acts as an encoding-independent bridge between FITS header cards
/// and AST FrameSet construction.
pub(crate) struct FitsStore {
    pub naxis: usize,
    pub crpix: Vec<Option<f64>>,       // [axis] reference pixel
    pub crval: Vec<Option<f64>>,       // [axis] reference value
    pub cdelt: Vec<Option<f64>>,       // [axis] pixel scale
    pub pc: Vec<Vec<Option<f64>>>,     // [i][j] rotation matrix
    pub cd: Vec<Vec<Option<f64>>>,     // [i][j] CD matrix (alt to PC+CDELT)
    pub ctype: Vec<Option<String>>,    // [axis] axis type code
    pub cunit: Vec<Option<String>>,    // [axis] axis units
    pub cname: Vec<Option<String>>,    // [axis] axis name
    pub pv: Vec<Vec<Option<f64>>>,     // [axis][param] projection params
    pub radesys: Option<String>,
    pub equinox: Option<f64>,
    pub lonpole: Option<f64>,
    pub latpole: Option<f64>,
    pub mjdobs: Option<f64>,
    pub specsys: Option<String>,
    pub restfrq: Option<f64>,
    pub restwav: Option<f64>,
    pub wcsaxes: Option<usize>,
    // SIP distortion (if CTYPE contains "-SIP")
    pub sip_a: Option<Vec<Vec<f64>>>,  // [i][j] forward A coefficients
    pub sip_b: Option<Vec<Vec<f64>>>,  // [i][j] forward B coefficients
    pub sip_ap: Option<Vec<Vec<f64>>>, // [i][j] inverse AP coefficients
    pub sip_bp: Option<Vec<Vec<f64>>>, // [i][j] inverse BP coefficients
}
```

Constructor: `FitsStore::new(naxis: usize) -> Self` — initializes
all Vecs to length naxis with None.

- [ ] **Step 2: Add helper methods**

```rust
impl FitsStore {
    pub fn has_cd_matrix(&self) -> bool;     // any CD element set?
    pub fn has_pc_matrix(&self) -> bool;     // any PC element set?
    pub fn has_sip(&self) -> bool;           // SIP distortion present?
    pub fn celestial_axes(&self) -> Option<(usize, usize)>; // (lon_idx, lat_idx)
    pub fn spectral_axis(&self) -> Option<usize>;
    pub fn projection_code(&self, axis: usize) -> Option<&str>; // last 3 chars of CTYPE after -
}
```

- [ ] **Step 3: Commit**

---

### Task 2: Keyword Extraction (CardList → FitsStore)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/wcs_keywords.rs`

Extract FITS WCS keywords from the CardList into a FitsStore.
Study the C `WcsFromStore()` function.

- [ ] **Step 1: Implement extract_wcs_keywords**

```rust
/// Extract FITS-WCS keywords from a CardList into a FitsStore.
pub(crate) fn extract_wcs_keywords(cards: &CardList) -> AstResult<FitsStore>
```

Extract in this order:
1. NAXIS (or WCSAXES if present)
2. CRPIX1..CRPIXn
3. CRVAL1..CRVALn
4. CDELT1..CDELTn
5. CD1_1..CDn_n OR PC1_1..PCn_n (mutually exclusive)
6. CTYPE1..CTYPEn
7. CUNIT1..CUNITn
8. CNAME1..CNAMEn
9. PV1_0..PVn_m (projection parameters)
10. RADESYS, EQUINOX, LONPOLE, LATPOLE
11. MJD-OBS, SPECSYS, RESTFRQ, RESTWAV
12. SIP keywords: A_ORDER, A_i_j, B_ORDER, B_i_j, AP_ORDER, AP_i_j, BP_ORDER, BP_i_j

Keyword names are constructed programmatically (e.g., `format!("CRPIX{}", axis+1)`).
Use `cards.get_float("CRPIX1")` etc.

- [ ] **Step 2: Handle CTYPE parsing for SIP suffix**

If CTYPE contains "-SIP" (e.g., "RA---TAN-SIP"), strip the "-SIP"
suffix and set a flag to extract SIP coefficients.

- [ ] **Step 3: Tests and commit**

Test with the sip.head fixture:
- Extract keywords, verify CRPIX/CRVAL/CD values match header
- Verify CTYPE parsed correctly
- Verify SIP coefficients extracted

---

### Task 3: WCS Matrix Operations

**Files:**
- Create: `crates/starlink-ast/src/fitschan/wcs_matrix.rs`

Build the pixel-to-intermediate-world transformation matrix from
PC/CD/CDELT keywords.

- [ ] **Step 1: Implement build_pixel_to_iwc_mapping**

```rust
/// Build the pixel → IWC (Intermediate World Coordinate) mapping.
///
/// This composes: CRPIXshift → PC*CDELT (or CD) → [SIP distortion]
pub(crate) fn build_pixel_to_iwc_mapping(
    store: &FitsStore,
) -> AstResult<DynMapping>
```

Algorithm:
1. **CRPIX shift**: `WinMap` with shifts = -CRPIX (moves reference
   pixel to origin). `shifts[i] = -(crpix[i] - 1.0)` (FITS pixels
   are 1-based, shift to 0-based).
2. **Rotation/scale matrix**:
   - If CD matrix present: `MatrixMap` from CD values directly
   - If PC + CDELT: `MatrixMap` from `PC[i][j] * CDELT[i]`
   - If only CDELT (no PC): diagonal `MatrixMap` from CDELT values
   - If nothing: identity
3. **SIP distortion** (if present): `PolyMap` from SIP A/B
   coefficients, composed in series after the linear part.
4. Compose all with CmpMap(Series).

- [ ] **Step 2: Tests**

- Minimal header (CRPIX + CDELT only) → WinMap + diagonal MatrixMap
- CD matrix header → WinMap + full MatrixMap
- Verify transform: pixel (128,128) → (0,0) in IWC for CRPIX=128

---

### Task 4: Celestial Axis Decoder

**Files:**
- Create: `crates/starlink-ast/src/fitschan/wcs_celestial.rs`

Detect celestial axes from CTYPE and build the IWC → sky mapping
(WcsMap projection + SkyFrame).

- [ ] **Step 1: Implement build_celestial**

```rust
/// If celestial axes are present, build the IWC→sky mapping and SkyFrame.
///
/// Returns (mapping, frame, lon_axis, lat_axis) or None if no celestial axes.
pub(crate) fn build_celestial(
    store: &FitsStore,
) -> Option<(DynMapping, Box<SkyFrame>, usize, usize)>
```

Algorithm (study C `WcsCelestial()` at line 35458):
1. Scan CTYPE for celestial patterns:
   - Longitude: starts with "RA" or ends with "LON" after removing projection suffix
   - Latitude: starts with "DEC" or ends with "LAT"
2. Extract projection code from CTYPE (last 3 chars after the last `-`)
3. Create WcsMap with the projection
4. Determine RADESYS/EQUINOX → SkySystem
5. Set CRVAL as the reference point (native longitude, latitude)
6. Handle LONPOLE/LATPOLE (native pole positioning)
7. Apply CRVAL rotation (shift IWC origin to CRVAL on sky)
8. Create SkyFrame with appropriate system

For the initial implementation, handle the common case:
- Projection applied via WcsMap(2, proj, lonax, latax)
- CRVAL rotation via additional SlaMap or MatrixMap step
- SkyFrame with system from RADESYS

- [ ] **Step 2: CTYPE parsing helper**

```rust
/// Parse a CTYPE string into (base_type, projection_code).
/// e.g., "RA---TAN" → ("RA", Some("TAN"))
///       "DEC--TAN-SIP" → ("DEC", Some("TAN"))
///       "FREQ" → ("FREQ", None)
fn parse_ctype(ctype: &str) -> (&str, Option<&str>);
```

- [ ] **Step 3: Tests**

- "RA---TAN" + "DEC--TAN" → detect as celestial, projection TAN
- "GLON-AIT" + "GLAT-AIT" → Galactic + AIT projection
- No celestial axes → returns None

---

### Task 5: Spectral Axis Decoder

**Files:**
- Create: `crates/starlink-ast/src/fitschan/wcs_spectral.rs`

Detect spectral axes and build SpecFrame.

- [ ] **Step 1: Implement build_spectral**

```rust
/// If a spectral axis is present, build the IWC→spectral mapping and SpecFrame.
pub(crate) fn build_spectral(
    store: &FitsStore,
) -> Option<(DynMapping, Box<SpecFrame>, usize)>
```

Detect spectral from CTYPE: "FREQ", "WAVE", "AWAV", "VRAD", "VOPT", "ZOPT", "VELO", "BETA".

For the spectral axis:
- Linear case: CRVAL shift via ShiftMap
- Create SpecFrame with appropriate system from CTYPE
- Set SPECSYS as StdOfRest if present
- Set RESTFRQ/RESTWAV if present

- [ ] **Step 2: Tests**

- CTYPE="FREQ" → detect spectral, system=Freq
- CTYPE="WAVE" → detect spectral, system=Wavelen

---

### Task 6: Main Decoder (FitsStore → FrameSet)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/wcs_read.rs`
- Modify: `crates/starlink-ast/src/fitschan/mod.rs`

Compose the full pipeline: FitsStore → FrameSet.

- [ ] **Step 1: Implement decode_wcs**

```rust
/// Build a FrameSet from a FitsStore.
///
/// Creates pixel frame (base), composes the transformation pipeline,
/// and creates the appropriate world frame (current).
pub(crate) fn decode_wcs(store: &FitsStore) -> AstResult<AnyObject>
```

Pipeline:
1. Build pixel→IWC mapping (`build_pixel_to_iwc_mapping`)
2. Check for celestial axes (`build_celestial`)
3. Check for spectral axis (`build_spectral`)
4. For non-celestial/non-spectral axes: CRVAL offset (ShiftMap)
5. Create pixel Frame (BasicFrame, domain="GRID")
6. Create world Frame:
   - If celestial: SkyFrame
   - If spectral: SpecFrame
   - If both: CmpFrame(SkyFrame, SpecFrame)
   - If neither: BasicFrame with CRVAL offset
7. Compose full mapping: pixel→IWC→world
8. Build FrameSet: base=pixel, current=world, mapping=composed

- [ ] **Step 2: Wire into FitsChan::read()**

Update `FitsChan::read()` for `Encoding::FitsWcs`:
```rust
Encoding::FitsWcs => {
    let store = wcs_keywords::extract_wcs_keywords(&self.cards)?;
    wcs_read::decode_wcs(&store)
}
```

- [ ] **Step 3: Tests**

- Minimal 2D celestial header → FrameSet with SkyFrame
- Verify transform: pixel(128,128) maps to CRVAL position
- Verify SkyFrame system matches RADESYS
- Verify projection type detected correctly

---

### Task 7: Integration Tests with Real Headers

**Files:**
- Create: `crates/starlink-ast/tests/test_fitschan_wcs.rs`

- [ ] **Step 1: Write tests using sip.head and minimal headers**

Tests:
1. **Minimal 2D TAN header**: Hand-crafted minimal header, verify FrameSet structure
2. **sip.head fixture**: Load, read via FitsChan(FitsWcs), verify SkyFrame in FrameSet
3. **Transform verification**: Pixel (128,128) → should be near CRVAL
4. **Inverse transform**: Sky → pixel round-trip
5. **SkyFrame attributes**: system=ICRS, equinox=2000
6. **SIP distortion** (if implemented): pixel transform deviates from pure linear

- [ ] **Step 2: Run and commit**

---

### Task 8: Final Verification

- [ ] Clippy, fmt, full test suite

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p starlink-ast` — all tests pass
3. FitsChan with FitsWcs encoding produces a FrameSet from FITS headers
4. FrameSet has pixel frame (base) and sky frame (current)
5. Transform maps reference pixel to reference sky position
6. SkyFrame system matches RADESYS keyword
7. PC/CD matrix handling works correctly
8. All existing tests still pass
