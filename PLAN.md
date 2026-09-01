# PLAN.md — Rust AST port: outstanding-work clearing house

Ticket DM-54793. This file is the single index of active, audited, outstanding
work in the Rust port. When a gap is found and written up, link it here so the
write-up is discoverable and does not go stale in isolation. When work lands,
update the status column (and, when a whole area closes, strike it through with
the closing commit).

Convention: detailed analysis lives in `docs/audits/` (structured gap sweeps) or
`docs/issues/` (per-subsystem port-gap catalogues). This file holds only the
one-line pointer and current status for each.

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

## 2. Frames and Mappings

| Area | Doc | Status |
|---|---|---|
| Frame + Mapping class C-port gaps (function-by-function) | `docs/issues/frame-mapping-c-port-gaps.md` | OPEN — audit COMPLETE; remaining items are deferred delivery-step work only (Rebin/Resample/TranGrid/QuadApprox, JSON/XML/YAML renderers, absent classes, wcslib port 6a, Plot-support Frame methods + NormalPoints, SkyAxis Format precision) |
| Per-class Mapping performance vs C oracle | `docs/issues/mapping-perf-vs-c-oracle.md` | OPEN — **no automated perf gate exists**; `ast-oracle-bench` is a placeholder and CI runs no benchmark, so the >10% hot-path gate CLAUDE.md describes is not enforced. Snapshot (2026-05-19) records cheap mappings 5–42× slower than C at N=100k (permmap-fwd 42×, winmap 6.6–7.6×, shiftmap 5.7–7.6×, diag matrixmap 5.5×, slamap 3.9×, wcsmap-tan-fwd 4.2×); PolyMap/MatrixMap — the SCUBA-2 / LSST path — remain at or ahead of parity. Untested hypothesis worth measuring before per-class work: `PointSet` is row-major `(npoint, ncoord)` where C keeps one contiguous array per axis, which would explain the whole cheap-mapping cluster at once |
| `astTran*` definedness validation | — | OPEN — `Mapping::tran1`/`tran2` raise `AST__TRNND` for an undefined direction (C `ValidateMapping`, mapping.c:179), but the general `transform` / `transform_into` path raises it nowhere: `Trnnd` appeared in no source file before `156cf68`. Deciding which classes should report it needs a C-oracle probe per class, since C validates in the public `astTran*` entry points rather than in `astTransform` |
| CmpMap `inv_a`/`inv_b`: split the load contract from the dump-shaping emulation | — | DEFERRED to post-oracle-compliance — measured, with a falsifiable go/no-go test; see the note below |
| TimeFrame port + Epoch/Equinox API | `docs/issues/timeframe-and-epoch-api-port.md` | OPEN (residual) — Issue A (DATE-OBS/TDB drift) RESOLVED; only Issue-B doc/hygiene items remain (Equinox=UTC note; `_year` setter public/private decision) |

### CmpMap orientation snapshots (`inv_a` / `inv_b`) — deferred cleanup

`CmpMap` stores each child's Invert flag at construction/load time and computes
each child's effective transform direction as `fwd XOR (inv_a != child.is_inverted())`,
through `first_invert_changed()` / `second_invert_changed()` at 15 call sites.
This mirrors C, where `CombineMaps` snapshots `invert1`/`invert2` and `Transform`
forces the child back to the recorded state before transforming — a defence
against another owner having inverted the shared child in the meantime.

The natural question is whether the whole mechanism is vestigial in an
owned-value port, since nothing outside the `CmpMap` can reach a child. It is
not, and the measurement matters more than the argument. Instrumenting the two
predicates and running the full suite with `--nocapture`:

| Probe | Result |
|---|---|
| Drift observed at transform time | 190,831 (first child) + 621,383 (second) |
| Test binaries exercising it | wcsconv 106,847; simplify regression 30,465; transform oracle 2,928; bench_lsst 2,852; serialisation 1,316; cmpmap 76 |
| **Loads that create drift** | **539** — dumps whose `InvA`/`InvB` disagrees with the child's own serialised `Invert` |

So the field is load-bearing, for a reason that survives the ownership change: a
C dump records **two independent bits** — the parent's snapshot and the child's
own flag — and 539 loads in the reference corpus carry them disagreeing. A
loaded `CmpMap` can therefore be in a state the port's own constructor cannot
produce (`CmpMap::new` always sets `inv_a = first.is_inverted()`, so drift is
false for anything the port builds itself). Reading those dumps correctly needs
both fields and the XOR.

What *is* separable is a second, distinct use of the same field. The simplifier
calls `set_first_orientation` / `set_second_orientation` (6 sites in
`simplify.rs` and `winmap_merge.rs`) to flip a child's stored Invert while
deliberately leaving `inv_a` untouched. Their only effect is to shape the dump
so it matches what C emits, which is C's aliasing bookkeeping reproduced on
purpose.

**No simplification rule depends on them — measured, not assumed.** Disabling
just the flip in both setters (keeping their `IsSimp` clear, so the two effects
stay separate) leaves the simplify regression harness reporting, over 427
fixtures:

```
0 equal() failures, 0 transform failures, 8 serialization failures
```

The harness checks structural equality against C's `.simp`, numerical transform
equivalence, and byte-exact dump independently, so the merges produce the same
mapping and only the serialised form drifts. Across the whole suite the damage
is 7 tests of 3294, every one a dump comparison: 5 wcsconv native byte diffs
(missing `INVERT_A= 1` cards), the simplify harness's 8 fixtures, and
`test_cmpmap`'s `parallel_with_inverted_cmpmap_child_canonicalises_for_serialisation`.

That follows from where the calls sit: merge rules never see a child inside a
CmpMap, because `flatten` decomposes the tree into a flat `(mapping, invert)`
list first and merging happens there. `set_*_orientation` runs during
`reconstruct`, after every simplification decision is made.

`invert_into_structure` was expected to be the load-bearing half, and measurement
says otherwise. Forcing the simplifier's `apply_effective` to always take the
cheap `m.invert()` flag toggle — never the structural rewrite — leaves the entire
suite green: 3294 passed, 0 failed, including all 194 wcsconv byte comparisons,
427 simplify fixtures at 0 serialisation failures, and 1,606 transform-oracle
sections. Instrumenting the branch instead shows it is reached **once** in the
whole suite, from one simplify fixture, in the `kind=Series, series=false` case.
Its `Parallel` arm — which the eleven-line comment above it is entirely about,
describing how C descends to the deepest parallel CmpMap wrapping leaves — is
never reached at all.

That is "unexercised", which is not the same as "wrong to have": the corpus
largely co-evolved with this port, so it may simply lack the shape the branch
defends against (the H10/H11 concern). But combined with the C-side reading it
is hard to see what the branch buys, and it should be either deleted or given
the fixture that justifies it.

`canonicalize_inverted_cmpmaps` and `normalize_unitmap_children` are untested
here and remain out of scope. This is the
`IsSimp` situation exactly (see H3 in `docs/porting-hazards.md`): one field
serving both a semantic role and a serialisation role. `IsSimp` was split into
`is_simple_serial` and `has_been_simplified`; Invert has not been split.

**A unification pathway (proposal).** The 30-odd touch points are a consequence
of *which* of two equivalent encodings the port chose, not of the format's
complexity. The format carries two bits per child:

- **E** — the effective direction, written as `InvA`/`InvB` on the parent.
- **S** — the orientation the child is serialised in, written as the child's own
  `Invert`.

*Encoding A (current).* Store the child in orientation **S**; keep **E** in
`inv_a`. Every transform then needs the effective direction recovered as
`fwd XOR (inv_a != child.is_inverted())` — which is exactly the shape all 16
call sites have. The artifact is threaded through the transform path, and six
further functions mutate children to keep S right for the dump.

*Encoding B (proposed).* Store the child in orientation **E**, so
`inv_a == child.is_inverted()` holds as an invariant and drift is structurally
impossible. Keep **S** in a dump-only field, e.g.
`dump_child_inverted: (bool, bool)`. Then:

| | Encoding A | Encoding B |
|---|---|---|
| Transform | `fwd != drift` at 16 sites | plain `fwd`; the two `*_invert_changed` helpers disappear |
| Semantic state | `inv_a`/`inv_b` fields | derived from the child; no separate field |
| Load | read `InvA`; leave child as-is | read `InvA`; invert the child to match; record S |
| Dump | write `inv_a`; child dumps itself | write `child.is_inverted()`; patch the child's node `Invert` to S |
| Dump shaping | 6 functions mutating children | set one field |

The whole artifact then lives in one field plus one line each in `to_node` and
`from_node`, instead of being spread across the transform path.

**Why this is worth doing regardless of upstream.** Under encoding A, retiring
the artifact means touching all 16 transform sites. Under encoding B it means
deleting one field and two lines. So B is both simpler now *and* the thing that
makes the eventual collapse trivial — it does not depend on C changing.

**Status: attempted; the decode step landed, the encoding change is blocked.**

`63782ac` did the part that stands alone: the sixteen inline decodes across
eleven functions are now two functions, `CmpMap::first_dir` / `second_dir`.
Behaviour-preserving, verified against every byte-exact corpus with no fixture
regeneration. That reduces the encoding change from a sixteen-site sweep to a
two-function edit.

**The blocker, found by attempting it.** Encoding B needs the child stored in
its *effective* orientation, so load must flip a child whose serialised
orientation disagrees. The only way to flip is `Mapping::invert`, and every
implementation deliberately clears the simplification tags:

```rust
fn invert(&mut self) {
    self.inverted = !self.inverted;
    // C astSetInvert clears IsSimple
    self.has_been_simplified = false;
}
```

So normalising at load would clear each child's `IsSimp` and change the dump —
byte-exactness lost. Avoiding that needs a tag-preserving flag setter, which
means a new method on `Mapping` implemented by **37 classes**, whose entire
purpose is to bypass a clear that exists to mirror `astSetInvert`.

**That is the real finding: the two artifacts are coupled.** The `InvA`/`InvB`
cleanup cannot land before the `IsSimp` artifact is resolved, because the
mechanism it needs is a hole in `IsSimp`'s own emulation. It is not a
coincidence that these two clusters were the port's most expensive area — each
one's workaround is load-bearing for the other's.

**Revised sequencing.** Do not attempt encoding B in isolation. Either:

1. Resolve `IsSimp` first — if upstream's deterministic-placement fix removes the
   need for the dual-flag split, `invert()` no longer has a tag to clear, the
   37-class method is unnecessary, and encoding B becomes a local change; or
2. Do both together, after the §8 pin move, as one behaviour-preserving refactor
   gated on the regenerated corpora.

The remaining risks, unchanged and still to work through when it is attempted:

- Patching the child's `Invert` in the emitted node must honour C's
  write-when-set convention (`Dump` writes `InvA` only when non-zero), or
  byte-exactness breaks on the "flag absent" cases.
- Nested CmpMaps: patching a child node's `Invert` must not disturb that
  child's own `InvA`/`InvB`.
- `equal()` and the simplifier's fingerprint read `inv_a`; both need review
  against the derived value. There are ~24 external readers of `inv_a()` /
  `inv_b()` across `simplify.rs`, `winmap_merge.rs`, `regions/mod.rs` and
  `fitschan/fset_to_store/`; most compute the drift expression and would
  simplify, but each needs checking.
- `TranMap` carries the same `inv_a`/`inv_b` pattern and would need the same
  treatment for the cleanup to be complete.

**Sequencing.** Do it as a behaviour-preserving refactor with the byte-exact
corpus as the gate — wcsconv 194, the 427 simplify fixtures and the 1,606
oracle sections should all stay green with no fixture regeneration, because
encoding B emits the same bytes. That makes it independently verifiable, unlike
the collapse, which needs the pin to move first.

**Where the state comes from in C.** `CombineMaps` (`cmpmap.c:527`) clones
rather than copies the components, temporarily sets each child's `Invert` to the
effective direction, lets the constructor snapshot it into the CmpMap's own
`invert1`/`invert2`, and then *restores the child's own flag* so the caller does
not find its Mapping altered. Because the child is shared, that restoration is
visible through the parent: the CmpMap's snapshot holds the effective direction
while the child's own flag holds the original. `Dump` writes both. The
disagreement the port must honour is therefore a courtesy to a caller holding a
shared pointer — a requirement that cannot arise under exclusive ownership.
Written up for upstream in `docs/issues/c-library-quirks.md`.

**Why it is deferred, and the test that settles it.** The read path is currently
required for byte-exactness, so nothing can be simplified today. But upstream is
removing sources of this state: `f54ff1b7` ("fitschan: do not invert an
encapsulated PermMap in `WcsNative`") and the deterministic-`IsSimp` change on
`u/timj/misc-bugs` both eliminate cases where C's dump records a flag the
Mapping itself does not justify — the two quirks in
`docs/issues/c-library-quirks.md` the port declined to mirror.

The go/no-go test is mechanical: **after the §8 pin move and corpus
regeneration, re-run the load-drift probe.** If the 539 goes to zero, then no
dump in the corpus carries disagreeing bits any more, and the mechanism can
collapse — maintain `inv_a == child.is_inverted()` as an invariant, drop the XOR
from all 15 sites, and replace the 6 dump-shaping calls with an ordinary
child inversion. If it stays non-zero, the mechanism is permanent and the doc
comments should say so rather than reading as scaffolding. Do not attempt either
before wcsconv, the simplify corpus and the transform oracle are all green
against the regenerated fixtures.

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
| Comment-less nested-object card wrote a trailing space C never writes | — | FIXED (`1d2aa8f`, `86810ce`, `crates/ast-core/src/serial/native_write.rs`) — C's `WriteObject` (`channel.c:5106-5112`) appends `" ="` then only `" \t# "` when the comment is non-empty; the Rust Object arm pushed `" = "` unconditionally and gated on `Some(_)` rather than non-empty, diverging only in the comment-less case (the with-comment case was byte-identical by coincidence) |
| Round-trip harness misses trailing-whitespace divergences and the corpus has no comment-less nested-object card | `crates/ast-core/tests/test_serialisation.rs:1097,1129` | OPEN — `native_round_trip_fixture`'s `trim()`/`split_whitespace()` comparison discards trailing whitespace, so `"Frm1 = "` and `"Frm1 ="` compare equal; separately, `grep -rhcE " =$"` is 0 across all of `simplify_fixtures/`, `frameset_fixtures/` and `generated/` — the four fixtures that do have a comment-less object card (`lsst.ast`, `lsst.simp`, `rigby.simp`, `simplify_fixtures_unsupported/intramap/intramap_inverse_cancel.map`) sit outside every scanned directory; needs the fallback comparison tightened to make trailing whitespace significant and a comment-less-object-card fixture moved into a scanned directory |

Cross-cutting blocker for the two rows above: the **FitsTable / `-TAB`
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
`const *_EXCLUDED` / `_LICENSED` / `_SKIPPED` in the test tree that is not in
its table. Adding a relaxation therefore means writing its reason down in two
places, and finding one by accident is no longer possible.

| Gate | Cases | Relaxed | What the relaxation is |
|---|---:|---:|---|
| wcsconv (`test_wcsconv.rs`) | 194 | 8 | `astequal` compare mode — FMA contraction / libm differences vs x86-64 Linux captures that C AST on arm64 does not reproduce either |
| wcsconv | 194 | 4 | `ISSIMP_PLACEMENT_LICENSED` — drops the `IsSimp` tag lines both sides *and pins the card count on each side*, so it covers the tag placement and nothing else. A deliberate divergence, not a gap: C's placement here records the order its reader simplified in (§8). `axes-swap`, `cd-cross`, `vopt-f2w`, `wave-f2w`; C emits more cards than the port in each. 24 rows preceded these; 20 were pinned against references the pinned C no longer reproduced and were retired with the corpus refresh (§9.3) |
| wcsconv | 194 | 0 | **No skip mode exists.** `NATIVE_READ_EXCLUDED` is empty and gated against refilling |
| simplify regression | 460 | 4 | `skip_string_compare` in the manifest (`rigby`, `cap_cmpmap_01`, `cap_cmpmap_04`, `cap_winmap_03`). `lsst` retired its skip when the manifest gained a `render_options` field: its reference was captured with the Channel's `Comment` and `Indent` off, and rendering the same way makes it byte-exact |
| simplify regression | 460 | 1 | `ISSIMP_PLACEMENT_LICENSED` — same counted form as the wcsconv row (`cap_specmap_02`, one card short of C: the CmpMap C tags there is one the port never forms, §10.3). Was 2: `cap_cmpmap_03` closed when the port stopped simplifying the reassembled parallel CmpMap (§10.3, divergence 3). The over-tagging half of the row closed with the per-member simplify |
| simplify regression | 460 | 7 | fixtures whose `IsSimp` card count still differs from C. Four under-tag: `rigby`, `cap_specmap_02`, and `cap_cmpmap_01` / `cap_cmpmap_04`, whose totals matched C at 18 only because tags C never sets made up for the same CmpMap deficit (§10.3); three are the Region-simplify shape gaps (`cefit_circle_conformal`, `cefit_circle_nonconformal_keep`, `cap_zoommap_04`). Retired before these: the three `cmpmap_perm_swap*` fixtures when the swap branch stopped simplifying the components C hands on as they stand, `cap_cmpmap_03` when the same branch stopped simplifying the parallel CmpMap it reassembles, and `parpair_one_side_reduces` when it stopped simplifying a dimension-matched sublist's members (all §10.3). `cap_cmpmap_01` and `cap_cmpmap_04` were retired with §10.16 and are back, because that closure was the coincidence above. Not a separate mask: they sit inside `skip_string_compare`, the counted licence above, or `SERIAL_UNGATED`. Listed because otherwise the number is invisible, and asserted as a set in `test_simplify_regression.rs` so it cannot grow. A stale fixture is no longer a possible cause: the corpus was regenerated against the pin at `f6762005` |
| simplify regression | 460 | 142 | `ignore_is_simp_line` for the negative fixtures, whose reference *is* their input and so cannot carry the tag `simplify` always stamps. The companion assertion requires the result to carry at least one, so the strip cannot hide an untagged result. 134 with `neg_time_mjdtojd_real` (§10.9), `neg_unit_parallel_forward` (§10.6) `neg_parpair_aligned_irreducible` (§10.14), `neg_lut_near_linear` (§10.8), and five upstream repaired at pin `8d4f526f`. Was 141 before `cap_box_permmap_nonbidi`, the negative control for a PermMap whose permutation is not bi-directional (§10.13) |
| transform oracle | 1614 sections | 0 | all three `*_ORACLE_EXCLUDED` lists are empty |
| transform oracle | 1614 sections | 2 | `tnx-cheb` round-trip off (ChebyMap cannot represent its own inverse); one UnitNormMap inverse golden off (BAD-vs-noise at a singular point) |
| frameset round-trip | 139 | 0 | `FRAMESET_FIXTURE_EXCLUDED` is empty — every wcsconv object round-trips byte-exact |
| deferred fixtures | — | 3 | `DEFERRED_CLASSES` — `IntraMap`, `XphMap`, `StcResourceProfile` are unported classes, not gates that could otherwise pass (§5) |

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

The pin moved from `50c2b21b` to `f6762005`: 41 commits, 25 `src/` files,
+872/−235, nineteen defect fixes, and 103 changed files under `ast_tester/`.
Every fix in the range had an entry in `docs/issues/c-library-quirks.md`, which
is what drove the sync — for each entry, either the port mirrored the bug and
the mirror had to go, or the port already did the right thing and needed a test
pinning the agreement.

Landed in `2efde3a`, `e0cb246`, `e029927` and `3a0a9d9`. 97 fixtures were
copied across byte-identical.

| C change | What the port did |
|---|---|
| `IsSimp` placement becomes deterministic (`e40e71b4`) | Ported: per-class `has_been_simplified` became a two-orientation `SimplifiedRecord`, and all six tag-clearing routines that emulated the old behaviour were deleted. `ALIASING_TAG_LICENSED` retired |
| `WcsNative` no longer inverts the encapsulated PermMap (`f54ff1b7`) | No code change — the port's shape was already the corrected one. `serpens_native` is now byte-exact and the `Invert` half of the old licence is gone |
| `EncodeFloat` justification fixed (`68bcb7db`) | Mirror removed |
| Duplicate-keyword consumption (`6301311f`) | Mirror removed; the write-side sweep gained the matching rule so the newly-surviving MJD-OBS reaches the output |
| `CDjjjiii` consumption (`fd92f9d2`) | Mirror removed |
| Mapping immutability (`AST__IMMUT`) | Not applicable to `ast-core`; the work belongs to `ast-capi`. `testimmutable.c` is deferred with that reason |
| Discard the simplified record on mutation (`6a4bc292`, `b2de3a33`) | Ported for the six classes with a settable attribute here; the other five have no `&mut self` setter, so the stale record cannot be constructed |
| KeyMap dumps in key order (`890707a8`) | Already the port's design; now gated |
| KeyMap: four mirrored defects (`6d114495`, `a76964c7`, `b7135abc`, `f444583f`, `d8edc072`) | All un-mirrored — see the commit and the quirk entries |
| `round()` rounding sweep (`e4e6376d`, `1199880a`, `e447556a`, `7b084bbf`) | Ported at the four sites the port has: the grism interference order, PermMap axis indices, the global-bounds threshold, and WCSAXES |

**One divergence, recorded rather than closed.** Four wcsconv cases differ from
C in `IsSimp` card placement, C emitting more cards than the port in each. It is
not a simplifier difference:
hand C the composed pixel→world Mapping this port builds *before* it simplifies
and C produces exactly this port's tags on exactly the same nodes. What differs
is that `fitschan.c` simplifies in about ten places while assembling, so its
closing `astSimplify` starts from an already-reduced tree, while this port
composes the whole chain and simplifies once. The extra cards are a record of
C's own division of labour inside its reader.

Not worth mirroring: the Mapping, its structure and its transforms are
identical, and reproducing the card means shaping `build_pixel_to_world` around
C's internal call sequence rather than around the WCS it assembles. Logged as a
quirk (`docs/issues/c-library-quirks.md`, "`IsSimp` placement in a FITS-WCS read
records the order the reader simplified in") and licensed with both card counts
pinned (§6), so a *shape* difference could never hide behind it.

`cap_specmap_02` on the simplify corpus is one card too few, from the same input
on both sides. Six fixes were tried and measured, and all were reverted —
including tagging every `simplify_mapping` result, which is `astSimplify_`'s
literal contract, and deferring leaf reduction entirely. The
finding is that this engine never has the missing node as the whole result of a
simplify, where C does: `IsSimp` records the set of sub-Mappings that were at
some instant the entire result of a simplification, and two engines reaching the
same Mapping by different internal routes disagree on that set. The Mapping is
identical — `equal()`, the transform check and every other byte all match. The
dead ends are listed in `test_simplify_regression.rs` so they are not retried.

The `.ast` FrameSet corpus (`frameset_fixtures/`) is byte-exact throughout and
could not have caught either: it round-trips read → write and never
re-simplifies.

**Not in this range, still open:** the `CmpMap` `InvA`/`InvB` quirk. It is why
`cap_cmpmap_01` and `cap_cmpmap_04` keep their `skip_string_compare`; the
`IsSimp` reason they used to share is gone.

General rule this instantiated, and it held: before moving the pin, diff the
incoming range for serialization changes and check
`docs/issues/c-library-quirks.md` for entries the port *mirrors*. Four of the
five mirrors found this way were invisible to every existing test, because the
fixtures had been generated by the buggy C.

---

## C → Rust test migration tracker

| C test (`ast_tester/`) | Rust counterpart | Status |
|---|---|---|
| testconvert.c | crates/ast-core/tests/test_convert.rs | ported 2026-06-12 |
| testframe.c | (deferred) | NormUnit(1) via capi units layer; orthogonal to the Frame-base port |
| check_transform_oracle | crates/ast-core/tests/test_transform_oracle.rs | ported 2026-08-05 (submodule `3a3c3e51`); consumes `tests/fixtures/oracle/*.oracle`; all 1614 sections gated by three asserting tests (`head_`, `slamap_`, `simplify_sections_match_oracle`), with `every_oracle_section_is_gated` proving the partition covers the corpus; `transform_oracle_report` remains `#[ignore]` as the investigative view; `gen_transform_oracle` intentionally not ported |
| testkeymap.c | crates/ast-core/tests/test_keymap.rs | ported 2026-08-08; the nine functions added upstream at `f6762005` ported 2026-08-20 |
| testmapping.c | crates/ast-core/tests/test_mapping.rs | partially ported 2026-08-20 — the four `IsSimple` functions added upstream at `f6762005`. `testselfsimplifyafterset` is unconstructible here (ZoomMap's `Zoom` and PcdMap's `Disco` have no `&mut self` setter). The file's original PolyMap / LinearApprox body, converted from `testmapping.f`, is covered by `test_polymap.rs` and `test_polytran.rs` |
| testfitschan.c | crates/ast-core/tests/test_fitschan*.rs | partially ported — `testgrismorder`, added upstream at `f6762005`, is in `test_fitschan_wcs_write.rs` (2026-08-20); the rest of the file is spread across the seven `test_fitschan_*` files |
| testimmutable.c | (deferred) | tests `AST__IMMUT`, which guards a change made to a Mapping whose reference count exceeds one. Owned values, deep `Clone` and single-owner `Box` make that condition unconstructible in `ast-core`; the guard belongs to `ast-capi`, which restores sharing at the handle boundary and is unstarted (§7) |
| testobjectkeymap.c | (deferred) | tests the Object-associated retained KeyMap (`object.c:1736`, `:2480`), `astGetObjSize` and `astSame`; those calls (`astMapPut0I`, `astMapGet0I`, `astMapSize`) are only reachable through the protected `astGetKeyMap`/`astHasKeyMap` machinery, an explicit spec Non-goal of the owned-value port |


## 9. Open after the fixture regeneration

The `.simp` corpus was stale relative to the submodule pin. Regenerating it
against the pinned C closed two disagreements and opened two questions.

### 9.0 The instrument was broken, and it hid eleven failures

`test_simplify_regression.rs` and `test_regions.rs` each hand-rolled their own
`&dyn Mapping` -> `AnyObject` dispatch. The simplify harness's version covered
18 Mapping classes and no Region, FrameSet or SwitchMap, and returned `None`
for anything else — from inside an `if let Some(..)` that guarded *both* the
byte comparison and the `IsSimp` count check. Every fixture whose simplified
result was a Region or a SwitchMap therefore skipped both, silently.

Both now call `AnyObject::from_mapping`, one authority in `ast-core` that is
the exact inverse of `AnyObject::as_mapping`, and an uncovered class is a
counted failure in the harness and a panic in the region test. Never a skip.

Turning the gate on exposed eleven pre-existing byte-comparison failures and
seven `IsSimp` count disagreements. The pinned C reproduces all eleven `.simp`
files byte-exactly, so every one is a port defect, not a stale fixture. They
are enumerated with causes in `SERIAL_UNGATED`, now down to six fixtures in
five causes:

| cause | fixtures | what is wrong |
|---|---|---|
| A | 4 SwitchMap cases — CLOSED | `IsSimp` missing on the FSMap / ISMap sub-Mappings; `SwitchMap::simplified` now stamps them |
| B | 2 `cefit_circle_*`, `neg_box_asymmetric_2d` | Region simplify tags a Mapping inside the encapsulated FrameSet that C leaves bare, and does not tag the returned Region at all |
| C | `nullregion_unc_fillfactor_merge` | emits a `Prism` uncertainty where C emits a `Box`. `box_unc_fillfactor_merge` was here, declined by the structural-hash recursion guard's one collision in the corpus, and is byte-exact now that the guard runs C's `astEqual` (§10.19) |
| D | `cap_zoommap_04` | emits `FrmSet`/FrameSet where C emits `Points`/PointSet |
| E | `cap_interval_01` | `Closed = 1` where C omits it. `cmpmap_parallel_series_dim_mismatch` was here — two `IsSimp` cards on the leaves where C has them on the intermediate CmpMaps — and is byte-exact now that the branch simplifies where C simplifies (§10.3) |
| F | `cap_pointlist_01` | two mesh coordinates diverge past 4 ulp |
| G | `parpair_one_side_reduces` — CLOSED | one or two extra `IsSimp` cards, and nothing else in the tree, on a leaf the port simplifies and C hands on as it stands. Diagnosed as `try_self_reduce` firing through the `simplified()` gate and predicted to close with §10.16; §10.16 landed and the counts did not move, so that diagnosis was wrong. The sources were two `.simplify()` calls with no C counterpart, both in CmpMap's parallel handling (§10.3): the first closed `cmpmap_perm_parallel_swap` and `cmpmap_perm_swap_{a,b}constants`, the second closed this fixture byte-exact. `cap_slamap_03`, `cap_wcsmap_01`, `cap_cmpmap_01` and `cap_cmpmap_04` did close with §10.16, when PermMap's rebuild moved into `map_merge`, and `lsst` closed the same way when PolyMap's coefficient consolidation and affine rebuild moved |

`equal()` passed all eleven, but that is not an `equal()` defect. C's
`region.c::Equal` compares class name, `points`, base Frame, current Frame, the
base->current Mapping, and `Negated`/`Closed` — and deliberately not the
uncertainty. Causes C and D both differ inside `Unc`, so C's `astEqual` returns
1 for them too, and ours agrees. Both languages *do* distinguish Box from Prism
at any compared position: `region.c::Equal` opens with `strcmp` on the class
names, and our `equal` downcasts.

What this establishes is narrower but still worth the gate: the byte comparison
covers fields `astEqual` is designed to ignore, so it is the only check that
will ever see a wrong uncertainty class.

### 9.3 24 of the 109 NATIVE-family wcsconv references were stale — DONE

Verified by building `ast_tester/wcsconverter.c` against the pinned C and
replaying every NATIVE-family manifest row *with its `attrs` field* — omitting
the attributes produces false structural differences.
94 of the 109 rows now reproduce byte-exactly and the rest are Linux captures.

**No keyword sequence differed in any of the 109.**
Nothing about core WCS structure changed upstream; every difference was either
an `IsSimp` card or a last-bits value.

| bucket | count | disposition |
|---|---|---|
| `IsSimp` cards only | 17 native + `jan.ast` | regenerated from the pinned oracle; written into the submodule tree for upstream |
| last-bits values *and* stale `IsSimp` | 6 | cards taken from a fresh capture, values kept, via `tools/oracle_fixtures/merge_card_hunks.py` |
| last-bits values only, `IsSimp` correct | 9 | not stale — Linux captures; documented in the manifest header, untouched |
| structural | 0 | — |

Mixed: `scp`, `tnx`, `tnx-cheb`, `ssysobs`, `skyspec3d-class`, `wave-f2w`.
Platform-only: `timj`, `multi-wcs`, `skyref`, `iraf-longpole`, `skyref-alt`,
`tidy-offsets`, `tidy-pole`, `offset-sref`, `azel`.

This is why the ShiftMap merge-revert fix looked like a six-case regression.
Measured against the pinned C rather than the committed reference:

| case | port | committed | pinned C |
|---|---|---|---|
| scp, serpens, tnx, tnx-cheb | 2 | 1 | **2** |
| crota-3d | 5 | 3 | **5** |
| cd-cross | 3 | 2 | 4 |

**Outcome.** With the corpus refreshed, 20 of the 24 `ISSIMP_PLACEMENT_LICENSED`
rows were pinned against references the pinned C no longer reproduced. They were
deleted rather than adjusted, and the port matches C on all of them. Four
survive — `axes-swap` (2 vs 4), `cd-cross` (3 vs 4), `vopt-f2w` (4 vs 5),
`wave-f2w` (4 vs 5) — all in the FitsChan read path, all with C emitting more
cards than the port. Which of C's ten mid-assembly `astSimplify` calls produced
which card is not attributed per case; that needs
`tools/oracle_fixtures/build_issimp_instrumented.sh`.

`tools/oracle_fixtures/check_corpus_fresh.sh` is the standing gate. It scans
every `native` and `ast` row, classifies cards and values independently so a
stale card cannot hide behind platform drift, and is part of the upstream-sync
protocol in `CLAUDE.md`. The FITS encodings carry no `IsSimp` record and are
left to the byte comparison in `test_wcsconv.rs`.

Specified in
[`docs/superpowers/specs/2026-08-21-simplify-mapmerge-audit-design.md`](superpowers/specs/2026-08-21-simplify-mapmerge-audit-design.md)
as W3.

### 9.1 `cefit_circle_*` — the two dump paths disagree with each other

`test_regions.rs::simplify_circle_conformal_keeps_circle` and
`simplify_circle_nonconformal_keeps_circle` fail. The port emits an `IsSimp`
card on the FrameSet's inner ZoomMap that the pinned C does not.

What the instrumentation established:

- The input `.map` carries `IsSimp = 1` on that ZoomMap, and C's loader honours
  it (`mapping.c:24138`).
- At dump time C is writing a *different* ZoomMap object with `flags = 0`
  (`DUMPALL obj=… class=ZoomMap invert=127 flags=0`). C's Mapping copy
  constructor clears only `report` (`mapping.c:23608-23648`), so a copy would
  have preserved the flag: the object C dumps was freshly constructed.
- Which constructor builds it is not yet identified. Do not guess — the rule
  has to come from the C source.

Resolved, and it was not a disagreement between the paths: the regression
harness was not comparing this fixture at all. See §9.0. Both paths now render
through `AnyObject::from_mapping` and agree that the port emits a card C does
not, which is cause (B). The C rule that clears it is still unidentified.

### 9.2 The clear-on-set convention is not ported

Upstream `e40e71b4` added a rule with no counterpart here: an attribute setter
whose value feeds the first part of that class's own `astMapMerge` discards
both simplified records (`mapping.h:449-457`). C applies it in 11 classes:

| class | attributes |
|---|---|
| ZoomMap | `Zoom` |
| LutMap | `LutInterp`, `LutEpsilon` |
| MathMap | `Seed`, `SimpFI`, `SimpIF` |
| PolyMap | `IterInverse`, `NiterInverse`, `TolInverse` |
| SplineMap | `InvNiter`, `InvTol`, `OutUnit` |
| SphMap | `PolarLong`, `UnitRadius` |
| WcsMap | `FITSProj`, `LonCheck` |
| PcdMap | `Disco` |
| PermMap | `PermSplit` |
| IntraMap | `IntraFlag` |
| GrismMap | (macro form, `grismmap.c:165,255`) |

Several of these attributes have no setter in the port yet, so the gap is
latent rather than active. What would expose it: a fixture that sets one of
these attributes on an already-simplified Mapping and then re-simplifies —
none exists today.

### 9.4 42 simplify fixtures have no transform-oracle section

`simplify_fixtures.oracle` predates 25 fixtures added since it was captured, and
holds only the `.map` half of two more, so 42 of the `.map` / `.simp` files in
`crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/` carry no
oracle section: `cmpmap_inverted_lone`, `cmpmap_inverted_nested`,
`lut_exactly_linear`, `parcomp_inverted_reemitted`,
`parcomp_inverted_nested_reemitted`, `parpair_aligned_reduces`,
`parpair_one_side_reduces`, `pcd_zero_disc_lone`, `pcd_zero_disc_series`,
`rate_inner_reduce_lone`, `rate_inner_reduce_series`, `time_jdtomjd_noop`,
`time_mjdtojd_noop`, `unit_parallel_invert_pcd`, `wcs_unknown_lone_reduce`, the
`.simp` halves of `switchmap_inv_fwdonly` and `switchmap_inv_invonly`, and ten
negatives.

This is not a hole in the transform-oracle gates: `resolves_every_oracle_relpath`
asserts every oracle *section* resolves to a fixture, and all 1614 that exist are
gated with empty exclusion lists. Each of these fixtures is compared for
serialisation byte-exactness and `IsSimp` placement by
`test_simplify_regression.rs`; what is missing is the independent check that the
simplified Mapping *transforms* points the way C's does.

Closing it means capturing the missing sections from the oracle rather than
regenerating the file: a full regeneration also imports
`cheby_linear_lone_reduce` and `neg_cheby_quadratic_no_reduce`, which reproduce
the C defect recorded in `docs/issues/c-library-quirks.md` and are deliberately
out of the port's corpus (§10.2), and rewrites every value in `headers.oracle`
and `framesets.oracle` in the last digit or two — platform drift, which the
corpus-freshness protocol says to leave alone.

## 10. Gaps named by the `MapMerge` / `Simplify` audit

Each entry is cited by a `gap` verdict in a checklist under
`docs/audits/mapmerge/`, and `test_mapmerge_audit.rs` fails if a cited entry
does not exist here.

[`docs/superpowers/specs/2026-08-24-simplify-parity-closeout-design.md`](docs/superpowers/specs/2026-08-24-simplify-parity-closeout-design.md)
orders these entries into waves and gives each one a design, a gate and a record
obligation.

### 10.1 RateMap's self-reduction is unreachable from the nominate loop — FIXED

`ratemap.c:717-727` opens `MapMerge` by simplifying the encapsulated Mapping and
rebuilding the RateMap around the result. The port had the logic in
`simplified()` and no `map_merge`, so it never ran on a RateMap nominated from
within a list. `RateMap::map_merge` now uses `self_reduce_map_merge`, which is
C's order: reduce first, then cancel against an inverse neighbour in series.

`rate_inner_reduce_series` is the fixture — a RateMap over a series ZoomMap pair
that fuses to one, inside a CmpMap so the RateMap is a list element. The pinned C
reduces it to `RateMap(ZoomMap(6))`. `rate_inner_reduce_lone` is the same RateMap
alone and passed from the start, which identified the seam rather than the
arithmetic as the gap.

**The second half of the fix, and why two earlier attempts failed.** Adding the
seam alone made `cap_cmpmap_01` and `cap_cmpmap_04` diverge structurally. C sets
the encapsulated Mapping's Invert to `map->invert` — the value it held when the
RateMap was built, which the `Inv` card carries — simplifies at *that*
orientation, and restores the stored value afterwards (`ratemap.c:710-715`,
`:806-808`). The port simplified at whatever the child currently stored.

In those two fixtures the encapsulated CmpMap stores `Invert = 1` while the
snapshot is 0, so C reduces the forward form and finds nothing, while the port
reduced the inverted form and folded the inversion into the component order.
`RateMap::reduced_child` — the step `map_merge` opens with — now orients to
`invert_snapshot` first, the field the port already had for the `Inv` card and was
using only for serialisation. That is the H3 hazard: two C names kept as two
fields, with only one of them wired up.

An earlier attempt reduced the *forward* form unconditionally. That is right only
when the snapshot is 0, which is true of every fixture in the corpus, so it went
green for the wrong reason and was reverted before this.

### 10.15 The `C-ARTIFACT` set: three layers redundant, the rest load-bearing — RESOLVED

`cmpmap.rs`'s module doc defined a `C-ARTIFACT` marker for items that "exist only
to reproduce a serialised value that C emits as a side effect of its memory
model". Nine sites, all about `InvA`/`InvB`. Auditing them the way
`dump_simplified` was audited — disable, measure — found **four** mechanisms
pushing an inverted CmpMap's inversion into its structure, of which three were
redundant.

Measured individually and then all together, against the full suite including all
194 `wcsconv` byte comparisons:

| layer | verdict |
|---|---|
| `apply_effective`'s structural branch in `reconstruct` | redundant |
| the canonicalisation in `CmpMap`'s recorded-orientation fixup | redundant |
| `canonicalize_inverted_cmpmaps`, the tree-wide pass | redundant |
| `normalize_unitmap_children` | **load-bearing** |

All three redundant layers are deleted, along with `invert_into_structure` and
`invert_parallel_child`, which had no other callers. `normalize_unitmap_children`
survives and its doc now records what demands it: removing it costs four
`equal()` failures — `cap_cmpmap_01`, `cap_cmpmap_04`, `cap_wcsmap_01` and
`parcomp_inverted_nested_reemitted` — plus two serialisation failures.

**Two things are worth keeping from how this went.** The three deleted layers
each carried a note saying they were dump-shaping and measured; the one that
turned out to matter was the one marked "not independently measured". And the
measurement only became meaningful once the corpus demanded the behaviour:
`parcomp_inverted_reemitted` and `parcomp_inverted_nested_reemitted` were
generated for it — an inverted parallel CmpMap that must be re-emitted while the
tree is rebuilt around it, since the trailing ZoomMaps merge and the parallel
component merges with nothing. C's output pushes the inversion down: no CmpMap
carries `Invert`, and the flags appear as `InvA` on the components.

**The rest of the set, measured.** `set_first_orientation` and
`set_second_orientation`, with their `winmap_merge.rs` caller, are strongly
load-bearing: disabling the flip costs 12 further simplify serialisation failures
*and 5 `wcsconv` byte comparisons* — `grism`, `skyref`, `skyref-alt`,
`tidy-offsets` and `tidy-pole`, all native. Their note claimed "8 serialisation
differences", which understated it and evidently never covered `wcsconv`, the
strict gate. `equal()` and transform comparisons stay clean either way, so the
"dump-only" label is accurate; "removable" was not.

Both `parcomp_*` fixtures are among the 12, so these setters are the second of
the two layers that produce C's inversion push-down, alongside
`normalize_unitmap_children`.

Every surviving note now names the fixtures that demand it, and the module doc
records that the set has been audited rather than leaving "candidates for
deletion" to be read as "safe to delete".

**Why this mattered beyond tidiness.** Every layer here existed to make a dump
match, and the dump is the contract, so a redundant layer is a place where a
correct change looks like a regression. That is how §10.1 cost three attempts:
`RateMap::invert_snapshot` was documented as existing "for serialization
compatibility" and was therefore ignored by the simplify path that needed it.

### 10.14 Whether the port over-reaches C's nominate loop — RETRACTED

Three versions of this entry claimed a reach or structure divergence, and a trace
of the C library disproved all of them.

`tools/oracle_fixtures/build_simplify_trace.sh` instruments the three dispatchers
in `mapping.c` — `astSimplify_`, `astMapMerge_` and `astMapList_` — so one build
reports every nomination, decomposition and short-circuit, for every class. Run
on `cap_cmpmap_01` it prints, at depth:

```
MAPMERGE RateMap where=1 series=0 nmap=3 issimp=0 inv=0
SIMPLIFY-ENTER CmpMap issimp=0 dns=0 inv=0 nin=1 nout=1
```

So C **does** nominate that RateMap, its `MapMerge` **does** run, and it **does**
call `astSimplify` on the encapsulated CmpMap, unwalled. Nothing about reach
differs. What differs is the orientation: C enters with `inv=0` on a CmpMap whose
dump says `Invert = 1`, because `astSetInvert( emap, map->invert )` put it at the
construction-time orientation first. That is §10.1's second half, and fixing it
there closed this.

What the three wrong versions had in common: each inferred a rule from a
fixture's output instead of watching the library run. The trace script exists so
the next question of this shape costs one command.

Two by-products are kept. `parpair_aligned_reduces`, `parpair_one_side_reduces`
and `neg_parpair_aligned_irreducible` pin C's parallel-pair expansion rule, which
the port already implements correctly. And one latent finding stands: no Region
class checks the `IsSimple` bit (`box_region`, `circle`, `cmpregion`, `ellipse`,
`interval`, `nullregion`, `pointlist`, `polygon`, `prism`, `regions/mod.rs`), so
the port re-simplifies a Region C returns untouched — measured as unobservable on
this corpus, and not shipped.

### 10.2 ChebyMap did not inherit PolyMap's `MapMerge` — consolidation half FIXED

In C, `chebymap.c` defines no `MapMerge`; only `polymap.c:2439` fills the slot,
so a ChebyMap's `MapMerge` is PolyMap's whole body at `polymap.c:3597-3961`. In
the port, `ChebyMap` is its own struct, and it had no `map_merge` at all, so it
took the trait default and reached none of PolyMap's replace-with-simpler half.

Two halves were missing, and they are not equally safe to port.

The coefficient consolidation (`polymap.c:3624-3745`) combines coefficients that
use the same set of input powers. That is basis-independent — two `T_2` terms for
one output add exactly as two `x^2` terms do — so it ports as it stands, and now
does. `PolyMap::consolidated_terms` (`polymap.rs:838`) is C's first half split out
of `replace_with_simpler` unchanged, and `ChebyMap::map_merge`
(`chebymap.rs:527-569`) calls it and rebuilds a ChebyMap with the same bounds.
`cap_cheby_consolidate` is the fixture — two forward `T_2` coefficients of 1.5 and
2.5 that C writes back as one of 4, with a `T_3` term present so C stays out of
the linear branch — and it fails byte-exactly with `ChebyMap::map_merge` removed.

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

### 10.16 The port has a per-class `simplified()` seam that C does not — FIXED

C has **two** per-class seams in the simplify machinery:

- `Simplify`, overridden by `CmpMap`, `FrameSet` and the nine Region classes and
  by nobody else. For every other class the base implementation
  (`mapping.c:16752`) builds a one-element list and calls
  `astMapMerge( map, 0, 1, &nmap, … )` **repeatedly** until nothing changes or
  the list grows past one (`mapping.c:16871-16883`).
- `MapMerge`, overridden per class.

So for a class outside that 11, every reduction C performs lives in `MapMerge`,
and `astSimplify` reaches it only by nominating the Mapping in a list of one.

The port had **three**: the public `simplify()`, a per-class `simplified()`, and a
per-class `map_merge()`. `simplified()` had no C counterpart for the other 25
classes, and the port put per-class reductions there.
`grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c` is the
authority: the split was exactly right for 11 classes and exactly wrong for 25.

**This is the H1 hazard, and it produced three bugs.**
RateMap's encapsulated-Mapping reduction (§10.1) and PcdMap's zero-distortion
replacement (§10.7) both lived in `simplified()` with no `map_merge` to reach
them, so neither ran on a nominee inside a list — C's only route. Both fixes were
the same shape: give `map_merge` the reduction C keeps there. §10.3's
blanket-stamp failure is the third: stamping `simplified()` over-tags precisely
because it is not `astSimplify`.

H1 warns about *collapsing* C's seams into one central `match`; here a seam was
*added*. The failure mode is identical, because what H1 is really about is the set
of seams differing from C's.

Planned in
[`docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`](superpowers/specs/2026-08-23-simplify-seam-realignment.md),
with the task breakdown in
[`docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md`](superpowers/plans/2026-08-23-simplify-seam-realignment.md):
six tasks, fixtures first, one class per commit, and a test that holds the keeper
list so the seam cannot drift back.

**Done.** Twenty-five class bodies moved out of `simplified()` into `map_merge`,
one commit each, and `simplified()` now survives only in the eleven classes whose
C counterpart overrides the `Simplify` vtab slot. The plan's count was 24; the
sweep found a twenty-fifth, `ChebyMap`, which had inherited a body nobody listed.

Two things the entry above did not anticipate:

- **The single-caller half is a compile error, not a test.**
  `Mapping::simplified` takes a `ViaSimplify`, a zero-sized token only
  `mappings::simplify_entry` can construct, so the seam has exactly one possible
  caller — C's `mapping.c:24772` — and reaching it any other way does not build.
  `test_simplify_seams.rs` is left holding the keeper list, which a type cannot
  express.
- **The gate is now unconditional.** Every reduction passes through
  `simplify_entry`, so every one short-circuits on `IsSimple` and stamps its
  result at `mapping.c:24763`'s equivalent. Serialisation failures in the simplify
  harness went 12 to 9. It also brought `cap_cmpmap_01` and `cap_cmpmap_04` to
  C's card count of 18, which §10.3 has since shown was a coincidence of two
  errors in opposite directions.

What it did not fix: the WinMap under-merge of §10.3, which the entry above
implied was downstream of the seam and is not. Cause (G) in §9.2 named the
`simplified()` gate as the source of five fixtures' extra cards and predicted they
would close here. They did not; the real source was two `.simplify()` calls in
CmpMap's parallel handling with no C counterpart, both of which have since been
removed (§10.3).

### 10.3 The parallel-in-series branch now simplifies where C simplifies; seven CmpMap tags remain

All three divergences in the port's handling of C's `cmpmap.c:1624-1783` are
closed, so rows 22, 23 and 26 of `docs/audits/mapmerge/cmpmap.md` are `ported`.
Two fixtures came byte-exact with the change — `cmpmap_parallel_series_dim_mismatch`
and `parpair_one_side_reduces` — and their `SERIAL_UNGATED` rows are deleted,
taking that list from nine cases to seven. `cap_specmap_02` went from ten
`IsSimp` cards to seven against C's eight; only the WinMap under-merge half of
its row survives, because the CmpMap C tags there is one the port never forms.

The entry is still open, for a reason the change itself exposed.

**`cap_cmpmap_01` and `cap_cmpmap_04` never agreed with C.** They read 18 cards
against C's 18 and now read 16 and 15. Both are `skip_string_compare`, so the
card count was the only gate on them — and it is a total, which two errors in
opposite directions can satisfy (H10). The port was tagging leaf classes C never
tags while already missing CmpMap tags C sets. Per class, the two sides now
differ in one place only:

| fixture | port | C |
|---|---|---|
| `cap_cmpmap_01` | 14 CmpMap, 1 MatrixMap, 1 UnitMap | 16 CmpMap, 1 MatrixMap, 1 UnitMap |
| `cap_cmpmap_04` | 13 CmpMap, 1 MatrixMap, 1 UnitMap | 16 CmpMap, 1 MatrixMap, 1 UnitMap |

Before the change the port tagged 14 CmpMaps in each and made both totals up to
18 with tags C does not set at all — a MathMap and a TranMap in `_01`, a TranMap
and a second UnitMap in `_04`. Dropping the per-member simplify removed those,
which is C's shape, and `_04` lost one of its CmpMap tags with them.

A positional diff against the reference is now short enough to itemise. `_01`
differs in three `IsSimp = 1` lines C emits on inner CmpMaps and the port does
not, plus one object where the port emits `IsSimp = 1` and C emits `Invert = 1`
— the open `InvA`/`InvB` dump quirk
(`docs/issues/c-library-quirks.md:320`), not a stamping difference. `_04` differs
in four such `IsSimp` lines and the same one `Invert` line. Both totals are
registered in `ISSIMP_DISAGREEMENTS` with that diagnosis, so nothing here is
hidden any more; what changed is that the number now measures something.

**What would expose the rest.** Whether the seven missing CmpMap tags share
`cap_specmap_02`'s cause — a CmpMap C forms and the port does not — is not
established: the diff says which objects lack the tag, not why. Two things would
settle it, and neither exists yet: a fixture covering these trees that is *not*
`skip_string_compare`, and a trace mapping each of C's 16 CmpMap tags to the
nomination that produced it. While a count is the only gate on `_01` and `_04`,
they cannot show it themselves.

#### What C does in this branch

C's shape at `cmpmap.c:1624-1783` is: match the two parallel lists into aligned
sublists, `CombineMaps` each pair in series **without simplifying the members**,
`tmap2 = astSimplify( tmap )` on the combined pair (`:1728`), set `simpler` from
the identity test `tmap != tmap2 || astGetInvert( tmap ) != astGetInvert( tmap2 )`
(`:1731-1733`) *before* annulling `tmap`, and assemble the survivors into one
parallel CmpMap with no further simplification. The port diverged in three
places, all now closed:

1. **Per-member simplify.** `dimension_match_parallel_lists` simplified every
   member of both sublists. C passes them to `CombineMaps( …, 0, … )` as they
   stand — that `0` is the series flag, not a simplify flag
   (`cmpmap.c:1694-1712`). The port now clones them unchanged
   (`simplify.rs:2244-2251`).
2. **The `simpler` test.** A three-branch structural test ending in a comparison
   of flat leaf counts stood in for C's identity test — a threshold with no
   analogue in `cmpmap.c` (stop signal 1), deciding whether the whole restructure
   fired. C's test is ported at `simplify.rs:2161-2166`:
   `simplify_mapping_reporting_change` (`simplify.rs:3403-3496`) reports whether
   the result differs from its input, which in an owned-value port is a fact the
   engine knows rather than a pointer comparison, and the Invert flags are
   compared beside it.
3. **Trailing simplify of the rebuilt CmpMap.** The port simplified the
   reassembled parallel CmpMap before returning it; C hands `new` to the list as
   built (`cmpmap.c:1738-1746`) and lets the nominate loop reach it when it is
   nominated in its own right (`simplify.rs:2191-2198`). Closing this took
   `cap_cmpmap_03` from six cards to **seven, which is C's count**.

The per-pair `astSimplify` of `:1728` was always ported, at
`simplify.rs:2148-2159`; the recursion was never the missing piece. An earlier
reading of divergence 1 had it standing in for a missing recursion, and that was
wrong — it was standing in for divergences 2 and 3 in the same function, which is
why removing it alone moved fixtures in both directions.

The same over-simplification was in `try_swap_permmap_parallel_at`, which
simplified each surviving component before rebuilding the swapped CmpMap. C
assembles the new CmpMap from `cmpmap2->map1` / `map2` directly
(`cmpmap.c:2053-2070`) and never simplifies them; the nominate loop reaches them
afterwards, through the rebuilt CmpMap. Removing those two calls closed
`cmpmap_perm_parallel_swap`, `cmpmap_perm_swap_aconstants` and
`cmpmap_perm_swap_bconstants` byte-exact with no losses elsewhere.

Seven approaches to `cap_cmpmap_03` were measured and reverted before divergence
3 was found, six recorded in `test_simplify_regression.rs` plus a blanket stamp
in `CmpMap::simplified`. Every one of them tried to add a stamp; what closed it
was removing a simplification C does not perform. The blanket stamp is the
clearest case: it moved `cap_cmpmap_04` one card closer *and* gave
`cap_zoommap_01` a card C does not emit, because it stamps nodes the port
produced and C did not while still missing the ones C produced and the port did
not.

#### The residual is over-nomination, not a missing merge

Both engines are instrumented — `build_simplify_trace.sh` for C,
`simplify::trace` behind the `tracing` feature for the port — and log each
nomination and what it returned. Re-measured on `cap_specmap_02` at pin
`07fbe2dc` with all three divergences closed:

**The two traces are not directly comparable, and the raw counts mislead.** When
`ShiftMap::MapMerge` converts to a WinMap and calls `astMapMerge` on it
(`shiftmap.c:671-745`), C's instrumentation emits a *second* nomination event for
the delegated WinMap at the same nesting depth. The port calls `win_map_merge`
directly, so only the ShiftMap event appears. Nine of C's raw nominations are
those echoes, and all nine are productive — so C's raw 96/23 is 87/14 in genuine
nominations. Detect an echo as a nomination whose immediately following
nomination is a different class at the same indentation.

| class | C noms | port noms | C productive | port productive |
|---|---:|---:|---:|---:|
| CmpMap | 22 | 23 | 5 | 5 |
| LutMap | 16 | 20 | 0 | 0 |
| PermMap | 9 | 11 | 0 | 0 |
| WinMap | 9 | 10 | 1 | 2 |
| ShiftMap | 9 | 11 | 3 | 3 |
| SpecMap | 8 | 14 | 1 | 1 |
| ZoomMap | 7 | 13 | 1 | 1 |
| MatrixMap | 4 | 4 | 0 | 0 |
| UnitMap | 3 | 3 | 3 | 3 |
| **total** | **87** | **109** | **14** | **15** |

The port's CmpMap row is 12 series plus 11 parallel; C reports both compositions
under the one class name.

**There is no productive deficit.** Both engines perform the same merges, and the
port's one extra productive WinMap is a *second* occurrence of the same
`WinMap where=1 series=0 nmap=2` merge C performs once. What remains is
over-nomination: 109 against C's 87, concentrated in LutMap (20 vs 16), SpecMap
(14 vs 8), ZoomMap (13 vs 7) and PermMap (11 vs 9) — every one of which is
productive **zero** times on this fixture. The port is not missing a merge rule;
it re-walks lists C walks once. Closing the three divergences removed two of
those extra passes, taking the port from 153 nominations to 109 while leaving the
productive count at 15, and brought the number of nominations of an
already-stamped Mapping from 51 to 15, against C's 16.

**Upstream has since fixed a C bug in the same branch.** At pin `8d4f526f`,
`cmpmap.c` commit `4f2fc0ed` — *"test for simplification before annulling the
pointer under test"* — repairs the parallel-CmpMaps-in-series case: the annul of
`tmap` ran before the `tmap != tmap2` test, so the test was always true and the
pair was rebuilt as a parallel CmpMap of series compositions **even when no
component reduced**. The sibling branch 120 lines earlier had it in the right
order. Any conclusion drawn about this branch before pin `8d4f526f` was drawn
against a buggy oracle. The regenerated
`cmpmap_parallel_series_dim_mismatch.simp` carries two more `IsSimp` cards than
the stale copy, and the port now emits all three — that fixture is one of the two
this entry closed.

**The ShiftMap delegation is not the divergence.** An earlier revision of this
entry narrowed the deficit to the `ShiftMap where=0 series=1 nmap=4` nomination,
on the reading that C's list still held four elements afterwards and the port's
held three. The C trace at pin `07fbe2dc` shows otherwise:

```
MAPMERGE ShiftMap where=0 series=1 nmap=4 issimp=0     # c_trace:108
MAPMERGE WinMap   where=0 series=1 nmap=4 issimp=0     # the delegation echo
MAPMERGE-RESULT WinMap   result=0
MAPMERGE-RESULT ShiftMap result=0
MAPMERGE WinMap   where=0 series=1 nmap=3 issimp=0     # C's list is 3 too
MAPMERGE-RESULT WinMap   result=-1
```

The `nmap=4` WinMap line is the delegation echo described above, not a separate
opportunity, and C's next genuine nomination is `WinMap where=0 series=1 nmap=3`
— exactly the port's. `ShiftMap::map_merge` (`shift.rs:168-238`) delegates and
reverts as C does, and the two agree on class, index, `issimp`, result and
resulting list length at that step.

### 10.4 The CmpMap/PermMap swap was hung off the wrong nominee — FIXED

`cmpmap.c:1816-2119` swaps a parallel CmpMap in front of an adjacent PermMap. It
runs when the **CmpMap** is nominated and the PermMap is its lower neighbour, as
the last block of `CmpMap::MapMerge`, reached only when nothing earlier
simplified (`result == -1`). There is no counterpart on the PermMap side:
`permmap.c` contains no reference to `CmpMap` at all, so a nominated PermMap
never attempts this.

The port had it the other way round: the swap lived in `perm_map_merge` and fired
when a **PermMap** was nominated, looking at the `(where_, where_ + 1)` pair. The
arithmetic was already C's. The branch now sits at the end of `cmp_map_merge`
(`simplify.rs:827-849`) behind C's four conditions, and
`try_swap_permmap_parallel_at` is unchanged.

The record's original premise was wrong about what this cost. It said the two
triggers report different indices and so resume one element apart. They do not:
C returns `result = imap1 = where - 1` from the CmpMap's nomination
(`cmpmap.c:2119`), the port returned `Some(where_)` from the PermMap's, and
because `run_nominate_loop` advances before clamping (`simplify.rs:1501`) both
name the PermMap's slot and both resume there. Nor is a pair reachable in C but
not here: C's `result == -1` guard *defers* the swap to a later round rather than
forbidding it, so the port's earlier trigger only ever preempted a merge C would
reach on its next pass over the same list. No corpus fixture and no purpose-built
candidate separates the two on output.

What the move is visible in is the nomination trace. `cap_cmpperm_resume`
(`[Zoom2, PermMap, parallel CmpMap, Zoom3]` in series) and its inverted-PermMap
sibling `cap_cmpperm_resume_inv` are byte-exact positive controls on the swap's
result; against the instrumented oracle the port now matches nomination for
nomination — `PermMap where=1` returning no change, then `CmpMap where=2`
returning 1 — where before it returned 1 from the PermMap's own nomination. That
trace agreement, not a fixture that flipped, is the evidence the seam moved.

This entry replaces the plan's §10.4, which duplicated §9.2.

### 10.6 A parallel UnitMap's Invert flag — NOT A GAP, retracted

Recorded as a gap on the strength of reading `UnitMap::map_merge` alone, which
handles the lone-UnitMap case and the series case and otherwise falls through to
`default_map_merge`, where a nominee with no UnitMap neighbour returns `None`.
Nothing there clears the flag, so `unitmap.c:579-587` looked unported.

It is ported, elsewhere. C clears `invert_list[ where ]` — the *list's* applied
direction, not the object's own attribute — and that surfaces in the dump as the
parent CmpMap dropping its `InvA` or `InvB`. The port carries the applied
direction as a separate value in `MergeCtx` and folds it into the leaf when the
tree is rebuilt (`simplify.rs:3204-3212`), reaching the same dump.

Two fixtures prove it, on independent configurations: `unit_parallel_invert_clear`
(a MathMap neighbour, 2 axes), which predates this audit, and
`unit_parallel_invert_pcd` (a PcdMap neighbour, 3 axes), added to check the first
was not passing by accident. `neg_unit_parallel_forward` is the control. All
three are byte-exact against the pinned C.

One residual difference is not observable in any fixture: C reports the clearing
as a change and so restarts its nominate loop, where the port reports `None`. If
that ever matters it will show as a resume-point difference, not as a wrong
Mapping.

### 10.7 PcdMap's zero-distortion reduction is unreachable from the nominate loop — FIXED

`pcdmap.c:1417-1433` opens `MapMerge` by replacing a PcdMap whose distortion
coefficient is zero with a UnitMap, before any neighbour is considered. The port
had that logic in `PcdMap::simplified`, while `pcd_map_merge` tried the swap and
then `default_map_merge` and consulted neither. A zero-distortion PcdMap
nominated from inside a list therefore stayed a PcdMap.

This is the same shape as §10.1: the class reduction exists, and the seam that
would reach it from the nominate loop does not.

Exposed and closed. `pcd_zero_disc_series` is
`[MatrixMap(2,3), PcdMap(disc=0), MatrixMap(5,7)]`, which the pinned C collapses
to a single diagonal `MatrixMap(10,21)`. `pcd_zero_disc_lone` is the same PcdMap
on its own, and it passed from the start — the logic was in `simplified()` all
along and only the seam was missing, which is exactly what the pair
discriminates.

`pcd_map_merge` now performs C's opening move itself, before the swap
(`simplify.rs:2903-2911`), and returns immediately: C makes the reduction the `if`
of an if/else whose `else` holds the whole neighbour search, so a reduced PcdMap
never looks at a neighbour in the same pass. `self_reduce_map_merge` cannot
express that ordering, which is why the reduction is inline rather than delegated.

### 10.8 LutMap's linearity test used a hardcoded tolerance — FIXED

C's `GetLinear` (`lutmap.c:598-627`) decides whether a lookup table is linear,
and `MapMerge` replaces a linear LutMap with an equivalent WinMap. Its tolerance
is scaled by the `LutEpsilon` attribute — the stated relative error of the table
values, `DBL_EPSILON` by default — and taken as the larger of two estimates, one
from the table's full range and one from the interpolated value at the element
under test, with linearity accepted within twice that.

The port used a hardcoded `1e-10`, a `1e-20` floor with no analogue in C, and one
estimate formed from the end values rather than the table's extremes. Roughly six
orders of magnitude looser, so it replaced nearly-linear LutMaps with WinMaps
where C keeps the LutMap — a shape difference in the simplified tree, not a
numeric one.

`GetLinear` is now ported as it stands, including the `hi == lo` constant-table
case, and `LutMap` carries a `lut_epsilon` field defaulting to `f64::EPSILON` and
read from the `LutEps` card (`lutmap.c:2012`) so a dump round-trips.

Two fixtures. `neg_lut_near_linear` departs from linear by about 3e-13 relative —
above `DBL_EPSILON`, far below `1e-10` — and C keeps it a LutMap; the port
converted it. `lut_exactly_linear` is the control C does replace with a WinMap,
without which the negative case would pass for a port that never converts a
LutMap at all.

No public setter was added. C's `LutEpsilon` setter is one of the eleven
clear-on-set sites (§9.2), so adding one means adding the clear; the attribute is
read from the dump and otherwise fixed at construction.

### 10.9 TimeMap drops one no-effect conversion where C drops three — FIXED

`timemap.c:2260-2266` eliminates a change-of-system step whose derived net offset
is zero, and applies it to `AST__MJDTOMJD`, `AST__MJDTOJD` and `AST__JDTOMJD`
alike, testing `cvtargs[ istep ][ 2 ]`. That third argument is derived at
`timemap.c:503,507,511`:

- `MJDTOMJD`: `args[0] - args[1]`
- `MJDTOJD`: `args[0] - args[1] + 2400000.5`
- `JDTOMJD`: `args[0] - args[1] - 2400000.5`

`time_step_is_no_effect` (`timemap.rs:1163-1174`) tests `args[0] == args[1]` and
only for `MjdToMjd`. For that type the two tests are the same condition. For the
other two they are not: a `MjdToJd` whose JD offset absorbs the 2400000.5
constant is a genuine no-op that C removes and the port keeps, leaving an extra
conversion step in the rebuilt TimeMap.

The port already derives `args[2]` identically (`timemap.rs:492-501`), so closing
this is a matter of testing that value for all three types instead of comparing
the first two arguments of one.

Exposed and closed. `time_mjdtojd_noop` and `time_jdtomjd_noop` are lone steps
whose offsets absorb the constant; the pinned C simplifies each to a UnitMap and
the port did not. `neg_time_mjdtojd_real` is the control, an MJDTOJD whose
offsets do not absorb the constant and which must survive.

Why the existing corpus missed it: `time_2arg_swapped_cancel` holds
`MJDTOJD {0.0, 2400000.5}` followed by `JDTOMJD {2400000.5, 0.0}`. Both steps
have a zero derived offset, so C drops them by this rule — but the port drops
them by the inverse-pair rule, reaching the same UnitMap for a different reason.
A lone step is what separates the two.

### 10.10 SwitchMap left its simplified children untagged — FIXED

This entry originally blamed the fused either/or: C's
`if( invert_list[where] ) { normalise; } else { simplify children; }`
(`switchmap.c:1052-1100`) makes those alternative outcomes of one nomination,
where the port did both in one call. Tracing C confirmed the shape — the same
SwitchMap is nominated at `inv=1`, then again at `inv=0`, then a third time once
the children were reduced — but that was not the cause.

Giving `map_merge` C's either/or was implemented and reverted twice: once in the
wrong order, which broke `switchmap_inverse_cancel` on both `equal()` and
*transform* comparisons because C attempts the neighbour cancellation first
(`switchmap.c:992`) and gates the self-simplify half on "no change made above"
(`:1054`); and once in the right order, which changed no fixture at all.

The cause was the tags. C reaches each encapsulated Mapping through
`astSimplify`, which records whatever it returns (`mapping.c:24763`), and the
port used `simplified()` — the untagged virtual — for the selectors and the route
Mappings. So C emitted 4 cards on `switchmap_invert_normalize` and the port
emitted 1. The trace made this readable directly: three `SIMPLIFY-ENTER ZoomMap`
entries plus the top-level SwitchMap is exactly C's 4.

Both child paths now stamp what they get back, as `RateMap` and `TranMap` already
did. All four fixtures — `switchmap_invert_normalize`,
`switchmap_internal_simplify`, `switchmap_fwdonly_simplify`,
`switchmap_inv_invonly` — pass byte-exact, and cause (A) is retired from both
pinned lists in `test_simplify_regression.rs`. Simplify serialisation failures
drop from 11 to 7.

§10.16 then moved the whole self-simplification into `SwitchMap::map_merge`, where
C keeps it, and gave it C's either/or after all: the third revert had selected the
arms on the SwitchMap's own `Invert` flag, but `flatten` resets every leaf to
forward and carries the inversion in the merge list, so the selector is
`ctx.entry(where_).1 != self.is_inverted()`. The corpus is unchanged by that move,
and the top-level SwitchMap normalisation the public entry point used to perform
is now dead code and removed.

### 10.11 TranMap's `map_merge` reached one of C's self-simplification cases — FIXED

`tranmap.c:834-897` gives a nominated TranMap four outcomes: normalise an
inverted TranMap to the equal forward form and stop; simplify the two component
Mappings; collapse a TranMap whose components are inverses of each other to one
of them; or fall through to the neighbour merge.

All four used to live in `TranMap::simplified`, and `TranMap::map_merge`
implemented only the component simplification, inline. A TranMap nominated from
inside a list therefore kept its inversion where C replaces it, and was not
collapsed when its two components are inverses.

§10.16 moved the whole body into `TranMap::map_merge`
(`mappings/tranmap.rs:256-333`), where C keeps it, with C's three exclusive cases
selected on `ctx.entry(where_).1` — `flatten` resets every leaf to forward and
carries the inversion in the merge list, so the requested orientation is the
merge entry's flag rather than the object's own. Case 3 stores the surviving
component normalised to forward with its intrinsic flag beside it, so the
replacement takes part in further merging exactly as C's does.

Two fixtures gate the cases the corpus could not reach before, because both need
the TranMap to be nominated from inside a list rather than as the whole Mapping:
`tranmap_invert_in_list` for `tranmap.c:837-850` and
`tranmap_equal_components_in_list` for `tranmap.c:864-887`. Both come out of C as
predicted — the first serialises a forward TranMap with the components swapped
and inverted, the second collapses to a single `ZoomMap` with `Zoom = 10` once
the exposed component merges with its neighbour — and both are byte-exact in the
port.

### 10.12 `GetDefUnc`'s unbounded-axis guard missed the upper bound — FIXED

`region.c:3560-3573` builds the default uncertainty as a Box covering 1.0E-6 of
the Region's base-Frame bounding box, and skips any axis whose bound is
`±DBL_MAX`, setting both output bounds to zero for it.

The port's guard was `!is_bad(ubnd[i]) && !is_bad(lbnd[i])`, and `is_bad(v)` is
`v == BAD_VALUE || !v.is_finite()` (`mappings/helpers/mod.rs:11-13`) with
`BAD_VALUE == -f64::MAX`. That catches an unbounded *lower* bound, which is
`-f64::MAX` and therefore `BAD_VALUE`. It did not catch an unbounded *upper*
bound: `f64::MAX` is finite and is not `BAD_VALUE`, so the axis took the bounded
branch and received a half-width of roughly `0.5e-6 * f64::MAX`, about 9e301,
where C gives it zero.

`regions/mod.rs:471` now carries C's asymmetric test as it stands —
`ubnd[i] != f64::MAX && lbnd[i] != -f64::MAX` — alongside the `is_bad` handling
the surrounding code needs. The magnitude form (`ubnd[i].abs() == f64::MAX`)
would have been wrong in the other direction: C takes the bounded branch for
`lbnd[i] == +DBL_MAX`.

The gate is `an_axis_unbounded_above_gets_a_zero_width_default_uncertainty` in
`crates/ast-core/tests/test_regions.rs`, which builds a one-axis `Interval` with
no upper limit, confirms `reg_base_box2` reports `+f64::MAX` for it, and asserts
both default-uncertainty bounds are zero. Before the fix it measured a half-width
of about 9e307; the guard is wrong on its own terms whether or not a Region using
the base rule can reach an unbounded axis today, so the direct unit test is the
gate rather than a fixture.

### 10.13 Box's PermMap simplification branch was unported — FIXED

`box.c:3719-3882` is an explicit branch for the case where a Box's base-to-current
Mapping is a PermMap. It reads the PermMap's permutation arrays and constants
directly and builds the current-Frame Region axis by axis:

- an output fed a constant gets that constant as both its limits;
- an output fed `AST__BAD` is unbounded, so the result is an Interval, not a Box;
- an output fed an input takes that input's limits;
- a missing bound likewise makes the result an Interval;
- when base axes are absent from the current Frame, the slice the PermMap selects
  is checked for intersecting the Box, which can make the result a NullRegion.

`BoxRegion::simplify_over_permmap` (`box_region.rs:426-525`) now carries all five
cases in C's order, ahead of the generic best-fitting-Box path, and the branch is
a downcast on the base-to-current Mapping (`box_region.rs:1629`). A permutation
that is not bi-directional returns `None`, which clears `simpler` and so discards
the parent simplification along with it, as `box.c:4064` does.

Two conventions had to be translated rather than copied. The port stores the
public 1-based permutation form (`+j` names axis `j`, `0` names `AST__BAD`, `-j`
names `constants[j-1]`) where C stores the 0-based form and `astPermMapId_`
(`permmap.c:2678-2707`) turns a public `0` into `nin` / `nout`; C's `feed >= nin`
test is therefore `feed == 0 || feed > nin` here. And C's `isNull` is an
assignment rather than an accumulation, so the last constant-fed input decides —
mirrored deliberately and recorded in `docs/issues/c-library-quirks.md`.

Three fixtures gate it: `cap_box_permmap_const` (an output pinned to a constant),
`cap_box_permmap_null` (a slice that misses the Box, giving a NullRegion) and
`cap_box_permmap_nonbidi` (a non-bidirectional permutation, its own reference
because C does not simplify it).

**Which construction the generator needed.** The public `astMapRegion` is
`astMapRegionId_`, whose tail simplifies the Region it is about to return
(`region.c:14020-14024`), so a fixture built through it holds the *answer* in its
`.map` and proves nothing — the same defect §10.17 records for
`normmap_frame_simplifies`. `astBox` on a FrameSet is no escape either: `astInitRegion`
resolves a supplied FrameSet to its current Frame. The three fixtures therefore call
the protected `astMapRegion_` (`region.c:5375`), which does not simplify, reaching it
through `astMakePointer` and using the protected `astWrite_` / `astSimplify_` on the
true Object pointer it returns. Any future fixture over a Region's base-to-current
Mapping needs the same construction.

**The unbounded exit has no oracle fixture.** `astMapRegion` refuses to build a
Region whose forward transformation gives a bad axis value, reporting
`AST__NODEF` (`region.c:5539`), and it is the only public way to install a
base-to-current PermMap — so no C program can produce a Box whose PermMap feeds
an output with `AST__BAD` (audit rows 21 and 23). It is covered instead by
`box_region.rs::tests::permmap_bad_output_simplifies_to_unbounded_interval`,
which performs the same FrameSet edge surgery `map_region` performs, without the
`AST__NODEF` check, and asserts the result is an Interval unbounded on the
affected axis. What would expose a divergence against C: a C path that installs a
base-to-current PermMap without that check — reading a Native Region dump whose
`FrmSet` already carries one is the only candidate, and nothing in the corpus
writes such a dump.

### 10.17 NormMap does not simplify its encapsulated Frame

`normmap.c:619-631` is the *first* thing C's `MapMerge` does: `astSimplify` the
encapsulated Frame, and if that returns a different object, replace the NormMap
with a new one wrapping the simplified Frame and report a change. Only if the
Frame did not simplify does C reach the basic-Frame test that turns the NormMap
into a UnitMap (`:632-640`), and only after that does it consider neighbours.

`NormMap::map_merge` (`normmap.rs:188-205`) performs the basic-Frame test and
nothing else, so the port never rebuilds a NormMap around a reduced Frame. The
audit recorded both halves as ported on the strength of the one downcast; the
module comment cites `normmap.c:632-640` alone, which is what the Rust code does.

The branch is reachable because `Simplify` is a real Frame-level seam: CmpFrame
overrides it to simplify each component and rebuild
(`cmpframe.c:9018-9034`), and every Region class overrides it too, so a Frame *can*
simplify to a different Frame.

**A fixture was written for it and does not reach it.**
`gen_simplify_fixtures.c:2612-2632` builds `normmap_frame_simplifies` as a NormMap
over a Box whose FrameSet holds `CmpMap(ZoomMap(2), ZoomMap(3))` in series,
expecting the Box to reduce. It does not: `astBox` resolves the FrameSet to its
current Frame at construction, so the dumped `.map` already holds a Box over a
plain `ZOOMED` Frame with the bounds transformed. Both `.map` and `.simp` carry the
same Box, and the port passes the fixture by declining a branch it has not ported.
The harness compares correctly; the fixture cannot discriminate, because the
reduction it was built around happened before `astSimplify` was ever called. That
is why the mis-verdict survived.

`astBox` is not the only thing standing in the way: the public `astMapRegion` is
`astMapRegionId_`, which simplifies the Region it returns (`region.c:14020-14024`),
so building the FrameSet after construction is no escape either. §10.13's fixtures
show the way round both — call the protected `astMapRegion_` through
`astMakePointer` and write the result with the protected `astWrite_`.

What would expose it: an encapsulated Frame whose reduction is *not* absorbed by
its own constructor. A CmpFrame is the direct route — `cmpframe.c:9019-9034`
rebuilds only when a component Frame simplifies, so a CmpFrame holding a Region
that reduces gives `astSimplify` something to return. Compare the `.simp` on the
NormMap's `Frame` value, not just the card count: C writes the reduced Frame and
the port writes the original.

Nothing in `ast-core` can currently supply that CmpFrame. `FrameSet` is the only
type under `crates/ast-core/src/frames/` that implements `Mapping`
(`frames/frameset.rs:1561`), so `CmpFrame` has no `simplified` to override and
there is no Frame-level simplify seam for a NormMap to call. C's
`Frame::Simplify` is inherited by every Frame through the Mapping vtab, which is
why `cmpframe.c:9019-9034` can override it. Closing this section therefore means
adding the seam, not just the NormMap branch: the fixture needs a Frame that
simplifies to a different Frame, and the port has no such Frame yet.

**FrameSet's half is closed.** The three `frame_frameset.md` rows that shared this
section — 10, 12 and 14, C's `tmp != map` per edge, the `simpler` flag it raises
and the `astClone( simpler ? new : this )` return — were gaps because the seam
returned a Mapping without saying whether it had reduced anything, so
`FrameSet::simplified` handed back its working copy unconditionally.
`Mapping::simplified` now returns `(DynMapping, bool)` and all three are ported.
`cap_frameset_nochange` is the fixture: a FrameSet whose one edge is a UnitMap,
gated by `frameset_with_no_reducible_edge_returns_the_original` in
`test_simplify_seams.rs` rather than the simplify manifest, because that harness
reduces a FrameSet input to its base→current Mapping first. What remains open here
is the NormMap branch and the Frame-level seam it needs.

### 10.18 Polygon's vertex-transform simplification skips both of C's guards

`polygon.c:5379-5437` re-expresses a Polygon in its current Frame by
transforming the vertices, and guards the result twice before adopting it:

- **Bad vertices.** C copies the transformed axis-1 values and clears `ok` if
  any is `AST__BAD` (`:5400-5407`), building the new Polygon only when `ok`.
  Axis 2 is copied without being tested — a C asymmetry, not a deliberate rule.
- **Bent edges.** When `SimpVertices` is zero, C meshes the *original* Polygon
  and requires `astRegPins` to place every mesh point on the new Polygon's
  boundary, discarding the new Polygon if not (`:5412-5428`). This is the whole
  purpose of the attribute: a non-linear Mapping can carry the vertices to the
  right places while bowing the edges between them.

`Polygon::simplified` (`polygon.rs:1095-1149`) does neither. It transforms the
vertices and adopts the result unconditionally. It also builds the new Polygon
with `this.data.uncertainty.clone()` — the uncertainty as stored, in the base
Frame — where C passes `astGetUncFrm( new, AST__CURRENT )`, the uncertainty
mapped into the Frame the new vertices live in (`:5384`, `:5411`).

`SimpVertices` does not exist in the port at all: no attribute, no accessor, no
serialisation of the `SimpVT` card C writes at `polygon.c:6692`. The default is
1 (`polygon.c` `astMAKE_GET(Polygon,SimpVertices,...)`), and on the default the
bend check is skipped, so the missing check is invisible until something sets
the attribute to zero — which the port cannot express.

What would expose it: a fixture with a Polygon whose base-to-current Mapping is
non-linear enough to bow the edges (a `WcsMap`, or a `PolyMap` with a quadratic
term), `SimpVertices` cleared, and an uncertainty large enough for `astRegPins`
to be the deciding test. C keeps the original Polygon; the port returns the
transformed one. A second fixture, with one vertex mapping to `AST__BAD`, covers
the `ok` guard. The uncertainty-Frame divergence needs a Polygon with an
explicit uncertainty and a non-UnitMap base-to-current Mapping: compare the
`Unc` value written under the simplified Polygon.

### 10.19 Two pieces of the simplify engine had no counterpart in C — both closed

Both sat in `mappings/simplify.rs`, in the port of `cmpmap.c::Simplify`. They are
listed together because the question they raised is the same: what did the engine
do that C's does not, and what showed it?

**A recursion-depth cap of ten, and a structural hash standing in for
`astEqual` — CLOSED.** `SimplifyGuard::enter` refused to simplify at all once ten
nested `simplify_mapping` calls were on the stack, returning the mapping as
supplied, and compared a structural hash (`mapping_fingerprint`) where C compares
Mappings. C has neither: `cmpmap.c:3487-3498` keeps `simplify_stackmaps` purely to
detect a Mapping *already being simplified*, holds one `astClone`d Mapping per
depth, tests each with `astEqual`, and grows the array with `astGrow` without
bound. Both substitutes declined reductions C performs — the cap unconditionally
past its depth, the hash whenever two Mappings of the same class and shape differ
only in their numbers. A constant with no analogue in the C source is stop signal
1, and this one changed the answer rather than merely costing time.

The guard is now C's: a `Vec<DynMapping>` pushed and popped by the same RAII
guard, with `Mapping::equal` as the test.

The hash half closed by demonstration. Instrumenting the guard to run both tests
and report where they part company found exactly one disagreement across all 454
corpus fixtures — `box_unc_fillfactor_merge`, at depth 1, where the hash claimed
a cycle and `equal` did not — and that fixture is byte-exact against the pinned C
now that the guard recurses there. Its `SERIAL_UNGATED` row under cause C is
retired; `nullregion_unc_fillfactor_merge` remains, so the Prism-for-Box
uncertainty defect in §6 cause C is a smaller thing than the pair suggested.

The depth half closed by construction, not by demonstration, and the reason is
worth recording because it says something about the port's shape. Nesting depth in
the *input* does not drive recursion depth here: `flatten` is iterative, so an
eleven-deep left-nested series CmpMap is levelled in one pass and enters the guard
once. `cap_deep_nest_zoom` was generated to reach eleven levels — eleven nested
series CmpMaps each holding a ZoomMap, which C collapses to a single ZoomMap — and
it passed at HEAD with the cap still in place. Instrumenting the refusal path
measured the real ceiling: the whole tier-1/2 suite never exceeds depth 7, and
only `cap_cmpmap_01` and `cap_cmpmap_04` reach that. The depth the engine does
reach comes from `map_merge`-internal simplify calls on trees under construction,
not from how deeply the caller nested its input, so exposing the old cap would
have needed a fixture that drives *that* number past ten. `cap_deep_nest_zoom`
stays in the corpus as a positive control for the collapse C performs, not as
coverage of the cap.

Removing the cap trades a silently-declined simplification for a stack overflow
on a genuine non-terminating recursion. That is C's exposure too, and a crash is
findable; if one appears the bug is in whichever `equal` fails to recognise the
repeat.

**A UnitMap strip that duplicated `UnitMap::map_merge` — CLOSED.**
`merge_loop_restricted` stripped `UnitMap` entries from a series list twice — inside
a fixpoint loop that re-ran the whole nominate loop after each strip, and again
after it — where C strips a series UnitMap in `UnitMap::MapMerge`, when the UnitMap
is nominated (`unit.rs:112-115`, `unit.md` rows 5-7). Both outer strips are gone;
`merge_loop_restricted` is now the nominate loop and nothing else.

The strip itself still happens, from the nominate loop, which is where C does it:
the whole tier-1/2 suite is green, including the two unit tests written against the
outer strips (`merge_loop_strips_unitmaps_from_series`,
`merge_loop_all_unitmaps_preserves_one`). Removing them also removes a source of
extra nominations, since the in-loop `continue` re-ran the loop over the whole
list after every strip (§10.3).

What they would have got wrong: under a restricted simplify a UnitMap that does
not carry `AllowSimplify` is never nominated, so C cannot strip it, while neither
outer strip consulted the flag. Nothing in `ast-core` sets `RestrictedSimplify`
except reading a `ReSimp` card (`cmpmap.rs:1034`), and nothing sets
`AllowSimplify` except reading an `AlSimp` card (`serial/mod.rs:160`), so that
case had no fixture in either direction until the three restricted-mode pairs of
§10.20 supplied one.

### 10.20 The restricted-simplify path dropped three of C's steps — FIXED

C's `Simplify` treats a restricted simplify as a distinct mode. Three of the
things it does in that mode had no counterpart in the port; all three are now
ported, and each has a fixture pair that fails without it.

**`simpler` is reset — FIXED (`simplify.rs:3337`).** `astMapList` sets `simpler`
whenever it meets an inverted CmpMap during the decomposition, and C then
discards that flag under a restricted simplify — it cannot know yet whether the
inverted CmpMap will be eligible, so it declines to count the decomposition
itself as an improvement (`cmpmap.c:3526-3530`). The port computed the same flag
and never consulted `restricted`, so where C returns `astClone( this )` the port
rebuilt the tree: components got swapped and inverted, and the result
right-associated. `cap_restricted_no_eligible` is the gate — a restricted CmpMap
holding an inverted CmpMap with nothing marked eligible, whose `.simp` differs
from its `.map` only in the dropped `ReSimp = 1` card.

**`AllowSimplify` is cleared on each component — FIXED
(`simplify.rs:3380-3382`).** C clears the flag on every component as it folds the
result back into a CmpMap (`cmpmap.c:3689-3692`), explicitly so the protected
flag does not appear in a public dump. C does this only in its multi-element
branch; a single-element result is an `astCopy` (`cmpmap.c:3651`) whose flag
`astSimplify` clears on the result instead. `cap_restricted_component_flags` is
the gate — three eligible components under a restricted CmpMap, `AlSimp` on
three cards in the `.map` and on none in the `.simp`.

**`astSimplify_`'s tail is C's — FIXED (`simplify_entry.rs:105`).** Under a
restricted simplify C clears `RestrictedSimplify` on the result and does *not*
set `IsSimple` (`mapping.c:24779-24786`), then clears `AllowSimplify` on the
result whichever branch ran (`mapping.c:24788`). `cap_restricted_inverted` is the
gate: its `.simp` carries neither `ReSimp` nor `IsSimp` nor `AlSimp`.

The mode's fourth consult point, the base driver's opening
`if( astRestrictedSimplify(this) ) { if( !astAllowSimplify(this) ) return
astClone(this); }` (`mapping.c:16853-16855`), is ported at `simplify.rs:1140`.
`RestrictedSimplify` moved from a `CmpMap` inherent method to a `Mapping` trait
pair, matching C's shared `flags` word (`mapping.h:415-482`); only `CmpMap` has
storage for it, the only class that can hold components to restrict.

The fixtures are generated by `gen_audit_gap_fixtures` in the submodule's
`gen_simplify_fixtures.c`. Setting the two protected flags needs a separate
translation unit — `gen_simplify_flags.c`, compiled with `astCLASS` and the
internal headers, converting IDs with `astMakePointer_` — because the generated
`build/ast.h` strips every `astCLASS` section, so defining the macro in a program
that includes only `ast.h` does nothing. That TU must also be compiled with
`THREAD_SAFE`, which is `PRIVATE` to the `ast` cmake target: without it the
`AstObject` layout it sees differs from the library's and the flag macros write
over the lock field.

`YamlChan` remains the only producer of these flags in C (`yamlchan.c:10998` for
`RestrictedSimplify`, `:9505`, `:9627` and `:10220` for the rad/deg ZoomMaps it
flags with `AllowSimplify`); in the port they still arrive only by
deserialisation (`cmpmap.rs:1034`, `serial/mod.rs:160`). Porting `YamlChan` would
exercise the mode as C's own callers do.

### 10.21 `astDoNotSimplify` and the Mapping-level `Ident` it reads — FIXED

C gates simplification on a per-class protected method with two callers, both in
generic drivers.
`astSimplify_` refuses to reach the `Simplify` slot when it returns non-zero
(`mapping.c:24764`), and `astMapList` refuses to decompose a CmpMap when it does
(`cmpmap.c:1154`), so a flagged Mapping is neither simplified nor taken apart.
The base rule is "the `Ident` attribute is set" (`mapping.c:1107-1152`), on the
reasoning that a user who named a Mapping wants it to survive intact.
`Frame` overrides it to return zero always, so Frames are simplified despite a set
`Ident` (`frame.c:3511-3554`).

The port had neither the seam nor the attribute it reads, so nothing in it could
put a Mapping into the state C checks for — the gap was inexpressible rather than
merely untested.
Both are now `Mapping` trait methods: `ident` / `set_ident` / `clear_ident` /
`test_ident` over `Option<String>` storage on all 26 concrete classes, and
`do_not_simplify`, whose default body is `self.test_ident()` exactly as C's is
`astTestIdent( this )`.
The predicate is consulted at both of C's sites, sharing the `astClone( this )`
exit with the `IsSimple` short-circuit in the gate (`simplify_entry.rs`) and
guarding the decomposing branch of `flatten`, the port of `astMapList`.
Every `Region` and `FrameSet` overrides it to `false`, mirroring C's `Frame`
override.
Storage is `Option<String>` rather than a `String` tested for emptiness because
C's rule is `astTestIdent`, which is `this->ident != NULL` (`object.c:5229`) and
so distinguishes an `Ident` set to `""` from one never set.
Serialisation follows C's Object dump — the card is written when set and
non-empty, before the `IsA Object` marker (`object.c:1697`) — and the reader
applies it generically for every Mapping class.

Two things surfaced while closing it.
`FrameSet` now owns its own `Ident` instead of forwarding to its current Frame,
which is what C has done since its 30-OCT-2009 change (`frameset.c:2759`,
`:4841`, `:10830` all pass `this`); `test_frameset_repair.rs`'s
`frameset_delegates_domain_but_owns_ident` pins both halves.
And `CmpMap::equal` needed a termination guard: it uses `flatten` as a
canonicaliser, so two `Ident`-bearing same-kind CmpMaps each flatten to a
one-element list holding themselves, and the element-wise comparison
(`cmpmap.c:399`) is then the comparison it started with.
C has the same unbounded recursion; it is reachable only when *both* sides
refuse, and there the port decomposes one level of children instead, which is the
same comparison C would make with either side's flag cleared.

Two fixtures cover it, both generated by `gen_simplify_fixtures.c`.
`cap_ident_cmpmap` is the gate: a series `CmpMap(CmpMap(Zoom2, Zoom3), Zoom5)`
with `Ident` on the inner CmpMap, which C leaves whole — including the outer
ZoomMap, which cannot merge across a named component.
`cap_ident_frame` is the negative control for the `Frame` override: a FrameSet
with `Ident` set on the FrameSet itself, whose edge C still simplifies to a
single `ZoomMap Zoom = 6`.
Registering the second one required `test_simplify_regression.rs` to stop
pre-reducing every FrameSet input to its base→current Mapping; it now takes the
comparison shape from the reference's class, so a reference that is itself a
FrameSet is compared against simplifying the FrameSet.

### 10.22 The public entry performs one tree rewrite C's `astSimplify_` does not

`mapping.c:24755-24808` adds no algorithm: an `IsSimple` short-circuit, the
`Simplify` call, the stamp, and the two flag clears of §10.20.
`<dyn Mapping>::simplify` (`simplify_entry.rs:36-82`) also rewrites the result
tree once before returning it.

- `normalize_unitmap_children` (`cmpmap.rs:223`, called at
  `simplify_entry.rs:70`) drops redundant `InvA`/`InvB` flags on a parallel
  CmpMap whose corresponding child is a UnitMap. It is load-bearing and measured:
  §10.15 records four `equal()` failures and two serialisation failures without
  it, and it is one of the two layers that reproduce C's inversion push-down.
- `narrow_unit_scale_winmap_children` — **FIXED**. The second rewrite converted
  an all-unit-scale WinMap child to a ShiftMap at the outermost public call,
  standing in for C's final `winmap.c:1819` step. It was measured dead at pin
  `07fbe2dc` and deleted along with `PublicSimplifyGuard` and the method itself:
  0 failing suites, the 453-fixture corpus at `0 equal() / 0 transform / 9
  serialization / 1 other`, and all 194 `wcsconv` byte comparisons, identical
  before and after. The narrowing happens where C does it, in `win_map_merge`'s
  closing pass reached through `flatten`'s `defer_winmap`;
  `unit_scale_winmap_in_a_parallel_branch_narrows_to_a_shiftmap`
  (`simplify.rs`) holds that, using a PermMap sibling the WinMap cannot merge
  with so the narrowing cannot be a side effect of a series merge.

The section stays open for `normalize_unitmap_children`, which §10.15 measured as
load-bearing.
