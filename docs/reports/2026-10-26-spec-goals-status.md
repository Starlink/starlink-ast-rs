# Status against the design spec's goals

**Written:** 2026-10-26 · **Tree:** `main` @ `e70bdc5e`, the day `starlink-ast` 0.1.0 was published · **Spec:** `docs/superpowers/specs/2026-04-20-rust-port-design.md` (rev 41) · **Submodule pin:** `c6c60753` (C master, 2026-10-22)

This report answers one question: how close the port came to the goals the spec set in April, leaving aside the parts that were never started.
Goals 2 and 5 (the C ABI shim and the ctests through it) and delivery steps 10 to 12 (Plot, the shim, the cutover) are unstarted rather than diverged, and are not assessed.
The August conformance review (`2026-08-19-spec-conformance-review.md`) checked the architecture, and the October progress report (`2026-10-19-port-progress-report.md`) recorded the performance and oracle numbers quoted here.
Every other figure was taken from the tree on the date above.

---

## 1. Verdict

**The architecture is exactly as specified, and the functional gap is three text channels and the wcslib port.**
Of the four goals in scope, goal 1 (the idiomatic API) is met outright.
Goal 4 (test migration) is met for everything the ported classes enable: every fixture is mirrored, 38 of 55 C programs are fully ported, 6 more are ported except for blocks that belong to the C API, and 11 wait on Plot, the text channels or C's object model.
Goal 3 (byte-exact serialization) is met for four of seven formats; XML, YAML and STC-S wait on their channels.
Goal 6 (performance within 10% of C) is met in aggregate, a geometric mean of 0.84 of C's time at a million points and the Rubin FrameSet 2.5× faster, but 16 of 72 benchmark cases remain more than 10% slower.

---

## 2. Goal 1: an idiomatic Rust API

| Spec commitment | Status |
|---|---|
| Sibling `Mapping` and `Frame` traits, `Region: Frame + Mapping` | Exact |
| Owned values, `&mut self` mutation, `Clone` is the deep copy | Exact |
| `Send` but not `Sync`, no locks in hot paths | Exact |
| `BAD_VALUE = -f64::MAX`, NaN accepted on input only | Exact |
| 1-based axis indices | Exact |
| `then` / `alongside`, no operator sugar | Exact |
| FrameSet and Region integrity, `frame_mut()` as the escape hatch | Exact, implemented per class |
| Private `Serializable`, no public `Object` trait | Exact, with the public `AsAny` downcast supertrait added in the first week |
| Typed attributes only; no `get_attr`, `set_fmt`, `edit` or `with_*` copies | Held; none reappeared |
| `ndarray`-backed `PointSet` | Changed (rev 34): one contiguous array per axis, with `ndarray` conversions |
| Builders on every class | Relaxed (rev 6): builders where construction has options, plain constructors elsewhere |

The trait surfaces grew about fivefold over the spec's sketches (`Mapping` from 13 to 28 methods, `Frame` from about 10 to 90, `Region` from 3 to about 50), almost entirely into C's per-class extension points: `MapMerge`, `Equal`, `MapSplit`, `Match`, `SubFrame`, `Overlay`, `RegPins` and their kin.
The spec's revision history records every change, 41 revisions in all, and none reverses a decision from the five review rounds.

---

## 3. Goal 3: byte-exact round trip of every serialized format

| Format | Status |
|---|---|
| Native | Byte-exact: 823 round-trip dumps and 517 simplify references, through a comparator that normalizes nothing |
| FITS-WCS, FITS-IRAF, FITS-PC, FITS-AIPS, FITS-AIPS++, FITS-CLASS and DSS | Byte-exact through the wcsconverter harness: 194 cases, 12 compared within `astEQUAL` for FMA and libm drift against x86-64 captures, 2 dropping `IsSimp` lines |
| FITS-Native encoding | Ported and oracle-tested (`test_fitschan.rs`, `test_channel_dump_oracle.rs`) |
| MOC | Ported, with `testmoc` and `testmocchan` and C's dump-format checks |
| XML | Not ported: `XmlChan` and its `xml.c` parser, 8.5k code lines of C |
| YAML | Not ported: `YamlChan`, 7.6k lines |
| STC-S | Not ported: `StcsChan`, 4.1k lines |

The JSON renderer that step 5 of the delivery order added was never built, and nothing has needed it.

---

## 4. Goal 4: full migration of `ast_tester/`

- **Fixtures: 100%.** `tools/fixtures/check_mirror.sh`, run by CI on every pull request, fails unless the Rust fixture tree is a byte-identical copy of upstream's at the pin.
- **Programs: 55, tracked in PLAN.md's migration table.** The other two `test*.c` files upstream are helpers for the port's own fixture generators.
  38 are fully ported.
  6 are ported apart from blocks that belong to the C API: `AST__IMMUT` (`testzoommap`), the handle registry (`testobject`), the string-attribute path and `astTuneC` (`testfitschan`, `testchannel`), the options-string constructor (`testpointset`) and ChebyMap's unexposed `IterInverse` (`testchebymap`).
  11 are deferred: 7 wait on unported classes (`teststc`, `testxmlchan`, `teststcschan`, `testyamlchan`, `testgrid`, `testplot3d`, `testplotter`), 3 test C's object and error model (`testimmutable`, `testobjectkeymap`, `testerror`), and `testthreads` does not apply to a library without C's handle machinery.
- **Harnesses: two of three.** `wcsconverter` (194 cases) and `simplify` (517 cases) are Rust test programs driven by the same fixtures as C; the plot and grid smoke matrix waits on Plot.
- **Tier 1 today:** 4,412 tests pass in `starlink-ast` on this tree, none fail, and 5 are ignored because they run only on request (the 45 GB `testhuge` among them).

---

## 5. Goal 6: performance within 10% of C

From `docs/perf/2026-10-19-macos-aarch64-bench.tsv`, Rust time over C time on an Apple M1 Ultra at `69c40996`:

| Points | Geometric mean | Cases faster than C (of 72) | Cases over 1.1× |
|---:|---:|---:|---:|
| 1 | 0.09 | 72 | 0 |
| 100 | 0.33 | 70 | 2 |
| 1,000,000 | 0.84 | 37 | 16 |

Rubin's `lsst` FrameSet, the stated condition for switching to the port, runs at 0.39 of C's time forward and 0.44 inverse.
The cases still more than 10% slower at a million points are MathMap (1.9×), SpecMap (1.5×), LutMap forward (1.3×), three mappings that cost under a nanosecond per point (PermMap, UnitMap and WinMap, 1.2 to 1.5×), and WcsMap and TimeMap (1.2×), the last two bound by the vendored C they call.
The hot-path constraint holds: `SkyFrame::set_epoch_mjd` and `set_sky_ref_is` are single field assignments with no allocation and no lock.
What the spec asked for and does not exist: benchmarks of simplification and serialization workloads, and a CI job running the gate.
`ast-bench compare` fails on a slowdown over 10%, but it is run by hand.

---

## 6. Delivery order, steps 0 to 9

| Step | State |
|---|---|
| 0 Bootstrap | Done, except the nightly tier-3/4 job, which was never created |
| 1 Workspace, errors, traits, `PointSet`, `PointList` | Done |
| 2 Simple Mappings | Done |
| 3 `BasicFrame`, `CmpFrame` | Done |
| 4 `FrameSet` and integrity | Done |
| 5 Node tree, Native, JSON | Native done and byte-exact; JSON never built |
| 6 `PolyMap`, `LutMap`, `MathMap` | Done, and every other Mapping class with them |
| 6a Rust port of `wcslib` | Not started; 3.3k code lines of C still linked, deferred to a later ticket |
| 7 Sky, Spec, Time, Flux and SpecFlux frames | Done, every attribute checked against C |
| 8 `Channel`, `FitsChan`, `YamlChan`, `StcsChan`, `MocChan` | Channel, FitsChan and MocChan done; YamlChan and StcsChan not, nor XmlChan, which the step never listed |
| 9 Regions and integrity | Done, the STC classes included |

Beyond the steps, the port added what the spec did not foresee: `Table`, `FitsTable`, `KeyMap`, `IntraMap`, `XphMap`, resampling and rebinning as free functions over borrowed slices in place of the spec's `DataCube<T>`, `astTranGrid`, `astQuadApprox`, and a user guide ported from SUN/211.

---

## 7. Where the spec was wrong rather than unfinished

- **The trait sketches read as complete surfaces and were used as one.** They omitted every per-class method a generic driver dispatches to, and the missing `MapMerge` seam cost a 47-commit simplifier rebuild (H1 in `docs/porting-hazards.md`).
- **Byte-exact comparators needed tests of their own.** Two of them normalized whitespace, which hid real defects until the October audit made them strict.
- **The spec has no section on the C library as a subject of the port.** The port became C's most productive defect source, with 101 catalogued defects fixed upstream (`docs/issues/c-library-quirks.md`), and the spec anticipated none of that.
- **The JSON renderer and `DataCube<T>` were designed and never needed.**
