# Fixture-Hierarchy Sync, Part 1: Upstream C Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the upstream AST `ast_tester/fixtures/` tree to the state the Rust port will mirror byte-for-byte: every reference reproducible from a committed generator, one `IsSimp` defect fixed, and all three transform oracles regenerated.

**Architecture:** All work happens in the `reference/starlink-ast` git submodule on branch `u/timj/more-fixtures-2`, which is currently at the pinned commit `b8fcd43c` with no commits of its own. Fixtures move into the existing `fixtures/` hierarchy; four generators become cmake targets so nothing in the tree is unreproducible; the ctest comparators gain the same ULP-and-`IsSimp` awareness the Rust harness already has, which is what lets both repos read one shared manifest.

**Tech Stack:** C99/C11, CMake + CTest, the AST public C API (`ast.h`), and the internal-header escape hatch (`astCLASS`) where a protected flag has to be read.

**Spec:** `docs/superpowers/specs/2026-09-03-upstream-fixture-reorg-design.md` (sections 2, 3, 4 and 7). Part 2 of this plan is `2026-09-03-fixture-reorg-part2-rust-sync.md` and must not start until this plan is merged and the submodule pin moved.

## Global Constraints

- **Repository.** Every task in this plan edits files under `reference/starlink-ast/`. Confirm `git -C reference/starlink-ast rev-parse --abbrev-ref HEAD` prints `u/timj/more-fixtures-2` before the first commit.
- **Never push.** Commit locally only. Do not push to any remote, and do not open a PR.
- **Build with cmake, never autotools.** A configured build directory already exists at `reference/starlink-ast/build` (`AST_C_STANDARD=11`, Fortran off). Reconfigure with `cmake -S . -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo -DAST_BUILD_FORTRAN=OFF` if it is missing.
- **Fixture paths are relative to `ast_tester/fixtures/`.** Every path written into a `cases.txt` row or an oracle section header starts with `simplify/`, `wcsconv/`, `serialisation/`, `oracle/`, `plot/` or `programs/`.
- **Generators run from the repository root**, not from `ast_tester/`. `gen_simplify_fixtures` writes to the hardcoded relative path `ast_tester/fixtures/simplify`.
- **`AST__BAD` is `-DBL_MAX`.** Comparisons involving it are exact equality, never tolerance.
- **Byte-identity is the gate for every regeneration task.** "Regenerate, then `git diff --stat` is empty" is the pass condition. A non-empty diff is a finding to explain in the commit message, never something to accept silently.
- **C99 in `src/`, C11 permitted in `ast_tester/`** (the existing `gen_transform_oracle` target already sets `C_STANDARD 11` for `DBL_DECIMAL_DIG`).

---

## File Structure

**Modified — library source (Task 1 only).** Every class whose attribute setter
calls `astClearIsSimple` *and* whose loader calls that setter, plus the comment
that made the defect survive (`src/mapping.h:455`):

| class | loader sites | attributes |
|---|---|---|
| `src/zoommap.c` | 1 | `Zoom` |
| `src/permmap.c` | 1 | `PermSplit` |
| `src/lutmap.c` | 2 | `LutInterp`, `LutEpsilon` |
| `src/sphmap.c` | 2 | `UnitRadius`, `PolarLong` |
| `src/pcdmap.c` | 3 | `PcdCen` axes 0 and 1 (local macro), `Disco` |
| `src/mathmap.c` | 3 | `SimpFI`, `SimpIF`, `Seed` |
| `src/polymap.c` | 3 | `IterInverse`, `NiterInverse`, `TolInverse` |
| `src/splinemap.c` | 3 | `InvNiter`, `InvTol`, `OutUnit` |
| `src/wcsmap.c` | 2 | `FITSProj`, `LonCheck` |
| `src/grismmap.c` | 8 | the eight `Grism*` attributes |

**10 classes, 28 sites.** Verified by enumerating each loader's `Set*` calls and
checking whether that setter's macro body clears the flag, so it excludes
`WcsMap`'s `SetTPNTan` and its two `SetPV` calls, whose setters do not clear.
`IntraMap` clears the flag but its loader calls no setter, so it is unaffected.

**Modified — test programs and harness:**
- `ast_tester/testmapping.c` — gains `testissimpsurvivesload`
- `ast_tester/gen_simplify_fixtures.c` — gains 25 constructor functions
- `ast_tester/CMakeLists.txt` — three new executables, one new comparator wiring
- `cmake/run_simplify_test.cmake`, `cmake/run_wcsconverter_test.cmake` — swap `compare_files` for the new comparator

**Created — generators (one responsibility each):**
- `ast_tester/gen_native_fixtures.c` — builds the 24 minimal per-class dumps in `serialisation/`
- `ast_tester/head2ast.c` — reads one FITS header, dumps the FrameSet; drives `wcsconv/framesets/`
- `ast_tester/gen_keymap_mapsz.c` — emits `oracle/keymap_mapsz.txt`
- `ast_tester/compare_dumps.c` — the ULP-and-`IsSimp`-aware comparator used by ctest

**Created — fixture data:**
- `ast_tester/fixtures/serialisation/` (29 files), `ast_tester/fixtures/wcsconv/framesets/` (139 files), 46 files added to `ast_tester/fixtures/simplify/`, `oracle/keymap_mapsz.txt`, `wcsconv/expected/offset-sref.native`, `wcsconv/inputs/pc-sparse-naxis.head`, `simplify/lsst.simp`

Each generator is its own file rather than a mode of one program, because they have unrelated inputs (nothing, a FITS header, a KeyMap) and unrelated failure modes, and because a reviewer should be able to reject one without touching the others.

---

## Task 1: `IsSimp` survives a load

The parent loader reads the `IsSimp` card and sets the flag (`src/mapping.c:24137`). A subclass loader then validates the attribute it just read by re-applying it through a setter that calls `astClearIsSimple`, wiping it. Net effect: the library writes `IsSimp = 1` and cannot read it back.

**The loader is the visible victim, not the cause.** All 28 loader sites assign the field *first* and then call `SetX( new, new->field )`, so the loader's set is a no-op in value terms — it exists only to borrow the setter's validator. Saving and restoring the flag around that call would suppress the symptom at 28 sites and leave the cause in place. The cause is the justification at `mapping.h:455`:

> Discarding anyway is safe — the records can only ever suppress a simplification, never make one wrong — so err towards it when in doubt.

True while `IsSimple` is an internal hint; false once it is serialized as an `IsSimp` card and compared byte-for-byte, because over-clearing then changes observable output. So the fix guards the clear on the value actually differing — setting an attribute to the value it already holds cannot invalidate a recorded simplification — and amends that comment, because a comment telling the next reader over-clearing is free is what let this defect survive.

The committed corpus loses cards in only four classes (44 on ZoomMap, 2 each on PcdMap, MathMap and GrismMap). That is which objects the corpus contains, not which classes are affected: ten are. `docs/issues/c-library-quirks.md` already records the full set under "The `IsSimp` card does not survive being read back, for twelve classes".

**Files:**
- Test: `ast_tester/testmapping.c` (append two static functions plus their `main` calls)
- Modify: `src/zoommap.c`, `src/permmap.c`, `src/lutmap.c`, `src/sphmap.c`, `src/pcdmap.c`, `src/mathmap.c`, `src/polymap.c`, `src/splinemap.c`, `src/wcsmap.c`, `src/grismmap.c`, `src/mapping.h`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: a library in which `astGetI( obj, "IsSimple" )` is preserved across `astToString` / `astFromString` for all 10 affected classes, and in which no no-op set clears the flag. Task 2 depends on this behavior; Tasks 6, 8 and 11 depend on Task 2's regenerated corpus.

- [ ] **Step 1: Write the failing test**

Append to `ast_tester/testmapping.c`, immediately before `int main( void ) {`:

```c
/* A simplified Mapping dumps an IsSimp card, and reading that dump back must
   yield a Mapping that still reports IsSimple.  astLoadMapping sets the flag
   from the card (mapping.c:24137); a subclass loader that validates an
   attribute by re-applying it through a clear-on-set setter used to wipe it
   again, so the library could not read back what it wrote.  Ten classes have
   both a clear-on-set setter and a loader that calls it, so all ten are
   checked here rather than only the four the fixture corpus happens to
   contain. */
static void checkissimpround( int *status, AstMapping *map, const char *what ) {
   AstMapping *reloaded;
   AstMapping *simp;
   char *dump;
   char msg[ 128 ];

   if( *status != 0 || !astOK ) return;

   simp = astSimplify( map );
   if( !astGetI( simp, "IsSimple" ) ) {
      snprintf( msg, sizeof msg, "Error issimpround-%s-simplify", what );
      stopit( status, msg );
   }

   dump = astToString( simp );
   if( !strstr( dump, "IsSimp" ) ) {
      snprintf( msg, sizeof msg, "Error issimpround-%s-dump", what );
      stopit( status, msg );
   }

   reloaded = astFromString( dump );
   if( !astGetI( reloaded, "IsSimple" ) ) {
      snprintf( msg, sizeof msg, "Error issimpround-%s-reload", what );
      stopit( status, msg );
   }

   dump = astFree( dump );
   reloaded = astAnnul( reloaded );
   simp = astAnnul( simp );
}

static void testissimpsurvivesload( int *status ) {
   if( *status != 0 || !astOK ) return;

   /* One object per affected class.  Each must have the relevant attribute
      *set*, or its loader's TestX guard is false and the validating setter is
      never reached -- which would make the test pass without exercising the
      defect. */
   {
      AstZoomMap *m = astZoomMap( 1, 3.5, " " );
      checkissimpround( status, (AstMapping *) m, "zoommap" );
      m = astAnnul( m );
   }
   {
      AstPcdMap *m;
      double centre[ 2 ] = { 0.0, 0.0 };
      m = astPcdMap( 0.5, centre, " " );
      checkissimpround( status, (AstMapping *) m, "pcdmap" );
      m = astAnnul( m );
   }
   /* ... one block per remaining class: permmap (PermSplit set), mathmap
      (SimpFI/SimpIF set), lutmap (LutInterp or LutEpsilon set), sphmap
      (its cleared attributes set), polymap, splinemap, grismmap (any
      Grism* attribute set), wcsmap, polymap, splinemap.  Build each with the
      smallest constructor that sets the attribute its loader validates; read
      the class's astClearIsSimple line to see which attribute that is:
        grep -n 'astClearIsSimple' src/<class>.c
   */

   /* A second check, for the general rule the guard rests on: a set to the
      value already held must leave the record alone, whoever the caller is. */
   {
      AstZoomMap *m = astZoomMap( 1, 3.5, " " );
      AstMapping *simp = astSimplify( m );
      astSetD( simp, "Zoom", astGetD( simp, "Zoom" ) );
      if( !astGetI( simp, "IsSimple" ) ) {
         stopit( status, "Error issimpnoopset-1" );
      }
      /* ...and a set to a different value must still clear it. */
      astSetD( simp, "Zoom", 7.0 );
      if( astGetI( simp, "IsSimple" ) ) {
         stopit( status, "Error issimpnoopset-2" );
      }
      simp = astAnnul( simp );
      m = astAnnul( m );
   }
}
```

Before writing each block, confirm which attribute matters:

```bash
cd reference/starlink-ast/src
for f in zoommap pcdmap permmap mathmap lutmap sphmap polymap splinemap grismmap; do
  echo "=== $f"; grep -n 'astClearIsSimple' $f.c
done
```

An object whose attribute is *unset* takes the `if ( TestX( new, status ) )` false branch on load, never reaches the setter, and would pass this test while the defect stood.

and add the call inside `main`, next to the other `test*( status )` lines:

```c
   testissimpsurvivesload( status );
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd reference/starlink-ast
cmake --build build --target testmapping -j
ctest --test-dir build -R '^testmapping$' --output-on-failure
```

Expected: FAIL, printing an `Error issimpround-<class>-reload` line for each affected class, plus `Error issimpnoopset-1`. The `-simplify` and `-dump` assertions pass — the library does set the flag and does write the card — and only the read-back assertion fails, which is what pins the defect to the setter's side effect rather than to the writer. `issimpnoopset-2` must *not* fire: a set to a different value has to keep clearing, and that assertion is what stops the guard being written too broadly. A class that reports no failure has an unset attribute, so its loader never reaches the setter — fix its constructor before touching any source.

- [ ] **Step 3: Guard the clear on the value changing**

Edit each `astClearIsSimple` call in the ten classes so it fires only when the value differs. In `src/zoommap.c:1704`:

```c
/* Clearing the simplified records is right when an attribute changes, and
   wrong when it does not: a set to the value already held cannot invalidate a
   recorded simplification.  The loaders rely on this -- each assigns the field
   and then calls this setter with that same value, purely to validate it. */
astMAKE_SET1(ZoomMap,Zoom,double,zoom,(
            ( value != this->zoom ) ? astClearIsSimple(this) : (void)0,
            ( value != 0.0 ) ?
            value :
            ( astError( AST__ZOOMI,
                       "astSetZoom(%s): A zoom factor of zero is not allowed.", status,
                        astGetClass( this ) ),
              this->zoom ) ))
```

Three shapes need care:

- **`MathMap`'s `Seed`** (`mathmap.c:5629`) has three further side effects in the same expression. Guard **only** the clear; `seed_set = 1` and `active = 0` must still run on every set:

```c
astMAKE_SET1(MathMap,Seed,int,rcontext.seed,(
                    ( value != this->rcontext.seed ) ? astClearIsSimple(this) : (void)0,
                                             this->rcontext.seed_set = 1,
                                             this->rcontext.active = 0,
                                             value ))
```

- **`GrismMap` and `PcdMap`** clear inside their own hand-written macros (`grismmap.c:165,255`, `pcdmap.c:311,506`), beside a clone check and `UpdateConstants`. Guard the clear there, leaving the assignment and `UpdateConstants` unconditional:

```c
   } else { \
      if( (assign) != this->component ) astClearIsSimple( this ); \
      this->component = (assign); \
   } \
```

- **`PcdMap`'s `PcdCen`** is per-axis, so the comparison is against `this->component[ axis ]`.

Find every site, and note that no single edit to `astMAKE_SET1`'s definition covers the set:

```bash
cd reference/starlink-ast/src
grep -n 'astClearIsSimple' zoommap.c permmap.c lutmap.c sphmap.c pcdmap.c \
                           mathmap.c polymap.c splinemap.c wcsmap.c grismmap.c
```

Then amend the justification at `src/mapping.h:455`, which is what made this defect survive review:

```c
/* Discarding a record can only ever suppress a simplification, never make one
   wrong, so a clear is safe wherever an attribute genuinely changes.  It is
   not free, though: the record is written out as an IsSimp card and dumps are
   compared card for card, so clearing on a set that changes nothing makes a
   Mapping dump differently from the way it was read.  Each setter therefore
   clears only when the value differs -- which is what lets a loader validate
   by re-applying the value it just read. */
```

- [ ] **Step 4: Run the test to verify it passes, and the rest of the suite to verify nothing regressed**

```bash
cd reference/starlink-ast
cmake --build build -j
ctest --test-dir build -R '^testmapping$' --output-on-failure
ctest --test-dir build -E '^(simplify_|wcsconv_|transform_oracle_)' --output-on-failure
```

Expected: `testmapping` PASSES. The excluded families are the fixture-driven ones, which Task 2 regenerates — they are expected to move and are not evidence at this step.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add src/zoommap.c src/permmap.c src/lutmap.c src/sphmap.c src/pcdmap.c \
        src/mathmap.c src/polymap.c src/splinemap.c src/wcsmap.c \
        src/grismmap.c src/mapping.h ast_tester/testmapping.c
git commit -m "mapping: keep IsSimple across a loader's validating set

astLoadMapping reads the IsSimp card and sets the flag, then the subclass
loader validates the attribute it just read by re-applying it through a
setter that calls astClearIsSimple.  The value has not changed, so the
clear is wrong and the library could not read back the card it wrote.
ZoomMap, PcdMap, MathMap and GrismMap are the four classes whose setters
clear the flag; each now restores it around the validating call."
```

---

## Task 2: Regenerate the simplify corpus after the `IsSimp` fix

**Outcome, recorded 2026-09-03: the blast radius is zero, and the plan's stated
mechanism was wrong.** Kept here as the record of what was measured.

The plan predicted that preserving nested `IsSimp` would change what a re-read
tree simplifies to, and that the regeneration would show it.
Neither held:

- `gen_simplify_fixtures` almost never reads a fixture — it constructs objects
  through the public API, simplifies and dumps (only `:1455` and `:3538` read
  anything), so the loader fix cannot change its output.
- The `simplify_*` ctests *do* read (`simplify.c:83`), so they were the real
  exposure. They did not move either: 1202 of 1202 fixture-driven tests pass,
  and 72 of 72 of the rest.

The reason is that the fix makes read→simplify *more* like the
construct→simplify path the references were generated from, not less. Before
it, dropping the card meant re-walking nodes that then simplified to the same
result.

Regenerating produced a diff in exactly two files, `neg_time_5arg_mismatch.map`
and `time_5arg_cancel.map`, both a one-ULP change to `Time1f` — the
ERFA-computed geodetic-to-geocentric value that
`crates/starlink-ast/tests/test_serialisation.rs:1122` and
`simplify_tests.txt:308` already name as the canonical platform drift in this
corpus. TimeMap is not one of the ten affected classes, and a guard on when a
flag is cleared cannot alter a computed double. That is numeric drift captured
on another host, so per the sync protocol it was reverted rather than committed.

**There is therefore no commit for this task.** The steps below are what was
run.

- [ ] **Step 1: Record the pre-regeneration state**

```bash
cd reference/starlink-ast
git rev-parse HEAD > /tmp/pre_regen_head
ctest --test-dir build -R '^simplify_' 2>&1 | tail -5
```

Note the pass/fail counts. They are the baseline the regeneration has to improve or hold.

- [ ] **Step 2: Regenerate**

```bash
cd reference/starlink-ast
cmake --build build --target gen_simplify_fixtures -j
./build/ast_tester/gen_simplify_fixtures
git diff --stat -- ast_tester/fixtures/simplify | tail -3
```

- [ ] **Step 3: Classify every changed file**

```bash
cd reference/starlink-ast
git diff -- ast_tester/fixtures/simplify > /tmp/regen.diff
grep -E '^[-+]' /tmp/regen.diff | grep -v '^[-+][-+]' | sed 's/=.*/= .../' | sort | uniq -c | sort -rn
```

Expected: changes confined to `IsSimp` lines. Any changed line that is *not* an `IsSimp` card means the fix altered a simplification result rather than only its recorded state — that is a finding. Do not commit it until you can name the fixture, the rule involved, and why the new result is correct; if you cannot, stop and report rather than blessing it.

- [ ] **Step 4: Verify the suite**

```bash
cd reference/starlink-ast
ctest --test-dir build -R '^simplify_' --output-on-failure 2>&1 | tail -5
```

Expected: no more failures than the Step 1 baseline, and normally zero.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/fixtures/simplify
git commit -m "simplify fixtures: regenerate after the IsSimple loader fix

Nested IsSimp cards now survive a load, and astSimplify returns a Mapping
unchanged when IsSimple is set, so a re-read tree can simplify differently.
Every changed line is an IsSimp card; no simplification result moved."
```

Amend the final sentence to state what actually changed if Step 3 found more than `IsSimp` lines.

---

## Task 2b: The read-then-dump gate (added during execution)

**Landed 2026-09-03** as `c2c50521` and `d13cbe54`. Recorded here because it was
not in the plan and changes two later tasks.

Task 2 showed that upstream cannot witness the `IsSimp` defect at all: both
simplify families call `astSimplify` after reading, and `astSimplify` re-sets
`IsSimple` on every node it visits, so a record a loader dropped is put straight
back before anything compares it. `_noop` also compares *stripped* files. The
Rust port's 43 `.rt` files existed precisely because its round-trip is read→dump
with no simplify in between.

So upstream gained the gate it lacked: `ast_tester/roundtrip.c` reads one Object
and writes it straight out, `cmake/run_roundtrip_test.cmake` requires the output
to be byte-identical to the input, and one ctest is registered per fixture —
both halves of every simplify pair, plus a glob over `programs/**/*.ast` and
`serialisation/**/*.ast`. No reference file and no skip flag: the assertion does
not depend on what the fixture contains, so a fixture that cannot round-trip is
a serialization defect. The four IntraMap functions `simplify.c` registers are
registered here too, so those fixtures are readable rather than excluded.
`wcsconv/` is excluded: its references are FITS-encoded, so reading one back
needs a FitsChan and the encoding it was written with.

**This is the fixture-level demonstration that the `IsSimp` fix works.** Of the
43 fixtures the Rust port needed a `.rt` override for, the 39 that exist
upstream now round-trip byte-identically, none failing — so the override
mechanism is provably unnecessary.

The gate found 13 pre-existing failures on first run, and the partition is
informative: **not one was built by `gen_simplify_fixtures`.** Every fixture the
generator owns already round-tripped; the ones that did not were exactly those
with no generator, whose stored form was whatever hand-made them rather than
what the Channel writes today. Each was replaced by its own round-trip output
and re-round-tripped to confirm it is now a fixed point:

| fixture | cause |
|---|---|
| `brad`, `lsst1`, `intramap_inverse_cancel` | no leading-space indentation, or no comments |
| `rigby` | fewer significant digits, plus a redundant `Invert = 0` |
| `series_parallel_absorb`, `reconstruct_right_assoc` | the only two files in the corpus ending in a blank line |
| `intra_invfwd_cancel_tail` | recorded the IntraMap `Purp` / `Auth` of whatever registration created it |
| `cap_timemap_02` | a KeyMap re-dumped with two entries transposed — the fixture predated upstream's KeyMap ordering fix, and the normalized form is a fixed point, which is the evidence that fix holds |
| the five `programs/**/*.ast` | 15-significant-digit values where the library now writes 17 (the same doubles, fully specified), dropped redundant `Invert = 0`, and five missing `MemCnt` cards |

Normalizing a `.map` changes only how the input is written, never the object
read from it, so no `.simp` reference moved.

**Two later tasks change as a result.** U2's "`gen_roundtrip_refs.sh` and
`ast2ast.c` are deleted, not ported" is wrong: `ast2ast.c` became `roundtrip.c`
here, so it moves upstream rather than being deleted, and only
`gen_roundtrip_refs.sh` is deleted. And Task 3's and Task 4's new directories
are covered by the glob automatically, so each must confirm its
`roundtrip_serialisation_*` / `roundtrip_wcsconv_framesets_*` tests pass rather
than assuming the byte-identity check in its own steps is the only gate.

---

## Task 3: `gen_native_fixtures` and the `serialisation/` tree

The 24 minimal per-class dumps pin the serializer for a *bare* object of each class — attribute set, defaults, comment text, `IsA` boundaries — which neither the simplify corpus (only simplified composites) nor `framesets/` (only FITS-derived FrameSets) covers. Their generator was added to this tree by `1f6b322` in the Rust repo and never committed, so the source is gone; only a stale binary survives in the untracked `build/ast_tester/gen_native_fixtures`. It is rewritten here from the fixtures it produced, which fully determine its output. See spec §2.1.

**Files:**
- Create: `ast_tester/gen_native_fixtures.c`
- Create: `ast_tester/fixtures/serialisation/` (29 files)
- Modify: `ast_tester/CMakeLists.txt`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `ast_tester/fixtures/serialisation/*.ast`, read by Task 11's oracle generation and by Part 2's round-trip sweep.

- [ ] **Step 1: Recover the target bytes and the object list**

The 24 committed fixtures are in the Rust repo. Copy them in as the reference the generator must reproduce, and read each one to recover its constructor arguments — a `.map`/`.ast` dump is the complete native serialization, so every attribute is present:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
mkdir -p reference/starlink-ast/ast_tester/fixtures/serialisation
cp crates/starlink-ast/tests/fixtures/native/generated/*.ast \
   reference/starlink-ast/ast_tester/fixtures/serialisation/
cp crates/starlink-ast/tests/fixtures/dssmap/c_dssmap.ast \
   crates/starlink-ast/tests/fixtures/frameset/c_variants.ast \
   crates/starlink-ast/tests/fixtures/jan-dsb.ast \
   crates/starlink-ast/tests/fixtures/frame_in_map_slot.ast \
   crates/starlink-ast/tests/fixtures/region_in_cmpframe.ast \
   reference/starlink-ast/ast_tester/fixtures/serialisation/
# specflux.ast duplicates wcsconv/inputs/specflux.ast byte-for-byte
rm reference/starlink-ast/ast_tester/fixtures/serialisation/specflux.ast
ls reference/starlink-ast/ast_tester/fixtures/serialisation | wc -l   # expect 28
```

`degen1.ast`, `lsst.ast`, `timj.ast` and `splittest1.ast` share a basename with a file elsewhere in the tree but **differ** from it — they are re-dumps through a Channel with specific attributes. Confirm that before assuming either copy is redundant:

```bash
cd reference/starlink-ast/ast_tester/fixtures
for f in degen1.ast lsst.ast timj.ast; do cmp -s serialisation/$f wcsconv/inputs/$f && echo "$f SAME" || echo "$f DIFFERS"; done
cmp -s serialisation/splittest1.ast programs/testcmpmap/splittest1.ast && echo "splittest1 SAME" || echo "splittest1 DIFFERS"
```

All four are expected to print DIFFERS. If one prints SAME, delete the `serialisation/` copy and drop it from the generator.

- [ ] **Step 2: Write the generator, one function per class group**

Create `ast_tester/gen_native_fixtures.c`. The 19 objects it builds are, with the class each dump declares at its `Begin` line:

| file | class | file | class |
|---|---|---|---|
| `zoommap.ast` | ZoomMap | `basicframe.ast` | Frame |
| `matrixmap.ast` | MatrixMap | `skyframe.ast` | SkyFrame |
| `matrixmap_diagonal.ast` | MatrixMap | `skyframe_galactic.ast` | SkyFrame |
| `permmap.ast` | PermMap | `specframe.ast` | SpecFrame |
| `permmap_constant.ast` | PermMap | `timeframe.ast` | TimeFrame |
| `shiftmap.ast` | ShiftMap | `cmpframe.ast` | CmpFrame |
| `winmap.ast` | WinMap | `frameset_simple.ast` | FrameSet |
| `unitmap.ast` | UnitMap | `frameset_3frame.ast` | FrameSet |
| `cmpmap_series.ast` | CmpMap | `frameset_skyspec.ast` | FrameSet |
| `cmpmap_parallel.ast` | CmpMap | | |

The remaining five (`degen1`, `lsst`, `timj`, `splittest1`, `c_dssmap`, `c_variants`, `jan-dsb`, and the two probes) are *not* built from scratch — they are re-dumps of existing objects or captures from other programs, so the generator does not produce them and they stay as committed data. Note that distinction in the file header so a reader does not expect 28 outputs from a 19-output program.

Follow `gen_simplify_fixtures.c`'s shape exactly — a `write_object` helper plus one `gen_*` function per group:

```c
/*
 * gen_native_fixtures.c
 *
 * Writes one minimal native dump per Mapping / Frame / FrameSet class, so the
 * serialiser is pinned for a bare object of each class: its attribute set, its
 * defaults, its comment text and its IsA boundaries.  The simplify corpus
 * cannot cover that, because it holds only simplified composites, and
 * wcsconv/framesets/ cannot, because it holds only FITS-derived FrameSets.
 *
 * This program writes 19 of the files in fixtures/serialisation/.  The rest of
 * that directory is committed data captured elsewhere: re-dumps of objects that
 * live under wcsconv/inputs/ and programs/, two probes for the Frame-in-a-
 * Mapping-slot question, and captures from testdsbspecframe and the DssMap
 * tests.
 *
 * Build:
 *   cmake --build build --target gen_native_fixtures
 * Run (from the repository root):
 *   ./build/ast_tester/gen_native_fixtures
 */

#include "ast.h"
#include <stdio.h>

static void write_object( const char *dir, const char *name, AstObject *obj ) {
    char path[ 512 ];
    AstChannel *chan;

    snprintf( path, sizeof path, "%s/%s.ast", dir, name );
    chan = astChannel( NULL, NULL, "SinkFile=%s", path );
    if( astWrite( chan, obj ) != 1 ) {
        fprintf( stderr, "ERROR: astWrite failed for %s.ast\n", name );
    }
    chan = astAnnul( chan );
    printf( "  %s\n", name );
}

static void gen_mapping_fixtures( const char *dir ) {
    printf( "Mapping fixtures:\n" );
    /* Read each committed .ast to recover the exact arguments; the dump lists
       every attribute the constructor set.  zoommap.ast, for example, is
       Nin = 2 with Zoom = 3.5. */
    {
        AstZoomMap *m = astZoomMap( 2, 3.5, " " );
        write_object( dir, "zoommap", (AstObject *) m );
        m = astAnnul( m );
    }
    /* ... one block per row of the table above ... */
}

int main( void ) {
    int status = 0;
    const char *dir = "ast_tester/fixtures/serialisation";

    astWatch( &status );
    astBegin;

    gen_mapping_fixtures( dir );
    gen_frame_fixtures( dir );
    gen_frameset_fixtures( dir );

    astEnd;
    return status != 0;
}
```

Recover each constructor's arguments by reading its committed dump, e.g.:

```bash
cd reference/starlink-ast/ast_tester/fixtures/serialisation
for f in zoommap matrixmap matrixmap_diagonal permmap permmap_constant shiftmap winmap unitmap; do echo "--- $f"; cat $f.ast; done
```

- [ ] **Step 3: Register the cmake target**

In `ast_tester/CMakeLists.txt`, beside the `gen_simplify_fixtures` block near line 383:

```cmake
add_executable(gen_native_fixtures gen_native_fixtures.c)
target_include_directories(gen_native_fixtures PRIVATE ${TEST_INCLUDES})
target_link_libraries(gen_native_fixtures PRIVATE ${TEST_LIBS})
target_compile_definitions(gen_native_fixtures PRIVATE HAVE_CONFIG_H)
set_target_properties(gen_native_fixtures PROPERTIES C_STANDARD 11)
ast_apply_dev_options(gen_native_fixtures)
```

`C_STANDARD 11` matches `gen_transform_oracle`: the dumps carry 17 significant digits, which needs `DBL_DECIMAL_DIG`.

- [ ] **Step 4: Verify byte-identity against the committed fixtures**

```bash
cd reference/starlink-ast
cmake --build build --target gen_native_fixtures -j
./build/ast_tester/gen_native_fixtures
git status --short -- ast_tester/fixtures/serialisation
git diff --stat -- ast_tester/fixtures/serialisation
```

Expected: the 19 generated files show as unmodified (they were staged in Step 1 as the reference), and `git diff` is empty for them. A diff means the reconstruction is wrong — read the diff, fix the constructor arguments, and re-run. Do not adjust the committed fixture to match the generator; the committed bytes are the reference.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/gen_native_fixtures.c ast_tester/CMakeLists.txt ast_tester/fixtures/serialisation
git commit -m "ast_tester: add serialisation fixtures and their generator

One minimal native dump per Mapping / Frame / FrameSet class, pinning the
serialiser for a bare object of each: attribute set, defaults, comment text
and IsA boundaries.  Neither the simplify corpus nor wcsconv/framesets/
covers that shape.

The generator reproduces 19 of the directory's files byte-for-byte; the rest
is committed data captured by other programs, as the file header records."
```

---

### Task 3 outcome (2026-09-03)

Landed. `gen_native_fixtures.c` reproduces all 19 built fixtures byte-exactly,
verified by deleting them and rebuilding from nothing.

Four things differed from the plan's steps:

- `splittest1.ast` turned out to be byte-identical to
  `programs/testcmpmap/splittest1.ast` *after* Task 2b normalized that file — the
  Rust-side copy had been in the modern 17-digit form all along. Dropped as a
  duplicate, along with `specflux.ast`. So `serialisation/` holds 27 files, not 29.
- `degen1.ast`, `lsst.ast` and `timj.ast` are exactly
  `roundtrip(wcsconv/inputs/<name>.ast)`, so they need no generator code. Their
  value is that they *are* fixed points where the wcsconv inputs are not.
- `jan-dsb.ast` lacked five commented default cards the library now writes, so it
  was normalized like the Task 2b fixtures.
- Two constructors needed correcting on the first byte-identity run: the SkyFrame
  in `frameset_3frame` has `Equinox` set, and the SpecFrame in
  `frameset_skyspec` has `Unit=GHz` set. The gate caught both, which is what it
  is for.

`wcsconv/inputs/*.ast` stay out of the round-trip gate. Stripping the leading
header (see Task 2b) is not enough: `degen1.ast` still differs by 40 lines —
seven redundant `Invert = 0` cards, thirteen `IM0`–`IM8` inverse-matrix values,
`M1`–`M8` precision, added `PlrLg` defaults, a changed commented `Domain`
default, and a dropped `ActUnt = 0`. Those are library changes since the fixture
was captured, so including them would mean normalizing the wcsconv inputs, which
is a larger change than this branch needs.

---

## Task 4: `head2ast` and the `wcsconv/framesets/` tree

139 `.ast` files, each the library's dump of the FrameSet obtained by reading one `wcsconv/inputs/` header. This is the corpus that proves the Native serializer round-trips FITS-derived FrameSets. The generator exists in the Rust repo at `tools/oracle_fixtures/head2ast.c` and moves here so the data is reproducible from this tree.

**Files:**
- Create: `ast_tester/head2ast.c` (moved), `ast_tester/fixtures/wcsconv/framesets/` (139 files)
- Modify: `ast_tester/CMakeLists.txt`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `wcsconv/framesets/*.ast`, read by Task 11 and by Part 2's round-trip sweep.

- [ ] **Step 1: Move the generator in and read what it does**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cp tools/oracle_fixtures/head2ast.c reference/starlink-ast/ast_tester/head2ast.c
cat reference/starlink-ast/ast_tester/head2ast.c
```

It mirrors the AST branch of `wcsconverter.c`: read a header through a `FitsChan`, then dump the resulting object through a `Channel` with `Comment=1,Full=0`. Update its header comment so it no longer refers to the Rust repo's paths, and so it names `fixtures/wcsconv/framesets/` as its output directory and `fixtures/wcsconv/cases.txt` as the source of the input list.

- [ ] **Step 2: Register the cmake target**

In `ast_tester/CMakeLists.txt`, beside the other generator targets:

```cmake
add_executable(head2ast head2ast.c)
target_include_directories(head2ast PRIVATE ${TEST_INCLUDES})
target_link_libraries(head2ast PRIVATE ${TEST_LIBS})
target_compile_definitions(head2ast PRIVATE HAVE_CONFIG_H)
set_target_properties(head2ast PROPERTIES C_STANDARD 11)
ast_apply_dev_options(head2ast)
```

- [ ] **Step 3: Generate the corpus**

`head2ast` converts one file, so drive it over the distinct inputs named by `wcsconv/cases.txt`:

```bash
cd reference/starlink-ast
cmake --build build --target head2ast -j
mkdir -p ast_tester/fixtures/wcsconv/framesets
grep -v '^#' ast_tester/fixtures/wcsconv/cases.txt | grep -v '^$' \
  | awk -F'|' '{gsub(/ /,"",$2); print $2}' | sort -u \
  | while read -r inp; do
      base=$(basename "$inp"); stem="${base%.*}"
      ./build/ast_tester/head2ast "ast_tester/fixtures/$inp" \
        "ast_tester/fixtures/wcsconv/framesets/$stem.ast" || echo "FAILED $inp"
    done
ls ast_tester/fixtures/wcsconv/framesets | wc -l
```

- [ ] **Step 4: Verify byte-identity against the Rust repo's committed copies**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
diff -r crates/starlink-ast/tests/fixtures/native/reference/frameset_fixtures \
        reference/starlink-ast/ast_tester/fixtures/wcsconv/framesets
```

Expected: no output. A difference means the driver loop's attribute settings differ from what `gen_frameset_fixtures.sh` used — read `tools/oracle_fixtures/gen_frameset_fixtures.sh` and match it, rather than accepting the new bytes. If the file *counts* differ, the manifest's distinct-input set has changed; reconcile that explicitly and say so in the commit message.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/head2ast.c ast_tester/CMakeLists.txt ast_tester/fixtures/wcsconv/framesets
git commit -m "ast_tester: add wcsconv/framesets and the head2ast generator

Each file is the library's dump of the FrameSet read from one wcsconv input,
which is what pins Native round-tripping of FITS-derived FrameSets.  The
generator moves in from the Rust port's tools directory so the data is
reproducible from this tree."
```

---

### Task 4 outcome (2026-09-03)

Landed. `head2ast.c` did not move across as-is: it converts one file, and the
manifest walk lived in a shell script in the Rust port. Upstream got
`gen_frameset_fixtures.c` instead, which walks `wcsconv/cases.txt` itself, in the
same shape as `gen_simplify_fixtures` and `gen_native_fixtures`. It writes 139
fixtures, one per distinct input, and two runs are byte-identical.

**The byte-identity gate against the Rust port's copies does not hold, and
should not.** 39 of the 139 differ, because those copies were generated
**2026-06-15** and every cause postdates that:

| files | difference | cause |
|---|---|---|
| 37 | `IsSimp` cards on different nodes | `e40e71b4` (2026-08-15) records the simplified state per orientation, which changed which nodes carry the card |
| 4 | redundant `Invert` cards dropped | same class of change seen when normalizing the older fixtures in Task 2b |
| 1 | `Epoch` 2017.5519807923772 → 2017.5519807675655 | `254d4c99` (2026-06-19), whose message names `azel.head`'s exact `DATE-OBS` and the 0.783 s error: `%d` dropped the leading zero in `.087`, reconstructing 0.87 s |
| 2 | `DSBCen`, `IF` | `ad04291c` (2026-06-20) stopped losing precision when setting RefRA/RefDec, which the standard-of-rest correction depends on |
| 1 | `Sft1`, `Scl1` (`vrad-v2f`) | the `astRate` fixes `e782c220` and `c1a88658` (2026-06-19); `project_vradv2f_astrate_dependency` already records that this fixture is astRate-driven |

So the regenerated fixtures are *more correct* than the ones the Rust port has
been testing against, and the gate for this task is that the generator is
deterministic and that every difference is explained — not that it reproduces
stale bytes.

Note for Part 2 R2: the wholesale mirror brings all 39 changed fixtures across at
once. They are not regressions, and the round-trip sweep should pass on them
immediately. `azel-head.ast`'s Epoch change in particular is a bug fix the Rust
port must match — if it does not, that is a real FITS-read gap in `DateObs`.

Also: `wcsconv/framesets/` **is** covered by the round-trip glob. Only
`wcsconv/inputs/` and `wcsconv/expected/` stay out. The framesets are pure native
dumps the generator wrote, so they are fixed points; the inputs are hand-edited
and the expected outputs are FITS-encoded.

---

## Task 5: `gen_keymap_mapsz` and `oracle/keymap_mapsz.txt`

The Rust port compares its `MapSz` card emission against a C capture currently living at `crates/starlink-ast/tests/fixtures/keymap/mapsz_oracle.txt`. Moving the probe here makes that capture reproducible.

**Files:**
- Create: `ast_tester/gen_keymap_mapsz.c`, `ast_tester/fixtures/oracle/keymap_mapsz.txt`
- Modify: `ast_tester/CMakeLists.txt`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `oracle/keymap_mapsz.txt`, read by Part 2's `test_keymap_serial.rs`.

- [ ] **Step 1: Read the format the Rust side parses**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
sed -n '285,320p' crates/starlink-ast/tests/test_keymap_serial.rs
head -5 crates/starlink-ast/tests/fixtures/keymap/mapsz_oracle.txt
```

The Rust parser's doc comment states the format is the probe's `printf`. Reproduce exactly that, field for field.

- [ ] **Step 2: Write the probe**

Create `ast_tester/gen_keymap_mapsz.c`. Build KeyMaps across the size range the committed capture covers, dump each through a `Channel`, and print one line per case in the recorded format. Read the committed file to recover the case list — it is the specification for this program:

```bash
cat /sdf/home/t/timj/WORK/starlink-astrs/crates/starlink-ast/tests/fixtures/keymap/mapsz_oracle.txt
```

- [ ] **Step 3: Register the cmake target**

```cmake
add_executable(gen_keymap_mapsz gen_keymap_mapsz.c)
target_include_directories(gen_keymap_mapsz PRIVATE ${TEST_INCLUDES})
target_link_libraries(gen_keymap_mapsz PRIVATE ${TEST_LIBS})
target_compile_definitions(gen_keymap_mapsz PRIVATE HAVE_CONFIG_H)
ast_apply_dev_options(gen_keymap_mapsz)
```

- [ ] **Step 4: Verify byte-identity**

```bash
cd reference/starlink-ast
cmake --build build --target gen_keymap_mapsz -j
./build/ast_tester/gen_keymap_mapsz > ast_tester/fixtures/oracle/keymap_mapsz.txt
diff ast_tester/fixtures/oracle/keymap_mapsz.txt \
     ../../crates/starlink-ast/tests/fixtures/keymap/mapsz_oracle.txt
```

Expected: no output.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/gen_keymap_mapsz.c ast_tester/CMakeLists.txt ast_tester/fixtures/oracle/keymap_mapsz.txt
git commit -m "ast_tester: add the KeyMap MapSz capture and its probe

The Rust port compares its MapSz card emission against this capture.  Moving
the probe here makes the capture reproducible from this tree rather than from
an uncommitted program."
```

---

## Task 6: 25 constructor functions for the Rust-originated simplify fixtures

46 files across 25 case stems are Rust-originated and reproducible by nothing: `gen_simplify_fixtures.c` does not know their names, and the commits that added them carry no `.c`. Copying the bytes would leave 25 fixtures that Task 11's regeneration orphans. Only the `.map` half needs reconstructing — `write_fixture` derives the `.simp` by calling `astSimplify` and dumping again. See spec §4 U3.

**Files:**
- Modify: `ast_tester/gen_simplify_fixtures.c`, `ast_tester/fixtures/simplify/cases.txt`
- Create: 46 files under `ast_tester/fixtures/simplify/`

**Interfaces:**
- Consumes: Task 2's regenerated corpus (so a re-run does not also move existing files).
- Produces: 46 fixtures plus 25 `cases.txt` rows; read by Tasks 8, 9 and 11.

- [ ] **Step 1: Stage the target bytes and the manifest rows**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
S=crates/starlink-ast/tests/fixtures/native/reference
U=reference/starlink-ast/ast_tester/fixtures/simplify
comm -13 <(ls $U | sort) <(ls $S/simplify_fixtures | sort) | grep -v '\.rt$' > /tmp/new_fixtures.txt
wc -l /tmp/new_fixtures.txt          # expect 46
sed 's/\.[^.]*$//' /tmp/new_fixtures.txt | sort -u > /tmp/new_stems.txt
wc -l /tmp/new_stems.txt             # expect 25
while read -r f; do cp "$S/simplify_fixtures/$f" "$U/$f"; done < /tmp/new_fixtures.txt
```

The 25 stems are: `cap_cmpmap_nested_tag`, `cefit_circle_conformal`, `cefit_circle_nonconformal_keep`, `cefit_circle_to_ellipse`, `cefit_ellipse_to_circle`, `defunc_cmpregion_component_unc`, `defunc_interval_unbounded`, `defunc_nullregion_2d`, `defunc_prism_box_interval`, `merge_box_fillfactor_product`, `merge_box_meshsize_scaled`, `merge_interval_meshsize_scaled`, `neg_cmpmap_neighbour_not_cmpmap`, `neg_wcsmap_perm_allconst`, `neg_wcsmap_perm_both_sides`, `neg_win_series_cmpmap_neighbour`, `regbasepick_circle_permaxes`, `regbasepick_cmpregion_subset`, `regbasepick_prism_subset`, `setunc_box_remapped_unc`, `setunc_unbounded_negated`, `setunc_unitmap_regionfs_omitted`, `simpstab_box_linear_unc`, `simpstab_box_nonlinear_unc`, `simpstab_permmap_slice_unc`. Four of them — the `neg_*` ones — have only a `.map`, and take `write_negative_fixture` rather than `write_fixture`.

- [ ] **Step 2: Recover each constructor from its committed dump**

A `.map` is the complete native serialization of the constructed object, so it lists every attribute. `docs/audits/region-phase5-parity-spec.md` in the Rust repo states each fixture's *intent*, which tells you which branch it must reach; the dump gives the numbers. Read both for the group you are working on:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cat reference/starlink-ast/ast_tester/fixtures/simplify/cefit_circle_to_ellipse.map
sed -n '/cefit_circle_to_ellipse/,+12p' docs/audits/region-phase5-parity-spec.md
```

- [ ] **Step 3: Add one `gen_*` function per group**

In `ast_tester/gen_simplify_fixtures.c`, add six functions grouping the 25 stems by the behavior they exercise, and call each from `main` alongside the existing `gen_*` calls. Following the existing file's shape:

```c
/* ===== Circle/Ellipse Simplify re-fit fixtures ===== */
static void gen_cefit_fixtures(const char *dir) {
    printf("Circle/Ellipse re-fit fixtures:\n");
    {
        /* A Circle mapped through an anisotropic scaling becomes an ellipse, so
           the .simp is an Ellipse: the Circle degrades.  This is the fixture a
           naive re-fit fails, by wrongly keeping a Circle. */
        AstFrame *frm = astFrame( 2, " " );
        double centre[ 2 ] = { 0.0, 0.0 };
        double radius = 1.0;
        AstCircle *circle = astCircle( frm, 1, centre, &radius, NULL, " " );
        double zoom[ 2 ] = { 2.0, 5.0 };
        AstMatrixMap *mm = astMatrixMap( 2, 2, 1, zoom, " " );
        AstRegion *mapped = astMapRegion( circle, (AstMapping *) mm, frm );
        write_fixture( dir, "cefit_circle_to_ellipse", (AstMapping *) mapped );
        mapped = astAnnul( mapped );
        mm = astAnnul( mm );
        circle = astAnnul( circle );
        frm = astAnnul( frm );
    }
    /* ... one block per stem in this group ... */
}
```

Substitute the actual center, radius, matrix and Frame attributes read from each committed dump — the code above shows the *shape*, and Step 4 is what proves the numbers. Add the analogous `gen_defunc_fixtures`, `gen_regbasepick_fixtures`, `gen_setunc_fixtures`, `gen_simpstab_fixtures` and `gen_merge_extra_fixtures`; put `cap_cmpmap_nested_tag` in `gen_cmpmap_fixtures` and the four `neg_*` stems in `gen_negative_fixtures`, beside their existing relatives.

- [ ] **Step 4: Regenerate and require an empty diff**

```bash
cd reference/starlink-ast
cmake --build build --target gen_simplify_fixtures -j
./build/ast_tester/gen_simplify_fixtures
git diff --stat -- ast_tester/fixtures/simplify
```

Expected: empty. Every one of the 46 staged files must be reproduced byte-for-byte. A diff names the fixture whose constructor arguments are still wrong; fix the constructor, never the fixture. This step is the whole point of the task — without it the functions are plausible rather than correct.

- [ ] **Step 5: Register the 25 cases and run them**

Copy the rows from the Rust manifest, rewriting the paths to the shared form and dropping the `render_options` column:

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
grep -v '^#\|^$' crates/starlink-ast/tests/fixtures/native/reference/simplify_tests.txt \
 | awk -F'|' '{n=$1; gsub(/^ +| +$/,"",n); print n"|"$0}' \
 | grep -F -f <(sed 's/^/|/;s/$/|/' /tmp/new_stems.txt | tr -d '|' | sed 's/^/^/;s/$/|/') \
 | cut -d'|' -f2-
```

Add each as a row in `ast_tester/fixtures/simplify/cases.txt` in the form
`<name> | simplify/<name>.map | simplify/<name>.simp |`, with the four `neg_*`
stems pointing their reference at their own `.map` exactly as the existing
negative rows do. Then:

```bash
cd reference/starlink-ast
cmake -S . -B build   # re-run configure: the fixture glob changed
cmake --build build -j
ctest --test-dir build -R '^simplify_(cefit|defunc|regbasepick|setunc|simpstab|merge_box|merge_interval|cap_cmpmap_nested_tag|neg_cmpmap_neighbour_not_cmpmap|neg_wcsmap_perm|neg_win_series)' --output-on-failure
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/gen_simplify_fixtures.c ast_tester/fixtures/simplify
git commit -m "ast_tester: adopt 25 simplify fixtures from the Rust port

These fixtures were written for the Region simplify parity work in the Rust
port and committed there as bytes, with no generator in either repo.  Each
now has a constructor function here, so a corpus regeneration reproduces it
rather than orphaning it, and the regenerated output is byte-identical to the
bytes the Rust port has been testing against."
```

---

### Task 6 outcome (2026-09-03)

Landed. 23 of the 25 stems adopted, each with a constructor in
`gen_simplify_fixtures.c`. Two were dropped, and the gate changed.

**The gate is intent, not bytes.** Byte-identity against the Rust port's copies
would have encoded inputs I could not verify: several of these fixtures are
*derived* values — the `defunc_*` set is whatever `astGetUnc` returns — so a
mismatch could mean a wrong reconstruction or a stale reference, and some of them
predate `e40e71b4`. Instead each Region was rebuilt from the intent recorded in
`docs/audits/region-phase5-parity-spec.md`, and the check is that it reaches the
same logic: 34 of the 46 files changed, all inside the 25 stems, no collateral
change anywhere in the 800-file corpus.

Seven converged byte-exactly anyway, and the byte gate earned its keep while it
applied — it caught an Interval upper bound that had to be `AST__BAD`, an
`astBox` form argument (0 is two corners, 1 is center-and-corner), and a guessed
`FillFactor` of 0.25 that had to be 0.8 to give the recorded product of 0.4.

**Two findings worth keeping.**

`regbasepick_cmpregion_subset` came out as a `CmpRegion` where the Rust port's
copy is a `Box`. The spec says the fixture exists to prove `astSimplify` "keeps a
CmpRegion of the picked sub-regions", so the Rust copy never reached the branch
it was written for. The reconstruction does. (A `Box` cannot leave an axis
unconstrained, which is why both components are Intervals.)

`neg_cmpmap_neighbour_not_cmpmap` and `neg_win_series_cmpmap_neighbour` are
dropped. Neither is referenced anywhere in the Rust repo — not its manifest, not
a test, not the audit docs — and C simplifies both, so neither was ever a valid
negative. Checked against the corpus for duplicate coverage rather than adopted
on the strength of their names: the first's load-bearing reduction is literally
`zoom_parallel_to_matrix` (same `Zoom = 2` and `3`, same diagonal MatrixMap),
with an inert MathMap beside it; the second decomposes into `zoom_series_merge`,
`win_zoom_series_merge` and the nested-series-CmpMap traversal already in
`win_swap_past_matrix`. Only their composition was new, and a composition is not
a rule. The other two `neg_wcsmap_perm_*` are genuine negatives and are kept.

The `defunc_*` group needed no internal access after all: the spec assumed a probe
calling `astGetUncFrm`, but `astGetUnc(region, def)` is public and gives the same
default.

---

## Task 7: The three remaining fixture imports

`lsst.simp` is the only fixture captured with `Comment=0,Indent=0`; regenerating it with the Channel's defaults lets the `render_options` column disappear from the shared manifest. `offset-sref.native` and `pc-sparse-naxis.head` are two more Rust-only inputs.

**Files:**
- Create: `ast_tester/fixtures/simplify/lsst.simp`, `ast_tester/fixtures/wcsconv/expected/offset-sref.native`, `ast_tester/fixtures/wcsconv/inputs/pc-sparse-naxis.head`
- Modify: `ast_tester/fixtures/simplify/cases.txt`, `ast_tester/fixtures/wcsconv/cases.txt`

**Interfaces:**
- Consumes: Task 6's `cases.txt` edits (same files — sequence after it to avoid a conflict).
- Produces: two more manifest rows and one more header input.

- [ ] **Step 1: Regenerate `lsst.simp` with default Channel attributes**

Its input is already in the tree as `wcsconv/inputs/lsst.ast`. The Rust port's copy was captured with comments and indentation off; the default capture is what goes upstream.

```bash
cd reference/starlink-ast
cmake --build build --target simplify -j
./build/ast_tester/simplify ast_tester/fixtures/wcsconv/inputs/lsst.ast \
                            ast_tester/fixtures/simplify/lsst.simp
wc -l ast_tester/fixtures/simplify/lsst.simp
```

Expected: roughly 213 lines and about 15 KB — the size of `rigby.simp`. The Rust port's copy is 211 lines / 3.5 KB because comments were suppressed; a result near 3.5 KB means the defaults did not apply.

- [ ] **Step 2: Import the two wcsconv files**

```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
R=crates/starlink-ast/tests/fixtures
U=reference/starlink-ast/ast_tester/fixtures
cp $R/wcsconv/expected/offset-sref.native $U/wcsconv/expected/offset-sref.native
cp $R/fits/headers/pc-sparse.head $U/wcsconv/inputs/pc-sparse-naxis.head
diff $U/wcsconv/inputs/pc-sparse.head $U/wcsconv/inputs/pc-sparse-naxis.head
```

Expected: the only difference is the added leading `NAXIS = 2` card. The distinct name is deliberate — the two headers exercise different things and neither should silently replace the other.

- [ ] **Step 3: Add the manifest rows**

In `ast_tester/fixtures/simplify/cases.txt`, beside the other scenario fixtures:

```
lsst  | wcsconv/inputs/lsst.ast | simplify/lsst.simp |
```

In `ast_tester/fixtures/wcsconv/cases.txt`, copying the encoding and attributes from the Rust manifest's `offset-sref_native` row:

```
offset-sref_native | wcsconv/inputs/offset-sref.ast | wcsconv/expected/offset-sref.native | native | FitsDigits=8 |
```

Confirm the input path and attributes against the source row before committing:

```bash
grep 'offset-sref' /sdf/home/t/timj/WORK/starlink-astrs/crates/starlink-ast/tests/fixtures/wcsconv/wcsconv_tests.txt
```

- [ ] **Step 4: Configure and run**

```bash
cd reference/starlink-ast
cmake -S . -B build
cmake --build build -j
ctest --test-dir build -R '^(simplify_lsst|wcsconv_offset-sref)' --output-on-failure
```

Expected: all pass. `ast_require_fixture` fails at configure time, naming the manifest and the path, if a row points at a file that is not there — so a configure failure here is a typo in a row, not a build problem.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/fixtures/simplify ast_tester/fixtures/wcsconv
git commit -m "ast_tester: adopt lsst, offset-sref and pc-sparse-naxis

lsst.simp is regenerated with the Channel's default Comment and Indent rather
than imported, so no manifest column is needed to describe how it was
captured.  pc-sparse-naxis.head differs from pc-sparse.head only by a leading
NAXIS card and takes a distinct name rather than replacing it."
```

---

### Task 7 outcome (2026-09-03)

Landed, with two of the three items changing shape.

**`lsst` needed a constructor, not a regeneration.** The Rust reference is a
`CmpMap`, not a `FrameSet`: it was captured with `gen_simp -m`, which replaces a
FrameSet by its base-to-current Mapping before simplifying. Upstream's `simplify`
has no such mode, and simplifying the FrameSet as a whole gives a FrameSet — a
different object. Rather than add a flag, `gen_lsst_scenario_fixture` extracts the
Mapping with `astGetMapping(fs, AST__BASE, AST__CURRENT)` and writes an ordinary
`.map`/`.simp` pair, so the corpus convention holds and the harness needs no new
mode. The result is structurally identical to the Rust reference once comments are
set aside, which is the intent check.

**`pc-sparse-naxis.head` is not added.** The Rust tree carries two `pc-sparse`
headers — `wcsconv/inputs/pc-sparse.head`, identical to upstream's, and
`fits/headers/pc-sparse.head`, which adds a leading `NAXIS = 2`. Only
`test_fitschan_encodings.rs` reads the second, and only to check that `PC001001`
drives encoding detection, which does not consult `NAXIS`. Adding an upstream
fixture no upstream test uses, to serve one Rust test that does not need the
difference, is the wrong way round.

*Part 2 action:* repoint `test_fitschan_encodings.rs`'s `load_fixture` at
`wcsconv/inputs/pc-sparse.head` and confirm its three assertions still hold. If
any of them turns out to depend on `NAXIS`, add the variant upstream then, with a
comment saying which assertion needs it.

**`offset-sref.native` was stale by one card.** `SCL1_A` differs in one column of
leading whitespace, from `68bcb7db` (2026-08-09) keeping wide float values right
justified in `EncodeFloat` — after the Rust reference was captured. Regenerated.
The row carries the Linux-capture note from the Rust manifest, since that caveat
is about the reference and travels with it.

---

## Task 8: `compare_dumps` — a ULP-and-`IsSimp`-aware comparator

`cmake/run_simplify_test.cmake` and `cmake/run_wcsconverter_test.cmake` compare with `cmake -E compare_files`, a byte comparison. That is why upstream opts out of string comparison on 165 of 467 simplify rows and 108 of 192 wcsconv rows, where the Rust harness — which compares numeric tokens to 4 ULP — opts out on 0 and 8. Until both sides compare to the same standard, the shared manifest's flags cannot be read by the Rust port without relaxing roughly 265 comparisons. See spec §3.

**Files:**
- Create: `ast_tester/compare_dumps.c`
- Modify: `ast_tester/CMakeLists.txt`, `cmake/run_simplify_test.cmake`, `cmake/run_wcsconverter_test.cmake`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `compare_dumps <ref> <out>`, exit 0 on match and 1 on mismatch with a unified diff on stderr. Tasks 9 and 10 re-audit the manifest flags against it.

- [ ] **Step 1: Write the comparator with its own self-test**

Create `ast_tester/compare_dumps.c`. The semantics are the Rust harness's, ported: compare line by line; a byte-equal line passes; otherwise split both lines on whitespace and compare token by token, where two tokens match if they are byte-equal or both parse as `double` and differ by at most 4 ULP.

```c
/*
 * compare_dumps.c
 *
 * Compare two AST dumps for equality, tolerating the last-bit variation that
 * libm and compiler differences produce across platforms.  A byte-equal line
 * passes; otherwise the two lines are compared token by token, and two tokens
 * match when they are byte-equal or both parse as a double within 4 ULP.
 *
 * This is what lets a dump comparison stay a real gate on a platform whose
 * last bits differ, instead of being opted out of altogether.
 *
 * Usage:
 *   compare_dumps <reference> <output>
 * Exit status:
 *   0  equal within tolerance
 *   1  differ (a unified diff of the first differing line goes to stderr)
 *   2  usage or I/O error
 */

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_ULPS 4

/* Number of representable doubles between a and b, saturating.  Values of
   opposite sign never match unless both are zero. */
static int within_ulps( double a, double b ) {
    long long ia, ib, diff;

    if( a == b ) return 1;
    if( isnan( a ) || isnan( b ) ) return 0;
    memcpy( &ia, &a, sizeof ia );
    memcpy( &ib, &b, sizeof ib );
    if( ( ia < 0 ) != ( ib < 0 ) ) return 0;
    diff = ia > ib ? ia - ib : ib - ia;
    return diff <= MAX_ULPS;
}
```

Add a `tokens_match`, a `lines_match`, and a `main` that reads both files, reports the first differing line to stderr in `--- ref` / `+++ out` form, and returns 1. Then add a self-test target so the comparator's own tolerance is tested rather than assumed:

```c
/* Built as compare_dumps_selftest with -DSELFTEST. */
#ifdef SELFTEST
int main( void ) {
    int fails = 0;
    /* Exact equality, and a one-ULP neighbour, match. */
    if( !within_ulps( 1.0, 1.0 ) ) { printf( "selftest-1\n" ); fails++; }
    if( !within_ulps( 1.0, nextafter( 1.0, 2.0 ) ) ) { printf( "selftest-2\n" ); fails++; }
    /* Five ULP does not. */
    {
        double v = 1.0;
        int i;
        for( i = 0; i < 5; i++ ) v = nextafter( v, 2.0 );
        if( within_ulps( 1.0, v ) ) { printf( "selftest-3\n" ); fails++; }
    }
    /* Opposite signs never match, however close. */
    if( within_ulps( nextafter( 0.0, 1.0 ), nextafter( 0.0, -1.0 ) ) ) {
        printf( "selftest-4\n" ); fails++;
    }
    return fails != 0;
}
#endif
```

- [ ] **Step 2: Register both targets and run the self-test to see it pass**

In `ast_tester/CMakeLists.txt`:

```cmake
add_executable(compare_dumps compare_dumps.c)
set_target_properties(compare_dumps PROPERTIES C_STANDARD 11)
ast_apply_dev_options(compare_dumps)

add_executable(compare_dumps_selftest compare_dumps.c)
target_compile_definitions(compare_dumps_selftest PRIVATE SELFTEST)
set_target_properties(compare_dumps_selftest PROPERTIES C_STANDARD 11)
add_test(NAME compare_dumps_selftest COMMAND compare_dumps_selftest)
```

`compare_dumps` links no AST library — it is pure text and arithmetic, which keeps it usable from a `cmake -P` driver without the library on the path.

```bash
cd reference/starlink-ast
cmake -S . -B build && cmake --build build --target compare_dumps compare_dumps_selftest -j
ctest --test-dir build -R '^compare_dumps_selftest$' --output-on-failure
```

Expected: PASS.

- [ ] **Step 3: Prove it on a real opted-out fixture before wiring it in**

Pick a fixture the manifest currently flags `yes` and check the comparator's verdict against a byte comparison:

```bash
cd reference/starlink-ast
./build/ast_tester/simplify ast_tester/fixtures/simplify/rigby.map /tmp/rigby.out
cmake -E compare_files ast_tester/fixtures/simplify/rigby.simp /tmp/rigby.out; echo "byte: $?"
./build/ast_tester/compare_dumps ast_tester/fixtures/simplify/rigby.simp /tmp/rigby.out; echo "ulp: $?"
```

Expected: the byte comparison reports a difference and the ULP comparison reports equality — that pair is the evidence the flag can be retired. If both report equality the flag was already unnecessary; if both report a difference the fixture needs its own diagnosis and stays flagged. Record which of the three you got, because Task 9 depends on it.

- [ ] **Step 4: Wire it into both drivers**

In `cmake/run_simplify_test.cmake`, replace the `compare_files` block with:

```cmake
execute_process(
    COMMAND "${COMPARE_DUMPS}" "${REF_FILE}" "${OUT_FILE}"
    RESULT_VARIABLE _rv
)
if(NOT _rv EQUAL 0)
    message(STATUS "Output mismatch; showing unified diff:")
    execute_process(COMMAND diff -u "${REF_FILE}" "${OUT_FILE}")
    message(FATAL_ERROR
        "simplify output ${OUT_FILE} differs from reference ${REF_FILE}")
endif()
```

Add `COMPARE_DUMPS` to the `foreach(_req IN ITEMS ...)` required-variable list at the top of the file, and pass `-DCOMPARE_DUMPS=$<TARGET_FILE:compare_dumps>` from `add_simplify_test` in `ast_tester/CMakeLists.txt` (near line 433) alongside the existing `-DSIMPLIFY=`. Make the identical change to `cmake/run_wcsconverter_test.cmake` and `add_wcsconv_test` (near line 309). Add `compare_dumps` to each test's dependencies so ctest cannot run before it is built.

- [ ] **Step 5: Run the two fixture families and confirm no regression**

```bash
cd reference/starlink-ast
cmake -S . -B build && cmake --build build -j
ctest --test-dir build -R '^(simplify_|wcsconv_)' --output-on-failure 2>&1 | tail -5
```

Expected: the same set passes as before. The comparator only ever *widens* what counts as equal, so a newly failing test means a bug in `compare_dumps` — most likely token splitting or a line-count mismatch going unreported. Fix it rather than re-flagging the fixture.

- [ ] **Step 6: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/compare_dumps.c ast_tester/CMakeLists.txt cmake/run_simplify_test.cmake cmake/run_wcsconverter_test.cmake
git commit -m "ast_tester: compare dumps to 4 ULP rather than byte-for-byte

A byte comparison forces a fixture whose last bits vary by platform to opt out
of string comparison altogether, which is why 273 manifest rows are flagged.
compare_dumps passes a line that is byte-equal and otherwise compares tokens,
treating two doubles within 4 ULP as equal, so those rows can be re-audited
and the comparison stays a real gate."
```

---

## Task 9: Re-audit the 165 `skip_string_compare` flags in `simplify/cases.txt`

Every flag that survives has to carry a reason. A flag left in place "just in case" is a comparison that silently stops happening.

**Files:**
- Modify: `ast_tester/fixtures/simplify/cases.txt`

**Interfaces:**
- Consumes: `compare_dumps` from Task 8.
- Produces: a manifest whose flags mean "this genuinely cannot be compared", which is the precondition for Part 2 reading it verbatim.

- [ ] **Step 1: Measure every flagged row**

```bash
cd reference/starlink-ast
cmake --build build --target simplify compare_dumps -j
grep -v '^#\|^$' ast_tester/fixtures/simplify/cases.txt \
 | awk -F'|' '$4 ~ /yes/ {for(i=1;i<=3;i++) gsub(/^ +| +$/,"",$i); print $1"|"$2"|"$3}' \
 | while IFS='|' read -r name inp ref; do
     ./build/ast_tester/simplify "ast_tester/fixtures/$inp" /tmp/o.$$ >/dev/null 2>&1 || { echo "RUNFAIL $name"; continue; }
     if ./build/ast_tester/compare_dumps "ast_tester/fixtures/$ref" /tmp/o.$$ >/dev/null 2>&1; then
        echo "RETIRE $name"
     else
        echo "KEEP   $name"
     fi
   done | sort | tee /tmp/simplify_audit.txt | uniq -c -w6
```

- [ ] **Step 2: Clear the flag on every `RETIRE` row**

```bash
cd reference/starlink-ast
grep '^RETIRE' /tmp/simplify_audit.txt | awk '{print $2}' \
 | while read -r n; do
     sed -i -E "s/^([[:space:]]*$n[[:space:]]*\|[^|]*\|[^|]*\|)[[:space:]]*yes[[:space:]]*$/\1/" \
        ast_tester/fixtures/simplify/cases.txt
   done
grep -v '^#\|^$' ast_tester/fixtures/simplify/cases.txt | awk -F'|' '{gsub(/ /,"",$4); print $4}' | sort | uniq -c
```

- [ ] **Step 3: Give every surviving flag a reason**

For each `KEEP` row, add a comment line immediately above it in `cases.txt` naming what varies — the pattern the Rust manifest already uses. Diagnose it rather than guessing:

```bash
cd reference/starlink-ast
./build/ast_tester/simplify ast_tester/fixtures/simplify/<name>.map /tmp/o
diff -u ast_tester/fixtures/simplify/<name>.simp /tmp/o | head -20
```

A difference beyond 4 ULP in a numeric token is a real divergence, not a platform artifact, and belongs in a bug report rather than behind a flag. Say so in the comment if that is what you find.

- [ ] **Step 4: Verify**

```bash
cd reference/starlink-ast
cmake -S . -B build && cmake --build build -j
ctest --test-dir build -R '^simplify_' --output-on-failure 2>&1 | tail -5
grep -c 'yes' ast_tester/fixtures/simplify/cases.txt
```

Expected: all pass, and the flag count is far below 165. Every remaining `yes` has a comment above it.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/fixtures/simplify/cases.txt
git commit -m "simplify cases: re-audit skip_string_compare against compare_dumps

Each of the 165 flagged rows was re-run and compared to 4 ULP.  The rows that
now compare cleanly have the flag cleared; each row that keeps it carries a
comment naming what varies, so a flag can no longer mean 'nobody looked'."
```

---

### Tasks 8 and 9 outcome (2026-09-03)

Landed together. **Task 9's premise was wrong in a way that matters: the 165
flags are not 165 tolerance opt-outs.**

Measuring every flagged row against `compare_dumps` gave 12 retirable and 155
that had to stay — the opposite of what the plan expected. The reason is
structural, not numeric: **151 of the 155 are `neg_*` rows**, and a negative
fixture's reference is its own *unsimplified* `.map`. `astSimplify` always stamps
`IsSimp` on whatever it returns, so the output differs from the reference by
exactly that one card, always, on every platform. No tolerance can fix that and
none should try.

Those rows are already gated, by the `_noop` test, which strips `IsSimp` before
comparing and so still asserts that nothing *else* changed. So the flag on a
`neg_*` row is correct and the manifest header was describing it wrongly. The
header now says what the value means, including that `yes` is not a tolerance
opt-out.

That left four non-negative rows, each needing its own answer:

| row | finding |
|---|---|
| `cap_winmap_03` | the reference carries `Sft2 = -1.0061396160665481e-16` where the library now produces exactly zero and omits the card. Stale, not tolerant — regenerated, flag cleared |
| `cap_timemap_02` | its `.simp` still had the pre-normalization KeyMap order, so entries were compared against the wrong keys and values differed by 1000×. Regenerated from its normalized `.map`, flag cleared. It was the only one of the eight fixtures normalized in Task 2b whose `.simp` needed following — the other seven were formatting-only changes that leave the object identical |
| `cap_interval_01` | uncertainty coordinates differ by 1.9e-12 relative — some four thousand ULP, deterministic here, reference from another platform. Flag kept, with the magnitude recorded above the row and a note that regenerating would re-anchor it to this host |
| `cap_pointlist_01` | the same at 1.2e-12. Flag kept, same reasoning |

Final distribution: 336 blank, 151 `yes`, 0 `astequal`.

**On `astequal` being unused here.** The value is supported by the simplify
driver and no row needs it, which would normally be a path that rots. It is kept
because Task 10 needs the same vocabulary for `wcsconv/cases.txt`, where rows do
need it, and divergent vocabularies between the two manifests would be worse.
The path is verified rather than assumed: `compare_dumps_selftest` covers the
widened tolerance, and the driver's pass-through was checked end to end with a
synthetic pair that 4 ULP rejects and `astequal` accepts.

`compare_dumps` links no AST library — it is text and arithmetic only — so a
`cmake -P` driver can call it without the library on the path. Its self-test is a
ctest, because a comparator nobody checks is how a gate quietly stops being one:
it covers exact equality, one ULP, five ULP, opposite signs, a real change,
non-numeric tokens, a partly numeric token, a dropped value, and `astequal` in
both directions.

---

## Task 10: Re-audit the 108 flags in `wcsconv/cases.txt`, and add `astequal`

The wcsconv column needs a third value. The Rust harness distinguishes "relax numeric tokens to AST's `astEQUAL` tolerance" — used for 8 cases whose last bits are cancellation-amplified, such as `OBSALT` derived from `OBSGEO-X/Y/Z` — from "skip the string comparison". One value cannot carry both meanings.

**Files:**
- Modify: `ast_tester/fixtures/wcsconv/cases.txt`, `ast_tester/CMakeLists.txt`, `cmake/run_wcsconverter_test.cmake`

**Interfaces:**
- Consumes: `compare_dumps` from Task 8.
- Produces: a three-valued sixth column (blank / `astequal` / `yes`) that Part 2 reads directly.

- [ ] **Step 1: Document the three values in the manifest header**

Edit the comment block at the top of `ast_tester/fixtures/wcsconv/cases.txt`:

```
# skip_string_compare: blank    = compare the output to the reference, numeric
#                                 tokens to 4 ULP, and run the astequal test
#                      astequal = relax numeric tokens to AST's astEQUAL
#                                 tolerance.  For a case whose last bits are
#                                 cancellation-amplified, e.g. OBSALT derived
#                                 from OBSGEO-X/Y/Z.
#                      yes      = run the astequal test only.  Every such row
#                                 carries a comment above it naming what varies.
```

- [ ] **Step 2: Teach the driver the third value**

`add_wcsconv_test` already reads six fields and registers the byte test only when the flag is not `yes`. Extend it so `astequal` registers the string test with a relaxed mode: pass `-DCOMPARE_MODE=${W_SKIP}` through to `cmake/run_wcsconverter_test.cmake`, and have that script invoke `compare_dumps` with an `--astequal` flag which raises `MAX_ULPS` to the tolerance AST's `astEQUAL` uses. Add the flag to `compare_dumps.c` and a self-test case for it beside the four already there.

- [ ] **Step 3: Measure every flagged row**

```bash
cd reference/starlink-ast
cmake --build build --target wcsconverter compare_dumps -j
grep -v '^#\|^$' ast_tester/fixtures/wcsconv/cases.txt \
 | awk -F'|' '$6 ~ /yes/ {for(i=1;i<=5;i++) gsub(/^ +| +$/,"",$i); print $1"|"$2"|"$3"|"$4"|"$5}' \
 | while IFS='|' read -r name inp ref enc attrs; do
     ./build/ast_tester/wcsconverter "ast_tester/fixtures/$inp" "$enc" /tmp/w.$$ >/dev/null 2>&1 || { echo "RUNFAIL $name"; continue; }
     if ./build/ast_tester/compare_dumps "ast_tester/fixtures/$ref" /tmp/w.$$ >/dev/null 2>&1; then
        echo "RETIRE $name"
     else
        echo "KEEP   $name"
     fi
   done | sort | tee /tmp/wcsconv_audit.txt | uniq -c -w6
```

Note that `wcsconverter` takes the attributes from the manifest; check how `cmake/run_wcsconverter_test.cmake` passes them and match it, or the outputs will not correspond to what ctest compares.

- [ ] **Step 4: Apply and verify**

Clear the flag on `RETIRE` rows, move a row to `astequal` where the diff is confined to a cancellation-amplified keyword, and leave `yes` only where neither applies — with a comment above it either way.

```bash
cd reference/starlink-ast
cmake -S . -B build && cmake --build build -j
ctest --test-dir build -R '^wcsconv_' --output-on-failure 2>&1 | tail -5
grep -v '^#\|^$' ast_tester/fixtures/wcsconv/cases.txt | awk -F'|' '{gsub(/ /,"",$6); print $6}' | sort | uniq -c
```

Expected: all pass; the `yes` count is far below 108; every `yes` has a comment.

- [ ] **Step 5: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/fixtures/wcsconv/cases.txt ast_tester/CMakeLists.txt cmake/run_wcsconverter_test.cmake ast_tester/compare_dumps.c
git commit -m "wcsconv cases: re-audit the skip flags and add an astequal mode

One value cannot mean both 'relax the last bits' and 'do not compare at all'.
The column now takes blank, astequal or yes, and each of the 108 flagged rows
was re-run against compare_dumps to decide which it needs."
```

---

## Task 11: Regenerate the three transform oracles

The committed `simplify_fixtures.oracle` has no section for 60 of its own 781 fixtures. Some postdate its last regeneration and one — `cap_ident_cmpmap` — could not be generated at all until `16ed5d37` stopped `astEqual` recursing on a protected CmpMap; but `tranmap_invert_in_list` predates it, so staleness is not the whole story. An unexplained absence is coverage lost silently, so this task accounts for every one. See spec §4 U5.

**Files:**
- Modify: `ast_tester/fixtures/oracle/simplify_fixtures.oracle`, `headers.oracle`, `framesets.oracle`
- Create: `ast_tester/fixtures/oracle/README.md`

**Interfaces:**
- Consumes: Tasks 1–7 (the finished fixture tree).
- Produces: the three oracles Part 2 mirrors and gates against.

- [ ] **Step 1: Record which fixtures the current oracles cover**

```bash
cd reference/starlink-ast/ast_tester/fixtures
grep '^\[' oracle/simplify_fixtures.oracle | sed 's/^\[//;s/  .*//' | sort -u > /tmp/cov_before.txt
(ls simplify/*.map simplify/*.simp | sort) > /tmp/have_before.txt
comm -23 /tmp/have_before.txt /tmp/cov_before.txt | wc -l    # expect 60
```

- [ ] **Step 2: Regenerate**

```bash
cd reference/starlink-ast
cmake --build build --target gen_transform_oracle -j
./build/ast_tester/gen_transform_oracle ast_tester/fixtures \
    /tmp/simplify.oracle /tmp/headers.oracle /tmp/framesets.oracle
grep -c '^\[' /tmp/simplify.oracle /tmp/headers.oracle /tmp/framesets.oracle
```

Writing to `/tmp` first is deliberate: it lets you diff before overwriting the committed baseline.

- [ ] **Step 3: Account for every fixture with no section**

```bash
cd reference/starlink-ast/ast_tester/fixtures
grep '^\[' /tmp/simplify.oracle | sed 's/^\[//;s/  .*//' | sort -u > /tmp/cov_after.txt
(ls simplify/*.map simplify/*.simp | sort) > /tmp/have_after.txt
comm -23 /tmp/have_after.txt /tmp/cov_after.txt
```

For each path printed, establish *why* the generator emitted nothing, by loading it and asking what it is:

```bash
cd reference/starlink-ast
./build/ast_tester/gen_simp ast_tester/fixtures/simplify/<name>.map /tmp/x 2>&1 | head -3
head -1 ast_tester/fixtures/simplify/<name>.map
```

Expect three legitimate categories: the object is not a Mapping (a bare Frame or a FrameSet, e.g. `cap_ident_frame`, `cap_frameset_nochange`); the object names a private transform function the generator cannot register (`IntraMap`); or the transform is undefined in both directions, so there is nothing to record. Anything that does not fall into one of those is a generator defect — stop and report it rather than recording it as expected.

Write the outcome into a new `ast_tester/fixtures/oracle/README.md`: one line per uncovered fixture with its category, plus a sentence saying that a fixture absent from the oracle for any other reason is a bug.

- [ ] **Step 4: Diff against the committed baseline, then install**

```bash
cd reference/starlink-ast
for f in simplify_fixtures headers framesets; do
  src=/tmp/$( [ $f = simplify_fixtures ] && echo simplify || echo $f ).oracle
  echo "=== $f"; diff <(grep -c '^\[' ast_tester/fixtures/oracle/$f.oracle) <(grep -c '^\[' $src)
done
cp /tmp/simplify.oracle  ast_tester/fixtures/oracle/simplify_fixtures.oracle
cp /tmp/headers.oracle   ast_tester/fixtures/oracle/headers.oracle
cp /tmp/framesets.oracle ast_tester/fixtures/oracle/framesets.oracle
ctest --test-dir build -R '^transform_oracle_' --output-on-failure 2>&1 | tail -5
```

Expected: all four `transform_oracle_*` tests pass, and the simplify oracle's section count has risen — it now covers the 25 fixtures from Task 6 and whatever the 60-fixture audit restored.

- [ ] **Step 5: Run the whole suite**

```bash
cd reference/starlink-ast
ctest --test-dir build --output-on-failure 2>&1 | tail -15
```

Expected: no failures. This is the gate for the whole plan — Part 2 must not start until it holds.

- [ ] **Step 6: Commit**

```bash
cd reference/starlink-ast
git add ast_tester/fixtures/oracle
git commit -m "oracle: regenerate against the finished fixture tree

Section headers now name every fixture added by this branch, and
oracle/README.md accounts for each fixture the generator emits no section
for -- not a Mapping, an unregisterable IntraMap, or no transform in either
direction.  A fixture missing for any other reason is a bug, which is the
point of writing the list down."
```

---

### Task 11 outcome (2026-09-03)

Landed. Coverage rose sharply: simplify 1411 → 1601 sections, framesets 20 → 351,
headers unchanged at 277, and simplify fixtures with no section 106 → 9.

**Two absences were removed rather than documented.** The generator refused any
fixture containing an IntraMap, on the stated grounds that such a dump "cannot be
loaded without registration" — so it now makes the same four registrations
`simplify.c` does, covering ten fixtures. The registration went into the shared
`transform_oracle_util.c` rather than the generator, because putting it in only
one produced sections the checker could not load. The remaining nine have neither
a forward nor an inverse transformation, so there is nothing to record; the
generator writes a section whenever either exists, which makes that reason exact.

**The SIP round-trip failures were a generator bug, not something broken.**
Discovering `wcsconv/framesets/` brought in 331 sections and 126 round-trip
mismatches across four SIP fixtures, one of them off by 136 pixels. The cause was
the sampling domain: a `.head` fixture is sampled over its NAXIS grid, while an
`.ast` dump got a hardcoded 1..1000 — and all four SIP headers are 256 pixels
square. A SIP inverse is iterative and need not converge that far outside the
image it was fitted over. The same mappings round-trip cleanly through their
headers, and comparing the header-read object against its own dump at the same
points gives *zero* differing rows, so serialization was never in question. The
generator now recovers a frameset's originating header from its name and takes its
grid. 126 mismatches → 2.

Those last two are `tnx-cheb`, which upstream already documents: its TNX Chebyshev
distortion builds a ChebyMap that cannot represent its own Jacobian, so sky→pixel
returns BAD for the whole fixture and the round-trip is already disabled for the
header. The dump needed the same entry, an override being keyed on the relpath.

### The FMA question, which this touched but did not settle

Regenerating changed four values in
`neg_unitnormmap_inv_fwd_diffcentre.map` from a real direction to `BAD`, and it is
worth being precise about which of those is the answer we want.

The fixture's inverse chain reconstructs a center as `B + |A-B| * (A-B)/|A-B|`.
Whether that lands on `A` exactly — residual zero, so C's guard fires and the
output is `BAD` — or about 1e-16 away, so the guard misses and the output is a
unit-vector direction made of rounding noise, depends on whether the compiler
contracts the multiply and add. The arm64 generator did; this x86-64 build does
not.

So the regenerated `BAD` is **the non-FMA answer**, and the direction the port
should be heading is the other one: macOS clang contracts by default, and modern
gcc is expected to. Two consequences worth carrying forward rather than
discovering later:

- Upstream already has `simplify/neg_unitnormmap_inv_fwd_diffcentre.map
  golden-inv off` for exactly this, with the divergence written out. That entry is
  load-bearing and must not be retired on the strength of this host agreeing with
  the new values.
- An earlier note in this plan said regenerating "retires the Rust port's
  `FMA_SUBSTITUTIONS_LICENSED` table". That is true only while the Rust port is
  built without contraction. Once it uses FMA — which Part 2's Task 7 already
  records as a separate audited pass — the substitution becomes needed again, in
  the other direction. The honest statement is that no value at that point is
  stable across contraction settings, which is why the golden inverse check is
  disabled rather than pinned.

---

## Self-Review

**Spec coverage.** §2.1 `wcsconv/framesets/` → Task 4; `serialisation/` → Task 3; `oracle/keymap_mapsz.txt` → Task 5. §2.3 the `IsSimp` defect → Tasks 1–2 (the 43 `.rt` deletions are Part 2, since the files live in the Rust repo). §3 manifest columns → Tasks 7 (`render_options` retired), 10 (`astequal` added). §4 U1 → Tasks 1–2; U2 → Tasks 3, 4, 5 (`gen_roundtrip_refs` deletion is Part 2, being our tool); U3 → Tasks 6, 7; U4 → Tasks 8, 9, 10; U5 → Task 11. §5 and §6 are entirely Part 2. §7's facts are used as expected values throughout. §8's three risks land in Tasks 2, 9/10 and 11 respectively.

**One deliberate omission.** Spec §8 raises "which host captures the references". This plan captures on the development host, which is correct for the fixtures added here because they have no prior baseline. It does **not** re-anchor the 42 pre-existing ULP-drifting simplify sections, because Task 11 regenerates the oracles wholesale and would silently do exactly that. Task 11 Step 4 diffs before installing so the change is visible; if that diff shows the 42 known-drifting sections moving, stop and use `tools/oracle_fixtures/merge_card_hunks.py` from the Rust repo to keep the existing values and take only the new cards. This is the one place in Part 1 where an empty diff is *not* the goal and judgement is required.

**Placeholder scan.** Three steps carry a deliberate `/* ... one block per ... */`: Task 3 Step 2, Task 6 Step 3, and Task 10 Step 2's `--astequal` flag. Each is a repetition of a shape shown in full immediately above it, driven by data the step tells you how to extract, and each is followed by a byte-identity or self-test step that fails if the repetition is wrong. Every other step contains its full content.

**Type consistency.** `write_fixture` / `write_negative_fixture` (Task 6) are the existing helpers in `gen_simplify_fixtures.c`, used with their current signatures. `write_object` (Task 3) is new and local to `gen_native_fixtures.c`. `within_ulps` / `tokens_match` / `lines_match` (Task 8) are local to `compare_dumps.c`; `MAX_ULPS` is the single tolerance constant, raised by `--astequal` in Task 10. `COMPARE_DUMPS` is the cmake variable name in both driver scripts.
