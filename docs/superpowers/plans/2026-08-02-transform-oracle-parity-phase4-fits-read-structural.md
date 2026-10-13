# Transform-Oracle Parity — Phase 4 (WcsMap FITS-read structural) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the 29 remaining `.head` transform-oracle mismatches (17 fixtures) by porting the missing FITS-read structural logic from C `fitschan.c`/`wcsmap.c`/`matrixmap.c` faithfully, so each fixture's Rust-decoded object has the same shape and transform as C, and add a per-class oracle gate with a shrinking exclusion list so the fixed sections cannot silently regress.

**Architecture:** These fixtures fail the transform oracle because the FITS *reader* builds a structurally different AST object than C — a plain `Frame` where C builds a `SkyFrame`, a `CmpMap(WinMap, WcsMap)` where C builds a bare `WinMap`, a linear `ShiftMap` where C builds a `MathMap`/`GrismMap`, or an attribute in the wrong units.
Because the native-serialisation round-trip and the simplifier are already proven correct for this corpus (the `.ast`/`.map`/`.simp` oracle sections pass), a `.head` transform mismatch cannot be a serialisation or simplification artifact — it is a read-path structural gap upstream of both.
The 29 mismatches collapse to **12 distinct root causes**, each a specific unported C branch.
This plan ports each branch faithfully, one task per root cause, and each fix simultaneously clears the fixture's entry in the parallel `NATIVE_READ_EXCLUDED` FITS-read-fidelity list (`test_wcsconv.rs:72`) — the two exclusion lists shrink together, as the Phase 4 reconciliation established.

**Tech Stack:** Rust 2024, `ast-core` crate. C ground truth: `reference/starlink-ast/src/{fitschan,wcsmap,matrixmap,mapping}.c`. Oracle fixtures: `crates/ast-core/tests/fixtures/oracle/headers.oracle` (the `.head` corpus) + `crates/ast-core/tests/fixtures/wcsconv/inputs/*.head`. Reference build for probes: `reference/starlink-ast/build-ast/libast.so`.

## Global Constraints

- **C-fidelity is the rule.** Every fix mirrors C control flow directly. No compute-then-check tolerance heuristics without asking the human first. Where C uses a heuristic, replicate it and comment that it mirrors C. C logic beats existing diverged Rust — where a Rust comment justifies a divergence from C (e.g. `wcs_read.rs:594-604`), the divergence is the bug; discard it and match C after confirming with a probe.
- **Port ALL C logic.** Lack of test coverage is never a reason to skip a C branch. A fixture that "only" needs one branch still gets the whole branch ported.
- **`AST__BAD` = `BAD_VALUE` = `-f64::MAX`** (bit-identical to C). Inputs may be `f64::NAN`; outputs always use `BAD_VALUE`. A transform-oracle `got=-1.797693e308` means Rust produced BAD where C produced finite.
- **Public Rust API only in tests.** No private modules from `ast-core` in `tests/`. Read-path behaviour is tested through `FitsChan::from_header(text)?.read()` (needs `use ast_core::Channel`).
- **Fixtures must work with the submodule absent** — the oracle corpus and the `wcsconv/inputs/*.head` headers are already vendored. Never symlink from `reference/`.
- **Comments describe the code as it is today** — no references to prior bugs, plans, "was previously", or the Phase number. State what C does and why, citing the C source location.
- **C-oracle golden values and object shapes come from the repo's own `libast.so`** at `reference/starlink-ast/build-ast/libast.so` (recipe: `docs/issues/c-oracle-probe-recipe.md`; mind the conda-RPATH trap — the stale conda `libast.so` wins by default and must be overridden). Never invent a golden value or an "expected" object shape.
- **The two exclusion lists shrink together.** Every fix task removes both its `.head` rows from `HEAD_ORACLE_EXCLUDED` (added in Task 1) and its `*_native` rows from `NATIVE_READ_EXCLUDED` (`test_wcsconv.rs`). A fix that clears more `NATIVE_READ_EXCLUDED` entries than its own fixture (shared root cause) removes all of them.
- **Formatting/lint gate before every commit:** `cargo fmt` and clippy clean. Clippy under the conda/rustup mismatch uses the documented workaround (isolated `CARGO_TARGET_DIR` + `RUSTC` override); commit with `--no-verify` only if the pre-commit clippy hook hits the E0514 quirk, after verifying clippy passes under the workaround.
- **One sentence per line** in prose/Markdown. American English.

## Background: the reconciliation (already done)

Every `.head` transform-oracle failure maps 1:1 onto a `NATIVE_READ_EXCLUDED` entry; nothing is untracked, and nothing the FITS-read-fidelity workstream already fixed is re-fixed here.
The 29 failing section-directions across 17 fixtures (from `target/transform_oracle_report.json`) are:

```
ait-highlatpole (fwd+inv)  car4 (fwd+inv)          class-noproj (fwd+inv)
crota-3d (fwd)             freq-log (inv)          freq-wav (inv)
gls (fwd+inv)              grism (inv)             serpens (fwd+inv)
skyref (fwd+inv)           skyref-alt (fwd+inv)    tan-polytan (fwd+inv)
tidy-offsets (fwd+inv)     tidy-pole (fwd+inv)     unknown-cel (fwd+inv)
unknown-cel2 (fwd+inv)     wave-f2w (inv)
```

The 12 root causes, each confirmed empirically against C (probe test reverted, `git status` clean at investigation end):

| # | Fixture(s) | Root cause | Primary locus |
|---|---|---|---|
| A1 | gls, ait-highlatpole | LONPOLE forced to 180° before `wcs_nat_pole`, defeating the projection-aware default for θ0=0 projections | `wcs_celestial.rs:563,664-671` |
| A2 | unknown-cel, unknown-cel2 | CTYPE recognition whitelist lacks generic `xLON`/`xLAT`/`xyLN`/`xyLT`; System=Unknown classification missing | `fits_store.rs:273-298`; `wcs_celestial.rs` system classification |
| A3 | class-noproj | no CAR-linear branch for a celestial CTYPE with no projection code (`RA--`/`DEC-`, ≤4 chars) | `wcs_celestial.rs:120-122`; port `fitschan.c:35878-35886,35896-35939` |
| B1 | car4 | `CarLin` FitsChan attribute unimplemented; CAR path builds domain-restricted `WcsMap[CAR]` that returns BAD | new `carlin` on `FitsChan`; `wcs_celestial.rs:130`; port `fitschan.c:36030-36035` |
| B2 | freq-log | `-LOG` falls through to linear `ShiftMap`; `LogWcs` MathMap unported | `wcs_spectral.rs:208-213`; port `fitschan.c:20777-20859` |
| B3 | grism | `-GRI`/`-GRA` falls through to linear `ShiftMap`; `GrismSpecWcs` unported | `wcs_spectral.rs:208-213`; port `fitschan.c:14306-14468` |
| C1 | serpens | DUT1 stored raw; C multiplies by seconds-per-day (86400) | `wcs_celestial.rs:321` |
| C2 | tidy-offsets, tidy-pole, skyref-alt (+ axref, degen-wcsaxes, rfval, scp, sip-alt, zpx) | SREFIS/SREF/SREFP reads ignore the alt-plane version letter `s` | `wcs_keywords.rs:266-270` |
| C3 | tan-polytan | PolyTan auto-detect (TAN→TPN when nonzero PV distortion present) unported | `spec_trans.rs:640-661`; port `GetUsedPolyTan` `fitschan.c:13986-14169` |
| C4 | skyref (+ skyref-alt matrix, fits-pc-alt, fits-pc-unit, gappt) | `SkyRefIs` set directly on the Frame, bypassing the FrameSet-integrity remap C gets from `astSetC(fset,...)` | `wcs_read.rs:563-607`; C `fitschan.c:34665` |
| C5 | freq-wav, wave-f2w | `astRate` finite-difference result drifts ~1e-9 from C, propagating into the ShiftMap scale | `mappings/rate.rs` vs `mapping.c` Rate/FindGradient |
| C6 | crota-3d | near-singular MatrixMap keeps a defined inverse; C leaves it undefined so `INV=0` propagates | `wcs_matrix.rs`; port `matrixmap.c` `InvertMatrix` singularity handling |

Root causes A2, A3, C2, C4 also clear `NATIVE_READ_EXCLUDED` entries beyond their own fixtures (shared branches); those extra entries are listed per task.

## File Structure

- `crates/ast-core/src/fitschan/wcs_celestial.rs` — celestial read path. Modified by A1 (LONPOLE default), A2 (System=Unknown), A3 (CAR-linear branch), B1 (CarLin threading), C1 (DUT1 ×86400).
- `crates/ast-core/src/fitschan/fits_store.rs` — CTYPE recognition. Modified by A2 (generic longitude/latitude predicates), A3 (no-projection-code detection).
- `crates/ast-core/src/fitschan/wcs_spectral.rs` — spectral read path. Modified by B2 (`log_wcs`), B3 (`grism_spec_wcs`).
- `crates/ast-core/src/fitschan/wcs_keywords.rs` — keyword extraction. Modified by C2 (version-letter SREF reads).
- `crates/ast-core/src/fitschan/spec_trans.rs` — SpecTrans translations. Modified by C3 (`GetUsedPolyTan` port).
- `crates/ast-core/src/fitschan/wcs_read.rs` — FrameSet assembly / TidyOffsets. Modified by C4 (SkyRefIs through FrameSet).
- `crates/ast-core/src/fitschan/mod.rs` — the `FitsChan` struct. Modified by B1 (new `carlin` field + setter).
- `crates/ast-core/src/mappings/rate.rs` — the `Rate`/`FindGradient` finite-difference kernel. Modified by C5 (numeric parity audit).
- `crates/ast-core/src/fitschan/wcs_matrix.rs` and `crates/ast-core/src/mappings/matrix.rs` — pixel→IWC matrix pipeline and MatrixMap inverse. Modified by C6 (inverse-undefined for near-singular matrices).
- `crates/ast-core/tests/test_transform_oracle.rs` — the oracle harness. Gets the `HEAD_ORACLE_EXCLUDED` gate (Task 1), which each fix task then shrinks. `transform_oracle_report` (the `#[ignore]` investigative test) is left untouched.
- `crates/ast-core/tests/test_wcsconv.rs` — the FITS-read-fidelity test. Its `NATIVE_READ_EXCLUDED` list is shrunk by each fix task.
- `crates/ast-core/tests/test_fitschan_wcs.rs` (the existing FitsChan WCS read-path test module; the sibling `test_fitschan.rs`, `test_alt_wcs_read.rs`, and `test_dsbspecframe_read.rs` cover related read paths) — gets a public-API read test per fix locking the corrected object shape/attribute.

## Task sequence rationale

**Task 1 establishes the gate first**, with all 29 head-sections excluded and a section floor, so every subsequent fix task can remove its rows and the gate proves the list only shrinks.
This inverts the Phase 1 order (gate last) because Phase 4 has 12 fixes, not two — a standing gate that each fix shrinks is the cleaner discipline for a long sequence.

The 12 fixes are then ordered **easy-and-independent first, structural-and-risky last**, matching the singleton investigation's recommended order and minimizing cross-task interference:

- **C1 (DUT1)** — one-line, fully isolated. Task 2.
- **C2 (SREF version letter)** — small, and a prerequisite for C4's skyref-alt case (the alt frame must read the right SREF*A before any remap). Task 3.
- **A1 (LONPOLE default)** — small, isolated to the rotation builder. Task 4.
- **A2 (generic celestial recognition)** — recognition + system classification, no rotation change (TAN is zenithal). Task 5.
- **B2 (LogWcs)** and **B3 (GrismSpecWcs)** — additive spectral readers, no interaction with celestial. Tasks 6-7.
- **C3 (PolyTan)** — SpecTrans CTYPE rewrite, isolated. Task 8.
- **A3 (CAR-linear no-proj-code)** — a new celestial codepath; sits after A1/A2 so it lands on an otherwise-correct celestial reader. Task 9.
- **B1 (CarLin)** — a new FitsChan attribute + celestial branch; after A3 because both touch the CAR/linear celestial path. Task 10.
- **C4 (SkyRefIs through FrameSet)** — reverses a documented direct-set decision; needs a probe to confirm, and depends on C2 for skyref-alt. Task 11.
- **C5 (astRate drift)** — numeric-kernel audit, no structural change. Task 12.
- **C6 (MatrixMap inverse-undefined)** — the deepest; touches MatrixMap inverse-definedness and fusion shape; last so its structural effects are judged against an otherwise-correct FrameSet. Task 13.

Each fix is independently reviewable: it ports one C branch, adds one locking read-test, and shrinks both exclusion lists by its cleared entries.

---

### Task 1: Establish the head-fixture oracle gate

Mirror the committed `SLAMAP_ORACLE_EXCLUDED` gate (`test_transform_oracle.rs:1196-1286`) for the `.head` corpus in `headers.oracle`.
Start with all 29 failing section-directions excluded; every later task removes its rows.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs` (add the gate below the SlaMap gate, ~line 1310)

**Interfaces:**
- Consumes: `run_oracle(&str, &Overrides) -> Vec<SectionReport>`, `parse_overrides(&str) -> Overrides`, `SectionReport { relpath, forward, outcome }`, `enum Outcome`, `describe_outcome(&Outcome) -> String` — all already in this file.
- Produces: `HEAD_ORACLE_EXCLUDED: &[(&str, bool, &str)]`, `HEAD_SECTION_FLOOR: usize`, `is_head_section(&str) -> bool`, test `head_sections_match_oracle`.

- [ ] **Step 1: Confirm the current head-section failure set**

Run the investigative report and read the head-fixture failures:

```bash
cargo test -p ast-core --test test_transform_oracle -- --ignored --nocapture
python3 -c "import json; d=json.load(open('target/transform_oracle_report.json')); \
print(sorted((s['relpath'], s['forward']) for s in d['sections'] \
if s['relpath'].endswith('.head') and s['status'] not in ('pass','skipped','skipped-override')))"
```

Expected: the 29 `(relpath, forward)` pairs for the 17 fixtures listed in the Background section.
Record the exact `relpath` strings (they are the oracle section markers, e.g. `wcsconv/inputs/gls.head`) — the exclusion rows must match them verbatim.

- [ ] **Step 2: Add the gate (initially excluding all 29), the floor, and the predicate**

Insert after the SlaMap gate. Use the exact relpaths from Step 1. Group the rows by root-cause letter with a comment, so each fix task knows which rows to delete:

```rust
/// `.head` oracle sections not yet matching C, each with a one-line reason
/// naming the unported C read-path branch. The goal is to keep this list
/// empty; it only ever shrinks. A section is identified by `(relpath,
/// forward)`. These are FITS-read structural gaps (the decoded object has a
/// different shape/attribute than C), never serialisation or simplifier
/// artifacts — the `.ast`/`.simp` corpus is already byte-exact.
const HEAD_ORACLE_EXCLUDED: &[(&str, bool, &str)] = &[
    // A1 LONPOLE default (wcs_celestial LONPOLE):
    ("wcsconv/inputs/gls.head", true, "A1: LONPOLE forced to 180 before wcs_nat_pole"),
    ("wcsconv/inputs/gls.head", false, "A1: LONPOLE forced to 180 before wcs_nat_pole"),
    ("wcsconv/inputs/ait-highlatpole.head", true, "A1: LONPOLE forced to 180 before wcs_nat_pole"),
    ("wcsconv/inputs/ait-highlatpole.head", false, "A1: LONPOLE forced to 180 before wcs_nat_pole"),
    // A2 generic celestial recognition + System=Unknown:
    ("wcsconv/inputs/unknown-cel.head", true, "A2: generic xLON/xLAT recognition missing"),
    ("wcsconv/inputs/unknown-cel.head", false, "A2: generic xLON/xLAT recognition missing"),
    ("wcsconv/inputs/unknown-cel2.head", true, "A2: generic xLON/xLAT recognition missing"),
    ("wcsconv/inputs/unknown-cel2.head", false, "A2: generic xLON/xLAT recognition missing"),
    // A3 CAR-linear no-projection-code branch:
    ("wcsconv/inputs/class-noproj.head", true, "A3: CAR-linear branch for no-proj-code CTYPE missing"),
    ("wcsconv/inputs/class-noproj.head", false, "A3: CAR-linear branch for no-proj-code CTYPE missing"),
    // B1 CarLin attribute:
    ("wcsconv/inputs/car4.head", true, "B1: CarLin attribute unimplemented"),
    ("wcsconv/inputs/car4.head", false, "B1: CarLin attribute unimplemented"),
    // B2 LogWcs:
    ("wcsconv/inputs/freq-log.head", false, "B2: -LOG LogWcs MathMap unported"),
    // B3 GrismSpecWcs:
    ("wcsconv/inputs/grism.head", false, "B3: -GRI/-GRA GrismSpecWcs unported"),
    // C1 DUT1 x86400:
    ("wcsconv/inputs/serpens.head", true, "C1: DUT1 stored raw, not x SPD"),
    ("wcsconv/inputs/serpens.head", false, "C1: DUT1 stored raw, not x SPD"),
    // C2 SREF version letter:
    ("wcsconv/inputs/tidy-offsets.head", true, "C2: SREF version letter ignored"),
    ("wcsconv/inputs/tidy-offsets.head", false, "C2: SREF version letter ignored"),
    ("wcsconv/inputs/tidy-pole.head", true, "C2: SREF version letter ignored"),
    ("wcsconv/inputs/tidy-pole.head", false, "C2: SREF version letter ignored"),
    ("wcsconv/inputs/skyref-alt.head", true, "C2: SREF version letter ignored"),
    ("wcsconv/inputs/skyref-alt.head", false, "C2: SREF version letter ignored"),
    // C3 PolyTan auto-detect:
    ("wcsconv/inputs/tan-polytan.head", true, "C3: PolyTan TAN->TPN auto-detect unported"),
    ("wcsconv/inputs/tan-polytan.head", false, "C3: PolyTan TAN->TPN auto-detect unported"),
    // C4 SkyRefIs through FrameSet:
    ("wcsconv/inputs/skyref.head", true, "C4: SkyRefIs set directly, bypassing FrameSet remap"),
    ("wcsconv/inputs/skyref.head", false, "C4: SkyRefIs set directly, bypassing FrameSet remap"),
    // C5 astRate drift:
    ("wcsconv/inputs/freq-wav.head", false, "C5: astRate finite-diff drift into ShiftMap scale"),
    ("wcsconv/inputs/wave-f2w.head", false, "C5: astRate finite-diff drift into ShiftMap scale"),
    // C6 MatrixMap inverse-undefined:
    ("wcsconv/inputs/crota-3d.head", true, "C6: near-singular MatrixMap keeps defined inverse"),
];

/// Number of `.head` oracle section-directions currently in the corpus. The
/// gate asserts it examined at least this many, so a rename or deletion of a
/// fixture cannot make the gate pass vacuously.
const HEAD_SECTION_FLOOR: usize = 0; // set in Step 3 to the observed count

/// Is this oracle section a `.head` FITS-read fixture?
fn is_head_section(relpath: &str) -> bool {
    relpath.ends_with(".head")
}
```

- [ ] **Step 3: Add the gate test and set the floor**

Copy the `slamap_sections_match_oracle` body verbatim, renaming to `head_sections_match_oracle`, reading `headers.oracle` instead of `simplify_fixtures.oracle`, using `HEAD_ORACLE_EXCLUDED` / `is_head_section` / `HEAD_SECTION_FLOOR`:

```rust
#[test]
fn head_sections_match_oracle() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let text =
        std::fs::read_to_string(format!("{dir}/headers.oracle")).expect("oracle present");
    let reports = run_oracle(&text, &ov);

    let excluded: std::collections::HashSet<(&str, bool)> = HEAD_ORACLE_EXCLUDED
        .iter()
        .map(|(rp, fwd, _)| (*rp, *fwd))
        .collect();

    let is_fail = |o: &Outcome| {
        matches!(
            o,
            Outcome::Mismatch(_) | Outcome::LoadFail(_) | Outcome::Arity(_)
        )
    };

    let mut unexpected_failures: Vec<String> = Vec::new();
    let mut stale_exclusions: std::collections::HashSet<(&str, bool)> = excluded.clone();
    let mut checked = 0usize;

    for r in &reports {
        if !is_head_section(&r.relpath) {
            continue;
        }
        checked += 1;
        let key = (r.relpath.as_str(), r.forward);
        if excluded.contains(&key) {
            if is_fail(&r.outcome) {
                stale_exclusions.remove(&key);
            }
        } else if is_fail(&r.outcome) {
            unexpected_failures.push(format!(
                "{} (forward={}): {:?}",
                r.relpath,
                r.forward,
                describe_outcome(&r.outcome),
            ));
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "head oracle sections diverged from C (not excluded):\n{}",
        unexpected_failures.join("\n"),
    );
    assert!(
        stale_exclusions.is_empty(),
        "HEAD_ORACLE_EXCLUDED entries now pass — remove them:\n{:?}",
        stale_exclusions,
    );
    assert!(
        checked >= HEAD_SECTION_FLOOR,
        "gate examined only {checked} head sections (expected >= {HEAD_SECTION_FLOOR}); \
         fixtures may have been renamed or removed",
    );
}
```

- [ ] **Step 4: Run the gate; set the floor to the observed count**

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle -- --nocapture
```

If it fails on the floor assertion, it prints the `checked` count; set `HEAD_SECTION_FLOOR` to that number.
Expected after setting the floor: PASS (all 29 live failures are excluded, no stale exclusion, `checked >= floor`).
If any `unexpected_failures` appear, a head-fixture failure was missed in Step 1 — add its row before proceeding.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: gate .head oracle sections with shrinking exclusion list

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2: C1 — DUT1 stored in seconds (× SPD), not raw days

C reads DUT1 and multiplies by seconds-per-day before setting the SkyFrame attribute.
Reference: `fitschan.c:38018` — `if( dut1 != AST__BAD ) astSetDut1( *frm, dut1*SPD );` (JACH store DUT1 in days; the SkyFrame attribute is in seconds). `SPD = 86400.0`.
The write path already divides by SPD symmetrically (`fitschan.c:3033` `astGetDut1(wcsfrm)/SPD`), so the round-trip is consistent only if the read multiplies.

**Fixture cleared:** serpens (fwd+inv). **NATIVE_READ_EXCLUDED:** `serpens_native`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:320-322`
- Test: `crates/ast-core/tests/test_fitschan_wcs.rs`

**Interfaces:**
- Consumes: `store.dut1: Option<f64>`, `SkyFrame::set_dut1(f64)`, `SkyFrame::dut1() -> f64`.
- Produces: no new API.

- [ ] **Step 1: Write the failing read test**

Load `serpens.head` through the public read path and assert the SkyFrame's Dut1 is the header value × 86400. First read the header to get the raw DUT1 value:

```bash
grep -i "DUT1" crates/ast-core/tests/fixtures/wcsconv/inputs/serpens.head
```

Then, in the FitsChan read-path test module (use `ast_core::Channel`), decode and assert. Get the exact expected value from the C oracle probe if the header value is not obvious, but Dut1_days × 86400 is the invariant. Write the test to read the raw value from the fixture and compare against `frame.dut1()`:

```rust
#[test]
fn serpens_dut1_is_seconds() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/serpens.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Navigate to the SkyFrame and read Dut1 (seconds). The header stores
    // DUT1 in days; the SkyFrame attribute is seconds, so the decoded value
    // is header_days * 86400. Replace EXPECTED with header_value * 86400.0.
    let sky = /* extract SkyFrame from obj — see sibling read tests */;
    assert!((sky.dut1() - EXPECTED).abs() < 1e-9);
}
```

The implementer fills `EXPECTED` from the fixture's DUT1 card × 86400 and the SkyFrame extraction from a sibling read test in the same module.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core serpens_dut1_is_seconds
```

Expected: FAIL — Rust sets Dut1 to the raw day value, so `sky.dut1()` is 86400× too small.

- [ ] **Step 3: Apply the fix**

In `wcs_celestial.rs:320-322`:

```rust
    if let Some(dut1) = store.dut1 {
        // C fitschan.c:38018: the SkyFrame Dut1 attribute is in seconds,
        // while the FITS DUT1 keyword is in days; convert with SPD.
        sky_frame.set_dut1(dut1 * crate::pal::SPD);
    }
```

Confirm the SPD constant's location (`grep -rn "const SPD\|SPD:" crates/ast-core/src`); if `pal::SPD` is not exposed, inline `86_400.0` with a comment citing `fitschan.c:38018`.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core serpens_dut1_is_seconds
```

Expected: PASS.

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the two `serpens.head` rows from `HEAD_ORACLE_EXCLUDED` and the `serpens_native` row from `NATIVE_READ_EXCLUDED`:

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS. The gate confirms serpens now matches C; the wcsconv test confirms the native card output is byte-exact.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/wcs_celestial.rs crates/ast-core/tests/
git commit -m "fitschan: read DUT1 in seconds (x SPD) matching C

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 3: C2 — SREFIS/SREF/SREFP honour the alt-plane version letter

The keyword extractor reads `SREFIS`, `SREF{i}`, `SREFP{i}` with hard-coded primary-plane names, ignoring the version letter `s` that is already threaded into `extract_wcs_keywords(cards, s: char)`.
An alternate WCS description (`s` = `A`..`Z`) therefore reads the primary SkyRef instead of the alt plane's `SREF1A` etc.
The helper `key_for(prefix, axis: Option<usize>, s: char) -> String` (`wcs_keywords.rs:18`) already builds version-aware names and is used elsewhere in this function.

**Fixtures cleared:** tidy-offsets, tidy-pole, skyref-alt (fwd+inv each). **NATIVE_READ_EXCLUDED:** `tidy-offsets_native`, `tidy-pole_native`, `skyref-alt_native`, and the shared-branch entries `axref_native`, `degen-wcsaxes_native`, `rfval_native`, `scp_native`, `sip-alt_native`, `zpx_native` (all `SREF1`/`SREF2` reasons).

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs:266-270`
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `key_for(prefix: &str, axis: Option<usize>, s: char) -> String`, `cards.get_string_marked(&str)`, `cards.get_float_marked(&str)`, `s: char` (in scope in `extract_wcs_keywords`).
- Produces: no new API.

- [ ] **Step 1: Write the failing read test**

Pick an alt-plane fixture that carries `SREF1A`/`SREFIS A` (confirm with `grep -i "SREF" crates/ast-core/tests/fixtures/wcsconv/inputs/skyref-alt.head`). Assert the alt SkyFrame's SkyRef and SkyRefIs come from the `A`-suffixed cards, not the primary:

```rust
#[test]
fn skyref_alt_reads_versioned_sref() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/skyref-alt.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Navigate to the alt-plane SkyFrame and assert SkyRef(1)/SkyRef(2) and
    // SkyRefIs equal the SREF1A/SREF2A/SREFIS-A card values (from the probe
    // or the fixture), not the primary SREF1/SREF2.
    // ... extraction + asserts filled from a sibling alt-plane read test.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core skyref_alt_reads_versioned_sref
```

Expected: FAIL — the alt plane reads the primary SREF (or BAD) because the version letter is ignored.

- [ ] **Step 3: Apply the fix**

Replace `wcs_keywords.rs:266-270`:

```rust
    store.skyrefis = cards.get_string_marked(&key_for("SREFIS", None, s));
    for i in 0..naxis {
        let axis_num = i + 1;
        store.skyref[i] = cards.get_float_marked(&key_for("SREF", Some(axis_num), s));
        store.skyrefp[i] = cards.get_float_marked(&key_for("SREFP", Some(axis_num), s));
    }
```

Confirm `key_for` renders `SREFIS`/`SREF1`/`SREFP1` for the primary (`s == ' '`) and `SREFISA`/`SREF1A`/`SREFP1A` for `s == 'A'` — match whatever C `WcsFcRead` emits (`grep -n "SREFIS\|SREF%d\|SREFP" reference/starlink-ast/src/fitschan.c`). If C uses a different alt-name layout (e.g. `SREF1A` vs `SREFA1`), match C exactly.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core skyref_alt_reads_versioned_sref
```

Expected: PASS.

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the six `HEAD_ORACLE_EXCLUDED` rows (tidy-offsets ×2, tidy-pole ×2, skyref-alt ×2) and the nine `NATIVE_READ_EXCLUDED` rows (`tidy-offsets_native`, `tidy-pole_native`, `skyref-alt_native`, `axref_native`, `degen-wcsaxes_native`, `rfval_native`, `scp_native`, `sip-alt_native`, `zpx_native`).
If any of the shared-branch native cases still differ, its residual reason is a *different* root cause — restore only that one row with an updated reason and note it for a later task; do not restore a SREF reason.

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/wcs_keywords.rs crates/ast-core/tests/
git commit -m "fitschan: read SREFIS/SREF/SREFP with alt-plane version letter

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 4: A1 — LONPOLE default resolved by WcsNatPole, not forced to 180°

`wcs_celestial.rs:563` computes `let lonpole = store.lonpole.unwrap_or(180.0) * deg2rad;` and then passes `Some(lonpole)` to `build_native_to_celestial_rotation_for_wcsmap` at line 669.
This defeats the projection-aware default: `wcs_nat_pole` (`fset_to_store/sky_pole.rs:241`, mirroring `fitschan.c::WcsNatPole`) already resolves the LONPOLE default correctly when it receives the "no value supplied" sentinel (`build_native_to_celestial_rotation_for_wcsmap` passes `f64::INFINITY` for `None`, `wcs_celestial.rs:810`).
For θ0=0 projections (SFL/GLS, AIT, ...) the correct default longitude of the pole is 0°, not 180°; forcing 180° upstream gives the wrong pole and a mirrored transform.
C never hard-defaults LONPOLE to 180 before `WcsNatPole` — it passes `AST__BAD` and lets `WcsNatPole` choose per projection (`fitschan.c::WcsNatPole`).

**Fixtures cleared:** gls, ait-highlatpole (fwd+inv each). **NATIVE_READ_EXCLUDED:** `gls_native`, `ait-highlatpole_native`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:563` and `:664-671`
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `store.lonpole: Option<f64>`, `build_native_to_celestial_rotation_for_wcsmap(&WcsMap, f64, f64, Option<f64>, Option<f64>)` (already accepts `Option` and maps `None → +∞` sentinel at line 810).
- Produces: no new API.

- [ ] **Step 1: Write the failing read test**

Confirm gls.head has no LONPOLE card (`grep -i "LONPOLE" crates/ast-core/tests/fixtures/wcsconv/inputs/gls.head` → empty).
Decode gls.head and transform a pixel through the FrameSet, asserting the output matches the C oracle golden for that fixture.
Get the golden from the existing oracle report (the gls section in `headers.oracle` carries the reference outputs) or a probe:

```rust
#[test]
fn gls_lonpole_default_matches_c() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/gls.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Transform a mid-frame pixel forward and assert (lon, lat) match the C
    // oracle golden for gls.head (from headers.oracle / probe), tol 1e-9.
    // ... transform + asserts filled from a sibling celestial read test.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core gls_lonpole_default_matches_c
```

Expected: FAIL — the forced-180 pole mirrors the transform for this θ0=0 projection.

- [ ] **Step 3: Apply the fix**

At `wcs_celestial.rs:563`, stop hard-defaulting; keep the value optional in radians:

```rust
    // LONPOLE (degrees) when supplied; None lets WcsNatPole pick the
    // projection-aware default (fitschan.c::WcsNatPole), which is 0 for
    // theta0=0 projections and 180 otherwise.
    let lonpole = store.lonpole.map(|v| v * deg2rad);
```

Delete the now-unused `let _latpole = store.latpole.map(...)` at line 564 only if it is genuinely unused (it is recomputed at line 664 as `latpole_rad`); confirm with the compiler.
At the call site (`:664-671`), pass `lonpole` straight through:

```rust
    let latpole_rad = store.latpole.map(|v| v * deg2rad);
    let rotation = build_native_to_celestial_rotation_for_wcsmap(
        &wcsmap_fwd,
        crval_lon,
        crval_lat,
        lonpole,
        latpole_rad,
    )?;
```

Any other use of the old `lonpole: f64` binding between 563 and 671 must be updated (search the function body). If `lonpole` is consumed only at the call site, this is a clean swap.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core gls_lonpole_default_matches_c
```

Expected: PASS. Also spot-check a projection that DOES default LONPOLE to 180 (a zenithal TAN fixture) still passes, to confirm no regression:

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
```

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the four `HEAD_ORACLE_EXCLUDED` rows (gls ×2, ait-highlatpole ×2) and the two `NATIVE_READ_EXCLUDED` rows (`gls_native`, `ait-highlatpole_native`).

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/wcs_celestial.rs crates/ast-core/tests/
git commit -m "fitschan: let WcsNatPole resolve the LONPOLE default

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 5: A2 — generic celestial CTYPE recognition (xLON/xLAT/xyLN/xyLT) + System=Unknown

C `GetSkyAxes` (`fitschan.c:10809-10825`) recognises a celestial axis when `strlen(ctype) > 4 && ctype[4] == '-'`, then classifies:
- longitude: `RA--`, `AZ--`, `ctype+1 == "LON"` (i.e. `xLON`), or `ctype+2 == "LN"` (i.e. `xyLN`);
- latitude: `DEC-`, `EL--`, `ctype+1 == "LAT"` (i.e. `xLAT`), or `ctype+2 == "LT"` (i.e. `xyLT`).

The Rust `is_longitude_ctype`/`is_latitude_ctype` (`fits_store.rs:273-298`) hard-list specific 4-char prefixes and miss the generic `xLON`/`xyLN`/`xLAT`/`xyLT` families, so `unknown-cel`/`unknown-cel2` (CTYPEs like `xLON`/`xLAT` for an unknown celestial system) are not recognised as celestial and decode to a plain `Frame` instead of a `SkyFrame` with `System=Unknown`.

**Fixtures cleared:** unknown-cel, unknown-cel2 (fwd+inv each). **NATIVE_READ_EXCLUDED:** `unknown-cel_native`, `unknown-cel2_native`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/fits_store.rs:273-298`
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs` (System classification — set `System=Unknown` when the CTYPE matches only the generic family)
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: CTYPE strings; `SkyFrame` System attribute.
- Produces: no new API.

- [ ] **Step 1: Write the failing read test**

Confirm the CTYPEs (`grep -i "CTYPE" crates/ast-core/tests/fixtures/wcsconv/inputs/unknown-cel.head`).
Assert the decoded object is a SkyFrame with `System=Unknown` (or C's exact reported system for that fixture — read it from the oracle / probe) and that a forward transform matches the C golden:

```rust
#[test]
fn unknown_cel_builds_unknown_skyframe() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/unknown-cel.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Assert a SkyFrame is present with System=Unknown, and a mid-frame
    // pixel transforms to the C oracle golden (tol 1e-9).
    // ... extraction + asserts from a sibling celestial read test + probe.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core unknown_cel_builds_unknown_skyframe
```

Expected: FAIL — the axis is not recognised as celestial, so no SkyFrame is built.

- [ ] **Step 3: Apply the recognition fix**

Rewrite `is_longitude_ctype`/`is_latitude_ctype` to mirror `fitschan.c:10809-10825` exactly — require a `-` at index 4 and match the generic families. Keep the specific prefixes as they are subsumed by the generic test, but preserve the AZ--/EL--/OFLN/OFLT handling:

```rust
/// Check if a CTYPE value represents a celestial longitude axis.
///
/// Mirrors C `GetSkyAxes` (fitschan.c:10809-10819): a celestial axis has a
/// `-` as its fifth character; a longitude axis is `RA--`, `AZ--`, `xLON`
/// (chars 1..4 == "LON"), or `xyLN` (chars 2..4 == "LN").
fn is_longitude_ctype(ctype: &str) -> bool {
    let ct = ctype.trim();
    let b = ct.as_bytes();
    if b.len() <= 4 || b[4] != b'-' {
        return false;
    }
    ct.starts_with("RA--")
        || ct.starts_with("AZ--")
        || &ct[1..4] == "LON"
        || &ct[2..4] == "LN"
}

/// Check if a CTYPE value represents a celestial latitude axis.
///
/// Mirrors C `GetSkyAxes` (fitschan.c:10821-10825): latitude is `DEC-`,
/// `EL--`, `xLAT` (chars 1..4 == "LAT"), or `xyLT` (chars 2..4 == "LT").
fn is_latitude_ctype(ctype: &str) -> bool {
    let ct = ctype.trim();
    let b = ct.as_bytes();
    if b.len() <= 4 || b[4] != b'-' {
        return false;
    }
    ct.starts_with("DEC-")
        || ct.starts_with("EL--")
        || &ct[1..4] == "LAT"
        || &ct[2..4] == "LT"
}
```

Note the `OFLN`/`OFLT`/`HLON`/`SLON`/... prefixes are subsumed by `xLON`/`xLAT`; verify no existing test depended on a CTYPE with `len() <= 4` being celestial (that is A3's class-noproj case, which C also excludes here and handles via the CAR-linear branch — do not add a `len <= 4` path in this task). Slicing `ct[1..4]`/`ct[2..4]` is safe because the guard requires `b.len() > 4`.

- [ ] **Step 4: Apply the System=Unknown classification**

In `wcs_celestial.rs`, where the SkyFrame's System is chosen from the CTYPE prefix, add the C branch that classifies an unrecognised celestial family as `System=Unknown` (find the existing match on RA/GLON/ELON... and add the fallthrough).
Confirm the exact C system string with `grep -n "Unknown\|UNKNOWN\|astSetSystem" reference/starlink-ast/src/fitschan.c` around the SkyFrame construction, and that the Rust `SkyFrame` System enum has an `Unknown` variant (`grep -n "enum.*System\|Unknown" crates/ast-core/src/frames/skyframe.rs`); if it does not, this step also adds the variant faithfully.

- [ ] **Step 5: Run the test to verify it passes**

```bash
cargo test -p ast-core unknown_cel_builds_unknown_skyframe
cargo test -p ast-core   # full crate: recognition change is broad, confirm no regression
```

Expected: PASS, and no other read test regresses.

- [ ] **Step 6: Shrink both exclusion lists and verify the gates**

Remove the four `HEAD_ORACLE_EXCLUDED` rows (unknown-cel ×2, unknown-cel2 ×2) and the two `NATIVE_READ_EXCLUDED` rows (`unknown-cel_native`, `unknown-cel2_native`).

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/fits_store.rs crates/ast-core/src/fitschan/wcs_celestial.rs crates/ast-core/tests/
git commit -m "fitschan: recognise generic xLON/xLAT/xyLN/xyLT celestial CTYPEs

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 6: B2 — LogWcs (`-LOG` spectral algorithm) MathMap

C `LogWcs` (`fitschan.c:20777-20859`) builds a 1×1 `MathMap` for a `-LOG` spectral axis: `S = Sr·exp(w/Sr)` forward, `w = Sr·log(S/Sr)` inverse, where `Sr = CRVAL` (a default of 0 yields a NULL mapping — C returns NULL, the caller then leaves the axis linear).
The Rust spectral reader falls through the `else` arm at `wcs_spectral.rs:208-213` and emits a linear `ShiftMap`, so `freq-log` transforms linearly where C is logarithmic.

**Fixture cleared:** freq-log (inv). **NATIVE_READ_EXCLUDED:** `freq-log_native`.

**Files:**
- Create/Modify: `crates/ast-core/src/fitschan/wcs_spectral.rs` — add `fn log_wcs(crval: f64) -> Option<DynMapping>` and dispatch to it from the `else` arm when `algcode == "-LOG"`.
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `MathMap::new(nin: usize, nout: usize, fwd: &[&str], inv: &[&str]) -> AstResult<Self>`, `store.crval[axis]`, `algcode: String`.
- Produces: `fn log_wcs(crval: f64) -> Option<DynMapping>`.

- [ ] **Step 1: Write the failing read test**

Confirm freq-log.head has a `-LOG` CTYPE (`grep -i "CTYPE" crates/ast-core/tests/fixtures/wcsconv/inputs/freq-log.head`).
Assert the inverse transform (S → w) matches the C log golden:

```rust
#[test]
fn freq_log_builds_log_mapping() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/freq-log.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Transform a spectral coordinate through the inverse and assert it
    // equals Sr*log(S/Sr) with Sr=CRVAL, matching the C oracle golden (1e-9).
    // ... extraction + asserts from a sibling spectral read test + probe.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core freq_log_builds_log_mapping
```

Expected: FAIL — the linear ShiftMap gives the wrong inverse.

- [ ] **Step 3: Add `log_wcs` and dispatch**

Add the reader, mirroring `LogWcs` (`fitschan.c:20824-20856`) — the MathMap has `simpfi=1,simpif=1` in C; construct the equivalent forward/inverse expression strings with full `f64` precision:

```rust
/// Build the IWC→S mapping for a FITS-WCS paper III `-LOG` spectral axis.
///
/// Mirrors C `LogWcs` (fitschan.c:20777-20859): S = Sr*exp(w/Sr) forward,
/// w = Sr*log(S/Sr) inverse, where Sr is the CRVAL of the axis. A zero Sr
/// has no logarithmic form, so C returns NULL and the axis stays linear.
fn log_wcs(crval: f64) -> Option<DynMapping> {
    let sr = if crval == BAD_VALUE { 0.0 } else { crval };
    if sr == 0.0 {
        return None;
    }
    let forexp = format!("s={sr:.17}*exp(w/{sr:.17})");
    let invexp = format!("w={sr:.17}*log(s/{sr:.17})");
    let map = crate::mappings::MathMap::new(1, 1, &[&forexp], &[&invexp]).ok()?;
    Some(Box::new(map))
}
```

Confirm `BAD_VALUE` is imported in this module (it guards `store.crval[axis].unwrap_or(0.0)` upstream, so `crval` here is already resolved; if the upstream `unwrap_or(0.0)` already substitutes 0 for a missing CRVAL, drop the `BAD_VALUE` check and just test `== 0.0`, matching C's post-default state at line 20735).
Verify `MathMap::new` accepts the C `simpfi/simpif` simplification hints or that they are irrelevant to the transform (they only affect simplification, which is out of scope for the transform oracle — a comment noting this suffices).
Then wire the dispatch in the `else` arm at `wcs_spectral.rs:208-213`:

```rust
    } else if algcode == "-LOG" {
        match log_wcs(crval) {
            Some(m) => m,
            None => Box::new(ShiftMap::new(vec![crval])),
        }
    } else {
        // Any remaining algorithm code has no non-linear reader yet; a
        // linear shift keeps the FrameSet loadable.
        Box::new(ShiftMap::new(vec![crval]))
    };
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core freq_log_builds_log_mapping
```

Expected: PASS.

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the `freq-log.head` (false) row from `HEAD_ORACLE_EXCLUDED` and the `freq-log_native` row from `NATIVE_READ_EXCLUDED`.

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/wcs_spectral.rs crates/ast-core/tests/
git commit -m "fitschan: port LogWcs for -LOG spectral axes

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 7: B3 — GrismSpecWcs (`-GRI`/`-GRA` spectral algorithm)

C `GrismSpecWcs` (`fitschan.c:14306-14468`) builds a CmpMap around a `GrismMap` for a `-GRI` (vacuum wavelength) or `-GRA` (air wavelength) axis:
1. Copy the SpecFrame, set System to `WAVELEN` (`-GRI`) or `AIRWAVE` (`-GRA`) and Unit `"m"`; `astConvert` to get `smap` (S → wavelength).
2. `crv = CRVAL` (default 0); `astTran1(smap, crv) → wcrv` (reference wavelength in m).
3. Build a `GrismMap` and set G/M/Alpha/NR/NRP/Eps/Theta from `PVi_0..6` (degrees→radians for Alpha/Eps/Theta via `AST__DD2R`, defaults `G=0, M=0, Alpha=0, NR=1, NRP=0, Eps=0, Theta=0`), and `WaveR = wcrv`.
4. Invert the GrismMap (wavelength → grism param), compose `map1 = smap ∘ gmap_inv` (S → grism param).
5. `astTran1(gmap, wcrv) → gcrv`; `dg = astRate(map1, crv, 0, 0)`; if `dg` finite and nonzero, build `map2 = ZoomMap(dg) ∘ ShiftMap(gcrv)` (w → grism param), invert `map1`, and return `map2 ∘ map1_inv` (w → S).

The Rust reader falls through the `else` arm and emits a linear ShiftMap, so `grism` transforms linearly.

**Fixture cleared:** grism (inv). **NATIVE_READ_EXCLUDED:** `grism_native`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_spectral.rs` — add `fn grism_spec_wcs(algcode: &str, store, axis, specfrm) -> AstResult<Option<DynMapping>>` and dispatch from the `else` arm for `-GRI`/`-GRA`.
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `GrismMap::new(nr, nrp, wave_r, alpha, g, m: i32, eps, theta)`, `SpecFrame::clone/set_system/set_unit/convert_to`, `SpecSystem::{Wavelen, Airwave}`, `ZoomMap::new`, `ShiftMap::new`, `WinMap`/`CmpMap` composition, `Mapping::rate`, `store.crval[axis]`, `store.pv[axis][0..=6]`, `PointSet::from_rows`, `FrameRef`. Follow `nonlin_spec_wcs` (`wcs_spectral.rs:257-320`) verbatim for the convert/rate/compose idioms — it is the same shape.
- Produces: `fn grism_spec_wcs(...) -> AstResult<Option<DynMapping>>`.

- [ ] **Step 1: Write the failing read test**

Confirm grism.head CTYPE and PV cards (`grep -iE "CTYPE|PV" crates/ast-core/tests/fixtures/wcsconv/inputs/grism.head`).
Assert the inverse transform matches the C grism golden:

```rust
#[test]
fn grism_builds_grism_mapping() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/grism.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Transform a spectral coordinate through the inverse and assert it
    // matches the C oracle golden for grism.head (tol 1e-9).
    // ... extraction + asserts from a sibling spectral read test + probe.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core grism_builds_grism_mapping
```

Expected: FAIL — the linear ShiftMap gives the wrong transform.

- [ ] **Step 3: Add `grism_spec_wcs` and dispatch**

Port `fitschan.c:14386-14464` faithfully. Read each `PVi_m` from the store; convert the reference CRVAL through `smap`; set every GrismMap parameter with the documented default and degrees→radians conversion; compose exactly as C does. Build the GrismMap via `GrismMap::new` with all eight parameters (the constructor is the Rust equivalent of `astGrismMap` + the seven `astSetGrism*` calls), then invert it:

```rust
/// Build the IWC→S mapping for a FITS-WCS paper III `-GRI`/`-GRA` grism axis.
///
/// Mirrors C `GrismSpecWcs` (fitschan.c:14306-14468). `-GRI` disperses in
/// vacuum wavelength, `-GRA` in air wavelength. The returned mapping is
/// `w -> S`, built from a GrismMap whose parameters come from PVi_0..6.
fn grism_spec_wcs(
    algcode: &str,
    store: &FitsStore,
    axis: usize,
    specfrm: &SpecFrame,
) -> AstResult<Option<DynMapping>> {
    // 1. SpecFrame copy in wavelength (vacuum for -GRI, air for -GRA), unit m.
    let wave_system = if algcode == "-GRI" {
        SpecSystem::Wavelen
    } else {
        SpecSystem::Airwave
    };
    let mut wfrm = specfrm.clone();
    wfrm.set_system(wave_system);
    wfrm.set_unit("m");
    let Some(fs) = specfrm.convert_to(&wfrm, &[])? else {
        return Ok(None);
    };
    let smap = fs.mapping(FrameRef::Base, FrameRef::Current)?; // S -> wavelength

    // 2. Reference CRVAL (S) -> wavelength (m).
    let crv = store.crval[axis].unwrap_or(0.0);
    let wcrv = {
        let out = smap.transform(&PointSet::from_rows([[crv]]), true)?;
        out.point(0)[0]
    };

    // 3. GrismMap from PVi_0..6 (defaults + deg->rad for Alpha/Eps/Theta).
    let pv = |m: usize| store.pv_at(axis, m); // -> Option<f64>; confirm accessor name
    let g = pv(0).unwrap_or(0.0);
    let m_order = pv(1).map(|v| (v + 0.5) as i32).unwrap_or(0);
    let alpha = pv(2).map(|v| v * DD2R).unwrap_or(0.0);
    let nr = pv(3).unwrap_or(1.0);
    let nrp = pv(4).unwrap_or(0.0);
    let eps = pv(5).map(|v| v * DD2R).unwrap_or(0.0);
    let theta = pv(6).map(|v| v * DD2R).unwrap_or(0.0);
    let mut gmap = crate::mappings::GrismMap::new(nr, nrp, wcrv, alpha, g, m_order, eps, theta);

    // 4. wavelength -> grism param; map1 = S -> grism param.
    gmap.invert();
    let map1 = crate::mappings::CmpMap::new(Box::new(smap.clone()), Box::new(gmap.clone()), true)?; // confirm CmpMap::new series-flag

    // 5. gcrv, dg, w->S.
    let gcrv = {
        let out = gmap.transform(&PointSet::from_rows([[wcrv]]), true)?;
        out.point(0)[0]
    };
    let dg = match map1.rate(&[crv], 1, 1) {
        Ok(r) if r != 0.0 && r.is_finite() => r,
        _ => return Ok(None),
    };
    let map2a = ZoomMap::new(1, dg)?;
    let map2b = ShiftMap::new(vec![gcrv]);
    let map2 = crate::mappings::CmpMap::new(Box::new(map2a), Box::new(map2b), true)?;
    let mut map1_inv = map1;
    map1_inv.invert();
    let ret = crate::mappings::CmpMap::new(Box::new(map2), Box::new(map1_inv), true)?;
    Ok(Some(Box::new(ret)))
}
```

The accessor names (`store.pv_at`, `CmpMap::new` signature and its series flag, whether `SpecSystem::Airwave` exists) must be confirmed against the codebase and adjusted — the algorithm is fixed by C, only the Rust spelling varies. Confirm `DD2R` (π/180) is available (`grep -rn "DD2R\|const.*deg2rad" crates/ast-core/src`); reuse the existing constant, do not redefine.
Wire the dispatch in the `else` arm before the final catch-all:

```rust
    } else if algcode == "-GRI" || algcode == "-GRA" {
        match grism_spec_wcs(&algcode, store, axis, &frame) {
            Ok(Some(m)) => m,
            _ => Box::new(ShiftMap::new(vec![crval])),
        }
    } else if algcode == "-LOG" {
        // (from Task 6)
        ...
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core grism_builds_grism_mapping
```

Expected: PASS.

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the `grism.head` (false) row from `HEAD_ORACLE_EXCLUDED` and the `grism_native` row from `NATIVE_READ_EXCLUDED`.

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/wcs_spectral.rs crates/ast-core/tests/
git commit -m "fitschan: port GrismSpecWcs for -GRI/-GRA grism axes

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 8: C3 — PolyTan auto-detect (TAN → TPN when PVi_m distortion present)

The PolyTan FitsChan attribute defaults to `-1` in C (`fitschan.c:42816`), meaning "auto-detect".
`GetUsedPolyTan` (`fitschan.c:13986-14169`) resolves the auto case: search the FitsChan for `PVi_m` cards; if any are found on the latitude axis, or any on the longitude axis with `m > 4`, use the distorted-TAN convention (return 1) — which drives the `TAN → TPN` CTYPE rewrite.
It then guards: if all distortion `PVi_m` (m ≥ 1) on the deciding axis are zero, it warns and returns 0 (standard TAN), deleting the PV cards.
The Rust `translate_tpv_and_polytan_to_tpn` (`spec_trans.rs:640-661`) handles only the `-TPV` rename and deliberately omits the PolyTan branch (comment at 659-660), so `tan-polytan` stays TAN where C promotes it to TPN.

**Fixture cleared:** tan-polytan (fwd+inv). **NATIVE_READ_EXCLUDED:** `tan-polytan_native`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs:640-662`
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `CardList::get_float`, `CardList::find`, `CardList::delete`, `celestial_axes(cards, s)`, `projection_suffix`, `replace_projection_suffix`.
- Produces: `fn get_used_poly_tan(cards, axlon, axlat, s) -> bool` (private to spec_trans).

- [ ] **Step 1: Write the failing read test**

Confirm tan-polytan.head is TAN with PV distortion (`grep -iE "CTYPE|PV" crates/ast-core/tests/fixtures/wcsconv/inputs/tan-polytan.head`).
Assert the decoded projection is TPN (distorted), verifiable by a forward transform matching the C oracle golden:

```rust
#[test]
fn tan_polytan_promotes_to_tpn() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/tan-polytan.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Transform a mid-frame pixel forward; assert it matches the C oracle
    // golden for tan-polytan.head (the TPN distortion moves the output away
    // from the plain-TAN result), tol 1e-9.
    // ... transform + asserts from a sibling celestial read test + probe.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core tan_polytan_promotes_to_tpn
```

Expected: FAIL — the plain-TAN projection ignores the PV distortion.

- [ ] **Step 3: Port `GetUsedPolyTan` and fire the rename**

Add the auto-detect helper mirroring `fitschan.c:14030-14169` (PolyTan default `-1` = auto; positive values are an explicit AST attribute we do not expose, so the auto path is the whole behaviour here). Then call it from `translate_tpv_and_polytan_to_tpn` for the TAN case:

```rust
/// Resolve the auto (`PolyTan = -1`) decision from the PV cards, mirroring
/// C `GetUsedPolyTan` (fitschan.c:14030-14169). Returns true when the
/// distorted-TAN (TPN) convention applies: a PVi_m on the latitude axis, or
/// a PVi_m with m > 4 on the longitude axis. Returns false when all
/// distortion coefficients (m >= 1) on the deciding axis are zero.
fn get_used_poly_tan(cards: &CardList, axlon: usize, axlat: usize, s: char) -> bool {
    let pv_name = |ax: usize, m: usize| {
        if s == ' ' {
            format!("PV{ax}_{m}")
        } else {
            format!("PV{ax}_{m}{s}")
        }
    };
    // Highest m present on each axis (C uses astKeyFields for ubnd).
    let ubnd = |ax: usize| -> Option<usize> {
        (0..=99).filter(|&m| cards.find(&pv_name(ax, m)).is_some()).max()
    };
    let ubnd_lat = ubnd(axlat);
    let ubnd_lon = ubnd(axlon);
    let nfound1 = ubnd_lat.is_some();
    let nfound2 = ubnd_lon.is_some();

    // Distorted TAN if any latitude PV, or a longitude PV with m > 4.
    if !(nfound1 || (nfound2 && ubnd_lon.is_some_and(|u| u > 4))) {
        return false;
    }

    // Non-zero-coefficient guard on the deciding axis (latitude first, then
    // longitude), skipping the constant PVi_0. Missing PVi_m defaults to
    // 1.0 for m == 1, else 0.0 (fitschan.c:14117).
    let axis_has_nonzero = |ax: usize, ubnd: usize| -> bool {
        (1..=ubnd).any(|m| {
            let pval = cards
                .get_float(&pv_name(ax, m))
                .unwrap_or(if m == 1 { 1.0 } else { 0.0 });
            pval != 0.0
        })
    };
    if let Some(u) = ubnd_lat {
        return axis_has_nonzero(axlat, u);
    }
    if let Some(u) = ubnd_lon {
        return axis_has_nonzero(axlon, u);
    }
    false
}
```

Confirm the `astKeyFields` upper-bound `m` cannot exceed the scan range (99 covers FITS PV limits; widen if a fixture uses more). C also deletes the zero-coefficient PV cards and warns; the delete only matters for the *written* output — port it if the wcsconv byte-compare needs it, otherwise a comment noting the omission is acceptable only if the native compare still passes (verify in Step 5).
Then in `translate_tpv_and_polytan_to_tpn`, replace the omitted-branch comment (659-660) with the TAN case:

```rust
        } else if projection_suffix(ctype.trim())
            .is_some_and(|p| p.eq_ignore_ascii_case("TAN"))
            && get_used_poly_tan(cards, axlon, axlat, s)
        {
            replace_projection_suffix(cards, &key("CTYPE", axlon), "TPN");
            replace_projection_suffix(cards, &key("CTYPE", axlat), "TPN");
        }
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core tan_polytan_promotes_to_tpn
```

Expected: PASS.

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the two `tan-polytan.head` rows from `HEAD_ORACLE_EXCLUDED` and the `tan-polytan_native` row from `NATIVE_READ_EXCLUDED`.
If `tan-polytan_native` still differs, the zero-coefficient PV-delete branch (fitschan.c:14133-14145) matters for the written cards — port it and re-run.

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/spec_trans.rs crates/ast-core/tests/
git commit -m "fitschan: auto-detect distorted TAN (PolyTan=-1) -> TPN

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 9: A3 — CAR-linear branch for a celestial CTYPE with no projection code

C `WcsCelestial` (`fitschan.c:35878-35886`) treats a celestial axis whose CTYPE is ≤ 4 chars (no `-PRJ` suffix) as `AST__CAR` with `carlin = 1` — a plain-linear CAR.
The build then (`fitschan.c:36037-36069`) uses a `WcsMap` with `AST__WCSBAD` (no projection) wrapped in a `WinMap` that adds CRVAL per axis, so pixel→celestial is linear.
Rust bails at `wcs_celestial.rs:121` (`store.projection_code(lon_axis)?` returns `None` for `RA--`/`DEC-`), so `class-noproj` decodes to a non-celestial `Frame` and its transform is wrong.

This task ports the `carlin`-implied path for the *no-projection-code* case (the explicit `CarLin` attribute for a CAR *with* a code is B1/Task 10).

**Fixture cleared:** class-noproj (fwd+inv). **NATIVE_READ_EXCLUDED:** `class-noproj_native`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:118-131` (detect no-proj-code celestial and take the CAR-linear branch)
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `store.celestial_axes()`, `store.ctype[axis]`, `store.crval[axis]`, `WinMap::from_corners`, `WcsMap::new` (with a no-projection variant), `SkyFrame`.
- Produces: a shared CAR-linear mapping builder, reused by Task 10.

- [ ] **Step 1: Write the failing read test**

Confirm class-noproj.head has short CTYPEs (`grep -i "CTYPE" crates/ast-core/tests/fixtures/wcsconv/inputs/class-noproj.head` → `RA--`/`DEC-` with no projection suffix).
Assert the decoded object is a celestial SkyFrame and a mid-frame pixel transforms linearly to the C golden:

```rust
#[test]
fn class_noproj_builds_car_linear() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/class-noproj.head"
    ))
    .unwrap();
    let obj = ast_core::FitsChan::from_header(&text).unwrap().read().unwrap();
    // Assert a SkyFrame is present and a mid-frame pixel maps to the C oracle
    // golden for class-noproj.head (linear CAR: IWC + CRVAL), tol 1e-9.
    // ... extraction + asserts from a sibling celestial read test + probe.
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core class_noproj_builds_car_linear
```

Expected: FAIL — `projection_code` returns None so no celestial frame is built.

- [ ] **Step 3: Detect the no-proj-code case and take the CAR-linear branch**

At `wcs_celestial.rs:120-122`, when `celestial_axes()` succeeds but the CTYPE has no projection code, C sets `prj = AST__CAR, carlin = 1` (`fitschan.c:35883-35885`). Replace the unconditional `?` bail with the C branch:

```rust
    // Step 2: Get the projection code. A celestial CTYPE with no `-PRJ`
    // suffix (<= 4 chars) is a linear CAR (fitschan.c:35878-35886:
    // `prj = AST__CAR; carlin = 1`).
    let (proj, carlin) = match store.projection_code(lon_axis) {
        Some(code) => (WcsProjection::from_code(&code)?, false),
        None => (WcsProjection::Car, true),
    };
```

Then, where the pixel→IWC→celestial pipeline is assembled, branch on `carlin` to build the linear form (`fitschan.c:36037-36069`): a `WcsMap` with `WcsProjection::Unknown` (AST__WCSBAD, no projection) plus a `WinMap` adding `CRVAL·DD2R` to the celestial axes. Extract this into a helper so Task 10 reuses it:

```rust
/// Build the CAR-linear celestial mapping (pixel-linear CAR): a
/// no-projection WcsMap plus a WinMap adding CRVAL (radians on the celestial
/// axes) per axis. Mirrors C WcsCelestial's carlin path (fitschan.c:36037-
/// 35939): `map2 = WcsMap(AST__WCSBAD)`, `map3 = WinMap(ina=0,inb=1 ->
/// outa=CRVAL,outb=1+CRVAL)`.
fn build_car_linear_mapping(
    store: &FitsStore,
    lon_axis: usize,
    lat_axis: usize,
) -> Option<DynMapping> {
    let naxes = store.naxis;
    let deg2rad = PI / 180.0;
    let wcs = WcsMap::new(naxes, WcsProjection::Unknown, lon_axis + 1, lat_axis + 1).ok()?;
    let mut ina = vec![0.0; naxes];
    let mut inb = vec![1.0; naxes];
    let mut outa = vec![0.0; naxes];
    let mut outb = vec![1.0; naxes];
    for (ax, is_cel) in [(lon_axis, true), (lat_axis, true)] {
        let _ = is_cel;
        if let Some(crval) = store.crval[ax] {
            let refv = crval * deg2rad; // latprj == CAR: convert to radians
            outa[ax] = refv;
            outb[ax] = 1.0 + refv;
        } else {
            outa[ax] = BAD_VALUE;
            outb[ax] = BAD_VALUE;
        }
    }
    let win = WinMap::from_corners(&ina, &inb, &outa, &outb).ok()?;
    // series compose wcs then win
    let cmp = crate::mappings::CmpMap::new(Box::new(wcs), Box::new(win), true).ok()?;
    Some(Box::new(cmp))
}
```

Confirm the exact CmpMap/WinMap ordering against C (`map1` degrees→radians zoom, `map2` WcsMap, `map3` WinMap composed in series — read `fitschan.c:35950-36090` in full before finalizing the compose order; the snippet above is the CAR-linear core, but the surrounding degrees-to-radians `map1` and the `SkyFrame` assembly must match the recognised-projection path in `build_celestial_with_def_b1950`). The `is_cel` scaffolding is illustrative — mirror C's actual per-axis loop (all axes get ina=0,inb=1,outa=0,outb=1; only the celestial axes get CRVAL added).

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core class_noproj_builds_car_linear
cargo test -p ast-core   # recognition/build change is broad; confirm no regression
```

Expected: PASS, no regression.

- [ ] **Step 5: Shrink both exclusion lists and verify the gates**

Remove the two `class-noproj.head` rows from `HEAD_ORACLE_EXCLUDED` and the `class-noproj_native` row from `NATIVE_READ_EXCLUDED`.

```bash
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle
cargo test -p ast-core --test test_wcsconv
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add crates/ast-core/src/fitschan/wcs_celestial.rs crates/ast-core/tests/
git commit -m "fitschan: CAR-linear branch for celestial CTYPE with no projection code

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 10: B1 — `CarLin` FitsChan attribute (explicit CAR-linear for a CAR *with* a projection code)

car4 has CTYPE `GLON-CAR` (a real `-CAR` projection code), so it does not take the no-proj-code path of Task 9.
Instead it relies on the `CarLin` FitsChan attribute: C `WcsCelestial` reads `carlin = astGetCarLin(this)` (`fitschan.c:35813`) and, when set, treats even a coded `-CAR` as the linear branch (`fitschan.c:36038` `if( ( latprj == AST__CAR && carlin ) || latprj == AST__WCSBAD )`).
The `CarLin` attribute defaults to `-1` → `0` (`fitschan.c:42716-42719`): unset means "off" (a coded CAR uses the full spherical CAR projection).
The wcsconv manifest supplies `carlin=1` for car4 (`wcsconv_tests.txt:35`), so its byte-exact native output — and its transform-oracle golden — were generated with `CarLin=1`.

**Investigation gate (do this first, before any code):** the transform-oracle harness loads `.head` fixtures with a bare `FitsChan::from_header(text)?.read()` and applies no attributes (`test_transform_oracle.rs:196-205`). If car4's oracle golden reflects `CarLin=1`, the harness must apply `CarLin=1` for car4 before `read()`, or the fixture can never match regardless of the port. Confirm with a C-oracle probe: read car4.head through the repo `libast.so` both with and without `astSetCarLin(fc, 1)` and compare against the recorded golden (`headers.oracle:628-680`). Record which one the golden matches. This decides whether Task 10 also touches the harness.

**Fixture cleared:** car4 (fwd+inv). **NATIVE_READ_EXCLUDED:** `car4_native` (and it must not regress `car1_native`/`car2_native`, which are already excluded for a *different* reason — `SREF1: CAR fiducial longitude` — and also carry `carlin=1`).

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs` — add `carlin: Option<bool>` field (mirroring `-1`→unset), a `set_carlin`/getter, initialise in `new()` and `from_header()`.
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs` — thread `carlin` into `build_celestial_with_def_b1950` and take the CAR-linear branch (reusing Task 9's `build_car_linear_mapping`) when `latprj == CAR && carlin`.
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs` — pass `carlin` through `decode_wcs_with_def_b1950` (mirror the `def_b1950` threading).
- Modify (if the gate requires): `crates/ast-core/tests/test_transform_oracle.rs` — apply `CarLin=1` for car4 in `load_fixture`.
- Test: the FitsChan read-path test module

**Interfaces:**
- Consumes: `build_car_linear_mapping(store, lon_axis, lat_axis)` from Task 9 (A3) — the shared CAR-linear builder (WcsMap(WCSBAD) + WinMap adding CRVALₙ·DD2R). Task 9 MUST land before Task 10.
- Consumes: `def_b1950` threading pattern (`build_celestial_with_def_b1950` / `decode_wcs_with_def_b1950`) as the template for a second threaded parameter.
- Produces: `FitsChan::set_carlin(&mut self, on: bool)` typed setter (per [[feedback_typed_setters_no_string_layer]] — no string attribute layer in ast-core).

- [ ] **Step 1: C-oracle probe to close the investigation gate**

Build a standalone probe against the repo `libast.so` (mind the conda-RPATH trap — link with `-Wl,-rpath,<repo>/reference/starlink-ast/build-ast --disable-new-dtags`, see [[reference_oracle_conda_rpath_trap]]). Read car4.head twice:

```c
/* probe_car4.c */
AstFitsChan *fc = astFitsChan(NULL, NULL, "");
/* feed car4.head lines via astPutFits ... */
AstFrameSet *fs_off = astRead(fc);              /* CarLin unset (default 0) */
/* re-load, then: */
astSetI(fc2, "CarLin", 1);
AstFrameSet *fs_on = astRead(fc2);              /* CarLin=1 */
/* astTran2 both over the golden's input points; print outputs */
```

Compare both against the recorded golden at `headers.oracle:628-680`. Expected: the golden matches `CarLin=1` (linear), confirming the manifest. Record the result in a comment. **If it matches CarLin=1**, the harness must apply the attribute (Step 6 below is required). **If it matches the default**, drop Step 6 and the fix is purely making the coded-CAR default path correct.

- [ ] **Step 2: Write the failing test**

In the FitsChan read-path test module, read car4.head, apply `CarLin=1`, transform the golden's forward input points, and assert they match the golden outputs (linear CAR). Without the fix the CAR-with-code path runs full spherical CAR → mismatch.

```rust
#[test]
fn car4_carlin_produces_linear_car() {
    let text = include_str!("fixtures/.../car4.head");
    let mut fc = FitsChan::from_header(text).unwrap();
    fc.set_carlin(true);
    let obj = fc.read().unwrap();
    let fs = obj.as_frame_set().unwrap();
    // forward-transform the golden input, assert linear-CAR output within 1 ULP
    let out = fs.tran2(&[/* golden lon inputs */], &[/* golden lat inputs */], true).unwrap();
    assert_close(&out, &[/* golden linear outputs from headers.oracle */]);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p ast-core car4_carlin_produces_linear_car -- --nocapture`
Expected: FAIL — outputs are the full spherical CAR projection, not linear.

- [ ] **Step 4: Add the `carlin` field + setter and thread it**

`mod.rs`: add `carlin: Option<bool>` to the struct, initialise `None` in `new()`/`from_header()`, add:

```rust
/// Set the CarLin attribute: treat a coded `-CAR` projection as a linear
/// mapping (C FitsChan CarLin, fitschan.c:36038). Default unset = off.
pub fn set_carlin(&mut self, on: bool) { self.carlin = Some(on); }
fn get_carlin(&self) -> bool { self.carlin.unwrap_or(false) }  // C -1 -> 0
```

`wcs_read.rs`: extend `decode_wcs_with_def_b1950` to also accept `carlin: bool` (or read it from `self` at the call site) and pass into `build_celestial_with_def_b1950`.

`wcs_celestial.rs`: at the projection dispatch, take the CAR-linear branch when `latprj == WcsProjection::Car && carlin` (mirror `fitschan.c:36038` — the `|| latprj == WCSBAD` half is already Task 9's no-code path):

```rust
if (matches!(proj, WcsProjection::Car) && carlin) || matches!(proj, WcsProjection::Unknown) {
    return build_car_linear_mapping(store, lon_axis, lat_axis);
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p ast-core car4_carlin_produces_linear_car`
Expected: PASS.

- [ ] **Step 6: (only if Step 1 gate requires) apply CarLin=1 in the transform-oracle harness for car4**

In `load_fixture` (`test_transform_oracle.rs:196`), after building the FitsChan and before `read()`, set `CarLin=1` for car4 (keyed on relpath). Mirror however per-fixture attribute application is expressed — if none exists yet, add a minimal `carlin` set keyed on the fixtures known to need it (car1/car2/car4, all `carlin=1` in the manifest). Do NOT hardcode a blanket default; the attribute is per-fixture, exactly as the manifest is.

- [ ] **Step 7: Shrink both exclusion lists and run both gates**

Remove `car4_native` from `NATIVE_READ_EXCLUDED` (`test_wcsconv.rs`). Remove car4 from `HEAD_ORACLE_EXCLUDED` (Task 1). Run:

```bash
cargo test -p ast-core --test test_wcsconv
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv car4 -- --nocapture
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle -- --ignored --nocapture
```

Expected: car4 native byte-exact; car4 transform-oracle fwd+inv pass; no stale-exclusion failure. car1/car2 still excluded (different reason) and unregressed.

- [ ] **Step 8: Format, clippy, commit**

```bash
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings   # isolated target dir + RUSTC override per reference_clippy_toolchain_mismatch
git add -A && git commit -m "fitschan: CarLin attribute treats coded -CAR as linear (car4)

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 11: C4 — `SkyRefIs` applied through the FrameSet (skyref, M0 matrix/offset)

skyref's native diff is bucketed `M0: matrix/offset` (`test_wcsconv.rs`) — the pixel→sky Mapping the reader produces differs from C in the matrix/offset that maps to the *offset* sky system. The transform-oracle symptom is a forward/inverse numeric drift in the offset-coordinate region.

**⚠️ This task reverses a documented decision.** `wcs_read.rs:593-607` currently sets `SkyRefIs` *directly on the Frame* (`apply_skyrefis(f, skyrefis)` via `frame_mut`), with a comment explaining that applying it *through* the FrameSet would re-fuse the Mapping and "lose the offset description". C `TidyOffsets` (`fitschan.c:34594-34598`) does the opposite: it sets `SkyRefIs` through the FrameSet so the FrameSet integrity machinery re-maps the SkyFrame to offset coordinates. The current Rust comment asserts the header's SkyFrame *already* carries the offset Mapping — if that assertion is wrong (or wrong for skyref specifically), the direct-set is the bug and the M0 mismatch is its symptom.

**Investigation gate (mandatory, before any code):** per [[feedback_c_logic_beats_existing_rust]] and [[feedback_no_corner_cuts_on_c]], do not trust the existing Rust comment. C-oracle probe required:

1. Read skyref.head through the repo `libast.so` (conda-RPATH trap: [[reference_oracle_conda_rpath_trap]]).
2. In C, dump the pixel→sky Mapping *and* the SkyFrame's `SkyRefIs`/`SkyRef` after `astRead` — before and after `TidyOffsets` runs (or instrument `TidyOffsets` via an internal-API probe, [[reference_oracle_internal_api]]).
3. Determine whether C's post-read Mapping equals the one the header's linear cards describe directly, or a re-fused one produced by FrameSet integrity.
4. Compare the Rust reader's Mapping (direct-set path) against C's. The `M0` bucket says they already differ.

Record the finding. Two outcomes:
- **C re-maps through the FrameSet** → the Rust direct-set is the bug. Change `wcs_read.rs:605-606` to set `SkyRefIs` through the FrameSet (via the integrity path, not `frame_mut`), rewrite the 593-604 comment to describe the corrected behavior, and re-verify the tidy-offsets / tidy-pole / skyref-alt fixtures (cleared by Task 3) do not regress. Update [[project_native_serialization_corpus]] / the c-library-quirks doc if a quirk is involved.
- **C keeps the direct Mapping** → the M0 mismatch is elsewhere (the offset matrix construction upstream); re-scope this task to that locus. Do NOT change the direct-set.

**Fixture cleared:** skyref (fwd+inv). **NATIVE_READ_EXCLUDED:** `skyref_native`. **Must not regress:** tidy-offsets, tidy-pole, skyref-alt (Task 3), and any absolute/offset FrameSet fixture.

**Files:**
- Modify (outcome-dependent): `crates/ast-core/src/fitschan/wcs_read.rs:593-607` — either the `apply_skyrefis` call site (through-FrameSet vs direct) or the upstream offset-matrix builder.
- Test: transform-oracle head gate + a targeted read-path test asserting skyref's pixel→sky Mapping matches the C golden.

**Interfaces:**
- Consumes: `apply_skyrefis(frame, skyrefis)` and the FrameSet integrity path (`FrameSet` `set_*` that re-maps on Frame-attribute mutation, per CLAUDE.md "FrameSet integrity").
- Produces: nothing new consumed by later tasks.

- [ ] **Step 1: C-oracle probe (investigation gate above). Record the outcome in a comment on the fix.**
- [ ] **Step 2: Write the failing test** — read skyref.head, transform the golden's offset-region input points, assert against the skyref golden in `headers.oracle`. Fails on the M0 drift.
- [ ] **Step 3: Run to verify it fails.** `cargo test -p ast-core skyref_offset_mapping -- --nocapture` → FAIL.
- [ ] **Step 4: Apply the outcome-dependent fix** (through-FrameSet SkyRefIs, or upstream offset-matrix correction). Rewrite the `wcs_read.rs:593-604` comment to match the corrected behavior — describe the code as it is today, no history.
- [ ] **Step 5: Run to verify it passes**, and run Task 3's fixtures (tidy-offsets/tidy-pole/skyref-alt) to confirm no regression.
- [ ] **Step 6: Shrink both exclusion lists, run both gates** (`test_wcsconv` for `skyref_native`; `head_sections_match_oracle`).
- [ ] **Step 7: Format, clippy, commit** — `"fitschan: apply SkyRefIs through FrameSet for offset sky (skyref)"` (+ AI trailer).

---

### Task 12: C5 — spectral `SFT1`/`SREF2` linearisation via `astRate` (freq-wav, wave-f2w)

freq-wav and wave-f2w are bucketed `SFT1`/`SREF2` in `test_wcsconv.rs` — the ShiftMap/scale the reader builds to linearise a non-linear spectral CTYPE (freq↔wavelength) differs from C. C `SpecTrans`/`WcsSpectral` linearises a non-linear spectral axis at the reference point using `astRate` (the analytic-where-possible, finite-difference-elsewhere derivative), then builds a `ZoomMap(rate) ∘ ShiftMap(crval)` around it — exactly the `nonlin_spec_wcs` template (`wcs_spectral.rs:257-320`). The `SFT1`/`SREF2` mismatch means the Rust linearisation point, rate, or shift diverges from C's.

**Investigation gate:** determine whether the divergence is (a) the `astRate` value (derivative computed at a different point, or Rust's `astRate` port drifting from C's finite-difference seed — cf. [[project_vradv2f_astrate_dependency]] where an astRate x0=0 seed fix was the real cause), or (b) the reference-point/shift value fed into the ZoomMap∘ShiftMap. Probe: read freq-wav.head and wave-f2w.head through the repo `libast.so`, dump the linearised spectral Mapping's scale+offset, and compare against the Rust reader's. Then, if the scale differs, call the repo `astRate` directly at the same `(map, coord, at)` and compare to the Rust `astRate` — isolating (a) vs (b).

**Fixtures cleared:** freq-wav, wave-f2w (fwd+inv). **NATIVE_READ_EXCLUDED:** `freq_wav_native` (`SFT1`), `wave_f2w_native` (`SREF2`).

**Files:**
- Modify (outcome-dependent): `crates/ast-core/src/fitschan/wcs_spectral.rs` — the `nonlin_spec_wcs` linearisation (rate/shift construction) around line 257-320, OR the `astRate` seed/step if the probe points at the derivative itself.
- Test: transform-oracle head gate + targeted spectral read-path test.

**Interfaces:**
- Consumes: `nonlin_spec_wcs(store, ...)` (`wcs_spectral.rs:257`), `astRate` (the Rust `Mapping::rate` port).
- Produces: nothing new for later tasks.

- [ ] **Step 1: C-oracle probe (gate above); isolate astRate value vs reference/shift. Record.**
- [ ] **Step 2: Write the failing test** — read each head, transform the golden spectral inputs, assert against the golden. Fails on SFT1/SREF2 drift.
- [ ] **Step 3: Run to verify it fails.**
- [ ] **Step 4: Apply the faithful fix** at the probe-identified locus (match C's linearisation point/rate exactly; if astRate seed, mirror C's finite-difference initialisation per [[project_vradv2f_astrate_dependency]]).
- [ ] **Step 5: Run to verify it passes**; confirm no regression to freq-log (Task 6), grism (Task 7), or other spectral fixtures.
- [ ] **Step 6: Shrink both exclusion lists, run both gates.**
- [ ] **Step 7: Format, clippy, commit** — `"fitschan: match C astRate linearisation for non-linear spectral (freq-wav, wave-f2w)"` (+ AI trailer).

---

### Task 13: C6 — MatrixMap inverse for near-singular 3-D rotation (crota-3d, INV matrix inverse)

crota-3d is bucketed `INV: matrix inverse` in `test_wcsconv.rs` — the forward transform matches but the inverse (sky→pixel) diverges. crota-3d is a 3-axis header with a CROTA rotation coupling two celestial axes plus a third (spectral/degenerate) axis; the CD/PC → MatrixMap the reader builds is near-singular or has a third-axis coupling whose inverse C computes differently.

**Investigation gate:** the `INV` bucket means the forward MatrixMap is correct but its inverse is not. Determine whether (a) the Rust MatrixMap inverse (matrix inversion via `pal::dmat` FFI, cf. [[project_rigby_winmat_trace]]) diverges from C's `astMtrInv`/wcslib inversion for this matrix, or (b) the *matrix itself* is built differently (wrong third-axis coupling, degenerate axis handling) so both fwd+inv start from a different matrix but only inv shows it numerically. Probe: read crota-3d.head through the repo `libast.so`, dump the pixel→sky MatrixMap's forward matrix and its inverse matrix; compare both against the Rust reader's. If forward matrices are bit-equal but inverses differ → (a) inversion routine. If forward matrices differ → (b) construction.

**Note on the singular case:** C MatrixMap marks the inverse undefined when the matrix is singular (`matrixmap.c` — inverse transform returns BAD). If crota-3d's matrix is *genuinely* singular in C, the golden inverse outputs are BAD and the Rust port must also produce BAD (per the BAD_VALUE output contract). Confirm from the golden at `headers.oracle` whether crota-3d's inverse golden is finite or BAD — this decides whether the fix is "compute the inverse like C" or "declare it undefined like C".

**Fixture cleared:** crota-3d (fwd+inv). **NATIVE_READ_EXCLUDED:** `crota_3d_native`.

**Files:**
- Modify (outcome-dependent): `crates/ast-core/src/mappings/matrix.rs` (inverse computation / singularity flag), OR `crates/ast-core/src/fitschan/wcs_read.rs` / the CD→MatrixMap builder (third-axis coupling).
- Test: transform-oracle head gate + targeted inverse-transform test.

**Interfaces:**
- Consumes: MatrixMap construction from CD/PC cards, `pal::dmat` inversion FFI ([[project_rigby_winmat_trace]]), BAD_VALUE contract.
- Produces: nothing new for later tasks (this is the last Phase 4 task).

- [ ] **Step 1: C-oracle probe (gate above); compare fwd matrix + inv matrix; check golden inv finite-vs-BAD. Record.**
- [ ] **Step 2: Write the failing test** — read crota-3d.head, run the *inverse* transform on the golden sky inputs, assert against the golden pixel outputs (finite or BAD as the probe determined). Fails on the INV drift.
- [ ] **Step 3: Run to verify it fails.**
- [ ] **Step 4: Apply the faithful fix** — match C's matrix construction (if fwd differs) or C's inversion/singularity handling (if only inv differs). No tolerance heuristics ([[feedback_no_corner_cuts_on_c]]); mirror C control flow.
- [ ] **Step 5: Run to verify it passes**; confirm no regression to other CD/PC/MatrixMap fixtures.
- [ ] **Step 6: Shrink both exclusion lists, run both gates.**
- [ ] **Step 7: Format, clippy, commit** — `"fitschan: match C MatrixMap inverse for near-singular 3-D rotation (crota-3d)"` (+ AI trailer).

---

### Task 14: C1x — swapped-axis celestial (lat on axis 1, lon on axis 2) mis-routes the native→celestial rotation

serpens.head is `CTYPE1='DEC--TAN'` (latitude on FITS axis 1), `CTYPE2='RA---TAN'` (longitude on FITS axis 2) — the celestial axes are transposed relative to the usual RA/DEC order. The forward pixel→sky transform diverges from C by a gross rotation error (not precision drift): serpens pixel `(150, 120)` forward gives Rust `(-0.021491650, 1.460209855)` vs oracle `(0.018049750, -1.440691695)` — the latitude axis sign flips. The transform-oracle report records serpens fwd `nmiss=48 worst_rel≈3.1`, inverse `nmiss=86`.

This is the residual serpens transform-oracle failure that the C1 DUT1 fix (Task 2) did *not* clear — DUT1 only touches the AzEl/LAST path and cannot affect an FK5→TAN pixel transform. It is verified swap-specific: the two closest non-swapped controls `tan-rotated.head` and `tan-cdmatrix.head` (same off-diagonal CD structure, same TAN projection, `CTYPE1=RA`/`CTYPE2=DEC`) transform bit-exact against the oracle. The only structural difference between them and serpens is the axis swap.

**Root cause (verified by source trace):** `wcs_celestial.rs:398-405` handles the `naxis == 2 && lat_axis < lon_axis` case by wrapping the *entire* `inner` pipeline (zoom → WcsMap → rotation) in an outer PermMap sandwich `PermMap[2,1] ; inner ; PermMap[2,1]`. But `build_iwc_to_sky_mapping` (`wcs_celestial.rs:551-696`) builds the zoom MatrixMap (573-576), the WcsMap lon/lat slots, the rotation (`build_native_to_celestial_rotation_for_wcsmap`, 667-673), and `ref_pos` (685-688) all assuming (lon at slot 0, lat at slot 1). Under the swap, `inner` emits (lat, lon), so the outer sandwich is a patch over a pipeline whose internal wiring never accounted for the swap — the WcsMap and rotation see the axes misordered.

C does not use an outer sandwich. In `WcsNative` (`reference/starlink-ast/src/fitschan.c:38203-38225`) the PermMap sandwich sits *immediately around the rotation only* (`SphMap ; MatrixMap ; SphMap`): longitude is routed to slot 0 and latitude to slot 1 *inside* the rotation, then restored. The WcsMap is built on physical axes `axlon+1`/`axlat+1` (`fitschan.c:~35965`) and `WcsNative` is invoked with the physical lon/lat indices, so the projection math always sees (lon, lat) regardless of FITS axis order.

**Faithful fix:** mirror C — route the swap *inside* the rotation embedding (the `build_native_to_celestial_rotation` / celestial-embed region), and build the WcsMap and rotation on the physical lon/lat axis indices, so they always receive (lon, lat) in the correct order. Remove the outer whole-pipeline PermMap sandwich at 398-405. Do not paper over it with an extra permutation or a tolerance ([[feedback_no_corner_cuts_on_c]], [[feedback_c_logic_beats_existing_rust]]).

**Investigation gate (mandatory — confirm the exact C construction before coding):** read serpens.head through the repo `libast.so` (`reference/starlink-ast/build-ast/libast.so`; mind the conda-RPATH trap, [[reference_oracle_conda_rpath_trap]]) and dump the pixel→sky mapping chain C builds for the swapped case. Confirm where C places the PermMap (around the rotation only, per `WcsNative`) and which physical axis indices the WcsMap uses. Record the dumped chain in the task report before writing the fix, so the Rust structure matches C node-for-node rather than being reverse-engineered from the numeric error.

**Fixture cleared:** serpens (fwd+inv) from `HEAD_ORACLE_EXCLUDED` (the `C1x` rows added in Task 2). This task does NOT clear `serpens_native` — that is Task 15.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:398-405` (remove outer sandwich) and `551-696` (route swap inside the rotation, build WcsMap/rotation on physical lon/lat indices).
- Test: `crates/ast-core/tests/test_fitschan_wcs.rs` (targeted swapped-axis forward-transform assertion) + the head oracle gate.

**Interfaces:**
- Consumes: `FitsChan::from_header(&str)`, `Channel::read`, the `Mapping` transform API, the swapped-axis fields already parsed into the celestial store (`lon_axis`, `lat_axis`).
- Produces: nothing new for later tasks.

- [ ] **Step 1: C-oracle probe (gate above)** — dump C's serpens pixel→sky chain via the repo `libast.so`; record the PermMap placement and the WcsMap physical axis indices in the report.
- [ ] **Step 2: Write the failing test** — in `test_fitschan_wcs.rs`, read serpens.head, run the *forward* transform on pixel `(150.0, 120.0)`, assert the sky result is `(0.018049750, -1.440691695)` (RA, DEC in radians) to ≤1e-6. Currently returns `(-0.021491650, 1.460209855)`.
- [ ] **Step 3: Run to verify it fails** — `cargo test -p ast-core --test test_fitschan_wcs serpens_swapped_axis_transform`.
- [ ] **Step 4: Apply the faithful fix** — mirror `WcsNative` (swap inside the rotation, physical-axis WcsMap); delete the outer PermMap sandwich. No heuristics.
- [ ] **Step 5: Run to verify it passes**; confirm the non-swapped controls `tan-rotated`/`tan-cdmatrix` and all other celestial head fixtures still transform exactly (no regression).
- [ ] **Step 6: Shrink the head gate** — remove the two `serpens.head` `C1x` rows from `HEAD_ORACLE_EXCLUDED`; run `cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle -- --ignored --nocapture`. Leave `serpens_native` in `NATIVE_READ_EXCLUDED` (Task 15 owns it).
- [ ] **Step 7: Format, clippy, commit** — `"fitschan: route swapped celestial axes inside the rotation like C WcsNative (serpens)"` (+ AI trailer).

---

### Task 15: C1y — off-diagonal-CD celestial read: fuse deg2rad into the CD MatrixMap and keep CRPIX as a leading ShiftMap

serpens's `.native` output diverges from C in the pixel→IWC block shape: Rust emits `MatrixMap`(CD alone) + `WinMap`(deg2rad scale + CRPIX residue), while C emits `ShiftMap`(CRPIX) + `MatrixMap`(CD·deg2rad, fused). This is the residual `serpens_native` failure that neither the DUT1 fix (Task 2) nor the swapped-axis fix (Task 14) clears.

**Verified independent of the axis swap.** The same probe on the non-swapped off-diagonal-CD controls (`tan-rotated`, `tan-cdmatrix`) produces the *identical* Rust normal form (`MatrixMap` CD-alone + `WinMap` deg2rad+shift) even though those fixtures transform numerically exact. A shape divergence that persists when the transform is correct cannot be downstream of the transform bug — so Task 14 (which fixes the swap) will remove serpens's *extra* PermMap sandwich but will not fuse deg2rad into CD, and serpens would remain `.native`-divergent until this task lands. This is a simplifier / `map_merge` normal-form issue, not a read-path issue (same class as [[project_wcsconv_tail_190of192]]).

**Root cause (verified by source trace):** C fuses the deg2rad zoom into the CD MatrixMap and keeps CRPIX as a leading ShiftMap; Rust does neither.
- The deg2rad zoom MatrixMap (`wcs_celestial.rs:573-576`) is not merged into the CD MatrixMap (`wcs_matrix.rs:120-132`, `build_cd_matrix`) during simplification — C performs a MatrixMap×(zoom MatrixMap) fusion in this order that Rust does not.
- The CRPIX shift (`wcs_matrix.rs:81-92`, `build_crpix_shift`, scales all 1.0) should remain a clean leading `ShiftMap`. Rust instead pushes the CRPIX shift through the CD matrix and absorbs it into the trailing deg2rad WinMap's `Sft`; because `winmap.rs:264/277` only narrows a WinMap→ShiftMap when `all_scales_one` and the deg2rad WinMap has `scale=0.01745≠1`, it never narrows and the CRPIX shift is stranded inside it.

For a *diagonal* CD, C itself collapses the whole block to a WinMap (`expected/sao-tan.native`: `WinMap SFT1=-17.88 SCL1=0.017453...`), so the divergence is specifically the off-diagonal case where C promotes to `ShiftMap` + fused `MatrixMap`.

**Faithful fix (in the simplifier / `map_merge`, not the read path):** make the deg2rad zoom fuse into the CD MatrixMap (MatrixMap×MatrixMap / MatrixMap×zoom merge in C's order), and keep the CRPIX ShiftMap as a separate leading node rather than pushing it through CD. Mirror C's `map_merge` ordering exactly ([[project_matrixmap_mapmerge_port]], [[feedback_no_corner_cuts_on_c]]); do not special-case serpens.

**Investigation gate (mandatory):** dump C's serpens pixel→IWC mapping chain via the repo `libast.so` (conda-RPATH trap: [[reference_oracle_conda_rpath_trap]]) and confirm the exact node order and fused matrix values (`ShiftMap` CRPIX, then `MatrixMap` == `CD · π/180`). Identify in the C simplifier (`matrixmap.c` map_merge and the wcslib→AST assembly in `fitschan.c`) the precise merge step that produces this order, and record it in the report before coding. Because this shape is general to off-diagonal CD, verify against `tan-rotated`/`tan-cdmatrix` too.

**Note for the reviewer:** the investigation labelled `tan-rotated`/`tan-cdmatrix` as "byte-exact-passing" controls, but they exhibit the same MatrixMap+WinMap Rust shape — they are almost certainly transform-only fixtures not present in the `.native` corpus, which is why no native test currently fails for them. Confirm which fixtures are in the wcsconv native corpus and re-run the *full* `test_wcsconv` after the fix: the simplifier change touches every off-diagonal-CD celestial read, so it must not regress any currently-passing native fixture (diagonal-CD WinMap collapse, sao-tan, etc.).

**Fixture cleared:** `serpens_native` from `NATIVE_READ_EXCLUDED`.

**Files:**
- Modify: `crates/ast-core/src/mappings/matrix.rs` (MatrixMap×MatrixMap / MatrixMap×zoom `map_merge` fusion) and/or `crates/ast-core/src/mappings/winmap.rs` (CRPIX ShiftMap narrowing / not absorbing the shift into the deg2rad WinMap). The read-path builders (`wcs_matrix.rs`, `wcs_celestial.rs`) are the *source* of the nodes but the fix is in how the simplifier combines them — match C's order there.
- Test: `crates/ast-core/tests/test_wcsconv.rs` native gate + a targeted `.native`-shape assertion.

**Interfaces:**
- Consumes: MatrixMap `map_merge` ([[project_matrixmap_mapmerge_port]]), WinMap→ShiftMap narrowing (`winmap.rs:264/277`), the native serializer (`to_native_string`).
- Produces: nothing new for later tasks.

- [ ] **Step 1: C-oracle probe (gate above)** — dump C's serpens pixel→IWC chain; record `ShiftMap`(CRPIX) + `MatrixMap`(CD·deg2rad) node order and values, and the C `map_merge` step that yields it. Confirm the shape on `tan-rotated`/`tan-cdmatrix`.
- [ ] **Step 2: Write the failing test** — assert serpens's `.native` (or `to_native_string`) pixel→IWC block begins with a `ShiftMap` node (SFT == −CRPIX) followed by a single `MatrixMap` whose `M0 == CD[0][0]·(π/180)`. Fails on the current MatrixMap+WinMap shape.
- [ ] **Step 3: Run to verify it fails.**
- [ ] **Step 4: Apply the faithful fix** — fuse deg2rad into the CD MatrixMap and keep CRPIX as a leading ShiftMap, mirroring C's `map_merge` order. No serpens special-case, no tolerance heuristic.
- [ ] **Step 5: Run to verify it passes**; run the *full* `cargo test -p ast-core --test test_wcsconv` and confirm no other native fixture regressed (diagonal-CD WinMap collapse, sao-tan, tan-rotated, tan-cdmatrix, every off-diagonal-CD celestial read).
- [ ] **Step 6: Shrink `NATIVE_READ_EXCLUDED`** — remove `serpens_native`; run `cargo test -p ast-core --test test_wcsconv`.
- [ ] **Step 7: Format, clippy, commit** — `"simplify: fuse deg2rad into CD MatrixMap and keep CRPIX ShiftMap like C (serpens)"` (+ AI trailer).

---

## Phase 4 completion

When Tasks 1-15 are all committed and both gates are green with empty Phase-4-owned exclusions:

- [ ] `HEAD_ORACLE_EXCLUDED` contains no Phase-4 rows (only rows owned by other phases, if any remain).
- [ ] Every Phase-4 fixture removed from `NATIVE_READ_EXCLUDED`.
- [ ] Run the full head section gate: `cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle -- --ignored --nocapture` — passes with `checked >= HEAD_SECTION_FLOOR`.
- [ ] Run the full native read-fidelity suite: `cargo test -p ast-core --test test_wcsconv` — green.
- [ ] Update `PLAN.md` and the design spec's Phase 4 status to DONE.
- [ ] Record a project memory summarising the 14 root causes fixed (C1 DUT1, C1x swapped-axis routing, C1y off-diagonal-CD deg2rad fusion, plus A1-A3/B1-B3/C2-C6) and the two-gate pattern, linking [[project_phase4_reconciliation]].
