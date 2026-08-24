# Simplify Seam Realignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Leave one way to simplify a Mapping, as C has, so the `IsSimple` short-circuit and the record apply to every reduction instead of to the eighth of them that happen to go through the front door.

**Architecture:** C's `Simplify` vtab slot has exactly **one** caller in the whole library — `mapping.c:24772`, inside `astSimplify_`. It is a private dispatch behind the short-circuit and the stamp, not an API. The port's `simplified()` is a public trait method with **65 call sites against `simplify()`'s 8**, each bypassing both. This plan converts those 65 call sites to `simplify()`, moves the 25 per-class `simplified()` bodies that correspond to nothing in C into their class's `map_merge`, and leaves `simplified()` with one caller and eleven implementations — the eleven whose C counterpart overrides the slot.

**Tech Stack:** Rust 2024 (`ast-core`), C99 (`reference/starlink-ast` at pin `8d4f526f`, built with cmake), the `tracing` feature for the port-side simplify trace.

**Spec:** [`docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`](../specs/2026-08-23-simplify-seam-realignment.md)

## Global Constraints

- The oracle is `reference/starlink-ast` at pin `8d4f526f`, built with cmake. Never reach into it from `ast-core`'s `build.rs`.
- **Never hand-edit a `.simp` or `.map` reference.** Every fixture comes from `ast_tester/gen_simplify_fixtures.c` built against the pinned C, copied into both `reference/starlink-ast/ast_tester/simplify_fixtures/` and `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/` so the trees stay byte-identical.
- `cargo fmt` is enforced; `cargo clippy --workspace -- -D warnings` and `cargo doc` are pre-commit hooks. Every task ends green on all three. A public doc comment may not link to a private item.
- Prose in Markdown is one sentence per line. American English.
- Comments describe the code as it is today. No references to plans, history, or previous mistakes.
- No tolerance, threshold or iteration count without an analogue in the C source.
- Never loosen a negative-control fixture to make a positive case pass.
- **`ISSIMP_DISAGREEMENTS` and `SERIAL_UNGATED` in `crates/ast-core/tests/test_simplify_regression.rs` are this work's audit trail.** Every movement in either is explained in the commit that causes it. Do not relax either gate.
- Every relaxation count is mirrored in `crates/ast-core/tests/test_verification_ledger.rs` and `PLAN.md` §6, updated together.
- Commit with `git commit --fixup` when amending earlier work in this plan.

## What the two seams actually are

Establish this before touching anything, because an earlier draft of this plan got it wrong.

```bash
cd /Users/timj/work/lsst/starlink-astrs
# C: every caller of the Simplify vtab slot.
grep -rn ',Mapping,Simplify)' reference/starlink-ast/src/*.c
# Port: every caller of the trait method, and of the public entry point.
rg -n --color never '\.simplified\(\)' crates/ast-core/src/ | grep -cv 'fn simplified'
rg -c --color never '\.simplify\(\)' crates/ast-core/src/ | awk -F: '{s+=$2} END {print s}'
```

Expected: **one** C caller, **65** port callers of `simplified()`, **8** of `simplify()`.

That asymmetry is the defect. In C there is one way to simplify a Mapping and it
always applies the short-circuit and the record. In the port the raw virtual is
the default choice and the real entry point is the exception, so 65 sites each
decide for themselves whether a reduction is recorded — which is why the port
emits a different set of `IsSimp` cards from C and why no stamping rule has ever
aligned them (`PLAN.md` §10.3).

`simplified()` is not C's `Simplify` slot ported. C's slot is private with a
single caller; this is a public trait method with 65. There is no C equivalent.

## The classification, derived from C

`grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c` returns
`box circle cmpframe cmpmap cmpregion ellipse frameset interval mapping nullregion pointlist polygon prism region stc`.
`mapping` is the base every other class inherits; `cmpframe` and `stc` have no
port counterpart yet.

**Keeps a `simplified()` body — 11 classes** (C overrides the slot):

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

**Loses it — 25 classes** (C has no slot override; all reduction is in `MapMerge`):

`chebymap`, `dssmap`, `grismmap`, `lutmap`, `mathmap`, `matrix`, `normmap`,
`pcdmap`, `perm`, `polymap`, `ratemap`, `selectormap`, `shift`, `slamap`,
`specmap`, `sphmap`, `splinemap`, `switchmap`, `timemap`, `tranmap`, `unit`,
`unitnormmap`, `wcsmap`, `winmap`, `zoom`.

`chebymap` has no `map_merge` at all, so it has nowhere to move to. Task 4
handles it.

**On enforcing the single caller.** Rust cannot make a trait method less public
than its trait, and `Mapping` is public, so `simplified()` cannot be made
crate-private. The single-caller rule is therefore enforced by a source-reading
test, the same technique `test_mapmerge_audit` already uses for the checklists.
That is weaker than a compiler error and it is what the language allows.

## File Structure

| file | responsibility after this plan |
|---|---|
| `crates/ast-core/src/mappings/simplify.rs` | adds `base_simplify`, C's base `Simplify` (one-element list, `map_merge` to fixpoint). Merge core otherwise unchanged. |
| `crates/ast-core/src/mappings/mod.rs` | `simplified()` gains C's base body as its default and a doc saying it has one legitimate caller; `simplify()` is unchanged. |
| `crates/ast-core/src/{frames,regions,fitschan}/**` | 57 call sites move from `simplified()` to `simplify()`. |
| `crates/ast-core/src/mappings/<class>.rs` × 25 | `simplified()` deleted, body folded into that class's `map_merge`. |
| `crates/ast-core/src/mappings/chebymap.rs` | gains a `map_merge` delegating to PolyMap's, which is what C's vtab does. |
| `crates/ast-core/tests/test_simplify_seams.rs` | new. Holds the keeper list and the single-caller rule, both derived from the C source. |
| `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c` | gains the characterisation fixtures Task 2 needs. Goes upstream. |
| `docs/audits/mapmerge/<class>.md` | each moved class's checklist gains the row for the moved reduction. |

---

## Task 1: Convert the 65 call sites

This is first because it is the substantive behavioural change and the one most likely to move `IsSimp` counts. Everything after it is structural.

Split into two commits: the 57 sites outside the merge core, then the 8 inside it. The merge core's sites are the ones where the short-circuit genuinely must not fire, so they need reading rather than replacing.

**Files:**
- Modify: `crates/ast-core/src/frames/{skyframe,timeframe,fluxframe,specfluxframe,frameset,sky_convert}.rs`
- Modify: `crates/ast-core/src/regions/{mod,interval,box_region}.rs`
- Modify: `crates/ast-core/src/fitschan/fset_to_store/{mod,celestial_axes,spectral_axes,other_axes,make_int_world}.rs`
- Modify: `crates/ast-core/src/mappings/{helpers/mod,matrix_merge,winmap_merge,map_box,polymap,dssmap,ratemap,selectormap,switchmap,tranmap}.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: no signature changes. `Mapping::simplify(&self) -> AstResult<DynMapping>` is the inherent method on `dyn Mapping` at `mappings/mod.rs:578`; call it as `Mapping::simplify(x)` where method syntax is ambiguous.

- [ ] **Step 1: Enumerate the sites, grouped**

```bash
cd /Users/timj/work/lsst/starlink-astrs
rg -n --color never '\.simplified\(\)' crates/ast-core/src/ \
  | grep -v 'fn simplified' > /tmp/sites.txt
wc -l /tmp/sites.txt
awk -F: '{print $1}' /tmp/sites.txt | sed 's|crates/ast-core/src/||' | sort | uniq -c | sort -rn
```

Record the total and the per-file counts. Step 6 checks the total fell to the
merge core's own.

- [ ] **Step 2: Record the baseline**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|left:"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```

Paste both somewhere. Every later comparison is against these numbers.

- [ ] **Step 3: Convert the sites outside `mappings/`**

For each site in `frames/`, `regions/` and `fitschan/`, replace
`x.simplified()?` with `Mapping::simplify(x.as_ref())?` — or
`Mapping::simplify(&x)?` where `x` is not boxed. The receiver differs per site;
the transformation does not.

Do not batch-edit with `sed`. Each site needs one judgement: **does C call
`astSimplify` here?** The answer is yes for every site in these three
directories, because C has no other way to reduce a Mapping, but confirm it
against the surrounding C reference that the file's comments already cite. A site
whose comment cites no C function is one to read before changing.

- [ ] **Step 4: Build and run**

```bash
cargo build -p ast-core 2>&1 | grep -E "^error" -A 4 | head -20
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|equal\(\)|left:|right:"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```

Three outcomes:

- **Fixtures that failed now pass** — the expected result. Each reduction now
  carries the record C gives it. Delete the corresponding `SERIAL_UNGATED` or
  `ISSIMP_DISAGREEMENTS` entries in this commit and name the gap each closes.
- **Counts unchanged** — suspicious rather than good. It means none of the 57
  sites was reached by any fixture, which contradicts `wcsconv` exercising the
  FitsChan path. Check the conversion actually compiled into the tested code
  before believing it.
- **A fixture that passed now fails** — a site where C does *not* call
  `astSimplify`, or where the short-circuit now fires on a Mapping that had been
  re-reduced. Diff the traces per Task 5 step 4 to find which site.

- [ ] **Step 5: Commit the 57**

```bash
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/src/frames crates/ast-core/src/regions crates/ast-core/src/fitschan crates/ast-core/tests
git commit -m "simplify: reduce through simplify(), not the raw virtual

C's Simplify vtab slot has one caller, mapping.c:24772 inside astSimplify_,
which applies the IsSimple short-circuit and records the result. Every reduction
in the library goes through it.

These 57 sites called the port's per-class virtual directly, so each decided for
itself whether a reduction was recorded. They are the frames, regions and
FitsChan paths, where C calls astSimplify."
```

- [ ] **Step 6: Read the 8 sites inside the merge core**

```bash
rg -n --color never '\.simplified\(\)' crates/ast-core/src/mappings/ | grep -v 'fn simplified'
```

These are different. `cmp_map_merge`'s self-simplify head, `try_self_reduce`, and
the parallel-branch helpers are inside a `Simplify` implementation, so calling
`simplify()` there would re-enter the short-circuit at a point C does not. For
each, decide from the C source it cites:

- Inside a `Simplify` body, where C calls `astSimplify` on a *component* — becomes
  `simplify()`.
- Inside a `Simplify` body, where C reaches the component another way — stays,
  with a comment naming the C line and why.

`cmp_map_merge`'s head is the first kind: `cmpmap.c:1437` is a literal
`astSimplify( map )`.

- [ ] **Step 7: Convert what step 6 found, and run**

Same commands as step 4. Expected: the corpus moves again, in the same direction.

- [ ] **Step 8: Update the ledger, PLAN.md and the checklists**

Run `cargo test -p ast-core --test test_verification_ledger` and set every count
its failure names, updating `PLAN.md` §6 in the same edit. In
`docs/audits/mapmerge/`, any row whose Rust site was one of the converted call
sites keeps its site and gains nothing — the row is about the step, not the entry
point.

- [ ] **Step 9: Commit the 8**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core PLAN.md docs
git commit -m "simplify: route the merge core's component reductions through simplify()

The remaining raw-virtual calls are inside Simplify implementations. Those where
C calls astSimplify on a component -- cmp_map_merge's self-simplify head is
cmpmap.c:1437, a literal astSimplify( map ) -- now do the same. The rest keep the
raw call with the C line that justifies it."
```

Expected: `0` failures.

---

## Task 2: Characterisation fixtures for the reductions nothing demands

Five classes keep a reduction in `simplified()` that no fixture reaches, because their `map_merge` never consults it. Moving such a reduction is unobservable, and an unobservable move can silently drop it.

**Files:**
- Modify: `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`
- Create: fixture pairs under both `simplify_fixtures/` trees
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Modify: `crates/ast-core/tests/test_verification_ledger.rs`, `PLAN.md`

**Interfaces:**
- Consumes: Task 1 complete, so the counts these fixtures are measured against are the post-conversion ones.
- Produces: one `<class>_lone_reduce` positive and one `neg_<class>_no_reduce` control per class that needs them. Later tasks rely on these failing the moment a moved reduction is lost.

- [ ] **Step 1: Confirm which classes still need one**

```bash
cd /Users/timj/work/lsst/starlink-astrs
for c in chebymap mathmap sphmap unitnormmap wcsmap; do
  f=$(ls crates/ast-core/src/mappings/$c.rs crates/ast-core/src/mappings/$c/mod.rs 2>/dev/null | head -1)
  printf "%-12s map_merge=%s reaches_simplified=%s\n" "$c" \
    "$(grep -c 'fn map_merge(' $f)" \
    "$(grep -cE 'self_reduce_map_merge|try_self_reduce' $f)"
done
```

Expected: every line reads `reaches_simplified=0`. Task 1 may have changed this
for some class; drop any that now reads non-zero and say so in the commit.

- [ ] **Step 2: Read what each `simplified()` reduces**

```bash
for c in chebymap mathmap sphmap unitnormmap wcsmap; do
  f=$(ls crates/ast-core/src/mappings/$c.rs crates/ast-core/src/mappings/$c/mod.rs 2>/dev/null | head -1)
  echo "=== $c ==="; rg -n --color never 'fn simplified' -A 30 "$f" | head -34
done
```

Write down the condition under which each body returns something other than
`self`. That condition is what the fixture must satisfy. A body that only ever
returns `Ok(Box::new(self.clone()))` has no reduction: record it as needing no
fixture and skip steps 3 to 6 for it.

- [ ] **Step 3: Add the generator cases**

Append to `gen_audit_gap_fixtures` in
`reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`, before its closing
brace. The ChebyMap pair is written out; the others follow the same shape with
step 2's condition.

```c
    /* A ChebyMap whose forward transformation holds only T_0 and T_1 terms over
       bounds that are not the identity interval. In C a ChebyMap inherits
       PolyMap's MapMerge -- polymap.c:2439 is the only place the slot is filled
       -- whose replace-with-simpler half rebuilds an all-linear polynomial as a
       MatrixMap and ShiftMap. The control is the same ChebyMap with a T_2 term,
       which is not linear and must survive. */
    {
        double lbnd[] = {0.0};
        double ubnd[] = {10.0};
        double coeff_f[] = {2.0, 1.0, 1.0};
        AstChebyMap *cm = astChebyMap(1, 1, 1, coeff_f, 0, NULL,
                                      lbnd, ubnd, NULL, NULL, " ");
        write_fixture(dir, "cheby_linear_lone_reduce", (AstMapping*)cm);
        cm = astAnnul(cm);
    }
    {
        double lbnd[] = {0.0};
        double ubnd[] = {10.0};
        double coeff_f[] = {2.0, 1.0, 2.0};
        AstChebyMap *cm = astChebyMap(1, 1, 1, coeff_f, 0, NULL,
                                      lbnd, ubnd, NULL, NULL, " ");
        write_negative_fixture(dir, "neg_cheby_quadratic_no_reduce",
                               (AstMapping*)cm);
        cm = astAnnul(cm);
    }
```

- [ ] **Step 4: Generate, and read what C did**

```bash
cd /Users/timj/work/lsst/starlink-astrs/reference/starlink-ast
cmake --build build --target gen_simplify_fixtures -j8
./build/ast_tester/gen_simplify_fixtures 2>&1 | grep -E "lone_reduce|no_reduce"
diff ast_tester/simplify_fixtures/cheby_linear_lone_reduce.map \
     ast_tester/simplify_fixtures/cheby_linear_lone_reduce.simp
```

Expected: the generator names every fixture, and the diff shows more than the
`IsSimp` line.

If a `.simp` differs from its `.map` only in `IsSimp`, C does **not** perform that
reduction. That is a finding: the port's `simplified()` does something C does
not, so the class belongs in Task 5's delete-outright bucket rather than its move
bucket. Record it in the commit and drop the pair.

- [ ] **Step 5: Copy into both trees and wire the manifest**

```bash
cd /Users/timj/work/lsst/starlink-astrs
U=reference/starlink-ast/ast_tester/simplify_fixtures
F=crates/ast-core/tests/fixtures/native/reference/simplify_fixtures
cp $U/cheby_linear_lone_reduce.map $U/cheby_linear_lone_reduce.simp $F/
cp $U/neg_cheby_quadratic_no_reduce.map $F/
```

Append to `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`:

```
# Reductions that live only in a class's simplified() and that no other fixture
# demands. Each pair is a positive case C does reduce and a control C leaves
# alone, so moving the reduction into map_merge cannot silently drop it.
cheby_linear_lone_reduce | simplify_fixtures/cheby_linear_lone_reduce.map | simplify_fixtures/cheby_linear_lone_reduce.simp |
neg_cheby_quadratic_no_reduce | simplify_fixtures/neg_cheby_quadratic_no_reduce.map | simplify_fixtures/neg_cheby_quadratic_no_reduce.map |
```

- [ ] **Step 6: Run, and license the expected failures**

Run: `cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression|equal\(\)" -A 4`

New positives may fail — that is the task's point, and each failure is a gap
Task 5 closes. For each, add a `SERIAL_UNGATED` entry with the reason
`"awaiting the simplified()-to-map_merge move, PLAN.md §10.16"`, so the suite is
green and the debt is named. Task 5 deletes the entry.

- [ ] **Step 7: Ledger, PLAN.md, verify, commit**

```bash
cargo test -p ast-core --test test_verification_ledger   # read the new count
# set it in test_verification_ledger.rs and PLAN.md §6 together, then:
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/tests reference/starlink-ast PLAN.md
git commit -m "test(simplify): characterise the reductions that live only in simplified()

Five classes keep a reduction their map_merge never consults, so no fixture
reaches it and moving it would be unobservable. Each pair is a positive case the
pinned C reduces and a control it leaves alone, generated by
ast_tester/gen_simplify_fixtures.c so the upstream copies are byte-identical."
```

Expected: `0` failures, `194 passed`, no warnings.

---

## Task 3: Write the seam tests

**Files:**
- Create: `crates/ast-core/tests/test_simplify_seams.rs`
- Modify: `docs/audits/mapmerge/README.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `only_c_simplify_classes_implement_simplified` and `simplified_has_one_caller`, both `#[ignore]`d until Task 6. Both read the source tree, as `test_mapmerge_audit` does.

- [ ] **Step 1: Write both failing tests**

Create `crates/ast-core/tests/test_simplify_seams.rs`:

```rust
//! The simplify seams must match C's.
//!
//! C has one public entry, `astSimplify`, and one private dispatch behind it:
//! the `Simplify` vtab slot, whose only caller in the whole library is
//! `mapping.c:24772`. Eleven of this port's classes have a C counterpart that
//! overrides that slot; the other 25 keep every reduction in `MapMerge`, where
//! `astSimplify` reaches it by nominating the Mapping in a list of one
//! (`mapping.c:16871-16883`).
//!
//! Rust cannot make a trait method less public than its trait, so neither rule
//! can be a compiler error. These tests read the source instead.

use std::path::{Path, PathBuf};

/// Port files whose C counterpart overrides the `Simplify` vtab slot.
///
/// Derived, not chosen:
/// `grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c`
/// returns `box circle cmpframe cmpmap cmpregion ellipse frameset interval
/// mapping nullregion pointlist polygon prism region stc`. `mapping` is the base
/// every other class inherits, and `cmpframe` and `stc` have no port counterpart.
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

/// Files allowed to call `simplified()`.
///
/// C's slot has one caller. The port's equivalent is the public `simplify()` in
/// `mappings/mod.rs`. `mappings/simplify.rs` is the `CmpMap` `Simplify` body
/// itself, where C calls `astSimplify` on components (`cmpmap.c:1437`), so it
/// reaches the virtual through `simplify()` and needs no exception — if it
/// appears here, that conversion was missed.
const SIMPLIFIED_CALLERS: &[&str] = &["mappings/mod.rs"];

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `src`, as a path relative to it.
fn rust_files() -> Vec<(String, String)> {
    let root = src_root();
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
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
            let rel = path
                .strip_prefix(&root)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path).expect("source readable");
            out.push((rel, text));
        }
    }
    out.sort();
    out
}

#[test]
#[ignore = "fails until the 25 stray bodies have moved; see \
            docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md"]
fn only_c_simplify_classes_implement_simplified() {
    let stray: Vec<String> = rust_files()
        .into_iter()
        .filter(|(_, text)| text.contains("fn simplified(&self) -> AstResult<DynMapping> {"))
        .map(|(rel, _)| rel)
        .filter(|rel| !C_SIMPLIFY_CLASSES.contains(&rel.as_str()))
        .collect();
    assert!(
        stray.is_empty(),
        "these classes implement `simplified()` and their C counterpart does not \
         override the `Simplify` slot, so the reduction belongs in `map_merge` \
         where the nominate loop can reach it: {stray:#?}"
    );
}

#[test]
#[ignore = "fails until the 65 call sites have moved; see \
            docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md"]
fn simplified_has_one_caller() {
    let mut offenders = Vec::new();
    for (rel, text) in rust_files() {
        if SIMPLIFIED_CALLERS.contains(&rel.as_str()) {
            continue;
        }
        let calls = text
            .lines()
            .filter(|l| l.contains(".simplified()"))
            .filter(|l| !l.trim_start().starts_with("//"))
            .count();
        if calls > 0 {
            offenders.push(format!("{rel}: {calls}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "`simplified()` is the per-class dispatch behind `simplify()`, and C's \
         equivalent has exactly one caller (mapping.c:24772). Calling it \
         directly skips the `IsSimple` short-circuit and the record: {offenders:#?}"
    );
}
```

- [ ] **Step 2: Run both to verify they fail, and name the right sets**

```bash
cargo test -p ast-core --test test_simplify_seams -- --ignored --nocapture
```

Expected: `only_c_simplify_classes_implement_simplified` lists 25 paths from
`mappings/chebymap.rs` to `mappings/zoom.rs` and nothing under `regions/` or
`frames/`. `simplified_has_one_caller` lists the files Task 1 has not yet
converted — if Task 1 is done, it lists only `mappings/simplify.rs` and whichever
merge-core files kept a justified raw call, and `SIMPLIFIED_CALLERS` must grow to
name each with the C line that justifies it.

If a Region or `frames/frameset.rs` appears in the first list, the keeper list has
a typo: paths are relative to `crates/ast-core/src` and `box_region.rs` keeps its
suffix.

- [ ] **Step 3: Document the rule where the checklists are read**

Append to `docs/audits/mapmerge/README.md`:

```markdown
## Which seam a reduction belongs in

C has one public entry, `astSimplify`, and one private dispatch behind it — the
`Simplify` vtab slot, whose only caller in the library is `mapping.c:24772`.
Eleven classes override that slot. For every other class **all** reduction lives
in `MapMerge`, and `astSimplify` reaches it by nominating the Mapping in a list of
one (`mapping.c:16871-16883`).

Two things follow, and `crates/ast-core/tests/test_simplify_seams.rs` holds both:

- A row whose Rust site is a `simplified()` body, in a class whose C counterpart
  does not override the slot, is not `ported`: it is in a seam C does not have and
  the nominate loop cannot reach it.
- A row whose Rust site calls `simplified()` rather than `simplify()` is skipping
  the `IsSimple` short-circuit and the record, so its `IsSimp` behaviour is not
  C's whatever the reduction does.
```

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_simplify_seams 2>&1 | grep "test result"
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/tests/test_simplify_seams.rs docs/audits/mapmerge/README.md
git commit -m "test: hold the simplify seam rules, derived from the C source

Two rules. Only the 11 classes whose C counterpart overrides the Simplify vtab
slot may implement simplified(). And that slot has exactly one caller in C
(mapping.c:24772), so the port's equivalent should too -- calling it directly
skips the IsSimple short-circuit and the record.

Rust cannot make a trait method less public than its trait, so neither can be a
compiler error; both read the source. Ignored until the work lands."
```

Expected: `0` failures, two ignored.

---

## Task 4: Give ChebyMap the `map_merge` C gives it

`chebymap` is the one class with a `simplified()` and no `map_merge`, so it has nowhere to move to. In C it has neither: `polymap.c:2439` is the only place the slot is filled, so a ChebyMap's `MapMerge` *is* PolyMap's.

**Files:**
- Modify: `crates/ast-core/src/mappings/chebymap.rs`
- Modify: `docs/audits/mapmerge/chebymap.md`, `PLAN.md`

**Interfaces:**
- Consumes: `cheby_linear_lone_reduce` and `neg_cheby_quadratic_no_reduce` from Task 2.
- Produces: `ChebyMap::map_merge`. No new public API.

- [ ] **Step 1: Confirm C's vtab claim before relying on it**

```bash
cd /Users/timj/work/lsst/starlink-astrs
grep -n 'MapMerge = MapMerge' reference/starlink-ast/src/chebymap.c reference/starlink-ast/src/polymap.c
grep -c 'static int MapMerge' reference/starlink-ast/src/chebymap.c
```

Expected: one hit, in `polymap.c`, and `0` for `chebymap.c`.
If `chebymap.c` defines its own, port that instead of delegating and say so.

- [ ] **Step 2: Read PolyMap's `map_merge`**

Run: `rg -n --color never 'fn map_merge' -A 40 crates/ast-core/src/mappings/polymap.rs | head -44`

Note whether the body reads `PolyMap`-only fields. If it does, step 5 needs a
`PolyMap` view of the ChebyMap; if it only uses `&dyn Mapping` methods, it can
take `self` directly.

- [ ] **Step 3: Write the failing test**

Add inside `chebymap.rs`'s existing `#[cfg(test)] mod tests`:

```rust
    /// A ChebyMap nominated inside a list must reach the replace-with-simpler
    /// half of PolyMap's `MapMerge`, which C reaches through the shared vtab
    /// slot (`polymap.c:2439`), so a linear ChebyMap is reduced there and not
    /// only when it is simplified on its own.
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
        let pair = CmpMap::new(
            Box::new(cheby) as DynMapping,
            Box::new(UnitMap::new(1)) as DynMapping,
            Composition::Series,
        )
        .expect("series pair constructs");

        let out = Mapping::simplify(&pair).expect("simplify succeeds");
        assert!(
            out.as_any().downcast_ref::<ChebyMap>().is_none(),
            "a linear ChebyMap nominated in a list should have been replaced, \
             got {out:?}"
        );
    }
```

Adjust the `ChebyMap::new` call to the constructor's real signature, which step 2
shows. The assertion is the part that matters.

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p ast-core --lib linear_chebymap_reduces -- --nocapture`
Expected: FAIL — still a `ChebyMap`, because `ChebyMap` has no `map_merge` and
takes the trait default, which only tries the neighbours.

- [ ] **Step 5: Add the delegation**

Insert into `impl Mapping for ChebyMap`, before `fn simplified`:

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
        super::polymap::poly_map_merge(self, ctx, where_, series)
    }
```

If step 2 showed the body reads `PolyMap` fields, add instead:

```rust
    /// This ChebyMap's terms and bounds as a `PolyMap`, for the `MapMerge` the
    /// two classes share in C.
    fn as_polymap_view(&self) -> PolyMap {
        PolyMap::from_terms(self.forward_terms(), self.inverse_terms())
            .expect("a ChebyMap's terms are a valid PolyMap")
    }
```

and call `self.as_polymap_view().map_merge(ctx, where_, series)`.

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p ast-core --lib linear_chebymap_reduces -- --nocapture`
Expected: PASS.

- [ ] **Step 7: Run the corpus**

Run: `cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression|equal\(\)|left:"`

Expected: `cheby_linear_lone_reduce` passes, its Task 2 `SERIAL_UNGATED` entry can
go, and `neg_cheby_quadratic_no_reduce` still passes.

**If the linear replacement loses the ChebyMap's domain scaling** — the open
question in `PLAN.md` §10.2 — one of the pair will disagree with C on values
rather than class. Stop and settle it: build a two-point probe per
`docs/issues/c-oracle-probe-recipe.md`, transform the same position through C's
simplified ChebyMap and the port's, and compare. If C loses the scaling, do not
mirror it — revert this task, record the finding in
`docs/issues/c-library-quirks.md` and `PLAN.md` §10.2, and raise it upstream.

- [ ] **Step 8: Update the checklist and PLAN.md, verify, commit**

Change the `gap: PLAN.md §10.2` verdicts on `chebymap.md` rows 4 to 12 to
`ported` with the `chebymap.rs` site, and replace the closing note with what
step 7 established. Mark `PLAN.md` §10.2 accordingly.

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

Expected: `0` failures, `194 passed`.

---

## Task 5: Add C's base `Simplify`, then move the 24 bodies

**Files:**
- Modify: `crates/ast-core/src/mappings/simplify.rs`, `crates/ast-core/src/mappings/mod.rs:282`
- Modify: `crates/ast-core/src/mappings/<class>.rs` × 24, one per commit
- Modify: `docs/audits/mapmerge/<class>.md` × 24

**Interfaces:**
- Consumes: Task 4's ChebyMap `map_merge`; Task 2's fixtures.
- Produces: `pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<DynMapping>` in `simplify.rs`; `Mapping::simplified()` becomes a provided method calling it. Each moved class ends with no `simplified()`.

### Part A — the driver

- [ ] **Step A1: Read C's loop**

Run: `sed -n '16860,16900p' reference/starlink-ast/src/mapping.c`

Three things it does: calls
`astMapMerge( map, 0, 1, &nmap, &map_list, &invert_list )`; breaks when
`nmap > 1` **or** `modified < 0`; treats "no simplification" as returning a clone.
The port's version must do all three.

- [ ] **Step A2: Write the failing tests**

Append to `simplify.rs`'s `#[cfg(test)] mod tests`:

```rust
    /// C's base `Simplify` builds a one-element list and calls `astMapMerge`
    /// until nothing changes or the list grows past one
    /// (`mapping.c:16871-16883`). A unit-factor ZoomMap is a UnitMap, and
    /// `zoom_map_merge` says so, so the driver must reach that without any
    /// per-class `simplified()` in the way.
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
        let z = ZoomMap::new(1, 3.0).expect("ZoomMap constructs");
        let out = base_simplify(&z).expect("base_simplify succeeds");
        assert_eq!(
            out.as_any()
                .downcast_ref::<ZoomMap>()
                .expect("still a ZoomMap")
                .factor(),
            3.0
        );
    }
```

- [ ] **Step A3: Run to verify it fails**

Run: `cargo test -p ast-core --lib base_simplify -- --nocapture`
Expected: FAIL to compile, `cannot find function base_simplify`.

- [ ] **Step A4: Write the driver**

Add to `simplify.rs` next to `default_map_merge`:

```rust
/// C's base `Simplify` (`mapping.c:16752`), inherited by every class whose C
/// counterpart does not override the slot.
///
/// Builds a one-element Mapping list and hands it to the class's own
/// `map_merge` repeatedly, stopping when the merge reports no change or the list
/// grows past one element (`mapping.c:16871-16883`). Returns a clone when
/// nothing reduced, as C does.
///
/// This is C's only route to such a class's reduction, so it is the only route
/// this port should have.
pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<DynMapping> {
    let mut ctx = MergeCtx::from_single(this.clone_box());
    let mut simpler = false;

    // C loops without a bound; the guard is the oscillation break the merge loop
    // already applies rather than a limit of its own.
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

If `MergeCtx` has no `from_single` or `is_frozen(usize)`, read its impl
(`rg -n 'impl MergeCtx' -A 40 crates/ast-core/src/mappings/simplify.rs`) and use
what it has. The shape — one element, loop, break on growth or no change — must
not change.

- [ ] **Step A5: Run to verify it passes**

Run: `cargo test -p ast-core --lib base_simplify -- --nocapture`
Expected: PASS, both tests.

- [ ] **Step A6: Make `simplified()` a provided method**

Replace `mappings/mod.rs:282`'s `fn simplified(&self) -> AstResult<DynMapping>;`
with:

```rust
    /// Reduce this Mapping without recording the result — C's `Simplify` vtab
    /// slot.
    ///
    /// **Call [`Mapping::simplify`] instead.** That is C's `astSimplify`, and it
    /// adds the two things this omits: the `IsSimple` short-circuit and the
    /// record on the result (`mapping.c:24755`). C's slot has exactly one caller
    /// in the entire library (`mapping.c:24772`), and so should this.
    ///
    /// The default is C's base implementation: build a one-element Mapping list
    /// and hand it to this class's own [`Mapping::map_merge`] until nothing
    /// changes (`mapping.c:16871-16883`). Override it only if the matching C
    /// class overrides the slot — `CmpMap`, `FrameSet` and the nine Region
    /// classes do, and the other 25 do not.
    /// `crates/ast-core/tests/test_simplify_seams.rs` holds both rules.
    fn simplified(&self) -> AstResult<DynMapping> {
        simplify::base_simplify(self.as_dyn_mapping())
    }
```

If the trait has no upcast helper, add one beside it, plus a one-line impl per
class as the compiler demands:

```rust
    /// This Mapping as `&dyn Mapping`, so provided methods can call free
    /// functions taking a trait object.
    fn as_dyn_mapping(&self) -> &dyn Mapping;
```

- [ ] **Step A7: Verify nothing moved, then commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo clippy -p ast-core --all-targets -- -D warnings
```

Expected: `0`, the same counts as before this part, `194 passed`. Every class
still overrides `simplified()`, so the default is unreachable and behaviour must
be unchanged. **A change means the default is being reached, so some class was
already missing an override.** Find it before continuing.

```bash
git add crates/ast-core/src
git commit -m "simplify: add C's base Simplify as the simplified() default

C's base Simplify (mapping.c:16752) builds a one-element Mapping list and calls
astMapMerge until nothing changes or the list grows past one. Every class whose C
counterpart does not override the slot inherits it, and it is C's only route to
such a class's reduction.

No override is removed yet, so nothing is reached differently."
```

### Part B — the 24 moves

Order by fixture coverage. **Group A** — `map_merge` already routes through a
self-reduce helper, so the reduction is reached and deleting `simplified()` is a
no-op the default absorbs: `dssmap`, `normmap`, `ratemap`, `selectormap`,
`splinemap`, `switchmap`.

**Group B** — small and heavily covered, `map_merge` present but not consulting
`simplified()`: `unit`, `zoom`, `shift`, `perm`, `matrix`, `winmap`.

**Group C** — the conversion classes, larger bodies: `slamap`, `specmap`,
`timemap`, `lutmap`, `polymap`, `pcdmap`, `grismmap`, `sphmap`, `wcsmap`,
`unitnormmap`, `mathmap`.

**Group D** — recurses into encapsulated Mappings and is orientation-sensitive:
`tranmap`.

#### Archetype 1 — a reduction with no encapsulated Mapping (Groups B and C)

`ZoomMap` is the worked example. Its `simplified()` today returns a `UnitMap` for
a unit effective factor and a forward `ZoomMap` with the reciprocal otherwise.
That is C's replace-with-simpler half of `zoommap.c`'s `MapMerge`, so it goes at
the top of `zoom_map_merge`, before the run condensing:

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
    let eff_inv = ctx.entry(where_).1 != zoom.is_inverted();
    let eff = if eff_inv { 1.0 / zoom.factor() } else { zoom.factor() };
    if (eff - 1.0).abs() <= 8.0 * f64::EPSILON {
        ctx.set_pair_single(where_, (Box::new(UnitMap::new(zoom.ncoord())), false, false));
        return Some(where_);
    }
    if eff_inv {
        let fwd = ZoomMap::new(zoom.ncoord(), eff).ok()?;
        ctx.set_pair_single(where_, (Box::new(fwd), false, false));
        return Some(where_);
    }

    // ... existing body unchanged from here ...
```

`8.0 * f64::EPSILON` is the comparison this function already uses for its run
condensing. Reuse whatever the file already does rather than introducing a
tolerance.

#### Archetype 2 — a reduction over an encapsulated Mapping (Groups A and D)

`RateMap` is the worked example and is already in this shape: `map_merge` routes
through `self_reduce_map_merge`, and `simplified()` orients the child to
`invert_snapshot` before reducing it (`ratemap.c:710-715`). For these classes the
move is a **deletion**: the default absorbs the call and the reduction is already
reached. **The orientation handling must move with the body, not be left behind** —
dropping it reproduces the §10.1 bug.

#### Steps, per class

- [ ] **Step B1: Read the class's two methods**

```bash
c=zoom   # the class being moved
f=crates/ast-core/src/mappings/$c.rs
rg -n --color never 'fn simplified' -A 40 $f | head -44
rg -n --color never 'fn map_merge' -A 12 $f | head -16
```

Decide the archetype. If `map_merge` delegates to a `simplify.rs` helper, the
reduction goes at the top of that helper; if inline, at the top of the body.

- [ ] **Step B2: Record the baseline**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|left:"
```

Copy the summary line and the `left:` list. Any movement after B3 is this class's
doing and must be explained.

- [ ] **Step B3: Move the body**

Archetype 1: insert the reduction at the top of `map_merge` or its helper, then
delete the `simplified()` impl. Archetype 2: delete the `simplified()` impl only,
after confirming `map_merge` reaches the same logic through
`self_reduce_map_merge` or `try_self_reduce`.

- [ ] **Step B4: Compare against the baseline**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|equal\(\)|left:|right:"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```

Expected: identical to B2, and `194 passed`.

- **Identical** — correct, continue.
- **A failing fixture now passes** — the reduction was unreachable and now is.
  Delete its `SERIAL_UNGATED` or `ISSIMP_DISAGREEMENTS` entry in this commit and
  say which gap it closes.
- **A passing fixture now fails** — stop. Either the reduction landed in the wrong
  position relative to the neighbour steps, or it lost its orientation handling.
  Diff the traces:

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

- [ ] **Step B5: Update the checklist**

In `docs/audits/mapmerge/<class>.md`, repoint every row whose Rust site was in
`simplified()` to its new `map_merge` location. A row whose verdict was `gap`
because the reduction was unreachable becomes `ported`.

- [ ] **Step B6: Commit**

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

Repeat B1 to B6 for each of the 24, in group order.

---

## Task 6: Turn both gates on

**Files:**
- Modify: `crates/ast-core/tests/test_simplify_seams.rs`
- Modify: `PLAN.md`, `docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`

**Interfaces:**
- Consumes: Tasks 1 to 5 complete.
- Produces: both seam tests passing without `#[ignore]`.

- [ ] **Step 1: Remove both ignores and run**

```bash
# delete the two #[ignore = …] attributes, then:
cargo test -p ast-core --test test_simplify_seams -- --nocapture
```

Expected: PASS, both.

If `only_c_simplify_classes_implement_simplified` still lists classes, Task 5 has
not reached them — finish them rather than adding them to the keeper list, which
is derived from C and is not a place to record incomplete work.

If `simplified_has_one_caller` still lists files, each is either a Task 1
conversion that was missed or a justified exception. An exception goes in
`SIMPLIFIED_CALLERS` **with the C line that justifies it in the comment**; a
missed conversion gets converted.

- [ ] **Step 2: Re-measure §10.3 against the current pin**

`PLAN.md` §10.3's nomination counts were taken before upstream fixed
`cmpmap.c` (`4f2fc0ed`) and before Task 1, so both sides have moved.

```bash
cd /Users/timj/work/lsst/starlink-astrs
D=${TMPDIR:-/tmp}/ast-trace3
FIX=crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/cap_specmap_02.map
AST_TRACE=1 $D/gen_simp $FIX /dev/null 2> /tmp/c.txt
cargo run -q --features tracing --example simp_one -- $FIX 2>&1 >/dev/null \
  | sed 's/\x1b\[[0-9;]*m//g' > /tmp/p.txt
for f in /tmp/c.txt /tmp/p.txt; do
  echo "$f nominations=$(grep -c 'MAPMERGE ' $f) productive=$(grep 'MAPMERGE-RESULT' $f | grep -vc 'result=-1')"
done
```

Rewrite §10.3's table with the new numbers and say which pin they were taken at.
If the counts now agree, §10.3 is closed by this work — say so and delete its
`SERIAL_UNGATED` and `ISSIMP_DISAGREEMENTS` entries.

- [ ] **Step 3: Close the PLAN.md entries**

Mark §10.16 `FIXED` with what the work established. Re-check §10.1 and §10.7,
which both cited the seam as their cause, and §10.11, whose four unreached
`TranMap` cases may now be reachable.

- [ ] **Step 4: Mark the spec done**

Change `**Status:** proposed, not started.` to
`**Status:** implemented, <date>.` and add a closing section on what the work
found that the spec did not predict — starting with the call-site asymmetry, 65
against 8, which is the finding the spec was pointing at and an earlier draft of
this plan mis-stated as a vtab-structure question.

- [ ] **Step 5: Full verification and commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "^Simplify regression"
cargo clippy -p ast-core --all-targets -- -D warnings
cargo clippy -p ast-core --all-targets --features tracing -- -D warnings
cargo doc -p ast-core --no-deps 2>&1 | grep -cE "^error"
git add crates/ast-core PLAN.md docs
git commit -m "test: require the simplify seams to match C's

One way to simplify a Mapping, as C has. simplified() has one legitimate caller
and eleven implementations -- the eleven whose C counterpart overrides the
Simplify vtab slot -- and every other class keeps its reduction in map_merge,
where astSimplify reaches it by nominating the Mapping in a list of one.

Both rules are enforced from the C source, so the seams cannot drift back."
```

Expected: `0` failures, `194 passed`, no clippy warnings either way, `0` doc
errors.

---

## Self-review

**Spec coverage.** The spec's five steps map to Tasks 2 (fixtures first), 3 (the
sweep, as an enforced classification), 5A (the driver), 5B (the moves), and 6 (the
deletion). Task 1 is new and is the spec's actual point: it says the three seams
"lead to confusion", and the confusion is that 65 sites call the raw virtual and 8
call the entry point. An earlier draft of this plan read the spec as a
vtab-structure question and put Task 1 nowhere; that was wrong, and the correction
is recorded in the spec's closing section per Task 6 step 4. Task 4 is also new:
`chebymap` has no `map_merge`, so it cannot participate in Task 5 until it does.

All four spec risks appear: blast radius (Task 1 step 2 and Task 5 step B2's
baselines, plus one class per commit), `IsSimp` counts moving both ways (a Global
Constraint and every comparison step), Region `Simplify` being real (the keeper
table and Task 3's gate), and the orientation-sensitive classes (Group D and
Archetype 2's warning).

**Placeholder scan.** Task 1 step 3 gives the transformation and the judgement to
apply rather than 57 diffs; the judgement is the content. Task 2 steps 3 and 5
give one worked class because steps 1 and 2 determine which classes need one at
all. Task 5B gives two archetypes with full code and assigns every class to one.
Task 4 steps 5 carries two branches because step 2 reads which applies; both are
written out.

**Type consistency.** `base_simplify(this: &dyn Mapping) -> AstResult<DynMapping>`
is used with that signature in Task 5 steps A2, A4 and A6. `MergeCtx::from_single`,
`is_frozen`, `entry`, `len`, `set_pair_single` appear in A4 and Archetype 1, with
A4 instructing substitution if the real API differs. `trace::map_merge` and
`trace::map_merge_result` are the signatures already in `simplify.rs`.
`Mapping::simplify(x)` is used in call position throughout, matching the inherent
method at `mappings/mod.rs:578`. `C_SIMPLIFY_CLASSES` and `SIMPLIFIED_CALLERS`
both hold paths relative to `crates/ast-core/src`, which is what `rust_files()`
computes.

**Two things I could not close.** Task 5B cannot state each class's expected
corpus movement in advance, because measuring it is the task; step B2's baseline
capture is the substitute. And `simplified()` cannot be made crate-private —
Rust has no way to make a trait method less public than its trait — so the
single-caller rule is a source-reading test rather than a compiler error, which
is weaker than it should be and is the best the language allows.
