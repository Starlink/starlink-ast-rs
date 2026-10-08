# Fidelity audit: CmpRegion and Prism

Read-only audit of the two compound-Region classes against the C reference.

- C: `reference/starlink-ast/src/cmpregion.c`, `reference/starlink-ast/src/prism.c`
- Rust: `crates/ast-core/src/regions/cmpregion.rs`, `crates/ast-core/src/regions/prism.rs`

Classification key:

- **PORTED** — faithful line-for-line port of the C control flow.
- **HEURISTIC** — deliberate approximation that does not follow the C algorithm.
- **MISSING-BRANCH** — ported but omits one or more C decision branches.
- **STUB** — placeholder that does far less than C.
- **ABSENT** — no override; falls back to a generic/parent path (note whether
  that parent path itself is faithful).

---

## 1. Gap matrix — Prism (`prism.c`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (prism.c:3210) | `Mapping::simplified` (prism.rs:752) | **PORTED** (one substantive divergence) | Covers the parallel-CmpMap collapse (3305/3318/3324), the map_region + re-simplify step (3342), and all three astMapSplit sub-branches: one-component-covers-all (3401 & 3418), both-split (3436), and the neither/clone fallback (3382). The `astRegOverlay` (3512) and uncertainty carry (3494) are present. **Divergence:** the C "final" step 3500–3502 re-invokes the *parent* Region Simplify on the new Region (to simplify the encapsulated FrameSet/uncertainty); the Rust port replaces this with per-branch `map_region`+`simplified` and an explicit comment (prism.rs:932-937) arguing it is subsumed for a single-Frame builder FrameSet. This is a reasoned substitution, not a faithful call — flag if a Prism ever carries a multi-Frame FrameSet. The `equal`-based "did it change?" tests (prism.rs:801, 794) replace C pointer identity; correct only if `Mapping::equal` is exact for every leaf class. |
| `GetRegions` (prism.c:1142) | inline in simplified (prism.rs:762-764) | **PORTED** | C clones region1/region2 and reads Negated. Rust does the same inline (`clone_region` + `self.data.negated`). No standalone helper, but semantically identical (Prism GetRegions does not rewrite operator/negation, unlike CmpRegion's). |
| `MapRegion` (region.c:5379, generic — Prism does *not* override) | `Region::map_region` (prism.rs:624) | **STUB** | **Gap.** C has no Prism override, so `astMapRegion` runs the generic region.c:5379 algorithm: validate fwd/inv, transform defining points, drop degenerate base axes, build a proper remapped Region, and carry uncertainty (region.c ~5700, `astCopy(usethis)` + `astSetUnc`). The Rust `map_region` **discards `_mapping` entirely** — it builds a new Prism reusing the *original* sub-regions with only a fresh FrameSet from `frame`. Any real coordinate remap is silently dropped, and uncertainty is not carried. This is the single largest Prism fidelity gap. |
| `GetRegionBounds`/`RegBaseBox` (prism.c:946/1969) | `reg_base_box` (prism.rs:287) | **PORTED** | Concatenates the two component bounding boxes. Matches C (base box = box1 ⊕ box2). |
| `RegBaseMesh` (prism.c:2052) | `reg_base_mesh` (prism.rs:301) | **PORTED (approx sizing)** | End-caps (grid1×mesh2) + side-walls (mesh1×grid2) cross-product structure matches C. Mesh/grid size budgeting (prism.rs:317-329) is a reasonable transcription; exact point counts may differ from C but structure is faithful. |
| `RegPins` (prism.c:2633) | `reg_pins` (prism.rs:256) | **HEURISTIC / MISSING-BRANCH** | C splits points into the two sub-Frames, temporarily forces both components Closed, tests boundary membership of *each* component and combines ("on boundary of one AND on-or-within the other"), applying uncertainty tolerance. Rust instead tests `point_in_region` (interior membership) of both parts — an *inside* test, not an *on-boundary* test. No Closed toggling, no uncertainty (`unc` unused). Wrong for genuine boundary-pin queries. |
| `Overlap`/`OverlapX` (prism.c:1668/1919) | `overlap` (prism.rs:461 → `compute_overlap`) | **ABSENT (→ HEURISTIC)** | Prism has no bespoke overlap; it delegates to the shared mesh-count heuristic (overlap.rs:40). C's Prism::Overlap is a full geometric port: split extrusion vs base axes via astMapSplit, map picked axes into each sub-Frame, overlap each pair, combine via the 7×7 `rtable` (prism.c:1768), then apply the negation-correction tables (1846-1880). None of this exists in Rust. |
| `Equal` (prism.c:521) | `Mapping::equal` (prism.rs:982) | **PORTED** | Parent check (class/Negated/Closed/current Frame) + both components equal. Matches C. |
| `astConvertToPrism_` (prism.c:237) | `Box::convert_to_prism` (box_region.rs:180) | **PORTED** | Box→Prism helper: scans all axis-subset bitmasks, picks each subset into a Region, simplifies, forms+permutes a Prism, remaps into the original current Frame, re-simplifies. Comment at box_region.rs:167 claims faithful port. (Only Box calls it in C — box.c:4036 — matching Rust placement.) |
| `MapList` (prism.c:1532) | `map_list_decompose` (prism.rs:1000) | **PORTED** | Parallel-only decomposition into the two component Regions, invert=0 each; series request returns None (atomic). Matches C. |
| `Transform` (prism.c:3529) | via `region_transform` (prism.rs:736) + `point_in_region` (prism.rs:411) | **PORTED** | Split-and-AND membership with negation. Faithful. |

### Prism blocking assessment

`Prism::simplified` is essentially fully ported. The two live Prism gaps are
`map_region` (discards the mapping) and the overlap/RegPins heuristics —
`map_region` is *invoked inside* `simplified` (prism.rs:813, 749, 767, 783),
so the STUB partly undermines the otherwise-faithful Simplify whenever the
base→current Mapping is non-trivial. For `cap_interval_01` and
`cap_pointlist_02` the relevant path is the astMapSplit branch where each
component maps to a distinct axis subset and `reg1.map_region(nmap1, …)` is a
leaf Region (Interval / PointList) whose *own* `map_region` is faithful — so
those two are not blocked by the Prism `map_region` stub (the stub only bites
when `map_region` is called *on a Prism*, e.g. the 3342 branch).

---

## 2. Gap matrix — CmpRegion (`cmpregion.c`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (cmpregion.c:3517) | `Mapping::simplified` (cmpregion.rs:732) | **STUB** | See §4 for the full branch-by-branch enumeration. The Rust body only: (a) simplifies the base→current Mapping, (b) short-circuits to a clone if it is a UnitMap, else (c) `map_region`s both components into the current Frame and rebuilds a same-operator CmpRegion. It performs **none** of C's simplification logic: no `GetRegions` operator/negation rewrite, no per-component simplify, no NullRegion-exclusion table (3644), no overlap-reduction decision table (3695: no-overlap+AND→Null, negation+OR→infinite, identical→one, containment→container/contained), no `astRegOverlay`. It can never collapse a CmpRegion to a NullRegion or to a single component. |
| `GetRegions` (cmpregion.c:981) | **ABSENT** | **MISSING** | The crucial operator/negation rewrite for a negated CmpRegion (AND↔OR with both components negated, 3xDe Morgan, 1063-1087) has no Rust equivalent. Rust `simplified` reads `self.operator`/`self.data.negated` verbatim without De Morgan folding. Needed before any decision-table logic is correct for negated CmpRegions. |
| `GetBounded` (cmpregion.c:703) | `is_bounded_inner` (cmpregion.rs:384) | **MISSING-BRANCH** | Rust: AND→(reg1‖reg2 bounded), OR→(reg1&reg2 bounded). C additionally (a) applies the GetRegions negation rewrite first, and (b) for AND with *neither* component bounded, calls `astOverlap` and returns bounded unless overlap∈{none,partial,negation} (807-811). Rust omits both — wrong for negated components and for the unbounded-AND overlap subcase. |
| `Overlap`/`OverlapX` (region.c generic — CmpRegion does not override) | `overlap` (cmpregion.rs:454 → `compute_overlap`) | **ABSENT (→ HEURISTIC)** | CmpRegion has no C override (uses region.c OverlapX). Rust delegates to the mesh-count heuristic (overlap.rs). This heuristic is what `is_bounded` and any future Simplify decision table would rely on — see §3/§4. |
| `RegPins` (cmpregion.c:2185) | `reg_pins` (cmpregion.rs:177) | **HEURISTIC** | C tests true boundary membership of each component with Closed-forcing and uncertainty tolerance, combining per operator. Rust tests interior `point_in_region` of both components — an inside test, no boundary/uncertainty semantics. |
| `MapRegion` (region.c:5379 generic — no CmpRegion override) | `Region::map_region` (cmpregion.rs:606) | **MISSING-BRANCH** | Rust maps both components then rebuilds. Reasonable, but it **does not carry uncertainty** (no `set_uncertainty` on the result; the generic C path preserves it via `astCopy(usethis)`+`astSetUnc`). Consistent with the project-wide "map_region drops uncertainty" gap noted in memory. |
| `RegBaseBox` (cmpregion.c:1508) | `reg_base_box` (cmpregion.rs:226) | **MISSING-BRANCH** | Rust does AND→intersect boxes, OR→union boxes with simple BAD handling. C's RegBaseBox/RegBaseBox2 (1508/1719) is considerably more involved (uses component meshes and the negation rewrite to bound intersections tightly, esp. for negated/unbounded components). Adequate for simple bounded cases; diverges for negated or partially-unbounded components. |
| `RegBaseMesh` (cmpregion.c:1824) | `reg_base_mesh` (cmpregion.rs:293) | **PORTED (approx)** | AND: keep mesh1∩reg2 + mesh2∩reg1; OR: keep mesh1∖reg2 + mesh2∖reg1 via negated transforms. Structurally matches C's approach (filter each component mesh by the other, using the region-as-Mapping transform). Faithful in spirit. |
| `Equal` (cmpregion.c:502) | `Mapping::equal` (cmpregion.rs:771) | **PORTED** | Parent check + equal operator + equal components. Matches C. Note: does not fold De Morgan, so an AND-of-negated and its OR-of-negated equivalent compare unequal (C `Equal` also does not fold, so faithful). |
| `GetRegions` negation table also feeds `Transform` (cmpregion.c:3882) | `region_transform`/`point_in_region` (cmpregion.rs:406) | **MISSING-BRANCH** | Rust `point_in_region` applies operator then a single outer negation. C applies GetRegions (which pushes negation into the components and swaps operator) before combining. For a *negated* CmpRegion the two are equivalent by De Morgan **only if** each component's own negation is honoured — Rust relies on `region1.point_in_region` already reflecting each component's Negated flag, which it does, and applies the outer NOT, so the result is correct; but it never exercises the operator swap, so it cannot share code with a future Simplify decision table. |
| `MergeCmpRegion` | — | **N/A** | No function named `MergeCmpRegion` exists in `cmpregion.c` (the checklist name is a misnomer). The nearest real machinery is `CmpRegionList` (228) + `RegSplit` (2472) + `MatchRegion` (1406), used by the parent Region simplify to flatten nested CmpRegions and match against uncertainty. **None of these are ported** — Rust has no CmpRegion flattening/list machinery at all. |

---

## 3. Blocking dependencies

**`cap_slamap_07` — CmpRegion AND/OR-of-disjoint → negated NullRegion.**
Fixture: input `Begin CmpRegion` (nested OR of AND-combined Box/Circle with
SlaMap), expected output `Begin NullRegion` with `Negate = 1` (a *boundless*
i.e. infinite region). Producing this requires the CmpRegion Simplify
decision table (cmpregion.c:3695): specifically the "negation + OR → infinite
(negated NullRegion)" branch (3708) and/or the "no-overlap + AND → NullRegion"
branch (3700). **Blocked** by:
1. `CmpRegion::simplified` STUB (cmpregion.rs:732) — none of the table exists.
2. `GetRegions` operator/negation rewrite — ABSENT.
3. `overlap` between components — only a HEURISTIC (overlap.rs), and the
   decision table keys on exact overlap codes 1/5/6 (disjoint / identical /
   negation). The mesh heuristic cannot reliably return code 6 (negation),
   which is exactly what the OR→infinite branch needs.
4. `NullRegionBuilder` — PORTED (nullregion.rs:28), so the *sink* exists; the
   logic that decides to emit it does not.

This is the deepest blocker: it needs the full §4 port **plus** an
overlap that returns exact geometric codes (currently a documented heuristic —
see memory "Overlap is heuristic not port"). Sub-project B (overlap) gates it.

**`cap_interval_01` — Prism → Interval.** Input `Begin Prism`, expected
`Begin Interval`. Path: `Prism::simplified` (PORTED) → parallel CmpMap
simplify collapses the two intervals into one Region, OR the astMapSplit
branch maps each component into a distinct current-Frame axis subset and the
combined Prism simplifies to a single Interval. Dependencies: `CmpMap`
parallel simplify + `Interval` MergeInterval/`map_region` (faithful) +
`map_split` on the base→current Mapping. **Not blocked by the CmpRegion stub.**
It is only blocked if the parallel-CmpMap Region-merge (`Interval::map_merge`
/ MergeInterval) or CmpMap `map_split` is incomplete — verify those separately;
the Prism driver itself is in place.

**`cap_pointlist_02` — Prism → PointList.** Same Prism path as
`cap_interval_01`, terminating in a `PointList`. Depends on `PointList`
participating in parallel merge / `map_region` and on the astMapSplit branch.
**Not blocked by the CmpRegion stub**; blocked only if PointList's merge or
map_region path is incomplete. The Prism `map_region` STUB does *not* block
this because the split branch calls `map_region` on the *leaf* components
(PointList/Interval), whose overrides are faithful — not on a Prism.

---

## 4. What `CmpRegion::simplified` full port needs

Enumerating each branch of `cmpregion.c::Simplify` (3517) and its Rust
dependency status. Rust stub today: cmpregion.rs:732-765.

1. **Parent simplify → `newc`** (3595). Invoke the inherited Region simplify to
   simplify the encapsulated FrameSet + uncertainty; result is a CmpRegion in
   the *current* Frame. `simpler = (newc != this)`.
   *Dependency:* parent Region `Simplify` FrameSet handling. Rust has the
   base→current Mapping simplify (cmpregion.rs:735) but not the full parent
   FrameSet-collapse; **partial gap**.

2. **`GetRegions(newc, &reg1,&reg2,&oper,&neg1,&neg2)`** (3615) — returns
   components in the *base* Frame plus the De Morgan-rewritten operator and
   per-component negation flags.
   *Dependency:* `GetRegions` — **ABSENT** (must be written; see §2).

3. **Apply required negation to each component** (3620/3629): if `neg1 !=
   reg1.Negated` use `astGetNegation(reg1)`.
   *Dependency:* `negated_copy` — **PORTED** (cmpregion.rs:458 etc.).

4. **Simplify each component** `sreg1 = astSimplify(reg1)` (3636).
   *Dependency:* per-leaf `Mapping::simplified` — **PORTED** for leaf Regions.

5. **NullRegion-exclusion table** (3644-3690): if either simplified component
   `astIsANullRegion`:
   - null (non-negated) + AND → `NullRegion(other)` ;
   - null + OR → `astCopy(other)` ;
   - infinite (negated null) + AND → `astCopy(other)` ;
   - infinite + OR → negated `NullRegion(other)`.
   *Dependencies:* detect NullRegion (`is::<NullRegion>()` / the overlap.rs
   `is_null_empty`/`is_null_universal` helpers exist), `NullRegionBuilder`
   (**PORTED**), `astGetUnc` carry. Logic itself **ABSENT** in Rust.

6. **Overlap-reduction decision table** (3695-3752), when neither is null —
   `overlap = astOverlap(sreg1, sreg2)`:
   - `(overlap∈{1,6}) && AND` → NullRegion (disjoint or negation-pair, AND is empty);
   - `overlap==6 && OR` → negated NullRegion (infinite);
   - `overlap==5` (identical) → `astCopy(sreg1)`;
   - `overlap==2` (first⊂second) → AND→sreg1, OR→sreg2;
   - `overlap==3` (second⊂first) → AND→sreg2, OR→sreg1;
   - else if `simpler` → rebuild `astCmpRegion(sreg1,sreg2,oper)`.
   *Dependency:* **`overlap` returning exact geometric codes** — currently a
   **HEURISTIC** (overlap.rs). Codes 5 (identical) and 6 (negation) are the
   hardest for a mesh heuristic to produce reliably; **this is the gating
   dependency** and is itself an open sub-project (memory: "Overlap is
   heuristic not port"). `NullRegionBuilder` PORTED; `Clone` PORTED.

7. **Map `newb` (base Frame) → current Frame** (3760-3767): if a base-Frame
   result was produced, `astMapRegion(newb, base→current map, current frm)`;
   else `astClone(newc)`.
   *Dependency:* `map_region` on the produced Region — **PORTED for leaf
   Regions**, STUB for Prism (not reachable here); uncertainty-carry gap noted.

8. **`astRegOverlay`** of original attributes onto the result — C does this via
   the parent Region simplify path.
   *Dependency:* `reg_overlay` — **PORTED** (mod.rs:311).

**Summary of blocking dependencies for the full port:** the only *ABSENT*
prerequisite that must be newly written is `GetRegions` (the De Morgan
rewrite). The only *inadequate* prerequisite is `overlap` (heuristic, needs
exact codes 1/2/3/5/6). Everything else — NullRegionBuilder, negated_copy,
leaf map_region, reg_overlay, is-NullRegion detection — is already PORTED.
Thus a faithful `CmpRegion::simplified` is gated primarily on **(a) writing
GetRegions and (b) a geometric overlap that returns exact overlap codes**, and
secondarily on wiring the two decision tables (§5/§6 above).
