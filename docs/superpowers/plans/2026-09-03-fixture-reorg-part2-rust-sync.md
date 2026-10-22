# Fixture-Hierarchy Sync, Part 2: Rust Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `crates/ast-core/tests/fixtures/` a byte-identical mirror of the upstream `ast_tester/fixtures/` tree, repoint every consumer at the shared paths and manifests, and land the three behavioral changes the regenerated oracle now expects.

**Architecture:** The oracle section keys become full fixture-root-relative paths, which collapses a five-directory search with a basename fallback into one `join`. The two Rust-only manifests are deleted in favor of upstream's `cases.txt`. Two Rust-only expectations that cannot live in a shared tree — the fixtures the port cannot load, and the one dump the reader refuses — become named lists in Rust source instead of directory boundaries. Three behavioral gaps close: a one-ULP WCS boundary clamp, a `CmpMap::Equal` recursion guard, and un-mirroring a C loader defect that upstream has now fixed.

**Tech Stack:** Rust 2024, `cargo test` tiers 1–4, `cargo fmt` and `cargo clippy --workspace -- -D warnings` as gates.

**Spec:** `docs/superpowers/specs/2026-09-03-upstream-fixture-reorg-design.md` (sections 2, 3, 5, 6 and 7). Part 1 is `2026-09-03-fixture-reorg-part1-upstream-c.md` and **must be complete and merged, with `ctest` green, before Task 1 of this plan.**

## Global Constraints

- **Part 1 first.** Do not start until `ctest --test-dir reference/starlink-ast/build` is green on the upstream branch. This plan's expected values come from Part 1's regenerated oracles.
- **Never push.** Commit locally only. No PRs.
- **`cargo fmt` and `cargo clippy --workspace -- -D warnings` before every commit.** `cargo` lives in the `lsst-scipipe` conda environment; if `clippy-driver` reports `E0514`, use an isolated target directory and an explicit `RUSTC` override.
- **1-based axis indices** everywhere in the public API. **`BAD_VALUE` = `-f64::MAX`** is the output sentinel; `f64::NAN` is accepted on input only.
- **Fixture paths are fixture-root-relative**: every path in an oracle header, a `cases.txt` row, or a Rust literal starts with `simplify/`, `wcsconv/`, `serialisation/`, `oracle/`, `plot/` or `programs/`.
- **No skip modes in a data-driven harness.** A case that cannot be compared goes in a named list with a per-case reason and a companion test that fails when it starts passing. The one exception, argued in spec §6, is the mirror gate, which needs the submodule.
- **Never loosen a negative control** to make a positive case pass, and never widen a global tolerance for one case.
- **Expected intermediate failures.** Tasks 2–6 leave `car6.head`, `CHIPASS_Equ.head` and the 43 formerly-`.rt` fixtures failing. Tasks 7 and 9 close them. Do not add exclusions to make the gates green in between.

---

## File Structure

**Replaced wholesale (Task 2):**
- `crates/ast-core/tests/fixtures/` — every file, from the submodule

**Created:**
- `tools/fixtures/check_mirror.sh` — the one command that proves the trees match

**Modified — the transform-oracle harness (Tasks 3, 7):**
- `crates/ast-core/tests/test_transform_oracle.rs` — `resolve_fixture`, three section predicates, three floors, the FMA substitution table, the framesets section-count assertion

**Modified — the manifest consumers (Task 4):**
- `crates/ast-core/tests/test_wcsconv.rs:58,114` — manifest path and row parsing
- `crates/ast-core/tests/test_simplify_regression.rs` — manifest path and row parsing
- `crates/ast-core/tests/test_verification_ledger.rs:174` — manifest paths and derived counts

**Modified — the round-trip sweeps (Task 5):**
- `crates/ast-core/tests/test_serialisation.rs:1124,1181,1267,1323` — the `.rt` lookup and three corpus paths
- `crates/ast-core/tests/test_deferred_fixtures.rs` — rewritten from a directory registry to a named-fixture registry that asserts the load actually fails

**Modified — library source (Tasks 7, 8, 9):**
- `crates/ast-core/src/mappings/wcsmap.rs:708` — the one-ULP clamp
- `crates/ast-core/src/mappings/cmpmap.rs` — the `Equal` guard
- `crates/ast-core/src/mappings/{zoom,pcdmap,mathmap/mod,perm,lutmap,sphmap,polymap,splinemap,grismmap}.rs` — un-mirror the loader clear

**Deleted:**
- `crates/ast-core/tests/fixtures/native/`, `fits/`, `dssmap/`, `frameset/`, `keymap/` and the three root `.ast` files (absorbed into the mirrored tree)
- `tools/oracle_fixtures/gen_roundtrip_refs.sh`, `tools/oracle_fixtures/ast2ast.c`, `tools/oracle_fixtures/head2ast.c`

---

## Task 1: Move the pin and classify the drift

The sync protocol in `CLAUDE.md` requires a freshness check on every corpus the pin feeds, and a card-only versus numeric-drift classification, before anything is rebuilt on top. Doing this first means a later failure can be attributed to the port rather than to a stale or re-anchored reference.

**Files:**
- Modify: `reference/starlink-ast` (submodule pointer only)

**Interfaces:**
- Consumes: Part 1's merged branch.
- Produces: a recorded drift classification that Tasks 2 and 10 refer to.

- [ ] **Step 1: Move the pin**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git -C reference/starlink-ast log --oneline -1
git add reference/starlink-ast
git status --short reference/starlink-ast
```

- [ ] **Step 2: Run both freshness checks against the old fixture tree**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
./tools/oracle_fixtures/check_corpus_fresh.sh   2>&1 | tail -30
./tools/oracle_fixtures/check_simplify_fresh.sh 2>&1 | tail -30
```

These still read the *old* Rust paths, which is deliberate: they classify the pre-existing corpus against the new oracle, which is the comparison the protocol asks for. They are repointed in Task 6.

- [ ] **Step 3: Classify every reported case**

Card-only drift means the reference predates the new pin and must be regenerated from the oracle. Numeric drift means the reference was captured on another platform and must be left alone. A case reported as both is stale in its cards and platform-drifted in its values: take the cards from a fresh capture and keep the values, which is what `tools/oracle_fixtures/merge_card_hunks.py` does.

Write the classification to `/tmp/drift_classification.txt`, one line per case. Expect the 42 known ULP-drifting simplify sections from spec §7 to appear as numeric drift; anything else needs its own line and reason.

- [ ] **Step 4: Verify nothing else moved**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
diff <(grep '^\[' crates/ast-core/tests/fixtures/oracle/framesets.oracle | sed 's/^\[//;s/  .*//;s|.*/||' | sort) \
     <(grep '^\[' reference/starlink-ast/ast_tester/fixtures/oracle/framesets.oracle | sed 's/^\[//;s/  .*//;s|.*/||' | sort)
```

Expected: no output. `framesets.oracle` has the same 20 sections in both trees and only the paths changed (spec §7), so a difference here means Part 1 altered that corpus and the change needs reading before you build on it.

- [ ] **Step 5: Commit the pin with the classification**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git commit -q reference/starlink-ast -m "$(printf 'ref: move the pin to the reorganised fixture tree\n\nFreshness checks run against both corpora before anything is rebuilt on\ntop, so a later failure is attributable to the port rather than to a stale\nreference. Drift classification:\n\n%s' "$(cat /tmp/drift_classification.txt)")"
```

---

## Task 2: Mirror the fixture tree, and gate it

**Files:**
- Delete: `crates/ast-core/tests/fixtures/` (entirely)
- Create: `crates/ast-core/tests/fixtures/` (copied), `tools/fixtures/check_mirror.sh`

**Interfaces:**
- Consumes: Task 1's pin.
- Produces: `tools/fixtures/check_mirror.sh`, exit 0 when the trees match. Every later task reads the mirrored paths.

- [ ] **Step 1: Write the gate first, and watch it fail**

```bash
mkdir -p tools/fixtures
cat > tools/fixtures/check_mirror.sh <<'SH'
#!/bin/bash
# The Rust fixture tree must be a byte-identical copy of the upstream
# ast_tester/fixtures/ tree at the current submodule pin.
#
# A recursive diff catches both directions at once: an upstream fixture that
# was never copied here, and a local fixture that was never upstreamed.  That
# is the whole invariant, so this is the whole gate.
#
# It needs the submodule, so it runs in the nightly job that initialises it,
# alongside tiers 3 and 4, and hard-fails there.  Skipping it when the
# submodule is absent loses nothing a tier-1 test proves: the fixtures are
# committed here and every harness reads the committed copies.  What this
# measures is drift from upstream, not test coverage.
set -uo pipefail

here=$(cd "$(dirname "$0")/../.." && pwd)
ours=$here/crates/ast-core/tests/fixtures
theirs=$here/reference/starlink-ast/ast_tester/fixtures

if [[ ! -d $theirs ]]; then
    echo "reference/starlink-ast not initialised; run:" >&2
    echo "  git submodule update --init reference/starlink-ast" >&2
    exit 2
fi

if diff -r "$theirs" "$ours"; then
    echo "fixture tree matches upstream at $(git -C "$here/reference/starlink-ast" rev-parse --short HEAD)"
    exit 0
fi

echo >&2
echo "fixture tree differs from upstream. Lines starting 'Only in $theirs'" >&2
echo "name an upstream fixture that was not copied here; lines starting" >&2
echo "'Only in $ours' name a local fixture that was never upstreamed." >&2
exit 1
SH
chmod +x tools/fixtures/check_mirror.sh
./tools/fixtures/check_mirror.sh 2>&1 | tail -20
```

Expected: exit 1, with a long list of differences. That list is the work of Step 2.

- [ ] **Step 2: Replace the tree**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git rm -r -q --cached crates/ast-core/tests/fixtures
rm -rf crates/ast-core/tests/fixtures
cp -a reference/starlink-ast/ast_tester/fixtures crates/ast-core/tests/fixtures
find crates/ast-core/tests/fixtures -name '*.rt' -delete   # expect 0; Part 1 never created them
git add crates/ast-core/tests/fixtures
```

`cp -a` rather than a selective copy is deliberate: `plot/expected/` is copied although nothing reads it until the Plot port, because that is what makes the gate a single recursive diff with no exclusion list (spec §2).

- [ ] **Step 3: Run the gate to see it pass**

```bash
./tools/fixtures/check_mirror.sh
```

Expected: exit 0, printing the pin's short hash.

- [ ] **Step 4: Wire it into CI beside tiers 3 and 4**

Add it to the nightly workflow that already runs `git submodule update --init reference/starlink-ast`, as a step before the tier-3 and tier-4 test invocations. It must not be added to the PR-time job, which does not initialize the submodule and would either skip (a mode this repo forbids in a harness) or fail spuriously.

- [ ] **Step 5: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git add tools/fixtures/check_mirror.sh crates/ast-core/tests/fixtures .github
git commit -q -m "test: mirror the upstream fixture tree, and gate the mirror

crates/ast-core/tests/fixtures is now a byte-identical copy of the upstream
ast_tester/fixtures tree, and check_mirror.sh is one recursive diff that
catches both an upstream fixture we failed to copy and a local fixture that
was never upstreamed. plot/expected/ is copied although unused until the Plot
port, because that is what lets the gate carry no exclusion list.

The Rust test suite does not build at this commit; Tasks 3-9 repoint it."
```

The build being broken mid-range is stated rather than hidden. Tasks 3–6 repoint the consumers; the range is green at Task 9.

---

## Task 3: Collapse `resolve_fixture`, rekey the oracle gates

Upstream's oracle headers now name full fixture-root-relative paths, so the five-candidate search with a basename fallback is not merely unnecessary but wrong: `car1.head` through `car5.head` exist in both `wcsconv/inputs/` and `programs/joye_car_headers/`, and a basename fallback picks whichever directory comes first in the list.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: Task 2's mirrored tree.
- Produces: `resolve_fixture(relpath) -> Option<PathBuf>` resolving exactly one path; `is_head_section`, `is_slamap_section`, `is_simplify_section` keyed on the new paths; three updated floors.

- [ ] **Step 1: Write the failing test — a duplicated basename must resolve by path**

Add to the `resolve_tests` module in `crates/ast-core/tests/test_transform_oracle.rs`:

```rust
/// Upstream holds `car1.head` in two directories with different contents.
/// A resolver that falls back to the basename picks whichever directory it
/// searches first, silently checking the wrong fixture, so resolution must be
/// by full relative path and nothing else.
#[test]
fn duplicated_basenames_resolve_by_path_not_basename() {
    let a = resolve_fixture("wcsconv/inputs/car1.head").expect("wcsconv copy");
    let b = resolve_fixture("programs/joye_car_headers/car1.head").expect("programs copy");
    assert_ne!(a, b, "the two car1.head fixtures must resolve to different files");
    assert!(a.ends_with("wcsconv/inputs/car1.head"));
    assert!(b.ends_with("programs/joye_car_headers/car1.head"));
    assert_ne!(
        std::fs::read_to_string(&a).unwrap(),
        std::fs::read_to_string(&b).unwrap(),
        "if the two copies were identical this test would not prove anything"
    );
}

/// A bare basename is not a valid oracle key any more and must not resolve.
#[test]
fn bare_basename_does_not_resolve() {
    assert!(resolve_fixture("car1.head").is_none());
    assert!(resolve_fixture("brad.map").is_none());
}
```

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p ast-core --test test_transform_oracle resolve_tests -- --nocapture
```

Expected: FAIL. `bare_basename_does_not_resolve` fails because the current fallback resolves it, and the whole test binary may not compile until Step 3 if the old `DIRS` paths no longer exist.

- [ ] **Step 3: Collapse the resolver**

Replace `resolve_fixture` (currently at `test_transform_oracle.rs:266`) in full:

```rust
/// Resolve an oracle relpath to its on-disk fixture. The relpath is relative
/// to the fixture root and names exactly one file — upstream holds
/// `car1.head` through `car5.head` in two directories, so a basename is not a
/// key.
fn resolve_fixture(relpath: &str) -> Option<PathBuf> {
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let p = PathBuf::from(format!("{ROOT}/{relpath}"));
    p.is_file().then_some(p)
}
```

Delete the `DIRS` constant and the `never_resolves_into_generated` test — the directory it guarded against no longer exists, and the resolver can no longer reach a directory it was not given.

- [ ] **Step 4: Rekey the section predicates and floors**

`is_head_section` (`relpath.ends_with(".head")`) needs no change. `is_slamap_section` currently strips to a basename before matching; it can now match on the path directly. `is_simplify_section` is the complement of the other two and needs no change. Update the FMA substitution table's two keys from `simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map` to `simplify/neg_unitnormmap_inv_fwd_diffcentre.map`, and the same for the two `load_fixture` calls in the unit tests that name `simplify_fixtures/matrix_diagonal_to_zoom.map`.

Then recompute the three floors and the framesets section count from the mirrored oracles rather than guessing:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs/crates/ast-core/tests/fixtures/oracle
echo -n "HEAD_SECTION_FLOOR = "; grep -c '^\[.*\.head ' headers.oracle
echo -n "framesets sections = "; grep -c '^\[' framesets.oracle
echo -n "all sections       = "; grep -ch '^\[' *.oracle | paste -sd+ | bc
grep -h '^\[' *.oracle | sed 's/^\[//;s/  .*//' | grep -v '\.head$' \
  | grep -cE '/(cap_)?sla|sla_'
```

Set `HEAD_SECTION_FLOOR`, `SLAMAP_SECTION_FLOOR` and `SIMPLIFY_SECTION_FLOOR` to the measured counts, and the `assert_eq!(reports.len(), 20, ...)` in `runs_framesets_oracle_without_panicking` to the measured framesets count. All three exclusion lists are empty today and stay empty — the floors are what stop a rename or deletion making a gate pass vacuously.

- [ ] **Step 5: Run and commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_transform_oracle resolve_tests
cargo test -p ast-core --test test_transform_oracle every_oracle_section_is_gated
cargo fmt && cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -q -m "test(oracle): resolve fixtures by path, not basename

Oracle section headers now name full fixture-root-relative paths, so the
five-directory search with a basename fallback collapses to one join. The
fallback was not merely redundant: upstream holds car1.head through car5.head
in two directories with different contents, and a basename fallback picks
whichever it searches first.

The three section floors are recomputed from the mirrored oracles. The three
exclusion lists are empty and stay empty."
```

`every_oracle_section_is_gated` passing is the meaningful check here — it proves the three predicates still partition the whole corpus after the rekeying. The golden comparisons themselves still fail until Tasks 7 and 9.

---

## Task 4: Read the shared manifests

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs`, `crates/ast-core/tests/test_simplify_regression.rs`, `crates/ast-core/tests/test_verification_ledger.rs`

**Interfaces:**
- Consumes: Task 2's `fixtures/{simplify,wcsconv}/cases.txt`.
- Produces: both harnesses reading the shared manifests, with the `render_options` column gone and `skip_string_compare` taking blank / `astequal` / `yes`.

- [ ] **Step 1: Write the failing test — the manifest is the shared one**

Add to `crates/ast-core/tests/test_wcsconv.rs`:

```rust
/// The manifest is upstream's, read from the shared tree, so a case added
/// upstream is picked up here without an edit. Its paths are
/// fixture-root-relative, which is what lets one file drive both harnesses.
#[test]
fn manifest_is_the_shared_upstream_one() {
    let cases = load_cases();
    assert!(cases.len() >= 192, "expected at least upstream's 192 rows, got {}", cases.len());
    for c in &cases {
        assert!(
            c.input_file.starts_with("wcsconv/"),
            "row {} names a non-root-relative input {:?}",
            c.name, c.input_file
        );
    }
}
```

and the analogous test in `test_simplify_regression.rs` asserting every row's input starts with `simplify/` or `wcsconv/` (the `lsst` row's input is `wcsconv/inputs/lsst.ast`).

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p ast-core --test test_wcsconv manifest_is_the_shared_upstream_one
```

Expected: FAIL — the harness still reads `wcsconv/wcsconv_tests.txt`, which no longer exists, so this is a read error rather than an assertion failure.

- [ ] **Step 3: Repoint and reparse**

In `test_wcsconv.rs`, change `fixtures_dir()` (line 58) to return `tests/fixtures` rather than `tests/fixtures/wcsconv`, and the manifest read (line 114) to `fixtures_dir().join("wcsconv/cases.txt")`. Because rows now carry root-relative paths, the two `fixtures_dir().join("inputs").join(&case.input_file)` sites (lines 202, 436) and the `join("expected")` site (line 345) become `fixtures_dir().join(&case.input_file)` and `fixtures_dir().join(&case.expected_file)`.

Replace the `compare_mode` parsing with a three-valued enum over the sixth column:

```rust
/// How a case's output is compared to its reference. The column is upstream's
/// `skip_string_compare`, which now carries three values because "relax the
/// last bits" and "do not compare at all" are different licences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CompareMode {
    /// Byte-exact for FITS encodings; numeric tokens to 4 ULP for the
    /// NATIVE family. The default — keep comparisons tight.
    Strict,
    /// Numeric tokens relaxed to AST's astEQUAL tolerance. For a case whose
    /// last bits are cancellation-amplified, e.g. OBSALT from OBSGEO-X/Y/Z.
    AstEqual,
    /// No string comparison. Every such row carries a comment above it in the
    /// manifest naming what varies.
    NoStringCompare,
}

impl CompareMode {
    fn parse(field: &str) -> Result<Self, String> {
        match field.trim() {
            "" => Ok(Self::Strict),
            "astequal" => Ok(Self::AstEqual),
            "yes" => Ok(Self::NoStringCompare),
            other => Err(format!(
                "unknown skip_string_compare value {other:?}; expected blank, \
                 \"astequal\" or \"yes\""
            )),
        }
    }
}
```

Returning an error on an unknown value rather than defaulting is what stops a typo quietly widening the comparison.

In `test_simplify_regression.rs`, repoint the manifest to `fixtures/simplify/cases.txt`, resolve inputs and references as root-relative, and **delete the `render_options` handling** — Part 1 regenerated `lsst.simp` with the Channel's defaults, so no row needs it. An unknown fifth field must be a hard error, not ignored.

- [ ] **Step 4: Update the verification ledger's derived counts**

`test_verification_ledger.rs:174` reads both manifests by path. Repoint them, then recompute every derived count from the new manifests instead of editing the numbers by guess:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs/crates/ast-core/tests/fixtures
echo -n "simplify rows: "; grep -vc '^#\|^$' simplify/cases.txt
echo -n "wcsconv rows:  "; grep -vc '^#\|^$' wcsconv/cases.txt
echo -n "negative fixtures stripping IsSimp: "
grep -v '^#\|^$' simplify/cases.txt | awk -F'|' '{gsub(/ /,"",$2);gsub(/ /,"",$3); if($2==$3) print}' | wc -l
echo -n "simplify skip flags: "; grep -v '^#\|^$' simplify/cases.txt | awk -F'|' '{gsub(/ /,"",$4); print $4}' | grep -c yes
echo -n "wcsconv skip flags:  "; grep -v '^#\|^$' wcsconv/cases.txt | awk -F'|' '{gsub(/ /,"",$6); print $6}' | grep -c yes
```

Part 1's Tasks 9 and 10 re-audited those flag counts down from 165 and 108, so the ledger's totals will drop substantially. That drop is the point: the ledger exists so the number of licensed relaxations cannot drift upward unnoticed, and a fall is evidence the re-audit worked. Record the new numbers with a one-line note saying where they came from.

- [ ] **Step 5: Run both harnesses and commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -20
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -20
cargo test -p ast-core --test test_verification_ledger 2>&1 | tail -20
cargo fmt && cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_wcsconv.rs crates/ast-core/tests/test_simplify_regression.rs crates/ast-core/tests/test_verification_ledger.rs
git commit -q -m "test: read the shared upstream case manifests

Both harnesses now read fixtures/{simplify,wcsconv}/cases.txt from the
mirrored tree, so a case added upstream is picked up here without an edit.
The render_options column is gone -- lsst.simp is regenerated with the
Channel's defaults -- and skip_string_compare takes blank, astequal or yes,
with an unknown value a hard error rather than a silent default.

The ledger's relaxation counts fall, because Part 1 re-audited 273 upstream
skip flags against a comparator that tolerates 4 ULP."
```

Expect `test_wcsconv` to have failures at this point from the FITS-read path, not from the manifest: those are the same cases Task 9 closes.

---

## Task 5: Repoint the round-trip sweeps, delete the `.rt` path, rewrite the deferred registry

The `.rt` override existed only because C could not read back the `IsSimp` card it wrote. Part 1 Task 1 fixed that, so the override has no reason to exist and the round-trip becomes a clean identity over every corpus. Separately, the fixtures the port cannot load move from a directory boundary to a named list — a directory would encode a Rust-only fact as a property of a shared tree (spec §2.2).

**Files:**
- Modify: `crates/ast-core/tests/test_serialisation.rs`, `crates/ast-core/tests/test_deferred_fixtures.rs`, `crates/ast-core/tests/test_native_roundtrip_all.rs`

**Interfaces:**
- Consumes: Task 2's mirrored tree.
- Produces: `SIMPLIFY_DEFERRED: &[(&str, &str)]` — `(fixture file name, reason)` — public within `test_deferred_fixtures.rs`; three sweeps reading `simplify/`, `wcsconv/framesets/` and `serialisation/`.

- [ ] **Step 1: Write the failing tests**

In `crates/ast-core/tests/test_deferred_fixtures.rs`, replace the file's contents with a registry keyed on fixture names, and — the part today's version lacks — an assertion that each one actually fails to load:

```rust
//! Registry for the simplify fixtures this port cannot read.
//!
//! The fixtures live in `tests/fixtures/simplify/` alongside every other
//! fixture, because the tree is a byte-identical mirror of upstream's and a
//! directory boundary would encode a Rust-only fact as a property of both
//! repos. What is Rust-only is the list below.
//!
//! Registering them is deliberate. Registering them without checking would
//! not be: a fixture that starts loading has to surface, or the port silently
//! keeps skipping work it could now do.

/// `(fixture file name, the class that is not ported)`.
///
/// When a class lands, delete its rows and add the cases to
/// `fixtures/simplify/cases.txt`.
const SIMPLIFY_DEFERRED: &[(&str, &str)] = &[
    ("cap_timemap_02.map", "StcResourceProfile not ported"),
    ("cap_timemap_02.simp", "StcResourceProfile not ported"),
    ("cap_xphmap_01.map", "XphMap not ported"),
    ("cap_xphmap_01.simp", "XphMap not ported"),
    ("cap_xphmap_02.map", "XphMap not ported"),
    ("cap_xphmap_02.simp", "XphMap not ported"),
    ("intra_invfwd_cancel.map", "IntraMap not ported"),
    ("intra_invfwd_cancel.simp", "IntraMap not ported"),
    ("intra_invfwd_cancel_tail.map", "IntraMap not ported"),
    ("intra_invfwd_cancel_tail.simp", "IntraMap not ported"),
    ("intramap_inverse_cancel.map", "IntraMap not ported"),
    ("intramap_inverse_cancel.simp", "IntraMap not ported"),
    ("neg_intra_diff_flag.map", "IntraMap not ported"),
    ("neg_intra_diff_func.map", "IntraMap not ported"),
    ("neg_intra_no_simpfi.map", "IntraMap not ported"),
    ("neg_intra_no_simpif.map", "IntraMap not ported"),
    ("neg_intra_nonintra_neighbour.map", "IntraMap not ported"),
    ("neg_intra_parallel.map", "IntraMap not ported"),
    ("neg_intra_same_direction.map", "IntraMap not ported"),
    ("neg_xph_nonxph_neighbour.map", "XphMap not ported"),
    ("neg_xph_not_opposite.map", "XphMap not ported"),
    ("neg_xph_parallel.map", "XphMap not ported"),
    ("xph_inverse_cancel.map", "XphMap not ported"),
    ("xph_inverse_cancel.simp", "XphMap not ported"),
];

fn simplify_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/simplify"))
}

/// Every registered fixture must exist. A rename upstream would otherwise
/// leave a stale row excusing a fixture that is no longer there.
#[test]
fn every_deferred_fixture_exists() {
    let dir = simplify_dir();
    let missing: Vec<_> = SIMPLIFY_DEFERRED
        .iter()
        .filter(|(f, _)| !dir.join(f).is_file())
        .map(|(f, _)| *f)
        .collect();
    assert!(missing.is_empty(), "stale SIMPLIFY_DEFERRED rows: {missing:?}");
}

/// Every registered fixture must still fail to load. This is the assertion
/// that makes the list a gate rather than a comment: when a class lands, this
/// fails and names the fixtures to re-enable.
#[test]
fn every_deferred_fixture_still_fails_to_load() {
    let dir = simplify_dir();
    let mut now_loading = Vec::new();
    for (file, reason) in SIMPLIFY_DEFERRED {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        if ast_core::from_native_string(&text).is_ok() {
            now_loading.push((*file, *reason));
        }
    }
    assert!(
        now_loading.is_empty(),
        "these fixtures now load — delete their SIMPLIFY_DEFERRED rows and add \
         their cases to fixtures/simplify/cases.txt: {now_loading:?}"
    );
    println!("{} deferred fixtures, all still unreadable", SIMPLIFY_DEFERRED.len());
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p ast-core --test test_deferred_fixtures
```

Expected: FAIL to compile or FAIL on `every_deferred_fixture_exists`, because the old file references the deleted `simplify_fixtures_unsupported` directory.

- [ ] **Step 3: Repoint the three sweeps and delete the `.rt` lookup**

In `crates/ast-core/tests/test_serialisation.rs`, delete this block from `native_round_trip_fixture` (around line 1135) along with the comment above it:

```rust
    let rt = path.with_extension(format!(
        "{}.rt",
        path.extension().and_then(|e| e.to_str()).unwrap_or_default()
    ));
    let want = if rt.is_file() {
        std::fs::read_to_string(&rt).map_err(|e| format!("read {}: {e}", rt.display()))?
    } else {
        text.clone()
    };
```

replacing it with `let want = text.clone();` and a comment recording why the override is gone:

```rust
    // The reference is the fixture itself. C used to lose the IsSimp card on
    // load -- a subclass loader validated the attribute it had just read by
    // re-applying it through a setter that clears the simplified record -- so
    // 43 fixtures needed a separate readback reference. Upstream fixed that,
    // and the round trip is the identity again.
    let want = text.clone();
```

Then change the three sweep directories: `simplify_fixtures` → `simplify` (line 1184), `native/reference/frameset_fixtures` → `wcsconv/framesets` (line 1270), `native/generated` → `serialisation` (line 1326). Rename the three test functions to match their corpora (`..._simplify_corpus`, `..._frameset_corpus`, `..._serialisation_corpus`) and update `test_native_roundtrip_all.rs`'s doc comment, which names the old function.

Have the `serialisation/` sweep skip the one dump the reader is required to refuse, by reason rather than by silence:

```rust
        // `frame_in_map_slot.ast` is a dump C wrote that this reader refuses:
        // C accepts a Frame in any Mapping slot because Frame::Transform
        // (frame.c:11739) goes through astUnitMap(astGetNaxes(this)), and this
        // port makes Region the only Frame that is also a Mapping. The refusal
        // is pinned by test_frame_in_map_slot.rs and counted in the
        // verification ledger; region_in_cmpframe.ast beside it is the control
        // that must keep loading.
        if path.file_name().is_some_and(|n| n == "frame_in_map_slot.ast") {
            continue;
        }
```

- [ ] **Step 4: Run the sweeps**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_deferred_fixtures
cargo test -p ast-core --test test_serialisation 2>&1 | tail -25
cargo test -p ast-core --test test_frame_in_map_slot 2>&1 | tail -10
```

Expected: `test_deferred_fixtures` passes. The simplify corpus sweep **fails on 43 fixtures** — with the override gone, the port still mirrors C's old loader behavior and drops the `IsSimp` card. That is the expected intermediate failure named in the Global Constraints; Task 9 closes it. Do not restore the override or add an exclusion.

- [ ] **Step 5: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt && cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_serialisation.rs crates/ast-core/tests/test_deferred_fixtures.rs crates/ast-core/tests/test_native_roundtrip_all.rs
git commit -q -m "test(serial): drop the readback override, repoint the corpora

The 43 .rt files blessed a C defect that upstream has now fixed, so the
round trip is the identity over every corpus and the override path is gone.
The three sweeps read simplify/, wcsconv/framesets/ and serialisation/.

The deferred-fixture registry moves from a directory to a named list, and now
asserts each fixture still fails to load -- the old version only counted
files, so a fixture that became readable stayed hidden.

43 simplify round trips fail at this commit: the port still mirrors the C
loader defect. Task 9 un-mirrors it."
```

---

## Task 6: Path literals, tooling, and the deleted generators

**Files:**
- Modify: ~31 test files, `crates/ast-core/build.rs`, three examples, four `src/` doc comments, `tools/oracle_fixtures/check_corpus_fresh.{sh,py}`, `check_simplify_fresh.{sh,py}`, `gen_frameset_fixtures.sh`, `tools/oracle_fixtures/README.md`
- Delete: `tools/oracle_fixtures/gen_roundtrip_refs.sh`, `tools/oracle_fixtures/ast2ast.c`, `tools/oracle_fixtures/head2ast.c`

**Interfaces:**
- Consumes: Task 2's tree.
- Produces: a compiling test suite with no reference to a pre-mirror path.

- [ ] **Step 1: Enumerate every stale reference**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
rg -n 'native/reference|native/generated|simplify_fixtures|frameset_fixtures|wcsconv_tests|simplify_tests|fixtures/fits/|fixtures/dssmap|fixtures/frameset/|fixtures/keymap|simplify_fixtures_unsupported' \
   crates tools docs --glob '!target/**' | tee /tmp/stale_paths.txt | wc -l
```

- [ ] **Step 2: Rewrite them**

The mapping is mechanical:

| old | new |
|---|---|
| `native/reference/simplify_fixtures/` | `simplify/` |
| `native/reference/frameset_fixtures/` | `wcsconv/framesets/` |
| `native/generated/` | `serialisation/` |
| `native/reference/lsst.ast` | `wcsconv/inputs/lsst.ast` |
| `native/reference/simplify_tests.txt` | `simplify/cases.txt` |
| `wcsconv/wcsconv_tests.txt` | `wcsconv/cases.txt` |
| `wcsconv/inputs/` | unchanged |
| `fits/headers/sip.head` | `wcsconv/inputs/sip.head` |
| `fits/headers/pc-sparse.head` | `wcsconv/inputs/pc-sparse-naxis.head` |
| `dssmap/c_dssmap.ast` | `serialisation/c_dssmap.ast` |
| `frameset/c_variants.ast` | `serialisation/c_variants.ast` |
| `keymap/mapsz_oracle.txt` | `oracle/keymap_mapsz.txt` |
| `jan-dsb.ast` | `serialisation/jan-dsb.ast` |
| `frame_in_map_slot.ast` | `serialisation/frame_in_map_slot.ast` |
| `region_in_cmpframe.ast` | `serialisation/region_in_cmpframe.ast` |

`fits/headers/pc-sparse.head` maps to the `-naxis` variant because that is the file this repo has been testing against — upstream's `pc-sparse.head` lacks the leading `NAXIS = 2` card. Check each `include_str!` of it after rewriting; if a test depends on `NAXIS` being absent, it wants upstream's copy instead.

- [ ] **Step 3: Repoint the tooling and delete what Part 1 replaced**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git rm -q tools/oracle_fixtures/gen_roundtrip_refs.sh tools/oracle_fixtures/ast2ast.c tools/oracle_fixtures/head2ast.c
```

`gen_roundtrip_refs.sh` and `ast2ast.c` are deleted because the defect they captured is fixed; `head2ast.c` because it is now an `ast_tester` cmake target. Update `check_corpus_fresh.py` (its `_rows` call reads `wcsconv_tests.txt`, and it joins `"inputs"` onto each input) and `check_simplify_fresh.sh` to the shared manifests and root-relative paths, and `gen_frameset_fixtures.sh` either to the new output directory or to a note that `head2ast` upstream now owns it. Remove the `.rt` section from `tools/oracle_fixtures/README.md` and the `gen_native_fixtures` / `head2ast` paragraphs that now describe upstream targets.

- [ ] **Step 4: Verify the whole suite compiles and nothing stale remains**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --no-run 2>&1 | tail -20
rg -n 'native/reference|native/generated|simplify_fixtures|frameset_fixtures|wcsconv_tests|simplify_tests' crates tools --glob '!target/**' | grep -v '^Binary'
```

Expected: the suite compiles, and the second command prints nothing. A remaining hit in `docs/` is acceptable at this step and is cleaned in Task 10; a hit in `crates/` or `tools/` is not.

- [ ] **Step 5: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt && cargo clippy --workspace --tests -- -D warnings
git add -A crates tools
git commit -q -m "test: repoint every fixture path at the mirrored tree

Mechanical: simplify_fixtures -> simplify, frameset_fixtures ->
wcsconv/framesets, native/generated -> serialisation, and the four one-off
directories into the shared tree. gen_roundtrip_refs.sh and ast2ast.c are
deleted with the defect they captured; head2ast.c moves upstream as a cmake
target."
```

---

## Task 7: Port the one-ULP WCS boundary clamp

A WinMap used by a FITS CAR mapping evaluates `-0.5 degree + 181 * 0.5 degree` as exactly 90 degrees when the compiler contracts the multiply and add into an FMA, and as one representable double above 90 when it does not. Upstream `460efce3` clamps rather than letting a WCS's validity depend on the compiler. Rust does not contract — rustc has no `-ffp-contract=fast` equivalent on by default, and fusion comes only from an explicit `f64::mul_add` — so this port behaves like the non-FMA C build and must adopt the clamp. See spec §5 R4.

**Files:**
- Modify: `crates/ast-core/src/mappings/wcsmap.rs:708`
- Test: `crates/ast-core/tests/test_wcsmap.rs`

**Interfaces:**
- Consumes: Task 3's rekeyed gates.
- Produces: `car6.head` and `programs/joye_car_headers/CHIPASS_Equ.head` matching the oracle in both directions.

- [ ] **Step 1: Write the failing test**

Add to `crates/ast-core/tests/test_wcsmap.rs`:

```rust
/// The corner pixel of an all-sky CAR header lands one representable double
/// beyond +90 degrees of latitude on any build that does not contract the
/// multiply and add into an FMA — which is every Rust build. It describes the
/// same valid pole, so C clamps it (wcsmap.c, upstream 460efce3) rather than
/// letting a header's validity depend on the compiler. Golden values are the
/// `wcsconv/inputs/car6.head` forward section of
/// `tests/fixtures/oracle/headers.oracle`.
#[test]
fn car_corner_pixel_is_not_bad() {
    let text = include_str!("fixtures/wcsconv/inputs/car6.head");
    let mut fc = ast_core::fitschan::FitsChan::from_header(text).unwrap();
    let obj = fc.read().unwrap().expect("car6.head reads");
    let map: &dyn ast_core::mappings::Mapping = mapping_of(&obj);

    let (lon, lat) = transform_point(map, 5401.0, 1741.0);
    assert!(
        lon != ast_core::constants::BAD_VALUE && lat != ast_core::constants::BAD_VALUE,
        "corner pixel came back BAD: ({lon}, {lat})"
    );
    assert!((lon - 0.0).abs() < 1e-12, "longitude {lon} != 0");
    assert!((lat - 0.453_785_605_518_525_7).abs() < 1e-12, "latitude {lat}");
}
```

Use whichever helper the file already has for extracting a `Mapping` from a read object and transforming one point; if there is none, write `mapping_of` and `transform_point` as small local helpers over `PointSet` rather than inventing a public API.

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p ast-core --test test_wcsmap car_corner_pixel_is_not_bad
```

Expected: FAIL with both values `BAD`.

- [ ] **Step 3: Port the clamp**

In `crates/ast-core/src/mappings/wcsmap.rs`, replace the inverse-direction acceptance test at lines 708-709:

```rust
                        if (!docheck || cyclic || (longitude < longhi && longitude >= longlo))
                            && latitude.abs() <= 90.0
```

with the clamped form, mirroring the C structure:

```rust
                        // Absorb a one-ULP rounding error at a limit of the
                        // accepted range. A WinMap used by a FITS CAR mapping
                        // may evaluate `-0.5 degree + 181*0.5 degree` as
                        // exactly 90 degrees where the compiler emits a fused
                        // multiply-add, and as one representable double above
                        // 90 where it does not, so a header's validity would
                        // otherwise depend on the compiler and the target.
                        // That value describes the same valid pole. Do not
                        // admit the next representable value, which differs
                        // by more than 90*EPSILON. Both latitude limits need
                        // this: the accepted range is closed, so -90 and +90
                        // are both inside it.
                        let mut latitude = latitude;
                        let mut abslat = latitude.abs();
                        if abslat > 90.0 && abslat - 90.0 <= 90.0 * f64::EPSILON {
                            latitude = if latitude < 0.0 { -90.0 } else { 90.0 };
                            abslat = 90.0;
                        }
                        // The accepted longitude range is half-open,
                        // [longlo,longhi), so only its lower limit needs the
                        // same treatment. Perturbing a longitude strictly
                        // below longhi by one ULP leaves it strictly below; a
                        // value at or above longhi is the wrap of longlo and
                        // is clipped deliberately, to stop a whole-sky grid
                        // mapping two pixels to the same meridian. long_range
                        // returns either [-180,180) or [0,360), so the width
                        // of the range is the scale at which a longitude
                        // computed within it rounds.
                        let mut longitude = longitude;
                        if docheck
                            && !cyclic
                            && longitude < longlo
                            && longlo - longitude <= (longhi - longlo) * f64::EPSILON
                        {
                            longitude = longlo;
                        }
                        if (!docheck || cyclic || (longitude < longhi && longitude >= longlo))
                            && abslat <= 90.0
```

This is the faithful port. Do not substitute `f64::mul_add` in the CAR expression instead: that would make this fixture pass while diverging structurally from `wcsmap.c` and leaving the clamp's other exposures unguarded (hazard H2). Using FMA where C's arithmetic is a multiply-add is a separate, separately-audited pass, because switching a site shifts results by ~1 ULP and every site needs re-checking against the oracle.

- [ ] **Step 4: Run the test and the head gate**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_wcsmap car_corner_pixel_is_not_bad
cargo test -p ast-core --test test_transform_oracle head_sections_match_oracle 2>&1 | tail -20
```

Expected: the unit test passes, and `head_sections_match_oracle` no longer reports `car6.head` or `CHIPASS_Equ.head`. `HEAD_ORACLE_EXCLUDED` stays empty.

- [ ] **Step 5: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt && cargo clippy -p ast-core -- -D warnings
git add crates/ast-core/src/mappings/wcsmap.rs crates/ast-core/tests/test_wcsmap.rs
git commit -q -m "wcsmap: absorb a one-ULP error at the coordinate limits

Port of upstream 460efce3. A WinMap used by a FITS CAR mapping evaluates
-0.5 degree + 181*0.5 degree as exactly 90 degrees where the compiler fuses
the multiply and add, and one representable double above 90 where it does
not. Rust never fuses unless asked, so this port took the second branch and
returned BAD for the corner pixel of two all-sky CAR headers.

The clamp is ported rather than replaced by an explicit mul_add: the clamp is
what the C source does, and it guards inputs FMA would not rescue."
```

---

## Task 8: Port `CmpMap::Equal`'s `MapList` guard

`astMapList` refuses to expand a CmpMap for which `astDoNotSimplify` is true (`cmpmap.c:1154`), returning a list holding a clone of the CmpMap itself. An `Equal` that decomposes both operands and compares element by element therefore calls `astEqual` on the same pair again and recurses until the stack is exhausted — reachable by any CmpMap carrying an `Ident`. `MapSplit` already guards the identical hazard at `cmpmap.c:2555`. Upstream fixed `Equal` in `16ed5d37` and `0f797ae7`, both already in the pin; the port and the two new C tests are this task.

**Files:**
- Modify: `crates/ast-core/src/mappings/cmpmap.rs`
- Test: `crates/ast-core/tests/test_cmpmap.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: an `equal` that terminates on protected CmpMaps and preserves effective-order and effective-sense semantics.

- [ ] **Step 1: Read the two C changes and the two new tests**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs/reference/starlink-ast
git show 16ed5d37 -- src/cmpmap.c
git show 0f797ae7 -- src/cmpmap.c
sed -n '/testprotectedcmpequal/,/^}/p' ast_tester/testmapping.c
```

The guard is deliberately narrow: it fires only when *both* decompositions returned the CmpMap itself, which is exactly the case that recursed. A mismatched pair — one blocked, one expanded — already gave differing list lengths and returned zero without recursing, so no comparison that previously produced an answer changes.

- [ ] **Step 2: Write the two failing tests, ported from `testmapping.c`**

Add to `crates/ast-core/tests/test_cmpmap.rs`:

```rust
/// Ported from `testprotectedcmpequal` (`ast_tester/testmapping.c`).
///
/// CmpMap equality keeps its usual effective-order and effective-sense
/// semantics when `Ident` stops `MapList` decomposing the outer CmpMap.
/// Inverting a reversed series pair gives the same effective sequence and
/// component senses, so the two must compare equal — and must not recurse
/// forever getting there.
#[test]
fn protected_cmpmap_equality_keeps_effective_semantics() {
    let a1 = ZoomMap::new(1, 2.0).unwrap();
    let a2 = ZoomMap::new(1, 3.0).unwrap();
    let mut a = CmpMap::series(a1.clone(), a2.clone()).unwrap();
    a.set_ident("keep");

    // The reversed pair, inverted, is the same effective sequence.
    let mut b1 = ZoomMap::new(1, 3.0).unwrap();
    b1.invert();
    let mut b2 = ZoomMap::new(1, 2.0).unwrap();
    b2.invert();
    let mut b = CmpMap::series(b1, b2).unwrap();
    b.set_ident("keep");
    b.invert();

    assert!(a.equal(&b), "protected CmpMaps with the same effective sequence must be equal");
}

/// Ported from `testprotectedcmpequalstable` (`ast_tester/testmapping.c`).
///
/// Comparing protected CmpMaps that share a component must not disturb the
/// component's live `Invert` value. Owned values make the sharing a clone
/// here, so what this pins is that `equal` restores whatever it changed.
#[test]
fn protected_cmpmap_equality_leaves_invert_alone() {
    let mut map1 = ZoomMap::new(1, 2.0).unwrap();
    let map2 = ZoomMap::new(1, 3.0).unwrap();
    let mut a = CmpMap::series(map1.clone(), map2.clone()).unwrap();
    a.set_ident("keep");
    let mut b = CmpMap::series(map1.clone(), map2.clone()).unwrap();
    b.set_ident("keep");

    map1.invert();
    let before = map1.is_inverted();
    assert!(a.equal(&b));
    assert_eq!(map1.is_inverted(), before, "equal() disturbed a component's Invert");
}
```

Adjust the constructor and accessor names to this crate's actual API — check `test_cmpmap.rs` for how it builds a series CmpMap and sets `Ident`, and `test_zoommap.rs` for `ZoomMap`'s constructor. Do not add a public setter for the sake of the test.

- [ ] **Step 3: Run to verify they fail**

```bash
cargo test -p ast-core --test test_cmpmap protected_cmpmap -- --nocapture
```

Expected: the first test either fails or overflows the stack. A stack overflow *is* the failure this task fixes; if the port already guards it, the test passes and you should confirm the guard is genuinely present rather than assuming the hazard is unreachable here.

- [ ] **Step 4: Port the guard**

In `crates/ast-core/src/mappings/cmpmap.rs`'s `equal`, take the same test `MapSplit` takes, and when both decompositions returned the CmpMap itself, compare the component Mappings directly with the `Invert` flags they held at construction temporarily re-instated. That descends into strictly smaller Mappings and terminates. Mirror C's structure rather than inventing a Rust-shaped equivalent — the point of the guard is that it fires in exactly one case, and a restructured version will not.

- [ ] **Step 5: Run the suite and commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_cmpmap
cargo test -p ast-core --test test_equal
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -10
cargo fmt && cargo clippy -p ast-core -- -D warnings
git add crates/ast-core/src/mappings/cmpmap.rs crates/ast-core/tests/test_cmpmap.rs
git commit -q -m "cmpmap: stop equal recursing when MapList declines to decompose

Port of upstream 16ed5d37 and 0f797ae7. astMapList refuses to expand a
CmpMap for which astDoNotSimplify is true and returns a list holding the
CmpMap itself, so an equal that decomposes both operands compared the same
pair again and recursed until the stack was exhausted. Any CmpMap carrying
an Ident reaches it. The guard fires only when both decompositions returned
the CmpMap itself, so no comparison that previously produced an answer
changes.

testprotectedcmpequal and testprotectedcmpequalstable ported from
ast_tester/testmapping.c."
```

`test_simplify_regression` is run because `cap_ident_cmpmap` is the fixture that exposed this upstream; watch that it does not regress.

---

## Task 9: Delete the redundant re-application that mirrors the `IsSimp` loss

The port does not inherit this defect from its structure. It builds a complete `Node` tree and constructs each object in one shot with the right values, so `from_node` would preserve the `IsSimp` card for free. Each `from_node` then *adds* a re-application through a setter, purely to reproduce C's clear. `ZoomMap::from_node` (`zoom.rs:339`) is the pattern:

```rust
let mut m = ZoomMap::new(base.nin, if raw != 0.0 { raw } else { 1.0 })?;   // correct already
if base.inverted { m.invert(); }
m.set_has_been_simplified(base.has_been_simplified);                       // installs the flag
if raw != 0.0 { m.set_zoom(raw)?; }                                        // wipes it again
```

That final `set_zoom(raw)` is a no-op in value terms — the object was constructed with `raw` three lines earlier — and its only effect is the clear. So the fix is a deletion, not a behavior change, which makes it easy to verify: nothing about the constructed object changes except that the flag survives.

Part 1 fixed ten classes in C, so these mirrors are now bugs — the case `CLAUDE.md`'s sync protocol warns about, where a faithful mirror becomes a defect the moment upstream fixes the original. `docs/issues/c-library-quirks.md` names the ten `from_node` sites and the seven fixtures that pin them.

**Files:**
- Modify: `crates/ast-core/src/mappings/{zoom,perm,lutmap,sphmap,pcdmap,mathmap/mod,polymap,splinemap,wcsmap,grismmap}.rs`, `docs/issues/c-library-quirks.md`
- Test: the Task 5 sweep is the test

**Interfaces:**
- Consumes: Task 5's override-free sweep.
- Produces: 43 previously-failing round trips passing; the quirks entry retired.

- [ ] **Step 1: Confirm the sweep still fails on exactly 43**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_serialisation native_byte_exact_round_trip_simplify_corpus 2>&1 | grep -c 'FAIL:'
```

Expected: 43. A different number means Part 1's fix or Task 5's repoint changed the population, and you need to know which before editing ten files.

- [ ] **Step 2: Find all ten sites**

The quirks entry lists them: `ZoomMap`, `MathMap`, `LutMap` (`LutInterp` and `LutEpsilon`), `PolyMap`, `SplineMap`, `SphMap`, `WcsMap`, `PcdMap` (`Disco` and both `PcdCen` axes), `PermMap`, and `GrismMap` (all eight attributes). Confirm against the source rather than trusting the list:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
rg -n 'discards the simplified|clear-on-set' crates/ast-core/src/mappings --glob '*.rs'
```

- [ ] **Step 3: Delete the re-application at each site**

At each one, the pattern is a `set_*` call *after* construction, guarded by a `node.get("Card").is_some()` or a `raw != default` test, whose value the constructor already used. Delete the call and its guard, and replace the comment that justified it:

```rust
        // The IsSimp card records the orientation the object was written in.
        // The object is fully constructed above, so nothing needs re-applying;
        // C used to re-apply through a clear-on-set setter to validate, which
        // discarded the card it had just read, and upstream fixed that.
```

Two need more than a deletion:

- **`MathMap::from_node`** (`mathmap/mod.rs:698`) builds its struct literally rather than through setters, so check whether the mirror is a trailing `set_simp_fi` / `set_simp_if` / seed re-application or a field assignment. Only the former is deleted; the field assignments are how the object is built.
- **`PcdMap::from_node`** (`pcdmap.rs`) re-applies three attributes (`PcdCen` axes 1 and 2, and `Disco`) under three separate `node.get(...).is_some()` guards. All three go; confirm the constructor already received all three values.

Do **not** keep `wcsmap.rs`'s mirror. An earlier draft of this plan said to, on the grounds that `wcsmap.c` has no loader validate-set call — that was a grep artifact, and `wcsmap.c` in fact has five.

- [ ] **Step 4: Run the sweep, the simplify gate, and the oracle**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_serialisation 2>&1 | tail -15
cargo test -p ast-core --test test_simplify_regression 2>&1 | tail -10
cargo test -p ast-core --test test_transform_oracle 2>&1 | tail -15
```

Expected: all pass. The seven fixtures the quirks entry names as pinning the mirror are the ones to watch: `grism_zoom_merge`, `grism_zoom_inv_merge`, `pcd_swap_zoom_simplifies`, `pcd_unit_series_merge`, `cefit_circle_conformal`, `cefit_circle_nonconformal_keep`, and `cap_poly_bent_edges` as the control — the last carries no `SimpFI` / `SimpIF` / `Seeded` card, so it routed nothing and must be unaffected by this change either way.

Preserving nested `IsSimp` means `simplified()` now short-circuits where it previously re-walked, so a simplify regression here is the Rust counterpart of the corpus change Part 1 Task 2 measured — compare against that task's diff rather than treating it as new.

- [ ] **Step 5: Retire the quirks entry and commit**

Move "The `IsSimp` card does not survive being read back, for twelve classes" to the RESOLVED section, in the same form as the `RESOLVED (upstream e40e71b4)` entry above it: name the upstream commit, the ten classes, and that the Rust mirror was a deletion. Keep the diagnosis — the composition of loader routing and clear-on-set — because it is the part worth remembering.

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt && cargo clippy --workspace -- -D warnings
git add crates/ast-core/src/mappings docs/issues/c-library-quirks.md
git commit -q -m "serial: stop re-applying attributes to mirror C's IsSimp loss

from_node builds a complete Node tree and constructs each object in one shot,
so it would preserve the IsSimp card for free. Ten classes then re-applied an
attribute through a setter afterwards, purely to reproduce C's loader
discarding the card -- a no-op in value terms whose only effect was the clear.
Upstream no longer clears on a set that changes nothing, so the mirror is a
bug and the re-applications are deleted.

Closes the 43 round-trip failures that the deleted .rt overrides were hiding."
```

---

## Task 10: Green every tier, and update the records

**Files:**
- Modify: `PLAN.md`, `docs/issues/native-roundtrip-gaps.md`, `docs/issues/native-roundtrip-simplify-gaps.md`, `tools/oracle_fixtures/README.md`, and any doc still naming a pre-mirror path

**Interfaces:**
- Consumes: Tasks 1–9.
- Produces: a green suite and records that match it.

- [ ] **Step 1: Run all four tiers plus the mirror gate**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core 2>&1 | tail -30
cargo test -p ast-core --test '*-regression' 2>&1 | tail -10
./tools/fixtures/check_mirror.sh
cargo test -p ast-ffi-tests 2>&1 | tail -10
cargo test -p ast-oracle-tests 2>&1 | tail -10
```

Expected: all green. Record the counts — they go in the commit message and in `PLAN.md`.

- [ ] **Step 2: Confirm every gate is still a gate**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
rg -n 'ORACLE_EXCLUDED|NATIVE_READ_EXCLUDED|SIMPLIFY_DEFERRED|FRAME_IN_MAP_SLOT_UNDECIDED' crates/ast-core/tests --glob '*.rs' | grep 'const'
cargo test -p ast-core --test test_transform_oracle every_oracle_section_is_gated -- --nocapture
cargo test -p ast-core --test test_verification_ledger -- --nocapture 2>&1 | tail -25
```

The three oracle exclusion lists must still be empty, `every_oracle_section_is_gated` must still prove the three predicates partition the corpus, and the ledger's totals must match what Task 4 recorded. A list that grew during this work is a license someone took without arguing for it.

- [ ] **Step 3: Update `PLAN.md`**

Close §8 with what landed: the pin moved, the tree is mirrored and gated, the manifests are shared, and the three behavioral ports. Replace the sentence "The sync that resolves §9.4 and unblocks §10.2's linear half is still to be planned: upstream has reorganized `ast_tester`, so the pin move needs a plan of its own" with a pointer to the two plans and the spec. Update the C → Rust migration tracker rows for `check_transform_oracle` (new submodule hash, new section count), `gen_simplify_fixtures / check_simplify` (new case count), and `testmapping.c` (the two protected-CmpMap tests now ported). Add the two rules this sync produced, in the same form as the two already recorded there:

- A fixture with no generator is a fixture that cannot be re-baselined. Landing reference bytes without the program that builds them defers work that gets much more expensive once the reference is being trusted.
- A mirrored C quirk needs re-checking on every pin move, not only when a test fails. The 43 `.rt` overrides passed every test in this repo while blessing a defect upstream had the information to fix.

- [ ] **Step 4: Fix the remaining docs**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
rg -ln 'native/reference|native/generated|simplify_fixtures|frameset_fixtures|wcsconv_tests\.txt|simplify_tests\.txt|\.rt\b' docs
```

Rewrite each hit using Task 6's mapping. `docs/issues/native-roundtrip-gaps.md` and `native-roundtrip-simplify-gaps.md` describe corpora that have moved and gaps that Task 9 closed — reconcile their contents, do not just repath them. Leave the two plan files and the spec alone: they are the historical record of this work and correctly describe the old layout as the starting point.

- [ ] **Step 5: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git add PLAN.md docs tools
git commit -q -m "docs: close out the fixture-hierarchy sync

The fixture tree is a byte-identical mirror of upstream's, gated by one
recursive diff; both harnesses read the shared cases.txt; and the three
behavioural changes the regenerated oracle expects have landed. PLAN.md §8
closes with the counts and with the two rules this sync produced."
```

---

## Self-Review

**Spec coverage.** §2 hierarchy → Task 2. §2.2 the two named lists → Task 5 (`SIMPLIFY_DEFERRED`) and Task 5 Step 3 (`frame_in_map_slot.ast` via the existing `FRAME_IN_MAP_SLOT_UNDECIDED`). §2.3 the `.rt` deletion → Task 5; the mirror deletion → Task 9. §3 manifests → Task 4. §5 R1 → Task 1; R2 → Task 2; R3 → Tasks 3, 4, 5, 6; R4 → Tasks 7, 8, 9; R5 → Task 10. §6 the mirror gate → Task 2. §7's facts are used as expected values (Task 1 Step 4, Task 3 Step 4, Task 9 Step 1). §8's capture-host risk belongs to Part 1 Task 11 and is noted there.

**Two places where a step names a search rather than a line.** Task 8 Step 4 (`CmpMap::equal`'s guard) and Task 9 Step 3 (ten `from_node` sites) give the C source to mirror, the command that finds each site, and a test that fails until it is right, but not a diff — because `cmpmap.rs`'s `equal` and the ten `from_node`s have shapes that differ per class, and a fabricated diff would be worse than the instruction to read the C and match it. Both are bracketed by a failing test before and a passing gate after.

**Placeholder scan.** No "TBD", no "add error handling", no "similar to Task N". Task 7's test uses two helper names (`mapping_of`, `transform_point`) with an explicit instruction to use the file's existing equivalents if it has them, because inventing a public API for a test would violate the design.

**Type consistency.** `resolve_fixture(&str) -> Option<PathBuf>` (Task 3) is used unchanged by `load_fixture` and the gates. `CompareMode::{Strict, AstEqual, NoStringCompare}` with `CompareMode::parse` (Task 4) is the only new type. `SIMPLIFY_DEFERRED: &[(&str, &str)]` (Task 5) is `(file name, reason)` — file names, not stems, because both halves of a pair are listed separately. `set_has_been_simplified(bool)` (Task 9) is the existing trait method and is *kept* at every site; what is deleted is the `set_*` attribute call after it. The three renamed sweep functions are `native_byte_exact_round_trip_{simplify,frameset,serialisation}_corpus`, and Task 9 Step 1 and Task 10 use the first of those names.
