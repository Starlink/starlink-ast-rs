# Region-handling audit — consolidated gap matrix & dependency-ordered plan

Synthesises the four per-area audits in this directory:

- `region-base.md` — base Region + shared infra (data/overlap/merge_util/masking)
- `region-box-interval-pointlist.md` — sliceable-geometry classes
- `region-circle-ellipse-polygon-null.md` — closed-curve + null classes
- `region-cmpregion-prism.md` — compound classes

Goal: close the 4 remaining `equal()` failures on the simplify corpus
(`cap_slamap_02`, `cap_interval_01`, `cap_pointlist_02`, `cap_slamap_07`)
by porting C control flow faithfully — no heuristics substituted for C
branches (per project CLAUDE.md and recorded feedback memories).

## 1. The two shared root-cause primitives

The four failures are not four independent gaps. They rest on two shared
primitives that are currently approximations rather than ports.

### R-PINS — `reg_pins` is an interior test, not C's on-boundary test

C `astRegPins(this, pset, unc, &mask)` answers: "does each supplied point
lie *on the boundary* of `this`, to within the joint uncertainty of `this`
and `unc`?" Each class ports a genuine geometric on-boundary + uncertainty
-width test (band = `0.5*(l1+l2)` of two uncertainty-box diagonals).

Rust `reg_pins` (data.rs:191 and every per-class override) is instead an
*interior* predicate (`lb<=p<=ub`, `dist<=radius`, crossing-number inside),
takes no `unc` argument, and never tests boundary proximity. Confirmed in
all classes: box_region.rs:310, interval.rs:406, pointlist.rs:161 (fabricates
a `1e-10 × bbox-diagonal` tolerance), circle.rs:182, ellipse.rs:194,
polygon.rs:278, prism.rs:156, cmpregion.rs:177.

Consumers that are consequently unfaithful:
- `verify_remap_exact` (data.rs:313) — the Box-simplify exactness check.
- `compute_overlap` (overlap.rs:40) — see R-OVL.

### R-OVL — `compute_overlap` is a 90%-threshold heuristic, not `OverlapX`

C `Overlap`→`OverlapX` (region.c:6912/6996): tries `astEqual`(→5) and
negated-equal(→6), does `astConvert` frame alignment, meshes, transforms
into the first region's base frame, calls `astRegPins` with the mapped
uncertainty to detect equivalent boundaries, then a submesh + edge
test-point logic with a touch/Closed rule.

Rust `compute_overlap` (overlap.rs:40): NullRegion special-cases,
bounding-box quick reject, then counts mesh points inside with a hard-coded
90% `THRESHOLD` (overlap.rs:24). It never returns value-6 (negation),
does no `astConvert` alignment, drops the touch/Closed rule. It depends on
R-PINS being fixed to become faithful.

These two are the deepest dependencies. cap_slamap_07 needs R-OVL to return
exact codes 1/5/6; cap_slamap_02 needs R-PINS for the Polygon fit validation.

## 2. Consolidated gap matrix

Each row is a distinct gap. "Feeds" lists which of the 4 fixtures the gap
blocks (— = correctness/parity gap not on any of the 4 critical paths).

| ID | Gap | Status | Location | Feeds |
|---|---|---|---|---|
| R-PINS | `reg_pins` interior not on-boundary+uncertainty | HEURISTIC | data.rs:191 + all per-class | 02, 07 |
| R-OVL | `compute_overlap` 90% threshold not `OverlapX` | HEURISTIC | overlap.rs:40 | 07 |
| R-UNC | `map_region` drops uncertainty (all 9 classes) | MISSING-BRANCH | box_region.rs:674 etc. | parity |
| B-POLY | `Box::simplified` missing 2-D→Polygon branch | MISSING-BRANCH | box_region.rs:941 (C box.c:3917) | 02 |
| P-ENSURE | Polygon `EnsureInside` (vertex order + Negated) absent | ABSENT | polygon.rs ctor:162 (C polygon.c:1768) | 02 |
| P-ACW | Polygon `acw`/`Cache` interior-point absent | ABSENT | polygon.rs (C polygon.c:575) | 02 |
| P-BOUND | Polygon `GetBounded` hardcoded true | MISSING-BRANCH | polygon.rs:482 (C polygon.c:2749) | 02 |
| P-PINS | Polygon on-boundary `RegPins` (resolve per edge) | HEURISTIC | polygon.rs:278 (C polygon.c:4562) | 02 |
| P-SIMP | Polygon `Simplify` nout==2 guard + bend validation | MISSING-BRANCH | polygon.rs:795 (C polygon.c:5291) | 02 |
| I-CACHE | Interval Box-reclass guard `!neg && closed` too strict | MISSING-BRANCH | interval.rs:1043 (C interval.c:489) | 01, 02b |
| I-PLIST | Interval+PointList merge branch not ported | ABSENT | interval.rs:876 (C interval.c:1458) | 02b |
| C-SIMP | `CmpRegion::simplified` is a stub (no decision tables) | STUB | cmpregion.rs:736 (C cmpregion.c:3517) | 07 |
| C-GETREG | `GetRegions` De Morgan rewrite absent | ABSENT | — (C cmpregion.c:981) | 07 |
| C-BOUND | CmpRegion `GetBounded` omits negation + unbounded-AND | MISSING-BRANCH | cmpregion.rs:384 (C cmpregion.c:703) | 07 |
| PR-MAP | `Prism::map_region` discards the mapping | STUB | prism.rs:514 | parity |
| RB-PICK | base `reg_base_pick_axes` default always None | MISSING-BRANCH | mod.rs:255 (C region.c:8345) | parity |
| GBND-SKY | `GetBounded` lacks SkyFrame-always-bounded rule | MISSING-BRANCH | box_region.rs:579 (C region.c:3406) | 07-adj |
| DEFUNC | No default-uncertainty Region (`GetDefUnc`) | ABSENT | (C region.c:3488) | parity |
| SETUNC | `set_uncertainty` bare field store | STUB | box_region.rs:656 (C region.c:10754) | parity |
| RCENT | `reg_centre` get-only (no re-centre) | MISSING-BRANCH | data.rs:215 (C region.c:8397) | R-PINS dep |
| SIMP-STAB | parent-Simplify uncertainty 0.1% stability check | ABSENT | (C region.c:11196) | parity |
| MERGE-FF | merge helpers omit FillFactor/MeshSize scaling | MISSING-BRANCH | merge_util.rs:43 (C box.c:2049) | parity |
| CE-FIT | Circle/Ellipse Simplify no re-fit/validate/degrade | STUB | circle.rs:607, ellipse.rs:565 | parity |

Note "02b" = the residual Interval reclassification path some analyses attribute
to cap_interval_01/cap_pointlist_02.

## 3. Per-fixture blocking chains

### cap_slamap_02 (Box → Prism-with-Polygon-cap)
The reference `.simp` is a Prism whose `RegionA` is a **Polygon** (the sky Box
rotated into 4 vertices) and `RegionB` a Box. `convert_to_prism` (A2, already
written) splits {sky}/{time+spec} correctly but neither half simplifies, so no
Prism forms — because the sky half is a rotated 2-D Box that C turns into a
Polygon via B-POLY, a branch Rust lacks. Chain:
**R-PINS → P-PINS → P-ENSURE + P-ACW + P-BOUND → P-SIMP → B-POLY → (A2 already done)**.

### cap_slamap_07 (CmpRegion → negated NullRegion)
Needs the full C decision table. Chain:
**R-PINS → R-OVL (exact codes 1/5/6) + C-GETREG + C-BOUND → C-SIMP**.
GBND-SKY feeds the NullRegion detection in overlap.

### cap_interval_01 (Prism → Interval) and cap_pointlist_02 (Prism → PointList)
`Prism::simplified` (A1) is ported and NOT blocked by C-SIMP or PR-MAP (the
split branch calls `map_region` on leaf Interval/PointList, whose overrides are
faithful). Two audits disagree on the residual blocker: one says these are
already unblocked by A1; the other flags I-CACHE (Interval reclassification too
strict) and I-PLIST. **These two fixtures need re-verification against current
HEAD before any new code** — they may already pass once the harness is re-run,
or need only the small I-CACHE guard fix. This is the cheapest thing to check
first and must gate whether we invest in I-CACHE/I-PLIST at all.

## 4. Dependency-ordered implementation plan

Ordered so no task is blocked by a later one. Each is a separate commit with
the full regression gate (simplify + serialisation + lib + wcsconv + frameset +
regions + clippy + fmt) run before landing.

### Phase 0 — Re-verify the "maybe already fixed" fixtures (no code) — DONE
Ran the simplify regression at current HEAD (with A2 in the working tree):
`407 passed, 2 equal() failures: cap_slamap_02, cap_slamap_07`.
**cap_interval_01 and cap_pointlist_02 already pass** (A1 fixed them). Phase 4
(I-CACHE / I-PLIST) is therefore DROPPED from scope. Remaining work: Phase 1
(R-PINS foundation) → Phase 2 (cap_slamap_02 Polygon) + Phase 3 (cap_slamap_07
CmpRegion).

### Phase 1 — R-PINS: port on-boundary `astRegPins` (shared foundation)
The keystone. Introduce a real on-boundary predicate with the `unc` argument
and the `0.5*(l1+l2)` uncertainty band. Requires RCENT (re-centre) as a
sub-dependency for uncertainty positioning. Port per class following C
box.c:3115 / interval.c:2640 / pointlist.c:1970 / circle.c:1450 /
ellipse.c:1538 / polygon.c:4737. Do NOT delete the existing interior
`point_in_region` (Transform still needs it) — add `reg_pins` as the distinct
boundary test C uses in Simplify/Overlap.

### Phase 2 — cap_slamap_02 (Polygon path), depends on Phase 1
1. P-ENSURE + P-ACW: port `EnsureInside` + `Cache` (vertex-order normalisation,
   `acw` flag, safe interior point).
2. P-BOUND: real `GetBounded` (`!negated` for ACW, `negated` for CW, SkyFrame
   always bounded).
3. P-SIMP: `nout==2` guard + bend validation via the Phase-1 `reg_pins`.
4. B-POLY: add the 2-D-Box→Polygon branch to `Box::simplified` (box.c:3917).
   The existing A2 `convert_to_prism` then composes: the sky half simplifies to
   a Polygon, so the Prism forms.

### Phase 3 — cap_slamap_07 (CmpRegion path), depends on Phase 1
1. R-OVL: port `OverlapX` (astConvert alignment, astRegPins-based equivalence,
   value-6 negation). Depends on Phase 1.
2. GBND-SKY + C-BOUND: SkyFrame-bounded rule + CmpRegion negation/unbounded-AND.
3. C-GETREG: port `GetRegions` De Morgan rewrite.
4. C-SIMP: port `CmpRegion::simplified` decision tables (NullRegion-exclusion
   3644 + overlap-reduction 3695).

### Phase 4 — cap_interval_01 / cap_pointlist_02 IF Phase 0 shows still-failing
1. I-CACHE: relax the Box-reclassification guard to C's `isBox && !neg`,
   handling Closed/Negated separately (interval.c:489-512).
2. I-PLIST: port the Interval+PointList merge branch (interval.c:1458) only if
   still needed after I-CACHE.

### Phase 5 — Cross-cutting parity (not on the 4 critical paths; land last)
R-UNC (map_region uncertainty carry — see memory, needs axis-slice care),
PR-MAP (Prism::map_region), RB-PICK, DEFUNC, SETUNC, SIMP-STAB, MERGE-FF,
CE-FIT. These are correctness gaps that do not block the 4 failures but should
be tracked; each is independently landable and re-runs the byte-exact corpus.

## 5. Risks

- **R-PINS is load-bearing and wide.** Changing the boundary predicate can shift
  the passing region fixtures. Re-run the FULL simplify corpus (not just the 4)
  after Phase 1, per `project_m1_winmap_narrow_blocker`.
- **A2 over-collapse guard** (`sp1 != pickfrm1 || sp2 != pickfrm2`) must stay
  exact so cap_pointlist_01 keeps its outer Prism.
- **R-UNC axis-slicing**: a verbatim uncertainty copy is wrong in axis-reducing
  contexts; must slice/remap like C astMapRegion+astGetUncFrm (see memory
  `project_map_region_drops_uncertainty`). Deferred to Phase 5 deliberately.
