# Transform-Oracle Parity — Design

Date: 2026-08-02
Branch: tickets/DM-54793
Status: approved for planning

## Goal

Reach full transform parity with the C oracle: every section the
`test_transform_oracle.rs` harness measures must match the C reference,
closing all 80 output-mismatches and 18 load-fails recorded in the current
report (`target/transform_oracle_report.json`: sections=1606, pass=1505,
failed=80, skipped=2, skipped_override=1, load_fail=18, arity=0).

Full parity is the stated merge criterion for this branch: an empty
exclusion list on the oracle harness indicates the core of the Rust port is
validated against C at the transform-output level.

## The harness is the through-line

`crates/starlink-ast/tests/test_transform_oracle.rs` reloads committed
C-generated `.oracle` fixtures through the Rust port, re-transforms the
recorded inputs, and compares outputs to the C reference values (golden
forward, golden inverse, equivalence, round-trip).
It is today an `#[ignore]` investigative harness that measures divergence
and asserts nothing.

This effort converts it, incrementally, into a regression gate.
As each phase greens its sections, we drop `#[ignore]` for that scope and
record any not-yet-fixed sections in a committed exclusion list — each entry
carrying a reason string, mirroring the existing `NATIVE_READ_EXCLUDED`
mechanism.
The list only ever shrinks.
When it is empty, the harness is a hard CI gate and parity is proven.

Every phase ends the same way: re-run the oracle report, green its sections,
drop `#[ignore]` for that scope, and remove those sections from the
exclusion list.

## What exploration found

The class names in the original request (SlaMap / SphMap / WinMap / TimeMap)
do not map one-to-one onto where the divergences actually live.
Two background surveys of the C-vs-Rust source established:

- **SlaMap** — two concrete, C-verified transform-math bugs, plus two minor
  fidelity gaps. Highest confidence; root causes known.
- **SphMap / WinMap** — no class-level bug found; both are faithful ports
  (transform, attributes, all MapMerge branches). Their oracle mismatches
  must be composed inside a chain or fixture-specific, requiring live
  diagnosis.
- **TimeMap** — already correct and fully wired (all 27 CVT types,
  leap-seconds, Fairhead-Bretagnon coefficients). The `cap_timemap_02`
  load-fail is caused by its *outer* class being `StcResourceProfile`, which
  is unported.
- **XphMap** — genuinely not ported (no `xphmap.rs`). Distinct from the
  WcsMap `Xph` *projection* variant, which does exist.
- **FITS-WCS / WcsMap cluster (~24 fixtures)** — splits into (1) FITS-read
  structural divergence ("BEGAST object shape": ait-highlatpole, car4, gls,
  tnx, unknown-cel…), which overlaps the existing `NATIVE_READ_EXCLUDED`
  FITS-read-fidelity workstream; and (2) rotation-pipeline transform
  mismatch (zpn, zpx, hpx, sao-tan) rooted in `wcs_celestial.rs` "Phase 2"
  approximations.

So "full parity" is five distinct workstreams of very different size and
confidence, not four class fixes.

## Phasing — confidence-ordered

Six phases, most-certain fixes first, so early wins de-risk the effort and
the exclusion list only shrinks.
Each phase is independently mergeable-in-principle: after any phase the
branch is strictly closer to parity, with a shrinking exclusion list and no
regressions.

| Phase | Scope | Confidence | Sections |
|---|---|---|---|
| 1 | SlaMap — 2 C-verified math bugs + 2 minor gaps | High | sla (9), slamap (8) |
| 2 | SphMap / WinMap — diagnose, then fix | Medium | sphmap (4), winmap (4) |
| 3 | WcsMap rotation-pipeline | Medium | ~4–8 (zpn, zpx, hpx, sao-tan) |
| 4 | WcsMap FITS-read structural | Lower (large, shared) | ~16 |
| 5 | XphMap — new class port | New port | xph (2), xphmap (6), neg_xph (6) |
| 6 | StcResourceProfile — new class port | New port | timemap (4) |

## Per-phase mechanics

### Phase 1 — SlaMap (High confidence)

Root causes are confirmed against C source. All required `pal` FFI helpers
(`prec`, `dmxm`, `epj`, `ecmat`, `dranrm`) are already exposed in
`crates/starlink-ast/src/pal.rs`; no new FFI is needed.

**Fix A — ECLEQ/EQECL precession** (`slamap.rs:986-994`).
The Rust code applies `ecmat(date)` alone, which is only correct at
J2000.0. C builds a combined matrix (slamap.c:3888-3894):

```
prec    = pal::prec(2000.0, pal::epj(args[0]))   // J2000 → date
rot     = pal::ecmat(args[0])                     // equatorial → ecliptic at date
convert = pal::dmxm(rot, prec)                    // rot · prec
```

Then ECLEQ (ecliptic→equatorial) = `dimxv(convert, v)`, EQECL
(equatorial→ecliptic) = `dmxv(convert, v)`.

**Fix B — AMP argument-order swap** (`slamap.rs:1186`).
The `mappa_cache` closure calls `pal::mappa(args[0], args[1])` for both MAP
and AMP. C orders the arguments differently per case: MAP args are
`(EQ, DATE)` → `mappa(eq=args[0], ep=args[1])` (slamap.c:3851-3854); AMP args
are `(DATE, EQ)` → `mappa(eq=args[1], ep=args[0])` (slamap.c:3815-3818). The
Rust AMP path replicates the pre-fix C bug noted at slamap.c:79. Split the
two cases so AMP passes `mappa(args[1], args[0])`.

**Fix C (minor) — per-step longitude normalization.**
C wraps longitude into `[0,2π]` via `palDranrm` after most steps. Port
faithfully where C does it; do not add it where C does not.

**Fix D (minor) — bad-args guard.**
C zeroes a step's output when any of its arguments is `AST__BAD`
(slamap.c:3672-3686). Add the equivalent guard.

**Regression protection.** The existing passing `test_slamap.rs` uses
date = J2000.0 (MJD 51544.5), where `prec(2000,2000)` is the identity, so
Fix A leaves it green. Add a test at an off-J2000 date to lock the fix in.

### Phase 2 — SphMap / WinMap (diagnose first)

Surveys found no class-level bug, so the first task is **diagnosis, not
fixing**. For each of the 8 sections, use the C-oracle probe recipe
(`docs/issues/c-oracle-probe-recipe.md`) to dump the intermediate PointSets
from both C and Rust and localize where they diverge. Only then write the
fix task. If diagnosis shows the divergence lives inside a *different* class
in the chain, that section moves to the phase owning that class, and the move
is noted.

### Phase 3 — WcsMap rotation-pipeline (zpn, zpx, hpx, sao-tan)

The projection math is present (vendored `proj.c` via FFI). The divergence
is in `wcs_celestial.rs::build_native_to_celestial_rotation_for_wcsmap` —
the "Phase 2" approximations in deriving the read-direction rotation
(LONPOLE/LATPOLE, fiducial handling). Diagnose per-fixture with the probe to
confirm the rotation matrix is the divergence point, then port the C
`WcsNatPole`/`WcsCelestial` logic faithfully. No tolerance heuristics.

### Phase 4 — WcsMap FITS-read structural ("BEGAST object shape")

This overlaps the existing FITS-read-fidelity workstream tracked as
`NATIVE_READ_EXCLUDED` (tail 190/192). **Reconcile, do not duplicate.** The
first task audits which oracle load-fail/mismatch sections correspond to
existing `NATIVE_READ_EXCLUDED` entries, so the two exclusion lists stay
consistent and landed work is not re-fixed. Then port the remaining
read-path structural fixes.

### Phase 5 — XphMap (new class port)

No `xphmap.rs` exists. Follow the CLAUDE.md "Adding a new Mapping" recipe:
struct + builder, `Mapping` impl (transform ported faithfully from
`xphmap.c`, reusing the existing `WcsProjection::Xph` FFI math where C does),
`Serializable` (`from_node`/`to_node`), `AnyObject` variant, `starlink-ast-capi`
C-ABI shim, and a ported test. The `neg_xph` fixtures are negated variants —
same class, exercising the negation path.

### Phase 6 — StcResourceProfile (new class port)

TimeMap is already correct; the `cap_timemap_02` load-fail is the unported
outer `StcResourceProfile`, an STC region-profile class larger than a
Mapping. The first task assesses its full dependency tree — it may pull in
other STC classes. If the port balloons beyond what the fixture needs, that
is surfaced to the human before committing to the full port. Port to the
extent the fixture requires, faithfully.

## Cross-cutting mechanics (all phases)

- **Execution model.** Each phase is executed via
  `superpowers:subagent-driven-development`: the writing-plans output becomes
  per-task briefs; implementer → review-package → task-reviewer → fix-loop →
  ledger, as this branch already ran the oracle-checker tasks.
- **C-fidelity rule.** Every fix mirrors C control flow directly. No
  compute-then-check tolerance heuristics without asking the human first. C
  logic beats existing diverged Rust. Lack of test coverage is never a reason
  to skip a C branch.
- **Diagnosis tooling.** The C-oracle probe recipe with the repo's own
  `libast.so` (reference build present at
  `reference/starlink-ast/build-ast/libast.so`; the conda-RPATH trap is
  documented in the recipe).
- **Exclusion-list discipline.** The list only shrinks; each entry carries a
  reason. Empty list = hard gate = parity proven = merge criterion met.

## Out of scope

- Rewriting `vendor/wcslib/` in Rust (that is delivery step 6a, separate).
- The `gen_transform_oracle` generator remains intentionally not ported; the
  `.oracle` fixtures are vendored.
- Any STC classes beyond what the `cap_timemap_02` fixture requires
  (Phase 6 ports only to the fixture's need).

## Risks

- **Phase 4 scope.** The FITS-read structural cluster is large and shares
  ground with an in-flight workstream; the reconciliation task must run
  before any fixing to avoid double work or conflicting exclusion lists.
- **Phase 6 dependency fan-out.** `StcResourceProfile` may transitively
  require several unported STC classes. The assessment task gates whether the
  full port is in scope; ballooning is surfaced to the human.
- **Phase 2 relocation.** SphMap/WinMap mismatches may prove to originate in
  other classes; sections can migrate between phases after diagnosis, which
  the plan must accommodate rather than assume a fixed section-to-phase map.
