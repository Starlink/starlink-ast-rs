# Tier-2 K — simplifier deep-traversal closure

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the simplifier's nested-CmpMap inverse-pair recognition gap so the IWC composition residue (currently ~1e-14 to ~1e-11°) becomes exactly 0, then delete the CROTA snap workaround in `enc_aips`. This unblocks 7 wcsconv tests in roadmap category F.

**Architecture:** Capture the offending pre-simplify mappings from the failing wcsconv cases, generate canonical post-simplify references via the C oracle binary, add them to the existing manifest-driven simplify regression harness as expected-failure fixtures, then iterate `Mapping::simplify` until they pass. Remove the encoder-side workaround as the final commit.

**Tech Stack:** Rust 2024, existing `Mapping::simplify` infrastructure (`crates/ast-core/src/mappings/simplify.rs`, `crates/ast-core/src/mappings/cmpmap.rs`), existing simplify regression harness (`crates/ast-core/tests/test_simplify_regression.rs` + `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`). C oracle binary at `reference/starlink-ast/build/ast_tester/simplify`.

---

## Background — why the existing fixture passes but wcsconv still fails

The `cmpmap_nested_inverse_cancel` fixture (commit `768b6e9`) was hand-crafted and currently passes the simplify regression harness. The 7 wcsconv cases listed in roadmap category F still fail with 1e-14/1e-15 residues:

| wcsconv case | Failure pattern |
|---|---|
| `gappt_fits-wcs` | `CD1_2 = -7.4280239E-15` instead of next expected `CD2_2 = 0.0002778` |
| `ukirt-dtai_fits-wcs` | `CD1_2 = -6.2943301E-15` |
| `galactic-tan_fits-iraf` | `CD1_2 = 1.1728295E-14` |
| `sin-plain_fits-iraf` | `CD1_2 = -1.6896697E-14` |
| `fk4-aips_fits-iraf` | `CD1_2 = -1.9636198E-14` |
| `fits-pc-altgal_fits-wcs` | `CD1_2 = 4.174478E-15` |
| `pixel-shear_fits-aips++` | `CRVAL1 = -7.1054274E-14` |

The encoder-side workaround in `enc_aips` (`crates/ast-core/src/fitschan/enc_aips.rs:266-270`) snaps CROTA values below 1e-9° to zero. This is fragile and only papers over the FITS-AIPS / FITS-AIPS++ paths; the FITS-WCS / FITS-IRAF / FITS-PC paths emit the residue as CD/CRVAL cards directly.

The actual residue source lives somewhere in the wcsconv-specific composition path, not in the abstract `cmpmap_nested_inverse_cancel` fixture. Task 1 captures the actual offender and Task 2 generates its canonical simplification.

---

## File structure

- **Diagnostic-only (deleted before final commit):** A scratch test in `crates/ast-core/tests/diag_capture_simplify_input.rs` that reproduces the wcsconv read+pre-simplify path for `gappt_fits-wcs` and writes the captured mapping to disk. Used in Task 1, deleted in Task 7.
- **Create:** `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_<name>_iwc_residue.{map,simp}` — one pair per distinct simplifier failure mode.
- **Modify:** `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt` — register the new fixtures.
- **Modify:** `crates/ast-core/src/mappings/simplify.rs` and/or `crates/ast-core/src/mappings/cmpmap.rs` — close the deep-traversal gap.
- **Modify (delete code):** `crates/ast-core/src/fitschan/enc_aips.rs` — remove the CROTA snap branch.

---

## Hard rule: every fix must be backed by a fixture

When iterating `Mapping::simplify`, do NOT make changes "by inspection" of the IWC composition logic. The fixture-driven workflow is non-negotiable: the C oracle's output is the contract. If a Rust simplifier rule produces a different result than the C oracle on a captured `.map`, the Rust rule is wrong even if it looks reasonable.

If the C oracle's output looks unexpected to you, READ THE C SOURCE in `reference/starlink-ast/src/cmpmap.c` and `reference/starlink-ast/src/mapping.c` to understand what algorithm produces it. Don't try to invent a simpler rule.

---

## Task 1: Capture the offending pre-simplify mapping

**Files:**
- Create: `crates/ast-core/tests/diag_capture_simplify_input.rs` (scratch, deleted in Task 7)

The wcsconv path runs `Mapping::simplify` somewhere in `wcs_write::store_from_frameset`. We need the mapping it sees before the simplify call.

- [ ] **Step 1: Locate the simplify call site**

```bash
grep -n "simplify\|simplified" crates/ast-core/src/fitschan/wcs_write.rs
```

Identify each `.simplify()` invocation. For each, note the variable being simplified — typically `pixel_to_world` and/or `pixel_to_iwc`.

- [ ] **Step 2: Add a diagnostic test that captures input and output**

Create `crates/ast-core/tests/diag_capture_simplify_input.rs`:

```rust
//! SCRATCH diagnostic — captures the pre-simplify and post-simplify
//! mappings from the gappt_fits-wcs wcsconv path so we can build a
//! fixture for tier-2 K. This file is deleted at the end of K.

use std::path::PathBuf;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::serial::native_write::render_native;
use ast_core::serial::any_object_to_node;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name)
}

#[test]
#[ignore = "diagnostic only — run manually with --ignored"]
fn capture_gappt_pre_simplify_mapping() {
    let text = std::fs::read_to_string(fixture_path("gappt.head")).unwrap();
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.guess_encoding();
    let obj = fc.read().unwrap().unwrap();
    let AnyObject::FrameSet(fs) = obj else { panic!() };

    // Walk the FrameSet's pixel-to-world mapping and dump pre-simplify.
    // The wcs_write path simplifies the composed pixel->world before
    // extracting CD; we want the pre-simplify shape.
    let mapping = fs.mapping();
    let pre_node = any_object_to_node(&AnyObject::Mapping(mapping.clone_box())).unwrap();
    let pre_text = render_native(&pre_node).unwrap();
    std::fs::write("/tmp/gappt_pre_simplify.map", &pre_text).unwrap();

    eprintln!("Pre-simplify mapping written to /tmp/gappt_pre_simplify.map");
    eprintln!("Length: {} chars, {} lines", pre_text.len(), pre_text.lines().count());

    // Also run our simplify and dump the result for comparison.
    let simplified = mapping.simplify().unwrap();
    let simp_node = any_object_to_node(&AnyObject::Mapping(simplified)).unwrap();
    let simp_text = render_native(&simp_node).unwrap();
    std::fs::write("/tmp/gappt_post_simplify_rust.map", &simp_text).unwrap();
    eprintln!("Rust simplified mapping written to /tmp/gappt_post_simplify_rust.map");
}
```

If `mapping.clone_box()` doesn't exist, find the appropriate clone method on `dyn Mapping`. If `any_object_to_node` requires an `AnyObject` variant we don't have for raw mappings, build the FrameSet's full pixel→world mapping by other means — what matters is capturing some serializable representation of the mapping handed to `simplify`. Check what's on `dyn Mapping`: `grep -n "pub fn" crates/ast-core/src/mappings/mod.rs | head -20`.

If you cannot dump a `dyn Mapping` directly, fall back to: dump the entire FrameSet via the existing `to_native_string` path, then in Task 2 hand-extract the pixel-to-world mapping from the FrameSet.

- [ ] **Step 3: Run the diagnostic**

```bash
cargo test -p ast-core --test diag_capture_simplify_input -- --ignored --nocapture
```

Should produce `/tmp/gappt_pre_simplify.map` and `/tmp/gappt_post_simplify_rust.map`. Inspect both:

```bash
wc -l /tmp/gappt_pre_simplify.map /tmp/gappt_post_simplify_rust.map
head -40 /tmp/gappt_pre_simplify.map
```

Confirm the file is non-empty native AST text starting with `Begin CmpMap` (or `Begin FrameSet` if you fell back to the full FrameSet dump).

- [ ] **Step 4: Choose the canonical fixture name**

Name: `wcsconv_gappt_iwc_residue` (descriptive, traceable to the source). The captured `.map` and the to-be-generated `.simp` will live at:
- `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.map`
- `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.simp`

- [ ] **Step 5: Move the captured `.map` into place**

```bash
mv /tmp/gappt_pre_simplify.map \
   crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.map
```

Don't commit yet — Task 2 generates the `.simp` reference and they commit together.

- [ ] **Step 6: Don't commit. Continue to Task 2.**

---

## Task 2: Generate the C-oracle reference and register the fixture

**Files:**
- Create: `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.simp`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`

- [ ] **Step 1: Generate the .simp reference via the C oracle binary**

```bash
LD_LIBRARY_PATH=reference/starlink-ast/build \
  reference/starlink-ast/build/ast_tester/simplify \
  crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.map \
  crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.simp
```

If the binary requires `DYLD_LIBRARY_PATH` instead (macOS) or different invocation, adjust. To learn its calling convention, run with no arguments and read the usage line.

- [ ] **Step 2: Inspect the output**

```bash
head -40 crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.simp
```

The expected shape: a much-simpler mapping (often `UnitMap` or a very short CmpMap) with no 1e-14 residues. If the .simp is identical to the .map (no simplification happened), the C oracle ALSO doesn't simplify this — meaning our residue is real and the C oracle gets to the same residue but the C *encoder* handles it elsewhere (this would falsify the K diagnosis). In that case, STOP and report findings; the fix may need to move to the encoder, not the simplifier.

- [ ] **Step 3: Register the fixture in the manifest**

Append to `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`:

```
wcsconv_gappt_iwc_residue | simplify_fixtures/wcsconv_gappt_iwc_residue.map | simplify_fixtures/wcsconv_gappt_iwc_residue.simp |
```

(Place it after the existing `cmpmap_nested_*` entries in the "Focused rule fixtures" section.)

- [ ] **Step 4: Run the simplify regression — confirm it FAILS**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "wcsconv_gappt|Simplify regression"
```

Expected: the new fixture appears under `equal()`, `transform`, or `serialization` failures (or all three). If it passes immediately, the fixture isn't exercising any gap — re-examine the captured `.map`.

- [ ] **Step 5: Commit the fixture pair as expected-failure**

```bash
git add crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.map \
        crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.simp \
        crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt
git commit -m "$(cat <<'EOF'
test(simplify): expected-failure fixture for gappt_fits-wcs IWC residue

Captures the pre-simplify mapping from the gappt_fits-wcs wcsconv
path, where our Mapping::simplify currently leaves a 1e-14 rotation
residue that the C oracle simplifies cleanly. Reference .simp
generated via reference/starlink-ast/build/ast_tester/simplify.

This fixture turns green when the simplifier deep-traversal gap
closes (Tier-2 task K).

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

The simplify regression harness must NOT hard-panic on this failure (it tracks `transform_failures` separately and only panics on those). Verify:

```bash
cargo test -p ast-core --test test_simplify_regression
```

The test should still report `ok` in the test result line (the harness reports failures via stderr but doesn't panic unless transform-equivalence fails). If the harness panics, you may need to mark the new fixture with a known-failure flag — check the manifest comment at the top of `simplify_tests.txt` for any flag mechanism. If none exists, add one (e.g. a fourth field `expected_to_fail` parsed alongside `skip_string_compare`) and update the harness in `test_simplify_regression.rs` to report-but-not-panic when set.

---

## Task 3: Identify the simplifier gap

**Files:** none (analysis only)

- [ ] **Step 1: Diff the Rust output against the C reference**

```bash
diff /tmp/gappt_post_simplify_rust.map \
     crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/wcsconv_gappt_iwc_residue.simp
```

The diff shows the structural gap. Common patterns:
- C reduces the chain to a single `MatrixMap` or `WinMap`; Rust leaves a multi-component CmpMap with tiny rotations.
- C cancels nested `WcsMap` ∘ `WcsMap⁻¹` pairs that Rust missed because they're separated by `UnitMap` siblings or wrapped in series-with-something-else.

- [ ] **Step 2: Read the C-library simplify recipe**

```bash
grep -n "deep traversal\|nested\|flatten\|inverse pair" reference/starlink-ast/src/cmpmap.c | head -20
grep -n "Simplify\|astSimplify\|FlattenSeries" reference/starlink-ast/src/cmpmap.c | head -20
```

The C `Simplify` for CmpMap (`reference/starlink-ast/src/cmpmap.c`) does:
1. Recursively simplify each sub-mapping.
2. Flatten nested CmpMap of the same composition (series-of-series, parallel-of-parallel) into a flat list.
3. For series compositions: pairwise combine adjacent siblings using each sub-mapping's `MapMerge` method, sweeping multiple passes until no more merges happen.
4. Detect inverse pairs (Map followed by Map⁻¹) including across `UnitMap` separators — that's the deep-traversal step.
5. For parallel compositions: equivalent flattening + pairwise combine.

Read the actual implementation (the function is large — `~1500 lines`) until you understand:
- How does C decide that two non-adjacent sub-mappings are inverses of each other?
- What's the loop structure that re-runs after a merge?

- [ ] **Step 3: Locate the equivalent Rust path**

```bash
grep -n "simplify\|fn simplified\|MapMerge" crates/ast-core/src/mappings/cmpmap.rs crates/ast-core/src/mappings/simplify.rs | head -20
```

Read the existing implementation. Identify which step is missing.

- [ ] **Step 4: Write a one-paragraph diagnosis**

In a scratch text file or in the next commit's body, write 3-5 sentences identifying:
- What rule does C apply that Rust doesn't?
- Where in the Rust code should the rule live?
- What's the minimal change to add it?

This isn't a deliverable file — it goes in the next commit message body. But thinking it through explicitly prevents speculative refactoring.

- [ ] **Step 5: Don't commit. Continue to Task 4.**

---

## Task 4: Implement the closure

**Files:**
- Modify: `crates/ast-core/src/mappings/simplify.rs` and/or `crates/ast-core/src/mappings/cmpmap.rs` (whichever the gap analysis points to)

The exact change depends on Task 3's diagnosis. Common shapes:

**If the gap is "flatten nested CmpMap-of-same-composition":**
- Add a `flatten_chain(&self) -> Vec<DynMapping>` helper on `CmpMap` that recursively walks same-composition children into a flat list.
- In `CmpMap::simplify_impl`, call `flatten_chain` before the pairwise-merge sweep.

**If the gap is "detect inverse pair across UnitMap separator":**
- Extend the pairwise-merge loop to also try non-adjacent siblings when an intervening sibling is a `UnitMap` (or, equivalently, simplify away the `UnitMap` first then re-run the pairwise merge).

**If the gap is "re-run pairwise merge after a single merge":**
- The existing loop may stop after a single pass. Replace with a fixed-point loop that re-runs until no more merges happen.

Whichever shape applies:

- [ ] **Step 1: Write the failing test first (TDD)**

The fixture from Task 2 is already failing. Use it.

- [ ] **Step 2: Implement the change**

Make the minimal change identified in Task 3. Don't rewrite the whole simplifier.

- [ ] **Step 3: Run the simplify regression — confirm the fixture passes**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "wcsconv_gappt|Simplify regression"
```

`wcsconv_gappt_iwc_residue` should no longer appear in any failure list.

- [ ] **Step 4: Run all simplify tests for regression**

```bash
cargo test -p ast-core --test test_simplify_regression
cargo test -p ast-core --lib mappings::cmpmap
cargo test -p ast-core --lib mappings::simplify
cargo test -p ast-core --test test_native_roundtrip_all
```

All must remain green.

- [ ] **Step 5: Run the wcsconv suite — confirm gappt now passes**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|gappt"
```

Expected: pass count up by at least 1 (gappt_fits-wcs); `gappt` should no longer appear in failure list.

- [ ] **Step 6: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/mappings/simplify.rs crates/ast-core/src/mappings/cmpmap.rs
git commit -m "$(cat <<'EOF'
fix(simplify): close <specific gap from Task 3 diagnosis>

<3-5 sentence body explaining what rule was missing and where it
was added. Reference the captured fixture and the C-library function
that implements the equivalent rule.>

The wcsconv_gappt_iwc_residue fixture now passes with exact
structural equality to the C reference.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

(Stage only the files you actually modified.)

---

## Task 5: Capture remaining category-F mappings

The gappt fix may close some or all of the other category-F failures. Check whether each remaining case (`ukirt-dtai_fits-wcs`, `galactic-tan_fits-iraf`, `sin-plain_fits-iraf`, `fk4-aips_fits-iraf`, `fits-pc-altgal_fits-wcs`, `pixel-shear_fits-aips++`) now passes. For any that still fail with 1e-14-style residues, repeat Tasks 1-4 with a new fixture pair.

- [ ] **Step 1: Check current wcsconv state**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "ukirt-dtai|galactic-tan|sin-plain|fk4-aips|fits-pc-altgal|pixel-shear"
```

- [ ] **Step 2: For each still-failing case in category F**

Repeat the workflow:
1. Modify the diagnostic in `diag_capture_simplify_input.rs` to capture this case's mapping (or duplicate the test function with the new fixture name).
2. Save under `simplify_fixtures/wcsconv_<name>_iwc_residue.map`.
3. Run C oracle to produce `.simp`.
4. Add to manifest.
5. Run regression — should fail.
6. Implement additional simplifier rule if needed (often the gappt fix already covers them all because they all hit the same root-cause CmpMap shape).
7. Commit each fixture+code pair separately.

- [ ] **Step 3: Final wcsconv check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: 110 → 116 (or close — `pixel-shear_fits-aips++` may have additional content failures beyond the residue).

- [ ] **Step 4: For each fixture, commit separately**

One commit per fixture pair (and any code change needed). Commit messages mirror Task 4's pattern.

If a fix in Task 4 covered all remaining cases without further code changes, skip directly to Task 6 — but still register fixtures for the additional cases so regressions are caught.

---

## Task 6: Delete the encoder-side workaround

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`

With the simplifier residue gone, the CROTA snap is dead code.

- [ ] **Step 1: Locate the snap**

```bash
grep -n "Snap rotations\|crota.abs() < 1e-9" crates/ast-core/src/fitschan/enc_aips.rs
```

Around line 252-272. Look at the surrounding comment block (lines 244-265) too — the long explanatory comment about the simplifier gap also goes.

- [ ] **Step 2: Replace with the simplified version**

Find the block:

```rust
    // CROTA → PC matrix in cards is good for AIPS, but the C library
    // ... <long comment about simplifier gap> ...
    if naxis >= 2 {
        let mut crota = pc_vals[[1, 0]].atan2(pc_vals[[1, 1]]).to_degrees();
        if crota.abs() < 1e-9 {
            crota = 0.0;
        }
        let k = crota_axis(store);
        cards.set_float(&format!("CROTA{k}"), crota, "Axis rotation");
    }
```

Replace with:

```rust
    if naxis >= 2 {
        let crota = pc_vals[[1, 0]].atan2(pc_vals[[1, 1]]).to_degrees();
        let k = crota_axis(store);
        cards.set_float(&format!("CROTA{k}"), crota, "Axis rotation");
    }
```

(Read lines 240-275 of the actual file to get the exact context — the snap block and its preceding comment may span more lines than shown above.)

- [ ] **Step 3: Run all FITS-AIPS tests**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|fits-aips"
```

The pass count should hold or rise. Specifically, no fits-aips fixture that was previously passing should now fail with a tiny CROTA residue — if any does, the simplifier closure is incomplete and you need to return to Task 4/5.

- [ ] **Step 4: Run the workspace tests for cross-cutting regression**

```bash
cargo test --workspace 2>&1 | tail -5
```

All green.

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_aips.rs
git commit -m "$(cat <<'EOF'
refactor(fitschan): remove CROTA snap-to-zero workaround

The 1e-9° snap was a workaround for the simplifier deep-traversal
gap. With that gap closed (Tier-2 K), the CROTA value computed from
the simplified PC matrix is exact and snapping is unnecessary.

The C library never snapped; this removal aligns Rust output with
C output for cases where the input genuinely produces a tiny but
non-zero rotation.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 7: Cleanup and final verification

**Files:**
- Delete: `crates/ast-core/tests/diag_capture_simplify_input.rs`

- [ ] **Step 1: Delete the diagnostic test file**

```bash
rm crates/ast-core/tests/diag_capture_simplify_input.rs
```

- [ ] **Step 2: Run the full workspace test suite**

```bash
cargo test --workspace 2>&1 | grep -E "FAILED|panic" | head -10
```

No failures expected.

- [ ] **Step 3: Final wcsconv check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: 110 → ≥116 (close to the +7 estimate; pixel-shear may stay red if the residue isn't its only issue).

- [ ] **Step 4: Confirm the existing `cmpmap_nested_inverse_cancel` fixture still passes** (it's the regression guard for this work)

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "cmpmap_nested|Simplify regression"
```

`cmpmap_nested_inverse_cancel` should pass without any expected-failure marker. If we added a `expected_to_fail` flag to the manifest in Task 2, ensure no fixture still carries it.

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/tests/diag_capture_simplify_input.rs
git commit -m "$(cat <<'EOF'
chore(simplify): remove K-tier diagnostic capture test

The diagnostic served its purpose of capturing the offending
pre-simplify mappings from the wcsconv path. Future regressions
of the simplifier gap are guarded by the wcsconv_*_iwc_residue
fixtures.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Self-review checklist

- [ ] **Spec coverage:** the K item from `docs/superpower/specs/2026-05-21-fitschan-wcsconv-roadmap.md` says (a) close the deep-traversal gap, (b) capture per-case fixtures, (c) delete the CROTA snap, (d) leave `cmpmap_nested_inverse_cancel` passing without expected-failure markers. All four are addressed.
- [ ] **No placeholders:** every step has explicit code, paths, commands, expected outputs.
- [ ] **Frequent commits:** 5+ commits (1 fixture + 1+ simplifier rules + 1 snap removal + 1 cleanup; possibly more if Task 5 needs additional fixtures).
- [ ] **Done criteria:** wcsconv ≥116/192, all simplify regression tests green, CROTA snap removed, diagnostic test deleted, workspace clean.

---

## Out of scope

- **Performance** of the simplifier closure. The change should be O(N²) at worst over CmpMap depth; benchmarking is out of scope for this plan.
- **Other simplifier gaps** unrelated to wcsconv category F. If you discover one while investigating, add it to the roadmap as a separate item, don't fix it here.
- **Encoder-level cleanups** other than the CROTA snap. The `enc_aips` snap is the only known workaround tied to this gap.
