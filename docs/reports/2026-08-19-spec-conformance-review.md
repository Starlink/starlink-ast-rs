# Design-spec conformance review

**Reviewed:** 2026-08-19 · **Tree:** `tickets/DM-54793` @ `cc981c8`
**Spec:** `docs/superpower/specs/2026-04-20-rust-port-design.md` (rev 5, 2026-04-27)

Assesses the *implemented* parts of the port against the design spec that
`CLAUDE.md` names as authoritative.
Unimplemented steps are covered only in §5, and only to separate "behind plan"
from "diverged from plan".

Every claim was checked against the source, not against the implementation plans.

---

## 1. Verdict

**The spec's load-bearing decisions all held.**
Every one of the architectural choices the spec argued hardest for — the ones that
went through five revisions of review — is present in the code exactly as
specified.
Nothing was quietly reverted under implementation pressure, which is the failure
mode a five-revision spec most invites.

**The trait surfaces grew by roughly 5×, and that growth is the port's central
lesson made concrete.**
The spec was explicitly "patterns-only" and deferred per-class method design, so
expansion was expected.
What was not anticipated is *where* it went: almost all of it is C's per-class
extension points, which the spec's trait sketches did not mention at all.
This is the same finding as H1 in `docs/porting-hazards.md`, seen from the spec
side rather than the commit side.

**Seven commitments narrowed or lapsed without a recorded decision.**
None is architecturally serious, but they are the difference between a spec that
is authoritative and one that is aspirational, and two of them are now
documented in `CLAUDE.md` as things a developer can do — which they cannot.

---

## 2. What held exactly

These survived four months of implementation pressure unchanged.
Given that most were contested during review, this is the strongest evidence that
the review was worth its cost.

| Spec commitment | Status in code |
|---|---|
| Sibling traits, no hierarchy: `Mapping` and `Frame` independent, `Region: Frame + Mapping` | Exact. `regions/mod.rs:93` is literally `pub trait Region: Frame + Mapping` |
| Owned values, `&mut self` mutation, `Clone` is the deep copy | Exact throughout |
| `Send` but not `Sync`; no locks in hot paths | Exact — `Mapping: AsAny + Send`, `Frame: AsAny + Send`, no `Mutex`/`RwLock` in `ast-core` |
| `BAD_VALUE = -f64::MAX`, NaN accepted on input only | Exact (`constants.rs:8`) |
| 1-based axis indices in the public API | Exact, and enforced — `map_split` even carries a `map_split_zero_based` internal twin so the public seam stays 1-based |
| `PointSet` row-major `(npoint, ncoord)`, `ndarray`-backed, `PointList` frame-aware | Exact, including the constructor and accessor list — only `axis()` was renamed `axis_iter()` |
| `then` / `alongside` verbs, no operator sugar | Exact — 38 call sites, `Composition` enum as specified, no `Mul`/`BitOr` impls |
| `snapshot` / `map_from` as the integrity primitives on `Frame` | Exact, and implemented per class across every Frame *and* every Region |
| `frame_mut()` as the documented integrity escape hatch | Present on both `FrameSet` and `Region` |
| `ErrorCode` generated from `ast_err.msg` by `build.rs` | Generated as specified, ~179 variants — but **without** the spec's `Unknown(i32)` variant; see §4.8 |
| `Serializable` as a *private* `to_node` / `from_node` trait | Exact |
| No public `Object` trait | Held in the literal sense; see §3.2 for the qualification |

The **"What was removed"** section deserves separate mention, because it is the
part of a spec most likely to erode.
It did not erode at all:

```
fn get_attr      0 occurrences
fn set_attr      0
fn test_attr     0
fn clear_attr    0
fn set_fmt       0
fn set_many      0
fn edit          0
```

The seventeen `with_*` functions in the tree are `with_integrity`,
`with_region_integrity`, `with_context` and `with_size_guess` — none is the
banned "return a modified copy" pattern.
Stringly-keyed attribute access never reappeared, and there is no `ast-capi` yet
for it to hide in.

---

## 3. What evolved, and why

### 3.1 The trait surfaces grew ~5×, almost entirely into C's extension points

| Trait | Methods sketched in spec | Methods now | What the growth is |
|---|---:|---:|---|
| `Mapping` | 13 | 28 | see below |
| `Frame` | ~10 named + "resolve, intersect, axnorm, matchaxes, …" | ~90 | attribute quartets, plus `match_template`, `sub_frame`, `overlay_template`, `cast`, `find_frame_template`, `primary_frame` |
| `Region` | 3 named + "mask, mesh, bounds" | ~50 | `reg_pins`, `reg_base_box2`, `reg_base_pick_axes`, `get_def_unc`, `reg_overlay`, `region_merge`, `map_region`, `reg_centre_*` |

The spec deferred per-class method design, so raw growth is not a divergence.
What matters is its composition.
Sorting the fifteen methods `Mapping` gained by cause:

- **C extension points the spec's sketch omitted** — `map_merge`, `equal`,
  `map_split_zero_based`.
  `map_merge` is the single most consequential addition in the port: it is the
  per-class simplification seam whose absence caused the 47-commit simplifier
  rebuild (H1).
  The spec's `Mapping` sketch listed `simplified()` and nothing else about
  simplification, which is exactly the reading that produced a central
  pattern-matcher.
- **Flags whose C semantics turned out to be two things** —
  `has_been_simplified` / `set_has_been_simplified` / `dump_is_simple` /
  `allow_simplify` / `set_allow_simplify`.
  Five methods where the spec implied none, because the serialized `IsSimp`
  token and the internal merge barrier are distinct (H3), and because C's
  `RESTRICTED_SIMPLIFY` / `ALLOW_SIMPLIFY` is real control state the spec
  dismissed in a parenthesis.
- **Definedness, which C derives and the spec assumed** — `has_forward`,
  `has_inverse`, `dump_tran_flags`.
- **An invention with no spec analogue** — `transform_into_nan`, the
  branchless NaN-as-bad bulk protocol.
  This is a genuine post-spec design addition, documented in `CLAUDE.md`'s
  bulk-transform section.
- **Rust mechanics the spec did not foresee** — `clone_box`, because `Clone`
  cannot appear on a trait object, and the spec's "`Clone` is the deep copy"
  had no story for `Box<dyn Mapping>`.
- **Renames** — `decompose` → `map_list_decompose`, closer to C's `astMapList`.

**Assessment.** This is the port evolving *toward* the spec's own stated
principle (faithfulness to C) and away from its trait sketches.
The sketches were the least reviewed part of the spec and the most misleading
part in practice: they read as a complete surface and were used as one.

### 3.2 A public downcasting trait had to be reintroduced

The spec removed the public `Object` trait on the argument that its bare methods
cannot express real cross-class operations.
That argument holds, but it left no way to ask "is this Mapping a `UnitMap`?" —
which the simplifier, `MapMerge` and `Region::overlap` all need constantly.

`pub trait AsAny: Any` was added on 2026-04-29, two days after the first
commit, as a supertrait of both `Mapping` and `Frame`
(commit `2529692`, *"Add as_any().is facility so is-a-unitmap can work without
guessing"*).

So the spec's claim is true as written — there is no public `Object` — but the
substance is qualified: there *is* a public supertrait every Mapping and Frame
must implement, and its purpose is runtime type interrogation.
Worth recording in the spec, both because it contradicts the surrounding prose
and because the prototype's failure metric was *"51 runtime type interrogations
where the C file has 10"*.
The facility that metric counts is now part of the public trait surface, so it
needs the density discipline of H14 rather than silence.

### 3.3 `Frame::convert` became `convert_to`, and `domains` became a slice

`fn convert(&self, to: &dyn Frame, domains: &str)` is now
`fn convert_to(&self, to: &dyn Frame, domains: &[&str])`.

Both changes are improvements, and the second is the more interesting: the spec
inherited C's comma-separated domain-list-in-a-string, and the port refused it.
This is the one place the port declined to mirror a C interface shape on
idiomatic grounds and was right to — the string is a C-API artifact, not
algorithm structure, so H2 does not apply.

### 3.4 `FrameRef` lost `Name` and gained `AllFrames`

The spec has a whole subsection deciding, "captured as a commitment", that
`FrameRef::Name` looks up by `Ident` rather than `Domain`, with the reasoning and
an appeal to astshim's precedent.

`FrameRef` today is `Base | Current | Index(usize) | AllFrames`.
`Name` was never implemented; the lifetime parameter the spec's
`Name(&'a str)` required is therefore absent, which is why the enum is
`'static` and simpler to pass around than specified.
`AllFrames` was added to mirror C's `AST__ALLFRAMES` axis-append mode
(`8893ff2`).

Neither change is wrong.
Both are unrecorded, and the `Ident`-vs-`Domain` reasoning — the expensive part —
is now stranded in a spec section describing an API that does not exist.

---

## 4. Commitments that narrowed or lapsed

Ordered by how much they matter.

### 4.1 The `tracing` feature does not exist, and `CLAUDE.md` says it does

Spec: *"Optional `tracing` feature emits every constructed `AstError` as a
`tracing::error!` event; off by default."*

`CLAUDE.md:69` documents it as a usable command:

```shell
cargo test -p ast-core --features tracing    # emit AstError as tracing events
```

Actual:

```
$ cargo check -p ast-core --features tracing
error: the package 'ast-core' does not contain this feature: tracing
```

`ast-core`'s only feature is `simd-eval`.
This is hazard H12 — a documented capability that does not hold — in the file
that documents H12.
It needs fixing in one place or the other; the debugging affordance is cheap
enough that implementing it is probably the better answer.

### 4.2 Builders exist on 11 of ~45 concrete classes, and not on the spec's own example

Spec: *"Each concrete class provides `builder()`."*
The worked example is `SkyFrame::builder().system(FK5).epoch(2000.0)…`.

Actual: eleven builders, all Regions plus `BasicFrame` and `SelectorMap`.
`SkyFrame::new(system)`, `SpecFrame::new(system)`, `ZoomMap::new(ncoord, factor)`
— plain constructors plus `&mut self` setters.

The de facto rule is sensible and consistent: builders where construction takes
many interdependent optional parameters (Regions need a frame, defining points,
an uncertainty and flags), plain `new` where it takes one or two.
It just isn't the rule the spec states, and the spec's illustration is now
misleading for the exact class a newcomer is most likely to reach for.

### 4.3 `Channel::warnings` returns `&[String]`, not `Option<&KeyMap>`

Spec: `fn warnings(&self) -> Option<&KeyMap>`.

When `Channel` was designed in May, `KeyMap` did not exist, so `&[String]` was
the only option.
`KeyMap` landed on 2026-08-08.
The blocker is gone and the divergence remains — this is now a small,
well-scoped piece of follow-up rather than a design question.

`Channel` diverges in three smaller ways too: `write` returns `AstResult<()>`
rather than the C-like `AstResult<usize>`; `encoding()` was generalised into
`config()` / `config_mut()` returning a `ChannelConfig`; and `register_intra`
is absent because `IntraMap` is unported.

### 4.4 `WritableToChannel` was never needed

Spec: `Channel::write(obj: &dyn WritableToChannel)`, with a private trait
implemented per concrete class, described as *"the 'enumerate the concrete types'
pattern @TallJimbo identified as unavoidable"*.

Actual: `Channel::write(obj: &AnyObject)`.
The closed `AnyObject` enum — which the spec introduced separately, for
`Channel::read` and for the CFFI handle registry — does the dispatch on its own,
so the parallel trait was redundant.

This is a spec simplification discovered by implementation, and the better
outcome: one enumeration of the concrete classes instead of two.
`DynAnyObject` is also just `AnyObject` in the code.

### 4.5 `tran1` / `tran2` were dropped

Spec: *"Single-axis / two-axis convenience `Mapping::tran1` / `tran2` **stay**
(avoid forcing users to allocate a `PointSet` for 1-D data)."*

Neither exists.
Worth a moment's thought rather than a shrug: the prototype's largest measured
performance win was single-point transforms at 3–8.5× C, and the spec's
convenience API for exactly that call pattern is the one that went missing.
The per-class benchmark still shows the port ahead of C at N=16 across all 24
variants, so the capability is there — it just has no ergonomic entry point,
and users will reach for a one-row `PointSet`.

### 4.6 `thiserror` is not used

Spec: *"`thiserror` provides boilerplate."*
`ast-core`'s dependencies are `ndarray`, `nalgebra`, `levenberg-marquardt` and
optional `wide`.
`AstError` is hand-rolled.
Fine — arguably better for a type with a `context` stack and a
`#[track_caller]` location — but it is a spec statement that is simply false now.

### 4.7 `AstError`'s fields are private

The spec shows `pub code`, `pub message`, `pub context`, `pub source_location`.
All four are private with accessors.
This is the right call and needs only a spec correction.

### 4.8 `ErrorCode` has no `Unknown(i32)` fallback

Spec:

```rust
pub enum ErrorCode {
    BadAttribute,        // AST__BADAT
    // ... one variant per AST__XXXX code from ast_err.msg
    Unknown(i32),
}
```

The generated enum has the per-code variants and is `#[non_exhaustive]`, but
there is no `Unknown` variant.
Unrecognised values are handled by `from_raw(value: u32) -> Option<Self>`
returning `None` instead.

For `ast-core` this is arguably tidier.
It matters for `ast-capi`, which is not yet written: `astStatus` can hand back a
value that is not in `ast_err.msg` (a caller's own code set via `astSetPutErr`,
or a code from a newer C release), and with no `Unknown` variant there is no
lossless way to carry it through the Rust error type and back out.
Worth settling before the shim is built rather than after.

*(This entry corrects an error in the first draft of this review, which listed
the `Unknown(i32)` variant as present. The check that produced it matched the
word "Unknown" inside an error description string — `Tunam`, "Unknown AST tuning
parameter name supplied" — rather than a variant name.)*

---

## 5. Delivery order: behind plan, not diverged from it

For separating the two.
Steps are the spec's own numbering.

| Step | Scope | State |
|---|---|---|
| 0 | Bootstrap, vendored C, submodule, CI | **Done**, except CI runs no nightly tier-3/4 job as specified |
| 1 | Workspace, errors, traits, `PointSet`, `PointList` | **Done** |
| 2 | `UnitMap`, `ZoomMap`, `ShiftMap`, `MatrixMap`, `PermMap`, `CmpMap` | **Done** |
| 3 | `BasicFrame`, `CmpFrame` | **Done** |
| 4 | `FrameSet` + integrity | **Done** |
| 5 | Node tree, Native renderer + parser, **JSON renderer + schema** | **Partial** — Native done and byte-exact; JSON never started |
| 6 | `PolyMap`, `LutMap`, `MathMap` | **Done**, plus 20 more mapping classes the step did not list |
| 6a | Rust port of vendored `wcslib` | **Not started** — 3,327 code lines of C still linked |
| 7 | `SkyFrame`, `SpecFrame`, `TimeFrame`, `FluxFrame`, `SpecFluxFrame` | **Done**, but *"oracle benchmarks for per-time-slice attribute mutation go green here"* never happened — that harness does not exist |
| 8 | `Channel`, `FitsChan`, `YamlChan`, `StcsChan`, `MocChan` | **Partial** — Channel and FitsChan done; the other three unported |
| 9 | Regions + region integrity | **Done**, with the geodesic-mesh gap (`MESH-GEO`) open |
| 10 | `Plot` / `Plot3D` + `ast-plugins` | **Not started** |
| 11 | Tier-1 completeness gate; `ast-capi` full export coverage | **Not started** |
| 12 | Cutover | **Not started** |

Steps 1–4, 6, 7 and 9 are complete and ahead of the listed scope; 5 and 8 are
partial with the omissions tracked in `PLAN.md`; 6a, 10, 11, 12 are unstarted.
That is a project behind schedule on breadth, not one that abandoned its plan.

### The tier-1 hard goal is the one to watch

Spec goal 4 makes full `ast_tester` migration a **hard project goal**: every C
test program gets a Rust counterpart, every fixture is copied, and `PLAN.md`
tracks it.

Current state: the C suite has grown from the spec's "~53" to **63** programs
(54 named `test*.c`), against **90** Rust test binaries.
By name, 24 of the 54 have a direct counterpart; another handful are covered
under a different name (`testspecflux` → `test_specfluxframe.rs`,
`testregionmasking` → `test_masking.rs`, `testerror` → `error_model.rs`), so
real coverage is around 30.
Of the remainder, most are blocked on unported subsystems — Plot, Moc, Stc,
Table/FitsTable, XmlChan, YamlChan, resample/rebin/trangrid — which is
consistent with steps 8, 10 and 11 being incomplete.

The gap that is *not* explained by sequencing is bookkeeping.
The spec requires a header comment naming the C ancestor in each ported test:
26 of the 90 test files carry one.
`PLAN.md`'s migration tracker records **5 rows**.
So the port cannot currently answer its own hard goal's progress question from
its own records, and the answer has to be recomputed by hand — as it was for
this document.

---

## 6. Recommendations

Split by whether the spec or the code is the thing that is wrong.

**Fix the code (small, well-scoped) — all three now done:**

1. ~~Implement the `tracing` feature, or delete the `CLAUDE.md` line that
   promises it.~~ §4.1 — implemented in `1ffe825`.
2. ~~Move `Channel::warnings` to a `KeyMap` now that `KeyMap` exists.~~ §4.3 —
   done in `69f1ec0`, as an owned `Option<KeyMap>` rather than the spec's
   borrow, because C's `astWarnings` builds a fresh KeyMap per call and caches
   nothing. Implementing it surfaced a latent defect: `FitsChan` wrote ASTWARN
   cards without recording the texts, so its warnings accessor had always
   returned empty.
3. ~~Add `tran1` / `tran2`.~~ §4.5 — done in `156cf68`, with C's
   `ValidateMapping` checks. That exposed a further gap, now tracked in
   `PLAN.md` §2: the general `transform` path raises `AST__TRNND` nowhere.

**Amend the spec (it is named authoritative, so stale text is load-bearing):**

4. Record `AsAny` and reconcile it with the "no public `Object`" prose. §3.2.
5. Either implement `FrameRef::Name` or retire the `Ident`-vs-`Domain`
   subsection, preserving the reasoning as a rejected alternative. §3.4.
6. Replace "each concrete class provides `builder()`" with the rule actually
   followed, and change the worked example off `SkyFrame`. §4.2.
7. Delete `WritableToChannel` and record that `AnyObject` subsumed it. §4.4.
8. Correct `thiserror`, the `AstError` field visibility, `convert` → `convert_to`
   and `axis` → `axis_iter`. §3.3, §4.6, §4.7.

**Amend the spec's trait sketches, which is the substantive one:**

9. The `Mapping` / `Frame` / `Region` sketches are the part of the spec that
   misled the implementation, because they read as complete surfaces while
   omitting every per-class extension point.
   Either annotate them as illustrative-only, or add the extension-point
   methods — `map_merge`, `equal`, `sub_frame`, `match_template`, `overlay`,
   `reg_pins`, `reg_base_mesh`, `get_def_unc` — to the sketches themselves.
   The second is better: it puts the H1 lesson where a developer reading the
   authoritative design will encounter it before writing code, rather than in a
   hazards file they have to know to consult.
