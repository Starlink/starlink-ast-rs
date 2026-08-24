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
| simplify regression | 451 | 4 | `skip_string_compare` in the manifest (`rigby`, `cap_cmpmap_01`, `cap_cmpmap_04`, `cap_winmap_03`). `lsst` retired its skip when the manifest gained a `render_options` field: its reference was captured with the Channel's `Comment` and `Indent` off, and rendering the same way makes it byte-exact |
| simplify regression | 451 | 2 | `ISSIMP_PLACEMENT_LICENSED` — same counted form as the wcsconv row (`cap_cmpmap_03`, one card short of C; `cap_specmap_02`, two over) |
| simplify regression | 451 | 15 | fixtures whose `IsSimp` card count still differs from C. Two under-tag (`rigby`, `cap_cmpmap_03`); ten over-tag by one or two cards on a leaf `try_self_reduce` reduced through the gate, which C reaches only through `astMapMerge` and so never stamps (`cap_cmpmap_01`, `cap_cmpmap_04`, `cap_specmap_02`, `cap_slamap_03`, `cap_wcsmap_01`, `cmpmap_perm_parallel_swap`, `cmpmap_perm_swap_aconstants`, `cmpmap_perm_swap_bconstants`, `lsst`, `parpair_one_side_reduces`) and close with §10.16; three are the Region-simplify shape gaps (`cefit_circle_conformal`, `cefit_circle_nonconformal_keep`, `cap_zoommap_04`). Not a separate mask: they sit inside `skip_string_compare`, the counted licence above, or `SERIAL_UNGATED`. Listed because otherwise the number is invisible, and asserted as a set in `test_simplify_regression.rs` so it cannot grow. A stale fixture is no longer a possible cause: the corpus was regenerated against the pin at `f6762005` |
| simplify regression | 451 | 141 | `ignore_is_simp_line` for the negative fixtures, whose reference *is* their input and so cannot carry the tag `simplify` always stamps. The companion assertion requires the result to carry at least one, so the strip cannot hide an untagged result. 134 with `neg_time_mjdtojd_real` (§10.9), `neg_unit_parallel_forward` (§10.6) `neg_parpair_aligned_irreducible` (§10.14), `neg_lut_near_linear` (§10.8), and five upstream repaired at pin `8d4f526f` |
| transform oracle | 1606 sections | 0 | all three `*_ORACLE_EXCLUDED` lists are empty |
| transform oracle | 1606 sections | 2 | `tnx-cheb` round-trip off (ChebyMap cannot represent its own inverse); one UnitNormMap inverse golden off (BAD-vs-noise at a singular point) |
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
`cap_specmap_02` moved to the counted licence. The negative-fixture row went from 132 to
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

`cap_cmpmap_03` and `cap_specmap_02` on the simplify corpus are one card too
few, from the same input on both sides. Six fixes were tried and measured, and
all were reverted — including tagging every `simplify_mapping` result, which is
`astSimplify_`'s literal contract, and deferring leaf reduction entirely. The
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
| check_transform_oracle | crates/ast-core/tests/test_transform_oracle.rs | ported 2026-08-05 (submodule `3a3c3e51`); consumes `tests/fixtures/oracle/*.oracle`; all 1606 sections gated by three asserting tests (`head_`, `slamap_`, `simplify_sections_match_oracle`), with `every_oracle_section_is_gated` proving the partition covers the corpus; `transform_oracle_report` remains `#[ignore]` as the investigative view; `gen_transform_oracle` intentionally not ported |
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
are enumerated with causes in `SERIAL_UNGATED`, which groups them into seven:

| cause | fixtures | what is wrong |
|---|---|---|
| A | 4 SwitchMap cases | `IsSimp` missing on the FSMap / ISMap sub-Mappings |
| B | 2 `cefit_circle_*`, `neg_box_asymmetric_2d` | Region simplify tags a Mapping inside the encapsulated FrameSet that C leaves bare, and does not tag the returned Region at all |
| C | 2 `*_unc_fillfactor_merge` | emits a `Prism` uncertainty where C emits a `Box` |
| D | `cap_zoommap_04` | emits `FrmSet`/FrameSet where C emits `Points`/PointSet |
| E | `cap_interval_01`, `cmpmap_parallel_series_dim_mismatch` | `Closed = 1` where C omits it; two `IsSimp` cards on the leaves where C has them on the intermediate CmpMaps |
| F | `cap_pointlist_01` | two mesh coordinates diverge past 4 ulp |
| G | `lsst`, `cap_slamap_03`, `cap_wcsmap_01`, `parpair_one_side_reduces`, `cmpmap_perm_parallel_swap`, `cmpmap_perm_swap_{a,b}constants` | one or two extra `IsSimp` cards, and nothing else in the tree, on a leaf `try_self_reduce` reduced through the gate. C reaches a leaf only through `astMapMerge` — verified on the tracing oracle, which emits no `SIMPLIFY-ENTER` for the leaf class in any of these — so C stamps no card there. Closed by §10.16 |

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

## 10. Gaps named by the `MapMerge` / `Simplify` audit

Each entry is cited by a `gap` verdict in a checklist under
`docs/audits/mapmerge/`, and `test_mapmerge_audit.rs` fails if a cited entry
does not exist here.

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
`RateMap::simplified` now orients to `invert_snapshot` first — the field the port
already had for the `Inv` card and was using only for serialisation. That is the
H3 hazard: two C names kept as two fields, with only one of them wired up.

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

### 10.2 ChebyMap does not inherit PolyMap's `MapMerge`

In C, `chebymap.c` defines no `MapMerge`; only `polymap.c:2439` fills the slot,
so a ChebyMap's `MapMerge` is PolyMap's whole body at `polymap.c:3597-3961`. In
the port, `ChebyMap` is its own struct with no `map_merge`, so it takes the trait
default and reaches none of PolyMap's replace-with-simpler half. Its
`simplified()` (`chebymap.rs:454-460`) returns an unchanged clone.

Two halves are missing, and they are not equally safe to port.

The coefficient consolidation (`polymap.c:3624-3745`) combines coefficients that
use the same set of input powers. That is basis-independent — two `T_2` terms for
one output add exactly as two `x^2` terms do — so it ports as it stands.

The linear replacement (`polymap.c:3747-3875`) is the open question. C reads the
coefficient arrays with nothing that distinguishes a Chebyshev basis from a power
basis, and accumulates a matrix and offset from them directly. A first-order
Chebyshev term is affine in the input, but only after the domain scaling the
ChebyMap's `lbnd`/`ubnd` define, and that scaling takes no part in what C
accumulates. So either C folds the scaling in somewhere this audit has not
found, or a linear ChebyMap simplifies to a MatrixMap that has lost it.

What would expose it: a ChebyMap whose forward transformation holds only `T_0`
and `T_1` terms, over bounds that are not the identity interval, transformed
through both the ChebyMap and its simplification. Build it against the C oracle
first, per `docs/issues/c-oracle-probe-recipe.md`. If C loses the scaling, the
port must not mirror it, and the finding belongs in
`docs/issues/c-library-quirks.md` and upstream. If C keeps it, the port needs the
same delegation.

No corpus fixture holds a linear ChebyMap, which is why neither answer has been
needed yet.

### 10.16 The port has a per-class `simplified()` seam that C does not

C has **two** per-class seams in the simplify machinery:

- `Simplify`, overridden by `CmpMap`, `FrameSet` and the nine Region classes and
  by nobody else. For every other class the base implementation
  (`mapping.c:16752`) builds a one-element list and calls
  `astMapMerge( map, 0, 1, &nmap, … )` **repeatedly** until nothing changes or
  the list grows past one (`mapping.c:16871-16883`).
- `MapMerge`, overridden per class.

So for a class outside that 11, every reduction C performs lives in `MapMerge`,
and `astSimplify` reaches it only by nominating the Mapping in a list of one.

The port has **three**: the public `simplify()`, a per-class `simplified()`, and a
per-class `map_merge()`. `simplified()` has no C counterpart for the other 25
classes, and the port put per-class reductions there.
`grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c` is the
authority: the split is exactly right for 11 classes and exactly wrong for 25.

**This is the H1 hazard, and it has produced three of this session's bugs.**
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

### 10.3 The four under-tagged fixtures are under-*merged* — it is WinMap

`cap_cmpmap_01` (16 vs C's 18), `cap_cmpmap_03` (6 vs 7), `cap_cmpmap_04`
(15 vs 18) and `cap_specmap_02` (7 vs 8) emit fewer `IsSimp` cards than C. The
title of this entry was wrong for a long time: the cards are missing because the
*reductions* are missing, not the stamps.

Both engines are instrumented — `build_simplify_trace.sh` for C,
`simplify::trace` behind the `tracing` feature for the port — and log each
nomination and what it returned. On `cap_specmap_02`:

| | C | port |
|---|---:|---:|
| nominations | 96 | 149 |
| **productive** | **23** | **15** |
| declined | 73 | 134 |

C's nominations succeed 24% of the time, the port's 10%. The port nominates 53
more times and merges 8 fewer. Broken down by the class nominated:

| class | C productive | port productive |
|---|---:|---:|
| **WinMap** | **10** | **2** |
| CmpMap | 5 | 5 |
| UnitMap | 3 | 3 |
| ShiftMap | 3 | 3 |
| ZoomMap | 1 | 1 |
| SpecMap | 1 | 1 |

**The whole deficit is WinMap.** Every other class agrees exactly. C's
`WinMap::MapMerge` is productive ten times on this fixture and the port's twice.

That also explains why stamping harder cannot work: blanket-stamping
`CmpMap::simplified` moved `cap_cmpmap_04` one card closer *and* gave
`cap_zoommap_01` a card C does not emit. It stamps nodes the port produced and C
did not, while still missing the ones C produced and the port did not.

**Upstream has since fixed a C bug in the same branch.** At pin `8d4f526f`,
`cmpmap.c` commit `4f2fc0ed` — *"test for simplification before annulling the
pointer under test"* — repairs the parallel-CmpMaps-in-series case: the annul of
`tmap` ran before the `tmap != tmap2` test, so the test was always true and the
pair was rebuilt as a parallel CmpMap of series compositions **even when no
component reduced**. The sibling branch 120 lines earlier had it in the right
order.

The regenerated `cmpmap_parallel_series_dim_mismatch.simp` carries two more
`IsSimp` cards than the stale copy, and the port emits one where C now emits
three. That is a new, licensed disagreement in exactly the branch this entry is
narrowed to, and it is better evidence than anything measured before it: the C
side of the comparison was wrong until now, so any conclusion drawn about this
branch before pin `8d4f526f` was drawn against a buggy oracle.

Re-measure the nomination counts against the new pin before doing anything else
here.

**Narrowed to one nomination.** Filtering both traces for WinMap and ShiftMap
with their outcomes:

- ShiftMap agrees exactly, all eight nominations, outcome for outcome.
- Both merge `ShiftMap where=0 series=1 nmap=4` and both report `result=0`.
- C's *next* nomination is `WinMap where=0 series=1 nmap=4` and it succeeds. The
  port's next is `WinMap where=0 series=1 nmap=3`.

Same class, same index, same reported result — and afterwards **C's list still
holds 4 elements and the port's holds 3.** The port's ShiftMap nomination
shortened the list where C's did not, so the WinMap opportunity C exploits never
arises, and the remaining WinMap deficit follows from that one step.

`ShiftMap::map_merge` converts to a WinMap, delegates to `win_map_merge`, and
reverts when that produced nothing (`shift.rs:168-238`, C `shiftmap.c:671-745`).
C's revert guard is `result == where && *nmap == old_nmap && astEqual(...)`. Both
report `result == where`; the lengths say the delegated merge did different things.
That single delegation is the whole of §10.3's remaining distance.

Seven approaches to `cap_cmpmap_03` have been measured and reverted, six recorded
in `test_simplify_regression.rs` plus the blanket stamp above. None touched
WinMap.

### 10.4 The CmpMap/PermMap swap is hung off the wrong nominee

`cmpmap.c:1816-2119` swaps a series CmpMap in front of an adjacent PermMap. It
runs when the **CmpMap** is nominated and the PermMap is its lower neighbour.
There is no counterpart on the PermMap side: `permmap.c` contains no reference to
`CmpMap` at all, so a nominated PermMap never attempts this.

The port has it the other way round. `CmpMap::map_merge` has no swap branch, and
the swap lives in `perm_map_merge` (`simplify.rs:1010-1030`), which fires when a
**PermMap** is nominated and looks at the `(where_, where_ + 1)` pair. The
arithmetic is C's — the constant-transform maths is cited to `cmpmap.c:1929-2041`
at `simplify.rs:1002` — but the trigger is the opposite element.

Both find the swap, because the nominate loop offers every element in turn. What
differs is which nomination performs it, hence the index reported and where the
loop resumes. That is the H2 hazard: a structurally different route to the same
Mapping, surfacing wherever something consumes the structure rather than the
Mapping.

It also means the two sides disagree about *which pair*. C looks behind the
nominated CmpMap; the port looks ahead of the nominated PermMap. Those are the
same pair, so a `[PermMap, CmpMap]` run is covered either way — but a nominated
CmpMap whose PermMap neighbour has already been passed by the cursor is reached
in C and not here.

What would expose it: a series list holding `[A, PermMap, parallel CmpMap, B]`
where `A` and `B` can each merge with what the swap brings next to them, so the
resume point decides which merge happens first. Replay it through the C oracle
and compare the `IsSimp` cards and the tree shape, not just `astEqual`.

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
that ever matters it will show as a resume-point difference of the kind §10.4
describes, not as a wrong Mapping.

### 10.7 PcdMap's zero-distortion reduction is unreachable from the nominate loop — FIXED

`pcdmap.c:1417-1433` opens `MapMerge` by replacing a PcdMap whose distortion
coefficient is zero with a UnitMap, before any neighbour is considered. The port
has that logic, in `PcdMap::simplified` (`pcdmap.rs:225-236`), but
`pcd_map_merge` (`simplify.rs:2719-2733`) tries the swap and then
`default_map_merge`, and neither consults `simplified()`. A zero-distortion
PcdMap nominated from inside a list therefore stays a PcdMap.

This is the same shape as §10.1: the class reduction exists, and the seam that
would reach it from the nominate loop does not. `self_reduce_map_merge`
(`simplify.rs:1137`) is that seam; five classes use it, and PcdMap and RateMap
do not.

Exposed and closed. `pcd_zero_disc_series` is
`[MatrixMap(2,3), PcdMap(disc=0), MatrixMap(5,7)]`, which the pinned C collapses
to a single diagonal `MatrixMap(10,21)`. `pcd_zero_disc_lone` is the same PcdMap
on its own, and it passed from the start — the logic was in `simplified()` all
along and only the seam was missing, which is exactly what the pair
discriminates.

`pcd_map_merge` now performs C's opening move before the swap, through a new
`try_self_reduce` helper factored out of `self_reduce_map_merge` so the ordering
can differ per class: C puts the swap between the reduction and the plain
neighbour merge, which `self_reduce_map_merge` alone cannot express.

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

`SwitchMap::simplified` now stamps each child, as `RateMap` and `TranMap` already
did. All four fixtures — `switchmap_invert_normalize`,
`switchmap_internal_simplify`, `switchmap_fwdonly_simplify`,
`switchmap_inv_invonly` — pass byte-exact, and cause (A) is retired from both
pinned lists in `test_simplify_regression.rs`. Simplify serialisation failures
drop from 11 to 7.

### 10.11 TranMap's `map_merge` reaches one of C's four self-simplification cases

`tranmap.c:834-897` gives a nominated TranMap four outcomes: normalise an
inverted TranMap to the equal forward form and stop; simplify the two component
Mappings; collapse a TranMap whose components are inverses of each other to one
of them; or fall through to the neighbour merge.

All four are implemented in `TranMap::simplified` (`tranmap.rs:256-324`). But
`TranMap::map_merge` (`tranmap.rs:326-361`) implements only the component
simplification, inline, and never calls `simplified()`. It also preserves
`self.inverted` on the rebuild (`tranmap.rs:349-351`).

A TranMap nominated from inside a list therefore keeps its inversion where C
replaces it, and is not collapsed when its two components are inverses.

The inversion case additionally has the fused shape of §10.10: `tranmap.rs:269`
returns `normalized.simplified()`, so the normalised TranMap is never the whole
result of a simplification on its own and never carries the record C stamps on
it.

What would expose it: a series list holding an inverted TranMap that is not the
whole Mapping. C's serialised tree holds a forward TranMap with the components
swapped; the port's holds an inverted one. Separately, a `TranMap(m, m)` inside a
list, which C replaces with `m` and the port leaves as a TranMap.

Closing it means routing `map_merge` through `simplified()` with C's either/or
ordering, the same change §10.10 needs for SwitchMap.

### 10.12 `GetDefUnc`'s unbounded-axis guard misses the upper bound

`region.c:3560-3573` builds the default uncertainty as a Box covering 1.0E-6 of
the Region's base-Frame bounding box, and skips any axis whose bound is
`±DBL_MAX`, setting both output bounds to zero for it.

The port's guard (`regions/mod.rs:466`) is
`!is_bad(ubnd[i]) && !is_bad(lbnd[i])`, and `is_bad(v)` is
`v == BAD_VALUE || !v.is_finite()` (`mappings/helpers/mod.rs:11-13`) with
`BAD_VALUE == -f64::MAX`.

That catches an unbounded *lower* bound, which is `-f64::MAX` and therefore
`BAD_VALUE`. It does not catch an unbounded *upper* bound: `f64::MAX` is finite
and is not `BAD_VALUE`, so the axis takes the bounded branch and receives a
half-width of roughly `0.5e-6 * f64::MAX`, about 9e301, where C gives it zero.

The correct test is against `f64::MAX` in magnitude, as `Interval` already does
for its own bounds (`interval.rs:160-163`).

Reachability today is limited: `default_bounding_box_unc` is the base-class rule,
and Interval, NullRegion, Prism and CmpRegion — the Regions that can be unbounded
— all override `get_def_unc`. The Regions that use the base rule (Box, Circle,
Ellipse, Polygon, PointList) have finite bounds. So the defect is latent rather
than active.

What would expose it: a Region using the base rule whose `reg_base_box2` returns
`f64::MAX` on an axis, or a direct unit test on `default_bounding_box_unc` with
such a box. The second is the cheaper gate and is worth adding regardless of
whether the first is currently reachable, because the guard is wrong on its own
terms.

### 10.13 Box's PermMap simplification branch is unported

`box.c:3719-3882` is an explicit branch for the case where a Box's base-to-current
Mapping is a PermMap. It reads the PermMap's permutation arrays and constants
directly and builds the current-Frame Region axis by axis:

- an output fed a constant gets that constant as both its limits;
- an output fed `AST__BAD` is unbounded, so the result is an Interval, not a Box;
- an output fed an input takes that input's limits;
- a missing bound likewise makes the result an Interval;
- when base axes are absent from the current Frame, the slice the PermMap selects
  is checked for intersecting the Box, which can make the result a NullRegion.

`crates/ast-core/src/regions/box_region.rs` contains no reference to `PermMap`.
Its simplification goes straight to the generic best-fitting-Box path
(`box_region.rs:1539-1566`), then the Polygon and Prism paths.

A plain axis permutation is handled anyway, because it is exact and the mesh
comparison succeeds. What the generic path cannot produce is a constant-valued
axis, an unbounded axis, or a result that is an Interval or a NullRegion rather
than a Box.

What would expose it: a Box whose FrameSet holds a PermMap with a constant
output — a 3-D Box sliced to 2-D with the third axis pinned — simplified and
serialised. C writes an Interval or a Box with equal limits on that axis; the port
writes whatever the mesh fit produced, or fails to simplify at all. No corpus
fixture builds a Box over a PermMap with constants.
