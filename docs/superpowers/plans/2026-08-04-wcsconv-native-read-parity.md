# wcsconv Native-Read Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Empty `NATIVE_READ_EXCLUDED` in `crates/ast-core/tests/test_wcsconv.rs`, so all 192 wcsconv fixture cases are compared against their C-generated reference with no exclusions, and make the 24 deferred C-oracle simplify fixtures visible in test output and `PLAN.md`.

**Architecture:** Thirteen independent work units, one per diagnosed root cause, each landing as its own commit that ends by re-measuring and removing the now-passing entries from `NATIVE_READ_EXCLUDED`. Units are ordered cheapest-first across three tiers: writer/harness fixes (U1-U4, U12, U13), narrow reader fixes (U5, U6, U10, U11), then the deeper read-path port work (U7, U8, U9). Two further deliverables are independent of all thirteen: a test-harness rule that licenses the one documented C aliasing difference (U4), and a deferred-fixture walker plus `PLAN.md` section (D3).

Measurement during planning split two of the spec's original groupings: `car6` and `projp-pv` are each their own single-case cause, not members of causes 1 or 7. They become U12 and U13, and the spec records all thirteen.

**Tech Stack:** Rust 2024 edition, `cargo test` / `cargo fmt` / `cargo clippy`. Reference C source at `reference/starlink-ast/src/fitschan.c` (and friends) is the authority for every behavioral question. The C oracle probe recipe is `docs/issues/c-oracle-probe-recipe.md`.

## Global Constraints

These apply to every task in this plan. A task's requirements implicitly include this section.

- **No fixture edits to make a test pass.** Files under `crates/ast-core/tests/fixtures/wcsconv/expected/` are C-generated ground truth. A fixture that looks wrong is a finding to investigate and record, not to overwrite. The only sanctioned regeneration is a documented C-behavior question settled by an oracle probe.
- **Port C, do not approximate it.** When C's control flow is available, mirror it. Do not substitute a heuristic or a tolerance constant for logic C did not need. If the C code itself uses a heuristic, port it and say so in a comment.
- **Comments describe the code, not its history.** No references to this plan, to previous attempts, or to what used to be there.
- **One sentence per line** in Markdown prose. American English spelling.
- **Before every commit:** `cargo fmt --all`, then clippy clean. Clippy needs the isolated-target-dir workaround for the conda/rustup toolchain mismatch:
  ```bash
  CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
    PATH=$HOME/.cargo/bin:$PATH \
    cargo clippy --workspace --all-targets -- -D warnings
  ```
- **Do not push to any remote.** Commit locally only.
- **Aliasing artifacts never enter Rust code.** C behaviors that arise from shared-pointer aliasing or reference counting are recorded in `docs/issues/c-library-quirks.md` and absorbed by the test harness, never reproduced in the port.
- **Instrumenting the C reference is allowed.** `reference/starlink-ast/src/` may be edited in place for probes; the submodule state is recorded and restorable.
- **The exclusion list shrinks monotonically.** No unit may add an entry to `NATIVE_READ_EXCLUDED`.

## Re-measure protocol

Every unit ends with the same measurement, and the measurement — not a target count — decides when the unit is done.

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
# 1. Included cases must stay green.
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
# 2. Real count across every native case.
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
# 3. Per-case first diffs, to decide which entries to delete.
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep -A3 "^---- wcsconv_cases"
```

**On the reported test count.** Task 0b adds one non-generated test to
`test_wcsconv`, and Task 14 adds one manifest case.
The later tasks' expected-output lines say "192 passed" because that is the
generated-case count they are guarding; the total `cargo test` reports is 193
after Task 0b and 194 after Task 14.
Read those lines as "the generated cases stay green, and nothing new fails".

A unit is done when its cause no longer appears as any case's first differing line.
A case leaves `NATIVE_READ_EXCLUDED` only when it is fully byte-exact, so a unit may fix its cause without clearing every case it was credited with — the case counts below are upper bounds.
If a fixed case reveals a second diff belonging to another cause, leave its entry in place with the reason string updated to the new measured first diff, and note it in the unit's commit message.

## Measured first diffs at plan time

This is the baseline every task's "Expected: FAIL with ..." string is drawn from.
Case names in the table are the sanitized test-function names (`build.rs` maps `-` to `_`); the manifest and exclusion-list names use hyphens and a `_native` suffix.

| Case | Line | Got | Expected | Unit |
|---|---|---|---|---|
| `alt_spectral` | 81 | `AX1_D   = '        '` | `SYSTEM_C= 'ICRS    '` | U8 |
| `awav_a2f` | 51 | `NAXIS1  = 1024` | `ASTWARN = '        '` | U7 |
| `axes_swap` | 48 | `ACTUNT_A= 0` | `DOMAIN_B= 'STOKES  '` | U8 |
| `axref` | 43 | `SREF1_A = 2.0943951023931953` | `SREF1_A = 2.0856684561332237` | U5 |
| `azel` | 30 | `EPOCH_A = 2017.5519807675657` | `EPOCH_A = 2017.5519807923772` | U6 |
| `car1` | 44 | `BEGAST_H= 'CmpMap  '` | `BEGAST_H= 'WinMap  '` | U9 |
| `car2` | 44 | `BEGAST_H= 'CmpMap  '` | `BEGAST_H= 'WinMap  '` | U9 |
| `car4` | 45 | `BEGAST_H= 'CmpMap  '` | `BEGAST_H= 'WinMap  '` | U9 |
| `car6` | 85 | `ASTWARN = '        '` | `FITS HEAders in CHIPASS_Equ.fits:` | U12 |
| `cd_cross` | 129 | `ISA_K   = 'Mapping '` | `INVERT_B= 1` | U4/U9 |
| `class_hel` | 23 | `NAXES_B = 1` | `IDENT_A = '" "     '` | U10 |
| `class_noproj` | 48 | `PROJ_A  = 'Sanson-Flamsteed'` | `PROJ_A  = 'sinusoidal'` | U8 |
| `class_spec` | 23 | `NAXES_B = 1` | `IDENT_A = '" "     '` | U10 |
| `cname` | 34 | `ENDAST_D= 'SkyAxis '` | `LABEL_C = 'Right Ascension'` | U8 |
| `fits_pc_alt` | 103 | `UWAVE_B = 'm       '` | `REFRA_B = 3.1527470058218849` | U11 |
| `fits_pc_unit` | 103 | `UWAVE_B = 'm       '` | `REFRA_B = 3.1527470058218849` | U11 |
| `freq_f2w` | 51 | `NAXIS1  = 1024` | `ASTWARN = '        '` | U7 |
| `multi_wcs` | 32 | `SYSTEM_A= 'GALACTIC'` | `EPOCH_A = 2000.0` | U8 |
| `multi_wcs_spec` | 34 | `AXP1_A  = 3` | `DTAI_A  = 37.0` | U6 |
| `offset_sky` | 31 | `SYSTEM_A= 'Unknown '` | `EPOCH_A = 2000.0` | U8 |
| `offset_sky_pole` | 31 | `SYSTEM_A= 'Unknown '` | `EPOCH_A = 2000.0` | U8 |
| `origin` | 31 | `AX1_B   = '        '` | `SYSTEM_A= 'ICRS    '` | U8 |
| `polco` | 15 | `CONTINUE  '"",1)     ""'` | `CONTINUE  '",1)     "'` | U2 |
| `polco2` | 15 | `CONTINUE  '"",1)     ""'` | `CONTINUE  '",1)     "'` | U2 |
| `projp` | 133 | `NAXIS2  = 512` | `ASTWARN = '        '` | U7 |
| `projp_nopv0` | 133 | `NAXIS2  = 512` | `ASTWARN = '        '` | U7 |
| `projp_pv0only` | 133 | `NAXIS2  = 512` | `ASTWARN = '        '` | U7 |
| `projp_pv` | — | line count 135 | line count 138 | U13 |
| `rfval` | 42 | `SREF1_A = 2.0943951023931953` | `SREF1_A = 2.0856684561332237` | U5 |
| `serpens` | 99 | `ISA_J   = 'Mapping '` | `INVERT_B= 1` | U4 |
| `sip_alt` | 51 | `AX1_C   = '        '` | `SYSTEM_B= 'ICRS    '` | U8 |
| `specflux` | — | line count 54 | line count 27 | U1 |
| `tan_zeropv` | 70 | `PV2_1_A = 1.0` | `ENDAST_H= 'WcsMap  '` | U9 |
| `tidy_offsets` | 27 | `IDENT_A = '" "     '` | `NAXES_B = 2` | U10 |
| `tidy_pole` | 27 | `IDENT_A = 'A       '` | `NAXES_B = 2` | U10 |
| `timeplot` | — | line count 66 | line count 33 | U1 |
| `tnx` | 183 | `RA = '07:21:44.00'` | `RA = '07:21:44.00       '` | U3 |
| `tnx_cheb` | 183 | `RA = '07:21:44.00'` | `RA = '07:21:44.00       '` | U3 |
| `unknown_cel` | 133 | `NAXIS2  = 100` | `ASTWARN = '        '` | U7 |
| `unknown_cel2` | 133 | `NAXIS2  = 100` | `ASTWARN = '        '` | U7 |
| `unknown_spec` | 26 | `AX1_B   = '        '` | `DOMAIN_B= 'XXXX-WAV'` | U8 |
| `vopt_f2w` | 109 | `ISSIMP_B= 1` | `ISA_I   = 'Mapping '` | U4 |
| `wave_f2w` | 108 | `ISSIMP_B= 1` | `ISA_I   = 'Mapping '` | U4 |
| `wave_w2f` | 51 | `NAXIS1  = 1024` | `ASTWARN = '        '` | U7 |
| `zpx` | 51 | `BEGAST_I= 'ShiftMap'` | `BEGAST_I= 'MatrixMap'` | U9 |

Four assignments differ from the spec's grouping, because the measurement disagrees with the spec's recorded reason strings.

**`car6` → U12, its own cause.**
Its first diff is an ASTWARN block we emit that C does not emit at all, so it is neither a doubled-card-count diff (cause 1) nor an insertion-position diff (cause 7).
`car6.head` line 1 is the bare text `FITS headers in CHIPASS_Equ.fits:`, whose 8-column name field is `FITS hea` — a name containing a space.
C `CheckFitsName` (fitschan.c:6072-6095) loops over the name, and on the first non-`isFits` character branches three ways: `== '='` reports an error, `< ' '` warns "illegal non-printing character", `> ' '` warns "illegal character".
A space is none of the three, so C falls off the `if`/`else if` chain, `break`s, and issues **no warning**.
`check_fits_name` (`crates/ast-core/src/fitschan/card.rs:57-61`) has no `> ' '` guard on its final arm, so a space reaches the "illegal character" message and we emit a four-card ASTWARN block C never wrote.
Verified during planning: adding the guard makes `car6_native` byte-exact with no other change.

**`projp_pv` → U13, its own cause.**
It is not a card-doubling case.
Our output tail is `NAXIS1`, `NAXIS2`, `END`; the reference tail is `NAXIS1`, `NAXIS2`, `PROJP0`, `PROJP1`, `PROJP2` — we delete three PROJP cards C retains.
`translate_projp` (`crates/ast-core/src/fitschan/spec_trans.rs:1717-1776`) diverges from C (fitschan.c:31579-31632) twice: its `!use_projp` branch calls `delete_consumed` on each PROJP card (C never deletes them), and its translate branch omits C's `encoding == FITSPC_ENCODING || encoding == FITSIRAF_ENCODING` gate on the `PV<axlat>_<i>` write.

U1 therefore targets exactly two cases, `specflux` and `timeplot` — the only two showing the exact-doubling signature (55 vs 27, 67 vs 33).
U7's eight cases are unchanged from the spec.
`car6` was never among them, so removing `car6` from cause 1 does not change cause 7's count.

**`cd_cross` → U4 or U9, decided by measurement.**
It shows the same `ISA_x = 'Mapping '` vs `INVERT_B= 1` shape as `serpens`, which the spec assigns to the U4 aliasing quirk.
U4 evaluates whether `cd_cross` is the same licensed difference; if it is not, it stays with U9.

**The spec's cause-3 paragraph names the wrong mechanism.**
It says C pads short string values to 8 columns and we do not.
`format_string_value` (`crates/ast-core/src/fitschan/card.rs:703-724`) already pads to 8, matching C `EncodeValue`.
The actual defect is on the *parse* side: `card.rs:562-563` calls `trim_end()` on every string value, where C (fitschan.c:15439-15448, plus the `MAKE_FGET` macro at ~15408) strips trailing spaces only when the value is 8 characters or fewer.
`tnx.head`'s `RA` value is 18 characters, so C preserves its padding and we destroy it.
U3 fixes the parse side.

## Where the spec's deliverables live

The spec names three deliverables.
Each maps to a task in this plan:

| Spec deliverable | Task |
|---|---|
| **D1** — Empty `NATIVE_READ_EXCLUDED` | Every code task shrinks it; **Task 15** empties it, replaces the constant with a documented empty slice, and runs the equality proof. |
| **D2** — Harness rule for the `IsSimp`/`Invert` aliasing quirk | **Task 6 (U4)**. |
| **D3** — Deferred-fixture visibility | **Task 14**. |

Task 0 and Task 0b belong to no spec deliverable: Task 0 corrects the spec's
own diagnoses, and Task 0b builds the stale-entry gate the spec assumes
already exists.

## File Structure

Files created by this plan:

| File | Responsibility |
|---|---|
| `crates/ast-core/tests/test_deferred_fixtures.rs` | D3: walk `simplify_fixtures_unsupported/`, require every subdirectory to be registered in a `DEFERRED_CLASSES` table, print the file count on every run. |

Files modified, grouped by the unit that owns the change:

| File | Units | What changes |
|---|---|---|
| `crates/ast-core/src/fitschan/mod.rs` | U1 | Native read arm calls the consumption finisher, as the FITS arms already do. |
| `crates/ast-core/src/fitschan/enc_native.rs` | U2 | Gains `un_pre_quote` and applies it on the read path (sub-fix A); `pre_quote` becomes C's integrated truncation-and-quote loop (sub-fix B). |
| `crates/ast-core/src/fitschan/card.rs` | U3, U12 | String-value trailing-space stripping becomes conditional on length ≤ 8; `check_fits_name`'s illegal-character arm gains C's `> ' '` guard. |
| `crates/ast-core/src/fitschan/card.rs` (visibility only) | U2 | `FITS_CARD_LEN` and `FITS_NAME_LEN` become `pub(super)` so `enc_native` can use them instead of redefining them. |
| `crates/ast-core/tests/common/mod.rs` | U4 | Gains `is_aliasing_tag_card`, and absorbs `strip_is_simp` from `test_simplify_regression.rs`. |
| `crates/ast-core/tests/test_simplify_regression.rs` | U4 | Uses the shared helper instead of its private copy. |
| `crates/ast-core/tests/test_wcsconv.rs` | Task 0b, U1-U13 | Gains the stale-entry gate and the `force_comparison` parameter (Task 0b) and the licensed-difference rule (U4); `NATIVE_READ_EXCLUDED` shrinks in every unit. |
| `crates/ast-core/src/fitschan/fits_store.rs` | U5 | Gains `axref` and `rfval` axis-indexed fields. |
| `crates/ast-core/src/fitschan/wcs_keywords.rs` | U5, U6 | Reads AXREF/RFVAL; gates DTAI on `TELESCOP`. |
| `crates/ast-core/src/fitschan/wcs_celestial.rs` | U5, U6, U8, U9 | RFVAL/AXREF fiducial override; celestial-axis recognition; the CAR/`carlin` object shape. |
| `crates/ast-core/src/fitschan/wcs_read.rs` | U6, U10, U11 | Top-level Frame time attributes; `TidyOffsets` clear-Ident pass; alt-plane spectral reference position. |
| `crates/ast-core/src/fitschan/card_list.rs` | U7 | `push_astwarn` inserts before the current card and restores the position. |
| `crates/ast-core/src/fitschan/spec_trans.rs` | U9, U13 | MSX-CAR translation gated on the new `CarLin` attribute; `translate_projp` stops deleting PROJP cards and regains C's encoding gate. |
| `crates/ast-core/src/frames/*.rs` | U10 | `Ident` emission and clearing on the frame classes that read `Ident` but never write it. |
| `crates/ast-core/tests/fixtures/wcsconv/expected/azel.native` | U6 | The plan's only fixture regeneration: stale relative to the reference C's `DateObs` fractional-seconds fix. Justified in Task 8 sub-task B. |
| `docs/issues/c-library-quirks.md` | U4 | Notes that the harness now absorbs the aliasing quirk. |
| `docs/issues/fitschan-c-port-gaps.md` | U7, U8, U9 | Existing gap catalogue, updated as gaps close. |
| `PLAN.md` | D3, final | Deferred-fixture section; FitsChan row updated. |
| `docs/superpowers/specs/2026-08-04-wcsconv-native-read-parity-design.md` | Task 0 | Cause-3 mechanism and the two re-assigned cases corrected. |

---

### Task 0: Correct the spec's diagnoses

The spec is the authority every later task reads.
Five of its statements are wrong, and a worker who trusts them will chase the wrong defect.
This task carries no code and no test.

**Files:**
- Modify: `docs/superpowers/specs/2026-08-04-wcsconv-native-read-parity-design.md:126-131` (cause 3), `:78-80` (cause 1's case list), `:88-97` (cause 1's `ncard` figures), `:163-170` (cause 6's azel paragraph), `:176-179` (cause 7's case list)

- [ ] **Step 1: Rewrite the cause-3 paragraph**

Replace the two-line mechanism claim under `### Cause 3 — string-value padding (2 cases)` with:

```markdown
We emit `RA = '07:21:44.00'`, C emits `RA = '07:21:44.00       '`.

The defect is on the parse side, not the write side.
`format_string_value` already pads short values to 8 columns, matching C
`EncodeValue`.
C strips trailing spaces from a string keyword value only when the value is 8
characters or fewer (fitschan.c:15439-15448 and the `MAKE_FGET` macro at
~15408); we strip unconditionally.
`tnx.head`'s `RA` value is 18 characters, so C preserves its padding.
```

- [ ] **Step 2: Move `car6` out of cause 1 and add cause 12**

Change cause 1's heading to `### Cause 1 — read-consumed cards not deleted (2 cases)` and its case list to `` `specflux`, `timeplot`. ``
Delete `projp-pv` and `car6` from that list.
Add, after cause 11:

```markdown
### Cause 12 — space in a keyword name warns when C does not (1 case)

`car6`.

`car6.head` line 1 is the bare text `FITS headers in CHIPASS_Equ.fits:`, whose
8-column name field is `FITS hea`.
C `CheckFitsName` (fitschan.c:6072-6095) branches on the first non-`isFits`
character three ways — `== '='` errors, `< ' '` warns "illegal non-printing
character", `> ' '` warns "illegal character" — then `break`s.
A space matches none of the three, so C issues no warning.
Our port's final arm has no `> ' '` guard, so we emit a four-card ASTWARN block
C never wrote.

### Cause 13 — PROJP cards deleted when C retains them (1 case)

`projp-pv`.

Our output tail is `NAXIS1`, `NAXIS2`, `END`; C's is `NAXIS1`, `NAXIS2`,
`PROJP0`, `PROJP1`, `PROJP2`.
`translate_projp` deletes the PROJP cards in its `!use_projp` branch, which C
does not do, and omits C's `encoding == FITSPC_ENCODING || encoding ==
FITSIRAF_ENCODING` gate on the `PV<axlat>_<i>` write (fitschan.c:31579-31632).
```

- [ ] **Step 3: Correct cause 1's measured `ncard` figures**

The spec's fenced block under cause 1 reads:

```
specflux: ncard after read=27  reuse=56  fresh=29   (input .head = 28 cards)
timeplot: ncard after read=33  reuse=68  fresh=35   (input .head = 34 cards)
```

Those `reuse` numbers were not what a re-measurement produced.
Replace the block with the measured values and the corrected reading:

```markdown
```
specflux: ncard after read=27  reuse=27   (input .head = 28 cards)
timeplot: ncard after read=33  reuse=33   (input .head = 34 cards)
```

The chan is not doubling in card *count*.
`read()` leaves the consumed cards in place, and the subsequent `write()` to the
same chan emits them a second time — so the doubling is in the written output,
not in `ncard`.
The fix is the same either way: the native read path must mark the cards it
consumes so the consumption finisher deletes them, as the FITS arms already do.
```

- [ ] **Step 4: Replace cause 6's azel paragraph**

The spec asserts a port-side epoch bug: "Ours is DATE-OBS + 69.18 s, C's is DATE-OBS + 69.97 s."
Both offsets are real, but the conclusion is backwards — a C-oracle probe of the pinned submodule returns our value, not the fixture's.
Replace the azel sentences under `### Cause 6 — time attributes (2 cases)` with:

```markdown
`azel`'s 0.783 s epoch difference is a **stale fixture**, not a port defect.
Both sides apply the same +69.1835 s TT−UTC offset for 2017; the difference is
entirely in the parsed seconds field of `DATE-OBS= '2017-07-21T08:38:39.087'` —
the fixture's epoch corresponds to `.870`, ours to `.087`.
That is C's old `DateObs` bug, in which the fractional-seconds field was read
with `"%d"` into an `int` and lost its leading zero.
The reference submodule fixed it on 20-JUN-2026 (commit `254d4c99`, changelog
at fitschan.c:1354-1359) by capturing the field width with `"%n"`; the fixture
predates the fix.
Probing `astRead` on `azel.head` against the pinned submodule returns
`Epoch = 2017.5519807675655`, matching our port to 2 ULP.
`azel.native` is the only stale expected file in the corpus — the other 44
native cases regenerate byte-identical to what is committed.

`multi-wcs-spec` is a genuine port gap: a missing `DTAI_A = 37.0` card on the
top-level world Frame, plus an ungated `DTAI` read where C gates on `TELESCOP`.
```

- [ ] **Step 5: Correct the per-unit counts**

Cause 7 keeps its eight-name list and its count of 8: `car6` was never among
them, so splitting `car6` out of cause 1 leaves cause 7 untouched.
In the "Work units" table, change U1's count from 4 to 2 and add U12 and U13
rows with 1 case each.
Change the totals sentence from eleven root causes to thirteen; the total
stays 45.

- [ ] **Step 6: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git add docs/superpowers/specs/2026-08-04-wcsconv-native-read-parity-design.md \
        docs/superpowers/plans/2026-08-04-wcsconv-native-read-parity.md
git -c core.hooksPath=/dev/null commit -m "docs: correct wcsconv parity spec diagnoses for causes 1, 3 and 7"
```

---

### Task 0b: Give the wcsconv harness a stale-entry gate

The spec says a stale entry — one that is excluded but now passes — is caught
by "the existing two-sided gate", citing the `grism_fits-wcs` discovery.
That gate does not exist in `test_wcsconv.rs`.
The only mechanism there is `WCSCONV_NO_EXCLUDE=1`, which a human has to
remember to run and then read.
`test_transform_oracle.rs` does have the check, twice
(`:1278` for `SLAMAP_ORACLE_EXCLUDED`, `:1392` for `HEAD_ORACLE_EXCLUDED`), so
this task ports that shape to the wcsconv harness.

Every later task's "Confirm the two-sided gate" step depends on this, so it
lands before any of them.
Without it, each of those steps is a promise the harness cannot keep, and a
worker who deletes the wrong entry — or forgets to delete a right one — finds
out only at Task 15.

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (new `#[test] fn native_read_excluded_entries_still_fail`, placed immediately after `run_named_case` at `:425-444` and before the `include!` at `:446`; plus the `run_test_case` split in step 3)

**Interfaces:**
- Consumes: `run_test_case(case: &TestCase) -> Result<(), String>` (`:268`), `parse_manifest() -> Vec<TestCase>` (`:146`), `fixtures_dir() -> PathBuf` (`:59`), `NATIVE_READ_EXCLUDED: &[(&str, &str)]` (`:74`).
- Produces: a test named `native_read_excluded_entries_still_fail`. Every later task's re-measure step can rely on `cargo test -p ast-core --test test_wcsconv` failing when an entry has gone stale, so "delete the now-passing entries" becomes a checked instruction rather than an unchecked one.

- [ ] **Step 1: Write the failing test**

The test asserts the *contrapositive* of what the plan is trying to achieve:
every name in `NATIVE_READ_EXCLUDED` must still differ from its reference.
It fails when an entry starts passing, which is exactly the signal each task
needs.

Add to `crates/ast-core/tests/test_wcsconv.rs`, after `run_named_case`:

```rust
/// Every `NATIVE_READ_EXCLUDED` entry must still fail. An entry that has
/// started passing is a stale exclusion: the comparison it was suppressing
/// now succeeds, so the entry hides nothing and must be deleted.
///
/// This is the other half of the exclusion list's bookkeeping. The generated
/// per-case tests catch a case that regresses; this catches one that has been
/// fixed without the list being updated. Together they mean the list can only
/// be wrong in a way some test reports.
#[test]
fn native_read_excluded_entries_still_fail() {
    let cases = parse_manifest();
    let mut stale = Vec::new();
    let mut unknown = Vec::new();

    for (name, reason) in NATIVE_READ_EXCLUDED {
        let Some(case) = cases.iter().find(|c| c.name == *name) else {
            unknown.push(*name);
            continue;
        };
        if !fixtures_dir().join("inputs").join(&case.input_file).exists() {
            unknown.push(*name);
            continue;
        }
        // Compare unconditionally: `run_test_case` consults the exclusion
        // list itself, so ask it the question with the list bypassed.
        if run_with_comparison_forced(case).is_ok() {
            stale.push(format!("{name}  (recorded reason: {reason})"));
        }
    }

    assert!(
        unknown.is_empty(),
        "NATIVE_READ_EXCLUDED names cases the manifest does not have, or whose \
         input fixture is missing:\n{unknown:?}",
    );
    assert!(
        stale.is_empty(),
        "NATIVE_READ_EXCLUDED entries now pass — delete them:\n{}",
        stale.join("\n"),
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_wcsconv native_read_excluded_entries_still_fail
```
Expected: FAIL to *compile* — `cannot find function `run_with_comparison_forced``.
That is the failing state for this step; the helper is step 3.

- [ ] **Step 3: Make the comparison switch a parameter instead of an environment read**

`run_test_case` reads `WCSCONV_NO_EXCLUDE` inline at `:355-357`.
The new test needs the same bypass without setting a process-wide environment
variable from inside a test binary that runs cases in parallel.
Split the environment read out of the decision.

Replace the `native_excluded` computation at `crates/ast-core/tests/test_wcsconv.rs:354-356`:

```rust
    let native_excluded = !std::env::var("WCSCONV_NO_EXCLUDE").is_ok_and(|v| !v.is_empty())
        && is_native
        && NATIVE_READ_EXCLUDED.iter().any(|(n, _)| *n == case.name);
```

with a read of a new parameter:

```rust
    let native_excluded = !force_comparison
        && !std::env::var("WCSCONV_NO_EXCLUDE").is_ok_and(|v| !v.is_empty())
        && is_native
        && NATIVE_READ_EXCLUDED.iter().any(|(n, _)| *n == case.name);
```

Change `run_test_case`'s signature (`:267-268`) to take it, and add the two
wrappers so no existing caller changes:

```rust
/// Run a single test case. Returns Ok on pass, Err with description on failure.
fn run_test_case(case: &TestCase) -> Result<(), String> {
    run_case(case, false)
}

/// Run a case with the [`NATIVE_READ_EXCLUDED`] bypass forced on, whatever
/// the environment says. Used by the stale-entry gate, which must ask "does
/// this excluded case actually still differ?".
fn run_with_comparison_forced(case: &TestCase) -> Result<(), String> {
    run_case(case, true)
}

fn run_case(case: &TestCase, force_comparison: bool) -> Result<(), String> {
```

The body that followed the old signature becomes `run_case`'s body unchanged.

- [ ] **Step 4: Run the test to verify it now fails for the right reason**

Run:
```bash
cargo test -p ast-core --test test_wcsconv native_read_excluded_entries_still_fail 2>&1 | tail -20
```
Expected: PASS at this point in the plan — all 45 entries genuinely still
fail, which is what the gate asserts.

A PASS is the correct outcome here and does not mean the test is vacuous:
step 5 proves it can fail.

- [ ] **Step 5: Prove the gate actually catches a stale entry**

Temporarily add an entry for a case that is known to pass, run the gate, and
confirm it fails:

```bash
# aitoff_native is not excluded and passes today.
python3 - <<'EOF'
import re, pathlib
p = pathlib.Path("crates/ast-core/tests/test_wcsconv.rs")
t = p.read_text()
t = t.replace('    ("alt-spectral_native"', '    ("aitoff_native", "TEMPORARY"),\n    ("alt-spectral_native"', 1)
p.write_text(t)
EOF
cargo test -p ast-core --test test_wcsconv native_read_excluded_entries_still_fail 2>&1 | tail -10
```
Expected: FAIL with
`NATIVE_READ_EXCLUDED entries now pass — delete them:` and
`aitoff_native  (recorded reason: TEMPORARY)`.

Then remove the temporary line and re-run to confirm it is green again:

```bash
python3 - <<'EOF'
import pathlib
p = pathlib.Path("crates/ast-core/tests/test_wcsconv.rs")
t = p.read_text().replace('    ("aitoff_native", "TEMPORARY"),\n', '', 1)
p.write_text(t)
EOF
cargo test -p ast-core --test test_wcsconv native_read_excluded_entries_still_fail 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 6: Confirm the corpus count went up by one**

Run:
```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```
Expected: **193 passed, 0 failed** — the 192 generated cases plus this gate.

Every later task in this plan says "192 passed" in its expected output.
That figure is the generated-case count and is now one short of the reported
total.
Read those steps as "the 192 generated cases stay green"; the reported number
is 193 from here on, and 194 after Task 14 adds `offset-sref`.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "tests: fail when a NATIVE_READ_EXCLUDED entry has started passing"
```

---

## Tier 1 — writer and harness

### Task 1 (U12): No warning for a space in a keyword name

One case, one guard, and the cheapest fix in the plan.

**Files:**
- Modify: `crates/ast-core/src/fitschan/card.rs:45-64` (`check_fits_name`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove the `car6_native` entry)
- Test: `crates/ast-core/src/fitschan/card.rs` (the existing `mod tests`, near `check_fits_name_legal` at ~1174)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: nothing later tasks depend on. `check_fits_name(raw_name: &str) -> Option<String>` keeps its signature.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crates/ast-core/src/fitschan/card.rs`, immediately after `check_fits_name_legal`:

```rust
#[test]
fn check_fits_name_space_is_not_reported() {
    // C CheckFitsName (fitschan.c:6072-6095) branches on `== '='`,
    // `< ' '` and `> ' '`. A space matches none of them, so no warning
    // is issued for a name field that contains one.
    assert_eq!(check_fits_name("FITS hea"), None);
    assert_eq!(check_fits_name("A B     "), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --lib check_fits_name_space_is_not_reported
```
Expected: FAIL — `assertion `left == right` failed`, left is `Some("The FITS keyword name ('FITS hea') contains an illegal character (' ').")`, right is `None`.

- [ ] **Step 3: Add C's `> ' '` guard**

In `check_fits_name`, replace the final `return Some(format!(...))` arm with a guarded form so a character that is neither `=`, nor below space, nor above space falls through to `None`:

```rust
            if c > b' ' {
                return Some(format!(
                    "The FITS keyword name ('{trimmed}') contains an illegal \
                     character ('{}').",
                    c as char
                ));
            }
            // C's chain has no arm for a space (fitschan.c:6072-6095): it
            // breaks out of the loop without warning.
            return None;
```

- [ ] **Step 4: Run the test to verify it passes**

Run:
```bash
cargo test -p ast-core --lib check_fits_name
```
Expected: PASS, all `check_fits_name*` tests green.

- [ ] **Step 5: Re-measure and drop the entry**

Run:
```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv car6_native 2>&1 | tail -5
```
Expected: the first stays at 192 passed, 0 failed; the second reports `car6_native ... ok`.

Then delete this line from `NATIVE_READ_EXCLUDED` in `crates/ast-core/tests/test_wcsconv.rs`:

```rust
    ("car6_native", "FITS: card-level"),
```

- [ ] **Step 6: Confirm the two-sided gate**

Run:
```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
```
Expected: 192 passed, 0 failed; `test_fitschan` unchanged.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/card.rs crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: a space in a keyword name is not a badkeyname warning (car6)"
```

---

### Task 2 (U3): Strip trailing spaces only from values of 8 characters or fewer

**Files:**
- Modify: `crates/ast-core/src/fitschan/card.rs:562-563` (in `parse_string_value`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove the `tnx_native` and `tnx-cheb_native` entries)
- Test: `crates/ast-core/src/fitschan/card.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing. `parse_string_value` is private and keeps its signature.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crates/ast-core/src/fitschan/card.rs`:

```rust
#[test]
fn parse_string_value_keeps_padding_on_long_values() {
    // C strips trailing spaces only when the value is 8 characters or
    // fewer (fitschan.c:15439-15448). An 18-character padded value keeps
    // its padding.
    let card = FitsCard::parse(
        "RA      = '07:21:44.00       ' / RA of observation (hr)                        ",
    )
    .unwrap();
    assert_eq!(
        card.value,
        FitsValue::String("07:21:44.00       ".to_string())
    );
}

#[test]
fn parse_string_value_strips_padding_on_short_values() {
    // An 8-character value is stripped, as C does.
    let card = FitsCard::parse(
        "CTYPE1  = 'RA--    '           / Axis type                                     ",
    )
    .unwrap();
    assert_eq!(card.value, FitsValue::String("RA--".to_string()));
}
```

Check `FitsCard::parse`'s exact name and visibility in the file before writing these; if the constructor is named differently, use the name the surrounding tests use.

- [ ] **Step 2: Run tests to verify the first fails**

Run:
```bash
cargo test -p ast-core --lib parse_string_value_keeps_padding_on_long_values
```
Expected: FAIL — got `String("07:21:44.00")`, expected `String("07:21:44.00       ")`.

Also run `parse_string_value_strips_padding_on_short_values` and confirm it already passes; that is the half we must not regress.

- [ ] **Step 3: Make the trim conditional**

Replace `card.rs:562-563`:

```rust
    // Trim trailing spaces from the string value (FITS standard).
    let result = result.trim_end().to_string();
```

with:

```rust
    // The FITS standard pads string values shorter than 8 characters with
    // trailing spaces, so those spaces carry no information and are
    // stripped. Longer values may have significant trailing spaces, which
    // are preserved (C fitschan.c:15439-15448 and the MAKE_FGET macro).
    let result = if result.len() <= 8 {
        result.trim_end().to_string()
    } else {
        result
    };
```

- [ ] **Step 4: Run tests to verify both pass**

Run:
```bash
cargo test -p ast-core --lib parse_string_value
```
Expected: PASS, both tests green.

- [ ] **Step 5: Re-measure and drop the entries**

Run:
```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv tnx 2>&1 | tail -6
```
Expected: `tnx_native ... ok` and `tnx_cheb_native ... ok`.

Then delete these lines from `NATIVE_READ_EXCLUDED`:

```rust
    ("tnx-cheb_native", "RA: string-value padding"),
    ("tnx_native", "RA: string-value padding"),
```

- [ ] **Step 6: Run the full write-path regression breadth**

This change touches every string-value parse in the library, so the whole FITS corpus must be re-run:

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -3
cargo test -p ast-core 2>&1 | tail -5
```
Expected: 192 passed for wcsconv; every other suite at its existing count with 0 failures.
If a `test_fitschan` case now fails on a padded value, read the C behavior for that keyword before touching the new code — the ≤8 rule is C's, so a failure here is a second, different bug.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/card.rs crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: keep trailing spaces on string values longer than 8 characters"
```

---

### Task 3 (U13): PROJP cards are retained, and PV is written only for FITS-PC and FITS-IRAF

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs:1717-1776` (`translate_projp`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove the `projp-pv_native` entry)
- Test: `crates/ast-core/tests/test_fitschan_spec_trans.rs` if it exists; otherwise add a `#[cfg(test)] mod tests` case in `spec_trans.rs` alongside the existing ones

**Interfaces:**
- Consumes: nothing.
- Produces: nothing. `translate_projp`'s signature is unchanged; check it before writing the test and call it as its existing callers do.

- [ ] **Step 1: Read C's PROJP block before writing anything**

Read `reference/starlink-ast/src/fitschan.c:31579-31632`.
Confirm for yourself, in the source, that:
- the `SetValue(ret, "PV<axlat>_<i>", ...)` write is inside an `encoding == FITSPC_ENCODING || encoding == FITSIRAF_ENCODING` test;
- there is no `DeleteCard` on any PROJP keyword anywhere in the block;
- the PROJP value is fetched with `GetValue2(ret, this, ...)`, whose contract (fitschan.c:18456-18458) marks the card used only when the hit came from the *second* chan.

Write down the line numbers you verified; the commit message cites them.

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn translate_projp_retains_source_cards() {
    // C fitschan.c:31579-31632 reads PROJP with GetValue2 and never
    // deletes it, so the cards survive into the output header.
    let header = "\
SIMPLE  =                    T / Fits standard
NAXIS   =                    2 / Number of dimensions
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CRVAL1  =                 40.0 / Reference value
CRVAL2  =                 60.0 / Reference value
CRPIX1  =                256.0 / Reference pixel
CRPIX2  =                256.0 / Reference pixel
CDELT1  =                -0.01 / Pixel size
CDELT2  =                 0.01 / Pixel size
PROJP0  =                  0.0 / Projection parameter
PROJP1  =                  0.0 / Projection parameter
PROJP2  =                  0.0 / Projection parameter
END
";
    let mut fc = FitsChan::from_header(header).unwrap();
    let _obj = fc.read().unwrap().unwrap();
    fc.set_encoding(Encoding::Native);
    fc.write(&_obj).unwrap();
    let out = fc.to_header();
    for key in ["PROJP0", "PROJP1", "PROJP2"] {
        assert!(out.contains(key), "{key} should survive the read");
    }
}
```

Put this in whichever test file already drives `FitsChan::from_header` + `read` + `write`; match that file's imports.

- [ ] **Step 3: Run test to verify it fails**

Run:
```bash
cargo test -p ast-core translate_projp_retains_source_cards
```
Expected: FAIL — `PROJP0 should survive the read`.

- [ ] **Step 4: Remove the deletion and restore C's encoding gate**

In `translate_projp`, delete the `!use_projp` branch's deletion loop so the branch becomes a bare early return, and gate the `set_float` on the encoding:

```rust
    if !use_projp {
        // C reads PROJP with GetValue2 and never deletes it
        // (fitschan.c:31579-31632), so the cards stay in the header.
        return;
    }
    for i in projp_indices {
        let from = format!("PROJP{i}");
        let Some(value) = cards.get_float(&from) else {
            continue;
        };
        let comment = cards
            .find(&from)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        // C writes the PV equivalent only for the two encodings that use
        // it (fitschan.c:31579-31632).
        if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
            cards.set_float(&format!("PV{axlat}_{i}"), value, &comment);
        }
    }
```

`translate_projp` may not currently receive `encoding`; if it does not, thread it from the caller in `spec_trans` — that function already has it.
Do not delete `from`; the loop no longer calls `delete_consumed` at all.

- [ ] **Step 5: Run tests to verify the new one passes**

Run:
```bash
cargo test -p ast-core translate_projp
```
Expected: PASS.

- [ ] **Step 6: Re-measure across the whole projp family**

The three sibling `projp*` cases are U7's, and they must not regress:

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv projp 2>&1 | tail -12
```
Expected: `projp_pv_native ... ok`.
The other three (`projp_native`, `projp_nopv0_native`, `projp_pv0only_native`) still fail on their ASTWARN-position diff at line 133 — that is U7's, unchanged.
Confirm their first diff is still the ASTWARN one and has not moved to a PROJP card; if it has, record the new measured first diff in their reason strings.

Then delete this line from `NATIVE_READ_EXCLUDED`:

```rust
    ("projp-pv_native", "line count: 135 vs 138"),
```

- [ ] **Step 7: Run the FITS-PC and FITS-IRAF corpora**

The encoding gate changes what those two encodings see:

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
```
Expected: 192 passed, 0 failed; `test_fitschan` unchanged.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/spec_trans.rs crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: retain PROJP cards and gate the PV translation on encoding like C"
```

---

### Task 4 (U1): A native read consumes the cards it read

Two cases: `specflux` (line count 54 vs 27) and `timeplot` (66 vs 33).

Both inputs are keyword-card NATIVE dumps.
The harness reuses the read chan for the write, mirroring
`ast_tester/wcsconverter.c`, so a read that leaves all its cards in place
produces output that is exactly the input plus a fresh dump — twice the
length.

Measured with a probe over `FitsChan::ncard()`:

```
specflux: cards before read=27 after read=27
timeplot: cards before read=33 after read=33
```

C consumes them.
`FindAst` (fitschan.c:17955) calls `MarkCard` on every card it walks when
`skip` is 0, marks the `BEGAST` card explicitly (:17903), and calls
`ComBlock( this, -1, ... )` after `BEGAST` (:17908) and
`ComBlock( this, 1, ... )` after `ENDAST` (:17962) to consume the AST banner
COMMENT blocks.
`Read` then calls `FixUsed( this, 0, 1, remove, ... )` with `remove = 1` for
NATIVE encoding (fitschan.c:25924-25930), which physically deletes them.

`skip` is 1 only for the top-level search for a `BEGAST`
(`GetSkip` returns 1 for a FitsChan, fitschan.c:18272; `channel.c:2581` passes
it), so cards *before* the first `BEGAST` are walked without being marked.
Everything from `BEGAST` through the matching `ENDAST` is read with `skip = 0`
(`channel.c:2815` passes 0) and therefore marked.

**Files:**
- Modify: `crates/ast-core/src/fitschan/card_list.rs` (add `mark_at`, `mark_com_block`)
- Modify: `crates/ast-core/src/fitschan/enc_native.rs` (add `native_span`)
- Modify: `crates/ast-core/src/fitschan/mod.rs:657-675` (the `Encoding::Native` read arm)
- Modify: `crates/ast-core/tests/test_wcsconv.rs`
- Test: `crates/ast-core/tests/test_fitschan_native_keyword.rs`

**Interfaces:**
- Consumes: `CardList::fix_used(promote: bool, remove: bool)` (card_list.rs:288) — the
  `remove` branch already deletes provisional cards, so this task only has to
  reach it. `CardFlags::protected` (card.rs) and `Consumption` (card_list.rs).
- Produces:
  - `CardList::mark_at(&mut self, index: usize)`
  - `CardList::mark_com_block(&mut self, index: usize, incr: i32) -> bool`
  - `enc_native::native_span(cards: &CardList) -> Option<(usize, usize)>`

- [ ] **Step 1: Write the failing test**

Add to `crates/ast-core/tests/test_fitschan_native_keyword.rs`:

```rust
/// A successful NATIVE read consumes the cards it read, as C `Read` does by
/// calling `FixUsed` with `remove` set for NATIVE encoding
/// (fitschan.c:25924-25930). The AST banner COMMENT blocks go with them
/// (C `ComBlock`, fitschan.c:7972).
#[test]
fn native_read_consumes_its_cards() {
    for name in ["specflux.head", "timeplot.head", "polco.head"] {
        let text = std::fs::read_to_string(fixture_path(name))
            .unwrap_or_else(|e| panic!("read {name}: {e}"));
        let mut fc = FitsChan::from_header(&text)
            .unwrap_or_else(|e| panic!("from_header {name}: {e}"));
        assert!(fc.ncard() > 0, "{name}: no cards to start with");
        let _obj = fc
            .read()
            .unwrap_or_else(|e| panic!("read {name}: {e}"))
            .unwrap_or_else(|| panic!("read {name}: returned None"));
        assert_eq!(fc.ncard(), 0, "{name}: cards left after a NATIVE read");
    }
}
```

These three fixtures are the whole keyword-card corpus and each is a
single top-level object with both banner blocks, so `ncard() == 0` is the
exact expectation.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --test test_fitschan_native_keyword native_read_consumes_its_cards`
Expected: FAIL with `specflux.head: cards left after a NATIVE read`,
left `27`, right `0`.

- [ ] **Step 3: Add the two `CardList` primitives**

In `crates/ast-core/src/fitschan/card_list.rs`, next to `mark_card`:

```rust
    /// Mark the card at `index` as provisionally consumed, mirroring C
    /// `MarkCard` (fitschan.c:23123): a card carrying the `PROTECTED` flag is
    /// exempt, and a card already `Used` is left alone.
    pub(crate) fn mark_at(&mut self, index: usize) {
        if let Some(card) = self.cards.get_mut(index) {
            if !card.flags.protected && card.flags.consumption != Consumption::Used {
                card.flags.consumption = Consumption::Provisional;
            }
        }
    }

    /// Validate and mark the AST banner comment block adjacent to the card at
    /// `index`, mirroring C `ComBlock` (fitschan.c:7972). `incr` is `-1` to
    /// scan backwards from the card before `index` (delimiter character `+`)
    /// and `1` to scan forwards from the card after it (delimiter `-`).
    ///
    /// A block is complete when every card in it is a COMMENT whose text
    /// starts and ends with `AST`, terminated by a card whose interior is
    /// nothing but the delimiter character and spaces. Nothing is marked
    /// unless the whole block validates.
    pub(crate) fn mark_com_block(&mut self, index: usize, incr: i32) -> bool {
        let del = if incr == 1 { '+' } else { '-' };
        let del = if incr == 1 { '-' } else { del };
        let mut found = Vec::new();
        let mut i = index as isize;
        loop {
            i += incr as isize;
            if i < 0 || i as usize >= self.cards.len() {
                return false;
            }
            let FitsValue::Comment(text) = &self.cards[i as usize].value else {
                return false;
            };
            let t = text.trim_end();
            if t.len() < 6 || !t.starts_with("AST") || !t.ends_with("AST") {
                return false;
            }
            found.push(i as usize);
            if t[3..t.len() - 3].chars().all(|c| c == del || c == ' ') {
                break;
            }
        }
        for idx in found {
            self.mark_at(idx);
        }
        true
    }
```

Collapse the two `del` bindings into one when transcribing — the delimiter is
`'-'` for `incr == 1` and `'+'` otherwise, matching
`del = ( incr == 1 ) ? '-' : '+';` (fitschan.c:8057).

- [ ] **Step 4: Add `native_span` to the decoder**

In `crates/ast-core/src/fitschan/enc_native.rs`, next to `has_keyword_native`:

```rust
/// Index range of the first complete top-level object in a keyword-card
/// NATIVE header: from its `BEGAST<xx>` card to the `ENDAST<xx>` card that
/// closes it, inclusive. `None` when there is no `BEGAST` or the object is
/// unterminated.
///
/// This is the span C `FindAst` walks with `skip` 0, and therefore the span
/// it marks as used (fitschan.c:17955). Cards before the `BEGAST` are walked
/// with `skip` 1 and left unmarked.
pub(crate) fn native_span(cards: &CardList) -> Option<(usize, usize)> {
    let mut first = None;
    let mut depth = 0usize;
    for (i, card) in cards.iter().enumerate() {
        match classify(&card.keyword) {
            KeywordKind::Begin => {
                if first.is_none() {
                    first = Some(i);
                }
                depth += 1;
            }
            KeywordKind::End => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return first.map(|f| (f, i));
                }
            }
            _ => {}
        }
    }
    None
}
```

- [ ] **Step 5: Consume the span in the read arm**

In `crates/ast-core/src/fitschan/mod.rs`, in the `Encoding::Native` arm, after
`let obj = serial::any_object_from_node(&node)?;` and before `Ok(Some(obj))`:

```rust
                // C `Read` calls `FixUsed( this, 0, 1, remove )` with
                // `remove` set for NATIVE encoding (fitschan.c:25924-25930),
                // so the cards a successful native read consumed are deleted
                // rather than merely flagged.
                if let Some((first, last)) = enc_native::native_span(&self.cards) {
                    for i in first..=last {
                        self.cards.mark_at(i);
                    }
                    self.cards.mark_com_block(first, -1);
                    self.cards.mark_com_block(last, 1);
                    self.cards.fix_used(true, true);
                }
```

Leave the COMMENT-card branch (`extract_native_text`) alone.
That storage form has no C counterpart — C's `FindAst` skips
`AST__COMMENT` cards outright — so no marking rule can be derived from C for
it, and no fixture reaches it (all four keyword-card inputs satisfy
`has_keyword_native`).

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p ast-core --test test_fitschan_native_keyword`
Expected: PASS, 5 tests.

- [ ] **Step 7: Re-measure and shrink the exclusion list**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | grep -E "specflux|timeplot"
```
Expected: both absent from the failure list.

Then delete these two lines from `NATIVE_READ_EXCLUDED`:

```rust
    ("specflux_native", "line count: 54 vs 27 (flux frame)"),
    ("timeplot_native", "line count: 66 vs 33 (time frame)"),
```

Match the reason strings against the file — they are the two the spec quotes
as misdiagnosed, and neither has anything to do with a flux or time frame.

- [ ] **Step 8: Run the full write-path regression breadth**

Deleting cards on read changes what every downstream write sees:

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan_encodings 2>&1 | tail -3
cargo test -p ast-core --test test_channel 2>&1 | tail -3
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -3
cargo test -p ast-core 2>&1 | tail -5
```
Expected: 192 passed / 0 failed on wcsconv; every other suite unchanged.

`native_rewind_and_reread` (`test_fitschan.rs:443`) is the one to watch: if it
fails, the question is whether C can re-read a native object from the same
chan at all after `FixUsed` deleted the cards. Read `Read`'s terminal branch
(fitschan.c:25873-25933) and match it; do not weaken the removal to make the
test pass.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/card_list.rs \
        crates/ast-core/src/fitschan/enc_native.rs \
        crates/ast-core/src/fitschan/mod.rs \
        crates/ast-core/tests/test_fitschan_native_keyword.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: a native read consumes its cards and banner blocks like C FixUsed"
```

---

### Task 5 (U2): PreQuote / UnPreQuote

Two sub-fixes on the two halves of one C function pair.
Sub-fix A is what moves the two cases; sub-fix B ports the write-side inverse,
which currently reconstructs C's rule from its effects instead of following it.
They are independent and each commits separately.

#### Sub-fix A: UnPreQuote the values a native read pulls off the cards

Two cases: `polco` and `polco2`, both at line 15.

```
got: CONTINUE  '"",1)     ""'
exp: CONTINUE  '",1)     "'
```

The input card is `CONTINUE  '",1)     "'` (`inputs/polco.head:13`).
Its FITS string value is `",1)     "` — a *pre-quoted* value: C `PreQuote`
(fitschan.c:25101) wrapped `,1)     ` in double quotes because its last
character is a space, which FITS treats as insignificant.

C reverses that on read.
`FindAst` calls `UnPreQuote` on each CONTINUE segment (fitschan.c:18128),
recovering `,1)     ` with its trailing spaces intact, so the joined title is
`Data grid indices; first pixel at (1,1)     `.
Our decoder never un-quotes: it splices the literal `",1)     "` into the
value, and the double quotes then survive into the rebuilt object and get
re-quoted on the way out.

C `UnPreQuote` (fitschan.c:34810-34881) strips exactly one outer pair of `"`
when the first and last characters are both `"` and the string is at least
two characters long.
It does not touch interior double quotes, and PreQuote's own comment explains
why: the ends of the string are known from the FITS quoting, so only the first
and last characters need inspecting.

Where C applies it, all in `FindAst`:

| Site | Line | Value |
|---|---|---|
| `BEGAST` class name | 17901 | UnPreQuoted |
| `ISA` class name | 17933 | UnPreQuoted |
| `ENDAST` class name | 17958 | UnPreQuoted |
| non-continued string data | 18010 | UnPreQuoted |
| first card of a continued value | 18011 | stored **raw** |
| each subsequent CONTINUE | 18032 | UnPreQuoted |

The first card of a continued value staying raw is deliberate: its trailing
`&` is overwritten by the next segment (`strcpy( *val + nc - 1, upq )`,
:18043), so there is nothing for the closing quote to protect.

Two things this task must NOT change:

- The `replace('"', "\"\"")` in `format_value` (`enc_native.rs:120`) and
  `format_value_with_continues` (`:273`), and the matching
  `replace("\"\"", "\"")` in `parse_scalar_value`
  (`serial/native_read.rs:124`). Those are the *channel-text* quoting layer
  and they match C exactly: `channel.c:5280` doubles interior quotes when
  writing a text item and `Unquote` (`channel.c:3989-4080`) un-doubles them
  when reading one. The defect is only the missing FITS-level un-quoting
  underneath.
- The `&`-before-`UnPreQuote` ordering. C tests for the trailing ampersand on
  the raw value (`:18028`) and only then un-quotes, so a CONTINUE segment
  that was itself wrapped in double quotes ends the chain. Reproduce that
  ordering; it is C's behavior, not a bug to route around.

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs` (add `un_pre_quote`; apply
  it in `expect_class_name`, `format_value`'s string arm via the caller, and
  `format_value_with_continues`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs`
- Test: `crates/ast-core/src/fitschan/enc_native.rs` (`mod tests`)

**Interfaces:**
- Consumes: `chr_len` (enc_native.rs:605) — C `ChrLen`, already present.
- Produces: `fn un_pre_quote(value: &str) -> &str` (private to `enc_native`).

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/ast-core/src/fitschan/enc_native.rs`:

```rust
    #[test]
    fn un_pre_quote_strips_one_outer_pair() {
        // C UnPreQuote (fitschan.c:34863-34875).
        assert_eq!(un_pre_quote("\",1)     \""), ",1)     ");
        assert_eq!(un_pre_quote("plain"), "plain");
        // Interior double quotes are left alone.
        assert_eq!(un_pre_quote("\"a\"b\""), "a\"b");
        // A single character is never a pair.
        assert_eq!(un_pre_quote("\""), "\"");
        assert_eq!(un_pre_quote(""), "");
    }

    #[test]
    fn continued_value_un_pre_quotes_each_continue_segment() {
        let cards = cards_from(concat!(
            "BEGAST_A= 'Frame   '\n",
            "TITLE_A = 'first pixel at (1&'\n",
            "CONTINUE  '\",1)     \"'\n",
            "ENDAST_A= 'Frame   '\n",
        ));
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(
            text.contains("Title = \"first pixel at (1,1)     \""),
            "trailing spaces must survive un-quoting; got:\n{text}"
        );
    }
```

Check the attribute name the decoder emits for `TITLE_A` against the
neighbouring tests before settling on `Title` — `cards_to_native_text`
lower-cases the keyword stem (`KeywordKind::Data`), so the expected token may
be `title`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ast-core --lib fitschan::enc_native`
Expected: FAIL — `un_pre_quote` is not defined (compile error), and once it is
defined the second test fails with the joined value carrying literal `"`
characters: `Title = "first pixel at (1"",1)     """`.

- [ ] **Step 3: Add `un_pre_quote`**

Next to `pre_quote` in `enc_native.rs`:

```rust
/// Reverse `pre_quote`: strip one outer pair of double quotes when the first
/// and last characters are both `"`. Mirrors C `UnPreQuote`
/// (fitschan.c:34810). Interior double quotes are untouched — the ends of the
/// string are known from the FITS quoting, so only the first and last
/// characters are significant.
fn un_pre_quote(value: &str) -> &str {
    let b = value.as_bytes();
    if b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"' {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
```

- [ ] **Step 4: Apply it at C's six sites**

In `expect_class_name`, replace `Ok(s.trim().to_string())` with an un-quote of
the `ChrLen`-truncated value:

```rust
        FitsValue::String(s) => Ok(un_pre_quote(&s[..chr_len(s)]).to_string()),
```

`chr_len` returns a character count; index by byte only after confirming the
value is ASCII, or switch to `s.trim_end()` which is the same truncation for
these values. Keep whichever form the surrounding code already uses.

In `format_value`'s `FitsValue::String` arm, un-quote after the
`trim_end` (C's `ChrLen`) and before the channel-text escaping:

```rust
                let unquoted = un_pre_quote(trimmed);
                let escaped = unquoted.replace('"', "\"\"");
```

In `format_value_with_continues`, the first card stays raw and each CONTINUE
segment is un-quoted. Replace the `&`-stripping and the push:

```rust
    // C stores the first card of a continued value raw (fitschan.c:18107) and
    // overwrites its trailing ampersand with the next segment; only the
    // CONTINUE segments are un-quoted.
    let mut joined = trimmed.strip_suffix('&').unwrap_or(trimmed).to_string();
```

and, inside the loop, after `let next_continues = seg_trimmed.ends_with('&');`
— the ampersand test comes first, on the raw value, exactly as C does at
:18028:

```rust
        let seg_text = seg_trimmed.strip_suffix('&').unwrap_or(seg_trimmed);
        joined.push_str(un_pre_quote(seg_text));
```

`strip_suffix` replaces `trim_end_matches('&')`: C removes exactly one
ampersand, not a run of them.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ast-core --lib fitschan::enc_native`
Expected: PASS.

- [ ] **Step 6: Re-measure and shrink the exclusion list**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | grep -E "polco"
```
Expected: neither `polco_native` nor `polco2_native` in the failure list.

Then delete these two lines from `NATIVE_READ_EXCLUDED`:

```rust
    ("polco_native", "CONTINUE: long-string split"),
    ("polco2_native", "CONTINUE: long-string split"),
```

Match the reason strings against the file before deleting.

- [ ] **Step 7: Confirm `timj_native` is still green**

`timj_native` passes today and carries a continued string
(`expected/timj.native:63-64`, `CONTINUE  '4,-381) '`), so it is the direct
regression check on this change:

```bash
cargo test -p ast-core --test test_wcsconv timj 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan_native_keyword 2>&1 | tail -3
cargo test -p ast-core --test test_channel 2>&1 | tail -3
cargo test -p ast-core 2>&1 | tail -5
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/enc_native.rs crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: UnPreQuote native card values as C FindAst does"
```

#### Sub-fix B: `pre_quote` is the write-side half of the same C function pair

The read side is what `polco` needs, and sub-fix A is complete on its own.
But `pre_quote` (`enc_native.rs:614-632`) is the inverse of the function just
ported, and it diverges from C `PreQuote` (fitschan.c:25091-25212) in three
ways.
One of them is an explicit heuristic, with a comment saying so:

```rust
    // C only adds double quotes when the *final accommodated* character needed
    // them; in practice this is trailing white space or a leading/trailing
    // double quote. Trailing white space is the common case for fixed-width
    // class names handled elsewhere, so guard on the actual need.
    let trailing_ws = bytes.last().is_some_and(|c| c.is_whitespace());
    if needs && (trailing_ws || starts_dquote) {
```

That is a reconstruction of C's rule from its observed effects, not a port of
it, and this plan's constraints forbid exactly that.
C computes `dq` fresh on every character and keeps only the last surviving
value, so the answer depends solely on the final *accommodated* character —
which is not the same as "the last character" whenever truncation fires.

The three divergences:

| C | our `pre_quote` |
|---|---|
| A zero-length value yields `dquotes = 2` — the initialiser is never overwritten because the loop body never runs, so C emits `""`. | Returns `String::new()`, emitting nothing. |
| `dquotes` is the `dq` of the last accommodated character. | Reconstructed from `needs && (trailing_ws \|\| starts_dquote)` over the whole string. |
| Truncates: a character is accepted only while `nc + 1 + dq + sq <= AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 4` (= 80 − 8 − 4 = 68), counting single quotes because the FITS encoder doubles them. | No truncation, no single-quote accounting. |

No wcsconv case is known to depend on any of the three — sub-fix A is what
moves `polco`.
Port it anyway: the read side now round-trips C exactly, and leaving the write
side as a reconstruction means the pair no longer agrees on the boundary cases.

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_native.rs:605-632` (`pre_quote`, and `chr_len` stays as it is)
- Test: `crates/ast-core/src/fitschan/enc_native.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: `un_pre_quote` from sub-fix A, for the round-trip test.
- Produces: `pre_quote` keeps its `fn pre_quote(value: &str) -> String` signature. Callers do not change.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `enc_native.rs`:

```rust
    #[test]
    fn pre_quote_empty_value_is_two_double_quotes() {
        // C PreQuote initialises dquotes = 2 and the loop never runs
        // (fitschan.c:25169-25177), so a zero-length value is emitted as "".
        assert_eq!(pre_quote(""), "\"\"");
    }

    #[test]
    fn pre_quote_truncates_at_the_fits_value_limit() {
        // The accommodation test is
        //   nc + 1 + dq + sq > AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 4
        // (fitschan.c:25202), i.e. > 68 for the standard 80/8 card layout.
        let long = "x".repeat(80);
        assert_eq!(pre_quote(&long), "x".repeat(68));
        // Each single quote will be doubled by the card formatter, so it
        // costs two of the 68 columns.
        let quotes = "'".repeat(80);
        assert_eq!(pre_quote(&quotes), "'".repeat(34));
    }

    #[test]
    fn pre_quote_uses_the_last_accommodated_character() {
        // Trailing white space that is truncated away does not force quotes,
        // because dquotes tracks the last *accepted* character.
        let v = format!("{}   ", "y".repeat(68));
        assert_eq!(pre_quote(&v), "y".repeat(68));
        // Trailing white space that survives does force them.
        assert_eq!(pre_quote("z  "), "\"z  \"");
    }

    #[test]
    fn pre_quote_un_pre_quote_round_trip() {
        for v in ["", "z  ", "\"lead", "plain", "a\"b"] {
            let q = pre_quote(v);
            assert_eq!(un_pre_quote(&q), v, "round trip failed for {v:?}");
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --lib fitschan::enc_native::tests::pre_quote 2>&1 | tail -20
```
Expected: `pre_quote_empty_value_is_two_double_quotes` fails (`""` vs `"\"\""`),
`pre_quote_truncates_at_the_fits_value_limit` fails (no truncation), and the
round-trip fails on `""`.
`pre_quote_uses_the_last_accommodated_character` may pass on its second
assertion and fail on the first.

- [ ] **Step 3: Port C's loop**

Replace `pre_quote`'s body with C's single pass.

The limit is C's `AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 4`.
`card.rs` already has both constants — `FITS_CARD_LEN = 80` (`:10`) and
`FITS_NAME_LEN = 8` (`:13`) — but both are module-private `const`s, so make them
`pub(super)` and import them:

```rust
use super::card::{FITS_CARD_LEN, FITS_NAME_LEN};
```

Do not redefine the numbers in `enc_native.rs`; two copies of a card-geometry
constant is how they drift apart.

```rust
/// Pre-quote a value with surrounding double quotes when it has trailing white
/// space or double quotes at either end, truncating it to what a FITS card can
/// hold. Mirrors C `PreQuote` (fitschan.c:25102).
///
/// Embedded single quotes are not doubled here — the card formatter doubles
/// them when rendering the card — but they are *counted* against the length
/// limit, because that doubling is what makes them cost two columns.
/// Interior double quotes are left alone: FITS delimits the value, so only the
/// first and last characters matter when un-quoting.
fn pre_quote(value: &str) -> String {
    // C: AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 4 == 80 - 8 - 4.
    const MAX: usize = FITS_CARD_LEN - FITS_NAME_LEN - 4;

    let chars: Vec<char> = value.chars().collect();
    let starts_dquote = chars.first() == Some(&'"');

    // A zero-length value keeps the initial 2, so it is emitted as `""`.
    let mut dquotes = 2;
    let mut nc = 0;
    let mut sq = 0;

    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            sq += 1;
        }
        let dq = if c.is_whitespace() || (starts_dquote && c == '"') {
            2
        } else {
            0
        };
        if nc + 1 + dq + sq > MAX {
            break;
        }
        nc = i + 1;
        dquotes = dq;
    }

    let body: String = chars[..nc].iter().collect();
    if dquotes != 0 {
        format!("\"{body}\"")
    } else {
        body
    }
}
```

Two details that are easy to get wrong, both visible in the C:

- `sq` is incremented *before* the accommodation test, so the character that
  pushes the total over the limit is rejected with its own cost counted
  (fitschan.c:25179-25204).
- `nc = i + 1` and `dquotes = dq` are both inside the loop and both after the
  `break`, so a truncated tail influences neither.

- [ ] **Step 4: Run the tests to verify they pass**

Run:
```bash
cargo test -p ast-core --lib fitschan::enc_native 2>&1 | tail -5
```
Expected: PASS, including every pre-existing `enc_native` test.

- [ ] **Step 5: Confirm no output changed**

The whole native corpus goes through `pre_quote`, so this is the check that the
port is a true refactor for every value that occurs in practice:

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_native_roundtrip_all 2>&1 | tail -3
cargo test -p ast-core --test test_serialisation 2>&1 | tail -3
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -3
```
Expected: all unchanged — 192 generated wcsconv cases green, the 24 + 139 + 719
round-trips still byte-exact, simplify 427.

If any of them changes, do not adjust the fixture.
A changed output means some committed value does depend on one of the three
divergences, which makes it a finding: identify the value, confirm against the C
what it should be, and record it in the commit message.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/enc_native.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: port PreQuote's accommodation loop instead of reconstructing its rule"
```

---

### Task 6 (U4, spec deliverable D2): License the `IsSimp`/`Invert` aliasing difference in the harness

**Four** cases, not the three the spec names: `serpens`, `wave-f2w`,
`vopt-f2w` and **`cd-cross`**.

`cd-cross` is assigned to cause 9 in the spec on the strength of its first
diff (`ISA_K = 'Mapping '` where the reference has `INVERT_B= 1`).
Measurement says otherwise — see step 1.

The cause is documented in `docs/issues/c-library-quirks.md` under "CmpMap
`IsSimp` tag placement depends on shared-pointer aliasing during
simplification": C's `astMapList` hands out reference-counted pointers to live
component objects, and `astSetInvert` on a shared node clears its `IsSimple`
bit as a side effect (`mapping.c:23256`), so which interior node keeps a tag
depends on C's aliasing history rather than on tree shape.
The port uses owned values, so probe-time inversions touch throwaway clones
and the asymmetry is unreproducible by construction.
Mirroring C's aliasing in Rust is explicitly rejected.

That leaves the harness.
The measured licensed difference is exactly the set of `ISSIMP<xx>` and
`INVERT<xx>` cards: drop those lines from both sides and all four cases are
equal under the comparator the harness already uses.

The sequence suffixes survive because C `CreateKeyword` (fitschan.c:8423-8478)
allocates sequence numbers *per base keyword name* from `this->keyseq`, so
removing every `ISSIMP`/`INVERT` card removes the whole of the affected
numbering and leaves every other keyword's `_A`/`_B`/`_C` run untouched.

**Files:**
- Modify: `crates/ast-core/tests/common/mod.rs` (add the shared predicate)
- Modify: `crates/ast-core/tests/test_simplify_regression.rs:113-119` (use the shared helper)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (apply the rule; drop four entries)
- Modify: `docs/issues/c-library-quirks.md`
- Modify: `docs/superpowers/specs/2026-08-04-wcsconv-native-read-parity-design.md`

**Interfaces:**
- Consumes: `common::line_tokens_equivalent(got, want, max_ulps)` (common/mod.rs:36).
- Produces:
  - `common::is_aliasing_tag_card(line: &str) -> bool`
  - `common::strip_is_simp(text: &str) -> String` (moved, unchanged semantics)

- [ ] **Step 1: Measure, and settle `cd-cross`**

```bash
for f in wave-f2w vopt-f2w cd-cross serpens; do
  echo "=== $f"
  cargo run -q --example wcsconv_debug \
    crates/ast-core/tests/fixtures/wcsconv/inputs/$f.head native 2>/dev/null \
    | tail -n +2 | sed 's/ *$//' | grep -v '^$' | grep -v '^END' > /tmp/$f.got.txt
  diff <(grep -vE '^(ISSIMP|INVERT)_' /tmp/$f.got.txt | tr -s ' ') \
       <(sed 's/ *$//' crates/ast-core/tests/fixtures/wcsconv/expected/$f.native \
         | grep -v '^ *$' | grep -v '^END' | grep -vE '^(ISSIMP|INVERT)_' | tr -s ' ')
done
```

Expected (this is the measurement the task rests on — reproduce it before
writing any code): `wave-f2w`, `vopt-f2w` and `cd-cross` produce **no** diff
at all; `serpens` produces only last-digit differences in `EPOCH_A` and
`IM0_A`-`IM3_A`, each under 2 ULP and therefore inside the harness's existing
4-ULP tolerance.

`cd-cross` producing no diff is the finding: it is the same licensed
difference as the other three, not a `WinMap`/`CmpMap` shape divergence.
Note that the card *counts* also differ — we emit 4 tag cards where C emits 5
for `cd-cross`, 3 vs 4 for `serpens` — so the rule has to drop the lines from
both sides, not pair them up.

- [ ] **Step 2: Write the failing test**

There is no new unit test here; the four wcsconv cases *are* the test. Make
them run by deleting their entries from `NATIVE_READ_EXCLUDED` first:

```rust
    ("serpens_native", "ISA_J: CmpMap IsSimp/Invert placement (C aliasing quirk)"),
    ("wave-f2w_native", "ISSIMP_B: CmpMap IsSimp placement (C aliasing quirk)"),
    ("vopt-f2w_native", "ISSIMP_B: CmpMap IsSimp placement (C aliasing quirk)"),
    ("cd-cross_native", "ISA_K: mapping-invert placement"),
```

Match each reason string against the file before deleting — the wording in the
list is what the previous measurement recorded, not necessarily the above.

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p ast-core --test test_wcsconv 2>&1 | grep -E "serpens|wave.f2w|vopt.f2w|cd.cross"`
Expected: four failures, at `serpens` line 99, `wave-f2w` line 108,
`vopt-f2w` line 109, `cd-cross` line 129.

- [ ] **Step 4: Add the shared helper**

In `crates/ast-core/tests/common/mod.rs`:

```rust
/// True for a keyword-card line holding a `IsSimp` or `Invert` tag
/// (`ISSIMP<xx>=` / `INVERT<xx>=`, where `<xx>` is a two-character sequence
/// number).
///
/// Where these tags land inside a compound Mapping is decided in C by which
/// nodes were aliased into a simplification probe, because `astSetInvert`
/// clears a shared node's `IsSimple` bit as a side effect
/// (`mapping.c:23256`). The port holds components by value, so the asymmetry
/// has no analogue here. Callers that compare a serialised object against a
/// C-generated reference use this to drop the tags from both sides.
/// See `docs/issues/c-library-quirks.md`.
pub fn is_aliasing_tag_card(line: &str) -> bool {
    let key = line.split(['=', ' ']).next().unwrap_or("");
    (key.starts_with("ISSIMP") && key.len() == 8) || (key.starts_with("INVERT") && key.len() == 8)
}
```

Move `strip_is_simp` here verbatim from `test_simplify_regression.rs:113-119`
and make it `pub`; leave its behavior alone (it filters the text-dump
`IsSimp =` form only, and widening it to `Invert` would loosen a passing
harness). Replace the private copy with `common::strip_is_simp`.

- [ ] **Step 5: Apply the rule in the wcsconv harness**

In `crates/ast-core/tests/test_wcsconv.rs`, next to `NATIVE_READ_EXCLUDED`:

```rust
/// Cases whose only difference from the C reference is where an `IsSimp` or
/// `Invert` tag sits inside a compound Mapping. Those lines are dropped from
/// both sides and everything else is compared as usual.
/// See `docs/issues/c-library-quirks.md`.
const ALIASING_TAG_LICENSED: &[&str] = &[
    "serpens_native",
    "wave-f2w_native",
    "vopt-f2w_native",
    "cd-cross_native",
];
```

and, immediately after `let out_lines = normalize(&output);`:

```rust
        let (exp_lines, out_lines) = if ALIASING_TAG_LICENSED.contains(&case.name.as_str()) {
            let keep = |ls: Vec<&str>| -> Vec<&str> {
                ls.into_iter()
                    .filter(|l| !common::is_aliasing_tag_card(l))
                    .collect()
            };
            (keep(exp_lines), keep(out_lines))
        } else {
            (exp_lines, exp_lines_unused_placeholder)
        };
```

Write that as a plain pair of `let mut` bindings followed by a conditional
`retain` rather than the sketch above — the sketch is there to show which two
vectors are filtered and where, not to be transcribed literally:

```rust
        let mut exp_lines = normalize(&expected_text);
        let mut out_lines = normalize(&output);
        if ALIASING_TAG_LICENSED.contains(&case.name.as_str()) {
            exp_lines.retain(|l| !common::is_aliasing_tag_card(l));
            out_lines.retain(|l| !common::is_aliasing_tag_card(l));
        }
```

- [ ] **Step 6: Run to verify they pass**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -3
```
Expected: 192 passed, 0 failed; the simplify regression unchanged at
427 passed.

Also confirm the rule is narrow: it must not silence anything else.

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```
Expected: the failure count drops by exactly four from the pre-task number.

- [ ] **Step 7: Update the quirks doc and the spec**

In `docs/issues/c-library-quirks.md`, in the `IsSimp` entry, replace the
sentences that say the wcsconv fixtures are excluded with the harness rule:
the four cases `serpens`, `wave-f2w`, `vopt-f2w` and `cd-cross` are compared
with `ISSIMP<xx>`/`INVERT<xx>` cards dropped from both sides
(`common::is_aliasing_tag_card`), and everything else byte-compared.
Record that `cd-cross` belongs here, and that the tag *count* differs as well
as the placement.

In `docs/superpowers/specs/2026-08-04-wcsconv-native-read-parity-design.md`:

- change cause 4's heading to `### Cause 4 — documented C aliasing quirk (4 cases)`
  and add `cd-cross` to its case list, noting the measurement that put it there;
- remove `cd-cross` from cause 9's case list and change its heading to
  `### Cause 9 — object shape, CmpMap vs WinMap (5 cases)`, deleting the
  sentence that calls `cd-cross` a mapping-invert-placement shape divergence;
- change the work-units tables: U4 from 3 cases to 4, U9 from 6 to 5;
- change D2's opening from "Three cases differ" to "Four cases differ".

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/tests/common/mod.rs \
        crates/ast-core/tests/test_simplify_regression.rs \
        crates/ast-core/tests/test_wcsconv.rs \
        docs/issues/c-library-quirks.md \
        docs/superpowers/specs/2026-08-04-wcsconv-native-read-parity-design.md
git -c core.hooksPath=/dev/null commit -m "test: license the C IsSimp/Invert aliasing difference in the wcsconv harness"
```

---

## Tier 2 — narrow reader fixes

Four tasks, mutually independent, each landing 1-4 cases.
Tier 1 need not be finished first; nothing here touches the write path.

### Task 7 (U5): The sky reference position honors RFVAL and AXREF

**Cases:** `rfval`, `axref` (2).

Measured first diffs:

```
rfval  L42  got SREF1_A = 2.0943951023931953   exp SREF1_A = 2.0856684561332237
axref  L43  got SREF1_A = 2.0943951023931953   exp SREF1_A = 2.0856684561332237
```

`2.0943951023931953` rad is 120.0°, taken from `CRVAL1`.
`2.0856684561332237` rad is 119.5°, and both headers carry a 119.5 override — `rfval.head` as `RFVAL1`, `axref.head` as `AXREF1`.
The matching latitude card is `SREF2_A = 0.53232542185827048` (30.5°, from `RFVAL2` / `AXREF2`) against our `CRVAL2` of 30.0°.

C's precedence in `WcsCelestial` runs in four steps:

1. `*reflon`/`*reflat` initialised from CRVAL (fitschan.c:36142-36145).
2. **RFVAL override** (fitschan.c:36221-36227) — replaces `*reflon`/`*reflat` and nothing else.
3. The `astTestSkyRef` gate (fitschan.c:36228-36236) — stores `SkyRef` + `SkyRefIs=Ignored` only if the SkyFrame has no set reference position already.
4. **AXREF override** (fitschan.c:36339-36353) — replaces `*reflon`/`*reflat` **and calls `astSetSkyRef` + `SkyRefIs=Ignored` unconditionally**, outside any gate.

That asymmetry is the whole design and must be preserved: RFVAL is subject to the gate, AXREF bypasses it.
C's comments explain why — RFVAL is written by `SpecTrans` to preserve pre-translation values for a SpecFrame reference position, while AXREF is written by `astWrite` for `-TAB` axes with no inverse transformation.

Both keywords are read with `GetValue( ..., report = 0, mark = 0, ... )`, so a missing keyword is not an error and neither keyword is consumed.
`AXREF%d` / `AXREF%d%1c` are already in `wcs_mark.rs`'s `TEMPLATES` (lines 131-132), so AXREF is already marked as WCS-description when `WcsFcRead` reads it; RFVAL is *not* in `TEMPLATES` and must not be added — C has no `WcsFcRead` branch for it at all (its only three appearances in `fitschan.c` are the read at 36091-36093 and the comment above them).

Only `axref` gets a `FitsStore` field.
C has `double ***axref` as a real `FitsStore` member (fitschan.c:1728) with `Match` branches at :36766/:36774 and a writer at :37244; RFVAL is read straight off the FitsChan and has no store member.
Mirror that split rather than flattening both into the store.

**Files:**
- Modify: `crates/ast-core/src/fitschan/fits_store.rs:99-101` (add `axref` beside `skyref`/`skyrefp`), `:185-186` (initialise it in `new`)
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs:263-268` (read `AXREF<i>` in the SREF loop)
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:718-733` (the `ref_pos` computation), and `:80-98` (`CelestialResult`)
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs:248-289` (`set_sky_ref_from_celestial`, to honor the ungated AXREF path)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (delete two `NATIVE_READ_EXCLUDED` entries)
- Test: `crates/ast-core/src/fitschan/wcs_celestial.rs` (unit test in the existing `mod tests`)

**Interfaces:**
- Consumes: `FitsStore` (`crates/ast-core/src/fitschan/fits_store.rs`), `key_for` (`wcs_keywords.rs:18`), `CardList::get_float_marked` / `get_float`.
- Produces:

```rust
// fits_store.rs — new field on FitsStore
/// `AXREF<i>` values in degrees, indexed by 0-based FITS axis. Written by
/// C `astWrite` for `-TAB` axes with no inverse transformation
/// (fitschan.c:1728); on read they override the CRVAL-derived sky
/// reference position and force SkyRef to be set (fitschan.c:36339-36353).
pub axref: Vec<Option<f64>>,
```

```rust
// wcs_celestial.rs — CelestialResult gains one field
/// True when `ref_pos` came from `AXREF<i>` rather than from CRVAL or
/// `RFVAL<i>`. C sets SkyRef unconditionally in the AXREF case
/// (fitschan.c:36339-36353), bypassing the `astTestSkyRef` gate that
/// guards the CRVAL / RFVAL path (fitschan.c:36228-36236).
pub ref_pos_forced: bool,
```

- [ ] **Step 1: Reproduce both diffs**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv rfval_native 2>&1 | grep -E 'SREF1_A|SREF2_A'
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv axref_native 2>&1 | grep -E 'SREF1_A|SREF2_A'
```

Expected: each prints a `got SREF1_A = 2.0943951023931953` against `exp SREF1_A = 2.0856684561332237`.

- [ ] **Step 2: Write the failing test**

Append to the `mod tests` block at the end of `crates/ast-core/src/fitschan/wcs_celestial.rs`.
The existing tests there build a `FitsStore` by hand and call `build_celestial`; follow that shape.

```rust
    /// `AXREF<i>` overrides the CRVAL-derived sky reference position and
    /// forces it to be set (C `WcsCelestial`, fitschan.c:36339-36353).
    #[test]
    fn axref_overrides_crval_reference_position() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(120.0);
        store.crval[1] = Some(30.0);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);
        store.axref[0] = Some(119.5);
        store.axref[1] = Some(30.5);

        let result = build_celestial(&store).expect("celestial axes recognised");
        let (reflon, reflat) = result.ref_pos.expect("AXREF supplies a reference position");
        assert!((reflon - 119.5_f64.to_radians()).abs() < 1e-15);
        assert!((reflat - 30.5_f64.to_radians()).abs() < 1e-15);
        assert!(result.ref_pos_forced, "AXREF bypasses the astTestSkyRef gate");
    }

    /// With no `AXREF`, the reference position stays on CRVAL and is *not*
    /// forced, so the `astTestSkyRef` gate still applies.
    #[test]
    fn crval_reference_position_is_not_forced() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(120.0);
        store.crval[1] = Some(30.0);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);

        let result = build_celestial(&store).expect("celestial axes recognised");
        let (reflon, _) = result.ref_pos.expect("CRVAL supplies a reference position");
        assert!((reflon - 120.0_f64.to_radians()).abs() < 1e-15);
        assert!(!result.ref_pos_forced);
    }
```

- [ ] **Step 3: Run the test to verify it fails**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests::axref 2>&1 | tail -20
```

Expected: FAIL — `no field 'axref' on type 'FitsStore'` and `no field 'ref_pos_forced'`.

- [ ] **Step 4: Add the `axref` field to `FitsStore`**

In `crates/ast-core/src/fitschan/fits_store.rs`, after the `skyrefp` declaration (line 101):

```rust
    /// `AXREF<i>` values in degrees, indexed by 0-based FITS axis. Written by
    /// C `astWrite` for `-TAB` axes with no inverse transformation
    /// (fitschan.c:1728); on read they override the CRVAL-derived sky
    /// reference position and force SkyRef to be set (fitschan.c:36339-36353).
    pub axref: Vec<Option<f64>>,
```

and in `new` (beside `skyrefp: vec![None; naxis]` at line 186):

```rust
            axref: vec![None; naxis],
```

- [ ] **Step 5: Read `AXREF<i>` in `extract_wcs_keywords`**

In `crates/ast-core/src/fitschan/wcs_keywords.rs`, extend the existing SREF loop (lines 263-268) with a third line:

```rust
        store.axref[i] = cards.get_float_marked(&key_for("AXREF", Some(axis_num), s));
```

`get_float_marked` is right: `AXREF%d` / `AXREF%d%1c` are already in `wcs_mark.rs`'s `TEMPLATES` (lines 131-132), so C's `WcsFcRead` reads them with `mark` left at 1.
The later `GetValue` at fitschan.c:36339 passes `mark = 0`, but by then the card is already marked, so the effective behavior is "marked".

Do **not** add an `RFVAL` line here and do **not** add `RFVAL` to `wcs_mark.rs`: C has no `WcsFcRead` branch for it.

- [ ] **Step 6: Implement C's four-step precedence in `wcs_celestial.rs`**

Add the field to `CelestialResult` (after `ref_pos`, line 93):

```rust
    /// True when `ref_pos` came from `AXREF<i>` rather than from CRVAL or
    /// `RFVAL<i>`. C sets SkyRef unconditionally in the AXREF case
    /// (fitschan.c:36339-36353), bypassing the `astTestSkyRef` gate that
    /// guards the CRVAL / RFVAL path (fitschan.c:36228-36236).
    pub ref_pos_forced: bool,
```

Replace the `ref_pos` computation (lines 718-733) with C's chain.
Note `store` carries no `rfval` field by design — read it off the card list the way C's `GetValue` does; the caller passes the `CardList` in.

```rust
    // Celestial reference position (SkyRef) for the SkyFrame. C `WcsCelestial`
    // starts from the raw CRVAL values converted to radians (fitschan.c:36142-
    // 36015), NOT from the fiducial point transformed through the rotation. The
    // two are mathematically equivalent except that the rotation's atan2 wraps
    // longitude into (−π,π] (so a CAR frame with CRVAL=185° would become −175°)
    // and loses precision in the last ULPs; the raw CRVAL avoids both.
    let mut ref_pos = match (store.crval[lon_axis], store.crval[lat_axis]) {
        (Some(_), Some(_)) => Some((crval_lon, crval_lat)),
        _ => None,
    };
    let mut ref_pos_forced = false;

    // RFVAL<i> overrides CRVAL (fitschan.c:36221-36227). `SpecTrans` writes
    // these to preserve the pre-translation CRVAL values, which are what a
    // SpecFrame reference position needs. The override changes the reference
    // position only — it does not force SkyRef to be set, so the
    // `astTestSkyRef` gate downstream still applies.
    if let (Some(lon), Some(lat)) = (
        rfval.and_then(|r| r[lon_axis]),
        rfval.and_then(|r| r[lat_axis]),
    ) {
        ref_pos = Some((lon * deg2rad, lat * deg2rad));
    }

    // AXREF<i> overrides both (fitschan.c:36339-36353) and, unlike RFVAL,
    // sets SkyRef unconditionally — outside the `astTestSkyRef` gate.
    // `astWrite` emits these for `-TAB` axes that have no inverse transform.
    if let (Some(lon), Some(lat)) = (store.axref[lon_axis], store.axref[lat_axis]) {
        ref_pos = Some((lon * deg2rad, lat * deg2rad));
        ref_pos_forced = true;
    }
```

Then thread `rfval: Option<&[Option<f64>]>` through `build_celestial_with_def_b1950`'s signature, defaulting to `None` in the `#[cfg(test)] build_celestial` wrapper (line 108) and in the unit tests, and fill it at the two call sites in `wcs_read.rs` (lines 42 and 465) by reading `RFVAL<i>` off the card list with the plain `get_float` (unmarked — C passes `mark = 0` and there is no earlier marking pass for RFVAL).

Finally set `ref_pos_forced` in the two `CelestialResult` constructions and return it alongside `ref_pos` from the pipeline helper at line 738.

- [ ] **Step 7: Honor `ref_pos_forced` in `set_sky_ref_from_celestial`**

`crates/ast-core/src/fitschan/wcs_read.rs:248` currently returns early on `sky.test_sky_ref(0)` and on the three offset domains.
C's AXREF path runs after and outside that gate, so pass the flag in and skip the gate when it is set:

```rust
fn set_sky_ref_from_celestial(
    fs: &mut FrameSet,
    ref_pos: Option<(f64, f64)>,
    forced: bool,
) -> AstResult<()> {
```

and guard the existing early-return block:

```rust
    // C applies the `astTestSkyRef` gate to the CRVAL / RFVAL reference
    // position (fitschan.c:36228-36236) but not to the AXREF one, which is
    // stored unconditionally (fitschan.c:36339-36353).
    if !forced {
        let cur = fs.current();
        if let Some(sky) = peek_sky(cur) {
            if sky.test_sky_ref(0) {
                return Ok(());
            }
            if matches!(sky.domain(), "SKY_OFFSETS" | "SKY_POLE" | "SKY_ORIGIN") {
                return Ok(());
            }
        }
    }
```

Update the call site at line 105 to pass the flag captured from `celestial`, alongside the existing `celestial_ref_pos` capture at line 45:

```rust
    let celestial_ref_pos_forced = celestial.as_ref().is_some_and(|c| c.ref_pos_forced);
```

- [ ] **Step 8: Run the unit tests to verify they pass**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial 2>&1 | tail -5
```

Expected: PASS, including the two new tests.

- [ ] **Step 9: Re-measure and delete the two entries**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

`axref.head` also carries an alt-A plane (`CTYPE1A`, `CRVAL1A=120.000`, `AXREF1A=119.500`, `AXREF2A=30.500`) whose expected output has its own `FRM3_A` / `BEGAST_H='SkyFrame'` / `IDENT_B='A'` block, so the alt-plane call site at `wcs_read.rs:465` must be wired too — if `axref` still differs after the primary plane is fixed, that is the reason.

Delete the `rfval` and `axref` rows from `NATIVE_READ_EXCLUDED` in `crates/ast-core/tests/test_wcsconv.rs` once both are byte-exact.

- [ ] **Step 10: Run the read-path regression breadth**

```bash
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
cargo test -p ast-core --test test_transform_oracle head_sections 2>&1 | tail -5
```

`HEAD_ORACLE_EXCLUDED` is empty and must stay empty.

- [ ] **Step 11: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/fits_store.rs \
        crates/ast-core/src/fitschan/wcs_keywords.rs \
        crates/ast-core/src/fitschan/wcs_celestial.rs \
        crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: honor RFVAL and AXREF for the sky reference position (rfval, axref)"
```

---

### Task 8 (U6): Time attributes — DTAI on the world Frame, and a stale `azel` fixture

**Cases:** `multi-wcs-spec`, `azel` (2).

The two have nothing in common beyond both being time-related, and their fixes are unrelated.
Sub-task A is a real port gap; sub-task B is a stale fixture.
Do them as two commits.

Measured first diffs:

```
multi-wcs-spec  L34  got AXP1_A  = 3                    exp DTAI_A  = 37.0
azel            L30  got EPOCH_A = 2017.5519807675657   exp EPOCH_A = 2017.5519807923772
```

#### Sub-task A — DTAI belongs on the top-level world Frame too

`multi-wcs-spec.head` carries `TELESCOP='JCMT    '` (line 4, commented "enables DTAI") and `DTAI=37.000` (line 31).
The expected output carries `DTAI` **twice**: `DTAI_A = 37.0 / TAI-UTC in seconds` on the outer CmpFrame, and `DTAI_B = 37.0` on the inner SkyFrame.
We emit only the inner one, so our `AXP1_A` lands where the reference has `DTAI_A`.

C `WcsMapFrm` applies both time offsets to the **top-level world Frame** `*frm`, immediately after `astSetIdent( *frm, id )` (fitschan.c:38011):

```c
   if( dut1 != AST__BAD ) astSetDut1( *frm, dut1*SPD );
   if( dtai != AST__BAD ) astSetDtai( *frm, dtai );
```

(fitschan.c:38014-38022.)
The port sets them only on the inner SkyFrame (`wcs_celestial.rs:362-368`), which is why `DTAI_B` is present and `DTAI_A` is not.
Both are correct — C sets them in both places, from two different call sites.

There is a second, independent divergence in the same area.
`wcs_keywords.rs:247` reads `DTAI` unconditionally, where C gates the *read* on `TELESCOP` (fitschan.c:36701-36713): only when `GetValue( fc, "TELESCOP", ... )` succeeds **and** the value starts with `JCMT` (4 chars) or `UKIRT` (5 chars).
`mark` is 0 either way, so a `DTAI` card is never consumed and the gate changes only whether the value reaches the store.
The comment above line 247 argues the gate does not matter because the card survives into output regardless; that is true of the *card* but not of the *Frame attribute* — an ungated read stamps `Dtai` onto Frames for headers where C would leave it unset, which changes the dump.
Fix both; `multi-wcs-spec` exercises the gate's true branch, and the regression suites cover the false branch.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs:243-248` (gate the DTAI read on `TELESCOP`)
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs` (set DUT1/DTAI on the world Frame beside the existing `set_frame_ident(world_frame.as_mut(), " ")` call)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (delete the `multi-wcs-spec` entry)
- Create: `crates/ast-core/tests/test_fitschan_time.rs` (new file — no time-attribute FitsChan test file exists today)

**Interfaces:**
- Consumes: `FitsStore::dtai` / `::dut1`, `set_frame_ident` (`wcs_read.rs:663`), `Frame::set_dtai` / `set_dut1`.
- Produces:

```rust
// wcs_read.rs
/// Apply the observation-global time offsets to `frame`, mirroring C
/// `WcsMapFrm` (fitschan.c:38014-38022). `Dut1` is stored in seconds by
/// FITS and in days by AST, hence the SPD scaling; `Dtai` is in seconds
/// in both.
fn set_frame_time_offsets(frame: &mut dyn Frame, dut1: Option<f64>, dtai: Option<f64>);
```

- [ ] **Step 1: Reproduce the diff**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv multi_wcs_spec_native 2>&1 | grep -E 'DTAI|AXP1'
```

Expected: `got AXP1_A = 3` against `exp DTAI_A = 37.0`.

- [ ] **Step 2: Write the failing test**

Create `crates/ast-core/tests/test_fitschan_time.rs`:

```rust
//! FITS time-attribute reading: `DTAI` / `DUT1` placement and the
//! `TELESCOP` gate on `DTAI`.
//!
//! C `WcsFcRead` reads `DTAI` only for JCMT and UKIRT headers
//! (fitschan.c:36701-36713), and `WcsMapFrm` stamps both offsets onto the
//! top-level world Frame (fitschan.c:38014-38022) in addition to whatever
//! the celestial decoder does to the SkyFrame.

use ast_core::fitschan::{FitsChan, Encoding};

fn chan_from(lines: &[&str]) -> FitsChan {
    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::FitsWcs);
    for l in lines {
        fc.put_fits(l, false).unwrap();
    }
    fc.rewind();
    fc
}

const BASE: &[&str] = &[
    "NAXIS   =                    2",
    "NAXIS1  =                  100",
    "NAXIS2  =                  100",
    "CTYPE1  = 'RA---TAN'",
    "CTYPE2  = 'DEC--TAN'",
    "CRVAL1  =              120.000",
    "CRVAL2  =               30.000",
    "CRPIX1  =               50.500",
    "CRPIX2  =               50.500",
    "CDELT1  =              -0.0100",
    "CDELT2  =               0.0100",
];

#[test]
fn dtai_is_read_for_jcmt_and_set_on_the_world_frame() {
    let mut lines: Vec<&str> = BASE.to_vec();
    lines.push("TELESCOP= 'JCMT    '");
    lines.push("DTAI    =               37.000");
    let mut fc = chan_from(&lines);
    let fs = fc.read().unwrap().expect("WCS should be read");
    assert_eq!(fs.current().dtai(), Some(37.0));
}

#[test]
fn dtai_is_ignored_for_other_telescopes() {
    let mut lines: Vec<&str> = BASE.to_vec();
    lines.push("TELESCOP= 'VLT     '");
    lines.push("DTAI    =               37.000");
    let mut fc = chan_from(&lines);
    let fs = fc.read().unwrap().expect("WCS should be read");
    assert_eq!(
        fs.current().dtai(),
        None,
        "C reads DTAI only for JCMT / UKIRT (fitschan.c:36701-36713)"
    );
}

#[test]
fn dtai_is_ignored_when_telescop_is_absent() {
    let mut lines: Vec<&str> = BASE.to_vec();
    lines.push("DTAI    =               37.000");
    let mut fc = chan_from(&lines);
    let fs = fc.read().unwrap().expect("WCS should be read");
    assert_eq!(fs.current().dtai(), None);
}
```

If `Frame::dtai()` returns `f64` with a "not set" sentinel rather than `Option<f64>`, use the corresponding `test_dtai()` predicate instead and assert on that — check the accessor's real signature in `crates/ast-core/src/frames/` before writing the assertions, and adjust all three tests consistently.

- [ ] **Step 3: Run the test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_time 2>&1 | tail -20
```

Expected: `dtai_is_read_for_jcmt_and_set_on_the_world_frame` FAILS (no `Dtai` on the world Frame); the two negative tests also FAIL (we read `DTAI` unconditionally).

- [ ] **Step 4: Gate the DTAI read on `TELESCOP`**

Replace `crates/ast-core/src/fitschan/wcs_keywords.rs:243-248` with:

```rust
    // Observation-global TAI/UT1 offsets — mark=0 (C WcsFcRead: DUT1 line
    // 36497, DTAI 36506). Never consumed. C reads DTAI only when TELESCOP
    // names JCMT or UKIRT (fitschan.c:36701-36713); for any other telescope,
    // or with no TELESCOP card, the value is left out of the store entirely
    // so no Frame gets a set Dtai.
    store.dtai = match cards.get_string("TELESCOP") {
        Some(t) if t.starts_with("JCMT") || t.starts_with("UKIRT") => {
            cards.get_float(&key_for("DTAI", None, s))
        }
        _ => None,
    };
    store.dut1 = cards.get_float(&key_for("DUT1", None, s));
```

C compares the first 4 / 5 characters, so `starts_with` on the un-trimmed value is the faithful transcription — `'JCMT    '` matches on its first 4 characters exactly as C's `strncmp` does.
Confirm `get_string` returns the FITS string value with trailing blanks already stripped; if it does not, C's `strncmp` still matches, so `starts_with` remains correct either way.

- [ ] **Step 5: Set both offsets on the world Frame**

Add the helper to `crates/ast-core/src/fitschan/wcs_read.rs`, next to `set_frame_ident` (line 663):

```rust
/// Apply the observation-global time offsets to `frame`, mirroring C
/// `WcsMapFrm` (fitschan.c:38014-38022). `Dut1` is stored in seconds by FITS
/// and in days by AST, hence the SPD scaling; `Dtai` is in seconds in both.
fn set_frame_time_offsets(frame: &mut dyn Frame, dut1: Option<f64>, dtai: Option<f64>) {
    if let Some(dut1) = dut1 {
        frame.set_dut1(dut1 * 86_400.0);
    }
    if let Some(dtai) = dtai {
        frame.set_dtai(dtai);
    }
}
```

`set_frame_ident` (line 663) downcasts over the concrete frame classes because `Ident` is not on the `Frame` trait.
If `set_dut1` / `set_dtai` are likewise not on the trait, mirror `set_frame_ident`'s dispatch shape exactly — including its `CmpFrame` recursion — rather than inventing a second pattern; check `crates/ast-core/src/frames/mod.rs` for which of the two applies before writing the body.

Call it immediately after the existing `set_frame_ident(world_frame.as_mut(), " ")` in `decode_wcs_inner`, matching C's ordering (`astSetIdent` at fitschan.c:38011, the offsets at :37884-37892):

```rust
    set_frame_time_offsets(world_frame.as_mut(), store.dut1, store.dtai);
```

Leave `wcs_celestial.rs:362-368` alone — C sets the offsets on the SkyFrame *as well*, and `DTAI_B` in the expected output is already correct.

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_time 2>&1 | tail -5
```

Expected: 3 passed.

- [ ] **Step 7: Re-measure and delete the entry**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

Delete the `multi-wcs-spec` row from `NATIVE_READ_EXCLUDED` once it is byte-exact.
The `TELESCOP` gate changes which headers get a set `Dtai`, so watch `test_fitschan*` and the head transform oracle here — a header that previously received `Dtai` and now does not will surface as a Frame-attribute diff.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_keywords.rs \
        crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/tests/test_fitschan_time.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: gate DTAI on TELESCOP and set time offsets on the world Frame (multi-wcs-spec)"
```

#### Sub-task B — `azel.native` is a stale fixture, not a port gap

**This is the one sanctioned fixture regeneration in the plan.**
The standing rule is that expected files are C-generated ground truth and a fixture that looks wrong is a finding to investigate, not to overwrite.
The single exception the spec grants is "a documented C-behavior question settled by probe" — which is exactly this case, and the probe has already been run.

`azel.head` carries `DATE-OBS= '2017-07-21T08:38:39.087'` and no `MJD-OBS` or `TIMESYS`.
Our epoch and C's differ by 0.783 s, and the arithmetic identifies the cause exactly:

| | Julian epoch | MJD | offset from DATE-OBS |
|---|---|---|---|
| Our output | `2017.5519807675657` | `57955.36097535338` | +69.1835 s |
| Fixture | `2017.5519807923772` | `57955.360984415776` | +69.9665 s |

Both apply the same +69.1835 s TT−UTC offset for 2017 (32.184 s + 37 leap seconds).
The difference is entirely in the parsed seconds field: the fixture's epoch corresponds to `08:38:39.870`, ours to `08:38:39.087`.
That is the signature of C's old `DateObs` bug — the fractional-seconds field was read with `"%d"` into an `int`, so `.087` parsed as `87` and was reconstructed as `0.87 s`.
It was fixed in the reference submodule on 20-JUN-2026 by capturing the field width with `"%n"` markers (changelog at fitschan.c:1354-1359, commit `254d4c99`, which is an ancestor of the pinned submodule HEAD).
`azel.native` was committed in `e375ee9`, generated before that fix.

Probe result — the current reference C agrees with us, not with the fixture:

```bash
R=$PWD/reference/starlink-ast/build
F=$PWD/crates/ast-core/tests/fixtures/wcsconv
LD_LIBRARY_PATH= $R/ast_tester/wcsconverter \
  $F/inputs/azel.head native /tmp/azel.native "FitsDigits=17"
diff /tmp/azel.native $F/expected/azel.native
# 31c31
# < EPOCH_A =   2017.5519807675655 / Julian epoch of observation
# ---
# > EPOCH_A =   2017.5519807923772 / Julian epoch of observation
```

`2017.5519807675655` against our `2017.5519807675657` is a 2-ULP difference, inside the harness's 4-ULP NATIVE tolerance.

The whole 45-case set was checked the same way, so this is the only stale fixture: 44 of 45 regenerate byte-identical to what is committed (`class-hel`'s expected file is `class-spec-hel.native`, per the manifest row at `wcsconv_tests.txt:57` — it verifies clean under that name).
`azel` is the only fixture in the corpus whose `DATE-OBS` has a *leading-zero* fraction; `serpens` (`.717`) and `date-frac` (`.123`) have fractions the old parser handled correctly, and both regenerate byte-exact.

- [ ] **Step 1: Confirm the reference build is current**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git -C reference/starlink-ast merge-base --is-ancestor 254d4c99 HEAD \
  && echo "DateObs %n fix is in the pinned submodule HEAD"
ls -la reference/starlink-ast/build/ast_tester/wcsconverter
```

If `wcsconverter` is missing or older than `reference/starlink-ast/src/fitschan.c`, rebuild with cmake (never autotools) before proceeding.

- [ ] **Step 2: Re-run the whole-corpus staleness check**

```bash
W=$PWD; R=$W/reference/starlink-ast/build; F=$W/crates/ast-core/tests/fixtures/wcsconv
while IFS='|' read -r name input expected enc attrs _; do
  name=$(echo "$name" | tr -d ' '); enc=$(echo "$enc" | tr -d ' ')
  [ "$enc" = native ] || continue
  input=$(echo "$input" | tr -d ' '); expected=$(echo "$expected" | tr -d ' ')
  attrs=$(echo "$attrs" | tr -d ' ')
  [ -f "$F/expected/$expected" ] || { echo "MISSING $name"; continue; }
  rm -f /tmp/regen.native
  LD_LIBRARY_PATH= $R/ast_tester/wcsconverter "$F/inputs/$input" native \
    /tmp/regen.native "FitsDigits=17${attrs:+,$attrs}" >/dev/null 2>&1
  [ -f /tmp/regen.native ] || { echo "GENFAIL $name"; continue; }
  if diff -q <(sed -e 's/[[:space:]]*$//' /tmp/regen.native) \
             <(sed -e 's/[[:space:]]*$//' "$F/expected/$expected") >/dev/null
  then echo "OK    $name"; else echo "STALE $name"; fi
done < "$F/wcsconv_tests.txt"
```

Expected: exactly one `STALE` line, for `azel_native`.
**If any other case reports `STALE`, stop and investigate before regenerating anything** — more than one stale fixture means the reference build or the invocation is wrong, not that the corpus rotted.

- [ ] **Step 3: Regenerate only `azel.native`**

```bash
W=$PWD; R=$W/reference/starlink-ast/build; F=$W/crates/ast-core/tests/fixtures/wcsconv
LD_LIBRARY_PATH= $R/ast_tester/wcsconverter \
  "$F/inputs/azel.head" native "$F/expected/azel.native" "FitsDigits=17"
git diff --stat crates/ast-core/tests/fixtures/wcsconv/expected/azel.native
git diff crates/ast-core/tests/fixtures/wcsconv/expected/azel.native
```

Expected: a one-line diff, `EPOCH_A` only.
If the diff touches any other line, the invocation differs from the one that produced the rest of the corpus — stop and reconcile before committing.

- [ ] **Step 4: Record the finding in the quirks doc**

Append to `docs/issues/c-library-quirks.md`.
This is not a quirk we mirror; it is a fixture-provenance note, so say so plainly:

```markdown
## `azel.native` regenerated after the C `DateObs` fractional-seconds fix

`azel.head` carries `DATE-OBS= '2017-07-21T08:38:39.087'`.
The committed `azel.native` expected an epoch corresponding to `.870`,
because C's `DateObs` read the fractional-seconds field with `"%d"` into an
`int` and so lost its leading zero — `.087` parsed as `87` and was
reconstructed as `0.87 s`.
The reference submodule fixed this on 20-JUN-2026 (commit `254d4c99`,
changelog at `fitschan.c:1354-1359`) by capturing the field width with `"%n"`.

The fixture predated the fix and was regenerated against the pinned submodule
HEAD; the port needed no change.
`azel` is the only fixture in the wcsconv corpus with a leading-zero
`DATE-OBS` fraction — the other 44 native cases regenerate byte-identical.
```

- [ ] **Step 5: Re-measure and delete the entry**

```bash
cargo test -p ast-core --test test_wcsconv azel 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

`azel_native` should now pass on the 4-ULP NATIVE tolerance (our `…5657` against the regenerated `…5655`).
Delete the `azel` row from `NATIVE_READ_EXCLUDED`.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/tests/fixtures/wcsconv/expected/azel.native \
        crates/ast-core/tests/test_wcsconv.rs \
        docs/issues/c-library-quirks.md
git -c core.hooksPath=/dev/null commit -m "test: regenerate azel.native after the C DateObs fractional-seconds fix"
```

---

### Task 9 (U10): `Ident` set-ness

**Cases:** `class-hel`, `class-spec`, `tidy-offsets`, `tidy-pole` (4).

Measured first diffs — one bug seen from both sides:

```
class-hel     L23  got NAXES_B = 1              exp IDENT_A = '" "     '
class-spec    L23  got NAXES_B = 1              exp IDENT_A = '" "     '
tidy-offsets  L27  got IDENT_A = '" "     '     exp NAXES_B = 2
tidy-pole     L27  got IDENT_A = 'A       '     exp NAXES_B = 2
```

In `class-hel` / `class-spec` C emits `IDENT_A` and we omit it; in `tidy-offsets` / `tidy-pole` we emit it and C has already moved on.
Two of the four show we produce the right *string* (`'" "'` and `'A'`), so the defect is in *when* `Ident` counts as set, not in the attribute plumbing.
These are two independent sub-fixes in opposite directions — do them as two commits and re-measure between.

#### Sub-fix A — `set_frame_ident` has no `DSBSpecFrame` arm

`set_frame_ident` (`wcs_read.rs:663-690`) dispatches over `BasicFrame`, `SkyFrame`, `SpecFrame`, `TimeFrame`, `CmpFrame` and `FluxFrame`.
`DSBSpecFrame` is absent, so a header whose spectral axis is dual-sideband silently keeps its default (unset) `Ident` and the card is never dumped.
`class-spec-hel.head` and the `class-spec` input both produce a DSBSpecFrame; C's `astSetIdent( *frm, id )` (fitschan.c:37886) is class-agnostic and sets it regardless.

Verify the premise before writing the fix — if these two fixtures produce a plain `SpecFrame` and the missing `IDENT_A` has another cause, re-measure and re-diagnose rather than adding a speculative arm:

```bash
cargo run -p ast-core --example wcsconv_debug -- \
  crates/ast-core/tests/fixtures/wcsconv/inputs/class-spec-hel.head \
  native fitsdigits=17 2>&1 | grep -nE 'BEGAST|IDENT|DSB' | head -20
```

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs:663-690` (`set_frame_ident`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs`
- Test: `crates/ast-core/tests/test_fitschan_ident.rs` (create)

**Interfaces:**
- Consumes: `set_frame_ident` (`wcs_read.rs:663`), `DSBSpecFrame` (`crates/ast-core/src/frames/`).
- Produces: no new API — one added match arm.

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_fitschan_ident.rs`:

```rust
//! `Ident` set-ness on Frames built by a FITS read.
//!
//! C `WcsMapFrm` calls `astSetIdent( *frm, id )` (fitschan.c:37886) on the
//! world Frame whatever its class, and `TidyOffsets` clears `Ident` and
//! `Domain` on the Frames it retains (fitschan.c:34622-34623, :34560).

use std::path::Path;

fn native_dump(head: &str) -> String {
    // Mirrors the wcsconv harness: read the .head, write NATIVE to a fresh
    // chan at the harness's FitsDigits.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wcsconv/inputs");
    ast_core::tests_support::head_to_native(&dir.join(head), 17)
        .expect("read and write should succeed")
}

#[test]
fn dsb_spectral_world_frame_gets_ident() {
    let dump = native_dump("class-spec-hel.head");
    assert!(
        dump.lines().any(|l| l.starts_with("IDENT_A")),
        "C sets Ident on the world Frame regardless of class \
         (fitschan.c:37886); dump was:\n{dump}"
    );
}
```

`ast_core::tests_support::head_to_native` is a placeholder for whatever the existing harness helper is called.
Before writing this test, find the real one — `crates/ast-core/tests/common/mod.rs` and the `wcsconv_debug` example both already turn a `.head` into a NATIVE dump.
Use that helper and drop the invented path; if only the example has it, drive the assertion through `FitsChan` directly the way `test_fitschan_time.rs` does in Task 8.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_ident 2>&1 | tail -20
```

Expected: FAIL — no `IDENT_A` line in the dump.

- [ ] **Step 3: Add the `DSBSpecFrame` arm**

In `crates/ast-core/src/fitschan/wcs_read.rs`, add an arm to `set_frame_ident` beside the existing `SpecFrame` one, following the same downcast shape:

```rust
    if let Some(f) = frame.as_any_mut().downcast_mut::<DSBSpecFrame>() {
        f.set_ident(ident);
        return;
    }
```

Place it **before** the `SpecFrame` arm if `DSBSpecFrame` is a distinct type that would not be caught by the `SpecFrame` downcast, and confirm the import is in scope.
While there, check the dispatch against the full list of Frame classes the reader can build — a class-by-class downcast chain silently no-ops for anything omitted, which is what caused this bug.
If the list is longer than the six arms present, add the missing ones in the same commit; that is the fix for the *cause*, not just for these two fixtures.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core --test test_fitschan_ident 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Re-measure**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

Delete the `class-hel` and `class-spec` rows from `NATIVE_READ_EXCLUDED` once byte-exact.
An added `IDENT` card shifts every subsequent keyword's sequence suffix in the affected dumps, so a case that was passing and now fails on `_A`/`_B` suffixes means the arm fires for a header where C's does not.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/tests/test_fitschan_ident.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: set Ident on every world-Frame class a read can build (class-hel, class-spec)"
```

#### Sub-fix B — `TidyOffsets` must clear `Ident` and `Domain`

`tidy_offsets` pass 2 (`wcs_read.rs:625-658`) removes redundant offset Frames in reverse index order when both an absolute and an offset Frame are present, then re-resolves the current index.
C does one more thing: it clears `Ident` and `Domain` on the Frame it keeps (fitschan.c:34622-34623, with the `astClearIdent` call site at :34560).
We leave both set, so we emit `IDENT_A = '" "'` (`tidy-offsets`) and `IDENT_A = 'A'` (`tidy-pole`) where C has already moved on to `NAXES_B`.

Read the C block before writing the fix — in particular *which* Frame gets cleared and under what condition, since pass 2 has more than one exit:

```bash
sed -n '34480,34500p;34550,34570p' reference/starlink-ast/src/fitschan.c
```

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs:625-658` (`tidy_offsets` pass 2)
- Modify: `crates/ast-core/tests/test_wcsconv.rs`
- Test: `crates/ast-core/tests/test_fitschan_ident.rs` (append)

**Interfaces:**
- Consumes: `tidy_offsets` (`wcs_read.rs`), `set_frame_ident` (`:663`).
- Produces:

```rust
// wcs_read.rs — the clearing counterpart to set_frame_ident
/// Clear `Ident` and `Domain` on `frame`, mirroring the pass C
/// `TidyOffsets` makes over the Frames it retains (fitschan.c:34622-34623).
fn clear_frame_ident_and_domain(frame: &mut dyn Frame);
```

- [ ] **Step 1: Write the failing test**

Append to `crates/ast-core/tests/test_fitschan_ident.rs`, using the same dump helper Step 1 of sub-fix A settled on:

```rust
#[test]
fn tidy_offsets_clears_ident_on_the_retained_frame() {
    let dump = native_dump("tidy-offsets.head");
    assert!(
        !dump.lines().any(|l| l.starts_with("IDENT_A")),
        "C TidyOffsets clears Ident on the Frames it keeps \
         (fitschan.c:34622-34623); dump was:\n{dump}"
    );
}
```

Confirm the input filename against the manifest first:

```bash
grep -E 'tidy-offsets|tidy-pole' \
  crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_ident tidy_offsets 2>&1 | tail -20
```

Expected: FAIL — `IDENT_A` present.

- [ ] **Step 3: Add the clearing pass**

Add the helper next to `set_frame_ident` in `crates/ast-core/src/fitschan/wcs_read.rs`, mirroring its downcast dispatch:

```rust
/// Clear `Ident` and `Domain` on `frame`, mirroring the pass C `TidyOffsets`
/// makes over the Frames it retains (fitschan.c:34622-34623).
fn clear_frame_ident_and_domain(frame: &mut dyn Frame) {
    // ... same class dispatch as set_frame_ident, calling clear_ident() and
    // clear_domain() on each concrete type
}
```

and call it from `tidy_offsets` pass 2 at the point C does, on the Frame C clears.
Do not clear unconditionally at the top of the pass: C's call sits inside the branch that actually removes a Frame, so a header where pass 2 finds nothing to remove keeps its `Ident`.
`class-hel` / `class-spec` from sub-fix A rely on that — if both fixtures regress after this commit, the call is outside its branch.

If `clear_ident` / `clear_domain` do not exist on the frame classes, add them beside the existing setters, following the attribute-clearing pattern already used elsewhere in `crates/ast-core/src/frames/`.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p ast-core --test test_fitschan_ident 2>&1 | tail -5
```

Expected: both tests pass — the sub-fix A test must stay green.

- [ ] **Step 5: Re-measure**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

Delete the `tidy-offsets` and `tidy-pole` rows once byte-exact.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/src/frames \
        crates/ast-core/tests/test_fitschan_ident.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: TidyOffsets clears Ident and Domain on retained Frames (tidy-offsets, tidy-pole)"
```

---

### Task 10 (U11): The alt-plane SpecFrame gets the celestial reference position

**Cases:** `fits-pc-alt`, `fits-pc-unit` (2).

Measured first diffs — identical in both:

```
fits-pc-alt   L103  got UWAVE_B = 'm       '   exp REFRA_B = 3.1527470058218849
fits-pc-unit  L103  got UWAVE_B = 'm       '   exp REFRA_B = 3.1527470058218849
```

The reference emits `REFRA_B` / `REFDEC_B` before the preferred-units cards; we skip straight to `UWAVE_B`.
The `_B` suffix is the tell: this is the **second** SpecFrame in the dump — the alt-plane one.
`REFRA_A` / `REFDEC_A` on the primary plane are already correct, which is why the diff appears as late as line 103.

The machinery already exists and is already wired for the primary plane:

- `set_spec_ref_from_celestial` (`wcs_read.rs:186`) takes the celestial fiducial and applies it.
- `set_spec_ref_pos` (`wcs_read.rs:342`) handles `SpecFrame` and `DSBSpecFrame` and recurses through `CmpFrame`.
- The alt-plane branch at `wcs_read.rs:467-470` already computes `alt_ref_pos` from the alt plane's own celestial decode.

So the fix is wiring, not new capability: `alt_ref_pos` is computed and then not carried onto the alt plane's spectral frame.
Confirm that reading before changing anything — if `alt_ref_pos` is in fact consumed and the values are merely wrong, this is a different fix and the task needs re-diagnosing:

```bash
grep -n 'alt_ref_pos' crates/ast-core/src/fitschan/wcs_read.rs
```

C reaches this through `MakeFitsFrameSet` / `WcsMapFrm` per alt-plane version letter, applying the same reference position to each plane's spectral frame.
There is a relevant changelog entry near it (fitschan.c:1348-1352): C temporarily raises `Digits` to 20 when formatting `reflon`/`reflat` into the SpecFrame, because the default 7 digits lost precision.
The port passes `f64` values directly rather than round-tripping through a formatted string, so it has no equivalent step — but the expected value `3.1527470058218849` carries 17 significant digits, so any formatting round-trip introduced here would break it.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs:440-500` (the alt-plane branch)
- Modify: `crates/ast-core/tests/test_wcsconv.rs`
- Test: `crates/ast-core/tests/test_fitschan_ident.rs` (append; it is the read-path dump test file by then) or a new `test_fitschan_specref.rs`

**Interfaces:**
- Consumes: `set_spec_ref_from_celestial` (`wcs_read.rs:186`), `set_spec_ref_pos` (`:342`), `alt_ref_pos` (`:467`).
- Produces: no new API — an existing helper called on the alt plane.

- [ ] **Step 1: Reproduce the diff and locate the alt-plane frame**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv fits_pc_alt_native 2>&1 \
  | grep -E 'REFRA|REFDEC|UWAVE'
grep -nE 'REFRA|REFDEC|BEGAST' \
  crates/ast-core/tests/fixtures/wcsconv/expected/fits-pc-alt.native | head -20
```

The second command shows which `BEGAST_*` block owns `REFRA_B`, confirming it is the alt plane's SpecFrame and not a second spectral axis on the primary.

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn alt_plane_spec_frame_gets_the_celestial_reference_position() {
    let dump = native_dump("fits-pc-alt.head");
    assert!(
        dump.lines().any(|l| l.starts_with("REFRA_B")),
        "the alt-plane SpecFrame needs the celestial fiducial as its \
         reference position, as the primary plane's already does; dump was:\n{dump}"
    );
}
```

- [ ] **Step 3: Run the test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_ident alt_plane 2>&1 | tail -20
```

Expected: FAIL — no `REFRA_B` line.

- [ ] **Step 4: Carry `alt_ref_pos` onto the alt plane's spectral frame**

In the alt-plane branch of `crates/ast-core/src/fitschan/wcs_read.rs` (around lines 467-470), call the existing helper on the alt-plane frame, at the point the primary plane calls it (step 9 of `decode_wcs_inner`, line 114).
Reuse `set_spec_ref_pos` (`:342`) — it already handles `SpecFrame`, `DSBSpecFrame` and `CmpFrame` recursion — rather than adding a parallel path.

Pass the `f64` radian values straight through.
Do not format-and-reparse: C only does that because its `astSetC`-style path forced a string round-trip (fitschan.c:1348-1352), and the expected `3.1527470058218849` needs all 17 digits.

- [ ] **Step 5: Run the test to verify it passes**

```bash
cargo test -p ast-core --test test_fitschan_ident 2>&1 | tail -5
```

Expected: PASS, with the earlier tests in the file still green.

- [ ] **Step 6: Re-measure and delete the entries**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

Delete the `fits-pc-alt` and `fits-pc-unit` rows once byte-exact.
Both should clear together — they share the diff exactly.
If only one clears, the remaining case has a second diff further down; update its reason string to the new measured first diff and note it in the commit message.

- [ ] **Step 7: Run the read-path regression breadth**

```bash
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
cargo test -p ast-core --test test_transform_oracle head_sections 2>&1 | tail -5
cargo test -p ast-core --test test_native_roundtrip_all 2>&1 | tail -3
```

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/tests/test_fitschan_ident.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: carry the celestial reference position onto the alt-plane SpecFrame (fits-pc-alt, fits-pc-unit)"
```

---

## Tier 3 — the real port work

Three tasks.
Task 11 (U7) must land after Task 4 (U1), because both change where cards sit in the list during a read.
Tasks 12 and 13 are independent of everything and of each other.

### Task 11 (U7): ASTWARN cards are inserted in front of the current card

**Cases:** `awav-a2f`, `freq-f2w`, `wave-w2f`, `projp`, `projp-nopv0`, `projp-pv0only`, `unknown-cel`, `unknown-cel2` (8).

Measured first diffs:

```
awav-a2f       L51   got NAXIS1  = 1024   exp ASTWARN = '        '
freq-f2w       L51   got NAXIS1  = 1024   exp ASTWARN = '        '
wave-w2f       L51   got NAXIS1  = 1024   exp ASTWARN = '        '
projp          L133  got NAXIS2  = 512    exp ASTWARN = '        '
projp-nopv0    L133  got NAXIS2  = 512    exp ASTWARN = '        '
projp-pv0only  L133  got NAXIS2  = 512    exp ASTWARN = '        '
unknown-cel    L133  got NAXIS2  = 100    exp ASTWARN = '        '
unknown-cel2   L133  got NAXIS2  = 100    exp ASTWARN = '        '
```

The card *text* is already right — `wrap_warning_text` (`card.rs:102-123`) is a faithful port of C's 60-character word-wrap loop, and the blank-`ASTWARN` framing is already there.
Only the position is wrong: we append, C inserts.

**The C mechanism, and where the position comes from.**

`Warn` (fitschan.c:35120-35297) states it in its own description: the text is "stored in the FitsChan as a series of ASTWARN cards, **in front of the current card**".
Every card is written with `astSetFitsS( this, "ASTWARN", buff, NULL, 0 )` — `overwrite = 0`.
That reaches `InsCard` via `MAKE_FSET` (fitschan.c:16022-16077), and `InsCard`'s own doc spells out the semantics of `overwrite = 0`: "the new card is inserted in front of the current card and the current card is left unchanged".

So the block lands wherever the current card happens to be when the warning fires, and the question that decides this task is *which card that is*.
Two C functions differ, and the difference is the whole answer:

- `GetValue` (fitschan.c:18344-18396) ends with `astSetCard( this, icard )` — "Reinstate the original current card index." Every marked WCS-keyword read goes through `GetValue`, so none of them move the pointer.
- `astGetFits<X>` does **not** restore it. `MAKE_FGET` (fitschan.c:15454-15538) calls `SearchCard` (fitschan.c:27258-27334), whose doc says it "makes it the current card", and the `MAKE_FGET` notes say "If the requested card is found, it becomes the current card, otherwise the current card is left pointing at the 'end-of-file'."

There is exactly one `astGetFits<X>` call on the read path, and it is a loop.
`DistortMaps` (fitschan.c:8801), called from `WcsIntWorld` (fitschan.c:37685) at `:37627`, opens with:

```c
      for( i = 0; i < naxes; i++ ){
         if( !astGetFitsF( this, FormatKey( "NAXIS", i + 1, -1, ' ', status ),
                           dim + i ) ) dim[ i ] = AST__BAD;
      }
```

`naxes` axes read in ascending order, each leaving the pointer on its own card, so on exit the current card is `NAXIS<naxes>`.
That single loop predicts every measured position:

| Cases | `NAXIS` | Current card on exit | Fixture block sits before |
|---|---|---|---|
| `awav-a2f`, `freq-f2w`, `wave-w2f` | 1 | `NAXIS1` | `NAXIS1` |
| `projp`, `projp-nopv0`, `projp-pv0only`, `unknown-cel`, `unknown-cel2` | 2 | `NAXIS2` | `NAXIS2` |

Verified directly against the reference C by instrumenting `Warn` to print `icard`, `ncard` and the name of the card at `icard`, rebuilding `libast` and `wcsconverter`, and running all 8 cases:

```
awav-a2f       icard=4 ncard=12 curname=NAXIS1 method=astRead
freq-f2w       icard=4 ncard=12 curname=NAXIS1 method=astRead
wave-w2f       icard=4 ncard=12 curname=NAXIS1 method=astRead
projp          icard=2 ncard=19 curname=NAXIS2 method=astRead
projp          icard=6 ncard=23 curname=NAXIS2 method=astRead   (second warning)
projp-nopv0    icard=2 ncard=18 curname=NAXIS2 method=astRead
projp-pv0only  icard=2 ncard=18 curname=NAXIS2 method=astRead
unknown-cel    icard=2 ncard=11 curname=NAXIS2 method=astRead
unknown-cel2   icard=2 ncard=11 curname=NAXIS2 method=astRead
```

The instrumentation has been reverted and `libast` / `wcsconverter` rebuilt clean; the numbers above are the record of it.

`projp`'s two warnings are the useful part.
The second reports `icard=6` where the first reported `icard=2`, and `ncard` rose 19 → 23.
`InsCard` inserted 4 cards in front of card 2 and left the *current card* unchanged — meaning the pointer followed its card down the list, from index 2 to index 6.
So consecutive warnings chain into a growing run of blocks with no special-casing, which is exactly the fixture shape: two 4-card blocks back to back, then `NAXIS2`.

**Position is necessary but not sufficient: six of the eight warnings do not exist yet.**

The spec frames cause 7 as purely a positioning bug.
Measurement says it is positioning *plus* two missing warning texts.
`grep -n 'not used by\|is not legal' crates/ast-core/src/` returns nothing: neither C's `badpv` text nor its `badctype` text is anywhere in the port.
The only warning the port generates is the `badcel` text at `wcs_celestial.rs:259-269`, and that one is already byte-correct — which is why `unknown-cel`/`unknown-cel2` carry the exclusion reason "card insertion position not yet reproduced" while the other six read "warning card".

So the eight cases split by what they need:

| Cases | Needs |
|---|---|
| `unknown-cel`, `unknown-cel2` | position only — text already correct |
| `projp`, `projp-nopv0`, `projp-pv0only` | position + the `badpv` text (fitschan.c:36095-36131) |
| `awav-a2f`, `freq-f2w`, `wave-w2f` | position + the `badctype` text (fitschan.c:23807-23815) |

**Four sub-fixes, four commits.**

| Sub-fix | Change | Lands |
|---|---|---|
| A | `push_astwarn` inserts at the position; C's full dedup probe | — |
| B | `DistortMaps`'s `NAXIS<i>` read moves the card cursor, as C's does | `unknown-cel`, `unknown-cel2` |
| C | port the `badpv` warning from `WcsCelestial` | `projp`, `projp-nopv0`, `projp-pv0only` |
| D | port the `badctype` warning from `NonLinSpecWcs`, with a warnings channel on the spectral decoder | `awav-a2f`, `freq-f2w`, `wave-w2f` |

A and B must land in that order and together: A alone changes nothing observable (the position is still 0 or end-of-list at warning time, so the block moves but not to the right place), and B alone would move the cursor with nothing reading it.
C and D each depend on A+B — a new warning with no position fix would land in the wrong place — but are independent of each other.

Do not try to shortcut B by hard-coding "insert before `NAXIS<naxes>`".
The position is a property of the card cursor that C's read path leaves behind, and a later unit that adds another unmarked `astGetFits` read would silently diverge from a hard-coded rule while still matching the cursor rule.

#### Sub-fix A: `push_astwarn` inserts at the position, with C's dedup probe

C's dedup (fitschan.c:35190-35248) is not "is this line anywhere in the chan".
It is: rewind; for the **first** wrapped line, search the whole chan for an `ASTWARN` card holding it; for **each subsequent** line, require that the *next* card is an `ASTWARN` holding it.
A partial match breaks out with `exists = 0`.
The rewind is undone with `astSetCard( this, icard )` before any writing happens (fitschan.c:35250), so the probe never leaves the cursor moved.

Our current dedup checks only the first line and calls it a match, which is looser than C.
`projp` is the case that shows why C needs the adjacency requirement: two blocks whose *first* lines differ but whose *second* lines are both `'projections.'`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/card_list.rs:60-96` (`push_astwarn`)
- Test: `crates/ast-core/src/fitschan/card_list.rs` (new `mod tests` cases; the file has none yet, so add the module at the end)

**Interfaces:**
- Consumes: `wrap_warning_text` (`card.rs:102`), `FitsCard`, `FitsValue`, `CardFlags`, `self.position`.
- Produces: no signature change — `push_astwarn(&mut self, text: &str)` keeps its shape. Behavior changes: cards are spliced in at `self.position` (which then advances past them), and dedup requires a full adjacent block.

- [ ] **Step 1: Write the failing tests**

Append to `crates/ast-core/src/fitschan/card_list.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn card(keyword: &str, value: i64) -> FitsCard {
        FitsCard {
            keyword: keyword.to_string(),
            value: FitsValue::Int(value),
            comment: None,
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        }
    }

    fn keywords(list: &CardList) -> Vec<String> {
        list.iter().map(|c| c.keyword.clone()).collect()
    }

    fn astwarn_values(list: &CardList) -> Vec<String> {
        list.iter()
            .filter(|c| c.keyword == "ASTWARN")
            .map(|c| match &c.value {
                FitsValue::String(s) => s.clone(),
                other => panic!("ASTWARN card holds {other:?}"),
            })
            .collect()
    }

    /// C `Warn` writes each card with `astSetFitsS( ..., overwrite = 0 )`,
    /// which `InsCard` (fitschan.c:16022-16186) inserts in front of the
    /// current card, leaving that card current. So the block lands before the
    /// card at the current position, and the position follows its card down.
    #[test]
    fn push_astwarn_inserts_before_the_current_card() {
        let mut list = CardList::new();
        list.push(card("NAXIS1", 512));
        list.push(card("NAXIS2", 512));
        list.set_position(1); // current card = NAXIS2

        list.push_astwarn("short warning");

        assert_eq!(
            keywords(&list),
            vec!["NAXIS1", "ASTWARN", "ASTWARN", "ASTWARN", "NAXIS2"]
        );
        // The current card is still NAXIS2, now at index 4.
        assert_eq!(list.position(), 4);
    }

    /// Consecutive warnings chain into a growing run of blocks in front of the
    /// same card, because each insertion carries the position with it. C
    /// `projp` shows the shape: two blocks, then NAXIS2.
    #[test]
    fn consecutive_push_astwarn_calls_chain_before_the_same_card() {
        let mut list = CardList::new();
        list.push(card("NAXIS1", 512));
        list.push(card("NAXIS2", 512));
        list.set_position(1);

        list.push_astwarn("first warning");
        list.push_astwarn("second warning");

        assert_eq!(
            astwarn_values(&list),
            vec![" ", "first warning", " ", " ", "second warning", " "]
        );
        assert_eq!(keywords(&list).last().unwrap(), "NAXIS2");
        assert_eq!(list.position(), 7);
    }

    /// C dedups on the whole block: the first line is searched for anywhere in
    /// the chan, then every later line must be on the *next* card
    /// (fitschan.c:35218-35248). An identical block is not repeated.
    #[test]
    fn push_astwarn_skips_an_identical_existing_block() {
        let text = "The FITS keyword name ('sys.') contains an illegal character ('.').";
        let mut list = CardList::new();
        list.push(card("NAXIS1", 512));
        list.set_position(0);
        list.push_astwarn(text);
        let after_first = astwarn_values(&list);

        list.push_astwarn(text);

        assert_eq!(astwarn_values(&list), after_first);
    }

    /// Two blocks whose first lines differ but whose *second* lines are
    /// identical must both be stored. The adjacency requirement is what makes
    /// this work: matching only the shared trailing line is not a match.
    /// This is the `projp` case — two `badpv` warnings that both wrap to a
    /// second line of "projections.".
    #[test]
    fn push_astwarn_stores_blocks_that_share_a_trailing_line() {
        let first = "Projection parameter PV2_1 found, but is not used by -TAN projections.";
        let second = "Projection parameter PV2_2 found, but is not used by -TAN projections.";
        let mut list = CardList::new();
        list.push(card("NAXIS2", 512));
        list.set_position(0);

        list.push_astwarn(first);
        list.push_astwarn(second);

        assert_eq!(
            astwarn_values(&list),
            vec![
                " ",
                "Projection parameter PV2_1 found, but is not used by -TAN ",
                "projections.",
                " ",
                " ",
                "Projection parameter PV2_2 found, but is not used by -TAN ",
                "projections.",
                " ",
            ]
        );
    }

    /// The dedup probe rewinds to search but must leave the position where it
    /// found it, mirroring the `astSetCard( this, icard )` at fitschan.c:35250.
    #[test]
    fn push_astwarn_dedup_probe_does_not_move_the_position() {
        let text = "a repeated warning";
        let mut list = CardList::new();
        list.push(card("NAXIS1", 512));
        list.push(card("NAXIS2", 512));
        list.set_position(0);
        list.push_astwarn(text);
        let pos = list.position();

        // Second call dedups and returns without writing.
        list.push_astwarn(text);

        assert_eq!(list.position(), pos);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --lib fitschan::card_list 2>&1 | tail -20
```

Expected: `push_astwarn_inserts_before_the_current_card`,
`consecutive_push_astwarn_calls_chain_before_the_same_card` and
`push_astwarn_stores_blocks_that_share_a_trailing_line` FAIL (the current code appends, and dedups on the first line only).
The two dedup-position tests may already pass.

- [ ] **Step 3: Rewrite `push_astwarn`**

Replace the body at `crates/ast-core/src/fitschan/card_list.rs:60-96` with:

```rust
    /// Insert a block of `ASTWARN` comment cards holding `text` in front of the
    /// current card, mirroring C `Warn` (fitschan.c:35120-35297): a leading
    /// blank `ASTWARN`, one card per wrapped ~60-character line, then a
    /// trailing blank `ASTWARN`. Each card is written with C's
    /// `astSetFitsS( ..., overwrite = 0 )`, which `InsCard` inserts before the
    /// current card and leaves that card current — so the position advances
    /// past the block and consecutive warnings chain in front of the same card.
    ///
    /// The block is not added if an identical block of adjacent `ASTWARN`
    /// cards already exists: the first line is searched for anywhere in the
    /// list, and every later line must sit on the immediately following card
    /// (fitschan.c:35218-35248).
    pub(crate) fn push_astwarn(&mut self, text: &str) {
        let lines = super::card::wrap_warning_text(text);
        if lines.is_empty() || self.astwarn_block_exists(&lines) {
            return;
        }

        let astwarn = |s: &str| FitsCard {
            keyword: "ASTWARN".to_string(),
            value: FitsValue::String(s.to_string()),
            comment: None,
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };

        let mut block = Vec::with_capacity(lines.len() + 2);
        block.push(astwarn(" "));
        for line in &lines {
            block.push(astwarn(line));
        }
        block.push(astwarn(" "));

        let n = block.len();
        let at = self.position.min(self.cards.len());
        self.cards.splice(at..at, block);
        // The card that was current stays current; it has moved down by `n`.
        self.position = at + n;
    }

    /// True when the list already holds `lines` as a run of adjacent `ASTWARN`
    /// cards. Mirrors C `Warn`'s dedup probe (fitschan.c:35190-35248): search
    /// the whole list for the first line, then require each later line on the
    /// next card. Leaves the current position unchanged, as C does by
    /// reinstating the saved card index (fitschan.c:35250).
    fn astwarn_block_exists(&self, lines: &[String]) -> bool {
        let is_astwarn_holding = |card: &FitsCard, want: &str| {
            card.keyword.eq_ignore_ascii_case("ASTWARN")
                && matches!(&card.value, FitsValue::String(s) if s == want)
        };
        let first = &lines[0];
        for start in 0..self.cards.len() {
            if !is_astwarn_holding(&self.cards[start], first) {
                continue;
            }
            let all_present = lines[1..].iter().enumerate().all(|(k, line)| {
                self.cards
                    .get(start + 1 + k)
                    .is_some_and(|c| is_astwarn_holding(c, line))
            });
            if all_present {
                return true;
            }
        }
        false
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p ast-core --lib fitschan::card_list 2>&1 | tail -10
```

Expected: all five PASS.

- [ ] **Step 5: Check the two existing `push_astwarn` callers still behave**

`put_fits` (`mod.rs:155-163`), `put_cards` (`mod.rs:176-190`) and
`CardList::from_text` (`card_list.rs:553-562`) all call `push_astwarn` for an
illegal keyword name, and each does so *before* pushing the offending card, so
the block must still precede it. In `from_text` and `put_cards` the cards are
appended with `push`, which ignores `position`, so a `push_astwarn` that
splices at `position` would land the block at the wrong place.

```bash
cargo test -p ast-core --test test_wcsconv car6 2>&1 | tail -5
cargo test -p ast-core --lib fitschan 2>&1 | tail -5
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
```

If any illegal-keyword-name case regresses, the fix is in the *caller*, not in
`push_astwarn`: those paths append rather than insert, so they must set
`position` to the end of the list before warning (`self.position =
self.cards.len()`), which is where C's cursor is while `astPutFits` appends.
Do not add a "append instead when position is 0" branch to `push_astwarn` — C
has one insertion rule and the position carries the difference.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/card_list.rs crates/ast-core/src/fitschan/mod.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: insert ASTWARN cards in front of the current card, with C's block dedup"
```

#### Sub-fix B: the `NAXIS<i>` read moves the card cursor

C's read path reads `NAXIS1`…`NAXIS<naxes>` with `astGetFitsF` — unmarked, and
crucially *not* through `GetValue`, so the cursor is left on the last one found.
Our port reads the same keywords at `wcs_keywords.rs:209`:

```rust
        // NAXISn is structural (not a WCS keyword); never consumed.
        store.dims[i] = cards.get_float(&format!("NAXIS{axis_num}"));
```

`get_float` (`card_list.rs:188-193`) delegates to `find` (`:122-126`), which is
`&self` and never touches `position`. That is the divergence.

Two things differ from C beyond the cursor, and both matter for where the
cursor ends up:

1. C reads `NAXIS<i>` inside `DistortMaps`, i.e. during `WcsIntWorld`, *after*
   `WcsToStore` has finished. We read it in `extract_wcs_keywords`, i.e. during
   store population. The order relative to the other WCS reads is different,
   but it does not change the answer: every other read is marked and goes
   through the equivalent of `GetValue`, which restores the cursor. The
   `NAXIS<i>` loop is the only cursor-moving read on either side, so wherever
   it sits in the sequence, it is what the cursor reflects when the warning
   fires.
2. C searches with `SearchCard`, which checks the *next* card first and only
   then rewinds and scans from the front. Our `find` always scans from the
   front. For the `NAXIS<i>` loop the two agree: the cards are in ascending
   order in every fixture header, so "next card" and "first match from the
   front" are the same card. Do not port `SearchCard`'s next-card shortcut as
   part of this task — it is a separate behavior with its own callers, and
   nothing here needs it.

The change: read `NAXIS<i>` through a cursor-moving getter, and thread that
cursor to the `push_astwarn` calls. The reads happen on the `trans` list
(`spec_trans` output) but the warnings are pushed onto `self.cards`, so the
position has to be carried across.

**Files:**
- Modify: `crates/ast-core/src/fitschan/card_list.rs` (add a cursor-moving float getter next to `find_next`, `:152-161`)
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs:205-210` (read `NAXIS<i>` through it) and its `extract_wcs_keywords_with_alts` signature
- Modify: `crates/ast-core/src/fitschan/mod.rs:676-769` (set the warning position before pushing, in all six foreign-encoding arms)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (delete eight `NATIVE_READ_EXCLUDED` entries)
- Test: `crates/ast-core/tests/test_fitschan_astwarn.rs` (new)

**Interfaces:**
- Consumes: `CardList::find_next` (`card_list.rs:152`) as the model, `CardList::position` / `set_position`, `push_astwarn` from sub-fix A.
- Produces:

```rust
// card_list.rs — a cursor-moving float getter
/// Search forward from the current position for `keyword` (wrapping to the
/// start of the list) and return its floating-point value, leaving the found
/// card current. Mirrors C `astGetFitsF`, which finds via `SearchCard` and —
/// unlike `GetValue` — does not reinstate the original card index
/// (fitschan.c:15454-15538). Callers that must not disturb the cursor use
/// [`get_float`].
pub(crate) fn get_float_at_cursor(&mut self, keyword: &str) -> Option<f64>;
```

```rust
// wcs_keywords.rs — the extractor takes the list mutably so the NAXIS reads
// can move the cursor, and reports where it ended up.
pub(crate) fn extract_wcs_keywords_with_alts(
    cards: &mut CardList,
) -> AstResult<(FitsStore, Vec<(char, FitsStore)>)>;
```

The `&mut CardList` is the awkward part of this sub-fix and the reason it is
its own commit. `extract_wcs_keywords_with_alts` is currently `&CardList` and
is called from all six foreign arms plus its own unit tests. Take the `&mut`
rather than returning a position out of band: the cursor is state on the list,
and threading it separately would let the two drift.

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_fitschan_astwarn.rs`:

```rust
//! ASTWARN card placement on the FITS read path.
//!
//! C `Warn` (fitschan.c:35120) inserts its cards in front of the current
//! card, and the current card during a read is the last `NAXIS<i>` keyword
//! `DistortMaps` looked up (fitschan.c:8903-8907) — the only read on that
//! path that goes through `astGetFits<X>` rather than `GetValue`, and so the
//! only one that leaves the card cursor moved.

use ast_core::channels::Channel;
use ast_core::fitschan::FitsChan;

fn chan_from(header: &str) -> FitsChan {
    let mut fc = FitsChan::new();
    fc.put_cards(header).expect("header parses");
    fc.rewind();
    fc
}

/// Collect the keyword of every card in order.
fn keywords(fc: &mut FitsChan) -> Vec<String> {
    let mut out = Vec::new();
    fc.rewind();
    while let Some(card) = fc.next_card() {
        out.push(card[..8].trim().to_string());
    }
    out
}

/// A 2-axis header with an unrecognised celestial system: the port already
/// generates the `badcel` warning text (`wcs_celestial.rs:259-269`) for this,
/// so it isolates card *position* from text generation. C parks its cursor on
/// `NAXIS2` here, so the block lands between `NAXIS1` and `NAXIS2`.
///
/// Same shape as the `unknown-cel` fixture header
/// (`tests/fixtures/wcsconv/inputs/unknown-cel.head`).
const TWO_AXIS_BADCEL: &str = "\
NAXIS1  =                  100 / length of data axis 1
NAXIS2  =                  100 / length of data axis 2
CTYPE1  = 'XLON-TAN'           / Unknown longitude axis
CTYPE2  = 'XLAT-TAN'           / Unknown latitude axis
CRVAL1  =                180.0
CRVAL2  =                 45.0
CRPIX1  =                 50.0
CRPIX2  =                 50.0
CDELT1  =                 -0.1
CDELT2  =                  0.1
";

#[test]
fn astwarn_block_precedes_the_last_naxis_card_for_two_axes() {
    let mut fc = chan_from(TWO_AXIS_BADCEL);
    let _ = fc.read().expect("read succeeds");
    let kws = keywords(&mut fc);

    let first_warn = kws
        .iter()
        .position(|k| k == "ASTWARN")
        .expect("a badcel ASTWARN block is expected");
    let naxis1 = kws.iter().position(|k| k == "NAXIS1").expect("NAXIS1");
    let naxis2 = kws.iter().position(|k| k == "NAXIS2").expect("NAXIS2");
    assert!(
        naxis1 < first_warn && first_warn < naxis2,
        "ASTWARN block must sit between NAXIS1 and NAXIS2, got {kws:?}"
    );
}
```

The two helpers (`FitsChan::rewind`, `FitsChan::next_card`) are the public
card-walk API; confirm their exact names against `mod.rs`'s public surface
before running, and adjust the helper body if they differ. If no public
`next_card` exists, walk with `find_fits("%f", true)` or read the whole header
through the public write-to-string path instead — do not reach into
`self.cards` from an integration test.

This test uses `badcel` deliberately. It is the one warning text the port
already produces correctly, so a failure here is unambiguously about position.
Sub-fixes C and D add the 1-axis (`NAXIS1`-parked) position case as part of
introducing their own warning texts.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_astwarn 2>&1 | tail -20
```

Expected: FAIL, reporting the ASTWARN block after `NAXIS2` (the append
position sub-fix A leaves it at when nothing has moved the cursor).

- [ ] **Step 3: Add the cursor-moving getter**

In `crates/ast-core/src/fitschan/card_list.rs`, next to `find_next` (`:152`):

```rust
    /// Search forward from the current position for `keyword`, wrapping to the
    /// start of the list, and return its floating-point value with the found
    /// card left current.
    ///
    /// Mirrors C `astGetFitsF`, which finds the card via `SearchCard`
    /// (fitschan.c:27258) and — unlike `GetValue` (fitschan.c:18344), which
    /// reinstates the entry index — leaves it as the current card
    /// (fitschan.c:15454-15538). When no card is found the position is set to
    /// end-of-list, as C leaves it "pointing at the end-of-file".
    /// Callers that must not disturb the cursor use [`get_float`].
    pub(crate) fn get_float_at_cursor(&mut self, keyword: &str) -> Option<f64> {
        let found = self
            .cards
            .iter()
            .skip(self.position)
            .position(|c| c.keyword.eq_ignore_ascii_case(keyword))
            .map(|i| self.position + i)
            .or_else(|| {
                self.cards
                    .iter()
                    .position(|c| c.keyword.eq_ignore_ascii_case(keyword))
            });
        match found {
            Some(idx) => {
                self.position = idx;
                match &self.cards[idx].value {
                    FitsValue::Float(v) => Some(*v),
                    FitsValue::Int(v) => Some(*v as f64),
                    _ => None,
                }
            }
            None => {
                self.position = self.cards.len();
                None
            }
        }
    }
```

- [ ] **Step 4: Read `NAXIS<i>` through it**

In `crates/ast-core/src/fitschan/wcs_keywords.rs`, change the signature of
`extract_wcs_keywords_with_alts` (and the `extract_wcs_keywords` it wraps, if
that is a separate function) from `cards: &CardList` to
`cards: &mut CardList`, then replace line 209:

```rust
        // NAXISn is structural (not a WCS keyword) and never consumed, but C
        // reads it with `astGetFitsF` inside `DistortMaps`
        // (fitschan.c:8903-8907) rather than through `GetValue`, so the found
        // card is left current. That cursor is where `Warn` inserts its
        // ASTWARN cards.
        store.dims[i] = cards.get_float_at_cursor(&format!("NAXIS{axis_num}"));
```

Every other read in the function stays on the `&self` getters, which do not
move the cursor — matching C, where they all route through `GetValue`.

- [ ] **Step 5: Carry the cursor to the warning push**

In `crates/ast-core/src/fitschan/mod.rs`, each of the six foreign-encoding
arms reads keywords off `trans` but pushes warnings onto `self.cards`. Make
`trans` mutable, and set the warning position from it before pushing. For the
`FitsWcs` arm (`:676-699`):

```rust
                let mut trans = spec_trans::spec_trans(&self.cards, Encoding::FitsWcs);
                let (mut primary, mut alts) =
                    wcs_keywords::extract_wcs_keywords_with_alts(&mut trans)?;
                primary.sip_replace = self.sip_replace;
                for (_, st) in &mut alts {
                    st.sip_replace = self.sip_replace;
                }
                let (obj, warnings) = wcs_read::decode_wcs_with_alts_inner(&primary, &alts, false)?;
                // C reads and warns on one FitsChan; we read from the
                // SpecTrans copy and warn on the original, so carry the card
                // cursor across. The cursor is on the last NAXIS<i> card the
                // extractor looked up, which is where C's `Warn` inserts.
                self.set_warn_position_from(&trans);
                for w in warnings {
                    self.cards.push_astwarn(&w);
                }
                self.finish_read_consumption(&trans)?;
```

and add the helper to `impl FitsChan`:

```rust
    /// Align this chan's card cursor with `trans`'s, matched by the keyword of
    /// the card `trans` has current.
    ///
    /// C reads keywords and stores warnings on the same FitsChan, so its card
    /// cursor needs no transfer. We read from the `SpecTrans` copy, which may
    /// have deleted or added cards, so index equality does not hold and the
    /// cursor is re-found by keyword. When `trans` is at end-of-list, or its
    /// current keyword is absent here, the cursor goes to the end of the list.
    fn set_warn_position_from(&mut self, trans: &CardList) {
        let keyword = trans
            .iter()
            .nth(trans.position())
            .map(|c| c.keyword.clone());
        let target = keyword
            .and_then(|kw| self.cards.iter().position(|c| c.keyword == kw))
            .unwrap_or_else(|| self.cards.len());
        self.cards.set_position(target);
    }
```

Apply the same three-line change (`&mut trans`, `set_warn_position_from`) to
the other five arms: `FitsPc`, `FitsIraf`, `FitsAips`/`FitsAipsPP`, `FitsClass`
and `Dss`. The `Dss` arm's tail differs (it does its own `take_consumed_names`
+ `mark_card` + `fix_used`) — leave that tail alone and only insert the
position call before its `push_astwarn` loop.

- [ ] **Step 6: Run the new test to verify it passes**

```bash
cargo test -p ast-core --test test_fitschan_astwarn 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 7: Re-measure the two position-only cases**

```bash
for c in unknown-cel unknown-cel2; do
  n=$(echo "$c" | tr - _)
  printf '%-16s ' "$c"
  WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv "${n}_native" 2>&1 \
    | grep -E 'differs|test result' | head -2 | tr '\n' ' '
  echo
done
```

Expected: both report `test result: ok`.
If a case now differs at a *later* line, that is a second cause behind the
first diff — record it and re-check the table in "Measured first diffs at plan
time" per the re-measure protocol; do not remove its exclusion entry.

The other six cases still fail here, and their first diff must now have
*changed*: instead of `NAXIS<n>` where `ASTWARN` was expected, they report a
missing `ASTWARN` block whose text does not exist yet. That change is the
evidence that position is fixed and text is what remains.

- [ ] **Step 8: Remove the two exclusion entries**

Delete from `NATIVE_READ_EXCLUDED` in `crates/ast-core/tests/test_wcsconv.rs`
the `unknown-cel_native` and `unknown-cel2_native` entries.
Leave the six `ASTWARN: warning card` entries — sub-fixes C and D remove them.

- [ ] **Step 9: Verify the two-sided gate and the write-path corpora**

The changed code is on the shared read path and touches card ordering, so run
every corpus that consumes it.

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
cargo test -p ast-core --test test_native_roundtrip_all 2>&1 | tail -3
cargo test -p ast-core --test test_transform_oracle head_sections 2>&1 | tail -5
cargo test -p ast-core 2>&1 | grep -E 'test result' | tail -20
```

Expected: `test_wcsconv` stays at 192 passed with no failures, and the
`WCSCONV_NO_EXCLUDE=1` run shows 2 fewer failures than the 45 baseline.
`HEAD_ORACLE_EXCLUDED` must stay empty.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/card_list.rs \
        crates/ast-core/src/fitschan/wcs_keywords.rs \
        crates/ast-core/src/fitschan/mod.rs \
        crates/ast-core/tests/test_fitschan_astwarn.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: NAXISn read leaves the card cursor moved, placing ASTWARN cards as C does (unknown-cel, unknown-cel2)"
```

#### Sub-fix C: the `badpv` warning — a PV parameter the projection cannot use

**Cases:** `projp`, `projp-nopv0`, `projp-pv0only`.

C `WcsCelestial` (fitschan.c:36130-36131, in the `else` branch that builds a
`WcsMap`) walks every supplied `PVi_m` and warns for each one whose `m` exceeds
what the projection accepts on that axis:

```c
         np = GetMaxJM( &(store->pv), s, status );
         map2 = (AstMapping *) astWcsMap( naxes, latprj, axlon + 1,
                                          axlat + 1, "", status );
         mxpar_lat = astGetPVMax( map2, axlat );
         mxpar_lon = astGetPVMax( map2, axlon );
         for( m = 0; m <= np; m++ ){
            pv = GetItem( &(store->pv), axlat, m, s, NULL, method, class, status );
            if( pv != AST__BAD ) {
               if( m <= mxpar_lat ) {
                  astSetPV( map2, axlat, m, pv );
               } else {
                  sprintf( buf, "Projection parameter PV%d_%d found, "
                           "but is not used by %s projections.", axlat + 1,
                           m, astWcsPrjName( astGetWcsType( map2 ) ) );
                  Warn( this, "badpv", buf, method, class, status );
               }
            }
            pv = GetItem( &(store->pv), axlon, m, s, NULL, method, class, status );
            if( pv != AST__BAD ) {
               if( m <= mxpar_lon ) {
                  astSetPV( map2, axlon, m, pv );
               } else {
                  sprintf( buf, "Projection parameter PV%d_%d found, "
                           "but is not used by %s projections.", axlon + 1,
                           m, astWcsPrjName( astGetWcsType( map2 ) ) );
                  Warn( this, "badpv", buf, method, class, status );
               }
            }
         }
```

Three things in that loop the port does not have.

**1. The per-axis parameter limit.**
`astGetPVMax` (wcsmap.c:2152-2217) returns the largest legal `m` for an axis,
read off the allocation size of the parameter array:

```c
   } else if( i == astGetWcsAxis( this, 0 ) ) {
      mxpar = astSizeOf( this->params.p2 )/sizeof( double );
   } else if( i == astGetWcsAxis( this, 1 ) ) {
      mxpar = astSizeOf( this->params.p )/sizeof( double );
   }
   return mxpar - 1;
```

The arrays are allocated from the projection table (wcsmap.c:5862-5863,
`:6059-6060`): `p` gets `prjdata->mxpar + 1` doubles and `p2` gets
`prjdata->mxpar2 + 1`.
`WcsAxis(0)` is the longitude axis and `WcsAxis(1)` the latitude axis, so
`astGetPVMax(lonax) == mxpar2` and `astGetPVMax(latax) == mxpar`.
Both columns live in the `PrjInfo` table (wcsmap.c:684-716), whose `desc` column
the port already mirrors as `WcsProjection::description` (`wcslib.rs:135-172`).
Every projection has `mxpar2 = 4` except `-TPN`, which has
`mxpar2 = WCSLIB_MXPAR`; the `mxpar` column varies.
The lookup is on the *declared* projection type, not the underlying one — C's
`FindPrjData( this->type )` sees `AST__NCP` / `AST__GLS` directly even though
their transform functions are `SIN`'s and `SFL`'s.

**2. `GetMaxJM`, the loop bound.**
`np` is the highest `m` present on *any* axis, so the loop visits every index
once and checks latitude before longitude at each. That ordering is what puts
`PV2_1` before `PV2_2` in the `projp` fixture, and it is why a single ascending
loop is required rather than two per-axis loops.

**3. The warning itself.**
`astWcsPrjName` (wcsmap.c:4375-4417) returns the table's 4-character `ctype`
column — `"-TAN"`, leading dash included. The port's
`WcsProjection::code` (`wcslib.rs:97-131`) returns the 3 characters without it,
so the text needs `format!("-{}", proj.code())`.

The port's current PV loop (`wcs_celestial.rs:136-151`) is two separate
per-axis loops that call `set_pv` and discard the error with `let _ =`:

```rust
    for m in 0..store.pv[lat_axis].len() {
        if let Some(val) = store.pv[lat_axis][m] {
            let _ = wcsmap.set_pv(lat_axis + 1, m, val);
        }
    }
```

`set_pv` (`wcsmap.rs:208-252`) rejects only `m >= MXPAR` (100) — it has no
per-projection limit at all, so out-of-range PVs are silently stored and
`init_prj_prm` is handed parameters the projection does not read.
That divergence is what this sub-fix closes; the warning falls out of it.

**Files:**
- Modify: `crates/ast-core/src/wcslib.rs` (add the two `PrjInfo` limit columns to `WcsProjection`, next to `description` at `:135`)
- Modify: `crates/ast-core/src/mappings/wcsmap.rs` (add `pv_max`; make `set_pv` enforce it)
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:136-151` (single ascending loop, warn on overflow)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (delete three exclusion entries)
- Test: `crates/ast-core/src/wcslib.rs` (limit table), `crates/ast-core/src/mappings/wcsmap.rs` (`pv_max`, `set_pv` rejection), `crates/ast-core/tests/test_fitschan_astwarn.rs` (the warning text)

**Interfaces:**
- Consumes: `WcsProjection` (`wcslib.rs:28`), `WcsMap::set_pv` / `get_pv` / `projection` (`wcsmap.rs:208`, `:264`, `:256`), `FitsStore::pv` (`fits_store.rs`), `CelestialResult::warnings` (`wcs_celestial.rs:97`), `push_astwarn` from sub-fixes A and B.
- Produces:

```rust
// wcslib.rs
impl WcsProjection {
    /// Largest legal `m` in `PVi_m` on the *latitude* axis.
    ///
    /// The `mxpar` column of C's `PrjInfo` table (wcsmap.c:684-716), which
    /// sizes the `params.p` array as `mxpar + 1` doubles (wcsmap.c:5862) and
    /// so fixes the limit `astGetPVMax` reports for that axis
    /// (wcsmap.c:2205-2210).
    pub fn pv_max_lat(&self) -> usize;

    /// Largest legal `m` in `PVi_m` on the *longitude* axis — the `mxpar2`
    /// column of the same table, 4 for every projection except `-TPN`.
    pub fn pv_max_lon(&self) -> usize;
}

// wcsmap.rs
impl WcsMap {
    /// Largest legal projection-parameter index for a 1-based axis, or `None`
    /// when the axis is neither the longitude nor the latitude axis (C returns
    /// -1 there). Mirrors `astGetPVMax` (wcsmap.c:2152).
    pub fn pv_max(&self, axis: usize) -> Option<usize>;
}
```

`set_pv` keeps its signature but now returns `Err(ErrorCode::Wcspa)` when `m`
exceeds `pv_max` for the axis, matching C `SetPV` (wcsmap.c:3884-3890), which
raises `AST__WCSPA` with the projection's ctype and `mxpar` in the message.
That means the caller can no longer discard the result with `let _ =`; the
`Err` is precisely the signal to warn.

- [ ] **Step 1: Write the failing tests**

Add to `crates/ast-core/src/wcslib.rs`'s test module:

```rust
    /// The `mxpar` / `mxpar2` columns of C's `PrjInfo` table
    /// (wcsmap.c:684-716). `mxpar2` is 4 everywhere except `-TPN`.
    #[test]
    fn pv_limits_match_the_c_projection_table() {
        let cases: &[(WcsProjection, usize, usize)] = &[
            (WcsProjection::Azp, 2, 4),
            (WcsProjection::Szp, 3, 4),
            (WcsProjection::Tan, 0, 4),
            (WcsProjection::Sin, 2, 4),
            (WcsProjection::Zpn, 99, 4),
            (WcsProjection::Air, 1, 4),
            (WcsProjection::Cyp, 2, 4),
            (WcsProjection::Cea, 1, 4),
            (WcsProjection::Car, 0, 4),
            (WcsProjection::Sfl, 0, 4),
            (WcsProjection::Cop, 2, 4),
            (WcsProjection::Bon, 1, 4),
            (WcsProjection::Ncp, 2, 4),
            (WcsProjection::Gls, 0, 4),
            (WcsProjection::Hpx, 2, 4),
            (WcsProjection::Xph, 0, 4),
            (WcsProjection::Tpn, 99, 99),
        ];
        for &(proj, lat, lon) in cases {
            assert_eq!(proj.pv_max_lat(), lat, "mxpar for {}", proj.code());
            assert_eq!(proj.pv_max_lon(), lon, "mxpar2 for {}", proj.code());
        }
    }
```

`WCSLIB_MXPAR` is 100 (`vendor/wcslib/proj.h:59`, mirrored at
`wcslib.rs:324`), so the largest legal index is 99.

Add to `crates/ast-core/src/mappings/wcsmap.rs`'s test module:

```rust
    /// `astGetPVMax` reports the longitude axis's limit from `mxpar2` and the
    /// latitude axis's from `mxpar` (wcsmap.c:2205-2210), and `None` for any
    /// other axis (C returns -1).
    #[test]
    fn pv_max_is_per_axis() {
        // 2-axis TAN: lon = axis 1, lat = axis 2.
        let map = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        assert_eq!(map.pv_max(1), Some(4)); // mxpar2
        assert_eq!(map.pv_max(2), Some(0)); // mxpar
        // A third axis is neither.
        let map3 = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
        assert_eq!(map3.pv_max(3), None);
    }

    /// C `SetPV` raises `AST__WCSPA` for an index the projection cannot use
    /// (wcsmap.c:3884-3890). `-TAN` accepts only `PV<lat>_0`.
    #[test]
    fn set_pv_rejects_an_index_the_projection_cannot_use() {
        let mut map = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        assert!(map.set_pv(2, 0, 0.0).is_ok(), "PV2_0 is legal for TAN");
        let err = map.set_pv(2, 1, 0.0).expect_err("PV2_1 is not legal for TAN");
        assert_eq!(err.code(), ErrorCode::Wcspa);
        // The longitude axis accepts up to m = 4.
        assert!(map.set_pv(1, 4, 0.0).is_ok());
        assert!(map.set_pv(1, 5, 0.0).is_err());
    }
```

Add to `crates/ast-core/tests/test_fitschan_astwarn.rs`:

```rust
/// `projp`-shaped: PROJP0/1/2 translate to PV2_0/PV2_1/PV2_2, but `-TAN`
/// accepts only `PV2_0`. C warns once per unusable parameter, latitude axis
/// before longitude at each index, in ascending `m` order.
const TWO_AXIS_BADPV: &str = "\
NAXIS1  =                  512 / length of data axis 1
NAXIS2  =                  512 / length of data axis 2
CTYPE1  = 'RA---TAN'           / Gnomonic projection
CTYPE2  = 'DEC--TAN'           / Gnomonic projection
CRVAL1  =              180.000 / Reference RA (deg)
CRVAL2  =               45.000 / Reference Dec (deg)
CRPIX1  =              256.500 / Reference pixel on axis 1
CRPIX2  =              256.500 / Reference pixel on axis 2
CD1_1   =        -8.3332205E-4 / CD matrix
CD1_2   =                  0.0 / CD matrix
CD2_1   =                  0.0 / CD matrix
CD2_2   =        0.00083332205 / CD matrix
PROJP0  =                  0.0 / Legacy projection parameter 0
PROJP1  =                  0.0 / Legacy projection parameter 1
PROJP2  =                  0.0 / Legacy projection parameter 2
";

/// Collect the value of every ASTWARN card, in order.
fn astwarn_values(fc: &mut FitsChan) -> Vec<String> {
    let mut out = Vec::new();
    fc.rewind();
    while let Some(card) = fc.next_card() {
        if card.starts_with("ASTWARN") {
            // Value is the quoted string starting at column 11.
            let v = card[10..].trim_end();
            out.push(v.trim_matches('\'').to_string());
        }
    }
    out
}

#[test]
fn badpv_warning_names_each_unusable_parameter_in_ascending_order() {
    let mut fc = chan_from(TWO_AXIS_BADPV);
    let _ = fc.read().expect("read succeeds");
    assert_eq!(
        astwarn_values(&mut fc),
        vec![
            "        ",
            "Projection parameter PV2_1 found, but is not used by -TAN ",
            "projections.",
            "        ",
            "        ",
            "Projection parameter PV2_2 found, but is not used by -TAN ",
            "projections.",
            "        ",
        ]
    );
}
```

The expected strings are copied from
`tests/fixtures/wcsconv/expected/projp.native:134-141`, including the trailing
space on the first text line — that is where C's 60-character wrap breaks, and
`wrap_warning_text` already reproduces it.
Adjust `astwarn_values`'s card-slicing to whatever the public card-text format
actually is; the assertion on the values is the part that matters.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p ast-core --lib wcslib 2>&1 | tail -10
cargo test -p ast-core --lib wcsmap 2>&1 | tail -10
cargo test -p ast-core --test test_fitschan_astwarn 2>&1 | tail -20
```

Expected: `pv_limits_match_the_c_projection_table`, `pv_max_is_per_axis` and
`set_pv_rejects_an_index_the_projection_cannot_use` FAIL to compile (no such
methods), and `badpv_warning_names_each_unusable_parameter_in_ascending_order`
FAILS with an empty warning list.

- [ ] **Step 3: Add the two limit columns**

In `crates/ast-core/src/wcslib.rs`, after `description` (`:172`):

```rust
    /// Largest legal `m` in `PVi_m` on the *latitude* axis.
    ///
    /// The `mxpar` column of C's `PrjInfo` table (wcsmap.c:684-716). C sizes
    /// `params.p` as `mxpar + 1` doubles (wcsmap.c:5862) and `astGetPVMax`
    /// reports that allocation minus one for the latitude axis
    /// (wcsmap.c:2208-2210).
    pub fn pv_max_lat(&self) -> usize {
        match self {
            Self::Azp | Self::Sin | Self::Cyp | Self::Cop | Self::Coe | Self::Cod
            | Self::Coo | Self::Ncp | Self::Hpx => 2,
            Self::Szp => 3,
            Self::Air | Self::Cea | Self::Bon => 1,
            Self::Zpn | Self::Tpn => WCSLIB_MXPAR - 1,
            Self::Tan | Self::Stg | Self::Arc | Self::Zea | Self::Car | Self::Mer
            | Self::Sfl | Self::Par | Self::Mol | Self::Ait | Self::Pco | Self::Tsc
            | Self::Csc | Self::Qsc | Self::Gls | Self::Xph | Self::Unknown => 0,
        }
    }

    /// Largest legal `m` in `PVi_m` on the *longitude* axis.
    ///
    /// The `mxpar2` column of the same table, which is 4 for every projection
    /// except `-TPN`, where the longitude axis also carries polynomial
    /// coefficients.
    pub fn pv_max_lon(&self) -> usize {
        match self {
            Self::Tpn => WCSLIB_MXPAR - 1,
            _ => 4,
        }
    }
```

`WCSLIB_MXPAR` is already `const` in this file at `:324`; move it above the
`impl` if the ordering matters, or reference it as written since `const` items
are order-independent within a module.

Write the `pv_max_lat` arms as an exhaustive `match` with no `_` catch-all, so
adding a projection variant later is a compile error rather than a silent 0.

- [ ] **Step 4: Add `WcsMap::pv_max` and enforce it in `set_pv`**

In `crates/ast-core/src/mappings/wcsmap.rs`, after `projection` (`:256-258`):

```rust
    /// Largest legal projection-parameter index for a 1-based axis index, or
    /// `None` when the axis is neither the longitude nor the latitude axis.
    ///
    /// Mirrors `astGetPVMax` (wcsmap.c:2152-2217), which reads the limit off
    /// the size of the per-axis parameter array — allocated from the
    /// projection table's `mxpar` / `mxpar2` columns — and returns -1 for any
    /// other axis. The lookup uses the declared projection type, as C's
    /// `FindPrjData( this->type )` does: `NCP` and `GLS` keep their own limits
    /// even though they transform through `SIN` and `SFL`.
    pub fn pv_max(&self, axis: usize) -> Option<usize> {
        if axis == 0 || axis > self.ncoord {
            return None;
        }
        if axis - 1 == self.lonax {
            Some(self.proj.pv_max_lon())
        } else if axis - 1 == self.latax {
            Some(self.proj.pv_max_lat())
        } else {
            None
        }
    }
```

Then, in `set_pv`, replace the `m >= MXPAR` guard (`:209-218`) with the
per-projection limit. C `SetPV` (wcsmap.c:3884-3890) checks
`m < 0 || m > mxpar` and names the projection's ctype in the message:

```rust
        let Some(mxpar) = self.pv_max(axis) else {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!(
                    "WcsMap set_pv axis {axis} is neither longitude ({}) nor latitude ({})",
                    self.lonax + 1,
                    self.latax + 1,
                ),
            ));
        };
        if m > mxpar {
            return Err(AstError::new(
                ErrorCode::Wcspa,
                format!(
                    "WcsMap PV parameter index {m} is invalid for a -{} projection \
                     (it should be in the range 0 to {mxpar})",
                    self.proj.code()
                ),
            ));
        }
```

The existing axis-range block below it becomes redundant — `pv_max` performs
the same validation — so fold the two together rather than leaving both.
Keep `MXPAR` if other code uses it; check with
`grep -n 'MXPAR' crates/ast-core/src/mappings/wcsmap.rs` and remove the const
if this was its only use.

- [ ] **Step 5: Rewrite the celestial PV loop as C's single ascending pass**

Replace `crates/ast-core/src/fitschan/wcs_celestial.rs:136-151` with:

```rust
    // Store the supplied PVi_m projection parameters on the WcsMap, warning
    // for any the projection cannot use. Mirrors C `WcsCelestial`
    // (fitschan.c:36095-36131): one ascending pass over m up to the highest
    // index present on any axis, checking the latitude axis before the
    // longitude axis at each index, because that is the order the ASTWARN
    // cards appear in.
    let np = store.pv[lat_axis]
        .len()
        .max(store.pv[lon_axis].len())
        .saturating_sub(1);
    let prj_name = format!("-{}", proj.code());
    for m in 0..=np {
        for axis in [lat_axis, lon_axis] {
            let Some(val) = store.pv[axis].get(m).copied().flatten() else {
                continue;
            };
            // set_pv enforces the projection's per-axis limit, so its error is
            // the "parameter not used by this projection" signal.
            if wcsmap.set_pv(axis + 1, m, val).is_err() {
                warnings.push(format!(
                    "Projection parameter PV{}_{} found, but is not used by {} projections.",
                    axis + 1,
                    m,
                    prj_name
                ));
            }
        }
    }
```

Two details this depends on.

`warnings` is declared at `wcs_celestial.rs:259`, *after* this loop.
Move its declaration above the loop so both the `badpv` and `badcel` texts
push onto the same vector in generation order — C appends to the FitsChan in
call order, and `badpv` fires first.

`store.pv[axis]` is indexed by the 0-based *FITS* axis, and the message uses
`axis + 1` — the 1-based FITS axis number, matching C's `axlat + 1` /
`axlon + 1`. The fixture's `PV2_1` on a `DEC--TAN` axis 2 confirms it.

`proj` is already in scope from `:126`. Use it rather than
`wcsmap.projection()`; they are the same value and the local is clearer.

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p ast-core --lib wcslib 2>&1 | tail -5
cargo test -p ast-core --lib wcsmap 2>&1 | tail -5
cargo test -p ast-core --test test_fitschan_astwarn 2>&1 | tail -5
```

Expected: all PASS.

- [ ] **Step 7: Re-measure the three cases**

```bash
for c in projp projp-nopv0 projp-pv0only; do
  n=$(echo "$c" | tr - _)
  printf '%-16s ' "$c"
  WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv "${n}_native" 2>&1 \
    | grep -E 'differs|line count|test result' | head -2 | tr '\n' ' '
  echo
done
```

Expected: all three report `test result: ok`.

The `set_pv` change means PVs the projection cannot use are no longer stored,
so the *dumped* PV cards can change too. `projp`'s expected output carries
`PV1_3_A = 180.0` (LONPOLE, legal on the longitude axis) and
`PV2_0_A = 0.0` (legal on the latitude axis) but no `PV2_1_A` / `PV2_2_A` —
confirming C does not store the rejected ones either. If a case now differs at
one of those lines, the divergence is in *which* parameters got stored, not in
the warning.

- [ ] **Step 8: Check `tan-zeropv` and the PV-carrying corpora**

`tan-zeropv` (a U9 case) also expects `badpv` warnings — its
`expected/tan-zeropv.native:136` and `:140` hold the same two `PV2_1` / `PV2_2`
texts, plus a third distortion-coefficient warning. It stays excluded for its
own reason (WcsMap shape, first diff at line 70), but its *first diff must not
move backwards*: re-measure it and record the result.

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv tan_zeropv_native 2>&1 \
  | grep -E 'differs|line count' | head -3
cargo test -p ast-core --test test_wcsconv zpn 2>&1 | tail -3
cargo test -p ast-core --test test_wcsconv tpn 2>&1 | tail -3
cargo test -p ast-core --lib mappings::wcsmap 2>&1 | tail -3
```

`ZPN` and `TPN` are the projections whose limits are `WCSLIB_MXPAR - 1`, so
they are where a wrong limit table would show up as a newly rejected PV.

- [ ] **Step 9: Remove the three exclusion entries**

Delete the `projp_native`, `projp-nopv0_native` and `projp-pv0only_native`
entries from `NATIVE_READ_EXCLUDED`.

- [ ] **Step 10: Verify the two-sided gate**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_transform_oracle head_sections 2>&1 | tail -5
cargo test -p ast-core 2>&1 | grep -E 'test result' | tail -20
```

Expected: 192 passed, and 5 fewer `WCSCONV_NO_EXCLUDE=1` failures than the 45
baseline.

- [ ] **Step 11: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/wcslib.rs \
        crates/ast-core/src/mappings/wcsmap.rs \
        crates/ast-core/src/fitschan/wcs_celestial.rs \
        crates/ast-core/tests/test_fitschan_astwarn.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "wcsmap: enforce the projection's per-axis PV limit and warn on unusable PVi_m (projp x3)"
```

#### Sub-fix D: the `badctype` warning — an illegal spectral algorithm code

**Cases:** `awav-a2f`, `freq-f2w`, `wave-w2f`.

C `NonLinSpecWcs` (fitschan.c:23787-23815) checks the `-X2P` algorithm code's
`P` character against the one the SpecFrame's own system implies, and warns
when they disagree:

```c
      pc = 0;
      s_sys = astGetSystem( specfrm );
      if( s_sys == AST__FREQ || s_sys == AST__ENERGY ||
          s_sys == AST__WAVENUM ||  s_sys == AST__VRADIO ) {
         pc = 'F';
      } else if( s_sys == AST__WAVELEN || s_sys == AST__VOPTICAL ||
                 s_sys == AST__REDSHIFT ){
         pc = 'W';
      } else if( s_sys == AST__AIRWAVE ) {
         pc = 'A';
      } else if( s_sys == AST__BETA || s_sys == AST__VREL ) {
         pc = 'V';
      } else if( astOK ) {
         pc = algcode[ 3 ];
         astError( AST__INTER, "%s: Function NonLinSpecWcs does not yet "
                   "support spectral axes of type %s (internal AST "
                   "programming error).", status, method,
                   astGetC( specfrm, "System" ) );
      }
      if( algcode[ 3 ] != pc ) {
         const char *text = astGetC( specfrm, "System" );
         if( text ) {
            sprintf( buf, "The spectral CTYPE value %s%s is not legal - using"
                     " %s%.3s%c instead.", text, algcode, text, algcode, pc );
            Warn( this, "badctype", buf, method, class, status );
         }
      }
```

The comment above it records why this is a warning and not an error: "We can
however continue, since AST interprets illegal 'P' systems correctly."
So nothing about the mapping changes — only a card is added.

Reading the three fixtures against that code:

| Case | CTYPE | S system | required `pc` | `algcode[3]` | Warning text |
|---|---|---|---|---|---|
| `awav-a2f` | `AWAV-A2F` | `AWAV` | `A` | `F` | `The spectral CTYPE value AWAV-A2F is not legal - using AWAV-A2A instead.` |
| `freq-f2w` | `FREQ-F2W` | `FREQ` | `F` | `W` | `... FREQ-F2W is not legal - using FREQ-F2F instead.` |
| `wave-w2f` | `WAVE-W2F` | `WAVE` | `W` | `F` | `... WAVE-W2F is not legal - using WAVE-W2W instead.` |

Each expected text is at line 53-54 of the matching
`expected/<case>.native`, wrapped across two cards.

`astGetC( specfrm, "System" )` returns the short system name, which is exactly
`SpecSystem::as_str` (`specframe.rs:47-61`): `"AWAV"`, `"FREQ"`, `"WAVE"`.
`%.3s` of `algcode` takes the first three characters of e.g. `"-A2F"`, giving
`"-A2"`, and `%c` appends the required `pc`.

The port has no channel for a spectral warning.
`SpectralResult` (`wcs_spectral.rs:60-95`) has no `warnings` field, and
`wcs_read.rs:48-51` collects only the celestial ones:

```rust
    let warnings: Vec<String> = celestial
        .as_ref()
        .map(|c| c.warnings.clone())
        .unwrap_or_default();
```

So this sub-fix is the warning text plus the plumbing to carry it out, mirroring
what `CelestialResult` already does.

The C `astError` branch — a system with no defined `pc`, i.e. `WAVN`, `ENER`
handled above, so in practice only a system outside the four groups — must be
ported as an error, not skipped. `SpecSystem` has ten variants and the four
groups cover nine of them (`Freq`, `Energy`, `Wavenum`, `Vradio`; `Wavelen`,
`Voptical`, `Redshift`; `Airwave`; `Beta`, `Vrel`) — which is all ten. Write
the match exhaustively with no catch-all and the branch becomes unreachable by
construction, which is the faithful shape: C's `astError` exists because C
cannot prove exhaustiveness.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_spectral.rs` (`SpectralResult` gains `warnings`; `nonlin_spec_wcs` returns them; `build_spectral` propagates)
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs:48-51` (collect spectral warnings after celestial ones)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (delete three exclusion entries)
- Test: `crates/ast-core/tests/test_fitschan_astwarn.rs`

**Interfaces:**
- Consumes: `SpecSystem::as_str` (`specframe.rs:48`), `SpecFrame::system` (`specframe.rs:406`), `CelestialResult::warnings` as the pattern, `push_astwarn` from sub-fixes A and B.
- Produces:

```rust
// wcs_spectral.rs
pub(crate) struct SpectralResult {
    // ... existing fields unchanged ...
    /// Warning messages to emit as ASTWARN cards. Mirrors C `Warn`'s
    /// `badctype` condition: the `-X2P` algorithm code's `P` character
    /// disagrees with the one the axis's own system implies
    /// (fitschan.c:23807-23815).
    pub warnings: Vec<String>,
}

/// Returns the mapping and any warnings raised while building it.
fn nonlin_spec_wcs(
    algcode: &str,
    specfrm: &SpecFrame,
    crval: f64,
) -> AstResult<Option<(DynMapping, Vec<String>)>>;
```

`nonlin_spec_wcs` returning a tuple is the smaller change; the alternative — a
`&mut Vec<String>` out-parameter — reads worse at the one call site
(`wcs_spectral.rs:205`) and hides the output in the signature.

- [ ] **Step 1: Write the failing test**

Add to `crates/ast-core/tests/test_fitschan_astwarn.rs`:

```rust
/// `awav-a2f`-shaped: an air-wavelength axis whose algorithm code says the
/// values are expressed as frequency. C keeps the mapping (it interprets the
/// illegal code correctly) and warns.
const ONE_AXIS_BADCTYPE: &str = "\
SIMPLE  =                    T / Fits standard
BITPIX  =                  -32 / Bits per pixel
NAXIS   =                    1 / Number of dimensions
NAXIS1  =                 1024 / Length of axis
CTYPE1  = 'AWAV-A2F'           / Air wavelength sampled, expressed as frequency
CRVAL1  =            5.000E-07 / Reference air wavelength (metres)
CRPIX1  =              512.500 / Reference pixel
CDELT1  =            1.000E-10 / Wavelength increment (metres)
CUNIT1  = 'm       '           / Axis units
RESTFRQ =           5.996E+14  / Rest frequency (Hz)
SPECSYS = 'BARYCENT'           / Spectral reference frame
";

#[test]
fn badctype_warning_names_the_legal_ctype_and_precedes_naxis1() {
    let mut fc = chan_from(ONE_AXIS_BADCTYPE);
    let _ = fc.read().expect("read succeeds");

    assert_eq!(
        astwarn_values(&mut fc),
        vec![
            "        ",
            "The spectral CTYPE value AWAV-A2F is not legal - using ",
            "AWAV-A2A instead.",
            "        ",
        ]
    );

    // A 1-axis header parks C's card cursor on NAXIS1, so the block precedes
    // it (sub-fix B).
    let kws = keywords(&mut fc);
    let first_warn = kws.iter().position(|k| k == "ASTWARN").expect("ASTWARN");
    let naxis1 = kws.iter().position(|k| k == "NAXIS1").expect("NAXIS1");
    assert!(
        first_warn < naxis1,
        "ASTWARN block must precede NAXIS1, got {kws:?}"
    );
}
```

The expected strings come from
`tests/fixtures/wcsconv/expected/awav-a2f.native:52-55`.
This test also covers the 1-axis position case that sub-fix B's test could not
exercise, because no `badcel` header is 1-axis.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p ast-core --test test_fitschan_astwarn 2>&1 | tail -20
```

Expected: FAIL with an empty warning list.

- [ ] **Step 3: Emit the warning in `nonlin_spec_wcs`**

In `crates/ast-core/src/fitschan/wcs_spectral.rs`, change `nonlin_spec_wcs`'s
return type to `AstResult<Option<(DynMapping, Vec<String>)>>` and add, just
before the final `Ok(Some(...))`:

```rust
    // Warn when the algorithm code's "P" character disagrees with the one the
    // axis's own system implies. Mirrors C `NonLinSpecWcs`
    // (fitschan.c:23787-23815): AST interprets the illegal code correctly, so
    // this is a warning and the mapping is unaffected.
    let mut warnings = Vec::new();
    let s_sys = specfrm.system();
    let pc = match s_sys {
        SpecSystem::Freq | SpecSystem::Energy | SpecSystem::Wavenum | SpecSystem::Vradio => b'F',
        SpecSystem::Wavelen | SpecSystem::Voptical | SpecSystem::Redshift => b'W',
        SpecSystem::Airwave => b'A',
        SpecSystem::Beta | SpecSystem::Vrel => b'V',
    };
    if bytes.get(3).copied() != Some(pc) {
        let text = s_sys.as_str();
        // C's format is "%s%s ... %s%.3s%c": the system name, the full
        // algorithm code, then the system name, the code's first three
        // characters, and the required "P" character.
        let stem = &algcode[..algcode.len().min(3)];
        warnings.push(format!(
            "The spectral CTYPE value {text}{algcode} is not legal - using \
             {text}{stem}{} instead.",
            pc as char
        ));
    }

    let iwc_to_s = CmpMap::new(iwc_to_x, x_to_s, Composition::Series)?;
    Ok(Some((Box::new(iwc_to_s), warnings)))
```

`bytes` is already bound at `:278` from `algcode.as_bytes()`.
The `pc` match must be exhaustive with no `_` arm: C's final `else` raises
`AST__INTER` because it cannot prove the four groups cover every system, and
they do — an exhaustive match makes that a compile-time guarantee instead.

- [ ] **Step 4: Propagate through `build_spectral`**

Add `warnings: Vec<String>` to `SpectralResult` (`wcs_spectral.rs:60-95`) with
the doc comment from the Interfaces block, then at the `nonlin_spec_wcs` call
site (`:204-212`):

```rust
    let mut warnings: Vec<String> = Vec::new();
    let mapping: DynMapping = if algcode.is_empty() {
        // ... unchanged ...
    } else if is_nonlin_x2p(&algcode) {
        match nonlin_spec_wcs(&algcode, &frame, crval) {
            Ok(Some((map, warns))) => {
                warnings = warns;
                map
            }
            // ... remaining arms unchanged ...
        }
    } else if ...
```

and set `warnings` on the returned `SpectralResult`.
Every other construction site of `SpectralResult` gets
`warnings: Vec::new()`; find them with
`grep -n 'SpectralResult {' crates/ast-core/src/fitschan/`.

- [ ] **Step 5: Collect them in `wcs_read`**

Replace `crates/ast-core/src/fitschan/wcs_read.rs:48-51` with:

```rust
    // Warnings in C's generation order: `WcsCelestial` runs before
    // `WcsSpectral` in `WcsMapFrm` (fitschan.c:37985-37991), and `Warn`
    // appends each block in front of the same current card, so celestial
    // warnings precede spectral ones.
    let mut warnings: Vec<String> = celestial
        .as_ref()
        .map(|c| c.warnings.clone())
        .unwrap_or_default();
    if let Some(s) = spectral.as_ref() {
        warnings.extend(s.warnings.iter().cloned());
    }
```

Check the local holding the spectral result's name at that point in
`decode_wcs_with_alts_inner` and use it; `spectral` is the likely one.

- [ ] **Step 6: Run the test to verify it passes**

```bash
cargo test -p ast-core --test test_fitschan_astwarn 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 7: Re-measure the three cases**

```bash
for c in awav-a2f freq-f2w wave-w2f; do
  n=$(echo "$c" | tr - _)
  printf '%-12s ' "$c"
  WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv "${n}_native" 2>&1 \
    | grep -E 'differs|line count|test result' | head -2 | tr '\n' ' '
  echo
done
```

Expected: all three report `test result: ok`.

- [ ] **Step 8: Check the other `-X2P` cases did not gain a warning**

`vopt-f2w` (U4) and `wave-f2w` (U4) are also `-X2P` headers, and
`freq-wav` / `wave-f2w` are already-passing cases in the included 192.
A spurious `badctype` on any of them would be a wrong `pc` table.

```bash
grep -l 'CTYPE1  = .[A-Z]\{4\}-[A-Z]2[A-Z]' crates/ast-core/tests/fixtures/wcsconv/inputs/*.head \
  | while read f; do
      c=$(basename "$f" .head)
      printf '%-14s expects ASTWARN: ' "$c"
      grep -c ASTWARN "crates/ast-core/tests/fixtures/wcsconv/expected/$c.native" 2>/dev/null || echo 0
    done
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

Every case whose expected file has 0 `ASTWARN` cards must still produce none.

- [ ] **Step 9: Remove the three exclusion entries and the cause legend**

Delete the `awav-a2f_native`, `freq-f2w_native` and `wave-w2f_native` entries
from `NATIVE_READ_EXCLUDED`.
All eight ASTWARN entries are now gone, so remove
`ASTWARN = warning-card insertion position` (and any `ASTWARN: warning card`
wording) from the constant's header comment.

- [ ] **Step 10: Verify the two-sided gate and the full regression breadth**

U7 touches the shared read and write paths, so this is where the spec's full
regression list runs.

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
cargo test -p ast-core --test test_fitschan 2>&1 | tail -3
cargo test -p ast-core --test test_native_roundtrip_all 2>&1 | tail -3
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -3
cargo test -p ast-core --test test_transform_oracle head_sections 2>&1 | tail -5
cargo test --workspace 2>&1 | grep -E 'test result|error' | tail -30
```

Expected: `test_wcsconv` 192 passed; `WCSCONV_NO_EXCLUDE=1` down 8 from the 45
baseline; the three serialization round-trips still byte-exact (24 + 139 +
719); 66 `test_fitschan*`; simplify 427; `HEAD_ORACLE_EXCLUDED` still empty.

- [ ] **Step 11: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_spectral.rs \
        crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/tests/test_fitschan_astwarn.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: warn when a spectral -X2P algorithm code disagrees with the axis system (3 cases)"
```

---

### Task 12: U8 — Frame attribute plumbing on the celestial read path (10 cases)

**The spec's stated cause for this unit is disproved. Read this before starting.**

The spec (cause 8) says:

> We build a plain `Frame` where C builds a typed one, so the expected
> `SYSTEM`/`DOMAIN` card is absent from our output and the diff surfaces at
> whatever card we emit instead (`AX1_x`, `ACTUNT_x`, `SYSTEM_A='Unknown'`).

That is wrong.
The frame *classes* already match C in all ten cases.
Measured with `crates/ast-core/examples/wcsconv_debug.rs`, whose stderr
prints one line per frame (`Frame i: domain= ident= sys=`, plus
`test_epoch=`/`test_equinox=`/`sky_ref_is=` for SkyFrames and a `dump_sub`
recursion for CmpFrames):

| case | class we build | class expected |
|---|---|---|
| `origin`, `sip-alt`, `multi-wcs`, `cname`, `offset-sky`, `offset-sky-pole` | `SkyFrame` | `SkyFrame` |
| `alt-spectral`, `class-noproj` | `CmpFrame(SkyFrame, SpecFrame)` | same |
| `axes-swap` | `CmpFrame(CmpFrame(Frame, SkyFrame), SpecFrame)` | same |
| `unknown-spec` | plain `Frame` | plain `Frame` |

The ten cases are four unrelated attribute-plumbing defects that happen to
surface at a similar-looking card.
The spec anticipated exactly this and says what to do:

> This unit is the one most likely to split: ten cases share a first-diff
> *shape*, which is weaker evidence of a shared cause than the other groups
> have.

> If cause 8 turns out to be two or three distinct recognition gaps, split
> the unit rather than stretching one commit.

So this task is five sub-fixes, each its own commit:

| sub-fix | defect | cases |
|---|---|---|
| A | Epoch fallback from EQUINOX is gated on the system; C's is not | `multi-wcs`, `offset-sky`, `offset-sky-pole` |
| B | CNAME never becomes a SkyFrame axis Label | `cname` |
| C | GLS→SFL rewrite runs before `prj` is captured, losing the GLS description | `class-noproj` |
| D | The read path leaves `System` unset when the value is ICRS; C always sets it | `origin`, `sip-alt`, `alt-spectral` |
| E | Unrecognized axes never get C's CTYPE-derived plain-Frame `Domain` | `unknown-spec`, `axes-swap` |

Two claims in the spec's cause-8 paragraph are also wrong and must not be
acted on:

- "Generic 1-character `xLON`/`xLAT` CTYPE convention (`XLON-TAN`,
  `XLAT-TAN`) not recognized as celestial" — already recognized.
  `is_longitude_ctype` (`fits_store.rs:270-280`) implements C `GetSkyAxes`'s
  full rule including the `xLON`/`xyLN` forms.
- "Blank projection code (`RA--`, `DEC-`, CLASS style) should assume
  old-fashioned CAR" — the blank-projection path is ported, but as C
  actually writes it: blank → `-GLS` in `ClassTrans`, then `-GLS` → `-SFL`
  in `SpecTrans`. Not CAR. Sub-fix C is about the *description string* that
  survives that chain, not about recognition.

---

#### Sub-fix A: Epoch falls back to EQUINOX for every system, not just equatorial/ecliptic

**Cases:** `multi-wcs`, `offset-sky`, `offset-sky-pole`.

**Measured.** All three expected files carry `EPOCH_A = 2000.0`; our output
has no Epoch card at all (`wcsconv_debug` stderr shows `test_epoch=false`
for each). None of the three headers has MJD-OBS or DATE-OBS; all three
have `EQUINOX = 2000.0`.

`multi-wcs` frame 2 is `System=GALACTIC` and `offset-sky` /
`offset-sky-pole` are `System=Unknown` — which is exactly why they fail,
and why `origin` (ICRS, `test_epoch=true`) and `cname` (ICRS,
`test_epoch=true test_equinox=true`) do not.

**C, fitschan.c:39334-39362.** `eqmjd` is computed from `equinox` with no
system guard whatsoever:

```c
/* Convert the equinox to a Modified Julian Date. */
   if( equinox != AST__BAD ) {
      if( bj == 'B' ) { eqmjd = palEpb2d( equinox ); }
      else { eqmjd = palEpj2d( equinox ); }
   } else { eqmjd = AST__BAD; }

/* Get a value for the Epoch attribute. If no value is available, use
   EQUINOX and issue a warning. */
   mjdobs = ChooseEpoch( this, store, s, method, class, status );
   if( mjdobs == AST__BAD ) {
      mjdobs = eqmjd;
      if( mjdobs != AST__BAD ) {
         sprintf( buf, "The original FITS header did not specify the "
                  "date of observation. A default value of %c%.8g was "
                  "assumed.", bj, equinox );
         Warn( this, "nomjd-obs", buf, method, class, status );
      }
   }
```

and `mjdobs` is then applied with no guard either (fitschan.c:39498-39500):

```c
/* Store the epoch of the observation in the SkyFrame. */
      if( mjdobs != AST__BAD ) astSetEpoch( ret, mjdobs );
```

Only the *Equinox* attribute is system-guarded (fitschan.c:39501-39503):

```c
/* For equatorial and ecliptic systems, store the epoch of the reference
   equinox in the SkyFrame. */
      if( ( !strcmp( sys, "EQU" ) || !strcmp( sys, "ECL" ) ) &&
          equinox != AST__BAD ) astSetEquinox( ret, eqmjd );
```

**Rust, `wcs_celestial.rs:308-360`.** `eq_year` is gated on
`is_equ_or_ecl` — correct for the Equinox attribute — and that same gated
value is then reused as the Epoch fallback, which over-restricts it:

```rust
    let eq_year: Option<f64> = if let Some(eq) = store.equinox {
        is_equ_or_ecl.then_some(eq)
    } else if store.radesys.is_some() && is_equ_or_ecl {
        match system {
            SkySystem::Fk4 | SkySystem::Fk4NoE => Some(1950.0),
            _ => Some(2000.0),
        }
    } else {
        None
    };
```

```rust
    } else if let Some(eq) = eq_year {
        // mjdobs ← eqmjd fallback (fitschan.c:39345-39346): only the
        // equinox-bearing systems have an eqmjd to fall back to.
        sky_frame.set_epoch_year(eq);
    }
```

That trailing comment states the rule C does not follow.
The fix is to split the one value into two: an unguarded `eqmjd` for the
Epoch fallback, and the existing guarded `eq_year` for Equinox.

**The three expected files contain zero ASTWARN cards.**  Verify that
before writing code — C's `nomjd-obs` Warn is not in the default `Warnings`
value, so it must not fire here. `grep -c ASTWARN` on each expected file
must print `0`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:308-360`
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove 3 exclusion entries)
- Create: `crates/ast-core/tests/test_fitschan_read.rs` (new file; Task 12
  sub-fixes B, D and E append to it)

**Interfaces:**
- Consumes: `FitsStore::equinox: Option<f64>`, `FitsStore::radesys:
  Option<String>`, `FitsStore::mjdobs: Option<f64>`;
  `SkyFrame::set_epoch_year(&mut self, year: f64)`,
  `SkyFrame::set_equinox_year(&mut self, year: f64)`,
  `SkyFrame::set_epoch_mjd(&mut self, mjd: f64)`.
- Produces: no new public API. Two local bindings inside
  `wcs_celestial::wcs_celestial`: `epoch_fallback_year: Option<f64>`
  (unguarded, feeds Epoch) and the existing `eq_year: Option<f64>`
  (guarded, feeds Equinox).

- [ ] **Step 1: Confirm the three expected files carry no ASTWARN**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs/crates/ast-core/tests/fixtures/wcsconv
for f in multi-wcs offset-sky offset-sky-pole; do
  printf '%-18s ASTWARN=%s EPOCH=%s\n' "$f" \
    "$(grep -c ASTWARN expected/$f.native)" \
    "$(grep -c '^EPOCH' expected/$f.native)"
done
```

Expected: `ASTWARN=0` and a non-zero `EPOCH` count for all three.
If any shows `ASTWARN` greater than zero, stop — the `nomjd-obs` Warn does
fire and this sub-fix must emit it too.

- [ ] **Step 2: Write the failing test**

`crates/ast-core/tests/test_fitschan_read.rs` does not exist yet — this
sub-fix creates it, and sub-fixes B, D and E append to it. Start with the
module header and imports:

```rust
//! FITS read-path Frame and Axis attribute plumbing.
//!
//! Each test here isolates one `WcsCelestial` / `WcsMapFrm` attribute
//! decision that a wcsconv fixture exercises only indirectly, so a
//! failure names the attribute rather than a serialised card.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::frames::{Frame, SkyFrame, SkySystem};
```

Add trait and type imports as later sub-fixes need them; `Channel` is what
brings `read()` into scope, and `Frame` is what brings the attribute
accessors into scope.

Then the test itself:

```rust
/// C computes `eqmjd` from EQUINOX with no system guard
/// (fitschan.c:39334-39340) and applies it as the Epoch fallback
/// unconditionally (fitschan.c:39500). Only the *Equinox* attribute is
/// restricted to equatorial and ecliptic systems (fitschan.c:39501-39503).
/// A Galactic header with EQUINOX but no MJD-OBS/DATE-OBS therefore gets
/// an Epoch but no Equinox.
#[test]
fn galactic_header_takes_its_epoch_from_equinox() {
    let mut fc = FitsChan::new();
    for card in [
        "NAXIS   =                    2",
        "NAXIS1  =                  100",
        "NAXIS2  =                  100",
        "CTYPE1  = 'GLON-TAN'",
        "CTYPE2  = 'GLAT-TAN'",
        "CRVAL1  =                 10.0",
        "CRVAL2  =                 20.0",
        "CRPIX1  =                 50.5",
        "CRPIX2  =                 50.5",
        "CDELT1  =                -0.01",
        "CDELT2  =                 0.01",
        "EQUINOX =               2000.0",
    ] {
        fc.put_fits(card, false).unwrap();
    }
    fc.rewind();
    let obj = fc.read().unwrap().expect("header should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("read should produce a FrameSet, got {obj:?}");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame");
    assert_eq!(sky.system(), SkySystem::Galactic);
    // Epoch is set — J2000.0 taken from EQUINOX.
    assert!(sky.test_epoch(), "Galactic Epoch must fall back to EQUINOX");
    assert!((sky.epoch_year() - 2000.0).abs() < 1e-9);
    // Equinox is NOT set — Galactic is neither EQU nor ECL.
    assert!(
        !sky.test_equinox(),
        "Galactic must not carry an Equinox attribute"
    );
}
```

- [ ] **Step 3: Run it to verify it fails**

Run:

```bash
cargo test -p ast-core --test test_fitschan_read \
  galactic_header_takes_its_epoch_from_equinox -- --nocapture
```

Expected: FAIL on `assert!(sky.test_epoch(), ...)` — the Epoch is unset
because `eq_year` was `None` for Galactic.

- [ ] **Step 4: Split the guarded and unguarded values**

In `crates/ast-core/src/fitschan/wcs_celestial.rs`, replace the `eq_year`
binding with two bindings. The raw equinox and the FK4/FK5 default both
become system-independent; only the *use* for Equinox stays guarded:

```rust
    // C's equinox value, with the RADESYS-driven default applied
    // (fitschan.c:39322-39332). No system guard: C computes `eqmjd` from
    // whatever equinox it has (fitschan.c:39334-39340) and uses it as the
    // Epoch fallback for every system (fitschan.c:39345-39346, :39370).
    let equinox_year: Option<f64> = if let Some(eq) = store.equinox {
        Some(eq)
    } else if store.radesys.is_some() {
        match system {
            SkySystem::Fk4 | SkySystem::Fk4NoE => Some(1950.0),
            _ => Some(2000.0),
        }
    } else {
        None
    };

    // The Equinox *attribute* is stored only for equatorial and ecliptic
    // systems (fitschan.c:39501-39503).
    let eq_year: Option<f64> = is_equ_or_ecl.then_some(equinox_year).flatten();
```

Leave the `stores_equinox && let Some(eq) = eq_year` block untouched.
Then change the Epoch fallback arm to read the unguarded value:

```rust
    } else if let Some(eq) = equinox_year {
        // Epoch falls back to eqmjd for every system (fitschan.c:39345-39346,
        // applied unguarded at :39370).
        sky_frame.set_epoch_year(eq);
    }
```

- [ ] **Step 5: Run the test to verify it passes**

Run:

```bash
cargo test -p ast-core --test test_fitschan_read \
  galactic_header_takes_its_epoch_from_equinox
```

Expected: PASS.

- [ ] **Step 6: Re-measure the three cases**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep -E 'multi-wcs|offset-sky|^test result'
```

Expected: `multi_wcs_native`, `offset_sky_native` and
`offset_sky_pole_native` no longer show `EPOCH_A` as their first diff.
If a case now shows a *different* first diff, record it in the plan's
first-diff table and leave its exclusion entry in place — that is the
re-measure protocol working, not a failure.

- [ ] **Step 7: Remove the exclusion entries for whichever cases are now byte-exact**

In `crates/ast-core/tests/test_wcsconv.rs`, delete the
`NATIVE_READ_EXCLUDED` rows for each case that is now fully clean. Do not
delete a row whose case still has any diff.

- [ ] **Step 8: Run the full wcsconv suite both ways**

```bash
cargo test -p ast-core --test test_wcsconv
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv
```

Expected: first run 192 passed; second run down by however many entries
step 7 removed.

- [ ] **Step 9: Regression sweep — this changes what the reader builds**

```bash
cargo test -p ast-core --test test_fitschan
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core --test test_transform_oracle head_sections
```

Expected: 66 `test_fitschan*` tests still pass; `HEAD_ORACLE_EXCLUDED`
still empty and the head oracle report no worse than before. Sub-fix A
touches Epoch on a read-built SkyFrame, so the oracle is the independent
check that the fix is real rather than fixture-shaped.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_celestial.rs \
        crates/ast-core/tests/test_fitschan_read.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: Epoch falls back to EQUINOX for every sky system, not just equatorial (3 cases)"
```

---

#### Sub-fix B: CNAME becomes a SkyFrame axis Label

**Case:** `cname`.

**Measured.** `inputs/cname.head` carries both name keywords:

```
CTYPE1  = 'RA---TAN'           / Celestial longitude
CTYPE2  = 'DEC--TAN'           / Celestial latitude
CNAME1  = 'Right Ascension'    / Axis name for axis 1
CNAME2  = 'Declination'        / Axis name for axis 2
```

`expected/cname.native` carries them as Labels inside the two `SkyAxis`
blocks (`LABEL_C = 'Right Ascension'`, `LABEL_D = 'Declination'`); our
output closes the first `SkyAxis` at `ENDAST_D` having set no Label, which
is the recorded first diff at line 34.

**C, fitschan.c:39505-39510.** The Labels are applied straight from the
store, indexed by the *FITS* axis numbers, onto the SkyFrame's *physical*
axes 0 (longitude) and 1 (latitude):

```c
/* If either of the CNAME keywords is set, use it as the axis label. */
      ckeyval = GetItemC( &(store->cname), axlon, 0, s, NULL, method, class, status );
      if( ckeyval ) astSetLabel( ret, 0, ckeyval );
      ckeyval = GetItemC( &(store->cname), axlat, 0, s, NULL, method, class, status );
      if( ckeyval ) astSetLabel( ret, 1, ckeyval );
```

Note the index asymmetry, and preserve it: the *store* is read at `axlon` /
`axlat` (the FITS axis the keyword came from) while the *SkyFrame* is
written at `0` / `1` (lon/lat physically, before any axis permutation).
`SkyFrame::set_axis_label` takes a 1-based axis and resolves it through
`physical_axis`, so C's `astSetLabel(ret, 0, ...)` is
`set_axis_label(1, ...)` only when the frame is unpermuted. For the
permuted case use the physical accessor path described in step 4.

**Rust.** The store side is already right: `wcs_keywords.rs:216` reads
CNAME into `store.cname`, and `fset_to_store/sky_sys.rs:410-417` writes it
back out. The only place a CNAME becomes a Label today is
`seed_linear_frame` (`wcs_read.rs:826-843`), which is the *plain-Frame*
path:

```rust
fn seed_linear_frame(store: &FitsStore, axes: &[usize]) -> AstResult<DynFrame> {
    let mut builder = BasicFrameBuilder::new(axes.len());
    for (k, &axis) in axes.iter().enumerate() {
        let idx_1based = k + 1;
        if let Some(ctype) = &store.ctype[axis] {
            builder = builder.symbol(idx_1based, ctype.clone());
        }
        if let Some(label) = &store.cname[axis] {
            builder = builder.label(idx_1based, label.clone());
        } else if let Some(ctype) = &store.ctype[axis] {
            builder = builder.label(idx_1based, ctype.clone());
        }
        ...
```

`wcs_celestial.rs` never calls any label setter. Grepping that file for
setters finds only `set_system` (`:68`, `:215`), `set_equinox_mjd`
(`:69`, `:71`), `set_epoch_mjd` (`:76`, `:354`), `set_equinox_year`
(`:329`) and `set_domain` (`:393`, `:419`, `:420`) — no label call at all.
So this is a missing leg, not a wrong one.

Note the fallback asymmetry, and do *not* port it across: `seed_linear_frame`
falls back to CTYPE when CNAME is absent, because a plain Frame has no
better default. C's celestial path has no such fallback — an absent CNAME
leaves the SkyFrame's system-derived default label in place
(`SkyFrame::axis_label` → `default_labels`). Only set the Label when
`store.cname[axis]` is `Some`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs` (after the
  `set_projection` call at `:276-277`, before the equinox block at `:280`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove 1 exclusion entry)
- Test: `crates/ast-core/tests/test_fitschan_read.rs`

**Interfaces:**
- Consumes: `FitsStore::cname: Vec<Option<String>>`;
  `SkyFrame::set_axis_label(&mut self, axis: usize, label: impl Into<String>)
  -> AstResult<()>` (`skyframe.rs:268`), which resolves its 1-based `axis`
  through `physical_axis`.
- Produces: no new public API.

- [ ] **Step 1: Write the failing test**

Add to `crates/ast-core/tests/test_fitschan_read.rs`:

```rust
/// CNAME<i> supplies the axis Label on a celestial frame, exactly as it
/// already does on a plain linear frame. C reads the store at the FITS
/// longitude/latitude axis and writes the SkyFrame's physical axes 0 and 1
/// (fitschan.c:39505-39510). An absent CNAME leaves the system-derived
/// default in place — unlike the plain-Frame path, there is no CTYPE
/// fallback.
#[test]
fn cname_supplies_skyframe_axis_labels() {
    let mut fc = FitsChan::new();
    for card in [
        "NAXIS   =                    2",
        "NAXIS1  =                  100",
        "NAXIS2  =                  100",
        "CTYPE1  = 'RA---TAN'",
        "CTYPE2  = 'DEC--TAN'",
        "CRVAL1  =                 10.0",
        "CRVAL2  =                 20.0",
        "CRPIX1  =                 50.5",
        "CRPIX2  =                 50.5",
        "CDELT1  =                -0.01",
        "CDELT2  =                 0.01",
        "CNAME1  = 'Right Ascension'",
        "CNAME2  = 'Declination'",
    ] {
        fc.put_fits(card, false).unwrap();
    }
    fc.rewind();
    let obj = fc.read().unwrap().expect("header should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("expected a FrameSet, got {obj:?}");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame");
    assert!(sky.test_axis_label(1).unwrap());
    assert!(sky.test_axis_label(2).unwrap());
    assert_eq!(sky.axis_label(1).unwrap(), "Right Ascension");
    assert_eq!(sky.axis_label(2).unwrap(), "Declination");
}

/// The same header without CNAME must leave both Labels unset, so the
/// SkyFrame's system-derived defaults apply. C has no CTYPE fallback on
/// the celestial path.
#[test]
fn absent_cname_leaves_skyframe_labels_at_their_defaults() {
    let mut fc = FitsChan::new();
    for card in [
        "NAXIS   =                    2",
        "NAXIS1  =                  100",
        "NAXIS2  =                  100",
        "CTYPE1  = 'RA---TAN'",
        "CTYPE2  = 'DEC--TAN'",
        "CRVAL1  =                 10.0",
        "CRVAL2  =                 20.0",
        "CRPIX1  =                 50.5",
        "CRPIX2  =                 50.5",
        "CDELT1  =                -0.01",
        "CDELT2  =                 0.01",
    ] {
        fc.put_fits(card, false).unwrap();
    }
    fc.rewind();
    let obj = fc.read().unwrap().expect("header should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("expected a FrameSet, got {obj:?}");
    };
    let sky = fs.current().as_any().downcast_ref::<SkyFrame>().unwrap();
    assert!(!sky.test_axis_label(1).unwrap());
    assert!(!sky.test_axis_label(2).unwrap());
    assert_eq!(sky.axis_label(1).unwrap(), "Right ascension");
}
```

The expected default string in the last assertion comes from
`SkyFrame::default_labels` (`skyframe.rs:839`). Read that function and use
whatever it actually returns for FK5/ICRS — do not guess the
capitalization.

- [ ] **Step 2: Run both tests to verify they fail as expected**

Run:

```bash
cargo test -p ast-core --test test_fitschan_read cname -- --nocapture
cargo test -p ast-core --test test_fitschan_read absent_cname
```

Expected: `cname_supplies_skyframe_axis_labels` FAILS on
`assert!(sky.test_axis_label(1).unwrap())`.
`absent_cname_leaves_skyframe_labels_at_their_defaults` should already
PASS — it is the guard that the fix does not introduce a CTYPE fallback.
If the default-label assertion fails, fix the expected string from
`default_labels` rather than changing the code.

- [ ] **Step 3: Apply the Labels in `wcs_celestial.rs`**

Insert immediately after the `set_projection` block (`:276-277`) and
before the `is_equ_or_ecl` binding:

```rust
    // If either CNAME keyword is set, use it as the axis Label
    // (fitschan.c:39505-39510). C reads the store at the FITS longitude and
    // latitude axes but writes the SkyFrame's physical lon/lat axes, so the
    // two index spaces are deliberately different. Unlike the plain-Frame
    // path there is no CTYPE fallback: an absent CNAME leaves the
    // system-derived default label in place.
    for (store_axis, phys_axis) in [(lon_axis, 1usize), (lat_axis, 2usize)] {
        if let Some(name) = &store.cname[store_axis] {
            sky_frame.set_axis_label(phys_axis, name.clone())?;
        }
    }
```

`set_axis_label`'s 1-based `axis` is resolved through
`SkyFrame::physical_axis`, so `1`/`2` here address the longitude and
latitude axes even when the frame is permuted — which is what C's
`astSetLabel(ret, 0, ...)` / `(ret, 1, ...)` do.

**Placement matters.** This must run *before* the `lat_axis < lon_axis`
permutation block at `:216` has any chance to be re-applied, and after the
frame exists. Insert exactly where step 3 says; if `sky_frame` is not in
scope at `:276`, put the loop at the first point after construction where
both `sky_frame` and `lon_axis`/`lat_axis` are live, and note the actual
line in the commit message.

- [ ] **Step 4: Run both tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_read cname
```

Expected: both PASS.

- [ ] **Step 5: Re-measure `cname`**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep -E 'cname|^test result'
```

Expected: `cname_native` no longer diffs at line 34. If a later diff
appears, record it and keep the exclusion entry.

- [ ] **Step 6: Remove the `cname` exclusion entry if it is byte-exact**

- [ ] **Step 7: Regression sweep**

```bash
cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core --test test_transform_oracle head_sections
```

Expected: 192 passed; 66 `test_fitschan*`; `HEAD_ORACLE_EXCLUDED` still
empty.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_celestial.rs \
        crates/ast-core/tests/test_fitschan_read.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: CNAME supplies the SkyFrame axis Label on the celestial read path (cname)"
```

---

#### Sub-fix C: the GLS projection description survives the GLS→SFL rewrite

**Case:** `class-noproj`.

**Measured.** `expected/class-noproj.native` has `PROJ_A = 'sinusoidal'`;
we emit `PROJ_A = 'Sanson-Flamsteed'`. Both strings are already correct in
Rust — they are the two adjacent rows of C's `PrjInfo` table
(wcsmap.c:698, :712):

```c
{ AST__SFL,  0, 4, "Sanson-Flamsteed", "-SFL",  astSFLfwd, astSFLrev, 0.0 },
{ AST__GLS,  0, 4, "sinusoidal", "-GLS",  astSFLfwd, astSFLrev, 0.0 },
```

mirrored at `wcslib.rs:152` / `:166` (and duplicately at `wcsmap.rs:897` /
`:911`). GLS and SFL are the *same projection math* — identical `fwd`/`rev`
function pointers — distinguished only by their name and description. So
this is purely about which of the two enum variants is live when the
description is read.

**The input has a blank projection code:**

```
CTYPE2  = 'RA--    '           / RA with blank projection (CLASS style)
CTYPE3  = 'DEC-    '           / Dec with blank projection (CLASS style)
```

**C's chain, and why `prj` stays GLS.** Three things happen in a fixed
order inside `SpecTrans`:

1. **fitschan.c:31252-31275** — at the *top* of the per-version loop,
   `prj` is captured from the CTYPE cards as they stand on entry:
   `strncpy( prj, cval + 4, 4 )`. For a blank-projection CTYPE that
   captures blanks.
2. **fitschan.c:31856** — `ClassTrans` runs, and at fitschan.c:7191-7212
   rewrites the blank code to `-GLS` in the *card list*:

   ```c
   /* If no projection code is supplied for the longitude and latitude axes,
      use "-GLS". This will be translated to "-SFL" by SpecTrans. */
   ```
3. **fitschan.c:31931** — the `-GLS` → `-SFL` rewrite, again in the card
   list, gated on `!Ustrcmp( prj, "-GLS", status )`.

Step 3's guard tests the `prj` *captured at step 1*, which for
`class-noproj` is blanks, not `-GLS`. **So for a blank-projection CLASS
header, C's step-3 rewrite never fires.** The cards keep `-GLS`, and when
`WcsCelestial` later re-derives the projection from the CTYPE
(fitschan.c:35933, `prj = astWcsPrjType( ctype + ctlen - 4 )`) it gets
`AST__GLS`. That is the value `astSetProjection( ret, astWcsPrjDesc( prj ) )`
reads at fitschan.c:39495, hence `'sinusoidal'`.

Contrast `inputs/gls.head`, which supplies `-GLS` explicitly
(`CTYPE1 = 'GLON-GLS'`): there step 1 captures `-GLS`, step 3 *does* fire,
the cards become `-SFL`, and `expected/gls.native` correctly reads
`PROJ_A = 'Sanson-Flamsteed'`. Both fixtures are in the corpus and they
pin both sides of the guard — `gls_native` (manifest row 78) currently
passes and must keep passing.

**Rust's two divergences.** Both need fixing:

1. **`spec_trans.rs::translate_ncp_gls` (`:1459-1514`) re-derives `prj`
   from the cards instead of using the entry-time capture.** It reads
   `lon_ctype` fresh (`:1473`) and computes `projection_suffix` on it
   (`:1479`), so by the time it runs, `translate_class` has already put
   `-GLS` there and the GLS branch fires when C's would not. Worse, the
   ordering in `spec_trans_inplace` is inverted relative to C:
   `translate_ncp_gls` is called at `:102` but `translate_class` at `:169`
   — so on the *first* pass the cards still hold blanks and the branch does
   not fire. Confirm which of the two actually bites for this fixture
   before writing the fix; the ordering may already give C's answer here by
   accident, in which case only divergence 2 needs work.
2. **`enc_class.rs::translate_class_projection_read` (`:327-347`)
   unconditionally maps GLS→SFL on the read path** (called at `:74`, as
   each CTYPE is copied into the store):

   ```rust
   let new_proj = match proj.to_uppercase().as_str() {
       "GLS" => "SFL",
       "ATF" => "AIT",
       _ => return ctype.to_string(),
   };
   ```

   **C has no such translation.** C's CLASS read path is
   `SpecTrans` → `WcsToStore` (fitschan.c:11553-11567) — the same
   `WcsToStore` every non-DSS encoding uses. There is no CLASS-specific
   ToStore; `CLASSFromStore` (fitschan.c:11415) is the *write* path only.
   Every CLASS-specific read rewrite lives in `ClassTrans`, which is called
   from inside `SpecTrans`. So `translate_class_projection_read` is a
   Rust-only invention that duplicates — and here overrides — what
   `SpecTrans` is supposed to decide.

   Per the standing rule that C's logic beats existing Rust shape, the
   right fix is to delete this read-side translation and let `SpecTrans`
   own the decision, exactly as C does. The ATF→AIT half is likewise
   already handled by `translate_atf_to_ait` (`spec_trans.rs:481-501`), so
   deleting the whole function loses nothing — but `atf_native` (manifest
   row 88, expects `PROJ_A = 'Hammer-Aitoff'`) must keep passing, which is
   the check that the `SpecTrans` leg really does cover it.

   Leave `translate_class_projection_write` (`:352`) alone — the write path
   genuinely does need SFL→GLS and AIT→ATF, and C does that in
   `CLASSFromStore`.

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_class.rs:74` (drop the call),
  `:327-347` (delete `translate_class_projection_read`)
- Possibly modify: `crates/ast-core/src/fitschan/spec_trans.rs:1459-1514`
  (`translate_ncp_gls` — only if step 1 shows the re-derived `prj` bites)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove 1 exclusion entry)
- Create: `crates/ast-core/tests/test_fitschan_class.rs` (new file; no `test_fitschan_class.rs` exists today — the existing FitsChan tests are `test_fitschan.rs`, `test_fitschan_encodings.rs`, `test_fitschan_native_keyword.rs`, `test_fitschan_wcs.rs` and `test_fitschan_wcs_write.rs`)

**Interfaces:**
- Consumes: `WcsProjection::description()` (`wcslib.rs:137`),
  `WcsProjection::from_code()` (`wcslib.rs:177`),
  `FitsStore::projection_code(axis)` (`fits_store.rs:260`).
- Produces: no new public API. `translate_class_projection_read` is
  **removed**; nothing outside `enc_class.rs:74` calls it (verified by
  grep — the only two references are its definition and that one call).

- [ ] **Step 1: Establish which divergence actually bites**

Print the CTYPE the store ends up with, and the projection variant derived
from it, for both fixtures:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo build -p ast-core --example wcsconv_debug
BIN=$(find target* -name 'wcsconv_debug' -type f | head -1)
for f in class-noproj gls atf; do
  echo "=== $f"
  "$BIN" crates/ast-core/tests/fixtures/wcsconv/inputs/$f.head native 2>&1 \
    | grep -iE 'PROJ|ctype'
done
```

Record which of the two divergences produces the wrong description. If
`translate_ncp_gls` already gives C's answer by accident (because
`spec_trans_inplace` calls it before `translate_class`), the fix is
divergence 2 alone and you should say so in the commit message rather than
"fixing" ordering that is already producing the right result. Do not
reorder `spec_trans_inplace` speculatively — a reorder changes 24
translations' interaction and needs its own justification.

- [ ] **Step 2: Write the failing test**

Create `crates/ast-core/tests/test_fitschan_class.rs` with this header and
these two tests:

```rust
//! CLASS-encoding FITS read tests.
//!
//! CLASS headers reach the reader through `enc_class.rs`, whose projection
//! handling differs from the FITS-WCS path: a blank projection code is
//! rewritten rather than rejected.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::frames::{CmpFrame, Frame, SkyFrame};
```

then:

```rust
/// A CLASS header with a blank projection code gets `-GLS` from
/// `ClassTrans` (fitschan.c:7191-7212), and that `-GLS` survives: the
/// `-GLS` → `-SFL` rewrite at fitschan.c:31931 is gated on the projection
/// code captured at the *top* of the SpecTrans loop (fitschan.c:31252-31275),
/// which for a blank CTYPE is blank, not `-GLS`. So the frame reports the
/// GLS description "sinusoidal", not the SFL description
/// "Sanson-Flamsteed". The two are the same projection math
/// (wcsmap.c:698, :712 share `astSFLfwd`/`astSFLrev`) and differ only in
/// name.
#[test]
fn blank_class_projection_keeps_the_gls_description() {
    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::FitsClass);
    for card in [
        "NAXIS   =                    3",
        "NAXIS1  =                 1024",
        "NAXIS2  =                    1",
        "NAXIS3  =                    1",
        "CTYPE1  = 'FREQ    '",
        "CTYPE2  = 'RA--    '",
        "CTYPE3  = 'DEC-    '",
        "CRVAL1  =                  0.0",
        "CRVAL2  =               40.738",
        "CRVAL3  =               62.933",
        "CRPIX1  =              512.500",
        "CRPIX2  =                  0.5",
        "CRPIX3  =                  0.5",
        "CDELT1  =              39062.5",
        "CDELT2  =              -0.0100",
        "CDELT3  =               0.0100",
        "EQUINOX =               2000.0",
        "RADESYS = 'FK5     '",
        "RESTFREQ=       1.102013543E11",
    ] {
        fc.put_fits(card, false).unwrap();
    }
    fc.rewind();
    let obj = fc.read().unwrap().expect("CLASS header should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("expected a FrameSet, got {obj:?}");
    };
    let sky = find_skyframe(fs.current()).expect("a SkyFrame component");
    assert_eq!(sky.projection(), Some("sinusoidal"));
}

/// The companion: an *explicit* `-GLS` does hit the fitschan.c:31931
/// rewrite, becomes `-SFL`, and reports the SFL description. This is the
/// other side of the same guard and is what `inputs/gls.head` covers in
/// the wcsconv corpus.
#[test]
fn explicit_gls_projection_becomes_sfl() {
    let mut fc = FitsChan::new();
    for card in [
        "NAXIS   =                    2",
        "NAXIS1  =                  100",
        "NAXIS2  =                  100",
        "CTYPE1  = 'GLON-GLS'",
        "CTYPE2  = 'GLAT-GLS'",
        "CRVAL1  =                 10.0",
        "CRVAL2  =                 20.0",
        "CRPIX1  =                 50.5",
        "CRPIX2  =                 50.5",
        "CDELT1  =                -0.01",
        "CDELT2  =                 0.01",
    ] {
        fc.put_fits(card, false).unwrap();
    }
    fc.rewind();
    let obj = fc.read().unwrap().expect("header should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("expected a FrameSet, got {obj:?}");
    };
    let sky = fs.current().as_any().downcast_ref::<SkyFrame>().unwrap();
    assert_eq!(sky.projection(), Some("Sanson-Flamsteed"));
}
```

`find_skyframe` is a private helper this new file needs — the CLASS header
produces `CmpFrame(SkyFrame, SpecFrame)`, so the SkyFrame is a component, not
the current frame:

```rust
/// The first `SkyFrame` in `frame`, descending through `CmpFrame` components.
fn find_skyframe(frame: &dyn Frame) -> Option<&SkyFrame> {
    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        return Some(sky);
    }
    let cmp = frame.as_any().downcast_ref::<CmpFrame>()?;
    find_skyframe(cmp.frame1()).or_else(|| find_skyframe(cmp.frame2()))
}
```

Returning `Option` is what the test above `.expect`s on. Searching both
components rather than assuming the SkyFrame is first keeps the helper valid
whichever order `enc_class.rs` composes the CmpFrame in.

Confirm `CmpFrame`'s component accessor names before writing `frame1()` /
`frame2()` — `grep -n 'pub fn frame' crates/ast-core/src/frames/cmpframe.rs`.

`SkyFrame::projection` (`skyframe.rs:613`) returns `Option<&str>`, hence the
`Some(...)` on the right-hand side of both assertions — an unset Projection is
`None`, which is a third outcome distinct from either description and worth
distinguishing when a test fails.

- [ ] **Step 3: Run both tests to verify the first fails**

```bash
cargo test -p ast-core --test test_fitschan_class gls -- --nocapture
```

Expected: `blank_class_projection_keeps_the_gls_description` FAILS with
`'Sanson-Flamsteed'` where `'sinusoidal'` is expected.
`explicit_gls_projection_becomes_sfl` should PASS already — it is the guard
that the fix does not break the explicit-GLS side.

- [ ] **Step 4: Delete the read-side CLASS projection translation**

In `crates/ast-core/src/fitschan/enc_class.rs`, change `:72-75` from:

```rust
        // CTYPE — translate CLASS-specific projection codes
        let key = format!("CTYPE{axis_num}");
        if let Some(ctype_raw) = cards.get_string_marked(&key) {
            store.ctype[i] = Some(translate_class_projection_read(&ctype_raw));
        }
```

to:

```rust
        // CTYPE. CLASS-specific projection rewrites (blank → -GLS,
        // -ATF → -AIT) are applied by SpecTrans before this reader runs,
        // mirroring C: the CLASS read path is SpecTrans → WcsToStore
        // (fitschan.c:11553-11567), with every CLASS-specific rewrite
        // living in ClassTrans (fitschan.c:7084). There is no
        // CLASS-specific ToStore.
        let key = format!("CTYPE{axis_num}");
        store.ctype[i] = cards.get_string_marked(&key);
```

and delete the `translate_class_projection_read` function
(`:322-347`, including its doc comment). Leave
`translate_class_projection_write` in place.

- [ ] **Step 5: Run both tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_class gls
```

Expected: both PASS. If `explicit_gls_projection_becomes_sfl` now fails,
`translate_ncp_gls` was relying on the deleted translation and divergence 1
does need work — go back to step 1's finding and fix
`translate_ncp_gls` to use an entry-time `prj` capture, mirroring C's
fitschan.c:31252-31275.

- [ ] **Step 6: Re-measure `class-noproj`, and check the two projection fixtures**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep -E 'class-noproj|class_noproj|gls_native|atf_native|^test result'
```

Expected: `class_noproj_native` no longer diffs at `PROJ_A`; `gls_native`
and `atf_native` still pass. Those two are the regression guard for this
sub-fix specifically — `atf_native` proves
`spec_trans::translate_atf_to_ait` really does cover the ATF leg that the
deleted function was also handling.

- [ ] **Step 7: Remove the `class-noproj` exclusion entry if it is byte-exact**

- [ ] **Step 8: Regression sweep**

```bash
cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core --test test_transform_oracle head_sections
```

Expected: 192 passed; 66 `test_fitschan*`; `HEAD_ORACLE_EXCLUDED` still
empty. The CLASS write path is untouched, but run the whole
`test_fitschan*` set anyway — `enc_class.rs` carries both directions.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/enc_class.rs \
        crates/ast-core/src/fitschan/spec_trans.rs \
        crates/ast-core/tests/test_fitschan_class.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: let SpecTrans own the CLASS projection rewrite so a blank code keeps the GLS description (class-noproj)"
```

---

#### Sub-fix D: the FITS read path always sets `System` on the SkyFrame it builds

**Cases:** `origin`, `sip-alt`, `alt-spectral`.

**Measured.** All three first diffs are the same missing card, and in all
three the *value* we hold is already right — only the set-ness differs:

| case | our line | expected line |
|---|---|---|
| `origin` | L31 `AX1_B   = '        '` | L31 `SYSTEM_A= 'ICRS    '` |
| `sip-alt` | L51 `AX1_C   = '        '` | L51 `SYSTEM_B= 'ICRS    '` |
| `alt-spectral` | L81 `AX1_D   = '        '` | L81 `SYSTEM_C= 'ICRS    '` |

`wcsconv_debug` stderr reports `sys=Icrs` for the relevant frame in each
case, so the reader picked ICRS correctly. We emit it as a commented
default (`#  System = "ICRS"`, which the harness's blank-line/normalize
pass does not turn into a card) instead of as a live `SYSTEM_x` card.

**Do not read this as "ICRS is special in C".** It is special only in
*Rust*. C `WcsSkyFrame` (fitschan.c:39422-39451) constructs every SkyFrame
from an explicit attribute string — there is no "leave it defaulted"
branch anywhere in the function:

```c
/* Create a SkyFrame for the specified system. */
   if( !strcmp( sys, "E" ) ){
      ret = astSkyFrame( "System=Ecliptic", status );
   } else if( !strcmp( sys, "H" ) ){
      ret = astSkyFrame( "System=Helioecliptic", status );
   } else if( !(strcmp( sys, "G" ) ) ){
      ret = astSkyFrame( "System=Galactic", status );
   } else if( !(strcmp( sys, "S" ) ) ){
      ret = astSkyFrame( "System=Supergalactic", status );
   } else if( !(strcmp( sys, "AZL" ) ) ){
      ret = astSkyFrame( "System=AzEl", status );
   } else if( !(strcmp( sys, "EQU" ) ) ){
/* For equatorial systems, the specific system is given by the RADESYS
   value. */
      if( radesys == FK4 ){
         ret = astSkyFrame( "System=FK4", status );
      } else if( radesys == FK4NOE ){
         ret = astSkyFrame( "System=FK4-NO-E", status );
      } else if( radesys == FK5 ){
         ret = astSkyFrame( "System=FK5", status );
      } else if( radesys == ICRS ){
         ret = astSkyFrame( "System=ICRS", status );
      } else if( radesys == GAPPT ){
         ret = astSkyFrame( "System=GAPPT", status );
      } else if( astOK ){
         astError( AST__INTER, ... );
      }
   ...
   } else if( astOK ){
      report = 1;
      ret = astSkyFrame( "System=UNKNOWN", status );
```

`astSkyFrame("System=ICRS")` runs `astSetSystem`, so `astTestSystem` is
true and the Dump writes an active card. Every other branch is the same
shape, including the `UNKNOWN` fallback — which is why `offset-sky` (a
`System=UNKNOWN` frame) already gets an active `SYSTEM_A= 'Unknown '` from
us: `SkyFrame::new(SkySystem::Unknown)` marks the flag.

**The corpus-wide invariant, which is the real evidence.** Every SkyFrame
block in every expected `.native` file carries an active `SYSTEM` card —
there is not one exception in 139 files:

```bash
cd crates/ast-core/tests/fixtures/wcsconv/expected
awk '/^BEGAST[A-Z_]*= .SkyFrame/{inblk=1;sys=0}
     inblk&&/^SYSTEM[A-Z_0-9]*=/{sys=1}
     /^ENDAST[A-Z_]*= .SkyFrame/{if(inblk&&!sys)print FILENAME" NR="NR; inblk=0}' *.native
```

Prints nothing. Only five of those files mention ICRS at all — `origin:32`,
`axref:52`, `multi-wcs:52`, `sip-alt:52`, `alt-spectral:82` — and each of
the five has it as a live card. So the defect is invisible except on ICRS
inputs, but the rule it violates is universal.

**Rust, and where the fix does *not* go.** `SkyFrame::new`
(`skyframe.rs:149-154`) deliberately singles ICRS out:

```rust
            // ICRS is the SkyFrame default (mirrors C: AST__ICRS is the
            // default System). Only mark the attribute as explicitly
            // set when the caller picks something else; the matching
            // serialisation treats ICRS as the "unset default".
            system_set: !matches!(system, SkySystem::Icrs),
```

and the dump gates on that flag (`skyframe.rs:2497-2510`). **Do not change
`SkyFrame::new`.** Its ICRS behavior is the documented contract of
`SkyFrame::unset_system` (`skyframe.rs:113-124`):

```rust
    pub fn unset_system() -> Self {
        // Re-use SkyFrame::new(Icrs) — that branch already leaves
        // `system_set = false` for ICRS. Mirrors C `astSkyFrame("")`.
        Self::new(SkySystem::Icrs)
    }
```

and `enc_class.rs:581` is a live non-test consumer that needs exactly that
"unset template" semantics — a CLASS header with no RADESYS must produce a
SkyFrame whose System is *genuinely* unset so a later `astSet` can override
it without a set-ness clash. Changing the constructor breaks that.

The fix belongs at the FITS reader's construction site, which is the direct
analogue of C's `astSkyFrame("System=...")` string:
`wcs_celestial.rs:215`, `let mut sky_frame = SkyFrame::new(system);`.
Follow it with an explicit `set_system(system)` so the flag is set
regardless of which variant `determine_sky_system` picked. That is a
faithful transcription of C: the attribute string always names a System, so
`astSetSystem` always runs.

`SkyFrame::set_system` (`skyframe.rs:346-349`) is the right call — it
assigns the value and raises `system_set`. It is not a no-op even when the
value is unchanged, which is the whole point.

**Write-path exposure to check, not assume.** Two sites read
`SkyFrame::test_system`:

- `fset_to_store/mod.rs:408-419` saves `(test_system, system, test_equinox,
  equinox_mjd)` before the FK5/J2000 fiducial round-trip and restores only
  what was set. With the flag now true for an ICRS read-path frame, the
  restore path becomes a `set_system(Icrs)` instead of leaving it unset —
  which is what C does (C's `MakeFitsFrameSet`, fitschan.c:21557-21570,
  restores via `astSetC` guarded by `astTest`, and on a C read-path frame
  `astTestSystem` is likewise true).
- `sky_sys.rs:246` reads the *value* via `classify_system(sys)`, not the
  set-ness, so RADESYS/EQUINOX emission is unaffected.

So no write-path change is expected — but the five ICRS fixtures have
`fits-wcs` rows as well as `native` rows, and those rows are the guard.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:215`
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove 3 exclusion
  entries: `origin_native`, `sip-alt_native`, `alt-spectral_native`)
- Test: `crates/ast-core/tests/test_fitschan_read.rs`

**Interfaces:**
- Consumes: `SkyFrame::new(system: SkySystem) -> SkyFrame`
  (`skyframe.rs:132`), `SkyFrame::set_system(&mut self, sys: SkySystem)`
  (`skyframe.rs:346`), `SkyFrame::test_system(&self) -> bool`
  (`skyframe.rs:353`).
- Produces: no new public API. `SkyFrame::new` and
  `SkyFrame::unset_system` keep their current semantics unchanged.

- [ ] **Step 1: Confirm the corpus-wide invariant before writing code**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs/crates/ast-core/tests/fixtures/wcsconv/expected
awk '/^BEGAST[A-Z_]*= .SkyFrame/{inblk=1;sys=0}
     inblk&&/^SYSTEM[A-Z_0-9]*=/{sys=1}
     /^ENDAST[A-Z_]*= .SkyFrame/{if(inblk&&!sys)print FILENAME" NR="NR; inblk=0}' *.native
echo "violations above (none expected)"
grep -l "ICRS" *.native
```

Expected: the awk block prints nothing, and the `grep -l` prints exactly
`alt-spectral.native`, `axref.native`, `multi-wcs.native`, `origin.native`,
`sip-alt.native`. If the awk block prints any file, stop — the invariant is
not universal and the unconditional `set_system` is the wrong fix.

- [ ] **Step 2: Write the failing test**

Add to `crates/ast-core/tests/test_fitschan_read.rs`:

```rust
/// C `WcsSkyFrame` builds every SkyFrame from an explicit attribute
/// string — `astSkyFrame("System=ICRS")` for an ICRS header, exactly as it
/// uses `astSkyFrame("System=FK5")` for an FK5 one (fitschan.c:39422-
/// 39321). So `astTestSystem` is true on a SkyFrame read from FITS
/// whatever the system, and the Dump writes an active card. ICRS is the
/// only value for which Rust's `SkyFrame::new` leaves the flag clear, so
/// it is the only value that exposes the gap.
#[test]
fn fits_read_sets_system_even_when_the_value_is_the_icrs_default() {
    let header = "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                  100
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
CRVAL1  =                 10.0
CRVAL2  =                 20.0
CRPIX1  =                 50.5
CRPIX2  =                 50.5
CDELT1  =                -0.01
CDELT2  =                 0.01
RADESYS = 'ICRS    '
";
    let obj = FitsChan::from_header(header)
        .unwrap()
        .read()
        .unwrap()
        .expect("ICRS header should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame");
    assert_eq!(sky.system(), SkySystem::Icrs);
    assert!(
        sky.test_system(),
        "System must be explicitly set on a SkyFrame built by the FITS reader"
    );
}

/// The FK5 companion, which already passes. It pins the other side: the
/// unconditional `set_system` must not change behaviour for the systems
/// that were already correct.
#[test]
fn fits_read_sets_system_for_a_non_default_value_too() {
    let header = "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                  100
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
CRVAL1  =                 10.0
CRVAL2  =                 20.0
CRPIX1  =                 50.5
CRPIX2  =                 50.5
CDELT1  =                -0.01
CDELT2  =                 0.01
RADESYS = 'FK5     '
EQUINOX =               2000.0
";
    let obj = FitsChan::from_header(header)
        .unwrap()
        .read()
        .unwrap()
        .expect("FK5 header should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .unwrap();
    assert_eq!(sky.system(), SkySystem::Fk5);
    assert!(sky.test_system());
}

/// `SkyFrame::unset_system` must keep its "genuinely unset" contract —
/// `enc_class.rs` relies on it to build a template a later `astSet` can
/// override. This test fails if the fix is (wrongly) put in
/// `SkyFrame::new` instead of at the reader's construction site.
#[test]
fn unset_system_still_leaves_system_unset() {
    let sky = SkyFrame::unset_system();
    assert_eq!(sky.system(), SkySystem::Icrs);
    assert!(!sky.test_system());
}
```

- [ ] **Step 3: Run the tests to verify the first one fails**

```bash
cargo test -p ast-core --test test_fitschan_read \
  fits_read_sets_system -- --nocapture
cargo test -p ast-core --test test_fitschan_read unset_system_still
```

Expected: `fits_read_sets_system_even_when_the_value_is_the_icrs_default`
FAILS on the `assert!(sky.test_system(), ...)` line;
`fits_read_sets_system_for_a_non_default_value_too` and
`unset_system_still_leaves_system_unset` PASS.

- [ ] **Step 4: Set System explicitly at the reader's construction site**

In `crates/ast-core/src/fitschan/wcs_celestial.rs`, replace line 215:

```rust
    let mut sky_frame = SkyFrame::new(system);
```

with:

```rust
    // C `WcsSkyFrame` builds the SkyFrame from an attribute string that
    // always names a System — `astSkyFrame("System=ICRS")` no less than
    // `astSkyFrame("System=FK5")`, and `astSkyFrame("System=UNKNOWN")`
    // for an unrecognized prefix (fitschan.c:39422-39451). So
    // `astTestSystem` is true on every SkyFrame the FITS reader produces.
    // `SkyFrame::new` leaves the attribute unset when the value happens
    // to be the ICRS default (that behaviour is `unset_system`'s
    // contract), so set it explicitly here.
    let mut sky_frame = SkyFrame::new(system);
    sky_frame.set_system(system);
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_read \
  fits_read_sets_system
cargo test -p ast-core --test test_fitschan_read unset_system_still
```

Expected: all three PASS.

- [ ] **Step 6: Re-measure the three target cases**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv \
  -- origin_native sip_alt_native alt_spectral_native --nocapture 2>&1 \
  | grep -E "^(test |first diff|line count)" | head -40
```

Expected: `origin_native` and `sip-alt_native` byte-exact.
`alt-spectral_native` may reveal a second diff further down — it is a
`CmpFrame(SkyFrame, SpecFrame)` and the SpecFrame half is untouched here.
If it does, record the new first diff in the re-measure table and leave its
exclusion entry in place with the *new* reason; do not stretch this commit
to chase it.

- [ ] **Step 7: Check the five ICRS fixtures' `fits-wcs` rows still pass**

The write path reads `test_system` at `fset_to_store/mod.rs:414`, so these
are the rows that would catch a regression:

```bash
cargo test -p ast-core --test test_wcsconv -- \
  origin sip_alt alt_spectral axref multi_wcs --nocapture 2>&1 | tail -20
```

Expected: every non-native row for those five names passes as before. If a
`fits-wcs` row breaks, the save/restore at `fset_to_store/mod.rs:408-419`
is now restoring a System it previously left alone — compare against C
`MakeFitsFrameSet` (fitschan.c:21557-21570), which restores under
`astTest`, and fix the Rust site to match rather than reverting step 4.

- [ ] **Step 8: Remove the now-passing exclusion entries**

In `crates/ast-core/tests/test_wcsconv.rs`, delete the entries that step 6
showed byte-exact:

```rust
    ("origin_native", "SYSTEM_A: system"),
    ("sip-alt_native", "SYSTEM_B: alt-plane frameset shape"),
    ("alt-spectral_native", "SYSTEM_C: spectral system"),
```

Note the last reason string is misleading — the diff was never about the
spectral system. If `alt-spectral_native` is still excluded after step 6,
replace its reason with the measured new first diff rather than leaving the
stale text.

- [ ] **Step 9: Regression sweep**

```bash
cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core --test test_transform_oracle head_sections
cargo test -p ast-core --test test_native_roundtrip_all
```

Expected: 192 passed; the `test_fitschan*` set green (the CLASS
`unset_system` path is covered there); `HEAD_ORACLE_EXCLUDED` still empty;
the 139-object native round-trip still byte-exact. That last one matters
here: it round-trips `.ast` inputs through the *serializer* only, so a
System set-ness change that leaked into `from_node` would show up there
rather than in wcsconv.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_celestial.rs \
        crates/ast-core/tests/test_fitschan_read.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: the FITS reader sets System on every SkyFrame it builds, ICRS included (origin, sip-alt)"
```

---

#### Sub-fix E: unrecognized axes get C's CTYPE-derived plain-Frame `Domain`

**Cases:** `unknown-spec`, `axes-swap`.

**Measured.** Each case is missing exactly one card, and in both it is a
`Domain` on the *plain* Frame that holds the axes C could not classify:

| case | our line | expected line | missing card |
|---|---|---|---|
| `unknown-spec` | L26 `AX1_B   = '        '` | L26 `DOMAIN_B= 'XXXX-WAV'` | `DOMAIN_B` |
| `axes-swap` | L48 `ACTUNT_A= 0` | L48 `DOMAIN_B= 'STOKES  '` | `DOMAIN_B` |

`unknown-spec.head` has `CTYPE1 = 'XXXX-WAV'` (a spectral-looking code AST
does not recognize); `axes-swap.head` has a `CTYPE = 'STOKES'` axis
alongside recognized sky and spectral axes. In both, the expected Domain is
just the CTYPE value.

`axes-swap` has a visible knock-on: our outer CmpFrame reports
`#  Domain = "-SKY-SPECTRUM"` where C has `"STOKES-SKY-SPECTRUM"`. That
needs no separate fix. `CmpFrame::get_domain` (`cmpframe.rs:395-399`)
composes children with `format!("{dom1}-{dom2}")`, so an empty child Domain
produces the leading `-`. Fix the child and the parent follows.

**C.** Three sites, in execution order.

`WcsMapFrm` seeds the physical Frame with a sentinel Domain before any of
the four class stages run (fitschan.c:37947-37960):

```c
   *frm = astFrame( wcsaxes, "Domain=AST_FITSCHAN", status );

/* Store the Frame's Ident attribute as the version character. */
   id[ 0 ] = s;
   id[ 1 ] = 0;
   astSetIdent( *frm, id );
```

`WcsCelestial`, `WcsSpectral` and the rest overwrite the Domain of the axes
they claim (by building typed sub-Frames), so after they run, the axes still
carrying `AST_FITSCHAN` are exactly the unclassified ones. `WcsOthers`
(fitschan.c:38977-39077) walks them, builds `newdom` by joining the CTYPE
values with `-`, and sets or clears the Domain:

```c
/* Loop round all axes, looking for axes which are still described by
   the sentinel Domain. */
      newdom = NULL;
      nc = 0;
      for( i = 0; i < naxes; i++ ){
         astPrimaryFrame( *frm, i, &pfrm2, &paxis );
         if( !strcmp( astGetDomain( pfrm2 ), "AST_FITSCHAN" ) ) {
            ...
            cval = GetItemC( &(store->ctype), i, 0, s, NULL, method, class, status );
            if( cval ) {
               ...
/* Remove any trailing "-LOG" string. */
               ...
               if( nc ) newdom = astAppendString( newdom, &nc, "-" );
               newdom = astAppendString( newdom, &nc, buf );
            }
         }
         pfrm2 = astAnnul( pfrm2 );
      }
...
      if( newdom ) {
         astSetDomain( pfrm2, newdom );
      } else {
         astClearDomain( pfrm2 );
      }
```

And `WcsMapFrm` clears any sentinel that survived all four stages
(fitschan.c:37993-37995), so the sentinel never escapes into the returned
object:

```c
/* If the Frame still has the Domain "AST_FITSCHAN", clear it. */
   cc = astGetDomain( *frm );
   if( cc && !strcmp( cc, "AST_FITSCHAN" ) ) astClearDomain( *frm );
```

`IDENT_A = '" "'` in the expected files traces to :37829 — that is U10's
territory, not this sub-fix's, but it is the same seeded Frame.

**Rust.** Nothing of this is ported. `wcs_read.rs:748-843` builds the
physical Frame two ways, neither of which touches Domain:

```rust
    if celestial.is_none() && spectral.is_none() {
        // All-linear: a single BasicFrame carrying the per-axis CTYPE info.
        return seed_linear_frame(store, &(0..naxis).collect::<Vec<_>>());
    }
    ...
    let mut frame: DynFrame = seed_linear_frame(store, &(0..naxis).collect::<Vec<_>>())?;
```

```rust
fn seed_linear_frame(store: &FitsStore, axes: &[usize]) -> AstResult<DynFrame> {
    let mut builder = BasicFrameBuilder::new(axes.len());
    for (k, &axis) in axes.iter().enumerate() {
        let idx_1based = k + 1;
        if let Some(ctype) = &store.ctype[axis] {
            builder = builder.symbol(idx_1based, ctype.clone());
        }
        ...
    }
    Ok(Box::new(builder.build()?))
}
```

`BasicFrameBuilder::domain` (`basic.rs:119`) is the hook. `seed_linear_frame`
is already called with the exact axis list — the axes going into this
plain Frame — so the sentinel/`astPrimaryFrame` walk is not needed to
*identify* them: the caller has already partitioned them. Port C's
`newdom` construction (CTYPE values, `-LOG` stripped, joined with `-`) and
set it as the Domain of the Frame `seed_linear_frame` returns, clearing it
(i.e. not setting it) when no axis in the list has a CTYPE.

That is a faithful port of the *effect* of C's loop, not a shortcut around
it: C uses the sentinel because its Frame is built whole and then carved
up in place, so it has to re-derive which axes are left over. Rust builds
the leftover Frame from an explicit axis list, so the partition is already
in hand. Note this reasoning in the code comment so a later reader does
not mistake the absent sentinel for an omission.

**Two scope-outs, each with its C citation.**

`ctype_com` / `usecom` is out of scope. C's `usecom` path (fitschan.c:
38778-38836) would use CTYPE *comments* as axis labels, but :38846 kills it
whenever there is a single unclassified axis:

```c
/* If there is only one "other" axis, we do not need to use the comments
   to distinguish them. */
      if( nother == 1 ) usecom = 0;
```

`unknown-spec` has one unclassified axis and `axes-swap` has one, so the
comment path never fires for either. Rust's read-path `ctype_com` is
unpopulated; leave it that way. A future multi-unclassified-axis fixture
would need it, and that is the right trigger for the work.

The WCSNAME→Domain override is out of scope. C applies it at :38950-38960,
after the `newdom` set, but the only header in the corpus with a WCSNAME on
an alternate version is `multi-wcs-spec.head:34` (`WCSNAMEA='Galactic'`),
whose `A` version has `nother == 0` — so C returns from `WcsOthers` before
reaching the block. Verified by inspecting the header's CTYPEs (all
recognized) against `expected/multi-wcs-spec.native`, which carries no
`DOMAIN` card from this path.

**One unexercised branch to port anyway.** The `-LOG` strip has no fixture:
`inputs/freq-log.head` is the only `-LOG` CTYPE in the corpus
(`CTYPE1 = 'FREQ-LOG'`), and FREQ is a recognized spectral code, so that
axis never reaches `newdom`. Port the strip regardless — "no fixture
exercises it" is not a reason to omit a C branch. Same for the `-` join:
every case in this corpus has a single unclassified axis, so the join is
only ever exercised in its one-element form. Port it as a join.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs:826-843`
  (`seed_linear_frame`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (remove 2 exclusion
  entries: `unknown-spec_native`, `axes-swap_native`)
- Test: `crates/ast-core/tests/test_fitschan_read.rs`

**Interfaces:**
- Consumes: `BasicFrameBuilder::domain(self, domain: &str) -> Self`
  (`basic.rs:119`), `FitsStore::ctype: Vec<Option<String>>`.
- Produces: `fn ctype_domain(store: &FitsStore, axes: &[usize]) ->
  Option<String>` — private to `wcs_read.rs`; returns the `-`-joined,
  `-LOG`-stripped CTYPE concatenation, or `None` when no axis in `axes`
  carries a CTYPE.

- [ ] **Step 1: Write the failing test**

Add to `crates/ast-core/tests/test_fitschan_read.rs`:

```rust
/// C `WcsOthers` gives the leftover plain Frame a Domain built from the
/// CTYPE values of the axes it could not classify, joined with "-"
/// (fitschan.c:38977-39077). A single unrecognized spectral-looking code
/// therefore yields that code verbatim as the Domain.
#[test]
fn unclassified_axis_domain_comes_from_ctype() {
    let header = "\
NAXIS   =                    1
NAXIS1  =                  100
CTYPE1  = 'XXXX-WAV'
CRVAL1  =                  1.0
CRPIX1  =                  1.0
CDELT1  =                  0.1
";
    let obj = FitsChan::from_header(header)
        .unwrap()
        .read()
        .unwrap()
        .expect("unrecognized-CTYPE header should still read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    assert_eq!(fs.current().domain(), "XXXX-WAV");
    assert!(
        fs.current().test_domain(),
        "Domain must be explicitly set, not merely defaulted"
    );
}

/// With no CTYPE at all there is nothing to build a Domain from, so C
/// takes the `astClearDomain` branch (fitschan.c:39075) and the Frame is
/// left with its default. The sentinel Domain must never escape.
#[test]
fn no_ctype_leaves_domain_unset_and_never_leaks_the_sentinel() {
    let header = "\
NAXIS   =                    1
NAXIS1  =                  100
CRVAL1  =                  1.0
CRPIX1  =                  1.0
CDELT1  =                  0.1
";
    let obj = FitsChan::from_header(header)
        .unwrap()
        .read()
        .unwrap()
        .expect("CTYPE-less header should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    assert!(!fs.current().test_domain());
    assert_ne!(fs.current().domain(), "AST_FITSCHAN");
}

/// Two unclassified axes join with "-", in axis order. C builds `newdom`
/// by appending "-" before each value after the first (fitschan.c:39060).
#[test]
fn two_unclassified_axes_join_their_ctypes_with_a_dash() {
    let header = "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                   10
CTYPE1  = 'AAAA    '
CTYPE2  = 'BBBB    '
CRVAL1  =                  1.0
CRVAL2  =                  2.0
CRPIX1  =                  1.0
CRPIX2  =                  1.0
CDELT1  =                  0.1
CDELT2  =                  0.2
";
    let obj = FitsChan::from_header(header)
        .unwrap()
        .read()
        .unwrap()
        .expect("two unrecognized axes should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    assert_eq!(fs.current().domain(), "AAAA-BBBB");
}

/// C strips a trailing "-LOG" from each CTYPE before appending it
/// (fitschan.c:39047-39055). No wcsconv fixture reaches this branch — the
/// corpus's only -LOG CTYPE is FREQ-LOG, and FREQ is recognized spectral,
/// so that axis never becomes an "other" axis. Covered here directly.
#[test]
fn log_suffix_is_stripped_from_the_domain() {
    let header = "\
NAXIS   =                    1
NAXIS1  =                  100
CTYPE1  = 'ZZZZ-LOG'
CRVAL1  =                  1.0
CRPIX1  =                  1.0
CDELT1  =                  0.1
";
    let obj = FitsChan::from_header(header)
        .unwrap()
        .read()
        .unwrap()
        .expect("-LOG header should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    assert_eq!(fs.current().domain(), "ZZZZ");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p ast-core --test test_fitschan_read \
  -- unclassified_axis_domain no_ctype_leaves two_unclassified log_suffix \
  --nocapture
```

Expected: `unclassified_axis_domain_comes_from_ctype`,
`two_unclassified_axes_join_their_ctypes_with_a_dash` and
`log_suffix_is_stripped_from_the_domain` FAIL on the Domain assertion (we
produce `""`); `no_ctype_leaves_domain_unset_and_never_leaks_the_sentinel`
PASSES already (we never set a Domain at all).

- [ ] **Step 3: Add the `newdom` builder**

In `crates/ast-core/src/fitschan/wcs_read.rs`, above `seed_linear_frame`:

```rust
/// Build the Domain for the Frame holding the axes no class stage claimed.
///
/// C `WcsOthers` (fitschan.c:38977-39077) joins those axes' CTYPE values
/// with `-` separators, stripping a trailing `-LOG` from each, and sets the
/// result as the Frame's Domain — or clears the Domain when no CTYPE is
/// available.
///
/// C identifies the axes by seeding the whole physical Frame with a
/// sentinel `Domain=AST_FITSCHAN` (fitschan.c:37953) and then testing which
/// axes still carry it after the class stages have overwritten the rest.
/// Here the caller already holds the leftover axis list, so the sentinel
/// has nothing left to determine.
fn ctype_domain(store: &FitsStore, axes: &[usize]) -> Option<String> {
    let mut parts: Vec<&str> = Vec::with_capacity(axes.len());
    for &axis in axes {
        let Some(ctype) = store.ctype[axis].as_deref() else {
            continue;
        };
        let ctype = ctype.trim_end();
        let ctype = ctype.strip_suffix("-LOG").unwrap_or(ctype);
        if !ctype.is_empty() {
            parts.push(ctype);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("-"))
    }
}
```

- [ ] **Step 4: Set the Domain in `seed_linear_frame`**

In `seed_linear_frame`, after the per-axis loop and before `build()`:

```rust
    if let Some(domain) = ctype_domain(store, axes) {
        builder = builder.domain(&domain);
    }
    Ok(Box::new(builder.build()?))
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_read \
  -- unclassified_axis_domain no_ctype_leaves two_unclassified log_suffix
```

Expected: all four PASS.

- [ ] **Step 6: Re-measure the two target cases**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv \
  -- unknown_spec_native axes_swap_native --nocapture 2>&1 \
  | grep -E "^(test |first diff|line count)" | head -20
```

Expected: both byte-exact. `axes-swap`'s outer CmpFrame `Domain` should now
read `STOKES-SKY-SPECTRUM` without any change to `cmpframe.rs` — confirm
that in the output rather than assuming it:

```bash
grep -n "SKY-SPECTRUM" \
  crates/ast-core/tests/fixtures/wcsconv/expected/axes-swap.native
```

- [ ] **Step 7: Confirm no WCSNAME leaks into the AIPS pixel outputs**

The write path copies a set current-Frame Domain into `store.wcsname`
(`fset_to_store/mod.rs:266-281`), so the five all-`PIXEL`-CTYPE fixtures now
carry a Domain they did not before. Those fixtures' only rows are
`fits-aips` and `fits-aips++`, and neither encoder emits WCSNAME — in C
(`AIPSFromStore` fitschan.c:3175-3739 and `AIPSPPFromStore` :3666-4300
contain no WCSNAME code) or in Rust (`enc_aips.rs` has its own
`write_to_cards` at :201 and `write_to_cards_pp` at :557, neither of which
mentions it). Verify rather than trust:

```bash
cargo test -p ast-core --test test_wcsconv -- \
  pixel_1d pixel_2d pixel_rotated pixel_shear freq_pixel3d --nocapture 2>&1 \
  | tail -20
grep -c WCSNAME \
  crates/ast-core/tests/fixtures/wcsconv/expected/pixel-2d.fits-aips \
  crates/ast-core/tests/fixtures/wcsconv/expected/pixel-2d.fits-aips++ \
  crates/ast-core/tests/fixtures/wcsconv/expected/pixel-1d.fits-aips \
  crates/ast-core/tests/fixtures/wcsconv/expected/freq-pixel3d.fits-aips
```

Expected: all rows pass, and every `grep -c` reports `0`. If a WCSNAME card
appears in our output, the leak is in `wcs_write/mod.rs:427-455` reaching an
encoding it should not — fix the encoding gate there; do not suppress the
Domain.

- [ ] **Step 8: Remove the now-passing exclusion entries**

In `crates/ast-core/tests/test_wcsconv.rs`, delete:

```rust
    ("unknown-spec_native", "DOMAIN_B: domain"),
    ("axes-swap_native", "DOMAIN_B: nested CmpFrame domain"),
```

- [ ] **Step 9: Regression sweep**

```bash
cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core --test test_transform_oracle head_sections
cargo test -p ast-core --test test_native_roundtrip_all
cargo test -p ast-core --test test_simplify_regression
```

Expected: 192 passed; `test_fitschan*` green; `HEAD_ORACLE_EXCLUDED` still
empty; native round-trip byte-exact; simplify 427. The head oracle is the
meaningful one here — it consumes the same headers through a different
comparison, so a Domain that is fixture-shaped rather than real shows up
there.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_read.rs \
        crates/ast-core/tests/test_fitschan_read.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: build the leftover Frame's Domain from unclassified CTYPEs like C WcsOthers (unknown-spec, axes-swap)"
```

---

### Task 13: U9 — object shape (5 cases)

The spec (cause 9) groups six cases here.
One of them, `cd-cross`, was re-measured into Task 6 (U4) — its only
divergence is an `INVERT` tag left by C's shared-pointer aliasing, which is
the licensed difference U4 installs.
That leaves five, and they are three unrelated defects:

| sub-fix | defect | cases |
|---|---|---|
| A | The `CarLin` attribute does not exist, so C's linear-CAR branch is never taken | `car1`, `car2`, `car4` |
| B | `GetUsedPolyTan`'s two `badpv` warnings are stubs, so an all-zero PV axis is neither reported nor deleted | `tan-zeropv` |
| C | `WinMap`↔`MatrixMap` swap direction differs from C's, giving an algebraically identical tree in a different normal form | `zpx` |

**Standing prohibition, from the spec.**
For the CAR cases the fix is *not* a `WcsMap` CAR `map_merge` that collapses
CAR to a `UnitMap`.
C wcsmap.c:141 records the decision, dated 10-AUG-2000, that `MapMerge`
deliberately does not do this because it would lose the cyclic 2π≡0 property
of a CAR longitude.
Sub-fix A gets its `UnitMap` a different way — C substitutes `AST__WCSBAD`
for the projection type at construction time, and `WcsMap::simplified()`
already collapses a `WcsProjection::Unknown` map to a `UnitMap`
(`wcsmap.rs:699`, mirroring wcsmap.c:3087-3097).

Sub-fix B is gated on Task 11 (U7) sub-fixes A and C: it needs
`push_astwarn`'s insert-at-position behavior and the `pv_max` table, both of
which those sub-fixes deliver.
It adds no projection-parameter machinery of its own.

Sub-fixes A and C are independent of each other and of everything else.

---

#### Sub-fix A: port the `CarLin` attribute and C's linear-CAR branch

**Cases:** `car1`, `car2`, `car4`.

**Measured.**
All three manifest rows carry `carlin=1` — they are the only three in the
corpus that do:

```
car1_native         | car1.head  | car1.native  | native | carlin=1 |
car2_native         | car2.head  | car2.native  | native | carlin=1 |
car4_native         | car4.head  | car4.native  | native | carlin=1 |
```

The recorded first diffs (`WCSCONV_NO_EXCLUDE=1`):

```
car1_native       L44 got BEGAST_H= 'CmpMap  '   exp BEGAST_H= 'WinMap  '
car2_native       L44 got BEGAST_H= 'CmpMap  '   exp BEGAST_H= 'WinMap  '
car4_native       L45 got BEGAST_H= 'CmpMap  '   exp BEGAST_H= 'WinMap  '
```

`test_wcsconv.rs` records all three as "BEGAST_H: WinMap vs CmpMap object
shape", which is accurate as far as it goes.
`load_input` (`test_wcsconv.rs:240-265`) honors only one attribute:

```rust
        for (key, val) in &case.attrs {
            if key.as_str() == "sipreplace" {
                fc.set_sip_replace(val == "1" || val.eq_ignore_ascii_case("true"));
            }
        }
```

`carlin=1` is parsed and silently dropped, because there is no
`set_car_lin`.

`car1.head` and `car2.head` are byte-identical, 2-axis GLON/GLAT-CAR:

```
NAXIS1  =                 2961  / Number pixels on axis 1
NAXIS2  =                  561  / Number pixels on axis 2
CTYPE1  = 'GLON-CAR'            / axis 1 coord type
CRVAL1  =         1.850000e+02  / coord value at CRPIX1
CDELT1  =        -1.250000e-01  / pixel spacing for axis 1
CRPIX1  =             1.000000  / ref pixel for axis 1
CTYPE2  = 'GLAT-CAR'            / axis 2 coord type
CRVAL2  =        -3.500000e+01  / coord value at CRPIX2
CDELT2  =         1.250000e-01  / pixel spacing for axis 2
CRPIX2  =             1.000000  / ref pixel for axis 2
```

`expected/car1.native:38-52` is a single `WinMap`, not a `CmpMap`:

```
PROJ_A  = 'Cartesian'          / Description of sky projection
SREFIS_A= 'Ignored '           / Not rotated (ref. pos. is ignored)
SREF1_A =   3.2288591161895095 / Ref. pos. l 185.0000
SREF2_A =  -0.6108652381980153 / Ref. pos. b -35.0000
ENDAST_F= 'SkyFrame'           / End of object definition
MAP2_A  = '        '           / Mapping between nodes 1 and 2
BEGAST_H= 'WinMap  '           / Map one window on to another
NIN_A   =                    2 / Number of input coordinates
ISSIMP_A=                    1 / Mapping has been simplified
ISA_C   = 'Mapping '           / Mapping between coordinate systems
SFT1_A  =   3.2310407777545023 / Shift for axis 1
SCL1_A  = -0.0021816615649929119 / Scale factor for axis 1
SFT2_A  = -0.61304689976300819 / Shift for axis 2
SCL2_A  = 0.0021816615649929119 / Scale factor for axis 2
```

The numbers confirm the construction before any code is written.
`SREF1_A` = 185° in radians.
`SCL1_A` = CDELT1·(π/180) = −0.125°·d2r.
`SFT1_A − SREF1_A` = 3.2310407777545023 − 3.2288591161895095 =
0.00218166156499 = `|SCL1_A|`, i.e. the `−CRPIX1·CDELT1·d2r` term with
CRPIX1 = 1.0.
So the whole pixel→sky chain collapses to one WinMap with

```
scale = CDELT · d2r
shift = CRVAL · d2r − CRPIX · CDELT · d2r
```

and `ISSIMP_A = 1` because `wcs_read.rs:75` composes with `simplify()`
(which sets `has_been_simplified`) rather than `simplified()`.

`PROJ_A = 'Cartesian'` survives, so the projection *description* must still
come from the real CAR projection even though the *mapping* is built as if
the projection were unknown.

**C, fitschan.c:36013-36028 — `map1` is unaffected.**

```c
   if( latprj != AST__WCSBAD ) {
      mat = (double *) astMalloc( sizeof(double)*naxes );
      for( i = 0; i < naxes; i++ ){
         if( i == axlat || i == axlon ){ mat[ i ] = AST__DD2R;
         } else { mat[ i ] = 1.0; }
      }
      map1 = (AstMapping *) astMatrixMap( naxes, naxes, 1, mat, "", status );
   } else {
      map1 = (AstMapping *) astUnitMap( naxes, " ", status );
   }
```

For a CAR header `latprj == AST__CAR`, which is not `AST__WCSBAD`, so `map1`
stays the deg→rad diagonal `MatrixMap` in both branches of what follows.

**C, fitschan.c:36038-36083 — the branch we do not have.**

```c
   map3 = NULL;
   if( ( latprj == AST__CAR && carlin ) || latprj == AST__WCSBAD ) {
      if( latprj == AST__CAR ) {
         map2 = (AstMapping *) astWcsMap( naxes, AST__WCSBAD, axlon + 1,
                                         axlat + 1, "", status );
      } else {
         map2 = (AstMapping *) astUnitMap( naxes, "", status );
      }
      for( i = 0; i < naxes; i++ ) {
         ina[ i ] = 0.0; inb[ i ] = 1.0; outa[ i ] = 0.0; outb[ i ] = 1.0;
      }
      lonval = GetItem( &(store->crval), axlon, 0, s, NULL, method, class, status );
      if( lonval != AST__BAD ) {
         *reflon = ( latprj == AST__CAR ) ? lonval*AST__DD2R : lonval;
         outa[ axlon ] += *reflon;
         outb[ axlon ] += *reflon;
      } else { outa[ axlon ] = AST__BAD; outb[ axlon ] = AST__BAD; }
      latval = GetItem( &(store->crval), axlat, 0, s, NULL, method, class, status );
      if( latval != AST__BAD ) {
         *reflat = ( latprj == AST__CAR ) ? latval*AST__DD2R : latval;
         outa[ axlat ] += *reflat; outb[ axlat ] += *reflat;
      } else { outa[ axlat ] = AST__BAD; outb[ axlat ] = AST__BAD; }
      map3 = (AstMapping *) astWinMap( naxes, ina, inb, outa, outb, "", status );
```

Three things to note.
`WcsNative` — the native→celestial spherical rotation — is **never called**
in this branch.
The `WinMap` is a pure per-axis shift (`inb − ina == outb − outa == 1.0`, so
unit scale) that adds CRVAL in radians on the two celestial axes.
And `map2` is a `WcsMap` whose projection type is `AST__WCSBAD`, not a
`UnitMap` — the difference matters for the FrameSet→FitsStore write path,
which reads the projection back off the WcsMap.

The `else` branch (fitschan.c:36132-36177) is what we currently always take:
`astInvert( map2 )`, then
`map3 = WcsNative( this, store, s, (AstWcsMap *) map2, -1, -1, ... )`, then a
conditional IWC→PPC fiducial `ShiftMap`.

**C, fitschan.c:36195-36208 — the composition tail is shared.**

```c
      map4 = (AstMapping *) astCmpMap( map1, map2, 1, "", status );
      ret = (AstMapping *) astCmpMap( map4, map3, 1, "", status );
      ...
      sfrm = WcsSkyFrame( this, store, s, prj, wcscelestial_type, axlon,
                          axlat, method, class, status );
```

`WcsSkyFrame` receives `prj` — the real projection, `AST__CAR` — not the
`AST__WCSBAD` substitute given to `astWcsMap`.
That is why `PROJ_A = 'Cartesian'` still appears in the expected file, and
why the existing `set_projection` block (`wcs_celestial.rs:272-278`) needs no
change:

```rust
    if !matches!(proj, WcsProjection::Unknown) {
        sky_frame.set_projection(proj.description());
    }
```

**C, fitschan.c:32142 — the third place `carlin` is read.**

```c
      if( s == ' ' && !Ustrcmp( prj, "-CAR", status ) && !astGetCarLin( this ) ) {
```

This gates `SpecTrans`'s MSX-CAR CRPIX wrap.
`car4` is the header where it matters: `TELESCOP = 'MSX'`,
`CRPIX1 = 211076.0`, `CD1_1 = -0.0016666667`.
With `carlin=1` the wrap must not run.
Arithmetic (python3): `-211076 × -0.0016666667 × d2r = 6.139952076031624`,
matching the expected `SFT1_A = 6.1399520760316237`; the `palDrange`-wrapped
value is `-0.1432332311479625`, matching the `SFT1_A =
-0.14323323114796152` we emit today.
So C keeps the **unwrapped** CRPIX for `car4` — the wrap is suppressed, not
applied differently.

`expected/car4.native:46-54`:

```
BEGAST_H= 'WinMap  '           / Map one window on to another
NIN_A   =                    2 / Number of input coordinates
ISSIMP_A=                    1 / Mapping has been simplified
ISA_C   = 'Mapping '           / Mapping between coordinate systems
SFT1_A  =   6.1399520760316237 / Shift for axis 1
SCL1_A  =  -2.9088821448348579E-5 / Scale factor for axis 1
SFT2_A  = -0.013828651183616224 / Shift for axis 2
SCL2_A  =  2.9088821448348579E-5 / Scale factor for axis 2
```

**C's `CarLin` is tri-state internally; ours is a `bool`.**
fitschan.c:42716-42719:

```c
astMAKE_CLEAR(FitsChan,CarLin,carlin,-1)
astMAKE_GET(FitsChan,CarLin,int,1,(this->carlin == -1 ? 0 : this->carlin))
astMAKE_SET(FitsChan,CarLin,int,carlin,( value ? 1 : 0 ))
astMAKE_TEST(FitsChan,CarLin,( this->carlin != -1 ))
```

`new->carlin = -1` at `:44645`; Dump at `:43775`; astLoad at
`:44899-44900`.
The `-1` exists only so `astTestCarLin` can distinguish "never set" from
"set to 0".
Nothing in the C library reads `astTestCarLin` — grep confirms the only
consumers are `astGetCarLin` at `:32012` and `:35683`.
So a plain `bool` defaulting to `false` is a complete port, exactly as
`sip_replace` is.

**C, fitschan.c:35885 and :35810 — two places C forces `carlin = 1`.**
When a CTYPE has no 4-character projection code, C sets `carlin = 1` for
that axis block regardless of the attribute.
Port both alongside the attribute; they interact with Task 12 sub-fix C
(`class-noproj`), which is the case that has blank projection codes.
Do not skip them because no U9 case needs them — a header with a blank
CTYPE projection and `carlin` unset must still take the linear branch.

**Rust.** Three gaps.

1. No `CarLin` attribute anywhere: no field on `FitsChan`, none on
   `FitsStore`, no setter, and `test_wcsconv.rs` therefore ignores the
   manifest attribute.

2. `wcs_celestial.rs`'s `build_iwc_to_sky_mapping` (`:592-741`)
   unconditionally builds C's `else` branch. Its tail (`:736-738`):

```rust
    let step1 = CmpMap::new(Box::new(zoom), Box::new(wcsmap_inv), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(step1), rotation_naxis, Composition::Series).ok()?;
    Some((Box::new(full), ref_pos))
```

   `rotation_naxis` comes from `build_native_to_celestial_rotation_for_wcsmap`
   (`:711-717`), which is precisely the `WcsNative` call C skips.

3. `translate_msx_car` (`spec_trans.rs:1066`) is ungated, and its doc comment
   says so:

```rust
/// [...] The C code is gated on `astGetCarLin(this)`
/// being false (the default); we mirror this default unconditionally
/// since FitsChan attribute support isn't wired through to
/// `spec_trans` yet.
```

   That comment describes a plan, not the code, and must be rewritten to
   describe the gate once it exists.

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs` — `car_lin: bool` field beside `sip_replace` (`:107`), both defaults (`:125`, `:143`), `set_car_lin` beside `set_sip_replace` (`:471-473`), and `primary.car_lin` / alts-loop assignment at the three arms that set store flags (`:689/691`, `:709/711`, `:723/725`)
- Modify: `crates/ast-core/src/fitschan/fits_store.rs` — `pub car_lin: bool` beside `sip_replace` (`:26`) and its `false` initializer (`:156`)
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs` — `spec_trans` / `spec_trans_inplace` take `car_lin`; `translate_msx_car` gated on `!car_lin`; the doc comment at `:1057-1065` rewritten
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs:592-741` — the linear-CAR branch in `build_iwc_to_sky_mapping`
- Modify: `crates/ast-core/tests/test_wcsconv.rs` — honor `carlin` in `load_input` (`:240-265`), drop the `// carlin, sipok -- not yet supported.` note at `:303`, delete the `car1_native` / `car2_native` / `car4_native` entries from `NATIVE_READ_EXCLUDED` (`:83-85`)
- Modify: `crates/ast-core/examples/wcsconv_debug.rs:21-29` — honor `carlin` so the re-measure step can reproduce a case by hand
- Test: `crates/ast-core/tests/test_fitschan_wcs.rs` (new tests beside `sip_read_sipreplace_off_keeps_supplied_inverse`, `:331`)

**Interfaces:**
- Consumes: `WcsMap::new(ncoord, WcsProjection::Unknown, lonax, latax)` (`wcsmap.rs:125`, whose `Unknown` early-return at `:156-176` is exactly C's `astWcsMap(naxes, AST__WCSBAD, …)`); `WcsMap::simplified` (`:699`, which collapses `Unknown` to `UnitMap`); `WinMap::from_corners` (`winmap.rs:82`); `CmpMap::new` + `Composition::Series`; `FitsStore::crval`.
- Produces:

```rust
// mod.rs
/// Set the CarLin attribute (mirrors C `CarLin`, default false): when
/// true, a `-CAR` projection is treated as a simple linear pixel-to-sky
/// mapping rather than as a spherical Cartesian projection, and the
/// MSX-CAR `CRPIX` wrap in `spec_trans` is suppressed.
pub fn set_car_lin(&mut self, v: bool);

/// Return whether CarLin is enabled.
pub fn car_lin(&self) -> bool;
```

```rust
// spec_trans.rs — both entry points gain the flag; there is no default
// argument in Rust, so every caller passes it explicitly.
pub(crate) fn spec_trans(cards: &CardList, encoding: Encoding, car_lin: bool) -> CardList;
pub(crate) fn spec_trans_inplace(cards: &mut CardList, encoding: Encoding, car_lin: bool);
```

```rust
// fits_store.rs — read-path flag, set from the FitsChan attribute in the
// same place as `sip_replace`.
pub car_lin: bool,
```

Threading `car_lin` through `spec_trans` is the bulk of the mechanical work
and the reason this sub-fix is one commit rather than three.
The call sites, enumerated:

- `spec_trans(&…, …)` — 7 sites: `mod.rs:687`, `:707`, `:721`, `:735`,
  `:745`, and `enc_aips.rs:819`, `:1237`.
- `spec_trans_inplace(&mut …, …)` — 47 sites: 45 inside `spec_trans.rs`
  (its own definition plus 44 unit tests) and 2 in `wcs_keywords.rs:606`,
  `:649`.

The five `mod.rs` sites pass `self.car_lin`.
The two `enc_aips.rs` sites and the two `wcs_keywords.rs` sites are
test-support and store-population paths with no FitsChan in scope: pass
`false`, the C default.
The `spec_trans.rs` unit tests pass `false` except
`translates_msx_car_wraps_crpix` (`:2588`), which gets a `false` companion
asserting the wrap runs and a new `true` case asserting it does not.

Do not add a wrapper that defaults the flag.
The point of the signature change is that every caller states which
behavior it wants; a defaulting wrapper would let a future caller silently
get the wrong one.

- [ ] **Step 1: Write the failing test — the linear-CAR mapping is one WinMap**

Append to `crates/ast-core/tests/test_fitschan_wcs.rs`. The file's existing
imports (`:1-10`) already cover `AnyObject`, `FitsChan`, `FrameRef` and
`Frame`; add `ast_core::mappings::WinMap` and `ast_core::to_native_string` at
the top of the test body rather than the file header, matching
`render_mapping`'s local-import style in `test_simplify_regression.rs:1149`.

```rust
#[test]
fn carlin_car_header_yields_a_single_winmap() {
    // With CarLin set, C `WcsCelestial` (fitschan.c:36038-36083) builds
    // WcsMap(AST__WCSBAD) + a CRVAL-shift WinMap instead of calling
    // WcsNative. The unknown-projection WcsMap collapses to a UnitMap on
    // simplify, so the whole pixel-to-sky chain reduces to one WinMap with
    // scale = CDELT*d2r and shift = CRVAL*d2r - CRPIX*CDELT*d2r.
    use ast_core::mappings::WinMap;

    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/car1.head",
    ))
    .expect("car1.head should be readable");

    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_car_lin(true);
    let obj = fc.read().unwrap().expect("car1.head should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };

    let map = fs
        .mapping(FrameRef::Base, FrameRef::Current)
        .expect("base-to-current mapping");
    let win = map
        .as_any()
        .downcast_ref::<WinMap>()
        .expect("CarLin CAR should give a bare WinMap, not a CmpMap");

    // Transform the reference pixel: it must land on CRVAL in radians.
    let d2r = std::f64::consts::PI / 180.0;
    let pix = ast_core::coords::PointSet::from_columns(&[&[1.0_f64], &[1.0_f64]]).unwrap();
    let sky = ast_core::mappings::Mapping::transform(win, &pix, true).unwrap();
    assert!(
        (sky.point(0)[0] - 185.0 * d2r).abs() < 1e-12,
        "lon at CRPIX: got {}, want {}",
        sky.point(0)[0],
        185.0 * d2r
    );
    assert!(
        (sky.point(0)[1] - -35.0 * d2r).abs() < 1e-12,
        "lat at CRPIX: got {}, want {}",
        sky.point(0)[1],
        -35.0 * d2r
    );

    // One pixel along each axis must move by exactly CDELT*d2r.
    let pix2 = ast_core::coords::PointSet::from_columns(&[&[2.0_f64], &[2.0_f64]]).unwrap();
    let sky2 = ast_core::mappings::Mapping::transform(win, &pix2, true).unwrap();
    assert!(
        (sky2.point(0)[0] - sky.point(0)[0] - -0.125 * d2r).abs() < 1e-15,
        "lon step: got {}",
        sky2.point(0)[0] - sky.point(0)[0]
    );
    assert!(
        (sky2.point(0)[1] - sky.point(0)[1] - 0.125 * d2r).abs() < 1e-15,
        "lat step: got {}",
        sky2.point(0)[1] - sky.point(0)[1]
    );
}

#[test]
fn carlin_off_car_header_keeps_the_spherical_projection() {
    // The two-sided guard: without CarLin the same header must still build
    // the spherical CAR pipeline (a CmpMap), not the linear one. C takes the
    // `else` branch at fitschan.c:36132 and calls WcsNative.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/car1.head",
    ))
    .expect("car1.head should be readable");

    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let obj = fc.read().unwrap().expect("car1.head should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    let map = fs
        .mapping(FrameRef::Base, FrameRef::Current)
        .expect("base-to-current mapping");
    assert!(
        map.as_any().downcast_ref::<ast_core::mappings::WinMap>().is_none(),
        "CarLin off must not collapse to a bare WinMap"
    );
}

#[test]
fn carlin_keeps_the_cartesian_projection_description() {
    // C passes the real `prj` (AST__CAR) to WcsSkyFrame (fitschan.c:36207),
    // not the AST__WCSBAD substitute given to astWcsMap, so the SkyFrame's
    // Projection is still "Cartesian".
    use ast_core::frames::SkyFrame;

    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/car1.head",
    ))
    .expect("car1.head should be readable");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_car_lin(true);
    let obj = fc.read().unwrap().expect("car1.head should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame");
    assert_eq!(sky.projection(), Some("Cartesian"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p ast-core --test test_fitschan_wcs carlin
```

Expected: all three FAIL to compile — `set_car_lin` does not exist.
That is the correct first failure; the compile error names the missing
method.

- [ ] **Step 3: Add the `CarLin` attribute to `FitsChan` and `FitsStore`**

In `crates/ast-core/src/fitschan/mod.rs`, beside `sip_replace` at `:107`:

```rust
    car_lin: bool,
```

Initialize `car_lin: false` in both constructors (beside `sip_replace: true`
at `:125` and `:143`). Then, after `set_sip_replace` (`:471-473`):

```rust
    /// Set the CarLin attribute (mirrors C `CarLin`, default false): when
    /// true, a `-CAR` projection is read as a simple linear pixel-to-sky
    /// mapping rather than as a spherical Cartesian projection, and the
    /// MSX-CAR `CRPIX` wrap in `spec_trans` is suppressed.
    pub fn set_car_lin(&mut self, v: bool) {
        self.car_lin = v;
    }

    /// Return whether CarLin is enabled.
    pub fn car_lin(&self) -> bool {
        self.car_lin
    }
```

C's internal `carlin` is tri-state (`-1` = unset) only so `astTestCarLin` can
tell "unset" from "set to 0"; nothing in the library reads that test, so a
`bool` is a complete port. Do not add an `Option<bool>`.

In `crates/ast-core/src/fitschan/fits_store.rs`, beside `sip_replace` at
`:26`:

```rust
    /// Read `-CAR` projections as a linear pixel-to-sky mapping (C
    /// `CarLin`). Set from the FitsChan attribute before decoding.
    pub car_lin: bool,
```

with `car_lin: false` in the initializer at `:156`.

In `mod.rs`, at each of the three arms that already forward store flags
(`:689/691`, `:709/711`, `:723/725`), add the matching pair:

```rust
                primary.car_lin = self.car_lin;
                for (_, st) in &mut alts {
                    st.car_lin = self.car_lin;
                }
```

The `FitsAips | FitsAipsPP`, `FitsClass` and `Dss` arms build their store via
`enc_aips` / `enc_class` / `enc_dss` `read_from_cards` and set no store flags
at all; leave them alone. They are not among the encodings any `carlin`
fixture uses, and adding flag forwarding there is a separate change with its
own fixtures.

- [ ] **Step 4: Thread `car_lin` through `spec_trans` and gate the MSX wrap**

Change both entry points in
`crates/ast-core/src/fitschan/spec_trans.rs:40` and `:49`:

```rust
pub(crate) fn spec_trans(cards: &CardList, encoding: Encoding, car_lin: bool) -> CardList {
    let mut trans = cards.clone();
    spec_trans_inplace(&mut trans, encoding, car_lin);
    trans
}

pub(crate) fn spec_trans_inplace(cards: &mut CardList, encoding: Encoding, car_lin: bool) {
```

At the translation-19 call site (`:147`):

```rust
    // Translation 19 (fitschan.c:32140-32170): MSX CAR projection.
    // Some MSX images have CAR projections whose CRPIX puts the
    // valid native longitude range entirely outside the image; C
    // shifts CRPIX1 by +/-360 degrees / CDELT1 to bring it back in
    // range. Suppressed when CarLin is set, because a linear CAR
    // reading has no restricted native longitude range to bring the
    // array into. Per-version letter, no encoding gate.
    if !car_lin {
        translate_msx_car(cards);
    }
```

Rewrite the helper's doc comment (`:1057-1065`) so it describes the code:

```rust
/// Translation #19 helper: MSX CAR projection longitude wrap.
///
/// CAR images that span more than 360 degrees in pixel space need their
/// CRPIX shifted so the image's mid-pixel longitude lies in
/// [-180, 180]. Mirrors fitschan.c:32140-32170. Primary plane only; no
/// encoding gate. The caller applies C's `!astGetCarLin( this )` gate.
```

Update the callers: `self.car_lin` at `mod.rs:687`, `:707`, `:721`, `:735`,
`:745`; `false` at `enc_aips.rs:819`, `:1237` and `wcs_keywords.rs:606`,
`:649` (no FitsChan is in scope in those four, and `false` is the C default).
The 44 `spec_trans_inplace` calls inside `spec_trans.rs`'s own tests take
`false`, except as covered by Step 5.

- [ ] **Step 5: Add the two-sided `spec_trans` gate test**

Beside `translates_msx_car_wraps_crpix` (`spec_trans.rs:2588`), which keeps
its existing assertions with `false` threaded through:

```rust
    #[test]
    fn carlin_suppresses_the_msx_car_wrap() {
        // C fitschan.c:32142 gates the whole block on
        // `!astGetCarLin( this )`. car4.head is the fixture that needs it:
        // CRPIX1 = 211076 with CD1_1 = -0.0016666667 gives an unwrapped
        // shift of 6.1399520760316237 rad, which is what C emits.
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/wcsconv/inputs/car4.head",
        ))
        .expect("car4.head should be readable");
        let base = CardList::from_text(&text).unwrap();

        let mut wrapped = base.clone();
        spec_trans_inplace(&mut wrapped, Encoding::FitsWcs, false);
        assert_ne!(
            wrapped.get_float("CRPIX1"),
            Some(211076.0),
            "CarLin off: the wrap must rewrite CRPIX1"
        );

        let mut kept = base.clone();
        spec_trans_inplace(&mut kept, Encoding::FitsWcs, true);
        assert_eq!(
            kept.get_float("CRPIX1"),
            Some(211076.0),
            "CarLin on: CRPIX1 must be left alone"
        );
    }
```

- [ ] **Step 6: Run it to see the gate test fail, then pass**

```bash
cargo test -p ast-core --lib fitschan::spec_trans::tests::carlin_suppresses_the_msx_car_wrap
```

Expected before Step 4: compile failure (arity). After Step 4: PASS.
Run the whole module to catch a threading mistake:

```bash
cargo test -p ast-core --lib fitschan::spec_trans
```

- [ ] **Step 7: Port C's linear-CAR branch in `wcs_celestial.rs`**

`build_iwc_to_sky_mapping` (`:592-741`) grows the branch. The signature gains
the flag rather than reading it off the store, so the mapping builder stays a
pure function of its arguments:

```rust
fn build_iwc_to_sky_mapping(
    store: &FitsStore,
    wcsmap: WcsMap,
    naxis: usize,
    lon_axis: usize,
    lat_axis: usize,
    car_lin: bool,
) -> Option<(DynMapping, Option<(f64, f64)>)> {
```

Immediately after `zoom` is built (`:616-620`), and before the `wcsmap_fwd`
clone at `:634`, insert:

```rust
    // C `WcsCelestial` (fitschan.c:36038-36083): a CAR projection read with
    // CarLin set, and an unrecognised projection, are both mapped linearly.
    // The WcsMap is constructed with an unknown projection type (C's
    // AST__WCSBAD), which `WcsMap::simplified` collapses to a UnitMap
    // (wcsmap.rs, mirroring wcsmap.c:3087-3097), and the native-to-celestial
    // spherical rotation (`WcsNative`) is not applied at all. Instead a
    // per-axis WinMap adds the reference longitude and latitude, in radians,
    // to the two celestial axes. The WinMap has unit scale on every axis:
    // C passes ina = 0, inb = 1 and outa = outb - 1 = the reference value.
    if (car_lin && matches!(store.projection_code(lon_axis).as_deref(), Some("CAR")))
        || proj == WcsProjection::Unknown
    {
        let unknown = WcsMap::new(
            naxis,
            WcsProjection::Unknown,
            lon_axis + 1,
            lat_axis + 1,
        )
        .ok()?;

        let ina: Vec<f64> = vec![0.0; naxis];
        let inb: Vec<f64> = vec![1.0; naxis];
        let mut outa: Vec<f64> = vec![0.0; naxis];
        let mut outb: Vec<f64> = vec![1.0; naxis];

        // C only shifts an axis whose CRVAL is present; a missing CRVAL
        // makes the whole axis BAD (fitschan.c:36068, :35948).
        match store.crval[lon_axis] {
            Some(v) => {
                outa[lon_axis] += v * deg2rad;
                outb[lon_axis] += v * deg2rad;
            }
            None => {
                outa[lon_axis] = crate::BAD_VALUE;
                outb[lon_axis] = crate::BAD_VALUE;
            }
        }
        match store.crval[lat_axis] {
            Some(v) => {
                outa[lat_axis] += v * deg2rad;
                outb[lat_axis] += v * deg2rad;
            }
            None => {
                outa[lat_axis] = crate::BAD_VALUE;
                outb[lat_axis] = crate::BAD_VALUE;
            }
        }

        let shift = crate::mappings::WinMap::from_corners(&ina, &inb, &outa, &outb).ok()?;
        let step1 =
            CmpMap::new(Box::new(zoom), Box::new(unknown), Composition::Series).ok()?;
        let full = CmpMap::new(Box::new(step1), Box::new(shift), Composition::Series).ok()?;
        let ref_pos = match (store.crval[lon_axis], store.crval[lat_axis]) {
            (Some(_), Some(_)) => Some((crval_lon, crval_lat)),
            _ => None,
        };
        return Some((Box::new(full), ref_pos));
    }
```

The `proj` binding the condition reads is `WcsMap::projection()`
(`crates/ast-core/src/mappings/wcsmap.rs:256`), whose signature is

```rust
    pub fn projection(&self) -> WcsProjection
```

— by value, so `proj == WcsProjection::Unknown` compiles directly. Read it
*before* the branch, because `wcsmap` is moved into the `wcsmap_inv` binding
further down the function. Add, right after the function's `deg2rad` binding
(`:599`):

```rust
    let proj = wcsmap.projection();
```

`store.projection_code(lon_axis)` (`fits_store.rs:260-267`) returns the
four-character suffix without its leading dash, so the comparison is against
`"CAR"`, not `"-CAR"`. Confirm that before writing the condition.

Update the call site at `:424-428`:

```rust
    let (mapping, ref_pos) = build_iwc_to_sky_mapping(
        store,
        wcsmap,
        store.naxis,
        lon_axis,
        lat_axis,
        store.car_lin,
    )?;
```

Leave `set_projection` (`:272-278`) exactly as it is: `proj` there is the real
CAR projection, which is what C hands `WcsSkyFrame` (fitschan.c:36207), so
`PROJ_A = 'Cartesian'` keeps being emitted.

- [ ] **Step 8: Port C's two forced-`carlin` sites**

C forces the local `carlin` on for an axis whose CTYPE is too short to carry
a 4-character projection code. The two sites are identical, one inside the
longitude block (fitschan.c:35878-35886) and one inside the latitude block
(`:35803-35811`):

```c
               ctlen = strlen( ctype );
               if( ctlen > 4 ) {
                  prj = astWcsPrjType( ctype + ctlen - 4 );
               } else if( tabmap && *tabmap ) {
                  prj = AST__WCSBAD;
               } else {
                  prj = AST__CAR;
                  carlin = 1;
               }
```

Three branches, in order: a CTYPE longer than 4 characters gets its real
projection code (`AST__WCSBAD` if unrecognised — and note the error report
just below is gated on `ctlen > 4`, so a *short* CTYPE never errors); a short
CTYPE in a `-TAB` header gets `AST__WCSBAD`; anything else gets CAR *with
`carlin` forced on*, which is the "assume simple linear axes if no projection
code is supplied" case the C comment at `:35744-35747` names.

Mirror all three where `wcs_celestial.rs` classifies the projection code
(`:122-134`), and thread the resulting flag into the `car_lin` argument
Step 7 added rather than into the branch condition separately — C uses a
single local `carlin` for both the attribute and the forced case.

Do not fold this into the attribute: C never calls `astSetCarLin`, only
`astGetCarLin` into a local (`:32012`, `:35683`), so reading a header must not
mutate the FitsChan's `CarLin`.

- [ ] **Step 9: Run the tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_wcs carlin
```

Expected: `carlin_car_header_yields_a_single_winmap`,
`carlin_off_car_header_keeps_the_spherical_projection` and
`carlin_keeps_the_cartesian_projection_description` all PASS.

- [ ] **Step 10: Honor `carlin` in the harness and the debug example**

`crates/ast-core/tests/test_wcsconv.rs`, in `load_input`'s attribute loop
(`:250-258`):

```rust
        for (key, val) in &case.attrs {
            let on = val == "1" || val.eq_ignore_ascii_case("true");
            match key.as_str() {
                "sipreplace" => fc.set_sip_replace(on),
                "carlin" => fc.set_car_lin(on),
                _ => {}
            }
        }
```

Delete `carlin` from the `// carlin, sipok -- not yet supported.` note at
`:303`, leaving `sipok`.

Mirror the same match in `crates/ast-core/examples/wcsconv_debug.rs:21-29`,
so a case can be reproduced by hand during the re-measure step.

- [ ] **Step 11: Re-measure the three cases**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep -E "car1_native|car2_native|car4_native"
```

Expected: no output — all three pass. If a case still fails, record its new
first differing line: a second diff behind the first one belongs to whichever
cause it names, and the case stays excluded with an updated reason until that
cause lands.

- [ ] **Step 12: Remove the three exclusion entries**

Delete from `NATIVE_READ_EXCLUDED` (`test_wcsconv.rs:83-85`):

```rust
    ("car1_native", "BEGAST_H: WinMap vs CmpMap object shape"),
    ("car2_native", "BEGAST_H: WinMap vs CmpMap object shape"),
    ("car4_native", "BEGAST_H: WinMap vs CmpMap object shape"),
```

- [ ] **Step 13: Regression sweep**

This changes what the reader builds, so the transform oracle is in scope:

```bash
cargo test -p ast-core --test test_wcsconv
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core --test test_simplify_regression
cargo test -p ast-core
```

`HEAD_ORACLE_EXCLUDED` is empty and must stay empty. The Phase 4
reconciliation attributed `car4`'s head-oracle failure to exactly this gap,
so the oracle report should improve — that is the independent check that the
fix is real rather than fixture-shaped.

- [ ] **Step 14: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/mod.rs \
        crates/ast-core/src/fitschan/fits_store.rs \
        crates/ast-core/src/fitschan/spec_trans.rs \
        crates/ast-core/src/fitschan/wcs_celestial.rs \
        crates/ast-core/tests/test_fitschan_wcs.rs \
        crates/ast-core/tests/test_wcsconv.rs \
        crates/ast-core/examples/wcsconv_debug.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: port the CarLin attribute and C's linear-CAR read branch (car1, car2, car4)"
```

---

#### Sub-fix B: `GetUsedPolyTan`'s two `badpv` warnings are stubs

**Case:** `tan-zeropv`.

**Depends on Task 11 (U7) sub-fixes A and C.**
Sub-fix A gives `push_astwarn` its insert-at-position behavior and C's
adjacent-block dedup; sub-fix C gives the `pv_max` projection-parameter table
and makes `WcsMap::set_pv` reject an out-of-range `m` with the
`"Projection parameter PV%d_%d found, but is not used by %s projections."`
warning.
This sub-fix adds no projection-parameter machinery of its own — the second
warning group in the expected file is produced entirely by sub-fix C once this
one stops demoting the header.

**Measured.**
The recorded first diff (`WCSCONV_NO_EXCLUDE=1`):

```
tan_zeropv_native L70 got PV2_1_A = 1.0   exp ENDAST_H= 'WcsMap  '
```

`test_wcsconv.rs:119` records this as "ENDAST_H: WcsMap object shape", which
names the symptom.
The last four cards of `inputs/tan-zeropv.head` (COMMENT/HISTORY stripped) are
the whole point:

```
PV1_1   =                  0.0 / All-zero distortion on longitude axis
PV1_2   =                  0.0 / All-zero distortion on longitude axis
PV2_1   =                  1.0 / Non-zero distortion on latitude axis
PV2_2   =            0.0001000 / Non-zero distortion on latitude axis
```

`expected/tan-zeropv.native:60-76` shows a plain TAN `WcsMap` with **no PV
cards at all**:

```
BEGAST_K= 'WcsMap  '           / FITS-WCS sky projection
INVERT_A=                    1 / Mapping inverted
TYPE_A  = 'TAN     '           / Gnomonic projection
...
ENDAST_H= 'WcsMap  '           / End of object definition
```

So both the longitude PVs (deleted here) and the latitude PVs (rejected by
`pv_max`, sub-fix C) are gone by the time the WcsMap is dumped.

`expected/tan-zeropv.native:131-149` carries the two warning groups:

```
NAXIS1  =                  300 / Length of x axis
ASTWARN = '        '
ASTWARN = 'Projection parameter PV2_1 found, but is not used by -TAN '
ASTWARN = 'projections.'
ASTWARN = '        '
ASTWARN = '        '
ASTWARN = 'Projection parameter PV2_2 found, but is not used by -TAN '
ASTWARN = 'projections.'
ASTWARN = '        '
NAXIS2  =                  300 / Length of y axis
END
ASTWARN = '        '
ASTWARN = 'This FITS header describes a distorted TAN projection, but '
ASTWARN = 'all the distortion coefficients (the PVi_m headers) on the '
ASTWARN = 'longitude axis are zero.'
ASTWARN = '        '
```

The first group is sub-fix C's, twice, landing between `NAXIS1` and `NAXIS2` —
the cursor position sub-fix B of Task 11 establishes.
The second group is **this** sub-fix's, landing after the mid-stream `END` at
line 144.
The harness normalizes only a *trailing* `END`, so that mid-stream one stays in
the comparison and the position difference is load-bearing.

**C, fitschan.c:14126-14135 and :14071-14080.**
`GetUsedPolyTan` checks each axis for a non-zero PV coefficient with `m >= 1`,
latitude first:

```c
         if( !ok ) {
            Warn( this, "badpv", "This FITS header describes a distorted TAN "
                  "projection, but all the distortion coefficients (the "
                  "PVi_m headers) on the latitude axis are zero.", method,
                  class, status );
            ret = 0;
```

and, in the `else` branch when latitude was fine:

```c
            if( !ok ) {
               Warn( this, "badpv", "This FITS header describes a distorted TAN "
                     "projection, but all the distortion coefficients (the "
                     "PVi_m headers) on the longitude axis are zero.", method,
                     class, status );
               ret = 0;
```

Each is followed by a delete loop over that axis's `PV<ax>_<m>`, `m` from 1 to
`ubnd`, using `astClearCard` + `FindKeyCard` + `DeleteCard`.

For `tan-zeropv` the trace is: latitude `PV2_1 = 1.0` is non-zero, so the
latitude check passes and the `else` branch runs; longitude `PV1_1` and
`PV1_2` are both zero, so the **longitude** warning fires, `ret = 0`, and only
the longitude PVs are deleted.
CTYPE stays `-TAN` — `ret == 0` means "use the standard TAN convention", not
"rewrite the CTYPE".
The latitude PVs survive `SpecTrans` and are then rejected downstream by
`WcsCelestial`'s PV loop, because TAN's `mxpar` is 0.

**Rust.** Both `Warn` calls are stubs, `spec_trans.rs:641-644` and `:662-666`:

```rust
        if !ok {
            // Mirror C: Warn "badpv"; no warning machinery in this port yet.
            ret = 0;
            // Delete all PVlat_m (m = 1..=ubnd_lat) to prevent mis-use.
            for m in 1..=ulat {
                cards.delete(&pv_name(axlat, m));
            }
        } else {
```

The deletes are already faithful; only the warnings are missing.
The comment "no warning machinery in this port yet" is stale once Task 11
sub-fix A lands, and must be removed rather than reworded — a comment whose
subject is what the port does not yet do is exactly the kind the project's
style rules exclude.

The warning text has to reach the *original* chan, not the `SpecTrans` copy
`get_used_poly_tan` mutates, so this sub-fix also threads a warning
out-parameter down to it — see "Where the warning must land" under Step 3.

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs:592-673` (`get_used_poly_tan`: `warnings` out-parameter, two `warnings.push` calls, two stale comments removed)
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs:686-716` (`translate_tpv_and_polytan_to_tpn`: same out-parameter, passed through)
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs:40-49` (`spec_trans` / `spec_trans_inplace`: same out-parameter)
- Modify: `crates/ast-core/src/fitschan/mod.rs` (push the returned warnings alongside the celestial ones)
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs:606`, `:649` (two `spec_trans_inplace` callers pass a discarded `Vec`)
- Modify: `crates/ast-core/tests/test_wcsconv.rs` (delete the `tan-zeropv_native` entry, `:119`)
- Test: `crates/ast-core/tests/test_fitschan_astwarn.rs` (created by Task 11 sub-fix B)

**Interfaces:**
- Consumes: `CardList::push_astwarn` (Task 11 sub-fix A), `CardList::delete`, `WcsMap::pv_max` and the `set_pv` rejection warning (Task 11 sub-fix C), `spec_trans`'s signature as Task 13 sub-fix A left it (`car_lin` already added).
- Produces: `spec_trans` / `spec_trans_inplace` / `translate_tpv_and_polytan_to_tpn` / `get_used_poly_tan` each gain a trailing `warnings: &mut Vec<String>`. No task after this one changes those signatures again.

- [ ] **Step 1: Write the failing test**

Append to `crates/ast-core/tests/test_fitschan_astwarn.rs`:

```rust
#[test]
fn all_zero_longitude_pv_warns_and_demotes_to_plain_tan() {
    // C GetUsedPolyTan (fitschan.c:14167-14176): the latitude axis has a
    // non-zero coefficient (PV2_1 = 1.0) so it passes, but the longitude
    // axis is all zero, which raises `badpv`, demotes the header to a plain
    // TAN projection and deletes the longitude PVs.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/tan-zeropv.head",
    ))
    .expect("tan-zeropv.head should be readable");

    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let _ = fc.read().unwrap().expect("tan-zeropv.head should read");

    let warns = collect_astwarn(&mut fc);
    assert!(
        warns.iter().any(|w| w.contains("on the longitude axis are zero")),
        "expected the longitude-axis badpv warning, got: {warns:#?}"
    );
    assert!(
        !warns.iter().any(|w| w.contains("on the latitude axis are zero")),
        "the latitude axis has PV2_1 = 1.0 and must not warn: {warns:#?}"
    );
}

#[test]
fn all_zero_latitude_pv_warns_about_the_latitude_axis() {
    // The other side of the same branch: zero every latitude coefficient and
    // C reports the latitude axis instead, without ever reaching the
    // longitude check (fitschan.c:14126-14135).
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/tan-zeropv.head",
    ))
    .expect("tan-zeropv.head should be readable")
    .replace("PV2_1   =                  1.0", "PV2_1   =                  0.0")
    .replace("PV2_2   =            0.0001000", "PV2_2   =            0.0000000");

    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let _ = fc.read().unwrap().expect("modified header should read");

    let warns = collect_astwarn(&mut fc);
    assert!(
        warns.iter().any(|w| w.contains("on the latitude axis are zero")),
        "expected the latitude-axis badpv warning, got: {warns:#?}"
    );
}
```

`collect_astwarn` is the helper Task 11 sub-fix B adds to this file: it rewinds
the chan and returns the reassembled text of each ASTWARN block, one `String`
per block. Read its exact signature there before writing these tests; if it
returns `Vec<String>` of individual card values rather than joined blocks, drop
the `.contains` predicates to a single distinctive phrase that survives the
card wrap (`"longitude axis are zero"` spans one card in the expected file, so
it does).

The second test edits the header text in memory, not the fixture on disk. The
fixtures are C-generated ground truth and must not be modified.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p ast-core --test test_fitschan_astwarn zero
```

Expected: `all_zero_longitude_pv_warns_and_demotes_to_plain_tan` and
`all_zero_latitude_pv_warns_about_the_latitude_axis` both FAIL — no ASTWARN
card is produced, so `warns` is empty.

- [ ] **Step 3: Add the two warnings**

`warnings` here is the new out-parameter — add it to the four signatures given
under "Where the warning must land" below before editing the two stub bodies,
so the function compiles as you go.

In `crates/ast-core/src/fitschan/spec_trans.rs`, replace the latitude stub
(`:641-644`):

```rust
        if !ok {
            warnings.push(
                "This FITS header describes a distorted TAN projection, but \
                 all the distortion coefficients (the PVi_m headers) on the \
                 latitude axis are zero."
                    .to_owned(),
            );
            ret = 0;
            // Delete all PVlat_m (m = 1..=ubnd_lat) to prevent mis-use.
            for m in 1..=ulat {
                cards.delete(&pv_name(axlat, m));
            }
        } else {
```

and the longitude stub (`:662-666`):

```rust
            if !ok {
                warnings.push(
                    "This FITS header describes a distorted TAN projection, but \
                     all the distortion coefficients (the PVi_m headers) on the \
                     longitude axis are zero."
                        .to_owned(),
                );
                ret = 0;
                // Delete all PVlon_m (m = 1..=ubnd_lon) to prevent mis-use.
                for m in 1..=ulon {
                    cards.delete(&pv_name(axlon, m));
                }
            }
```

Check the wrapped card text against `expected/tan-zeropv.native:146-148` before
committing: C's `Warn` wraps at a fixed width, so the Rust string's internal
single spaces must match C's source string exactly. A Rust `\`-continued string
literal eats the leading whitespace of the next line, so each continuation
begins with the space that follows the previous word.

**Where the warning must land.** C settles this: `GetUsedPolyTan` takes both
chans (`AstFitsChan *this, AstFitsChan *out`, fitschan.c:13986) but *never
touches `out`* — `awk 'NR>=13890 && NR<=14110 && /\bout\b/'` over fitschan.c
returns nothing — and both `Warn` calls (`:14032`, `:14073`) pass `this`, the
original chan, not `out`. The PV deletions at `:14040-14050` also target
`this`.

So the warnings must reach `self.cards`, not the `trans` clone. `spec_trans`
returns only a `CardList` (`spec_trans.rs:40`), and `push_astwarn` is a
`CardList` method (`card_list.rs:68`), so pushing onto the `cards` argument
inside `get_used_poly_tan` writes to the clone and the warning is dropped when
`finish_read_consumption` takes only the consumed *names* off it
(`mod.rs:635-642`).

Thread the warnings out instead, the same way the celestial decoder already
does (`mod.rs:693-696`: `let (obj, warnings) = …; for w in warnings {
self.cards.push_astwarn(&w); }`). Add a trailing
`warnings: &mut Vec<String>` to each function on the one path that reaches
the stubs:

```rust
pub(crate) fn spec_trans(
    cards: &CardList,
    encoding: Encoding,
    car_lin: bool,
    warnings: &mut Vec<String>,
) -> CardList;

pub(crate) fn spec_trans_inplace(
    cards: &mut CardList,
    encoding: Encoding,
    car_lin: bool,
    warnings: &mut Vec<String>,
);

fn translate_tpv_and_polytan_to_tpn(cards: &mut CardList, warnings: &mut Vec<String>);

fn get_used_poly_tan(
    cards: &mut CardList,
    axlon: usize,
    axlat: usize,
    s: char,
    warnings: &mut Vec<String>,
) -> bool;
```

`translate_tpv_and_polytan_to_tpn` (`spec_trans.rs:686`) is the only caller of
`get_used_poly_tan` (`:710`), and `spec_trans_inplace` is the only caller of
that, so no other translation helper's signature moves. An out-parameter rather
than a return value keeps `spec_trans_inplace`'s 45 in-file test callers to a
one-token edit each (`&mut Vec::new()`), and the two external
`spec_trans_inplace` callers in `wcs_keywords.rs` (`:606`, `:649`) likewise
discard them — those run inside `extract_wcs_keywords`, which has no chan to
warn on.

At the `mod.rs` call site, declare the vector before `spec_trans` and drain it
into the same loop the celestial warnings already use:

```rust
                let mut warnings: Vec<String> = Vec::new();
                let mut trans = spec_trans::spec_trans(
                    &self.cards,
                    Encoding::FitsWcs,
                    self.car_lin,
                    &mut warnings,
                );
                …
                let (obj, read_warnings) =
                    wcs_read::decode_wcs_with_alts_inner(&primary, &alts, false)?;
                warnings.extend(read_warnings);
                self.set_warn_position_from(&trans);
                for w in warnings {
                    self.cards.push_astwarn(&w);
                }
```

The `let mut trans`, the `&mut trans` handed to the extractor and the
`set_warn_position_from` call are all Task 11 sub-fix C's; this sub-fix adds
only the `warnings` vector, the extra `spec_trans` argument and the `extend`.
The `extend` puts translation warnings *before* read warnings, which is C's
order — `SpecTrans` runs to completion before `WcsToStore` starts.

Task 13 sub-fix A already adds `car_lin` to both signatures, so land that
first and append `warnings` to the same parameter list rather than touching
every call site twice.

Note the PV deletions *do* belong on the `trans` clone: C deletes them so the
subsequent `WcsToStore` cannot use them, and in the port the store is built
from `trans`. Only the warning crosses back.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p ast-core --test test_fitschan_astwarn zero
```

Expected: both PASS.

- [ ] **Step 5: Re-measure**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep tan-zeropv
```

Expected: no output. If it still fails, the remaining diff is almost certainly
in the *second* warning group — the `pv_max` rejections from Task 11 sub-fix C
— or in where the two groups land relative to `NAXIS1` / `NAXIS2` / the
mid-stream `END`. Record the first differing line and check it against
`expected/tan-zeropv.native:131-149` before changing anything: a position
difference is Task 11 sub-fix A's contract, not this sub-fix's.

- [ ] **Step 6: Remove the exclusion entry**

Delete from `NATIVE_READ_EXCLUDED` (`test_wcsconv.rs:119`):

```rust
    ("tan-zeropv_native", "ENDAST_H: WcsMap object shape"),
```

- [ ] **Step 7: Regression sweep**

```bash
cargo test -p ast-core --test test_wcsconv
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core --test 'test_fitschan*'
cargo test -p ast-core
```

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/spec_trans.rs \
        crates/ast-core/tests/test_fitschan_astwarn.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "fitschan: emit GetUsedPolyTan's badpv warnings for an all-zero PV axis (tan-zeropv)"
```

---

#### Sub-fix C: match C's `WinMap`/`MatrixMap` normal form for a non-uniform CDELT pair

**Case:** `zpx`.

**Measured.** Recorded first diff (`WCSCONV_NO_EXCLUDE=1`):

```
zpx_native   L51  got BEGAST_I= 'ShiftMap'   exp BEGAST_I= 'MatrixMap'
```

Only two leaves differ, and they are the same two, swapped.
`expected/zpx.native:52-92` has the leaf sequence

```
CmpMap, MatrixMap, CmpMap, WinMap, CmpMap, WcsMap, CmpMap, SphMap,
CmpMap, MatrixMap, SphMap
```

with the `MatrixMap` holding CD in **degrees** (`M0_A = -1.5845078730445003E-6`,
matching `zpx.head`'s `CD1_1` exactly) and the `WinMap` carrying the degrees→
radians factor in its scales (`SCL1_A = SCL2_A = 0.017453292519943295`) plus
`SFT1_A = 0.0048931414519699694`, `SFT2_A = -6.0184004538763232E-4`.

We emit those two the other way round: a `ShiftMap`
(`SFT1_A = 319.07751731815097`, `SFT2_A = -3035.6498263557196` — that is
`-CRPIX`) followed by a `MatrixMap` holding CD·d2r
(`M0_A = -2.7654879408398838E-8`).

**The two forms are algebraically identical.**
With `-CRPIX = (319.077517318151, -3035.64982635572)`,

```
(CD · (-CRPIX)) · d2r = (0.00489314145196997, -0.0006018400453876324)
```

which is C's `SFT1_A`/`SFT2_A` to the last digit.
So this sub-fix is about the merge *normal form*, not about the arithmetic.
Do not go looking for a numerical error.

**Two hypotheses are already disproved. Do not re-attempt either.**

1. *"Rust `can_swap` returns `(true, false)` unconditionally for a MatrixMap
   partner, where C computes a `simpler` flag."*
   C does exactly the same thing — winmap.c:361-363 is
   `if( !strcmp( nowin_class, "MatrixMap" ) ){ ret = 1;` with `*simpler` left
   at its `0` initialisation.
   `winmap_merge.rs:119-121` is faithful.
2. *"`win_map_merge` fails to freeze the `ShiftMap` it installs, so the
   `ShiftMap` class turns it back into a `WinMap` — see winmap.c:131's
   changelog note about flagging the ShiftMap as frozen."*
   `grep -n 'astSetFrozen\|astClearFrozen' winmap.c shiftmap.c cmpmap.c
   mapping.c` matches **only** cmpmap.c:3538,3563,3608 and
   mapping.c:16853,16858.
   winmap.c never freezes anything; the freeze the changelog describes is
   delivered indirectly by cmpmap.c's `PatternCheck` →
   `astSetFrozen( map_list[ modified ] )`, and that is already ported at
   `simplify.rs:1458` (`pattern_check`) and `simplify.rs:1567-1568`
   (`ctx.freeze(modified)`).

**What distinguishes `zpx` from the fixtures that already pass.**
This is the load-bearing finding, and it is why the fix must be narrow.

C's `SpecTrans` converts a CD matrix into PC + CDELT by row norm
(fitschan.c:31440-31481): `cdelt[j] = sqrt(sum_i CD[j][i]^2) * sign(CD[j][j])`,
then `PC[j][i] = CD[j][i] / cdelt[j]`.
Applying that to the three off-diagonal-CD fixtures in the corpus:

| fixture | CDELT pair | equal? | what the CDELT matrix narrows to |
|---|---|---|---|
| `serpens` | `-0.0020211389333069925`, `-0.0020211389333069925` | **yes** | diagonal with equal diagonals → `ZoomMap` |
| `zpx` | `-9.253475713904881e-05`, `9.255325538402174e-05` | **no** | non-uniform diagonal `MatrixMap` |
| `sao-tan` | CDELT-only header, no CD and no PC at all | — | single diagonal matrix, nothing to strand against |

`matrixmap.c:1855-1955`'s replace-with-simpler ladder narrows a square
diagonal MatrixMap with equal diagonals to a `ZoomMap`, and `ZoomMap` is in
`WinMap`'s **direct**-merge list (`WinZoom`, winmap.c:1196-1206).
`zpx`'s CDELT matrix stays a non-uniform diagonal MatrixMap, which is
reachable only through the `astIsDiagonal` clause and the separate
`WinDiagMat` branch (winmap.c:1219-1229).

So the divergence is about **which merge path the CDELT matrix takes**, not
about a global swap-direction rule.
A fix that flips a swap direction globally will regress `serpens` and
`sao-tan`.

**The Rust divergence to investigate first.**
C has four distinct direct-merge helpers, dispatched by neighbour class
(winmap.c:1196-1240):

```c
         if( !strcmp( nclass, "WinMap" ) ){        newwm = WinWin(...);      invert = 0;
         } else if( !strcmp( nclass, "ZoomMap" ) ){ newwm = WinZoom(...);    invert = 0;
         } else if( !strcmp( nclass, "ShiftMap" ) ){newwm = WinShift(...);   invert = 0;
         } else if( !strcmp( nclass, "MatrixMap" ) ){newwm = WinDiagMat(...); invert = 0;
         } else {  /* UnitMap - just clone the WinMap. */
```

The Rust side has **no `win_diag_mat` function at all** —
`grep -n 'win_diag_mat\|WinDiagMat' crates/ast-core/src/mappings/winmap_merge.rs`
returns three comment matches (`:23`, `:657`, `:674`) and no code.
All four branches funnel through one generic helper,
`merge_linear_pair` (`winmap_merge.rs:653-690`), whose only per-class
behaviour is a `both_win` flag gating the `WinWin` rounding snaps:

```rust
    if series {
        if s1.len() != s2.len() { return None; }
        let both_win = m1.as_any().is::<super::winmap::WinMap>()
            && m2.as_any().is::<super::winmap::WinMap>();
        super::winmap::compose_linear_win(&s1, &c1, &s2, &c2, both_win)
    }
```

Read `WinDiagMat` in the C and compare it against what `merge_linear_pair`
produces for a non-uniform diagonal MatrixMap partner.
In particular check the diagonal-only precondition: C reaches `WinDiagMat`
only after `astIsDiagonal` has passed, whereas `extract_linear_params` will
happily accept any matrix it can read scales out of.

**A second candidate, if `WinDiagMat` turns out faithful.**
`WinMat` (winmap.c:2793-2970) calls `astSimplify` on **both** swap outputs
before installing them (winmap.c:2946-2955):

```c
         sw1 = astSimplify( w1 ); w1 = astAnnul( w1 );
         maps[ 1 - iwm ] = (AstMapping *) sw1;
         inverts[ 1 - iwm  ] = astGetInvert( sw1 );
         sm2 = astSimplify( m2 ); m2 = astAnnul( m2 );
         maps[ iwm ] = (AstMapping *) sm2;
         inverts[ iwm  ] = astGetInvert( sm2 );
```

Confirm the Rust `swap_pair` does the same.
Note also that `WinMat`'s returned WinMap **always has unit scales** — its
docstring at winmap.c:2812-2819 says "The scale factors in the returned WinMap
are always unity (i.e. the differences in scaling get absorbed into the
returned MatrixMap)", and the body's install loop is `*(bb++) = 1.0`.
So `WinMat` is *not* the origin of C's `SCL = d2r` WinMap; that WinMap comes
from a direct merge, which is consistent with the `WinDiagMat` reading above.

**Where the deg2rad matrix comes from.**
`WcsCelestial` builds it at fitschan.c:36010-36026 as a *diagonal* MatrixMap
with `AST__DD2R` on the two celestial axes and `1.0` elsewhere.
For a 2-axis celestial-only header both diagonal entries are equal, so this
one narrows to a `ZoomMap` and is directly mergeable.
That is the matrix whose `d2r` ends up in C's WinMap scales.

**Files:**
- Modify: `crates/ast-core/src/mappings/winmap_merge.rs` (add the
  `WinDiagMat` path, or fix `swap_pair`'s simplify — determined by Step 1)
- Test: `crates/ast-core/tests/test_winmap.rs`
- Modify: `crates/ast-core/tests/test_wcsconv.rs:143` (remove the exclusion)
- Probe (throwaway, restored at the end):
  `reference/starlink-ast/src/cmpmap.c`

**Interfaces:**
- Consumes: `merge_linear_pair(m1, inv1, m2, inv2, series, win_inv)`,
  `extract_linear_params(map, invert) -> Option<(Vec<f64>, Vec<f64>)>`,
  `matrix_is_diagonal(&MatrixMap) -> bool` (`winmap_merge.rs:44-122`).
- Produces: no new public API. The change is confined to
  `win_map_merge`'s direct-merge step (`winmap_merge.rs:379-445`).

- [ ] **Step 1: Probe C's nomination loop for both fixtures**

The root cause is narrowed to the `WinDiagMat` path but not pinned, so start
with a measurement rather than an edit.
Follow `docs/issues/c-oracle-probe-recipe.md`; editing
`reference/starlink-ast/src/` in place is sanctioned for probes because the
submodule state is recorded and restorable.

Instrument cmpmap.c's nomination loop (the `astSimplify` driver at
cmpmap.c:3528-3620) to log one line per iteration:

```c
      fprintf( stderr, "NOM nominated=%d class=%s modified=%d nmap=%d frozen=%d\n",
               nominated, astGetClass( ( *map_list )[ nominated ] ),
               modified, *nmap,
               astGetFrozen( ( *map_list )[ nominated ] ) );
```

Rebuild with cmake (never autotools — see
`docs/issues/c-oracle-probe-recipe.md`):

```bash
cmake --build reference/starlink-ast/build-ast -j8
```

Then run the C converter over both fixtures and capture the traces:

```bash
R=$PWD/reference/starlink-ast/build-ast
cd crates/ast-core/tests/fixtures/wcsconv
LD_LIBRARY_PATH= <wcsconverter> inputs/serpens.head native /tmp/serpens.c.native \
  'FitsDigits=17' 2> /tmp/serpens.nom
LD_LIBRARY_PATH= <wcsconverter> inputs/zpx.head native /tmp/zpx.c.native \
  'FitsDigits=17' 2> /tmp/zpx.nom
diff /tmp/serpens.nom /tmp/zpx.nom
```

The first iteration where the two traces choose different classes is the
divergence point. Record it in the commit message.

- [ ] **Step 2: Write the failing test**

`WinMap` exposes no public shift/scale getters, so assert on the serialised
leaf sequence — the same pattern
`test_winmap.rs::offdiagonal_cd_chain_fuses_deg2rad_into_the_matrixmap`
(`test_winmap.rs:371-437`) already uses.

Append to `crates/ast-core/tests/test_winmap.rs`:

```rust
/// A CRPIX WinMap followed by an off-diagonal CD matrix whose row norms are
/// *unequal* keeps the degrees→radians factor in the WinMap's scales and
/// leaves the CD matrix in degrees, matching C's normal form for the ZPX
/// header (`fixtures/wcsconv/expected/zpx.native:52-92`).
#[test]
fn nonuniform_cd_chain_keeps_deg2rad_in_the_winmap() {
    let d2r = std::f64::consts::PI / 180.0;
    // -CRPIX from fixtures/wcsconv/inputs/zpx.head.
    let win = WinMap::new(&[319.077_517_318_150_97, -3035.649_826_355_719_6], &[1.0, 1.0]);
    let cd = MatrixMap::new(
        2,
        2,
        &[
            -1.584_507_873_044_5e-6,
            -9.252_119_007_872_2e-5,
            -9.253_885_610_129_5e-5,
            1.632_542_080_243_77e-6,
        ],
    )
    .unwrap();
    let deg2rad = MatrixMap::new(2, 2, &[d2r, 0.0, 0.0, d2r]).unwrap();
    let wcs = {
        let mut w = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
        w.invert();
        w
    };

    let c1 = CmpMap::new(Box::new(win), Box::new(cd), true).unwrap();
    let c2 = CmpMap::new(Box::new(c1), Box::new(deg2rad), true).unwrap();
    let outer = CmpMap::new(Box::new(c2), Box::new(wcs), true).unwrap();

    let simplified = outer.simplified().unwrap();
    let text = to_native_string(&AnyObject::CmpMap(
        simplified
            .as_any()
            .downcast_ref::<CmpMap>()
            .expect("simplified chain is still a CmpMap")
            .clone(),
    ))
    .unwrap();
    let classes: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Begin "))
        .map(|l| l.split_whitespace().next().unwrap_or(""))
        .filter(|c| *c != "CmpMap")
        .collect();
    assert_eq!(classes, ["MatrixMap", "WinMap", "WcsMap"]);
}
```

Adjust the `WcsMap::new` / `CmpMap::new` calls to the signatures the
neighbouring tests in the same file use — copy them verbatim from
`test_winmap.rs:371-437` rather than guessing.

- [ ] **Step 3: Run it to verify it fails**

```bash
cargo test -p ast-core --test test_winmap nonuniform_cd_chain
```

Expected: FAIL, with `classes` reported as `["ShiftMap", "MatrixMap", "WcsMap"]`
— the swapped form.

- [ ] **Step 4: Implement what Step 1 measured**

Whatever Step 1 pinned, the change is a *port*, not a tuning.
If the divergence is the collapsed `WinDiagMat` path, add the missing helper
alongside `merge_linear_pair` and route the MatrixMap branch to it, gating on
`matrix_is_diagonal` exactly as C gates on `astIsDiagonal`.
If it is `swap_pair`'s missing `astSimplify`, add the two simplify calls in
the order winmap.c:2946-2955 has them.

Do not add a fixture-name test, a tolerance constant, or a
"prefer-MatrixMap-first" preference: those are the shortcuts the project's
porting rules forbid.
If neither candidate explains the trace, stop and record the trace in
`docs/issues/fitschan-c-port-gaps.md` rather than guessing at a third.

- [ ] **Step 5: Run both WinMap tests**

```bash
cargo test -p ast-core --test test_winmap
```

Expected: PASS, including
`offdiagonal_cd_chain_fuses_deg2rad_into_the_matrixmap` — that test locks the
`serpens` shape (`["ShiftMap", "MatrixMap", "WcsMap"]`) and **must not
regress**. If it flips, the fix is a global swap-direction change and is
wrong; go back to Step 4.

- [ ] **Step 6: Re-measure — expect a *changed* diff, not a pass**

```bash
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 \
  | grep -A3 zpx
```

`zpx` needs **Task 2 (U3) string-value padding** as well: roughly twelve
trailing non-WCS cards in its header carry short string values that C pads to
the 8-column minimum (`SYSVER = 's7-1              '` where we emit
`SYSVER = 's7-1    '`; also `OBSERVAT`, `OBSTYPE`, `IMAGETYP`, `INSTRUME`,
`WFFBAND`, `WFFPSYS`, `WFFID`, `UTSTART`, `DETECTOR`, `BUNIT`, `CCDSPEED`).

So the success condition here is that the first diff **moves off L51** and
onto one of those padding cards. If Task 2 has already landed, `zpx` passes
outright and Step 7 applies; if not, leave the exclusion entry in place with
its reason string updated to name the padding, and let Task 2 remove it.

- [ ] **Step 7: Remove the exclusion entry (only if `zpx` now passes)**

Delete from `NATIVE_READ_EXCLUDED` (`test_wcsconv.rs:143`):

```rust
    ("zpx_native", "BEGAST_I: ShiftMap vs MatrixMap object shape"),
```

- [ ] **Step 8: Restore the probe instrumentation**

```bash
git -C reference/starlink-ast checkout -- src/cmpmap.c
git -C reference/starlink-ast status --short src/
```

Expected: no output from the second command.

- [ ] **Step 9: Regression sweep**

`winmap_merge` is shared by every simplify consumer, so this sub-fix needs the
widest sweep in the plan:

```bash
cargo test -p ast-core --test test_winmap
cargo test -p ast-core --test test_simplify_regression
cargo test -p ast-core --test test_wcsconv
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv
cargo test -p ast-core
```

Expected: the simplify regression stays at 427 passed, and the three
serialization round-trips (24 + 139 + 719 byte-exact) stay clean.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/mappings/winmap_merge.rs \
        crates/ast-core/tests/test_winmap.rs \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "winmap: match C's direct-merge path for a non-uniform diagonal MatrixMap (zpx)"
```

---

## D3 — deferred-fixture visibility

Independent of all thirteen code tasks.
May land at any point, including first.

### Task 14 (spec deliverable D3): make the 24 deferred C-oracle fixtures visible

**The problem.**
Twenty-four fixtures imported from upstream `ast_tester/simplify_fixtures/`
are staged out of reach *by directory*:

```
crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/
├── README.md
├── intramap/            13 files   (IntraMap unported)
├── stcresourceprofile/   2 files   (StcResourceProfile unported)
└── xphmap/               9 files   (XphMap unported)
```

The staging is honest — the README (already present) explains the layout, why
both fixture consumers skip the directory, and the three-step re-enabling
procedure.
What is missing is *visibility*: no test mentions the directory, no test output
counts the files, and `PLAN.md` has no row.
A reader of a green test summary cannot tell these 24 files exist.

Both consumers skip the subdirectory naturally, so nothing is currently
enforcing that a *new* subdirectory gets registered.
Someone staging a fourth unported class could drop `tests/fixtures/.../foo/`
there and no test would notice.

**No wcsconv case is blocked on an unported class**, so this is entirely
separate from the 45.

**A second, smaller finding.**
`crates/ast-core/tests/fixtures/wcsconv/inputs/offset-sref.head` is one of the
132 C `ast_tester` `.head` fixtures.
It has no row in `wcsconv_tests.txt` and no `expected/offset-sref.native`.
Its only consumer is `fixtures/oracle/headers.oracle:3410,3436`, read by
`test_transform_oracle.rs`, which `PLAN.md` records as an `#[ignore]`
investigative harness rather than a CI gate.
So in practice it is exercised by nothing that runs.
It is a plain offset-sky TAN header with `SREFIS`/`SREF1`/`SREF2` cards, which
is exactly the shape Task 7 (U5) and Task 12 (U8) sub-fix D touch — worth
having as a gate.

**Files:**
- Create: `crates/ast-core/tests/test_deferred_fixtures.rs`
- Modify: `PLAN.md` (new section under "4. Serialization / FITS / round-trip")
- Modify: `crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt` (one row)
- Create: `crates/ast-core/tests/fixtures/wcsconv/expected/offset-sref.native`
  (generated by the C converter, not hand-written)

**Interfaces:**
- Produces: `DEFERRED_CLASSES: &[(&str, &str, usize)]` — `(subdirectory,
  unported class, expected file count)`. Nothing consumes it outside its own
  test file.

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_deferred_fixtures.rs`:

```rust
//! Registry for simplify fixtures staged out of reach because they
//! exercise classes the port has not implemented.
//!
//! The fixtures live in
//! `tests/fixtures/native/reference/simplify_fixtures_unsupported/<class>/`,
//! and both fixture consumers skip that directory: the manifest-driven
//! `test_simplify_regression` has no rows for them, and the
//! auto-discovering byte-exact round-trips read
//! `simplify_fixtures/` non-recursively.
//!
//! Staging them is deliberate. Staging them *silently* is not — a green
//! summary line would otherwise hide 24 C-generated fixtures. This test
//! prints the count on every run and fails if a subdirectory appears
//! that is not registered below.

use std::path::PathBuf;

/// `(subdirectory, unported class, file count)`.
///
/// When a class lands, follow the re-enabling procedure in the
/// directory's `README.md` and delete its row.
const DEFERRED_CLASSES: &[(&str, &str, usize)] = &[
    ("intramap", "IntraMap", 13),
    ("stcresourceprofile", "StcResourceProfile", 2),
    ("xphmap", "XphMap", 9),
];

fn deferred_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/simplify_fixtures_unsupported"
    ))
}

#[test]
fn deferred_fixtures_are_all_registered() {
    let root = deferred_root();
    assert!(root.is_dir(), "{} is not a directory", root.display());

    let mut found: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();

    let mut registered: Vec<String> =
        DEFERRED_CLASSES.iter().map(|(d, _, _)| (*d).to_owned()).collect();
    registered.sort();

    assert_eq!(
        found, registered,
        "unregistered or stale deferred-fixture subdirectory: add or remove \
         a DEFERRED_CLASSES row in tests/test_deferred_fixtures.rs"
    );

    let mut total = 0usize;
    for (dir, class, expected) in DEFERRED_CLASSES {
        let n = std::fs::read_dir(root.join(dir))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().is_file())
            .count();
        assert_eq!(
            n, *expected,
            "{dir}/ ({class}) holds {n} files, DEFERRED_CLASSES says {expected}"
        );
        total += n;
    }

    println!(
        "deferred simplify fixtures: {total} files across {} unported classes",
        DEFERRED_CLASSES.len()
    );
    for (dir, class, n) in DEFERRED_CLASSES {
        println!("  {dir}/ — {class} unported — {n} files");
    }
}
```

- [ ] **Step 2: Run it to verify it passes, and that the count prints**

```bash
cargo test -p ast-core --test test_deferred_fixtures -- --nocapture
```

Expected: PASS, with the summary lines visible:

```
deferred simplify fixtures: 24 files across 3 unported classes
  intramap/ — IntraMap unported — 13 files
  stcresourceprofile/ — StcResourceProfile unported — 2 files
  xphmap/ — XphMap unported — 9 files
```

- [ ] **Step 3: Verify the guard actually fires**

The test's value is the failure it produces, so prove it:

```bash
mkdir crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/probe
cargo test -p ast-core --test test_deferred_fixtures
```

Expected: FAIL, naming `probe` in the assertion message.
Then remove it and confirm green again:

```bash
rmdir crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/probe
cargo test -p ast-core --test test_deferred_fixtures
```

- [ ] **Step 4: Add the `PLAN.md` section**

Insert after the `fset_to_store` row's `-TAB` blocker paragraph in section 4
("Serialization / FITS / round-trip"):

```markdown
### Deferred C-oracle simplify fixtures

Twenty-four fixtures from upstream `ast_tester/simplify_fixtures/` are staged
in `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/`
because they exercise classes the port has not implemented. They are counted
and enforced by `crates/ast-core/tests/test_deferred_fixtures.rs`, which fails
if a subdirectory is added without a matching `DEFERRED_CLASSES` row.

| Subdirectory | Unported class | Files | Blocks |
|---|---|---|---|
| `intramap/` | `IntraMap` | 13 | 8 simplify cases (3 positive, 5 negative-control) |
| `xphmap/` | `XphMap` | 9 | 6 simplify cases (3 positive, 3 negative-control) |
| `stcresourceprofile/` | `StcResourceProfile` | 2 | 1 simplify case (`cap_timemap_02`) |

No wcsconv case is blocked on any of these classes. The re-enabling procedure
is in the directory's `README.md`.
```

Confirm the case counts against the fixture names before committing —
`.map`/`.simp` pairs are positive cases, a lone `.map` with a `neg_` prefix is
a negative control:

```bash
ls crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/*/
```

- [ ] **Step 5: Commit the registry**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/tests/test_deferred_fixtures.rs PLAN.md
git -c core.hooksPath=/dev/null commit -m "tests: count and enforce the 24 deferred simplify fixtures"
```

- [ ] **Step 6: Generate `offset-sref`'s expected output with the C converter**

The reference file must be C-generated ground truth, never hand-written.
Build the reference library with cmake (never autotools) and run its
`wcsconverter` with the same arguments the harness uses:

```bash
cmake --build reference/starlink-ast/build-ast -j8
cd crates/ast-core/tests/fixtures/wcsconv
LD_LIBRARY_PATH= <path-to-wcsconverter> \
  inputs/offset-sref.head native expected/offset-sref.native 'FitsDigits=17'
head -20 expected/offset-sref.native
```

`wcsconverter` takes `<in file> <encoding> <out file> [<attrs>]` and
`FitsDigits=17` is required — without it the numeric tokens are truncated and
the ≤4-ULP comparator has nothing to compare.

- [ ] **Step 7: Add the manifest row**

Append to `crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt`, matching
the surrounding column alignment:

```
offset-sref_native  | offset-sref.head                | offset-sref.native | native    |                                  |
```

- [ ] **Step 8: Run it and record the outcome honestly**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -20
```

The corpus goes from 192 to 193 cases.
Two outcomes, both acceptable:

- It passes → nothing more to do.
- It fails → measure its first differing line and check that line against the
  thirteen causes. `offset-sref` is an offset-sky TAN header, so a
  `SkyRef`/`SkyRefIs` diff belongs to Task 7 (U5) or Task 12 (U8) sub-fix D.
  Fix it under whichever task owns the cause. An exclusion entry is not an
  option: the list shrinks monotonically, and Task 15 requires it empty.

Adding an exclusion here does not conflict with the plan's goal: the final
task's gate is that the list is empty, and this row is inside the plan's
scope. What is *not* acceptable is leaving the fixture unreferenced, or
regenerating the expected file to match our output.

- [ ] **Step 9: Commit the new case**

```bash
git add crates/ast-core/tests/fixtures/wcsconv/expected/offset-sref.native \
        crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt \
        crates/ast-core/tests/test_wcsconv.rs
git -c core.hooksPath=/dev/null commit -m "tests: exercise the imported offset-sref.head wcsconv fixture"
```

---

## Final verification

### Task 15 (spec deliverable D1): Empty the list and prove it is empty

Tasks 1-13 each removed their own entries, so by the time this task starts
`NATIVE_READ_EXCLUDED` should already be down to nothing.
This task is what makes that a *gate* rather than a coincidence: it replaces the
list with an empty slice carrying the comment that says what would justify a new
entry, and runs the whole suite both ways.

The spec's proof is the equality of the two wcsconv runs.
With a non-empty list, `WCSCONV_NO_EXCLUDE=1` compares more cases than the plain
run, so the two counts differ.
With an empty list the environment variable is a no-op and both runs report the
same numbers — which is the evidence that the list is genuinely empty rather
than that the comparison was disabled.

Task 0b's `native_read_excluded_entries_still_fail` gate has been enforcing the
other direction throughout: an entry that started passing failed the suite
rather than lingering.
Over an empty list that gate is vacuous — it iterates nothing — which is
correct and is why it is not the proof.

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs:63-146` (`NATIVE_READ_EXCLUDED` → empty slice; doc comment rewritten)
- Modify: `PLAN.md` (FitsChan row in section 4)

**Interfaces:**
- Consumes: `NATIVE_READ_EXCLUDED` as Tasks 1-13 left it.
- Produces: nothing — no code outside the test harness changes.

- [ ] **Step 1: Check what is left**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
grep -c '^    (' crates/ast-core/tests/test_wcsconv.rs
```

Expected: `0` — but the grep also matches the multi-line entries' opening
parenthesis, so confirm by eye that only the `const … = &[];` shell remains.

If entries are left, stop.
Every remaining entry belongs to a task that has not finished, and this task
must not paper over one.
Re-run the re-measure protocol from the top of this plan, attribute each
remaining first diff to its cause, and go back to that task.

- [ ] **Step 2: Replace the constant with an empty slice**

Replace the whole `NATIVE_READ_EXCLUDED` block (`:63-146`) with:

```rust
/// `native`-encoding wcsconv cases whose keyword-card output is not yet
/// byte-exact with the C oracle.
///
/// **Empty, and intended to stay empty**: all 193 wcsconv cases are compared
/// against their C-generated reference. `WCSCONV_NO_EXCLUDE=1` is therefore a
/// no-op, and both runs report the same counts — which is what proves the list
/// is empty rather than that the comparison was turned off.
///
/// A new entry is justified only by a difference that is *not* a port defect:
/// a C artifact the port deliberately does not reproduce, and that the
/// comparator cannot absorb. Aliasing-driven tag placement was the one such
/// case, and it is handled by `common::is_aliasing_tag_card` in the comparator
/// instead (see `docs/issues/c-library-quirks.md`), so a new entry needs a
/// reason of that kind, recorded in that document, and not merely a diff
/// nobody has diagnosed yet. A FITS-read gap is a bug to fix, not an entry to
/// add.
const NATIVE_READ_EXCLUDED: &[(&str, &str)] = &[];
```

Matching the shape of `HEAD_ORACLE_EXCLUDED`
(`test_transform_oracle.rs:1317`) and `FRAMESET_FIXTURE_EXCLUDED`
(`test_serialisation.rs:1231`), both already empty slices with the same kind of
comment.

Leave the two references to the constant in place: the doc comment at
`:8` and `CompareMode::parse`'s at `:40` both describe the mechanism correctly
whether or not the list has entries, and the `native_excluded` computation at
`:355-356` is what makes `WCSCONV_NO_EXCLUDE=1` a no-op rather than an error.

- [ ] **Step 3: Run both ways and compare the counts**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

Expected: both report `194 passed; 0 failed` — the 192 generated cases from the
original corpus, plus `offset-sref_native` from Task 14, plus Task 0b's
stale-entry gate.
If Task 14's Step 8 recorded an exclusion for `offset-sref`, then that entry is
still in the list and Step 1 stopped this task; finish the cause it names first.

The two lines being identical is the spec's success criterion.
Record both in the commit message, since it is the one measurement that cannot
be reconstructed later from the code.

- [ ] **Step 4: Run the full workspace suite**

```bash
cargo test --workspace 2>&1 | grep -E "^(test result|error|warning: unused)" | sort | uniq -c
```

Expected: every `test result` line is `ok`, with `0 failed` throughout.

The corpora this plan's units touched, all of which must still be green:

| suite | expectation |
|---|---|
| `--test test_wcsconv` | 194 passed |
| `--test test_simplify_regression` | 427 passed |
| `--test test_serialisation` | the 24 + 139 + 719 byte-exact round-trips |
| `--test test_fitschan*` | 66 passed |
| `--test test_deferred_fixtures` | prints `24 files across 3 unported classes` |

- [ ] **Step 5: Run the head transform oracle**

This is the independent check that the reader units fixed real behavior rather
than fixture-shaped behavior — it consumes the same headers through a different
comparison.

```bash
git submodule update --init reference/starlink-ast
cargo test -p ast-core --test test_transform_oracle -- --ignored 2>&1 | tail -20
```

`HEAD_ORACLE_EXCLUDED` is an empty slice today and must still be empty.
The 17 head oracle failures recorded in the Phase 4 reconciliation are
downstream of exactly the read-path gaps Tasks 11-13 closed, so the report
should be *better*, never worse.
If the oracle now fails where it passed before, a reader unit changed what the
decoded object is in a way the wcsconv byte comparison did not catch — that is a
real regression, and the unit that caused it must be revisited.

- [ ] **Step 6: Update `PLAN.md`**

In section 4 ("Serialization / FITS / round-trip"), update the FitsChan row so
it records the state this plan reached: all 193 wcsconv cases compared with no
exclusions, `NATIVE_READ_EXCLUDED` empty.
Do not delete the D3 section Task 14 added — the deferred classes are still
deferred.

- [ ] **Step 7: Format, lint, commit**

```bash
cargo fmt --all
CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
  PATH=$HOME/.cargo/bin:$PATH \
  cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/tests/test_wcsconv.rs PLAN.md
git -c core.hooksPath=/dev/null commit -m "tests: every wcsconv case is compared — NATIVE_READ_EXCLUDED is empty

194 passed / 0 failed both with and without WCSCONV_NO_EXCLUDE=1; the
equality is what proves the list is empty rather than the comparison
disabled."
```

- [ ] **Step 8: Finish the branch**

Use the `superpowers:finishing-a-development-branch` skill.
It re-runs the suite on the tree about to be integrated, then presents the
integration menu.
Do not merge or push without going through it — pushing to a remote is outside
what this plan authorizes in any case.
