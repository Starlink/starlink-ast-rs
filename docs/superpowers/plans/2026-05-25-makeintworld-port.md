# MakeIntWorld port — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the C oracle's full CRPIX/CD/CDELT extraction pipeline (`FsetToStore` → `CelestialAxes`/`SpectralAxes`/`OtherAxes` → `MakeIntWorld` → `SIPIntWorld` plus the Mapping primitives `astMapSplit`, `astLinearApprox`, `AnalysePoly`, `ScalePolyInputs`) so every wcsconv test that today fails on a numeric-value mismatch passes bytewise.

**Architecture:** Replace the value-extraction half of `store_from_frameset_to` with a faithful Rust port of `FsetToStore`. The IWC mapping is *constructed* from chosen header values rather than extracted from the FrameSet's `pixel→world` chain — that's the structural fix for the ~µpixel CRPIX residual. New code lives under `crates/ast-core/src/fitschan/wcs_keywords/`. Mapping primitives go in `crates/ast-core/src/mappings/helpers/`. Eight commit groups; intermediate states allowed to fail tests.

**Tech Stack:** Rust 2024, ndarray, ast-core crate. C reference at `reference/starlink-ast/src/fitschan.c`. Tests use `cargo test -p ast-core`.

**Spec:** `docs/superpower/specs/2026-05-25-makeintworld-port-design.md` (commit `53cc230`). Read it first.

---

## Hard rules

1. **No heuristics.** When the C oracle has an exact algorithm, port it. Do not substitute a tolerance constant or a Newton iteration that doesn't appear in the C source. If you find yourself writing `if (max_step < 1e-9)` or similar without a corresponding `fitschan.c:NNNN` reference, stop.
2. **Match C control flow.** Use idiomatic Rust signatures (`Result<T, E>`, `Option<T>`, owned `Vec`s) but preserve the C source's conditionals, iteration order, intermediate values, and fallback paths line-for-line.
3. **C-oracle-derived fixtures.** Every numerical-output test that would otherwise require hand-computing an expected value must capture the value from the reference C library once and embed it as a Rust constant. Generation programs live in `tools/oracle_fixtures/`. Both the `.c` source and captured `.rs` constants are committed together.
4. **Build stays green per commit group.** `cargo fmt && cargo clippy --workspace -- -D warnings && cargo build --workspace` passes before each group's terminal commit. Tests are allowed to fail in groups 1–6 only when an intentionally-not-yet-wired path exists.
5. **No code from the existing path leaks into the new path.** Don't copy from `wcs_write.rs` helpers like `decompose_linear` callsites, `populate_celestial`, `populate_spectral`, `solve_crpix_from_decomp` callsites, etc. Those get deleted in Group 7. The new code reads C source, not the existing Rust path.

---

## File structure

### Files created

```
crates/ast-core/src/fitschan/wcs_keywords/
├── mod.rs                       # Public re-exports + fset_to_store entry
├── celestial_axes.rs            # ~700 LoC — CelestialAxes port + GetFiducialWCS + SkySys
├── spectral_axes.rs             # ~700 LoC — SpectralAxes port + algorithm-code helpers
├── other_axes.rs                # ~400 LoC — OtherAxes port
├── make_int_world.rs            # ~600 LoC — MakeIntWorld body (+ orth.rs / invertable.rs callers)
├── sip_int_world.rs             # ~600 LoC — SIPIntWorld port
├── orth.rs                      # ~150 LoC — OrthVectorSet + OrthVector
├── invertable.rs                # ~50 LoC — MakeInvertable
├── sky_sys.rs                   # ~250 LoC — SkySys + IsSkyOff helpers
└── (small helpers as needed)

crates/ast-core/src/mappings/helpers/
├── linear_approx.rs             # ~250 LoC — astLinearApprox port
├── analyse_poly.rs              # ~350 LoC — AnalysePoly port
└── scale_poly_inputs.rs         # ~80 LoC — ScalePolyInputs port

tools/oracle_fixtures/            # NEW directory at repo root
├── README.md                    # How to build + use the fixture generators
├── Makefile                     # `make linear_approx` etc.
├── linear_approx.c              # Generates Rust const arrays for linear_approx tests
├── analyse_poly.c               # Generates Rust const arrays for analyse_poly tests
├── scale_poly_inputs.c          # …
├── find_basis_vectors.c         # …
├── fit_line.c                   # …
├── make_int_world.c             # …
└── sip_int_world.c              # …
```

### Files modified (substantially)

```
crates/ast-core/src/mappings/mod.rs      # New `Mapping::map_split` signature
crates/ast-core/src/mappings/{cmpmap,matrix,perm,polymap,shift,unit,winmap,wcsmap,zoom}.rs
                                          # New per-class `map_split` impls
crates/ast-core/src/mappings/helpers/mod.rs  # Re-exports new helpers
crates/ast-core/src/fitschan/mod.rs       # Add `wcs_keywords` submodule
crates/ast-core/src/fitschan/wcs_write.rs # ~1000 LoC removed in Group 7
crates/ast-core/src/fitschan/wcs_decompose.rs  # Drop #[allow(dead_code)] in Group 4
crates/ast-core/tests/test_map_split.rs   # Rewritten to new semantics in Group 1
```

### Files deleted

None outright. The diagnostic helper `tools/oracle_fixtures/` stays after the port (we'll need it for future quirks too).

---

## Reading the C source

For every port task, the workflow is:

1. **Read the C function** in `reference/starlink-ast/src/fitschan.c` (or `mapping.c`, `polymap.c` for primitives) at the line range cited.
2. **Read its callees** that you haven't ported yet — but stop at the project boundary (`pal*`, `erfa*`, `palDmat`, etc. don't get re-ported; we already have equivalents).
3. **Translate to idiomatic Rust** preserving conditionals, iteration order, and intermediate values. Replace `astOK`/`*status` cascades with `?`. Replace output pointers with returned tuples / `Option<T>`.
4. **Capture C reference output** for any non-trivial numerical input via `tools/oracle_fixtures/<fn>.c`, embed in tests.
5. **Run tests.** If output mismatches, READ THE C source again before "fixing" — don't invent rules.

---

## Group 0: Setup (preliminary)

### Task 0.1: Verify branch state and reference build

**Files:** none modified.

- [ ] **Step 1: Check out the working branch and confirm reference submodule**

```bash
git status
git submodule status reference/starlink-ast
```

Expected: on `tickets/DM-54793` (or successor branch); submodule initialised.

- [ ] **Step 2: Build reference libast.a if not already built**

```bash
cd reference/starlink-ast
[ -d build ] || (mkdir -p build && cd build && cmake .. -DCMAKE_BUILD_TYPE=Release && cmake --build . -j)
ls build/libast*.a
cd ../..
```

Expected: `libast.a`, `libast_err.a`, `libast_grf3d.a` present.

- [ ] **Step 3: Run the wcsconv baseline**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: `=== wcsconv results: 152 passed, 40 failed, 0 skipped (of 192 total) ===`

Record this baseline; the final commit must improve it for the numeric-value tests.

- [ ] **Step 4: Confirm `find_basis_vectors`, `fit_line`, `fit_ok`, `fit_line_cd` already exist**

```bash
grep -c "pub(crate) fn find_basis_vectors\|pub(crate) fn fit_line\|fn fit_ok\|pub(crate) fn fit_line_cd" crates/ast-core/src/fitschan/wcs_decompose.rs
```

Expected: `4`

These were ported in commit `1f9f3b7` and currently carry `#[allow(dead_code)]`. We'll wire them in during Group 4.

### Task 0.2: Set up the oracle fixtures directory

**Files:**
- Create: `tools/oracle_fixtures/README.md`
- Create: `tools/oracle_fixtures/Makefile`

- [ ] **Step 1: Create the directory and README**

```bash
mkdir -p tools/oracle_fixtures
```

Write `tools/oracle_fixtures/README.md`:

````markdown
# Oracle fixture generators

Each `.c` file here builds against `reference/starlink-ast` and prints
captured outputs as Rust constant declarations. The output is copied
into the corresponding Rust unit test file once and committed alongside
the `.c` source — tests then run without requiring the oracle.

## Building

```
make
```

Builds every `.c` here and links against `reference/starlink-ast/build/libast*.a`.

## Capturing fixtures

```
make linear_approx > /tmp/linear_approx.rs
# inspect output, then paste into the corresponding test file
```

The output is formatted as a Rust `const` block intended to be pasted
into the relevant `#[cfg(test)] mod tests` in the ported function's
source file.

## Provenance rule

When a fixture's expected value changes (e.g., a captured number drifts
across reference rebuilds), regenerate ALL fixtures in the same module
and re-run the affected tests. Don't update one number by hand.
````

Write `tools/oracle_fixtures/Makefile`:

```make
# Build oracle fixture generators against reference libast.

REF := ../../reference/starlink-ast
INCS := -I$(REF)/src
LIBS := -L$(REF)/build -Wl,--whole-archive -last_grf_log -Wl,--no-whole-archive \
        -last -last_err -last_grf3d -lpal -lerfa -lm
CFLAGS := -O0 -g -Wall

PROGS := linear_approx analyse_poly scale_poly_inputs find_basis_vectors \
         fit_line make_int_world sip_int_world

all: $(PROGS)

%: %.c
	$(CC) $(CFLAGS) $(INCS) -o $@ $< $(LIBS)

clean:
	rm -f $(PROGS)

.PHONY: all clean
```

- [ ] **Step 2: Verify the Makefile works with an empty test program**

```bash
cat > tools/oracle_fixtures/_smoke.c <<'EOF'
#include "ast.h"
#include <stdio.h>
int main(void) {
    int status = 0;
    astBegin;
    AstUnitMap *um = astUnitMap(2, "");
    printf("nin=%d nout=%d\n", astGetNin(um), astGetNout(um));
    astEnd;
    return status;
}
EOF
cd tools/oracle_fixtures
make _smoke
./_smoke
rm _smoke _smoke.c
```

Expected: `nin=2 nout=2`. Confirms the C oracle build links and runs.

- [ ] **Step 3: Commit setup**

```bash
git add tools/oracle_fixtures/README.md tools/oracle_fixtures/Makefile
git commit -m "$(cat <<'EOF'
build: oracle fixture generator scaffolding

Adds tools/oracle_fixtures/ as the home for small C programs that
capture C-library reference values and emit them as Rust const
declarations for use in unit tests. Lets per-function tests run
without requiring the oracle at test time, while keeping the
captured numbers traceable to a reproducible C invocation.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Group 1: `Mapping::map_split` reshape

**Goal:** Replace `fn map_split(&self, first_out, nout_wanted) -> Option<DynMapping>` with a faithful port of `astMapSplit` semantics: take a list of input axes to retain, return both the output axes the kept inputs feed into and the sub-mapping.

**C reference:**
- Trait default: `mapping.c::MapSplit` ~line 7798.
- Per-class overrides in `cmpmap.c`, `permmap.c`, `matrixmap.c`, `winmap.c`, `shiftmap.c`, `zoommap.c`, `unitmap.c`, `wcsmap.c`, `tranmap.c`, `ratemap.c`, `normmap.c`.

**Out of scope:** `frameset.c::MapSplit`, `frame.c::MapSplit` — Frame and FrameSet aren't Mapping-trait implementers in our codebase.

### Task 1.1: Reshape the trait method + UnitMap/ZoomMap/ShiftMap impls

**Files:**
- Modify: `crates/ast-core/src/mappings/mod.rs` (trait signature)
- Modify: `crates/ast-core/src/mappings/unit.rs`
- Modify: `crates/ast-core/src/mappings/zoom.rs`
- Modify: `crates/ast-core/src/mappings/shift.rs`
- Modify: `crates/ast-core/src/mappings/cmpmap.rs` (existing recursive callsites)
- Test: `crates/ast-core/tests/test_map_split.rs` (rewrite)

- [ ] **Step 1: Update the trait signature in `mod.rs`**

In `crates/ast-core/src/mappings/mod.rs:215-225`, replace the existing `map_split` method with:

```rust
    /// Split this mapping to produce a sub-mapping covering a chosen set of inputs.
    ///
    /// Mirrors the C library's `astMapSplit` (mapping.c ~7798): given input
    /// indices `in_axes`, return the output axes those inputs feed into plus
    /// the sub-mapping that maps `in_axes` → those outputs. Returns `None`
    /// when no such split exists — i.e. when some retained input feeds an
    /// output that *also* depends on a non-retained input, or when some
    /// non-retained input feeds an output that depends only on retained
    /// inputs.
    ///
    /// `in_axes` indices are 0-based and may be in any order. The returned
    /// `Vec<usize>` has length equal to the sub-mapping's nout. The default
    /// implementation handles the trivial case `in_axes.len() == self.nin()`
    /// by composing a `PermMap` with `self`; classes that can split for
    /// partial input selections override.
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Default impl mirrors mapping.c::MapSplit (line 7902): only succeeds
        // when nin == mapnin, otherwise None. The PermMap rearranges the
        // requested axis order onto the supplied Mapping's input order.
        let mapnin = self.nin();
        if in_axes.len() != mapnin {
            return None;
        }
        // Verify each input index is in range and used exactly once.
        let mut seen = vec![false; mapnin];
        for &ix in in_axes {
            if ix >= mapnin || seen[ix] {
                return None;
            }
            seen[ix] = true;
        }
        // outperm[i] = position of input i within in_axes (1-based for PermMap).
        let mut outperm = vec![0_i64; mapnin];
        for (pos, &ix) in in_axes.iter().enumerate() {
            outperm[ix] = (pos + 1) as i64;
        }
        // inperm[pos] = which Mapping input the supplied axis at `pos` lands at.
        let inperm: Vec<i64> = in_axes.iter().map(|&ix| (ix + 1) as i64).collect();
        let perm = crate::mappings::PermMap::new(&inperm, &outperm, &[]).ok()?;
        let composed = crate::mappings::CmpMap::new(
            Box::new(perm),
            self.clone_box(),
            crate::mappings::Composition::Series,
        )
        .ok()?;
        let outs: Vec<usize> = (0..self.nout()).collect();
        Some((outs, Box::new(composed)))
    }
```

- [ ] **Step 2: Build to surface every callsite that won't compile yet**

```bash
cargo build -p ast-core 2>&1 | grep -E "^error" | head -20
```

Expected: errors at every existing `.map_split(first_out, nout_wanted)` callsite. Note them; we'll fix all callers in this group.

- [ ] **Step 3: Update CmpMap's internal recursion to call the new method**

In `crates/ast-core/src/mappings/cmpmap.rs:541-590`, the existing `map_split` method recurses through children with the old signature. Rewrite the entire `map_split` impl as a port of `cmpmap.c::MapSplit` (cmpmap.c ~line 4163). Read that C function before writing — it handles series and parallel separately.

For series: split the second mapping by `in_axes` first, then split the first mapping by *those* output indices.

For parallel: partition `in_axes` between the two children based on each child's input range; recursively split each child; concatenate output index arrays (offsetting the second child's outputs by the first child's nout).

The full port runs ~150 LoC. Show structure only here; the agent reads cmpmap.c for the body:

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors cmpmap.c::MapSplit (~line 4163). Series and parallel
        // composition need different treatment.
        if !in_axes.iter().all(|&ix| ix < self.nin()) {
            return None;
        }
        if in_axes.iter().enumerate().any(|(i, &a)| in_axes[..i].contains(&a)) {
            return None; // duplicates rejected to match C
        }
        match self.composition() {
            Composition::Series => {
                // Step 1: split `second` by in_axes.
                // Step 2: split `first` by the output indices that step 1 returned
                //   as its input indices (i.e. which outputs of `first` we kept).
                // Step 3: compose surviving fragments in series.
                // (See cmpmap.c lines ~4250-4350 for the full algorithm.)
                todo!("series implementation — port cmpmap.c lines ~4250-4350")
            }
            Composition::Parallel => {
                // Partition in_axes into "feeds first child" vs "feeds second child"
                // based on the first child's input range. Recursively split each.
                // Concatenate output indices, offsetting second child by first.nout().
                // (See cmpmap.c lines ~4350-4450.)
                todo!("parallel implementation — port cmpmap.c lines ~4350-4450")
            }
        }
    }
```

The agent fills the `todo!` bodies in Task 1.6 (CmpMap proper). For now this stub gets the trait satisfied so the module compiles.

- [ ] **Step 4: UnitMap impl**

In `crates/ast-core/src/mappings/unit.rs`, replace the existing `map_split` with:

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors unitmap.c::MapSplit (line ~152). UnitMap is identity
        // per axis, so the kept inputs map exactly to those output indices.
        let nin = self.nin();
        for &ix in in_axes {
            if ix >= nin {
                return None;
            }
        }
        // Reject duplicates.
        for (i, &a) in in_axes.iter().enumerate() {
            if in_axes[..i].contains(&a) {
                return None;
            }
        }
        Some((
            in_axes.to_vec(),
            Box::new(UnitMap::new(in_axes.len())),
        ))
    }
```

- [ ] **Step 5: ZoomMap impl**

In `crates/ast-core/src/mappings/zoom.rs`, replace `map_split`:

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors zoommap.c::MapSplit (line ~176). Per-axis scaling, so
        // each kept input maps to its own output. Same scale factor is
        // shared across all axes.
        let nin = self.nin();
        for (i, &ix) in in_axes.iter().enumerate() {
            if ix >= nin || in_axes[..i].contains(&ix) {
                return None;
            }
        }
        let mut sub = ZoomMap::new(in_axes.len(), self.factor()).ok()?;
        if self.is_inverted() {
            sub.invert();
        }
        Some((in_axes.to_vec(), Box::new(sub)))
    }
```

- [ ] **Step 6: ShiftMap impl**

In `crates/ast-core/src/mappings/shift.rs`, replace `map_split`:

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors shiftmap.c::MapSplit (line 750). Per-axis shift. Pick
        // the shift values for the chosen axes.
        let nin = self.nin();
        for (i, &ix) in in_axes.iter().enumerate() {
            if ix >= nin || in_axes[..i].contains(&ix) {
                return None;
            }
        }
        let shifts: Vec<f64> = in_axes.iter().map(|&ix| self.shifts()[ix]).collect();
        let mut sub = ShiftMap::new(shifts);
        if self.is_inverted() {
            sub.invert();
        }
        Some((in_axes.to_vec(), Box::new(sub)))
    }
```

- [ ] **Step 7: Rewrite `tests/test_map_split.rs` for the new semantics**

Replace the entire file content with tests exercising the new API. Include:

```rust
//! Tests for the C-faithful `Mapping::map_split` (mirrors astMapSplit).
//!
//! Given `in_axes` (input indices to retain), each test checks that
//! the returned `(out_axes, sub_map)` has the right shape and that
//! `sub_map.transform(input[in_axes])` equals
//! `original.transform(full_input)[out_axes]`.

use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ShiftMap, UnitMap, ZoomMap};

#[test]
fn unit_split_picks_chosen_axes() {
    let u = UnitMap::new(3);
    let (outs, sub) = u.map_split(&[0, 2]).expect("split");
    assert_eq!(outs, vec![0, 2]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
}

#[test]
fn unit_split_rejects_duplicate_input() {
    let u = UnitMap::new(3);
    assert!(u.map_split(&[1, 1]).is_none());
}

#[test]
fn unit_split_rejects_out_of_range() {
    let u = UnitMap::new(3);
    assert!(u.map_split(&[0, 3]).is_none());
}

#[test]
fn zoom_split_carries_factor() {
    let z = ZoomMap::new(3, 5.0).unwrap();
    let (_outs, sub) = z.map_split(&[0]).expect("split");
    let pts = PointSet::from_rows([[2.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 10.0);
}

#[test]
fn shift_split_picks_axis_offset() {
    let s = ShiftMap::new(vec![10.0, 20.0, 30.0]);
    let (outs, sub) = s.map_split(&[2, 0]).expect("split");
    assert_eq!(outs, vec![2, 0]);
    let pts = PointSet::from_rows([[5.0, 1.0]]);
    let out = sub.transform(&pts, true).unwrap();
    // Input axis 0 → applies shift[2]=30; input axis 1 → applies shift[0]=10.
    assert_eq!(out.point(0)[0], 35.0);
    assert_eq!(out.point(0)[1], 11.0);
}
```

(Add similar tests for `WinMap`, `MatrixMap`, `PermMap`, `WcsMap`, and `CmpMap` as those impls land in subsequent tasks. Task 1.6, 1.7, 1.8, 1.9 each append their own tests.)

- [ ] **Step 8: Run the tests**

```bash
cargo test -p ast-core --test test_map_split
```

Expected: all tests added in Step 7 pass. Tests for unimplemented classes (CmpMap nontrivial, MatrixMap, etc.) are absent — they get added in their own tasks.

- [ ] **Step 9: Commit Task 1.1**

```bash
git add crates/ast-core/src/mappings/mod.rs \
        crates/ast-core/src/mappings/unit.rs \
        crates/ast-core/src/mappings/zoom.rs \
        crates/ast-core/src/mappings/shift.rs \
        crates/ast-core/src/mappings/cmpmap.rs \
        crates/ast-core/tests/test_map_split.rs
git commit -m "$(cat <<'EOF'
mappings: reshape map_split to mirror astMapSplit

Trait method now takes input axes to retain and returns both the
dependent output axes and the sub-mapping. Default impl mirrors
mapping.c::MapSplit @ 7798 (full-input case via PermMap+CmpMap).
UnitMap, ZoomMap, ShiftMap get faithful per-class impls; CmpMap
gets a stub (Series/Parallel todo!()) that satisfies the trait
until Task 1.6.

Test file rewritten to the new semantics. Existing
contiguous-output-range API had no production callers; safely
retired.

Mirrors fitschan.c:28775 / 28866 callsites that the upcoming
SIPIntWorld port needs.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 1.2: WinMap impl

**Files:**
- Modify: `crates/ast-core/src/mappings/winmap.rs`
- Test: `crates/ast-core/tests/test_map_split.rs` (append)

**C reference:** `winmap.c::MapSplit` ~line 254 / body around line 1100. WinMap stores per-axis (a, b) pairs; split picks the chosen axes' (a, b) and constructs a smaller WinMap.

- [ ] **Step 1: Add the failing test**

Append to `crates/ast-core/tests/test_map_split.rs`:

```rust
#[test]
fn win_split_picks_chosen_axes() {
    use ast_core::mappings::WinMap;
    // 3-axis WinMap: y_i = a_i + b_i * x_i
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![10.0, 20.0, 30.0];
    let w = WinMap::new_with_coeffs(a, b).unwrap();
    let (outs, sub) = w.map_split(&[2, 0]).expect("split");
    assert_eq!(outs, vec![2, 0]);
    assert_eq!(sub.nin(), 2);
    let pts = PointSet::from_rows([[5.0, 1.0]]);
    let out = sub.transform(&pts, true).unwrap();
    // input pos 0 → axis 2 → 3 + 30*5 = 153; input pos 1 → axis 0 → 1 + 10*1 = 11.
    assert!((out.point(0)[0] - 153.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 11.0).abs() < 1e-12);
}
```

(If `WinMap::new_with_coeffs` doesn't exist, find the constructor that takes per-axis a/b and use it; or pass the equivalent two corner points. Read `crates/ast-core/src/mappings/winmap.rs` constructors.)

- [ ] **Step 2: Verify it fails**

```bash
cargo test -p ast-core --test test_map_split win_split
```

Expected: FAIL — default impl rejects partial input selection.

- [ ] **Step 3: Implement `map_split` on WinMap**

In `crates/ast-core/src/mappings/winmap.rs`, add the method to its `impl Mapping for WinMap` block:

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors winmap.c::MapSplit (line ~1100). Pick the chosen axes'
        // (a, b) coefficients and build a smaller WinMap.
        let nin = self.nin();
        for (i, &ix) in in_axes.iter().enumerate() {
            if ix >= nin || in_axes[..i].contains(&ix) {
                return None;
            }
        }
        let a: Vec<f64> = in_axes.iter().map(|&ix| self.a()[ix]).collect();
        let b: Vec<f64> = in_axes.iter().map(|&ix| self.b()[ix]).collect();
        let mut sub = WinMap::new_with_coeffs(a, b).ok()?;
        if self.is_inverted() {
            sub.invert();
        }
        Some((in_axes.to_vec(), Box::new(sub)))
    }
```

(If the `a()` / `b()` accessors don't exist on WinMap, add them as `pub(crate)` first — they're needed here.)

- [ ] **Step 4: Run test**

```bash
cargo test -p ast-core --test test_map_split win_split
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/winmap.rs crates/ast-core/tests/test_map_split.rs
git commit -m "mappings(winmap): map_split picks per-axis (a, b) coefficients

Mirrors winmap.c::MapSplit. Adds the Mapping::map_split impl plus
its test. Pure structural mirror; uses WinMap's existing per-axis
storage.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.3: MatrixMap impl

**Files:**
- Modify: `crates/ast-core/src/mappings/matrix.rs`
- Test: `crates/ast-core/tests/test_map_split.rs` (append)

**C reference:** `matrixmap.c::MapSplit` ~line 306 / body further down. MatrixMap is `y = M * x` for a square matrix M. Split is only possible when the chosen input columns of M feed into a *disjoint* set of output rows — i.e., the off-block entries are zero.

- [ ] **Step 1: Add the failing tests**

Append to `crates/ast-core/tests/test_map_split.rs`:

```rust
#[test]
fn matrix_split_diagonal_picks_axis() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Diagonal — every axis splits cleanly.
    let m = array![[2.0_f64, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 5.0]];
    let mm = MatrixMap::new_full(m).unwrap();
    let (outs, sub) = mm.map_split(&[1]).expect("split");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
}

#[test]
fn matrix_split_block_diagonal() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Block-diagonal — first 2 axes coupled, third independent.
    let m = array![
        [1.0_f64, 2.0, 0.0],
        [3.0,     4.0, 0.0],
        [0.0,     0.0, 5.0],
    ];
    let mm = MatrixMap::new_full(m).unwrap();
    let (outs, sub) = mm.map_split(&[2]).expect("split");
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 1);
}

#[test]
fn matrix_split_coupled_returns_none() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Fully coupled: picking a subset of inputs is impossible.
    let m = array![[1.0_f64, 1.0], [1.0, 1.0]];
    let mm = MatrixMap::new_full(m).unwrap();
    assert!(mm.map_split(&[0]).is_none());
}
```

(Replace `MatrixMap::new_full` with the actual constructor name from `crates/ast-core/src/mappings/matrix.rs`.)

- [ ] **Step 2: Verify failures**

```bash
cargo test -p ast-core --test test_map_split matrix_split
```

- [ ] **Step 3: Implement `map_split` on MatrixMap**

In `crates/ast-core/src/mappings/matrix.rs`, port `matrixmap.c::MapSplit`. The algorithm:

1. For each output row `j`, compute `feeds[j]` = set of input columns where M[j][i] != 0.
2. Let `kept_in = set(in_axes)`. An output `j` is "in scope" iff `feeds[j] ⊆ kept_in`.
3. An output `j` is "out of scope" iff `feeds[j] ∩ kept_in == ∅`.
4. If any output is *neither* fully in scope nor fully out of scope, return None (cross-coupled).
5. Build the sub-matrix by selecting in-scope rows (in original order) and `in_axes` columns.

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors matrixmap.c::MapSplit. A MatrixMap split succeeds only
        // when the input/output dependency graph is block-diagonal w.r.t.
        // the chosen inputs.
        let nin = self.nin();
        let nout = self.nout();
        for (i, &ix) in in_axes.iter().enumerate() {
            if ix >= nin || in_axes[..i].contains(&ix) {
                return None;
            }
        }
        let m = self.matrix();
        let kept: std::collections::HashSet<usize> = in_axes.iter().copied().collect();

        let mut kept_outs: Vec<usize> = Vec::new();
        for j in 0..nout {
            let mut in_scope = false;
            let mut out_scope = false;
            for i in 0..nin {
                if m[[j, i]] != 0.0 {
                    if kept.contains(&i) {
                        in_scope = true;
                    } else {
                        out_scope = true;
                    }
                }
            }
            match (in_scope, out_scope) {
                (true, true) => return None,    // cross-coupled
                (true, false) => kept_outs.push(j),
                _ => {}
            }
        }

        // Build sub-matrix.
        let mut sub_m = ndarray::Array2::<f64>::zeros((kept_outs.len(), in_axes.len()));
        for (rj, &j) in kept_outs.iter().enumerate() {
            for (ri, &i) in in_axes.iter().enumerate() {
                sub_m[[rj, ri]] = m[[j, i]];
            }
        }
        let mut sub = MatrixMap::new_full(sub_m).ok()?;
        if self.is_inverted() {
            sub.invert();
        }
        Some((kept_outs, Box::new(sub)))
    }
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p ast-core --test test_map_split matrix_split
```

Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/mappings/matrix.rs crates/ast-core/tests/test_map_split.rs
git commit -m "mappings(matrix): map_split block-diagonalises against chosen inputs

Mirrors matrixmap.c::MapSplit — succeeds only when each output
row's nonzero columns are either entirely in the kept-input set
or entirely outside it. Returns the in-scope output indices plus
the sub-matrix.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.4: PermMap impl

**Files:**
- Modify: `crates/ast-core/src/mappings/perm.rs`
- Test: `crates/ast-core/tests/test_map_split.rs` (append)

**C reference:** `permmap.c::MapSplit` ~line 1438. PermMap permutes / drops / inserts axes via `inperm` and `outperm` arrays. Split picks outputs that depend on the kept inputs through the inverse permutation.

- [ ] **Step 1: Add failing tests**

Append:

```rust
#[test]
fn perm_split_simple() {
    use ast_core::mappings::PermMap;
    // Swap axes 0 and 1.
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let (outs, sub) = pm.map_split(&[1]).expect("split");
    // input 1 → output 0 (because outperm[0] = 2 means output 0 takes input 1)
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
}

#[test]
fn perm_split_with_constants_returns_none() {
    use ast_core::mappings::PermMap;
    // outperm has a negative entry → output is a constant; not splittable.
    let pm = PermMap::new(&[1], &[1, -1], &[42.0]).unwrap();
    assert!(pm.map_split(&[0]).is_none());
}
```

- [ ] **Step 2: Verify failures**

```bash
cargo test -p ast-core --test test_map_split perm_split
```

- [ ] **Step 3: Implement**

Read `permmap.c::MapSplit` body (around line 1438-1550) before writing. The algorithm walks outperm: for each output `j`, `outperm[j]` is the source input index (1-based) or a negative pointer to a constant. An output is in scope iff its source input is in `in_axes`.

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors permmap.c::MapSplit (line 1438). PermMap with constants
        // (negative outperm entries) cannot be split — kept outputs
        // depend on a constant table that the sub-mapping wouldn't
        // carry over reliably.
        let nin = self.nin();
        for (i, &ix) in in_axes.iter().enumerate() {
            if ix >= nin || in_axes[..i].contains(&ix) {
                return None;
            }
        }

        let outperm = self.outperm();   // 1-based, negative = constant
        let kept: std::collections::HashSet<usize> = in_axes.iter().copied().collect();

        let mut kept_outs: Vec<usize> = Vec::new();
        let mut sub_outperm: Vec<i64> = Vec::new();
        for (j, &op) in outperm.iter().enumerate() {
            if op > 0 {
                let src = (op as usize) - 1;
                if kept.contains(&src) {
                    kept_outs.push(j);
                    let new_pos = in_axes.iter().position(|&ix| ix == src).unwrap();
                    sub_outperm.push((new_pos + 1) as i64);
                }
            } else {
                // Constant output: not splittable per C semantics.
                return None;
            }
        }
        if kept_outs.is_empty() {
            return None;
        }

        // Build sub-PermMap.
        let sub_inperm: Vec<i64> = kept_outs
            .iter()
            .enumerate()
            .map(|(new_in_pos, _)| (new_in_pos + 1) as i64)
            .collect();
        // Note: sub_inperm is the inverse direction; check semantics against
        // your existing PermMap constructor signature.
        let sub = PermMap::new(&sub_inperm, &sub_outperm, &[]).ok()?;
        Some((kept_outs, Box::new(sub)))
    }
```

(The exact `inperm`/`outperm` directionality may differ; verify against `permmap.rs` and the C source. Adjust accordingly.)

- [ ] **Step 4: Run tests**

- [ ] **Step 5: Commit**

```bash
git commit -am "mappings(perm): map_split walks outperm to find dependent outputs

Mirrors permmap.c::MapSplit. Outputs whose source is in kept inputs
join the sub-mapping; constant outputs (negative outperm entries)
make the split fail.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.5: WcsMap impl

**Files:**
- Modify: `crates/ast-core/src/mappings/wcsmap.rs`
- Test: `crates/ast-core/tests/test_map_split.rs` (append)

**C reference:** `wcsmap.c::MapSplit` ~line 808 / body around line 1730. WcsMap is 2-in/2-out (the projection's longitude/latitude pair plus pass-through for any extra axes — actually our WcsMap is strictly 2-in/2-out for the projection axes; we don't carry pass-through).

- [ ] **Step 1: Add tests**

```rust
#[test]
fn wcs_split_both_axes_succeeds() {
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(WcsProjection::Tan, 2, 1, 2).unwrap();
    let (outs, sub) = wm.map_split(&[0, 1]).expect("split");
    assert_eq!(outs, vec![0, 1]);
    assert_eq!(sub.nin(), 2);
}

#[test]
fn wcs_split_one_axis_returns_none() {
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(WcsProjection::Tan, 2, 1, 2).unwrap();
    assert!(wm.map_split(&[0]).is_none());
    assert!(wm.map_split(&[1]).is_none());
}
```

(Adjust constructor args to match your `WcsMap::new` signature.)

- [ ] **Step 2-5: implement and verify**

```rust
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors wcsmap.c::MapSplit. The projection couples the two
        // input axes inseparably; only a full split (both axes) succeeds.
        let nin = self.nin();
        for (i, &ix) in in_axes.iter().enumerate() {
            if ix >= nin || in_axes[..i].contains(&ix) {
                return None;
            }
        }
        if in_axes.len() != nin {
            return None;
        }
        // Hand off to default-impl-style PermMap+self composition for
        // axis reordering. (Or just check that in_axes is the identity
        // permutation; in practice C's wcsmap.c only succeeds when both
        // axes are kept and in their original order.)
        if in_axes != [0, 1] {
            return None;
        }
        Some((vec![0, 1], self.clone_box()))
    }
```

- [ ] **Step 5: Commit**

```bash
git commit -am "mappings(wcs): map_split only succeeds for full identity input

Mirrors wcsmap.c::MapSplit. WcsMap couples its lon/lat axes
inseparably, so any partial input selection fails.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.6: CmpMap series + parallel impls

**Files:**
- Modify: `crates/ast-core/src/mappings/cmpmap.rs`
- Test: `crates/ast-core/tests/test_map_split.rs` (append)

**C reference:** `cmpmap.c::MapSplit` ~line 4163. The most complex of the per-class impls; about 250 lines of C.

This task replaces the `todo!()` stubs from Task 1.1's CmpMap update.

- [ ] **Step 1: Add tests covering series and parallel cases**

```rust
#[test]
fn cmpmap_parallel_split_per_child() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap};
    // Parallel of two ShiftMaps: axes 0,1 from first; 2,3 from second.
    let s1 = ShiftMap::new(vec![1.0, 2.0]);
    let s2 = ShiftMap::new(vec![10.0, 20.0]);
    let cm = CmpMap::new(Box::new(s1), Box::new(s2), Composition::Parallel).unwrap();
    let (outs, _sub) = cm.map_split(&[2]).expect("split second-child axis");
    assert_eq!(outs, vec![2]);
    let (outs2, _) = cm.map_split(&[0, 3]).expect("split mixed");
    assert_eq!(outs2, vec![0, 3]);
}

#[test]
fn cmpmap_series_split_threads_through() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap, ZoomMap};
    // shift(3) → zoom(3, 5): linear chain, every axis independent.
    let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    let z = ZoomMap::new(3, 5.0).unwrap();
    let cm = CmpMap::new(Box::new(s), Box::new(z), Composition::Series).unwrap();
    let (outs, sub) = cm.map_split(&[1]).expect("split");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    let pts = PointSet::from_rows([[3.0]]);
    let out = sub.transform(&pts, true).unwrap();
    // (3 + 2) * 5 = 25
    assert!((out.point(0)[0] - 25.0).abs() < 1e-12);
}
```

- [ ] **Step 2: Verify failures**

- [ ] **Step 3: Implement series**

Read `cmpmap.c::MapSplit` lines ~4250-4350. The algorithm:

```rust
            Composition::Series => {
                // first.nout() == second.nin() in series.
                // Split second by in_axes (which are first.nin's inputs):
                //   Step A: split self.first by in_axes, getting
                //     (first_outs, first_sub). first_outs are indices into
                //     self.first.nout() == self.second.nin().
                //   Step B: split self.second by first_outs, getting
                //     (second_outs, second_sub). second_outs are indices
                //     into self.second.nout() == self.nout().
                //   Step C: compose first_sub then second_sub in series.
                let (first_outs, first_sub) = self.first.map_split(in_axes)?;
                let (second_outs, second_sub) = self.second.map_split(&first_outs)?;
                let composed = CmpMap::new(first_sub, second_sub, Composition::Series).ok()?;
                Some((second_outs, Box::new(composed)))
            }
```

- [ ] **Step 4: Implement parallel**

Read `cmpmap.c::MapSplit` lines ~4350-4450:

```rust
            Composition::Parallel => {
                // self.first feeds inputs 0..first.nin().
                // self.second feeds inputs first.nin()..nin().
                let nin1 = self.first.nin();
                let mut first_inputs: Vec<usize> = Vec::new();
                let mut second_inputs: Vec<usize> = Vec::new();
                for &ix in in_axes {
                    if ix < nin1 {
                        first_inputs.push(ix);
                    } else {
                        second_inputs.push(ix - nin1);
                    }
                }
                // Recursively split each child.
                let mut combined_outs: Vec<usize> = Vec::new();
                let nout1 = self.first.nout();
                let first_part: Option<(Vec<usize>, DynMapping)> = if first_inputs.is_empty() {
                    None
                } else {
                    let (outs, sub) = self.first.map_split(&first_inputs)?;
                    combined_outs.extend(outs.iter().copied());
                    Some((outs, sub))
                };
                let second_part: Option<(Vec<usize>, DynMapping)> = if second_inputs.is_empty() {
                    None
                } else {
                    let (outs, sub) = self.second.map_split(&second_inputs)?;
                    combined_outs.extend(outs.iter().map(|&o| o + nout1));
                    Some((outs, sub))
                };
                // Compose results based on which children participated.
                let result_map: DynMapping = match (first_part, second_part) {
                    (Some((_, fsub)), Some((_, ssub))) => {
                        Box::new(CmpMap::new(fsub, ssub, Composition::Parallel).ok()?)
                    }
                    (Some((_, fsub)), None) => fsub,
                    (None, Some((_, ssub))) => ssub,
                    (None, None) => return None,
                };
                Some((combined_outs, result_map))
            }
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p ast-core --test test_map_split cmpmap
```

- [ ] **Step 6: Run the FULL map_split test file to catch regressions**

```bash
cargo test -p ast-core --test test_map_split
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git commit -am "mappings(cmpmap): map_split implements series + parallel splits

Mirrors cmpmap.c::MapSplit. Series threads in_axes through first
then second; parallel partitions in_axes by first.nin() then
recursively splits each child. Replaces the Task 1.1 todo!() stub.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.7: PolyMap default + remaining classes (TranMap, RateMap, NormMap)

**Files:**
- Modify: `crates/ast-core/src/mappings/{polymap,tranmap,ratemap,normmap}.rs` if needed.

**C references:**
- `polymap.c` — no MapSplit override. Uses default (full-input only).
- `tranmap.c::MapSplit` ~line 168.
- `ratemap.c::MapSplit` ~line 166.
- `normmap.c::MapSplit` ~line 175.

The default impl (added in Task 1.1) succeeds only when `in_axes.len() == nin()`. PolyMap inherits this default — confirm it does the right thing. TranMap/RateMap/NormMap have their own logic in C; port them.

- [ ] **Step 1: Add a test confirming PolyMap uses default behaviour**

```rust
#[test]
fn polymap_split_full_input_only() {
    use ast_core::mappings::{PolyMap, PolyTerm};
    // Identity-ish PolyMap: y0 = x0, y1 = x1.
    let fwd = vec![
        PolyTerm { coeff: 1.0, output: 1, powers: vec![1, 0] },
        PolyTerm { coeff: 1.0, output: 2, powers: vec![0, 1] },
    ];
    let pm = PolyMap::new(2, 2, &fwd, None).unwrap();
    assert!(pm.map_split(&[0]).is_none());
    assert!(pm.map_split(&[0, 1]).is_some());
}
```

(Adjust to your `PolyMap::new` constructor signature.)

- [ ] **Step 2: Run, verify it passes (default impl handles this)**

```bash
cargo test -p ast-core --test test_map_split polymap_split
```

- [ ] **Step 3: Port TranMap, RateMap, NormMap impls**

For each, read the corresponding C `MapSplit` and port it. These are short; each ~30-50 LoC. Add a quick test exercising each.

- [ ] **Step 4: Commit**

```bash
git commit -am "mappings: tranmap/ratemap/normmap map_split impls

Each mirrors its C counterpart. PolyMap inherits the default
(full-input-only split) per C polymap.c (no MapSplit override).

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 1.8: Group-1 finalisation

- [ ] **Step 1: Run full ast-core test suite**

```bash
cargo test -p ast-core 2>&1 | grep -E "^test result|FAILED"
```

Expected: every test result is `ok`. If anything is FAILED, that's a regression to fix before committing the group.

- [ ] **Step 2: Run clippy + fmt**

```bash
cargo fmt
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 3: Sanity-check the wcsconv suite is unchanged**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: still `152 passed, 40 failed`. Group 1 changes the trait but doesn't yet wire anything new in.

---

## Group 2: Mapping helper primitives (`linear_approx`, `analyse_poly`, `scale_poly_inputs`)

**Goal:** Port the three free-function primitives that `sip_int_world` depends on. Each has its own oracle fixture program for capturing reference values.

### Task 2.1: `linear_approx`

**Files:**
- Create: `crates/ast-core/src/mappings/helpers/linear_approx.rs`
- Modify: `crates/ast-core/src/mappings/helpers/mod.rs` (re-export)
- Create: `tools/oracle_fixtures/linear_approx.c`
- Test: inside `linear_approx.rs`

**C reference:** `mapping.c::astLinearApprox` (search `LinearApprox(` in mapping.c). About 200 lines. Algorithm:
1. Sample the mapping at the lower-bound corner, every `nin` corners adjacent along one axis, and the centre.
2. Solve for the affine fit `y = matrix·x + shift`.
3. Verify the fit reproduces the corner samples within `tol` per output axis (RMS gate).

- [ ] **Step 1: Read the C source**

```bash
grep -n "LinearApprox\|^.*LinearApprox(" reference/starlink-ast/src/mapping.c | head
```

Open the file; read the function body in full. Note the corner-sampling order — it determines the matrix-row order in the output. Do this BEFORE writing any Rust.

- [ ] **Step 2: Write the oracle fixture generator**

Create `tools/oracle_fixtures/linear_approx.c`:

```c
/*
 * Captures reference output values for astLinearApprox tests.
 *
 * For each test fixture, builds a small Mapping, calls
 * astLinearApprox over a chosen [lbnd, ubnd] box, and prints the
 * resulting `fit` array (or "None" if the fit fails) as a Rust
 * constant declaration suitable for pasting into linear_approx's
 * #[cfg(test)] mod tests.
 *
 * Usage:
 *   make linear_approx
 *   ./linear_approx > /tmp/linear_approx_fixtures.rs
 *   # paste captured constants into linear_approx.rs tests
 */

#include "ast.h"
#include <stdio.h>
#include <string.h>

static void emit_fit(const char *name, AstMapping *map,
                     double lbnd[], double ubnd[], double tol) {
    int status = 0;
    int astStatus_local = 0;
    int nout = astGetNout(map);
    int nin = astGetNin(map);
    double *fit = (double *)malloc(sizeof(double) * (nout + nin * nout));
    int ok = astLinearApprox(map, lbnd, ubnd, tol, fit);

    printf("    // Fixture: %s\n", name);
    if (!ok) {
        printf("    pub const %s: Option<&[f64]> = None;\n\n", name);
    } else {
        printf("    pub const %s: Option<&[f64]> = Some(&[\n", name);
        for (int i = 0; i < nout + nin * nout; ++i) {
            printf("        %.17g,\n", fit[i]);
        }
        printf("    ]);\n\n");
    }
    free(fit);
}

int main(void) {
    int status = 0;
    astBegin;

    printf("// Auto-generated by tools/oracle_fixtures/linear_approx.c.\n");
    printf("// Do not edit by hand — regenerate with `make linear_approx`.\n\n");

    /* Fixture 1: identity 2-in/2-out PolyMap (existing testmapping.c case). */
    {
        double coeff[] = {
            1.0, 1, 1, 0,
            1.0, 1, 0, 1,
            2.0, 1, 0, 0,
            3.0, 2, 0, 1,
        };
        AstPolyMap *pm = astPolyMap(2, 2, 4, coeff, 0, NULL, "");
        double lbnd[] = {-1.0, -1.0};
        double ubnd[] = {1.0, 1.0};
        emit_fit("LINEAR_APPROX_IDENTITY_POLY", (AstMapping *)pm, lbnd, ubnd, 0.001);
    }

    /* Fixture 2: pure ShiftMap. */
    {
        double shifts[] = {10.0, 20.0, 30.0};
        AstShiftMap *sm = astShiftMap(3, shifts, "");
        double lbnd[] = {0.0, 0.0, 0.0};
        double ubnd[] = {100.0, 100.0, 100.0};
        emit_fit("LINEAR_APPROX_SHIFT_3D", (AstMapping *)sm, lbnd, ubnd, 0.001);
    }

    /* Fixture 3: TAN WcsMap over a small box → should fit linearly. */
    {
        AstWcsMap *wm = astWcsMap(2, AST__TAN, 1, 2, "");
        double lbnd[] = {-0.01, -0.01};
        double ubnd[] = {0.01, 0.01};
        emit_fit("LINEAR_APPROX_TAN_TINY", (AstMapping *)wm, lbnd, ubnd, 0.001);
    }

    /* Fixture 4: TAN WcsMap over a large box → expected to fail. */
    {
        AstWcsMap *wm = astWcsMap(2, AST__TAN, 1, 2, "");
        double lbnd[] = {-1.0, -1.0};
        double ubnd[] = {1.0, 1.0};
        emit_fit("LINEAR_APPROX_TAN_LARGE", (AstMapping *)wm, lbnd, ubnd, 0.001);
    }

    /* Fixture 5: SIP-shape PolyMap from sip-3d header. */
    /* TODO: lift coefficients from
     *   crates/ast-core/tests/fixtures/wcsconv/inputs/sip-3d.head
     * and capture LINEAR_APPROX_SIP_OVER_FULL_BOX. */

    astEnd;
    return status;
}
```

- [ ] **Step 3: Build and capture**

```bash
cd tools/oracle_fixtures
make linear_approx
./linear_approx > /tmp/linear_approx_captured.rs
cat /tmp/linear_approx_captured.rs
cd ../..
```

- [ ] **Step 4: Create `crates/ast-core/src/mappings/helpers/linear_approx.rs`**

Skeleton with imports, a `LinearFit` struct, a public `linear_approx` function returning `Option<LinearFit>`, and a `#[cfg(test)] mod tests` block at the bottom containing the captured constants pasted from Step 3.

```rust
//! Linear-approximation fitting for `Mapping`s.
//!
//! Mirrors C `astLinearApprox` (mapping.c). Given a `Mapping` and a
//! sample box `[lbnd, ubnd]`, returns the best-fit affine map
//! `y = matrix · x + shift` if the fit is good (max RMS deviation
//! ≤ `tol` per output axis), `None` otherwise.

use crate::coords::PointSet;
use crate::mappings::Mapping;

/// Affine fit to a Mapping over a sample box.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearFit {
    /// Constant offset, length `nout`.
    pub shift: Vec<f64>,
    /// Linear matrix, row-major `nout × nin`.
    pub matrix: Vec<f64>,
}

/// Best-fit linear approximation to `map` over `[lbnd, ubnd]`.
///
/// Returns `Some(fit)` when the fit reproduces every corner sample
/// within `tol` per output axis; `None` otherwise. Mirrors
/// `astLinearApprox` (mapping.c).
pub fn linear_approx(
    map: &dyn Mapping,
    lbnd: &[f64],
    ubnd: &[f64],
    tol: f64,
) -> Option<LinearFit> {
    let nin = map.nin();
    let nout = map.nout();
    if lbnd.len() != nin || ubnd.len() != nin {
        return None;
    }
    // Step 1: build sampling points — lbnd corner, then each "nin"
    // corner adjacent to lbnd along one axis, then ubnd, then centre.
    // Mirrors mapping.c order.
    todo!("port mapping.c::astLinearApprox body — see C source")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{PolyMap, PolyTerm, ShiftMap};

    // Captured from tools/oracle_fixtures/linear_approx.c via
    // `make linear_approx > captured.rs`. Regenerate when the
    // upstream reference C library is rebuilt.
    //
    // PASTE CAPTURED CONSTS HERE
    //
    // pub const LINEAR_APPROX_IDENTITY_POLY: Option<&[f64]> = Some(&[...]);
    // pub const LINEAR_APPROX_SHIFT_3D: Option<&[f64]> = Some(&[...]);
    // pub const LINEAR_APPROX_TAN_TINY: Option<&[f64]> = Some(&[...]);
    // pub const LINEAR_APPROX_TAN_LARGE: Option<&[f64]> = None;

    fn fit_to_vec(fit: &LinearFit) -> Vec<f64> {
        let mut v = fit.shift.clone();
        v.extend_from_slice(&fit.matrix);
        v
    }

    fn assert_close(actual: &[f64], expected: &[f64], tol: f64, label: &str) {
        assert_eq!(actual.len(), expected.len(), "{} length mismatch", label);
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!(
                (a - e).abs() < tol,
                "{}: index {} actual={} expected={}",
                label, i, a, e
            );
        }
    }

    #[test]
    fn identity_polymap_2x2() {
        let pm = PolyMap::new(/* construct from coeffs in linear_approx.c */).unwrap();
        let fit = linear_approx(&pm, &[-1.0, -1.0], &[1.0, 1.0], 0.001);
        let expected = LINEAR_APPROX_IDENTITY_POLY.expect("oracle returned a fit");
        let got = fit.expect("rust returned a fit");
        assert_close(&fit_to_vec(&got), expected, 1e-12, "identity polymap");
    }

    #[test]
    fn shift_3d() {
        let sm = ShiftMap::new(vec![10.0, 20.0, 30.0]);
        let fit = linear_approx(&sm, &[0.0, 0.0, 0.0], &[100.0, 100.0, 100.0], 0.001);
        let expected = LINEAR_APPROX_SHIFT_3D.expect("oracle returned a fit");
        let got = fit.expect("rust returned a fit");
        assert_close(&fit_to_vec(&got), expected, 1e-12, "shift 3d");
    }

    #[test]
    fn tan_too_large_refuses() {
        // TAN over [-1, 1] in radians is highly non-linear → C refuses.
        // Construct a 2-in/2-out TAN WcsMap and verify Rust also refuses.
        // ...
        // assert!(LINEAR_APPROX_TAN_LARGE.is_none());
    }

    #[test]
    fn tan_tiny_box_fits() { /* … */ }
}
```

- [ ] **Step 5: Implement the body**

Read `mapping.c::astLinearApprox` and translate. The corner-sampling layout, the 4-step least-squares fit, and the per-output RMS check all need to be faithful. Total ~200 LoC.

- [ ] **Step 6: Run tests**

```bash
cargo test -p ast-core mappings::helpers::linear_approx
```

Expected: identity, shift_3d, tan_tiny pass; tan_too_large refuses (returns None).

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/mappings/helpers/linear_approx.rs \
        crates/ast-core/src/mappings/helpers/mod.rs \
        tools/oracle_fixtures/linear_approx.c
git commit -m "$(cat <<'EOF'
mappings: port astLinearApprox

Best-fit affine approximation to a Mapping over a sample box.
Mirrors mapping.c::astLinearApprox line-for-line. Returns
Option<LinearFit> with idiomatic Rust signature; internal control
flow follows the C source.

Tests use C-oracle-derived expected values captured via
tools/oracle_fixtures/linear_approx.c. Both the .c source and
the captured Rust constants are committed for traceability.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 2.2: `analyse_poly`

**Files:**
- Create: `crates/ast-core/src/mappings/helpers/analyse_poly.rs`
- Modify: `crates/ast-core/src/mappings/helpers/mod.rs` (re-export)
- Create: `tools/oracle_fixtures/analyse_poly.c`

**C reference:** `fitschan.c::AnalysePoly` (search the file). Decomposes a 2-in/2-out PolyMap into `(ShiftMap, canonical_PolyMap, MatrixMap)` such that the composition matches the original.

- [ ] **Step 1: Read C source**

```bash
grep -n "static.*AnalysePoly\|^.*AnalysePoly(" reference/starlink-ast/src/fitschan.c | head
```

Read the function body fully. Note that AnalysePoly EXTRACTS:
- A leading ShiftMap from the constant terms of the PolyMap.
- A canonical PolyMap with no constant terms and unit linear coefficients on each input's own variable.
- A trailing MatrixMap from any non-unit linear coefficients.

- [ ] **Step 2: Write the oracle fixture program**

Create `tools/oracle_fixtures/analyse_poly.c`. For each fixture polymap, run `AnalysePoly`, then dump:
- The shift values from the resulting ShiftMap.
- The polymap coefficients of the canonical middle term.
- The matrix elements of the trailing MatrixMap.

Output as Rust constants. Cover at minimum:
1. SIP-shape (no shift, identity linear, only quadratic+).
2. PolyMap with constant terms → expect non-zero shift.
3. PolyMap with non-unit linear → expect non-identity matrix.
4. Non-analyseable PolyMap → expect None.

(The oracle program structure mirrors `linear_approx.c`. Adapt the same pattern.)

- [ ] **Step 3: Capture fixtures**

```bash
cd tools/oracle_fixtures && make analyse_poly && ./analyse_poly > /tmp/analyse_poly_captured.rs && cd ../..
```

- [ ] **Step 4: Create `analyse_poly.rs`**

```rust
//! Decompose a 2-in/2-out PolyMap into shift / canonical / matrix.
//!
//! Mirrors fitschan.c::AnalysePoly.

use crate::mappings::{MatrixMap, PolyMap, ShiftMap};

/// Decompose a 2-in/2-out PolyMap into three serial components such that
/// `original.transform(x) == matrix.transform(canonical.transform(shift.transform(x)))`.
///
/// Returns `None` if the polymap can't be put into the canonical SIP shape.
/// Mirrors fitschan.c::AnalysePoly.
pub fn analyse_poly(pm: &PolyMap) -> Option<(ShiftMap, PolyMap, MatrixMap)> {
    if pm.nin() != 2 || pm.nout() != 2 {
        return None;
    }
    todo!("port fitschan.c::AnalysePoly body — see C source")
}

#[cfg(test)]
mod tests {
    use super::*;

    // PASTE CAPTURED CONSTS HERE.

    #[test]
    fn sip_shape_polymap() { /* … */ }

    #[test]
    fn polymap_with_constants_extracts_shift() { /* … */ }

    #[test]
    fn polymap_with_linear_extracts_matrix() { /* … */ }

    #[test]
    fn non_analyseable_returns_none() { /* … */ }
}
```

- [ ] **Step 5: Implement, test, commit**

Same TDD loop as Task 2.1.

```bash
git add crates/ast-core/src/mappings/helpers/analyse_poly.rs \
        crates/ast-core/src/mappings/helpers/mod.rs \
        tools/oracle_fixtures/analyse_poly.c
git commit -m "mappings: port AnalysePoly

Decomposes a 2-in/2-out PolyMap into (ShiftMap, canonical
SIP-shape PolyMap, MatrixMap). Used by the SIPIntWorld port to
strip non-canonical structure off a SIP polymap before extracting
header coefficients. Mirrors fitschan.c::AnalysePoly.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.3: `scale_poly_inputs`

**Files:**
- Create: `crates/ast-core/src/mappings/helpers/scale_poly_inputs.rs`
- Modify: `crates/ast-core/src/mappings/helpers/mod.rs` (re-export)
- Create: `tools/oracle_fixtures/scale_poly_inputs.c`

**C reference:** `fitschan.c::ScalePolyInputs` (search). Rebuilds a PolyMap so its inputs are pre-scaled by `scales[i]` while producing the same outputs.

Same structure as Task 2.2. Single test fixture set: 2-3 PolyMaps × 2-3 scale vectors, captured.

- [ ] **Step 1-7: standard TDD loop with oracle fixtures**

```rust
pub fn scale_poly_inputs(pm: &PolyMap, scales: &[f64]) -> PolyMap {
    // Mirrors fitschan.c::ScalePolyInputs. For each polynomial term,
    // multiply its coefficient by ∏ scales[input_i]^power_i.
    todo!("port fitschan.c::ScalePolyInputs body")
}
```

```bash
git commit -am "mappings: port ScalePolyInputs

Rebuild a PolyMap with input axes pre-scaled. Multiplies each
term's coefficient by the product of scales raised to that
term's per-input power. Used by SIPIntWorld when factoring a
non-unit lower-mapping scaling into the polymap.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 2.4: Group-2 finalisation

- [ ] **Step 1: Verify all helpers tests pass**

```bash
cargo test -p ast-core mappings::helpers
```

- [ ] **Step 2: clippy + fmt**

- [ ] **Step 3: Confirm wcsconv unchanged**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: `152 passed, 40 failed`.

---

## Group 3: `wcs_keywords/` skeleton + `other_axes`

**Goal:** Stand up the new `wcs_keywords/` submodule and land the simplest top-level port (`other_axes`) end-to-end. Establishes the file-layout pattern and tests in isolation; not yet wired into the writer pipeline.

### Task 3.1: Create the submodule skeleton

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`
- Modify: `crates/ast-core/src/fitschan/mod.rs` (declare `wcs_keywords`)

- [ ] **Step 1: Create `wcs_keywords/mod.rs`**

```rust
//! Port of FsetToStore + its callees (CelestialAxes, SpectralAxes, OtherAxes,
//! MakeIntWorld, SIPIntWorld). Builds a FitsStore from a FrameSet by
//! constructing the wcs→iwc and pixel→iwc mappings from chosen header
//! values, then extracting CRPIX/CD/PC/CDELT via a least-squares fit.
//!
//! See docs/superpower/specs/2026-05-25-makeintworld-port-design.md.
//!
//! Mirrors fitschan.c (function references on each item).

mod other_axes;
// Submodules added in subsequent tasks:
// mod celestial_axes;
// mod spectral_axes;
// mod make_int_world;
// mod sip_int_world;
// mod orth;
// mod invertable;
// mod sky_sys;

pub(crate) use other_axes::other_axes;

// Top-level entry — added in Group 7.
// pub(crate) fn fset_to_store(...) -> AstResult<FitsStore> { ... }
```

- [ ] **Step 2: Declare the submodule in `fitschan/mod.rs`**

In `crates/ast-core/src/fitschan/mod.rs`, near the other submodule declarations, add:

```rust
mod wcs_keywords;
```

- [ ] **Step 3: Build and confirm nothing breaks**

```bash
cargo build -p ast-core 2>&1 | tail -5
```

Expected: clean build (Other_axes doesn't exist yet but mod.rs only references the submodule, not the function — but actually our mod.rs has `pub(crate) use other_axes::other_axes;` which WILL fail).

Adjust: comment out the `pub(crate) use` line until Task 3.2 lands.

- [ ] **Step 4: Commit skeleton**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords/mod.rs \
        crates/ast-core/src/fitschan/mod.rs
git commit -m "fitschan: stand up wcs_keywords submodule skeleton

Adds the home for the FsetToStore / MakeIntWorld port. Submodule
declarations are commented out until each port lands (Tasks 3.2,
4.x, 5.x, 6.x). See spec
docs/superpower/specs/2026-05-25-makeintworld-port-design.md.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 3.2: Port `other_axes`

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/other_axes.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs` (uncomment re-export)

**C reference:** `fitschan.c::OtherAxes` @ line 24030. ~180 executable lines. Iterates over WCS frame axes that haven't been claimed by celestial or spectral, picks per-axis CRVAL (input override or `world(central_pixel)`), stores CTYPE/CUNIT/CRVAL, builds a `wcs→iwc` ShiftMap component for each axis.

- [ ] **Step 1: Read the C function carefully**

```bash
sed -n '24030,24420p' reference/starlink-ast/src/fitschan.c | less
```

Understand:
- The `axis_done` array tracking.
- How `crvals[i]` (input override) interacts with `world(central_pixel)`.
- How the returned Mapping is assembled (UnitMap on the rest plus per-axis ShiftMap on the handled axes).

- [ ] **Step 2: Sketch the Rust signature**

In `crates/ast-core/src/fitschan/wcs_keywords/other_axes.rs`:

```rust
//! Port of fitschan.c::OtherAxes (~line 24030).
//!
//! Catch-all for WCS frame axes that aren't celestial or spectral.
//! Stores CTYPE/CUNIT/CRVAL for each such axis and returns a
//! `wcs→iwc` mapping that's a per-axis ShiftMap (subtracts CRVAL),
//! identity on the already-handled axes.

use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::FrameSet;
use crate::mappings::DynMapping;

/// Parameters carried through the axis-classifier sequence
/// (celestial → spectral → other), each updated in place.
pub(crate) struct OtherAxesArgs<'a> {
    pub fs: &'a FrameSet,
    pub dim: &'a [Option<f64>],
    pub wperm: &'a mut [usize],
    pub store: &'a mut FitsStore,
    pub crvals: &'a mut [Option<f64>],
    pub axis_done: &'a mut [bool],
}

/// Returns the wcs→iwc mapping for unclaimed (non-celestial,
/// non-spectral) axes. Stores CTYPE/CUNIT/CRVAL in `args.store`.
pub(crate) fn other_axes(args: OtherAxesArgs<'_>) -> AstResult<DynMapping> {
    todo!("port fitschan.c::OtherAxes — see C source")
}
```

- [ ] **Step 3: Add a unit test that exercises a simple case**

In the same file, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{BasicFrameBuilder, FrameRef};
    use crate::mappings::{Mapping, ShiftMap, UnitMap};

    /// A 2-axis FrameSet with both axes unclaimed → other_axes builds
    /// a per-axis ShiftMap, sets CTYPE/CUNIT/CRVAL.
    #[test]
    fn two_axes_basic_frame() {
        // Build pixel-frame and wcs-frame as 2-axis BasicFrames.
        let pixel = BasicFrameBuilder::new(2)
            .domain("GRID")
            .label(1, "pixel x")
            .label(2, "pixel y")
            .build()
            .unwrap();
        let wcs = BasicFrameBuilder::new(2)
            .label(1, "world x")
            .label(2, "world y")
            .unit(1, "m")
            .unit(2, "m")
            .build()
            .unwrap();
        let mut fs = crate::frames::FrameSet::new(Box::new(pixel));
        // Identity pixel→world for the test.
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(wcs))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut crvals: Vec<Option<f64>> = vec![None, None];
        let mut axis_done = vec![false; 2];

        let mapping = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Both axes should be done.
        assert!(axis_done[0] && axis_done[1]);
        // CTYPE / CUNIT / CRVAL set.
        assert_eq!(store.ctype[0].as_deref(), Some("world x"));
        assert_eq!(store.cunit[0].as_deref(), Some("m"));
        assert!(store.crval[0].is_some());
        // Mapping has 2-in / 2-out.
        assert_eq!(mapping.nin(), 2);
        assert_eq!(mapping.nout(), 2);
    }
}
```

- [ ] **Step 4: Run the test, watch it fail**

```bash
cargo test -p ast-core fitschan::wcs_keywords::other_axes
```

Expected: FAIL with `todo!()` panic or compile error if signatures don't quite match. Adjust signatures to match the actual `FitsStore` field names.

- [ ] **Step 5: Implement the body**

Translate `fitschan.c::OtherAxes`. Key control flow:

1. Iterate axes in WCS frame order.
2. Skip if `axis_done[i]` is already true.
3. Get the axis's primary frame (handle CmpFrame → recursive frame).
4. Choose CRVAL: `crvals[i]` if `Some`, else `world(central_pixel)[i]`.
5. Store `CTYPE` (axis label or symbol → FITS type code), `CUNIT`, `CRVAL`.
6. Build a `ShiftMap(-crval)` for that axis, compose into the running wcs→iwc.
7. Mark `axis_done[i] = true`.

Read the C carefully — it has a few special cases (PIXEL frames, etc.) that need to be preserved.

- [ ] **Step 6: Run the test until it passes**

```bash
cargo test -p ast-core fitschan::wcs_keywords::other_axes
```

- [ ] **Step 7: Add additional fixtures for non-trivial cases**

Append tests for: 1D BasicFrame, partial pre-claimed axes, frame with CUNIT.

- [ ] **Step 8: Uncomment the `pub(crate) use` in `mod.rs`**

```rust
pub(crate) use other_axes::other_axes;
```

- [ ] **Step 9: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords/other_axes.rs \
        crates/ast-core/src/fitschan/wcs_keywords/mod.rs
git commit -m "$(cat <<'EOF'
fitschan/wcs_keywords: port OtherAxes

Catch-all builder for WCS frame axes not handled by
celestial_axes or spectral_axes. Builds a per-axis ShiftMap
(subtracts chosen CRVAL) for the wcs→iwc segment, stores CTYPE /
CUNIT / CRVAL in the FitsStore. Mirrors fitschan.c::OtherAxes
@ line 24030.

Tested standalone against a 2-axis BasicFrame fixture; not yet
wired into the writer pipeline (lands in Group 7).

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 3.3: Group-3 finalisation

- [ ] **Step 1: Run full ast-core test suite**

```bash
cargo test -p ast-core 2>&1 | grep -E "^test result|FAILED"
```

Expected: every result `ok`. The new `other_axes` test passes; nothing else changes.

- [ ] **Step 2: clippy + fmt**

- [ ] **Step 3: wcsconv baseline unchanged**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: `152 passed, 40 failed`.

---

## Group 4: `make_int_world` + supporting helpers

**Goal:** Port the central CRPIX/CD extractor (without the SIP branch yet) plus its column-orthogonalisation and invertability helpers. Wire in `find_basis_vectors` / `fit_line_cd` (currently `#[allow(dead_code)]`).

### Task 4.1: `orth_vector` + `orth_vector_set`

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/orth.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:**
- `fitschan.c::OrthVectorSet` ~line 23938.
- `fitschan.c::OrthVector` (search the file). Returns a unit vector orthogonal to the supplied set.

- [ ] **Step 1: Read the C source**

```bash
grep -n "static.*OrthVector\b\|static.*OrthVectorSet\b" reference/starlink-ast/src/fitschan.c
```

Read both function bodies. `OrthVector` is the workhorse; `OrthVectorSet` is the loop.

- [ ] **Step 2: Capture oracle fixtures**

Create `tools/oracle_fixtures/orth_vector_set.c` that supplies several `m`-of-`n` partial vector sets and prints the C-computed orthogonal completion. At minimum:
- 2-of-3 (independent vectors → expect specific 3rd unit-perp)
- 1-of-2 trivial case
- Degenerate input (parallel partial vectors) → expect None

```bash
cd tools/oracle_fixtures && make orth_vector_set && ./orth_vector_set > /tmp/orth_captured.rs && cd ../..
```

- [ ] **Step 3: Create `orth.rs`**

```rust
//! Orthogonal-vector completion of a partial vector set.
//!
//! Mirrors fitschan.c::OrthVectorSet + OrthVector. Used by
//! `make_int_world` to fill in CD columns when the supplied
//! Mapping has nout > nin.

use ndarray::Array2;

/// Find a unit vector orthogonal to every vector currently in
/// `vectors[..k]` (where `vectors` is a `n × n` square buffer).
/// Returns `None` if the partial set already spans `n` dimensions
/// or contains a degenerate combination.
///
/// Mirrors fitschan.c::OrthVector.
pub(super) fn orth_vector(vectors: &[Vec<f64>], n: usize) -> Option<Vec<f64>> {
    todo!("port fitschan.c::OrthVector body")
}

/// Given `m` vectors of length `n` (where `m ≤ n`), return a set
/// of `n` mutually orthogonal vectors whose first `m` entries are
/// the supplied vectors and whose remaining entries are unit
/// vectors orthogonal to all the others. Returns `None` if no
/// such completion exists.
///
/// Mirrors fitschan.c::OrthVectorSet @ ~line 23938.
pub(super) fn orth_vector_set(in_vectors: &[Vec<f64>], n: usize) -> Option<Vec<Vec<f64>>> {
    todo!("port fitschan.c::OrthVectorSet body")
}

#[cfg(test)]
mod tests {
    use super::*;

    // PASTE CAPTURED CONSTS HERE.

    #[test]
    fn two_of_three_completes() {
        // Two independent vectors in 3D → expect specific orthogonal third.
    }

    #[test]
    fn parallel_partial_returns_none() { /* … */ }
}
```

- [ ] **Step 4-7: TDD loop. Implement, run tests, commit.**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords/orth.rs \
        crates/ast-core/src/fitschan/wcs_keywords/mod.rs \
        tools/oracle_fixtures/orth_vector_set.c
git commit -m "fitschan/wcs_keywords: port OrthVectorSet + OrthVector

Pads a partial vector set with mutually-orthogonal unit vectors
to fill an n-dim square. Used by make_int_world when nout > nin.
Mirrors fitschan.c::OrthVectorSet @ 23938 + OrthVector.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 4.2: `make_invertable`

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/invertable.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:** `fitschan.c::MakeInvertable` @ line 22314. ~30 lines. Patches a square CD matrix to be invertible by setting diagonal to 1 for inputs that have no effect on any output, when those inputs are degenerate (single-pixel) axes.

- [ ] **Step 1-5: TDD loop**

Hand-rolled tests (no oracle fixtures needed; algorithm is trivial):

```rust
#[test]
fn already_invertable_unchanged() { /* … */ }

#[test]
fn degenerate_input_zeroed_diag_patched() { /* … */ }

#[test]
fn non_degenerate_zero_diag_left_alone() { /* … */ }
```

```rust
//! Mirrors fitschan.c::MakeInvertable @ 22314.

use ndarray::Array2;

/// Patch the diagonal of `cd` for inputs that have no effect on any
/// output AND correspond to a degenerate (single-pixel) axis.
/// Sets such diagonal entries to 1.0; leaves the matrix unchanged
/// otherwise.
pub(super) fn make_invertable(cd: &mut Array2<f64>, dim: &[Option<f64>]) {
    let n = cd.nrows();
    for i in 0..n {
        let unused = (0..n).all(|j| cd[[j, i]] == 0.0);
        if unused && dim.get(i).copied().flatten() == Some(1.0) {
            cd[[i, i]] = 1.0;
        }
    }
}
```

- [ ] **Step 6: Commit**

```bash
git commit -am "fitschan/wcs_keywords: port MakeInvertable

Patches degenerate-axis singularities so the palDmat-style solve
in make_int_world has a well-conditioned matrix to work with.
Mirrors fitschan.c::MakeInvertable @ 22314.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 4.3: Wire `find_basis_vectors` / `fit_line_cd` into the public path

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_decompose.rs` (drop `#[allow(dead_code)]`)

The helpers were ported in commit `1f9f3b7` and currently carry `#[allow(dead_code)]` on every public-crate item. With `make_int_world` about to use them, drop the markers.

- [ ] **Step 1: Drop `#[allow(dead_code)]` markers**

```bash
grep -n "#\[allow(dead_code)\]" crates/ast-core/src/fitschan/wcs_decompose.rs
```

Remove every match. The helpers we keep:
- `pub(crate) struct BasisVectors`
- `fn make_basis_vectors`
- `pub(crate) fn find_basis_vectors`
- `fn fit_ok`
- `pub(crate) fn fit_line`
- `pub(crate) fn fit_line_cd`

- [ ] **Step 2: Build, tests still green**

```bash
cargo build -p ast-core
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Note: build succeeds — but if `make_int_world` doesn't use these symbols yet, dead-code warnings reappear. Order this commit AFTER make_int_world's first compile (Task 4.4 step 4), or move the markers' removal into the make_int_world commit.

Better: skip this as a standalone commit; it's done as part of Task 4.4 below.

### Task 4.4: `make_int_world` body (no SIP branch)

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/make_int_world.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_decompose.rs` (drop `#[allow(dead_code)]`)

**C reference:** `fitschan.c::MakeIntWorld` @ line 21689. ~300 executable lines. The SIP branch (lines 21882-21895 + 21999-22004) is wired up in Group 6 — for now, leave the call site as a `None` short-circuit and process every axis through FitLine.

- [ ] **Step 1: Read the C function in full**

```bash
sed -n '21689,22310p' reference/starlink-ast/src/fitschan.c | less
```

Note specifically:
- The simplify-then-FindBasisVectors-then-FitLine pipeline.
- The OrthVectorSet call when nout > nin.
- The MakeInvertable call.
- The palDmat solve, computing `y` from `w0`.
- `crpix[j] = g0[j] - y[j]`.
- The handling of `nin` vs `nout` mismatch (extra grid axes get CRPIX = 1).
- The CDELT/PC split-out at the end (already in our codebase as `split_cd` in `wcs_decompose.rs`).

- [ ] **Step 2: Capture an oracle fixture for at least the linear-only case**

Create `tools/oracle_fixtures/make_int_world.c`. For each fixture, build a FrameSet, run the C MakeIntWorld, dump CRPIX / CD / PC / CDELT as Rust constants. Cover:
1. Pure-linear pixel→IWC chain (a series of MatrixMap + ShiftMap on a 2-axis or 3-axis FrameSet).
2. Same with one axis where `nout > nin` (degenerate axis).
3. (SIP case deferred to Group 6.)

```bash
cd tools/oracle_fixtures && make make_int_world && ./make_int_world > /tmp/make_int_world_captured.rs && cd ../..
```

- [ ] **Step 3: Create `make_int_world.rs`**

```rust
//! Port of fitschan.c::MakeIntWorld @ line 21689.
//!
//! Receives an assembled pixel→IWC mapping; extracts CRPIX/CD/PC/CDELT
//! into the supplied FitsStore. Returns `Ok(true)` on success, `Ok(false)`
//! when the chain isn't sufficiently linear (mirrors C's int return value).

use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::fitschan::wcs_decompose::{find_basis_vectors, fit_line_cd, split_cd};
use crate::frames::Frame;
use crate::mappings::{Mapping, polymap::solve_linear_system};
use ndarray::Array2;

use super::invertable::make_invertable;
use super::orth::orth_vector_set;

pub(crate) fn make_int_world(
    pixel_iwc: &dyn Mapping,
    wcs_frame: &dyn Frame,
    wperm: &[usize],
    store: &mut FitsStore,
    dim: &[Option<f64>],
    fitstol: f64,
    sip_ok: bool,
) -> AstResult<bool> {
    // Step-by-step port of fitschan.c::MakeIntWorld:
    // 1. Simplify the mapping.
    // 2. (SIP branch — Group 6.)
    // 3. find_basis_vectors → BasisVectors { g0, w0, tol }.
    // 4. fit_line_cd → CD-column vectors per input axis (returns None
    //    if any axis fails the linearity gate; in that case return
    //    Ok(false) and do not touch the store).
    // 5. orth_vector_set to fill missing columns when nout > nin.
    // 6. make_invertable.
    // 7. solve_linear_system: y = CD⁻¹ · w0.
    // 8. crpix[j] = g0[j] - y[j].
    // 9. split_cd → CDELT + PC; store both alongside CRPIX/CD.
    todo!("port make_int_world body")
}

#[cfg(test)]
mod tests {
    use super::*;

    // PASTE CAPTURED CONSTS HERE.

    #[test]
    fn linear_chain_recovers_crpix_and_cd_exactly() {
        // Build a linear pixel→IWC mapping matching the oracle fixture's
        // expected CRPIX/CD; run make_int_world; assert exact match.
    }

    #[test]
    fn non_linear_chain_returns_false() {
        // A genuinely non-linear chain (e.g. WcsMap over a wide box).
    }
}
```

- [ ] **Step 4: Implement the body**

Roughly:

```rust
    let pixel_iwc = pixel_iwc.simplified()?;

    // SIP wiring is Group 6.
    let _ = sip_ok;

    let nin = pixel_iwc.nin();
    let nout = pixel_iwc.nout();
    if nout < nin {
        return Ok(false);
    }

    let basis = match find_basis_vectors(pixel_iwc.as_ref(), nin, nout, dim, fitstol) {
        Some(b) => b,
        None => return Ok(false),
    };

    let cd = match fit_line_cd(
        pixel_iwc.as_ref(),
        nin,
        nout,
        &basis.g0,
        &basis.w0,
        dim,
        &basis.tol,
    ) {
        Some(c) => c,
        None => return Ok(false),
    };

    // Convert ndarray cd into the column-vector representation
    // OrthVectorSet expects.
    let mut partmat: Vec<Vec<f64>> = (0..nin)
        .map(|i| (0..nout).map(|j| cd[[j, i]]).collect())
        .collect();

    let mut fullmat_cols = match orth_vector_set(&partmat, nout) {
        Some(c) => c,
        None => return Ok(false),
    };

    // Build full nout×nout matrix.
    let mut fullmat = Array2::<f64>::zeros((nout, nout));
    for (i, col) in fullmat_cols.iter().enumerate() {
        for j in 0..nout {
            fullmat[[j, i]] = col[j];
        }
    }
    make_invertable(&mut fullmat, dim);

    // palDmat-style solve: fullmat · y = w0.
    let mut mat: Vec<f64> = (0..nout)
        .flat_map(|j| (0..nout).map(move |i| fullmat[[j, i]]))
        .collect();
    let mut y = basis.w0.clone();
    if !solve_linear_system(&mut mat, &mut y, nout) {
        return Ok(false);
    }

    // CRPIX = g0 - y.  For inputs without a corresponding pixel axis
    // (when nout > nin), CRPIX = 1.0 per fitschan.c:22182.
    let mut crpix = vec![1.0_f64; nout];
    for j in 0..nin {
        crpix[j] = basis.g0[j] - y[j];
    }

    // Split CD into CDELT + PC and store. Apply wperm.
    let (cdelt, pc) = split_cd(&fullmat);
    for j in 0..nout {
        let fits_j = wperm[j];
        store.crpix[fits_j] = Some(crpix[fits_j]);
        store.cdelt[fits_j] = Some(cdelt[fits_j]);
        for i in 0..nout {
            let fits_i = wperm[i];
            store.cd[fits_j][fits_i] = Some(fullmat[[j, i]]);
            store.pc[fits_j][fits_i] = Some(pc[[j, i]]);
        }
    }

    Ok(true)
```

(Adjust to match `FitsStore`'s actual field shapes; `cdelt` may be `Vec<Option<f64>>` etc.)

- [ ] **Step 5: Drop `#[allow(dead_code)]` markers in `wcs_decompose.rs`**

Now that `make_int_world` uses `find_basis_vectors`, `fit_line_cd`, etc., the markers can come off without dead-code warnings.

- [ ] **Step 6: Run tests**

```bash
cargo test -p ast-core fitschan::wcs_keywords::make_int_world
```

- [ ] **Step 7: Uncomment in `mod.rs`**

```rust
mod orth;
mod invertable;
mod make_int_world;

pub(crate) use make_int_world::make_int_world;
pub(crate) use other_axes::other_axes;
```

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords/make_int_world.rs \
        crates/ast-core/src/fitschan/wcs_keywords/mod.rs \
        crates/ast-core/src/fitschan/wcs_decompose.rs \
        tools/oracle_fixtures/make_int_world.c
git commit -m "$(cat <<'EOF'
fitschan/wcs_keywords: port MakeIntWorld (no SIP branch)

Receives the assembled pixel→IWC mapping, extracts CRPIX/CD/PC/CDELT
via find_basis_vectors + fit_line_cd + orth_vector_set + palDmat
solve. Mirrors fitschan.c::MakeIntWorld @ 21689.

SIP-distortion branch (lines 21882-21895 + 21999-22004) lands in
Group 6 — for now, returns Ok(false) for any chain whose celestial
sub-mapping carries a SIP PolyMap.

Tests use C-oracle-derived fixtures captured via
tools/oracle_fixtures/make_int_world.c.

Drops #[allow(dead_code)] from find_basis_vectors / fit_line /
fit_line_cd / fit_ok in wcs_decompose.rs — they're now used by the
wired-in path.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 4.5: Group-4 finalisation

- [ ] **Step 1: Full ast-core test run**

```bash
cargo test -p ast-core 2>&1 | grep -E "^test result|FAILED"
```

Expected: every result `ok`.

- [ ] **Step 2: clippy + fmt**

- [ ] **Step 3: wcsconv unchanged**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: `152 passed, 40 failed`. New code is reachable from unit tests only.

---

## Group 5: `celestial_axes` + `spectral_axes`

**Goal:** Port the two large axis-classifier functions and their helpers. These are the most code-intensive part of the port.

### Task 5.1: `is_sky_off` + `sky_sys` helpers

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/sky_sys.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:**
- `fitschan.c::IsSkyOff` @ line 20222.
- `fitschan.c::SkySys` @ line 29599. (Sets up CTYPE/CUNIT/CRVAL/RADESYS/EQUINOX/MJD-OBS for a SkyFrame, including offset-coordinate handling.)

`SkySys` is large (~250 lines) and depends heavily on existing SkyFrame attribute accessors. Read it before writing.

- [ ] **Step 1: Read both C functions**

```bash
sed -n '20222,20299p' reference/starlink-ast/src/fitschan.c   # IsSkyOff
sed -n '29599,29900p' reference/starlink-ast/src/fitschan.c   # SkySys
```

- [ ] **Step 2: Sketch signatures**

```rust
//! Port of fitschan.c::SkySys + IsSkyOff.

use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{Frame, FrameSet, SkyFrame};
use crate::fitschan::Encoding;

pub(super) fn is_sky_off(fset: &FrameSet, iframe: usize) -> i32 {
    // Returns +1 if the frame at `iframe` is a SkyFrame with
    // SkyRefIs ∈ {Pole, Origin}, else 0.
    todo!("port fitschan.c::IsSkyOff @ 20222")
}

pub(super) fn sky_sys(
    sky: &SkyFrame,
    wcstype: i32,
    wcsproj: crate::wcslib::WcsProjection,
    store: &mut FitsStore,
    axlon: usize,
    axlat: usize,
    s: char,
    isoff: i32,
) -> AstResult<bool> {
    // Stores CTYPE/CUNIT/CRVAL/RADESYS/EQUINOX/MJD-OBS/etc.
    // for the celestial axes. Returns true on success.
    todo!("port fitschan.c::SkySys @ 29599")
}
```

- [ ] **Step 3: Capture oracle fixtures**

Create `tools/oracle_fixtures/sky_sys.c`. For each input (a SkyFrame with specific system + equinox + epoch + offset attributes), dump the resulting FitsStore content as a Rust constant struct.

Cover at minimum:
- ICRS / FK5 / FK4 / Galactic / Ecliptic systems.
- `SkyRefIs = Origin` (offset coords).
- Various epoch / equinox combinations.

- [ ] **Step 4: Implement IsSkyOff**

Trivial port (~30 LoC). Uses `SkyFrame::sky_ref_is`.

- [ ] **Step 5: Implement SkySys**

Translate the long C function. Match every CTYPE prefix, every conditional on `system`, every offset-coord branch. Critical: the CTYPE prefix selection table at `fitschan.c:29906-29964` must be reproduced exactly.

- [ ] **Step 6: Tests + commit**

```bash
git commit -am "fitschan/wcs_keywords: port IsSkyOff + SkySys

Stores celestial-axis CTYPE/CUNIT/CRVAL/RADESYS/EQUINOX/MJD-OBS
in the FitsStore. Mirrors fitschan.c:29687 + IsSkyOff @ 20222.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 5.2: `get_fiducial_wcs`

**Files:**
- Add to `crates/ast-core/src/fitschan/wcs_keywords/celestial_axes.rs` (created in Task 5.3).

**C reference:** `fitschan.c::GetFiducialWCS` (search). Traces the WcsMap's native fiducial point through the post-Wcs chain to compute the celestial coordinate at IWC = (0, 0). This is the function whose absence causes the µpixel CRPIX residual today.

- [ ] **Step 1: Read the C source**

```bash
grep -n "static.*GetFiducialWCS" reference/starlink-ast/src/fitschan.c
```

- [ ] **Step 2: Capture an oracle fixture**

For at least 2 cases (pure TAN, TAN with SIP), capture the fiducial CRVAL output as a Rust constant.

- [ ] **Step 3: Implement**

```rust
pub(super) fn get_fiducial_wcs(
    map: &dyn Mapping,
    lonax: usize,
    latax: usize,
) -> Option<(f64, f64)> {
    // Mirrors fitschan.c::GetFiducialWCS. Walks through any WcsMap
    // in the chain, transforms the projection's fiducial native
    // point through the post-Wcs sub-chain, returns (RA, Dec) in
    // radians. None if no WcsMap is present.
    todo!("port fitschan.c::GetFiducialWCS body")
}
```

- [ ] **Step 4: Tests + commit (folded into Task 5.3's commit if convenient).**

### Task 5.3: `celestial_axes`

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/celestial_axes.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:** `fitschan.c::CelestialAxes` @ line 4890. ~340 executable lines. Depends on `sky_sys`, `is_sky_off`, `get_fiducial_wcs` from Tasks 5.1-5.2.

- [ ] **Step 1: Read the full C function**

```bash
sed -n '4890,5603p' reference/starlink-ast/src/fitschan.c | less
```

Critical pieces:
- `astFindFrame` to find a SkyFrame inside the current frame.
- `wcs_proj` selection.
- `SkyRef` → fiducial CRVAL.
- The wcs→iwc Mapping construction (WcsMap inverse + native rotation).

- [ ] **Step 2: Capture oracle fixtures**

Create `tools/oracle_fixtures/celestial_axes.c`. For 4-5 representative wcsconv inputs (e.g. `tan-pv.head`, `freq-ghz-bary.head`, `sip-3d.head`, `offset-sky.head`), build the FrameSet, run CelestialAxes, dump the resulting FitsStore content + the returned wcs→iwc mapping (sample at 5-10 points).

- [ ] **Step 3: Implement**

Roughly 700 LoC of Rust. Take it section by section, mirroring the C control flow.

- [ ] **Step 4: Tests + commit**

```bash
git commit -am "$(cat <<'EOF'
fitschan/wcs_keywords: port CelestialAxes

Stores celestial-axis WCS keywords in the FitsStore and returns
the wcs→iwc mapping segment for those axes. Uses get_fiducial_wcs
(also in this commit) so the chosen CRVAL is the projection's
exact fiducial point — not world(g0) — which is the structural
fix for the µpixel CRPIX residual the spec calls out.

Mirrors fitschan.c::CelestialAxes @ 4890 + GetFiducialWCS.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.4: `spectral_axes`

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/spectral_axes.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:** `fitschan.c::SpectralAxes` @ line 30050. ~370 executable lines. Inspects the SpecMap chain to pick the FITS spectral algorithm code (`-FRQ`, `-VRD`, `-WAV`, `-OHL`, `-F2W`, `-V2F`, etc.), stores spec CTYPE/CUNIT/CRVAL/RESTFRQ/SPECSYS/VELOSYS, returns the wcs→iwc segment.

- [ ] **Step 1-4: standard TDD pattern with oracle fixtures**

Capture from inputs covering each algorithm code (`freq-ghz-bary.head`, `vopt-geo.head`, `vrad-lsrd.head`, `wave-f2w.head`, `vopt-f2w.head`, `class-arc.head` for FREQ-LSR variants).

- [ ] **Step 5: Commit**

```bash
git commit -am "$(cat <<'EOF'
fitschan/wcs_keywords: port SpectralAxes

Inspects the SpecMap chain to pick the FITS spectral algorithm
code, stores spec keywords, returns the wcs→iwc segment for the
spectral axis. Mirrors fitschan.c::SpectralAxes @ 30050.

Where vopt-geo / freq-ghz-bary / vrad-lsrd / class-* fixtures get
their CRVAL3 right.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 5.5: Group-5 finalisation

- [ ] **Step 1: Test suite + clippy + fmt + wcsconv baseline check**

Same final-block as previous groups. Expected: `152 passed, 40 failed` (still not wired in).

---

## Group 6: `sip_int_world`

**Goal:** Port the SIP-distortion structural extractor. Depends on `map_split` (Group 1), `linear_approx` / `analyse_poly` / `scale_poly_inputs` (Group 2), and `make_int_world`'s SIP wiring point (Group 4 left a placeholder).

### Task 6.1: `sip_int_world` body

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_keywords/sip_int_world.rs`
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:** `fitschan.c::SIPIntWorld` @ line 28495. ~300 executable lines.

- [ ] **Step 1: Read the full C function**

```bash
sed -n '28495,29107p' reference/starlink-ast/src/fitschan.c | less
```

Walk through:
- The `astMapSplit` call to extract the 2-in/2-out celestial sub-mapping.
- The PolyMap discovery in the simplified chain.
- The pre-Poly / Poly / post-Poly partitioning.
- The pre-Poly linearity check via `linear_approx`.
- The scaling factor-out via `scale_poly_inputs`.
- The `analyse_poly` decomposition.
- The post-Poly linearity check.
- The CRPIX = pre-Poly applied to (0, 0).
- The CD = post-Poly's matrix.
- The SIP A/B/AP/BP coefficient storage.
- The returned stripped Mapping (PolyMap removed).

- [ ] **Step 2: Capture oracle fixture**

Create `tools/oracle_fixtures/sip_int_world.c`. For at least:
- `sip-3d.head` → expect `Some(SipResult)` with specific CRPIX/CD/SIP coeffs.
- `sip-3d-swap.head` → expect `Some(SipResult)` with axis-swapped `in_axes`.
- A non-SIP input (any TAN-only header) → expect `None`.

- [ ] **Step 3: Skeleton**

```rust
//! Port of fitschan.c::SIPIntWorld @ line 28495.

use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::mappings::{DynMapping, Mapping};

pub(super) struct SipResult {
    pub stripped: DynMapping,
    pub in_axes: [usize; 2],
    pub crpix: [f64; 2],
    pub cd: [f64; 4],
}

pub(super) fn sip_int_world(
    map: &dyn Mapping,
    fitstol: f64,
    lonax: usize,
    latax: usize,
    store: &mut FitsStore,
    dim: &[Option<f64>],
) -> Option<SipResult> {
    todo!("port fitschan.c::SIPIntWorld body")
}

#[cfg(test)]
mod tests {
    use super::*;

    // PASTE CAPTURED CONSTS HERE.

    #[test]
    fn sip_3d_extracts_correctly() { /* … */ }

    #[test]
    fn sip_3d_swap_records_swapped_axes() { /* … */ }

    #[test]
    fn non_sip_returns_none() { /* … */ }
}
```

- [ ] **Step 4: Implement**

Translate C section by section. Read the C carefully — there are several non-obvious branches around `scaling`, `rotation`, the AnalysePoly factor-out, the SIP coefficient storage loop. Don't simplify; mirror.

- [ ] **Step 5: Run tests until they pass**

```bash
cargo test -p ast-core fitschan::wcs_keywords::sip_int_world
```

- [ ] **Step 6: Commit Task 6.1**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords/sip_int_world.rs \
        crates/ast-core/src/fitschan/wcs_keywords/mod.rs \
        tools/oracle_fixtures/sip_int_world.c
git commit -m "$(cat <<'EOF'
fitschan/wcs_keywords: port SIPIntWorld

Structural extraction of CRPIX/CD/SIP-coefficients from a chain
containing a SIP PolyMap. Splits the chain around the PolyMap,
verifies pre/post linearity, factors any scaling, runs
analyse_poly, computes CRPIX from the pre-Poly shift and CD from
the post-Poly matrix. Returns the stripped Mapping for
make_int_world to continue with.

Mirrors fitschan.c::SIPIntWorld @ 28495.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 6.2: Wire SIP branch into `make_int_world`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/make_int_world.rs`

In Task 4.4 the SIP branch was left as a placeholder. Now flesh it out.

- [ ] **Step 1: Add the SIP branch logic**

Insert near the top of `make_int_world` (after simplify, before `find_basis_vectors`):

```rust
    // SIP detection (mirrors fitschan.c:21970-21983).
    let sip_result = if sip_ok {
        // Find lonax/latax inside wcs_frame; need them to feed sip_int_world.
        if let Some((lonax, latax)) = celestial_lon_lat(wcs_frame) {
            sip_int_world(
                pixel_iwc.as_ref(),
                fitstol,
                lonax,
                latax,
                store,
                dim,
            )
        } else {
            None
        }
    } else {
        None
    };

    let pixel_iwc = match &sip_result {
        Some(r) => r.stripped.clone(),
        None => pixel_iwc,
    };
```

After `fit_line_cd` returns, override the celestial CD columns with `sip_result.cd`. Mirrors fitschan.c:22087-22092.

After the CRPIX solve, override `crpix[sip_result.in_axes[0]]` and `crpix[sip_result.in_axes[1]]` with `sip_result.crpix`. Mirrors fitschan.c:22184-22190.

- [ ] **Step 2: Add a `celestial_lon_lat` helper**

```rust
fn celestial_lon_lat(frame: &dyn Frame) -> Option<(usize, usize)> {
    // Walk into CmpFrame children if needed; return the (lonax, latax)
    // pair of any contained SkyFrame, or None.
    todo!("inspect frame for SkyFrame, return its lon/lat axis indices")
}
```

- [ ] **Step 3: Add SIP test to `make_int_world.rs`**

Capture an oracle fixture (`make_int_world.c` already covered linear; add a SIP-input variant that exercises the SIP path):

```rust
#[test]
fn sip_chain_uses_sip_int_world_path() {
    // Fixture: sip-3d.head → CRPIX should match SIP extraction
    // exactly, not the LSQ fit on the SIP-distorted chain.
}
```

- [ ] **Step 4: Run tests; commit**

```bash
git commit -am "$(cat <<'EOF'
fitschan/wcs_keywords: wire SIP branch into make_int_world

Detect a SIP PolyMap in the chain, route through sip_int_world to
extract CRPIX/CD structurally, override the FitLine result for the
celestial axes. Mirrors fitschan.c:21970-21983 + 21999-22004 +
22096-22102.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 6.3: Group-6 finalisation

Same final-block as previous groups. wcsconv baseline still `152 / 40` (no integration yet).

---

## Group 7: Wire `fset_to_store` into the writer; delete old code

**Goal:** Add `fset_to_store` as the new entry point in `wcs_keywords/`, replace the body of `store_from_frameset_to`, delete the now-unreachable legacy helpers. This is the commit where wcsconv numbers swing.

### Task 7.1: Implement `fset_to_store`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords/mod.rs`

**C reference:** `fitschan.c::FsetToStore` @ line 12006. ~250 lines.

- [ ] **Step 1: Read the C function**

```bash
sed -n '12006,12270p' reference/starlink-ast/src/fitschan.c | less
```

Note the call sequence:
1. `CelestialAxes` → returns wcs→iwc segment, fills celestial keywords.
2. `SpectralAxes` → composes onto the running wcs→iwc, fills spec keywords.
3. `OtherAxes` → composes onto the running wcs→iwc, fills other-axis keywords.
4. Compose `pixel→wcs ∘ wcs→iwc = pixel→iwc`.
5. Get `fitstol`, `sip_ok`.
6. Call `MakeIntWorld(pixel_iwc, …)`.
7. Post-process: set WCSNAME, DUT1/DTAI, snap small-CRVAL residue.

- [ ] **Step 2: Add `fset_to_store` to `wcs_keywords/mod.rs`**

```rust
use crate::error::AstResult;
use crate::fitschan::Encoding;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping, UnitMap};

pub(crate) fn fset_to_store(
    fs: &FrameSet,
    naxis: usize,
    dim: &[Option<f64>],
    encoding: Encoding,
    s: char,
) -> AstResult<FitsStore> {
    let mut store = FitsStore::new(naxis);
    let mut wperm: Vec<usize> = (0..naxis).collect();
    let mut axis_done = vec![false; naxis];
    let mut crvals: Vec<Option<f64>> = vec![None; naxis];
    let mut wcs_iwc: DynMapping = Box::new(UnitMap::new(naxis));

    // Step 1: celestial.
    let cel_seg = celestial_axes::celestial_axes(/* args */)?;
    wcs_iwc = Box::new(CmpMap::new(wcs_iwc, cel_seg, Composition::Series)?);

    // Step 2: spectral.
    let spec_seg = spectral_axes::spectral_axes(/* args */)?;
    wcs_iwc = Box::new(CmpMap::new(wcs_iwc, spec_seg, Composition::Series)?);

    // Step 3: other.
    let other_seg = other_axes::other_axes(/* args */)?;
    wcs_iwc = Box::new(CmpMap::new(wcs_iwc, other_seg, Composition::Series)?);

    // Verify every axis was claimed.
    if !axis_done.iter().all(|&d| d) {
        return Err(/* axis-not-handled error */);
    }

    // Step 4: assemble pixel→iwc.
    let pixel_wcs = fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let pixel_iwc: DynMapping =
        Box::new(CmpMap::new(pixel_wcs, wcs_iwc, Composition::Series)?);
    let pixel_iwc = pixel_iwc.simplified()?;

    // Step 5: linear-keyword extraction.
    let fitstol = 0.001_f64; // mirrors astGetFitsTol default
    let sip_ok = matches!(encoding, Encoding::FitsWcs); // sip only in FITS-WCS
    let wcs_frame = fs.frame(FrameRef::Current)?;
    let _ok = make_int_world::make_int_world(
        pixel_iwc.as_ref(),
        wcs_frame.as_ref(),
        &wperm,
        &mut store,
        dim,
        fitstol,
        sip_ok,
    )?;

    // Step 6: post-processing (WCSNAME, snap small CRVAL residue).
    // Mirrors fitschan.c:2968-3002.

    Ok(store)
}
```

(Fill in argument structs / fields per the actual signatures; this is a sketch.)

- [ ] **Step 3: Wire into `store_from_frameset_to`**

In `crates/ast-core/src/fitschan/wcs_write.rs`, replace the body of `store_from_frameset_to` with a single call:

```rust
fn store_from_frameset_to(
    fs: &FrameSet,
    target_idx: usize,
    dim: &[Option<f64>],
    input_crvals: &[Option<f64>],  // unused now — see deletion in Task 7.2
    s: char,
    encoding: Encoding,
) -> AstResult<FitsStore> {
    // The target_idx parameter is for alternate-axis dispatch; the
    // FrameSet's `current` is set by the caller before calling this.
    // FsetToStore reads the current frame, so target_idx selection
    // happens at the call site (encode_wcs / encode_wcs_to_store).
    let _ = (target_idx, input_crvals);  // documented unused
    crate::fitschan::wcs_keywords::fset_to_store(fs, fs.base().naxes(), dim, encoding, s)
}
```

- [ ] **Step 4: Build, observe wcsconv swing**

```bash
cargo build -p ast-core
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: significant swing in numbers. Many fixtures turn green (the numeric-value ones). A few may turn red on lines they previously passed (because the new path's CRVAL / CTYPE choice is bytewise different in some non-numeric way). Group 8 walks the residuals.

- [ ] **Step 5: Commit Task 7.1**

```bash
git commit -am "$(cat <<'EOF'
fitschan: wire fset_to_store into store_from_frameset_to

The new wcs_keywords::fset_to_store path replaces the legacy
helpers. Mirrors fitschan.c::FsetToStore @ 12006: celestial →
spectral → other axes each compose into wcs→iwc, then
pixel→iwc = pixel→wcs ∘ wcs→iwc, then make_int_world extracts
CRPIX/CD/PC/CDELT.

wcsconv numbers swing here; Group 8 walks the residuals.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 7.2: Delete legacy helpers

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

The new path doesn't call any of these. Remove them. Each is one or several functions in `wcs_write.rs`:

- `populate_celestial`
- `populate_spectral`
- `populate_other_axis`
- `populate_velosys` (or move to spectral_axes if not already there)
- `identify_axes`
- `build_pixel_to_iwc`
- `fiducial_crval`
- `extract_input_crvals` (only if no longer needed; `fset_to_store` doesn't use input override the way the old code did)
- The chosen-CRVAL block inside `store_from_frameset_to` (Group 1's `pre_split_succeeded` cascade)
- `solve_crpix_from_decomp` if no other caller uses it
- `decompose_linear` callsites in `wcs_write.rs` (the function itself stays — lives in `wcs_decompose.rs` and is used by `find_basis_vectors`'s tolerance path).

- [ ] **Step 1: Identify dead callers**

```bash
cargo build -p ast-core 2>&1 | grep "never used\|never read\|dead_code" | head
```

For each warning, delete the function. Don't `#[allow(dead_code)]`.

- [ ] **Step 2: Delete the chosen-CRVAL / 2π-wrap / w0 / solve_crpix block**

These were temporary heuristics; in the new path the IWC mapping handles all this structurally.

- [ ] **Step 3: Re-run tests**

```bash
cargo test -p ast-core 2>&1 | grep -E "^test result|FAILED" | head
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

- [ ] **Step 4: Commit Task 7.2**

```bash
git commit -am "$(cat <<'EOF'
fitschan/wcs_write: delete legacy CRPIX/CD extraction helpers

populate_celestial, populate_spectral, populate_other_axis,
identify_axes, build_pixel_to_iwc, fiducial_crval, the
chosen-CRVAL cascade, solve_crpix_from_decomp callsites — all
unreachable after Task 7.1 wired in fset_to_store. Removed
outright per the spec's clean-slate rule.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

### Task 7.3: Group-7 finalisation

- [ ] **Step 1: Test suite + clippy + fmt**

- [ ] **Step 2: Capture wcsconv swing**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "differs:" > /tmp/wcsconv_after_7.txt
wc -l /tmp/wcsconv_after_7.txt
```

Save this list — Group 8 walks it.

---

## Group 8: Landing fixes

**Goal:** Walk the post-Group-7 wcsconv failure list. For each test that's red on a numeric-value line, dig in and fix. For each that's red on a structural / comment / formatting line, classify as out-of-scope and leave alone.

### Task 8.1: Classify the residual failures

**Files:** none modified yet.

- [ ] **Step 1: Walk every line in `/tmp/wcsconv_after_7.txt`**

For each test, run with `--nocapture` and capture the actual diff:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -A2 "differs:" > /tmp/wcsconv_diffs.txt
```

- [ ] **Step 2: Classify each failure into one of four buckets**

Per the spec:

- **A. Numeric value mismatch** → must fix. Examples: `CRPIX1 = 50.499998` vs `50.5`, `CRVAL3 = 1.1020135E+11` vs `1.1020137E+11`.
- **B. Structural failure (out of scope)** → leave red. Examples: `dss_*` (encoding selection), `multi-wcs-dup_fits-wcs` (RADESYSA card ordering), `degen1_fits-wcs` (WCSAXES feature).
- **C. Formatting only (out of scope)** → leave red. Example: `tan-pv0_fits-wcs` (`-0.001` vs `-1.0E-3`).
- **D. Comment text differs (out of scope)** → leave red. Example: `vopt-geo_fits-aips` line 15 (`Channel width (m/s)` vs `Pixel size`).

Build a table at the top of `docs/issues/c-library-quirks.md` recording each B/C/D classification with the test name and reason. This is the spec's "out-of-scope failures classified" deliverable.

### Task 8.2: Fix numeric-value residuals one at a time

For each bucket-A failure, the workflow:

- [ ] **Step 1: Identify the failing line**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 \
  | grep -B0 -A2 "<test_name>"
```

- [ ] **Step 2: Run the C oracle for the same input**

Build a small C oracle program (`/tmp/oracle_<n>.c` style; pattern from earlier work) that reads the same `.head`/`.ast` input, runs the C library through the same encoding path, dumps the relevant computed values.

- [ ] **Step 3: Add `eprintln!` debug prints inside the new Rust path**

Print the same intermediate values the C oracle dumped. Compare numerically.

- [ ] **Step 4: Identify the divergence**

The divergence is always in one of these places:
- `celestial_axes` chose a different CRVAL (likely `get_fiducial_wcs` mismatch).
- `spectral_axes` picked a different algorithm code or VELOSYS.
- `other_axes` had wrong axis identification.
- `make_int_world` got a different basis g0 (`find_basis_vectors`).
- `sip_int_world` extracted different SIP coeffs.

In every case the fix is to read the C source for the diverging step and verify the Rust matches line-for-line. **No tolerance constants. No "round to fix it" workarounds.**

- [ ] **Step 5: Apply the fix; re-run the failing test**

- [ ] **Step 6: Run the full wcsconv suite to ensure no regression**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

- [ ] **Step 7: Commit the fix**

Each numeric-value fix is its own commit with a message naming the fixture and the C source line that was being mismatched.

Repeat Steps 1-7 for every bucket-A failure.

### Task 8.3: Document any new C-library quirks

**Files:**
- Modify: `docs/issues/c-library-quirks.md`

If during Task 8.2 you discover that the C oracle does something quirky / lossy (the way we documented the SkyFrame format-string round trip earlier), add an entry to `docs/issues/c-library-quirks.md`. Pattern from the existing entries.

### Task 8.4: Retire the project memory note

**Files:**
- Modify: `/sdf/home/t/timj/.claude/projects/-sdf-home-t-timj-WORK-starlink-astrs/memory/MEMORY.md`
- Delete: `/sdf/home/t/timj/.claude/projects/-sdf-home-t-timj-WORK-starlink-astrs/memory/project_makeintworld_port.md`

The memory note `project_makeintworld_port.md` documented this port as pending work. Now it's done. Replace with whatever lessons the port surfaced (if any) — or delete outright if everything important is captured in the spec + commit messages.

- [ ] **Step 1: Decide what's worth keeping in memory**

Probably: the structural insight (IWC mapping must be built from inputs, not extracted) is generally useful for future similar work. Capture as a smaller successor memory note.

- [ ] **Step 2: Update MEMORY.md index**

Remove the `project_makeintworld_port.md` line; add the successor note's line.

- [ ] **Step 3: Commit**

```bash
git commit -am "docs: retire project_makeintworld_port memory note

The port has landed. Replace the pending-work note with a smaller
successor capturing the structural insight (build IWC from inputs,
don't extract from FrameSet) for similar future work.

Generated with AI

Co-Authored-By: SLAC AI"
```

### Task 8.5: Final group finalisation

- [ ] **Step 1: Full test suite**

```bash
cargo test -p ast-core 2>&1 | grep -E "^test result|FAILED"
```

Expected: every result `ok`. Any FAILED is a regression to investigate.

- [ ] **Step 2: clippy + fmt**

- [ ] **Step 3: Final wcsconv check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "results:"
```

Expected: every numeric-value-failing test now passes. Out-of-scope failures still red and classified.

Compute the success delta:

```bash
# Should be ≥ 152 + 30 = 182 (give or take, depending on classification of edge cases).
```

- [ ] **Step 4: Final branch summary commit**

```bash
git commit --allow-empty -m "$(cat <<'EOF'
end: MakeIntWorld port complete

Bytewise wcsconv pass count: <N> / 192. Numeric-value failures
addressed; out-of-scope structural / formatting / comment-text
failures classified in docs/issues/c-library-quirks.md.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Closing notes

This plan is large. Read the spec (`docs/superpower/specs/2026-05-25-makeintworld-port-design.md`) in full before starting. Treat the C source as the contract; treat any deviation as a bug regardless of whether it "looks reasonable" in Rust.

Each commit group is reviewable on its own. If a commit group passes its finalisation checklist, the next group can start without further review. If it fails, fix it before moving on.

When in doubt, read the C source again. Don't invent rules.

## Post-validation idiomatic-Rust pass (TODO, all groups)

The whole plan — Group 1's `Mapping::map_split` reshape and all the
`make_int_world` pieces in Groups 2-7 — is being written **C-first**:
mirror the source line-for-line, leak C idioms (index-style loops,
`Option<...>` returns where C uses out-pointers, free functions where
trait methods would fit, `BTreeMap` accumulators that mimic C's
"build-then-simplify" pattern, and so on) so that bit-for-bit oracle
comparison stays tractable while the pipeline is being assembled.

Once the writer pipeline is wired up (Group 7) and the wcsconv
fixtures all pass (Group 8), revisit this work for an
**idiomatic-Rust pass**:

- Promote free helpers that are conceptually methods to default
  trait methods (`linear_approx` was already done as a follow-up to
  Group 2.1 — apply the same lens to anything in `helpers/` that
  takes a `&dyn Mapping` as its first argument and is logically a
  Mapping operation). Subclass overrides for closed-form cases
  (`MatrixMap::linear_approx`, `ZoomMap::linear_approx`, etc.)
  belong in this pass.
- Replace index-loop bodies in `mappings/linear_approx.rs`,
  `helpers/analyse_poly.rs`, `helpers/scale_poly_inputs.rs`, and
  the per-class `map_split` implementations with iterator pipelines
  where the result is genuinely clearer. The
  `#![allow(clippy::needless_range_loop, clippy::manual_memcpy)]`
  module attributes that were added to keep clippy quiet during the
  C-mirror phase mark the call sites worth revisiting.
- Replace `Option<...>` returns that encode C's "0 = failure, set
  out-pointers on success" idiom with `Result<T, AstError>` or
  domain-specific enums where the failure modes are meaningful.
  Specifically: `analyse_poly`'s `AstResult<Option<...>>` collapses
  badly, and the per-class `map_split` failure modes deserve named
  variants instead of bare `None`.
- Reconsider `AnalysePolyResult.map3_matrix: Option<MatrixMap>`
  (where `None` encodes the UnitMap branch) as an explicit
  `enum { Identity, Matrix(MatrixMap) }` — the current shape is a
  C-compat artefact.
- Audit which `pub` items in `helpers/` are still reachable from
  outside the crate after the integration work lands. Anything not
  needed externally should drop to `pub(crate)`.
- Audit the `#[allow(dead_code)]` attributes added during Groups
  2-6 (they exist because callers in later groups hadn't landed
  yet); remove the ones that are now reachable.

**Constraint:** do this pass only after wcsconv green. The
C-oracle test suite is the safety net; an idiomatic refactor without
a passing oracle baseline risks regressions that look fine in Rust
but produce different bytes from C.

Track this in a future plan (`docs/superpower/plans/...-idiomatic-rust.md`)
once Group 8 lands; don't try to fold it into this plan.
