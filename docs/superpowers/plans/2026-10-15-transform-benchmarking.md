# Transform Benchmarking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build call-count parity checking (Phase 0) and a Rust-first transform benchmark harness with an optional C engine (Phase 1), then run the first full sweep (Phase 2).

**Architecture:** `ast-core` gains a zero-cost `call-counts` feature (a `math` module plus counted FFI boundaries).
A new `ast-bench` crate (renamed from `ast-oracle-bench`) reads the committed transform oracles, selects points, and times Rust always and C optionally, loading C at runtime from a shared library built by `tools/perf/build_c_engine.sh`.
`ast-oracle-tests` hosts the Phase 0 parity test, which uses the same loader against a counting build of C.

**Tech Stack:** Rust 2024 (workspace MSRV 1.85), `libloading` (optional), C99 shim compiled against the `reference/starlink-ast` submodule via cmake, Python 3 for the count-header generator.

**Spec:** `docs/superpowers/specs/2026-10-15-transform-benchmarking-design.md`

## Global Constraints

- Point counts: N ∈ {1, 10, 100, 100 000, 1 000 000}.
- Objects are timed as loaded; never simplify before timing.
- No explicit SIMD on either side; `ast-core`'s `simd-eval` feature must not be enabled in any timed build.
- C is built with `-O3 -ffp-contract=off`; Rust with the release profile.
- Small N (1, 10, 100) uses only oracle rows whose inputs and outputs are all non-BAD; a case with no such row is a hard error.
- Large N (100 000, 1 000 000) uses all oracle rows, BAD included, repeated in order.
- `ast-bench` must build and test with no submodule and no C (the `c-oracle` feature off).
- No skip modes: anything not compared is in a named list with a per-case reason and a companion test that fails when the entry starts passing.
- Every check lands with a negative control that is shown to fire.
- Prose: one sentence per line, American spelling. Comments describe the current code only.
- Use the clean-environment wrapper for cargo and git commit: conda's `CC`/`LDFLAGS` break links (see memory `conda-toolchain-breaks-cargo-links`).
- Never stage `reference/starlink-ast` in the parent repo; never modify the submodule in this plan.
- Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --tests --examples -- -D warnings`, and for `ast-core` changes also with `--features call-counts`.

## Review Focus

1. **A timing run built with `call-counts` enabled** (feature unification via `ast-oracle-tests`) would silently time counting code; `ast-bench run` must refuse when `ast_core::CALL_COUNTS_ENABLED` is true (test in Task 10).
2. **A missing or stale C engine library** must fail with the exact build command to run, never fall back to Rust-only (test in Task 7).
3. **A case that errors** (a missing fixture, or a transform that fails only at large N once BAD rows reach an error path) must be reported as an error naming the case and N, while the rest of the set still runs (test in Task 10).
4. **`compare` against a baseline from another machine** must refuse, naming the differing fingerprint field (test in Task 11).
5. **An oracle section with fewer rows than N** must repeat rows in order, so N = 10 from 24 rows uses rows 0-9 and N = 100 cycles (test in Task 3).

---

### Task 1: Rename `ast-oracle-bench` to `ast-bench` with an empty CLI

**Files:**
- Delete: `crates/ast-oracle-bench/` (Cargo.toml, `benches/oracle.rs`, `src/lib.rs`)
- Create: `crates/ast-bench/Cargo.toml`, `crates/ast-bench/src/lib.rs`, `crates/ast-bench/src/main.rs`, `crates/ast-bench/README.md`
- Modify: `CLAUDE.md` (repository layout block; "Build & test" tier-5 line; "Hot-path constraint"; "Bulk-transform optimisation"), `PLAN.md` (rows mentioning `ast-oracle-bench`), `docs/superpowers/specs/2026-04-20-rust-port-design.md` (crate name, plus a revision-history entry)

**Interfaces:**
- Produces: crate `ast_bench` (lib) and binary `ast-bench`; feature `c-oracle` (enables optional dep `libloading`).

- [ ] **Step 1: Find every reference to the old name**

Run: `rg -n 'ast-oracle-bench|ast_oracle_bench' --glob '!target' .`
Record the list; each one is updated in Step 4.

- [ ] **Step 2: Create the crate**

`crates/ast-bench/Cargo.toml`:

```toml
[package]
name = "ast-bench"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Transform benchmarks: Rust ast-core, optionally against the reference C libast"

[lints]
workspace = true

[features]
# Load the reference C library at runtime as a second engine.
c-oracle = ["dep:libloading"]

[dependencies]
ast-core = { path = "../ast-core" }
libloading = { version = "0.8", optional = true }
```

`crates/ast-bench/src/lib.rs`:

```rust
//! Transform benchmarking for the AST port.
//!
//! The Rust engine is always present. The reference C library is an optional
//! second engine behind the `c-oracle` feature, used while Rust is being
//! brought to parity with C; regression checking needs only Rust.
```

`crates/ast-bench/src/main.rs`:

```rust
//! `ast-bench` command-line entry point.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") => print!("{}", USAGE),
        Some(other) => {
            eprintln!("unknown command {other:?}\n{USAGE}");
            std::process::exit(2);
        }
    }
}

const USAGE: &str = "usage: ast-bench <command> [options]\n\
commands:\n\
  run --set <name> [--engine rust|c|both] [--save] [--format table|tsv]\n\
  compare --set <name> [--base <git-ref>]\n\
  sweep\n\
  snapshot --set <name>\n";
```

`crates/ast-bench/README.md`: a title, the purpose paragraph from the spec, and a "Fairness rules" section copying the spec's four bullets verbatim plus the known bias of repeated oracle rows.

- [ ] **Step 3: Delete the old crate and build**

Run: `git rm -r crates/ast-oracle-bench && cargo build -p ast-bench && cargo run -q -p ast-bench -- help`
Expected: build succeeds; usage text printed.

- [ ] **Step 4: Update references**

For each hit from Step 1, replace the crate name.
In CLAUDE.md "Hot-path constraint", replace the paragraph beginning "**There is currently no automated performance gate.**" with:

```markdown
**The performance gate is `ast-bench`.**
`cargo run --release -p ast-bench -- compare --set bench --base main` times the current tree against `main` on the same machine and fails on a slowdown; `crates/ast-bench/README.md` has the details.
CI does not run it yet.
```

In the port design spec, rename the crate in the repository layout and the tier-5 description, and add a revision-history entry dated 2026-10-15: "Tier-5 crate renamed `ast-oracle-bench` → `ast-bench`; C is an optional engine (see `2026-10-15-transform-benchmarking-design.md`)."

- [ ] **Step 5: Verify and commit**

Run: `cargo clippy --workspace --tests --examples -- -D warnings && cargo fmt --all -- --check && rg -n 'ast-oracle-bench' --glob '!target' --glob '!docs/superpowers/plans/*' --glob '!docs/superpowers/specs/2026-10-15-*' .`
Expected: clippy and fmt clean; the `rg` prints nothing (the plan and the benchmarking spec name the old crate on purpose).

```bash
git add -A crates/ast-bench crates/ast-oracle-bench CLAUDE.md PLAN.md docs/superpowers/specs/2026-04-20-rust-port-design.md
git commit -m "Rename ast-oracle-bench to ast-bench"
```

---

### Task 2: Oracle reader and fixture loader in `ast-bench`

**Files:**
- Create: `crates/ast-bench/src/oracle.rs`, `crates/ast-bench/src/load.rs`
- Modify: `crates/ast-bench/src/lib.rs` (add `pub mod oracle; pub mod load;`)

**Interfaces:**
- Produces:
  - `pub struct Section { pub relpath: String, pub nin: usize, pub nout: usize, pub forward: bool, pub inputs: Vec<Vec<f64>>, pub outputs: Vec<Vec<f64>> }` (column-major: `inputs[axis][row]`), with `pub fn npoint(&self) -> usize`.
  - `pub fn parse_oracle(text: &str) -> Result<Vec<Section>, String>`
  - `pub const CORE_FIXTURES: &str` — absolute path of `crates/ast-core/tests/fixtures`.
  - `pub const CORE_ORACLES: [&str; 3] = ["simplify_fixtures.oracle", "headers.oracle", "framesets.oracle"]` (under `CORE_FIXTURES/oracle/`).
  - `pub enum Loaded { Mapping(ast_core::AnyObject), Untransformable(&'static str) }`
  - `pub fn load_fixture(root: &Path, relpath: &str) -> Result<Loaded, String>`
  - `pub fn intra_registry() -> ast_core::mappings::IntraRegistry`

The parser and loader are copies of `crates/ast-core/tests/test_transform_oracle.rs:62-266` and the IntraMap helper in `crates/ast-core/tests/common/mod.rs:186-296`, because integration-test code cannot be imported.
Each copied block carries a comment naming the file it mirrors.

- [ ] **Step 1: Write the failing tests** (in `oracle.rs` and `load.rs` `#[cfg(test)]` modules)

```rust
// oracle.rs
#[test]
fn parses_all_core_oracles_with_known_section_count() {
    let mut total = 0;
    for name in CORE_ORACLES {
        let text = std::fs::read_to_string(format!("{CORE_FIXTURES}/oracle/{name}")).unwrap();
        total += parse_oracle(&text).unwrap().len();
    }
    // Same total `every_oracle_section_is_gated` asserts in ast-core.
    assert_eq!(total, 2261);
}

#[test]
fn malformed_row_is_hard_error() {
    let text = "[a.map  nin=2 nout=2 dir=forward]\n1 2 3\n";
    assert!(parse_oracle(text).is_err());
}

// load.rs
#[test]
fn every_core_section_loads_or_names_why_not() {
    let root = std::path::Path::new(crate::oracle::CORE_FIXTURES);
    for name in crate::oracle::CORE_ORACLES {
        let text = std::fs::read_to_string(root.join("oracle").join(name)).unwrap();
        for s in crate::oracle::parse_oracle(&text).unwrap() {
            load_fixture(root, &s.relpath).unwrap_or_else(|e| panic!("{}: {e}", s.relpath));
        }
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ast-bench`
Expected: compile errors (`parse_oracle`, `load_fixture` not defined).

- [ ] **Step 3: Implement**

`oracle.rs`: copy `Section`, `parse_double`, `parse_oracle` verbatim from `test_transform_oracle.rs:62-150`, making the items `pub`, and add:

```rust
pub const CORE_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ast-core/tests/fixtures");
pub const CORE_ORACLES: [&str; 3] = ["simplify_fixtures.oracle", "headers.oracle", "framesets.oracle"];
```

`load.rs`:

```rust
//! Load an oracle fixture as the transform-oracle gate does
//! (`crates/ast-core/tests/test_transform_oracle.rs`, `load_fixture`).

use std::path::Path;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

pub enum Loaded {
    Mapping(AnyObject),
    /// Resolved and read, but there is no transform to time.
    Untransformable(&'static str),
}

pub fn load_fixture(root: &Path, relpath: &str) -> Result<Loaded, String> {
    let path = root.join(relpath);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {relpath}: {e}"))?;
    let obj = if relpath.ends_with(".head") {
        let mut fc = FitsChan::from_header(&text).map_err(|e| format!("from_header {relpath}: {e}"))?;
        fc.read()
            .map_err(|e| format!("read {relpath}: {e}"))?
            .ok_or_else(|| format!("FitsChan read returned None: {relpath}"))?
    } else {
        ast_core::from_native_string_with(&text, &intra_registry())
            .map_err(|e| format!("from_native_string {relpath}: {e}"))?
    };
    if obj.as_mapping().is_some() {
        return Ok(Loaded::Mapping(obj));
    }
    Ok(Loaded::Untransformable(if obj.as_frame().is_some() {
        "bare Frame: not a Mapping in the Rust port; C treats it as a unit transform"
    } else {
        "not a Mapping in C either"
    }))
}
```

Copy `TestIntraTran` and `test_intra_transforms` from `tests/common/mod.rs:186-288` into `load.rs` as private items, and add:

```rust
pub fn intra_registry() -> ast_core::mappings::IntraRegistry {
    let mut registry = ast_core::mappings::IntraRegistry::new();
    for t in test_intra_transforms() {
        registry.register(t).expect("the four test IntraMap names are distinct");
    }
    registry
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-bench`
Expected: 3 passed.

- [ ] **Step 5: Negative control and commit**

Change `2261` to `2260`; run; expected FAIL. Restore.

```bash
git add crates/ast-bench
git commit -m "ast-bench: read the transform oracles and load their fixtures"
```

---

### Task 3: Point selection

**Files:**
- Create: `crates/ast-bench/src/points.rs`
- Modify: `crates/ast-bench/src/lib.rs` (`pub mod points;`)

**Interfaces:**
- Consumes: `oracle::Section`.
- Produces:
  - `pub const SMALL_N_MAX: usize = 100;`
  - `pub const NPOINTS: [usize; 5] = [1, 10, 100, 100_000, 1_000_000];`
  - `pub fn select_rows(s: &Section, n: usize) -> Result<Vec<usize>, String>` — row indices, length `n`.
  - `pub fn rows_to_pointset(s: &Section, rows: &[usize]) -> ast_core::coords::PointSet` (row-major, `nin` coords).
  - `pub fn rows_to_columns(s: &Section, rows: &[usize]) -> Vec<f64>` (C layout: `col[axis * n + i]`).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::Section;
    use ast_core::constants::BAD_VALUE as B;

    fn sec(inputs: Vec<Vec<f64>>, outputs: Vec<Vec<f64>>) -> Section {
        Section { relpath: "t.map".into(), nin: inputs.len(), nout: outputs.len(), forward: true, inputs, outputs }
    }

    #[test]
    fn small_n_uses_only_rows_good_on_both_sides() {
        // Row 0 bad input, row 1 bad output, row 2 good.
        let s = sec(vec![vec![B, 1.0, 2.0]], vec![vec![0.0, B, 4.0]]);
        assert_eq!(select_rows(&s, 1).unwrap(), vec![2]);
        assert_eq!(select_rows(&s, 3).unwrap(), vec![2, 2, 2]);
    }

    #[test]
    fn small_n_with_no_good_row_is_an_error() {
        let s = sec(vec![vec![B, 1.0]], vec![vec![0.0, B]]);
        let e = select_rows(&s, 10).unwrap_err();
        assert!(e.contains("t.map") && e.contains("non-BAD"), "{e}");
    }

    #[test]
    fn large_n_uses_every_row_in_order_bad_included() {
        let s = sec(vec![vec![B, 1.0, 2.0]], vec![vec![0.0, B, 4.0]]);
        assert_eq!(select_rows(&s, 100_000).unwrap()[..7], [0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn fewer_rows_than_n_repeat_in_order() {
        let s = sec(vec![(0..24).map(f64::from).collect()], vec![(0..24).map(f64::from).collect()]);
        assert_eq!(select_rows(&s, 10).unwrap(), (0..10).collect::<Vec<_>>());
        let r = select_rows(&s, 100).unwrap();
        assert_eq!((r[23], r[24], r[99]), (23, 0, 3));
    }

    #[test]
    fn column_layout_matches_c() {
        let s = sec(vec![vec![1.0, 2.0], vec![10.0, 20.0]], vec![vec![0.0, 0.0]]);
        assert_eq!(rows_to_columns(&s, &[1, 0]), vec![2.0, 1.0, 20.0, 10.0]);
        let ps = rows_to_pointset(&s, &[1, 0]);
        assert_eq!(ps.point(0), &[2.0, 20.0]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-bench points::`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Choose oracle rows for a timing run of N points.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;

use crate::oracle::Section;

pub const SMALL_N_MAX: usize = 100;
pub const NPOINTS: [usize; 5] = [1, 10, 100, 100_000, 1_000_000];

/// Row indices for an N-point run, repeating the candidate rows in order.
///
/// Up to `SMALL_N_MAX` points, only rows that are non-BAD in every input and
/// output are candidates: a short run made only of BAD points would time the
/// early exit, not the transform. Above it, every row is a candidate.
pub fn select_rows(s: &Section, n: usize) -> Result<Vec<usize>, String> {
    let all = 0..s.npoint();
    let candidates: Vec<usize> = if n <= SMALL_N_MAX {
        all.filter(|&r| {
            s.inputs.iter().chain(&s.outputs).all(|col| col[r] != BAD_VALUE)
        })
        .collect()
    } else {
        all.collect()
    };
    if candidates.is_empty() {
        return Err(format!(
            "{} dir={}: no non-BAD row to time at N={n}",
            s.relpath,
            if s.forward { "forward" } else { "inverse" }
        ));
    }
    Ok((0..n).map(|i| candidates[i % candidates.len()]).collect())
}

pub fn rows_to_pointset(s: &Section, rows: &[usize]) -> PointSet {
    PointSet::from_rows(rows.iter().map(|&r| s.inputs.iter().map(|c| c[r]).collect::<Vec<f64>>()))
}

pub fn rows_to_columns(s: &Section, rows: &[usize]) -> Vec<f64> {
    s.inputs.iter().flat_map(|c| rows.iter().map(move |&r| c[r])).collect()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-bench points::`
Expected: 5 passed.

- [ ] **Step 5: Negative control and commit**

Change `n <= SMALL_N_MAX` to `n < 1`; run; expected `small_n_uses_only_rows_good_on_both_sides` FAILS. Restore.

```bash
git add crates/ast-bench
git commit -m "ast-bench: select oracle rows for N-point runs"
```

---

### Task 4: `ast-core` `math` module, `call_counts` module, and the clippy ban

**Files:**
- Create: `crates/ast-core/src/math.rs`, `crates/ast-core/src/call_counts.rs`, `crates/ast-core/clippy.toml`
- Modify: `crates/ast-core/Cargo.toml` (feature), `crates/ast-core/src/lib.rs`, every file using a banned `f64` method (about 21 files; list with the `rg` in Step 4)

**Interfaces:**
- Produces:
  - Feature `call-counts` on `ast-core`.
  - `pub const CALL_COUNTS_ENABLED: bool` at the crate root (`cfg!(feature = "call-counts")`).
  - `pub(crate) fn call_counts::hit(name: &'static str)` (no-op without the feature).
  - With the feature only: `pub fn ast_core::call_counts::reset()` and `pub fn ast_core::call_counts::snapshot() -> std::collections::BTreeMap<String, u64>`.
  - `pub(crate) fn math::{sin, cos, tan, asin, acos, atan, atan2, sin_cos, exp, ln, log10, powf, powi, sqrt, hypot, sinh, cosh, tanh}` with the same signatures as the `f64` methods (receiver becomes the first argument).
  - Count keys are the C names: `sin cos tan asin acos atan atan2 exp log log10 pow sqrt hypot sinh cosh tanh`; `sin_cos` counts one `sin` and one `cos`; `powi` counts as `powi` (C has no such function, so a difference there is a real source difference).

- [ ] **Step 1: Write the failing test** (`crates/ast-core/src/math.rs` test module, run under the feature)

```rust
#[cfg(all(test, feature = "call-counts"))]
mod tests {
    #[test]
    fn counts_by_c_name() {
        crate::call_counts::reset();
        let _ = super::sin(1.0) + super::ln(2.0) + super::powf(2.0, 0.5);
        let _ = super::sin_cos(0.5);
        let c = crate::call_counts::snapshot();
        assert_eq!(c.get("sin"), Some(&2));
        assert_eq!(c.get("cos"), Some(&1));
        assert_eq!(c.get("log"), Some(&1));
        assert_eq!(c.get("pow"), Some(&1));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-core --features call-counts --lib math::`
Expected: compile error (feature and modules missing).

- [ ] **Step 3: Implement the modules**

`Cargo.toml` `[features]`:

```toml
# Count calls into libm, PAL, ERFA and wcslib, keyed by C function name, for
# checking that the port makes the same calls as C (`ast_core::call_counts`).
# Never enable in a timed build.
call-counts = []
```

`lib.rs`: `pub mod call_counts; pub(crate) mod math;` and

```rust
/// Whether this build counts library calls (the `call-counts` feature).
/// Timing a counting build measures the counters, so benchmarks refuse it.
pub const CALL_COUNTS_ENABLED: bool = cfg!(feature = "call-counts");
```

`call_counts.rs`:

```rust
//! Per-thread counts of calls into libm, PAL, ERFA and wcslib, keyed by the C
//! function name, so the port's calls can be compared with C's.
//!
//! Without the `call-counts` feature `hit` is an empty inline function.

#[cfg(feature = "call-counts")]
mod imp {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static COUNTS: RefCell<BTreeMap<&'static str, u64>> = const { RefCell::new(BTreeMap::new()) };
    }

    pub(crate) fn hit(name: &'static str) {
        COUNTS.with(|c| *c.borrow_mut().entry(name).or_default() += 1);
    }

    /// Zero this thread's counts.
    pub fn reset() {
        COUNTS.with(|c| c.borrow_mut().clear());
    }

    /// This thread's counts since the last `reset`.
    pub fn snapshot() -> BTreeMap<String, u64> {
        COUNTS.with(|c| c.borrow().iter().map(|(k, v)| ((*k).to_string(), *v)).collect())
    }
}

#[cfg(feature = "call-counts")]
pub use imp::{reset, snapshot};
#[cfg(feature = "call-counts")]
pub(crate) use imp::hit;

#[cfg(not(feature = "call-counts"))]
#[inline(always)]
pub(crate) fn hit(_name: &'static str) {}
```

`math.rs` (one function per method; all follow this shape):

```rust
//! The floating-point library functions `ast-core` calls, routed through one
//! place so `call-counts` can count them by their C names. The bare `f64`
//! methods are banned in this crate by `clippy.toml`.
#![allow(clippy::disallowed_methods)]

use crate::call_counts::hit;

#[inline(always)]
pub(crate) fn sin(x: f64) -> f64 { hit("sin"); x.sin() }
#[inline(always)]
pub(crate) fn cos(x: f64) -> f64 { hit("cos"); x.cos() }
#[inline(always)]
pub(crate) fn tan(x: f64) -> f64 { hit("tan"); x.tan() }
#[inline(always)]
pub(crate) fn asin(x: f64) -> f64 { hit("asin"); x.asin() }
#[inline(always)]
pub(crate) fn acos(x: f64) -> f64 { hit("acos"); x.acos() }
#[inline(always)]
pub(crate) fn atan(x: f64) -> f64 { hit("atan"); x.atan() }
#[inline(always)]
pub(crate) fn atan2(y: f64, x: f64) -> f64 { hit("atan2"); y.atan2(x) }
#[inline(always)]
pub(crate) fn sin_cos(x: f64) -> (f64, f64) { hit("sin"); hit("cos"); x.sin_cos() }
#[inline(always)]
pub(crate) fn exp(x: f64) -> f64 { hit("exp"); x.exp() }
#[inline(always)]
pub(crate) fn ln(x: f64) -> f64 { hit("log"); x.ln() }
#[inline(always)]
pub(crate) fn log10(x: f64) -> f64 { hit("log10"); x.log10() }
#[inline(always)]
pub(crate) fn powf(x: f64, y: f64) -> f64 { hit("pow"); x.powf(y) }
#[inline(always)]
pub(crate) fn powi(x: f64, n: i32) -> f64 { hit("powi"); x.powi(n) }
#[inline(always)]
pub(crate) fn sqrt(x: f64) -> f64 { hit("sqrt"); x.sqrt() }
#[inline(always)]
pub(crate) fn hypot(x: f64, y: f64) -> f64 { hit("hypot"); x.hypot(y) }
#[inline(always)]
pub(crate) fn sinh(x: f64) -> f64 { hit("sinh"); x.sinh() }
#[inline(always)]
pub(crate) fn cosh(x: f64) -> f64 { hit("cosh"); x.cosh() }
#[inline(always)]
pub(crate) fn tanh(x: f64) -> f64 { hit("tanh"); x.tanh() }
```

(rustfmt will expand these onto multiple lines; that is fine.)

`crates/ast-core/clippy.toml`:

```toml
# Library math goes through `crate::math` so `call-counts` sees every call.
disallowed-methods = [
  { path = "f64::sin", reason = "use crate::math::sin" },
  { path = "f64::cos", reason = "use crate::math::cos" },
  { path = "f64::tan", reason = "use crate::math::tan" },
  { path = "f64::asin", reason = "use crate::math::asin" },
  { path = "f64::acos", reason = "use crate::math::acos" },
  { path = "f64::atan", reason = "use crate::math::atan" },
  { path = "f64::atan2", reason = "use crate::math::atan2" },
  { path = "f64::sin_cos", reason = "use crate::math::sin_cos" },
  { path = "f64::exp", reason = "use crate::math::exp" },
  { path = "f64::ln", reason = "use crate::math::ln" },
  { path = "f64::log10", reason = "use crate::math::log10" },
  { path = "f64::powf", reason = "use crate::math::powf" },
  { path = "f64::powi", reason = "use crate::math::powi" },
  { path = "f64::sqrt", reason = "use crate::math::sqrt" },
  { path = "f64::hypot", reason = "use crate::math::hypot" },
  { path = "f64::sinh", reason = "use crate::math::sinh" },
  { path = "f64::cosh", reason = "use crate::math::cosh" },
  { path = "f64::tanh", reason = "use crate::math::tanh" },
]
```

Integration tests and examples under `crates/ast-core/` are not library code and may keep the bare methods: add `#![allow(clippy::disallowed_methods)]` to each test area's `main.rs` and to each example that trips it.

- [ ] **Step 4: Convert the library call sites**

Run: `cargo clippy -p ast-core --lib -- -D warnings 2>&1 | rg -c disallowed_methods`
Convert each reported site, `x.sin()` → `math::sin(x)`, `y.atan2(x)` → `math::atan2(y, x)`, `x.powi(2)` → `math::powi(x, 2)`, adding `use crate::math;` per file.
Repeat until clippy is clean.
Do not reorder operands or change expressions otherwise: the arithmetic must be bit-identical.

- [ ] **Step 5: Verify**

Run: `cargo test -p ast-core --features call-counts --lib math:: && cargo clippy --workspace --tests --examples -- -D warnings && cargo clippy -p ast-core --features call-counts --tests --examples -- -D warnings`
Then the whole suite, which must be unchanged: for each area in `domain_frames fitschan frames mappings meta regions serial`, `cargo test -q -p ast-core --test <area>`, plus `cargo test -q -p ast-core --lib`.
Expected: all pass.

- [ ] **Step 6: Negative controls and commit**

1. Add `let _ = 1.0f64.sin();` in any `ast-core/src` function; clippy must FAIL with `disallowed_methods`. Remove.
2. In `math::sin`, delete `hit("sin");`; `counts_by_c_name` must FAIL. Restore.

```bash
git add crates/ast-core
git commit -m "ast-core: route library math through a counted math module"
```

---

### Task 5: Count the PAL, ERFA and wcslib boundaries

**Files:**
- Modify: `crates/ast-core/src/pal.rs` (54 `unsafe { astPal…/astEra… }` call sites), `crates/ast-core/src/wcslib.rs` (the `astPRJset`/`astTPNset` calls and the four `fwd`/`rev` function-pointer calls at about lines 715, 753, 788, 807)
- Test: `crates/ast-core/src/pal.rs`, `crates/ast-core/src/wcslib.rs` test modules

**Interfaces:**
- Consumes: `call_counts::hit`.
- Produces: count keys equal to the C symbol names (`astPalEqgal`, `astEraGc2gd`, `astPRJset`, `astTPNset`), plus `wcs_fwd` and `wcs_rev` for projection evaluations (C reaches them only through function pointers, `wcsmap.c:2855,2883`).

- [ ] **Step 1: Write the failing tests**

In `pal.rs`:

```rust
#[cfg(test)]
mod count_tests {
    /// Every FFI call in this file goes through `counted!`, so `call-counts`
    /// sees every call into PAL and ERFA.
    #[test]
    fn every_ffi_call_is_counted() {
        let src = include_str!("pal.rs");
        let body = &src[src.find("\n}\n").expect("end of extern block")..];
        let bare = body.matches("unsafe { astP").count() + body.matches("unsafe { astE").count();
        assert_eq!(bare, 0, "uncounted PAL/ERFA call in pal.rs");
    }

    #[cfg(feature = "call-counts")]
    #[test]
    fn eqgal_counts_by_symbol() {
        crate::call_counts::reset();
        let _ = super::eqgal(1.0, 0.5);
        assert_eq!(crate::call_counts::snapshot().get("astPalEqgal"), Some(&1));
    }
}
```

The analogous `wcslib.rs` test asserts that `forward_raw` on a TAN projection counts one `wcs_fwd`, and that the file contains no `unsafe { fwd(` or `unsafe { rev(` outside `counted!`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-core --features call-counts --lib count_tests`
Expected: FAIL (54 bare calls).

- [ ] **Step 3: Implement**

At the top of `pal.rs` (after the `extern` block):

```rust
/// Call an FFI function, counting it under its symbol name.
macro_rules! counted {
    ($f:ident ( $($arg:expr),* $(,)? )) => {{
        crate::call_counts::hit(stringify!($f));
        // SAFETY: as for the bare call this wraps; see the calling function.
        unsafe { $f($($arg),*) }
    }};
}
```

Rewrite each `unsafe { astPalX(args) }` as `counted!(astPalX(args))`, keeping any `SAFETY` comment above it.
In `wcslib.rs`, wrap `astPRJset`/`astTPNset` the same way, and the pointer calls as:

```rust
crate::call_counts::hit("wcs_fwd");
// SAFETY: the function pointer was set by astPRJset and operates
// on `self.inner`, which outlives the call.
let status = unsafe { fwd(phi, theta, &mut self.inner, &mut x, &mut y) };
```

(and `wcs_rev` for `rev`), then update the source-scan test to look for `hit("wcs_fwd")` immediately preceding each `unsafe { fwd(` line.

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-core --features call-counts --lib count_tests && cargo test -q -p ast-core --test mappings && cargo clippy -p ast-core --features call-counts --tests -- -D warnings`
Expected: pass.

- [ ] **Step 5: Negative control and commit**

Revert one `counted!` back to `unsafe { astPalEpj(date) }`; `every_ffi_call_is_counted` must FAIL. Restore.

```bash
git add crates/ast-core/src/pal.rs crates/ast-core/src/wcslib.rs
git commit -m "ast-core: count calls into PAL, ERFA and wcslib"
```

---

### Task 6: The C engine shim and build script

**Files:**
- Create: `crates/ast-bench/c/bench_engine.c`, `crates/ast-bench/c/bench_engine.h`, `tools/perf/build_c_engine.sh`, `tools/perf/gen_count_header.py`

**Interfaces:**
- Produces a shared library `target/perf/c/<variant>/libast_bench_engine.{dylib,so}`, `<variant>` ∈ {`bench`, `count`}, plus `target/perf/c/<variant>/build-info.txt` (one `key=value` per line: `cc`, `cflags`, `submodule`, `variant`).
- C ABI (`bench_engine.h`):

```c
/* Load a fixture (path relative to root); returns NULL and fills err on failure.
   .head is read through a FitsChan, everything else as Native text, exactly as
   ast_tester/transform_oracle_util.c's oracle_load_mapping does. */
void *abe_load( const char *root, const char *relpath, char *err, int errlen );
int abe_nin( void *map );
int abe_nout( void *map );
/* in: ncoord_in columns of npoint values; out: ncoord_out columns.
   Returns 0 on success, nonzero (status cleared) on an AST error. */
int abe_tran( void *map, int forward, int npoint, int ncoord_in,
              const double *in, int ncoord_out, double *out );
void abe_free( void *map );
/* Counting build only; the bench build returns -1 from both. */
int abe_counts_reset( void );
/* Writes up to max "name\tcount\n" lines into buf; returns bytes written. */
int abe_counts_dump( char *buf, int max );
```

- [ ] **Step 1: Write `bench_engine.c`**

```c
#include <stdio.h>
#include <string.h>
#include "ast.h"
#include "transform_oracle.h"
#include "bench_engine.h"

static int registered = 0;

void *abe_load( const char *root, const char *relpath, char *err, int errlen ) {
   int status_value = 0;
   if( !registered ) { oracle_register_intramaps( &status_value ); registered = 1; }
   astBegin;
   AstMapping *map = oracle_load_mapping( root, relpath, NULL, NULL );
   if( map && astOK ) astExempt( map );
   astEnd;
   if( !astOK || !map ) {
      snprintf( err, (size_t) errlen, "C could not load %s", relpath );
      astClearStatus;
      return NULL;
   }
   return map;
}

int abe_nin( void *map ) { return astGetI( (AstMapping *) map, "Nin" ); }
int abe_nout( void *map ) { return astGetI( (AstMapping *) map, "Nout" ); }

int abe_tran( void *map, int forward, int npoint, int ncoord_in,
              const double *in, int ncoord_out, double *out ) {
   astTranN( (AstMapping *) map, npoint, ncoord_in, npoint, in, forward,
             ncoord_out, npoint, out );
   if( !astOK ) { astClearStatus; return 1; }
   return 0;
}

void abe_free( void *map ) { astAnnul( (AstMapping *) map ); }

#ifdef AST_BENCH_COUNTS
#include "ast_counts.h"
int abe_counts_reset( void ) { ast_counts_reset(); return 0; }
int abe_counts_dump( char *buf, int max ) { return ast_counts_dump( buf, max ); }
#else
int abe_counts_reset( void ) { return -1; }
int abe_counts_dump( char *buf, int max ) { (void) buf; (void) max; return -1; }
#endif
```

- [ ] **Step 2: Write `gen_count_header.py`**

It reads `pal2ast.h` and `erfa2ast.h` (lines `#define palXxx astPalXxx` / `#define eraXxx astEraXxx`) and emits `ast_counts.h` and `ast_counts.c`:

- `ast_counts.h` includes `<math.h>`, `"pal.h"`, `"erfa.h"` and `"erfa2ast.h"` first (all include-guarded, so the source's own later includes are no-ops), declares `void ast_counts_hit( const char * );`, `void ast_counts_reset( void );` and `int ast_counts_dump( char *, int );`, and then for every PAL/ERFA name:

```c
#undef palEqgal
#define palEqgal(...) ( ast_counts_hit( "astPalEqgal" ), astPalEqgal( __VA_ARGS__ ) )
```

  and for each libm function in the Task 4 key list:

```c
#define sin(x) ( ast_counts_hit( "sin" ), (sin)( x ) )
```

- `ast_counts.c` keeps a fixed table of `{const char *name; unsigned long long n;}` grown by linear search (the name set is small), and `ast_counts_dump` writes `name\tcount\n` lines in name order.

- [ ] **Step 3: Write `build_c_engine.sh`**

```sh
#!/bin/sh
# Build the reference C library as a runtime-loadable engine for ast-bench.
#
#   tools/perf/build_c_engine.sh bench   # timing build
#   tools/perf/build_c_engine.sh count   # counts calls into libm/PAL/ERFA/wcslib
#
# Output: target/perf/c/<variant>/libast_bench_engine.{dylib,so} and
# build-info.txt.  The submodule is never modified: everything happens in a copy.
set -e
VARIANT=${1:?usage: $0 bench|count}
HERE=$(cd "$(dirname "$0")/../.." && pwd)
SRC=$HERE/reference/starlink-ast
DEST=$HERE/target/perf/c/$VARIANT
[ -d "$SRC/src" ] || { echo "reference submodule not initialised"; exit 1; }
CFLAGS_PINNED="-O3 -ffp-contract=off"

rm -rf "$DEST"; mkdir -p "$DEST/src"
( cd "$SRC" && git ls-files ) > "$DEST/.files"
( cd "$SRC" && tar cf - -T "$DEST/.files" ) | ( cd "$DEST/src" && tar xf - )

EXTRA=""
if [ "$VARIANT" = count ]; then
   python3 "$HERE/tools/perf/gen_count_header.py" "$DEST/src"
   # Count calls made by AST's own sources only: never the PAL, ERFA or wcslib
   # sources, whose internal calls are the same code in both engines.
   for f in "$DEST"/src/src/*.c; do
      printf '#include "ast_counts.h"\n' | cat - "$f" > "$f.tmp" && mv "$f.tmp" "$f"
   done
   # Projection evaluations are reached through function pointers.
   sed -i.bak -e 's/prjdata->WcsFwd(/( ast_counts_hit( "wcs_fwd" ), prjdata->WcsFwd )(/g' \
              -e 's/prjdata->WcsRev(/( ast_counts_hit( "wcs_rev" ), prjdata->WcsRev )(/g' \
              "$DEST/src/src/wcsmap.c"
   EXTRA="-DAST_BENCH_COUNTS $DEST/src/ast_counts.c"
fi

cmake -S "$DEST/src" -B "$DEST/build" -DCMAKE_BUILD_TYPE=Release -DAST_BUILD_FORTRAN=OFF \
   -DCMAKE_C_FLAGS="$CFLAGS_PINNED -I$DEST/src" -DCMAKE_POSITION_INDEPENDENT_CODE=ON > "$DEST/cmake.log" 2>&1
for t in ast ast_pal ast_cminpack ast_err ast_grf_2.0 ast_grf_3.2 ast_grf_5.6 ast_grf3d; do
   cmake --build "$DEST/build" --target "$t" -j8 >> "$DEST/build.log" 2>&1 \
      || { tail -25 "$DEST/build.log"; exit 1; }
done

case "$(uname -s)" in
   Darwin) LIB=libast_bench_engine.dylib; SHARED="-dynamiclib"; WHOLE="-Wl,-all_load" ;;
   *)      LIB=libast_bench_engine.so;    SHARED="-shared -Wl,-Bsymbolic"; WHOLE="-Wl,--whole-archive" ;;
esac
YAML=$(ls -d /opt/homebrew/opt/libyaml/lib /usr/local/opt/libyaml/lib 2>/dev/null | head -1)
B=$DEST/build
cc $CFLAGS_PINNED $SHARED -fPIC -o "$DEST/$LIB" \
   -I "$B" -I "$DEST/src" -I "$DEST/src/ast_tester" -I "$HERE/crates/ast-bench/c" \
   "$HERE/crates/ast-bench/c/bench_engine.c" "$DEST/src/ast_tester/transform_oracle_util.c" $EXTRA \
   "$B/libast.a" "$B/libast_pal.a" "$B/libast_cminpack.a" "$B/libast_err.a" \
   "$B/libast_grf_2.0.a" "$B/libast_grf_3.2.a" "$B/libast_grf_5.6.a" "$B/libast_grf3d.a" \
   ${YAML:+-L "$YAML"} -lyaml -lm

{ echo "variant=$VARIANT"; echo "cc=$(cc --version | head -1)"; echo "cflags=$CFLAGS_PINNED";
  echo "submodule=$(git -C "$SRC" rev-parse HEAD)"; } > "$DEST/build-info.txt"
echo "built $DEST/$LIB"
```

`$WHOLE` is unused when every symbol the shim needs is referenced directly; drop it if the link succeeds without it (it is listed only because `ld` may otherwise discard archive members that the loader needs).

- [ ] **Step 4: Build both variants and check them**

Run (clean env): `tools/perf/build_c_engine.sh bench && tools/perf/build_c_engine.sh count`
Then `nm -gU target/perf/c/bench/libast_bench_engine.dylib | rg ' _abe_'` (Linux: `nm -D --defined-only`).
Expected: the seven `abe_*` symbols in both; `rg -c ast_counts_hit target/perf/c/count/src/src/skyframe.c` > 0; `otool -tv target/perf/c/bench/libast_bench_engine.dylib | rg -c fmadd` prints 0 (no FMA contraction).

- [ ] **Step 5: Commit**

```bash
git add crates/ast-bench/c tools/perf
git commit -m "Build the reference C library as a loadable bench engine"
```

---

### Task 7: Load the C engine from Rust

**Files:**
- Create: `crates/ast-bench/src/c_engine.rs`
- Modify: `crates/ast-bench/src/lib.rs` (`#[cfg(feature = "c-oracle")] pub mod c_engine;`)

**Interfaces:**
- Consumes: the C ABI from Task 6.
- Produces:
  - `pub enum Variant { Bench, Count }` with `pub fn default_path(self) -> PathBuf` (`<workspace>/target/perf/c/<bench|count>/libast_bench_engine.<dylib|so>`), overridable by env `AST_BENCH_C_LIB`.
  - `pub struct CEngine` with `pub fn open(v: Variant) -> Result<CEngine, String>`, `pub fn build_info(&self) -> String`, `pub fn load(&self, root: &Path, relpath: &str) -> Result<CMapping<'_>, String>`, `pub fn counts_reset(&self) -> Result<(), String>`, `pub fn counts(&self) -> Result<BTreeMap<String, u64>, String>`.
  - `pub struct CMapping<'a>` with `pub fn nin(&self) -> usize`, `pub fn nout(&self) -> usize`, `pub fn tran(&self, forward: bool, npoint: usize, ncoord_in: usize, input: &[f64], ncoord_out: usize, out: &mut [f64]) -> Result<(), String>`, freed on `Drop`.

- [ ] **Step 1: Write the failing tests** (compiled only with `c-oracle`; they hard-fail if the library is missing)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::{parse_oracle, CORE_FIXTURES};

    #[test]
    fn missing_library_names_the_build_command() {
        let e = CEngine::open_path(Path::new("/nonexistent/libx.dylib")).unwrap_err();
        assert!(e.contains("tools/perf/build_c_engine.sh"), "{e}");
    }

    #[test]
    fn c_reproduces_its_own_oracle_rows() {
        let eng = CEngine::open(Variant::Bench).unwrap();
        let text = std::fs::read_to_string(format!("{CORE_FIXTURES}/oracle/simplify_fixtures.oracle")).unwrap();
        let s = parse_oracle(&text).unwrap().into_iter().find(|s| s.relpath == "simplify/cap_slamap_01.simp").unwrap();
        let m = eng.load(Path::new(CORE_FIXTURES), &s.relpath).unwrap();
        let n = s.npoint();
        let input: Vec<f64> = s.inputs.concat();
        let mut out = vec![0.0; s.nout * n];
        m.tran(s.forward, n, s.nin, &input, s.nout, &mut out).unwrap();
        assert_eq!(out, s.outputs.concat());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-bench --features c-oracle c_engine::`
Expected: compile error.

- [ ] **Step 3: Implement**

Use `libloading::Library::new` (on Linux, `libloading::os::unix::Library::open(Some(path), RTLD_NOW | RTLD_LOCAL)` so the library's PAL and ERFA symbols stay private) and resolve the seven `abe_*` symbols once into plain function pointers stored in `CEngine`.
`open_path` errors are formatted as:

```rust
format!("cannot load the C engine {}: {e}\nbuild it with: tools/perf/build_c_engine.sh {variant}", path.display())
```

`counts()` calls `abe_counts_dump` into a 64 KiB buffer, errors on `-1` ("this is the bench build; counts need tools/perf/build_c_engine.sh count"), and parses the `name\tcount` lines.
`build_info()` reads `build-info.txt` beside the library.

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-bench --features c-oracle c_engine::` (after Task 6's builds)
Expected: 2 passed. Also `cargo clippy -p ast-bench --features c-oracle --tests -- -D warnings`.

- [ ] **Step 5: Negative control and commit**

Change `assert_eq!(out, …)` input to use `!s.forward`; expected FAIL. Restore.

```bash
git add crates/ast-bench
git commit -m "ast-bench: load the C engine at runtime"
```

---

### Task 8: Phase 0 — call-count parity over the oracles

**Files:**
- Modify: `crates/ast-oracle-tests/Cargo.toml`, `crates/ast-oracle-tests/src/lib.rs` (module doc only)
- Create: `crates/ast-oracle-tests/tests/call_count_parity.rs`

**Interfaces:**
- Consumes: `ast_bench::{oracle, load, c_engine}`, `ast_core::call_counts::{reset, snapshot}`.
- Produces: `COUNT_PARITY_EXCLUDED: &[(&str, &str)]` (`"relpath dir=forward|inverse"`, reason).

- [ ] **Step 1: Dependencies**

```toml
[dependencies]
ast-core = { path = "../ast-core", features = ["call-counts"] }
ast-bench = { path = "../ast-bench", features = ["c-oracle"] }
```

- [ ] **Step 2: Write the test**

```rust
//! Phase 0 of the benchmarking design: for every transform-oracle section the
//! port must make the same calls into libm, PAL, ERFA and wcslib as C, counted
//! at the boundary of AST's own code on both sides.
//!
//! Needs the counting C engine: `tools/perf/build_c_engine.sh count`.

use std::collections::BTreeMap;
use std::path::Path;

use ast_bench::c_engine::{CEngine, Variant};
use ast_bench::load::{load_fixture, Loaded};
use ast_bench::oracle::{parse_oracle, CORE_FIXTURES, CORE_ORACLES};
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;

/// Sections whose counts differ for an understood reason, each with that reason.
const COUNT_PARITY_EXCLUDED: &[(&str, &str)] = &[];

fn key(relpath: &str, forward: bool) -> String {
    format!("{relpath} dir={}", if forward { "forward" } else { "inverse" })
}

fn mismatches() -> Vec<(String, BTreeMap<String, (u64, u64)>)> {
    let c = CEngine::open(Variant::Count).unwrap();
    let root = Path::new(CORE_FIXTURES);
    let mut found = Vec::new();
    for name in CORE_ORACLES {
        let text = std::fs::read_to_string(root.join("oracle").join(name)).unwrap();
        for s in parse_oracle(&text).unwrap() {
            let Loaded::Mapping(obj) = load_fixture(root, &s.relpath).unwrap() else { continue };
            let map = obj.as_mapping().unwrap();
            let n = s.npoint();
            let rows: Vec<Vec<f64>> = (0..n).map(|r| s.inputs.iter().map(|c| c[r]).collect()).collect();
            let pts = PointSet::from_rows(rows);
            let mut out = PointSet::zeros(n, s.nout);
            ast_core::call_counts::reset();
            map.transform_into(&pts, s.forward, &mut out).unwrap();
            let rust = ast_core::call_counts::snapshot();

            let cm = c.load(root, &s.relpath).unwrap();
            let mut cout = vec![0.0; s.nout * n];
            c.counts_reset().unwrap();
            cm.tran(s.forward, n, s.nin, &s.inputs.concat(), s.nout, &mut cout).unwrap();
            let cc = c.counts().unwrap();
            for (i, (got, want)) in cout.iter().zip(s.outputs.concat()).enumerate() {
                let ok = if *got == BAD_VALUE || want == BAD_VALUE { *got == want }
                    else { (got - want).abs() <= 1e-7 + 1e-7 * want.abs() };
                assert!(ok, "{} value {i}: C {got} vs oracle {want}", key(&s.relpath, s.forward));
            }

            let mut diff = BTreeMap::new();
            for k in rust.keys().chain(cc.keys()) {
                let (r, cv) = (rust.get(k).copied().unwrap_or(0), cc.get(k).copied().unwrap_or(0));
                if r != cv { diff.insert(k.clone(), (r, cv)); }
            }
            if !diff.is_empty() { found.push((key(&s.relpath, s.forward), diff)); }
        }
    }
    found
}

#[test]
fn rust_and_c_make_the_same_library_calls() {
    let found = mismatches();
    let unexplained: Vec<_> = found.iter()
        .filter(|(k, _)| !COUNT_PARITY_EXCLUDED.iter().any(|(e, _)| e == k))
        .collect();
    assert!(unexplained.is_empty(), "count mismatches (name: rust vs c):\n{unexplained:#?}");
}

#[test]
fn every_exclusion_still_differs() {
    let found = mismatches();
    for (k, reason) in COUNT_PARITY_EXCLUDED {
        assert!(found.iter().any(|(f, _)| f == k), "{k} now matches; remove it ({reason})");
    }
}
```

- [ ] **Step 3: Run it and stop to report**

Run: `tools/perf/build_c_engine.sh count && cargo test -p ast-oracle-tests --test call_count_parity -- --nocapture 2>&1 | tee <scratchpad>/phase0.txt`
Expected: the first run very likely FAILS, listing mismatches (for example `astPRJset`, which the port calls explicitly and C reaches only inside the projection functions, and `powi`, which has no C counterpart).

**Stop here and report the grouped mismatches to the user.**
For each group, the user decides: a porting defect to fix (its own commit, with the C source cited), or an understood difference to add to `COUNT_PARITY_EXCLUDED` with its reason.
Do not add an exclusion without that decision.

- [ ] **Step 4: After the decisions, verify and commit**

Run the test again; expected PASS for both tests.
Negative control: change one exclusion's key to a section that matches; `every_exclusion_still_differs` must FAIL. Restore.

```bash
git add crates/ast-oracle-tests
git commit -m "Check the port's library call counts against C over the transform oracles"
```

---

### Task 9: `bench` fixtures, their oracle, and their gate

**Files:**
- Create: `tools/perf/gen_bench_fixtures.c`, `crates/ast-bench/fixtures/*.map` (generated), `crates/ast-bench/fixtures/bench.oracle` (generated), `crates/ast-bench/fixtures/README.md`
- Modify: `tools/perf/build_c_engine.sh` (also build `gen_bench_fixtures` in the bench variant), `crates/ast-bench/src/oracle.rs`

**Interfaces:**
- Produces: `pub const BENCH_FIXTURES: &str` (`crates/ast-bench/fixtures`) and `pub const BENCH_ORACLE: &str = "bench.oracle"` in `oracle.rs`; fixture names as in the table below.

The generator constructs each Mapping with C's public constructors, writes it as Native text with `astChannel`/`astWrite`, then samples `ORACLE_NPOINT` (24) points with `oracle_sample_points` over the fixture's own domain, runs `astTranP` forward and (when `TranInverse`) inverse with the forward outputs as inverse inputs, and writes each section exactly as `gen_transform_oracle.c:183-213` does (copy `transform_write` and its column format).

| Fixture | Construction | Domain |
|---|---|---|
| `zoommap_2d.map` | `astZoomMap(2, 2.5, "")` | [-1, 1] |
| `shiftmap_2d.map` | `astShiftMap(2, {0.25, -0.5}, "")` | [-1, 1] |
| `matrixmap_3x3_full.map` | `astMatrixMap(3, 3, 0, {0.866,-0.5,0, 0.5,0.866,0, 0.1,0.05,0.995}, "")` | [-1, 1] |
| `matrixmap_3x3_diag.map` | `astMatrixMap(3, 3, 1, {1.5, 0.75, 1.25}, "")` | [-1, 1] |
| `winmap_2d.map` | `astWinMap(2, {0,0}, {1,1}, {0.5,-0.25}, {1.5,0.75}, "")` | [-1, 1] |
| `permmap_2d_swap.map` | `astPermMap(2, {2,1}, 2, {2,1}, NULL, "")` | [-1, 1] |
| `unitmap_2d.map` | `astUnitMap(2, "")` | [-1, 1] |
| `polymap_2d_deg5.map` | the 18 terms of `bench_per_class.rs:282-316`; `IterInverse=1, NiterInverse=10, TolInverse=1e-7` | [-0.5, 0.5] |
| `chebymap_2d_deg3.map` | the 12 terms of `bench_per_class.rs:372-390`, bounds [-2, 2] | [-1, 1] |
| `lutmap_100.map` | 100 entries `i + 0.1*i^1.3`, start 0, inc 1, `LutInterp=0` | [0, 99] |
| `wcsmap_tan.map` | `astWcsMap(2, AST__TAN, 1, 2, "")` | [-0.05, 0.05] |
| `wcsmap_sin.map` | `astWcsMap(2, AST__SIN, 1, 2, "")` | [-0.05, 0.05] |
| `wcsmap_zpn.map` | `astWcsMap(2, AST__ZPN, 1, 2, "PV2_1=1, PV2_3=0.05")` | [-0.05, 0.05] |
| `sphmap.map` | `astSphMap("")` | [-1, 1] |
| `pcdmap.map` | `astPcdMap(1.0e-3, {0, 0}, "")` | [-0.1, 0.1] |
| `grismmap.map` | `astGrismMap("GrismNR=1.5, GrismWaveR=5e-7, GrismG=1e7, GrismM=1, GrismEps=0, GrismAlpha=0, GrismTheta=0, GrismNRP=0")` | [-1e-3, 1e-3] |
| `unitnormmap_2d.map` | `astUnitNormMap(2, {0.5, -0.5}, "")` | [-1, 1] |
| `normmap_2d.map` | `astNormMap(astFrame(2, ""), "")` | [-1, 1] |
| `tranmap_zoom_shift.map` | `astTranMap(astZoomMap(2,2,""), astShiftMap(2,{0.5,0.5},""), "")` | [-1, 1] |
| `ratemap_polymap.map` | `astRateMap(` the 6-term PolyMap of `bench_per_class.rs:441-455` `, 1, 1, "")` | [-0.5, 0.5] |
| `slamap_eqgal.map` | `astSlaMap(0, "")` then `astSlaAdd(m, "EQGAL", 0, NULL)` | [0, 1.5] |
| `timemap_taiutc.map` | `astTimeMap(0, "")` then `astTimeAdd(m, "TAITOUTC", 1, {50000})` | [50000, 50100] |
| `specmap_wn_to_fr.map` | `astSpecMap(1, 0, "")` then `astSpecAdd(m, "WNTOFR", 0, NULL)` | [1, 1000] |
| `splinemap_5x5_cubic.map` | the knots and coefficients of `bench_per_class.rs:496-513` | [0.1, 0.9] |
| `mathmap_2d_linear.map` | `astMathMap(2, 2, 2, {"u = x + y","v = x - y"}, 2, {"x = 0.5 * (u + v)","y = 0.5 * (u - v)"}, "")` | [-1, 1] |
| `cmpmap_series.map` | `astCmpMap(zoommap_2d, shiftmap_2d, 1, "")` | [-1, 1] |
| `cmpmap_parallel.map` | `astCmpMap(zoommap 1-D ×2.5, shiftmap 1-D 0.25, 0, "")` | [-1, 1] |

DssMap has no public C constructor; it is covered by the DSS headers in the later curated FrameSet set, and the README says so.
Before writing each constructor, check its argument order in the class's `.h` in the submodule; the table gives the values, not a verified prototype.

- [ ] **Step 1: Write the failing gate test** (`crates/ast-bench/tests/bench_fixtures.rs`)

```rust
//! The bench fixtures are not in ast-core's oracle gate (they are not yet in
//! ast_tester), so this test holds the port to C's recorded outputs for them,
//! with the tolerances of `crates/ast-core/tests/test_transform_oracle.rs`.

use std::path::Path;

use ast_bench::load::{load_fixture, Loaded};
use ast_bench::oracle::{parse_oracle, BENCH_FIXTURES, BENCH_ORACLE};
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;

const GOLDEN_RTOL: f64 = 1e-7;
const GOLDEN_ATOL: f64 = 1e-7;
/// One per fixture per defined direction; update when a fixture is added.
const BENCH_SECTIONS: usize = 49;

fn within_tol(got: f64, reference: f64) -> bool {
    if got == BAD_VALUE || reference == BAD_VALUE {
        return got == reference;
    }
    !got.is_nan() && (got - reference).abs() <= GOLDEN_ATOL + GOLDEN_RTOL * reference.abs()
}

#[test]
fn bench_fixtures_match_oracle() {
    let root = Path::new(BENCH_FIXTURES);
    let text = std::fs::read_to_string(root.join(BENCH_ORACLE)).unwrap();
    let sections = parse_oracle(&text).unwrap();
    assert_eq!(sections.len(), BENCH_SECTIONS);
    for s in sections {
        let Loaded::Mapping(obj) = load_fixture(root, &s.relpath).unwrap() else {
            panic!("{} is not a Mapping", s.relpath)
        };
        let n = s.npoint();
        let rows: Vec<Vec<f64>> = (0..n).map(|r| s.inputs.iter().map(|c| c[r]).collect()).collect();
        let out = obj.as_mapping().unwrap().transform(&PointSet::from_rows(rows), s.forward).unwrap();
        for r in 0..n {
            for a in 0..s.nout {
                assert!(within_tol(out.point(r)[a], s.outputs[a][r]),
                    "{} row {r} axis {a}: {} vs C {}", s.relpath, out.point(r)[a], s.outputs[a][r]);
            }
        }
    }
}
```

Set `BENCH_SECTIONS` to the count the generator reports in Step 3 (each line of the table with a defined inverse gives two sections).

- [ ] **Step 2: Write the generator and wire it into the bench build**

Write `tools/perf/gen_bench_fixtures.c` per the table, with `main(int argc, char **argv)` taking the output directory, writing `<dir>/<name>.map` and `<dir>/bench.oracle` (header lines `# bench oracle - regenerate with tools/perf/gen_bench_fixtures` and the same `# tol:` line as the core oracles), and printing `wrote <k> sections`.
Every fixture's `has_inverse`, as `TranInverse` reports it, decides its inverse section; a construction or transform error aborts with the fixture name.
In `build_c_engine.sh`, for the bench variant, also compile `gen_bench_fixtures` against the same archives as an executable in `$DEST`.

- [ ] **Step 3: Generate**

Run: `tools/perf/build_c_engine.sh bench && target/perf/c/bench/gen_bench_fixtures crates/ast-bench/fixtures`
Expected: `wrote N sections`; set `BENCH_SECTIONS` to N.
Write `crates/ast-bench/fixtures/README.md`: what these are, how to regenerate, that they move to `ast_tester/fixtures/bench/` in a later C pull request, and that DssMap is absent with the reason.

- [ ] **Step 4: Run the gate**

Run: `cargo test -p ast-bench --test bench_fixtures`
Expected: PASS. A failure is a porting defect in that class, or a construction the Rust reader cannot load; report it to the user before changing anything.

- [ ] **Step 5: Negative control and commit**

Edit one output value in `bench.oracle` by 1e-3 relative; expected FAIL naming that fixture. Restore with `git checkout`.

```bash
git add tools/perf/gen_bench_fixtures.c tools/perf/build_c_engine.sh crates/ast-bench/fixtures crates/ast-bench/tests crates/ast-bench/src/oracle.rs
git commit -m "ast-bench: one plain fixture per Mapping class, with a C-recorded oracle"
```

---

### Task 10: Timing core and `run`

**Files:**
- Create: `crates/ast-bench/src/timing.rs`, `crates/ast-bench/src/cases.rs`, `crates/ast-bench/src/report.rs`, `crates/ast-bench/sets/bench.txt`
- Modify: `crates/ast-bench/src/main.rs`, `crates/ast-bench/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 2, 3, 7, 9.
- Produces:
  - `pub struct Case { pub root: PathBuf, pub section: Section }`; `pub fn load_set(name: &str) -> Result<Vec<Case>, String>` reading `sets/<name>.txt` lines `<oracle-file> <relpath> # reason` (`<oracle-file>` is `bench` for `bench.oracle` or one of the three core names); every relpath's sections in that oracle (forward and inverse) become cases; an unknown relpath is an error.
  - `pub struct Stats { pub median_ns_per_point: f64, pub min_ns_per_point: f64, pub samples: usize }`
  - `pub fn measure(budget: Duration, min_samples: usize, npoint: usize, f: &mut dyn FnMut() -> Result<(), String>) -> Result<Stats, String>`
  - `pub struct Row { pub case: String, pub dir: &'static str, pub n: usize, pub engine: &'static str, pub stats: Stats }` (`case` is the relpath, `engine` is `rust` or `c`)
  - `pub fn run_set(cases: &[Case], engines: Engines, npoints: &[usize]) -> Vec<Result<Row, String>>`
  - `report::table(&[Row]) -> String` and `report::tsv(&[Row]) -> String` (columns `case dir n engine median_ns min_ns samples`), plus `report::parse_tsv(&str) -> Result<Vec<Row>, String>`.

`sets/bench.txt` lists every fixture from Task 9 as `bench <name>.map # one plain <Class>`.

- [ ] **Step 1: Write the failing tests**

```rust
// timing.rs
#[test]
fn measure_reports_per_point_time() {
    let mut calls = 0u64;
    let s = measure(Duration::from_millis(20), 5, 1000, &mut || {
        calls += 1;
        std::thread::sleep(Duration::from_micros(100));
        Ok(())
    }).unwrap();
    assert!(s.samples >= 5);
    // 100 µs over 1000 points is about 100 ns per point; sleep only overshoots.
    assert!(s.min_ns_per_point >= 100.0 && s.min_ns_per_point < 1000.0, "{s:?}");
}

#[test]
fn measure_propagates_an_error() {
    assert!(measure(Duration::from_millis(5), 1, 1, &mut || Err("boom".into())).is_err());
}

// cases.rs
#[test]
fn bench_set_covers_every_bench_section() {
    let cases = load_set("bench").unwrap();
    let text = std::fs::read_to_string(format!("{}/{}", crate::oracle::BENCH_FIXTURES, crate::oracle::BENCH_ORACLE)).unwrap();
    assert_eq!(cases.len(), crate::oracle::parse_oracle(&text).unwrap().len());
}

// main.rs or lib-level test
#[test]
fn run_refuses_a_counting_build() {
    assert!(!ast_core::CALL_COUNTS_ENABLED, "ast-bench must never be built with call-counts");
}

#[test]
fn a_failing_case_is_reported_not_fatal() {
    // A section whose fixture does not exist yields an Err row naming it; the rest still run.
    let mut cases = load_set("bench").unwrap();
    cases.truncate(1);
    let mut bad = cases[0].clone();
    bad.section.relpath = "missing.map".into();
    cases.push(bad);
    let rows = run_set(&cases, Engines::Rust, &[1]);
    assert!(rows[0].is_ok());
    assert!(rows.last().unwrap().as_ref().unwrap_err().contains("missing.map"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-bench`
Expected: compile errors.

- [ ] **Step 3: Implement**

`measure`: first calibrate `reps` so one sample takes at least 1 ms (double from 1); then take samples until `budget` has elapsed and at least `min_samples` exist; each sample's ns/point is `elapsed_ns / (reps * npoint)`; report median and min.
`run_set`: for each case, load it once per engine (Rust via `load_fixture`, C via `CEngine::load`), then for each N select rows, build the input (`rows_to_pointset` / `rows_to_columns`) and pre-allocated output (`PointSet::zeros(n, nout)` / `vec![0.0; nout * n]`), and time the transform: Rust `map.transform_into(&pts, forward, &mut out)`, C `cm.tran(..)`.
With both engines, alternate them sample by sample (`measure` on each in turn, budget split) so drift affects both.
Budgets: 200 ms per (case, direction, N, engine) for N ≤ 100 000, 1 s for 1 000 000; `min_samples` 7.
`main.rs` `run`: parse `--set`, `--engine` (default `rust`; `c` and `both` require the `c-oracle` feature and error otherwise naming it), `--format`, `--save`; at startup `if ast_core::CALL_COUNTS_ENABLED { exit with "this build counts library calls; rebuild ast-bench without call-counts" }`; print `report::table` (with a ratio column when both engines ran) or `report::tsv`; print each `Err` row to stderr and exit nonzero at the end if any.

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-bench && cargo run --release -q -p ast-bench -- run --set bench` and, after Task 6, `cargo run --release -q -p ast-bench --features c-oracle -- run --set bench --engine both`.
Expected: tests pass; a table with one line per case, direction and N.

- [ ] **Step 5: Negative control and commit**

In `run_set`, time `transform_into` twice per call; the Rust column for every case must roughly double. Restore.

```bash
git add crates/ast-bench
git commit -m "ast-bench: time the bench set on Rust and, optionally, C"
```

---

### Task 11: Fingerprints, `--save` and `compare`

**Files:**
- Create: `crates/ast-bench/src/fingerprint.rs`, `crates/ast-bench/src/compare.rs`, `crates/ast-bench/build.rs`
- Modify: `crates/ast-bench/src/main.rs`, `crates/ast-bench/src/lib.rs`, `PLAN.md` §6 (relaxation count)

**Interfaces:**
- Produces:
  - `pub struct Fingerprint { pub fields: BTreeMap<&'static str, String> }` with `pub fn current(c_info: Option<String>) -> Fingerprint`, `pub fn id(&self) -> String` (16-hex FNV-1a of `key=value\n` lines in key order), `pub fn diff(&self, other: &Fingerprint) -> Vec<String>` (names of differing fields).
  - Fields: `cpu` (macOS `sysctl -n machdep.cpu.brand_string`, Linux the first `model name` in `/proc/cpuinfo`), `os` (`uname -sr`), `rustc` (from `build.rs`: `rustc -V` into `env!("AST_BENCH_RUSTC")`), `profile` (`release` or `debug` from `cfg!(debug_assertions)`), `c` (the C engine's `build-info.txt`, or `none`).
  - Baselines: `target/perf/baselines/<id>/fingerprint.txt` and `<set>.tsv`.
  - `pub const SLOWDOWN_THRESHOLD: f64 = 0.10;` and `pub const RELAXED: &[(&str, f64, &str)] = &[];` (`case`, threshold, reason).
  - `pub fn slowdowns(base: &[Row], cur: &[Row]) -> Vec<String>` (compares medians per `case dir n engine`).

- [ ] **Step 1: Write the failing tests**

```rust
// fingerprint.rs
#[test]
fn id_is_stable_and_field_sensitive() {
    let mut a = Fingerprint { fields: BTreeMap::new() };
    a.fields.insert("cpu", "M4".into());
    let mut b = a.clone();
    assert_eq!(a.id(), b.id());
    b.fields.insert("cpu", "M3".into());
    assert_ne!(a.id(), b.id());
    assert_eq!(a.diff(&b), vec!["cpu".to_string()]);
}

// compare.rs
fn row(case: &str, median: f64) -> Row { /* n = 1, engine "rust", dir "forward" */ }

#[test]
fn flags_only_slowdowns_over_threshold() {
    let base = vec![row("a.map", 100.0), row("b.map", 100.0)];
    let cur = vec![row("a.map", 109.0), row("b.map", 115.0)];
    let s = slowdowns(&base, &cur);
    assert_eq!(s.len(), 1);
    assert!(s[0].contains("b.map"));
}

#[test]
fn refuses_a_baseline_from_another_machine() {
    let mut saved = Fingerprint::current(None);
    saved.fields.insert("cpu", "some other cpu".into());
    let e = check_same_machine(&saved, &Fingerprint::current(None)).unwrap_err();
    assert!(e.contains("cpu") && e.contains("--save"), "{e}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-bench fingerprint:: compare::`
Expected: compile errors.

- [ ] **Step 3: Implement**

`slowdowns` reports `"{case} {dir} N={n} {engine}: {base:.1} -> {cur:.1} ns/pt (+{pct:.0}%)"` when `cur / base - 1 > threshold`, where `threshold` is `RELAXED`'s entry for the case or `SLOWDOWN_THRESHOLD`.
`check_same_machine` errors with `"baseline was saved on a different machine or toolchain ({fields}); re-run with --save"`.
`run --save` writes the fingerprint and TSV; `compare --set S` re-runs, loads the baseline for the current id (error if none: "no baseline for this machine; run `run --set S --save` first"), prints slowdowns, exits 1 if any.
Add a `PLAN.md` §6 line: "ast-bench `RELAXED`: 0 entries".

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-bench && cargo run --release -q -p ast-bench -- run --set bench --save && cargo run --release -q -p ast-bench -- compare --set bench`
Expected: tests pass; `compare` reports no slowdowns (or a few noise-level ones at N = 1; record which in the commit message and do not change the threshold).

- [ ] **Step 5: Negative control and commit**

Add `std::hint::black_box((0..200).sum::<u64>())` per point to `ZoomMap`'s transform loop temporarily; `compare` must flag `zoommap_2d.map`. Revert.

```bash
git add crates/ast-bench PLAN.md
git commit -m "ast-bench: machine-keyed baselines and compare"
```

---

### Task 12: `compare --base <git-ref>`

**Files:**
- Create: `crates/ast-bench/src/base.rs`
- Modify: `crates/ast-bench/src/main.rs`, `crates/ast-bench/README.md`

**Interfaces:**
- Consumes: `report::parse_tsv`, `compare::slowdowns`.
- Produces: `pub fn compare_with_base(git_ref: &str, set: &str, rounds: usize) -> Result<Vec<String>, String>` (slowdowns), `pub const ROUNDS: usize = 5;`, and `pub fn combine_rounds(rounds: &[Vec<Row>]) -> Vec<Row>` (per-key median of the rounds' medians).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn combine_takes_the_median_round() {
    let r = |m| vec![row("a.map", m)];
    let c = combine_rounds(&[r(100.0), r(300.0), r(110.0)]);
    assert_eq!(c[0].stats.median_ns_per_point, 110.0);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ast-bench base::`
Expected: compile error.

- [ ] **Step 3: Implement**

`compare_with_base`:
1. `git rev-parse --verify <ref>^{commit}`; error if not a commit.
2. `git worktree add --detach target/perf/base/<sha> <sha>` (reuse if it exists at that sha).
3. Build both: `cargo build --release -p ast-bench --target-dir target/perf/base/target` inside the worktree, and `cargo build --release -p ast-bench` here; if the base tree has no `ast-bench` crate, error: "<ref> predates ast-bench; choose a later base".
4. For `round in 0..rounds`: run base then current, each as `<bin> run --set <set> --engine rust --format tsv`, parse both.
5. `slowdowns(&combine_rounds(base), &combine_rounds(cur))`.
6. Leave the worktree for reuse; `README.md` documents `git worktree remove target/perf/base/<sha>`.

The set file is read from the current tree for both builds (pass `--set-file <abs path>`; add that option to `run`), so both time the same cases.

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-bench base:: && cargo run --release -q -p ast-bench -- compare --set bench --base HEAD`
Expected: no slowdowns (base and current are the same code).

- [ ] **Step 5: Negative control and commit**

Commit the Task 11 ZoomMap slowdown on a scratch branch, run `compare --base <the commit before it>`; it must flag `zoommap_2d.map`. Delete the scratch branch.

```bash
git add crates/ast-bench
git commit -m "ast-bench: compare against a base commit on the same machine"
```

---

### Task 13: `sweep`, `snapshot` and the ratio warning

**Files:**
- Create: `crates/ast-bench/src/sweep.rs`, `docs/perf/README.md`
- Modify: `crates/ast-bench/src/main.rs`, `crates/ast-bench/src/compare.rs`

**Interfaces:**
- Produces:
  - `sweep`: every section of the three core oracles plus `bench.oracle`, N ∈ {100, 100 000}, `--engine both` (requires `c-oracle`); writes `target/perf/sweep-<UTC date>.tsv` and prints the 20 worst Rust/C ratios; untransformable fixtures are listed with their reason at the end, counted.
  - `snapshot --set S`: writes `docs/perf/<os>-<arch>.tsv` (`std::env::consts::{OS, ARCH}`, e.g. `macos-aarch64.tsv`): comment lines `# commit=… date=… rustc=… c=…`, then `case dir n rust_ns c_ns ratio`.
  - `compare` (both forms) additionally loads `docs/perf/<os>-<arch>.tsv` when present and, for a `--engine both` run, prints a warning (never an exit code) for each ratio worse than the snapshot's by more than `SLOWDOWN_THRESHOLD`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn ratio_warning_is_advisory() {
    let snap = parse_snapshot("# commit=x\na.map\tforward\t100\t10\t10\t1.0\n").unwrap();
    let w = ratio_warnings(&snap, &[("a.map", "forward", 100, 1.5)]);
    assert_eq!(w.len(), 1);
}
```

- [ ] **Step 2: Run to verify failure, implement, run to pass**

Run: `cargo test -p ast-bench sweep:: compare::`
Expected: fail, then pass after implementing `parse_snapshot`, `ratio_warnings`, `sweep` and `snapshot`.
`docs/perf/README.md` states that the snapshots are per platform, informative only, and how to regenerate one.

- [ ] **Step 3: Produce the first snapshot**

Run: `cargo run --release -q -p ast-bench --features c-oracle -- snapshot --set bench`
Expected: `docs/perf/macos-aarch64.tsv` written.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-bench docs/perf
git commit -m "ast-bench: full sweep, committed ratio snapshots, advisory ratio check"
```

---

### Task 14: Housekeeping

**Files:**
- Delete: `crates/ast-core/examples/bench_per_class.rs`, `crates/ast-core/tests/bench_per_class_c.c`
- Keep: `crates/ast-core/examples/bench_polymap_simd.rs` and the `simd-eval` feature, for the later SIMD investigation
- Modify: `.github/workflows/ci.yml` (the `test` job also runs `cargo test -p ast-bench`, which needs no submodule and no C), `docs/issues/mapping-perf-vs-c-oracle.md` (top: a pointer to `docs/perf/macos-aarch64.tsv` and `ast-bench`), `PLAN.md` (the per-class performance row and the tier-5 row), `CLAUDE.md` ("Bulk-transform optimisation": prototype comparisons now use `ast-bench run --engine both`)

- [ ] **Step 1: Confirm coverage before deleting**

Every variant name in `bench_per_class.rs:73-231` has a fixture in Task 9's table except `dssmap_typical` (documented).
`bench_polymap_simd.rs` stays: SIMD is excluded from the parity comparison, not abandoned.
Add a `PLAN.md` row: SIMD comparison deferred until the C side's SIMD work (https://github.com/Starlink/ast/pull/51) lands, then compared with `ast-bench run --engine both` against a C engine built from that branch.

- [ ] **Step 2: Delete, update, verify**

Run: `cargo test -p ast-bench && cargo build -p ast-core --examples && cargo clippy --workspace --tests --examples -- -D warnings && rg -n 'bench_per_class' --glob '!target' --glob '!docs/superpowers/*' .`
Expected: clean; the `rg` finds only historical mentions in `docs/issues/` (leave those; they describe past snapshots).

- [ ] **Step 3: Commit**

```bash
git add -A crates/ast-core/examples crates/ast-core/tests/bench_per_class_c.c .github/workflows/ci.yml docs/issues/mapping-perf-vs-c-oracle.md PLAN.md CLAUDE.md
git commit -m "Retire the hand-run per-class bench in favor of ast-bench"
```

---

### Task 15: Phase 2 — the first full sweep (overnight)

- [ ] **Step 1: Start it**

Run in the background: `cargo run --release -q -p ast-bench --features c-oracle -- sweep > <scratchpad>/sweep.log 2>&1`

- [ ] **Step 2: Report**

When it finishes, report to the user: the 20 worst ratios, every case worse than 3×, and any case that errored.
Propose which to add to a new curated set `sets/framesets.txt` (the large series and parallel FrameSets, plus anything unusually slow), each with its one-line reason; add them after the user agrees, and commit.
