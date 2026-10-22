# Upstream Quirk-Fix Sync + Simplify Fixture Import

**Date:** 2026-07-13
**Branch:** `tickets/DM-54793` (part of the initial C→Rust core port)
**Submodule:** `reference/starlink-ast` bumped `6b5d1ce8` → `50c2b21b`
(already committed; 147 upstream commits in range)

## Problem

Upstream C AST fixed most of the bugs we recorded in
`docs/issues/c-library-quirks.md` (chiefly PR #58 "quirks" and PR #59
"RefPosPrecision"), and added a large batch of new `astSimplify`
coverage fixtures. Our Rust port currently *mirrors* several of those
now-fixed C bugs on purpose, so that C-generated fixtures round-trip.
With the C behavior corrected, those mirrors are now wrong: they make
Rust reproduce a bug the oracle no longer has. We also lag the upstream
simplify corpus by 164 fixtures.

This work realigns the port with the corrected oracle and imports the
new corpus.

## Goals

1. Remove every Rust workaround that mirrors a C bug the upstream fixed,
   so Rust matches the now-correct C behavior.
2. Update the tier-2 golden fixtures that upstream regenerated as a
   result of those fixes.
3. Update `docs/issues/c-library-quirks.md` so each resolved entry is
   marked resolved (with the fixing commit), and leave the still-open
   entries intact.
4. Import the 164 new simplify fixtures, wire them into the regression
   manifest, and make all fixtures for *supported* classes pass.

## Non-goals

- Porting `IntraMap` or `XphMap` (the two unported classes some new
  fixtures exercise). Those fixtures are imported but explicitly tagged
  as blocked.
- Region *geometric* simplify beyond what already works.
- Any change to quirks that upstream did **not** fix (see Bucket 3).

## Delivery sequence

Single branch `tickets/DM-54793`; two logical commit groups, quirks
first:

- **Group A — un-mirror fixed quirks:** behavior changes + regenerated
  goldens + quirks-doc updates + new/ported tests. Each behavior change
  and its golden update land in the **same commit** so tier-2 never
  breaks mid-sequence.
- **Group B — import simplify corpus:** copy 164 fixtures, sync the
  manifest, fix all supported-class failures.

## Verification posture

Both approaches, where practical (per the decision in brainstorming):

- **Oracle rebuild** for numeric quirks: rebuild
  `reference/starlink-ast` `libast.a` at `50c2b21b` with **cmake** (never
  autotools — see memory `reference_oracle_build_cmake`), then compare
  Rust against the fixed C via tier-4 oracle probes and, where a
  standalone reproducer is cleaner, a small C program linked against the
  prebuilt oracle (memory `reference_c_oracle_reproducer`).
- **Ported C-test assertions** for discrete quirks: port the assertions
  the upstream commits added into `ast-core` tier-1 Rust tests, locking
  the corrected behavior.

Each Bucket 1 change is re-derived by reading the actual upstream
`src/*.c` diff hunk — not the proposed patch text in the quirks doc —
and mirroring C control flow directly (per the project's porting
conventions: no heuristics, no corner-cutting).

## Section A: Per-quirk classification

### Bucket 1 — mirror the C bug today; remove workaround to match fixed C

Each requires a behavior change plus verification.

| Quirk | Rust site | Upstream fix | Change |
|---|---|---|---|
| SpecMap `f32` cast on RA/Dec | `mappings/specmap.rs` (`lsrk_vel`/`lsrd_vel`/`lg_vel`/`gal_vel`) | `409f1311` | Drop `as f32 as f64` truncation; pass f64 straight through |
| VELOSYS lossy `RefRA`/`RefDec` round-trip | `fitschan/fset_to_store/spectral_axes.rs` | `ad04291c` | Remove the `format`→`unformat` quantization; store full-precision double |
| `AxAngle` stale loop index | `frames/mod.rs` (`ax_angle`, ~line 674) | `da9ddb39` | `b[naxes-1]` → `b[axis-1]` |
| `astRate` returns BAD at `x0=0` | `mappings/rate.rs` (~line 44) | `e782c220` | Match fixed-C domain-aware seed |
| `astRate` zero-range stale slot | `mappings/rate.rs` (~lines 99, 141) | `c1a88658` | Index range array by the store index, not the loop counter `iin` |
| `PermMap::Equal` matching-index short-circuit | `mappings/perm.rs` (`equal`) | `0fbd9cc3` | Compare constant *values* whenever both entries are negative |
| `timemap` `ast_dat` inverse divisor | `mappings/timemap.rs` (~lines 701–721) | `8be504ac` | Correct the pre-1972 rubber-second-era divisors |

Notes on the two most delicate ones:

- **VELOSYS / RefRA-RefDec (`ad04291c`):** upstream now sets the
  SpecFrame's `RefRA`/`RefDec` via the typed double interface rather
  than formatting to a sexagesimal string and re-parsing. The Rust
  mirror deliberately replayed the lossy string round-trip; that replay
  is removed. Confirm the fixed C's exact store path from the diff
  before editing.
- **`astRate` seed (`e782c220`):** the Rust code once used the
  mathematically-correct seed, then was changed to mirror the C
  `h0 = 1.0` bug. Upstream now fixes C; re-read the new
  `mapping.c` `Rate` seed logic and match it exactly (it is
  domain-aware, not simply "revert to the old Rust value"). The ported
  ChebyMap `x0=0` assertions flip from `BAD_VALUE` to the true gradient.

### Bucket 2 — Rust already correct; documentation + fixtures only

No behavior change. Update the quirks-doc entry to "resolved upstream at
`<commit>`" and remove now-obsolete "mirrored"/"deliberate divergence"
mirror comments in the code where they only exist to explain the
divergence.

| Quirk | Upstream fix |
|---|---|
| MatrixMap MapSplit uninitialized read | `c9ce955a` |
| ScalePolyInputs stale pointer | `f4c019d4` |
| SIPIntWorld `nout` typo | `232643bc` |
| CLASSFromStore double `specfactor` | `17416d33` |
| SpecFluxFrame `Equal` | `a3b9468c` |
| DATE-OBS fractional-seconds | `254d4c99` |

The `SpecFluxFrame::make_sf_mapping` "one place we intentionally don't
match C" note is retired: upstream now makes identical SpecFluxFrames
compare equal, so Rust and C agree.

### Bucket 3 — not fixed upstream; leave mirrored, no change

- `CmpMap::Rate` chain-rule override — still commented out at
  `cmpmap.c:892`. Keep the whole-map numeric path and the doc entry.
- `Frame::unformat` `strtod` vs `f64::parse` divergence — no C change,
  no domain impact. Keep as documented.

## Section B: Regenerated golden fixtures

Upstream regenerated goldens we ship in
`crates/ast-core/tests/fixtures/wcsconv/expected/`. Verified diffs — our
copies still carry the **old buggy values**:

| Golden | Change | Driven by |
|---|---|---|
| `class-arc.fits-class` | VELO-LSR/VLSR `6876.2765` → `6876.2752` | SpecMap f32-cast removal |
| `class-olddate.fits-class` | regenerated | SpecMap f32-cast removal |
| `class-sin.fits-class` | regenerated | SpecMap f32-cast removal |
| `skyspec3d-class.fits-class` | regenerated | SpecMap f32-cast removal |
| `vrad-v2f.native` | regenerated | SpecMap f32-cast removal |
| `jan.ast` | regenerated | SpecMap f32-cast removal |
| `longslit.fits-wcs` | VELOSYS `-20038.807` → `-20038.808` | RefRA/RefDec precision |
| `azel.fits-wcs` / `azel.native` | already correct (`.087`) — no change | DATE-OBS (fixed earlier) |

Rules:

- The code change and its golden update land in the **same commit**.
- Copy the exact bytes from `reference/starlink-ast/ast_tester/<file>`,
  trimmed to match our stored whitespace form.
- Confirm each regenerated golden is fully explained by its
  corresponding code change and does not mask an unrelated drift.

## Section C: New simplify fixture import

### Corpus

- Upstream `ast_tester/simplify_fixtures/`: 419 `.map` / 276 `.simp`.
- Ours `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`:
  261 `.map` / 177 `.simp`.
- **164 new `.map`** to import (with their `.simp` partners; some
  `neg_*` cases are input-only "no simplification" fixtures).
- We keep 6 local-only `neg_*` fixtures upstream lacks (ours, not
  regressions).

Class distribution of the new fixtures (by name hint): specmap 16,
slamap 11, unitnormmap 10, timemap 10, switchmap 9, **intra 9**, interval
9, winmap 7, pcd 7, selectormap 6, pointlist 6, cmpmap 6, box 6, wcsmap
5, nullregion 5, zoom(map) 7, **xph(map) 6**, ratemap 4, matrix(map) 6,
etc. `intramap` and `xphmap` are unported.

### Mechanics

1. **Copy** all new `.map`/`.simp` into
   `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`.
   Committed, never symlinked (CLAUDE.md).
2. **Sync the manifest** `simplify_tests.txt` (upstream 422 entries,
   ours 259): merge upstream's entries with our 6 local-only ones,
   preserving the `name | input | reference | [skip]` format and
   carrying upstream `skip_string_compare` (`yes`) flags verbatim.
3. **Tag unported classes explicitly:** fixtures whose `.map` body
   contains `IntraMap` or `XphMap` get a leading `#`-comment block in the
   manifest naming the reason (`# blocked: IntraMap/XphMap not yet
   ported — see PLAN.md`), rather than a silent parse-skip. Detect by
   grepping the `.map` body for the class token, not by filename.

### Harness change: distinguish skip reasons

Extend the regression summary so it separates:

- `skipped: unported class (N)` — expected, matches the tagged manifest
  entries.
- `skipped: unexpected (N)` — parse/other failure to investigate.

This prevents a silent regression from hiding inside the "skipped"
counter.

### Failure handling (fix all supported-class failures before landing)

Run `cargo test -p ast-core --test test_simplify_regression`. For
**supported-class** fixtures:

- **equal()/transform failures** → genuine simplifier gaps. Root-cause
  each against the rebuilt oracle (standalone C reproducer where it
  helps), then fix by mirroring C control flow. Fix all before landing.
- **serialization failures** on comment-less references → set the
  `skip_string_compare` flag **iff** upstream's manifest also flags that
  entry; otherwise fix.

Bound: "fix all" applies to supported classes. If a supported-class
fixture exposes a genuinely large simplifier gap, stop and raise it
rather than silently expanding scope or silently skipping. Default is to
fix.

## New / updated tests

Upstream added assertions in these `ast_tester/` files relevant to
Bucket 1; port the relevant cases into `ast-core` tier-1 Rust tests
(or extend existing ported tests):

- `testtime.c` → timemap `ast_dat` inverse round-trip (pre-1972).
- `testframe.c` → `ax_angle` near-axis-aligned cases.
- `testmapping.c` → `PermMap::Equal` constant-comparison cases (four
  cases: shared negative index differing constants, differing indices
  equal/differing values, surplus unreferenced constants).
- `testchebymap.c` → `astRate` at `x0=0` returns the true gradient.
- `testspecflux.c` → identical SpecFluxFrames compare equal (Bucket 2,
  confirms Rust already matches).

SpecMap and VELOSYS/RefRA changes are covered by the regenerated
tier-2 goldens (`class-*`, `longslit`, `jan`, `vrad-v2f`).

## Risks

- **Oracle rebuild cost/setup.** Mitigated: cmake build is documented;
  only needed for numeric verification.
- **A Bucket 1 golden diff might not be fully explained by its code
  change** (masking unrelated drift). Mitigated by the same-commit rule
  and per-golden explanation check.
- **Supported-class fixture failures could reveal a large simplifier
  gap.** Mitigated by the "stop and raise" bound rather than unbounded
  scope creep.

## Success criteria

- All Bucket 1 workarounds removed; Rust matches fixed C, verified by
  oracle and/or ported tests.
- All regenerated goldens updated; tier-2 green.
- `docs/issues/c-library-quirks.md` resolved entries marked resolved
  with fixing commit; Bucket 3 entries unchanged.
- 164 new fixtures imported; manifest synced; unported-class fixtures
  tagged; supported-class fixtures all pass.
- `cargo fmt` + `cargo clippy --workspace -- -D warnings` clean
  (memory `feedback_format_clippy`).
