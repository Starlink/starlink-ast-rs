# starlink-astrs: Progress Review, August to October 2026

**Reviewed:** 2026-10-19 · **Tree:** `tickets/DM-54793` @ `69c40996`
**C reference:** pinned at `d9e43a6b` (2026-09-30), the tip of the unmerged `u/timj/norm-unit` branch, 7 commits behind C master
**Previous review:** [`2026-08-18-port-progress-report.md`](2026-08-18-port-progress-report.md), at `bc620a6`

This review updates the August one and keeps its structure where the structure still fits.
Every figure was re-measured on the current trees.
Production line counts come from `scc` with inline `#[cfg(test)]` modules stripped, as the corrected August table did; history counts come from git and from `code-metrics repo-history` (tokei) in `~/work/lsst/code-metrics-data`.
Line counts are **code lines** (blanks and comments excluded) unless stated otherwise.

---

## Contents

1. [Where the port stands](#1-where-the-port-stands)
2. [Why tokei shows more Rust than C](#2-why-tokei-shows-more-rust-than-c)
3. [Timeline since August](#3-timeline-since-august)
4. [The oracle relationship](#4-the-oracle-relationship)
5. [Performance](#5-performance)
6. [Status against the oracle](#6-status-against-the-oracle)
7. [What is absent](#7-what-is-absent)
8. [The August risks, revisited](#8-the-august-risks-revisited)
9. [New risks](#9-new-risks)
10. [The pattern since August](#10-the-pattern-since-august)

---

## 1. Where the port stands

In August the port covered one pipeline in depth: read a FITS header, build a `FrameSet`, transform, simplify, write the header back.
It now covers the core library: every `Mapping`, `Frame` and `Region` class, `FitsChan` including `-TAB`, `Channel`, `KeyMap`, `Table`, `FitsTable`, `Moc` and `MocChan`, the `Stc` family, and `astResample` / `astRebin` / `astRebinSeq` / `astTranGrid` / `astQuadApprox`.
Each subsystem has had a function-by-function C-parity audit, opened and closed, and a final audit of the core found seven gaps, all fixed.

The thing the design exists for has still not started.
`ast-capi/src/lib.rs` is a 16-line doc comment and exports no symbols; no C program has linked this library.

| Measure | August (`bc620a6`) | Now (`69c40996`) |
|---|---:|---:|
| Commits on the branch | 1,087 | 1,736 (649 since August, 29 of them `fixup!`) |
| Active days since the previous review | — | 31 |
| `ast-core` production code | 67,277 | **96,341** (111,347 with inline tests) |
| C `src/` code, `.c` files | 184,817 | 181,769 (plus 12,732 in headers) |
| C code with a Rust counterpart | 116,845 (63%) | **128,174 (71%)** |
| Port vs C, subsystems with a counterpart | 0.57× | **0.74×** |
| Tests passing | 3,280 | **4,052**, 0 failing, 4 ignored |
| wcsconv round-trip cases | 194, 0 excluded | 194, 0 excluded, comparator now strict |
| Simplify fixtures byte-compared | 427 | **515** |
| Transform-oracle sections gated | 1,606 | **2,261** |
| C quirks catalogued | 36 | **143** (101 resolved upstream) |
| C-ABI symbols exported | 0 | 0 |

Per-subsystem production code, against the C files each replaces:

| Subsystem | Rust | C | Ratio | August |
|---|---:|---:|---:|---:|
| `serial/` + `channel/` | 4,175 | 2,270 | 1.84× | 0.69× |
| `units/` | 2,648 | 2,532 | 1.05× | 0.94× |
| `fitschan/` | 18,053 | 18,234 | 0.99× | 0.73× |
| `mappings/` | 32,427 | 44,053 | 0.74× | 0.57× |
| `coords/` | 747 | 1,079 | 0.69× | — |
| `table/` + `fitstable/` | 1,733 | 2,589 | 0.67× | — |
| `frames/` | 17,744 | 27,440 | 0.65× | 0.49× |
| `regions/` | 15,134 | 24,893 | 0.61× | 0.46× |
| `keymap/` | 2,148 | 5,084 | 0.42× | 0.41× |
| **Total** | **94,809** | **128,174** | **0.74×** | 0.57× |

Every ratio rose, and the rise is the August caveat coming true.
August's C column counted whole files whose algorithms the port had not yet written (`astResample` and `astRebin` inside `mapping.c`, `-TAB` inside `fitschan.c`, the attribute surfaces of the Frame classes), so its ratios were lower bounds on density, not measures of it.
Those algorithms are now ported, and the ratios are the true ones: Rust needs about three quarters of C's code lines for the same function.

`serial/` + `channel/` is the one subsystem well above 1.0×, and the comparison is lopsided by construction.
The port centralizes in `serial/` the per-class dump and load that C spreads across every class file, so C's side of the comparison is only `channel.c` and `mocchan.c`, while the ceremony it replaces sits in the other columns.
`fitschan/` at 0.99× now includes the card cursor, `Clean` / `PurgeWCS`, warnings, source and sink handling, and `-TAB` in both directions.

---

## 2. Why tokei shows more Rust than C

`tokei` reports about 200k code lines and 280k comment lines for C's `src/`, and about 225k code lines and 40k comments for `crates/ast-core`.
The two numbers count different things, for four reasons.

**1 → Tests live inside `ast-core`; C's live in `ast_tester/`.**
Of `ast-core`'s 225k, `src/` is 111k and `tests/` is 113.5k.
C's `ast_tester/` is 29k code lines and is not in `src/`.

**2 → Half of `ast-core/tests/` is generated data.**
The 125 `*_oracle/cases.rs` directories hold 49.6k lines of C output transcribed as Rust literals, the transform oracle's recorded values among them.
They are fixtures in Rust syntax, not test logic.

**3 → `src/` itself carries about 15k lines of inline unit tests.**
Stripped, `src/` is 96k code lines of production code against C's 182k in `.c` files (`scc`; tokei's 200k also counts headers).

**4 → C's comments are documentation, and Rust's mostly are not counted as such.**
About 265k of C's comment lines are Starlink prologues and SUN/211 reference blocks.
Rust's equivalent documentation is far shorter, and AST's reference manual is not being re-written into rustdoc.

Like for like, the C that has a Rust counterpart is 128k code lines and its Rust replacement is 95k (0.74×).
The product classes not yet ported (Plot, YamlChan, XmlChan, StcsChan and the rest, §7) are 39k more lines of C.
The August review's finding still holds: Rust uses about 1.8 lines per statement where C uses 1, and wins because it needs far fewer statements (no `int *status` threading, no declaration preambles, fewer runtime checks).

The whole-repository history in `code-metrics-data/repos/starlink-astrs.csv` shows the same thing over time.
Rust code went from 115,955 lines at `bc620a6` to 229,873 now, roughly doubling in October.
Most of October's growth is test code and generated oracle data from the C test migration and the per-subsystem audits, not production code: production `src/` grew by about 29k lines over the whole period.
Over the same period the C repository's history (`starlink-ast.csv`, whole repository, first-parent on master) went from 228,801 C code lines at the August pin `50c2b21b` to 234,733 at `011d1ee6` (2026-10-05), most of the growth in tests rather than `src/`.

---

## 3. Timeline since August

| Dates | Commits | Work |
|---|---:|---|
| Aug 18–20 | — | `docs/porting-hazards.md` (H1–H14) written. The C `u/timj/misc-bugs` branch merges upstream as PR #68 and the pin moves the same day; the five mirrored C bugs are removed and the corpora regenerated. The executable verification ledger (`test_verification_ledger.rs`) lands. |
| Aug 19 – Sep 4 | ~246 | **Simplifier parity.** A MapMerge/Simplify audit; the `simplified()` seam collapsed into per-class `map_merge`; bypassing the `IsSimple` gate made a compile error; the depth cap removed; all 31 gaps in PLAN §10 closed. Permanent simplify tracing on both sides, so C and the port can be diffed line by line. The fixture tree is shared with upstream and mirrored in CI. Final statement in [`2026-09-03-simplify-parity-final.md`](2026-09-03-simplify-parity-final.md). |
| Sep 5–23 | 1 | Gap. Upstream releases AST 9.5.0 (PR #92, Sep 22). |
| Sep 24 – Oct 6 | ~74 | 9.5.0 sync. **Every Frame attribute** on Frame, SkyFrame, TimeFrame, SpecFrame, DSBSpecFrame and FluxFrame, C-faithful; great-circle `Resolve` / `Intersect`; C's `printf` and `strtod` ported for formatting. Frame-family C-parity audit opened. |
| Oct 6–13 | ~151 | **Audits closed, classes landed.** Mapping audit opened and closed; XphMap, IntraMap with a registry; Resample/Rebin/RebinSeq, TranGrid, QuadApprox. Region audit (59 findings), per-class overlap and mask seams, the geodesic Polygon mesh, Moc, Stc and StcResourceProfile. FitsChan audit (66 findings), `-TAB` read and write. Channels became objects that read one Object at a time and write `Full = 1`. Table, FitsTable, MocChan. |
| Oct 14 | ~33 | **C test migration.** The remaining parts of `testregions.c`, `testmapping.c`, `testfitschanerrors.c`, `testswitchmap.c`, `testrate.c`, `testchannel.c` and `testpolymap.c`. |
| Oct 15–19 | ~121 | **Performance.** `ast-bench`; call-count parity against C; the per-axis `PointSet` and whole-axis kernels; real-world FrameSets added to the benchmark set; SplineMap, LutMap and PolyMap kernels; the Roman benchmark from Starlink/ast PR 51. Then a final audit of Frame, Mapping, Region, FitsChan and Channel checking that the tests run what they claim; its seven findings are fixed. |

---

## 4. The oracle relationship

August described a port that had turned from consumer of C into C's most productive defect source, with a loop from report to upstream fix of about a day.
That held.

`docs/issues/c-library-quirks.md` now has 143 entries, up from 36, sorted into three sections:

- **Resolved upstream: 101.**
  91 of them name the C commit that fixes them: 29 are merged on C master, and 62 are on `u/timj/norm-unit`, which is expected to be accepted.
- **Open questions for upstream: 34.**
- **Addressed here, not going upstream: 8.**

### C fixes since August

| | Commits | Merged |
|---|---:|---|
| C master since the August pin (`50c2b21b..origin/master`) | 292, in 20 pull requests | Yes, including `misc-bugs` (PR #68, 19 defects) and the AST 9.5.0 release (PR #92) |
| `u/timj/norm-unit` (`origin/master..u/timj/norm-unit`) | 76: 63 source fixes and 10 new tests or fixtures | Not yet; the pin sits on this branch |
| `u/timj/polymap-bad-input-test` (local) | 1 test | Not yet |

In August, 13 catalogued defects were fixed upstream and about 19 were in flight.
Now 101 are fixed, on C master or on `norm-unit`.
The `norm-unit` fixes, by file:

- **`fitschan.c`: 20.** Mostly found by the FitsChan audit: `-TAB` and `-LOG` axes, MJD-OBS and DATE-OBS consistency, alternate axis descriptions, `EncodeFloat` losing a sign, `CnvType`, SIP only for FITS-WCS, and attributes lost when the cards are emptied.
- **Moc and MocChan: 7.** Malformed MOC text and data, `MaxOrder` 0, an empty or whole-sky Moc, a mesh that depended on `qsort`'s order.
- **Regions: 12** across `region.c`, `box.c`, `interval.c`, `pointlist.c`, `prism.c`, `nullregion.c`, `cmpregion.c`, `stc.c` and `stcobsdatalocation.c`: an XOR component split apart, a `Box` transform that widened its cache, `Overlap` arguments in the wrong order.
- **Mappings: 9**, one each in `lutmap.c` (a read before the start of the table), `polymap.c`, `chebymap.c`, `splinemap.c`, `timemap.c`, `tranmap.c`, `permmap.c`, `selectormap.c` and `wcsmap.c`.
- **Frames and PointSet: 8** across `cmpframe.c`, `frameset.c`, `specframe.c`, `fluxframe.c`, `skyaxis.c` and `pointset.c`.
- **Units, tables and IntraMap: 7**: three in `unit.c`, three in `table.c` and `fitstable.c`, and one in `intramap.c`.

The test on `u/timj/polymap-bad-input-test` pins PolyMap's rule for bad inputs (an output is bad only if one of its terms uses the bad input), so that Starlink/ast PR 51's SIMD code, which makes every output bad, fails it when rebased.

### Performance findings that apply to C

This period added a new kind of entry: a speed-up found in the port that would work in C's default build, with the result unchanged.
Each records the C location, what the port did, the port's measured gain, and a suggested C change; none of the gains has been measured in C.

| C code | Change | Port's gain, per point |
|---|---|---|
| `polymap.c` `Transform` | Drop the `pow > 0` branch, skip bad-value tests for an all-good point, and evaluate a block of points term by term (PR 51's loop order, which needs no `-ffast-math`) | degree-5: 53 → 38 ns from the branch alone; blocked, 5× C |
| `matrixmap.c` full-matrix `Transform` | Loop outputs outside points; replace the early exit with a bad flag and a conditional add (also in PR 51's SIMD build only) | 3×3: 6.8 → 3.3 ns; C 11.5 |
| `lutmap.c` inverse | Run a block of 32 bisections, then the rest of the work for the block | 16.7 → 13.8 ns; C 13.0 |
| `splinemap.c` `Dbvalu` | One copy per spline order with fixed-size arrays | `lsst2` bicubic: 281 → 170 ns forward; C 206 |
| `pointlist.c`, `selectormap.c`, `matrixmap.c` `CompressMatrix` | Work repeated per point or per row: PointList moves its uncertainty twice per point, SelectorMap negates and re-closes its Regions on every call, `CompressMatrix` recomputes `sqrt( DBL_EPSILON )` | not timed |

Two of these, the PolyMap and MatrixMap loop orders, are what PR 51 does inside its opt-in SIMD build.
The port shows they give C's exact bits without `-ffast-math`, so they can go into C's default build independently of the SIMD work; only PR 51's vector `atan2`, `sin` and `cos` need fast math.

Alongside these sit C defects the performance work exposed: a LutMap that returns a stale result after `LutInterp` changes (confirmed by probe), and unchecked `double`-to-integer conversions in SwitchMap and Moc.

---

## 5. Performance

August had no current benchmark: a three-month-old hand-run snapshot, a documented CI gate that did not exist, and PermMap 42× slower than C at large N.
There is now a benchmark, a record of every round of optimization, and a check that no optimization changed a result.

**The tooling.**
`ast-bench` times the port against C on the same machine (`run`, `compare --base`, `snapshot`, `sweep`, `roman`), fails on a slowdown over 10% with `SLOWDOWN_LICENSED` empty, and has a `bits` check that fails if any corpus transform's output bits change.
Every performance change in this period passed `bits`, so the speed-ups below changed no output value.
Dated records are in `docs/perf/`; CI still runs none of it (§9).
The speed-ups that would also work in C are listed in §4.

### Where it stands today

Ratios below are Rust time ÷ C time, so under 1 means the port is faster.
They are from `docs/perf/2026-10-19-macos-aarch64-bench.tsv` and `-framesets.tsv`, recorded for this review at `69c40996` on an Apple M1 Ultra (load average 6.5 to 7.6 from other work), comparing each engine's fastest sample.

**The `bench` set, 36 Mappings and FrameSets in both directions (72 cases):**

| Points | Geometric mean | Cases faster than C | Cases over 1.1× |
|---:|---:|---:|---:|
| 1 | 0.09 | 72 | 0 |
| 100 | 0.33 | 70 | 2 |
| 1,000,000 | **0.84** | 37 | 16 |

On the 52 cases also in the 2026-10-17 record, the geometric mean at 1M points went from 1.04 to 0.86 in two days, mostly from the PolyMap, SplineMap and LutMap kernels.

**Real-world FrameSets at 1M points:**

| FrameSet | Forward | Inverse | Rust ns per point, forward |
|---|---:|---:|---:|
| `lsst` (Rubin) | **0.39** | **0.44** | 81 |
| `lsst2` | 0.61 | 0.67 | 244 |
| `jan` | 0.93 | 0.93 | 50 |
| `rigby` | 0.91 | 0.93 | 118 |
| ACSIS cube | 0.94 | 0.94 | 55 |
| `dss` | 0.95 | 1.00 | 87 |
| `timj` | 0.98 | 0.98 | 102 |
| `brad` | 1.02 | 1.10 | 37 |
| `lsst1` | 1.08 | 1.08 | 2.5 |

Rubin's `lsst` FrameSet was the stated condition for switching to the port: it had to be faster than C.
It is now about 2.5× faster in both directions, almost all of it from the PolyMap block evaluation.
`brad`'s inverse, a LutMap, is the one real-world case clearly slower than C; it was 1.6× before the bisection was rewritten.
`lsst1` is a 2.5 ns-per-point transform where both engines are bound by memory.

**Where the port is still slower, at 1M points:**

| Case | Ratio | Rust / C ns per point | Why |
|---|---:|---:|---|
| MathMap, linear 2-D | 1.9× | 7.5 / 3.9 | The expression evaluator; not profiled since the per-axis change |
| SpecMap, wavenumber to frequency | 1.5× | 0.78 / 0.51 | Down from 4–5× on Oct 17 |
| PermMap, UnitMap, WinMap | 1.2–1.5× | 0.4–0.6 / 0.3–0.5 | Under a nanosecond per point; not investigated further |
| LutMap forward | 1.3× | 2.0 / 1.5 | |
| WcsMap SIN inverse, TimeMap TAI to UTC | 1.2× | 8.5 / 7.1, 6.5 / 5.5 | Dominated by per-point math in vendored wcslib and ERFA |

The `framesets` set also shows a parallel IntraMap at 19×, which is the benchmark harness's own test transform, not the port's code, and a cheap spectral header (`vopt-wav`) at 1.75× and 0.28 ns per point.
Where the port is fastest is where C's per-point loop does avoidable work: NormMap 0.14×, the degree-5 PolyMap 0.20×, the full 3×3 MatrixMap 0.28×.
Single-point calls are about 11× faster than C (geometric mean 0.09), because C's per-call set-up dominates.

**The Roman benchmark reproduces Starlink/ast PR 51's transforms.**
At the largest sizes the port matches C on SphMap, is about 5× faster on PolyMap (after block evaluation; the committed 2026-10-18 record predates it and shows 1.4×), 3.5× faster on the full 2×2 MatrixMap, and runs the diagonal MatrixMap at 0.54 of C's speed while the data fits in cache.
Rust and C output bits are identical over all 200M values.
PR 51's own SIMD build runs about 4× faster than scalar C on the same machine.

**The diagonal-MatrixMap result is a contract cost, not a missed optimization.**
The port must turn any NaN or infinity in an output into `AST__BAD`, which costs a pass over the outputs that C's code skips for that branch.
`docs/issues/bad-value-and-nan-cost.md` records the cost and proposes discussing with upstream an API that declares BAD unused and lets IEEE values pass through unchanged.

**Further gains are in vectorizing ERFA, PAL and wcslib**, recorded as future work in `docs/issues/vectorized-erfa-pal-wcslib.md`, and deliberately left until the parity work merges.

---

## 6. Status against the oracle

| Tier | What it is meant to prove | State now | August |
|---|---|---|---|
| 1 · Idiomatic Rust | The public API behaves. | 4,052 passing across the workspace (3,980 in `ast-core`, run per package; see §9), 0 failing. The 4 ignored are a timing benchmark, a 45 GB test that C also runs only on request, the investigative oracle report, and a probe-output writer; none skips an assertion. | Green |
| 2 · Byte-exact regression | Output byte-identical to C on a real corpus. | 515 simplify fixtures; 194 wcsconv cases with nothing excluded; 2,261 transform-oracle sections; 823 round-trip dumps; all through a comparator that now fails on any whitespace or spelling difference except a number's width change in a fixed-width card. | Green, one known hole |
| 3 · C tests vs Rust libast | `ast_tester` programs link the Rust library. | Not started: `ast-capi` exports nothing. | Not started |
| 4 · Live C comparison | Rust and C in one binary, compared. | **Partly started.** `ast-oracle-tests` runs C live and compares library call counts (libm, PAL, ERFA, wcslib) per section; `ast-bench roman` and `bits` compare output bits live. No value-by-value test suite yet, and none in CI. | Not started |
| 5 · Performance gate | Fail on a regression. | `ast-bench compare` and `bits`, run by hand. | Not started |

The final audit asked whether the tests run what they claim.
It found seven places where they did not quite, and all are fixed:

- **Transform oracle:** sections for fixtures that load as a bare Frame were counted as checked; they are now an `UNTRANSFORMABLE` list of 11 with the identity C records per direction, and exact counts of round-trip and equivalence checks are asserted.
- **Dump comparator:** the simplify and wcsconv comparator normalized whitespace, the same blind spot the round-trip comparator had in August; it is now strict and has its own tests.
- **Missing fixtures:** a missing wcsconv input now fails instead of being passed over.
- **Blank FITS cards:** the port dropped blank cards where C keeps them (confirmed by probe); fixed.
- **Ledger scope:** the verification ledger now scans `ast-oracle-tests` and `ast-bench` as well as `ast-core`, and gained 10 entries.
- **Unchecked zips:** four tests zipped two lists without checking their lengths matched.
- **Missing C error checks:** parts of `testskyframe.c`, `testcmpframe.c` and `testspecframe.c` had not been ported; they are, and the port matches C.

Mutation testing then planted one bug of each kind (forward and inverse transforms, simplify values and rules, serialization, whitespace, Region, Frame, FITS) and the suite caught every one.

### The relaxations

The ledger in PLAN §6, mirrored by `test_verification_ledger.rs`, counts every relaxation, and adding one without a reason in both places fails a test.
The ones that matter for a parity claim:

- **wcsconv:** 12 cases compare within `astEQUAL` (FMA or libm differences against x86-64 captures), and 2 drop `IsSimp` lines where C's placement records its reader's simplification order.
- **Simplify:** 158 negative fixtures strip `IsSimp`, because their reference is the unsimplified input; 4 values are licensed as platform drift; 5 `IsSimp` cards and 4 `Invert` cards record C merge probes or shared-object history that owned values cannot reproduce; 2 rows compare within `astEQUAL`.
- **Transform oracle:** 3 overrides (two `tnx-cheb` round trips a ChebyMap cannot invert, one zero-norm UnitNormMap pair), 5 rows where C's own inverse fails too, 1 fixture refused by decision.
- **Smaller oracles:** about 20 entries across cursor, object-ID, map-split, `-TAB`, FitsChan-error and region tests, each with a reason and a companion test that fails when the entry starts passing.

No harness has a skip mode.

---

## 7. What is absent

**39,464 code lines of C product classes** have no Rust counterpart: `Plot`, `Plot3D`, the `grf` backends, `YamlChan`, `XmlChan` and its `xml.c` parser, and `StcsChan`.
Since August, `Moc`, `MocChan`, `Table`, `FitsTable`, `IntraMap`, `XphMap`, the `Stc*` classes, and the resampling and grid algorithms have all landed.

Two other blocks of C are not ported and are not meant to be, in the same form:

- **The Fortran interface** (`f*.c`, 67 files, 8,659 lines) belongs with the C ABI shim.
- **Object infrastructure** (`object.c`, `memory.c`, `error.c`, the loader and globals; 11 files, 5,472 lines) is replaced by Rust's ownership and the planned shim.

The vendored C is unchanged: wcslib, ERFA and PAL are still compiled into every build.
Their Rust rewrite (step 6a) is deferred to a future ticket, after this branch merges.

---

## 8. The August risks, revisited

**Risk 1, the ABI shim has no code: unchanged.**
`ast-capi`, `ast-plugins` and `ast-ffi-tests` are still stubs, and tier 3 cannot start.
This remains the largest gap between what the port has proved and what it is for.

**Risk 2, the next C merge changes serialized output: resolved.**
`misc-bugs` merged on Aug 20, the pin moved, the mirrored bugs were removed and the corpora regenerated together.
`check_corpus_fresh.sh` and a drift classifier now separate stale references from platform drift on every pin move.

**Risk 3, a tier-2 gate with a known hole: closed, twice.**
The round-trip comparator was made exact on Sep 4.
The final audit found the same blind spot in the simplify and wcsconv comparator and closed it, with `test_dump_comparator.rs` pinning what it may and may not normalize.

**Risk 4, two latent Region gaps: closed.**
The geodesic Polygon mesh landed on Oct 8.
Of the other six region findings, two remain, and both wait on Plot.

**Risk 5, performance undefended: half answered.**
The measurements, the tooling and the results are in §5.
What is still missing is a gate nobody has to remember to run.

---

## 9. New risks

**1 → `cargo test --workspace` fails.**
It turns on call counting for every crate at once (Cargo merges features across a workspace build), and `ast-bench`'s `a_timed_build_does_not_count_calls` correctly refuses to time a counting build.
Each crate's tests pass on their own, which is how they have been run.
The August review's test count came from `--workspace`; that command has failed since call counting landed on Oct 15, without anyone noticing, and needs fixing or replacing.

**2 → No CI for tiers 3–5.**
Call-count parity, `ast-bench compare` and `bits` all run only by hand.
The design's nightly job does not exist.

**3 → The pin is an unmerged branch tip.**
`d9e43a6b` is `u/timj/norm-unit`, not upstream master; the pin has to move again once it merges, with a corpus freshness pass.

**4 → One FitsChan read gap is known.**
With two copies of the same keyword, the port takes the first and C the last (`error-bad-radesys`, excluded with a reason and gated).

**5 → PLAN.md has stale text.**
§7 still says no test runs C live and that the `misc-bugs` merge is pending; §6 still points at a closed whitespace hole; §2's performance figures predate the October work.
Anyone using PLAN.md as the status page will be misled until these are fixed.

---

## 10. The pattern since August

August's conclusion was that the port's best work reads the C function, ports it whole and writes down what looks wrong, while its worst work builds a plausible Rust shape and pays for it later.
It recommended making that discipline mechanical rather than remembered.

That is what happened.
Bypassing the `IsSimple` gate is a compile error, relaxations are counted by a test, comparators are tested against what they must not hide, every subsystem got a written parity audit before being called done, and performance work could not change a single output bit without `ast-bench bits` failing.
The period's two biggest results, simplifier parity and the large-N performance recovery, both came from porting C's structure rather than approximating it: C's merge order for the first, C's per-axis memory layout for the second.

What has not changed is the order of work.
The core library is now close to complete and verified against frozen C output, and the C ABI, the part that lets existing programs use it, still has no code.
