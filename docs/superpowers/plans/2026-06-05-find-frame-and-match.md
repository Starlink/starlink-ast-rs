# `astFindFrame` / `astMatch` / `astSubFrame` Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the C library's `astFindFrame` / `astMatch` / `astSubFrame` triplet to Rust as the full general-purpose API. This is foundational AST functionality used in many places (template matching, FrameSet alignment, region matching, plot axis selection); the FITS-CLASS encoder is one consumer but the implementation must cover every concrete Frame class and every documented edge case, not just the CLASS-specific subset.

**Architecture:** Add three new methods to the `Frame` trait — `match_template`, `sub_frame`, `find_frame_template` — implemented per concrete class.
Override `FrameSet::find_frame_template` to iterate over its internal frames in C's documented search order (current → base → others) and prefix the base→frame Mapping onto each match.

**Relationship to `astConvert`:** In C, `astConvert(target, source)` and `astFindFrame(target, template)` share most of their machinery — both call into per-class `astMatch`/`astSubFrame` to build the conversion Mapping. The Rust port already has `Frame::convert(self, to)` (used as `astConvert`) but with a per-class implementation that bypasses the `Match`/`SubFrame` trampoline. After this port lands, both `astConvert` (Rust `convert`) and `astFindFrame` (Rust `find_frame_template`) should route through the same `match_template` / `sub_frame` plumbing — keeping them in lockstep with C semantics. This avoids the existing risk where `convert` and `find_frame_template` could disagree on what counts as a "match".

**Tech Stack:** Pure Rust, using existing `Frame`, `FrameSet`, `Mapping`, `CmpMap`, `PermMap`, `UnitMap`, `SkyFrame`, `SpecFrame`, `CmpFrame`, `BasicFrame`, `FluxFrame` types in `crates/ast-core/src/frames/` and `crates/ast-core/src/mappings/`.

---

## Background

C `astFindFrame(target, template, domainlist)` answers "is there a coordinate system with these characteristics reachable from `target`?". When the target is a FrameSet, every Frame in it is searched. When the target is a `CmpFrame`, its component Frames are searched. The matching itself is per-class virtual: `SkyFrame::Match` knows celestial systems align via Euler rotations + epoch conversions, `SpecFrame::Match` knows spectral systems align via SpecMap chains, etc.

The Rust port currently has:
- `Frame::convert(self, to)` — high-level "build a mapping between two specific frames". Returns `Option<FrameSet>` with base=self, current=to. This is what `astConvert` does, and the per-class `convert` already understands celestial / spectral alignment.
- `FrameSet::find_frame(...)` — stub returning `None`.

The Rust port currently lacks:
- `Frame::match_template(template)` — "does this template match me?". The polymorphic kernel of FindFrame.
- `Frame::sub_frame(template, indices)` — "select these axes from me, overlay template attributes, return Mapping + result Frame". Called by `match_template` to do the actual work.
- `Frame::find_frame_template(template, domainlist)` — top-level FindFrame. Default implementation calls `match_template` on self; overridden by FrameSet to iterate internal frames; overridden by CmpFrame to recurse into components.

This plan adds those three methods, with per-class implementations for BasicFrame, SkyFrame, SpecFrame, CmpFrame, FluxFrame, and the FrameSet override.

## Spec coverage

| C source | Rust target | Tasks |
|----------|-------------|-------|
| frame.c:7073 `Match` (default) | `BasicFrame::match_template` (and Frame trait default) | Task 4 |
| frame.c:10838 `SubFrame` (default) | `BasicFrame::sub_frame` (and Frame trait default) | Task 3 |
| frame.c:3735 `FindFrame` (default) | `Frame::find_frame_template` default impl | Task 5 |
| frame.c `Overlay` (default) | `Frame::overlay` default | Task 3 |
| frame.c `PickAxes` | `Frame::pick_axes_snapshot` | Task 3 |
| skyframe.c:6604 `Match` | `SkyFrame::match_template` | Task 6 |
| skyframe.c:9869 `SubFrame` | `SkyFrame::sub_frame` | Task 6 |
| skyframe.c `Overlay` | `SkyFrame::overlay` | Task 6 |
| specframe.c:2998 `Match` | `SpecFrame::match_template` | Task 7 |
| specframe.c:4705 `SubFrame` | `SpecFrame::sub_frame` | Task 7 |
| specframe.c `Overlay` | `SpecFrame::overlay` | Task 7 |
| dsbspecframe.c `Match` / `SubFrame` / `Overlay` | `DsbSpecFrame::{match_template,sub_frame,overlay}` | Task 7b |
| cmpframe.c:5051 `Match` | `CmpFrame::match_template` | Task 8 |
| cmpframe.c:9184 `SubFrame` | `CmpFrame::sub_frame` | Task 8 |
| cmpframe.c `Overlay` | `CmpFrame::overlay` | Task 8 |
| fluxframe.c `Match` / `SubFrame` / `Overlay` | `FluxFrame::{match_template,sub_frame,overlay}` | Task 9 |
| specfluxframe.c `Match` / `SubFrame` / `Overlay` | `SpecFluxFrame::{match_template,sub_frame,overlay}` | Task 9b |
| timeframe.c `Match` / `SubFrame` / `Overlay` | `TimeFrame::{match_template,sub_frame,overlay}` | Task 9c |
| frameset.c:3790 `FindFrame` override | `FrameSet::find_frame_template` override | Task 10 |
| Public `astFindFrame` API | `Frame::find_frame_template` (default + overrides) | Task 5, 10 |
| FrameSet+CmpFrame nesting | Recursive `find_frame_template` resolution | Task 10 |

Tasks 1-2 add scaffolding (attributes + signatures) without breaking the existing build. Task 11 wires FITS-CLASS as the first consumer of the new infrastructure. Task 12 ports any CLASS-specific edge cases. Task 13 ports the corresponding Region overrides (region.c) so Region matching also works through the new API.

## Test strategy

Each Match/SubFrame implementation gets focused unit tests under `crates/ast-core/src/frames/<class>.rs`'s `#[cfg(test)] mod tests`:

- BasicFrame: domain match, axis cardinality, MatchEnd permutation.
- SkyFrame: `Permute=0` template against permuted target, system override, equinox override.
- SpecFrame: System override, StdOfRest override, Unit override.
- CmpFrame: template Frame matched against component, MaxAxes/MinAxes derived from components.
- FrameSet: search order (current → base → others), Mapping prefix.

**Canonical reference test harness:** `reference/starlink-ast/ast_tester/testconvert.c` is the C-side test suite for `astConvert` and `astFindFrame`. It covers 51 numbered assertions exercising `astConvert` and `astFindFrame` over BasicFrame, SkyFrame, SpecFrame, and CmpFrame combinations including:

- BasicFrame ↔ SkyFrame with matching/non-matching Domain (errors 1-18) — exercises the implicit-Domain-makes-them-equivalent rule and the explicit-non-SKY-Domain-blocks-match rule.
- BasicFrame ↔ CmpFrame(SpecFrame, SkyFrame) (errors 19-36) — exercises CmpFrame::SubFrame's PermMap construction.
- SkyFrame ↔ CmpFrame (errors 37-44) — exercises matching the SkyFrame component.
- `astFindFrame(sf, cf)` succeeding when sf is a 2-axis SkyFrame and cf is a CmpFrame containing a SkyFrame — but only after `MaxAxes=3, MinAxes=1` is set on the template.
- `astFindFrame(cf, sf)` failing (target=CmpFrame, template=SkyFrame): without setting the template's MaxAxes/MinAxes (errors 45-46), the cardinality check rejects the 3-axis target.
- Domain-mismatch case at errors 51 (Frame{Domain=ARDAPP} target, SkyFrame{System=GAPPT} template).

Port this test program to a Rust integration test (`crates/ast-core/tests/test_convert_findframe.rs`) early in the plan (Task 5) so each subsequent implementation can be validated against the canonical assertions.

Integration tests:
- `crates/ast-core/tests/test_convert_findframe.rs` (new): line-for-line port of `testconvert.c`'s 51 assertions.
- `crates/ast-core/tests/test_find_frame.rs` (new): additional cases beyond `testconvert.c` for the FITS-CLASS use cases (find SpecFrame in a CmpFrame; find SkyFrame in a CmpFrame; find AZEL SkyFrame in a CmpFrame containing FK5 SkyFrame).

Regression tests:
- `cargo test -p ast-core` — every existing test must still pass after each task. Lib tests currently 828 passing.
- `cargo test -p ast-core --test test_wcsconv` — currently 173 passing. CLASS bucket (10 fixtures) lights up only after Task 11+.

## File structure

Files to create:

- `crates/ast-core/src/frames/find_frame.rs` — top-level `Frame::find_frame_template` default implementation, domain-list parser, helpers shared by per-class implementations.

Files to modify:

- `crates/ast-core/src/frames/mod.rs` — add `match_template`, `sub_frame`, `find_frame_template` to the `Frame` trait, with sensible defaults that delegate to `find_frame.rs`.
- `crates/ast-core/src/frames/basic.rs` — add `match_template`, `sub_frame` impls.
- `crates/ast-core/src/frames/skyframe.rs` — add `match_template`, `sub_frame` impls.
- `crates/ast-core/src/frames/specframe.rs` — add `match_template`, `sub_frame` impls.
- `crates/ast-core/src/frames/cmpframe.rs` — add `match_template`, `sub_frame` impls.
- `crates/ast-core/src/frames/fluxframe.rs` — add `match_template`, `sub_frame` impls.
- `crates/ast-core/src/frames/frameset.rs` — replace `find_frame` stub with the FrameSet override.
- Each Frame class: expose `MatchEnd`, `PreserveAxes`, `Permute`, `MinAxes`, `MaxAxes` if not already. (Most are already exposed for serialisation.)

Files NOT to modify in this plan:

- Anything outside `crates/ast-core/src/frames/`. The CLASS encoder consumer comes in Task 11+ as a follow-up.

---

### Task 1: Frame attributes — `MinAxes`, `MaxAxes`, `MatchEnd`, `PreserveAxes`, `Permute`

C uses these five integer attributes (declared on the Frame base class) to control matching. Defaults: `MinAxes = naxes`, `MaxAxes = naxes`, `MatchEnd = 0`, `PreserveAxes = 0`, `Permute = 1`.

Per-class overrides exist (e.g., SpecFrame defaults `Permute=0`).

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs`
- Modify: `crates/ast-core/src/frames/skyframe.rs`
- Modify: `crates/ast-core/src/frames/specframe.rs`
- Modify: `crates/ast-core/src/frames/cmpframe.rs`
- Modify: `crates/ast-core/src/frames/fluxframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs` — add trait getter / setter signatures with defaults.

- [ ] **Step 1: Add Frame trait methods (with defaults)**

```rust
// In crates/ast-core/src/frames/mod.rs, inside `pub trait Frame { ... }`.

/// Minimum acceptable target axis count when this Frame is used as
/// a template. Default: `self.naxes()`.
fn min_axes(&self) -> usize {
    self.naxes()
}

/// Set the minimum acceptable target axis count.
fn set_min_axes(&mut self, _n: usize) {}

/// Test whether MinAxes has been explicitly set.
fn test_min_axes(&self) -> bool {
    false
}

/// Maximum acceptable target axis count when this Frame is used as
/// a template. Default: `self.naxes()`.
fn max_axes(&self) -> usize {
    self.naxes()
}

/// Set the maximum acceptable target axis count.
fn set_max_axes(&mut self, _n: usize) {}

/// Test whether MaxAxes has been explicitly set.
fn test_max_axes(&self) -> bool {
    false
}

/// MatchEnd: when the template has fewer axes than the target, match
/// the *final* template axes onto the *final* target axes
/// (preserving end alignment) rather than the default initial-axes
/// alignment. Mirrors C `astGetMatchEnd`. Default: false (0).
fn match_end(&self) -> bool {
    false
}

fn set_match_end(&mut self, _v: bool) {}

/// PreserveAxes: when set, `Match` returns a result Frame whose
/// axis count equals the target's (rather than the template's).
/// Mirrors C `astGetPreserveAxes`. Default: false (0).
fn preserve_axes(&self) -> bool {
    false
}

fn set_preserve_axes(&mut self, _v: bool) {}

/// Permute: whether the template is willing to match a target whose
/// axes are in a permuted order. Mirrors C `astGetPermute`.
/// Default: true (1) for plain Frame; false for SkyFrame
/// (overridden in skyframe.rs).
fn permute(&self) -> bool {
    true
}

fn set_permute(&mut self, _v: bool) {}
```

- [ ] **Step 2: Override per concrete class — add internal `Option<usize>` / `Option<bool>` storage where missing**

For each Frame class that doesn't already store these attributes (most don't — only the ones touched by serialisation already do), add `Option<>` fields and the corresponding overrides on the `Frame` trait impl.

Concretely:
- `BasicFrame`: add `min_axes_opt`, `max_axes_opt`, `match_end_opt`, `preserve_axes_opt`, `permute_opt` (all `Option<>`).
- `SkyFrame`: same.  Override `permute()` default to `false` — SkyFrame's `Permute` defaults to 0 in C (skyframe.c).
- `SpecFrame`: same. (`Permute` default is also 0 — SpecFrame is a 1-axis class.)
- `CmpFrame`: same; `min_axes` / `max_axes` defaults are derived from sub-frames (cmpframe.c — sum of sub min/max).
- `FluxFrame`: same.

For each class:

```rust
// Example for BasicFrame in crates/ast-core/src/frames/basic.rs.
// Inside `pub struct BasicFrame { … }`:
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,

// In `impl Frame for BasicFrame`:
fn min_axes(&self) -> usize {
    self.min_axes_opt.unwrap_or_else(|| self.naxes())
}
fn set_min_axes(&mut self, n: usize) {
    self.min_axes_opt = Some(n);
}
fn test_min_axes(&self) -> bool {
    self.min_axes_opt.is_some()
}
// ... similarly for max_axes, match_end, preserve_axes, permute.
```

Update `BasicFrame::new(...)` and any builders to default the new fields to `None`. Update `Clone` if hand-rolled (most are derive(Clone), which Just Works).

- [ ] **Step 3: Build & lib tests**

```bash
cargo build -p ast-core 2>&1 | tail -10
cargo test -p ast-core --lib 2>&1 | tail -3
```

Expected: 828 lib tests still passing. No new test logic — this task is plumbing.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/frames/
git commit -m "frames: expose MinAxes/MaxAxes/MatchEnd/PreserveAxes/Permute attributes

Adds the trait getters/setters required by the upcoming astMatch /
astSubFrame port. Defaults mirror C: MinAxes=MaxAxes=naxes,
MatchEnd=PreserveAxes=0, Permute=1 (0 for Sky/SpecFrame).

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 2: `MatchResult` and `Frame` trait scaffolding

**Files:**
- Create: `crates/ast-core/src/frames/find_frame.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`

- [ ] **Step 1: Create `find_frame.rs` with the result struct**

```rust
//! `astFindFrame` / `astMatch` / `astSubFrame` port (frame.c:3735, 7073, 10838).
//!
//! The triplet answers "is there a coordinate system reachable from
//! this Frame that matches a template?". `match_template` is the
//! per-class predicate; `sub_frame` is the per-class workhorse that
//! selects axes from the target and overlays template attributes;
//! `find_frame_template` is the public entry point that wraps both
//! plus the domain-list filter.

use crate::error::AstResult;
use crate::frames::{DynFrame, Frame, FrameSet};
use crate::mappings::DynMapping;

/// Result of a successful `match_template` call.
///
/// Mirrors the four out-parameters of C `astMatch`:
/// `template_axes`, `target_axes`, `map`, `result`.
pub(crate) struct MatchResult {
    /// For each axis in the result Frame, the (zero-based) index of
    /// the *template* Frame axis it derives from, or `-1` if it has
    /// no template association.
    pub template_axes: Vec<i32>,
    /// For each axis in the result Frame, the (zero-based) index of
    /// the *target* Frame axis it derives from, or `-1` if it has
    /// no target association.
    pub target_axes: Vec<i32>,
    /// Mapping from `target` to `result`. Apply forward to convert
    /// target coordinates into the result coordinate system.
    pub map: DynMapping,
    /// Result Frame: combines target attribute values with any
    /// attributes the template explicitly set.
    pub result: DynFrame,
}

/// Result of a successful `find_frame_template` call. Identical shape
/// to C `AstFrameSet *astFindFrame` (a 2-frame FrameSet, base = target,
/// current = result).
pub(crate) type FindFrameResult = FrameSet;
```

- [ ] **Step 2: Add Frame trait signatures**

```rust
// Add to `pub trait Frame` in crates/ast-core/src/frames/mod.rs.

/// Match `target` against this template.
///
/// Returns `Some(MatchResult)` if `target` can be reached from this
/// template, `None` otherwise. Mirrors C `astMatch`
/// (frame.c:7073).
///
/// `match_sub`: when `true`, allow the target to be a more
/// specialised class than the template (the target is cast down).
/// `false` requires the target to be the same class or more
/// specialised. The wrapper layer (`Frame::find_frame_template`)
/// supplies this argument.
fn match_template(
    &self,
    target: &dyn Frame,
    match_sub: bool,
) -> AstResult<Option<find_frame::MatchResult>>;

/// Select axes from `self` (the target), overlaying `template`'s
/// explicitly-set attributes on the result.
///
/// `target_axes` is a list of (zero-based) axis indices from `self`
/// to retain. `template_axes` is a list of corresponding axis
/// indices into `template` whose attributes should be overlaid; the
/// two arrays have equal length.
///
/// Mirrors C `astSubFrame` (frame.c:10838).
fn sub_frame(
    &self,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>>;

/// Top-level FindFrame: search self (or self's components, in the
/// FrameSet / CmpFrame overrides) for a Frame that the supplied
/// template matches.
///
/// `domainlist` is a comma-separated, case-insensitive priority
/// list. An empty field permits any domain. Mirrors C
/// `astFindFrame` (frame.c:3735).
///
/// Default implementation: try `template.match_template(self, …)`,
/// gate on domain match, and wrap the result as a 2-frame FrameSet.
fn find_frame_template(
    &self,
    template: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<find_frame::FindFrameResult>> {
    find_frame::default_find_frame(self, template, domainlist)
}
```

(Add `pub(crate) mod find_frame;` to `crates/ast-core/src/frames/mod.rs` near the existing module declarations.)

- [ ] **Step 3: Stub default impls returning `Ok(None)` for every concrete class so the crate compiles**

Each Frame impl block gets temporary stubs:

```rust
fn match_template(
    &self,
    _target: &dyn Frame,
    _match_sub: bool,
) -> AstResult<Option<find_frame::MatchResult>> {
    Ok(None)
}

fn sub_frame(
    &self,
    _template: Option<&dyn Frame>,
    _result_naxes: usize,
    _target_axes: &[i32],
    _template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    Ok(None)
}
```

Apply to: BasicFrame, SkyFrame, SpecFrame, DsbSpecFrame, CmpFrame, FluxFrame, SpecFluxFrame, TimeFrame, FrameSet (FrameSet's `find_frame_template` will be overridden in Task 10).

- [ ] **Step 4: Add `default_find_frame` body in find_frame.rs**

```rust
// In crates/ast-core/src/frames/find_frame.rs.

/// Default `find_frame_template` body — mirrors the public C
/// `astFindFrame` (frame.c:3735) without the FrameSet override.
pub(crate) fn default_find_frame(
    target: &dyn Frame,
    template: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<FindFrameResult>> {
    // Domain-list pre-processing: comma-separated, case-insensitive,
    // whitespace-stripped, with an extra leading + trailing comma so
    // strstr-style substring tests match exactly (mirrors
    // frame.c:4112-4124).
    let mut domainlist_canonical = String::with_capacity(domainlist.len() + 2);
    domainlist_canonical.push(',');
    for c in domainlist.chars() {
        if !c.is_whitespace() {
            domainlist_canonical.push(c.to_ascii_uppercase());
        }
    }
    domainlist_canonical.push(',');

    // Per the C contract, astMatch is invoked on the template (with
    // target as second arg). Default Rust dispatch: template.match_template(target).
    let Some(m) = template.match_template(target, false)? else {
        return Ok(None);
    };

    // Gate on domain. Mirrors frame.c:4136-4154.
    let mut domain_canonical = String::with_capacity(m.result.domain().len() + 2);
    domain_canonical.push(',');
    domain_canonical.push_str(&m.result.domain().to_ascii_uppercase());
    domain_canonical.push(',');

    let domain_ok = domainlist_canonical.contains(&domain_canonical[..])
        || domainlist_canonical.contains(",,");
    if !domain_ok {
        return Ok(None);
    }

    // Simplify the mapping (frame.c:4157-4159).
    let map = m.map.simplified()?;

    // Build the 2-frame result FrameSet.
    let mut fs = FrameSet::new(target.snapshot());
    fs.add_frame(crate::frames::FrameRef::Base, map, m.result)?;
    Ok(Some(fs))
}
```

- [ ] **Step 5: Build & lib tests**

```bash
cargo build -p ast-core 2>&1 | tail -10
cargo test -p ast-core --lib 2>&1 | tail -3
```

Expected: builds cleanly, 828 lib tests passing.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/find_frame.rs crates/ast-core/src/frames/
git commit -m "frames: scaffold match_template / sub_frame / find_frame_template

Adds the Frame trait surface for the upcoming astFindFrame port and
the default top-level find_frame_template body. Per-class
match_template / sub_frame are stubbed to Ok(None) — concrete
implementations land in tasks 4-9.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 3: Default `SubFrame` (frame.c:10838)

C's default `SubFrame` selects target axes via a `PermMap`, overlays template attributes via a copy + `astOverlay` call, and returns the (PermMap, result Frame) pair. The Rust port lives in `BasicFrame` because it's the no-frills fallback; CmpFrame and FrameSet have their own overrides (Tasks 8 and 10).

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs` — replace stub `sub_frame` with full impl.
- Modify: `crates/ast-core/src/frames/find_frame.rs` — add a `default_sub_frame` helper that the per-class impls reuse.

- [ ] **Step 1: Write failing test for `BasicFrame::sub_frame`**

```rust
// In crates/ast-core/src/frames/basic.rs, inside `mod tests`:

#[test]
fn sub_frame_select_2_of_3_axes() {
    // Target: 3-axis BasicFrame.
    let target = BasicFrame::new(3);
    let target_axes: Vec<i32> = vec![0, 2]; // pick axis 1 and axis 3.
    let template_axes: Vec<i32> = vec![0, 1];
    let result = target
        .sub_frame(None, 2, &target_axes, &template_axes)
        .unwrap()
        .unwrap();
    let (map, frame) = result;
    assert_eq!(frame.naxes(), 2);
    assert_eq!(map.nin(), 3);
    assert_eq!(map.nout(), 2);
    // Forward: 3-vector -> (axis 1, axis 3).
    let pts = crate::coords::PointSet::from_rows([vec![10.0, 20.0, 30.0]]);
    let out = map.transform(&pts, true).unwrap();
    let row = out.point(0);
    assert!((row[0] - 10.0).abs() < 1e-12);
    assert!((row[1] - 30.0).abs() < 1e-12);
}
```

- [ ] **Step 2: Run test (FAIL — `sub_frame` is stubbed to Ok(None))**

```bash
cargo test -p ast-core --lib basic::tests::sub_frame_select_2_of_3_axes
```

Expected: panic on `.unwrap().unwrap()` because the stub returns `Ok(None)`.

- [ ] **Step 3: Implement `default_sub_frame` in find_frame.rs**

```rust
// In crates/ast-core/src/frames/find_frame.rs.

use crate::mappings::{DynMapping, PermMap};

/// Default `sub_frame` body for plain Frames (no per-axis attributes
/// to overlay beyond Domain/Title/Label/Symbol/Unit/Format/Direction).
///
/// Builds a PermMap that picks `target_axes` from the target's axis
/// space and copies the target into a result Frame whose axis count
/// matches `target_axes.len()`. Template attributes are overlaid via
/// the caller-supplied `overlay` closure.
///
/// Mirrors C `Frame::SubFrame` at frame.c:10838.
pub(crate) fn default_sub_frame<F>(
    target: &dyn Frame,
    template: Option<&dyn Frame>,
    target_axes: &[i32],
    template_axes: &[i32],
    overlay: F,
) -> AstResult<Option<(DynMapping, DynFrame)>>
where
    F: FnOnce(&mut dyn Frame, &dyn Frame, &[i32], &[i32]) -> AstResult<()>,
{
    let result_naxes = target_axes.len();
    let target_naxes = target.naxes();

    // Build the PermMap. C `astPickAxes` is the axis-pick PermMap
    // helper. The forward direction maps target → result; the
    // inverse maps result → target. Axes with -1 in target_axes
    // contribute a synthetic 1-axis (handled by PermMap's "extra
    // input/output" slot).
    let outperm: Vec<i64> = target_axes.iter().map(|&a| (a + 1) as i64).collect();
    let mut inperm = vec![0i64; target_naxes];
    for (i, &t) in target_axes.iter().enumerate() {
        if t >= 0 && (t as usize) < target_naxes {
            inperm[t as usize] = (i + 1) as i64;
        }
    }
    let map: DynMapping = Box::new(PermMap::new(&outperm, &inperm, &[])?);

    // Build the result Frame: snapshot the target, then trim to the
    // selected axes (each Frame class knows how to "pick" axes).
    // For BasicFrame this is just trimming the per-axis arrays;
    // SkyFrame / SpecFrame / FluxFrame have their own pick semantics
    // (see per-class sub_frame overrides).
    let mut result = target.pick_axes_snapshot(target_axes)?;

    // Overlay any explicitly-set template attributes.
    if let Some(t) = template {
        overlay(result.as_mut(), t, target_axes, template_axes)?;
    }

    Ok(Some((map, result)))
}
```

(`pick_axes_snapshot` is a new Frame trait method — added below.)

- [ ] **Step 4: Add `pick_axes_snapshot` to the Frame trait**

```rust
// In crates/ast-core/src/frames/mod.rs:

/// Snapshot this Frame, retaining only the axes listed in
/// `target_axes`. `target_axes` entries of `-1` produce default axes
/// with no inherited data.
///
/// Mirrors C `astPickAxes` (frame.c:8400). The default implementation
/// (BasicFrame-shaped) calls `snapshot()` and trims any per-axis
/// metadata. Frame classes with stronger axis identity (SkyFrame,
/// SpecFrame, FluxFrame) override this to produce the right kind
/// of result Frame.
fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
    // Default: snapshot and rely on the concrete class's `perm_axes`
    // (with truncation handled by the caller) — only correct for
    // pure BasicFrame-shaped subclasses with no per-axis behaviour.
    let _ = target_axes;
    Ok(self.snapshot())
}
```

Override on each concrete class as the per-class `sub_frame` need arises.

- [ ] **Step 5: Wire `BasicFrame::sub_frame` to call `default_sub_frame`**

```rust
// In crates/ast-core/src/frames/basic.rs:

fn sub_frame(
    &self,
    template: Option<&dyn Frame>,
    _result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    crate::frames::find_frame::default_sub_frame(
        self,
        template,
        target_axes,
        template_axes,
        |result, template, _t_ax, _tt_ax| {
            // BasicFrame overlay: copy template Domain/Title if set.
            if template.test_domain() {
                result.set_domain(template.domain());
            }
            // (Other attributes — Title, Label, Symbol, Unit — are
            // per-axis on a BasicFrame; mirror C `Frame::Overlay` if
            // we end up needing those for FITS-CLASS, otherwise
            // leave for a follow-up.)
            Ok(())
        },
    )
}

fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
    let mut snap = BasicFrame::new(target_axes.len());
    // Copy domain.
    snap.set_domain(self.domain());
    // (Per-axis copy goes here — Title/Label/Symbol/Unit/Direction —
    // currently a no-op pending the Frame::pick_axis trait expansion;
    // BasicFrame doesn't carry per-axis state in our port today.)
    Ok(Box::new(snap))
}
```

- [ ] **Step 6: Run test — should pass now**

```bash
cargo test -p ast-core --lib basic::tests::sub_frame_select_2_of_3_axes
```

Expected: PASS.

- [ ] **Step 7: Run full lib tests + commit**

```bash
cargo test -p ast-core --lib 2>&1 | tail -3
git add crates/ast-core/src/frames/basic.rs crates/ast-core/src/frames/find_frame.rs crates/ast-core/src/frames/mod.rs
git commit -m "frames: port default Frame::SubFrame

Mirrors frame.c:10838. Builds a PermMap selecting target axes and
overlays the template's explicitly-set attributes. Used as the
fallback when a per-class SubFrame doesn't have specialised axis
semantics.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 4: Default `Match` (frame.c:7073)

The default `Match` checks axis-cardinality bounds, optionally checks Domain equality, then defers all axis-selection to `SubFrame`.

**Files:**
- Modify: `crates/ast-core/src/frames/basic.rs` — replace stub `match_template` with full impl.
- Modify: `crates/ast-core/src/frames/find_frame.rs` — add `default_match` helper.

- [ ] **Step 1: Write failing test**

```rust
// In crates/ast-core/src/frames/basic.rs, inside `mod tests`:

#[test]
fn match_template_basic_2axes_2axes() {
    let template = BasicFrame::new(2);
    let target = BasicFrame::new(2);
    let m = template.match_template(&target, false).unwrap().unwrap();
    assert_eq!(m.template_axes, vec![0, 1]);
    assert_eq!(m.target_axes, vec![0, 1]);
    assert_eq!(m.result.naxes(), 2);
}

#[test]
fn match_template_basic_axis_mismatch() {
    let template = BasicFrame::new(3);
    let target = BasicFrame::new(2);
    let m = template.match_template(&target, false).unwrap();
    assert!(m.is_none(), "MaxAxes/MinAxes default to template's naxes");
}

#[test]
fn match_template_basic_domain_mismatch() {
    let mut template = BasicFrame::new(2);
    template.set_domain("A");
    let mut target = BasicFrame::new(2);
    target.set_domain("B");
    let m = template.match_template(&target, false).unwrap();
    assert!(m.is_none(), "Domains differ; should fail to match");
}
```

- [ ] **Step 2: Run tests (FAIL)**

```bash
cargo test -p ast-core --lib basic::tests::match_template
```

Expected: all three fail (stub returns Ok(None)).

- [ ] **Step 3: Implement `default_match` in find_frame.rs**

```rust
// Add to crates/ast-core/src/frames/find_frame.rs.

/// Default `match_template` body — mirrors C frame.c:7073.
pub(crate) fn default_match(
    template: &dyn Frame,
    target: &dyn Frame,
    _match_sub: bool,
) -> AstResult<Option<MatchResult>> {
    let target_naxes = target.naxes();
    let min_axes = template.min_axes();
    let max_axes = template.max_axes();

    if target_naxes < min_axes || target_naxes > max_axes {
        return Ok(None);
    }

    // Domain check: template's domain must equal target's, but only
    // when the template has its Domain *explicitly* set (test_domain).
    if template.test_domain() && template.domain() != target.domain() {
        return Ok(None);
    }

    // PreserveAxes / MatchEnd / template axis count.
    let preserve = template.preserve_axes();
    let match_end = template.match_end();
    let template_naxes = template.naxes();

    let result_naxes = if preserve { target_naxes } else { template_naxes };

    let mut template_axes = vec![0i32; result_naxes];
    let mut target_axes = vec![0i32; result_naxes];
    for i in 0..result_naxes {
        let mut t_idx = i as i32;
        let mut tg_idx = i as i32;
        if match_end {
            t_idx += (template_naxes as i32) - (result_naxes as i32);
            tg_idx += (target_naxes as i32) - (result_naxes as i32);
        }
        template_axes[i] = if t_idx < 0 || t_idx >= template_naxes as i32 {
            -1
        } else {
            t_idx
        };
        target_axes[i] = if tg_idx < 0 || tg_idx >= target_naxes as i32 {
            -1
        } else {
            tg_idx
        };
    }

    let Some((map, result)) =
        target.sub_frame(Some(template), result_naxes, &target_axes, &template_axes)?
    else {
        return Ok(None);
    };

    Ok(Some(MatchResult {
        template_axes,
        target_axes,
        map,
        result,
    }))
}
```

- [ ] **Step 4: Wire `BasicFrame::match_template` to call `default_match`**

```rust
fn match_template(
    &self,
    target: &dyn Frame,
    match_sub: bool,
) -> AstResult<Option<find_frame::MatchResult>> {
    crate::frames::find_frame::default_match(self, target, match_sub)
}
```

- [ ] **Step 5: Run tests + commit**

```bash
cargo test -p ast-core --lib basic::tests::match_template
cargo test -p ast-core --lib 2>&1 | tail -3
git add crates/ast-core/src/frames/basic.rs crates/ast-core/src/frames/find_frame.rs
git commit -m "frames: port default Frame::Match

Mirrors frame.c:7073: axis-cardinality + Domain check, then defers
to SubFrame for axis selection and result construction.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 5: Top-level `find_frame_template` and integration test

**Files:**
- Modify: `crates/ast-core/src/frames/find_frame.rs` — done in Task 2; verify default_find_frame works end-to-end.
- Create: `crates/ast-core/tests/test_find_frame.rs`

- [ ] **Step 1: Write integration test**

```rust
//! astFindFrame end-to-end tests.

use ast_core::frames::{BasicFrame, Frame};

#[test]
fn find_frame_basic_self() {
    // Target = template, both 2-axis BasicFrames.
    let target = BasicFrame::new(2);
    let template = BasicFrame::new(2);
    let result = target.find_frame_template(&template, "").unwrap().unwrap();
    assert_eq!(result.nframe(), 2);
    assert_eq!(result.base().naxes(), 2);
    assert_eq!(result.current().naxes(), 2);
}

#[test]
fn find_frame_basic_domain_priority() {
    // Search for a 2-axis frame in a "GRID" domain.
    let target = BasicFrame::new(2);
    let mut template = BasicFrame::new(2);
    template.set_domain("PIXEL");
    // Template has a domain but target has none, so no match.
    let result = template.find_frame_template(&target, "").unwrap();
    assert!(result.is_none());
}

#[test]
fn find_frame_basic_domain_list_blank() {
    // Empty domainlist permits any domain.
    let target = BasicFrame::new(2);
    let template = BasicFrame::new(2);
    assert!(target.find_frame_template(&template, "").unwrap().is_some());
}

#[test]
fn find_frame_basic_domain_list_match() {
    // Domain list explicitly mentions the target domain.
    let mut target = BasicFrame::new(2);
    target.set_domain("PIXEL");
    let template = BasicFrame::new(2);
    assert!(target
        .find_frame_template(&template, "PIXEL,SKY")
        .unwrap()
        .is_some());
}

#[test]
fn find_frame_basic_domain_list_miss() {
    let mut target = BasicFrame::new(2);
    target.set_domain("PIXEL");
    let template = BasicFrame::new(2);
    assert!(target
        .find_frame_template(&template, "GRID,SKY")
        .unwrap()
        .is_none());
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p ast-core --test test_find_frame
```

Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_find_frame.rs
git commit -m "tests: add astFindFrame integration tests for BasicFrame

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 5b: Port `ast_tester/testconvert.c` to Rust integration test

`reference/starlink-ast/ast_tester/testconvert.c` (237 lines, 51 numbered assertions) is the C library's canonical `astConvert` / `astFindFrame` test. Port it line-for-line to a Rust integration test so each downstream task validates against the same assertions.

**Files:**
- Create: `crates/ast-core/tests/test_convert_findframe.rs`

- [ ] **Step 1: Write the integration test (template)**

```rust
//! Port of reference/starlink-ast/ast_tester/testconvert.c.
//!
//! 51 numbered assertions against `astConvert` and `astFindFrame`
//! covering BasicFrame, SkyFrame, SpecFrame, and CmpFrame combinations.

use ast_core::frames::{BasicFrame, CmpFrame, Frame, FrameRef, SkyFrame, SpecFrame, SkySystem};

#[test]
fn convert_findframe_full_suite() {
    // Errors 1-3: astConvert(BasicFrame{Domain=SKY}, SkyFrame).
    let mut bf = BasicFrame::new(2);
    bf.set_domain("SKY");
    let sf = SkyFrame::new(SkySystem::Icrs);
    let df = SpecFrame::new();
    let cf = CmpFrame::new(Box::new(df.clone()), Box::new(sf.clone()));

    // -- error 1-3: convert succeeds, base = bf, current = sf, mapping = UnitMap.
    let fs = bf.convert(&sf, &[]).unwrap().expect("Error 4");
    assert!(fs.base().equals(&bf), "Error 1");
    assert!(fs.current().equals(&sf), "Error 2");
    assert!(fs.mapping_base_to_current().unwrap().is_unit_map(), "Error 3");

    // -- error 5-7: reverse direction.
    let fs = sf.convert(&bf, &[]).unwrap().expect("Error 8");
    assert!(fs.base().equals(&sf), "Error 5");
    assert!(fs.current().equals(&bf), "Error 6");
    assert!(fs.mapping_base_to_current().unwrap().is_unit_map(), "Error 7");

    // -- error 9: with bf.domain = "NOTSKY", convert(bf, sf) returns None.
    let mut bf_notsky = BasicFrame::new(2);
    bf_notsky.set_domain("NOTSKY");
    assert!(bf_notsky.convert(&sf, &[]).unwrap().is_none(), "Error 9");
    assert!(sf.convert(&bf_notsky, &[]).unwrap().is_none(), "Error 10");

    // -- error 11-18: bf with no domain — same checks 1-7 should pass again.
    let bf_clear = BasicFrame::new(2);
    let fs = bf_clear.convert(&sf, &[]).unwrap().expect("Error 14");
    // ...

    // [Continue porting errors 19-51 line-for-line.]
}
```

(The full body is mechanical; mirror each error number with a Rust assertion. Use `equals` from the existing `Frame` trait for frame equality.)

- [ ] **Step 2: Run the test**

```bash
cargo test -p ast-core --test test_convert_findframe
```

Expected: at this point, many assertions will FAIL because Tasks 6-10 haven't landed. The test serves as a moving target: each Task 6+ commit should reduce the failure count.

- [ ] **Step 3: Commit (with `#[ignore]` initially if it makes CI noisy)**

```bash
git add crates/ast-core/tests/test_convert_findframe.rs
git commit -m "tests: port ast_tester/testconvert.c (51 assertions, partial)

Initial port — many assertions fail until astFindFrame / astSubFrame
are fully landed in tasks 6-10. Test marked #[ignore] until then.

Generated with AI

Co-Authored-By: SLAC AI"
```

After each subsequent task, remove the `#[ignore]` flag once that task's assertion subset passes.

---

### Task 6: `SkyFrame::Match` and `SkyFrame::SubFrame`

C `SkyFrame::Match` (skyframe.c:6604) is significantly more involved than the default: it gates on axis cardinality, then calls `SubFrame`. `SkyFrame::SubFrame` (skyframe.c:9869) handles the heavy lifting:
- If target is a SkyFrame, build the celestial-conversion Mapping using the **existing** Rust `convert` logic (already ported).
- If target is a CmpFrame, recurse.
- If target is a basic Frame, defer to `default_sub_frame`.

The `convert` infrastructure already exists — we just route SkyFrame::SubFrame through it.

**Files:**
- Modify: `crates/ast-core/src/frames/skyframe.rs`

- [ ] **Step 1: Write failing test**

```rust
// In crates/ast-core/src/frames/skyframe.rs, inside `mod tests`:

#[test]
fn match_template_skyframe_fk5_target_is_galactic() {
    // Template is FK5; target is Galactic. Both 2-axis SkyFrames.
    // Expect a successful match with a non-trivial Mapping.
    let template = SkyFrame::new(SkySystem::Fk5);
    let target = SkyFrame::new(SkySystem::Galactic);
    let m = template.match_template(&target, false).unwrap().unwrap();
    assert_eq!(m.template_axes, vec![0, 1]);
    assert_eq!(m.target_axes, vec![0, 1]);
    // The Mapping must transform a known Galactic point back to FK5.
    // Sanity: just check that it's not a UnitMap (i.e. there's a real
    // rotation in there).
    assert!(m.map.nin() == 2 && m.map.nout() == 2);
}

#[test]
fn match_template_skyframe_against_basicframe_fails() {
    let template = SkyFrame::new(SkySystem::Fk5);
    let target = crate::frames::BasicFrame::new(2);
    let m = template.match_template(&target, false).unwrap();
    assert!(m.is_none(),
        "BasicFrame is not a SkyFrame; SkyFrame template should fail match");
}
```

- [ ] **Step 2: Run tests (FAIL — stub returns Ok(None))**

```bash
cargo test -p ast-core --lib skyframe::tests::match_template
```

- [ ] **Step 3: Implement `SkyFrame::match_template` and `sub_frame`**

```rust
// In crates/ast-core/src/frames/skyframe.rs, replace the stub impls:

fn match_template(
    &self,
    target: &dyn Frame,
    _match_sub: bool,
) -> AstResult<Option<find_frame::MatchResult>> {
    use crate::frames::CmpFrame;

    let target_naxes = target.naxes();
    let min_axes = self.min_axes();
    let max_axes = self.max_axes();
    if target_naxes < min_axes || target_naxes > max_axes {
        return Ok(None);
    }

    // SkyFrame requires the target to be a SkyFrame (directly or
    // inside a CmpFrame); if it's a CmpFrame, defer to its
    // match_template override which will recurse into components.
    if let Some(cmp) = target.as_any().downcast_ref::<CmpFrame>() {
        return cmp_frame_match_with_sky_template(self, cmp);
    }
    let target_sky = match target.as_any().downcast_ref::<SkyFrame>() {
        Some(s) => s,
        None => return Ok(None),
    };

    // Domain check.
    if self.test_domain() && self.domain() != target.domain() {
        return Ok(None);
    }

    // SkyFrame template's `Permute=0` default disallows axis-permuted
    // matches (e.g. a (Dec, RA) target). When the target is permuted
    // away from (lon, lat), only match if Permute is explicitly true.
    if !self.permute() {
        // C `SkyFrame::Match` consults astGetSkyAxis(target, 0)
        // (skyframe.c:6685+) — the lat/lon axis identity is the
        // discriminator. The Rust SkyFrame stores axis perm in its
        // internal axes; check perm_axes equivalent.
        let target_perm = target_sky.axis_permutation();
        if target_perm != [1, 2] {
            return Ok(None);
        }
    }

    // Build axis association. SkyFrame templates always have
    // template_naxes = 2 = target_naxes for the SkyFrame branch.
    let result_naxes = if self.preserve_axes() { target_naxes } else { 2 };
    let template_axes = vec![0i32, 1];
    let target_axes = vec![0i32, 1];

    // SubFrame produces the celestial conversion mapping using the
    // existing `convert` infrastructure (which already does the
    // SkyFrame -> SkyFrame Euler-rotation + epoch conversion path).
    let Some((map, result)) =
        target.sub_frame(Some(self), result_naxes, &target_axes, &template_axes)?
    else {
        return Ok(None);
    };

    Ok(Some(find_frame::MatchResult {
        template_axes,
        target_axes,
        map,
        result,
    }))
}

fn sub_frame(
    &self,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // SkyFrame::SubFrame mirrors skyframe.c:9869. The general path:
    // 1. Take a snapshot of self (the target SkyFrame).
    // 2. Overlay the template's explicit attributes (System,
    //    StdOfRest, Equinox, Epoch, ...) on the snapshot — this
    //    produces the result Frame.
    // 3. Build the Mapping target -> result using the existing
    //    `convert` logic.

    // Snapshot self.
    let mut result = self.clone();
    if let Some(t) = template {
        if let Some(t_sky) = t.as_any().downcast_ref::<SkyFrame>() {
            // Overlay: copy attributes that the template has
            // explicitly set onto `result`.
            overlay_sky_attributes(&mut result, t_sky);
        }
    }

    // Build the mapping self -> result via convert().
    let map: DynMapping = match self.convert(&result, &[])? {
        Some(fs) => fs.mapping_base_to_current()?,
        None => return Ok(None),
    };

    // Apply axis permutation if target_axes selects axes in a non-
    // identity order.
    let final_map = if target_axes != [0, 1] || result_naxes != 2 {
        // (Compose with a PermMap to handle permutation/axis-pick.)
        compose_axis_pick_map(map, target_axes, self.naxes())?
    } else {
        map
    };

    let _ = template_axes;
    Ok(Some((final_map, Box::new(result))))
}
```

The helper `overlay_sky_attributes`, `cmp_frame_match_with_sky_template`, `compose_axis_pick_map` are private to `skyframe.rs` (or in `find_frame.rs` if shared). Implement them as needed; refer to skyframe.c:8100-8400 for the C overlay code.

- [ ] **Step 4: Run tests + commit**

```bash
cargo test -p ast-core --lib skyframe::tests::match_template
cargo test -p ast-core --lib 2>&1 | tail -3
git add crates/ast-core/src/frames/skyframe.rs crates/ast-core/src/frames/find_frame.rs
git commit -m "frames: port SkyFrame::Match and SubFrame

Mirrors skyframe.c:6604 (Match) and skyframe.c:9869 (SubFrame).
Uses the existing convert() infrastructure for celestial alignment.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 7: `SpecFrame::Match` and `SpecFrame::SubFrame`

Same shape as SkyFrame, but for spectral systems. C source: specframe.c:2998 (Match), specframe.c:4705 (SubFrame). The existing Rust `SpecFrame::convert` already handles the spectral conversion via SpecMap chains; route SubFrame through it.

**Files:**
- Modify: `crates/ast-core/src/frames/specframe.rs`

- [ ] **Step 1-4: Same TDD shape as Task 6, with test fixtures using SpecFrame templates and targets in different rest-frames.**

The CLASS encoder use case (find a SpecFrame with `System=vrad,StdOfRest=lsrk,Unit=m/s,MinAxes=1,MaxAxes=3`) must succeed when the target is a CmpFrame containing a SpecFrame. The CmpFrame branch is handled by Task 8; SpecFrame's own branch handles the SpecFrame-target case.

- [ ] **Step 5: Commit**

---

### Task 8: `CmpFrame::Match` and `CmpFrame::SubFrame`

C `CmpFrame::Match` (cmpframe.c:5051) is the linchpin for the CLASS use case: it lets a SkyFrame template match a CmpFrame *if the CmpFrame contains a SkyFrame*, and similarly for SpecFrame templates. The MaxAxes/MinAxes attributes on the template are crucial — the CmpFrame's axis count won't equal the template's, so the template must explicitly say it tolerates a wider range.

C `CmpFrame::SubFrame` (cmpframe.c:9184) has two flavours:
- Template is itself a CmpFrame: select axes from each component.
- Template is non-CmpFrame: identify which component the template applies to, recurse on that component, and graft a PermMap into the full CmpFrame axis space.

**Files:**
- Modify: `crates/ast-core/src/frames/cmpframe.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn match_template_cmpframe_skyframe_template() {
    // Target: CmpFrame(SkyFrame(FK5), SpecFrame(FREQ)) -- 3 axes.
    let sky = SkyFrame::new(SkySystem::Fk5);
    let spec = SpecFrame::new();
    let cmp = CmpFrame::new(Box::new(sky), Box::new(spec));

    // Template: SkyFrame with MaxAxes=3 to accept the 3-axis target.
    let mut template = SkyFrame::new(SkySystem::Fk5);
    template.set_max_axes(3);
    template.set_min_axes(2);

    let m = template.match_template(&cmp, false).unwrap().unwrap();
    // Result is a 2-axis SkyFrame.
    assert_eq!(m.result.naxes(), 2);
}
```

- [ ] **Step 2-4: Implement CmpFrame::match_template and sub_frame.**

The match logic mirrors cmpframe.c:5060-5300:
- Build template_axes / target_axes that map result axes to (component_index, axis_within_component) tuples.
- For each component, call `template.match_template(component, false)`. If exactly one component matches, that's the result.
- For 2D template against (Sky+Spec) target: SkyFrame template matches the Sky component, SpecFrame template matches the Spec component.

The sub_frame logic builds the Mapping by composing per-component sub_frame results in parallel (`CmpMap` with `Composition::Parallel`).

- [ ] **Step 5: Commit**

---

### Task 9: `FluxFrame::Match` / `SubFrame` / `Overlay`

FluxFrame is a 1-axis class. Match defers to base + flux-system overlay; SubFrame returns a flux-aware result Frame using the existing flux conversion infrastructure. C: fluxframe.c.

- [ ] **Step 1-5: Standard TDD, commit.**

---

### Task 7b: `DsbSpecFrame::Match` / `SubFrame` / `Overlay`

DsbSpecFrame extends SpecFrame with image-frequency sideband flag. Match must align IF (image-frequency) and SB (sideband) attributes; SubFrame routes through the existing DsbSpecFrame conversion. C: dsbspecframe.c.

- [ ] **Step 1-5: Standard TDD, commit.**

---

### Task 9b: `SpecFluxFrame::Match` / `SubFrame` / `Overlay`

SpecFluxFrame is a 2-axis specialised composition (1 spectral + 1 flux). C: specfluxframe.c. Standard shape.

- [ ] **Step 1-5: Standard TDD, commit.**

---

### Task 9c: `TimeFrame::Match` / `SubFrame` / `Overlay`

TimeFrame is a 1-axis class. C: timeframe.c.

- [ ] **Step 1-5: Standard TDD, commit.**

---

### Task 10: `FrameSet::find_frame_template` override

C `FrameSet::FindFrame` (frameset.c:3790) is the integration point. It iterates the FrameSet's internal frames in priority order (current → base → others), calls `Frame::find_frame_template` on each, and prefixes the FrameSet's base→frame Mapping when a match is found.

**Files:**
- Modify: `crates/ast-core/src/frames/frameset.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn frameset_find_frame_currentfirst() {
    // FrameSet: base=GRID, current=Sky(FK5), with a SpecFrame in
    // between. find_frame with a SpecFrame template should find the
    // intermediate frame.
    let mut fs = FrameSet::new(Box::new(BasicFrame::with_domain(2, "GRID")));
    fs.add_frame(FrameRef::Base, /* mapping */, Box::new(SpecFrame::new()))?;
    fs.add_frame(FrameRef::Index(2), /* mapping */, Box::new(SkyFrame::new(SkySystem::Fk5)))?;

    let mut template = SpecFrame::new();
    template.set_min_axes(1);
    template.set_max_axes(3);

    let result = fs.find_frame(&template, &[]).unwrap().unwrap();
    assert_eq!(result.nframe(), 2);
    // Base of result == FrameSet's base.
    assert_eq!(result.base().domain(), "GRID");
    // Current of result == the SpecFrame we found.
    assert!(result.current().as_any().is::<SpecFrame>());
}
```

- [ ] **Step 2: Replace stub `FrameSet::find_frame` with the override**

```rust
pub fn find_frame(
    &self,
    template: &dyn Frame,
    domainlist: &[&str],
) -> AstResult<Option<FrameSet>> {
    let target_nframe = self.nframe();
    let target_current = self.current_idx() + 1;  // 1-based
    let target_base = self.base_idx() + 1;        // 1-based

    // Build the search-order list: current first, then base, then
    // remaining frames. Mirrors frameset.c:3922-3934.
    let mut order: Vec<usize> = vec![target_current];
    if target_base != target_current {
        order.push(target_base);
    }
    for i in 1..=target_nframe {
        if i != target_current && i != target_base {
            order.push(i);
        }
    }

    // Canonicalise the domainlist (frame.c:4112-style).
    let domainlist_canonical = canonicalise_domainlist(domainlist);

    for domain_field in domainlist_canonical.iter() {
        for &target_idx in &order {
            let selected = self.frame(FrameRef::Index(target_idx))?;
            let found = selected.find_frame_template(template, domain_field)?;
            if let Some(found) = found {
                let map_base_to_current = found.mapping_base_to_current()?;
                let result_frame = found.current().snapshot();

                // Prefix the base→frame[target_idx] mapping.
                let prefix = self.mapping_base_to_index(target_idx)?;
                let combined: DynMapping = Box::new(CmpMap::new(
                    prefix,
                    map_base_to_current,
                    Composition::Series,
                )?);
                let combined_simplified = combined.simplified()?;

                let mut result_fs = FrameSet::new(self.base().snapshot());
                result_fs.add_frame(FrameRef::Base, combined_simplified, result_frame)?;
                return Ok(Some(result_fs));
            }
        }
    }

    Ok(None)
}
```

`canonicalise_domainlist` is a small helper.

- [ ] **Step 3: Run tests + commit**

```bash
cargo test -p ast-core --lib frameset::tests::find_frame
cargo test -p ast-core --test test_find_frame
git add crates/ast-core/src/frames/frameset.rs
git commit -m "frames: implement FrameSet::find_frame override

Mirrors frameset.c:3790. Iterates internal frames in current→base→
others order, prefixes the base→frame mapping onto each match.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 11: Wire `find_frame` into the FITS-CLASS encoder

With the infrastructure in place, the CLASS encoder can now follow C `CLASSFromStore` line-for-line for the SpecFrame/SkyFrame conversions.

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write/mod.rs` — add a `encode_class` entry point that takes the FrameSet directly.
- Modify: `crates/ast-core/src/fitschan/enc_class.rs` — replace `write_to_cards` with a CLASS-aware version.
- Modify: `crates/ast-core/src/fitschan/mod.rs` — switch the FitsClass dispatch to `encode_class`.

- [ ] **Step 1: Add `encode_class` in wcs_write/mod.rs**

The signature differs from `encode_wcs_per_plane` because CLASS needs the live FrameSet (not just the FitsStore) to query SpecFrame/SkyFrame attributes for the velocity / Az-El conversion.

```rust
pub(crate) fn encode_class(
    obj: &AnyObject,
    cards: &mut CardList,
    dim: &[Option<f64>],
) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => return Err(AstError::new(
            ErrorCode::Nodef,
            "FITS-CLASS encoding requires a FrameSet object",
        )),
    };
    let primary_crvals = extract_input_crvals(cards, ' ', frameset.base().naxes());
    let planes = collect_plane_stores(frameset, dim, Encoding::FitsClass, &primary_crvals)?;

    if let Some(primary) = planes.first() {
        crate::fitschan::enc_class::class_from_store(&primary.store, cards, frameset, dim)?;
    }
    Ok(())
}
```

- [ ] **Step 2: Implement `class_from_store` in enc_class.rs**

Port `CLASSFromStore` line-for-line from fitschan.c:6369-6998. The structure is documented in the C source already:

```rust
pub(crate) fn class_from_store(
    store: &FitsStore,
    cards: &mut CardList,
    fs: &FrameSet,
    dim: &[Option<f64>],
) -> AstResult<()> {
    // 1. Identify celestial + spectral axes (FindLonLatSpecAxes).
    // 2. Validate: spectral & celestial axes must be at indices 0,1,2;
    //    no PV parameters; SPECSYS=SOURCE; CUNIT spectral = Hz/kHz/MHz/GHz.
    //    See fitschan.c:6485-6745.
    //
    // 3. Emit NAXIS, NAXIS1, NAXIS2, NAXIS3 (handle the
    //    degenerate-spatial-axis case at fitschan.c:6750-6768).
    //
    // 4. Emit CRPIX, CRVAL (with the spectral CRVAL adjusted by
    //    `val *= specfactor; val -= rf;` per fitschan.c:6795-6798),
    //    CTYPE, CDELT.
    //
    // 5. Emit EQUINOX, DATE-OBS (via the format-by-MJD-vs-1999 dance
    //    at fitschan.c:6842-6866), RESTFREQ, IMAGFREQ.
    //
    // 6. Ensure OBJECT and LINE cards exist (HasCard test).
    //
    // 7. Compute VELO-LSR, VLSR, DELTAV via SpecFrame conversion:
    //    - velofrm = SpecFrame{ System=vrad, StdOfRest=lsrk, Unit=m/s,
    //                            MinAxes=1, MaxAxes=3 }
    //    - fsconv1 = curfrm.find_frame(velofrm, "")  // <- THIS IS THE
    //                                                //    BLOCKED CALL
    //    - velofrm = fsconv1.current()
    //    - freqfrm = velofrm.clone() with System=freq, StdOfRest=Source, Unit=Hz
    //    - fsconv1 = freqfrm.convert(velofrm)
    //    - aval[0] = crval[axspec]*specfactor; aval[1] = aval[0] + cdelt[axspec]*specfactor
    //    - fsconv1.transform(aval) -> velocities
    //    - emit VELO-LSR, VLSR (= aval[0]), DELTAV (= aval[1] - aval[0]).
    //    See fitschan.c:6888-6943.
    //
    // 8. Compute AZIMUTH, ELEVATIO via SkyFrame Az/El conversion:
    //    - radecfrm = SkyFrame{ Permute=0, MinAxes=3, MaxAxes=3 }
    //    - fsconv1 = curfrm.find_frame(radecfrm, "")
    //    - azelfrm = SkyFrame{ System=AZEL, Permute=0, MinAxes=3, MaxAxes=3 }
    //    - fsconv2 = curfrm.find_frame(azelfrm, "")
    //    - map3 = compose(fsconv1.mapping(current_to_base), fsconv2.mapping(base_to_current))
    //    - radec = (crval[axlon]*DD2R, crval[axlat]*DD2R)
    //    - map3.transform(radec) -> azel
    //    - normalise via azelfrm.norm
    //    - emit AZIMUTH (azel[0]*DR2D), ELEVATIO (azel[1]*DR2D).
    //    See fitschan.c:6946-6989.
}
```

Each numbered step lands as a sub-step with its own focused test against an appropriate fixture (`class-sfl_fits-class` for the spectral path; `class-write_fits-class` for the AZ/EL path).

- [ ] **Step 3: Switch mod.rs dispatch**

```rust
// In mod.rs, replace:
//   Encoding::FitsClass => wcs_write::encode_wcs_per_plane(... enc_class::write_to_cards),
// with:
//   Encoding::FitsClass => wcs_write::encode_class(obj, &mut self.cards, &dims),
```

- [ ] **Step 4: Run wcsconv suite + commit**

```bash
cargo test -p ast-core --test test_wcsconv -- class --nocapture 2>&1 | tail -30
cargo test -p ast-core 2>&1 | tail -5
```

Expected: at least the simpler CLASS fixtures pass (`class-sfl`, `class-ait`, `class-arc`, `class-gal`, `class-sin`, `class-write`, `class-olddate`); skyspec3d / jan_class may need additional follow-up.

```bash
git add crates/ast-core/src/fitschan/enc_class.rs crates/ast-core/src/fitschan/wcs_write/mod.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "fitschan: port CLASSFromStore using astFindFrame infrastructure

Replaces the placeholder enc_class write path with a line-for-line
port of fitschan.c:6369-6998. Uses the new Frame::find_frame_template
to extract SpecFrame/SkyFrame from the cube's CmpFrame for velocity
and Az/El emission.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 12: CLASS edge-case fixtures

The fixtures `skyspec3d_class_fits_class`, `jan_fits_class_ast`, `jan_fits_class_rt` may exercise paths Task 11 didn't cover (spatial pixel-axis degenerate handling, FK4 epoch, ClassTrans round-trip). Triage and fix.

- [ ] **Step 1: Identify the failure mode for each.**
- [ ] **Step 2: Trace the C source for the corresponding branch.**
- [ ] **Step 3: Port the missing logic; iterate.**
- [ ] **Step 4: Commit per fixture or per branch.**

---

### Task 13: Region overrides

`Region` (region.c) overrides `Match` and `SubFrame` to delegate to its encapsulated Frame plus its bounding contour. The overrides are needed for any AST consumer that treats a Region as a Frame for matching purposes (e.g., aligning two NDFs whose WCS frames are Regions).

**Files:**
- Modify: `crates/ast-core/src/regions/mod.rs` (or wherever the Region trait/struct lives in the port).

- [ ] **Step 1: Find the existing Region trait surface in Rust.**
- [ ] **Step 2: Add `match_template`, `sub_frame`, `find_frame_template` overrides mirroring region.c.**
- [ ] **Step 3: TDD with a Box / Circle / Ellipse fixture.**
- [ ] **Step 4: Commit.**

---

### Task 14: Public API surface check

Audit the project's existing `astFindFrame` callers (search for `find_frame`, `findframe`, `FindFrame`) and update any that were stubbed-out or routed around the old `FrameSet::find_frame` returning None.

- [ ] **Step 1: `grep -rn "find_frame" crates/` — list every consumer.**
- [ ] **Step 2: For each, verify the new behaviour matches the C library's expected output.**
- [ ] **Step 3: Add regression tests covering at least the existing wcsconv fixtures that exercise virtual-system matching (e.g., the AzEl one).**
- [ ] **Step 4: Commit.**

---

## Self-review checklist

After implementing the plan:

1. Tasks cover every C source range listed in the spec coverage table.
2. Every code step has either a `cargo build` or `cargo test` follow-up step.
3. Test cases use the existing test patterns (`#[test]` inside `mod tests`).
4. No placeholder `TODO` / `TBD` strings.
5. Method signatures are consistent across tasks: `match_template`, `sub_frame`, `find_frame_template`, `pick_axes_snapshot`.
6. No regressions: lib tests stay at 828, wcsconv stays at ≥173 throughout.

## Risks

- **`SkyFrame::SubFrame` / `SpecFrame::SubFrame` may not produce the exact same Mapping as C** when the celestial / spectral conversion path differs from our existing `convert` impl. Mitigate: spot-check the wcsconv fixtures against C's wcsconverter output after Task 6/7.
- **`CmpFrame::Match` is the most complex piece** (cmpframe.c:5051 is 522 lines). Allow extra time for Task 8.
- **`pick_axes_snapshot`'s default impl may be wrong for SkyFrame's lat/lon axis identity** — SkyFrame's axes are not interchangeable. Override on SkyFrame if the default copies the wrong axis.
- **Nested CmpFrame** (e.g., `CmpFrame(SkyFrame, CmpFrame(SpecFrame, BasicFrame))`) — the recursion depth is bounded by the number of frame classes but the template-axis bookkeeping is fiddly. Cmpframe.c:9180-9700 has the full recipe.
