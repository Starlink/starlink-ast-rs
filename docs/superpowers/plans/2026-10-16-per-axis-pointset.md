# Per-axis PointSet Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Store `PointSet` one array per axis and make every transform read and write through borrowed per-axis views, so a C-shaped caller pays no conversion and the cheap kernels run at C speed or better.

**Architecture:** A bit-identity gate and a C-caller timing go into `ast-bench` first, against the unchanged code.
Then the new view types land, `PointSet` storage flips to `(ncoord, npoint)`, and the `Mapping` trait takes views; both of those changes keep every result bit-identical.
Last, six cheap kernels and CmpMap are rewritten as loops down whole axes, each gated on bits and on `ast-bench compare --base`.

**Tech Stack:** Rust 2024, ndarray 0.16, smallvec 1 (new dependency of `ast-core`, already in `Cargo.lock`), `ast-bench`.

**Spec:** `docs/superpowers/specs/2026-10-16-per-axis-pointset-design.md`

## Global Constraints

- Never push; never stage `reference/starlink-ast`.
- Commit through `$S/cg.sh git commit` where `S=/private/tmp/claude-501/-Users-timj-work-lsst-starlink-astrs/15dbb8eb-1717-4ce7-bbc1-3b8bdf4071a4/scratchpad`; every cargo command also runs through `$S/cg.sh` (the conda environment breaks links and hooks otherwise).
- Every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean, with and without `--features c-oracle` on `ast-bench` (run the two clippy commands separately, not in a shell loop).
- `AST__BAD` is `BAD_VALUE` (`-f64::MAX`); outputs never contain NaN from the public transform path.
- Results stay bit-identical to the commit that ends Task 1: `ast-bench bits --base <T1>` reports no difference after every later task.
- No tolerance, threshold or iteration count without a C analogue; loop tiling that cannot change results is allowed and says so in a comment.
- Comments describe the code as it is, never its history.
- Prose: one sentence per line, American spelling.
- The inline axis capacity is 8 (`INLINE_AXES`); the corpora use at most 5 axes.

## Review Focus

1. A transform of zero points: must succeed and still validate the Mapping (direction, axis counts), as it does today. Pinned in Task 3 (views) and Task 9 (CmpMap series runs its stages once for zero points).
2. An output buffer with more points than the input: `validate_transform` allows it, and kernels must write only the first `npoint` values of each output axis. Pinned in Task 6.
3. NaN and ±infinity inputs to rewritten kernels: `is_bad` treats every non-finite value as bad, so each must give `BAD_VALUE`, never NaN. Pinned in Task 6.
4. A parallel CmpMap whose first child has different input and output axis counts (a PermMap with constants): the output must split at the first child's output count, not its input count. Pinned in Task 9.
5. A series CmpMap over a point count that is not a multiple of the 8192-point batch: the last, partial batch must use only its own points. Pinned by the `bits` gate at 10 000 points (Task 1) and in Task 9.

## File Structure

- Create `crates/ast-bench/src/bits.rs`: hashes of every corpus transform's output bits, and their comparison with a base commit.
- Create `crates/ast-core/src/coords/points.rs`: `PointBuf`, `Points`, `PointsMut`, `PointShape`.
- Create `crates/ast-core/tests/points.rs` (in the `meta` area): view tests.
- Modify `crates/ast-core/src/coords/pointset.rs`: storage flip and new accessors.
- Modify `crates/ast-core/src/mappings/mod.rs`: the trait, `validate_transform`, `tran1`, `tran2`, `transform_nan_protocol`.
- Modify every file that implements `transform_into` (41 in `ast-core/src`), every `point_mut` caller (121 sites) and every `view()` / `view_mut()` caller (51 sites).
- Modify `crates/ast-bench/src/{run.rs,report.rs,main.rs,base.rs,lib.rs}` and `crates/ast-bench/README.md`.

---

### Task 1: The bit-identity gate

**Files:**
- Create: `crates/ast-bench/src/bits.rs`
- Modify: `crates/ast-bench/src/lib.rs`, `crates/ast-bench/src/main.rs`, `crates/ast-bench/src/base.rs`, `crates/ast-bench/README.md`

**Interfaces:**
- Produces: `ast_bench::bits::{BITS_N, case_bits, all_bits, diff_bits}`; CLI `ast-bench bits` (TSV `key\tplain\tnan` per case) and `ast-bench bits --base <git-ref>`; `base::build_base` made `pub(crate)`.

- [ ] **Step 1: Write the failing tests** in `crates/ast-bench/src/bits.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bits_have_no_difference() {
        let a = "x.map forward #0\t00ff\t00fe\n";
        assert!(diff_bits(a, a).unwrap().is_empty());
    }

    #[test]
    fn a_changed_hash_is_a_difference() {
        let base = "x.map forward #0\t00ff\t00fe\n";
        let cur = "x.map forward #0\t00ff\t00fd\n";
        let d = diff_bits(base, cur).unwrap();
        assert_eq!(d.len(), 1);
        assert!(d[0].contains("x.map forward #0"), "{d:?}");
    }

    #[test]
    fn a_case_in_only_one_side_is_a_difference() {
        let base = "x.map forward #0\t00ff\t00fe\n";
        let cur = "";
        assert_eq!(diff_bits(base, cur).unwrap().len(), 1);
    }

    #[test]
    fn a_bench_case_hashes_the_same_twice_and_crosses_a_batch() {
        assert!(BITS_N > 8192 && BITS_N % 8192 != 0);
        let cases = crate::cases::load_set("bench").unwrap();
        let c = cases
            .iter()
            .find(|c| c.section.relpath == "cmpmap_series.map")
            .unwrap();
        assert_eq!(case_bits(c), case_bits(c));
        assert!(case_bits(c).0.len() == 16, "{:?}", case_bits(c));
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `$S/cg.sh cargo test -q -p ast-bench bits::`
Expected: compile errors, `diff_bits`, `case_bits`, `BITS_N` not found.

- [ ] **Step 3: Implement `bits.rs`**

```rust
//! A hash of the output bits of every corpus transform, so a change that
//! must not alter results can be checked against a base commit exactly.
//!
//! Hashes are platform-specific (libm differs), so they are compared with a
//! base commit built on the same machine, never with a committed file.

use std::collections::BTreeMap;

use ast_core::coords::PointSet;

use crate::cases::Case;
use crate::load::{Loaded, load_fixture};
use crate::oracle::Section;

/// Points per case: every oracle row, repeated to more than one 8192-point
/// series batch and ending part-way through the second.
pub const BITS_N: usize = 10_000;

/// FNV-1a over the bits of every output value, point by point.
fn hash(out: &PointSet) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |x: u64| {
        for b in x.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(out.npoint() as u64);
    eat(out.ncoord() as u64);
    for i in 0..out.npoint() {
        for &v in out.point(i).iter() {
            eat(v.to_bits());
        }
    }
    format!("{h:016x}")
}

fn rows(s: &Section) -> PointSet {
    let n = s.npoint();
    let idx: Vec<usize> = (0..BITS_N).map(|i| i % n).collect();
    crate::points::rows_to_pointset(s, &idx)
}

/// The hashes of `case` through `transform` and through
/// `transform_nan_protocol`; a case that cannot load or transform gives the
/// reason in place of a hash, so a change in failures is a difference too.
pub fn case_bits(case: &Case) -> (String, String) {
    let s = &case.section;
    let obj = match load_fixture(&case.root, &s.relpath) {
        Ok(Loaded::Mapping(obj)) => obj,
        Ok(Loaded::Untransformable(why)) => return (format!("untransformable: {why}"), String::new()),
        Err(e) => return (format!("load error: {e}"), String::new()),
    };
    let map = obj.as_mapping().expect("loaded as a Mapping");
    let pts = rows(s);
    let plain = match map.transform(&pts, s.forward) {
        Ok(out) => hash(&out),
        Err(e) => format!("error {:?}", e.code()),
    };
    let nan = match map.transform_nan_protocol(&pts, s.forward) {
        Ok(out) => hash(&out),
        Err(e) => format!("error {:?}", e.code()),
    };
    (plain, nan)
}

/// `key\tplain\tnan` for every case of every oracle, in a fixed order. The
/// key numbers repeated sections so each is distinct.
pub fn all_bits() -> Result<String, String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = String::new();
    for case in crate::sweep::all_cases()? {
        let s = &case.section;
        let base = format!("{} {}", s.relpath, if s.forward { "forward" } else { "inverse" });
        let k = seen.entry(base.clone()).or_default();
        let key = format!("{base} #{k}");
        *k += 1;
        let (plain, nan) = case_bits(&case);
        out += &format!("{key}\t{plain}\t{nan}\n");
    }
    Ok(out)
}

fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.split_once('\t')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("bad bits line {l:?}"))
        })
        .collect()
}

/// Every case whose hashes differ between `base` and `cur`, or that is in
/// only one of them.
pub fn diff_bits(base: &str, cur: &str) -> Result<Vec<String>, String> {
    let (b, c) = (parse(base)?, parse(cur)?);
    let mut out = Vec::new();
    for (k, bv) in &b {
        match c.get(k) {
            None => out.push(format!("only in the base: {k}")),
            Some(cv) if cv != bv => out.push(format!("{k}: {bv} -> {cv}")),
            Some(_) => {}
        }
    }
    for k in c.keys().filter(|k| !b.contains_key(*k)) {
        out.push(format!("only in the current build: {k}"));
    }
    Ok(out)
}
```

Add `pub mod bits;` to `lib.rs` (alphabetical, after `pub mod base;`).
If `Section::npoint` does not exist, use `s.inputs[0].len()`.

- [ ] **Step 4: Add the CLI command.** In `base.rs`, make `build_base` `pub(crate)` and add:

```rust
/// The output of `ast-bench bits` from the build at `git_ref`.
pub fn bits_at(git_ref: &str) -> Result<String, String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bin = build_base(&repo, git_ref)?;
    let out = Command::new(&bin)
        .arg("bits")
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} bits failed ({}): {}",
            bin.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
```

In `main.rs`, add to `USAGE` the line `  bits [--base <git-ref>]`, and:

```rust
/// `bits`: print the output-bit hashes of every corpus transform, or with
/// `--base`, list every case whose hashes differ from that commit's.
fn bits(args: &[String]) -> Result<Vec<String>, String> {
    check_not_counting()?;
    let cur = ast_bench::bits::all_bits()?;
    match opt(args, "--base") {
        None => {
            print!("{cur}");
            Ok(Vec::new())
        }
        Some(git_ref) => {
            let base = ast_bench::base::bits_at(git_ref)?;
            let diff = ast_bench::bits::diff_bits(&base, &cur)?;
            if diff.is_empty() {
                println!("every transform's output bits match {git_ref}");
            }
            Ok(diff.into_iter().map(|d| format!("bits differ: {d}")).collect())
        }
    }
}
```

and the dispatch arm `Some("bits") => bits(&args[1..]),`.

- [ ] **Step 5: Run the tests**

Run: `$S/cg.sh cargo test -q -p ast-bench`
Expected: all pass, including the four new ones.

- [ ] **Step 6: Negative control.** Temporarily change `ZoomMap::transform_into`'s `v * scale` to `v * scale * (1.0 + f64::EPSILON)` in `crates/ast-core/src/mappings/zoom.rs`, commit nothing, and run `$S/cg.sh cargo run -q --release -p ast-bench -- bits --base HEAD`.
Expected: exit status 1 and `bits differ:` lines naming `zoommap_2d.map`.
Revert the change and run it again.
Expected: `every transform's output bits match HEAD`, exit status 0.

- [ ] **Step 7: README.** Add to `crates/ast-bench/README.md`, after the `compare` section:

```markdown
    ast-bench bits [--base <git-ref>]

prints a hash of the output bits of every transform in the sweep corpus, through `transform` and through the NaN protocol, at 10 000 points per case.
With `--base`, it builds `ast-bench` at that commit, runs the same command there, and fails on any case whose hashes differ.
Use it for changes that must not alter any result, such as a change of data layout.
The hashes depend on the platform's libm, so they are only ever compared on one machine, never committed.
```

- [ ] **Step 8: fmt, clippy, commit**

```bash
$S/cg.sh cargo fmt --all
$S/cg.sh cargo clippy --workspace --all-targets -- -D warnings
$S/cg.sh cargo clippy -p ast-bench --all-targets --features c-oracle -- -D warnings
git add crates/ast-bench
$S/cg.sh git commit -m "ast-bench: bits, a bit-identity gate against a base commit

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Record this commit's hash as `T1` in the ledger; every later task runs `ast-bench bits --base T1`.

---

### Task 2: A C-caller timing

**Files:**
- Modify: `crates/ast-bench/src/run.rs`, `crates/ast-bench/src/report.rs`, `crates/ast-bench/src/points.rs`, `crates/ast-bench/README.md`

**Interfaces:**
- Consumes: `rows_to_columns(s, &rows) -> Vec<f64>` (axis after axis).
- Produces: rows with `engine == "rust-cols"` whenever the Rust engine runs; `table` gains a `cols ns/pt` column.

- [ ] **Step 1: Write the failing tests.** In `run.rs` tests:

```rust
    #[test]
    fn the_rust_engine_also_times_a_c_shaped_caller() {
        let cases = load_set("bench").unwrap();
        let zoom: Vec<_> = cases
            .into_iter()
            .filter(|c| c.section.relpath == "zoommap_2d.map" && c.section.forward)
            .collect();
        let rows: Vec<_> = run_set(&zoom, Engines::Rust, &[10])
            .into_iter()
            .map(Result::unwrap)
            .collect();
        let engines: Vec<&str> = rows.iter().map(|r| r.engine.as_str()).collect();
        assert_eq!(engines, ["rust", "rust-cols"]);
    }
```

In `report.rs` tests:

```rust
    #[test]
    fn table_shows_the_c_shaped_timing_in_its_own_column() {
        let row = |engine: &str, ns: f64| Row {
            case: "a.map".into(),
            dir: "forward".into(),
            n: 10,
            engine: engine.into(),
            stats: crate::timing::Stats {
                median_ns_per_point: ns,
                min_ns_per_point: ns,
                samples: 7,
            },
        };
        let t = table(&[row("rust", 1.0), row("rust-cols", 4.0), row("c", 2.0)]);
        assert!(t.contains("cols ns/pt"), "{t}");
        let line = t.lines().nth(1).unwrap();
        assert!(line.contains("4.00") && line.contains("2.00"), "{t}");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `$S/cg.sh cargo test -q -p ast-bench run:: report::`
Expected: the run test fails with `["rust"]` against `["rust", "rust-cols"]`; the report test fails on the missing column.

- [ ] **Step 3: Implement.** In `run.rs`, build the columns unconditionally (drop the `#[cfg(feature = "c-oracle")]` on `let columns` and delete `let _ = rows_to_columns;`), and add after `time_rust`:

```rust
            // A caller holding one array per axis, as C's astTranN takes
            // them: any conversion the port needs is inside the timing.
            let mut cols_out = vec![0.0; s.nout * n];
            let mut time_cols = || -> Result<(), String> {
                let fail = |e: ast_core::error::AstError| format!("{} {dir} N={n}: {e}", s.relpath);
                let axes: Vec<&[f64]> = columns.chunks_exact(n).collect();
                let pts = PointSet::from_columns(&axes).map_err(fail)?;
                let mut o = PointSet::zeros(n, s.nout);
                map.transform_into(&pts, s.forward, &mut o).map_err(fail)?;
                for (j, dst) in cols_out.chunks_exact_mut(n).enumerate() {
                    for (d, v) in dst.iter_mut().zip(o.axis_iter(j)) {
                        *d = v;
                    }
                }
                Ok(())
            };
```

and make the engine table:

```rust
                Engines::Rust => (vec![&mut time_rust, &mut time_cols], vec!["rust", "rust-cols"]),
                #[cfg(feature = "c-oracle")]
                Engines::C => (vec![&mut time_c], vec!["c"]),
                #[cfg(feature = "c-oracle")]
                Engines::Both => (
                    vec![&mut time_rust, &mut time_cols, &mut time_c],
                    vec!["rust", "rust-cols", "c"],
                ),
```

If `ast_core::error::AstError` is not the public path, use the path `crates/ast-core/src/lib.rs` re-exports.
In `report.rs`, make the per-key value a triple `(rust, cols, c)`, match `"rust"`, `"rust-cols"` and `"c"` explicitly (any other engine name is a programming error: `unreachable!("unknown engine {other}")`), and print header `"case", "dir", "n", "rust ns/pt", "cols ns/pt", "c ns/pt", "rust/c"` with one more `{:>12}` cell.
`snapshot.rs` filters by exact engine name already, so its ratios are unaffected.

- [ ] **Step 4: Run the tests**

Run: `$S/cg.sh cargo test -q -p ast-bench`
Expected: all pass.

- [ ] **Step 5: See the number.**

Run: `$S/cg.sh cargo run -q --release -p ast-bench -- run --set bench --engine rust | rg zoommap_2d`
Expected: at N=1000000 the `cols ns/pt` value is about 3 to 4 and `rust ns/pt` about 1.5.

- [ ] **Step 6: README.** In `crates/ast-bench/README.md`'s engines section add:

```markdown
Whenever the Rust engine runs, it also times `rust-cols`: the same transform called the way a C caller of `astTranN` calls it, with one input array per axis and one output array per axis, and any conversion the port needs inside the timing.
`rust-cols` against `c` is what a C program would see on switching libraries.
```

and note under `compare --base` that a base commit older than this one has no `rust-cols` rows, so `compare --base` from here on needs a base at or after this commit.

- [ ] **Step 7: fmt, clippy (both feature sets), commit** with message `ast-bench: time a C-shaped caller as rust-cols`.
Then save fresh local baselines, since the rows changed: `$S/cg.sh cargo run -q --release -p ast-bench -- run --set bench --save`.

---

### Task 3: `PointBuf`, `Points` and `PointsMut`

**Files:**
- Create: `crates/ast-core/src/coords/points.rs`, `crates/ast-core/tests/points.rs`
- Modify: `crates/ast-core/src/coords/mod.rs`, `crates/ast-core/Cargo.toml`, `Cargo.toml` (workspace dependency), `crates/ast-core/tests/meta/main.rs`

**Interfaces:**
- Produces, all re-exported from `ast_core::coords`: `INLINE_AXES: usize = 8`; `PointBuf`; `Points<'a>`; `PointsMut<'a>`; `pub(crate) trait PointShape { fn npoint(&self) -> usize; fn ncoord(&self) -> usize; }`.
- `Points<'a>`: `new(impl IntoIterator<Item = &'a [f64]>) -> AstResult<Self>`, `from_strided(&'a [f64], ncoord, npoint, stride) -> AstResult<Self>`, `pub(crate) from_parts(SmallVec<[&'a [f64]; INLINE_AXES]>, npoint) -> Self`, `npoint()`, `ncoord()`, `axis(j) -> &'a [f64]`, `point(i) -> PointBuf`, `points(Range<usize>) -> Points<'a>`, `axes(Range<usize>) -> Points<'a>`.
- `PointsMut<'a>`: `new(impl IntoIterator<Item = &'a mut [f64]>) -> AstResult<Self>`, `from_strided(&'a mut [f64], ncoord, npoint, stride) -> AstResult<Self>`, `pub(crate) from_parts(..)`, `npoint()`, `ncoord()`, `axis(j) -> &[f64]`, `axis_mut(j) -> &mut [f64]`, `point(i) -> PointBuf`, `set_point(i, &[f64])`, `copy_from(&Points<'_>)`, `as_points(&self) -> Points<'_>`, `reborrow(&mut self) -> PointsMut<'_>`, `points_mut(&mut self, Range<usize>) -> PointsMut<'_>`, `split_axes_at(self, k) -> (PointsMut<'a>, PointsMut<'a>)`.

- [ ] **Step 1: Add the dependency.** In the workspace `Cargo.toml` `[workspace.dependencies]` add `smallvec = "1"`; in `crates/ast-core/Cargo.toml` `[dependencies]` add `smallvec.workspace = true`.

- [ ] **Step 2: Write the failing tests** in `crates/ast-core/tests/points.rs` and add to `crates/ast-core/tests/meta/main.rs`:

```rust
#[path = "../points.rs"]
mod points;
```

```rust
//! Borrowed per-axis views of points.

use ast_core::coords::{PointBuf, Points, PointsMut};
use ast_core::error::ErrorCode;

#[test]
fn a_view_over_columns_reads_points_and_axes() {
    let (x, y) = ([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
    let p = Points::new([&x[..], &y[..]]).unwrap();
    assert_eq!((p.npoint(), p.ncoord()), (3, 2));
    assert_eq!(p.point(1), &[2.0, 5.0]);
    assert_eq!(p.axis(1), &[4.0, 5.0, 6.0]);
}

#[test]
fn no_axes_is_ncoin() {
    let e = Points::new(std::iter::empty::<&[f64]>()).unwrap_err();
    assert_eq!(e.code(), ErrorCode::Ncoin);
}

#[test]
fn one_short_axis_is_ncpin() {
    let (x, y) = ([1.0, 2.0, 3.0], [4.0, 5.0]);
    let e = Points::new([&x[..], &y[..]]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::Ncpin);
}

#[test]
fn the_trann_layout_reads_axes_at_a_stride() {
    // Two axes of 3 points in a buffer whose axis dimension (indim) is 5.
    let buf = [1.0, 2.0, 3.0, -9.0, -9.0, 4.0, 5.0, 6.0];
    let p = Points::from_strided(&buf, 2, 3, 5).unwrap();
    assert_eq!(p.axis(0), &[1.0, 2.0, 3.0]);
    assert_eq!(p.axis(1), &[4.0, 5.0, 6.0]);
}

#[test]
fn a_stride_below_npoint_is_dimin() {
    let buf = [0.0; 8];
    assert_eq!(Points::from_strided(&buf, 2, 3, 2).unwrap_err().code(), ErrorCode::Dimin);
    let mut buf = [0.0; 8];
    assert_eq!(PointsMut::from_strided(&mut buf, 2, 3, 2).unwrap_err().code(), ErrorCode::Dimin);
}

#[test]
fn a_buffer_too_short_for_its_stride_is_dimin() {
    let buf = [0.0; 7];
    assert_eq!(Points::from_strided(&buf, 2, 3, 5).unwrap_err().code(), ErrorCode::Dimin);
}

#[test]
fn writable_strided_views_write_only_their_points() {
    let mut buf = [0.0; 8];
    {
        let mut o = PointsMut::from_strided(&mut buf, 2, 3, 5).unwrap();
        o.set_point(1, &[7.0, 8.0]);
        o.axis_mut(0)[2] = 9.0;
    }
    assert_eq!(buf, [0.0, 7.0, 9.0, 0.0, 0.0, 0.0, 8.0, 0.0]);
}

#[test]
fn point_and_axis_ranges_are_views_of_the_same_data() {
    let (x, y, z) = ([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]);
    let p = Points::new([&x[..], &y[..], &z[..]]).unwrap();
    let q = p.points(1..3).axes(1..3);
    assert_eq!((q.npoint(), q.ncoord()), (2, 2));
    assert_eq!(q.point(0), &[5.0, 8.0]);
}

#[test]
fn splitting_writable_axes_gives_disjoint_views() {
    let (mut a, mut b, mut c) = ([0.0; 2], [0.0; 2], [0.0; 2]);
    let o = PointsMut::new([&mut a[..], &mut b[..], &mut c[..]]).unwrap();
    let (mut left, mut right) = o.split_axes_at(1);
    left.axis_mut(0)[0] = 1.0;
    right.axis_mut(1)[1] = 2.0;
    assert_eq!((a, b, c), ([1.0, 0.0], [0.0; 2], [0.0, 2.0]));
}

#[test]
fn zero_points_is_a_valid_view() {
    let (x, y): ([f64; 0], [f64; 0]) = ([], []);
    let p = Points::new([&x[..], &y[..]]).unwrap();
    assert_eq!((p.npoint(), p.ncoord()), (0, 2));
}

#[test]
fn nine_axes_spill_to_the_heap_with_the_same_results() {
    let cols: Vec<Vec<f64>> = (0..9).map(|j| vec![j as f64; 2]).collect();
    let p = Points::new(cols.iter().map(Vec::as_slice)).unwrap();
    assert_eq!(p.point(1), (0..9).map(f64::from).collect::<Vec<_>>());
}

#[test]
fn point_bufs_compare_with_slices_arrays_and_vectors() {
    let b: PointBuf = [1.0, 2.0].into_iter().collect();
    assert_eq!(b, [1.0, 2.0]);
    assert_eq!(b, &[1.0, 2.0]);
    assert_eq!(b, &[1.0, 2.0][..]);
    assert_eq!(b, vec![1.0, 2.0]);
    assert_eq!(b[1], 2.0);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `$S/cg.sh cargo test -q -p ast-core --test meta points::`
Expected: compile errors, `Points`, `PointsMut`, `PointBuf` not found in `ast_core::coords`.

- [ ] **Step 4: Implement `crates/ast-core/src/coords/points.rs`**

```rust
//! Borrowed per-axis views of point coordinates.
//!
//! A view holds one slice per axis, all of the same length, the layout C's
//! `astTranN` passes and C's PointSet stores. Transforms read a [`Points`]
//! and write a [`PointsMut`], so a caller's own arrays can be transformed
//! without copying.

use std::ops::{Deref, DerefMut, Range};

use smallvec::SmallVec;

use crate::error::{AstError, AstResult, ErrorCode};

/// Axis counts up to this are held without allocating; the transform oracle
/// corpora use at most 5 axes.
pub const INLINE_AXES: usize = 8;

/// One point's coordinates, copied out of a PointSet or a view.
#[derive(Clone, Debug, Default)]
pub struct PointBuf(SmallVec<[f64; INLINE_AXES]>);

impl PointBuf {
    /// A point of `ncoord` zeros.
    pub fn zeros(ncoord: usize) -> Self {
        Self(SmallVec::from_elem(0.0, ncoord))
    }
}

impl Deref for PointBuf {
    type Target = [f64];
    fn deref(&self) -> &[f64] {
        &self.0
    }
}

impl DerefMut for PointBuf {
    fn deref_mut(&mut self) -> &mut [f64] {
        &mut self.0
    }
}

impl FromIterator<f64> for PointBuf {
    fn from_iter<I: IntoIterator<Item = f64>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl PartialEq for PointBuf {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl PartialEq<[f64]> for PointBuf {
    fn eq(&self, other: &[f64]) -> bool {
        self[..] == *other
    }
}

impl PartialEq<&[f64]> for PointBuf {
    fn eq(&self, other: &&[f64]) -> bool {
        self[..] == **other
    }
}

impl<const N: usize> PartialEq<[f64; N]> for PointBuf {
    fn eq(&self, other: &[f64; N]) -> bool {
        self[..] == other[..]
    }
}

impl<const N: usize> PartialEq<&[f64; N]> for PointBuf {
    fn eq(&self, other: &&[f64; N]) -> bool {
        self[..] == other[..]
    }
}

impl PartialEq<Vec<f64>> for PointBuf {
    fn eq(&self, other: &Vec<f64>) -> bool {
        self[..] == other[..]
    }
}

/// The shape of a set of points, for checking it against a Mapping.
pub(crate) trait PointShape {
    fn npoint(&self) -> usize;
    fn ncoord(&self) -> usize;
}

/// The common length of `lens`, or C's error for none or for a mismatch.
fn common_length(lens: impl IntoIterator<Item = usize>) -> AstResult<usize> {
    let mut lens = lens.into_iter();
    let Some(npoint) = lens.next() else {
        return Err(AstError::new(ErrorCode::Ncoin, "a set of points needs at least one axis"));
    };
    for (j, len) in lens.enumerate() {
        if len != npoint {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!("axis {} has {len} points but axis 1 has {npoint}", j + 2),
            ));
        }
    }
    Ok(npoint)
}

/// The checks `astTranN` makes on its array dimension (mapping.c `TranN`),
/// plus that `len` holds every axis.
fn check_stride(len: usize, ncoord: usize, npoint: usize, stride: usize) -> AstResult<usize> {
    if ncoord == 0 {
        return Err(AstError::new(ErrorCode::Ncoin, "a set of points needs at least one axis"));
    }
    if stride < npoint {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!("the array dimension ({stride}) is less than the number of points ({npoint})"),
        ));
    }
    let need = (ncoord - 1) * stride + npoint;
    if len < need {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!("{ncoord} axes at a dimension of {stride} need {need} values; the array has {len}"),
        ));
    }
    Ok(need)
}

/// A read-only view of `npoint` points, one slice per axis.
#[derive(Clone, Debug)]
pub struct Points<'a> {
    axes: SmallVec<[&'a [f64]; INLINE_AXES]>,
    npoint: usize,
}

impl<'a> Points<'a> {
    /// A view of one slice per axis; every slice must have the same length.
    pub fn new(axes: impl IntoIterator<Item = &'a [f64]>) -> AstResult<Self> {
        let axes: SmallVec<[&'a [f64]; INLINE_AXES]> = axes.into_iter().collect();
        let npoint = common_length(axes.iter().map(|a| a.len()))?;
        Ok(Self { axes, npoint })
    }

    /// A view of `ncoord` axes of `npoint` points held one after another in
    /// `data`, axis `j` starting at `j * stride`: C's `astTranN` layout,
    /// whose `indim` is `stride`.
    pub fn from_strided(data: &'a [f64], ncoord: usize, npoint: usize, stride: usize) -> AstResult<Self> {
        check_stride(data.len(), ncoord, npoint, stride)?;
        let axes = (0..ncoord).map(|j| &data[j * stride..j * stride + npoint]).collect();
        Ok(Self { axes, npoint })
    }

    /// A view whose axes the caller knows are all `npoint` long.
    pub(crate) fn from_parts(axes: SmallVec<[&'a [f64]; INLINE_AXES]>, npoint: usize) -> Self {
        debug_assert!(axes.iter().all(|a| a.len() == npoint));
        Self { axes, npoint }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.npoint
    }

    /// Number of axes.
    pub fn ncoord(&self) -> usize {
        self.axes.len()
    }

    /// Axis `j` (0-based).
    pub fn axis(&self, j: usize) -> &'a [f64] {
        self.axes[j]
    }

    /// A copy of point `i`.
    pub fn point(&self, i: usize) -> PointBuf {
        self.axes.iter().map(|a| a[i]).collect()
    }

    /// Points `range` of every axis.
    pub fn points(&self, range: Range<usize>) -> Points<'a> {
        let npoint = range.len();
        Points {
            axes: self.axes.iter().map(|a| &a[range.clone()]).collect(),
            npoint,
        }
    }

    /// Axes `range` (0-based).
    pub fn axes(&self, range: Range<usize>) -> Points<'a> {
        Points {
            axes: self.axes[range].iter().copied().collect(),
            npoint: self.npoint,
        }
    }
}

impl PointShape for Points<'_> {
    fn npoint(&self) -> usize {
        self.npoint
    }
    fn ncoord(&self) -> usize {
        self.axes.len()
    }
}

/// A writable view of `npoint` points, one slice per axis.
#[derive(Debug)]
pub struct PointsMut<'a> {
    axes: SmallVec<[&'a mut [f64]; INLINE_AXES]>,
    npoint: usize,
}

impl<'a> PointsMut<'a> {
    /// A view of one slice per axis; every slice must have the same length.
    pub fn new(axes: impl IntoIterator<Item = &'a mut [f64]>) -> AstResult<Self> {
        let axes: SmallVec<[&'a mut [f64]; INLINE_AXES]> = axes.into_iter().collect();
        let npoint = common_length(axes.iter().map(|a| a.len()))?;
        Ok(Self { axes, npoint })
    }

    /// The writable form of [`Points::from_strided`].
    pub fn from_strided(data: &'a mut [f64], ncoord: usize, npoint: usize, stride: usize) -> AstResult<Self> {
        let need = check_stride(data.len(), ncoord, npoint, stride)?;
        let mut axes = SmallVec::new();
        let mut rest = &mut data[..need];
        for j in 0..ncoord {
            let take = if j + 1 < ncoord { stride } else { npoint };
            let (chunk, tail) = std::mem::take(&mut rest).split_at_mut(take);
            axes.push(&mut chunk[..npoint]);
            rest = tail;
        }
        Ok(Self { axes, npoint })
    }

    /// A view whose axes the caller knows are all `npoint` long.
    pub(crate) fn from_parts(axes: SmallVec<[&'a mut [f64]; INLINE_AXES]>, npoint: usize) -> Self {
        debug_assert!(axes.iter().all(|a| a.len() == npoint));
        Self { axes, npoint }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.npoint
    }

    /// Number of axes.
    pub fn ncoord(&self) -> usize {
        self.axes.len()
    }

    /// Axis `j` (0-based).
    pub fn axis(&self, j: usize) -> &[f64] {
        self.axes[j]
    }

    /// Axis `j` (0-based), writable.
    pub fn axis_mut(&mut self, j: usize) -> &mut [f64] {
        self.axes[j]
    }

    /// A copy of point `i`.
    pub fn point(&self, i: usize) -> PointBuf {
        self.axes.iter().map(|a| a[i]).collect()
    }

    /// Set point `i`; `coords` must have one value per axis.
    pub fn set_point(&mut self, i: usize, coords: &[f64]) {
        assert_eq!(coords.len(), self.axes.len(), "set_point needs one value per axis");
        for (a, &v) in self.axes.iter_mut().zip(coords) {
            a[i] = v;
        }
    }

    /// Copy `src` into the first `src.npoint()` points of each axis.
    pub fn copy_from(&mut self, src: &Points<'_>) {
        assert_eq!(src.ncoord(), self.ncoord(), "copy_from needs the same number of axes");
        let n = src.npoint();
        for (j, a) in self.axes.iter_mut().enumerate() {
            a[..n].copy_from_slice(src.axis(j));
        }
    }

    /// A read-only view of the same points.
    pub fn as_points(&self) -> Points<'_> {
        Points {
            axes: self.axes.iter().map(|a| &**a).collect(),
            npoint: self.npoint,
        }
    }

    /// A shorter-lived writable view of the same points.
    pub fn reborrow(&mut self) -> PointsMut<'_> {
        PointsMut {
            axes: self.axes.iter_mut().map(|a| &mut **a).collect(),
            npoint: self.npoint,
        }
    }

    /// Points `range` of every axis.
    pub fn points_mut(&mut self, range: Range<usize>) -> PointsMut<'_> {
        let npoint = range.len();
        PointsMut {
            axes: self.axes.iter_mut().map(|a| &mut a[range.clone()]).collect(),
            npoint,
        }
    }

    /// The first `k` axes and the rest, as two views.
    pub fn split_axes_at(self, k: usize) -> (PointsMut<'a>, PointsMut<'a>) {
        let npoint = self.npoint;
        let mut axes = self.axes.into_iter();
        let left: SmallVec<_> = axes.by_ref().take(k).collect();
        let right: SmallVec<_> = axes.collect();
        (PointsMut { axes: left, npoint }, PointsMut { axes: right, npoint })
    }
}

impl PointShape for PointsMut<'_> {
    fn npoint(&self) -> usize {
        self.npoint
    }
    fn ncoord(&self) -> usize {
        self.axes.len()
    }
}
```

In `coords/mod.rs` add `mod points;` and `pub use points::{INLINE_AXES, PointBuf, Points, PointsMut};` and `pub(crate) use points::PointShape;`.
If `ErrorCode::Dimin` is spelled differently in the generated enum, use the variant generated from `DIMIN` in `vendor/ast_err.msg`.
`PointShape` and `from_parts` are unused until Task 4; mark both `#[allow(dead_code)]` only if the build fails on them, and remove the attribute in Task 4.

- [ ] **Step 5: Run the tests**

Run: `$S/cg.sh cargo test -q -p ast-core --test meta points::`
Expected: 12 passed.

- [ ] **Step 6: fmt, clippy, commit** with message `Add per-axis point views: Points, PointsMut, PointBuf`.

---

### Task 4: Flip `PointSet` to one array per axis

**Files:**
- Modify: `crates/ast-core/src/coords/pointset.rs`
- Modify every `point_mut` caller: `crates/ast-core/src/frames/{cmpframe.rs,mod.rs,skyframe.rs}`, `crates/ast-core/src/mappings/{dssmap,grismmap,linear_approx,lutmap,map_box,matrix,normmap,pcdmap,perm,polymap,quad_approx,ratemap,selectormap,shift,slamap,specmap,sphmap,splinemap,switchmap,timemap,unitnormmap,wcsmap,winmap,xphmap,zoom}.rs`, `crates/ast-core/src/mappings/mathmap/mod.rs`, `crates/ast-core/src/regions/{box_region,circle,cmpregion,data,ellipse,interval,moc,mod,nullregion,overlap,pointlist,polygon,prism,stc}.rs`, `crates/ast-bench/src/load.rs`, and the tests `crates/ast-core/tests/{common/mod.rs,pointset.rs,test_box_region.rs,test_circle.rs,test_intramap.rs,test_polymap.rs,test_transform_memory.rs}`.
- Modify every `view()` / `view_mut()` / `from_ndarray` / `into_ndarray` caller: `crates/ast-core/src/fitschan/fset_to_store/world_axes.rs`, `crates/ast-core/src/fitschan/wcs_decompose.rs`, `crates/ast-core/src/mappings/{cmpmap,dssmap,lutmap,mod,polymap_simd,polymap,ratemap,selectormap,specmap,splinemap,switchmap,timemap,tran_grid,unit,unitnormmap}.rs`, `crates/ast-core/src/mappings/{mathmap,rebin,resample}/mod.rs`, `crates/ast-core/tests/{pointset.rs,test_trangrid.rs}`.
- Any other compile error the flip exposes, including `crates/ast-oracle-tests/tests/call_count_parity.rs`.

**Interfaces:**
- Consumes: `PointBuf`, `Points::from_parts`, `PointsMut::from_parts` (Task 3).
- Produces on `PointSet`: `point(i) -> PointBuf`, `set_point(i, &[f64])`, `axis(j) -> &[f64]`, `axis_mut(j) -> &mut [f64]`, `axis_iter(j)`, `as_points() -> Points<'_>`, `as_points_mut() -> PointsMut<'_>`, `from_points(&Points<'_>) -> PointSet`; `view()`, `view_mut()`, `from_ndarray`, `into_ndarray` with shape `(ncoord, npoint)`; `point_mut` removed.
- The `Mapping` trait is unchanged in this task.

- [ ] **Step 1: Update the PointSet tests first.** In `crates/ast-core/tests/pointset.rs`, replace `point_mut_modifies_in_place` with:

```rust
#[test]
fn set_point_modifies_in_place() {
    let mut ps = PointSet::zeros(2, 3);
    ps.set_point(0, &[0.0, 42.0, 0.0]);
    assert_eq!(ps.point(0), &[0.0, 42.0, 0.0]);
}

#[test]
fn each_axis_is_one_contiguous_slice() {
    let ps = PointSet::from_rows([[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]);
    assert_eq!(ps.axis(0), &[1.0, 2.0, 3.0]);
    assert_eq!(ps.axis(1), &[10.0, 20.0, 30.0]);
    assert_eq!(ps.view().shape(), &[2, 3]);
}

#[test]
fn a_point_set_and_its_views_agree() {
    let mut ps = PointSet::from_rows([[1.0, 10.0], [2.0, 20.0]]);
    assert_eq!(ps.as_points().point(1), &[2.0, 20.0]);
    ps.as_points_mut().axis_mut(1)[0] = 5.0;
    assert_eq!(ps.point(0), &[1.0, 5.0]);
    let copy = PointSet::from_points(&ps.as_points());
    assert_eq!(copy.point(0), &[1.0, 5.0]);
}

#[test]
fn a_point_set_of_no_points_has_empty_axes() {
    let mut ps = PointSet::zeros(0, 2);
    assert_eq!(ps.as_points().ncoord(), 2);
    assert_eq!(ps.as_points_mut().npoint(), 0);
}
```

Fix the file's other `view()` uses for the `(ncoord, npoint)` shape.

- [ ] **Step 2: Run them to see them fail**

Run: `$S/cg.sh cargo test -q -p ast-core --test meta pointset::`
Expected: compile errors, `set_point`, `axis`, `as_points`, `from_points` not found.

- [ ] **Step 3: Rewrite `pointset.rs`.** Storage is `Array2<f64>` of shape `(ncoord, npoint)` in standard layout:

```rust
/// Frame-unaware coordinate block.
///
/// One contiguous array per axis, as C's PointSet stores them: the data is
/// an `(ncoord, npoint)` array in standard layout, so axis `j` is row `j`.
#[derive(Debug, Clone)]
pub struct PointSet {
    data: Array2<f64>,
}

impl PointSet {
    pub fn zeros(npoint: usize, ncoord: usize) -> Self {
        Self { data: Array2::zeros((ncoord, npoint)) }
    }

    /// Create from an iterator of points (each point is a slice of coordinates).
    ///
    /// All points must have the same length. Panics if the iterator is empty.
    /// When the iterator knows its exact length the points are written
    /// straight into place; otherwise they are gathered first and
    /// transposed, which needs twice the memory.
    pub fn from_rows(rows: impl IntoIterator<Item = impl AsRef<[f64]>>) -> Self {
        let mut rows = rows.into_iter();
        let exact = match rows.size_hint() {
            (lo, Some(hi)) if lo == hi => Some(lo),
            _ => None,
        };
        let first = rows.next().expect("PointSet::from_rows requires at least one row");
        let first = first.as_ref();
        let ncoord = first.len();
        let check = |row: &[f64]| {
            assert_eq!(row.len(), ncoord, "PointSet::from_rows needs every point to have {ncoord} coordinates");
        };
        if let Some(npoint) = exact {
            let mut out = Self::zeros(npoint, ncoord);
            out.set_point(0, first);
            for (i, row) in rows.enumerate() {
                let row = row.as_ref();
                check(row);
                out.set_point(i + 1, row);
            }
            return out;
        }
        let mut flat = first.to_vec();
        let mut npoint = 1;
        for row in rows {
            let row = row.as_ref();
            check(row);
            flat.extend_from_slice(row);
            npoint += 1;
        }
        let by_point = Array2::from_shape_vec((npoint, ncoord), flat).unwrap();
        Self { data: by_point.reversed_axes().as_standard_layout().into_owned() }
    }

    pub fn from_columns(cols: &[&[f64]]) -> AstResult<Self> {
        // (keep the existing Ncoin / Ncpin checks unchanged)
        let ncoord = cols.len();
        let npoint = cols[0].len();
        let mut data = Array2::zeros((ncoord, npoint));
        for (mut row, col) in data.rows_mut().into_iter().zip(cols) {
            row.as_slice_mut().expect("standard layout").copy_from_slice(col);
        }
        Ok(Self { data })
    }

    /// A copy of the points of a view.
    pub fn from_points(p: &Points<'_>) -> Self {
        let mut out = Self::zeros(p.npoint(), p.ncoord());
        out.as_points_mut().copy_from(p);
        out
    }

    /// Create from an `(ncoord, npoint)` array.
    pub fn from_ndarray(a: Array2<f64>) -> Self {
        Self { data: a.as_standard_layout().into_owned() }
    }

    pub fn npoint(&self) -> usize {
        self.data.ncols()
    }

    pub fn ncoord(&self) -> usize {
        self.data.nrows()
    }

    /// The `(ncoord, npoint)` array.
    pub fn view(&self) -> ArrayView2<'_, f64> {
        self.data.view()
    }

    /// The `(ncoord, npoint)` array, writable.
    pub fn view_mut(&mut self) -> ArrayViewMut2<'_, f64> {
        self.data.view_mut()
    }

    /// Consume and return the `(ncoord, npoint)` array.
    pub fn into_ndarray(self) -> Array2<f64> {
        self.data
    }

    /// A copy of point `i`. Panics if `i >= npoint()`.
    pub fn point(&self, i: usize) -> PointBuf {
        self.data.column(i).iter().copied().collect()
    }

    /// Set point `i`; `coords` must have one value per axis.
    pub fn set_point(&mut self, i: usize, coords: &[f64]) {
        assert_eq!(coords.len(), self.ncoord(), "set_point needs one value per axis");
        self.data.column_mut(i).iter_mut().zip(coords).for_each(|(d, &v)| *d = v);
    }

    /// Axis `j` (0-based).
    pub fn axis(&self, j: usize) -> &[f64] {
        self.data.row(j).to_slice().expect("standard layout")
    }

    /// Axis `j` (0-based), writable.
    pub fn axis_mut(&mut self, j: usize) -> &mut [f64] {
        self.data.row_mut(j).into_slice().expect("standard layout")
    }

    /// The values of axis `j` (0-based).
    pub fn axis_iter(&self, j: usize) -> impl Iterator<Item = f64> + '_ {
        self.axis(j).iter().copied()
    }

    /// A view of every axis.
    pub fn as_points(&self) -> Points<'_> {
        let npoint = self.npoint();
        Points::from_parts(
            self.data.rows().into_iter().map(|r| r.to_slice().expect("standard layout")).collect(),
            npoint,
        )
    }

    /// A writable view of every axis.
    pub fn as_points_mut(&mut self) -> PointsMut<'_> {
        let npoint = self.npoint();
        PointsMut::from_parts(
            self.data.rows_mut().into_iter().map(|r| r.into_slice().expect("standard layout")).collect(),
            npoint,
        )
    }
}

impl PointShape for PointSet {
    fn npoint(&self) -> usize {
        self.data.ncols()
    }
    fn ncoord(&self) -> usize {
        self.data.nrows()
    }
}
```

Keep the existing doc comments where a method's meaning is unchanged; rewrite the type's and `view`'s.

- [ ] **Step 4: Fix every caller until `$S/cg.sh cargo build --workspace --all-targets` and `$S/cg.sh cargo build -p ast-core --all-targets --features simd-eval` succeed.** Three `point_mut` patterns cover the call sites:

```rust
// A whole point copied in:
out.point_mut(i).copy_from_slice(x);        // before
out.set_point(i, x);                        // after

// A point built element by element:
let outp = out.point_mut(i);                // before
outp[k] = f(..);
let mut outp = PointBuf::zeros(out.ncoord()); // after
outp[k] = f(..);
out.set_point(i, &outp);

// One element:
out.point_mut(i)[k] = v;                    // before
out.axis_mut(k)[i] = v;                     // after
```

`point(i)` now returns a `PointBuf`: where a `&[f64]` is required write `&pts.point(i)`, and where a loop reads many coordinates of one point, bind it once (`let p = pts.point(i);`).
For each `view()` / `view_mut()` / `from_ndarray` / `into_ndarray` site, swap the two axes of every index, slice and shape: `s![a..b, ..]` (points) becomes `s![.., a..b]`; `s![.., ..k]` (axes) becomes `s![..k, ..]`; `.row(i)` (a point) becomes `.column(i)`; `.column(j)` (an axis) becomes `.row(j)`; an array built as `(npoint, ncoord)` for `from_ndarray` is built as `(ncoord, npoint)`.
For matrix products, write out which index is the point and which the axis before changing a `.dot()`, and keep the sum over input axes in the same order as before.
Check each of the 51 sites by reading it, not only by compiling it; record each file in the ledger as done.
In `test_transform_memory.rs`, `points()` becomes `pts.set_point(i, &[i as f64, -(i as f64)])`.

- [ ] **Step 5: Run the suites**

Run, each separately: `$S/suite.sh`, `$S/cg.sh cargo test -q -p ast-bench`, `$S/cg.sh cargo test -q -p ast-core --features simd-eval --lib`
Expected: every area green with the same pass counts as before the task (mappings 1024 passed 1 ignored, fitschan 355, domain_frames 298, plus the new pointset tests in meta).

- [ ] **Step 6: The bit gate and the oracle tier**

Run: `$S/cg.sh cargo run -q --release -p ast-bench -- bits --base T1`
Expected: `every transform's output bits match T1`.
Run: `$S/cg.sh cargo test -q -p ast-oracle-tests`
Expected: the same result as at T1 (run it at T1 first if not already known, and record both in the ledger).

- [ ] **Step 7: Record speed (not gated).** `$S/cg.sh cargo run -q --release -p ast-bench -- compare --set bench --base T1`; put the slowest five rows in the ledger.
A slowdown here is expected for kernels that now copy each point; Tasks 6 to 9 recover it.

- [ ] **Step 8: fmt, clippy (both feature sets, plus `--features simd-eval` on ast-core), commit** with message `PointSet: one contiguous array per axis`.

---

### Task 5: Transforms take views

**Files:**
- Modify: `crates/ast-core/src/mappings/mod.rs` (trait, `validate_transform`, `tran1`, `tran2`, `transform_nan_protocol`)
- Modify: the 41 files in `ast-core/src` that implement `transform_into` (listed by `rg -l "fn transform_into\b" crates/ast-core/src`), every caller of `transform_into` / `transform_into_nan` (37 in src, 5 in tests, 3 elsewhere), `crates/ast-core/tests/traits_compile.rs`, `crates/ast-bench/src/run.rs`
- Test: `crates/ast-core/tests/points.rs`

**Interfaces:**
- Consumes: Task 3's views and Task 4's `PointSet::{as_points, as_points_mut, from_points}`.
- Produces:
  - `fn transform_into(&self, input: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()>;` (required)
  - `fn transform_into_nan(&self, input: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()>` (provided, delegates as today)
  - `fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>` (provided)
  - `pub(crate) fn validate_transform<M: Mapping + ?Sized>(map: &M, pts: &impl PointShape, forward: bool, out: Option<&dyn PointShape>) -> AstResult<()>`

- [ ] **Step 1: Write the failing tests** at the end of `crates/ast-core/tests/points.rs`:

```rust
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ZoomMap};

#[test]
fn a_transform_can_write_into_a_callers_trann_buffers() {
    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let input = [1.0, 2.0, 3.0, 0.0, 10.0, 20.0, 30.0];
    let mut output = [-1.0; 8];
    zoom.transform_into(
        Points::from_strided(&input, 2, 3, 4).unwrap(),
        true,
        PointsMut::from_strided(&mut output, 2, 3, 5).unwrap(),
    )
    .unwrap();
    assert_eq!(output, [2.0, 4.0, 6.0, -1.0, -1.0, 20.0, 40.0, 60.0]);
}

#[test]
fn tran2_matches_transform() {
    let zoom = ZoomMap::new(2, 1.5).unwrap();
    let (x, y) = (vec![1.0, -2.0, 3.5], vec![4.0, 0.25, -6.0]);
    let (xo, yo) = zoom.tran2(&x, &y, true).unwrap();
    let out = zoom.transform(&PointSet::from_columns(&[&x, &y]).unwrap(), true).unwrap();
    assert_eq!(xo, out.axis(0));
    assert_eq!(yo, out.axis(1));
}

#[test]
fn a_transform_of_no_points_still_checks_the_axes() {
    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let (x, y, z): ([f64; 0], [f64; 0], [f64; 0]) = ([], [], []);
    let mut out: [[f64; 0]; 2] = [[], []];
    let [a, b] = &mut out;
    let e = zoom
        .transform_into(
            Points::new([&x[..], &y[..], &z[..]]).unwrap(),
            true,
            PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
        )
        .unwrap_err();
    assert_eq!(e.code(), ErrorCode::Ncpin);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `$S/cg.sh cargo test -q -p ast-core --test meta points::`
Expected: compile errors, `transform_into` expects `&PointSet`.

- [ ] **Step 3: Change the trait and `validate_transform`** in `crates/ast-core/src/mappings/mod.rs`:

```rust
    /// Transform a set of points.
    ///
    /// If `forward` is true, applies the forward transformation; otherwise
    /// applies the inverse.
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        validate_transform(self, pts, forward, None)?;
        let nout = if forward { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        self.transform_into(pts.as_points(), forward, out.as_points_mut())?;
        Ok(out)
    }

    /// Transform points into a caller's buffer.
    ///
    /// `out` must have the Mapping's output axis count for the direction and
    /// at least `input.npoint()` points; only the first `input.npoint()`
    /// points of each output axis are written. Both views may borrow a
    /// caller's own per-axis arrays, so a C-shaped caller pays no copy.
    fn transform_into(&self, input: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()>;
```

`transform_into_nan` takes `(input: Points<'_>, forward: bool, out: PointsMut<'_>)` and its default body becomes `self.transform_into(input, forward, out)`.
`validate_transform` takes `pts: &impl PointShape` and `out: Option<&dyn PointShape>`; its body is unchanged apart from the types.
`transform_nan_protocol` becomes:

```rust
        let pts_nan = substitute_bad_to_nan(pts);
        let nout = if forward { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        self.transform_into_nan(pts_nan.as_points(), forward, out.as_points_mut())?;
        substitute_nan_to_bad_in_place(&mut out);
        Ok(out)
```

`tran1` and `tran2` write straight into their results:

```rust
    fn tran2(&self, xin: &[f64], yin: &[f64], forward: bool) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // (keep the existing length check and validate_tran call)
        let mut xout = vec![0.0; xin.len()];
        let mut yout = vec![0.0; xin.len()];
        self.transform_into(
            Points::new([xin, yin])?,
            forward,
            PointsMut::new([&mut xout[..], &mut yout[..]])?,
        )?;
        if self.report() {
            let pts = PointSet::from_columns(&[xin, yin])?;
            let out = PointSet::from_columns(&[&xout, &yout])?;
            self.report_points(forward, &pts, &out);
        }
        Ok((xout, yout))
    }
```

and `tran1` the same with one axis.

- [ ] **Step 4: Convert every implementation.** Rename parameters and change types:

```rust
fn transform_into(&self, input: Points<'_>, forward: bool, mut out: PointsMut<'_>) -> AstResult<()> {
    crate::mappings::validate_transform(self, &input, forward, Some(&out))?;
    // body: `pts.` -> `input.`; `out.point(..)`, `out.set_point(..)`, `out.axis_mut(..)` are unchanged
```

Delete every `transform` override whose body is only `validate_transform`, `PointSet::zeros`, `transform_into` and `Ok(out)`: the provided method does the same.
Keep the overrides that do more (IntraMap, TranMap, SelectorMap, SwitchMap, RateMap, MatrixMap, PermMap, CmpMap, FrameSet, and the Regions); where such a class's `transform_into` calls `self.transform` and copies the result, write it as:

```rust
    let result = self.transform(&PointSet::from_points(&input), forward)?;
    out.copy_from(&result.as_points());
```

Any `out.view_mut()` or `pts.view()` inside a kernel becomes axis access through the views.
Callers change from `m.transform_into(&pts, fwd, &mut out)` to `m.transform_into(pts.as_points(), fwd, out.as_points_mut())`; a caller that already holds views passes them, using `reborrow()` when it needs `out` again afterwards.
In `crates/ast-bench/src/run.rs`, `time_rust` passes `pts.as_points()` and `rust_out.as_points_mut()`, and `time_cols` becomes zero-copy:

```rust
            let mut time_cols = || -> Result<(), String> {
                let fail = |e: ast_core::error::AstError| format!("{} {dir} N={n}: {e}", s.relpath);
                let input = Points::new(columns.chunks_exact(n)).map_err(fail)?;
                let out = PointsMut::new(cols_out.chunks_exact_mut(n)).map_err(fail)?;
                map.transform_into(input, s.forward, out).map_err(fail)
            };
```

- [ ] **Step 5: Run the tests**

Run, separately: `$S/cg.sh cargo test -q -p ast-core --test meta points::`, `$S/suite.sh`, `$S/cg.sh cargo test -q -p ast-bench`
Expected: the three new tests pass; every area has the same pass counts as at the end of Task 4.

- [ ] **Step 6: Bits and oracle tier**

Run: `$S/cg.sh cargo run -q --release -p ast-bench -- bits --base T1`
Expected: `every transform's output bits match T1`.
Run: `$S/cg.sh cargo test -q -p ast-oracle-tests`
Expected: as at T1.

- [ ] **Step 7: Record speed** with `compare --set bench --base T1` in the ledger, as in Task 4; the `rust-cols` rows should now be close to the `rust` rows.

- [ ] **Step 8: fmt, clippy (all three feature sets), commit** with message `Mapping::transform_into takes per-axis views`.

---

### Task 6: ZoomMap, ShiftMap and UnitMap kernels

**Files:**
- Modify: `crates/ast-core/src/mappings/zoom.rs`, `crates/ast-core/src/mappings/shift.rs`, `crates/ast-core/src/mappings/unit.rs`
- Test: `crates/ast-core/tests/test_bulk_kernels.rs` (new, in the `mappings` area)

**Interfaces:**
- Consumes: Task 5's trait.
- Produces: no new API; one commit per class.

- [ ] **Step 1: Write the tests** in `crates/ast-core/tests/test_bulk_kernels.rs`, and add `#[path = "../test_bulk_kernels.rs"] mod test_bulk_kernels;` to `crates/ast-core/tests/mappings/main.rs`:

```rust
//! Whole-axis kernels: non-finite inputs and oversized outputs.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::{Points, PointsMut};
use ast_core::mappings::{Mapping, ShiftMap, UnitMap, ZoomMap};

/// Every non-finite input is bad: BAD, NaN, and both infinities.
const ODD: [f64; 5] = [BAD_VALUE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0];

/// Transform `ODD` on both axes into outputs one point longer, which must
/// keep their last value.
fn run(map: &dyn Mapping, forward: bool) -> [Vec<f64>; 2] {
    let mut out = [vec![7.0; ODD.len() + 1], vec![7.0; ODD.len() + 1]];
    let [a, b] = &mut out;
    map.transform_into(
        Points::new([&ODD[..], &ODD[..]]).unwrap(),
        forward,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    out
}

fn assert_bad_then(axis: &[f64], good: f64) {
    assert!(axis[..4].iter().all(|&v| v == BAD_VALUE), "{axis:?}");
    assert_eq!(axis[4], good);
    assert_eq!(axis[5], 7.0, "a point past the input was written");
}

#[test]
fn zoommap_kernel() {
    let m = ZoomMap::new(2, 4.0).unwrap();
    for axis in run(&m, true) {
        assert_bad_then(&axis, 8.0);
    }
    for axis in run(&m, false) {
        assert_bad_then(&axis, 0.5);
    }
}

#[test]
fn shiftmap_kernel() {
    let m = ShiftMap::new(&[1.0, -1.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 3.0);
    assert_bad_then(&b, 1.0);
}

#[test]
fn unitmap_kernel() {
    let m = UnitMap::new(2);
    let [a, _] = run(&m, true);
    // UnitMap replaces only NaN; it passes infinities through.
    assert_eq!(a[..5], [BAD_VALUE, BAD_VALUE, f64::INFINITY, f64::NEG_INFINITY, 2.0]);
    assert_eq!(a[5], 7.0);
}
```

Adjust the constructors to their real signatures (`rg -n "pub fn new" crates/ast-core/src/mappings/{zoom,shift,unit}.rs`).
These tests pin behavior and pass before the rewrite; they are the regression check for it.

- [ ] **Step 2: Run them**

Run: `$S/cg.sh cargo test -q -p ast-core --test mappings test_bulk_kernels::`
Expected: 3 passed.
If `unitmap_kernel` fails, the current UnitMap behavior differs from the comment; take the current behavior as truth, fix the test, and note it in the ledger.

- [ ] **Step 3: Rewrite ZoomMap**

```rust
    fn transform_into(&self, input: Points<'_>, forward: bool, mut out: PointsMut<'_>) -> AstResult<()> {
        crate::mappings::validate_transform(self, &input, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.zoom() } else { 1.0 / self.zoom() };
        let n = input.npoint();
        for j in 0..input.ncoord() {
            for (o, &v) in out.axis_mut(j)[..n].iter_mut().zip(input.axis(j)) {
                *o = if is_bad(v) { BAD_VALUE } else { v * scale };
            }
        }
        Ok(())
    }
```

`transform_into_nan` is the same loop with `*o = v * scale;`.

- [ ] **Step 4: Gate and commit ZoomMap.**
Run: `$S/cg.sh cargo test -q -p ast-core --test mappings`, then `bits --base T1` (expect match), then `$S/cg.sh cargo run -q --release -p ast-bench -- compare --set bench --base HEAD`.
Expected: no slowdown; `zoommap_2d.map` faster at N≥100000. If any row is slower, stop and find out why before committing.
Commit `ZoomMap: whole-axis kernel` with the N=1000000 `rust` and `rust-cols` figures before and after in the message.

- [ ] **Step 5: Rewrite ShiftMap**

```rust
    fn transform_into(&self, input: Points<'_>, forward: bool, mut out: PointsMut<'_>) -> AstResult<()> {
        crate::mappings::validate_transform(self, &input, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);
        let sign = if fwd { 1.0 } else { -1.0 };
        let n = input.npoint();
        for (j, &shift) in self.shift.iter().enumerate() {
            let o = &mut out.axis_mut(j)[..n];
            // A bad shift makes its axis bad (shiftmap.c Transform).
            if shift == BAD_VALUE {
                o.fill(BAD_VALUE);
                continue;
            }
            let d = sign * shift;
            for (o, &v) in o.iter_mut().zip(input.axis(j)) {
                *o = if is_bad(v) { BAD_VALUE } else { v + d };
            }
        }
        Ok(())
    }
```

`transform_into_nan`: a bad shift fills `f64::NAN`; otherwise `*o = v + d;`.
Gate and commit as in Step 4 (`ShiftMap: whole-axis kernel`).

- [ ] **Step 6: Rewrite UnitMap**

```rust
    fn transform_into(&self, input: Points<'_>, forward: bool, mut out: PointsMut<'_>) -> AstResult<()> {
        crate::mappings::validate_transform(self, &input, forward, Some(&out))?;
        let n = input.npoint();
        for j in 0..input.ncoord() {
            for (o, &v) in out.axis_mut(j)[..n].iter_mut().zip(input.axis(j)) {
                *o = if v.is_nan() { BAD_VALUE } else { v };
            }
        }
        Ok(())
    }
```

`transform_into_nan`: `out.axis_mut(j)[..n].copy_from_slice(input.axis(j))` per axis.
Gate and commit as in Step 4 (`UnitMap: whole-axis kernel`).

---

### Task 7: WinMap and PermMap kernels

**Files:**
- Modify: `crates/ast-core/src/mappings/winmap.rs`, `crates/ast-core/src/mappings/perm.rs`
- Test: `crates/ast-core/tests/test_bulk_kernels.rs`

**Interfaces:**
- Consumes: `run` and `assert_bad_then` from Task 6's test file.

- [ ] **Step 1: Add the tests** to `test_bulk_kernels.rs`:

```rust
use ast_core::mappings::{PermMap, WinMap};

#[test]
fn winmap_kernel() {
    // Axis 1 maps [0, 1] to [0, 2]; axis 2 maps [0, 1] to [1, 2].
    let m = WinMap::new(&[0.0, 0.0], &[1.0, 1.0], &[0.0, 1.0], &[2.0, 2.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 4.0);
    assert_bad_then(&b, 3.0);
    let [a, b] = run(&m, false);
    assert_bad_then(&a, 1.0);
    assert_bad_then(&b, 1.0);
}

#[test]
fn permmap_kernel() {
    // Output 1 is input 2, output 2 is the constant 5.
    let m = PermMap::new(&[2, 1], &[2, -1], Some(&[5.0])).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 2.0);
    assert!(b[..5].iter().all(|&v| v == 5.0), "{b:?}");
    assert_eq!(b[5], 7.0);
}
```

Adjust the constructors to their real signatures.

- [ ] **Step 2: Run them**

Run: `$S/cg.sh cargo test -q -p ast-core --test mappings test_bulk_kernels::`
Expected: 5 passed.

- [ ] **Step 3: Rewrite WinMap**

```rust
    fn transform_into(&self, input: Points<'_>, forward: bool, mut out: PointsMut<'_>) -> AstResult<()> {
        crate::mappings::validate_transform(self, &input, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);
        let n = input.npoint();
        for (j, (&shift, &scale)) in self.shifts.iter().zip(&self.scales).enumerate() {
            let o = &mut out.axis_mut(j)[..n];
            let x = input.axis(j);
            if fwd {
                // forward: output = shift + scale * input
                if is_bad(shift) || is_bad(scale) {
                    o.fill(BAD_VALUE);
                    continue;
                }
                for (o, &v) in o.iter_mut().zip(x) {
                    *o = if is_bad(v) { BAD_VALUE } else { shift + scale * v };
                }
            } else {
                // inverse: output = -shift/scale + input/scale, with the
                // coefficients inverted first as winmap.c `Transform` does,
                // so the rounding matches C's.
                if is_bad(shift) || is_bad(scale) || scale == 0.0 {
                    o.fill(BAD_VALUE);
                    continue;
                }
                let bb = 1.0 / scale;
                let a = -shift * bb;
                for (o, &v) in o.iter_mut().zip(x) {
                    *o = if is_bad(v) { BAD_VALUE } else { a + bb * v };
                }
            }
        }
        Ok(())
    }
```

`transform_into_nan`: the same structure, filling `f64::NAN` for a bad channel and computing without the `is_bad(v)` test.
Gate (`test_bulk_kernels`, the mappings area, `bits --base T1`, `compare --set bench --base HEAD`) and commit `WinMap: whole-axis kernel` with before and after figures.

- [ ] **Step 4: Rewrite PermMap's `apply_perm`**

```rust
    fn apply_perm(perm: &[i64], constants: &[f64], input: &Points<'_>, out: &mut PointsMut<'_>) {
        let ncoord_in = input.ncoord();
        let n = input.npoint();
        for (k, &p) in perm.iter().enumerate() {
            let o = &mut out.axis_mut(k)[..n];
            // Mirrors C permmap.c:1976-1997: a positive entry copies the
            // input axis only when in range; a negative entry reads the
            // constants pool when in range; everything else is BAD. C's
            // NULL-constants path uses AST__BAD for any negative entry, so
            // an out-of-range constant index resolves to BAD here too.
            if p > 0 && ((p as usize) - 1) < ncoord_in {
                for (o, &v) in o.iter_mut().zip(input.axis((p as usize) - 1)) {
                    *o = if is_bad(v) { BAD_VALUE } else { v };
                }
            } else if p < 0 {
                o.fill(constants.get((-p as usize) - 1).copied().unwrap_or(BAD_VALUE));
            } else {
                o.fill(BAD_VALUE);
            }
        }
    }
```

Its NaN counterpart copies the axis unchanged for `p > 0` in range, fills from `constants_nan` (or NaN) for `p < 0`, and fills NaN otherwise.
Gate and commit `PermMap: whole-axis kernel` as in Step 3.

---

### Task 8: MatrixMap kernel

**Files:**
- Modify: `crates/ast-core/src/mappings/matrix.rs`
- Test: `crates/ast-core/tests/test_bulk_kernels.rs`

- [ ] **Step 1: Add the tests**

```rust
use ast_core::mappings::MatrixMap;

#[test]
fn diagonal_matrixmap_kernel() {
    let m = MatrixMap::diagonal(&[2.0, -1.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 4.0);
    assert_bad_then(&b, -2.0);
}

#[test]
fn full_matrixmap_kernel_ignores_a_bad_input_with_a_zero_coefficient() {
    // Output 1 = 1*x + 0*y ; output 2 = 3*x + 2*y.
    let m = MatrixMap::full(2, 2, &[1.0, 0.0, 3.0, 2.0]).unwrap();
    let x = [1.0, 2.0, BAD_VALUE];
    let y = [BAD_VALUE, 1.0, 1.0];
    let mut out = [vec![0.0; 3], vec![0.0; 3]];
    let [a, b] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    assert_eq!(out[0], [1.0, 2.0, BAD_VALUE]);
    assert_eq!(out[1], [BAD_VALUE, 8.0, BAD_VALUE]);
}

#[test]
fn full_matrixmap_kernel_crosses_its_block_boundary() {
    let m = MatrixMap::full(2, 2, &[1.0, 2.0, 3.0, 4.0]).unwrap();
    let n = 1000;
    let x: Vec<f64> = (0..n).map(f64::from).collect();
    let y: Vec<f64> = (0..n).map(|i| f64::from(i) * 0.5).collect();
    let mut out = [vec![0.0; n as usize], vec![0.0; n as usize]];
    let [a, b] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    for i in 0..n as usize {
        assert_eq!(out[0][i], 0.0 + 1.0 * x[i] + 2.0 * y[i]);
        assert_eq!(out[1][i], 0.0 + 3.0 * x[i] + 4.0 * y[i]);
    }
}
```

Adjust the constructors to their real names and argument order.

- [ ] **Step 2: Run them**

Run: `$S/cg.sh cargo test -q -p ast-core --test mappings test_bulk_kernels::`
Expected: 8 passed.

- [ ] **Step 3: Rewrite `apply_matrix`**

```rust
/// Points per pass of the full-matrix kernel: the sums and bad flags of one
/// block live on the stack. The size only tiles the loop; it cannot change
/// any result.
const BLOCK: usize = 256;

    fn apply_matrix(m: &Array2<f64>, form: MatrixForm, input: &Points<'_>, out: &mut PointsMut<'_>) {
        let n = input.npoint();
        let form = if matches!(form, MatrixForm::Unit) { MatrixForm::Diagonal } else { form };
        match form {
            MatrixForm::Unit => unreachable!(),
            MatrixForm::Diagonal => {
                let nax = m.nrows().min(m.ncols());
                for j in 0..nax {
                    // A BAD diagonal element makes the whole output axis BAD;
                    // otherwise BAD input gives BAD output (matrixmap.c:5253).
                    let diag = m[[j, j]];
                    let o = &mut out.axis_mut(j)[..n];
                    if is_bad(diag) {
                        o.fill(BAD_VALUE);
                        continue;
                    }
                    for (o, &v) in o.iter_mut().zip(input.axis(j)) {
                        *o = if is_bad(v) { BAD_VALUE } else { diag * v };
                    }
                }
                for j in nax..m.nrows() {
                    out.axis_mut(j)[..n].fill(0.0);
                }
            }
            MatrixForm::Full => {
                for row in 0..m.nrows() {
                    // A BAD element makes its whole row BAD.
                    if m.row(row).iter().any(|&c| is_bad(c)) {
                        out.axis_mut(row)[..n].fill(BAD_VALUE);
                        continue;
                    }
                    let mut start = 0;
                    while start < n {
                        let end = (start + BLOCK).min(n);
                        let mut sum = [0.0_f64; BLOCK];
                        let mut bad = [false; BLOCK];
                        // Each point's sum runs over the input axes in order,
                        // as C's does.
                        for col in 0..m.ncols() {
                            let coeff = m[[row, col]];
                            for (k, &v) in input.axis(col)[start..end].iter().enumerate() {
                                if is_bad(v) {
                                    // A BAD input only matters through a
                                    // non-zero coefficient.
                                    if coeff != 0.0 {
                                        bad[k] = true;
                                    }
                                } else {
                                    sum[k] += coeff * v;
                                }
                            }
                        }
                        for (k, o) in out.axis_mut(row)[start..end].iter_mut().enumerate() {
                            *o = if bad[k] { BAD_VALUE } else { sum[k] };
                        }
                        start = end;
                    }
                }
            }
        }
    }
```

The NaN variant has the same shape: a `BAD_VALUE` diagonal fills NaN; a `BAD_VALUE` element fills its row with NaN; otherwise each block sums `coeff * v` for every `coeff != 0.0`, with no bad flags.
Update both callers to pass `&input` and `&mut out`.

- [ ] **Step 4: Gate and commit** `MatrixMap: whole-axis kernel` as in Task 6 Step 4, quoting the `matrixmap_3x3_diag` and `matrixmap_3x3_full` figures.

---

### Task 9: CmpMap without copies

**Files:**
- Modify: `crates/ast-core/src/mappings/cmpmap.rs`
- Test: `crates/ast-core/tests/test_bulk_kernels.rs`, `crates/ast-core/tests/test_transform_memory.rs` (unchanged; must still pass)

- [ ] **Step 1: Add the tests**

```rust
use ast_core::mappings::{CmpMap, Composition};

#[test]
fn a_parallel_cmpmap_splits_its_output_at_the_first_childs_output_count() {
    // First child: 1 input -> 2 outputs (the input and the constant 9).
    let first = PermMap::new(&[1], &[1, -1], Some(&[9.0])).unwrap();
    let second = ZoomMap::new(1, 2.0).unwrap();
    let m = CmpMap::new(Box::new(first), Box::new(second), Composition::Parallel).unwrap();
    let (x, y) = ([1.0, 2.0], [3.0, 4.0]);
    let mut out = [vec![0.0; 2], vec![0.0; 2], vec![0.0; 2]];
    let [a, b, c] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..], &mut c[..]]).unwrap(),
    )
    .unwrap();
    assert_eq!(out, [vec![1.0, 2.0], vec![9.0, 9.0], vec![6.0, 8.0]]);
}

#[test]
fn a_series_cmpmap_handles_a_partial_last_batch() {
    let m = CmpMap::new(
        Box::new(ZoomMap::new(1, 2.0).unwrap()),
        Box::new(ShiftMap::new(&[1.0]).unwrap()),
        Composition::Series,
    )
    .unwrap();
    let n = 8192 + 5;
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut o = vec![0.0; n];
    m.transform_into(Points::new([&x[..]]).unwrap(), true, PointsMut::new([&mut o[..]]).unwrap())
        .unwrap();
    assert!(o.iter().zip(&x).all(|(&o, &x)| o == 2.0 * x + 1.0));
}

#[test]
fn a_series_cmpmap_of_no_points_still_checks_its_stages() {
    // A UnitMap of 2 axes after a PermMap whose inverse is undefined.
    let first = PermMap::new(&[1, 1], &[1], None).unwrap();
    let m = CmpMap::new(Box::new(first), Box::new(UnitMap::new(1)), Composition::Series).unwrap();
    let x: [f64; 0] = [];
    let mut a: [f64; 0] = [];
    let mut b: [f64; 0] = [];
    let r = m.transform_into(
        Points::new([&x[..]]).unwrap(),
        false,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    );
    assert!(r.is_err(), "{r:?}");
}
```

Adjust constructors as needed; if the third test's chosen PermMap has a defined inverse, pick any Mapping whose inverse C leaves undefined (`rg -n "tran_inverse" crates/ast-core/src/mappings/perm.rs`).

- [ ] **Step 2: Run them**

Run: `$S/cg.sh cargo test -q -p ast-core --test mappings test_bulk_kernels::`
Expected: 11 passed (these pin current behavior).

- [ ] **Step 3: Rewrite the parallel paths**

```rust
    fn parallel_transform_into(&self, input: Points<'_>, fwd: bool, out: PointsMut<'_>) -> AstResult<()> {
        let split_in = self.effective_first_nin(fwd);
        let (out_a, out_b) = out.split_axes_at(self.effective_first_nout(fwd));
        self.first.transform_into(input.axes(0..split_in), self.first_dir(fwd), out_a)?;
        self.second.transform_into(input.axes(split_in..input.ncoord()), self.second_dir(fwd), out_b)
    }
```

`parallel_transform_into_nan` is the same with `transform_into_nan`.
Delete `parallel_transform`.

- [ ] **Step 4: Rewrite the series paths**

```rust
    /// Run the two stages [`SERIES_BATCH`] points at a time through one
    /// reused intermediate buffer, as cmpmap.c `Transform` does. Zero points
    /// still run both stages once, so their checks apply.
    fn series_transform_into(&self, input: Points<'_>, fwd: bool, mut out: PointsMut<'_>) -> AstResult<()> {
        let [(m1, d1), (m2, d2)] = self.series_stages(fwd);
        let npoint = input.npoint();
        let nmid = if d1 { m1.nout() } else { m1.nin() };
        let mut mid = PointSet::zeros(npoint.min(SERIES_BATCH), nmid);
        let mut mid = mid.as_points_mut();
        let mut start = 0;
        loop {
            let end = (start + SERIES_BATCH).min(npoint);
            let mut batch = mid.points_mut(0..end - start);
            m1.transform_into(input.points(start..end), d1, batch.reborrow())?;
            m2.transform_into(batch.as_points(), d2, out.points_mut(start..end))?;
            start = end;
            if start >= npoint {
                return Ok(());
            }
        }
    }
```

`series_transform_into_nan` is the same with `transform_into_nan`.
Delete `series_transform`, `series_transform_batches`, `batch_of`, and CmpMap's `transform` override (the provided method is equivalent).

- [ ] **Step 5: Gate**

Run: `$S/cg.sh cargo test -q -p ast-core --test mappings test_bulk_kernels::`, `$S/cg.sh cargo test -q -p ast-core --test meta test_transform_memory::`, `$S/suite.sh`, `bits --base T1`, `compare --set bench --base HEAD`, `compare --set framesets --base HEAD`.
Expected: all green; bits match; `cmpmap_parallel` and `cmpmap_series` faster, nothing slower.

- [ ] **Step 6: Commit** `CmpMap: split and batch views instead of copying` with the before and after figures.

---

### Task 10: Close out

**Files:**
- Create: `docs/perf/2026-10-<dd>-macos-aarch64-bench.tsv`, `docs/perf/2026-10-<dd>-macos-aarch64-sweep.tsv` (the date they are recorded)
- Modify: `docs/perf/README.md`, `docs/superpowers/specs/2026-04-20-rust-port-design.md`, `CLAUDE.md`, `docs/superpowers/specs/2026-10-16-per-axis-pointset-design.md` (status line)

- [ ] **Step 1: Record.** With the machine otherwise idle, run `$S/cg.sh cargo run -q --release -p ast-bench --features c-oracle -- snapshot --set bench` and `... -- sweep` (overnight), and copy the outputs into `docs/perf/` beside the first ones.

- [ ] **Step 2: Summarize.** Add to `docs/perf/README.md` a section "After per-axis views" with the table of `rust`, `rust-cols` and `c` at N=1000000 for the seven rewritten classes against the 2026-10-16 figures, and the five worst remaining ratios.

- [ ] **Step 3: Amend the port design (rev 7).** In `2026-04-20-rust-port-design.md`: add a revision-history entry; change the `PointSet` sketch to `(ncoord, npoint)`, one contiguous array per axis; replace "`Array2<f64>` owns its data as a contiguous row-major `Vec<f64>` which `ast-capi` can hand to C without copy" with the statement that `transform_into` takes `Points` and `PointsMut`, which the C shim builds over the caller's per-axis arrays without copying, as C's `TranN` does with `astSetPoints`; and change the transform signature sketch.

- [ ] **Step 4: CLAUDE.md.** In "Bulk-transform optimisation", replace the description of "the default per-point loop pattern" with: kernels loop down whole axes of a `Points` view into a `PointsMut` view, testing `is_bad` per value; a per-point loop through `point(i)` and `set_point` is for code that is not a bulk kernel.
Add `ast-bench bits --base <ref>` to the performance-gate paragraph as the check for changes that must not alter results.

- [ ] **Step 5: Spec status.** Set the per-axis spec's status to `implemented (stages 1-4); SpecMap, MathMap, SplineMap and LutMap kernels are a follow-up plan`.

- [ ] **Step 6: Commit** `docs: per-axis PointSet results and spec rev 7`.

Run the whole gate one last time before the final review: `$S/suite.sh`, `cargo test -p ast-bench`, `cargo test -p ast-oracle-tests`, `bits --base T1`, fmt and clippy on all feature sets.
