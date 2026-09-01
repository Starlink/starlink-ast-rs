# Simplify parity completion — design

Date: 2026-09-01
Status: approved
Pin: `reference/starlink-ast` at `3e60abaa` (`v9.2.10-576-g3e60abaa`), frozen for the duration

## 1. Scope and the definition of done

This design closes the simplifier port.
It is the single plan for every outstanding simplify divergence, not a further increment.

### 1.1 What done means

Done means that every step of C's `MapMerge` and `Simplify` logic has a counterpart in Rust at the same conceptual granularity, that the counterpart is recorded in a checklist whose citations resolve at HEAD, and that the equivalence is confirmed byte for byte by the `.map` / `.simp` corpus.

`astEqual` compliance is explicitly not evidence.
The port reached good `astEqual` compliance once before and was believed finished, and what that state concealed was the absence of whole algorithms — `MapSplit`, and the flatten logic in `CmpMap`.
Two Mappings can be numerically equivalent while being structurally different, and the structure is what the simplifier, the serializer and `MapSplit` consume.
Byte-equal output is therefore the only acceptable confirmation, and a passing `astEqual` assertion is never accepted in its place.

C branch coverage is also not the measure.
The simplify corpus was designed to cover C's simplify logic and is treated as adequate for that purpose; the deficiency being closed here is that the Rust port has not followed C's logic in all cases, not that the corpus is thin.
Accordingly, the audit demonstrates that nothing has been missed by comparing Rust against C source structure, and not by measuring which C branches a fixture happens to reach.

### 1.2 Concrete exit state

At completion:

- `SERIAL_UNGATED`, `NO_ISSIMP_CARD_UNGATED` and `ISSIMP_DISAGREEMENTS` are all empty.
- The four `skip_string_compare` opt-outs in `simplify_tests.txt` are gone, so all 468 fixtures byte-compare.
- Both entries in `crates/ast-core/tests/fixtures/oracle/transform_oracle_overrides.txt` are gone.
- Every audit row cites a line that exists at HEAD and carries an explicit granularity verdict.
- The verification-posture ledger holds no non-zero relaxation other than the two approved exclusions.
- `PLAN.md` sections 9.1, 9.2, 9.5, 9.6 and 10.3 close.

### 1.3 Exclusions

Only the following are out of scope, and each is out for a stated reason that is not difficulty and not absence of a fixture.

| Excluded | Reason |
|---|---|
| `IntraMap`, `XphMap`, `StcResourceProfile` | Unported classes, not relaxations of a gate that could otherwise pass. Tracked by `DEFERRED_CLASSES`. |
| The `InvA` / `InvB` dump line in `cap_cmpmap_01` and `cap_cmpmap_04` | Depends on C pointer aliasing, which owned Rust values cannot reproduce. Recorded in `docs/issues/c-library-quirks.md`, never mirrored in Rust code. |
| Four wcsconv reader-order `IsSimp` placements | C's reader reduces Mappings as it assembles them, so its cards record that reader's order; this port simplifies once at the end. The Mapping is identical. Tracked by `ISSIMP_PLACEMENT_LICENSED` in `test_wcsconv.rs`. |
| ChebyMap's linear-reduction half (`PLAN.md` §10.2) | Needs an upstream C fix and two fixtures that live in the submodule, so it runs through a pin move. Deferred, with an `UNPORTED` marker in `ChebyMap::map_merge`. |
| The 51 fixture files with no transform-oracle section (`PLAN.md` §9.4) | Closed by the maintainer at the next upstream sync, not by this plan. |

Nothing is excluded for being hard, and nothing is excluded because no fixture exercises it.
Where a C branch has no fixture, this plan generates one.

## 2. Clusters

Work proceeds as paired closeout by cluster: each cluster ports a group of related C logic and drives the fixtures that group unblocks to byte-equality in the same commit range.
Clusters are ordered so that shared primitives land before their consumers, and so that a regression is attributable to one cluster.

Order: 1 → 2 → 3 → 5 → 6 → 7 → 4.

### 2.1 Cluster 1 — Region `IsSimp` stamping

Scope: the stamping rule at `mapping.c:24764`, the Region base, `Circle`, `Box`, and `FrameSet`.

Closes `cefit_circle_conformal`, `cefit_circle_nonconformal_keep`, and `neg_box_asymmetric_2d`.

C stamps whatever `astSimplify` returns, guarded by `if( !astGetIsSimple( this ) && !astDoNotSimplify( this ) )`.
The port does not stamp a Region result.
The root cause is genuinely unknown at the time of writing: C dumps a freshly constructed `ZoomMap` with `flags = 0`, and `mapping.c:23608-23648` clears only `report`.
The `do_not_simplify` half of the guard is already ported faithfully — `mappings/mod.rs:666` carries the base `Ident` rule, and each Region overrides it to `false` exactly as `frame.c:3511` does — so the defect lies elsewhere and the cluster begins by reading the C source rather than by patching.

This cluster is first because the rule is shared by every consumer downstream.

### 2.2 Cluster 2 — CmpMap flatten and WinMap merge

Scope: `CmpMap` flatten, `recombine_distributed_branches`, `CmpMap::map_merge`, `WinMap::map_merge`, and the `MatrixMap` inverse.

Closes the seven CmpMap tags of `PLAN.md` §10.3 — `cap_specmap_02` at 7 tags against C's 8, `cap_cmpmap_01` at 16 against 18, `cap_cmpmap_04` at 15 against 18 — the sub-ulp shift residue in `cap_winmap_03`, and the near-zero inverse-matrix element in `rigby`.

All four `skip_string_compare` opt-outs live here: `rigby`, `cap_cmpmap_01`, `cap_cmpmap_04`, `cap_winmap_03`.
Removing them is this cluster's headline result, less the aliasing line named in §1.3.

### 2.3 Cluster 3 — Region simplify results and singularity handling

Scope: `NullRegion`, `Prism`, `Box`, `Interval`, `PointList`, the Region mesh, and `astNormBox`.

Closes `nullregion_unc_fillfactor_merge` (the port emits a `Prism` where C emits a `Box`), `cap_interval_01` (a `Closed = 1` card C omits at its default), `cap_pointlist_01` (two mesh coordinates past 4 ulp), and `cap_zoommap_04` (a `FrmSet` / FrameSet where C emits `Points` / PointSet).

This cluster also closes three `astNormBox` gaps found while auditing CmpFrame.
C's `astNormBox` extends a box to cover coordinate-system singularities, and its fourth argument is declared `AstMapping *reg` but is a Region at every call site — `circle.c:1096-1100` copies the Circle, clears `Negated`, and passes it, and `SkyFrame::NormBox` then calls `astTran2( reg, ... )` and reads `AST__BAD` as "outside".
The port replaced that argument with a membership closure, `norm_box(&self, lbnd, ubnd, inside: &dyn Fn(&[f64]) -> AstResult<bool>)` at `frames/mod.rs:1119`, which captures exactly the semantics C uses and is retained.
Only one of C's three call sites is ported, however.
`Circle::reg_base_box` calls it at `circle.rs:376`; `Ellipse::RegBaseMesh` (`ellipse.c:1000`, calling at `:1184`) and `Polygon::RegBaseBox` (`polygon.c:4180`, calling at `:4314`) do not, and `CmpFrame` has no `norm_box` override against `cmpframe.c:5830`.
Both `RegBaseMesh` and `RegBaseBox` are known extension points, and all three gaps feed Region bounds, which feed simplify results.
The closure form means `CmpFrame::norm_box` is written by composing the two component predicates, so it needs nothing from cluster 4.

### 2.4 Cluster 5 — clear-on-set setters

Scope: `PLAN.md` §9.2 — `ZoomMap::Zoom`, `LutMap::LutEpsilon`, `PcdMap::Disco`, and the `GrismMap` macro-form attributes.
`IntraMap` is excluded per §1.3.

### 2.5 Cluster 6 — transform-oracle overrides

Scope: the two entries in `transform_oracle_overrides.txt` — `tnx-cheb.head off` and `neg_unitnormmap_inv_fwd_diffcentre golden-inv off`.

### 2.6 Cluster 7 — instrument sweep

Scope: the residual citation rot of `PLAN.md` §9.5 for every class no earlier cluster touched, and the granularity-verdict column across all remaining audit rows.

Last by necessity: it records the state the other clusters leave behind.

### 2.7 Cluster 4 — a bare Frame where a Mapping is expected

Scope: one failing fixture, and a decision.

`PLAN.md` §9.6 is re-framed by this cluster: it reads as a Frame-wide gap and is in fact a bare-Frame-in-a-`Map<n>`-slot gap.

This cluster is deliberately last and is gated on a question to the AST maintainers, because unlike every other cluster it is not driven by an existing fixture.
Its content is set out in §4.

## 3. The audit instrument

The audit is 38 checklists under `docs/audits/mapmerge/`, holding 1031 rows, 978 Rust citations across 678 distinct sites, and 1151 C citations.
`docs/audits/mapmerge/region_polygon.md` is the model for a repaired checklist: a numbered C-step table, each row citing a C site and a Rust site, each carrying a verdict, followed by prose on the rows that carry real risk.

Three changes make the instrument load-bearing.

### 3.1 A citation that rots must fail a test

Citations become `polygon.rs:1158 (fn simplified)`.
A gate re-derives the enclosing `fn` at the cited line and compares it with the parenthesised name, so a citation that has drifted fails rather than misleading a reader.

The rewrite of 978 citations is script-driven, not hand-edited.
The script that derives the enclosing `fn` from a file and a line is the same derivation the gate re-runs, so the tool and the gate cannot disagree.
Rows are rewritten cluster by cluster as each cluster touches its classes, and cluster 7 sweeps the rest.

This is also what makes the eventual pin move survivable.
The pin stays frozen for the duration of this work so that line numbers can be locked in; when it later moves, the gate names the rows that rotted instead of leaving them silently wrong.

### 3.2 Every row states whether the C step is an extension point

Each row records whether the C step is architecture or ceremony, applying the mechanical test from `docs/porting-hazards.md` H1: does a generic driver in another file dispatch to this through the vtab?
A row whose verdict is architecture and whose Rust site is not a trait method fails a gate.
This is what would have caught the two most expensive mistakes the port has made.

### 3.3 The universe of steps stays C-derived

`C_SIMPLIFY_SEAMS` in `test_mapmerge_audit.rs` already re-derives the seam set from `reference/starlink-ast/src/*.c`, so a seam cannot be omitted from the audit by omitting it from a list.
It is extended one level deeper, enumerating the per-class C *steps* from source, so that an omitted step inside an otherwise-ported body is detectable.

### 3.4 Verdict vocabulary

| Verdict | Meaning |
|---|---|
| `ported` | A Rust counterpart exists at the same conceptual granularity. |
| `n/a: <reason>` | The step has no counterpart because a Rust idiom subsumes it — owned values instead of `astAnnul`, `AstResult` instead of ambient status. |
| `gap: PLAN.md §x` | Missing work, tracked. No row may hold this verdict at completion. |
| `deferred: <class>` | The class is unported per §1.3. |
| `driver: <path>` | The step lives in a generic driver rather than the class. |
| `mirrors-quirk: <anchor>` | The port reproduces a C defect deliberately; the anchor is in `docs/issues/c-library-quirks.md`. |
| `unreachable: <gate>` | Correct to omit while a gated decision stands. Used only for the `cmpframe.c` `Transform` and `Simplify` rows, gated on cluster 4. |

## 4. Region is the only Frame that is a Mapping

In C, every Frame is a Mapping.
`Frame::Transform` (`frame.c:11739`) is literally `astUnitMap( astGetNaxes( this ) )` followed by a transform through it, so a bare Frame used as a Mapping is an exact copy.
`CmpFrame` overrides both `Transform` (`cmpframe.c:9898`) and `Simplify` (`cmpframe.c:8963`), and its `Transform` builds `astCmpMap( this->frame1, this->frame2, 0, ... )` at `cmpframe.c:9975` — a CmpMap whose two components are Frames — then wraps it in PermMaps at `:9997-10018` to undo the CmpFrame's own axis permutation.
C's own documentation for that function states the purpose: the result "will normally be a UnitMap but may not be if the CmpFrame contains any Regions".

The useful half of Frame-as-Mapping is therefore Region-as-Mapping, and the port already has it.

The design position is consequently that **Region is the only Frame that is a Mapping in this port**, which is what the design spec's sibling-trait separation already says.
No reversal of the sibling-trait decision is proposed, and the earlier proposal to implement `Mapping` for seven Frame classes is withdrawn.

### 4.1 A Region is already accepted as both, and that is settled

A Region is a Frame and a Mapping in this port, so it is accepted as a component of a `CmpFrame` and as a component of a `CmpMap`.
That is not a proposal and not open: both nestings are read, written and byte-compared today.
`frame_from_node` carries all nine Region arms and `mapping_from_node_inner` carries the same nine, and the corpus exercises both — fifteen fixtures nest a Region inside a `CmpFrame`, and thirty-five nest one inside a `CmpMap`.
Nothing in this plan changes that, and cluster 4 cannot regress it, because those fifty fixtures are ordinary members of the byte-compared corpus.

### 4.2 The open question is a bare Frame, in one direction only

Two symmetrical-looking questions exist, and only one of them is open.

| Construct | Components | C | This port |
|---|---|---|---|
| `CmpFrame` holding Frames | Frames | accepted | accepted |
| `CmpFrame` holding a Region | Frames — a Region is a Frame | accepted | accepted, 15 fixtures |
| `CmpMap` holding a Region | Mappings — a Region is a Mapping | accepted | accepted, 35 fixtures |
| `CmpFrame` holding a general Mapping | — | **rejected** | rejected |
| `CmpMap` holding a bare Frame | Mappings — a bare Frame is a Mapping in C only | accepted | **rejected — the one gap** |

**A CmpFrame holding a general Mapping is not a gap**, because C refuses it too.
`AstCmpFrame`'s fields are `AstFrame *frame1` and `AstFrame *frame2` (`cmpframe.h:301-302`), and `astCmpFrame_` calls `astCheckFrame` on both arguments before use (`cmpframe.c:10634-10635`), so a Mapping is rejected there in C exactly as the port's `DynFrame` components reject it.
The apparent asymmetry with `CmpMap` is real but it is C's asymmetry, faithfully reproduced, and there is nothing to decide.

**A CmpMap holding a bare Frame is the one gap**, and it is the same thing as a bare Frame in a FrameSet's `Map<n>` slot.
It exists because in C a bare Frame is a Mapping and here it is not.
The Region content of such a Frame is irrelevant to the failure: a `CmpFrame` in a `Map<n>` slot fails identically whether or not it holds a Region, and a plain `Frame` fails the same way.
So the question for the maintainers is narrow, and it is a question about C's intent rather than about this port: is a bare Frame in a Mapping slot a side effect of C having made Frame a Mapping in order to support Region, or is there a real use case for it?
The corpus supplies no evidence for the second reading — no fixture in it puts a bare Frame in a Mapping slot, and the Region case, which is the one with obvious meaning, already works — which is precisely why the question is worth a broader discussion with the team rather than a decision taken here.

### 4.3 What cluster 4 delivers

A C program can write such a dump today.
An oracle probe produced a FrameSet whose `Map2` slot holds `Begin CmpFrame`: `astAddFrame` accepted it, `astTran2` transformed through it, and the Channel dumped it without error, while the Rust reader rejects the result with `unknown Mapping class: CmpFrame`.

Cluster 4 therefore delivers that dump as a committed failing fixture and nothing else, so the maintainers have a concrete artifact to judge rather than a description.

If they identify a real workflow, the work is `impl Mapping for CmpFrame` — one class, not seven.
`Transform` follows `cmpframe.c:9975-10018`, using each Region component directly and `UnitMap::new(naxes)` for a non-Region component, and `Simplify` follows `cmpframe.c:8963`.
Substituting a `UnitMap` is faithful rather than a shortcut, because it is the same object C constructs at `frame.c:11806`, and the downcast that distinguishes the cases is already in the tree — `regions::any_as_region` (`regions/mod.rs:1183`) operates on a `&dyn Any`, and `regions/data.rs:145` already applies it to a Frame.
One further both-traits class alongside `FrameSet` and the nine Regions sits inside the existing design rather than against it, so even that outcome needs no amendment to the port design spec.

If they identify none, the fixture stays as a documented, named exclusion carrying that answer as its reason, and `PLAN.md` §9.6 closes on that basis.

### 4.4 One fact recorded for fairness

C accommodated Frame-as-Mapping deliberately.
`frame.c:6233` installs a `DoNotSimplify` override returning zero, so that a Frame with `Ident` set remains simplifiable as a Mapping where the base rule at `mapping.c:1150` would have blocked it.
That is a considered accommodation rather than an accident, which is why the question goes to the maintainers rather than being answered here.
The port already mirrors the override, at `mappings/mod.rs:666` with `false` on each Region, so nothing is owed on that point.

## 5. Fixtures and gating

### 5.1 The ledger is the ratchet

`crates/ast-core/tests/test_verification_ledger.rs` already records every relaxation in the crate as `(test file, const name, count, reason)` and asserts two things: each relaxation is exactly the size the table says, and no relaxation exists outside the table.
Because the first assertion tests equality rather than an upper bound, a fix that lands without lowering its number fails the build, so a partial fix cannot pass quietly.
No new instrument is needed; progress is those numbers falling.

| Cluster | Ledger rows it closes |
|---|---|
| 1 | `NO_ISSIMP_CARD_UNGATED` 1 → 0, and `SERIAL_UNGATED`'s two stamping causes |
| 2 | four `skip_string_compare` manifest entries → 0, `ISSIMP_DISAGREEMENTS` 7 → 0, `ISSIMP_PLACEMENT_LICENSED` in `test_simplify_regression.rs` 1 → 0 |
| 3 | the remainder of `SERIAL_UNGATED` → 0 |
| 5, 6 | both `transform_oracle_overrides.txt` entries → 0 |

Two rows survive: `DEFERRED_CLASSES` at 3, and `ISSIMP_PLACEMENT_LICENSED` in `test_wcsconv.rs` at 4.
Both are approved exclusions, and their reason strings in the ledger are where those exclusions are recorded, which makes them executable rather than prose.
`SERIAL_UNGATED` falls in two stages, cluster 1 taking its stamping causes and cluster 3 the rest.
Because the ledger asserts an exact count rather than an upper bound, the intermediate value is written into the table when cluster 1 lands, which is the point at which the remaining causes have to be named.

`PLAN.md` §6 mirrors the table and moves in the same commits.

### 5.2 Fixtures come from the oracle

Every new fixture is generated by a program under `tools/oracle_fixtures/` against the frozen pin.
`.map` and `.simp` references are captured, never hand-edited.
`check_corpus_fresh.sh` keeps the existing distinction: card-only drift means references predate the pin and must be regenerated, while numeric drift means they were captured on another platform and must be left alone.

Three additions:

- Cluster 1 needs a *pair* of Region-through-ZoomMap cases, one stamped and one not, because a rule that always stamps passes a lone positive fixture.
- Cluster 3 needs pole-straddling and non-straddling cases for `Ellipse::RegBaseMesh`, `Polygon::RegBaseBox` and `CmpFrame::norm_box`.
  Without them the closure-predicate ports are unfalsifiable.
- Cluster 4 commits the Frame-in-`Map<n>` dump described in §4.1 as a failing fixture.

### 5.3 Landing a fixture is itself gated

Two tests consume the simplify corpus — the manifest-driven regression test and the auto-discovering byte-exact round-trip test — and both must be green when a fixture lands.
The ledger's second assertion catches any new `const FOO_EXCLUDED` invented to accommodate a fixture.
No new tolerance, no skip mode, and no widened comparator is introduced.
Whatever a comparator normalizes becomes unfalsifiable, so any relaxation that survives is per case, named in source, and justified in a comment.

## 6. Verification and exit criteria

### 6.1 One command decides it

`cargo test -p ast-core` with all nine simplify-relevant binaries green — `test_simplify_regression`, `test_simplify_seams`, `test_regions`, `test_mapmerge_audit`, `test_verification_ledger`, `test_transform_oracle`, `test_serialisation`, `test_wcsconv`, `test_deferred_fixtures` — together with `cargo fmt --all -- --check` and `cargo clippy --workspace -- -D warnings`.
Nothing is reported done on a narrower run.

### 6.2 The ledger's final state

| Row | Now | After |
|---|---:|---:|
| `SERIAL_UNGATED` | 6 | 0 |
| `ISSIMP_DISAGREEMENTS` | 7 | 0 |
| `NO_ISSIMP_CARD_UNGATED` | 1 | 0 |
| `ISSIMP_PLACEMENT_LICENSED` (simplify) | 1 | 0 |
| manifest `skip_string_compare` | 4 | 0 |
| `transform_oracle_overrides.txt` | 2 | 0 |
| `ISSIMP_PLACEMENT_LICENSED` (wcsconv) | 4 | 4 |
| `DEFERRED_CLASSES` | 3 | 3 |
| the five rows already at zero | 0 | 0 |

Twenty-one relaxations go and seven remain, both surviving rows being approved exclusions.
`PLAN.md` sections 9.1, 9.2, 9.5, 9.6 and 10.3 close; 10.2 and 9.4 stay open and are stated as deferred rather than quietly dropped.

### 6.3 Three conditions that are not counts

- Every audit citation resolves to the named `fn` at HEAD, and no row is left at `gap`.
  The permitted verdicts are those of §3.4 less `gap`.
- `C_SIMPLIFY_SEAMS`, re-derived from `reference/starlink-ast/src/*.c` one level deeper than today, still matches, so no per-class C step is missing from a checklist.
- Every fixture the plan adds is oracle-generated, and `check_corpus_fresh.sh` is clean.

### 6.4 Two negative checks

These are what catch a false pass, and neither is optional.

- Where a rule is conditional, the fixture arrives as a pair — cluster 1's stamped and not-stamped, cluster 3's straddling and not.
  A lone positive cannot distinguish "implemented the rule" from "always says yes".
- Each cluster gets a revert-check: after the fix, temporarily back it out and confirm the new fixture fails.
  If it still passes, the fixture was never gating the fix.
  A declared gate that does not hold is worse than no gate, because it stops anyone looking.

### 6.5 Performance is measured, not assumed

Clusters 2 and 3 touch transform paths — the WinMap merge, the MatrixMap inverse, and the Region mesh — and there is no automated performance gate.
`cargo run --release --example bench_per_class -p ast-core` runs against `docs/issues/mapping-perf-vs-c-oracle.md`, and the result is reported whichever way it goes.

### 6.6 What done reports

A closing report under `docs/reports/` carrying the before-and-after ledger, the sections closed, the two surviving exclusions with their reasons, and the deferred items.
It answers directly whether the simplifier has validated parity with C, and states the scope of that claim exactly: byte-equal `.map` and `.simp` output across 468 fixtures and the wcsconv comparisons at pin `3e60abaa`, with the named exceptions of §1.3 and nothing else.

## Revision history

| Rev | Date | Change |
|---|---|---|
| 1 | 2026-09-01 | Initial design. Section 4 revised during review from "implement `Mapping` for seven Frame classes, reversing the sibling-trait decision" to "Region is the only Frame that is a Mapping", after establishing that a Region is already accepted as a component of both a `CmpFrame` and a `CmpMap`, exercised by fifty corpus fixtures, and that the single gap is a bare Frame where a Mapping is expected. `CmpFrame` refusing a general Mapping is not the mirror gap it resembles: C refuses it too, at `cmpframe.c:10634`. The sectionless-fixture cluster dropped as maintainer-owned, leaving seven clusters. Cluster 4 moved last and gated. |
