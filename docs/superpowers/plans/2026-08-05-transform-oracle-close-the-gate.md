# Transform-oracle: fix the five divergences and gate the remainder

**Status: complete.**
The oracle reports 0 mismatches across all 1606 sections, and all 1606 sit inside an asserting gate.
Landed as `e33a396` (U1 regions), `d398ca7` (U2 matrixmap), `56b537e` (pointlist `GetClosed`), `5b3d22f` (U3 gate).
Two claims in the analysis below were disproved while implementing it and are corrected in place: the boundary rule is not joint in Closed and Negated, and polygon is not an exception to it; and the deferred-class sections are licensed by outcome rather than excluded by selector.

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
They are **out of scope** here, and U3 must not silently swallow them.
It admits them to the gate and licenses only their `LoadFail` outcome, which turned out to be stronger than keeping them out of the selector — 4 of those sections already pass, and a selector-level exclusion would have discarded that coverage.
The 2 `skipped` are `specflux.ast` ("not a mapping"); the 1 `skipped-override` is the live entry in `transform_oracle_overrides.txt`.

## Section coverage today

`transform_oracle_report` (test_transform_oracle.rs:1408) is `#[ignore]` and contains no assertion — it measures and writes JSON.
The two asserting gates select narrow slices:

| gate | selector | sections | failing |
|---|---|---|---|
| `head_sections_match_oracle` | `relpath.ends_with(".head")` | 259 | 0 |
| `slamap_sections_match_oracle` | `basename.contains("sla")` | 136 | 0 |
| **no gate** | everything else | **1211** | **23** |

Of the 1211 ungated sections, 1191 are `simplify_fixtures*` and 20 are top-level `*.ast`.
U3 gates all 1211 as one complement group, so the arithmetic closes and nothing is left uncounted.

---

## U1 — Region membership must use the *effective* Closed flag

**Root cause.** Every C Region `Transform` selects its boundary comparison from the Negated *and* Closed attributes jointly, not from Closed alone.
Reading the four independent implementations:

- `box.c:4200-4270` — un-negated uses `astAxIn(..., closed)`; negated uses `astAxIn(..., !closed)`.
- `circle.c:2078-2092` — un-negated: `d <= radius` (closed) / `d < radius` (open); negated: `d >= radius` (closed) / `d > radius` (open).
- `ellipse.c:2196-2210` — the same four-way split on `d` vs `1.0`.
- `interval.c:3666-3890` — four separate labelled blocks: closed-unnegated, closed-negated, open-unnegated, open-negated.

In all four the comparison used for the raw geometry is strict iff `closed == negated`, and inclusive iff `closed != negated`.

The reason is *not* that negation moves the boundary from one side to the other.
It is the opposite: in C a point exactly on the boundary belongs to the region iff Closed, **independent of Negated**, so a region and its negation both contain their shared boundary.
`polygon.c:6423-6444` states that directly, assigning `in_region = closed` for an `ON` point in both the negated and the un-negated arm; the other classes reach the same result by pairing each arm with a complementary comparison.
Because Rust's `reg_base_inside` reports un-negated membership and the caller boolean-inverts it, the boundary has to be reported *inverted* there in order to come back out the right way — which is why the expression is `closed != negated` even though the attribute rule is plain `closed`.

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
`polygon.rs` reads it for its `ON`-boundary case and needs the same fix — `polygon.c:6423-6444` is where the underlying rule is clearest, not an exception to it.
One `self.data.closed` read must stay raw: the `norm_box` probe in `circle.rs` mirrors C copying the Circle and clearing Negated (`circle.c:1095-1097`), which leaves Closed alone, so the raw value is already the effective one there.
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

- [x] Read `box.c:4200-4270`, `interval.c:3666-3890`, `circle.c:2078-2092`, `ellipse.c:2196-2210`, and `polygon.c:6390-6455` and confirm the rule in each before touching Rust.
      This is what showed the rule is *not* joint: boundary membership is plain `closed`, and the `closed != negated` expression is an artifact of `reg_base_inside` returning un-negated membership to a caller that inverts it.
- [x] In `crates/ast-core/src/regions/box_region.rs::reg_base_inside`, derive the comparison from the effective flag rather than `self.data.closed`.
- [x] Do the same in `interval.rs::reg_base_inside`, covering all four of C's labelled blocks including the excluded-interval (`lb > ub`) orientation.
- [x] Apply the identical rule in `circle.rs`, `ellipse.rs`, and `polygon.rs::reg_base_inside`.
      Polygon needed it too, contrary to the guess above.
      `circle.rs`'s `norm_box` probe is the one deliberate exception and keeps the raw flag, commented against `circle.c:1095-1097`.
- [x] Confirm `cmpregion.rs`, `prism.rs`, and `nullregion.rs` need no `reg_base_inside` change, and record why in the commit message rather than in a code comment.
- [x] `pointlist.rs::is_closed` (line 396) returns the raw `self.data.closed`, but `pointlist.c:380-422` overrides `GetClosed` to return `astGetNegated(this) == 0` unconditionally — a point has zero volume, so it is all boundary, and is closed exactly when it is not negated.
      Port the override.
      No oracle section currently fails on it, so land it as its own commit with its own measurement so a regression it causes is attributable; if it moves any count, stop and diagnose rather than reverting silently.
      Counts unmoved, but it did expose `test_pointlist.rs::region_attributes` asserting a settable Closed; a C probe confirms `astSetI(pl,"Closed",0)` leaves Closed=1, so the test was corrected.
- [x] Consider whether the rule is better expressed once as a helper on `RegionData` (e.g. `effective_closed()`) than repeated per class; prefer the helper if it does not obscure the per-class C correspondence.
      Landed as `RegionData::closed_for_base_inside`, named for the one question it answers so it cannot be mistaken for the Closed attribute itself, with the per-class C citations in its doc comment.
- [x] Fix `crates/ast-core/tests/test_interval.rs::reg_pins_semi_infinite_interval_boundary`.
      It asserts `!reg_pins((10,5))`, which the C probe contradicts: C returns `RegPins=1` for that point, `0` for the interior `(-100,5)`, and `1` for the y-face `(5,0)`.
      Post-fix Rust returns exactly that.
      Update the assertion **and** its explanatory comment — the comment's claim about the zero-width uncertainty band leading to rejection is the mis-derivation that produced the wrong expectation.
- [x] Re-measure (protocol above). Expect mismatches `5 → 1`, and the whole-crate run green with no test other than the one above changed.
- [x] `cargo fmt --all`; clippy clean; commit.

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
The Full arm looks correct at a glance but is not: it implements the zero-coefficient mask from `matrixmap.c:5203-5205` and omits the other half of the same test, so a BAD matrix element there produced a finite result instead of BAD.
No oracle section exercises it, but it is the same defect and was fixed in the same commit, with `matrix_full_bad_coefficient_and_masking` added to cover it.

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

- [x] Re-read `matrixmap.c:5227-5283` in full, including the Unit sub-case and the surplus-output-axis fill, and confirm the Rust arm's structure against it.
- [x] Rewrite the Diagonal arm of `apply_matrix` to mirror C: BAD diagonal ⇒ the whole output axis is BAD; otherwise BAD input ⇒ BAD output, good input ⇒ `diag * val`.
      Remove the invented `diag == 0.0` special-case.
- [x] Verify the surplus-axis fill still matches `matrixmap.c:5271-5281` (`nax < ncoord_out` fills axis `nax` with zeros and copies it to the rest).
- [x] Check whether hoisting the per-axis BAD test out of the point loop, as C does, is worth it here.
      Left nested per point: a `PointSet` is row-major over points, so C's axis-outer order would stride the wrong way.
      The comment in the arm records why the nesting differs from C.
- [x] Fix the Full arm too: `is_bad(coeff)` was missing alongside the zero-coefficient mask.
- [x] Fix `test_matrixmap.rs::matrix_diagonal_bad_handling`, which asserted the invented zero-diagonal behaviour, and add `matrix_full_bad_coefficient_and_masking`.
- [x] Re-measure. Expect mismatches `1 → 0` and a green whole-crate run.
- [x] `cargo fmt --all`; clippy clean; commit.

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
The plan's original answer was to exclude them at the *selector*, by requiring the resolved path to be under `simplify_fixtures/` and not under `simplify_fixtures_unsupported/`.
Measurement rejected that: 4 of the 22 sections under the unsupported root already **pass** (`cap_xphmap_01.simp` ×2 and `xph_inverse_cancel.simp` ×2), so a path-based selector would have discarded live coverage.
What landed instead gates every one of those sections and licenses only the single outcome an unwritten class can legitimately produce — a `LoadFail` on a path under `simplify_fixtures_unsupported/`.
Anything else there, including a `Mismatch`, fails the gate.
That is strictly stronger than the selector exclusion, and a class moving out of limbo needs no gate change.
The deferred set stays enforced where it belongs, by `test_deferred_fixtures.rs` and `PLAN.md` §4.

**Section arithmetic.** The 1606 sections partition as:

| group | sections | gated by |
|---|---|---|
| `*.head` | 259 | `head_sections_match_oracle` |
| basename contains `sla` | 136 | `slamap_sections_match_oracle` |
| everything else | 1211 | **U3, new** |

The third group is the complement of the first two, so the three selectors cover the corpus by construction and `every_oracle_section_is_gated` asserts it.
It takes in the 1191 `simplify_fixtures*` sections and the 20 top-level `*.ast` sections together; of its 1211, the 18 deferred `LoadFail`s are licensed structurally, leaving a floor of 1193.

Defining the third selector as a complement rather than as its own path predicate is what makes the coverage claim hold: a fixture added under a new root lands inside a gate automatically instead of falling through every one of them.
`Outcome::Skipped` needs no new policy — the shared `is_fail` classifier already does not count it, so the two `specflux.ast` skips pass through the gate untouched.

### Tasks

- [x] Read `head_sections_match_oracle` and `slamap_sections_match_oracle` end to end and note every part that must be replicated.
- [x] Add `fn is_simplify_section(...)`, defined as the complement of the `.head` and SlaMap selectors so the three partition the corpus.
- [x] Add `SIMPLIFY_ORACLE_EXCLUDED: &[(&str, bool, &str)] = &[]` with a doc comment stating the same rule the other lists carry: an entry is justified only by a diagnosed non-defect, and a divergence nobody has diagnosed is a bug to fix rather than an entry to add.
      Note in it that a deferred-class `LoadFail` does not belong there.
- [x] Add `SIMPLIFY_SECTION_FLOOR`, set from the measured count, with a comment saying what it guards against.
      Measured 1193, not the 1173 guessed above; the floor assertion caught the arithmetic error, which is what it is for.
- [x] Add `simplify_sections_match_oracle`, using the shared `is_fail` classifier unmodified, reporting unexpected failures and stale exclusions separately, and asserting `checked >= SIMPLIFY_SECTION_FLOOR`.
- [x] Verify the gate is not vacuous.
      Reintroducing the U2 divergence made it fail naming `neg_matrix_singular_diag.map (forward=false)`; reintroducing U1 made it fail naming all four `selectormap_region_simplify*` sections.
      A gate that has never been seen to fail has not been tested.
- [x] Add `every_oracle_section_is_gated`, asserting no section falls outside all three selectors and that the total is 1606, so a future fixture cannot land ungated.
- [x] Update `PLAN.md`: record that the whole corpus is gated.
- [x] Re-measure; whole-crate green; `cargo fmt --all`; clippy clean; commit.

---

## Definition of done

- [x] `transform_oracle_report` shows 0 `mismatch` across all 1606 sections (1585 pass, 18 load-fail, 2 skipped, 1 skip-override).
- [x] Three asserting gates cover 259 + 136 + 1211 = 1606 sections — the whole corpus — of which 1588 are gated outcomes and 18 are the documented unported-class load-fails, licensed by outcome rather than dropped from a selector.
- [x] `SIMPLIFY_ORACLE_EXCLUDED` is empty, as are `HEAD_ORACLE_EXCLUDED` and `SLAMAP_ORACLE_EXCLUDED`.
- [x] The new gate has been observed to fail on both reintroduced defects.
- [x] `cargo test -p ast-core` is green; `cargo fmt` and clippy clean.
- [x] `PLAN.md` reflects the new gate.

## Out of scope

- The 18 `XphMap` / `StcResourceProfile` load-fails — they need those classes ported, tracked in `PLAN.md` §4.
- The two live entries in `transform_oracle_overrides.txt` (`tnx-cheb.head off`, `neg_unitnormmap_inv_fwd_diffcentre.map golden-inv off`).
- `ALIASING_TAG_LICENSED` in `test_wcsconv.rs` — five diagnosed C aliasing artifacts, documented in `docs/issues/c-library-quirks.md`. (Superseded at the `f6762005` sync by `ISSIMP_PLACEMENT_LICENSED`, once upstream `e40e71b4` made the C side deterministic; see PLAN.md §8.)
- The seven `skip_string_compare` rows in the simplify manifest — they relax serialization comparison only; `equal()` and the transform still assert.
