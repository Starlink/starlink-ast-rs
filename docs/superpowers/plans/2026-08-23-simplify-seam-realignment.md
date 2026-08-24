# Simplify Seam Realignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make it impossible to simplify a Mapping without passing the `IsSimple` gate, so every reduction is short-circuited and recorded exactly as C's is.

**Architecture:** The port's two-level structure already matches C's. `simplify()`, an inherent method on `dyn Mapping` at `mappings/mod.rs:568`, is `astSimplify_`; the trait method `simplified()` is the `Simplify` vtab slot; `map_merge()` is `MapMerge`. The defect is not the shape, it is that **155 call sites call `simplified()` directly** where C's slot has exactly one caller. Each bypasses the gate in both directions: it can fail to record a reduction, and it can re-reduce something C returns untouched. This plan makes the bypass a **compile error** by giving `simplified()` a parameter only `simplify()` can construct, works the resulting error list, then moves the 25 per-class `simplified()` bodies that correspond to nothing in C into their class's `map_merge`.

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

## The problem, stated once

C has one public function, `astSimplify_` (`mapping.c:24755`). It does three
things: short-circuits if the Mapping's `IsSimple` bit is already set, calls the
per-class `Simplify` vtab slot, and stamps the result. The slot is per-class —
`CmpMap`, `FrameSet` and the nine Region classes override it; the other 25
inherit the base implementation (`mapping.c:16752`), which builds a one-element
Mapping list and loops on `MapMerge`.

**Only `astSimplify_` ever calls the slot.** One caller, `mapping.c:24772`. So
every reduction anywhere in C passes the gate.

The port mirrors that structure: `simplify()` on `dyn Mapping` is `astSimplify_`,
the trait method `simplified()` is the slot, `map_merge()` is `MapMerge`. What it
does not mirror is the discipline.

```
                        simplified()   simplify()
crates/ast-core/src           65            8
crates/ast-core/tests         88           28
crates/ast-core/examples       2            0
```

Every one of those 155 `simplified()` calls skips the gate, and it matters in both
directions:

- **The record is not set**, so a reduction the port performs carries no `IsSimp`
  card where C's would.
- **The short-circuit does not fire**, so the port re-reduces a Mapping C returns
  untouched — which is why it makes 149 nominations to C's 96 on `cap_specmap_02`
  (`PLAN.md` §10.3) while performing *fewer* productive merges.

No stamping rule can reconcile the card counts while both of those are true,
which is what seven measured-and-reverted attempts at §10.3 have established the
hard way.

## The gate, enforced by the compiler

`simplified()` is a method on the public `Mapping` trait, so it cannot be made
crate-private. It can be made *uncallable* by giving it a parameter whose type
only one module can construct:

```rust
// crates/ast-core/src/mappings/simplify_entry.rs
pub struct ViaSimplify(());
```

A `pub` struct with a private field is nameable everywhere and constructible only
inside its defining module **and that module's descendants**. Put it in a leaf
module with no children, put `simplify()` in the same module, and the arithmetic
is exact: `simplify()` can build one, nothing else can, and
`fn simplified(&self, _: ViaSimplify)` therefore has exactly one possible caller —
the compile-time equivalent of `mapping.c:24772`.

The 11 legitimate overrides are unaffected: they *implement* the method, which
needs only to name the type, not construct it.

This is the enforcement an earlier draft of this plan said Rust could not provide.
It can.

## The classification, derived from C

`grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c` returns
`box circle cmpframe cmpmap cmpregion ellipse frameset interval mapping nullregion pointlist polygon prism region stc`.
`mapping` is the base; `cmpframe` and `stc` have no port counterpart yet.

**Keeps a `simplified()` body — 11 classes:**

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

**Loses it — 25 classes**, whose C counterpart has no slot override so all their
reduction lives in `MapMerge`: `chebymap`, `dssmap`, `grismmap`, `lutmap`,
`mathmap`, `matrix`, `normmap`, `pcdmap`, `perm`, `polymap`, `ratemap`,
`selectormap`, `shift`, `slamap`, `specmap`, `sphmap`, `splinemap`, `switchmap`,
`timemap`, `tranmap`, `unit`, `unitnormmap`, `wcsmap`, `winmap`, `zoom`.

`chebymap` has no `map_merge` at all, so it has nowhere to move to. Task 4 gives
it the one C's vtab gives it.

## File Structure

| file | responsibility after this plan |
|---|---|
| `crates/ast-core/src/mappings/simplify_entry.rs` | new leaf module. Holds `ViaSimplify` and the `impl dyn Mapping` block containing `simplify()`. Nothing else may live here, or the token stops being exclusive. |
| `crates/ast-core/src/mappings/mod.rs` | `simplified()` takes `ViaSimplify` and gains C's base body as its default. The `simplify()` block moves out to `simplify_entry`. |
| `crates/ast-core/src/mappings/simplify.rs` | adds `base_simplify`, C's base `Simplify`. Merge core otherwise unchanged. |
| `crates/ast-core/src/{frames,regions,fitschan}/**` and the merge core | 65 call sites resolved, each to `simplify()` or to a justified `simplified(ViaSimplify)` — but only 11 files can even write the latter, so in practice all become `simplify()`. |
| `crates/ast-core/tests/**`, `examples/**` | 90 call sites become `simplify()`. |
| `crates/ast-core/src/mappings/<class>.rs` × 25 | `simplified()` deleted, body folded into that class's `map_merge`. |
| `crates/ast-core/tests/test_simplify_seams.rs` | new. Holds the keeper list, derived from the C source. The single-caller rule needs no test — it is a compile error. |
| `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c` | gains the characterisation fixtures Task 3 needs. Goes upstream. |

---

## Task 1: Make bypassing the gate a compile error

**Files:**
- Create: `crates/ast-core/src/mappings/simplify_entry.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: every file the compiler names — expect ~40 across `src`, `tests` and `examples`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub struct ViaSimplify(())` in `mappings::simplify_entry`, re-exported from `mappings`; `Mapping::simplified(&self, _: ViaSimplify) -> AstResult<DynMapping>`; `<dyn Mapping>::simplify(&self) -> AstResult<DynMapping>` unchanged in signature and moved to `simplify_entry`.

- [ ] **Step 1: Record the baseline**

```bash
cd /Users/timj/work/lsst/starlink-astrs
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|left:"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
rg -n --color never '\.simplified\(\)' crates/ast-core | grep -cv 'fn simplified'
```

Paste all three. Every later comparison is against them, and the last is the size
of the error list to expect.

- [ ] **Step 2: Create the leaf module with the token**

Create `crates/ast-core/src/mappings/simplify_entry.rs`:

```rust
//! The one way into a per-class simplification.
//!
//! C has a single public entry, `astSimplify_` (`mapping.c:24755`), which
//! short-circuits on the `IsSimple` bit, calls the per-class `Simplify` vtab
//! slot, and records the result. That slot has exactly one caller in the whole
//! library, `mapping.c:24772`, so every reduction anywhere in C passes the gate.
//!
//! [`ViaSimplify`] makes that arithmetic hold here too. It is nameable
//! everywhere and constructible only in this module, and this module contains
//! nothing but the entry point, so [`Mapping::simplified`] has exactly one
//! possible caller.
//!
//! **Do not add anything else to this file.** A second function here could
//! construct a token, and the guarantee is that nothing but `simplify()` can.

use super::{CmpMap, DynMapping, Mapping};
use crate::AstResult;

/// Proof that a per-class reduction was reached through
/// [`Mapping::simplify`], and therefore through the `IsSimple` gate.
///
/// Constructible only in this module. Implementations of
/// [`Mapping::simplified`] receive one and never build one.
pub struct ViaSimplify(());

impl dyn Mapping {
    /// Simplify this Mapping, C's `astSimplify` (`mapping.c:24755`).
    ///
    /// Returns an unchanged clone when the `IsSimple` record is already set, and
    /// otherwise records the result, which is the whole of what `astSimplify_`
    /// adds to the per-class [`Mapping::simplified`] it dispatches to.
    ///
    /// This is the only way to reach a per-class reduction. Anything that wants
    /// one calls this.
    pub fn simplify(&self) -> AstResult<DynMapping> {
        super::simplify::trace::simplify_enter("public-simplify", self);
        // ... the existing body from mappings/mod.rs, with its single
        // `self.simplified()` call becoming `self.simplified(ViaSimplify(()))`
    }
}
```

Move the whole existing `impl dyn Mapping { … }` block from
`crates/ast-core/src/mappings/mod.rs:568` into this file verbatim, then change
its one `self.simplified()` call to `self.simplified(ViaSimplify(()))`. If that
block holds methods other than `simplify()`, leave those behind in `mod.rs` in
their own `impl dyn Mapping` block — this file must contain only the entry point.

Register the module and re-export the token in `mappings/mod.rs`:

```rust
mod simplify_entry;
pub use simplify_entry::ViaSimplify;
```

- [ ] **Step 3: Give the trait method the parameter**

In `crates/ast-core/src/mappings/mod.rs`, change line 282's declaration to:

```rust
    /// Reduce this Mapping without the `IsSimple` gate — C's `Simplify` vtab
    /// slot.
    ///
    /// Reachable only from [`Mapping::simplify`], which is what supplies the
    /// [`ViaSimplify`]. That mirrors C, where `mapping.c:24772` is the slot's
    /// only caller, so no reduction can happen without being short-circuited
    /// and recorded.
    ///
    /// Override this only if the matching C class overrides the slot: `CmpMap`,
    /// `FrameSet` and the nine Region classes do, and the other 25 keep every
    /// reduction in [`Mapping::map_merge`].
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<DynMapping>;
```

Leave it a required method for now; Task 5 gives it a default.

- [ ] **Step 4: Build, and read the error list**

```bash
cargo build -p ast-core 2>&1 | grep -E "^error" | sort | uniq -c | sort -rn | head
cargo build -p ast-core 2>&1 | grep -E '^\s+--> ' | awk '{print $2}' | cut -d: -f1 | sort | uniq -c | sort -rn
```

Expected: two error kinds — "this method takes 1 argument but 0 were supplied"
at every call site, and "missing argument" or a mismatched-signature error at
every `impl`. The file list is the work.

This error list **is** the audit. Every entry is a place that was reducing a
Mapping without the gate, which is exactly what needs looking at.

- [ ] **Step 5: Fix the 36 implementations**

Each of the 36 `impl Mapping for X` blocks needs its signature updated:

```rust
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<DynMapping> {
```

Nothing inside the bodies changes yet. Where a body recursively reduces a
component — `CmpMap`, `RateMap`, `TranMap`, `SwitchMap`, `SelectorMap`,
`NormMap`, and the Region classes — that inner call is now a compile error too,
and it becomes `Mapping::simplify(child.as_ref())?`. **That is the behavioural
change this task exists for**, so expect the corpus to move at step 7.

- [ ] **Step 6: Fix the call sites**

Every remaining error is a site that must become `simplify()`:

```rust
// before
let smap = map.simplified()?;
// after
let smap = Mapping::simplify(map.as_ref())?;
```

The receiver differs — `map.as_ref()`, `&*boxed`, `&concrete` — and the compiler
says which. Do not reach for `ViaSimplify(())`: outside `simplify_entry` it will
not compile, which is the point.

Work `src` first, then `tests`, then `examples`. A test asserting on an
unrecorded reduction is asserting on something C never produces, so converting it
is a correction, not an accommodation — but if a test *deliberately* checks the
ungated path, it can no longer do so, and that is worth a note in the commit.

- [ ] **Step 7: Compare against the baseline**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 \
  | grep -E "^Simplify regression|equal\(\)|left:|right:"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
```

The corpus **will** move, and that is the task working. Three cases:

- **Fixtures that failed now pass** — a reduction now carries the record C gives
  it, or a re-reduction C never performs is now short-circuited away. Delete the
  matching `SERIAL_UNGATED` or `ISSIMP_DISAGREEMENTS` entries in this commit and
  name the gap each closes.
- **Fixtures that passed now fail** — a short-circuit is firing where C's does
  not, meaning a record is being set earlier than C sets it. Diff the traces to
  find where:

```bash
D=${TMPDIR:-/tmp}/ast-trace3
FIX=crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/<name>.map
AST_TRACE=1 $D/gen_simp $FIX /dev/null 2> /tmp/c.txt
cargo run -q --features tracing --example simp_one -- $FIX 2> /tmp/p.txt >/dev/null
grep -E 'SIMPLIFY-ENTER|SIMPLIFY-WALLED' /tmp/c.txt | sed 's/^ *//' | head -30
sed 's/\x1b\[[0-9;]*m//g' /tmp/p.txt | grep SIMPLIFY-ENTER | head -30
```

C's `SIMPLIFY-WALLED` lines are where its short-circuit fires. If the port walls
somewhere C does not, the record is being set too early — look at the `simplify()`
call that set it, not at the class that read it.

- **Nothing moved** — implausible with 155 sites converted. Check the build
  actually recompiled the tested code.

- [ ] **Step 8: Update the ledger, PLAN.md and the checklists**

Run `cargo test -p ast-core --test test_verification_ledger`, set every count its
failure names, and update `PLAN.md` §6 in the same edit. In
`docs/audits/mapmerge/`, repoint any row whose Rust site was a converted call.

- [ ] **Step 9: Verify and commit**

```bash
cargo clippy -p ast-core --all-targets -- -D warnings
cargo clippy -p ast-core --all-targets --features tracing -- -D warnings
cargo doc -p ast-core --no-deps 2>&1 | grep -cE "^error"
git add crates/ast-core PLAN.md docs
git commit -m "simplify: make bypassing the IsSimple gate a compile error

C has one public entry, astSimplify_, which short-circuits on the IsSimple bit,
calls the per-class Simplify vtab slot and records the result. That slot has
exactly one caller in the library, mapping.c:24772, so every reduction anywhere
in C passes the gate.

The port had 155 calls to the equivalent trait method against 36 to the entry
point, each skipping the gate in both directions: a reduction could go
unrecorded, and a Mapping C returns untouched could be reduced again -- which is
why the port made 149 nominations to C's 96 on cap_specmap_02 while performing
fewer productive merges.

simplified() now takes a ViaSimplify, a token constructible only in the leaf
module that holds simplify(), so the trait method has exactly one possible
caller. The bypass is no longer a convention to be tested for; it does not
compile.

Corpus movement: <list each fixture that changed state and why>."
```

Expected: `0` failures, no warnings either way, `0` doc errors.

---

## Task 2: The keeper-list gate

**Files:**
- Create: `crates/ast-core/tests/test_simplify_seams.rs`
- Modify: `docs/audits/mapmerge/README.md`

**Interfaces:**
- Consumes: Task 1 complete.
- Produces: `only_c_simplify_classes_implement_simplified`, `#[ignore]`d until Task 6. The single-caller rule needs no test — Task 1 made it a compile error.

- [ ] **Step 1: Write the failing test**

Create `crates/ast-core/tests/test_simplify_seams.rs`:

```rust
//! Which classes may implement the per-class simplify seam.
//!
//! C overrides the `Simplify` vtab slot in 11 of this port's classes. For every
//! other class all reduction lives in `MapMerge`, and the base `Simplify`
//! (`mapping.c:16752`) reaches it by nominating the Mapping in a list of one
//! (`mapping.c:16871-16883`).
//!
//! The companion rule — that the seam has one caller — is a compile error rather
//! than a test: `Mapping::simplified` takes a `ViaSimplify`, which only
//! `mappings::simplify_entry` can construct.

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

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
#[ignore = "fails until the 25 stray bodies have moved; see \
            docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md"]
fn only_c_simplify_classes_implement_simplified() {
    let root = src_root();
    let mut stray = Vec::new();
    let mut stack = vec![root.clone()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("source dir readable") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source readable");
            if !text.contains("fn simplified(&self, _gate: ViaSimplify)") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            // `mappings/mod.rs` declares the method; it does not implement it.
            if rel == "mappings/mod.rs" || C_SIMPLIFY_CLASSES.contains(&rel.as_str()) {
                continue;
            }
            stray.push(rel);
        }
    }

    stray.sort();
    assert!(
        stray.is_empty(),
        "these classes implement `simplified()` and their C counterpart does not \
         override the `Simplify` slot, so the reduction belongs in `map_merge` \
         where the base `Simplify` can reach it: {stray:#?}"
    );
}
```

- [ ] **Step 2: Run it to verify it fails and names the right set**

Run: `cargo test -p ast-core --test test_simplify_seams -- --ignored --nocapture`

Expected: FAIL listing 25 paths from `mappings/chebymap.rs` to
`mappings/zoom.rs`, and nothing under `regions/` or `frames/`.

If a Region or `frames/frameset.rs` appears, the keeper list has a typo: paths are
relative to `crates/ast-core/src` and `box_region.rs` keeps its suffix. If the
list is empty, the `text.contains` pattern does not match Task 1's actual
signature — print one impl and align the string.

- [ ] **Step 3: Document the rule where the checklists are read**

Append to `docs/audits/mapmerge/README.md`:

```markdown
## Which seam a reduction belongs in

C has one public entry, `astSimplify`, and one private dispatch behind it — the
`Simplify` vtab slot, whose only caller is `mapping.c:24772`. Eleven classes
override that slot. For every other class **all** reduction lives in `MapMerge`,
and the base `Simplify` reaches it by nominating the Mapping in a list of one
(`mapping.c:16871-16883`).

So a row whose Rust site is a `simplified()` body, in a class whose C counterpart
does not override the slot, is not `ported`: it is in a seam C does not have.
`crates/ast-core/tests/test_simplify_seams.rs` holds the list of 11.

The companion rule — that nothing but `simplify()` reaches the seam — is enforced
by the compiler, not by a checklist: `simplified()` takes a `ViaSimplify` and
only `mappings::simplify_entry` can construct one.
```

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "^test result: FAILED"
cargo clippy -p ast-core --all-targets -- -D warnings
git add crates/ast-core/tests/test_simplify_seams.rs docs/audits/mapmerge/README.md
git commit -m "test: hold the list of classes that may implement the simplify seam

C overrides the Simplify vtab slot in 11 of this port's classes and in none of
the other 25, where every reduction lives in MapMerge. The list is derived by
grepping the C source rather than chosen.

Ignored until the 25 stray bodies have moved. The companion rule -- one caller --
needs no test: it is a compile error since ViaSimplify."
```

Expected: `0` failures, one ignored.

---

## Task 3: Characterisation fixtures for the reductions nothing demands

Five classes keep a reduction in `simplified()` that no fixture reaches, because their `map_merge` never consults it. Moving such a reduction is unobservable, and an unobservable move can silently drop it.

**Files:**
- Modify: `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`
- Create: fixture pairs under both `simplify_fixtures/` trees
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`, `test_verification_ledger.rs`, `PLAN.md`

**Interfaces:**
- Consumes: Task 1, so the counts are the post-gate ones.
- Produces: one `<class>_lone_reduce` positive and one `neg_<class>_no_reduce` control per class needing them.

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

Expected: every line `reaches_simplified=0`. Task 1 may have changed this for a
class; drop any that now reads non-zero and say so in the commit.

- [ ] **Step 2: Read what each `simplified()` reduces**

```bash
for c in chebymap mathmap sphmap unitnormmap wcsmap; do
  f=$(ls crates/ast-core/src/mappings/$c.rs crates/ast-core/src/mappings/$c/mod.rs 2>/dev/null | head -1)
  echo "=== $c ==="; rg -n --color never 'fn simplified' -A 30 "$f" | head -34
done
```

Write down the condition under which each body returns something other than
`self`. That condition is what the fixture must satisfy. A body that only ever
returns a clone has no reduction: record it and skip steps 3 to 6 for it.

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

Expected: every fixture named, and the diff showing more than the `IsSimp` line.

If a `.simp` differs only in `IsSimp`, C does **not** perform that reduction. That
is a finding: the port's `simplified()` does something C does not, so the class
belongs in Task 5's delete-outright bucket. Record it and drop the pair.

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

New positives may fail — that is the point, and each is a gap Task 5 closes. For
each, add a `SERIAL_UNGATED` entry with the reason
`"awaiting the simplified()-to-map_merge move, PLAN.md §10.16"`. Task 5 deletes
it.

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

Expected: `0` failures, `194 passed`.

---

## Task 4: Give ChebyMap the `map_merge` C gives it

**Files:**
- Modify: `crates/ast-core/src/mappings/chebymap.rs`
- Modify: `docs/audits/mapmerge/chebymap.md`, `PLAN.md`

**Interfaces:**
- Consumes: Task 3's ChebyMap pair.
- Produces: `ChebyMap::map_merge`. No new public API.

- [ ] **Step 1: Confirm C's vtab claim before relying on it**

```bash
cd /Users/timj/work/lsst/starlink-astrs
grep -n 'MapMerge = MapMerge' reference/starlink-ast/src/chebymap.c reference/starlink-ast/src/polymap.c
grep -c 'static int MapMerge' reference/starlink-ast/src/chebymap.c
```

Expected: one hit, in `polymap.c`, and `0` for `chebymap.c`. If `chebymap.c`
defines its own, port that instead of delegating and say so.

- [ ] **Step 2: Read PolyMap's `map_merge`**

Run: `rg -n --color never 'fn map_merge' -A 40 crates/ast-core/src/mappings/polymap.rs | head -44`

Note whether the body reads `PolyMap`-only fields. If it does, step 5 needs a
`PolyMap` view; if it only uses `&dyn Mapping` methods, it can take `self`.

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

Adjust the `ChebyMap::new` call to the real signature, which step 2 shows. The
assertion is the part that matters.

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p ast-core --lib linear_chebymap_reduces -- --nocapture`
Expected: FAIL — still a `ChebyMap`, because it has no `map_merge` and takes the
trait default, which only tries the neighbours.

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

Expected: `cheby_linear_lone_reduce` passes, its Task 3 `SERIAL_UNGATED` entry
goes, and the control still passes.

**If the linear replacement loses the ChebyMap's domain scaling** — the open
question in `PLAN.md` §10.2 — one of the pair will disagree with C on values
rather than class. Stop and settle it: build a two-point probe per
`docs/issues/c-oracle-probe-recipe.md`, transform the same position through C's
simplified ChebyMap and the port's, and compare. If C loses the scaling, do not
mirror it — revert, record the finding in `docs/issues/c-library-quirks.md` and
`PLAN.md` §10.2, and raise it upstream.

- [ ] **Step 8: Update the checklist and PLAN.md, verify, commit**

Change `chebymap.md` rows 4 to 12 from `gap: PLAN.md §10.2` to `ported` with the
`chebymap.rs` site, and replace the closing note with step 7's finding. Mark
`PLAN.md` §10.2 accordingly.

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
- Modify: `crates/ast-core/src/mappings/simplify.rs`, `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/mappings/<class>.rs` × 24, one per commit
- Modify: `docs/audits/mapmerge/<class>.md` × 24

**Interfaces:**
- Consumes: Task 4's ChebyMap `map_merge`; Task 3's fixtures.
- Produces: `pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<DynMapping>`; `Mapping::simplified` becomes a provided method calling it. Each moved class ends with no `simplified()`.

### Part A — the driver

- [ ] **Step A1: Read C's loop**

Run: `sed -n '16860,16900p' reference/starlink-ast/src/mapping.c`

Three things it does: calls
`astMapMerge( map, 0, 1, &nmap, &map_list, &invert_list )`; breaks when
`nmap > 1` **or** `modified < 0`; treats no simplification as returning a clone.
The port's version must do all three.

- [ ] **Step A2: Write the failing tests**

Append to `simplify.rs`'s `#[cfg(test)] mod tests`:

```rust
    /// C's base `Simplify` builds a one-element list and calls `astMapMerge`
    /// until nothing changes or the list grows past one
    /// (`mapping.c:16871-16883`). A unit-factor ZoomMap is a UnitMap and
    /// `zoom_map_merge` says so, so the driver must reach that with no
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
Expected: PASS, both.

- [ ] **Step A6: Give the trait method a default**

In `crates/ast-core/src/mappings/mod.rs`, add a body to the declaration Task 1
step 3 wrote:

```rust
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<DynMapping> {
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
be unchanged. **A change means some class was already missing an override.** Find
it before continuing.

```bash
git add crates/ast-core/src
git commit -m "simplify: add C's base Simplify as the simplified() default

C's base Simplify (mapping.c:16752) builds a one-element Mapping list and calls
astMapMerge until nothing changes or the list grows past one. Every class whose C
counterpart does not override the slot inherits it.

No override is removed yet, so nothing is reached differently."
```

### Part B — the 24 moves

Order by fixture coverage. **Group A**, whose `map_merge` already routes through a
self-reduce helper so the move is a deletion the default absorbs: `dssmap`,
`normmap`, `ratemap`, `selectormap`, `splinemap`, `switchmap`.

**Group B**, small and heavily covered: `unit`, `zoom`, `shift`, `perm`, `matrix`,
`winmap`.

**Group C**, the conversion classes with larger bodies: `slamap`, `specmap`,
`timemap`, `lutmap`, `polymap`, `pcdmap`, `grismmap`, `sphmap`, `wcsmap`,
`unitnormmap`, `mathmap`.

**Group D**, orientation-sensitive: `tranmap`.

#### Archetype 1 — a reduction with no encapsulated Mapping (Groups B and C)

`ZoomMap` is the worked example. Its `simplified()` returns a `UnitMap` for a unit
effective factor and a forward `ZoomMap` with the reciprocal otherwise. That is
C's replace-with-simpler half of `zoommap.c`'s `MapMerge`, so it goes at the top
of `zoom_map_merge`, before the run condensing:

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
condensing. Reuse what the file already does rather than introducing a tolerance.

#### Archetype 2 — a reduction over an encapsulated Mapping (Groups A and D)

`RateMap` is the worked example and is already in this shape: `map_merge` routes
through `self_reduce_map_merge`, and `simplified()` orients the child to
`invert_snapshot` before reducing it (`ratemap.c:710-715`). The move is a
**deletion**: the default absorbs the call and the reduction is already reached.
**The orientation handling must move with the body** — dropping it reproduces the
§10.1 bug.

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
delete the `simplified()` impl. Archetype 2: delete the impl only, after
confirming `map_merge` reaches the same logic through `self_reduce_map_merge` or
`try_self_reduce`.

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
override -- and the base Simplify reaches it by nominating the Mapping in a list
of one. The port had it in a simplified() override.

Corpus unchanged: <paste the summary line>."
```

Repeat B1 to B6 for each of the 24, in group order.

---

## Task 6: Turn the gate on and close the entries

**Files:**
- Modify: `crates/ast-core/tests/test_simplify_seams.rs`
- Modify: `PLAN.md`, `docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`

**Interfaces:**
- Consumes: Tasks 1 to 5 complete.
- Produces: `only_c_simplify_classes_implement_simplified` passing without `#[ignore]`.

- [ ] **Step 1: Remove the ignore and run**

```bash
# delete the #[ignore = …] attribute, then:
cargo test -p ast-core --test test_simplify_seams -- --nocapture
```

Expected: PASS.

If it still lists classes, Task 5 has not reached them — finish them rather than
adding them to the keeper list, which is derived from C and is not a place to
record incomplete work.

- [ ] **Step 2: Re-measure §10.3 against the current pin**

§10.3's counts were taken before upstream fixed `cmpmap.c` (`4f2fc0ed`) and
before Task 1, so both sides have moved.

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

Rewrite §10.3's table with the new numbers and the pin they were taken at. If the
counts now agree, §10.3 is closed by this work — say so and delete its
`SERIAL_UNGATED` and `ISSIMP_DISAGREEMENTS` entries.

- [ ] **Step 3: Close the PLAN.md entries**

Mark §10.16 `FIXED` with what the work established. Re-check §10.1 and §10.7,
which both cited the seam, and §10.11, whose four unreached `TranMap` cases may
now be reachable.

- [ ] **Step 4: Mark the spec done**

Change `**Status:** proposed, not started.` to
`**Status:** implemented, <date>.` and add a closing section on what the work
found that the spec did not predict. Two things belong there: the call-site
asymmetry, 155 against 36, which is what the spec was pointing at; and that the
single-caller rule turned out to be enforceable by the compiler through a
capability token, which two earlier drafts of the plan said it was not.

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

One way to simplify a Mapping, as C has, and the compiler enforces it: the
per-class seam takes a token only the entry point can build. Eleven classes
implement that seam -- the eleven whose C counterpart overrides the Simplify vtab
slot -- and every other class keeps its reduction in map_merge, where the base
Simplify reaches it by nominating the Mapping in a list of one."
```

Expected: `0` failures, `194 passed`, no clippy warnings either way, `0` doc
errors.

---

## Self-review

**Spec coverage.** The spec's five steps map to Tasks 3 (fixtures first), 2 (the
sweep, as an enforced classification), 5A (the driver), 5B (the moves), and 6 (the
deletion). Task 1 is the spec's actual point — it says the three seams "lead to
confusion", and the confusion is that 155 sites call the raw virtual against 36
that call the entry point. Task 4 exists because `chebymap` has no `map_merge` and
so cannot participate in Task 5 until it does.

All four spec risks appear: blast radius (Task 1 step 1 and Task 5 step B2's
baselines, plus one class per commit), `IsSimp` counts moving both ways (a Global
Constraint and every comparison step), Region `Simplify` being real (the keeper
table and Task 2), and the orientation-sensitive classes (Group D and Archetype 2).

**Two corrections carried from earlier drafts.** The first draft argued from vtab
shape and had no call-site task at all. The second added one but claimed Rust
could not enforce the single-caller rule, so it proposed converting 65 sites by
hand and testing for regressions with a grep. Both are wrong: the count is 155 not
65, and a capability token makes the bypass a compile error, which turns the
conversion from a careful sweep into a compiler-generated worklist. Task 6 step 4
records both corrections in the spec.

**Placeholder scan.** Task 1 steps 5 and 6 give the transformation and the
judgement rather than 155 diffs; the compiler enumerates the sites, so the list is
generated rather than deferred. Task 3 steps 3 and 5 give one worked class because
steps 1 and 2 determine which classes need one. Task 5B gives two archetypes with
full code and assigns every class to one. Task 4 step 5 carries two branches
because step 2 reads which applies; both are written out.

**Type consistency.** `ViaSimplify` is introduced in Task 1 step 2, used in the
signature in step 3, in the 36 impls in step 5, and named in Task 2's `contains`
pattern and Task 6's commit message — all as `fn simplified(&self, _gate: ViaSimplify)`.
`base_simplify(this: &dyn Mapping) -> AstResult<DynMapping>` is consistent across
Task 5 steps A2, A4 and A6. `MergeCtx::from_single`, `is_frozen`, `entry`, `len`
and `set_pair_single` appear in A4 and Archetype 1, with A4 instructing
substitution if the real API differs. `Mapping::simplify(x)` is used in call
position throughout, matching the inherent method that Task 1 moves to
`simplify_entry`.

**One thing I could not close.** Task 5B cannot state each class's expected corpus
movement in advance, because measuring it is the task. Step B2's baseline capture
is the substitute: the numbers are recorded before the file is touched, so
"identical to the baseline" is checkable without my knowing what the baseline is.
