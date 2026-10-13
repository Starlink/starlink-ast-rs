# Alternate-WCS support — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** End-to-end FITS-WCS alternate-plane support: reader builds a 2+nalt-frame FrameSet from `*A..*Z` axis-indexed keywords; writer loops version letters and emits per-axis cards with the appropriate suffix; global keywords (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DTAI, DUT1) follow the C library's last-plane-wins rule. Mirrors `FsetToStore`/`AddVersion` (fitschan.c:12263-12353, 2674) and `WcsFromStore`/`WcsSkyFrame` (fitschan.c:36130+, 39008+).

**Architecture:** Six commit groups landing the read side first, then writer plumbing, then per-encoder retrofits, then DTAI/DUT1 storage + wcsconv validation. The native-format round-trip is the per-group integration check; the wcsconv harness is the final gate. Each group leaves the build green; some groups may leave a small number of formerly-passing fixtures temporarily red until a downstream group lands the matching encoder change.

**Tech Stack:** Rust 2024, ndarray, existing FitsChan / FitsStore / FrameSet / Channel infrastructure. C reference at `reference/starlink-ast/src/fitschan.c`. Tests use `cargo test -p ast-core --test test_wcsconv`.

**Spec:** `docs/superpower/specs/2026-06-01-alternate-wcs-support-design.md` (commit `ba4a9a2`). Read it first.

---

## Hard rules

1. **Match C structure.** The reader produces a FrameSet with `Lnk{i}=1` for every alt frame (parent = base node). The writer's per-letter loop processes alt planes in alphabetical order. Last-plane-wins for global cards.
2. **Primary-fallback on read.** Per-axis keys for an alt plane fall back to the matching primary key when absent. Mirrors C `GetItem(store, axis, m, s)` which returns the `s=' '` value when the requested `s` slot is empty.
3. **Bit-exact emission required.** Every per-axis card in the alt plane must match C `wcsconverter` output byte-for-byte. The native intermediate (`to_native_string` → `parse_native`) round-trips losslessly.
4. **No new placeholders.** Each commit group leaves the codebase compileable with `cargo fmt`, `cargo clippy --workspace -- -D warnings`, and `cargo test -p ast-core` (allowing for the documented temporary red fixtures inside groups 4–5; group 6 finalises).
5. **Don't change the FrameSet integrity contract.** `add_frame(FrameRef::Base, ...)` stores the supplied mapping verbatim; the writer's `mapping(...)` call simplifies on retrieval. Both behaviours match today's primary-frame path; alt frames go through the same path.

---

## File structure

### Files created

```
crates/ast-core/src/fitschan/wcs_keywords/
└── alt_planes.rs                    # detect_alt_letters, primary fallback (Group 2)

crates/ast-core/src/fitschan/wcs_write/
└── plane_stores.rs                  # PlaneStore, collect_plane_stores (Group 4)

crates/ast-core/tests/
├── test_alt_wcs_read.rs             # multi-WCS read-side fixture tests (Group 3)
└── test_alt_wcs_round_trip.rs       # native + per-encoder round-trip (Group 5)
```

### Files modified

```
crates/ast-core/src/frames/
├── basic.rs                         # set_ident (Group 1)
├── skyframe.rs                      # set_ident (Group 1)
├── specframe.rs                     # set_ident (Group 1)
├── timeframe.rs                     # set_ident (Group 1)
├── cmpframe.rs                      # set_ident (Group 1)
├── fluxframe.rs                     # set_ident (Group 1)
└── specfluxframe.rs                 # set_ident (Group 1) — if it has Ident

crates/ast-core/src/fitschan/
├── wcs_keywords.rs                  # extract_wcs_keywords gains s: char (Group 2)
├── wcs_read.rs                      # decode_wcs_with_alts (Group 3)
├── wcs_celestial.rs                 # alt-plane Equinox/Epoch defaults (Group 3)
├── mod.rs                           # FitsChan::read dispatches alt-aware (Group 3)
├── fits_store.rs                    # dtai/dut1 fields + doc comments (Group 6)
├── fset_to_store/mod.rs             # add s: char param (Group 4)
├── fset_to_store/celestial_axes.rs  # accept s, propagate to sky_sys (Group 4)
├── fset_to_store/spectral_axes.rs   # accept s, propagate (Group 4)
├── fset_to_store/other_axes.rs      # accept s (Group 4)
├── fset_to_store/sky_sys.rs         # already takes s; doc comment polish (Group 4)
├── wcs_write.rs                     # collect_plane_stores; encode_wcs uses last-wins (Group 4-5)
├── enc_pc.rs                        # write_to_cards accepts &[PlaneStore] (Group 5)
├── enc_iraf.rs                      # ditto (Group 5)
├── enc_aips.rs                      # ditto + ditto _pp (Group 5)
└── enc_class.rs                     # ditto (Group 5)
```

### Files deleted

```
crates/ast-core/src/fitschan/wcs_write.rs
└── _legacy_store_from_frameset_to    # body + private helpers (Group 5)
```

---

## Background — C source reference

| Topic | Location | Notes |
|-------|----------|-------|
| `FsetToStore` outer per-version loop | fitschan.c:12263-12353 | drives per-plane `AddVersion` |
| `AddVersion` per-letter dispatch | fitschan.c:2743 | invokes per-axis `MakeIntWorld` etc. |
| `WcsFromStore` reader top | fitschan.c:36030+ | per-version-letter scan for axis keys |
| `WcsSkyFrame` per-version SkyFrame build | fitschan.c:39138-39450 | NORADEC / DefB1950 fallback |
| `SkySys` MJD-OBS overwrite (forces `s=' '`) | fitschan.c:29917 | the global last-wins source |
| NORADEC + DefB1950 default | fitschan.c:39302-39310 | yields B1950 Epoch on Galactic alt |
| nomjd-obs fallback to eqmjd | fitschan.c:39344-39352 | sets SkyFrame Epoch from EQUINOX |
| `GetItem` primary-fallback semantics | fitschan.c, search `GetItem` | look near `FitsStore *store` accesses |
| `astChrMatch(astGetFitsAxisOrder(this), "<auto>")` | fitschan.c:5405 | gates the auto axis-swap |

When in doubt about a specific keyword's per-version behaviour, search `fitschan.c` for `SetItem( &(store->`*name*`,` and look at the `s` parameter in the call.

---

## Group 1: Frame `Ident` setters

**Goal:** Add `set_ident(&mut self, ident: impl Into<String>)` on every Frame variant. Pure plumbing, no behavioural change.

### Task 1.1: BasicFrame::set_ident

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs`

- [ ] **Step 1: Locate the existing `ident()` getter**

```bash
grep -n "fn ident\b\|ident: Option" crates/ast-core/src/frames/basic.rs
```

Expected: getter at ~line 237 returning `&str`; field `ident: Option<String>` at ~line 34.

- [ ] **Step 2: Add `set_ident` next to the getter**

After the existing `pub fn ident(&self) -> &str` (around line 237 in `basic.rs`):

```rust
    /// Set the frame identification string. Mirrors `astSetIdent`.
    /// Used by the FITS-WCS reader to label alternate-plane frames
    /// with their version letter (`'A'..'Z'`).
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }
```

- [ ] **Step 3: Add unit test**

In the existing `#[cfg(test)] mod tests` block at the bottom of `basic.rs`, add:

```rust
    #[test]
    fn set_ident_round_trip() {
        let mut f = BasicFrameBuilder::new(2).build().unwrap();
        assert_eq!(f.ident(), "");
        f.set_ident("A");
        assert_eq!(f.ident(), "A");
        f.set_ident("");
        assert_eq!(f.ident(), "");
    }
```

- [ ] **Step 4: Run the test**

```bash
cargo test -p ast-core --lib frames::basic::tests::set_ident_round_trip
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/basic.rs
git commit -m "$(cat <<'EOF'
frames/basic: add set_ident setter

Mirrors astSetIdent. Will be used by the FITS-WCS reader to label
alternate-plane frames with their version letter.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 1.2: SkyFrame::set_ident

**Files:**
- Modify: `crates/ast-core/src/frames/skyframe.rs`

- [ ] **Step 1: Add `set_ident`**

After the existing `pub fn ident(&self) -> &str` (around line 418 in `skyframe.rs`):

```rust
    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }
```

- [ ] **Step 2: Add unit test**

In the existing `#[cfg(test)] mod tests` block, add:

```rust
    #[test]
    fn skyframe_set_ident_round_trip() {
        use crate::frames::sky_convert::SkySystem;
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        assert_eq!(sky.ident(), "");
        sky.set_ident("A");
        assert_eq!(sky.ident(), "A");
        sky.set_ident("");
        assert_eq!(sky.ident(), "");
    }
```

- [ ] **Step 3: Run the test**

```bash
cargo test -p ast-core --lib frames::skyframe::tests::skyframe_set_ident_round_trip
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/skyframe.rs
git commit -m "$(cat <<'EOF'
frames/skyframe: add set_ident setter

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 1.3: Remaining Frame variants

Same pattern for SpecFrame, TimeFrame, CmpFrame, FluxFrame. Each step is the same shape as 1.1/1.2; the only thing that changes is which file's `ident()` getter the new setter sits next to.

**Files:**
- Modify: `crates/ast-core/src/frames/specframe.rs`
- Modify: `crates/ast-core/src/frames/timeframe.rs`
- Modify: `crates/ast-core/src/frames/cmpframe.rs`
- Modify: `crates/ast-core/src/frames/fluxframe.rs`

- [ ] **Step 1: Add `set_ident` to SpecFrame**

In `crates/ast-core/src/frames/specframe.rs`, find the existing `ident: Option<String>` field and the `ident()` getter (use `grep -n "fn ident\b" crates/ast-core/src/frames/specframe.rs`). Add adjacent:

```rust
    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }
```

- [ ] **Step 2: Add `set_ident` to TimeFrame**

In `crates/ast-core/src/frames/timeframe.rs`, same shape:

```rust
    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }
```

- [ ] **Step 3: Add `set_ident` to CmpFrame**

In `crates/ast-core/src/frames/cmpframe.rs`:

```rust
    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }
```

- [ ] **Step 4: Add `set_ident` to FluxFrame**

In `crates/ast-core/src/frames/fluxframe.rs`:

```rust
    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }
```

- [ ] **Step 5: Verify no setter is missing**

```bash
grep -L "pub fn set_ident" \
  crates/ast-core/src/frames/basic.rs \
  crates/ast-core/src/frames/skyframe.rs \
  crates/ast-core/src/frames/specframe.rs \
  crates/ast-core/src/frames/timeframe.rs \
  crates/ast-core/src/frames/cmpframe.rs \
  crates/ast-core/src/frames/fluxframe.rs
```

Expected: empty output (every file contains `pub fn set_ident`).

- [ ] **Step 6: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean build.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/frames/specframe.rs crates/ast-core/src/frames/timeframe.rs crates/ast-core/src/frames/cmpframe.rs crates/ast-core/src/frames/fluxframe.rs
git commit -m "$(cat <<'EOF'
frames: add set_ident on Spec/Time/Cmp/Flux frames

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 1.4: Group 1 finalisation

- [ ] **Step 1: Format + clippy**

```bash
cargo fmt -p ast-core
cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 2: Full test suite still green**

```bash
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | head -5
```

Expected: all `ok` lines, no `FAILED`.

- [ ] **Step 3: Group commit (allow-empty if no fmt changes)**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: Group 1 (Frame Ident setters) complete

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Group 2: Reader — multi-version FitsStore extraction

**Goal:** Add a `s: char` parameter to `extract_wcs_keywords`. Build a discovery helper that scans cards for non-empty alt-version letters. Add `extract_wcs_keywords_with_alts` that returns `(primary_store, Vec<(letter, alt_store)>)`. No FrameSet shape change yet — just plumbing the keyword extraction.

### Task 2.1: Add `key_for` helper + `s: char` parameter

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs`

**C reference:** `fitschan.c` `FormatKey` (search `static char *FormatKey`); semantics: `if (s == ' ' || s == 0) return base; else return base+s`.

- [ ] **Step 1: Add a `key_for` helper near the top of `wcs_keywords.rs`**

After the `use` block at the top of the file, add:

```rust
/// Build a FITS keyword name with the version-letter suffix.
///
/// `s == ' '` means the primary plane (no suffix).
/// `s` in `'A'..='Z'` produces the alt-plane key (e.g. `CRVAL1A`).
///
/// Mirrors `fitschan.c::FormatKey` for the (axis-indexed, scalar)
/// keyword shapes used by FITS-WCS.
pub(crate) fn key_for(prefix: &str, axis: Option<usize>, s: char) -> String {
    match (axis, s) {
        (Some(i), ' ') => format!("{prefix}{i}"),
        (Some(i), c) => format!("{prefix}{i}{c}"),
        (None, ' ') => prefix.to_string(),
        (None, c) => format!("{prefix}{c}"),
    }
}

/// Build a FITS keyword name for a 2-index keyword (`PC{i}_{j}{S}`,
/// `CD{i}_{j}{S}`).
pub(crate) fn key_for_2(prefix: &str, i: usize, j: usize, s: char) -> String {
    if s == ' ' {
        format!("{prefix}{i}_{j}")
    } else {
        format!("{prefix}{i}_{j}{s}")
    }
}
```

- [ ] **Step 2: Add unit tests for `key_for` / `key_for_2`**

In the existing `#[cfg(test)] mod tests` at the bottom of `wcs_keywords.rs`, add:

```rust
    #[test]
    fn key_for_primary_no_suffix() {
        assert_eq!(key_for("CRVAL", Some(1), ' '), "CRVAL1");
        assert_eq!(key_for("RADESYS", None, ' '), "RADESYS");
    }

    #[test]
    fn key_for_alt_appends_letter() {
        assert_eq!(key_for("CRVAL", Some(1), 'A'), "CRVAL1A");
        assert_eq!(key_for("RADESYS", None, 'A'), "RADESYSA");
    }

    #[test]
    fn key_for_2_handles_pc_cd() {
        assert_eq!(key_for_2("PC", 1, 2, ' '), "PC1_2");
        assert_eq!(key_for_2("CD", 1, 1, 'B'), "CD1_1B");
    }
```

- [ ] **Step 3: Run the new tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_keywords::tests::key_for_ 2>&1 | tail -10
```

Expected: 3 PASSes.

- [ ] **Step 4: Refactor `extract_wcs_keywords` to take `s: char`**

Change the signature:

```rust
pub(crate) fn extract_wcs_keywords(cards: &CardList, s: char) -> AstResult<FitsStore> {
```

Inside the body, replace every `format!("{prefix}{axis_num}")` style construction with `key_for(prefix, Some(axis_num), s)` and every `format!("{prefix}{i}_{j}")` with `key_for_2(prefix, axis_num, j, s)`. Replace scalar lookups (`RADESYS`, `EQUINOX`, `LONPOLE`, `LATPOLE`, `WCSAXES`, `WCSNAME`, `SPECSYS`, `RESTFRQ`, `RESTWAV`) with `key_for(name, None, s)`.

For naxis discovery, also use `s`-aware keys:

```rust
    let wcsaxes = cards.get_int(&key_for("WCSAXES", None, s)).map(|v| v as usize);
    let naxis_val = cards.get_int("NAXIS").map(|v| v as usize);  // NAXIS itself is not s-suffixed
```

When the alt plane lookup returns `None` for a per-axis key, fall back to the primary key (matches C `GetItem`):

```rust
    fn lookup_with_fallback<T, F>(cards: &CardList, prefix: &str, axis: Option<usize>, s: char, f: F) -> Option<T>
    where F: Fn(&CardList, &str) -> Option<T> {
        let alt = f(cards, &key_for(prefix, axis, s));
        if alt.is_some() || s == ' ' {
            alt
        } else {
            f(cards, &key_for(prefix, axis, ' '))
        }
    }
```

Use this helper for every per-axis and scalar lookup inside `extract_wcs_keywords`. Example:

```rust
        store.crpix[i] = lookup_with_fallback(cards, "CRPIX", Some(axis_num), s, |c, k| c.get_float(k));
        store.crval[i] = lookup_with_fallback(cards, "CRVAL", Some(axis_num), s, |c, k| c.get_float(k));
        store.ctype[i] = lookup_with_fallback(cards, "CTYPE", Some(axis_num), s, |c, k| c.get_string(k));
        // ...same for CDELT, CUNIT, CNAME, CD{i}_{j}, PC{i}_{j}, PV{i}_{m}.
```

For 2-index keys (CD/PC), inside the `for j in 0..naxis` loop:

```rust
        store.cd[i][j] = lookup_with_fallback(cards, "CD", None, s, |c, _| c.get_float(&key_for_2("CD", axis_num, j+1, s)))
            .or_else(|| if s != ' ' { c.get_float(&key_for_2("CD", axis_num, j+1, ' ')) } else { None });
```

(`key_for_2` doesn't fit `lookup_with_fallback`'s shape; inline the fallback for these two keys.)

- [ ] **Step 5: Update existing call sites**

`extract_wcs_keywords` is currently called by `FitsChan::read` (in `mod.rs`) — locate via `grep -n "extract_wcs_keywords" crates/ast-core/src/fitschan/mod.rs`. Update the caller to pass `' '`:

```rust
            Encoding::FitsWcs => {
                let store = wcs_keywords::extract_wcs_keywords(&self.cards, ' ')?;
                ...
            }
```

Also update test-only callers inside `wcs_keywords.rs` itself (search `extract_wcs_keywords(&cards)` in the tests block) to pass `' '`.

- [ ] **Step 6: Build + run lib tests**

```bash
cargo build -p ast-core 2>&1 | tail -3
cargo test -p ast-core --lib fitschan::wcs_keywords 2>&1 | tail -5
```

Expected: clean build; existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_keywords: thread s: char through extract_wcs_keywords

Adds key_for / key_for_2 helpers and a lookup_with_fallback helper
that mirrors fitschan.c::GetItem (alt-plane lookup falls back to
the primary key when the alt-suffixed card is absent).

extract_wcs_keywords now takes a coordinate-version character;
existing callers pass ' ' (primary). Reader-side alt support
arrives in Group 3.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 2.2: Detect alt-version letters from a CardList

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs`

**C reference:** `fitschan.c::WcsFromStore` walks `s = ' '` then `s = 'A'..'Z'` and tests whether any axis-indexed key for that `s` exists in the store. We do the equivalent at the card level.

- [ ] **Step 1: Add detector**

After `key_for_2`, add:

```rust
/// Discover the set of alt-plane version letters present in a
/// FITS-WCS card list.
///
/// Returns letters in alphabetical order. A letter is included iff
/// at least one of `CTYPE{i}{S}`, `CRVAL{i}{S}`, `CRPIX{i}{S}`,
/// `CDELT{i}{S}` is present for some axis `i ∈ 1..=9`.
///
/// Mirrors C `WcsFromStore`'s per-version axis-key scan.
pub(crate) fn detect_alt_letters(cards: &CardList) -> Vec<char> {
    let mut found = Vec::new();
    for c in 'A'..='Z' {
        let any = (1..=9).any(|i| {
            cards.find(&format!("CTYPE{i}{c}")).is_some()
                || cards.find(&format!("CRVAL{i}{c}")).is_some()
                || cards.find(&format!("CRPIX{i}{c}")).is_some()
                || cards.find(&format!("CDELT{i}{c}")).is_some()
        });
        if any {
            found.push(c);
        }
    }
    found
}
```

- [ ] **Step 2: Unit tests**

In the tests block at the bottom of `wcs_keywords.rs`:

```rust
    #[test]
    fn detect_alt_letters_empty_when_only_primary() {
        let mut cards = CardList::new();
        cards.set_string("CTYPE1", "RA---TAN", "");
        cards.set_string("CTYPE2", "DEC--TAN", "");
        assert!(detect_alt_letters(&cards).is_empty());
    }

    #[test]
    fn detect_alt_letters_finds_a_and_b() {
        let mut cards = CardList::new();
        cards.set_string("CTYPE1A", "GLON-TAN", "");
        cards.set_string("CRPIX1B", "100.0", "");
        let letters = detect_alt_letters(&cards);
        assert_eq!(letters, vec!['A', 'B']);
    }
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_keywords::tests::detect_alt_letters 2>&1 | tail -10
```

Expected: 2 PASSes.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_keywords: detect_alt_letters discovery helper

Walks 'A'..'Z' and reports each letter for which any of CTYPE/CRVAL/
CRPIX/CDELT is present in the cards. Used by Group 3's
decode_wcs_with_alts to decide which planes to materialise.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 2.3: `extract_wcs_keywords_with_alts`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs`

- [ ] **Step 1: Add the multi-plane entry point**

After `extract_wcs_keywords`, add:

```rust
/// Extract primary + every detected alt-plane FitsStore from a
/// CardList.
///
/// The returned alt list is in alphabetical letter order. Each alt
/// FitsStore was extracted with `extract_wcs_keywords(cards, s)`,
/// which falls back to the primary keyword when the alt-suffixed
/// version is absent — see [`extract_wcs_keywords`] for the
/// fallback rule. Mirrors fitschan.c::WcsFromStore's per-version
/// loop.
pub(crate) fn extract_wcs_keywords_with_alts(
    cards: &CardList,
) -> AstResult<(FitsStore, Vec<(char, FitsStore)>)> {
    let primary = extract_wcs_keywords(cards, ' ')?;
    let alt_letters = detect_alt_letters(cards);
    let mut alts = Vec::with_capacity(alt_letters.len());
    for s in alt_letters {
        let store = extract_wcs_keywords(cards, s)?;
        alts.push((s, store));
    }
    Ok((primary, alts))
}
```

- [ ] **Step 2: Unit test using a multi-WCS fixture-style header**

Add to the tests block:

```rust
    #[test]
    fn extract_with_alts_reads_two_planes() {
        let mut cards = CardList::new();
        cards.set_int("NAXIS", 2, "");
        cards.set_string("CTYPE1", "RA---TAN", "");
        cards.set_string("CTYPE2", "DEC--TAN", "");
        cards.set_float("CRVAL1", 180.0, "");
        cards.set_float("CRVAL2", 45.0, "");
        cards.set_float("CRPIX1", 50.5, "");
        cards.set_float("CRPIX2", 50.5, "");
        cards.set_float("CDELT1", -0.01, "");
        cards.set_float("CDELT2", 0.01, "");
        // Alt 'A': override CTYPE only — CRVAL/CRPIX/CDELT fall back to primary.
        cards.set_string("CTYPE1A", "GLON-TAN", "");
        cards.set_string("CTYPE2A", "GLAT-TAN", "");

        let (primary, alts) = extract_wcs_keywords_with_alts(&cards).unwrap();
        assert_eq!(primary.ctype[0].as_deref(), Some("RA---TAN"));
        assert_eq!(alts.len(), 1);
        let (letter, alt) = &alts[0];
        assert_eq!(*letter, 'A');
        assert_eq!(alt.ctype[0].as_deref(), Some("GLON-TAN"));
        // Primary fallback for CRVAL/CRPIX/CDELT.
        assert_eq!(alt.crval[0], Some(180.0));
        assert_eq!(alt.crpix[0], Some(50.5));
        assert_eq!(alt.cdelt[0], Some(-0.01));
    }
```

- [ ] **Step 3: Run the test**

```bash
cargo test -p ast-core --lib fitschan::wcs_keywords::tests::extract_with_alts_reads_two_planes 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_keywords: extract_wcs_keywords_with_alts

Returns (primary_store, [(letter, alt_store), ...]). Each alt store
is built by extract_wcs_keywords(cards, letter) and inherits primary
values for axis keywords that are absent at the alt-suffixed key.
Mirrors fitschan.c::WcsFromStore's per-version loop.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 2.4: Group 2 finalisation

- [ ] **Step 1: fmt + clippy + full test suite**

```bash
cargo fmt -p ast-core
cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -10
```

Expected: clean clippy; all tests `ok`.

- [ ] **Step 2: Group commit (allow-empty if no fmt changes)**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: Group 2 (multi-version FitsStore extraction) complete

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---
## Group 3: Reader — alt-plane FrameSets

**Goal:** Add `decode_wcs_with_alts` that builds the primary FrameSet via the existing decoder, then adds one alt frame per `(letter, alt_store)` pair via `add_frame(FrameRef::Base, …)`. Each alt frame gets `Ident=letter`. Wire `FitsChan::read` to use the alt-aware decoder for every FITS-* encoding. The native dump of a multi-WCS header now matches the C library byte-for-byte.

### Task 3.1: Alt SkyFrame default Equinox/Epoch

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs`

**C reference:** `fitschan.c::WcsSkyFrame` (39008-39320). The branch we need is at 39172-39180 (NORADEC + DefB1950 fallback) and 39214-39222 (nomjd-obs → mjdobs = eqmjd).

The Rust `build_celestial` function already handles RADESYS / EQUINOX / MJD-OBS for primary planes. For alt planes whose RADESYS is unset and EQUINOX is unset (the typical Galactic alt-A case), we need: `equinox = palEpb2d(1950)` and `epoch = equinox` (the AST `DefB1950` default).

- [ ] **Step 1: Locate the `build_celestial` body**

```bash
grep -n "fn build_celestial\b\|fn determine_sky_system\|sky_frame.set_epoch\b" crates/ast-core/src/fitschan/wcs_celestial.rs
```

- [ ] **Step 2: Add a helper that mirrors C's NORADEC fallback**

Near the top of `wcs_celestial.rs` (after the existing `use` block), add:

```rust
/// Apply C `WcsSkyFrame`'s NORADEC + DefB1950 fallback rule to an
/// alt-plane SkyFrame whose RADESYS is unset and EQUINOX is unset.
///
/// Mirrors fitschan.c:39302-39310. AST's `DefB1950` attribute
/// defaults to true, so an EQUINOX-less, RADESYS-less alt SkyFrame
/// is interpreted as B1950 (FK4) and gets Equinox = palEpb2d(1950)
/// ≈ MJD 33281.92345.
///
/// Also applies the nomjd-obs fallback (39214-39222): when no
/// MJD-OBS / DATE-OBS card was present, Epoch defaults to Equinox.
fn apply_b1950_fallback_if_needed(store: &FitsStore, sky_frame: &mut SkyFrame) {
    let no_radesys = store.radesys.as_deref().map_or(true, str::is_empty);
    let no_equinox = store.equinox.is_none();
    let no_mjdobs = store.mjdobs.is_none();
    if no_radesys && no_equinox {
        // palEpb2d(1950.0). Pre-compute as a constant to avoid
        // adding a runtime palEpb2d call here.
        const MJD_B1950: f64 = 33281.92345905;
        sky_frame.set_equinox(MJD_B1950);
        if no_mjdobs {
            sky_frame.set_epoch(MJD_B1950);
        }
    }
}
```

- [ ] **Step 3: Call the helper from `build_celestial`**

In `build_celestial`, find the block that sets EQUINOX / Epoch on the SkyFrame (it's the area that calls `set_equinox_year` / `set_epoch_year` / `set_epoch`). Right after that block, add:

```rust
    apply_b1950_fallback_if_needed(store, &mut sky_frame);
```

- [ ] **Step 4: Add a unit test**

In the tests block at the bottom of `wcs_celestial.rs`:

```rust
    #[test]
    fn alt_galactic_no_radesys_no_equinox_falls_back_to_b1950() {
        let mut store = FitsStore::new(2);
        store.naxis = 2;
        store.ctype[0] = Some("GLON-TAN".to_string());
        store.ctype[1] = Some("GLAT-TAN".to_string());
        store.crval[0] = Some(101.255);
        store.crval[1] = Some(16.177);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);
        // No RADESYS, no EQUINOX, no MJD-OBS.
        let result = build_celestial(&store).expect("celestial axes recognised");
        // Expect Equinox set to B1950 ≈ 33281.92345.
        let eq = result.frame.equinox_mjd();
        assert!((eq - 33281.923).abs() < 1.0, "equinox = {eq}");
        // Epoch likewise.
        let ep = result.frame.epoch_mjd();
        assert!((ep - 33281.923).abs() < 1.0, "epoch = {ep}");
    }
```

- [ ] **Step 5: Run the test**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests::alt_galactic_no_radesys_no_equinox_falls_back_to_b1950 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_celestial.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_celestial: apply WcsSkyFrame NORADEC+DefB1950 fallback

Mirrors fitschan.c:39302-39310 (NORADEC default eq=B1950) and
39214-39222 (nomjd-obs → epoch=eqmjd). When an alt-plane SkyFrame
arrives with no RADESYS, no EQUINOX, no MJD-OBS — typical for the
GALACTIC alt-A in fits-pc-gal — Equinox and Epoch default to B1950
(MJD 33281.92345). This is the value that subsequently lands in the
emitted MJD-OBS via the writer's last-plane-wins rule.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 3.2: `decode_wcs_with_alts`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs`

- [ ] **Step 1: Inspect the existing `decode_wcs`**

```bash
grep -n "pub.*fn decode_wcs\b\|fn build_world_frame\|fn build_iwc_to_world" crates/ast-core/src/fitschan/wcs_read.rs
```

The existing `decode_wcs(store) -> AstResult<AnyObject>` returns an `AnyObject::FrameSet`. We need to keep it (used by the FITS-CLASS / DSS / FITS-AIPS reader paths via `FitsChan::read`) while adding a sibling that consumes a `(primary, Vec<(char, FitsStore)>)` pair.

- [ ] **Step 2: Add the alt-aware decoder**

After the existing `decode_wcs` body, add:

```rust
/// Decode the primary plane plus zero or more alt planes into a
/// FrameSet whose base = pixel grid, current = primary world, and
/// extra frames = alt-plane world frames (one per detected version
/// letter).
///
/// Each alt frame is added with `add_frame(FrameRef::Base, …)`, so
/// the native dump shows `Lnk{i}=1` for every alt — matching the C
/// library's structure exactly. Each alt frame's `Ident` is the
/// version letter; the writer reads this back to recover the
/// per-plane suffix on round-trip.
///
/// Mirrors fitschan.c::WcsFromStore + the per-version `WcsSkyFrame`
/// loop.
pub(crate) fn decode_wcs_with_alts(
    primary: &FitsStore,
    alts: &[(char, FitsStore)],
) -> AstResult<AnyObject> {
    // Step 1: build the primary FrameSet via the existing decoder.
    let primary_obj = decode_wcs(primary)?;
    let mut fs = match primary_obj {
        AnyObject::FrameSet(f) => f,
        _ => unreachable!("decode_wcs always returns AnyObject::FrameSet"),
    };

    // Step 2: append one alt frame per (letter, alt_store).
    for (letter, alt_store) in alts {
        // Reuse the per-store helpers from decode_wcs to produce the
        // pixel→alt-world mapping and the alt world frame.
        let pixel_to_iwc = wcs_matrix::build_pixel_to_iwc(alt_store)?;
        let celestial = wcs_celestial::build_celestial(alt_store);
        let spectral = wcs_spectral::build_spectral(alt_store);
        let mut world_frame = build_world_frame(alt_store, &celestial, &spectral)?;
        let iwc_to_world = build_iwc_to_world(alt_store, celestial, spectral)?;
        let full_mapping: DynMapping = Box::new(CmpMap::new(
            pixel_to_iwc,
            iwc_to_world,
            Composition::Series,
        )?);

        // Stamp the version letter onto the alt frame.
        // The Frame trait has no set_ident; downcast to the concrete
        // type to call the per-class setter. We try every variant we
        // know how to label.
        set_frame_ident(world_frame.as_mut(), &letter.to_string());

        fs.add_frame(FrameRef::Base, full_mapping, world_frame)?;
    }

    // Restore current to the primary frame (add_frame moved it).
    fs.set_current(FrameRef::Index(2))?;
    Ok(AnyObject::FrameSet(fs))
}

/// Helper: stamp `ident` onto a boxed Frame, dispatching by concrete
/// type. Falls back silently for frame variants we don't yet label
/// (the writer's letter-assignment second pass will pick a letter
/// for them).
fn set_frame_ident(frame: &mut dyn crate::frames::Frame, ident: &str) {
    use crate::frames::{BasicFrame, CmpFrame, SkyFrame, SpecFrame, TimeFrame, FluxFrame};
    if let Some(f) = frame.as_any_mut().downcast_mut::<BasicFrame>() { f.set_ident(ident); return; }
    if let Some(f) = frame.as_any_mut().downcast_mut::<SkyFrame>() { f.set_ident(ident); return; }
    if let Some(f) = frame.as_any_mut().downcast_mut::<SpecFrame>() { f.set_ident(ident); return; }
    if let Some(f) = frame.as_any_mut().downcast_mut::<TimeFrame>() { f.set_ident(ident); return; }
    if let Some(f) = frame.as_any_mut().downcast_mut::<CmpFrame>() { f.set_ident(ident); return; }
    if let Some(f) = frame.as_any_mut().downcast_mut::<FluxFrame>() { f.set_ident(ident); return; }
}
```

- [ ] **Step 3: Verify `Frame::as_any_mut` exists**

```bash
grep -n "as_any_mut\|fn as_any_mut" crates/ast-core/src/frames/mod.rs
```

If `as_any_mut` is not on the Frame trait yet, add it next to the existing `as_any` (search `fn as_any\b` in `frames/mod.rs` and the per-Frame `impl Frame for X` blocks):

In `crates/ast-core/src/frames/mod.rs`, on the `Frame` trait:

```rust
    /// Mutable `&mut dyn Any` accessor.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
```

In every `impl Frame for X { ... }` block (basic.rs, skyframe.rs, specframe.rs, timeframe.rs, cmpframe.rs, fluxframe.rs, frameset.rs), add:

```rust
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
```

(Skip frames that don't already implement `fn as_any` — those don't need it for this work.)

- [ ] **Step 4: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean. (Compiler will tell you about any missing `as_any_mut` impl; add the four-line stub to the file it complains about.)

- [ ] **Step 5: Add a small integration test**

Create `crates/ast-core/tests/test_alt_wcs_read.rs`:

```rust
//! Reader-side tests for FITS-WCS alternate-plane support.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;
use ast_core::frames::FrameRef;

const FITS_PC_GAL_HEAD: &str = "\
NAXIS1  =                  100 / Spatial axis
NAXIS2  =                  100 / Spatial axis
CTYPE1  = 'RA---TAN'           / Celestial longitude
CTYPE2  = 'DEC--TAN'           / Celestial latitude
CRVAL1  =              180.000 / Reference RA (deg)
CRVAL2  =               45.000 / Reference Dec (deg)
CRPIX1  =               50.500 / Reference pixel on axis 1
CRPIX2  =               50.500 / Reference pixel on axis 2
CDELT1  =              -0.0100 / Degrees/pixel
CDELT2  =               0.0100 / Degrees/pixel
RADESYS = 'FK5     '           / Reference frame
EQUINOX =               2000.0 / Equinox
C1YPE1  = 'GLON-TAN'           / FITS-PC alternate A: CTYPE1
C1YPE2  = 'GLAT-TAN'           / FITS-PC alternate A: CTYPE2
C1VAL1  =              101.255 / FITS-PC alternate A: Galactic lon
C1VAL2  =               16.177 / FITS-PC alternate A: Galactic lat
C1PIX1  =               50.500 / FITS-PC alternate A: CRPIX1
C1PIX2  =               50.500 / FITS-PC alternate A: CRPIX2
C1ELT1  =              -0.0100 / FITS-PC alternate A: CDELT1
C1ELT2  =               0.0100 / FITS-PC alternate A: CDELT2
END
";

#[test]
fn fits_pc_gal_reads_into_three_frame_frameset() {
    let mut fc = FitsChan::from_header(FITS_PC_GAL_HEAD).unwrap();
    fc.guess_encoding();
    let obj = fc.read().unwrap().unwrap();
    let fs = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => panic!("expected FrameSet"),
    };
    assert_eq!(fs.nframe(), 3, "expect base + primary + alt-A");

    // Frame 1 is base (GRID).
    let base = fs.frame(FrameRef::Index(1)).unwrap();
    assert_eq!(base.domain(), "GRID");
    // Frame 2 is primary (FK5 SKY); Ident=" " (or empty).
    let primary = fs.frame(FrameRef::Index(2)).unwrap();
    assert_eq!(primary.domain(), "SKY");
    // Frame 3 is alt-A (Galactic SKY); Ident="A".
    let alt = fs.frame(FrameRef::Index(3)).unwrap();
    assert_eq!(alt.domain(), "SKY");
    assert_eq!(alt.ident(), "A");
}
```

Note: the `C1YPE1` etc. naming is a FITS-PC-style alternate, not the FITS-WCS `CTYPE1A` form — they're different encodings. Use the FITS-WCS form for this test instead:

```rust
const FITS_WCS_MULTI_HEAD: &str = "\
NAXIS   =                    2
NAXIS1  =                  300
NAXIS2  =                  300
WCSAXES =                    2
CTYPE1  = 'GLON-TAN'           / Galactic longitude
CTYPE2  = 'GLAT-TAN'           / Galactic latitude
CRVAL1  =              180.000
CRVAL2  =                0.000
CRPIX1  =              150.500
CRPIX2  =              150.500
CDELT1  =             -0.01000
CDELT2  =              0.01000
WCSAXESA=                    2
CTYPE1A = 'RA---TAN'
CTYPE2A = 'DEC--TAN'
CRVAL1A =              266.405
CRVAL2A =              -28.936
CRPIX1A =              150.500
CRPIX2A =              150.500
CDELT1A =             -0.01000
CDELT2A =              0.01000
EQUINOXA=               2000.0
RADESYSA= 'FK5     '
END
";

#[test]
fn fits_wcs_multi_reads_into_three_frame_frameset() {
    let mut fc = FitsChan::from_header(FITS_WCS_MULTI_HEAD).unwrap();
    fc.guess_encoding();
    let obj = fc.read().unwrap().unwrap();
    let fs = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => panic!("expected FrameSet"),
    };
    assert_eq!(fs.nframe(), 3);
    let primary = fs.frame(FrameRef::Index(2)).unwrap();
    assert_eq!(primary.ident(), "");
    let alt = fs.frame(FrameRef::Index(3)).unwrap();
    assert_eq!(alt.ident(), "A");
}
```

(Drop the FITS-PC test at this stage — that encoding has its own `C1xxx` keyword scheme not covered by this design's reader changes.)

- [ ] **Step 6: Run the test**

```bash
cargo test -p ast-core --test test_alt_wcs_read 2>&1 | tail -5
```

Expected: PASS once Task 3.3 wires `decode_wcs_with_alts` into `FitsChan::read`. Until then, the test will fail because `FitsChan::read` still calls the primary-only `decode_wcs`. Mark this step as expected-fail-until-3.3 and move on.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_read.rs crates/ast-core/src/frames/mod.rs crates/ast-core/src/frames/basic.rs crates/ast-core/src/frames/skyframe.rs crates/ast-core/src/frames/specframe.rs crates/ast-core/src/frames/timeframe.rs crates/ast-core/src/frames/cmpframe.rs crates/ast-core/src/frames/fluxframe.rs crates/ast-core/src/frames/frameset.rs crates/ast-core/tests/test_alt_wcs_read.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_read: decode_wcs_with_alts builds alt frames

Each alt FitsStore is decoded into its own world frame and added to
the FrameSet via add_frame(FrameRef::Base, ...) so the native dump
shows Lnk{i}=1 for every alt — matching C structure. Alt frames
carry Ident=letter so the writer can recover the version letter.

Adds Frame::as_any_mut on every Frame variant.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 3.3: Wire `decode_wcs_with_alts` into FitsChan::read

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`

- [ ] **Step 1: Update the FITS-WCS read branch**

Find the FITS-WCS arm in `FitsChan::read` (search `Encoding::FitsWcs` in mod.rs around line 553-557). Change:

```rust
            Encoding::FitsWcs => {
                let store = wcs_keywords::extract_wcs_keywords(&self.cards, ' ')?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
```

to:

```rust
            Encoding::FitsWcs => {
                let (primary, alts) = wcs_keywords::extract_wcs_keywords_with_alts(&self.cards)?;
                let obj = wcs_read::decode_wcs_with_alts(&primary, &alts)?;
                Ok(Some(obj))
            }
```

The other FITS-style encodings (`FitsPc`, `FitsIraf`, `FitsAips`, `FitsAipsPP`, `FitsClass`, `Dss`) use their own per-encoder `enc_*::read_from_cards` functions which produce a single `FitsStore`. Those encodings don't standard-FITS alt-WCS keywords (e.g. FITS-PC uses its own `C1xxx` scheme), so leaving them on the single-plane `decode_wcs` path is correct. Add a comment noting this near the FITS-WCS branch:

```rust
            // Other FITS-* encodings (FitsPc, FitsIraf, FitsAips, FitsClass,
            // Dss) don't share the FITS-WCS '*A..*Z' alt-plane convention;
            // their alt-plane support lives inside their own enc_*::read
            // helpers (or doesn't exist — see encoding-specific docs).
```

- [ ] **Step 2: Run the read-side integration test**

```bash
cargo test -p ast-core --test test_alt_wcs_read 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 3: Run the full lib + integration suite**

```bash
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -5
```

Expected: every test result line `ok`. If any fixture turns red, capture which.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/mod.rs
git commit -m "$(cat <<'EOF'
fitschan: FITS-WCS read uses decode_wcs_with_alts

A multi-WCS input header now produces a FrameSet with Nframe = 2 +
nalt: base + primary current + one alt frame per detected version
letter. The non-FITS-WCS encodings keep their existing single-plane
reader path; FITS-WCS is the only encoding that uses the bare
'*A..*Z' axis suffix convention.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 3.4: Native round-trip parity

**Files:**
- New file: `crates/ast-core/tests/test_alt_wcs_round_trip.rs`

- [ ] **Step 1: Add a native-round-trip test**

```rust
//! Round-trip tests for FITS-WCS alternate planes.

use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;
use ast_core::{from_native_string, to_native_string, AnyObject};

const FITS_WCS_MULTI: &str = include_str!("fixtures/wcsconv/inputs/multi-wcs.head");

#[test]
fn fits_wcs_multi_native_round_trip_preserves_alt() {
    let mut fc = FitsChan::from_header(FITS_WCS_MULTI).unwrap();
    fc.guess_encoding();
    let obj = fc.read().unwrap().unwrap();

    // Render to native and re-parse.
    let native = to_native_string(&obj).unwrap();
    let parsed = from_native_string(&native).unwrap();

    let nframe_before = match &obj {
        AnyObject::FrameSet(fs) => fs.nframe(),
        _ => panic!("expected FrameSet"),
    };
    let nframe_after = match &parsed {
        AnyObject::FrameSet(fs) => fs.nframe(),
        _ => panic!("expected FrameSet"),
    };
    assert_eq!(nframe_before, nframe_after);
    assert!(nframe_after >= 3, "expect base + primary + ≥1 alt");

    // Round-trip through native is byte-stable.
    let native2 = to_native_string(&parsed).unwrap();
    assert_eq!(native, native2, "native round-trip must be byte-stable");
}
```

- [ ] **Step 2: Verify the fixture is present**

```bash
ls crates/ast-core/tests/fixtures/wcsconv/inputs/multi-wcs.head
```

Expected: file exists. (If not, copy from `reference/starlink-ast/ast_tester/multi-wcs.head` — see CLAUDE.md re fixtures committed under `tests/fixtures/`.)

- [ ] **Step 3: Run the round-trip test**

```bash
cargo test -p ast-core --test test_alt_wcs_round_trip 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/tests/test_alt_wcs_round_trip.rs
git commit -m "$(cat <<'EOF'
tests: native round-trip for FITS-WCS alt planes

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 3.5: Group 3 finalisation

- [ ] **Step 1: fmt + clippy + full tests**

```bash
cargo fmt -p ast-core
cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -10
```

Expected: clippy clean; all tests `ok`.

- [ ] **Step 2: Capture wcsconv state**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Record the count. The reader change alone may move some fixtures from a "wrong CTYPE" failure to a "wrong CTYPE on a different line" failure — that's expected; group 5 lands the writer. If the count regresses by more than ~5, investigate before continuing.

- [ ] **Step 3: Group commit**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: Group 3 (alt-plane FrameSets) complete

Multi-WCS headers now produce FrameSets with Nframe = 2 + nalt
matching C structure exactly. Native round-trip is bit-stable.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---
## Group 4: Writer plumbing — `s: char` through `fset_to_store`, `collect_plane_stores`

**Goal:** Add `s: char` as a parameter to `fset_to_store` (today hardcoded to `' '`), and propagate it through the helpers it calls. Add a `collect_plane_stores` helper that runs the per-plane pipeline and returns `Vec<PlaneStore>`. No encoder behaviour change yet.

### Task 4.1: Thread `s` through `fset_to_store`

**Files:**
- Modify: `crates/ast-core/src/fitschan/fset_to_store/mod.rs`
- Modify: `crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs`
- Modify: `crates/ast-core/src/fitschan/fset_to_store/spectral_axes.rs`
- Modify: `crates/ast-core/src/fitschan/fset_to_store/other_axes.rs`

**C reference:** `fitschan.c::AddVersion` takes `char s` and threads it to every call (`SetItem(... s ...)`).

- [ ] **Step 1: Update the public signature in `fset_to_store/mod.rs`**

Find the existing signature near line 75:

```rust
pub(crate) fn fset_to_store(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
) -> AstResult<FitsStore>
```

Change to:

```rust
pub(crate) fn fset_to_store(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
    s: char,
) -> AstResult<FitsStore>
```

- [ ] **Step 2: Replace hardcoded `' '` inside the function body with `s`**

Find every literal `s: ' '` or `' '` passed to a callee inside `fset_to_store`'s body (these will be calls to `celestial_axes(...)`, `spectral_axes(...)`, `other_axes(...)`):

```bash
grep -n "s: ' '" crates/ast-core/src/fitschan/fset_to_store/mod.rs
```

For each match, replace `s: ' '` with `s,` (using shorthand field init). Example before:

```rust
    let cel_seg = celestial_axes(CelestialAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s: ' ',
        store: &mut store,
        axis_done: &mut axis_done,
        isoff: 0,
    })?;
```

After:

```rust
    let cel_seg = celestial_axes(CelestialAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s,
        store: &mut store,
        axis_done: &mut axis_done,
        isoff: 0,
    })?;
```

Apply the same change to the `spectral_axes` and `other_axes` blocks. Step (b) of the post-processing section in `fset_to_store` (the `wcsname` write) also benefits from honouring `s`, but the `WCSNAME` keyword name itself isn't axis-indexed and there's only one slot in `FitsStore::wcsname`. Leave the `WCSNAME` storage alone — the per-letter suffix is applied at card emission time in Group 5.

- [ ] **Step 3: Update existing call sites**

```bash
grep -rn "fset_to_store(" crates/ast-core/src/ | head
```

Likely callers:
- `crates/ast-core/src/fitschan/wcs_write.rs::store_from_frameset_to`

Update each call to pass `' '` for now (alt-plane callers will be added in Task 4.3):

```rust
    crate::fitschan::fset_to_store::fset_to_store(fs, dim, encoding, ' ')
```

- [ ] **Step 4: Update inline tests in `fset_to_store/mod.rs`**

Search for `fset_to_store(` calls inside the `#[cfg(test)] mod tests` block at the bottom and add `, ' '` to each.

- [ ] **Step 5: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 6: Tests**

```bash
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -5
```

Expected: all `ok`. (Behaviour is unchanged — every call site passes `' '`.)

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/fitschan/fset_to_store/mod.rs crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
fitschan/fset_to_store: thread s: char through fset_to_store

The coordinate-version letter is propagated to celestial_axes /
spectral_axes / other_axes (which already accept it). Today every
caller passes ' '; alt-plane callers arrive in Task 4.3.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 4.2: Move plane-letter assignment + helper into `wcs_write.rs::plane_stores`

**Files:**
- New file: `crates/ast-core/src/fitschan/wcs_write/plane_stores.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs` (split the file)
- Modify: `crates/ast-core/src/fitschan/mod.rs` (add `wcs_write` mod path if `wcs_write` becomes a directory)

**Note:** `wcs_write.rs` is currently a single file. To add a sibling `plane_stores.rs` cleanly, convert `wcs_write.rs` into a directory `wcs_write/` containing `mod.rs` (the existing content) plus `plane_stores.rs`.

- [ ] **Step 1: Move `wcs_write.rs` to `wcs_write/mod.rs`**

```bash
mkdir -p crates/ast-core/src/fitschan/wcs_write
git mv crates/ast-core/src/fitschan/wcs_write.rs crates/ast-core/src/fitschan/wcs_write/mod.rs
```

- [ ] **Step 2: Build to confirm the rename works**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 3: Add `plane_stores.rs`**

Create `crates/ast-core/src/fitschan/wcs_write/plane_stores.rs`:

```rust
//! Per-plane FitsStore collection for FITS-WCS encoders.
//!
//! Mirrors `fitschan.c::FsetToStore` (12169-12259): given a FrameSet,
//! enumerate the primary and every alt-version frame, run the
//! `fset_to_store` pipeline once per (letter, frame), and return the
//! per-plane FitsStores ready for the encoder to emit per-axis cards
//! with the version-letter suffix.

use crate::error::AstResult;
use crate::fitschan::Encoding;
use crate::fitschan::fits_store::FitsStore;
use crate::fitschan::fset_to_store::fset_to_store;
use crate::frames::{FrameRef, FrameSet};

/// One plane in the writer's per-letter loop.
///
/// `letter == ' '` for the primary plane; `'A'..'Z'` for alt planes.
/// Plane order in the returned `Vec<PlaneStore>` is: primary first,
/// then alt planes in alphabetical letter order. The last element is
/// the plane whose **global** keyword values land in the emitted
/// header — see `FitsStore::mjdobs` doc-comment for the contract.
pub(crate) struct PlaneStore {
    pub letter: char,
    pub store: FitsStore,
}

/// Build per-plane FitsStores for every coordinate-version letter
/// the FrameSet carries.
///
/// The returned Vec begins with the primary `(letter=' ', store)`
/// and continues with alt planes in alphabetical order. Mirrors
/// fitschan.c::FsetToStore's outer loop.
pub(crate) fn collect_plane_stores(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
) -> AstResult<Vec<PlaneStore>> {
    let mut planes = Vec::new();

    // Primary plane (letter ' ').
    let primary_store = fset_to_store(fs, dim, encoding, ' ')?;
    planes.push(PlaneStore { letter: ' ', store: primary_store });

    // Alt planes — assigned via the existing
    // `frames_with_version_letters` helper (Ident-first, then A..Z
    // in frame-index order).
    let alt_assignments = super::frames_with_version_letters(fs);

    for (frame_idx_1based, letter) in alt_assignments {
        // Switch the FrameSet's current to the alt frame, run the
        // pipeline, then we discard the clone (caller's fs is
        // unchanged).
        let mut alt_fs = fs.clone();
        alt_fs.set_current(FrameRef::Index(frame_idx_1based))?;
        let alt_store = fset_to_store(&alt_fs, dim, encoding, letter)?;
        planes.push(PlaneStore { letter, store: alt_store });
    }

    Ok(planes)
}
```

- [ ] **Step 4: Wire the new module**

In `crates/ast-core/src/fitschan/wcs_write/mod.rs` (the file you just renamed), add at the very top of the file (after the existing module-level `//!` doc comment):

```rust
mod plane_stores;
pub(crate) use plane_stores::{collect_plane_stores, PlaneStore};
```

- [ ] **Step 5: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 6: Add a unit test inside `plane_stores.rs`**

At the bottom of `plane_stores.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{BasicFrameBuilder, FrameSet};
    use crate::mappings::UnitMap;

    #[test]
    fn primary_only_yields_one_plane() {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let world = BasicFrameBuilder::new(2).domain("WORLD").build().unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(world)).unwrap();
        let dim = vec![Some(100.0), Some(100.0)];
        let planes = collect_plane_stores(&fs, &dim, Encoding::FitsWcs).unwrap();
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0].letter, ' ');
    }

    #[test]
    fn alt_frame_with_ident_a_yields_two_planes() {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let primary = BasicFrameBuilder::new(2).domain("WORLD").build().unwrap();
        let mut alt = BasicFrameBuilder::new(2).domain("WORLD2").build().unwrap();
        alt.set_ident("A");

        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(primary)).unwrap();
        // Adding a second frame at base; current moves to it. Reset
        // current to primary (frame 2) before adding alt.
        fs.set_current(FrameRef::Index(2)).unwrap();
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(alt)).unwrap();
        fs.set_current(FrameRef::Index(2)).unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let planes = collect_plane_stores(&fs, &dim, Encoding::FitsWcs).unwrap();
        assert_eq!(planes.len(), 2);
        assert_eq!(planes[0].letter, ' ');
        assert_eq!(planes[1].letter, 'A');
    }
}
```

- [ ] **Step 7: Run the new tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_write::plane_stores 2>&1 | tail -10
```

Expected: 2 PASSes.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write/
git commit -m "$(cat <<'EOF'
fitschan/wcs_write: collect_plane_stores helper

Mirrors fitschan.c::FsetToStore's per-letter loop. Returns
Vec<PlaneStore> beginning with primary (' ') then alt planes in
alphabetical order. Uses the existing frames_with_version_letters
helper for letter assignment.

Splits wcs_write.rs into wcs_write/ directory; the new helper lives
in wcs_write/plane_stores.rs to keep the orchestration logic
separate from the keyword-emission body of wcs_write/mod.rs.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 4.3: Group 4 finalisation

- [ ] **Step 1: fmt + clippy + full tests**

```bash
cargo fmt -p ast-core
cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -10
```

Expected: clippy clean; all tests `ok`. wcsconv pass count is unchanged (no encoder yet uses `collect_plane_stores`).

- [ ] **Step 2: Group commit**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: Group 4 (writer plumbing) complete

fset_to_store accepts a coordinate-version letter; collect_plane_stores
is available for encoders to pivot to in Group 5.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---
## Group 5: Per-encoder retrofit + global last-wins

**Goal:** Switch every FITS-style encoder's top-level write entry from `store_from_frameset(...)` to `collect_plane_stores(...)`, emit per-axis cards with version-letter suffixes for every plane, emit global keywords once from the alphabetically-last plane. Delete the now-unreachable `_legacy_store_from_frameset_to`.

### Task 5.1: Re-shape `wcs_write::write_store_to_cards` global emission

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write/mod.rs`

`encode_wcs` already loops over alt planes (via `store_from_frameset_with_alts`). The global keywords (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS) are currently emitted from the **primary** store. Switch them to the alphabetically-last plane.

- [ ] **Step 1: Find the global-keyword emission path**

```bash
grep -n "MJD-OBS\|OBSGEO-X\|TIMESYS" crates/ast-core/src/fitschan/wcs_write/mod.rs | head -20
```

Identify the helper that emits `MJD-OBS` from the FitsStore. It will be inside `write_store_to_cards` or a sibling helper called from the FITS-WCS write path.

- [ ] **Step 2: Refactor `encode_wcs` to use `collect_plane_stores`**

Find the existing body of `encode_wcs` (around line 72 of `wcs_write/mod.rs`):

```rust
pub(crate) fn encode_wcs(
    obj: &AnyObject,
    cards: &mut CardList,
    use_cd: bool,
    dim: &[Option<f64>],
) -> AstResult<()> {
    let frameset = match obj { ... };
    let primary_crvals = extract_input_crvals(cards, ' ', frameset.base().naxes());
    let (primary, alts) = store_from_frameset_with_alts(frameset, dim, &primary_crvals, cards)?;
    write_store_to_cards(&primary, cards, use_cd, ' ', None)?;
    for alt in &alts {
        write_store_to_cards(&alt.store, cards, use_cd, alt.letter, Some(&alt.domain))?;
    }
    Ok(())
}
```

Replace the body with:

```rust
pub(crate) fn encode_wcs(
    obj: &AnyObject,
    cards: &mut CardList,
    use_cd: bool,
    dim: &[Option<f64>],
) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    let planes = collect_plane_stores(frameset, dim, Encoding::FitsWcs)?;

    // Per-axis cards: emit each plane's per-axis keywords with the
    // version-letter suffix.
    for plane in &planes {
        let domain = plane_domain_for_alt(frameset, plane.letter);
        write_store_per_axis_cards(
            &plane.store,
            cards,
            use_cd,
            plane.letter,
            domain.as_deref(),
        )?;
    }

    // Global cards: emit once, taking values from the alphabetically-
    // last plane. This mirrors C's SetItem(..., ' ', ...) overwrite at
    // fitschan.c:29917 — see FitsStore::mjdobs doc-comment.
    if let Some(last) = planes.last() {
        write_store_global_cards(&last.store, cards)?;
    }

    Ok(())
}
```

- [ ] **Step 3: Split `write_store_to_cards` into per-axis vs global halves**

The existing `write_store_to_cards(store, cards, use_cd, s, domain)` does both. Refactor into two helpers:

```rust
/// Emit per-axis (and per-plane) keywords from a FitsStore.
///
/// All keys are suffixed with `s` when `s != ' '`. Mirrors the
/// per-axis half of `fitschan.c::AddVersion`.
fn write_store_per_axis_cards(
    store: &FitsStore,
    cards: &mut CardList,
    use_cd: bool,
    s: char,
    domain: Option<&str>,
) -> AstResult<()> {
    // Move the existing axis-indexed and per-version emission code
    // here. Drop the MJD-OBS / OBSGEO / TIMESYS / DTAI / DUT1 calls.
    ...
}

/// Emit observation-global keywords (MJD-OBS, OBSGEO-{X,Y,Z},
/// TIMESYS, DTAI, DUT1) using a single FitsStore as the source.
///
/// Called once after every plane has been processed; the source
/// store is the alphabetically-last plane — last-plane-wins matches
/// fitschan.c:29917 where SkySys does
/// `SetItem(&store->mjdobs, 0, 0, ' ', ep, status)` regardless of
/// the current `s`.
fn write_store_global_cards(
    store: &FitsStore,
    cards: &mut CardList,
) -> AstResult<()> {
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }
    if let Some(x) = store.obsgeox {
        cards.set_float("OBSGEO-X", x, "[m] Observatory geocentric X");
    }
    if let Some(y) = store.obsgeoy {
        cards.set_float("OBSGEO-Y", y, "[m] Observatory geocentric Y");
    }
    if let Some(z) = store.obsgeoz {
        cards.set_float("OBSGEO-Z", z, "[m] Observatory geocentric Z");
    }
    if let Some(ref ts) = store.timesys {
        cards.set_string("TIMESYS", ts, "Time scale");
    }
    Ok(())
}

/// Helper: read the alt-plane frame's domain (used for WCSNAME{S}
/// emission). For the primary plane returns None.
fn plane_domain_for_alt(fs: &FrameSet, letter: char) -> Option<String> {
    if letter == ' ' { return None; }
    let assignments = frames_with_version_letters(fs);
    for (idx, l) in assignments {
        if l == letter {
            if let Ok(frame) = fs.frame(FrameRef::Index(idx)) {
                let d = frame.domain();
                if !d.is_empty() {
                    return Some(d.to_string());
                }
            }
            return None;
        }
    }
    None
}
```

Move the per-axis body of the existing `write_store_to_cards` into `write_store_per_axis_cards`. Keep the existing `write_store_to_cards` symbol available as a thin wrapper for any caller that still wants the old behaviour, or delete it if no caller remains.

- [ ] **Step 4: Build**

```bash
cargo build -p ast-core 2>&1 | tail -5
```

Expected: clean. (Compiler will tell you about any unused `write_store_to_cards` or symbol clashes.)

- [ ] **Step 5: Run wcsconv**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: pass count goes up. Specifically, `fits-pc-gal_fits-pc` is still red (PC encoder hasn't been retrofitted yet — Task 5.2 lands that), but `multi-wcs_fits-wcs` and `fits-pc-altgal_fits-wcs` and similar should advance significantly because the FITS-WCS encoder now sees the multi-frame FrameSet and emits both planes correctly with the right global MJD-OBS.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_write: encode_wcs uses collect_plane_stores; global last-wins

Per-axis cards emit per-plane with the version-letter suffix.
Global cards (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS) emit once from the
alphabetically-last plane — mirroring C's SetItem(..., ' ', ...)
overwrite pattern at fitschan.c:29917.

write_store_to_cards split into write_store_per_axis_cards and
write_store_global_cards.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.2: Retrofit `enc_pc::write_to_cards`

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_pc.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_write/mod.rs` (encoder dispatcher)

The non-FITS-WCS encoders go through `encode_wcs_to_store(obj, cards, dim, writer)`. Their `writer` callback consumes a single `&FitsStore`. The retrofit gives the dispatcher a per-plane variant.

- [ ] **Step 1: Add a per-plane writer signature**

In `crates/ast-core/src/fitschan/wcs_write/mod.rs`, after the existing `encode_wcs_to_store`, add:

```rust
/// Variant of `encode_wcs_to_store` that drives a per-plane writer.
///
/// The writer callback is invoked once per plane in
/// `collect_plane_stores` order — primary first, then alt planes
/// alphabetically. The encoder's writer is responsible for
/// applying the version-letter suffix to per-axis card names; the
/// final call (last plane) is also responsible for emitting the
/// global keywords.
pub(crate) fn encode_wcs_per_plane(
    obj: &AnyObject,
    cards: &mut CardList,
    dim: &[Option<f64>],
    encoding: Encoding,
    mut writer: impl FnMut(&FitsStore, &mut CardList, char, bool) -> AstResult<()>,
) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "Per-plane FITS encoding requires a FrameSet object",
            ));
        }
    };
    let planes = collect_plane_stores(frameset, dim, encoding)?;
    let last_idx = planes.len().saturating_sub(1);
    for (i, plane) in planes.iter().enumerate() {
        let is_last = i == last_idx;
        writer(&plane.store, cards, plane.letter, is_last)?;
    }
    Ok(())
}
```

The `is_last` flag tells the encoder when to emit the observation-global cards (only on the last plane).

- [ ] **Step 2: Update `enc_pc::write_to_cards` signature**

In `crates/ast-core/src/fitschan/enc_pc.rs`, change:

```rust
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
```

to:

```rust
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
```

Inside the body, replace every `format!("CTYPE{}", i + 1)` style construction with:

```rust
    let key = super::wcs_keywords::key_for("CTYPE", Some(i + 1), s);
```

(Same for CRPIX, CRVAL, CDELT, CUNIT, CNAME, PC{i}_{j}, CD{i}_{j}, PV{i}_{m}, RADESYS / EQUINOX / LONPOLE / LATPOLE / WCSAXES / WCSNAME.)

For the global keys (MJD-OBS, OBSGEO-X/Y/Z, TIMESYS, DATE-OBS), guard each emit on `is_last`:

```rust
    if is_last {
        if let Some(mjdobs) = store.mjdobs {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
        // ...same for OBSGEO-X/Y/Z, TIMESYS, DATE-OBS as before.
    }
```

- [ ] **Step 3: Update the FitsChan dispatch**

In `crates/ast-core/src/fitschan/mod.rs`, the FITS-PC write arm:

```rust
                    Encoding::FitsPc => wcs_write::encode_wcs_to_store(
                        obj,
                        &mut self.cards,
                        &dims,
                        enc_pc::write_to_cards,
                    ),
```

Change to:

```rust
                    Encoding::FitsPc => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsPc,
                        enc_pc::write_to_cards,
                    ),
```

- [ ] **Step 4: Update enc_pc unit tests**

Search `enc_pc::write_to_cards` calls inside the `#[cfg(test)] mod tests` block of `enc_pc.rs` and update each call to pass `' '` and `true`:

```rust
    enc_pc::write_to_cards(&store, &mut cards, ' ', true).unwrap();
```

- [ ] **Step 5: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 6: Run wcsconv**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: `fits-pc-gal_fits-pc` and `fits-pc-altgal_*` advance. Pass count goes up.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_pc.rs crates/ast-core/src/fitschan/wcs_write/mod.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/enc_pc: per-plane FITS-PC writer

write_to_cards now takes (s: char, is_last: bool); per-axis keys
get suffixed with s, global keys (MJD-OBS, OBSGEO, TIMESYS,
DATE-OBS) emit only when is_last. FitsChan dispatch switches to
encode_wcs_per_plane.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.3: Retrofit `enc_iraf::write_to_cards`

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_iraf.rs`
- Modify: `crates/ast-core/src/fitschan/mod.rs`

Same shape as Task 5.2. Walk through every `format!("...{i}")` keyword construction in `enc_iraf.rs` and route through `key_for` / `key_for_2`. Guard global emission on `is_last`. Switch the FitsChan dispatch to `encode_wcs_per_plane`.

- [ ] **Step 1: Update enc_iraf::write_to_cards signature and body**

```rust
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
```

Inside the body, replace every `format!("CRPIX{}", i + 1)` style with `super::wcs_keywords::key_for("CRPIX", Some(i + 1), s)` and similarly for CRVAL, CTYPE, CDELT, CUNIT, RADECSYS, EQUINOX. For the IRAF CD-matrix path inside `write_iraf_cd_matrix`, the helper also needs `s`. Update its signature:

```rust
fn write_iraf_cd_matrix(store: &FitsStore, cards: &mut CardList, s: char) {
    // existing body, but replace `format!("CD{}_{}", i + 1, j + 1)`
    // with `super::wcs_keywords::key_for_2("CD", i + 1, j + 1, s)`.
}
```

- [ ] **Step 2: Guard MJD-OBS / DATE-OBS emission**

The existing body has:

```rust
    if cards.find("MJD-OBS").is_some()
        && let Some(mjdobs) = store.mjdobs { ... }
    if let Some(ref dateobs) = store.dateobs { ... }
```

Wrap both blocks in `if is_last { ... }`.

- [ ] **Step 3: Switch the FitsChan dispatch**

In `crates/ast-core/src/fitschan/mod.rs`:

```rust
                    Encoding::FitsIraf => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsIraf,
                        enc_iraf::write_to_cards,
                    ),
```

- [ ] **Step 4: Update enc_iraf unit tests**

Add `, ' ', true` to every test-only `write_to_cards` call.

- [ ] **Step 5: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

- [ ] **Step 6: Run wcsconv**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: `*_fits-iraf` fixtures advance.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_iraf.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/enc_iraf: per-plane FITS-IRAF writer

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.4: Retrofit `enc_aips::write_to_cards` and `write_to_cards_pp`

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`
- Modify: `crates/ast-core/src/fitschan/mod.rs`

Both AIPS variants take the same `(store, cards)` shape today. Apply the same retrofit to each.

- [ ] **Step 1: Update both signatures**

```rust
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
    ...
}

pub(crate) fn write_to_cards_pp(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
    ...
}
```

- [ ] **Step 2: Replace per-axis key builders with `key_for` / `key_for_2`**

Same pattern as 5.2/5.3 across both bodies. AIPS-specific keys to handle: CTYPE, CRPIX, CRVAL, CDELT, CROTA{i}, EPOCH, EQUINOX, RADECSYS, BUNIT, RESTFREQ, VELREF, BMAJ, BMIN, BPA, OBSGEO-X/Y/Z. The CROTA path is per-version (`CROTA{i}{S}`), the rest of the BMAJ/BMIN/BPA/OBSGEO/etc. are observation-global — guard on `is_last`.

- [ ] **Step 3: Switch the FitsChan dispatch for both encodings**

```rust
                    Encoding::FitsAips => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAips,
                        enc_aips::write_to_cards,
                    ),
                    Encoding::FitsAipsPP => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAipsPP,
                        enc_aips::write_to_cards_pp,
                    ),
```

- [ ] **Step 4: Update enc_aips unit tests**

Add `, ' ', true` to every test-only call.

- [ ] **Step 5: Build + wcsconv**

```bash
cargo build -p ast-core 2>&1 | tail -3
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: `*_fits-aips` and `*_fits-aips++` fixtures advance.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_aips.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/enc_aips: per-plane FITS-AIPS / FITS-AIPS++ writers

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.5: Retrofit `enc_class::write_to_cards`

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_class.rs`
- Modify: `crates/ast-core/src/fitschan/mod.rs`

Same retrofit pattern.

- [ ] **Step 1: Update signature and body**

```rust
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
    ...
}
```

Replace per-axis key builders with `key_for` / `key_for_2`. Guard global emission (TIMESYS, DATE-OBS, MJD-OBS, OBSGEO, RESTFREQ, etc.) on `is_last`.

- [ ] **Step 2: Switch the FitsChan dispatch**

```rust
                    Encoding::FitsClass => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsClass,
                        enc_class::write_to_cards,
                    ),
```

- [ ] **Step 3: Update enc_class unit tests**

Add `, ' ', true`.

- [ ] **Step 4: Build + wcsconv**

```bash
cargo build -p ast-core 2>&1 | tail -3
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_class.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/enc_class: per-plane FITS-CLASS writer

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.6: Delete legacy `_legacy_store_from_frameset_to`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write/mod.rs`

- [ ] **Step 1: Confirm zero callers**

```bash
grep -n "_legacy_store_from_frameset_to" crates/ast-core/src/
```

Expected: only the definition site. If anything else references it, investigate before deleting.

- [ ] **Step 2: Delete the function and its private helpers**

In `wcs_write/mod.rs`, delete:
- `_legacy_store_from_frameset_to` body
- Any `#[allow(dead_code)]` helpers it was the sole caller of
  (`split_pre_wcsmap`, `decompose_linear` callsites that aren't used
  by `find_basis_vectors`, `solve_crpix_from_decomp`, `fiducial_crval`
  if no other caller remains).

The Group 7 plan in `2026-05-25-makeintworld-port.md::Group 7 Task 7.2` already lists this — re-use that task's deletion list as the canonical reference.

- [ ] **Step 3: Build + clippy + tests**

```bash
cargo build -p ast-core 2>&1 | tail -3
cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -5
```

Expected: clean clippy, all tests `ok`.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_write: delete legacy CRPIX/CD extraction helpers

_legacy_store_from_frameset_to and its private helpers
(split_pre_wcsmap, solve_crpix_from_decomp, fiducial_crval, the
chosen-CRVAL cascade) become unreachable once every encoder uses
collect_plane_stores. Removed outright.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.7: Group 5 finalisation

- [ ] **Step 1: fmt + clippy + full tests**

```bash
cargo fmt -p ast-core
cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -10
```

Expected: clippy clean; all non-wcsconv tests `ok`.

- [ ] **Step 2: Capture wcsconv state**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results" | tee /tmp/wcsconv_after_g5.txt
```

Record the count. Group 6 will walk the residuals.

- [ ] **Step 3: Group commit**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: Group 5 (per-encoder retrofit + global last-wins) complete

Every FITS-* encoder now emits per-plane axis cards with the version-
letter suffix. Global cards (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS,
DATE-OBS, etc.) emit once from the alphabetically-last plane,
matching C's SetItem(..., ' ', ...) overwrite at fitschan.c:29917.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---
## Group 6: DTAI/DUT1 storage + wcsconv validation

**Goal:** Add `dtai` / `dut1` to FitsStore and the corresponding read/write paths so the `*` last-plane-wins emission can carry these globals. Then walk the wcsconv residual list and fix bucket-A (numeric value) failures one at a time.

### Task 6.1: Add `dtai` / `dut1` to FitsStore

**Files:**
- Modify: `crates/ast-core/src/fitschan/fits_store.rs`

- [ ] **Step 1: Add the fields**

In `crates/ast-core/src/fitschan/fits_store.rs`, near the existing `mjdobs` / `timesys` fields (around line 46):

```rust
    /// TAI - UTC offset (seconds). Mirrors the FITS DTAI keyword.
    /// Global per-FrameSet field; emit and read once per header.
    /// See [`FitsStore::mjdobs`] for the alt-plane last-wins
    /// contract.
    pub dtai: Option<f64>,

    /// UT1 - UTC offset (seconds). Mirrors the FITS DUT1 keyword.
    /// Global per-FrameSet field; emit and read once per header.
    pub dut1: Option<f64>,
```

In the `FitsStore::new` constructor (around line 142), add:

```rust
            dtai: None,
            dut1: None,
```

Add a doc-comment block on `mjdobs` (and the other globals) explaining the alt-plane contract. From the spec:

```rust
    /// Modified Julian Date of the observation (FITS MJD-OBS).
    ///
    /// Global per-FrameSet field. When alt-WCS planes are present,
    /// the writer's last-plane-wins rule (alphabetical order)
    /// chooses which plane's value lands in the emitted MJD-OBS
    /// card. Mirrors C's `SetItem(&store->mjdobs, 0, 0, ' ', ...)`
    /// at fitschan.c:29917 which forces s=' ' regardless of which
    /// coordinate-version SkySys is processing.
    pub mjdobs: Option<f64>,
```

(Same shape for `obsgeox`, `obsgeoy`, `obsgeoz`, `timesys`, `dtai`, `dut1`. Cross-link via `[`FitsStore::mjdobs`]` to keep duplication minimal.)

- [ ] **Step 2: Build**

```bash
cargo build -p ast-core 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/src/fitschan/fits_store.rs
git commit -m "$(cat <<'EOF'
fitschan/fits_store: add dtai / dut1 globals

Both are observation-global fields (TAI-UTC offset and UT1-UTC
offset), follow the same last-plane-wins contract as MJD-OBS /
OBSGEO / TIMESYS — see FitsStore::mjdobs doc-comment.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 6.2: Wire DTAI/DUT1 read

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs`

- [ ] **Step 1: Read DTAI / DUT1 in `extract_wcs_keywords`**

In `crates/ast-core/src/fitschan/wcs_keywords.rs`, locate the scalar-keyword block in `extract_wcs_keywords` (after `read_epoch_into_store`). Add:

```rust
    store.dtai = lookup_with_fallback(cards, "DTAI", None, s, |c, k| c.get_float(k));
    store.dut1 = lookup_with_fallback(cards, "DUT1", None, s, |c, k| c.get_float(k));
```

(Both keys are observation-global per the FITS standard, but C still admits an `s`-suffixed version. Use the same primary-fallback semantics for symmetry.)

- [ ] **Step 2: Stamp DTAI / DUT1 onto the SkyFrame in `build_celestial`**

In `crates/ast-core/src/fitschan/wcs_celestial.rs`, find the block where `set_epoch` / `set_equinox` are called on `sky_frame`. After those, add:

```rust
    if let Some(dtai) = store.dtai {
        sky_frame.set_dtai(dtai);
    }
    if let Some(dut1) = store.dut1 {
        sky_frame.set_dut1(dut1);
    }
```

(`SkyFrame::set_dtai` / `set_dut1` already exist — see `crates/ast-core/src/frames/skyframe.rs:440` and `:445`.)

- [ ] **Step 3: Add a unit test**

In `wcs_celestial.rs` tests:

```rust
    #[test]
    fn dtai_dut1_round_trip_into_skyframe() {
        let mut store = FitsStore::new(2);
        store.naxis = 2;
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);
        store.equinox = Some(2000.0);
        store.radesys = Some("FK5".to_string());
        store.dtai = Some(37.0);
        store.dut1 = Some(0.3);

        let result = build_celestial(&store).expect("celestial recognised");
        assert_eq!(result.frame.dtai(), Some(37.0));
        assert_eq!(result.frame.dut1(), Some(0.3));
    }
```

- [ ] **Step 4: Run**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests::dtai_dut1_round_trip_into_skyframe 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs crates/ast-core/src/fitschan/wcs_celestial.rs
git commit -m "$(cat <<'EOF'
fitschan: read DTAI / DUT1 from header into SkyFrame

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 6.3: Wire DTAI/DUT1 write (per encoder)

**Files:**
- Modify: `crates/ast-core/src/fitschan/fset_to_store/sky_sys.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_write/mod.rs`
- Modify: `crates/ast-core/src/fitschan/enc_pc.rs`
- Modify: `crates/ast-core/src/fitschan/enc_iraf.rs`
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`
- Modify: `crates/ast-core/src/fitschan/enc_class.rs`

- [ ] **Step 1: Populate `store.dtai` / `store.dut1` inside `sky_sys`**

In `crates/ast-core/src/fitschan/fset_to_store/sky_sys.rs`, find the section that writes `MJD-OBS` (search `store.mjdobs = Some`). Add:

```rust
    if let Some(dtai) = skyfrm.dtai() {
        store.dtai = Some(dtai);
    }
    if let Some(dut1) = skyfrm.dut1() {
        store.dut1 = Some(dut1);
    }
```

- [ ] **Step 2: Emit DTAI / DUT1 in `write_store_global_cards`**

In `crates/ast-core/src/fitschan/wcs_write/mod.rs::write_store_global_cards`, after the `TIMESYS` emission:

```rust
    if let Some(dtai) = store.dtai {
        cards.set_float("DTAI", dtai, "[s] TAI - UTC");
    }
    if let Some(dut1) = store.dut1 {
        cards.set_float("DUT1", dut1, "[s] UT1 - UTC");
    }
```

- [ ] **Step 3: Emit DTAI / DUT1 in each per-encoder write_to_cards**

In each of `enc_pc.rs`, `enc_iraf.rs`, `enc_aips.rs` (both variants), `enc_class.rs`, inside the `if is_last { ... }` global block, add the same two `cards.set_float(...)` calls.

- [ ] **Step 4: Build + tests**

```bash
cargo build -p ast-core 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED" | tail -5
```

Expected: clean.

- [ ] **Step 5: Run wcsconv**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Record the new count.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/
git commit -m "$(cat <<'EOF'
fitschan: emit DTAI / DUT1 globals per last-plane-wins rule

sky_sys populates store.dtai / store.dut1 from the SkyFrame; every
encoder emits them once on the alphabetically-last plane.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 6.4: Walk wcsconv residuals, classify

- [ ] **Step 1: Capture the residual list**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 \
  | grep -E "differs:|line count|write:" > /tmp/wcsconv_residuals.txt
wc -l /tmp/wcsconv_residuals.txt
```

- [ ] **Step 2: Classify each entry**

For every line in `/tmp/wcsconv_residuals.txt`, classify into:

- **A. Numeric value mismatch** (e.g. `CRPIX1 = 50.499998` vs `50.5`) → fix in Task 6.5.
- **B. Structural failure (out of scope)** (e.g. `dss_*`, `multi-wcs-dup_fits-wcs` if RADESYSA card ordering, `degen1_fits-wcs` WCSAXES feature) → leave red, document.
- **C. Formatting only (out of scope)** (e.g. `tan-pv0_fits-wcs` `-0.001` vs `-1.0E-3`) → leave red, document.
- **D. Comment text differs (out of scope)** (e.g. `vopt-geo_fits-aips` line 15 `Channel width (m/s)` vs `Pixel size`) → leave red, document.

### Task 6.5: Fix bucket-A residuals one at a time

For every fixture classified as bucket A:

- [ ] **Step 1: Run the failing fixture in isolation**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -B0 -A2 "<fixture_name>" | head -10
```

- [ ] **Step 2: Build a tiny C oracle for the same input**

Pattern: copy a short `tools/oracle_fixtures/` program (any of the existing ones is a fine template), modify to read the fixture's `.head` and dump the relevant computed values. Run the C oracle, capture the values.

- [ ] **Step 3: Add `eprintln!` traces in the matching Rust code path**

For each diverging value, place a temporary `eprintln!` at the point of divergence — `celestial_axes`, `spectral_axes`, `make_int_world::find_basis_vectors`, `sip_int_world`, etc. depending on which keyword diverged.

- [ ] **Step 4: Identify the divergence**

Per the spec's `Group 8: Landing fixes` checklist (in `2026-05-25-makeintworld-port.md`): the divergence is always one of:

- `celestial_axes` chose a different CRVAL.
- `spectral_axes` picked a different algorithm code or VELOSYS.
- `other_axes` had wrong axis identification.
- `make_int_world` got a different basis g0.
- `sip_int_world` extracted different SIP coeffs.

In every case the fix is to read the C source for the diverging step and verify the Rust matches line-for-line. **No tolerance constants. No "round to fix it" workarounds.**

- [ ] **Step 5: Apply the fix; remove the `eprintln!` traces**

- [ ] **Step 6: Re-run the full wcsconv suite**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: pass count goes up by 1 (the fixture you fixed) and no other fixture regresses.

- [ ] **Step 7: Commit per fixture**

```bash
git commit -am "$(cat <<'EOF'
fitschan/<area>: fix <fixture_name> <line>

Mirrors fitschan.c:<line>. <one-sentence root-cause>.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

Repeat steps 1-7 for every bucket-A residual.

### Task 6.6: Group 6 finalisation

- [ ] **Step 1: Final fmt + clippy + full test suite**

```bash
cargo fmt -p ast-core
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p ast-core 2>&1 | grep -E "test result|FAILED"
```

Expected: every test result `ok`. Any FAILED is a regression to investigate.

- [ ] **Step 2: Final wcsconv check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: pass count ≥ 170/192.

- [ ] **Step 3: Retire any stale memory notes**

Check `/sdf/home/t/timj/.claude/projects/-sdf-home-t-timj-WORK-starlink-astrs/memory/` for notes superseded by this work. Likely candidates:

- `project_crpix_blockers.md` — the two issues described there are out of date as of the make_int_world port and the alt-WCS work.

If a note is stale, edit or delete it (and update `MEMORY.md` index).

- [ ] **Step 4: Final summary commit**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: Alternate-WCS support complete

Reader: multi-WCS headers parse into FrameSets with Nframe = 2 +
nalt; alt frames carry Ident=letter and are wired Lnk{i}=1
matching C structure exactly.

Writer: every FITS-* encoder loops version letters and emits
per-axis cards with the appropriate suffix. Global cards
(MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DTAI, DUT1) emit once from the
alphabetically-last plane — last-plane-wins matches fitschan.c
SetItem(..., ' ', ...) overwrite semantics.

DTAI / DUT1 storage added to FitsStore.

wcsconv: <N>/192 passing.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Closing notes

The plan is large; treat each commit group as a reviewable unit. If a group's finalisation step shows regressions, fix them before moving to the next group.

When in doubt about C semantics, read `fitschan.c` again. The C library has 26-letter-aware `FitsStore` matrices behind every per-axis lookup; our Rust port simulates that with a `Vec<(char, FitsStore)>` of separate stores plus a primary-fallback rule on read. The two are semantically equivalent for the keyword set we touch.

Don't change the `FrameSet` integrity contract. Don't introduce locks or allocation into hot paths. Mirror C control flow line-for-line in the new helpers.
