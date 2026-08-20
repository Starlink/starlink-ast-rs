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

Every relaxation in the C-comparison gates, counted so the total cannot grow
silently. Adding a row here is a deliberate act; the numbers are the claim.

| Gate | Cases | Relaxed | What the relaxation is |
|---|---:|---:|---|
| wcsconv (`test_wcsconv.rs`) | 193 | 8 | `astequal` compare mode — FMA contraction / libm differences vs x86-64 Linux captures that C AST on arm64 does not reproduce either |
| wcsconv | 193 | 5 | `ALIASING_TAG_LICENSED` — drops `IsSimp` / `Invert` tag lines both sides; both are C quirks being fixed upstream (see §8) |
| wcsconv | 193 | 0 | **No skip mode exists.** `NATIVE_READ_EXCLUDED` is empty and gated against refilling |
| simplify regression | 427 | 7 | `skip_string_compare` in the manifest (`lsst`, `rigby`, `cap_cmpmap_01`, `cap_cmpmap_04`, `cap_matrixmap_03`, `cap_specmap_02`, `cap_winmap_03`) |
| transform oracle | 1606 sections | 2 | `tnx-cheb` round-trip off (ChebyMap cannot represent its own inverse); one UnitNormMap inverse golden off (BAD-vs-noise at a singular point) |

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

## 8. Pending upstream sync

The pin is `50c2b21b`, which is still C master's head. The next sync will not be
routine: the unmerged `u/timj/misc-bugs` branch (40 commits, 25 `src/` files,
+872/−235) carries nineteen defect fixes, **four of which change serialized
output**. Task 15 on that branch alone regenerated 69 `.native` fixtures.

The table below covers those four plus one behaviour change that needs no
Rust-side work — listed so nobody spends the sync looking for the fix.

| C change | Port's current position | Action required when the pin moves |
|---|---|---|
| `IsSimp` placement becomes deterministic | Quirk documented; 5 wcsconv cases licensed via `ALIASING_TAG_LICENSED` | Port the deterministic placement rule; retire the licence; byte-exactness becomes achievable on those 5 |
| `WcsNative` no longer inverts the encapsulated PermMap | Deliberately not mirrored (`docs/issues/c-library-quirks.md`); same 5 licensed cases | Retire the licence — the port's shape becomes the correct one with no code change |
| `EncodeFloat` justification fixed | **Mirrored** in `bfacdfe` (length-preserving exponent-zero strip) | Remove the mirror. A faithful mirror of a fixed bug is a bug |
| Mapping immutability enforced in every class (`AST__IMMUT`) | Not applicable to `ast-core` — the condition cannot be constructed (deep `Clone`, single-owner `Box`, `pub(crate)` component accessors) | **No action in `ast-core`.** Do not go looking for a fix: the work lands in `ast-capi`, which restores sharing at the handle boundary. See the §7 note |
| KeyMap dumps emitted in key order | Port already dumps in a stable key order by design | Verify convergence; regenerate KeyMap fixtures |

Sequence: land the C merge, regenerate the affected corpora
(`.native` wcsconv expectations, KeyMap fixtures, any `.simp` carrying an
`IsSimp` tag) in one commit range, then remove the two mirrors and the aliasing
licence. Do not move the pin piecemeal — four output changes arriving separately
would make each fixture regeneration ambiguous.

General rule this instantiates: before moving the pin, diff the incoming range
for serialization changes and check `docs/issues/c-library-quirks.md` for
entries the port *mirrors*.

---

## C → Rust test migration tracker

| C test (`ast_tester/`) | Rust counterpart | Status |
|---|---|---|
| testconvert.c | crates/ast-core/tests/test_convert.rs | ported 2026-06-12 |
| testframe.c | (deferred) | NormUnit(1) via capi units layer; orthogonal to the Frame-base port |
| check_transform_oracle | crates/ast-core/tests/test_transform_oracle.rs | ported 2026-08-05 (submodule `3a3c3e51`); consumes `tests/fixtures/oracle/*.oracle`; all 1606 sections gated by three asserting tests (`head_`, `slamap_`, `simplify_sections_match_oracle`), with `every_oracle_section_is_gated` proving the partition covers the corpus; `transform_oracle_report` remains `#[ignore]` as the investigative view; `gen_transform_oracle` intentionally not ported |
| testkeymap.c | crates/ast-core/tests/test_keymap.rs | ported 2026-08-08 |
| testobjectkeymap.c | (deferred) | tests the Object-associated retained KeyMap (`object.c:1736`, `:2480`), `astGetObjSize` and `astSame`; those calls (`astMapPut0I`, `astMapGet0I`, `astMapSize`) are only reachable through the protected `astGetKeyMap`/`astHasKeyMap` machinery, an explicit spec Non-goal of the owned-value port |
