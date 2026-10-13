# H1 — Alt-WCS plane emission

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit FITS-WCS alternate axis-description planes (`WCSAXESA`, `CRPIX1A`, `CTYPE1A`, `CD1_1A`, etc.) for every non-base/non-current frame in the FrameSet. Mirrors C library `FsetToStore` (fitschan.c:12257-12347) + `AddVersion` (fitschan.c:2737).

**Architecture:** Extend `wcs_write::store_from_frameset` to compute one `FitsStore` per non-base/non-current frame in addition to the primary. Each alt store carries a version letter (`A`-`Z`). The shared write path (`write_store_to_cards`) and the per-encoder write paths get an `s: char` parameter that suffixes axis-indexed keywords. After the primary plane is emitted, loop the alt planes and emit each.

The C library encodes "is this an alt plane" by storing per-version data in 27-element arrays (one slot per `' '`..`'Z'`). We use a more idiomatic Rust shape: a `Vec<(char, FitsStore)>` of alt planes alongside the primary.

**Tech Stack:** Rust 2024, existing FitsChan infrastructure. Adds a `domain()` accessor to the `Frame` trait and a small `AltPlane` carrier struct.

---

## File structure

- **Modify:** `crates/ast-core/src/frames/mod.rs` — add `domain()` and `ident()` default methods to `Frame` trait (return `""` by default).
- **Modify:** `crates/ast-core/src/frames/basic.rs` — override `domain()` and `ident()` to expose the existing fields.
- **Modify:** `crates/ast-core/src/frames/skyframe.rs`, `specframe.rs`, `cmpframe.rs`, etc. — same overrides where the field exists. Default `""` is fine for frames that don't carry the attribute.
- **Modify:** `crates/ast-core/src/fitschan/wcs_write.rs` — split `encode_wcs` so it produces (primary store, Vec<(char, FitsStore)>) and writes both. Add helper `frames_with_version_letters(fs) -> Vec<(usize, char)>` mirroring fitschan.c:12260-12318 logic.
- **Modify:** `crates/ast-core/src/fitschan/wcs_write.rs::write_store_to_cards` — add a `s: char` parameter that suffixes axis-indexed keywords. `' '` means primary (no suffix); `'A'`-`'Z'` is alt.
- **Modify:** Per-encoder write paths (`enc_iraf.rs`, `enc_pc.rs`, `enc_aips.rs`, `enc_class.rs`) — same `s: char` parameter. Or accept that alt planes only apply to FITS-WCS encoding (the C library does the same — `AddVersion` is only called from `FitsFromStore`, not from `IRAFFromStore` etc.). **Decision: only FITS-WCS emits alt planes** (Task 5 confirms this from the C source).
- **Add:** A new pub(crate) struct `AltPlane { letter: char, frame_name: String, store: FitsStore }` in `wcs_write.rs`.

---

## Background — C library reference

### FsetToStore loop (fitschan.c:12257-12347)

After the primary plane is built (current Frame as WCS), a second loop walks all other frames:

```c
// Skip current, base, and IWC-domain frames.
// Step 1: Use Ident attribute (if single uppercase letter) as version letter.
// Step 2: Assign remaining letters A..Z in order (skipping already-used).
// Step 3: For each (frame, letter) pair, call AddVersion(this, fset, ibase, ifrm, store, dim, letter, ...)
```

### AddVersion (fitschan.c:2737)

This re-runs the entire `MakeIntWorld` extraction for the (base → ifrm) mapping. The store's per-version arrays get populated for that letter. The encoder write path then emits the per-version cards using `FormatKey(name, i, j, s, status)` which appends the version letter to axis-indexed keywords.

### Per-version-letter cards in the output

| Card | Per-version | Notes |
|---|---|---|
| `WCSAXES{S}` | yes | Number of WCS axes in this plane |
| `WCSNAME{S}` | yes | Frame domain (e.g. "PIXEL", "AXIS") |
| `CRPIX{i}{S}` | yes | Reference pixel |
| `CRVAL{i}{S}` | yes | Reference value |
| `CTYPE{i}{S}` | yes | Axis type |
| `CD{i}_{j}{S}` | yes | CD matrix |
| `PC{i}_{j}{S}` | yes | PC matrix |
| `CDELT{i}{S}` | yes | Pixel scale |
| `CUNIT{i}{S}` | yes | Axis units |
| `PV{i}_{m}{S}` | yes | Projection parameters |
| `MJD-OBS` | NO | Plane-shared (one observation epoch per FrameSet) |
| `DATE-OBS` | NO | Plane-shared |
| `EQUINOX{S}` | yes (if frame has equinox) | E.g. EQUINOX or EQUINOXA |
| `RADESYS{S}` | yes (if frame has reference frame) | E.g. RADESYS or RADESYSA |
| `LONPOLE{S}`, `LATPOLE{S}` | yes (if non-default) | |
| `RESTFRQ{S}` | yes (if spectral) | |
| `SPECSYS{S}` | yes (if spectral) | |

Tests this should unblock (per roadmap):
- `timj_fits-wcs` — has PIXEL and AXIS frames as alt planes A and B
- `multi-wcs_fits-wcs` — has multiple WCS frames
- `multi-wcs-dup_fits-wcs` — has duplicate frames
- `cname_fits-wcs` — needs CNAME emission, ties into alt planes
- `tan-pv0_fits-wcs` — PV1_0 not preserved; may also tie into alt planes if PV is per-version
- `date-z_fits-wcs` — input has DATE-OBS, related but separate

Estimate: **+5 to +7** wcsconv passes.

---

## Hard rules

1. **Primary plane behavior must not change.** All currently-passing tests must continue to pass byte-for-byte. If the primary write logic refactors, do it without changing emitted-card content.
2. **IWC-domain frames are skipped.** If a frame's domain matches `"IWC"` (any axis), it gets no version letter and no plane.
3. **Letter assignment matches C order.** First pass: use `frame.ident()` if it's a single uppercase letter. Second pass: assign remaining letters `A`, `B`, `C`, ... in frame-index order.
4. **Plane-shared cards (MJD-OBS, DATE-OBS) are emitted ONCE only**, on the primary plane. Don't duplicate them per alt plane.
5. **Only FITS-WCS encoding emits alt planes.** AIPS, IRAF, PC, CLASS, DSS only get the primary plane. (Verify this against `IRAFFromStore` etc. in the C library — none of them call `AddVersion`.)

---

## Task 1: Add `domain()` and `ident()` to the Frame trait

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/frames/basic.rs`
- Modify: `crates/ast-core/src/frames/skyframe.rs`

- [ ] **Step 1: Add the trait methods**

In `crates/ast-core/src/frames/mod.rs`, find the `Frame` trait (around line 37). Add after the existing default methods:

```rust
    /// Frame domain, e.g. "GRID", "PIXEL", "SKY", "IWC". Empty string if unset.
    fn domain(&self) -> &str {
        ""
    }

    /// Frame identification token (single uppercase letter recommended for
    /// FITS-WCS alt-plane assignment). Empty string if unset.
    fn ident(&self) -> &str {
        ""
    }
```

- [ ] **Step 2: Override on BasicFrame**

In `basic.rs`, locate the existing `pub fn domain(&self) -> &str { ... }` (around line 218). It's currently an inherent method. Either:
- Add a separate trait impl method that calls the inherent: `fn domain(&self) -> &str { self.domain.as_deref().unwrap_or("") }` — but only if there's no name clash; or
- Rename the inherent method or expose via the trait directly.

Look at the exact existing implementation to decide. The cleanest path: rename the inherent `domain(&self)` so the trait impl can take its name. Or simpler: have the trait method delegate to the inherent. If both exist with the same signature, Rust's trait-method-shadowing-inherent works without conflict.

Add `ident()` similarly.

- [ ] **Step 3: Override on SkyFrame**

Same pattern in `skyframe.rs` (around line 244 has the existing inherent `domain(&self)`).

- [ ] **Step 4: Build & test**

```bash
cargo build -p ast-core
cargo test -p ast-core --lib frames -- --nocapture
```

If any frame implementation fails to compile (because the trait now requires `domain` / `ident` and the type doesn't have a sensible source), it can fall through to the trait default `""`. The trait defaults already handle this — concrete types only override when they have a real value to expose.

- [ ] **Step 5: Clippy / fmt**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/
git commit -m "$(cat <<'EOF'
feat(frames): add domain() and ident() to Frame trait

Default empty-string methods on the Frame trait, overridden on
BasicFrame and SkyFrame to expose existing fields. Needed by
FitsChan's H1 (alt-WCS plane emission) so it can identify which
non-base/non-current frames in a FrameSet are eligible for an
alternate axis description, and read their FITS Ident hint.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 2: Frame-to-version-letter assignment helper

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

Mirrors the loop in fitschan.c:12260-12318. Returns a list of `(frame_idx, version_letter)` pairs for non-base/non-current frames.

- [ ] **Step 1: Write a unit test first (TDD)**

In `wcs_write.rs::tests`:

```rust
    #[test]
    fn frames_with_version_letters_skips_base_and_current() {
        // Build a FrameSet with 4 frames: GRID(base), SKY(current), PIXEL, AXIS.
        // Expect alt-plane assignment: PIXEL → A, AXIS → B.
        // (Exact construction depends on FrameSet API; placeholder pseudo-test.)
        // Verify: the helper returns 2 entries, in frame-index order, with letters A and B.
    }

    #[test]
    fn frames_with_version_letters_skips_iwc_domain() {
        // FrameSet with GRID(base), SKY(current), and an IWC-domain frame.
        // Expect: no alt planes (IWC frame skipped, base/current skipped).
    }

    #[test]
    fn frames_with_version_letters_uses_ident_first() {
        // FrameSet where one extra frame has ident="C". Expect that frame
        // gets letter C, others get A, B, etc. (skipping C in the auto pass).
    }
```

(The implementer should look at how existing FrameSet tests are written in the codebase and follow the same construction pattern. The pseudo-tests above are just specifications.)

- [ ] **Step 2: Implement the helper**

In `wcs_write.rs`:

```rust
/// Identify which frames in the FrameSet are eligible for FITS alt-plane
/// emission, and assign each a version letter ('A'..='Z').
///
/// Mirrors the loop at fitschan.c:12260-12318:
///   - Skip base and current frames (they're emitted as the primary plane).
///   - Skip frames with domain "IWC" (intermediate world coords; not externalised).
///   - First pass: if a frame's `ident()` is a single uppercase letter and
///     not yet used, use it.
///   - Second pass: assign remaining letters A, B, C, ... in frame-index
///     order, skipping any already used in the first pass.
///
/// Returns a Vec of (1-based frame index, version letter) pairs.
pub(crate) fn frames_with_version_letters(fs: &FrameSet) -> Vec<(usize, char)> {
    let nframe = fs.nframe();
    let base_idx = fs.base_idx() + 1; // 1-based to match C
    let curr_idx = fs.current_idx() + 1;

    let mut used = [false; 26]; // index 0 = 'A', 25 = 'Z'
    let mut assignment: Vec<(usize, Option<char>)> = Vec::new();

    // First pass: explicit Ident.
    for ifrm in 1..=nframe {
        if ifrm == base_idx || ifrm == curr_idx {
            continue;
        }
        let frame = fs.frame(FrameRef::Index(ifrm)).expect("frame in range");
        if frame.domain().eq_ignore_ascii_case("IWC") {
            continue;
        }
        let ident = frame.ident();
        let letter = if ident.len() == 1 {
            let c = ident.chars().next().unwrap();
            if c.is_ascii_uppercase() && !used[(c as u8 - b'A') as usize] {
                used[(c as u8 - b'A') as usize] = true;
                Some(c)
            } else {
                None
            }
        } else {
            None
        };
        assignment.push((ifrm, letter));
    }

    // Second pass: assign remaining letters in order.
    let mut next = b'A';
    for (_, opt_letter) in assignment.iter_mut() {
        if opt_letter.is_none() {
            while next <= b'Z' && used[(next - b'A') as usize] {
                next += 1;
            }
            if next > b'Z' {
                break; // out of letters; remaining frames don't get planes
            }
            *opt_letter = Some(next as char);
            used[(next - b'A') as usize] = true;
            next += 1;
        }
    }

    assignment
        .into_iter()
        .filter_map(|(idx, opt)| opt.map(|c| (idx, c)))
        .collect()
}
```

`fs.base_idx()` / `fs.current_idx()` may need to be added to the public API of FrameSet if they aren't already exposed. Check first; if missing, add `pub(crate) fn base_idx(&self) -> usize` and `current_idx(&self) -> usize` returning 0-based indices.

- [ ] **Step 3: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_write::tests::frames_with_version_letters
```

3 tests pass.

- [ ] **Step 4: Clippy / fmt clean. Commit.**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs crates/ast-core/src/frames/frameset.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): assign FITS alt-plane version letters per FrameSet frame

Mirrors fitschan.c::FsetToStore lines 12172-12230. Skip base, current,
and IWC-domain frames. First pass uses Ident-as-uppercase-letter when
available; second pass assigns remaining letters in order.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 3: Compute one FitsStore per alt plane

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

The current `store_from_frameset` builds the (base → current) plane. We need a generalized version that takes a target frame index.

- [ ] **Step 1: Refactor `store_from_frameset` into a more general entry**

Rename the existing function to `store_from_frameset_to(fs, target_idx)`. The current behavior corresponds to `target_idx = fs.current_idx()`. Add a wrapper:

```rust
pub(crate) fn store_from_frameset(fs: &FrameSet) -> AstResult<FitsStore> {
    store_from_frameset_to(fs, fs.current_idx())
}

pub(crate) fn store_from_frameset_to(fs: &FrameSet, target_idx: usize) -> AstResult<FitsStore> {
    // existing body, but instead of `FrameRef::Current`, use `FrameRef::Index(target_idx + 1)`
    // wherever the function currently maps base → current.
    // ...
}
```

In the body, replace every `FrameRef::Current` with the supplied target reference. Specifically:
- The `fs.mapping(FrameRef::Base, FrameRef::Current)?.simplify()?` call.
- Anywhere else that consults the "current" frame — e.g. for axis names, frame domain, etc.

- [ ] **Step 2: Compute the alt-plane stores**

Add a new top-level function:

```rust
/// Build the primary FitsStore plus per-frame alt planes.
///
/// Returns (primary_store, alt_planes) where alt_planes is a Vec of
/// (version_letter, frame_domain, alt_store). Alt planes are computed
/// using the same store_from_frameset_to logic with each non-base/
/// non-current frame as the target.
///
/// Mirrors fitschan.c::FsetToStore (line 12169-12259).
pub(crate) fn store_from_frameset_with_alts(
    fs: &FrameSet,
) -> AstResult<(FitsStore, Vec<AltPlane>)> {
    let primary = store_from_frameset(fs)?;
    let assignments = frames_with_version_letters(fs);

    let mut alts = Vec::with_capacity(assignments.len());
    for (frame_idx_1based, letter) in assignments {
        let target_idx = frame_idx_1based - 1;
        let alt_store = store_from_frameset_to(fs, target_idx)?;
        let frame = fs.frame(FrameRef::Index(frame_idx_1based))?;
        let domain = frame.domain().to_string();
        alts.push(AltPlane {
            letter,
            domain,
            store: alt_store,
        });
    }

    Ok((primary, alts))
}

/// One FITS alt-WCS plane: a version letter (A-Z), the frame domain
/// (used to populate WCSNAME{S}), and a FitsStore holding axis-indexed
/// values for that plane.
pub(crate) struct AltPlane {
    pub(crate) letter: char,
    pub(crate) domain: String,
    pub(crate) store: FitsStore,
}
```

- [ ] **Step 3: Verify build + existing tests still pass**

```bash
cargo build -p ast-core
cargo test --workspace 2>&1 | grep -cE "FAILED"
```

`store_from_frameset` is unchanged in observable behavior; existing tests should remain green.

- [ ] **Step 4: Add a unit test for alt-plane computation**

```rust
    #[test]
    fn store_from_frameset_with_alts_emits_two_alts_for_timj() {
        // Load timj.ast, expect (primary, [(A, "PIXEL", store), (B, "AXIS", store)]).
        let text = std::fs::read_to_string(
            "tests/fixtures/wcsconv/inputs/timj.ast"
        ).expect("read timj.ast");
        let obj = ast_core::from_native_string(&text).expect("parse");
        let fs = match obj {
            AnyObject::FrameSet(fs) => fs,
            _ => panic!("expected FrameSet"),
        };
        let (_primary, alts) = store_from_frameset_with_alts(&fs).expect("alt planes");
        assert_eq!(alts.len(), 2, "timj has 2 alt planes (PIXEL, AXIS)");
        assert_eq!(alts[0].letter, 'A');
        assert_eq!(alts[1].letter, 'B');
        assert!(alts[0].domain == "PIXEL" || alts[1].domain == "PIXEL");
        assert!(alts[0].domain == "AXIS" || alts[1].domain == "AXIS");
    }
```

(Adjust path resolution: `tests/fixtures/...` is relative to the integration test runner; for unit tests inside `wcs_write.rs`, use `concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/wcsconv/inputs/timj.ast")`.)

- [ ] **Step 5: Run + commit**

```bash
cargo test -p ast-core --lib fitschan::wcs_write::tests::store_from_frameset_with_alts
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): compute FitsStore per alt-WCS plane

Adds store_from_frameset_with_alts() returning (primary,
Vec<AltPlane>). Each alt plane is a (version_letter, frame_domain,
FitsStore) triple. Mirrors fitschan.c::FsetToStore lines 12241-12258
which calls AddVersion per non-base/non-current frame.

The existing store_from_frameset() is preserved and now delegates
to a generalised store_from_frameset_to(fs, target_idx) helper that
takes any frame as the WCS target.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 4: Suffix-aware encoder write path

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

Add an `s: char` parameter to `write_store_to_cards` (`' '` for primary, `'A'`-`'Z'` for alt). Suffix every axis-indexed key.

- [ ] **Step 1: Define a key-formatting helper**

```rust
/// Format an axis-indexed FITS key with optional version-letter suffix.
///
/// `s == ' '` returns the bare key (primary plane); other letters
/// append the suffix. Mirrors fitschan.c::FormatKey.
fn fmt_axis_key(name: &str, axis: usize, s: char) -> String {
    if s == ' ' {
        format!("{name}{axis}")
    } else {
        format!("{name}{axis}{s}")
    }
}

fn fmt_matrix_key(name: &str, i: usize, j: usize, s: char) -> String {
    if s == ' ' {
        format!("{name}{i}_{j}")
    } else {
        format!("{name}{i}_{j}{s}")
    }
}

fn fmt_global_key(name: &str, s: char) -> String {
    if s == ' ' {
        name.to_string()
    } else {
        format!("{name}{s}")
    }
}
```

- [ ] **Step 2: Refactor `write_store_to_cards` to take `s: char`**

Find the current signature. Add `s: char` parameter. Replace every `format!("CRPIX{}", i + 1)` with `fmt_axis_key("CRPIX", i + 1, s)`, etc.

For PLANE-SHARED cards (MJD-OBS, DATE-OBS), gate them on `s == ' '`:

```rust
    // Plane-shared: only emit on primary.
    if s == ' ' {
        if let Some(mjdobs) = store.mjdobs {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
    }
```

For per-plane cards that may differ between planes (RADESYS, EQUINOX, LONPOLE, LATPOLE), use `fmt_global_key`.

WCSAXES becomes `fmt_global_key("WCSAXES", s)`.

WCSNAME (new for alt planes — the C library writes it from the frame domain): emit only when `s != ' '` (primary doesn't typically have WCSNAME unless the user set it explicitly; for now, alt-only is fine):

```rust
    // WCSNAME (alt planes only — set from frame domain).
    if s != ' ' && !alt_domain.is_empty() {
        cards.set_string(&fmt_global_key("WCSNAME", s),
                         alt_domain, "Reference name for the coord. frame");
    }
```

(`alt_domain` would need to come in as a parameter — pass `Option<&str>` or a separate parameter.)

A cleaner refactor: have `write_store_to_cards(store, cards, use_cd, s, frame_name)` where `frame_name` is `Some("PIXEL")` etc. for alt planes, `None` for primary.

- [ ] **Step 3: Update the existing primary-plane caller**

The existing call site (in `encode_wcs`) becomes `write_store_to_cards(&primary_store, cards, use_cd, ' ', None)`.

- [ ] **Step 4: Build + run regression**

```bash
cargo build -p ast-core
cargo test --workspace 2>&1 | grep -cE "FAILED"
```

The primary-plane behavior must be unchanged. All currently-passing tests stay green.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
refactor(fitschan): add version-letter parameter to write_store_to_cards

Threads `s: char` through every axis-indexed key formation, where
' ' means the primary plane (bare keys) and 'A'-'Z' means alt
planes (suffixed keys). Plane-shared cards (MJD-OBS, DATE-OBS) are
gated to primary only.

Primary-plane behavior is unchanged; this is purely preparation
for emitting alt planes in the next commit.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 5: Wire alt planes into encode_wcs

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

- [ ] **Step 1: Update `encode_wcs`**

Change from:

```rust
let store = store_from_frameset(frameset)?;
write_store_to_cards(&store, cards, use_cd)
```

to:

```rust
let (primary, alts) = store_from_frameset_with_alts(frameset)?;
write_store_to_cards(&primary, cards, use_cd, ' ', None)?;
for alt in &alts {
    write_store_to_cards(&alt.store, cards, use_cd, alt.letter, Some(&alt.domain))?;
}
Ok(())
```

- [ ] **Step 2: Run the timj_fits-wcs test**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|timj_fits-wcs|multi-wcs|cname_fits-wcs"
```

Expected: `timj_fits-wcs` passes (or at least the alt-plane card lines now present). May reveal additional issues (e.g. `cname_fits-wcs` requires CNAME emission, separate work).

- [ ] **Step 3: Run regression**

```bash
cargo test --workspace 2>&1 | grep -cE "FAILED"
```

Must be 0. Existing passing tests must not regress.

- [ ] **Step 4: Run pure-FITS-WCS tests**

```bash
cargo test -p ast-core --test test_fitschan_wcs
cargo test -p ast-core --test test_fitschan_wcs_write
```

Must remain green.

- [ ] **Step 5: Clippy / fmt clean. Commit.**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): emit FITS-WCS alt-WCS planes for non-primary frames

After the primary plane is written, loop the AltPlane vec returned
by store_from_frameset_with_alts and emit each with its version
letter. Mirrors fitschan.c::FsetToStore loop at lines 12241-12258.

Closes wcsconv timj_fits-wcs (PIXEL→A, AXIS→B alt planes) and
related tests where alt-plane absence was the next blocker after
the primary-plane work.

Only the FITS-WCS encoding emits alt planes — IRAF / AIPS / PC /
CLASS / DSS only emit the primary plane (matches the C library's
encoding-specific FromStore routines).

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 6: Final verification

**Files:** none.

- [ ] **Step 1: Wcsconv state**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: 116 → ~120-123 (depending on which tests cleanly turn green vs which still need other category work).

- [ ] **Step 2: Specific cluster check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "timj_fits-wcs|multi-wcs|cname_fits-wcs|tan-pv0|date-z"
```

Capture which still fail and why. Some may need read-side DATE-OBS plumbing or CNAME work — track for follow-up.

- [ ] **Step 3: Workspace tests + clippy**

```bash
cargo test --workspace 2>&1 | grep -cE "FAILED"
cargo clippy --workspace -- -D warnings
```

Both clean.

- [ ] **Step 4: Done if no further fixes needed.**

---

## Self-review checklist

- [ ] **Spec coverage:** Frame trait extended (Task 1); version-letter assignment (Task 2); per-plane FitsStore (Task 3); suffix-aware emission (Task 4); orchestration (Task 5).
- [ ] **C library fidelity:** version-letter assignment matches FsetToStore lines 12172-12230 exactly; AddVersion-equivalent emits the same per-version cards as fitschan.c.
- [ ] **No regression:** primary-plane behavior unchanged; existing tests stay green.
- [ ] **Frequent commits:** 5-6 commits, one per task.

---

## What this plan does NOT cover

- **CNAME emission.** `cname_fits-wcs` expects `CNAME` cards; this is per-axis name override, similar machinery to alt planes but separate. If the test still fails after H1, file a follow-up.
- **AltAxes attribute.** The C library has an `AltAxes` setting that controls whether alt planes are emitted. We don't currently support this; default behavior is "emit all eligible alt planes", which matches `AltAxes = ALL`. Out of scope.
- **PV parameters per version letter.** `tan-pv0_fits-wcs` expects `PV1_0` (note: primary plane, no version letter); this is a primary-plane PV-emission gap, not alt-plane. Track separately.
- **Offset SkyFrame extra alt plane.** The C library's FsetToStore lines 12232-12237 emit an EXTRA alt plane for offset SkyFrames (one for offset, one for absolute coords). Defer until a test fixture exercises it.
- **Other encoders.** AIPS, IRAF, PC, CLASS, DSS get only the primary plane. Matches C library.
