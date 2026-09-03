# PLAN.md — Rust AST port: outstanding-work clearing house

Ticket DM-54793. This file is the single index of active, audited, outstanding
work in the Rust port. When a gap is found and written up, link it here so the
write-up is discoverable and does not go stale in isolation. When work lands,
update the status column (and, when a whole area closes, strike it through with
the closing commit).

Convention: detailed analysis lives in `docs/audits/` (structured gap sweeps) or
`docs/issues/` (per-subsystem port-gap catalogues). This file holds only the
one-line pointer and current status for each. A finding closes to one sentence
here, and its write-up moves to
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md) under the
same section number, so this file stays cheap to read in full.

---

## 1. Region classes

| Area | Doc | Status |
|---|---|---|
| Phase 5 cross-cutting parity (simplify / uncertainty machinery) | `docs/audits/region-phase5-parity-spec.md` | **CLOSED** — all 8 gaps landed (RB-PICK/MERGE-FF/SETUNC/DEFUNC/SIMP-STAB/CE-FIT + R-UNC/PR-MAP by refactor); final commit `a96bc80` |
| 4 critical `equal()` failures + shared root-cause primitives (R-PINS, R-OVL) | `docs/audits/region-synthesis-plan.md` | **CLOSED** — all 4 fixtures pass; `d834b81` closed the last (cap_slamap_07) |
| Post-Phase-5 coverage sweep (broader vtab override audit) | `docs/audits/region-class-coverage-audit.md` | OPEN — 7 findings, none a current red gate; see below |

Per-area source audits feeding the two closed docs above (kept for reference):
`region-base.md`, `region-box-interval-pointlist.md`,
`region-circle-ellipse-polygon-null.md`, `region-cmpregion-prism.md`.

### Outstanding region findings (from the coverage sweep)

Detail and C↔Rust locations in `docs/audits/region-class-coverage-audit.md`.

| ID | Gap | Gated behind | Priority |
|---|---|---|---|
| MESH-GEO | Polygon `reg_base_mesh` planar not geodesic (polygon.rs:623 vs polygon.c:4329) | — (latent; sky-polygon fixture) | highest — affects overlap/MOC when reached |
| PLY-DOWN | Polygon `Downsize`/`DownsizePoly` unported; verify Simplify vertex reduction | — (latent; serialization parity) | high — confirm before scheduling |
| GRID-FRM | `reg_base_grid` generic odometer, no Frame-aware `astFrameGrid` (data.rs:354) | Frame work | medium |
| BND-INT | Interval `BndBaseMesh` override missing (overlap.rs:376 vs interval.c:215) | overlap pass | medium |
| RT-TRACE | `RegTrace` boundary tracing unported (base + 6 classes) | Plot/Grf backend | deferred |
| RS-SPLIT | CmpRegion `RegSplit` disjoint split unported (cmpregion.c:2472) | Plot/Grf backend | deferred |
| ACC-PARS | Geometry accessors (`CirclePars` etc.) unexposed | STC-S | deferred |
| CIR-NB3 | Circle `reg_base_box` calls `norm_box` only when `naxes == 2` (circle.rs:329), where C calls `astNormBox` for both the 2-D and the >2-D branch (circle.c:1100, outside the if/else); above two axes the port also takes an axis-offset path C does not have | — (latent; needs a >2-D Circle on a Frame with an override, i.e. a Circle on a CmpFrame(SkyFrame, ...) enclosing a pole, whose bounds C reports unwidened via the quirk in `docs/issues/c-library-quirks.md` and whose Frame-order variant C errors on) | medium |

## 2. Frames and Mappings

| Area | Doc | Status |
|---|---|---|
| Frame + Mapping class C-port gaps (function-by-function) | `docs/issues/frame-mapping-c-port-gaps.md` | OPEN — audit COMPLETE; remaining items are deferred delivery-step work only (Rebin/Resample/TranGrid/QuadApprox, JSON/XML/YAML renderers, absent classes, wcslib port 6a, Plot-support Frame methods + NormalPoints, SkyAxis Format precision) |
| Per-class Mapping performance vs C oracle | `docs/issues/mapping-perf-vs-c-oracle.md` | OPEN — **no automated perf gate exists**; `ast-oracle-bench` is a placeholder and CI runs no benchmark, so the >10% hot-path gate CLAUDE.md describes is not enforced. Snapshot (2026-05-19) records cheap mappings 5–42× slower than C at N=100k (permmap-fwd 42×, winmap 6.6–7.6×, shiftmap 5.7–7.6×, diag matrixmap 5.5×, slamap 3.9×, wcsmap-tan-fwd 4.2×); PolyMap/MatrixMap — the SCUBA-2 / LSST path — remain at or ahead of parity. Untested hypothesis worth measuring before per-class work: `PointSet` is row-major `(npoint, ncoord)` where C keeps one contiguous array per axis, which would explain the whole cheap-mapping cluster at once |
| `astTran*` definedness validation | — | OPEN — `Mapping::tran1`/`tran2` raise `AST__TRNND` for an undefined direction (C `ValidateMapping`, mapping.c:179), but the general `transform` / `transform_into` path raises it nowhere: `Trnnd` appeared in no source file before `156cf68`. Deciding which classes should report it needs a C-oracle probe per class, since C validates in the public `astTran*` entry points rather than in `astTransform` |
| CmpMap `inv_a`/`inv_b`: split the load contract from the dump-shaping emulation | `docs/issues/cmpmap-invert-snapshots.md` | DEFERRED to post-oracle-compliance — measured, with a falsifiable go/no-go test: after the §8 pin move, re-run the load-drift probe and collapse the mechanism only if it reports zero |
| TimeFrame port + Epoch/Equinox API | `docs/issues/timeframe-and-epoch-api-port.md` | OPEN (residual) — Issue A (DATE-OBS/TDB drift) RESOLVED; only Issue-B doc/hygiene items remain (Equinox=UTC note; `_year` setter public/private decision) |

Closed (marked historical in-doc, removed as active work): `mapping-gaps-audit.md`
(superseded by `frame-mapping-c-port-gaps.md`), `wcsmap-naxis-refactor.md`
(landed), `timeframe-unformat-findings.md` (resolved 2026-06-25).

## 3. Containers

| Area | Doc | Status |
|---|---|---|
| KeyMap port | `docs/superpowers/specs/2026-08-06-keymap-port-design.md` | DONE — `crates/ast-core/src/keymap/`; entries in a `HashMap` with `MapSz` reconstructed at dump time; `testkeymap.c` green; C-ABI shim deferred with the rest of `ast-capi` |
| Table / FitsTable | — | UNPORTED — gates the `-TAB` branches in the two section-5 rows; `KeyMap` is shaped to be embedded by both |

## 4. Simplifier

All four simplifier docs are now **historical** (rebuild complete, bugs fixed —
each carries an in-doc note): `simplifier-mapmerge-rebuild-plan.md` (nominate
loop + per-class `map_merge` landed, rigby byte-exact),
`simplifier-winmap-inverted-merge-bug.md` (fixed `a4bff3c`, `spec_cel_invert`
active regression), `simplify-serialization-gaps.md` (0 failures, 719/719),
`sph_fwd_inv_unitradius_cancel_analysis.md` (fixture byte-exact). No active
simplifier work remains here; ongoing simplify parity is tracked by the
regression corpus.

## 5. Serialization / FITS / round-trip

| Area | Doc | Status |
|---|---|---|
| FitsChan C-port gaps | `docs/issues/fitschan-c-port-gaps.md` | PARTIALLY DONE — DSS/SAO/DSBSetUp/SipReplace, `DistortMaps`/`ZPXMapping` and `SIPMapping`'s store-derived powers ported; **all 194 wcsconv cases now byte-compared with `NATIVE_READ_EXCLUDED` empty**; remaining: unported attr quartets, `-TAB` family, public-API helpers, Warn cards |
| `fset_to_store` port gaps | `docs/issues/fset_to_store-port-gaps.md` | MOSTLY DONE — SIPIntWorld + VELOSYS + Group 3/5 branches ported; only `-TAB` (IsMapTab1D/2D) remains, blocked on the un-ported FitsTable infrastructure |
| Round-trip harness misses trailing-whitespace divergences and the corpus has no comment-less nested-object card | `crates/ast-core/tests/test_serialisation.rs:1097,1129` | OPEN — `native_round_trip_fixture`'s `trim()`/`split_whitespace()` comparison discards trailing whitespace, so `"Frm1 = "` and `"Frm1 ="` compare equal; separately, `grep -rhcE " =$"` is 0 across all of `simplify_fixtures/`, `frameset_fixtures/` and `generated/` — the four fixtures that do have a comment-less object card (`lsst.ast`, `lsst.simp`, `rigby.simp`, `simplify_fixtures_unsupported/intramap/intramap_inverse_cancel.map`) sit outside every scanned directory; needs the fallback comparison tightened to make trailing whitespace significant and a comment-less-object-card fixture moved into a scanned directory |
| Comment-less nested-object card wrote a trailing space C never writes | — | FIXED (`1d2aa8f`, `86810ce`) — C's `WriteObject` (`channel.c:5106-5112`) appends `" ="` and adds the separator only when the comment is non-empty; the Rust Object arm pushed `" = "` unconditionally |

Cross-cutting blocker for the first two rows above: the **FitsTable / `-TAB`
algorithm** port is unported and gates the remaining FitsChan and
`fset_to_store` `-TAB` branches.

### Deferred C-oracle simplify fixtures

Twenty-four fixtures from upstream `ast_tester/simplify_fixtures/` are staged in
`crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/`
because they exercise classes the port has not implemented.
They are counted and enforced by
`crates/ast-core/tests/test_deferred_fixtures.rs`, which fails if a
subdirectory is added without a matching `DEFERRED_CLASSES` row.

| Subdirectory | Unported class | Files | Blocks |
|---|---|---|---|
| `intramap/` | `IntraMap` | 13 | 10 simplify cases (3 positive, 7 negative-control) |
| `xphmap/` | `XphMap` | 9 | 6 simplify cases (3 positive, 3 negative-control) |
| `stcresourceprofile/` | `StcResourceProfile` | 2 | 1 simplify case (`cap_timemap_02`) |

No wcsconv case is blocked on any of these classes.
The re-enabling procedure is in the directory's `README.md`.

Closed (marked historical in-doc, removed as active work):
`native-roundtrip-gaps.md` (24/24 + 139/139 byte-exact),
`native-roundtrip-simplify-gaps.md` (719/719 byte-exact),
`wcsconv-residual-classification.md` (wcsconv 192/192, all buckets cleared).

## 6. Cross-cutting

| Area | Doc | Status |
|---|---|---|
| C-library quirks mirrored in the port | `docs/issues/c-library-quirks.md` | running list — discuss with upstream |
| Whether `XmlChan` / `YamlChan` / `StcsChan` carry a `KeyMap` or a `Node` | `docs/superpowers/specs/2026-08-06-keymap-port-design.md` (Deferred decisions) | OPEN — decide per subclass when the first one is ported; `yamlchan.c` relies on `SortBy` ordering as algorithm state (`:9814`, `:11391`, `:11531`, `:12191`), which `Node` cannot express |

### Verification-posture ledger

Every way a gate in `ast-core` can be relaxed, counted so the total cannot grow
silently. **This table is mirrored by `crates/ast-core/tests/test_verification_ledger.rs`,
which is the executable copy**: it asserts each count and fails on any
`const *_EXCLUDED` / `_LICENSED` / `_SKIPPED` / `_UNGATED` / `_DISAGREEMENTS`
in the test tree that is not in its table. Adding a relaxation therefore means
writing its reason down in two places, and finding one by accident is no longer
possible.

The last two suffixes were added at the simplify parity closeout, and with them
three lists that had been documented in §10 but never counted here: the six
byte-comparison divergences in `SERIAL_UNGATED` and the seven-fixture
`ISSIMP_DISAGREEMENTS` set (both since reduced).
Each was already asserted as an exact set in its own harness, so none could
grow unnoticed; what was missing was their appearance in the total. The counter
gained the ability to count them at the same time: it read a flat `&[&str]`
list as empty, so a ledger row for one would have passed at any size.

| Gate | Cases | Relaxed | What the relaxation is |
|---|---:|---:|---|
| wcsconv (`test_wcsconv.rs`) | 194 | 8 | `astequal` compare mode — FMA contraction / libm differences vs x86-64 Linux captures that C AST on arm64 does not reproduce either |
| wcsconv | 194 | 3 | `ISSIMP_PLACEMENT_LICENSED` — drops the `IsSimp` tag lines both sides *and pins the card count on each side*, so it covers the tag placement and nothing else. A deliberate divergence, not a gap: C's placement here records the order its reader simplified in (§8). `axes-swap`, `vopt-f2w`, `wave-f2w`; C emits more cards than the port in each. 24 rows preceded these; 20 were pinned against references the pinned C no longer reproduced and were retired with the corpus refresh (§9.3), and `cd-cross` retired when the nominated-component branch stopped discarding the stamps its own simplify raised (§10.3) — its four cards now sit exactly where C's do, and `axes-swap`'s pin rose from two to three the same way |
| wcsconv | 194 | 0 | **No skip mode exists.** `NATIVE_READ_EXCLUDED` is empty and gated against refilling |
| simplify regression | 471 | 0 | **No skip mode exists.** `skip_string_compare` is empty and gated against refilling: every row in the manifest is byte-compared. The last two, `cap_cmpmap_01` and `cap_cmpmap_04`, gave theirs up for the two positional aliasing licences below. Was 4 before that: `tools/oracle_fixtures/check_simplify_fresh.sh` recaptured the whole corpus from the pinned C and showed `cap_winmap_03`'s reference was stale — it predates both this pin and `f6762005`, so the sub-ulp `Sft2` residue it carried was never this C's output — and showed `rigby`'s difference is a single named value, now licensed by `NUMERIC_PLATFORM_DRIFT` instead. `lsst` retired its skip earlier, when the manifest gained a `render_options` field: its reference was captured with the Channel's `Comment` and `Indent` off, and rendering the same way makes it byte-exact |
| simplify regression | 471 | 5 | `NUMERIC_PLATFORM_DRIFT` — one `IM5` card of `rigby`, `-5.9658198263111446e-17` in the reference against `-5.8980598183211441e-17` from both the port and the pinned C on this machine; `cap_interval_01`'s `X5` / `X6`, the two sky half-widths of its uncertainty Box, `2.4240684055495722e-06` and `2.4240684055430873e-06` in the reference against `2.4240684055476816e-06` from both the port and the pinned C here; and `cap_pointlist_01`'s `X5` / `X6`, the sky half of its merged uncertainty Box's corner, `3.1241551949160478` and `-0.20069734795308758` in the reference against `3.1241551949160575` and `-0.20069734795284888` here. Numeric drift, not a port gap: the references were captured on another platform's libm, and the corpus-freshness rule keeps such a reference rather than replacing it with this machine's. `rigby`'s value is cancellation residue with no significant figures; `cap_interval_01`'s two agree with the reference to twelve significant figures and are nominally equal to each other, which this machine's C makes them and that platform's did not; `cap_pointlist_01`'s two are what `astSetUnc` regenerates by re-centring the Box and re-offsetting its corner through a `CmpFrame(TimeFrame, SkyFrame)`, so they carry that round trip's `atan2` / `asin` pair. Each licence names the fixture, the card, *both* values and a bound on the difference between them, and requires exactly one line to drop from each side — `rigby.simp` carries a second `IM5`, holding an exact zero, that a card-name-only rule would have covered too |
| simplify regression | 471 | 5 | `ABANDONED_PROBE_STAMP_LICENSED` — one reference-only `IsSimp = 1` line each in `rigby`, `cap_specmap_02`, `cap_cmpmap_nested_tag`, `cap_cmpmap_01` and `cap_cmpmap_04`. C stamped a sub-Mapping inside a merge probe it then abandoned: `CombineMaps` assembles the trial CmpMaps by cloning (`cmpmap.c:626-627`), so a trial holds the very sub-Mappings the live tree holds, the recursion stamps whatever `astSimplify` returns (`cmpmap.c:1443`), and discarding the probe (`cmpmap.c:1621-1628`) leaves the mark behind. The neighbouring parallel-in-series probe guards against exactly this with `SeparateMappings` (`cmpmap.c:1650-1651`); this one does not. Owned values give each tree its own children, so no probe in the port can leave a mark that outlives it (`docs/issues/c-library-quirks.md`, "An `IsSimp` card can record a merge probe C threw away"). Spent positionally — a line counts only where the port's line is the reference's *next* line — one per fixture, required to be found, with every other line compared |
| simplify regression | 471 | 2 | `INVAB_ALIASING_LICENSED` — one node in each of `cap_cmpmap_01` and `cap_cmpmap_04` whose dump carries `Invert = 1` in C and `IsSimp = 1` in the port. C's `CombineMaps` records a shared child's `Invert` on the CmpMap it builds, so the pair the dump carries is reference-count history rather than a property of the Mapping (`docs/issues/c-library-quirks.md`, "CmpMap dumps InvA/InvB disagreeing with the child's own Invert"); the port holds components by value and has no such history. The two cards are consumed as a pair at one position, so neither can go missing elsewhere unnoticed. This is what retired both fixtures' `skip_string_compare`, which had been hiding the whole 350-line file. Preceded by `ISSIMP_PLACEMENT_LICENSED`, which read `cap_specmap_02`'s missing card as a CmpMap the port never forms (§10.3): the port forms it byte for byte, and the card was the abandoned probe's |
| simplify regression | 471 | 3 | `ISSIMP_DISAGREEMENTS` — fixtures whose `IsSimp` card count still differs from C. Three under-tag by exactly one card, and that card is one C wrote during a merge probe it then abandoned: the probe's CmpMaps are assembled by cloning, so they share their sub-Mappings with the live tree and the stamp outlives the probe (`docs/issues/c-library-quirks.md`, "An `IsSimp` card can record a merge probe C threw away"). Owned values give each tree its own children, so the port cannot reproduce it — `rigby`, `cap_specmap_02` and `cap_cmpmap_nested_tag`, the last shrunk from `cap_specmap_02` to isolate the signature. Retired with this cluster: `cap_zoommap_04`, which over-tagged by three because its uncertainty simplification was being discarded (§10.23); `lsst`, when `PolyMap`'s linear branch began composing its MatrixMap and ShiftMap and handing the pair to `astSimplify` as C does, and `cap_cmpmap_01` / `cap_cmpmap_04`, when `CmpMap::map_merge` began adopting the object its own self-simplify returns unchanged — C's `new == map`, which carries the recursion's descendant stamps forward (§10.3). Retired earlier: `cefit_circle_conformal` and `cefit_circle_nonconformal_keep` when `from_node` began routing through the setters that discard the record, as C's loaders do (§9.2); the three `cmpmap_perm_swap*` fixtures when the swap branch stopped simplifying the components C hands on as they stand; `cap_cmpmap_03` when the same branch stopped simplifying the parallel CmpMap it reassembles; and `parpair_one_side_reduces` when it stopped simplifying a dimension-matched sublist's members (all §10.3). The byte comparison licenses all three through `ABANDONED_PROBE_STAMP_LICENSED`, so the count is their only gate. Listed because otherwise the number is invisible, and asserted as a set in `test_simplify_regression.rs` so it cannot grow. A stale reference is ruled out per fixture by `tools/oracle_fixtures/check_simplify_fresh.sh`, which recaptures the corpus from the pinned C |
| simplify regression | 471 | 144 | `ignore_is_simp_line` for the negative fixtures, whose reference *is* their input and so cannot carry the tag `simplify` always stamps. The companion assertion requires a top-level card exactly when the result is not Frame-kind — C's `IsSimple` getter is overridden on the Frame class to return zero (`frame.c:5590`) — so the strip can hide neither an untagged Mapping result nor a wrongly tagged Region. 134 with `neg_time_mjdtojd_real` (§10.9), `neg_unit_parallel_forward` (§10.6) `neg_parpair_aligned_irreducible` (§10.14), `neg_lut_near_linear` (§10.8), and five upstream repaired at pin `8d4f526f`. Was 141 before `cap_box_permmap_nonbidi`, the negative control for a PermMap whose permutation is not bi-directional (§10.13), and 142 before `neg_wcsmap_perm_allconst` and `neg_wcsmap_perm_both_sides`, the two controls on C's WcsMap swap decision (§10.31) |
| simplify regression | 471 | 0 | `SERIAL_UNGATED` — empty. Every fixture's simplified dump is byte-compared against C's `.simp` in full, and any difference fails. The table survives so a newly diagnosed C-side artefact has a home, and the harness asserts the set exactly, so a row cannot appear without a diagnosis. All seven causes enumerated in the table below are closed: A (SwitchMap `IsSimp` on FSMap / ISMap), B (a card on a Mapping inside a Region's FrameSet, §9.2), C (`nullregion_unc_fillfactor_merge`'s `Prism` uncertainty), D (`cap_zoommap_04`'s `FrmSet` block, §10.23), E (`cap_interval_01`'s corner point, §10.25), F (`cap_pointlist_01`'s uncertainty corner, §10.26) and G (`parpair_one_side_reduces`'s extra `IsSimp` cards, §10.3) |
| simplify round-trip (`test_serialisation.rs`) | 794 | 0 | **No relaxation exists.** Every `.map` / `.simp` in the corpus is read, written and byte-compared against C's own round-trip, 43 of them against a `.rt` reference captured from the oracle where C is not idempotent, and any difference fails. Was 4 under `ROUNDTRIP_DISCARD_LICENSED`, two GrismMap and two PcdMap fixtures whose reference drops an `IsSimp` card the port kept: C's loaders re-apply each attribute their `Test*` predicate reports set through the private setter, which discards the record the parent loader installed from the card, and the port now does the same for GrismMap's eight attributes, `LutMap::LutEpsilon` and `PcdMap::Disco` (§9.2) |
| transform oracle | 1752 sections | 0 | all three `*_ORACLE_EXCLUDED` lists are empty |
| transform oracle | 1752 sections | 0 | `transform_oracle_overrides.txt` declares no overrides, and a test asserts the file stays empty |
| transform oracle | 25 rows | 2 | `RTRIP_REFERENCE_GAPS_LICENSED` — rows where C's own recorded inverse does not recover the pixel either: `tnx-cheb.head` (all 24 rows, a ChebyMap has no usable inverse) and `tsc.head` row 10 (a 360 deg alias). Asserted as an exact set against the corpus with no fixture loaded, and reached only after the live result has failed both the direct and the aliased comparison |
| transform oracle | 2 rows | 2 | `FMA_SUBSTITUTIONS_LICENSED` — two rows of one UnitNormMap inverse section where the generator host's compiler fused the multiply-add at `unitnormmap.c:1079` and stepped past C's own zero-norm branch; the expectation is the unfused C value, asserted exactly |
| frameset round-trip | 139 | 0 | `FRAMESET_FIXTURE_EXCLUDED` is empty — every wcsconv object round-trips byte-exact |
| deferred fixtures | — | 3 | `DEFERRED_CLASSES` — `IntraMap`, `XphMap`, `StcResourceProfile` are unported classes, not gates that could otherwise pass (§5) |
| Frame in a Mapping slot (`test_frame_in_map_slot.rs`) | 1 | 1 | `FRAME_IN_MAP_SLOT_UNDECIDED` — one C-written dump the reader refuses, a bare Frame in a `Map<n>` slot. Whether to support the construct is an open design question, stated with its artifact in `docs/issues/frame-in-a-mapping-slot.md` (§9.6). The exclusion carries the reason string, and two companion tests keep the refusal narrow: the settled Region-in-`CmpMap` and Region-in-`CmpFrame` nestings must keep reading |

What the upstream `IsSimp` fix did and did not do, since the two get conflated:
`e40e71b4` made **C's** placement deterministic — a property of the Mapping tree
rather than of which nodes some earlier probe happened to invert. Before it, no
structural rule could reproduce C's answer, so the differences were unfixable in
principle and were licensed as such. After it, the answer is a well-defined
target the port can be held to, and the six fixtures above are ordinary port
bugs against that target rather than C artifacts. The fix changed them from
"cannot be matched" to "not yet matched"; it did not match them.

Movement at the `f6762005` sync: the wcsconv tag licence went from 5 cases to
20, but narrowed from "drop every `IsSimp` and `Invert` line, unpaired" to "drop
the `IsSimp` lines with both counts pinned" — `Invert` is byte-compared again,
and the residue is one diagnosed card per plane rather than an undiagnosed
count mismatch. Refreshing the corpus against that same pin then took the
licence from 24 rows to 4: 20 of the 24 were pinned against references the
pinned C no longer reproduced, and the port already matched C on them. Only the
placement residue itself remains. The table row read 20 while the
test-enforced ledger read 24; the ledger was right. `skip_string_compare` went from 7 cases to 5:
`cap_matrixmap_03` became byte-exact outright, and `cap_cmpmap_03` /
`cap_specmap_02` moved to the counted licence (`cap_cmpmap_03` has since left it
altogether — §10.3). The negative-fixture row went from 132 to
131 when `neg_unitnormmap_nonunit_scale` was found to assert something C does not do —
C replaces its zero-shift WinMap with a diagonal MatrixMap — and was given a real
oracle reference instead of naming its own input. The `ignore_is_simp_line` row and
the empty rows are newly *listed* rather than newly added — they predate the
sync and were simply not written down, which is what the executable ledger now
prevents.

Movement at the simplify parity closeout: the corpus grew and the relaxations
did not. The simplify manifest went from 460 cases to 468 and the transform
oracle from 1614 sections to 1752, with every `*_ORACLE_EXCLUDED` list still
empty; the `ignore_is_simp_line` count moved with the negative fixtures added
alongside them. The ChebyMap linear-reduction divergence recorded in that pass
is not a row here: the port declines a C step that loses a ChebyMap's domain
scaling, and the two fixtures that would pin C's answer are kept out of the
corpus rather than compared under a loosened gate — `chebymap.md` carries it as
a `divergence:` verdict citing `docs/issues/c-library-quirks.md`, which is where
work refused rather than work owed belongs. The Polygon axis-1 guard asymmetry
from the same pass is a faithful mirror, logged in the same document, and
relaxes nothing.

Known hole, not a relaxation: `native_round_trip_fixture`'s `trim()` /
`split_whitespace()` comparison — see the open row in §5.

---

## 7. Delivery-step work not yet started

Tracked here because these are the largest outstanding items in the project and
were previously indexed nowhere. The first row is the design's stated purpose.

| Area | State | Blocks |
|---|---|---|
| `ast-capi` C-ABI shim | UNSTARTED — `crates/ast-capi/src/lib.rs` is a one-line comment; 0 exported symbols, no `ast.h`, no `astClone`/`astAnnul` refcounting, no `astBegin`/`astEnd` handle stack, no thread-local status, no variadic `astSet` parsing, no `catch_unwind` boundary, and neither of the two CI header-drift checks exists | tiers 3 and 4 below; the drop-in-replacement goal (pyast / Starlink / DS9 linking unchanged) |
| `ast-capi` immutability enforcement (`AST__IMMUT`) | OPEN, design decision — see the note below | `ast-capi`; drop-in compatibility for callers that mutate a shared Mapping |
| Tier 3 — `ast-ffi-tests` | UNSTARTED — crate body is `#![allow(missing_docs)]`. Would run the real C `ast_tester` programs against the Rust `libast` | blocked on `ast-capi` |
| Tier 4 — `ast-oracle-tests` | UNSTARTED — same. Would link `ast-core` + reference `libast.a` for live bit/ULP comparison. **No test in the repo currently executes C code and compares live**; all C comparison is against frozen committed corpora | blocked on `ast-capi` |
| Tier 5 — `ast-oracle-bench` | UNSTARTED — 6-line criterion placeholder; see the perf row in §2 | — |
| `ast-plugins` (Grf / Grf3d backends) | UNSTARTED — one-line stub | `Plot` / `Plot3D`, and the `RegTrace` / `RegSplit` region deferrals in §1 |
| wcslib Rust port (step 6a) | UNSTARTED — `vendor/wcslib` (3,327 code lines) still compiled and linked, alongside ERFA (16,626) and PAL (1,386 + a 234-line `palwrap.c`) | — |

### `AST__IMMUT` at the shim boundary

C enforces, in every class as of the pending `u/timj/misc-bugs` merge, that a
Mapping which has been cloned or included in another Object may not have its
attributes changed — `Invert` excepted. `ast.news` files this under "may break
existing code": callers that previously changed `PolyMap.IterInverse`,
`MathMap.Seed` or `SplineMap.InvTol` through a shared pointer succeeded
silently and now get `AST__IMMUT`.

**None of this can arise in `ast-core`**, for three independent reasons: `Clone`
on `Box<dyn Mapping>` is a deep copy that recurses through compound classes;
`DynMapping` is single-owner and compound constructors consume their arguments;
and component accessors are `pub(crate)`, so a component is unreachable from
outside the crate. `ErrorCode::Immut` is generated from `ast_err.msg` and is
correctly unused. The rationale is recorded in `crates/ast-core/src/error.rs`'s
module docs so the unused variant is not mistaken for an omission.

**`ast-capi` restores the hazard by design** and has to decide how to police it.
The spec commits to `astClone` = `Arc::clone` sharing one `RefCell<AnyObject>`
across handles (design spec, "Handle registry in the owned-value world"), and
says nothing about immutability — so this is a gap in the shim design, not
deferred work.

The two halves are not equally hard:

- **"has been cloned"** is tractable: `Arc::strong_count() > 1` is a natural
  test at the point of mutation.
- **"included in another Object"** has no Rust equivalent, because inclusion
  *consumes* the object. When a C caller runs `astCmpMap(a, b)`, none of the
  three obvious options is right: deep-copying into the CmpMap leaves the
  caller's handles valid and independently mutable, which is what C forbids;
  moving makes the handles dangle, breaking the ABI; and doing nothing loses
  the error C now raises. The shim therefore needs to keep the `Arc`s alive and
  shared *and* carry an immutability flag per slot — state `ast-core` has no
  concept of, which means it belongs in the registry rather than in the object.

Deciding this before the shim is written is much cheaper than retrofitting it,
because it constrains the registry's slot representation.

Absent classes, for completeness (52,168 code lines of C, 28% of the library by
code volume, with no Rust counterpart): `Plot` / `Plot3D` + `grf*`, `YamlChan`,
`XmlChan`, `Moc` / `MocChan`, `StcsChan`, all `Stc*` regions, `Table` /
`FitsTable`, `IntraMap`, `XphMap`. Absent `Mapping`-level algorithms: `astResample`,
`astRebin`, `astRebinSeq`, `astTranGrid`, `astQuadApprox`. Of these only
`Table` / `FitsTable` (§3, §5) and `IntraMap` / `XphMap` (§5 deferred fixtures)
currently gate active work.

## 8. Upstream sync at `f6762005` — done

Landed in `2efde3a`, `e0cb246`, `e029927` and `3a0a9d9`: nineteen upstream defect
fixes reconciled against `docs/issues/c-library-quirks.md`, five mirrored bugs
removed, 97 fixtures copied across byte-identical, and one divergence recorded
rather than closed (`IsSimp` card placement in a FITS-WCS read). The full account
is in [`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).

Two rules came out of it and hold for the next pin move:

- Before moving the pin, diff the incoming range for serialization changes and
  check `docs/issues/c-library-quirks.md` for entries the port *mirrors*. Four of
  the five mirrors found this way were invisible to every existing test, because
  the fixtures had been generated by the buggy C.
- Freshness must be checked on every corpus the pin feeds, not just the ones a
  test names. `tools/oracle_fixtures/check_simplify_fresh.sh` covers the simplify
  corpus and must be run alongside `check_corpus_fresh.sh` after any pin move;
  before it existed, two stale references were read as port defects.

The sync that resolves §9.4 and unblocks §10.2's linear half is still to be
planned: upstream has reorganised `ast_tester`, so the pin move needs a plan of
its own.

## C → Rust test migration tracker

| C test (`ast_tester/`) | Rust counterpart | Status |
|---|---|---|
| testconvert.c | crates/ast-core/tests/test_convert.rs | ported 2026-06-12 |
| testframe.c | (deferred) | NormUnit(1) via capi units layer; orthogonal to the Frame-base port |
| check_transform_oracle | crates/ast-core/tests/test_transform_oracle.rs | ported 2026-08-05 (submodule `3a3c3e51`); consumes `tests/fixtures/oracle/*.oracle`; all 1752 sections gated by three asserting tests (`head_`, `slamap_`, `simplify_sections_match_oracle`), with `every_oracle_section_is_gated` proving the partition covers the corpus; `transform_oracle_report` remains `#[ignore]` as the investigative view; `gen_transform_oracle` intentionally not ported |
| testkeymap.c | crates/ast-core/tests/test_keymap.rs | ported 2026-08-08; the nine functions added upstream at `f6762005` ported 2026-08-20 |
| testmapping.c | crates/ast-core/tests/test_mapping.rs | partially ported 2026-08-20 — the four `IsSimple` functions added upstream at `f6762005`. `testselfsimplifyafterset` is unconstructible here (ZoomMap's `Zoom` and PcdMap's `Disco` have no `&mut self` setter). The file's original PolyMap / LinearApprox body, converted from `testmapping.f`, is covered by `test_polymap.rs` and `test_polytran.rs` |
| testfitschan.c | crates/ast-core/tests/test_fitschan*.rs | partially ported — `testgrismorder`, added upstream at `f6762005`, is in `test_fitschan_wcs_write.rs` (2026-08-20); the rest of the file is spread across the seven `test_fitschan_*` files |
| testimmutable.c | (deferred) | tests `AST__IMMUT`, which guards a change made to a Mapping whose reference count exceeds one. Owned values, deep `Clone` and single-owner `Box` make that condition unconstructible in `ast-core`; the guard belongs to `ast-capi`, which restores sharing at the handle boundary and is unstarted (§7) |
| gen_simplify_fixtures / check_simplify | crates/ast-core/tests/test_simplify_regression.rs | ported; 471 manifest cases against the pinned oracle's `.map` / `.simp` dumps, byte-exact with no skip mode, plus `test_serialisation`'s 794-file round trip over the same corpus. `gen_simplify_fixtures` itself stays in C: it writes the references |
| (no C ancestor) | crates/ast-core/tests/test_frame_in_map_slot.rs | added 2026-09-03; pins the reader's refusal of a C-written bare Frame in a `Map<n>` slot, with the settled Region nestings as controls (§9.6) |
| testobjectkeymap.c | (deferred) | tests the Object-associated retained KeyMap (`object.c:1736`, `:2480`), `astGetObjSize` and `astSame`; those calls (`astMapPut0I`, `astMapGet0I`, `astMapSize`) are only reachable through the protected `astGetKeyMap`/`astHasKeyMap` machinery, an explicit spec Non-goal of the owned-value port |

## 9. Findings from the fixture regeneration

The `.simp` corpus was stale relative to the submodule pin. Regenerating it
against the pinned C closed two disagreements and opened two questions.
Only §9.4 and §9.6 remain open, and neither is a simplifier gap: §9.4 waits on
the upstream fixture sync and §9.6 on a maintainer decision.
What the gates hold today is stated with its evidence in
[`docs/reports/2026-09-03-simplify-parity-final.md`](docs/reports/2026-09-03-simplify-parity-final.md).

Closed entries keep their numbers, and each one's write-up is in
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).

| § | Finding | Resolution |
|---|---|---|
| 9.0 | The simplify and Region harnesses each hand-rolled a `&dyn Mapping` -> `AnyObject` dispatch that returned `None` for whole classes, from inside the `if let` that guarded the byte comparison | CLOSED — one shared dispatch; the eleven failures it had been hiding were exposed and fixed |
| 9.1 | `cefit_circle_*` emitted an `IsSimp` card on an inner Mapping that the pinned C does not | CLOSED — C's loader discards the card, so the two dumps describe the same Mapping |
| 9.2 | C's clear-on-set convention: a setter for an attribute feeding the first part of its own `astMapMerge` discards both simplified records | PORTED for every attribute the port can mutate, and gated; the five attributes named in §10's open list have no setter to attach it to yet |
| 9.3 | 24 of the 109 NATIVE-family wcsconv references looked stale against the new pin | DONE — replayed against the pinned C with each row's `attrs`; no keyword sequence differed in any of the 109 and the residue is cross-platform capture drift |
| 9.5 | Audit checklist citations were not anchored to a pin, so a line number could rot unnoticed | CLOSED — all 37 checklists in `docs/audits/mapmerge/` declare pin `3e60abaa`, held by six gates in `test_audit_citations.rs` |

### 9.4 51 simplify fixture files have no transform-oracle section — OPEN, resolves on the upstream sync

Every section the generator can produce for a fixture that exists in
`reference/starlink-ast/ast_tester/simplify_fixtures/` is now committed, so of
the 790 `.map` / `.simp` files in
`crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`, 51 carry no
oracle section, for two reasons.

**Nine cannot have one.** `cap_matrixmap_04.map`, `cap_specmap_02.map` and
`.simp`, `neg_ratemap_different_inner.map`, `neg_ratemap_diff_indices.map`,
`neg_ratemap_diff_inner.map`, `ratemap_inverse_cancel.map`,
`selectormap_inverse_cancel.map` and `selectormap_inverse_cancel_tail.map` are
CmpMaps whose dump carries `Fwd = 0` and `Inv = 0`: neither transformation is
defined, so `astTran` has nothing to compute and `gen_transform_oracle` emits
nothing (`gen_transform_oracle.c:210-211` reads `TranForward` / `TranInverse` and
skips each direction that is absent). These fixtures exist to pin structure —
what the simplifier does or declines to do to the tree — and that is what
`test_simplify_regression.rs` compares.

**Forty-two are Rust-only fixtures the generator never sees.** Twenty-two stems
live in the Rust fixture tree but were never copied into the submodule's
`ast_tester/simplify_fixtures/`: `cefit_circle_conformal`,
`cefit_circle_nonconformal_keep`, `cefit_circle_to_ellipse`,
`cefit_ellipse_to_circle`, `defunc_cmpregion_component_unc`,
`defunc_interval_unbounded`, `defunc_nullregion_2d`, `defunc_prism_box_interval`,
`merge_box_fillfactor_product`, `merge_box_meshsize_scaled`,
`merge_interval_meshsize_scaled`, `neg_cmpmap_neighbour_not_cmpmap`,
`neg_win_series_cmpmap_neighbour`, `regbasepick_circle_permaxes`,
`regbasepick_cmpregion_subset`, `regbasepick_prism_subset`,
`setunc_box_remapped_unc`, `setunc_unbounded_negated`,
`setunc_unitmap_regionfs_omitted`, `simpstab_box_linear_unc`,
`simpstab_box_nonlinear_unc` and `simpstab_permmap_slice_unc`. Twenty of the
twenty-two are Region `Simplify` fixtures; the two `neg_` entries are CmpMap
negatives. Closing this bucket means copying them into the submodule fixture
directory, committing there and moving the pin, which belongs with §8's pending
upstream sync rather than to a corpus capture.

This is not a hole in the transform-oracle gates: `resolves_every_oracle_relpath`
asserts every oracle *section* resolves to a fixture, and all 1752 that exist are
gated with empty exclusion lists. Each of these fixtures is compared for
serialisation byte-exactness and `IsSimp` placement by
`test_simplify_regression.rs`; what is missing is the independent check that the
simplified Mapping *transforms* points the way C's does.

Sections are captured from the oracle, never by regenerating the file: a full
regeneration also imports `cheby_linear_lone_reduce` and
`neg_cheby_quadratic_no_reduce`, which reproduce the C defect recorded in
`docs/issues/c-library-quirks.md` and are deliberately out of the port's corpus
(§10.2), and rewrites every value in `headers.oracle` and `framesets.oracle` in
the last digit or two — platform drift, which the corpus-freshness protocol says
to leave alone.

The submodule's own copy, `reference/starlink-ast/ast_tester/simplify_fixtures.oracle`,
holds 1399 sections and is tracked and clean at the pinned commit, so the two
trees are not byte-identical and cannot be made so without committing inside the
submodule and moving the pin. Nothing in the Rust test tree reads that copy —
`test_transform_oracle.rs` reads only
`crates/ast-core/tests/fixtures/oracle/`, and the submodule copy feeds the C-side
`check_transform_oracle`. Bringing it forward belongs with §8's pending sync,
alongside the Rust-only fixtures above.

### 9.6 A bare Frame where a Mapping is expected — OPEN, awaiting a maintainer decision

C accepts a Frame in any Mapping slot, because `Frame::Transform`
(`frame.c:11739`) transforms through a UnitMap (`frame.c:11806`).
This port makes Region the only Frame that is also a Mapping, so a `Map<n>` slot
holding `Begin Frame` or `Begin CmpFrame` is refused.

The narrow question — whether C's acceptance is a side effect of having made
Frame a Mapping in order to support Region, or answers a real use case — is
written up with its C citations, the cost of each answer, and the evidence
available in
[`docs/issues/frame-in-a-mapping-slot.md`](docs/issues/frame-in-a-mapping-slot.md).

The artifact is `crates/ast-core/tests/fixtures/frame_in_map_slot.ast`, a dump
the C library wrote, reproducible with
`tools/oracle_fixtures/frame_in_map_slot_probe.c`.
`crates/ast-core/tests/test_frame_in_map_slot.rs` pins the refusal and keeps it
narrow: the 33 Region-in-`CmpMap` nestings in the fixture corpus and the
Region-in-`CmpFrame` dump the same probe wrote must keep reading, so the gap
cannot be closed by refusing Frames in Mapping slots wholesale.

`C_SIMPLIFY_SEAMS` records `cmpframe.c` as
`undecided: docs/issues/frame-in-a-mapping-slot.md`, and requires that document
to state the question and name the fixture; `FRAME_IN_MAP_SLOT_UNDECIDED` counts
the one fixture in §6.

`CmpFrame::Simplify` (`cmpframe.c:8963-9050`, installed at `:4745`) is the seam
the decision governs. It calls `astSimplify` on each component Frame and, if
either returned a different object and both are still Frames, returns a copy of
the CmpFrame holding the simplified components.
`crates/ast-core/src/frames/cmpframe.rs` has no counterpart and cannot have one
while `CmpFrame` is not a `Mapping`, because the operation C spells
`astSimplify( cmpframe )` is not expressible through the Rust API.
Reachability is narrow but real: only `region.c`, `frameset.c`, `stc.c` and
`cmpframe.c` override the Frame-level `Simplify` slot, so simplifying a component
Frame changes something only when that component is a Region, a FrameSet, an Stc
or a nested CmpFrame containing one.
Nothing inside AST reaches the body except itself — `astSimplify` is applied to a
Frame-typed argument only at `cmpframe.c:9019-9020`, and `FrameSet::Simplify`
reduces the inter-node Mappings without touching the Frames
(`frameset.c:10195`) — so the entry point is a caller's own `astSimplify` on a
CmpFrame, which is why no fixture in the corpus exercises it and no result in the
corpus is wrong because of it.

What made this invisible is worth more than the gap itself: `audit_universe` in
`test_mapmerge_audit.rs` was derived from the Rust module tree, so a C seam with
no Rust module produced no checklist to be missing, and
`every_audited_class_has_a_checklist` passed. The gate now takes the C source as
the authority on which classes have a seam: `C_SIMPLIFY_SEAMS` lists all 41 C
files that install a `MapMerge` or `Simplify` vtab entry with what covers each,
and when the submodule is present the list is re-derived from
`reference/starlink-ast/src/*.c` and required to match, so a pin move that adds
a seam fails there rather than passing unnoticed.

## 10. Gaps named by the `MapMerge` / `Simplify` audit

Each entry was cited by a `gap` verdict in a checklist under
`docs/audits/mapmerge/`. Every one is closed, so no row cites this section any
more and the `gap:` verdict form has been withdrawn; `no_row_is_left_at_gap` and
`every_checklist_row_has_a_verdict` together keep it that way.

Closed entries keep their numbers, and each one's write-up is in
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).
Two carry an open remainder and stay here in full: §10.2's linear ChebyMap
replacement and §10.22's `normalize_unitmap_children`.

What is left open anywhere in this section:

- **§9.2, for five attributes with no setter** — `ZoomMap::Zoom`,
  `LutMap::LutEpsilon`, `PcdMap::Disco`, `IntraMap::IntraFlag` and GrismMap's
  macro-form attributes. The convention is ported for every attribute the port
  can mutate; these have nothing to attach it to yet.
- **§10.2's linear half, as a `divergence:`** — C's linear reduction loses a
  ChebyMap's domain scaling, so the port declines the step and the two fixtures
  that would pin C's answer stay out of the corpus.

| § | Gap | Resolution |
|---|---|---|
| 10.1 | RateMap's self-reduction sat in `simplified()`, unreachable from the nominate loop | FIXED — `map_merge` uses `self_reduce_map_merge`, which is C's order: reduce first, then cancel against an inverse neighbour |
| 10.3 | Three divergences in the port's handling of C's parallel-in-series branch | FIXED — rows 22, 23 and 26 of `cmpmap.md` are `ported`, and two fixtures came byte-exact with the change |
| 10.4 | The CmpMap/PermMap swap was hung off the PermMap, where C hangs it off the CmpMap | FIXED — it runs when the CmpMap is nominated, as the last block of its `map_merge` |
| 10.6 | A parallel UnitMap's Invert flag looked unported | RETRACTED — C clears the list's applied direction, not the object's attribute, which the port does elsewhere |
| 10.7 | PcdMap's zero-distortion reduction was unreachable from the nominate loop | FIXED — same shape as §10.1, moved into `pcd_map_merge` ahead of the neighbour tests |
| 10.8 | LutMap's linearity test used a hardcoded tolerance with no analogue in C | FIXED — scaled by `LutEpsilon` and taken as the larger of C's two estimates |
| 10.9 | TimeMap dropped one no-effect change-of-system step where C drops three | FIXED — the derived third argument is tested for `MJDTOMJD`, `MJDTOJD` and `JDTOMJD` alike |
| 10.10 | SwitchMap left its simplified children untagged | FIXED — the tag is raised where C raises it, confirmed against a C trace of the three nominations |
| 10.11 | `TranMap::map_merge` reached one of C's four self-simplification outcomes | FIXED — all four live in `map_merge`, in C's order |
| 10.12 | `GetDefUnc`'s unbounded-axis guard caught an unbounded lower bound but not an upper one | FIXED — both bounds tested against `±DBL_MAX` as C does |
| 10.13 | Box's PermMap simplification branch was unported | FIXED — the permutation arrays and constants are read directly and the current-Frame Region built axis by axis |
| 10.14 | Three claims that the port over-reaches C's nominate loop | RETRACTED — a trace of the C library's three dispatchers disproved all of them |
| 10.15 | Nine `C-ARTIFACT` sites said to exist only to reproduce C's memory model | RESOLVED — four mechanisms push an inverted CmpMap's inversion into its structure, three were redundant and are gone, one is load-bearing and measured |
| 10.16 | The port had a per-class `simplified()` seam C does not have | FIXED — collapsed onto C's two seams, `Simplify` and the repeated `astMapMerge` of the base implementation |
| 10.17 | NormMap did not simplify its encapsulated Frame | FIXED — in C's order: simplify the Frame first, then the basic-Frame test, then the neighbours |
| 10.18 | Polygon's vertex-transform simplification skipped both of C's guards | FIXED — both guards ported, `SimpVertices` added, and the uncertainty passed in the Frame the new vertices live in |
| 10.19 | A recursion-depth cap of ten and a structural hash standing in for `astEqual`, neither with a counterpart in C | CLOSED — both removed, and what C does instead is gated |
| 10.20 | The restricted-simplify path dropped three of C's steps | FIXED — all three ported, each with a fixture pair that fails without it |
| 10.21 | `astDoNotSimplify` and the Mapping-level `Ident` it reads were unported | FIXED — all three generic-driver call sites honour it |
| 10.23 | The uncertainty simplification asked `Mapping::equal` where C asks pointer identity | FIXED — any reduction counts, including one that rewrites only the structure |
| 10.24 | A merged Region's combined uncertainty went straight to the parallel CmpMap `Prism::Simplify` starts from | FIXED — the Prism's tail contributes, so the uncertainty is built as C builds it |
| 10.25 | A bounded Interval was reclassified to a Box without simplifying it | FIXED — C's `Cache` builds the base-Frame Box and the result is simplified |
| 10.26 | `MergePointList` installed its combined uncertainty by assignment | FIXED — installed through `set_unc`, as C's shared merge tail does |
| 10.27 | PcdMap's swap half was a sketch of C's | FIXED — five divergences closed, including the speculative swap with no merge target in reach |
| 10.28 | LutMap's linearity test and merge tail diverged from C's in four places | FIXED — nearest-neighbour tables refused up front, and the tail matches C |
| 10.29 | PolyMap's merge half, and a third `astDoNotSimplify` site in the shared dispatcher | FIXED — three divergences closed |
| 10.30 | ChebyMap's cancellation test was hand-rolled and never looked at the inverse terms | FIXED — it goes through the inherited equality, bounding boxes and both coefficient sets |
| 10.31 | WcsMap's swap driver was two attempts where C runs a search | FIXED — C's bidirectional PermMap walk, nearer-target choice and speculative swap are ported |

### 10.2 ChebyMap's linear replacement — DEFERRED

In C, `chebymap.c` defines no `MapMerge`; only `polymap.c:2439` fills the slot, so
a ChebyMap's `MapMerge` is PolyMap's whole body at `polymap.c:3597-3961`. The port
had no `map_merge` on `ChebyMap` at all and reached neither half of PolyMap's
replace-with-simpler branch. The coefficient consolidation half is ported and
gated by `cap_cheby_consolidate`. The linear half is deferred, and this is why.

The linear replacement (`polymap.c:3747-3875`) will not be ported. C reads the
coefficient arrays with nothing that distinguishes a Chebyshev basis from a power
basis, and accumulates a matrix and offset from them directly. A first-order
Chebyshev term is affine in the input, but only after the domain scaling the
ChebyMap's `lbnd`/`ubnd` define, and that scaling takes no part in what C
accumulates.

Settled on the oracle: C loses the scaling. A one-input ChebyMap over bounds
`[0,10]` holding the single term `2 * T_1` computes `2 * (0.2x - 1)`, and
`astSimplify` returns a ZoomMap with `Zoom = 2` — `x = 0, 1, 10` gives `0, 2, 20`
where the ChebyMap gives `-2, -1.6, 2`. The reduction is sound only for bounds of
`[-1,1]`, and the branch has no ChebyMap guard, so it is a defect rather than a
convention. It is recorded in `docs/issues/c-library-quirks.md` for upstream, and
the port does not mirror it: a linear ChebyMap is left alone whatever its bounds.
The reproduction lives in `ast_tester/gen_simplify_fixtures.c` as
`cheby_linear_lone_reduce` and its non-linear control, which stay out of the
port's simplify corpus because a positive fixture there asserts the port
reproduces the `.simp` byte for byte, which for this pair means reproducing the
wrong Mapping.

The sound half of that branch is still a port gap, and it is deferred rather than
closed. C's arithmetic is correct when the bounds are exactly `[-1,1]`: the
domain scaling is then the identity, so a linear ChebyMap over those bounds
reduces to the same MatrixMap and ShiftMap a PolyMap would. The port does not
reduce it. `ChebyMap::map_merge` carries an `UNPORTED` marker at the point the
branch would go, and the class's module doc names the gap, so `rg UNPORTED
crates/ast-core/src/mappings/chebymap.rs` finds it.

Three things have to happen together, which is why it waits. The general case
needs the C defect fixed at source — guard `polymap.c:3747-3875` against a
Chebyshev basis, or give `chebymap.c` its own `MapMerge` — and that is an
upstream change, not a port change. The `[-1,1]` case needs
`cheby_linear_lone_reduce` and `neg_cheby_quadratic_no_reduce` inside the corpus
to be falsifiable, and they live in the submodule's
`ast_tester/gen_simplify_fixtures.c`. Both routes therefore run through a
reference-pin move, and upstream has since reorganised `ast_tester`, so the pin
move needs planning of its own (§8). Until that lands, a linear ChebyMap is left
alone whatever its bounds — never reduced wrongly, and, for `[-1,1]`, not reduced
at all.

### 10.22 The public entry performs one tree rewrite C's `astSimplify_` does not

`mapping.c:24755-24808` adds no algorithm: an `IsSimple` short-circuit, the
`Simplify` call, the stamp, and the two flag clears of §10.20.
`<dyn Mapping>::simplify` (`simplify_entry.rs:36-82`) also rewrites the result
tree once before returning it.

`normalize_unitmap_children` (`cmpmap.rs:223`, called at `simplify_entry.rs:70`)
drops redundant `InvA`/`InvB` flags on a parallel CmpMap whose corresponding
child is a UnitMap. It is load-bearing and measured: §10.15 records four
`equal()` failures and two serialisation failures without it, and it is one of
the two layers that reproduce C's inversion push-down. The section stays open for
it.

A second rewrite, `narrow_unit_scale_winmap_children`, was measured dead and
deleted; the account is in
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).
