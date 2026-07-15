# Transform Oracle Checker (Rust) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the C transform-output oracle *checker* (submodule commit `3a3c3e51`) to Rust as an `#[ignore]` investigative harness that measures how far the Rust port's transform outputs diverge from the committed C reference values.

**Architecture:** Vendor the three committed `.oracle` files as a frozen cross-implementation baseline. A single integration test at `crates/ast-core/tests/test_transform_oracle.rs` parses the oracle, reloads each named fixture through the public Rust API, re-transforms the recorded inputs, runs four checks (golden fwd/inv, equivalence, round-trip), and writes a machine-readable JSON divergence report. The test is `#[ignore]` (out of CI); its pure helpers (parser, comparison, resolver) are covered by ordinary non-ignored unit tests.

**Tech Stack:** Rust 2024, `ast-core` public API only (`FitsChan`, `from_native_string`, `FrameSet`/`Mapping`, `Frame::norm`, `PointSet`). No new crate dependencies — JSON is hand-written.

**Design spec:** `docs/superpowers/specs/2026-07-15-transform-oracle-checker-design.md`.

## Global Constraints

- **Rust 2024 edition; MSRV pinned in `rust-toolchain.toml`.** Copy the style of neighbouring test files.
- **`cargo fmt` and `cargo clippy --workspace -- -D warnings` are CI gates.** Every commit must be fmt- and clippy-clean. `cargo`/`rustc` are in the `lsst-scipipe` conda bin — ensure it is on `PATH`.
- **Public Rust API only in tests** (no private `ast-core` modules), per CLAUDE.md "Porting a test" rule.
- **Fixtures must be committed and work with the submodule absent.** Never symlink to `reference/starlink-ast/`. Never read from `reference/` at test time.
- **`BAD_VALUE = -f64::MAX`** (`ast_core::constants::BAD_VALUE`), bit-identical to C `AST__BAD`. NaN never matches anything.
- **No new workspace/crate dependencies.** `ast-core` has no `serde`; the JSON report is written by a small hand-rolled serializer in the test file.
- **Tolerances (C constants, verbatim):** golden `rtol = atol = 1e-7`; equivalence `rtol = atol = 1e-9`; round-trip absolute `atol = 0.05` px, `rtol = 1e-6`.
- **The investigative test never asserts on divergence.** It runs to completion over all 1606 sections and always writes the report. It panics only on a broken harness (corrupt oracle, unreadable fixture dir, unwritable report).

---

## File Structure

- **Create** `crates/ast-core/tests/test_transform_oracle.rs` — the entire harness: oracle parser, comparison primitives, fixture-path resolver, fixture loader, the four checks with C section-pairing, overrides parser, findings model, hand-rolled JSON report writer, the `#[ignore]` investigative test, and non-ignored unit tests for the pure helpers.
- **Create** `crates/ast-core/tests/fixtures/oracle/simplify_fixtures.oracle` — vendored verbatim from submodule.
- **Create** `crates/ast-core/tests/fixtures/oracle/headers.oracle` — vendored verbatim.
- **Create** `crates/ast-core/tests/fixtures/oracle/framesets.oracle` — vendored verbatim.
- **Create** `crates/ast-core/tests/fixtures/oracle/transform_oracle_overrides.txt` — vendored verbatim.
- **Create** the four absent corpus fixtures (vendored verbatim from submodule `ast_tester/`):
  - `crates/ast-core/tests/fixtures/wcsconv/inputs/offset-sref.head`
  - `crates/ast-core/tests/fixtures/native/reference/plot3d-test1.ast`
  - `crates/ast-core/tests/fixtures/native/reference/sparse.ast`
  - `crates/ast-core/tests/fixtures/native/reference/stcschan-test1-doc3.ast`
- **Modify** `PLAN.md` — record the new harness in the "C → Rust test migration tracker".

Everything lives in one test file because the pieces are cohesive (they all serve the one harness) and share private types; splitting across a `common/` module would only add indirection for no reuse.

---

## Task 1: Vendor the oracle files and four missing fixtures

No code — pure data vendoring, committed on its own so the diff is a clean import of C-provenance files.

**Files:**
- Create: `crates/ast-core/tests/fixtures/oracle/{simplify_fixtures,headers,framesets}.oracle`
- Create: `crates/ast-core/tests/fixtures/oracle/transform_oracle_overrides.txt`
- Create: `crates/ast-core/tests/fixtures/wcsconv/inputs/offset-sref.head`
- Create: `crates/ast-core/tests/fixtures/native/reference/{plot3d-test1,sparse,stcschan-test1-doc3}.ast`

**Interfaces:**
- Consumes: nothing.
- Produces: the oracle corpus at `tests/fixtures/oracle/` and the four fixtures at the paths the resolver (Task 4) searches.

- [ ] **Step 1: Extract the four oracle files from the submodule commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
mkdir -p crates/ast-core/tests/fixtures/oracle
for f in simplify_fixtures.oracle headers.oracle framesets.oracle transform_oracle_overrides.txt; do
  git -C reference/starlink-ast show 3a3c3e51:ast_tester/$f > crates/ast-core/tests/fixtures/oracle/$f
done
```

- [ ] **Step 2: Extract the four absent corpus fixtures**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git -C reference/starlink-ast show 3a3c3e51:ast_tester/offset-sref.head        > crates/ast-core/tests/fixtures/wcsconv/inputs/offset-sref.head
git -C reference/starlink-ast show 3a3c3e51:ast_tester/plot3d-test1.ast        > crates/ast-core/tests/fixtures/native/reference/plot3d-test1.ast
git -C reference/starlink-ast show 3a3c3e51:ast_tester/sparse.ast              > crates/ast-core/tests/fixtures/native/reference/sparse.ast
git -C reference/starlink-ast show 3a3c3e51:ast_tester/stcschan-test1-doc3.ast > crates/ast-core/tests/fixtures/native/reference/stcschan-test1-doc3.ast
```

- [ ] **Step 3: Verify the imports are non-empty and hash-match the source**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
for f in simplify_fixtures.oracle headers.oracle framesets.oracle transform_oracle_overrides.txt; do
  a=$(git -C reference/starlink-ast show 3a3c3e51:ast_tester/$f | md5sum | cut -d' ' -f1)
  b=$(md5sum crates/ast-core/tests/fixtures/oracle/$f | cut -d' ' -f1)
  [ "$a" = "$b" ] && echo "OK $f" || echo "FAIL $f"
done
```
Expected: four `OK` lines. Confirm the three `.oracle` files total 1606 section headers:
```bash
grep -hc '^\[' crates/ast-core/tests/fixtures/oracle/*.oracle | paste -sd+ | bc
```
Expected: `1606`.

- [ ] **Step 4: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git add crates/ast-core/tests/fixtures/oracle crates/ast-core/tests/fixtures/wcsconv/inputs/offset-sref.head crates/ast-core/tests/fixtures/native/reference/plot3d-test1.ast crates/ast-core/tests/fixtures/native/reference/sparse.ast crates/ast-core/tests/fixtures/native/reference/stcschan-test1-doc3.ast
git commit -m "test: vendor transform-oracle corpus and four missing fixtures"
```

---
## Task 2: Comparison primitives

Port the pure tolerance helpers from `transform_oracle_util.c`. These are the numeric heart of every check and are pinned by the exact arm64-vs-x86 pairs the C `test_oracle_util.c` records.

**Files:**
- Create: `crates/ast-core/tests/test_transform_oracle.rs` (start the file with these helpers)

**Interfaces:**
- Consumes: `ast_core::constants::BAD_VALUE`.
- Produces:
  - `fn within_tol(got: f64, reference: f64, rtol: f64, atol: f64) -> bool`
  - `fn within_tol_wrap(got: f64, reference: f64, rtol: f64, atol: f64) -> bool`
  - constants `GOLDEN_RTOL/GOLDEN_ATOL = 1e-7`, `EQUIV_RTOL/EQUIV_ATOL = 1e-9`, `RTRIP_RTOL = 1e-6`, `RTRIP_ATOL = 0.05`.

- [ ] **Step 1: Write the failing tests**

Put at the top of the new file:

```rust
//! Investigative transform-output oracle checker.
//!
//! Ported from the C `check_transform_oracle` / `transform_oracle_util`
//! (reference/starlink-ast `ast_tester/`, submodule commit 3a3c3e51). Reads
//! the committed `.oracle` files, reloads each named fixture through the
//! public Rust API, re-transforms the recorded inputs, and reports how far
//! the Rust outputs diverge from the C reference values. This harness never
//! regenerates the oracle: the C values are the frozen baseline.

use ast_core::constants::BAD_VALUE;

/// Golden tolerance: live outputs vs recorded oracle values. Wide enough to
/// absorb cross-architecture libm differences (~5e-9 near singularities)
/// while still catching real algorithmic change (>= ~1e-6 relative).
const GOLDEN_RTOL: f64 = 1e-7;
const GOLDEN_ATOL: f64 = 1e-7;
/// Equivalence tolerance: live `.map` vs live `.simp`, same run/machine.
const EQUIV_RTOL: f64 = 1e-9;
const EQUIV_ATOL: f64 = 1e-9;
/// Round-trip accuracy: absolute pixel tolerance for GRID-domain corpora.
const RTRIP_RTOL: f64 = 1e-6;
const RTRIP_ATOL: f64 = 0.05;

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// Port of C `oracle_within_tol`: `|got-ref| <= atol + rtol*|ref|`.
/// `BAD_VALUE` matches only `BAD_VALUE`; any NaN never matches.
fn within_tol(got: f64, reference: f64, rtol: f64, atol: f64) -> bool {
    let got_bad = got == BAD_VALUE;
    let ref_bad = reference == BAD_VALUE;
    if got_bad || ref_bad {
        return got_bad && ref_bad;
    }
    if got.is_nan() || reference.is_nan() {
        return false;
    }
    (got - reference).abs() <= atol + rtol * reference.abs()
}

/// Port of C `oracle_within_tol_wrap`: as `within_tol`, but also accepts a
/// difference of one full turn (2*pi) — a longitude may emerge as 0 vs 2*pi
/// or +pi vs -pi across architectures. Angular axes only.
fn within_tol_wrap(got: f64, reference: f64, rtol: f64, atol: f64) -> bool {
    if within_tol(got, reference, rtol, atol) {
        return true;
    }
    if got == BAD_VALUE || reference == BAD_VALUE || got.is_nan() || reference.is_nan() {
        return false;
    }
    ((got - reference).abs() - TWO_PI).abs() <= atol + rtol * reference.abs()
}

#[cfg(test)]
mod primitive_tests {
    use super::*;

    #[test]
    fn within_tol_matches_c_semantics() {
        assert!(within_tol(1.0, 1.0, 0.0, 0.0));
        assert!(within_tol(1.0 + 4e-16, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(1.0001, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol(5e-13, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol(BAD_VALUE, BAD_VALUE, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(BAD_VALUE, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(1.0, BAD_VALUE, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(f64::NAN, 1.0, 1e9, 1e9));
    }

    #[test]
    fn within_tol_wrap_accepts_full_turn_and_ci_pairs() {
        assert!(within_tol_wrap(1.0, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(TWO_PI, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(0.0, TWO_PI, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(
            std::f64::consts::PI,
            -std::f64::consts::PI,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(!within_tol_wrap(1.0, 2.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol_wrap(BAD_VALUE, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        // Exact arm64-ref vs x86-64-got pairs observed in C CI.
        assert!(within_tol_wrap(5.0999352672935785e-17, 6.2831853071795862, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(1.9478449064398693e-20, 6.2831853071795862, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(3.1415926535897931, -3.1415926535897931, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(1.0, 1.0000000047018207, GOLDEN_RTOL, GOLDEN_ATOL));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail to compile then pass**

Run: `cargo test -p ast-core --test test_transform_oracle primitive_tests -- --nocapture`
Expected: compiles and PASSES (both tests). If a `dead_code` warning trips `-D warnings` on the not-yet-used constants, add `#![allow(dead_code)]` at the top of the file with a comment that later tasks consume them.

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle comparison primitives"
```

---
## Task 3: Oracle file parser

Port `oracle_read_file` from `check_transform_oracle.c`. Parse a `.oracle` file into an ordered list of sections, storing inputs and outputs column-major (as C does). A malformed row is a hard error.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: `within_tol` (not directly; Section only holds data).
- Produces:
  - `struct Section { relpath: String, nin: usize, nout: usize, forward: bool, inputs: Vec<Vec<f64>>, outputs: Vec<Vec<f64>> }` where `inputs.len() == nin`, `outputs.len() == nout`, each inner vec length `npoint`.
  - `impl Section { fn npoint(&self) -> usize }`
  - `fn parse_oracle(text: &str) -> Result<Vec<Section>, String>`
  - `fn parse_double(tok: &str) -> Option<f64>` (BAD token → `BAD_VALUE`; else full-token `f64` parse).

- [ ] **Step 1: Write the failing tests**

Add to the file (outside `#[cfg(test)]`, these are used by the real checker):

```rust
/// One corpus entry: recorded inputs and outputs for one fixture+direction.
struct Section {
    relpath: String,
    nin: usize,
    nout: usize,
    forward: bool,
    /// Column-major: `inputs[axis][row]`, `axis in 0..nin`.
    inputs: Vec<Vec<f64>>,
    /// Column-major: `outputs[axis][row]`, `axis in 0..nout`.
    outputs: Vec<Vec<f64>>,
}

impl Section {
    fn npoint(&self) -> usize {
        self.inputs.first().map_or(0, Vec::len)
    }
}

/// Port of C `oracle_parse_double`: the `BAD` token maps to `BAD_VALUE`;
/// otherwise the whole token must parse as an `f64`.
fn parse_double(tok: &str) -> Option<f64> {
    if tok == "BAD" {
        return Some(BAD_VALUE);
    }
    tok.parse::<f64>().ok()
}

/// Parse a whole `.oracle` file. Section headers look like
/// `[<relpath>  nin=N nout=M dir=forward]`; data rows are `nin` inputs then
/// `nout` outputs. A malformed data row is a hard error (a truncated oracle
/// must not pass on reduced coverage), matching the C checker.
fn parse_oracle(text: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let inside = rest.strip_suffix(']').unwrap_or(rest);
            let mut it = inside.split_whitespace();
            let relpath = it.next().ok_or("empty section header")?.to_string();
            let mut nin = None;
            let mut nout = None;
            let mut forward = true;
            for tok in it {
                if let Some(v) = tok.strip_prefix("nin=") {
                    nin = v.parse().ok();
                } else if let Some(v) = tok.strip_prefix("nout=") {
                    nout = v.parse().ok();
                } else if let Some(v) = tok.strip_prefix("dir=") {
                    forward = v != "inverse";
                }
            }
            let nin = nin.ok_or_else(|| format!("bad section header: {line}"))?;
            let nout = nout.ok_or_else(|| format!("bad section header: {line}"))?;
            sections.push(Section {
                relpath,
                nin,
                nout,
                forward,
                inputs: vec![Vec::new(); nin],
                outputs: vec![Vec::new(); nout],
            });
            continue;
        }
        let cur = sections
            .last_mut()
            .ok_or("data row before any section header")?;
        let vals: Vec<f64> = line
            .split_whitespace()
            .map(|t| parse_double(t).ok_or_else(|| format!("unparseable token {t:?}")))
            .collect::<Result<_, _>>()?;
        if vals.len() != cur.nin + cur.nout {
            return Err(format!(
                "malformed data row in [{}]: got {} of {} columns",
                cur.relpath,
                vals.len(),
                cur.nin + cur.nout
            ));
        }
        for (axis, v) in vals.iter().take(cur.nin).enumerate() {
            cur.inputs[axis].push(*v);
        }
        for (axis, v) in vals.iter().skip(cur.nin).enumerate() {
            cur.outputs[axis].push(*v);
        }
    }
    Ok(sections)
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parses_header_bad_token_and_direction() {
        let text = "# header\n\n\
                    [foo.map  nin=2 nout=1 dir=forward]\n\
                    \x201 2  3\n\
                    \x204 5  BAD\n\n\
                    [bar.head  nin=1 nout=1 dir=inverse]\n\
                    \x207  8\n";
        let secs = parse_oracle(text).expect("parse ok");
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].relpath, "foo.map");
        assert_eq!((secs[0].nin, secs[0].nout, secs[0].forward), (2, 1, true));
        assert_eq!(secs[0].npoint(), 2);
        assert_eq!(secs[0].inputs[0][0], 1.0);
        assert_eq!(secs[0].inputs[1][1], 5.0);
        assert_eq!(secs[0].outputs[0][0], 3.0);
        assert_eq!(secs[0].outputs[0][1], BAD_VALUE);
        assert!(!secs[1].forward);
    }

    #[test]
    fn malformed_row_is_hard_error() {
        let text = "[foo.map  nin=2 nout=1 dir=forward]\n  1 2\n";
        assert!(parse_oracle(text).is_err());
    }

    #[test]
    fn parses_all_three_committed_oracles() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
        let mut total = 0;
        for name in ["simplify_fixtures.oracle", "headers.oracle", "framesets.oracle"] {
            let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
            total += parse_oracle(&text).expect("committed oracle parses").len();
        }
        assert_eq!(total, 1606);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_transform_oracle parse_tests -- --nocapture`
Expected: all three PASS (the third depends on Task 1's vendored files).

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle file parser"
```

---
## Task 4: Fixture-path resolver

The oracle relpaths assume the C `ast_tester/` root, but the Rust fixtures are scattered across several directories, with real basename collisions. Investigation established that an ordered search list resolves all 817 relpaths to the byte-identical submodule content, provided `native/generated/` (which holds *Rust output*, e.g. a divergent `lsst.ast`) is never searched. This task builds that resolver.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `fn resolve_fixture(relpath: &str) -> Option<std::path::PathBuf>` — returns the on-disk path of a fixture named by an oracle relpath, or `None` if absent.

**Background (verified during planning — do not re-derive):**
- Search order, all relative to `crates/ast-core/tests/fixtures/`:
  1. `wcsconv/inputs/`
  2. `native/reference/`
  3. `native/reference/simplify_fixtures/`
  4. `native/reference/simplify_fixtures_unsupported/xphmap/`
  5. `native/reference/simplify_fixtures_unsupported/intramap/`
  6. `native/reference/simplify_fixtures_unsupported/stcresourceprofile/`
  7. `wcsconv/expected/`
  8. `fits/headers/`
- For each directory, try the full relpath first (preserves `simplify_fixtures/foo.map`), then the basename.
- `native/generated/` is deliberately excluded: it contains Rust-produced dumps that legitimately differ from the C source (`lsst.ast`, `degen1.ast`, `timj.ast`). Searching it would load the wrong bytes.
- With Task 1's vendoring, this order resolves every oracle relpath; only fixtures under `simplify_fixtures_unsupported/` (XphMap not yet ported, IntraMap intentionally skipped) will later surface as `LoadFail`/`Skipped`, which is the divergence signal we want.

- [ ] **Step 1: Write the failing tests**

Add to the file:

```rust
use std::path::PathBuf;

/// Resolve an oracle relpath (rooted at the C `ast_tester/` dir) to the
/// on-disk Rust fixture. Searches a fixed, ordered list of the directories
/// the corpus was vendored into, trying the full relpath then the basename
/// in each. `native/generated/` is intentionally excluded: it holds
/// Rust-produced dumps that differ from the C-provenance source.
fn resolve_fixture(relpath: &str) -> Option<PathBuf> {
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    const DIRS: [&str; 8] = [
        "wcsconv/inputs",
        "native/reference",
        "native/reference/simplify_fixtures",
        "native/reference/simplify_fixtures_unsupported/xphmap",
        "native/reference/simplify_fixtures_unsupported/intramap",
        "native/reference/simplify_fixtures_unsupported/stcresourceprofile",
        "wcsconv/expected",
        "fits/headers",
    ];
    let basename = relpath.rsplit('/').next().unwrap_or(relpath);
    for dir in DIRS {
        for candidate in [format!("{ROOT}/{dir}/{relpath}"), format!("{ROOT}/{dir}/{basename}")] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn resolves_every_oracle_relpath() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
        let mut missing = Vec::new();
        for name in ["simplify_fixtures.oracle", "headers.oracle", "framesets.oracle"] {
            let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
            for s in parse_oracle(&text).unwrap() {
                if resolve_fixture(&s.relpath).is_none() {
                    missing.push(s.relpath);
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "unresolved fixtures: {missing:?}");
    }

    #[test]
    fn never_resolves_into_generated() {
        // lsst.ast exists in native/generated (Rust output) and in
        // native/reference + wcsconv/inputs (C source). The resolver must
        // pick a non-generated copy.
        let p = resolve_fixture("lsst.ast").expect("lsst.ast resolves");
        assert!(!p.to_string_lossy().contains("native/generated"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_transform_oracle resolve_tests -- --nocapture`
Expected: both PASS. If `resolves_every_oracle_relpath` reports unresolved fixtures, Task 1 vendoring is incomplete — fix that, do not add directories to work around it.

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle fixture-path resolver"
```

---
## Task 5: Fixture loader

Port `oracle_load_mapping`. Load a fixture into an owned object, exposing it as a `&dyn Mapping` and — for FrameSet fixtures — the base and current `&dyn Frame` (needed later for angular-axis detection). Because Rust returns owned values, the loader returns an owned `AnyObject` the caller keeps alive, plus the frame indices.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: `resolve_fixture`; `ast_core::{from_native_string, AnyObject}`; `ast_core::fitschan::FitsChan`; `ast_core::frames::FrameRef`.
- Produces:
  - `struct Loaded { obj: AnyObject, is_frameset: bool }`
  - `impl Loaded { fn mapping(&self) -> Option<&dyn Mapping>; fn frame(&self, forward: bool) -> Option<&dyn Frame> }`
    - `mapping()` returns the transform under test (for a FrameSet, itself as base→current Mapping; for a bare Mapping, itself; `None` for a non-Mapping such as a KeyMap).
    - `frame(forward)` returns the output frame for angular detection: current frame when `forward`, base frame otherwise; `None` for a bare Mapping.
  - `fn load_fixture(relpath: &str) -> Result<Option<Loaded>, String>` — `Ok(None)` when the fixture resolves but is not a Mapping (skip); `Err` when it resolves but fails to read.

- [ ] **Step 1: Write the failing tests**

Add imports at the top and the loader:

```rust
use ast_core::AnyObject;
use ast_core::fitschan::FitsChan;
use ast_core::frames::{Frame, FrameRef};
use ast_core::mappings::Mapping;

/// A loaded fixture: the owned object plus whether it is a FrameSet (so the
/// checker knows to pull base/current frames for angular detection).
struct Loaded {
    obj: AnyObject,
    is_frameset: bool,
}

impl Loaded {
    /// The mapping under test. For a FrameSet this is the object itself
    /// (it implements `Mapping` as base->current); for a bare Mapping,
    /// itself; `None` for a non-Mapping object.
    fn mapping(&self) -> Option<&dyn Mapping> {
        self.obj.as_mapping()
    }

    /// The output frame for a given direction: current frame (forward) or
    /// base frame (inverse). `None` unless the fixture is a FrameSet.
    fn frame(&self, forward: bool) -> Option<&dyn Frame> {
        if !self.is_frameset {
            return None;
        }
        let AnyObject::FrameSet(fs) = &self.obj else {
            return None;
        };
        let which = if forward { FrameRef::Current } else { FrameRef::Base };
        fs.frame(which).ok()
    }
}

/// Load a fixture named by an oracle relpath. Dispatches by extension:
/// `.head` through a `FitsChan`, everything else through
/// `from_native_string`. Returns `Ok(None)` for a resolved-but-non-Mapping
/// object (KeyMap, non-Region STC), `Err` for a resolved-but-unreadable one.
fn load_fixture(relpath: &str) -> Result<Option<Loaded>, String> {
    let path = match resolve_fixture(relpath) {
        Some(p) => p,
        None => return Err(format!("fixture not found: {relpath}")),
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {relpath}: {e}"))?;

    let obj = if relpath.ends_with(".head") {
        let mut fc = FitsChan::from_header(&text).map_err(|e| format!("from_header {relpath}: {e}"))?;
        match fc.read().map_err(|e| format!("read {relpath}: {e}"))? {
            Some(o) => o,
            None => return Err(format!("FitsChan read returned None: {relpath}")),
        }
    } else {
        from_native_string(&text).map_err(|e| format!("from_native_string {relpath}: {e}"))?
    };

    let is_frameset = matches!(obj, AnyObject::FrameSet(_));
    if obj.as_mapping().is_none() {
        return Ok(None); // resolved but not a Mapping — legitimately skipped
    }
    Ok(Some(Loaded { obj, is_frameset }))
}

use ast_core::from_native_string;
```

```rust
#[cfg(test)]
mod loader_tests {
    use super::*;

    #[test]
    fn loads_native_mapping() {
        let l = load_fixture("simplify_fixtures/matrix_diagonal_to_zoom.map")
            .expect("no error")
            .expect("is a mapping");
        let m = l.mapping().expect("mapping");
        assert_eq!(m.nin(), 2);
        assert!(l.frame(true).is_none(), "bare mapping has no frame");
    }

    #[test]
    fn loads_head_frameset_with_frames() {
        let l = load_fixture("cobe.head").expect("no error").expect("is a mapping");
        assert!(l.is_frameset);
        let m = l.mapping().expect("mapping");
        assert_eq!(m.nin(), 2);
        assert!(l.frame(true).is_some(), "forward -> current frame");
        assert!(l.frame(false).is_some(), "inverse -> base frame");
    }
}
```

Note on import placement: keep all `use` at the top of the file with the others; the `use ast_core::from_native_string;` shown inline above is illustrative — during implementation, merge it into the top import block so clippy's `-D warnings` does not flag a mid-file `use`.

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_transform_oracle loader_tests -- --nocapture`
Expected: both PASS. `cobe.head` is a sky FrameSet (base GRID, current SkyFrame), verified in the C `test_oracle_util.c`.

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle fixture loader"
```

---
## Task 6: Angular-axis detection and the transform helper

Port `axis_is_cyclic` (via `Frame::norm`) and a helper that runs a section's recorded inputs through the live mapping into a column-major output matrix matching the oracle's layout.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: `Section`, `Loaded`, `ast_core::coords::PointSet`, `Mapping::transform`, `Frame::norm`.
- Produces:
  - `fn axis_is_cyclic(frame: &dyn Frame, axis: usize) -> bool`
  - `fn angular_flags(loaded: &Loaded, section: &Section) -> Vec<bool>` — one flag per output axis (`section.nout` long); for a bare Mapping every axis is angular (C fallback).
  - `fn transform_section(map: &dyn Mapping, s: &Section) -> Result<Vec<Vec<f64>>, String>` — column-major `[nout][npoint]`; `Err` on arity mismatch.

- [ ] **Step 1: Write the failing tests**

```rust
use ast_core::coords::PointSet;

/// Port of C `axis_is_cyclic`: is output `axis` of `frame` 2*pi-periodic?
/// Set a point, shift `axis` by 2*pi, `norm` both, and treat the axis as
/// cyclic iff it returns to the original value (true for a SkyFrame
/// longitude; false for GRID/pixel, spectral, linear axes).
fn axis_is_cyclic(frame: &dyn Frame, axis: usize) -> bool {
    let naxes = frame.naxes();
    if axis >= naxes {
        return false;
    }
    let mut p = vec![0.1_f64; naxes];
    let mut q = vec![0.1_f64; naxes];
    q[axis] += TWO_PI;
    if frame.norm(&mut p).is_err() || frame.norm(&mut q).is_err() {
        return false;
    }
    (p[axis] - q[axis]).abs() < 1e-6
}

/// One angular flag per output axis. Bare Mappings (no frame) are all-angular,
/// matching C (coordinate mappings with no pixel axes to protect).
fn angular_flags(loaded: &Loaded, section: &Section) -> Vec<bool> {
    match loaded.frame(section.forward) {
        Some(frame) => (0..section.nout).map(|a| axis_is_cyclic(frame, a)).collect(),
        None => vec![true; section.nout],
    }
}

/// Run the recorded inputs through the live mapping. Returns column-major
/// `[nout][npoint]`. `Err` on arity mismatch (a structural fixture change).
fn transform_section(map: &dyn Mapping, s: &Section) -> Result<Vec<Vec<f64>>, String> {
    let (want_in, want_out) = if s.forward { (s.nin, s.nout) } else { (s.nout, s.nin) };
    if map.nin() != want_in || map.nout() != want_out {
        return Err(format!(
            "arity: fixture nin/nout {}/{} != mapping {}/{}",
            want_in, want_out, map.nin(), map.nout()
        ));
    }
    // Build a PointSet from the recorded inputs (row-major rows of length nin).
    let npoint = s.npoint();
    let rows: Vec<Vec<f64>> = (0..npoint)
        .map(|r| (0..s.nin).map(|a| s.inputs[a][r]).collect())
        .collect();
    let pts = PointSet::from_rows(rows);
    let out = map.transform(&pts, s.forward).map_err(|e| format!("transform: {e}"))?;
    // Re-lay column-major to match the oracle.
    let cols: Vec<Vec<f64>> = (0..s.nout)
        .map(|a| (0..out.npoint()).map(|r| out.point(r)[a]).collect())
        .collect();
    Ok(cols)
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn skyframe_longitude_is_cyclic_pixel_is_not() {
        let l = load_fixture("cobe.head").unwrap().unwrap();
        let flags = angular_flags(&l, &Section {
            relpath: "cobe.head".into(),
            nin: 2, nout: 2, forward: true,
            inputs: vec![vec![], vec![]],
            outputs: vec![vec![], vec![]],
        });
        // A sky FrameSet's current frame has a cyclic longitude axis.
        assert!(flags.iter().any(|&f| f), "at least one axis cyclic (sky lon)");
    }

    #[test]
    fn bare_mapping_all_axes_angular() {
        let l = load_fixture("simplify_fixtures/matrix_diagonal_to_zoom.map").unwrap().unwrap();
        let s = Section {
            relpath: "x.map".into(), nin: 2, nout: 2, forward: true,
            inputs: vec![vec![], vec![]], outputs: vec![vec![], vec![]],
        };
        assert_eq!(angular_flags(&l, &s), vec![true, true]);
    }

    #[test]
    fn transform_reproduces_oracle_shape() {
        let l = load_fixture("simplify_fixtures/matrix_diagonal_to_zoom.map").unwrap().unwrap();
        let s = Section {
            relpath: "x.map".into(), nin: 2, nout: 2, forward: true,
            inputs: vec![vec![1.0, 2.0], vec![3.0, 4.0]],
            outputs: vec![vec![0.0, 0.0], vec![0.0, 0.0]],
        };
        let out = transform_section(l.mapping().unwrap(), &s).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 2);
    }
}
```

Before writing, confirm `Frame::naxes()` exists (public). If the accessor is named differently, grep `crates/ast-core/src/frames/mod.rs` for the axis-count method and use that name; the C code calls `astGetI(frame,"Naxes")`.

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_transform_oracle transform_tests -- --nocapture`
Expected: all three PASS.

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle angular detection and transform helper"
```

---
## Task 7: Overrides parser

Port `load_overrides` / `rtrip_tol` / `golden_off`. Two independent per-fixture entry kinds keyed by relpath (a fixture may have both): round-trip lines (`off` or loosened tolerances) and golden lines (`golden|golden-fwd|golden-inv off`).

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: `RTRIP_RTOL`, `RTRIP_ATOL`.
- Produces:
  - `struct Overrides { ... }` (opaque)
  - `fn parse_overrides(text: &str) -> Overrides`
  - `impl Overrides`:
    - `fn rtrip(&self, relpath: &str) -> Option<(f64, f64)>` — `None` = round-trip disabled; `Some((rtol, atol))` = enabled with these tolerances (defaults when no line).
    - `fn golden_off(&self, relpath: &str, forward: bool) -> bool`

- [ ] **Step 1: Write the failing tests**

```rust
use std::collections::HashMap;

#[derive(Clone, Copy)]
struct RtripRule {
    off: bool,
    rtol: f64,
    atol: f64,
}
#[derive(Clone, Copy, Default)]
struct GoldenRule {
    fwd_off: bool,
    inv_off: bool,
}

/// Parsed `transform_oracle_overrides.txt`. Round-trip and golden rules are
/// looked up independently, matching the C checker.
#[derive(Default)]
struct Overrides {
    rtrip: HashMap<String, RtripRule>,
    golden: HashMap<String, GoldenRule>,
}

fn parse_overrides(text: &str) -> Overrides {
    let mut o = Overrides::default();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(rel), Some(what)) = (it.next(), it.next()) else {
            continue;
        };
        if let Some(kind) = what.strip_prefix("golden") {
            let entry = o.golden.entry(rel.to_string()).or_default();
            let (fwd, inv) = match kind {
                "" => (true, true),           // "golden"
                "-fwd" => (true, false),
                "-inv" => (false, true),
                _ => (false, false),
            };
            entry.fwd_off |= fwd;
            entry.inv_off |= inv;
        } else if what == "off" {
            o.rtrip.insert(rel.to_string(), RtripRule { off: true, rtol: 0.0, atol: 0.0 });
        } else {
            // "<rtol> <atol>" loosened tolerances (fall back to defaults).
            let rtol = what.parse().unwrap_or(RTRIP_RTOL);
            let atol = it.next().and_then(|t| t.parse().ok()).unwrap_or(RTRIP_ATOL);
            o.rtrip.insert(rel.to_string(), RtripRule { off: false, rtol, atol });
        }
    }
    o
}

impl Overrides {
    /// `None` = round-trip disabled for this fixture; `Some((rtol, atol))`
    /// = enabled with these tolerances.
    fn rtrip(&self, relpath: &str) -> Option<(f64, f64)> {
        match self.rtrip.get(relpath) {
            Some(r) if r.off => None,
            Some(r) => Some((r.rtol, r.atol)),
            None => Some((RTRIP_RTOL, RTRIP_ATOL)),
        }
    }

    fn golden_off(&self, relpath: &str, forward: bool) -> bool {
        self.golden.get(relpath).is_some_and(|g| if forward { g.fwd_off } else { g.inv_off })
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;

    #[test]
    fn parses_independent_rtrip_and_golden_lines() {
        let text = "# comment\n\
                    rtoff.head off\n\
                    loose.head 2e-3 4e-2\n\
                    noisy.map golden-inv off\n\
                    both.map golden off\n\
                    fwdonly.map golden-fwd off\n";
        let o = parse_overrides(text);
        assert_eq!(o.rtrip("rtoff.head"), None);
        assert_eq!(o.rtrip("loose.head"), Some((2e-3, 4e-2)));
        // golden-only fixture keeps default round-trip enabled
        assert_eq!(o.rtrip("noisy.map"), Some((RTRIP_RTOL, RTRIP_ATOL)));
        assert!(!o.golden_off("rtoff.head", true) && !o.golden_off("rtoff.head", false));
        assert!(!o.golden_off("noisy.map", true) && o.golden_off("noisy.map", false));
        assert!(o.golden_off("both.map", true) && o.golden_off("both.map", false));
        assert!(o.golden_off("fwdonly.map", true) && !o.golden_off("fwdonly.map", false));
    }

    #[test]
    fn parses_committed_overrides_file() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/oracle/transform_oracle_overrides.txt"
        );
        let o = parse_overrides(&std::fs::read_to_string(path).unwrap());
        // tnx-cheb.head has round-trip disabled.
        assert_eq!(o.rtrip("tnx-cheb.head"), None);
        // neg_unitnormmap_inv_fwd_diffcentre.map skips golden inverse only.
        let rel = "simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map";
        assert!(o.golden_off(rel, false) && !o.golden_off(rel, true));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_transform_oracle override_tests -- --nocapture`
Expected: both PASS.

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle overrides parser"
```

---
## Task 8: Findings model and the four checks

Define the per-section outcome model and the four comparison functions. These are the numeric checks; Task 9 drives them with section pairing.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: `within_tol`, `within_tol_wrap`, tolerance constants.
- Produces:
  - `enum CheckKind { GoldenFwd, GoldenInv, Equivalence, RoundTrip }`
  - `struct AxisMiss { check: CheckKind, row: usize, axis: usize, reference: f64, got: f64, rel: f64 }`
  - `fn axis_match(got: f64, reference: f64, rtol: f64, atol: f64, angular: bool) -> bool`
  - `fn compare_golden(check: CheckKind, live: &[Vec<f64>], recorded: &[Vec<f64>], angular: &[bool]) -> Vec<AxisMiss>`
  - `fn compare_equiv(map_live: &[Vec<f64>], simp_live: &[Vec<f64>], map_ref: &[Vec<f64>], simp_ref: &[Vec<f64>], angular: &[bool]) -> Vec<AxisMiss>`
  - `fn compare_rtrip(map: &dyn Mapping, fwd: &Section, inv_live: &[Vec<f64>], rtol: f64, atol: f64) -> Vec<AxisMiss>`

- [ ] **Step 1: Write the failing tests**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckKind {
    GoldenFwd,
    GoldenInv,
    Equivalence,
    RoundTrip,
}

#[derive(Clone, Copy, Debug)]
struct AxisMiss {
    check: CheckKind,
    row: usize,
    axis: usize,
    reference: f64,
    got: f64,
    rel: f64,
}

fn rel_diff(got: f64, reference: f64) -> f64 {
    let d = (got - reference).abs();
    if reference != 0.0 { d / reference.abs() } else { d }
}

/// Wrap-aware on angular axes, plain otherwise (C `axis_match`).
fn axis_match(got: f64, reference: f64, rtol: f64, atol: f64, angular: bool) -> bool {
    if angular {
        within_tol_wrap(got, reference, rtol, atol)
    } else {
        within_tol(got, reference, rtol, atol)
    }
}

/// Golden check A/B: live output vs recorded output, per axis.
fn compare_golden(
    check: CheckKind,
    live: &[Vec<f64>],
    recorded: &[Vec<f64>],
    angular: &[bool],
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = recorded.first().map_or(0, Vec::len);
    for row in 0..npoint {
        for (axis, rec_col) in recorded.iter().enumerate() {
            let (got, reference) = (live[axis][row], rec_col[row]);
            if !axis_match(got, reference, GOLDEN_RTOL, GOLDEN_ATOL, angular[axis]) {
                misses.push(AxisMiss { check, row, axis, reference, got, rel: rel_diff(got, reference) });
            }
        }
    }
    misses
}

/// Equivalence check C, gated on stored agreement: compare live `.map` vs
/// live `.simp` only where the recorded values already agree.
fn compare_equiv(
    map_live: &[Vec<f64>],
    simp_live: &[Vec<f64>],
    map_ref: &[Vec<f64>],
    simp_ref: &[Vec<f64>],
    angular: &[bool],
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = map_ref.first().map_or(0, Vec::len);
    for row in 0..npoint {
        for axis in 0..map_ref.len() {
            if !axis_match(map_ref[axis][row], simp_ref[axis][row], EQUIV_RTOL, EQUIV_ATOL, angular[axis]) {
                continue; // out of shared domain at generation time
            }
            let (a, b) = (map_live[axis][row], simp_live[axis][row]);
            if !axis_match(a, b, EQUIV_RTOL, EQUIV_ATOL, angular[axis]) {
                misses.push(AxisMiss {
                    check: CheckKind::Equivalence, row, axis,
                    reference: b, got: a, rel: rel_diff(a, b),
                });
            }
        }
    }
    misses
}

/// Round-trip accuracy (GRID corpora): inverse(forward(P)) ~= P, BAD-skipping
/// and alias-aware (a different recovered pixel is accepted if it
/// forward-maps to the same sky). `inv_live` is the live output of the
/// inverse section (i.e. inverse(forward(P))).
fn compare_rtrip(
    map: &dyn Mapping,
    fwd: &Section,
    inv_live: &[Vec<f64>],
    rtol: f64,
    atol: f64,
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = fwd.npoint();
    // Forward-transform the recovered pixels to detect aliased preimages.
    let recovered = Section {
        relpath: fwd.relpath.clone(), nin: fwd.nin, nout: fwd.nin, forward: true,
        inputs: inv_live.to_vec(), outputs: Vec::new(),
    };
    let sky2 = transform_section(map, &recovered).ok();

    for row in 0..npoint {
        // Skip points whose forward output was BAD (no inverse).
        if (0..fwd.nout).any(|b| fwd.outputs[b][row] == BAD_VALUE) {
            continue;
        }
        let pixel_ok = (0..fwd.nin)
            .all(|a| within_tol(inv_live[a][row], fwd.inputs[a][row], rtol, atol));
        if pixel_ok {
            continue;
        }
        // Different pixel: accept if it forward-maps to the same sky.
        if let Some(sky2) = &sky2 {
            let alias = (0..fwd.nout).all(|b| {
                within_tol_wrap(sky2[b][row], fwd.outputs[b][row], GOLDEN_RTOL, GOLDEN_ATOL)
            });
            if alias {
                continue;
            }
        }
        for a in 0..fwd.nin {
            let (got, reference) = (inv_live[a][row], fwd.inputs[a][row]);
            if !within_tol(got, reference, rtol, atol) {
                misses.push(AxisMiss {
                    check: CheckKind::RoundTrip, row, axis: a,
                    reference, got, rel: rel_diff(got, reference),
                });
            }
        }
    }
    misses
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn golden_flags_out_of_tolerance_axis() {
        let live = vec![vec![1.0, 2.0]];
        let recorded = vec![vec![1.0, 2.5]];
        let misses = compare_golden(CheckKind::GoldenFwd, &live, &recorded, &[false]);
        assert_eq!(misses.len(), 1);
        assert_eq!((misses[0].row, misses[0].axis), (1, 0));
    }

    #[test]
    fn equiv_skips_points_where_recorded_disagree() {
        // recorded map/simp disagree at row 0 (out of shared domain) but the
        // live values also disagree there — must be skipped. Row 1 agrees in
        // the record but diverges live — must be flagged.
        let map_ref = vec![vec![10.0, 1.0]];
        let simp_ref = vec![vec![20.0, 1.0]];
        let map_live = vec![vec![10.0, 1.0]];
        let simp_live = vec![vec![20.0, 9.0]];
        let misses = compare_equiv(&map_live, &simp_live, &map_ref, &simp_ref, &[false]);
        assert_eq!(misses.len(), 1);
        assert_eq!(misses[0].row, 1);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_transform_oracle check_tests -- --nocapture`
Expected: both PASS.

- [ ] **Step 3: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle four comparison checks"
```

---
## Task 9: The driver — walk sections with C pairing

Port `main`'s section loop: for each section, load the fixture, transform, run the applicable checks, and record an outcome. Reproduce the C one-slot caches (`prev_fwd` for round-trip; `prev_map` + `prev_map_live` for equivalence) that rely on the generator's emission order.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: everything from Tasks 3–8.
- Produces:
  - `enum Outcome { Pass, LoadFail(String), Skipped(String), SkippedByOverride(String), Arity(String), Mismatch(Vec<AxisMiss>) }`
  - `struct SectionReport { relpath: String, forward: bool, outcome: Outcome }`
  - `fn run_oracle(oracle_text: &str, overrides: &Overrides) -> Vec<SectionReport>`

- [ ] **Step 1: Write the driver**

```rust
enum Outcome {
    Pass,
    LoadFail(String),
    Skipped(String),
    SkippedByOverride(String),
    Arity(String),
    Mismatch(Vec<AxisMiss>),
}

struct SectionReport {
    relpath: String,
    forward: bool,
    outcome: Outcome,
}

/// Does relpath end with the given extension?
fn has_ext(relpath: &str, ext: &str) -> bool {
    relpath.rsplit('.').next().map(|e| format!(".{e}")) == Some(ext.to_string())
}
/// Stem (path minus final extension).
fn path_stem(relpath: &str) -> &str {
    match relpath.rfind('.') {
        Some(i) => &relpath[..i],
        None => relpath,
    }
}

/// Run every section of one oracle file. Never asserts; records an outcome
/// per section. Section pairing mirrors the C generator's emission order.
fn run_oracle(oracle_text: &str, overrides: &Overrides) -> Vec<SectionReport> {
    let sections = parse_oracle(oracle_text).expect("oracle parses");
    let mut reports = Vec::with_capacity(sections.len());

    // One-slot caches keyed on the C emission order.
    let mut prev_fwd: Option<Section> = None;                 // for round-trip
    let mut prev_map: Option<(Section, Vec<Vec<f64>>)> = None; // (.map fwd, its live outputs) for equiv

    for s in sections {
        let loaded = match load_fixture(&s.relpath) {
            Ok(Some(l)) => l,
            Ok(None) => {
                reports.push(SectionReport { relpath: s.relpath.clone(), forward: s.forward,
                    outcome: Outcome::Skipped("not a mapping".into()) });
                continue;
            }
            Err(e) => {
                reports.push(SectionReport { relpath: s.relpath.clone(), forward: s.forward,
                    outcome: Outcome::LoadFail(e) });
                continue;
            }
        };
        let map = loaded.mapping().expect("load_fixture guaranteed a mapping");
        let live = match transform_section(map, &s) {
            Ok(v) => v,
            Err(e) => {
                reports.push(SectionReport { relpath: s.relpath.clone(), forward: s.forward,
                    outcome: Outcome::Arity(e) });
                continue;
            }
        };
        let angular = angular_flags(&loaded, &s);
        let mut misses: Vec<AxisMiss> = Vec::new();
        let mut skipped_by_override = false;

        // Golden check (fwd or inv), unless disabled by an override.
        if overrides.golden_off(&s.relpath, s.forward) {
            skipped_by_override = true;
        } else {
            let kind = if s.forward { CheckKind::GoldenFwd } else { CheckKind::GoldenInv };
            misses.extend(compare_golden(kind, &live, &s.outputs, &angular));
        }

        // Round-trip: an inverse section of a GRID fixture paired with the
        // immediately-preceding forward section of the same fixture.
        let grid = has_ext(&s.relpath, ".head") || has_ext(&s.relpath, ".ast");
        if !s.forward && grid {
            if let Some(fwd) = prev_fwd.as_ref().filter(|f| {
                f.relpath == s.relpath && f.nin == s.nout && f.npoint() == s.npoint()
            }) {
                if let Some((rtol, atol)) = overrides.rtrip(&s.relpath) {
                    misses.extend(compare_rtrip(map, fwd, &live, rtol, atol));
                }
            }
        }

        // Equivalence: a .simp forward whose stem matches the cached .map fwd.
        if s.forward && has_ext(&s.relpath, ".simp") {
            if let Some((mmap, mlive)) = prev_map.as_ref().filter(|(m, _)| {
                m.nout == s.nout && m.npoint() == s.npoint()
                    && path_stem(&m.relpath) == path_stem(&s.relpath)
            }) {
                misses.extend(compare_equiv(mlive, &live, &mmap.outputs, &s.outputs, &angular));
            }
        }

        // Update caches (before moving `s` into the report).
        if s.forward {
            prev_fwd = Some(clone_section_meta(&s));
        }
        if s.forward && has_ext(&s.relpath, ".map") {
            prev_map = Some((clone_section_meta(&s), live.clone()));
        }

        let outcome = if skipped_by_override && misses.is_empty() {
            Outcome::SkippedByOverride(format!("golden-{}", if s.forward { "fwd" } else { "inv" }))
        } else if misses.is_empty() {
            Outcome::Pass
        } else {
            Outcome::Mismatch(misses)
        };
        reports.push(SectionReport { relpath: s.relpath, forward: s.forward, outcome });
    }
    reports
}

/// Cheap copy of a section including its data columns (the caches need the
/// recorded inputs/outputs for later pairing).
fn clone_section_meta(s: &Section) -> Section {
    Section {
        relpath: s.relpath.clone(),
        nin: s.nin,
        nout: s.nout,
        forward: s.forward,
        inputs: s.inputs.clone(),
        outputs: s.outputs.clone(),
    }
}
```

Implementation note: `prev_fwd` must retain the forward section's `inputs` and `outputs` (round-trip reads `fwd.inputs`/`fwd.outputs`), so `clone_section_meta` clones the data columns, not just the header. If clones prove costly across 1606 sections it is still fine — this is an on-demand investigative run, not a hot path. Keep it simple.

- [ ] **Step 2: Add a smoke test**

```rust
#[cfg(test)]
mod driver_tests {
    use super::*;

    #[test]
    fn runs_framesets_oracle_without_panicking() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
        let oracle = std::fs::read_to_string(format!("{dir}/framesets.oracle")).unwrap();
        let ov = parse_overrides(
            &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt")).unwrap(),
        );
        let reports = run_oracle(&oracle, &ov);
        assert_eq!(reports.len(), 20, "framesets.oracle has 20 sections");
    }
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p ast-core --test test_transform_oracle driver_tests -- --nocapture`
Expected: PASS (20 sections; some may be `Mismatch`/`LoadFail` — that is fine, the test only checks it runs and counts).

- [ ] **Step 4: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle section driver with C pairing"
```

---
## Task 10: Report writer and the `#[ignore]` investigative test

Aggregate the `SectionReport`s into headline counts, magnitude buckets, and per-check-type groupings; write a hand-rolled JSON report and a stdout summary; wire it into the single `#[ignore]` test.

The spec also calls for grouping by *fixture class mix*. The JSON emits one object per section carrying `relpath`, `status` (which encodes the failing check via `mismatch:<check>`), and `detail` (which carries the magnitude `bucket`). That is a complete per-section record: class-mix grouping is a trivial offline `jq`/Python pivot over `relpath` (the fixture name encodes its class family, e.g. `matrix_*`, `cap_timemap_*`, `*-tan.head`), so the report does not need an in-Rust class parser. Do **not** build a `Begin`-token class walker in this task — keep the class dimension as an offline pivot over the committed JSON.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs`

**Interfaces:**
- Consumes: `run_oracle`, `SectionReport`, `Outcome`, `AxisMiss`, `CheckKind`.
- Produces:
  - `fn magnitude_bucket(rel: f64, bad_mismatch: bool) -> &'static str` — one of `"bad-mismatch"`, `"<=1e-6"`, `"1e-6..1e-3"`, `"1e-3..1"`, `">1"`.
  - `fn write_json_report(path: &str, reports: &[SectionReport]) -> std::io::Result<()>`
  - the `#[ignore]` test `transform_oracle_report`.

- [ ] **Step 1: Implement the aggregation, JSON writer, and test**

```rust
use std::fmt::Write as _;

fn check_name(c: CheckKind) -> &'static str {
    match c {
        CheckKind::GoldenFwd => "golden-fwd",
        CheckKind::GoldenInv => "golden-inv",
        CheckKind::Equivalence => "equivalence",
        CheckKind::RoundTrip => "round-trip",
    }
}

/// Coarse magnitude bucket for a mismatch's worst relative divergence.
fn magnitude_bucket(rel: f64, bad_mismatch: bool) -> &'static str {
    if bad_mismatch {
        "bad-mismatch"
    } else if rel <= 1e-6 {
        "<=1e-6"
    } else if rel <= 1e-3 {
        "1e-6..1e-3"
    } else if rel <= 1.0 {
        "1e-3..1"
    } else {
        ">1"
    }
}

/// Minimal JSON string escaping (quotes, backslashes, control chars).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out
}

/// A mismatch's worst (highest-rel) axis, and whether it is a BAD mismatch.
fn worst_miss(misses: &[AxisMiss]) -> Option<(&AxisMiss, bool)> {
    misses
        .iter()
        .map(|m| {
            let bad = m.got == BAD_VALUE || m.reference == BAD_VALUE;
            (m, bad)
        })
        .max_by(|a, b| a.0.rel.partial_cmp(&b.0.rel).unwrap_or(std::cmp::Ordering::Equal))
}

/// Write the machine-readable divergence report as JSON (no serde: this crate
/// has no serde dependency, and the schema is tiny and fixed).
fn write_json_report(path: &str, reports: &[SectionReport]) -> std::io::Result<()> {
    let mut pass = 0;
    let mut load_fail = 0;
    let mut skipped = 0;
    let mut skipped_override = 0;
    let mut arity = 0;
    let mut failed = 0;

    let mut sections_json = String::new();
    for r in reports {
        let (status, detail) = match &r.outcome {
            Outcome::Pass => { pass += 1; ("pass".to_string(), String::new()) }
            Outcome::LoadFail(e) => { load_fail += 1; ("load-fail".to_string(), e.clone()) }
            Outcome::Skipped(e) => { skipped += 1; ("skipped".to_string(), e.clone()) }
            Outcome::SkippedByOverride(e) => { skipped_override += 1; ("skipped-override".to_string(), e.clone()) }
            Outcome::Arity(e) => { arity += 1; ("arity".to_string(), e.clone()) }
            Outcome::Mismatch(m) => {
                failed += 1;
                let (worst, bad) = worst_miss(m).unwrap();
                (
                    format!("mismatch:{}", check_name(worst.check)),
                    format!(
                        "bucket={} worst_rel={:.3e} nmiss={} ref={:.17e} got={:.17e}",
                        magnitude_bucket(worst.rel, bad), worst.rel, m.len(),
                        worst.reference, worst.got
                    ),
                )
            }
        };
        let _ = write!(
            sections_json,
            "    {{\"relpath\":\"{}\",\"forward\":{},\"status\":\"{}\",\"detail\":\"{}\"}},\n",
            json_escape(&r.relpath), r.forward, status, json_escape(&detail)
        );
    }
    // Trim trailing comma+newline for valid JSON.
    let sections_json = sections_json.trim_end().trim_end_matches(',');

    let json = format!(
        "{{\n  \"totals\": {{\"sections\":{},\"pass\":{},\"failed\":{},\"skipped\":{},\"skipped_override\":{},\"load_fail\":{},\"arity\":{}}},\n  \"sections\": [\n{}\n  ]\n}}\n",
        reports.len(), pass, failed, skipped, skipped_override, load_fail, arity, sections_json
    );
    std::fs::write(path, json)?;

    // stdout human summary: headline + worst divergences.
    println!(
        "transform-oracle: {} sections | pass={} fail={} skip={} skip-override={} load-fail={} arity={}",
        reports.len(), pass, failed, skipped, skipped_override, load_fail, arity
    );
    let mut worst: Vec<(&str, bool, f64, &AxisMiss)> = reports
        .iter()
        .filter_map(|r| match &r.outcome {
            Outcome::Mismatch(m) => worst_miss(m).map(|(w, _bad)| (r.relpath.as_str(), r.forward, w.rel, w)),
            _ => None,
        })
        .collect();
    worst.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    println!("worst {} divergences:", worst.len().min(40));
    for (relpath, forward, rel, m) in worst.iter().take(40) {
        println!(
            "  {:<52} {:<7} {:<11} rel={:.3e} row={} axis={} ref={:.6e} got={:.6e}",
            relpath, if *forward { "fwd" } else { "inv" },
            check_name(m.check), rel, m.row, m.axis, m.reference, m.got
        );
    }
    Ok(())
}

/// Investigative harness. Not part of the normal suite or CI: run with
///   cargo test -p ast-core --test test_transform_oracle -- --ignored --nocapture
/// It runs every oracle section, prints a summary, and writes a JSON report
/// to `target/transform_oracle_report.json`. It never asserts on divergence
/// (the whole point is to measure it); it panics only on a broken harness.
#[test]
#[ignore = "investigative: measures Rust-vs-C transform divergence; run with --ignored"]
fn transform_oracle_report() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let mut reports = Vec::new();
    for name in ["simplify_fixtures.oracle", "headers.oracle", "framesets.oracle"] {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).expect("oracle present");
        reports.extend(run_oracle(&text, &ov));
    }
    assert_eq!(reports.len(), 1606, "harness must cover every section");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/transform_oracle_report.json");
    write_json_report(out, &reports).expect("report writes");
    println!("wrote {out}");
}
```

- [ ] **Step 2: Verify the ignored test is excluded from the normal run**

Run: `cargo test -p ast-core --test test_transform_oracle`
Expected: the unit-test modules pass; `transform_oracle_report` shows as `ignored`.

- [ ] **Step 3: Run the investigative test and inspect the report**

Run: `cargo test -p ast-core --test test_transform_oracle -- --ignored --nocapture`
Expected: prints the headline counts and the worst-40 table, prints `wrote .../target/transform_oracle_report.json`, and PASSES (it asserts only the 1606-section coverage and that the report writes). Confirm the JSON parses:
```bash
python3 -c "import json,sys; d=json.load(open('target/transform_oracle_report.json')); print(d['totals'])"
```

- [ ] **Step 4: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo fmt -p ast-core
cargo clippy -p ast-core --tests -- -D warnings
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test: transform-oracle report writer and investigative test"
```

---

## Task 11: Record the harness in PLAN.md

**Files:**
- Modify: `PLAN.md`

**Interfaces:**
- Consumes: nothing.
- Produces: a tracker entry so the migration status reflects the new harness.

- [ ] **Step 1: Add the tracker entry**

Under the "C → Rust test migration tracker" section of `PLAN.md`, add a row/line recording that `check_transform_oracle` (submodule `3a3c3e51`) is ported as the `#[ignore]` investigative harness `crates/ast-core/tests/test_transform_oracle.rs`, consuming the vendored `tests/fixtures/oracle/*.oracle`. Note that it is investigative (not a CI gate) pending divergence fixes, and that the generator (`gen_transform_oracle`) is intentionally not ported. Match the surrounding format of that section (read it first and follow the existing convention).

- [ ] **Step 2: Commit**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git add PLAN.md
git commit -m "docs: track transform-oracle checker in migration tracker"
```

---

## Follow-up (out of scope for this plan)

Once the divergence set is understood from the report, a later change turns the harness into a CI gate: drop `#[ignore]`, add a committed exclusion list of known-divergent sections (each with a reason, mirroring `NATIVE_READ_EXCLUDED`), and assert that every non-excluded section passes. Shrinking that list is the porting work the report will scope.
