# Native Rust ERFA, PAL and wcslib with bulk, vectorizable interfaces

Ticket: DM-56839.
Status: design, approved for planning once reviewed.
Starting point: `docs/issues/vectorized-erfa-pal-wcslib.md` (the 2026-10-18 investigation) and `PLAN.md` §7's "wcslib Rust port (step 6a)" row.
This design supersedes the open decisions listed at the end of that note.

## Intent

SphMap and WcsMap are slow, in the port and in C, because every point goes through a per-value C call into ERFA, PAL or wcslib, and almost all of that time is the platform libm, one value at a time.
The port today runs those same C functions through FFI, so it sits at C parity on exactly the Mappings where the most time goes.
Starlink/ast pull request 51 showed about 8x for SphMap by moving to vector math; a branchless Rust `atan2` measured in the investigation note runs 4.6x faster than libm within 1 ULP.

This ticket replaces the vendored C in `crates/starlink-ast/vendor/{erfa,pal,wcslib}` with Rust, reshapes the interfaces to take whole axes (slices) where callers loop, and then vectorizes the math inside them.
It must show the gain with recorded before/after timings.

## Goals

1. Every ERFA, PAL and wcslib function the port calls is Rust, and `vendor/erfa`, `vendor/pal`, `vendor/wcslib` and `vendor/palwrap.c` (with `pal2ast.h`, `erfa2ast.h`) are deleted.
2. Functions called inside per-point loops have slice interfaces, and their callers are reorganized to call them once per axis set rather than once per point.
3. Inside those functions, libm calls are replaced by branchless code the compiler vectorizes, accepting at most 1 ULP difference from today's results.
4. The gain is measured against a baseline recorded before any Rust replaces the C, on the six timeable real-world FrameSets below, and written up per milestone.
5. The vendored libraries are brought up to date first, in C AST, and offered upstream as one pull request: wcslib's projection code picks up the upstream bug fixes since AST forked it, ERFA moves to the current release, and PAL functions that now have ERFA equivalents call them.

## Non-goals

- Byte-exact agreement with the C oracle after vectorization. The port already differs from C between x86 and arm and between FMA and non-FMA builds; the C comparisons move to a stated ULP bound for the affected Mappings.
- Vectorizing single calls that are not in a loop (FitsChan header analysis, Frame attribute code, one-off SkyFrame set-up). They are ported to Rust, keep a scalar call, and are not reorganized.
- A public API. The ported libraries are private modules of `starlink-ast`; nothing in them is `pub` beyond `pub(crate)`, and no item is re-exported or reachable from outside the crate.
- Vectorizing XPH ahead of the other projections. (Amended 2026-10-28: the first version said AST handles XPH with XphMap, not through wcslib. C's WcsMap has routed XPH through the fork's `astXPHfwd` / `astXPHrev` since 2014, so XPH is ported with the rest of `proj.c`; XphMap is a separate class.)
- Porting parts of wcslib 8.10 that AST's fork does not have (`cel`, `lin`, `dis`, `spc`, `tab`, `wcs`). Only the projection code and trig helpers AST's fork contains are in scope.
- Platform intrinsics or `unsafe` SIMD. Vectorization comes from the compiler auto-vectorizing branchless loops, as in the investigation note.

## Anti-goals

These would make the work a failure even if it ran faster.

- A result that depends on how many points are transformed together. A 1-point probe (FrameSet and Region integrity use them) must give the same bits as the same point inside a bulk call.
- A separate scalar implementation that drifts from the vector one.
- Fixture churn accepted wholesale. Every changed fixture is inspected and shown to be ULP drift or a deliberate wcslib fix, never re-baselined in bulk.
- Lost provenance. Code whose ancestry cannot be traced to a named C function and version.
- A slowdown at small N relative to C. The N=1 and N=100 timings must stay faster than C's.
  (Amended 2026-10-27: the first version asked for no slowdown against the previous port. A branchless kernel always pays its full latency, so SphMap forward on the bench points went from 23.7 to 37.7 ns at N=1 and from 12.5 to 16.3 ns per point at N=10, while staying about 15 times faster than C at N=1 and becoming 3.9 times faster than before on large random point sets. The user accepted this, to be revisited: a kernel with one division instead of two per arctangent, or a shorter polynomial evaluation, would cut the latency; switching to libm below a size threshold would not be acceptable, since a point's bits would then depend on how many points are transformed together.)

## Constraints

- The repository's rules apply: `CLAUDE.md`'s porting rules, stop signals, the hot-path constraint on `SkyFrame` setters, `#![warn(missing_docs)]`, clippy and fmt gates, and the source header on new files.
- `CLAUDE.md`'s "What NOT to do" row against changing `vendor/wcslib` C before step 6a is lifted for the C fix pass in step 0; this design records that, and the row is updated in the same commit range.
- No new vendored C (the user's standing rule); the work only removes C.
- The `reference/starlink-ast` submodule stays the C oracle; the fixed wcslib goes there first so oracle and port agree.
- The pinned C library is built per `docs/issues/c-oracle-probe-recipe.md` and its stale-libast trap.

## Decisions

### Interface shape: one body, two entry points

Each ported function has one per-value body, `#[inline(always)]`, returning its results as a value, a tuple or a small named struct rather than through output pointers.
Functions that callers loop over also get a slice entry point that loops over that body, so the compiler can vectorize the loop; for example `c2s(v: [f64; 3]) -> (f64, f64)` and `c2s_slice(x: &[f64], y: &[f64], z: &[f64], lon: &mut [f64], lat: &mut [f64])`.
One-off callers call the body directly.
There is no separate scalar implementation and no one-element-slice convention.

Inputs that the branchless body does not cover (zeros, infinities, NaN, `BAD_VALUE`, poles, out-of-domain values) are handled by one shared special-case routine per function.
The per-value entry branches into it; the slice entry runs the branchless body over every element and then a fix-up pass that calls the same routine for the flagged elements.
Both paths therefore give identical bits for the same input.

#### Interim: vectorized kernels in slice entries only (2026-10-27)

Making the per-value entry use the vectorized kernel too moved every one-off caller (SkyFrame, SlaMap, FitsChan, Regions) by up to 1 ULP and failed 19 exact C-oracle tests.
Until the vendored C is gone, the user has decided:

- The per-value entry keeps the C operations bit for bit; only the slice entry runs the kernel, within 1 ULP of the per-value entry.
- A slice gives each element the same bits whatever its length, and a Mapping that uses a slice sends every point through it, a single point included, so the "same bits whatever N" rule still holds per Mapping.
- One-off callers are therefore not changed by vectorization.

This is reassessed when the vendored-code removal is complete; one option is to adopt the same branchless kernels in C AST, after which the per-value entries could use them without moving away from the oracle.

#### ULP bound: per elementary function (2026-10-27)

The 1 ULP bound applies to each elementary function a kernel replaces (an arctangent, a sine, a cosine), not to every output of a ported ERFA function.
A function that combines several, such as `eraS2c`'s products of a sine and a cosine, can be correspondingly further from C: `eraS2c`'s x and y components were up to 3 ULP from C's over 8 million angle pairs, its z component 1 ULP.
The user accepted this for SphMap's inverse, which went from 19.5 to 11.7 ns per point on large random point sets.
The final review found that `vmath::atan2`, like fdlibm's, carries the rounding of `y / x` into its result and so is 2 ULP from libm for a few pairs in a hundred million; its bound is stated as 2 ULP. A residual correction of the division would restore 1 ULP at some cost in latency, and is left for the user to decide. (Decided 2026-10-28: `vmath::atan2` carries the quotient's rounding error as a correction and rounds its final additions once, which restores 1 ULP and brings it to within the rounding of libm for all but 1.6% of longitudes and 4.2% of latitudes, against 12% and 18% before, for 19% on SphMap's forward transform.)

The bound is against the exact value, and libm is not always exact: macOS's x86-64 libm is up to about 1.5 ULP from the exact sine or cosine at large arguments, so a kernel within 1 ULP of the exact value can be 2 ULP from that libm (found 2026-10-29).
Every drift bound derived for a projection's slice therefore takes 2 ULP per kernel sine or cosine, so that it holds on every platform rather than only where it was measured.

### Errors and status codes

A function that can genuinely fail returns `AstResult<T>`; one that cannot returns `T`.
C status codes that are warnings are not errors: the function returns the value with a typed warning, and the caller does whatever AST's C does with that status today.
The exception is `eraDat`'s +1 ("dubious year", raised when the date is more than five years after the table's last revision): with no further leap seconds that warning no longer means anything, so the Rust `dat` drops it; its error for dates before 1960 stays.
wcslib's per-point status, which WcsMap turns into bad output values, becomes `BAD_VALUE` written directly by the slice entry; there is no status array.

### wcslib shape

The fork's per-projection C functions, selected through a `prjprm` struct, become an enum of projections dispatched with `match`, with the per-projection parameters (`p`, `w`, `r0`, the set-up results) in the enum or a struct it holds.
wcslib 8.10's strided `nx` × `ny` interface is not copied; slices are the interface.
TPN, which exists only in AST's fork, is ported from the fork.

### Lineage

Every ported function carries a `//` comment naming its C ancestor: library, file, function, and the version or commit it came from (AST's fork at the pinned submodule commit for PAL, ERFA and wcslib, plus the wcslib 8.10 revision for any adopted fix).
ERFA is BSD-3-Clause: its copyright and licence notice is kept in each Rust file that contains ERFA-derived code.
PAL and wcslib are LGPL-3, as this repository is.
Rendered docs follow the repository rule: no C file or line citations in `///` docs; provenance stays in `//` comments.

### Leap seconds

As of October 2026 no further leap seconds will be added (future adjustments are leap hours), so ERFA's `dat` table is final.
The Rust `dat` uses a fixed `const` table with a comment saying why it does not need to grow.
DUT1 remains a user-supplied attribute, as today.

### Module layout (builder's latitude within this)

Three private modules in `starlink-ast`, one per library (`erfa`, `pal`, `wcslib`), replacing today's FFI modules `pal.rs` and `wcslib.rs`.
Each depends only on its own types, `f64` math and `BAD_VALUE`, never on Mapping, Frame or `Points` types, so any of them could later move into its own crate without untangling.
Shared vector-math kernels (branchless `atan2`, `sin`/`cos`, and so on) live in one internal module that the three use.
Assumption (this design's call): PAL calls ERFA in Rust exactly as it does in C, so `pal` depends on `erfa`.

## Sequence

SphMap and WcsMap come first, function by function; each function is ported bit-identical, then vectorized, and timed after each step.
Steps 2 and 3 are therefore interleaved per function, not global phases.
The rest of PAL and ERFA follows in the same pattern, ordered by how hot the caller is.
C is deleted per library once its last caller is Rust.

Because earlier functions will already be vectorized when later ones are ported, the bit-identical gate is always relative to the commit's parent, never to the step 1 baseline (see step 2).

### Step 0: update the vendored libraries in C AST

Every change of behaviour lands in C AST first, so the C oracle and the port agree before any Rust replaces the C.
All three libraries are updated on one `u/timj/...` branch of `reference/starlink-ast`, which becomes one upstream pull request.
Each change is its own commit, with a `History:` entry (TIMJ) in the C file where the file carries a History block, and a test in `ast_tester` when it changes behaviour, in the style of `7b67e1b4`'s commit message (what changed, the upstream version it came from, why it matters).

#### wcslib

AST's fork is wcslib 2.9 (about 2002), with AST changes: `ast`-prefixed symbols, dynamic `p`/`p2` arrays in `prjprm`, and TPN.
Commit `7b67e1b4` (2026-04-01) already applied fixes found by comparing against upstream wcslib as of April 2026 (ZPN `w[0]`, AIT's `u < 0.5` bound, quadrant-based `cosd`/`sind`); `vendor/wcslib` already contains them.

- Compare the fork's `wcslib/proj.c` and `wcstrig.c` against `../wcsLib-8.10/C/prj.c` and its trig code, using the RCS histories in `../wcsLib-8.10/C/RCS/` to date each change; the emphasis is on changes since April 2026, but anything `7b67e1b4` missed counts too.
- Apply each relevant fix. A refactor, a projection AST does not route through wcslib (XPH), or an API change is not a fix and is not applied.

#### ERFA

AST's `erfa/` predates ERFA 2.0.1; the current ERFA is cloned at `../erfa` (v2.0.1-12-g1a8044c).
Its `dat` table year moved from 2016 to 2023, and `jd2cal`, `epj` and `epb`, which AST calls through `palDjcl`, `palDat`, `palEpj` and `palEpb`, changed code.
Many other files differ only in comments, copyright years and `return` statements; `c2s` and `s2c`, SphMap's functions, are among those.

- Replace AST's `erfa/` with the current ERFA's versions of the functions AST uses, plus any new functions the PAL changes below call.
  Only the functions AST calls are carried; AST's symbol-renaming header (`erfa2ast.h`) is extended for new ones.

#### PAL to ERFA

Current ERFA has functions that duplicate PAL's own implementations.
Where one exists, the PAL function becomes a call to it, so that provenance is consistent and the code comes from the maintained library:

- `palFk45z` → `eraFk45z`, `palFk54z` → `eraFk54z` (FK4 conversions);
- `palDe2h` → `eraHd2ae`, `palDh2e` → `eraAe2hd` (AzEl);
- `palDrange` → `eraAnpm`, if their results agree; otherwise it stays.

The builder checks the remaining PAL functions AST calls for further ERFA equivalents and applies the same rule.
Where PAL and ERFA results differ by more than rounding, the commit says by how much and why, and the test shows it.

#### Landing it in this repository

1. The user pushes the branch and opens the upstream pull request to Starlink/ast; nothing is pushed by the implementer.
   The pin-move commit below lands only after that push, since CI's `fixture-mirror` job and fresh clones need the commit to exist upstream.
2. Move this repository's submodule pin to that branch's commit (precedent: `be69341a`) without waiting for the upstream merge.
   Re-copy `vendor/wcslib`, `vendor/erfa`, `vendor/pal` and `erfa2ast.h` / `pal2ast.h` from it, record the new origin in `vendor/README.md`, run `tools/oracle_fixtures/check_corpus_fresh.sh` and `check_c_headers_fresh.sh`, and regenerate affected corpora in the same commit range.
3. Re-pin to upstream `master` once the pull request merges.

SphMap's ERFA functions (`c2s`, `s2c`) do not change in substance, so SphMap's port does not wait for step 0; its bit-identical gate is against whichever C is vendored at the time.

### Step 1: baseline

Recorded on the C library as fixed in step 0, before any vendored function is replaced by Rust.
SphMap may start in parallel with step 0; if it does, a SphMap-only baseline is recorded first, and the full baseline once step 0 lands.

### Step 2: bit-identical Rust port, per function

C's operations in C's order, calling `f64`'s libm functions.
Gated two ways:

- A per-function differential test, in tier 1 (so CI runs it), calling the Rust function and the still-linked vendored C function over the same inputs and requiring identical bits.
  This covers functions the transform corpus never reaches (the long tail of PAL and ERFA used by SkyFrame, SpecFrame and TimeFrame).
  When a library's C is deleted, the C half of these tests goes with it.
- `ast-bench bits --base <parent commit>` showing no changed output bits across the corpus, plus the existing test suite.

The test inputs per function are the realistic points (the corpus and Roman sky points for celestial functions), random values spanning the function's domain (for two-argument angle functions, pairs over 2^±40 as in the investigation note), and the special values: zeros of both signs, infinities, NaN, `BAD_VALUE`, poles and domain edges.
This is this design's call.

### Step 3: vectorize, per function

Replace libm calls in the slice paths with branchless kernels.
Gated by:

- A per-function test that the vectorized result is within 1 ULP of the step 2 body over the inputs above, and that the per-value and slice entries agree bit for bit for every input.
- A ULP comparison of the corpus against the parent commit, reporting the largest ULP difference per case and output axis.
  `ast-bench bits` only hashes, so it cannot tell drift from a regression; a ULP mode (a new `ast-bench` subcommand or flag) is a deliverable of this ticket and is the tool used to inspect each changed fixture.
  The bound at a chain's output is per case, chosen after inspection: one ULP of drift in one stage can grow through later stages near cancellation (outputs near zero, poles, large projection radii), and that growth is expected, not a defect, when the per-function test passes.
- The timing comparison.

Because the kernels are pure Rust and the port never contracts into FMA, their results are the same on every platform, unlike today's libm-dependent results.
The keep-or-reject decision for each kernel is made once, from the timings, and applied on every platform.
A kernel that cannot be made both faster than libm and within 1 ULP stays scalar on libm; the user has said this is acceptable for `sin`/`cos` in particular.

### WcsMap stage (2026-10-28)

The second stage, on `tickets/DM-56839-wcsmap`, stacked on the SphMap branch, as one pull request.
WcsMap is the only Mapping that transforms through wcslib; FitsChan uses only the `WcsProjection` enum, which stays.
At the end of the stage `vendor/wcslib` (`proj.c`, `tpn.c`, `wcstrig.c`, `ast_memory_stub.c` and headers) is deleted, with its `build.rs` compilation, and `wcslib.rs`'s FFI layer is replaced by the native module.

Two phases, in this order:

1. **Bit-identical port of every projection, then vectorize TAN, TPN, SIN and ZPN.**
   All 28 projections in the fork's `proj.c` (XPH and HPX included), plus TPN from `tpn.c`, are ported bit-identical (step 2), each projection's forward and reverse with its set-up, together with the degree trigonometry from `wcstrig.c` they call.
   Then TAN, TPN, SIN and ZPN are vectorized (step 3): the six FrameSets in the `native` set are five TAN and one TPN, and the class cases are `wcsmap_sin`, `wcsmap_tan` and `wcsmap_zpn`.
2. **Vectorize the remaining projections.**
   The fixtures already cover them (`test_wcsmap_edges.rs`, the wcsconv corpus, the C-oracle tests), so the accuracy gates exist.
   Since they have no bench cases, this phase adds one `ast-bench` class case per projection family (zenithal, cylindrical, pseudo-cylindrical, conic, polyconic, quad-cube, HEALPix) so the keep-or-reject rule has a timing to decide from; a kernel that loses to libm on its case stays scalar.

**Acceptance corpus.** `crates/starlink-ast/tests/test_wcsmap_edges.rs` is the stage's acceptance corpus: the cases from `ast_tester/testwcsmap.c` at face centres, poles, longitude edges, non-default projection parameters and points just past a limit, with upstream wcslib 8.10's values.
Every case passes at the end of each phase.
After phase 1's bit-identical port it passes unchanged.
ULP drift from vectorization is expected (see Non-goals); a case whose tolerance is zero or too tight for the measured drift gets a named per-case bound set from the drift measured, in the pattern of SphMap's `SPHMAP_DRIFT_LICENSED`, counted in `PLAN.md` §6.
A case that changes between bad and good, or by more than the drift the per-function test allows through the projection's formula, is a defect, not drift.

**Iterative projections.** Some projections solve for their result iteratively: TPN's forward runs Newton's method from starting values computed with `sind` and `cosd`, stopping when each step is within 1e-5 of the value (relative) or after 50 steps, and AIR's and PCO's reverses iterate similarly.
A 1 ULP change in a starting value can add or remove one step, moving the result by up to the solver's own stopping error, far more than the ULP drift of a closed-form projection: for TPN about 1e-10 of a coordinate plus the rounding error of its polynomials, with a point whose coordinate is so near zero that rounding decides the convergence test redone exactly as the per-value entry does it.
The user decided (2026-10-28) that this is drift, not a defect: the convergence tolerance states the accuracy the solver promises, so for a case whose chain contains an iterative projection the per-case bound is derived from that solver's tolerance, with the derivation in a comment beside the bound, rather than measured in ULP.
A point that changes between bad and good (for example by reaching the iteration cap on one side only) is still a defect and is investigated.
These bounds, like the closed-form ones, are set by the builder from measurement and listed in the pull request; this design's call.

**Exact angles.** `wcstrig.c`'s `cosd`, `sind`, `tand` and their inverses return exact results at multiples of 90° (and the inverses at ±1 and 0), and the pole and face-centre cases depend on that.
The vectorized degree trigonometry keeps those results: those arguments go through the function's special-case routine, so the per-value and slice entries agree and the exact values survive.

**Set-up.** The C fork sets a projection up lazily on its first call (`prjprm.flag`), which is why `WcsMap` holds its `PrjParams` in a `RefCell`.
How the native port handles set-up (eagerly at construction and on each projection-parameter change, or otherwise) is the builder's call, as long as a set-up error is still reported where C reports it.

**x86-64.** Vectorized slice loops follow SphMap's runtime AVX2 and FMA dispatch where a kernel uses FMA (see Platforms).

## Measurement

The success measure is the real-world FrameSets in `crates/starlink-ast/tests/fixtures/wcsconv/inputs/`:
`a20070718_00010_02_cube.ast`, `degen1.ast`, `dss.ast`, `lsst.ast`, `lsst2.ast`, `timj.ast`.
The seventh file there, `specflux.ast`, is a bare CmpFrame with no Mapping, so there is nothing to time.
They form a new `ast-bench` set (`crates/ast-bench/sets/native.txt` or similar), using the `framesets` oracle that already holds them.
The set also carries the plain single-class cases from the `bench` set whose transforms reach the vendored C (`sphmap`, `wcsmap_sin`, `wcsmap_tan`, `wcsmap_zpn`, `slamap_eqgal`, `specmap_wn_to_fr`, `timemap_taiutc`), so a gain or loss can be attributed to a class; the six FrameSets are the headline.

A `docs/perf/` record (`ast-bench snapshot`, both engines, forward and inverse, at the set's standard point counts) is written at:

1. the baseline (step 1);
2. each Mapping class's functions being ported bit-identical (expected: little change, except where removing FFI and reorganizing loops helps);
3. the same class's functions being vectorized;
4. the end.

Each record gets a row in `docs/perf/README.md`, and the end of the ticket adds a report in `docs/reports/` that shows each of the six FrameSets and each class case at every milestone, as Rust before, Rust after, and C.
`ast-bench roman` is run at the same milestones as supporting evidence, since it is pull request 51's benchmark.

### Platforms

The primary records are macOS aarch64 (the user's M1).
x86-64 numbers come from the user's shared batch machines: at each milestone the implementer commits to the branch and hands the user the exact commands to run there (build, `snapshot`, `compare --base <ref>`), and the user returns the output, which becomes a `docs/perf/` record of its own.
Because those machines are shared, x86-64 comparisons use `compare --base`, which alternates the two builds in one session, rather than comparing records taken at different times.
No `target-cpu` is set.
(Amended 2026-10-28: the first version also ruled out runtime CPU dispatch. The SphMap kernels ran at 0.79 of C's speed on the default SSE2 target, so their slice loops gained a copy built with AVX2 and FMA, chosen at run time when the CPU has them, and giving the same bits as the SSE2 build; `pal.rs`'s `dcc2s_slice` is the pattern. Later stages follow it wherever a kernel uses FMA for its residuals.)
There is no numeric pass threshold: success is the recorded gain on the six FrameSets and the class cases, with no slowdown at small N.

### ULP verification

The C-oracle tests that compare transform outputs move to a per-case bound for cases whose chain contains a vectorized function, set from the ULP comparison in step 3, named per case in source, following the repository's rule that relaxations are per case and counted in `PLAN.md` §6.
Printed outputs (FitsChan cards at 15 to 16 significant figures, Native dumps of computed values) and simplifier decisions that test a value against a tolerance can change by more than their own last digit's worth under 1 ULP input drift; each such fixture change is inspected and recorded, not re-baselined in bulk.

### Call-count parity

`call_counts.rs` keys counts by the C symbol names (`astPal…`, `astEra…`), and `ast-oracle-tests` compares them with an instrumented C build.
Once a function is Rust the hook stays at the same call site with the same name, counting calls to the per-value body or, for a slice entry, adding the slice length, so counts stay comparable with C's per-point calls.

## Left to the builder

- The per-function order within SphMap and WcsMap, and which PAL/ERFA functions follow and in what order.
- The names and exact signatures of slice entry points, warning types and the vector-math module.
- How each branchless kernel is constructed, including making `sin`/`cos` vectorize (the note's suggestion is to pick the quadrant with floating-point arithmetic).
- Whether a kernel that does not beat libm on the measured shapes is kept scalar; per the repository's bulk-transform guidance, a vector path that loses at realistic N is not committed.
- How WcsMap's and SphMap's `transform_into` are reorganized to call the slice entries, within the per-axis `Points` view model.
- Splitting the ticket into pull requests (the repository prefers small, focused ones).

## Calls made in this design that the user did not state

- A new `ast-bench` set, including single-class cases alongside the six FrameSets.
- Records at each milestone and a final `docs/reports/` write-up.
- Call counts keep C's symbol names and count per point.
- ERFA's licence notice is carried in each Rust file with ERFA-derived code.
- `pal` depends on `erfa` in Rust as in C, and a shared internal vector-math module.
- `dat` as a fixed `const` table.
- Bit-identity gated per function against the still-linked vendored C, plus `ast-bench bits` against the parent commit.
- The test input domains per function.
- A ULP-comparison mode in `ast-bench` as a deliverable, with chain-output bounds chosen per case after inspection.
- Kernel keep-or-reject decided once and applied on every platform.
- The upstream branch is pushed (by the user) before the pin-move commit lands.
- No numeric pass threshold; success is the recorded gain with no small-N slowdown.
- Apple's Accelerate (vForce) is timed in `ast-bench` as a reference for what is achievable, not used by the library: it is array-only, macOS-only and needs `unsafe` FFI, so it would make results depend on the platform and on N.
- `specflux.ast` is a bare CmpFrame with no Mapping, so `ast-bench` cannot time it; it stays out of the set and the headline is the other six.
- `palDrange` → `eraAnpm` only if their results agree.
