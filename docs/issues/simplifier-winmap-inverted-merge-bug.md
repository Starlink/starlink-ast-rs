# Simplifier corruption: WinMap merged past an inverted WcsMap/SpecMap

> **HISTORICAL DOCUMENT (resolved).** Fixed `a4bff3c`; `spec_cel_invert` is an
> active passing regression (string + astequal). The `try_swap_winmap_wcsmap`
> helper cited below was later retired in the map-merge rebuild — the
> invert-flag handling now lives in the nominate-loop model — but the bug
> remains fixed. Retained as a record; no outstanding work.

**Status (2026-06-25):** FIXED. The reproducing fixture
(`simplify_fixtures/spec_cel_invert.{map,simp}`, commit `5595fbc`) is now an
active regression test (manifest line re-enabled). This was a **simplifier
correctness bug**, not a serialization/representation difference.

## Root cause and fix

The bug was in `try_swap_winmap_wcsmap` (`mappings/simplify.rs`). The series
list holds `(mapping, invert_flag)` tuples; `Vec::swap(i, i+1)` moves the whole
tuple, so a positional reorder already carries each map's invert flag to its new
position. The code then *also* swapped the two `.1` (invert) fields back, under
the mistaken comment "Also swap their invert flags" — net effect: each mapping
ended up wearing the *other* map's invert flag. When the WcsMap neighbour was
inverted (`InvB=1`), this flipped the passing-through linear map (the spectral
`ShiftMap`/`WinMap`) to inverted, so its `+CRVAL` shift was applied as `-CRVAL`
— the `2*CRVAL` (= forward − inverse) signature. Earlier fixtures masked it
because their WcsMap neighbour was not inverted, making the double-swap a no-op.

Fix: drop the manual flag swap; `Vec::swap` alone is correct. Each map keeps its
own direction across a positional reorder. With the fix, Rust's simplified
output matches C's `CmpMap{ WinMap(Sft1=907405752), WcsMap } + InvB=1` and the
spectral-axis transform reproduces 907505752.

## One-paragraph summary

When `Mapping::simplify()` reduces a series chain in which a `WinMap`
(carrying a spectral CRVAL shift) precedes a trailing **inverted**
`WcsMap`/`SpecMap`, Rust corrupts the WinMap's shift value — it subtracts
`2 * CRVAL`, producing a mapping that is *numerically wrong*, not merely a
different-but-equivalent normal form. C's `astSimplify` keeps the trailing
mapping inverted via the wrapping `CmpMap`'s `InvB` flag and leaves the
WinMap shift untouched. This blocks a set of FITS-cube wcsconv native cases
(FREQ/WAVE spectral axis + celestial axes) and, critically, also blocks an
otherwise-correct `map8`-construction rewrite (see below) that cannot land
until this is fixed.

## The smoking gun

Fixture: `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/spec_cel_invert.map`
(the pixel→world `map8` for `class-ait.head`, a FREQ + RA/DEC-AIT cube, dumped
from C `WcsMapFrm` just before `astSimplify`) and `…/spec_cel_invert.simp`
(C's `astSimplify(map8)`).

Transform the spectral axis (axis 0) of input point `[0.1, 0.2, 0.3]`:

| mapping                       | axis-0 output |
|-------------------------------|--------------:|
| original `.map` (pre-simplify)|     907505752 |
| C `astSimplify` (`.simp`)     |     907505752 |
| Rust `simplify()`             |   −1933305752 |

`−1933305752 = 907505752 − 2 × 1420405752`, where `1420405752` is the FREQ
`CRVAL` (the reference frequency). Rust double-applies the spectral CRVAL while
swapping the WinMap past the inverted trailing mapping.

The off-axis (celestial) values differ only at the ULP level — those are fine.
The spectral axis is genuinely corrupted.

## Structural comparison (simplified output)

Both C and Rust reduce the input to a two-component `CmpMap{ WinMap, WcsMap }`.
The difference is purely in how the inversion is represented — and Rust gets
the WinMap shift wrong as a side effect:

**C `.simp`:**
```
 Begin CmpMap
    InvB = 1                    # WcsMap applied in its inverse direction
    Begin WinMap
       Sft1 = 907405752         # spectral shift — CORRECT
       Scl1 = 1000000
       Sft2 = 0.0087266462599716477
       Scl2 = -0.00017453292519943296
       Sft3 = -0.0087266462599716477
       Scl3 = 0.00017453292519943296
    End WinMap
    Begin WcsMap
       Invert = 1
    End WcsMap
 End CmpMap
```

**Rust `simplify()`:**
```
 Begin CmpMap
    (no InvB)
    Begin WinMap
       Sft1 = -1933405752       # spectral shift — WRONG (= 907405752 - 2*CRVAL)
       Scl1 = 1000000
       Sft2 = 0.0087266462599716477
       Scl2 = -0.00017453292519943296
       Sft3 = -0.0087266462599716477
       Scl3 = 0.00017453292519943296
    End WinMap
    Begin WcsMap
       (not inverted)
    End WcsMap
 End CmpMap
```

C keeps the WcsMap stored forward and flags it inverted via the CmpMap
`InvB=1`. Rust instead bakes the inversion into the components (drops InvB,
un-inverts the WcsMap) and — incorrectly — adjusts the *leading* WinMap's
shift as if it had to compensate, subtracting `2*CRVAL`.

## Why it is a correctness bug, not a representation choice

The first component of a series `CmpMap` is applied first; its value cannot
legitimately depend on how the *second* component is represented. A WinMap
followed by a WcsMap (in either inverted form) must leave the WinMap shift
identical. Rust changes it, and the resulting mapping does not reproduce the
input's forward transform (table above). The simplify regression harness
flags it as a `transform failure`, not just an `equal()`/serialization
mismatch.

## How to reproduce / debug

The committed fixture lets you work entirely on the simplify harness — no FITS
read path, no C instrumentation needed:

1. Re-enable the fixture by uncommenting its line in
   `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`:
   ```
   spec_cel_invert | simplify_fixtures/spec_cel_invert.map | simplify_fixtures/spec_cel_invert.simp |
   ```
2. Run the harness:
   ```
   cargo test -p ast-core --test test_simplify_regression simplify_regression -- --nocapture
   ```
   It will report `spec_cel_invert` under both `equal() failures` and
   `transform failures`. (The harness hard-fails on transform failures — that
   is why the line ships commented out.)

For an ad-hoc numeric check, a throwaway example like:
```rust
use ast_core::coords::PointSet;
use ast_core::from_native_string;
fn main() {
    let d = "crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/";
    let m = from_native_string(&std::fs::read_to_string(format!("{d}spec_cel_invert.map")).unwrap())
        .unwrap().into_mapping().unwrap();
    let s = m.simplify().unwrap();
    let pts = PointSet::from_rows([[0.1, 0.2, 0.3]]);
    println!("orig axis0 = {}", m.transform(&pts, true).unwrap().point(0)[0]); // 907505752
    println!("simp axis0 = {}", s.transform(&pts, true).unwrap().point(0)[0]); // -1933305752
}
```

### Regenerating the fixtures from C (if ever needed)

The `.map`/`.simp` were captured by temporarily instrumenting C `WcsMapFrm`
in `reference/starlink-ast/src/fitschan.c` around the `astSimplify(map8)` call
(~line 37810). **This instrumentation must be reverted and the library rebuilt
before committing anything** — `reference/starlink-ast` is the oracle and must
stay pristine.

`astToString` produces comment-less native text; the committed corpus uses the
comment+indent style emitted by an `astChannel` sink with `Comment=1`. The
simplest path that worked: dump via `astToString` (env-gated), then reformat
through the Rust reader/writer (`from_native_string` → `to_native_string`),
which preserves structure and values (including `InvB=1`, `Invert=1`) and only
adds the comment formatting. Steps:

1. In `WcsMapFrm`, before/after `ret = astSimplify( map8 );` add env-gated
   `astToString(map8)`/`astToString(ret)` dumps guarded on `s == ' '` (primary
   axis only), each writing to the file named by an env var. `stdio.h`/
   `stdlib.h` are already included; `astChannel` inside the library hits an
   internal-macro arity problem, so prefer `astToString`.
2. `cmake --build reference/starlink-ast/build --target ast` (never autotools).
3. Build the probe: `make -C tools/oracle_fixtures head2ast` (links libast.so
   dynamically, so the rebuild takes effect).
4. `AST_DUMP_MAP8=/tmp/x.map AST_DUMP_SIMP=/tmp/x.simp \
      tools/oracle_fixtures/head2ast crates/ast-core/tests/fixtures/wcsconv/inputs/class-ait.head >/dev/null`
5. Reformat each through Rust (parse + re-serialize) into the corpus.
6. **Revert the C edits, rebuild the library, confirm
   `grep -c AST_DUMP reference/starlink-ast/src/fitschan.c` is 0.**

## Where the bug lives / fix direction

- Rust's `CmpMap` already carries `inv_a`/`inv_b` flags (`cmpmap.rs:69-72`)
  that mirror C's `InvA`/`InvB`, so the *capability* to represent C's output
  exists. The bug is in the merge/swap path, not a missing feature.
- The corruption happens while swapping/merging a `WinMap` past a trailing
  **inverted** mapping. Study the C side:
  - `winmap.c` `WinMerge` / `MapMerge` — how a WinMap combines with a
    neighbour, and specifically how it treats a neighbour whose `Invert` flag
    is set.
  - `cmpmap.c` `CmpMapMerge` — how the series walk decides whether to bake a
    component's inversion in vs. preserve it as `InvA`/`InvB`.
- Likely Rust path: the WinMap-swap logic (see `winmap`/`cmpmap` swap helpers
  exercised by the `winmap_swaps_past_*` tests in
  `test_simplify_regression.rs`) computes the swapped shift assuming the
  neighbour is in forward direction, then the inversion gets applied a second
  time. The `2*CRVAL` signature is consistent with applying the shift's
  forward and inverse forms in sequence.
- Acceptance: feeding `spec_cel_invert.map` to Rust `simplify()` must yield a
  mapping whose forward transform matches the input on the spectral axis
  (907505752, not −1933305752), and ideally serialises to the same
  `CmpMap{ WinMap(Sft1=907405752), WcsMap }` + `InvB=1` shape as C's `.simp`.

## Blocked work: the map8-construction rewrite (RE-APPLIED 2026-06-25)

This bug also blocked an otherwise-correct rewrite of the FITS read path that
makes Rust's *pre-simplification* mapping byte-match C's `map8`. That rewrite
matches C structurally and clears `degen-wcsaxes`/`cube-reorder`/`ssysobs`; it
previously **regressed** `class-ait`/`class-ait_fits-class`/`class-sfl_fits-class`
only because it fed the simplifier the map shape that triggered the bug above.
With the simplifier fix in place it has been re-applied and all of those cases
pass (the only remaining `test_wcsconv` failure is the unrelated, pre-existing
`skyspec3d-class_native` OBSALT_A ULP difference).

The rewrite had three parts (all now applied):

### 1. `wcs_read.rs` — compose pixel→world in C's exact left-associated order

C `WcsMapFrm` (fitschan.c:37898-37940) builds
`map8 = (((map1_pixel→IWC ; map2_celestial) ; map3_spectral) ; map4_others)`
— a **left-associated** series chain, class order, with `map4` (WcsOthers)
always composed (a bare `UnitMap(naxis)` when every axis is claimed).

Replace `build_iwc_to_world` + `build_general_iwc_to_world` with a single
`build_pixel_to_world(store, pixel_to_iwc, celestial, spectral)` that pushes
factors in order `[pixel_to_iwc, celestial?, spectral?, others]` and folds them
left-associated:
```rust
let mut parts: Vec<DynMapping> = vec![pixel_to_iwc];
if let Some(cel) = celestial { parts.push(cel.mapping); }
if let Some(spec) = spectral { parts.push(spec.mapping); }
if claimed.contains(&false) {
    let mut shifts = vec![0.0; naxis];
    for (i, &c) in claimed.iter().enumerate() {
        if !c { shifts[i] = store.crval[i].unwrap_or(0.0); }
    }
    parts.push(Box::new(ShiftMap::new(shifts)));
} else {
    parts.push(Box::new(UnitMap::new(naxis)));   // C still composes map4
}
let mut result = parts.remove(0);
for part in parts { result = Box::new(CmpMap::new(result, part, Composition::Series)?); }
```
Call it from both the primary path and the alt-plane loop (replacing the old
`build_iwc_to_world(...)` + `CmpMap::new(pixel_to_iwc, iwc_to_world, Series)`).
The simple-case early returns (pure 2D celestial, pure 1D spectral) are
unaffected — they are a single `CmpMap` either way.

### 2. `wcs_spectral.rs` — flat parallel chain, no PermMap sandwich

`embed_1axis_in_naxis` wrapped the 1D spectral map in
`PermMap ; (inner || UnitMap) ; PermMap`, adding identity PermMaps. C
`WcsSpectral` (fitschan.c:39791-39806) instead composes one 1D map per axis
**in parallel**, in FITS order (`ret = astCmpMap(ret, map1, 0, …)`), with no
PermMap (only the *Frame* is permuted). Rewrite:
```rust
fn embed_1axis_in_naxis(inner: DynMapping, naxis: usize, axis_0based: usize) -> AstResult<DynMapping> {
    if naxis == 1 { return Ok(inner); }
    let mut inner = Some(inner);
    let mut result: Option<DynMapping> = None;
    for k in 0..naxis {
        let part: DynMapping = if k == axis_0based {
            inner.take().unwrap()
        } else {
            Box::new(UnitMap::new(1))
        };
        result = Some(match result {
            None => part,
            Some(acc) => Box::new(CmpMap::new(acc, part, Composition::Parallel)?),
        });
    }
    Ok(result.unwrap())
}
```
Drop the now-unused `PermMap` import.

### 3. `matrix.rs` — detect Unit form (all-1.0 diagonal)

C `CompressMatrix` (matrixmap.c:661-676) compresses a DIAGONAL matrix whose
diagonal is all `1.0` to UNIT form (serialises as `Form="Unit"` with no element
values). Rust's `compute_form` only distinguished Diagonal vs Full, so it
emitted `Form="Diagonal"` with explicit `M0=1…`, diverging from C. Fix:
```rust
fn compute_form(m: &Array2<f64>) -> MatrixForm {
    if is_diagonal(m) {
        let n = m.nrows();
        if (0..n).all(|i| ast_equal(m[[i, i]], 1.0)) {
            MatrixForm::Unit
        } else {
            MatrixForm::Diagonal
        }
    } else {
        MatrixForm::Full
    }
}
```
The `MatrixForm::Unit` variant and its serialization already existed; only the
detection was missing.

## Related

- `docs/issues/native-roundtrip-simplify-gaps.md` — the broader simplify-corpus
  byte-exact contract this fixture belongs to.
- `docs/issues/c-library-quirks.md` — `astFormat`/`astSetC` RefRA/RefDec
  precision quirk (different spectral-axis issue, already handled).
- Memory: `fits-read-fidelity` (wcsconv native read-path tracker; this bug is
  its current top blocker).
