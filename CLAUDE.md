# starlink-astrs — Developer Guide for Claude

## What is this?

A pure-Rust port of the Starlink AST library (world coordinate system
manipulation for astronomical data — FITS-WCS, celestial / spectral
frames, plotting, FITS / Native / YAML / STC-S / MOC serialisation).
The port keeps full **binary ABI compatibility** with the existing C
`libast.{so,dylib,a}` via a thin CFFI shim, so existing applications
(including `pyast`, the Starlink suite, and DS9) link against the
Rust build with no source or recompile changes.

**The design is fixed.** Read it before making non-trivial changes:
[`docs/superpowers/specs/2026-04-20-rust-port-design.md`](docs/superpowers/specs/2026-04-20-rust-port-design.md).
The CLAUDE.md you're reading is the operational summary; the spec is
authoritative when they disagree.

## Repository layout

```
starlink-astrs/
├── Cargo.toml                 # workspace root
├── crates/
│   ├── ast-core/              # pure-Rust idiomatic API (rlib)
│   ├── ast-capi/              # cdylib; re-exports ast.h symbols
│   ├── ast-plugins/           # Grf / Grf3d backends (PLplot, SVG, logging)
│   ├── ast-ffi-tests/         # builds ast-capi, runs C ast_tester programs
│   ├── ast-oracle-tests/      # links ast-core + reference libast.a
│   └── ast-oracle-bench/      # criterion: Rust vs C performance
├── vendor/                    # C source we own and build into ast-core
│   ├── pal/                   # AST's pal fork (build with cc)
│   ├── erfa/                  # AST's erfa fork (build with cc)
│   ├── wcslib/                # AST's wcslib fork; deleted at step 6a
│   └── README.md              # provenance: source, commit, mods
├── reference/
│   └── starlink-ast/          # git submodule, OPTIONAL, used by tier-3/4 tests
├── docs/
│   ├── superpowers/specs/     # design specs — the overarching port design
│   │                          #   is 2026-04-20-rust-port-design.md
│   ├── superpowers/plans/     # per-step implementation plans
│   ├── audits/                # structured gap sweeps
│   ├── issues/                # per-subsystem port-gap catalogues
│   ├── reports/               # point-in-time assessments of the whole port
│   └── porting-hazards.md     # failure modes, indexed by trigger
└── PLAN.md                    # index of outstanding work + test-migration tracker
```

Two principles:

1. **`vendor/` is owned by this repo.** Everyday `cargo build` /
   `cargo test` works on a fresh clone with no submodule init.
2. **`reference/starlink-ast/` is opt-in.** Initialised only for
   tier-3 (`ast-ffi-tests`) and tier-4 (`ast-oracle-tests`), and for
   the fixture-mirror check. **CI runs neither tier.** The workflow
   has two jobs: `fixture-mirror`, which initialises the submodule and
   diffs the fixture trees, and `test`, which runs tier-1 / tier-2
   (`cargo test -p ast-core`) plus clippy, fmt and docs without it.
   The design (`2026-04-20-rust-port-design.md` step 0) calls for a
   nightly job running tiers 3 and 4; it has not been created, so
   those tiers are run by hand only.

## Build & test

```shell
cargo build                                  # everything, no submodule needed
cargo test -p ast-core                       # tier 1: idiomatic Rust tests
cargo test -p ast-core --test meta test_simplify_regression::  # tier 2: native-format byte-exact

# Tiers requiring the submodule:
git submodule update --init reference/starlink-ast
cargo test -p ast-ffi-tests                  # tier 3: C tests vs Rust libast
cargo test -p ast-oracle-tests               # tier 4: bit/ULP comparison
cargo bench -p ast-oracle-bench              # tier 5: Rust-vs-C performance
```

Useful flags during development:

```shell
cargo test -p ast-core --features tracing    # emit AstError and simplify
                                             # events as tracing events
cargo +nightly miri test -p ast-core         # ub check on hot paths
```

## Architecture (compressed)

The full design is in the spec; key load-bearing facts:

- **Sibling traits, no hierarchy.** `trait Mapping` and `trait Frame`
  are independent. `trait Region: Frame + Mapping`. `FrameSet` and
  every `Region` implement *both* `Mapping` and `Frame` directly.
  There is no `Object` trait in the public API.
- **Owned values, `&mut self` mutation.** Every concrete class is
  `struct Foo { … }` held by value. `Clone` is the deep copy.
  `set_epoch(&mut self, e)` etc. mutate in place. Builders construct
  new objects; there is no `with_*` / `edit()` / `Arc<T>` story.
- **`Send`, not `Sync`.** Move across threads is fine. Sharing a
  reference across threads is not — there are no locks in hot paths.
- **`AST__BAD` = `-f64::MAX`** (bit-identical to C). Inputs accept
  `f64::NAN`; outputs always use `BAD_VALUE`.
- **1-based axis indices** in the Rust API (`set_label(1, …)`,
  `FrameRef::Index(1)`). Deliberate Rust-convention violation;
  matches AST docs, the C API, and the Native serialisation tokens
  (`Lbl1`, `Lbl2`, …).
- **FrameSet integrity** and **Region integrity**: when a
  `Frame`-level attribute is mutated *through* a FrameSet or Region,
  the internal mappings / defining points are auto-updated to
  preserve the invariant. `frame_mut()` is the documented escape
  hatch that bypasses integrity.
- **Errors**: `AstResult<T> = Result<T, AstError>`. `ErrorCode` enum
  generated from `vendor/ast_err.msg` (or `reference/.../ast_err.msg`)
  by `ast-core`'s `build.rs`.
- **CFFI**: handles in `ast-capi` are `Arc<RefCell<AnyObject>>` over
  a closed `enum AnyObject { … }`. `astClone` / `astAnnul` /
  `astBegin` / `astEnd` / `astExempt` / `astSetPutErr` /
  `astStatus` all preserved exactly.
- **Serialisation**: each class implements `to_node` / `from_node`
  on a private `Serializable` trait once. Format-specific renderers
  (`native`, `json`, `xml`, `yaml`, `fits_native`) walk the `Node`
  tree. Native output is byte-exact with the C library.

## Hot-path constraint (do not regress)

The SCUBA-2 / SMURF pipeline mutates `SkyFrame::Epoch` and
`SkyFrame::SkyRefIs` per time slice (see
`reference/starlink-ast/applications/smurf/libsmf/smf_check_coords.c`
~line 193 and `smf_cubebounds.c` ~line 750). These methods must be:

- **O(1)** — single field update, no iteration.
- **Allocation-free.**
- **Lock-free.**

If you find yourself adding a `Mutex`, `RwLock`, allocation, or
non-trivial work into any `SkyFrame::set_*` method, stop and
reconsider — you're regressing the hot path.

**There is currently no automated performance gate.**
`ast-oracle-bench` is a placeholder and CI runs no benchmark, so a
hot-path regression will not be caught for you. Until it is wired up,
re-run `cargo run --release --example bench_per_class -p ast-core`
against the C oracle by hand when you touch a transform path, and
compare with `docs/issues/mapping-perf-vs-c-oracle.md` (snapshot
2026-05-19). Note that snapshot already records several cheap mappings
5–42× slower than C at bulk N; the SCUBA-2 / LSST path (PolyMap,
MatrixMap) is not among them.

## Bulk-transform optimisation (per-mapping evaluation)

The default per-point loop pattern — `for i in 0..npoint { check
point_has_bad; do math }` — has a known faster variant for transforms
whose math is a polynomial in the inputs (no division, no
log/sqrt/etc):

1. Substitute `BAD_VALUE` → `f64::NAN` on a copy of the input.
2. Run the math without per-point branching (lets the compiler
   vectorise, or lets you call BLAS via ndarray's `general_mat_mul`,
   `axpy`, etc — Apple Accelerate or OpenBLAS via `blas-src`).
3. Convert any `NaN` outputs back to `BAD_VALUE`.

IEEE 754 propagates NaN through arithmetic for free, so the kernel
runs branchless and the only added cost is two memory-bandwidth-bound
passes.

**Whether it's worth doing depends on the mapping**, and must be
evaluated per class via `ast-oracle-bench`. The setup costs (input
clone + two NaN passes + kernel call overhead) only amortise when
both:

- The math body is large enough per point — `MatrixMap` at 3×3 (the
  typical AST direction-cosine case) is *too small*; the prototype
  was 15-22% slower than the scalar loop at every npoint. At 6×6 it
  wins ~1.3×; at 100×100, ~5×.
- npoint is large enough — a single-point call always loses on the
  bulk path (the SCUBA-2 hot path *is* bulk-N within a time slice,
  so this is fine for that workload, but Frame-attribute integrity
  re-fusion uses 1-point probes that should stay scalar).

When adding a new `Mapping` whose math is polynomial in the inputs:
prototype both paths behind a temporary `bench-internals` cargo
feature, run a parametric criterion bench across the realistic shape
× npoint grid, and only commit the GEMM/BLAS path if it wins on the
shapes that actually occur for that class. For mappings whose
typical shape stays at ≤3 inputs/outputs, the scalar loop is
generally the right answer — `MatrixMap` was evaluated and rejected
on those grounds; do not re-prototype it without new motivation.

## Code style

- Rust 2024 edition. MSRV pinned in `rust-toolchain.toml`.
- Lints: `#![warn(missing_docs)]` on all public items in `ast-core`.
  CI enables `RUSTFLAGS="-D warnings"`.
- `cargo fmt` is enforced. `cargo clippy --workspace -- -D warnings`
  is a CI gate.
- C99 is the language used in `vendor/`. Don't add C++ to vendored
  sources; if you need to wrap C++ for a plugin, do it in a separate
  crate.
- `unsafe` is allowed in `ast-capi` (the entire crate is FFI) and in
  the private vendored-C FFI module of `ast-core`. Anywhere else,
  `unsafe` requires a justifying comment and review.
- Public item names: AST attribute names → snake_case (`SkyRefIs` →
  `set_sky_ref_is`). Class names stay PascalCase (`PolyMap`,
  `SkyFrame`). Concrete-class types disambiguate from traits where
  needed: `BasicFrame` (vs trait `Frame`), `BoxRegion` (vs
  `std::boxed::Box`).

## Before porting a class — find C's extension points

C's per-class `vtab` methods are not uniformly boilerplate. Some are
the extension mechanism a *generic driver in another file* calls back
into per class, and collapsing those into one central function makes
the driver's control flow unportable. This is the most expensive
mistake this project has made, twice — see H1 in
[`docs/porting-hazards.md`](docs/porting-hazards.md).

Before writing Rust for a class, list its `vtab` entries in the C
source and classify each:

- **Ceremony** — `astGetAttrib` / `astSetAttrib` string dispatch,
  per-attribute dump/load, loader glue. Collapse freely; that is the
  point of the port.
- **Architecture** — a method a generic algorithm dispatches to per
  class. Port it as a trait method at the *same granularity as C*,
  even when one central `match` would pass today's fixtures.

The test is mechanical: **does a generic driver in another file call
this through the vtab?** If yes, the Rust port needs the same seam.

Known architecture, not boilerplate: `MapMerge`, `Transform`,
`Simplify`, `Equal`, `MapSplit`, `Match`, `SubFrame`, `Overlay`,
`RegPins`, `RegBaseMesh`, `RegBaseBox`, `GetDefUnc`, `GetAxis`.

Two related rules:

- **Match C's construction shape, not just its arithmetic.** A chain
  that is mathematically equivalent but structurally different will
  diverge in whatever consumes the structure — the simplifier, the
  serialiser, `MapSplit` — and it surfaces far from the change (H2).
- **If the C source has two names, keep two fields** until a fixture
  proves they cannot diverge. Cheap to split early, expensive late (H3).
- **Write down how a C mutable-state convention maps onto owned
  values** before the first call site that depends on it (`Invert`,
  `Frozen`, reference-counted sharing). A paragraph in the module doc
  is enough; without it, every call site re-derives it differently (H4).

## Stop signals

Three signals mean *stop patching and go read the C source*. Each is
cheap to check and each has been ignored here at measurable cost
(H5–H7 in [`docs/porting-hazards.md`](docs/porting-hazards.md)).

1. **A threshold, tolerance or iteration count with no analogue in the
   C source.** Grep the C source for the constant. If C gets the right
   answer without it, C is doing something structurally different —
   find out what. If you must ship the heuristic to unblock something,
   do what `5d9aaa6` did: name it, say what it costs, schedule the port.
2. **Your fix makes the target case pass and breaks a negative
   control.** Negative-control fixtures encode "C does *not*
   simplify/merge/convert this". Breaking one means your rule is not
   C's rule. Two or three in a row is an architectural mismatch, not a
   missing rule. Never loosen a negative control to pass a positive case.
3. **The error shrinks monotonically across successive fixes and never
   reaches zero.** You are curve-fitting. Stop at the second round and
   look two layers away from the symptom.

Corollary: at the *second* one-off unit / epoch / time-scale conversion
added at a call site, stop and port C's conversion function instead (H8).

## Adding a new Mapping / Frame / Region

The pattern is identical regardless of which trait it implements.
Use an existing concrete class as the template (e.g. `ZoomMap` once
it lands).

1. Create `crates/ast-core/src/{mappings,frames,regions}/<name>.rs`.
2. Define the struct with `&mut self` setters for attributes; use a
   `<Name>Builder` for construction. Strongly typed accessors per
   attribute. Axis-indexed attributes take `axis: usize` (1-based).
3. Implement the appropriate trait(s).
4. Implement the private `Serializable` trait (`to_node` /
   `from_node`) — without this, the class can't round-trip through a
   `Channel`.
5. Add the variant to `enum AnyObject` in `ast-capi` (this also
   unlocks the Channel-write dispatch).
6. Write the corresponding C-ABI shim in `ast-capi` — usually one
   `#[no_mangle] extern "C" fn` per public method, plus the
   constructor.
7. Add a Rust test under `crates/ast-core/tests/` ported from the
   matching `reference/starlink-ast/ast_tester/test<name>.c`, and add
   it to its area's test program (see "Porting a test" below).
   Copy any reference fixtures into
   `crates/ast-core/tests/fixtures/`.
8. Mark the test ported in `PLAN.md`'s "C → Rust test migration"
   tracker.

### Landing several classes in one pass

Breadth sweeps are legitimate — they unblock fixtures and surface shape
problems early. They are also how both this port and the prototype
accumulated their largest debts, because a sweep that passes the
existing tests looks finished (H9). If you sweep, record the debt in the
same commit range:

- Name the C algorithm you did *not* port, per class, in the module doc,
  with the C file and line. `polygon.rs`'s planar-mesh note citing
  `polygon.c:4329` is the model — not a bare `TODO`.
- Add a `PLAN.md` row per gap stating **what would expose it**, not just
  what is missing. "Latent; needs a sky-polygon fixture" names the
  missing test.
- Prefer `todo!()` or an error to a plausible return value. The
  prototype's `BoxRegion::overlap()` returned a hardcoded `4` for weeks.

## Porting a test from `ast_tester/`

1. Read `reference/starlink-ast/ast_tester/<test>.c` to understand
   what it covers.
2. Create `crates/ast-core/tests/<test>.rs` with a header comment
   recording the C ancestor's path and any idiomatic Rust
   divergences, and add it as a module to the area program it belongs
   to, `crates/ast-core/tests/<area>/main.rs`. The package sets
   `autotests = false` because one test program per file made the suite
   take most of an hour (each program links all of ast-core, and macOS
   scans each new program on its first run); `test_area_membership`
   fails for a file no area program names. Run one file's tests with
   `cargo test -p ast-core --test <area> <test>::`.
3. Use the **public Rust API only** (no private modules from
   `ast-core`). The Rust test should also serve as a worked example
   for users.
4. Copy any reference fixtures the C test reads (`*.head`, `*.ast`,
   `*.native`, `*.simp`, `*.fits-*`, etc.) into
   `crates/ast-core/tests/fixtures/`. Don't symlink to the submodule
   — fixtures must work with the submodule absent.
5. For the three regression harnesses (`wcsconverter`, `simplify`,
   plot/grid smoke), don't reinvent — port to a Rust binary under
   `crates/ast-core/tests/` and drive with the same fixture set as
   the C harness.
6. Update `PLAN.md`.

### Comparators and skip modes

A harness that can pass without comparing is not a gate. Three in this
repo could, and the one that survived longest hid a real defect (H10–H11).

- **No skip mode.** A case that cannot be compared yet goes in a named
  exclusion list with a per-case reason string, plus a companion test
  that fails when an excluded case starts passing.
  `NATIVE_READ_EXCLUDED` in `test_wcsconv.rs` is the pattern.
- **Data-driven harnesses hard-fail.** Printing a failure count and
  returning `Ok` is not a test.
- **Relaxations are per case, named in source, justified in a comment.**
  Never widen a global tolerance to make one case pass. Every relaxation
  is counted in `PLAN.md` §6 so the total cannot drift.
- **Whatever a comparator normalises becomes unfalsifiable.** `trim()`
  and `split_whitespace()` in a byte-exactness comparator make trailing
  whitespace invisible — which is exactly the defect fixed in `86810ce`.
  If the comparator normalises anything, the corpus needs a fixture that
  exercises what was normalised.
- **Before documenting a gate, run it.** A declared gate that does not
  hold is worse than no gate, because it stops anyone looking (H12).

## Modifying `ast-capi` (CFFI shim) — extra care

The C ABI is **frozen**. Existing applications must keep linking.

- Don't change any function signature in
  `crates/ast-capi/include/ast.h` without a deliberate ABI bump and
  spec update. The two CI checks (libclang prototype extraction vs
  `nm` on the cdylib; cbindgen view vs the hand-written header)
  catch unintentional drift.
- New `#[no_mangle] extern "C" fn` symbols require a matching
  declaration in `ast.h` — both checks fail otherwise.
- `astBegin` / `astEnd` / `astExempt` / `astSetPutErr` / `astStatus`
  semantics are part of the C contract; their handle-stack and
  thread-local-status implementations are tested by tier-3.
- Variadic `astSet` / constructors parse the printf-style format
  inside the shim and dispatch to typed Rust setters. Don't expose
  format-string parsing on the Rust public API.
- Any `extern "C" fn` body must be wrapped in `catch_unwind` so a
  Rust panic translates to `AST__INTER` rather than crossing FFI.

## What NOT to do

| Anti-pattern | Why |
|---|---|
| Adding `Sync` to a concrete class | Hot paths are explicitly lock-free; `Sync` invites locks |
| Using `Arc<T>` in `ast-core` for non-plugin types | Owned values + `&mut self` is the rule |
| Using `Mutex` / `RwLock` inside `SkyFrame` (or any Frame attribute setter) | Regresses the SCUBA-2 hot path |
| Using `f64::NAN` as the canonical bad-value sentinel in outputs | `BAD_VALUE` (= `-f64::MAX`) is the contract; `NaN` is accepted on input only |
| 0-based axis arguments | All axis indices are 1-based throughout the public API |
| String-typed `get_attr(name)` / `set_fmt(spec)` in `ast-core` | These exist only in `ast-capi` for the C-ABI dispatch path |
| `Mul` / `BitOr` operator overloading for mapping composition | Use `then` / `alongside` verbs only — readability |
| Reaching into `reference/starlink-ast/` from `ast-core`'s `build.rs` | The everyday build must not depend on the submodule |
| Rewriting `vendor/wcslib/` in C | The Rust port is step 6a; keep the C source unchanged until then |
| Symlinking test fixtures from `reference/starlink-ast/ast_tester/` | Fixtures must be committed into `crates/ast-core/tests/fixtures/` so tier-1 works without the submodule |
| Hand-editing `ast_err.h` | Generated from `ast_err.msg` by `build.rs` |
| Collapsing a per-class `MapMerge` / `RegPins` / `SubFrame` into one central `match` | These are C's extension points, not boilerplate — see "Before porting a class" |
| Adding a tolerance or iteration count that has no analogue in the C source | Stop signal 1; the CRPIX ladder cost a week |
| Loosening a negative-control fixture to make a positive case pass | Stop signal 2; inverts what the fixture exists to prove |
| A skip mode in a data-driven harness | A gate that can pass without comparing is not a gate |
| Creating `Object::copy()` or `with_*` methods | Removed in design — `Clone` is the deep copy, mutation is direct |

## Important pointers

- **Porting hazards** (read the entry matching what you're about to do):
  [`docs/porting-hazards.md`](docs/porting-hazards.md). Failure modes
  that have already cost this project time, indexed by trigger, with the
  commits that prove the cost. The checks above are its summary; the file
  carries the evidence.
- **Upstream sync protocol**: before moving the
  `reference/starlink-ast` pin, diff the incoming range for
  serialisation changes and grep `docs/issues/c-library-quirks.md` for
  entries the port *mirrors* rather than merely records — a quirk fixed
  upstream turns a faithful mirror into a bug. Regenerate all affected
  corpora in one commit range. `PLAN.md` §8 tracks the pending sync.
  After moving the pin, run `tools/oracle_fixtures/check_corpus_fresh.sh`.
  Card-only drift means references predate the new pin and must be regenerated
  from the oracle; numeric drift means they were captured on another platform
  and must be left alone. A case reported as both is stale in its cards and
  platform-drifted in its values: take the cards from a fresh capture and keep
  the values, which is what `tools/oracle_fixtures/merge_card_hunks.py` does.
  Regenerate in the same commit range as the pin move.
- **Design spec** (authoritative for API shape and the reasoning behind it):
  [`docs/superpowers/specs/2026-04-20-rust-port-design.md`](docs/superpowers/specs/2026-04-20-rust-port-design.md).
  This is *the* overarching port design — the one document that fixes the
  object model, ownership rules, error model, serialisation architecture and
  FFI strategy. Everything else under `docs/superpowers/specs/` is a per-step
  or per-class design that sits underneath it. Read it before any non-trivial
  change to a public trait, an attribute surface, or the FFI boundary, and
  read its revision history first: rev 6 marks the trait sketches as
  illustrative and records where the implementation diverged.
  When the spec and this file disagree, the spec wins on *what the API should
  be*; this file wins on *how to work* (the checks in "Before porting a class"
  and "Stop signals"). If the spec turns out to be wrong about the code, amend
  the spec in the same change — it has a revision history for exactly that, and
  a spec nobody trusts is worse than no spec.
- **Conformance status**: the most recent check of the code against the spec is
  [`docs/reports/2026-08-19-spec-conformance-review.md`](docs/reports/2026-08-19-spec-conformance-review.md),
  with a companion progress report beside it. Consult it before concluding that
  a divergence you have found is new.
  [`docs/reports/2026-09-03-simplify-parity-final.md`](docs/reports/2026-09-03-simplify-parity-final.md)
  is the matching statement for the simplifier: what byte-equality against the C
  oracle covers today, every relaxation transcribed from the executable ledger,
  and the one open design question.
- **Test-migration tracker**: `PLAN.md` — every C test program in
  the upstream `ast_tester/` directory must end up with a green
  Rust counterpart by step 11 of the delivery order.
- **C reference for porting**:
  `reference/starlink-ast/src/<class>.c`. Read it to understand
  what the Rust class needs to do; do not copy its structure — the
  Rust shape diverges from the C OO system by design.
- **C-oracle probe recipe**: `docs/issues/c-oracle-probe-recipe.md`.
  When the C source alone doesn't settle a "what does C do here?"
  question, build a tiny C program linked against the repo's own
  `libast` and call the same functions. Records the stale-conda-libast
  trap and how to call protected APIs (`astGetBounded_` etc.) via
  `astMakePointer`.
- **Vendored C provenance**: `vendor/README.md` records where each
  vendored directory was copied from and what AST-specific local
  modifications were applied.
- **AST documentation**:
  - SUN/210 (user guide): https://www.starlink.ac.uk/docs/sun210.htx/sun210.html
  - SUN/211 (class reference): https://www.starlink.ac.uk/docs/sun211.htx/sun211.html

## Workflow notes

- This is a port-in-progress. At any given time, most of `ast_tester/`
  is **not yet portable** — its dependencies haven't landed in Rust.
  Don't try to land tests whose dependencies aren't built yet.
  Each delivery step in `PLAN.md` lists the tests it newly enables.
- When in doubt about API shape, the design spec wins. When the spec
  is silent or contradictory, ask before guessing. The design has
  been through six revisions — five of stakeholder feedback, then one
  reconciliation against the implementation; resist the urge to
  redesign on the fly.
- Prefer small, focused PRs that land one class or one batch of tests
  at a time. The implementation plan (forthcoming) will decompose
  the delivery order into PR-sized chunks.
