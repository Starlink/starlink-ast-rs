# Simplify Seam Realignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every Mapping class exactly the simplify seams its C counterpart has, so a per-class reduction is reached the way `astSimplify` reaches it and nowhere else.

**Architecture:** C has a per-class `Simplify` seam, but only 11 of this port's classes have a C counterpart that overrides it; the other 25 keep all their reduction in `MapMerge`. The port gave all 36 a `simplified()` override. This plan gives `Mapping::simplified()` a **default body that is C's base `Simplify`** — build a one-element Mapping list, call `map_merge` to a fixpoint — then moves each of the 25 wrong overrides into that class's `map_merge` and deletes it. The 11 legitimate overrides stay. A test derived from the C source enforces the split so it cannot drift back.

**Tech Stack:** Rust 2024 (`ast-core`), C99 (`reference/starlink-ast` at pin `8d4f526f`, built with cmake), the `tracing` feature for the port-side simplify trace.

**Spec:** [`docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`](../specs/2026-08-23-simplify-seam-realignment.md)

## Global Constraints

- The oracle is `reference/starlink-ast` at pin `8d4f526f`, built with cmake. Never reach into it from `ast-core`'s `build.rs`.
- **Never hand-edit a `.simp` or `.map` reference.** Every fixture comes from `ast_tester/gen_simplify_fixtures.c` built against the pinned C, and is copied into both `reference/starlink-ast/ast_tester/simplify_fixtures/` and `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/` so the two trees stay byte-identical.
- `cargo fmt` is enforced; `cargo clippy --workspace -- -D warnings` and `cargo doc` are pre-commit hooks. Every task ends green on all three. A public doc comment may not link to a private item.
- Prose in Markdown is one sentence per line. American English.
- Comments describe the code as it is today. No references to plans, history, or previous mistakes.
- No tolerance, threshold or iteration count without an analogue in the C source.
- Never loosen a negative-control fixture to make a positive case pass.
- **`ISSIMP_DISAGREEMENTS` and `SERIAL_UNGATED` in `crates/ast-core/tests/test_simplify_regression.rs` are the audit trail for this work.** Every movement in either list is explained in the commit that causes it. Do not relax either gate.
- Every relaxation count is mirrored in `crates/ast-core/tests/test_verification_ledger.rs` and `PLAN.md` §6, updated together.
- Commit with `git commit --fixup` when amending earlier work in this plan.

## The classification, derived from C

`grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c` returns
`box circle cmpframe cmpmap cmpregion ellipse frameset interval mapping nullregion pointlist polygon prism region stc`.

**Keeps its `simplified()` override — 11 classes** (C overrides `Simplify`):

| port file | C file |
|---|---|
| `mappings/cmpmap.rs` | `cmpmap.c` |
| `frames/frameset.rs` | `frameset.c` |
| `regions/box_region.rs` | `box.c` |
| `regions/circle.rs` | `circle.c` |
| `regions/cmpregion.rs` | `cmpregion.c` |
| `regions/ellipse.rs` | `ellipse.c` |
| `regions/interval.rs` | `interval.c` |
| `regions/nullregion.rs` | `nullregion.c` |
| `regions/pointlist.rs` | `pointlist.c` |
| `regions/polygon.rs` | `polygon.c` |
| `regions/prism.rs` | `prism.c` |

**Loses its `simplified()` override — 25 classes** (C has no `Simplify`; all reduction is in `MapMerge`):

`chebymap`, `dssmap`, `grismmap`, `lutmap`, `mathmap`, `matrix`, `normmap`, `pcdmap`, `perm`, `polymap`, `ratemap`, `selectormap`, `shift`, `slamap`, `specmap`, `sphmap`, `splinemap`, `switchmap`, `timemap`, `tranmap`, `unit`, `unitnormmap`, `wcsmap`, `winmap`, `zoom`.

Of those 25, one — `chebymap` — has no `map_merge` at all, so it has nowhere to move to yet. Task 3 handles it.

**A note on "removing `simplified()` completely".** The spec asks for the seam to be deleted. Taken literally that would also delete CmpMap's and the Region family's overrides, which C genuinely has. What this plan removes is the 25 overrides that correspond to nothing, and it makes the surviving trait method mean what C's `Simplify` means by giving it C's base body as its default. If a reviewer wants the literal reading instead, stop at Task 4 and raise it — the driver added there is the same either way.

## File Structure

| file | responsibility after this plan |
|---|---|
| `crates/ast-core/src/mappings/simplify.rs` | adds `base_simplify`, C's base `Simplify` (one-element list, `map_merge` to fixpoint). Existing merge core unchanged. |
| `crates/ast-core/src/mappings/mod.rs` | `Mapping::simplified()` gains a default body calling `base_simplify`; the public `simplify()` is untouched. |
| `crates/ast-core/src/mappings/<class>.rs` × 25 | `simplified()` deleted; its body folded into that class's `map_merge`. |
| `crates/ast-core/src/mappings/chebymap.rs` | gains a `map_merge` delegating to PolyMap's, which is what C's vtab does. |
| `crates/ast-core/tests/test_mapmerge_audit.rs` | gains `only_c_simplify_classes_override_simplified`, reading the keeper list from this plan's table. |
| `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c` | gains the characterisation fixtures Task 1 needs. Goes upstream. |
| `docs/audits/mapmerge/<class>.md` | each moved class's checklist gains the row for the moved reduction. |

---

## Task 1: Characterisation fixtures for the classes with nothing to lose

Five of the 25 have a `simplified()` whose reduction no fixture currently demands, so moving it would be unobservable — and an unobservable move is one that can silently drop the reduction. This task closes that before anything moves.

The five are the ones whose `simplified()` is reached only from the public entry: `chebymap` (no `map_merge`), plus `mathmap`, `sphmap`, `unitnormmap`, `wcsmap`, whose `map_merge` never consults `simplified()`.

**Files:**
- Modify: `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`
- Create: 10 files under `reference/starlink-ast/ast_tester/simplify_fixtures/` and the same 10 under `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Modify: `crates/ast-core/tests/test_verification_ledger.rs`
- Modify: `PLAN.md`

**Interfaces:**
- Consumes: nothing.
- Produces: five `<class>_lone_reduce` positive fixtures and five `neg_<class>_no_reduce` negative controls, wired in the manifest. Later tasks rely on these failing the moment a moved reduction is lost.

- [ ] **Step 1: Confirm which of the five have an unobserved reduction**

For each class, check whether its `map_merge` reaches `simplified()`:

```bash
cd /Users/timj/work/lsst/starlink-astrs
for c in chebymap mathmap sphmap unitnormmap wcsmap; do
  f=$(ls crates/ast-core/src/mappings/$c.rs crates/ast-core/src/mappings/$c/mod.rs 2>/dev/null | head -1)
  printf "%-12s map_merge=%s reaches_simplified=%s\n" "$c" \
    "$(grep -c 'fn map_merge(' $f)" \
    "$(grep -cE 'self_reduce_map_merge|try_self_reduce' $f)"
done
```

Expected: every line reads `reaches_simplified=0`.
If any reads non-zero, that class is already covered — drop it from this task and note it in the commit message.

- [ ] **Step 2: Read what each `simplified()` actually reduces**

```bash
for c in chebymap mathmap sphmap unitnormmap wcsmap; do
  f=$(ls crates/ast-core/src/mappings/$c.rs crates/ast-core/src/mappings/$c/mod.rs 2>/dev/null | head -1)
  echo "=== $c ==="; rg -n --color never 'fn simplified' -A 30 "$f" | head -34
done
```

Write down, per class, the condition under which the body returns something
different from `self`. That condition is what the fixture must satisfy. A body
that only ever returns `Ok(Box::new(self.clone()))` has no reduction: record that
class as needing no fixture and skip it in steps 3 to 6.

- [ ] **Step 3: Add the generator cases**

Append to the `gen_audit_gap_fixtures` function in
`reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`, just before its
closing brace. One pair per class that step 2 found a reduction for. The
ChebyMap pair is written out here; the other four follow the same shape with the
condition step 2 recorded.

```c
    /* A ChebyMap whose forward transformation holds only T_0 and T_1 terms over
       bounds that are not the identity interval. In C a ChebyMap inherits
       PolyMap's MapMerge (polymap.c:2439 is the only place the slot is filled),
       whose replace-with-simpler half rebuilds an all-linear polynomial as a
       MatrixMap and ShiftMap. The negative control is the same ChebyMap with a
       T_2 term, which is not linear and must survive. */
    {
        double lbnd[] = {0.0};
        double ubnd[] = {10.0};
        double coeff_f[] = {2.0, 1.0, 1.0};   /* 2 * T_1(x') for output 1 */
        AstChebyMap *cm = astChebyMap(1, 1, 1, coeff_f, 0, NULL,
                                      lbnd, ubnd, NULL, NULL, " ");
        write_fixture(dir, "cheby_linear_lone_reduce", (AstMapping*)cm);
        cm = astAnnul(cm);
    }
    {
        double lbnd[] = {0.0};
        double ubnd[] = {10.0};
        double coeff_f[] = {2.0, 1.0, 2.0};   /* 2 * T_2(x'), not linear */
        AstChebyMap *cm = astChebyMap(1, 1, 1, coeff_f, 0, NULL,
                                      lbnd, ubnd, NULL, NULL, " ");
        write_negative_fixture(dir, "neg_cheby_quadratic_no_reduce",
                               (AstMapping*)cm);
        cm = astAnnul(cm);
    }
```

- [ ] **Step 4: Generate, and read what C actually did**

```bash
cd /Users/timj/work/lsst/starlink-astrs/reference/starlink-ast
cmake --build build --target gen_simplify_fixtures -j8
./build/ast_tester/gen_simplify_fixtures 2>&1 | grep -E "lone_reduce|no_reduce"
cat ast_tester/simplify_fixtures/cheby_linear_lone_reduce.simp
```

Expected: the generator names every fixture, and each `.simp` differs from its
`.map` in more than the `IsSimp` line.

If a `.simp` is identical to its `.map` apart from `IsSimp`, C does **not**
perform that reduction. That is a finding, not a setback: the port's
`simplified()` is doing something C does not, and the class belongs in Task 2's
"delete outright" bucket rather than the "move" bucket. Record it in the commit
message and remove the pair.

- [ ] **Step 5: Copy into both trees and wire the manifest**

```bash
cd /Users/timj/work/lsst/starlink-astrs
U=reference/starlink-ast/ast_tester/simplify_fixtures
F=crates/ast-core/tests/fixtures/native/reference/simplify_fixtures
for b in cheby_linear_lone_reduce; do cp $U/$b.map $U/$b.simp $F/; done
for b in neg_cheby_quadratic_no_reduce; do cp $U/$b.map $F/; done
```

Append to `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`:

```
# Reductions that live only in a class's simplified() and that no other fixture
# demands. Each pair is a positive case C does reduce and a negative control C
# leaves alone, so moving the reduction into map_merge cannot silently drop it.
cheby_linear_lone_reduce | simplify_fixtures/cheby_linear_lone_reduce.map | simplify_fixtures/cheby_linear_lone_reduce.simp |
neg_cheby_quadratic_no_reduce | simplify_fixtures/neg_cheby_quadratic_no_reduce.map | simplify_fixtures/neg_cheby_quadratic_no_reduce.map |
```

- [ ] **Step 6: Run and record the starting state**

Run: `cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression"`

Expected: the fixture count rises by the number of pairs added. Some new
positives may **fail** — that is the point of the task, and each failure is a
gap the move in Task 5 has to close. Record the failing names.

If a new positive fails, add it to `SERIAL_UNGATED` in
`crates/ast-core/tests/test_simplify_regression.rs` with the reason
`"awaiting the simplified()-to-map_merge move, PLAN.md §10.16"`, so the suite is
green and the debt is named. Task 5 removes the entry.

- [ ] **Step 7: Update the ledger and PLAN.md together**

Run: `cargo test -p ast-core --test test_verification_ledger`
The failure message states the new negative-fixture count. Set that number in the
`"simplify manifest: negative fixtures stripping IsSimp"` arm of
`crates/ast-core/tests/test_verification_ledger.rs`, and update the matching
`simplify regression` rows in `PLAN.md` §6 in the same edit.

- [ ] **Step 8: Verify everything**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo clippy -p ast-core --all-targets -- -D warnings
```

Expected: `0`, `194 passed`, no warnings.

- [ ] **Step 9: Commit**

```bash
git add crates/ast-core/tests reference/starlink-ast PLAN.md
git commit -m "test(simplify): characterise the reductions that live only in simplified()

Five classes keep a reduction in simplified() that no fixture demands, because
their map_merge never consults it. Moving such a reduction is unobservable, and
an unobservable move can silently drop it.

Each pair is a positive case the pinned C does reduce and a negative control it
leaves alone, generated by ast_tester/gen_simplify_fixtures.c so the upstream
copies are byte-identical."
```

---

## Task 2: Record the classification, and gate it

The keeper list is derived from the C source, so a test can hold it. This task writes the classification down where the work will read it and makes drift impossible.

**Files:**
- Modify: `crates/ast-core/tests/test_mapmerge_audit.rs`
- Modify: `docs/audits/mapmerge/README.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `C_SIMPLIFY_CLASSES: &[&str]` in `test_mapmerge_audit.rs`, and the test `only_c_simplify_classes_override_simplified`, which fails for any class outside that list that still has a `simplified()` override. Tasks 5 onward drive that failure list to empty.

- [ ] **Step 1: Write the failing test**

Append to `crates/ast-core/tests/test_mapmerge_audit.rs`:

```rust
/// Port files whose C counterpart overrides the `Simplify` vtab slot, and which
/// may therefore carry a `simplified()` override.
///
/// Derived from the C source, not chosen:
/// `grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c`
/// returns `box circle cmpframe cmpmap cmpregion ellipse frameset interval
/// mapping nullregion pointlist polygon prism region stc`. `mapping` is the base
/// implementation every other class inherits, and `cmpframe` and `stc` have no
/// port counterpart yet.
///
/// For every class *not* listed here, C keeps all its reduction in `MapMerge`,
/// and `astSimplify` reaches it only by nominating the Mapping in a list of one.
const C_SIMPLIFY_CLASSES: &[&str] = &[
    "mappings/cmpmap.rs",
    "frames/frameset.rs",
    "regions/box_region.rs",
    "regions/circle.rs",
    "regions/cmpregion.rs",
    "regions/ellipse.rs",
    "regions/interval.rs",
    "regions/nullregion.rs",
    "regions/pointlist.rs",
    "regions/polygon.rs",
    "regions/prism.rs",
];

/// A class may override `simplified()` only if its C counterpart overrides
/// `Simplify`. Anything else is a seam C does not have, and a reduction placed
/// there is unreachable from the nominate loop — which is C's only route to it.
#[test]
fn only_c_simplify_classes_override_simplified() {
    let root = repo_root().join("crates/ast-core/src");
    let mut stray = Vec::new();

    for dir in ["mappings", "frames", "regions"] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).expect("source dir readable") {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("source readable");
                if !text.contains("fn simplified(&self) -> AstResult<DynMapping> {") {
                    continue;
                }
                let rel = path
                    .strip_prefix(&root)
                    .expect("under src")
                    .to_string_lossy()
                    .replace('\\', "/");
                if !C_SIMPLIFY_CLASSES.contains(&rel.as_str()) {
                    stray.push(rel);
                }
            }
        }
    }

    stray.sort();
    assert!(
        stray.is_empty(),
        "these classes override `simplified()` and their C counterpart does not \
         override `Simplify`, so the reduction belongs in `map_merge` where the \
         nominate loop can reach it. See \
         docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md: {stray:#?}"
    );
}
```

- [ ] **Step 2: Run it to verify it fails, and that it names the right set**

Run: `cargo test -p ast-core --test test_mapmerge_audit only_c_simplify -- --nocapture`

Expected: FAIL, listing 25 paths — `mappings/chebymap.rs` through
`mappings/zoom.rs`, and nothing under `regions/` or `frames/`.

If a region or `frames/frameset.rs` appears, the keeper list has a typo: the path
in `C_SIMPLIFY_CLASSES` must match the path relative to `crates/ast-core/src`
exactly, including the `_region` suffix on `box_region.rs`.

- [ ] **Step 3: Mark the test as the plan's progress meter**

The test fails until Task 6. Add `#[ignore]` with the reason, so the suite stays
green while the work proceeds:

```rust
#[test]
#[ignore = "fails until the 25 stray simplified() overrides have moved; \
            see docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md"]
fn only_c_simplify_classes_override_simplified() {
```

- [ ] **Step 4: Document the seam rule where the checklists are read**

Append to `docs/audits/mapmerge/README.md`:

```markdown
## Which seam a reduction belongs in

C has two per-class seams: `Simplify`, overridden by 11 classes, and `MapMerge`,
overridden per class. For every class outside that 11, **all** reduction lives in
`MapMerge`, and `astSimplify` reaches it only by nominating the Mapping in a list
of one (`mapping.c:16871-16883`).

So a row whose Rust site is a `simplified()` body, in a class whose C counterpart
has no `Simplify` override, is not `ported` — it is in a seam C does not have and
the nominate loop cannot reach it.
`only_c_simplify_classes_override_simplified` in `test_mapmerge_audit.rs` holds
the list of 11 and the reasoning.
```

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_mapmerge_audit 2>&1 | grep "test result"
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/tests/test_mapmerge_audit.rs docs/audits/mapmerge/README.md
git commit -m "test: gate which classes may override simplified()

C overrides the Simplify vtab slot in 11 of this port's classes and in none of
the other 25, where every reduction lives in MapMerge and astSimplify reaches it
only by nominating the Mapping in a list of one.

The test holds that list, derived from the C source rather than chosen, and is
ignored until the 25 stray overrides have moved."
```

Expected: `0` failures, the audit test reporting one ignored.

---

## Task 3: Give ChebyMap the `map_merge` C gives it

`chebymap` is the one class with a `simplified()` and no `map_merge`, so it has nowhere to move to. In C it has no `Simplify` **and** no `MapMerge`: `polymap.c:2439` is the only place the slot is filled, so a ChebyMap's `MapMerge` *is* PolyMap's.

**Files:**
- Modify: `crates/ast-core/src/mappings/chebymap.rs`
- Modify: `docs/audits/mapmerge/chebymap.md`
- Modify: `PLAN.md`

**Interfaces:**
- Consumes: `cheby_linear_lone_reduce` and `neg_cheby_quadratic_no_reduce` from Task 1.
- Produces: `ChebyMap::map_merge`, delegating to `PolyMap`'s. No new public API.

- [ ] **Step 1: Confirm C's vtab claim before relying on it**

```bash
cd /Users/timj/work/lsst/starlink-astrs
grep -n 'MapMerge = MapMerge' reference/starlink-ast/src/chebymap.c reference/starlink-ast/src/polymap.c
grep -c 'static int MapMerge' reference/starlink-ast/src/chebymap.c
```

Expected: one hit, in `polymap.c`, and `0` for `chebymap.c`.
If `chebymap.c` defines its own `MapMerge`, this task is wrong: port that instead
of delegating, and say so in the commit message.

- [ ] **Step 2: Read PolyMap's `map_merge` signature and body shape**

Run: `rg -n --color never 'fn map_merge' -A 40 crates/ast-core/src/mappings/polymap.rs | head -44`

Note whether the body reads any `PolyMap`-only field directly. If it does,
delegation needs a `PolyMap` view of the ChebyMap rather than a call on `self`,
and step 4's code must build that view.

- [ ] **Step 3: Write the failing test**

Add to the bottom of `crates/ast-core/src/mappings/chebymap.rs`, inside the
existing `#[cfg(test)] mod tests` block:

```rust
    /// A ChebyMap nominated inside a list must reach the same replace-with
    /// -simpler half of PolyMap's `MapMerge` that C reaches through the shared
    /// vtab slot (`polymap.c:2439`), so a linear ChebyMap is reduced there and
    /// not only when it is simplified on its own.
    #[test]
    fn linear_chebymap_reduces_when_nominated_in_a_list() {
        let lbnd = [0.0];
        let ubnd = [10.0];
        let terms = vec![PolyTerm {
            out_axis: 1,
            coeff: 2.0,
            powers: vec![1],
        }];
        let cheby = ChebyMap::new(terms, None, &lbnd, &ubnd)
            .expect("linear ChebyMap constructs");
        let unit = UnitMap::new(1);
        let pair = CmpMap::new(
            Box::new(cheby) as DynMapping,
            Box::new(unit) as DynMapping,
            Composition::Series,
        )
        .expect("series pair constructs");

        let simplified = Mapping::simplify(&pair).expect("simplify succeeds");
        assert!(
            simplified.as_any().downcast_ref::<ChebyMap>().is_none(),
            "a linear ChebyMap nominated in a list should have been replaced, \
             got {simplified:?}"
        );
    }
```

Adjust the `ChebyMap::new` call to the constructor's real signature, which
step 2's output shows; the assertion is the part that matters.

- [ ] **Step 4: Run it to verify it fails**

Run: `cargo test -p ast-core --lib linear_chebymap_reduces -- --nocapture`
Expected: FAIL — the result is still a `ChebyMap`, because `ChebyMap` has no
`map_merge` and takes the trait default, which only tries the neighbours.

- [ ] **Step 5: Add the delegation**

Insert into the `impl Mapping for ChebyMap` block in
`crates/ast-core/src/mappings/chebymap.rs`, immediately before `fn simplified`:

```rust
    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // `chebymap.c` fills no `MapMerge` slot. `polymap.c:2439` is the only
        // place it is filled, so a ChebyMap's `MapMerge` is PolyMap's: the
        // coefficient consolidation and the linear replacement both apply,
        // reading the Chebyshev coefficients as PolyMap reads its own.
        self.as_polymap_view().map_merge(ctx, where_, series)
    }
```

If step 2 showed PolyMap's body only touches `&dyn Mapping` methods, replace the
call with `super::polymap::poly_map_merge(self, ctx, where_, series)` and give
that helper the `&dyn Mapping` signature. If it reads `PolyMap` fields, add:

```rust
    /// This ChebyMap's terms and bounds as a `PolyMap`, for the `MapMerge` the
    /// two classes share in C.
    fn as_polymap_view(&self) -> PolyMap {
        PolyMap::from_terms(self.forward_terms(), self.inverse_terms())
            .expect("a ChebyMap's terms are a valid PolyMap")
    }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p ast-core --lib linear_chebymap_reduces -- --nocapture`
Expected: PASS.

- [ ] **Step 7: Run the corpus**

Run: `cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression|equal\(\)|left:" `

Expected: `cheby_linear_lone_reduce` passes and its `SERIAL_UNGATED` entry from
Task 1 step 6 can be deleted. `neg_cheby_quadratic_no_reduce` still passes.

**If the linear replacement loses the ChebyMap's domain scaling** — the open
question in `PLAN.md` §10.2 — the negative control or the positive will disagree
with C on values rather than class. Stop and settle it against the oracle before
going further: build a two-point probe per
`docs/issues/c-oracle-probe-recipe.md`, transform the same position through C's
simplified ChebyMap and the port's, and compare. If C loses the scaling, do not
mirror it: revert this task, record the finding in
`docs/issues/c-library-quirks.md` and `PLAN.md` §10.2, and raise it upstream.

- [ ] **Step 8: Update the checklist and PLAN.md**

In `docs/audits/mapmerge/chebymap.md`, change the `gap: PLAN.md §10.2` verdict on
rows 4 to 12 to `ported` with the Rust site `chebymap.rs`, and replace the closing
note with what step 7 established about the domain scaling.

In `PLAN.md`, mark §10.2 `FIXED` or record the oracle finding, whichever step 7
produced.

- [ ] **Step 9: Verify and commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core PLAN.md docs
git commit -m "simplify(chebymap): reach PolyMap's MapMerge, as C's vtab does

chebymap.c fills no MapMerge slot; polymap.c:2439 is the only place it is
filled, so a ChebyMap's MapMerge is PolyMap's. The port had neither, so a
ChebyMap nominated inside a list took the trait default and only tried its
neighbours."
```

Expected: `0` failures, `194 passed`, no warnings.

---

## Task 4: Add C's base `Simplify` as the trait default

**Files:**
- Modify: `crates/ast-core/src/mappings/simplify.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs:282`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<DynMapping>` in `simplify.rs`, and `Mapping::simplified()` becomes a provided method calling it. Tasks 5 and 6 delete the overrides that currently shadow it.

- [ ] **Step 1: Read C's base `Simplify` loop once more**

Run: `sed -n '16860,16900p' reference/starlink-ast/src/mapping.c`

Note the three things the loop does: it calls
`astMapMerge( map, 0, 1, &nmap, &map_list, &invert_list )`, it breaks when
`nmap > 1` **or** `modified < 0`, and it treats "no simplification" as returning a
clone. The port's version must do all three.

- [ ] **Step 2: Write the failing test**

Append to the `#[cfg(test)] mod tests` block in
`crates/ast-core/src/mappings/simplify.rs`:

```rust
    /// C's base `Simplify` builds a one-element list and calls `astMapMerge`
    /// until nothing changes or the list grows past one
    /// (`mapping.c:16871-16883`). A ZoomMap of factor 1 is a UnitMap, and
    /// `zoom_map_merge` says so, so the base driver must reach that verdict
    /// without any per-class `simplified()` in the way.
    #[test]
    fn base_simplify_reaches_a_classes_own_map_merge() {
        let zoom = ZoomMap::new(2, 1.0).expect("unit-factor ZoomMap constructs");
        let out = base_simplify(&zoom).expect("base_simplify succeeds");
        assert!(
            out.as_any().is::<UnitMap>(),
            "expected a UnitMap from a unit-factor ZoomMap, got {out:?}"
        );
    }

    /// A Mapping its own `map_merge` cannot reduce comes back unchanged rather
    /// than erroring, which is C returning a clone.
    #[test]
    fn base_simplify_returns_a_clone_when_nothing_reduces() {
        let math = ZoomMap::new(1, 3.0).expect("ZoomMap constructs");
        let out = base_simplify(&math).expect("base_simplify succeeds");
        let z = out
            .as_any()
            .downcast_ref::<ZoomMap>()
            .expect("still a ZoomMap");
        assert_eq!(z.factor(), 3.0);
    }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p ast-core --lib base_simplify -- --nocapture`
Expected: FAIL to compile — `cannot find function base_simplify in this scope`.

- [ ] **Step 4: Write the driver**

Add to `crates/ast-core/src/mappings/simplify.rs`, next to `default_map_merge`:

```rust
/// C's base `Simplify` (`mapping.c:16752`), the implementation every class
/// without its own `Simplify` override inherits.
///
/// Builds a one-element Mapping list and hands it to the class's own
/// `map_merge` repeatedly, stopping when the merge reports no change or the
/// list grows past one element (`mapping.c:16871-16883`). Returns a clone when
/// nothing reduced, which is what C returns.
///
/// This is the only route C has to a per-class reduction for such a class, so it
/// is the only route this port should have either.
pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<DynMapping> {
    let mut ctx = MergeCtx::from_single(this.clone_box());
    let mut simpler = false;

    // C loops without a bound; the guard mirrors the oscillation break the merge
    // loop already applies (`MergeCtx::is_frozen`) rather than adding a limit of
    // its own.
    while !ctx.is_frozen(0) {
        let nominee = ctx.entry(0).0.clone_box();
        trace::map_merge(nominee.as_ref(), 0, true, ctx.len());
        let modified = nominee.map_merge(&mut ctx, 0, true);
        trace::map_merge_result(nominee.as_ref(), modified);

        if ctx.len() > 1 || modified.is_none() {
            break;
        }
        simpler = true;
    }

    if !simpler {
        return Ok(this.clone_box());
    }
    reconstruct(std::mem::take(&mut ctx.list), true)
}
```

If `MergeCtx` has no `from_single` or `is_frozen(usize)`, read its definition
(`rg -n 'impl MergeCtx' -A 40 crates/ast-core/src/mappings/simplify.rs`) and use
the constructor and freeze accessor it does have. The shape — one element, loop,
break on growth or no change — is what must not change.

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p ast-core --lib base_simplify -- --nocapture`
Expected: PASS, both tests.

- [ ] **Step 6: Make `simplified()` a provided method**

In `crates/ast-core/src/mappings/mod.rs`, replace line 282's declaration:

```rust
    fn simplified(&self) -> AstResult<DynMapping>;
```

with:

```rust
    /// Reduce this Mapping without recording the result, C's `Simplify` vtab
    /// slot.
    ///
    /// The default is C's base implementation: build a one-element Mapping list
    /// and hand it to this class's own [`Mapping::map_merge`] until nothing
    /// changes (`mapping.c:16871-16883`). Override it only if the matching C
    /// class overrides `Simplify` — `CmpMap`, `FrameSet` and the Region family
    /// do, and nothing else does. `only_c_simplify_classes_override_simplified`
    /// in `tests/test_mapmerge_audit.rs` holds that list and the reasoning.
    ///
    /// The public [`Mapping::simplify`] wraps this with the `IsSimple`
    /// short-circuit and the record, which is all `astSimplify_` adds
    /// (`mapping.c:24755`).
    fn simplified(&self) -> AstResult<DynMapping> {
        simplify::base_simplify(self.as_dyn_mapping())
    }
```

If the trait has no `as_dyn_mapping` upcast helper, add one beside it:

```rust
    /// This Mapping as `&dyn Mapping`, so provided methods can call free
    /// functions that take a trait object.
    fn as_dyn_mapping(&self) -> &dyn Mapping;
```

and implement it as `fn as_dyn_mapping(&self) -> &dyn Mapping { self }` in each
class, which the compiler will demand one file at a time.

- [ ] **Step 7: Verify nothing moved**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```

Expected: `0`, the same fixture and failure counts as before this task, and
`194 passed`. Every class still overrides `simplified()`, so the default is
unreachable for now and behaviour must be unchanged. **A change here means the
default is being reached by something, which means a class was already missing an
override.** Find it before continuing.

- [ ] **Step 8: Commit**

```bash
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/src
git commit -m "simplify: add C's base Simplify as the simplified() default

C's base Simplify (mapping.c:16752) builds a one-element Mapping list and calls
astMapMerge until nothing changes or the list grows past one. Every class whose C
counterpart does not override Simplify inherits it, and it is C's only route to
such a class's reduction.

base_simplify is that loop, and simplified() now defaults to it. No override is
removed yet, so nothing is reached differently and the corpus is unchanged."
```

---

## Task 5: Move the reductions, one class per commit

24 classes remain (25 minus `chebymap`, done in Task 3). Each is the same five steps against a different file. The order is by fixture coverage: a class whose reduction a fixture already pins moves safely.

**Files, per class:**
- Modify: `crates/ast-core/src/mappings/<class>.rs`
- Modify: `docs/audits/mapmerge/<class>.md`

**Interfaces:**
- Consumes: `base_simplify` from Task 4; the Task 1 fixtures.
- Produces: nothing new. Each class ends with no `simplified()` and a `map_merge` that performs what it used to.

**Order.** Group A first — their `map_merge` already routes through a self-reduce helper, so the reduction is already reached and deleting `simplified()` is a no-op that the default absorbs:

`dssmap`, `normmap`, `ratemap`, `selectormap`, `splinemap`, `switchmap`.

Group B next — small, heavily covered, `map_merge` present but not consulting `simplified()`:

`unit`, `zoom`, `shift`, `perm`, `matrix`, `winmap`.

Group C — the conversion classes, whose `simplified()` bodies are large:

`slamap`, `specmap`, `timemap`, `lutmap`, `polymap`, `pcdmap`, `grismmap`, `sphmap`, `wcsmap`, `unitnormmap`, `mathmap`.

Group D last — the two that recurse into encapsulated Mappings and are
orientation-sensitive:

`tranmap`, and re-check `ratemap` if Group A left anything.

### The two archetypes

**Archetype 1, a reduction with no encapsulated Mapping** (Group B and most of C).
`ZoomMap` is the worked example. Its `simplified()` today:

```rust
    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified() {
            return Ok(Box::new(self.clone()));
        }
        let effective_factor = if self.inverted {
            1.0 / self.factor
        } else {
            self.factor
        };
        if effective_factor == 1.0 {
            Ok(Box::new(UnitMap::new(self.ncoord)))
        } else {
            Ok(Box::new(ZoomMap { /* forward copy */ }))
        }
    }
```

That is C's replace-with-simpler half of `zoommap.c`'s `MapMerge`, so it belongs
at the top of `zoom_map_merge`, before the run condensing:

```rust
pub(crate) fn zoom_map_merge(
    zoom: &ZoomMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // C's replace-with-simpler step, before any neighbour is considered
    // (zoommap.c:802-816): a unit effective zoom is a UnitMap, and an inverted
    // ZoomMap is stored forward with the reciprocal factor.
    let eff_inv = ctx.entry(where_).1;
    let eff = if eff_inv != zoom.is_inverted() {
        1.0 / zoom.factor()
    } else {
        zoom.factor()
    };
    if (eff - 1.0).abs() <= 8.0 * f64::EPSILON {
        ctx.set_pair_single(where_, (Box::new(UnitMap::new(zoom.ncoord())), false, false));
        return Some(where_);
    }
    if eff_inv != zoom.is_inverted() {
        let fwd = ZoomMap::new(zoom.ncoord(), eff).ok()?;
        ctx.set_pair_single(where_, (Box::new(fwd), false, false));
        return Some(where_);
    }

    // ... existing body unchanged from here ...
```

The `8.0 * f64::EPSILON` is the comparison `zoom_map_merge` already uses for its
run condensing; reuse whatever the file already does rather than introducing a
new tolerance.

**Archetype 2, a reduction over an encapsulated Mapping** (Group A and D).
`RateMap` is the worked example and it is already done — `ratemap.rs`'s
`map_merge` routes through `self_reduce_map_merge`, and its `simplified()`
orients the child to `invert_snapshot` before reducing it. For these classes the
move is a deletion, not a transplant: the default absorbs the call and the
reduction is already reached. **The orientation handling must move with the body,
not be left behind** — a transplant that drops it reproduces the §10.1 bug.

### Steps, per class

- [ ] **Step 1: Read the class's `simplified()` and its `map_merge`**

```bash
c=zoom   # the class being moved
f=crates/ast-core/src/mappings/$c.rs
rg -n --color never 'fn simplified' -A 40 $f | head -44
rg -n --color never 'fn map_merge' -A 12 $f | head -16
```

Decide the archetype. If `map_merge` delegates to a helper in `simplify.rs`, the
reduction goes at the top of that helper; if the body is inline, it goes at the
top of the body.

- [ ] **Step 2: Record the baseline**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|left:"
```

Copy the summary line and the `ISSIMP_DISAGREEMENTS` `left:` list somewhere. Any
movement in either after step 4 is this class's doing and must be explained.

- [ ] **Step 3: Move the body**

For archetype 1, insert the reduction at the top of `map_merge` (or its helper)
per the ZoomMap example above, then delete the `simplified()` impl from the class
entirely. For archetype 2, delete the `simplified()` impl only, after confirming
`map_merge` reaches the same logic through `self_reduce_map_merge` or
`try_self_reduce`.

- [ ] **Step 4: Run the corpus and compare against the baseline**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|equal\(\)|left:|right:"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```

Expected: identical to the baseline, and `194 passed`.

Three outcomes and what each means:

- **Identical** — the move is correct. Continue.
- **A fixture that failed now passes** — the reduction was unreachable before and
  is now reached. Delete its `SERIAL_UNGATED` or `ISSIMP_DISAGREEMENTS` entry in
  the same commit and say which gap it closes.
- **A fixture that passed now fails** — stop. Either the reduction moved to the
  wrong position in `map_merge` relative to the neighbour steps, or it lost its
  orientation handling. Diff the two traces to see which:

```bash
cd /Users/timj/work/lsst/starlink-astrs
D=${TMPDIR:-/tmp}/ast-trace3
FIX=crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/<name>.map
AST_TRACE=1 $D/gen_simp $FIX /dev/null 2> /tmp/c.txt
cargo run -q --features tracing --example simp_one -- $FIX 2> /tmp/p.txt >/dev/null
grep MAPMERGE /tmp/c.txt | sed 's/^ *//' | awk '{print $2, $4, $5}' > /tmp/cm.txt
sed 's/\x1b\[[0-9;]*m//g' /tmp/p.txt | grep MAPMERGE \
  | sed 's/.*class="\([^"]*\)".*series=\([0-9]*\) nmap=\([0-9]*\).*/\1 series=\2 nmap=\3/' > /tmp/pm.txt
diff /tmp/cm.txt /tmp/pm.txt | head -20
```

The first differing line names the nomination that went wrong.

- [ ] **Step 5: Update the class's checklist**

In `docs/audits/mapmerge/<class>.md`, change the Rust site of every row that
pointed into `simplified()` to its new `map_merge` location. If a row's verdict
was `gap` because the reduction was unreachable, it becomes `ported`.

- [ ] **Step 6: Commit**

```bash
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/src/mappings/$c.rs docs/audits/mapmerge/$c.md
git commit -m "simplify($c): move the reduction into map_merge

C keeps every reduction for this class in MapMerge -- $c.c has no Simplify
override -- and astSimplify reaches it only by nominating the Mapping in a list
of one. The port had it in a simplified() override the nominate loop never
consulted.

Corpus unchanged: <paste the summary line>."
```

Repeat steps 1 to 6 for each of the 24 classes, in the group order above.

---

## Task 6: Turn the gate on

**Files:**
- Modify: `crates/ast-core/tests/test_mapmerge_audit.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `PLAN.md`
- Modify: `docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`

**Interfaces:**
- Consumes: Tasks 3 to 5 complete.
- Produces: `only_c_simplify_classes_override_simplified` passing without `#[ignore]`.

- [ ] **Step 1: Remove the ignore**

Delete the `#[ignore = …]` attribute added in Task 2 step 3.

- [ ] **Step 2: Run it**

Run: `cargo test -p ast-core --test test_mapmerge_audit only_c_simplify -- --nocapture`
Expected: PASS.

If it still lists classes, those are the ones Task 5 has not reached. Finish them
rather than adding them to the keeper list — the keeper list is derived from C and
is not the place to record incomplete work.

- [ ] **Step 3: Make the trait doc final**

In `crates/ast-core/src/mappings/mod.rs`, the `simplified()` doc written in
Task 4 step 6 says overriding is for the 11 classes only. Confirm that is now
true and add the count:

```rust
    /// Override it only if the matching C class overrides `Simplify`: `CmpMap`,
    /// `FrameSet` and the nine Region classes do, and the other 25 do not.
```

- [ ] **Step 4: Close the PLAN.md entries**

Mark §10.16 `FIXED` with what the work established, and re-check §10.1, §10.3
and §10.7 — each cited the seam as its cause, and §10.3's nomination counts were
measured against a C that has since been fixed (`4f2fc0ed`), so re-measure them
against pin `8d4f526f` before writing any conclusion.

- [ ] **Step 5: Mark the spec done**

Change the spec's `**Status:** proposed, not started.` to
`**Status:** implemented, <date>.` and add a closing section recording what the
work found that the spec did not predict.

- [ ] **Step 6: Full verification and commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression"
cargo clippy -p ast-core --all-targets -- -D warnings
cargo clippy -p ast-core --all-targets --features tracing -- -D warnings
cargo doc -p ast-core --no-deps 2>&1 | grep -cE "^error"
git add crates/ast-core PLAN.md docs
git commit -m "test: require the simplify seams to match C's

Every class whose C counterpart has no Simplify override now keeps its reduction
in map_merge, where astSimplify reaches it by nominating the Mapping in a list of
one. The 11 classes whose C counterpart does override Simplify keep theirs.

only_c_simplify_classes_override_simplified enforces the split from the C source,
so the seam cannot drift back."
```

Expected: `0` failures, `194 passed`, no clippy warnings either way, `0` doc
errors.

---

## Self-review

**Spec coverage.** Step 1 of the spec (pin the behaviour with oracle fixtures
first) is Task 1. Step 2 (sweep and classify) is Task 2, and the classification
is stronger than the spec's — derived from `grep`ping the C source rather than
listed by hand, which also caught that `mathmap` and `frameset` belong in the
count and that `chebymap` needs Task 3 before it can move at all. Step 3 (give
the driver C's base `Simplify`) is Task 4. Step 4 (move bodies, one class per
commit) is Task 5, with the spec's suggested `ZoomMap`/`UnitMap` start kept as
Group B. Step 5 (delete the trait method) is Task 6, narrowed: the trait method
survives as C's base `Simplify` and only the 25 wrong overrides go, which the
plan flags as a deliberate departure at the end of the classification section.

The spec's four named risks all appear: the merge core's blast radius (Task 5
step 2's baseline and step 4's three outcomes), `IsSimp` counts moving in both
directions (a Global Constraint plus Task 5 step 4), Region `Simplify` being a
real seam (the keeper table and Task 2's gate), and the orientation-sensitive
classes (Group D and Archetype 2's warning).

**Placeholder scan.** Task 1 steps 3 and 5 give one worked class rather than five
because steps 1 and 2 determine which of the five have a reduction at all —
writing five sets of fixture code now would be inventing conditions before
reading the bodies. Task 5 gives two archetypes with full code rather than 24
transplants, and names which archetype each class is; that is the content, not a
deferral. Task 3 steps 3 and 5 carry conditional code because the ChebyMap
constructor signature and PolyMap's field access are read in step 2 — each branch
is written out.

**Type consistency.** `base_simplify(this: &dyn Mapping) -> AstResult<DynMapping>`
is used with that signature in Task 4 steps 2, 4 and 6. `MergeCtx::from_single`,
`is_frozen`, `entry`, `len` and `set_pair_single` appear in Task 4 step 4 and
Task 5's archetype 1, with step 4 instructing the reader to substitute the real
constructor if it differs. `C_SIMPLIFY_CLASSES` holds paths relative to
`crates/ast-core/src`, matching what the test computes in Task 2 step 1 and what
Task 2 step 2 warns about. `trace::map_merge` and `trace::map_merge_result` are
the signatures already in `simplify.rs`.

**One gap I could not close.** Task 5's per-class steps cannot state each class's
expected corpus movement in advance, because that is what the task measures.
Step 2's baseline capture is the substitute: the reader records the numbers before
touching the file, so "identical to the baseline" is checkable without my knowing
what the baseline is.
