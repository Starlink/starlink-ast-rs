# Transform-oracle: fix the five divergences and gate the remainder

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the five real transform-oracle divergences and make the suite *fail* when they regress.
Today all five live in the 1191 simplify-fixture oracle sections that no asserting test covers: the only harness that reads them is `transform_oracle_report`, which is `#[ignore]` and never asserts.
On completion, every one of the 1606 oracle sections is covered by an asserting gate, and the report harness stops being the only thing that can see a divergence.

**Architecture:** Three work units, ordered so each is independently verifiable and the gate lands last on a green tree.
U1 and U2 are the two root causes behind the five failing sections — both are single-site divergences from the C source, and both are already confirmed by experiment (see *Evidence* under each).
U3 adds the asserting gate over the simplify-fixture sections, following the existing `head_sections_match_oracle` / `slamap_sections_match_oracle` pattern.
Because U1 and U2 take the mismatch count to zero, U3's exclusion list starts **empty** — the gate is born with nothing masked, which is the outcome the user asked for ("make the test fail when they fail and not ignore").

**Tech Stack:** Rust 2024 edition, `cargo test` / `cargo fmt` / `cargo clippy`.
Reference C source under `reference/starlink-ast/src/` is the authority for every behavioral question.
The C-oracle probe recipe is `docs/issues/c-oracle-probe-recipe.md`.

## Global Constraints

These apply to every task in this plan.
A task's requirements implicitly include this section.

- **No fixture edits.** Files under `crates/ast-core/tests/fixtures/` are C-generated ground truth.
  A fixture that looks wrong is a finding to investigate and record, not to overwrite.
- **Port C, do not approximate it.** When C's control flow is available, mirror it.
  Do not substitute a heuristic or a tolerance constant for logic C did not need.
  If the C code itself uses a heuristic, port it and say so in a comment.
- **Comments describe the code, not its history.** No references to this plan, to previous attempts, or to what used to be there.
- **One sentence per line** in Markdown prose. American English spelling.
- **Before every commit:** `cargo fmt --all`, then clippy clean.
  Clippy needs the isolated-target-dir workaround for the conda/rustup toolchain mismatch:
  ```bash
  CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc \
    PATH=$HOME/.cargo/bin:$PATH \
    cargo clippy --workspace --all-targets -- -D warnings
  ```
- **Do not push to any remote.** Commit locally only, with `git -c core.hooksPath=/dev/null`.
- **A test that contradicts the C oracle is the thing that is wrong.** U1 changes one such test; the probe output justifying the change is recorded in the task.
- **The gate's exclusion list shrinks monotonically.** No unit may add an entry to it.

## Measurement protocol

Every unit ends with the same two measurements, and the measurement — not a target count — decides when the unit is done.

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs

# 1. Re-measure the oracle. Writes target/transform_oracle_report.json.
cargo test -p ast-core --test test_transform_oracle -- --ignored transform_oracle_report

# 2. Print the status histogram and every non-passing section.
python3 -c "
import json, collections
secs = json.load(open('target/transform_oracle_report.json'))['sections']
print(dict(collections.Counter(s['status'].split(':')[0] for s in secs)))
for s in secs:
    if s['status'].startswith('mismatch'):
        print(' ', s['relpath'], 'fwd' if s['forward'] else 'inv', s['detail'][:100])
"

# 3. Whole-crate regression check.
cargo test -p ast-core 2>&1 | grep -E 'FAILED|failed;' | grep -v '0 failed'
```

Baseline at the time of writing (commit `96d289f`), across 1606 sections:

| status | count |
|---|---|
| pass | 1580 |
| load-fail | 18 |
| mismatch | 5 |
| skipped | 2 |
| skipped-override | 1 |

The 18 `load-fail` are the deliberate unported-class limbo — `XphMap` (14) and `StcResourceProfile` (4) — already tracked in `PLAN.md` §4 and enforced by `test_deferred_fixtures.rs`.
They are **out of scope** here and U3 must keep them out of the gate rather than silently swallow them.
The 2 `skipped` are `specflux.ast` ("not a mapping"); the 1 `skipped-override` is the live entry in `transform_oracle_overrides.txt`.

## Section coverage today

`transform_oracle_report` (test_transform_oracle.rs:1408) is `#[ignore]` and contains no assertion — it measures and writes JSON.
The two asserting gates select narrow slices:

| gate | selector | sections | failing |
|---|---|---|---|
| `head_sections_match_oracle` | `relpath.ends_with(".head")` | 259 | 0 |
| `slamap_sections_match_oracle` | `basename.contains("sla")` | 136 | 0 |
| **no gate** | everything else | **1211** | **23** |

Of the 1211 ungated sections, 1191 are `simplify_fixtures/*` and 20 are top-level `*.ast`.
U3 gates the 1191; the 20 `.ast` sections are named explicitly in the task so the arithmetic is closed and nothing is left uncounted by accident.

---

## U1 — Region membership must use the *effective* Closed flag

**Root cause.** Every C Region `Transform` selects its boundary comparison from the Negated *and* Closed attributes jointly, not from Closed alone.
Reading the four independent implementations:

- `box.c:4200-4270` — un-negated uses `astAxIn(..., closed)`; negated uses `astAxIn(..., !closed)`.
- `circle.c:2078-2092` — un-negated: `d <= radius` (closed) / `d < radius` (open); negated: `d >= radius` (closed) / `d > radius` (open).
- `ellipse.c:2196-2210` — the same four-way split on `d` vs `1.0`.
- `interval.c:3666-3890` — four separate labelled blocks: closed-unnegated, closed-negated, open-unnegated, open-negated.

In all four the comparison used for the raw geometry is strict iff `closed == negated`, and inclusive iff `closed != negated`.
Negating a region necessarily moves the boundary from one side to the other: the boundary of a closed region belongs to it, so it must *not* belong to the complement.

The Rust port factors membership into `RegionGeometry::reg_base_inside`, which returns *un-negated* membership, with negation applied by the caller (`region_transform`, `regions/data.rs:554-560`).
That factoring is sound, but each `reg_base_inside` reads the raw `self.data.closed` and so computes un-negated membership with the *wrong* boundary convention whenever the region is negated.
The caller then flips the interior correctly and the boundary incorrectly.

**Blast radius.** `reg_base_inside` has four callers, and every one of them wants effective-closed semantics, so the fix belongs in the implementations rather than at one call site:

| caller | C ancestor | wants |
|---|---|---|
| `region_transform` (data.rs:546) | each class's `Transform` | effective |
| `reg_base_inside_point` (data.rs:407) → `point_in_region` | `astTransform` on one point | effective |
| `reg_base_grid` filter (data.rs:407) | `region.c:8066` `astBTransform` | effective |
| `verify_remap_exact` (data.rs:445) | `box.c:3905` boundary-on-region check | effective |

Only `box_region.rs` and `interval.rs` read `self.data.closed` in `reg_base_inside`; `circle.rs` and `ellipse.rs` read it too and must be checked.
`polygon.rs` reads it for its `ON`-boundary case (`polygon.c:6423-6444` shows the same joint split).
`pointlist.rs`, `cmpregion.rs`, `prism.rs`, and `nullregion.rs` do not read it: PointList's C `GetClosed` is an override returning `!Negated` (`pointlist.c:420`), so its effective flag is constant; CmpRegion and Prism delegate to their components, which apply their own.

**Evidence (already gathered).** Two independent confirmations.

1. C oracle probe, on the exact `Interval(lo=[BAD,0], hi=[10,10])` from the failing test, driven through `astTranN`:

   | state | (10,5) | (-100,5) | (5,0) |
   |---|---|---|---|
   | un-negated, Closed=1 | inside | inside | inside |
   | negated, Closed=1 | **inside** | outside | **inside** |
   | negated, Closed flipped to 0 (SelectorMap's state) | BAD | BAD | BAD |

   The middle row is the proof: with the region negated and Closed=1, the two boundary points are *inside* the complement.
   Pre-fix Rust reports them outside.

2. Applying `let closed = self.data.closed != self.data.negated;` in `box_region.rs` and `interval.rs` alone takes the oracle mismatch count from 5 to 1 — i.e. it fixes all four `selectormap_region_simplify*` sections.

**Why the SelectorMap fixtures are the ones that caught it.** `selectormap.c:917-923` negates each Region *and* flips its Closed attribute before transforming, precisely so that "inside" maps to BAD.
That is the one code path in the corpus that reaches a negated region's boundary with a fixture point sitting exactly on it — `(0,0)` on the `[5±5, 5±5]` Box, and `0` on the `[0,1]` Interval.
The Rust `SelectorMap::transform` already mirrors C faithfully (`selectormap.rs:207-228`); it was the region underneath that answered wrongly.

### Tasks

- [ ] Read `box.c:4200-4270`, `interval.c:3666-3890`, `circle.c:2078-2092`, `ellipse.c:2196-2210`, and `polygon.c:6423-6444` and confirm the joint rule in each before touching Rust.
- [ ] In `crates/ast-core/src/regions/box_region.rs::reg_base_inside`, derive the comparison from the effective flag rather than `self.data.closed`.
      Add a comment naming the C ancestor and stating the rule (boundary belongs to the region iff `closed != negated`).
- [ ] Do the same in `interval.rs::reg_base_inside`, covering all four of C's labelled blocks including the excluded-interval (`lb > ub`) orientation.
- [ ] Check `circle.rs`, `ellipse.rs`, and `polygon.rs::reg_base_inside` and apply the identical rule wherever they read `self.data.closed`.
- [ ] Confirm `cmpregion.rs`, `prism.rs`, and `nullregion.rs` need no `reg_base_inside` change, and record why in the commit message rather than in a code comment.
- [ ] `pointlist.rs::is_closed` (line 396) returns the raw `self.data.closed`, but `pointlist.c:380-422` overrides `GetClosed` to return `astGetNegated(this) == 0` unconditionally — a point has zero volume, so it is all boundary, and is closed exactly when it is not negated.
      Port the override.
      No oracle section currently fails on it, so land it as its own commit with its own measurement so a regression it causes is attributable; if it moves any count, stop and diagnose rather than reverting silently.
- [ ] Consider whether the rule is better expressed once as a helper on `RegionData` (e.g. `effective_closed()`) than repeated per class; prefer the helper if it does not obscure the per-class C correspondence.
- [ ] Fix `crates/ast-core/tests/test_interval.rs::reg_pins_semi_infinite_interval_boundary`.
      It asserts `!reg_pins((10,5))`, which the C probe contradicts: C returns `RegPins=1` for that point, `0` for the interior `(-100,5)`, and `1` for the y-face `(5,0)`.
      Post-fix Rust returns exactly that.
      Update the assertion **and** its explanatory comment — the comment's claim about the zero-width uncertainty band leading to rejection is the mis-derivation that produced the wrong expectation.
- [ ] Re-measure (protocol above). Expect mismatches `5 → 1`, and the whole-crate run green with no test other than the one above changed.
- [ ] `cargo fmt --all`; clippy clean; commit.

**Note for the implementer.** Do not reach for a tolerance or an epsilon here.
C uses exact `<=` / `<` comparisons throughout; the bug is a boundary *convention*, not a precision problem.

---

## U2 — Diagonal MatrixMap must propagate a BAD diagonal element

**Root cause.** `matrixmap.c:5248-5270`, the Diagonal branch of `Transform`, keys on whether the diagonal element is BAD:

```c
for( out_coord = 0; out_coord < nax; out_coord++ ){
   diag_term = matrix[ out_coord ];
   if( diag_term != AST__BAD ){
      for( point = 0; point < npoint; point++ ){
         val = *(indata++);
         *(outdata++) = ( val != AST__BAD ) ? diag_term*val : AST__BAD;
      }
   } else {
      for( point = 0; point < npoint; point++ ){
         *(outdata++) = AST__BAD;      /* whole axis is BAD */
      }
   }
}
```

Rust (`crates/ast-core/src/mappings/matrix.rs`, `apply_matrix`, Diagonal arm) instead tests `diag == 0.0` and returns `0.0`:

```rust
if diag == 0.0 {
    outp[j] = 0.0;
} else if is_bad(inp[j]) {
    outp[j] = BAD_VALUE;
} else {
    outp[j] = diag * inp[j];
}
```

Two divergences in three lines.
There is no BAD-diagonal branch at all, and the zero special-case has no C ancestor — C multiplies a zero diagonal through normally, which yields `0.0` for good input and `AST__BAD` for BAD input, whereas the Rust arm returns `0.0` even for BAD input.
The Full arm is already correct: it mirrors `matrixmap.c:5203-5205`, treating a zero coefficient as a mask and a BAD coefficient as poison.

**Reproducer.** `simplify_fixtures/neg_matrix_singular_diag.map` is a Diagonal MatrixMap with `M0 = 0`, `M1 = 2`, `IM1 = 0.5`, and *no* `IM0`.
A BAD inverse element is not written on dump, so `IM0` loads back as `AST__BAD` — `from_node`'s Diagonal branch (`matrix.rs:1193-1200`) already defaults it correctly, and `diagonal_inverse` (`matrix.rs:1450`) already emits `BAD_VALUE` for a zero diagonal.
The BAD genuinely reaches the kernel; the kernel is the sole defect.
C's golden (`crates/ast-core/tests/fixtures/oracle/simplify_fixtures.oracle:12562`, `dir=inverse`) has output column 3 entirely `BAD`; Rust produces `0.0` for all 24 rows.
The forward direction passes, which is why the simplify regression corpus (427/427) never caught it — that corpus does not exercise this map's inverse.

**Evidence (already gathered).** Replacing the arm with C's test:

```rust
if is_bad(diag) || is_bad(inp[j]) {
    outp[j] = BAD_VALUE;
} else {
    outp[j] = diag * inp[j];
}
```

takes the remaining mismatch to zero, with no other test in the crate affected.

### Tasks

- [ ] Re-read `matrixmap.c:5227-5283` in full, including the Unit sub-case and the surplus-output-axis fill, and confirm the Rust arm's structure against it.
- [ ] Rewrite the Diagonal arm of `apply_matrix` to mirror C: BAD diagonal ⇒ the whole output axis is BAD; otherwise BAD input ⇒ BAD output, good input ⇒ `diag * val`.
      Remove the invented `diag == 0.0` special-case.
- [ ] Verify the surplus-axis fill still matches `matrixmap.c:5271-5281` (`nax < ncoord_out` fills axis `nax` with zeros and copies it to the rest).
- [ ] Check whether hoisting the per-axis BAD test out of the point loop, as C does, is worth it here; the arm is on the bulk-transform path, so prefer C's shape (test once per axis, not once per point) as it is both faithful and faster.
- [ ] Re-measure. Expect mismatches `1 → 0` and a green whole-crate run.
- [ ] `cargo fmt --all`; clippy clean; commit.

---

## U3 — Gate the simplify-fixture oracle sections

**Purpose.** Close the structural hole.
U1 and U2 fix the five divergences, but nothing in CI would notice if they came back, because no asserting test reads those sections.
This unit adds `simplify_sections_match_oracle`, modelled on the two gates that already exist.

**Pattern to follow.** `head_sections_match_oracle` (test_transform_oracle.rs:1334) and `slamap_sections_match_oracle` (:1219).
Each has four parts: a selector predicate, an exclusion list, a stale-entry check, and a `SECTION_FLOOR` assertion that stops the gate going vacuous if a rename or a path change silently empties its selector.
The failure classifier is shared:

```rust
let is_fail = |o: &Outcome| {
    matches!(o, Outcome::Mismatch(_) | Outcome::LoadFail(_) | Outcome::Arity(_))
};
```

**The load-fail question.** `Outcome::LoadFail` counts as a failure in the existing classifier, and 18 simplify sections are load-fails by design — the `XphMap` and `StcResourceProfile` fixtures held in limbo pending those classes.
Do **not** relax the classifier, and do **not** put the 18 into the exclusion list, which is reserved for diagnosed divergences and must shrink monotonically.
Instead exclude them at the *selector*, the same way the deferred fixtures are already excluded from the simplify corpus: the fixtures live under `native/reference/simplify_fixtures_unsupported/<class>/`, so a selector that requires the resolved path to be under `simplify_fixtures/` and not under `simplify_fixtures_unsupported/` separates them structurally rather than by name.
`resolve_fixture` (test_transform_oracle.rs:219) already searches both roots, so the resolved path is available.
The deferred set stays enforced where it belongs, by `test_deferred_fixtures.rs` and `PLAN.md` §4.

**Section arithmetic to preserve.** After U1 and U2 the 1606 sections are:

| group | sections | gated by |
|---|---|---|
| `*.head` | 259 | `head_sections_match_oracle` |
| basename contains `sla` | 136 | `slamap_sections_match_oracle` |
| `simplify_fixtures/*`, class ported | 1173 | **U3, new** |
| `simplify_fixtures/*`, class unported | 18 | deferred-fixture limbo (`test_deferred_fixtures.rs`) |
| top-level `*.ast` | 20 | see below |

The 20 top-level `.ast` sections are `a20070718_00010_02_cube.ast`, `degen1.ast`, `dss.ast`, `jan.ast`, `lsst.ast`, `plot3d-test1.ast`, `sparse.ast` (1), `specflux.ast` (2 `skipped`), `splittest1.ast` (1), `stcschan-test1-doc3.ast`, and `timj.ast` — 2 sections each except where noted.
All pass today except the two `specflux.ast` skips.
Gating them is a natural follow-on but is deliberately **not** in this unit's scope: it would mean deciding what `Outcome::Skipped` should mean to a gate, which is a separate question from the one the user asked.
Record it as a follow-up in `PLAN.md` so it is not lost.

### Tasks

- [ ] Read `head_sections_match_oracle` and `slamap_sections_match_oracle` end to end and note every part that must be replicated.
- [ ] Add `fn is_simplify_section(...)` selecting sections whose fixture resolves under `native/reference/simplify_fixtures/` and *not* under `simplify_fixtures_unsupported/`.
      Prefer resolved-path structure over name matching so a newly-ported class moving out of limbo is picked up by the gate automatically.
- [ ] Add `SIMPLIFY_ORACLE_EXCLUDED: &[(&str, &str)] = &[]` with a doc comment stating the same rule the other lists carry: an entry is justified only by a diagnosed non-defect, and a divergence nobody has diagnosed is a bug to fix rather than an entry to add.
- [ ] Add `SIMPLIFY_SECTION_FLOOR`, set from the measured count (expected 1173 — confirm, do not assume) with a comment saying what it guards against.
- [ ] Add `simplify_sections_match_oracle`, using the shared `is_fail` classifier unmodified, reporting unexpected failures and stale exclusions separately, and asserting `checked >= SIMPLIFY_SECTION_FLOOR`.
- [ ] Verify the gate is not vacuous: temporarily reintroduce the U2 one-line divergence, confirm the new gate fails and names `neg_matrix_singular_diag.map`, then revert.
      A gate that has never been seen to fail has not been tested.
- [ ] Confirm the 18 deferred load-fails are outside the selector, and that removing a `DEFERRED_CLASSES` row would not quietly move a section out of every gate.
- [ ] Update `PLAN.md`: record that the simplify-fixture sections are gated, and add the top-level `.ast` sections as a named follow-up.
- [ ] Re-measure; whole-crate green; `cargo fmt --all`; clippy clean; commit.

---

## Definition of done

- [ ] `transform_oracle_report` shows 0 `mismatch` across all 1606 sections.
- [ ] Three asserting gates cover 259 + 136 + 1173 = 1568 sections; the remaining 38 are the 18 documented unported-class load-fails and the 20 top-level `.ast` sections recorded as a follow-up.
- [ ] `SIMPLIFY_ORACLE_EXCLUDED` is empty, as are `HEAD_ORACLE_EXCLUDED` and `SLAMAP_ORACLE_EXCLUDED`.
- [ ] The new gate has been observed to fail on a reintroduced defect.
- [ ] `cargo test -p ast-core` is green; `cargo fmt` and clippy clean.
- [ ] `PLAN.md` reflects the new gate and the follow-up.

## Out of scope

- The 18 `XphMap` / `StcResourceProfile` load-fails — they need those classes ported, tracked in `PLAN.md` §4.
- The 20 top-level `.ast` sections — recorded as a follow-up by U3.
- The two live entries in `transform_oracle_overrides.txt` (`tnx-cheb.head off`, `neg_unitnormmap_inv_fwd_diffcentre.map golden-inv off`).
- `ALIASING_TAG_LICENSED` in `test_wcsconv.rs` — five diagnosed C aliasing artifacts, documented in `docs/issues/c-library-quirks.md`.
- The seven `skip_string_compare` rows in the simplify manifest — they relax serialization comparison only; `equal()` and the transform still assert.
