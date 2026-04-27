# AST Rust port — API design

## Revision history

- **2026-04-27 (rev 5)**: tighten test-coverage scope after the
  upstream sync of `Starlink/starlink-ast` (which added batch 13 and
  more fixture coverage):
  - Full ast_tester migration is now a **hard project goal**, not a
    "ported class-by-class" aspiration. Every `.c` test program in
    `ast_tester/` must have a Rust counterpart in `ast-core/tests/`,
    and every reference fixture (`.head`, `.ast`, `.native`,
    `.fits-*`, `.attr`, `.box`, `.fattr`, `.simp`, `.current`,
    `.moctohtml`, `teststc_eg*`, …) must be copied into the Rust
    test corpus and exercised by the corresponding Rust test or
    harness.
  - Three regression harnesses get explicit Rust analogues:
    `wcsconverter` (drives the `.fits-*` encoding round-trips),
    `simplify` (drives the `.simp` and `.current` references), and
    the plot/grid smoke matrix (drives `.head` + `.attr` + `.fattr`
    + `.box` per fixture).
  - Tier-1 migration is **paced with the implementation** (each
    delivery step ports the tests its classes enable). Step 11 is the
    completion gate that confirms 100% migration, not the step at
    which the bulk of migration work happens.
- **2026-04-27 (rev 4)**: further PR feedback and a project layout
  decision:
  - **Repository layout reworked.** The Rust port lives in its own
    fresh git repository (`starlink-astrs`) with Rust-only commit
    history. The new repo takes ownership of the vendored C
    dependencies (`pal`, `erfa`, `wcslib`) by copying them in-tree at
    `vendor/`, so the everyday build (`cargo build`, `cargo test`)
    works on a fresh clone with no submodule initialisation. A
    submodule at `reference/starlink-ast/` brings in the upstream C
    library and is needed *only* by the optional oracle test tier
    (Rust-vs-C comparison) and the FFI parity tier (existing C
    `ast_tester` programs against `ast-capi`). Replaces the rev-3
    layout's four separate `vendor/` directories.
  - **Axis indexing switched to 1-based in the Rust API.** Rev 3
    proposed 0-based with CFFI translation; per @timj that produces a
    silent footgun (users reading AST docs see `Label(1)` and would
    reach for `set_label(1, …)` and quietly target the wrong axis) and
    requires unwanted translation at the Native-serialisation layer
    (`Lbl1`/`Lbl2` tokens). 1-based for axis attributes and for
    `FrameRef::Index` matches AST docs, the Native format, and the C
    API throughout.
  - **TimeFrame `jd1 + jd2` precision** added as a forward-looking
    note inside the inherited-limitations section: a future
    `TimeFrame` attribute supplying a fixed offset MJD would let the
    internal transforms use ERFA's two-double-precision API without
    changing the public `f64` interface.
- **2026-04-24 (rev 3)**: further review decisions from
  [lsst-dm/starlink-astrs#2](https://github.com/lsst-dm/starlink-astrs/pull/2):
  - Open questions closed:
    - generics-over-`Frame` is dropped (not appropriate for this port's
      goals, per @TallJimbo).
    - enum-based Mapping dispatch is dropped from the spec; may be
      introduced later as a pure internal optimisation.
    - Rust-backed Python binding is out of scope and removed from the
      open-questions list.
  - `test_attr(name)` and `clear_attr(name)` by string are removed from
    the Rust public API. Only typed per-attribute `test_*` / `clear_*`
    methods remain. Per @TallJimbo: stringly-keyed attribute access is
    a `__getattr__`-style pattern we should avoid without a reason.
  - **Axis-indexed attributes** (`Label(1)`, `SkyRef(2)`, …) get an
    explicit section specifying the typed-accessor shape.
  - Ownership of frames within a `FrameSet` made explicit: `DynFrame`
    is a single-owner `Box`; `add_frame` transfers ownership.
  - `wcslib` promoted from "non-goal, revisit later" to **planned for
    a Rust port** later in the implementation. Motivation: the vendored
    snapshot is ~20 years old; upstream has since diverged
    significantly and gained vectorised APIs that `WcsMap` could use if
    we control the implementation. `pal` / `erfa` remain vendored C
    (simple enough to port later if a perf win motivates it).
  - Added a "Known inherited limitations" section noting the A&C-paper
    items (radians-vs-degrees for `SkyFrame`; coordinate-system vs
    domain conflation; double-precision time axes) that cannot be
    fixed while preserving the C ABI.
- **2026-04-24 (rev 2)**: incorporates review feedback from
  [lsst-dm/starlink-astrs#2](https://github.com/lsst-dm/starlink-astrs/pull/2).
  Major changes:
  - Mutability model flipped from immutable+Arc (v1 option A) to
    owned-values with `&mut self` mutation (v1 option B). Driven by
    SCUBA-2 / SMURF evidence of per-time-slice `Epoch` and `SkyRefIs`
    mutation on `SkyFrame`, and plotting's tight-loop `Digits` updates.
  - Trait hierarchy flattened: `Frame` and `Mapping` are now
    independent sibling traits. `FrameSet` and `Region` implement both
    directly. Converged position between @TallJimbo and @dsberry.
  - New first-class section on **FrameSet integrity** and the matching
    **Region integrity** mechanism — attribute mutation on the composite
    propagates to internal mappings / defining positions.
  - Coordinate-array types split into `PointSet` (frame-unaware,
    `ndarray`-backed) and `PointList` (frame-aware, a `Region`),
    matching the existing AST distinction.
  - Operator-overload sugar for mapping composition removed; `then` /
    `alongside` verbs only.
  - String-typed `get_attr` / `set_fmt` removed from the Rust public API
    and relegated to the CFFI shim; typed accessors plus `test_attr`
    and `clear_attr` remain.
  - `Object` demoted from public trait to an implementation-detail
    super-trait, per @TallJimbo's observation that the public interface
    cannot actually be expressed on the bare `Object` methods.
  - `Object::copy()` removed — `Clone` is the deep copy.
  - Explicit semantics for adding a `FrameSet` to another `FrameSet`.
  - CFFI handle story updated for the owned-value world
    (`Arc<RefCell<T>>` at the handle boundary).
- **2026-04-20 (rev 1)**: initial design.

## Background

The Starlink AST library is a ~100 kLOC C library providing world coordinate
system (WCS) manipulation for astronomical data: FITS-WCS parsing, celestial
and spectral frame transformations, graphical output, and associated
utilities. It is consumed widely in the Starlink application suite, by
`pyast`, and by numerous third-party astronomical codes. The current
implementation is a hand-rolled object system with ~50 `Mapping` subclasses,
a `Frame` / `FrameSet` hierarchy built on top of `Mapping`, `Channel`-based
serialisation (Native / XML / YAML / FITS / STC-S / MOC), and `Plot` /
`Plot3D` graphics driven through a link-time plugin system.

This document is the design spec for a full native Rust port of the AST
library, exposing an idiomatic Rust API and a thin CFFI shim that preserves
full binary ABI compatibility with the existing `ast.h`. It is deliberately
a **patterns-only** spec: it commits to the overarching trait shape,
ownership model, error conventions, serialisation architecture, plugin
traits, and FFI strategy, with worked examples for representative classes.
Per-class method-level design is deferred to the implementation plan.

## Goals

1. A pure-Rust implementation of AST with an idiomatic Rust API — traits,
   `Result`-based errors, owned-value mutation via `&mut self`,
   strongly-typed attributes, `ndarray`-backed coordinate data.
2. A `cdylib` shim (`ast-capi`) that preserves full binary ABI
   compatibility with today's `libast.{so,dylib,a}`: existing applications
   link without recompile, without relink, without header changes.
3. Byte-for-byte round-trip compatibility with every existing AST-serialised
   document (Native, XML, YAML, FITS-WCS, FITS-Native-encoded, STC-S, MOC).
4. **Full migration of `ast_tester/` to native Rust.** Every C test
   program (~53 `.c` files) becomes an idiomatic Rust integration test
   under `crates/ast-core/tests/`. Every reference fixture currently
   shipped alongside the C tests — `.head` (132), `.ast` (12),
   `.native`, `.simp`, `.current`, `.fits-{wcs,iraf,pc,aips,aips++,class,dss}`,
   `.attr`, `.fattr`, `.box`, `.moctohtml`, `teststc_eg*`, etc. — is
   copied into `crates/ast-core/tests/fixtures/` and exercised by the
   matching Rust test or regression harness. The three large
   regression harnesses (`wcsconverter`, `simplify`, plot/grid smoke
   matrix) are reimplemented in Rust as test binaries that drive the
   same fixture sets. Reaching 100% migration is a delivery milestone
   (step 11 of the indicative order).
5. All 107+ existing CMake ctests (107 default + 20 PLplot-conditional
   + 1 stress) pass against the Rust-built `libast` via the CFFI shim.
6. Performance parity (within 10%) against the current C library on
   representative transform, simplification, and serialisation workloads.
   **Hot-path constraint**: attribute mutation (notably `Epoch` and
   `SkyRefIs` on `SkyFrame`) must be O(1) with no allocation and no
   locking, to preserve SCUBA-2-style per-time-slice call patterns.

## Non-goals

- Porting `pal` / `erfa` to Rust. These are small and community-trusted;
  they remain vendored C and are built via `cc` from `ast-core`'s
  `build.rs`. Future perf work may motivate a port, but it is not
  planned here.

`wcslib` is in a distinct position: the vendored snapshot is ~20 years
old and has diverged from upstream too far for the upstream to be
adopted directly. A Rust port of the vendored fork is **planned** as
one of the later implementation steps (step 6a in the delivery order),
both to remove the Rust↔C boundary crossing on the transform hot path
and to expose upstream's later-added vectorised APIs to `WcsMap`.
- Exposing the internal `AstObject` struct layout to external C callers.
  `AstObject*` remains opaque, as it is today.
- Supporting a locking-based multi-threaded mutation story through the
  Rust API. Value-typed objects are `Send` but not `Sync`; ownership is
  exclusive within a thread. The CFFI shim preserves the C thread-safety
  story via its own handle registry.
- A Python binding driven off the Rust types. `pyast` calls through the
  C ABI today; it continues to do so unchanged after the port. A future
  Rust-backed Python binding is possible but is neither a goal nor a
  constraint on this spec.

## Approach

### Repository and workspace layout

The Rust port is its own fresh git repository (`starlink-astrs`),
**not** a branch or subdirectory of the existing C `starlink-ast` repo.
This keeps the Rust commit history clean and uncluttered by the
20+ years of C history.

```
starlink-astrs/                # new fresh repo, Rust-only history
├── .gitmodules                # only for the optional reference submodule
├── Cargo.toml                 # workspace root
├── README.md
├── crates/
│   ├── ast-core/              # pure-Rust idiomatic API (rlib)
│   ├── ast-capi/              # cdylib; re-exports ast.h symbols
│   ├── ast-plugins/           # optional Grf / Grf3d backends (PLplot, SVG, logging)
│   ├── ast-ffi-tests/         # builds ast-capi, runs C ast_tester programs
│   ├── ast-oracle-tests/      # links ast-core + reference libast.a
│   └── ast-oracle-bench/      # criterion benches: Rust vs C on matched workloads
├── vendor/                    # copied in-tree, owned by THIS repo
│   ├── pal/                   #   small astrometry routines (was AST's pal/)
│   ├── erfa/                  #   ERFA snapshot (was AST's erfa/)
│   ├── wcslib/                #   AST's wcslib fork, pre-port (deleted at step 6a)
│   └── README.md              #   provenance notes: where each came from, what was changed
└── reference/                 # optional, populated only for tests
    └── starlink-ast/          # git submodule -> Starlink/starlink-ast at a pinned commit
                               #   used by ast-ffi-tests (its ast_tester C programs)
                               #   and ast-oracle-tests (its libast.a as the oracle)
```

Two key principles:

1. **The everyday build does not need the submodule.** A fresh
   `git clone` + `cargo build` + `cargo test -p ast-core` works
   without any submodule initialisation. The vendored C sources in
   `vendor/` are committed in-tree, owned by this repo. This matters
   for packaging (`cargo publish`, distribution tarballs) and for
   contributors who don't care about the C parity tests.
2. **The submodule is opt-in for parity testing.** Running tier-3
   FFI tests (`cargo test -p ast-ffi-tests`) and tier-4 oracle
   correctness tests (`cargo test -p ast-oracle-tests`) requires
   `git submodule update --init reference/starlink-ast`. CI runs them;
   contributors who want them enable the submodule explicitly.

Notes:

- `vendor/pal`, `vendor/erfa`, `vendor/wcslib` are **copies** of what
  the AST C library currently bundles, not git submodules to upstream
  projects. We take ownership: AST-specific symbol-renaming, patches,
  and dynamic-array changes are preserved verbatim during the copy.
  `vendor/README.md` records provenance (source repo, commit hash,
  date copied, list of AST-specific local modifications) so the
  history is recoverable without git.
- `ast-core`'s `build.rs` compiles `vendor/pal/`, `vendor/erfa/`, and
  (until step 6a) `vendor/wcslib/` via the `cc` crate, using the same
  symbol-renaming conventions `palwrap.c` / `pal2ast.h` / `erfa2ast.h`
  use today.
- `ast-oracle-tests` and `ast-ffi-tests` add the submodule path to
  their build dependencies. If the submodule is not initialised,
  those crates are skipped (with a warning) rather than failing the
  workspace build.
- After step 6a (`wcslib` port), `vendor/wcslib/` is deleted from the
  Rust repo. `vendor/pal/` and `vendor/erfa/` stay until / unless a
  later perf decision motivates Rust ports.
- After step 12 (cutover), the upstream C library `libast` is
  effectively retired from active development. The submodule pointer
  may stop being bumped at that point; the oracle tests continue to
  validate against the historical C reference.

### Vendored C dependencies

The Rust repo owns its own copies of the vendored C sources at
`vendor/`. They are not pulled from the reference submodule at build
time, so the everyday workflow does not need it.

`pal` (`vendor/pal/`) and `erfa` (`vendor/erfa/`) are compiled by
`ast-core`'s `build.rs` using the `cc` crate, with the same
symbol-renaming conventions `palwrap.c` / `pal2ast.h` / `erfa2ast.h`
use today, keeping them private to `ast-core`. Porting to Rust is a
later perf decision, not a phase-one goal — the C source is small,
well-tested, and stable.

`wcslib` (`vendor/wcslib/`) is treated differently. The fork is ~20
years old and upstream has diverged too far to re-adopt; @timj's
conclusion on the PR was that once we are reimplementing AST we should
also take on the `wcslib` port to remove the FFI crossing and to
expose upstream's newer **vectorised transform API** to `WcsMap`. That
port is sequenced as **step 6a** of the indicative delivery order,
immediately before `WcsMap` itself. Until then, `vendor/wcslib/`
continues to be built as C by `ast-core`'s `build.rs` and called
through a small private FFI. After step 6a it is removed from the
repo entirely.

`cminpack` is **not** copied across — it is retired in favour of the
`levenberg-marquardt` crate (already validated in the prototype port
at lsst-dm/starlink-astrs). Before retirement, fixture-level validation
confirms Rust-LM output matches cminpack output to within fitting
tolerances; the cminpack source we validate against lives inside the
reference submodule and is not vendored into the new repo.

`vendor/README.md` records provenance for each copied directory: the
upstream URL it came from, the commit hash and date of the import,
and a list of any AST-specific local modifications applied. This
keeps the chain of custody legible without relying on git history of
the source repos.

## Object model

### Sibling traits, no hierarchy

The v1 trait hierarchy (`Object → Mapping → Frame → FrameSet`) is
abandoned. Instead, each public capability is its own trait, at the same
level:

```rust
pub trait Mapping {
    fn nin(&self) -> usize;
    fn nout(&self) -> usize;
    fn is_inverted(&self) -> bool;
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>;
    fn transform_into(&self, pts: &PointSet, forward: bool,
                      out: &mut PointSet) -> AstResult<()>;
    fn invert(&mut self);
    fn simplified(&self) -> AstResult<DynMapping>;
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64>;
    // decompose, map_box, map_split, linear_approx, quad_approx ...
}

pub trait Frame {
    fn naxes(&self) -> usize;
    fn format(&self, axis: usize, value: f64) -> AstResult<String>;
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)>;
    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64>;
    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64>;
    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>>;
    fn norm(&self, coords: &mut [f64]) -> AstResult<()>;
    fn convert(&self, to: &dyn Frame, domains: &str) -> AstResult<Option<FrameSet>>;
    // resolve, intersect, axnorm, matchaxes, axdistance, axoffset ...

    // Self-identifying metadata used by FrameSet / Region integrity:
    fn snapshot(&self) -> DynFrame;                    // deep clone for delta computation
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;
}

pub trait Region: Frame + Mapping {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;
    fn negated(&self) -> DynRegion;
    // mask, mesh, bounds ...
}
```

Notes:

- `trait Mapping` and `trait Frame` are **independent** — neither extends
  the other. This matches @TallJimbo / @dsberry's converged position.
- `trait Region: Frame + Mapping` — Region keeps both super-traits,
  since per @dsberry it must work as both an inside/outside predicate
  (Mapping) and a coordinate-system descriptor (Frame), and that
  combination is load-bearing for the region-integrity feature
  (changing the coordinate system auto-transforms the defining points).
- `Frame::snapshot` / `Frame::map_from` are the primitives used by
  FrameSet integrity (described later). They're on `Frame` because every
  Frame subclass needs to know how to produce a deep-clone of itself and
  how to derive the Mapping to a previous version. The default `map_from`
  uses `Frame::convert` under the hood.
- **No public `Object` trait.** Per @TallJimbo's analysis, the bare
  `Object` methods cannot express real cross-class operations
  (e.g. writing to a Channel is dispatched per-concrete-type, not via
  `Object` methods). A private `Object` super-trait may exist inside
  `ast-core` as an implementation detail for shared bookkeeping (class
  name, ID, Ident), but it is not part of the public API.

### Concrete classes

Each public class is a plain Rust struct holding its state by value:

```rust
pub struct ZoomMap    { /* fields */ }   impl Mapping for ZoomMap { .. }
pub struct CmpMap     { /* fields */ }   impl Mapping for CmpMap  { .. }
pub struct PolyMap    { /* fields */ }   impl Mapping for PolyMap { .. }
// ... one struct per public Mapping subclass

pub struct BasicFrame { /* fields */ }   impl Frame for BasicFrame { .. }
pub struct SkyFrame   { /* fields */ }   impl Frame for SkyFrame   { .. }
pub struct SpecFrame  { /* fields */ }   impl Frame for SpecFrame  { .. }
pub struct CmpFrame   { /* fields */ }   impl Frame for CmpFrame   { .. }

pub struct FrameSet   { /* fields */ }
impl Frame   for FrameSet { .. }         // acts as its current Frame
impl Mapping for FrameSet { .. }         // acts as base→current Mapping

pub struct Circle     { /* fields */ }
impl Region  for Circle   { .. }
impl Frame   for Circle   { .. }         // via Region : Frame
impl Mapping for Circle   { .. }         // via Region : Mapping
// ... and so on for Ellipse, Polygon, BoxRegion, CmpRegion, PointList, NullRegion, ...

pub struct FitsChan   { /* fields */ }   // Channel is its own trait (below)
```

The generic n-axis frame produced by C's `astFrame(naxes, ...)` is
`BasicFrame`. `Box` (AST region) is renamed `BoxRegion` to avoid
colliding with `std::boxed::Box`.

### Trait objects and dispatch

Where the public API needs polymorphism it returns or accepts trait
objects, boxed:

```rust
pub type DynMapping = Box<dyn Mapping + Send>;
pub type DynFrame   = Box<dyn Frame   + Send>;
pub type DynRegion  = Box<dyn Region  + Send>;
```

Concrete-typed call sites (e.g. `ZoomMap::new(...).transform(...)`) use
static dispatch for zero overhead. Heterogeneous collections
(a `FrameSet`'s list of frames; user-composed pipelines) use the `Dyn*`
aliases.

**`Channel` remains a trait** (and is borrow-mutable distinct from
`Mapping` / `Frame`):

```rust
pub trait Channel {
    fn read(&mut self) -> AstResult<Option<DynAnyObject>>;   // discriminated by class
    fn write(&mut self, obj: &dyn WritableToChannel) -> AstResult<usize>;
    fn warnings(&self) -> Option<&KeyMap>;
    fn encoding(&self) -> Encoding;
    fn register_intra(&mut self, t: Arc<dyn IntraTransform>);
}
```

`DynAnyObject` is an enum of all concrete AST classes that can
round-trip through a Channel — enumeration is possible because the
set is closed (no extension classes). `WritableToChannel` is a private
trait implemented by every class and used only to dispatch writes;
this is the "enumerate concrete types" pattern @TallJimbo flagged as
unavoidable.

## Ownership, construction, and mutation

### Owned by value; mutate via `&mut self`

Every concrete class is held and passed by value. `Clone` on a concrete
class is a deep copy (matching `astCopy`). Attribute changes mutate in
place:

```rust
let mut frame = SkyFrame::builder()
    .system(SkySystem::FK5)
    .epoch(2000.0)
    .build()?;

frame.set_epoch(2010.0)?;              // O(1), no allocation, no lock
frame.set_sky_ref_is(SkyRefIs::Origin)?;

let copy = frame.clone();              // deep copy, explicit
```

This restores the SCUBA-2 hot path: each `set_epoch` is a single struct
field update, same cost as the C version (minus the trailing-underscore
variadic overhead).

### Builders for construction

Construction still goes through typestate builders — builders are the
one place in the API where an immutable-style chain is idiomatic and
the ergonomic payoff is highest:

```rust
let frame = SkyFrame::builder()
    .system(SkySystem::FK5)
    .epoch(2000.0)
    .equinox(2000.0)
    .domain("SKY")
    .build()?;
```

Each concrete class provides `builder()`. The builder is consumed by
`build()` which returns the owned concrete value (or `AstResult<Self>`
when construction can fail — e.g. invalid attribute combinations).

### No `edit()`, no `with_*`

In v1 these existed to work around immutability. They're gone: direct
mutation replaces them. If a user wants a modified copy, they
`clone()` then mutate. For batching, multiple `set_*` calls on a
`&mut` binding are O(N) field updates, no allocation.

### Thread safety

All concrete classes are `Send` — they can be moved between threads.
None are `Sync` — they cannot be shared across threads without external
synchronisation, because mutation is via `&mut self` and is deliberately
lock-free. This matches the performance constraint from the review: no
mutex in the hot paths.

Channels (`FitsChan`, etc.) are also `Send` but not `Sync` — they hold
live I/O state.

### Channel / Plot split

- **Value-like, mutable state small**: `Mapping` / `Frame` / `Region`
  concrete classes. Held by value, mutated via `&mut self`, `Clone` is
  deep-copy, `Send` but not `Sync`.
- **Stateful I/O**: `FitsChan`, `YamlChan`, `StcsChan`, `MocChan`. Same
  ownership model; in practice these are not `Clone` (cloning a half-read
  channel is nonsensical). `Send` but not `Sync`.
- **Stateful drawing**: `Plot`, `Plot3D`. Held by value, but drawing is
  routed through a user-supplied `&mut dyn Grf` passed to each draw call
  (the Plot itself carries configuration, not the output stream). `Plot`
  is `Clone` (you might want to draw the same Plot over multiple Grf
  backends).

## Error model

Unchanged from rev 1 except for cosmetics. One canonical error type, one
`Result` alias, no global status in the Rust API.

```rust
pub type AstResult<T> = Result<T, AstError>;

#[derive(Debug, Clone)]
pub struct AstError {
    pub code: ErrorCode,
    pub message: String,
    pub context: Vec<String>,
    pub source_location: Option<&'static Location<'static>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    BadAttribute,        // AST__BADAT
    BadBox,              // AST__BADBX
    // ... one variant per AST__XXXX code from ast_err.msg
    Unknown(i32),
}
```

Rules (unchanged):

- No global state in `ast-core`. `astOK` / `astStatus` / `astWatch` etc.
  exist only in the CFFI shim.
- `thiserror` provides boilerplate; `anyhow` is not used in public APIs.
- `ErrorCode` is generated from `ast_err.msg` by `build.rs`, keeping
  Rust and C codes in lockstep.
- Panics never cross FFI: `ast-capi` wraps each entry point with
  `catch_unwind` and translates panics to `AST__INTER`.
- Optional `tracing` feature emits every constructed `AstError` as a
  `tracing::error!` event; off by default.

## Coordinate data

Two types, matching the C library's own split between the protected
internal `PointSet` and the public `PointList`:

```rust
use ndarray::{Array2, ArrayView2, ArrayViewMut2};

/// Frame-unaware coordinate block. Used inside Mapping implementations.
/// Row-major: shape = (npoint, ncoord).
pub struct PointSet {
    data: Array2<f64>,
}

impl PointSet {
    pub fn zeros(npoint: usize, ncoord: usize) -> Self;
    pub fn from_rows(rows: impl IntoIterator<Item = impl AsRef<[f64]>>) -> Self;
    pub fn from_columns(cols: &[&[f64]]) -> AstResult<Self>;
    pub fn from_ndarray(a: Array2<f64>) -> Self;

    pub fn npoint(&self) -> usize;
    pub fn ncoord(&self) -> usize;
    pub fn view(&self) -> ArrayView2<f64>;
    pub fn view_mut(&mut self) -> ArrayViewMut2<f64>;
    pub fn into_ndarray(self) -> Array2<f64>;

    pub fn point(&self, i: usize) -> &[f64];
    pub fn point_mut(&mut self, i: usize) -> &mut [f64];
    pub fn axis(&self, j: usize) -> impl Iterator<Item = f64> + '_;
}

/// Frame-aware point collection. Implements Region (inherits from it in
/// the C object model). Changing the Frame triggers Region integrity:
/// the stored positions are transformed through the Frame delta so
/// they continue to describe the same physical locations.
pub struct PointList {
    frame: DynFrame,
    points: PointSet,
}

impl Region for PointList { .. }
```

- **`ndarray` is a public dependency of `ast-core`.** The C interop
  remains cheap: `Array2<f64>` owns its data as a contiguous row-major
  `Vec<f64>` which `ast-capi` can hand to C without copy. Users who want
  to avoid the ndarray dependency can still consume `.view()` /
  `.into_ndarray()` outputs trivially.
- **`BAD_VALUE` = `-f64::MAX`**, bit-identical to `AST__BAD` (unchanged
  from v1). NaN accepted on input, `BAD_VALUE` always on output. `Points`
  / `PointSet` buffers cross the CFFI boundary zero-copy.
- **Transform signature**:
  `fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>`,
  with `transform_into(..., out: &mut PointSet)` for buffer reuse.
- **Single-axis / two-axis convenience** `Mapping::tran1` / `tran2` stay
  (avoid forcing users to allocate a `PointSet` for 1-D data).
- **Grid transforms**
  `fn transform_grid(&self, lbnd: &[i64], ubnd: &[i64], tol: f64,
  maxpix: usize, forward: bool) -> AstResult<PointSet>`.
- **Resample / Rebin** generic over `SampleType`. A `DataCube<T>` type
  (sibling of `PointSet` for N-D gridded data with optional variance)
  is the input/output. Detailed signatures deferred to the implementation
  plan.

Generics-over-`Frame` for compile-time coordinate-system checking was
discussed on the PR and dropped: the interaction with AST's dynamic
class structure (needed for serialisation and the C ABI) makes the
payoff marginal for this port.

## Attribute access

### Typed accessors are the primary surface

Every concrete class exposes one typed getter, one typed setter, one
typed tester, and one typed clearer per public attribute:

```rust
impl SkyFrame {
    pub fn system(&self) -> SkySystem;
    pub fn set_system(&mut self, s: SkySystem) -> AstResult<()>;
    pub fn test_system(&self) -> bool;        // attribute explicitly set?
    pub fn clear_system(&mut self) -> AstResult<()>;

    pub fn epoch(&self) -> f64;
    pub fn set_epoch(&mut self, e: f64) -> AstResult<()>;
    pub fn test_epoch(&self) -> bool;
    pub fn clear_epoch(&mut self) -> AstResult<()>;
    // ... one four-tuple per settable attribute
}
```

Each getter returns the AST-documented default when the attribute is
unset, matching C behaviour. `test_*` distinguishes set from default-used.

### Axis-indexed attributes

A subset of AST attributes are indexed by axis — e.g. `Label(1)` /
`Label(2)`, `Unit(axis)`, `Format(axis)`, `SkyRef(1)` / `SkyRef(2)`,
`Symbol(axis)`, `Bottom(axis)`, `Top(axis)`, `Domain(axis)` on certain
frames, etc. The typed pattern expands naturally:

```rust
impl SkyFrame {
    pub fn sky_ref(&self, axis: usize) -> f64;
    pub fn set_sky_ref(&mut self, axis: usize, v: f64) -> AstResult<()>;
    pub fn test_sky_ref(&self, axis: usize) -> bool;
    pub fn clear_sky_ref(&mut self, axis: usize) -> AstResult<()>;
}

impl BasicFrame {
    pub fn label(&self, axis: usize) -> &str;
    pub fn set_label(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()>;
    pub fn test_label(&self, axis: usize) -> bool;
    pub fn clear_label(&mut self, axis: usize) -> AstResult<()>;
    // ... same shape for unit, format, symbol, bottom, top, ...
}
```

Rules:

- **`axis` is 1-based in the Rust API** to match AST's documentation,
  the C API, and the Native serialisation tokens (`Lbl1`, `Lbl2`, …).
  This deliberately violates the usual Rust 0-based convention — the
  alternative would silently retarget the wrong axis when users
  translate code or examples from the AST documentation. Documented
  prominently on every axis-indexed accessor.
- `axis = 0` is rejected with `AstError::BadAxis`. Out-of-range values
  (`axis > naxes()`) produce the same error.
- Non-indexed equivalents (`title`, `system`, `epoch`) are plain
  `fn title(&self)` without the `axis` argument. The typed accessor
  set makes the distinction visible at the type level.

### What was removed

- `get_attr(name: &str) -> AttrValue` is **not** in the Rust public API.
  Users doing programmatic attribute work write a macro or call the
  typed accessors. Rust's macro system covers the "loop over attributes"
  case without runtime string dispatch. The C-binding shim keeps
  `astGetC` / `astGetI` / etc. internally, mapped to the typed getters
  plus a formatter — users of the C API still see the full `astGetC`
  experience.
- `test_attr(name: &str)` and `clear_attr(name: &str)` are **not** in
  the Rust public API either. Per @TallJimbo: stringly-keyed attribute
  presence tests are close kin to Python `__getattr__`-style tricks
  and shouldn't exist without a concrete reason. The typed `test_*` /
  `clear_*` methods cover every attribute the class exposes; the CFFI
  shim maps `astTest` / `astClear` on an attribute-name string to the
  corresponding typed call (parsing axis indices like `SkyRef(1)` at
  the same time).
- `set_fmt("A=1; B=foo")` is not in the Rust public API. `astSet` in C
  parses the format and dispatches to the typed setters inside the shim.
- `set_many(&[(name, value), ...])` is not in the Rust public API. Use
  multiple typed setter calls.
- The `edit()` builder is not in the Rust public API. Direct mutation
  replaces it.

## Mapping composition

```rust
impl dyn Mapping {
    pub fn then(self: Box<Self>, next: DynMapping) -> AstResult<CmpMap>;
    pub fn alongside(self: Box<Self>, other: DynMapping) -> AstResult<CmpMap>;
}

// Also provided on every concrete Mapping type as inherent methods
// so `zoom.then(wcs)?.then(perm)?` works without up-casting first.
impl ZoomMap {
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap>;
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap>;
}
// ... and so on for every concrete Mapping
```

- `then` is series composition (read left-to-right for application order);
  `alongside` is parallel. Verbs only — operator sugar (`Mul` / `BitOr`)
  is not provided, per @TallJimbo's readability concern.
- `Mapping::simplified` returns `DynMapping`; the simplified result may
  be a different class (`UnitMap`, a fused `MatrixMap`, etc.).
- `Mapping::invert` takes `&mut self` and toggles the invert flag in
  place. (`invert()` is not `inverted()` because there is no reason to
  allocate a new object just to flip a bool.)
- Compound constructors still accept owned inputs:
  ```rust
  pub fn new(first: DynMapping, second: DynMapping,
             kind: Composition) -> AstResult<CmpMap>;
  ```

## FrameSet

### Basic API

```rust
pub enum FrameRef<'a> {
    Base,
    Current,
    Index(usize),         // 1-based, matching AST docs and the C API
    Name(&'a str),         // by Ident attribute (see open question below)
}

impl FrameSet {
    pub fn new(base: DynFrame) -> AstResult<FrameSet>;

    pub fn add_frame(&mut self, anchor: FrameRef, map: DynMapping,
                     new_frame: DynFrame) -> AstResult<()>;
    pub fn remove_frame(&mut self, which: FrameRef) -> AstResult<()>;
    pub fn remap_frame(&mut self, which: FrameRef, map: DynMapping) -> AstResult<()>;

    pub fn frame(&self, which: FrameRef) -> AstResult<&dyn Frame>;
    pub fn frame_mut(&mut self, which: FrameRef) -> AstResult<&mut dyn Frame>;
    pub fn mapping(&self, from: FrameRef, to: FrameRef) -> AstResult<DynMapping>;
    pub fn base(&self) -> &dyn Frame;
    pub fn current(&self) -> &dyn Frame;
    pub fn nframe(&self) -> usize;

    pub fn set_base(&mut self, which: FrameRef) -> AstResult<()>;
    pub fn set_current(&mut self, which: FrameRef) -> AstResult<()>;

    pub fn find_frame(&self, template: &dyn Frame, domains: &str)
        -> AstResult<Option<FrameSet>>;
    pub fn add_variant(&mut self, map: DynMapping, name: &str) -> AstResult<()>;
}

impl Frame for FrameSet { .. }       // delegates to current Frame, trapping mutations
impl Mapping for FrameSet { .. }     // base→current mapping
```

**Ownership.** `DynFrame` is `Box<dyn Frame + Send>` — a single-owner
pointer. Calling `fs.add_frame(…, new_frame)` transfers ownership of
`new_frame` into the FrameSet; the caller can no longer touch that
particular Frame instance directly. This is deliberate: if the caller
could keep a handle and mutate the Frame behind the FrameSet's back,
the integrity invariant (below) would silently break. Users who want
an independent copy of a frame after handing one to a FrameSet
`clone()` it first.

`DynMapping` follows the same rule — `add_frame` / `remap_frame`
consume the mapping.

### FrameSet integrity

This is the mechanism that makes attribute mutation on a FrameSet
correctly propagate into the internal Mapping chain. Per @dsberry:

> Changing a (Frame) attribute on a FrameSet causes the same attribute
> to be changed within the current Frame. The Mapping from the original
> Frame to the modified Frame is then found and is used to modify all
> the Mappings that connect with that Frame within the FrameSet.

Rust implementation:

```rust
impl Frame for FrameSet {
    fn set_epoch(&mut self, e: f64) -> AstResult<()> {
        let previous = self.current_frame().snapshot();     // cheap Frame deep-copy
        self.current_frame_mut().set_epoch(e)?;              // direct mutation
        let delta = self.current_frame().map_from(&*previous)?;
        self.fuse_into_current_chain(delta)?;                // integrity update
        Ok(())
    }
    // ... every Frame attribute follows the same pattern
}
```

The `snapshot` + `map_from` primitives on `Frame` are the reason those
methods are part of the trait. Their cost determines how fast
per-time-slice FrameSet attribute updates can be; for the common case
(a `SkyFrame`'s `Epoch` or `SkyRefIs` changing) `snapshot` is a small
struct copy and `map_from` returns a precession or rotation Mapping
that is trivial to compose. No heap allocation in the common path
beyond the resulting `CmpMap` (which is necessary to record the
integrity correction).

**Escape hatch**: `FrameSet::frame_mut(which)` returns `&mut dyn Frame`
and lets the caller mutate a Frame **without** integrity propagation.
This matches `astFrameSet.getFrame()` in C. Documented as the advanced
option; typical callers use the delegated setter methods.

### Adding a FrameSet to another FrameSet

Per @dsberry, the semantics depend on the role:

- `fs.add_frame(anchor, map = inner_fs, new_frame = other)` — `inner_fs`
  is used as a **Mapping**; it is replaced by its own base→current
  mapping before insertion. No flattening of inner frames.
- `fs.add_frame(anchor, map, new_frame = inner_fs)` — `inner_fs` is used
  as a **Frame**; its frames and mappings are **flattened and merged**
  into the outer FrameSet, with the inner's current Frame being
  connected to `anchor` via the supplied `map`.

The Rust API keeps both by relying on the fact that `FrameSet`
implements both `Mapping` and `Frame`. The caller's choice is visible
at the call site. `add_frame` dispatches on the trait-object types of
its arguments.

### `Ident` vs `Domain` for `FrameRef::Name`

@TallJimbo noted astshim used `Ident` rather than `Domain` for its
`FrameDict` lookup type. `Ident` is user-assigned and intended to be
unique; `Domain` groups frames by coordinate family (`PIXEL`, `SKY`)
and is typically **not** unique within a FrameSet.

**Decision** (captured as a commitment): `FrameRef::Name` looks up by
`Ident`. Rationale: identity of a single frame is the useful semantic
for lookup; `Domain` is for template-matching via `find_frame`, which
already exists. This also matches astshim's considered choice.

## Region

The `Region` trait inherits from both `Frame` and `Mapping`. Its
**integrity mechanism** mirrors FrameSet's, per @dsberry:

> When you use the Region reference to change the properties of the
> Frame, it can automatically work out the Mapping from the old Frame
> to the new Frame and use that Mapping to transform the positions that
> define the Region.

```rust
impl Frame for Circle {
    fn set_system(&mut self, s: SkySystem) -> AstResult<()> {
        let previous = self.frame().snapshot();
        self.frame_mut().set_system(s)?;
        let delta = self.frame().map_from(&*previous)?;
        self.transform_defining_points_in_place(&*delta)?;
        Ok(())
    }
    // ...
}
```

The same escape-hatch pattern applies: `Region::frame_mut()` exposes the
raw Frame for callers who do not want the defining positions re-projected.

Regions implement `Mapping::transform` as the inside/outside predicate
mapper (point → 0 or 1, or input-passthrough vs `BAD_VALUE` depending on
attributes) used by masking and overlap operations.

## Plugin system

Unchanged from rev 1 in shape; the five traits (`Grf`, `Grf3d`, `Source`,
`Sink`, `IntraTransform`, `Interpolator`) still form the extension
surface. Error handling is still `tracing`-based on the Rust side with
the `astPutErr` protocol preserved in the CFFI shim.

Most plugin objects become owned `Box<dyn Trait>` under the new
ownership model (`Grf`, `Grf3d`, `Source`, `Sink`, `Interpolator`) —
they are handed to a single consumer per use and nothing shares them.

`IntraTransform` is the exception: it stays `Arc<dyn IntraTransform>`
because a single user transform may be referenced by a long-lived
Channel registry **and** by one or more `IntraMap` instances read from
that channel. Shared ownership is the simplest expression of that
lifetime.

## Serialisation

Unchanged from rev 1: the node-tree intermediate, multiple renderers
(Native, JSON with schema, XML, YAML, FITS-Native), FITS double
formatter with nine-run / zero-run trimming. The `to_node` / `from_node`
protocol is on a private trait now (since public `Object` is gone),
used internally by the Channel-write dispatch.

```rust
// Internal, not in the public prelude:
pub(crate) trait Serializable {
    fn to_node(&self) -> AstResult<Node>;
    fn from_node(n: &Node) -> AstResult<Self> where Self: Sized;
}
```

The `Channel::write(obj: &dyn WritableToChannel)` entry enumerates
classes via a `WritableToChannel` private trait with one implementation
per concrete class (dispatched by `class() -> &'static str`). This is
the "enumerate the concrete types" pattern @TallJimbo identified as
unavoidable.

## CFFI shim (`ast-capi`)

A single `cdylib` crate producing `libast.{so,dylib,a}` with symbols
matching the existing `ast.h` byte-for-byte. Full binary ABI
compatibility: existing applications link without recompile.

### Handle registry in the owned-value world

The v1 handle registry held `Arc<dyn Object>`. Under the new ownership
model the shim uses `Arc<RefCell<AnyObject>>` where `AnyObject` is the
same closed enum used by `Channel::read`:

```rust
enum AnyObject {
    ZoomMap(ZoomMap),
    CmpMap(CmpMap),
    PolyMap(PolyMap),
    // ...
    SkyFrame(SkyFrame),
    FrameSet(FrameSet),
    FitsChan(FitsChan),
    // ...
}

struct Registry {
    slots: RwLock<SlotMap<Handle, Arc<RefCell<AnyObject>>>>,
}
```

- `astClone` = `Arc::clone`; the same `RefCell<AnyObject>` is shared
  across handles, matching C's ref-count-shared semantics exactly.
- `astAnnul` = drop `Arc`; slot freed at refcount zero. `astDelete` =
  force-remove.
- `astSet` etc. take `&mut` on the inner object via `RefCell::borrow_mut`.
  No lock (the RefCell is borrow-checked at runtime but not threaded).
- **Thread safety**: `Arc<RefCell<T>>` is `Send` but not `Sync`. C
  callers who want to share a handle across threads must synchronise
  themselves, exactly matching the current C library's documented
  behaviour.
- The registry's outer `RwLock` is acquired only for slot
  insert/remove/lookup, not for any actual operation on the object.
- `astBegin` / `astEnd` / `astExempt` are implemented on a per-thread
  stack of handle sets as before.

The `AnyObject` enum being closed is acceptable because the AST public
class set is closed (no user-extension classes at the class level; users
extend through `IntraMap`, custom `Channel` sources, custom `Grf`, etc.,
all of which the public API already contains). This also matches the
Channel-write dispatch requirement.

### Global status shim

Unchanged from rev 1. Thread-local `status: i32` plus `AstPutErrFun`
slot. `catch_unwind` around every `extern "C"` entry. Translation of
`Err(AstError)` to status + `putErr` + documented failure sentinel.
Source-location fidelity preserved via the C macro capture of
`__FILE__` / `__LINE__`.

### Variadic `astSet` and constructors

Unchanged from rev 1. `va_list` parsing; format-string tokens dispatched
to the typed setters. The absence of `set_fmt` on the Rust public API
does not affect the shim — it implements the parser internally.

### Generic resample / rebin

Each `astResampleD` / `astResampleF` / `astResampleI` / … is its own
`#[no_mangle] extern "C" fn` monomorphising the Rust generic.

### Plugin re-exports

`astSetPutErr`, `astGrfSet` / `Push` / `Pop`, `astIntraReg`, `astChannel`
source/sink function pointers all bridge to the Rust traits via the five
shims from the plugin section.

### `ast.h` maintenance

`makeh` retired. Current `ast.h` becomes a hand-maintained artifact at
`crates/ast-capi/include/ast.h`. Two CI checks prevent drift (libclang
prototype extraction vs `nm`; cbindgen prototype view vs hand-written
header). Macros, error codes, struct typedefs stay by hand. `ast_err.h`
continues to be generated from `ast_err.msg` by
`cmake/gen_ast_err_h.cmake`, feeding both the C header and the Rust
`ErrorCode` enum.

## Test strategy

Five tiers, each answering a distinct question.

1. **`ast-core` Rust tests — does the idiomatic API work, and does it
   cover everything the C suite covered?**
   Lives at `crates/ast-core/tests/`. The completion goal (a hard
   project goal — see Goals §4) is **full migration** of every C test
   in the upstream `ast_tester/` directory:
   - **One Rust test file per C test program.** `testzoommap.c` →
     `tests/testzoommap.rs`, etc. ~53 files at the time of writing
     (107 default + 20 PLplot + 1 stress in PLAN.md, with several C
     programs producing many ctest cases via fixture iteration).
     Each Rust test has a header comment recording the C ancestor's
     path and any idiomatic Rust divergences (same convention used
     today for Fortran→C ports).
   - **All reference fixtures copied verbatim** into
     `crates/ast-core/tests/fixtures/`:
       - `*.head` — FITS header inputs for the WCS converter (132
         files).
       - `*.ast`, `*.native`, `*.simp`, `*.current` — Native-encoded
         AST dumps used by `ast_astequal` semantic-comparison tests.
       - `*.fits-{wcs,iraf,pc,aips,aips++,class,dss}`,
         `*.fits-class-roundtrip` — FITS-encoding-specific reference
         outputs from the `wcsconverter` regression harness.
       - `*.attr`, `*.fattr`, `*.box` — Plot configuration triples
         used by the plot/grid smoke matrix.
       - `*.moctohtml` — MOC encoding references.
       - `teststc_com`, `teststc_eg1` … `teststc_eg10` — STC-S
         reference inputs.
       - `testxmlchan_com`, `testmocchan_com` — channel-test inputs.
   - **The three regression harnesses become Rust binaries** that
     drive their fixture sets:
       - `wcsconverter` (Rust port of `ast_tester/wcsconverter.c`):
         reads `.head`, writes in each requested encoding, diffs
         against the matching `.fits-*` reference. Powers ~14
         `wcsconv_*` ctests + 20 `wcsconv_*_native_astequal` ctests
         today.
       - `simplify` (Rust port of `ast_tester/simplify.c`): drives
         `.simp` / `.current` references via 3 + 3 ctests today.
       - Plot / grid smoke matrix: ports `testplotter.c` and
         `testgrid.c` to a single Rust harness driven by the
         `.attr` / `.fattr` / `.box` triples per fixture (40 ctests).
   - **All 13 conversion batches in `PLAN.md`** are ported. PLAN.md
     gains a "C → Rust test migration" section that tracks
     completion the way it tracks today's Fortran→C migrations.
   - Tier-1 migration is **paced with the implementation**, not
     batched at the end. Each step in the delivery order ports the
     ast_tester programs whose dependencies it has just made
     buildable: simple-Mappings step ports `testzoommap` /
     `testpermmap` / `testcmpmap` / etc.; the `BasicFrame` step ports
     `testframe` / `testaxis`; the `FrameSet` step ports
     `testframeset` / `testconvert`; the serialisation step ports
     `testchannel` / `testxmlchan` / the `ast_astequal` regression
     harness; and so on. Step 11 of the delivery order is the
     **completion gate** that confirms every program and fixture has
     been ported and is green — not the step at which the bulk of
     migration work happens. The implementation plan (next document)
     enumerates the per-step test set; this spec only commits to the
     end-state.
2. **Native-format regression tests** — byte-exact round-trip against
   captured C output. Fixtures captured from the current C library
   are committed; Rust reads them, writes them back, `diff` must be
   clean. Catches attribute-ordering drift and float-precision
   changes.
3. **`ast-capi` C test harness** — all CMake tests from
   `reference/starlink-ast/ast_tester/` link against the Rust-built
   `libast` and must pass unchanged. Skipped (with a notice) when the
   submodule is not initialised. This tier proves the CFFI layer
   preserves the C ABI; tier 1 already proves the Rust API itself
   covers the same surface.
4. **Oracle correctness tests** (`ast-oracle-tests`) — matched inputs
   driven through both `ast-core` and the C `libast.a` built from the
   `reference/starlink-ast/` submodule. Submodule pinned to a specific
   commit; bumping the pin is an explicit, reviewable action. Bit-exact
   for integer/classification outputs; bounded ULP tolerance for
   floating-point; property-based for random pipelines. Skipped (with
   a notice) when the submodule is not initialised.
5. **Oracle benchmarks** (`ast-oracle-bench`) — `criterion` benches of
   matched workloads on 10k / 100k / 1M point clouds. Rust/C ratio
   reported; >10% slower fails CI.

**New benchmark case added for rev 2**: per-time-slice attribute
mutation hot path. `SkyFrame::set_epoch` and `SkyFrame::set_sky_ref_is`
in a tight loop (as in SMURF's `smf_check_coords.c` and
`smf_cubebounds.c`) must be within 10% of the C timing. This is the
specific performance constraint that drove the v1→v2 redesign.

`PLAN.md` gains a "C → Rust test migration" section tracking tier-1
progress the same way it tracks today's Fortran → C migrations.

## Indicative delivery order

0. **Project bootstrap.** Initialise the new `starlink-astrs` repo,
   copy `pal` / `erfa` / `wcslib` from the current C tree into
   `vendor/` with a `vendor/README.md` recording provenance, and add
   `reference/starlink-ast/` as a submodule pinned to a known-good
   commit. CI configured to run tier-1 / tier-2 / tier-5 tests on
   every PR; tier-3 / tier-4 (submodule-dependent) on a nightly job.
1. Workspace, error model, traits (`Mapping`, `Frame`, `Region`), both
   coordinate array types (`PointSet`, `PointList`).
2. Simplest Mappings: `UnitMap`, `ZoomMap`, `ShiftMap`, `MatrixMap`,
   `PermMap`, `CmpMap`.
3. `BasicFrame`, `CmpFrame`. Run `testframe` / `testcmpframe` in Rust.
4. `FrameSet` including the integrity mechanism. Run `testframeset`.
5. Serialisation: node tree, Native renderer + parser, JSON renderer +
   schema. Native round-trip regression tests go green.
6. Harder Mappings: `PolyMap` (+ `levenberg-marquardt`), `LutMap`,
   `MathMap`.
6a. **Rust port of vendored `wcslib`**, with `WcsMap` as its first
    consumer. Delivers the FFI-free hot path for projections and
    exposes upstream's vectorised transform API to `WcsMap`. `pal` and
    `erfa` remain vendored C. Oracle tests validate bit-compatibility
    against the old C `wcslib` implementation; once green,
    `vendor/wcslib/` is deleted from the repo and the `cc`-driven
    `wcslib` build is removed from `ast-core`'s `build.rs`.
7. `SkyFrame`, `SpecFrame`, `TimeFrame`, `FluxFrame`, `SpecFluxFrame`.
   Oracle benchmarks for per-time-slice attribute mutation go green
   here.
8. `Channel`, `FitsChan` (including FITS double formatter), `YamlChan`,
   `StcsChan`, `MocChan`.
9. Regions (including region integrity).
10. `Plot` / `Plot3D` + `ast-plugins` crate (PLplot, SVG, logging
    backends).
11. **Tier-1 completeness gate.** Confirms every C test program in
    `ast_tester/` has a green Rust counterpart and every reference
    fixture has landed under `crates/ast-core/tests/fixtures/`. By
    this point most of the migration has happened incrementally
    inside steps 2-10 (each step pulling in the tests its classes
    enable); step 11 audits coverage, ports any stragglers (typically
    cross-cutting harnesses like `testhuge` and edge-case-only
    programs that depend on many subsystems), and confirms the
    `wcsconverter` / `simplify` / plot+grid Rust harnesses match
    their C ancestors fixture-for-fixture. Concurrently, `ast-capi`
    reaches full export coverage and `ast-ffi-tests` (tier 3) runs
    the whole C test matrix green against the Rust-built `libast`.
    The port is not declarable complete before this step.
12. Cutover: `ast.h` now comes from the Rust build; vendored C source
    for AST itself is retired. `pal` / `erfa` remain vendored C.

## Known inherited limitations

The following limitations of the current C library are called out in the
A&C paper's "what we would do differently" section. They cannot be fixed
in this port without breaking the C ABI that existing applications link
against, so they are accepted as inherited:

- **`SkyFrame` uses radians, not degrees.** The decision pre-dates
  AST's unit-conversion machinery, and `SkyFrame` has been unable to
  benefit from it because format strings like `"hh:mm:ss"` conflate
  unit conversion with formatting. A future Rust-only `SkyFrame`
  successor could fix this; the CFFI-compatible `SkyFrame` cannot.
- **Coordinate system vs domain conflation.** The A&C authors note
  that `CoordinateSystem` (the mathematical abstraction) and `Domain`
  (the physical space) ought to have been different classes.
  Re-splitting them would require new public APIs; out of scope.
- **Double-precision time axes.** Time axes sometimes need precision
  beyond `f64`; the public API is `f64`-only. Not changing here. A
  future enhancement (proposed by @timj on the PR) could add an
  optional fixed-offset MJD attribute to `TimeFrame`, defaulting to
  zero, so that internal transforms can use ERFA's two-double
  `jd1 + jd2` API for full precision while keeping the public surface
  `f64`. That requires a new public attribute, so it's a forward
  enhancement rather than a fix to the existing port.

## Open questions deferred to the implementation plan

- **Second, cleaner C ABI (`ast2.h`).** Not on day one; revisit after
  the port is complete.
- **`DataCube<T>` detailed signature.** Largest deferred piece.
- **Per-class builder and accessor enumeration** for all ~50 Mapping
  subclasses, all Frame subclasses, all Region subclasses. Mechanical
  once the patterns are implemented for representative cases.
