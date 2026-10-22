# Transform Oracle Checker (Rust) — Design

Date: 2026-07-15
Status: Approved design, ready for implementation planning

## Motivation

Commit `3a3c3e51` of the `reference/starlink-ast` submodule (PR #63,
"Add some reference transformations so we can detect drift") added a
*transform output oracle* to the C library: three git-tracked text files
that pin the numerical output of AST's transforms over a fixed input
corpus, plus a checker (`check_transform_oracle.c`) and a generator
(`gen_transform_oracle.c`).

The C design document for that work is
`reference/starlink-ast` `docs/superpowers/specs/2026-06-27-transform-oracle-design.md`
(read via `git show 3a3c3e51:...`); it is the authoritative description of
the oracle *format* and *checks*.
This spec covers the Rust side: porting the **checker** so we can measure
whether the Rust port's transforms agree with the committed C reference
values, and use that measurement to drive a methodical fix plan.

The oracle is deliberately tolerance-based, not bit-exact.
`libmvec`/SIMD kernels, FMA contraction, and cross-architecture `libm`
differences move transcendental results by a few ULP, and the committed
golden values may have been generated on a different architecture (they
were produced on arm64 macOS).
The C golden tolerance is therefore `rtol = atol = 1e-7`, wide enough to
absorb that spread while still catching real algorithmic change (orders of
magnitude larger, `>= ~1e-6`).

## Goals

- Consume the three committed C `.oracle` files, unchanged, as the frozen
  cross-implementation baseline.
- Reload each named fixture through the **public Rust API**, re-transform
  the recorded inputs, and compare outputs within the C tolerances.
- Run in **investigative mode**: complete over the whole corpus and emit a
  machine-readable divergence report grouped by class, check type, and
  magnitude, so we can quantify "how bad the situation is" and plan fixes.
- Stay out of the normal `cargo test` / CI run while the port is in
  progress (the tool reports; it does not gate).
- Be trivially convertible to a hard CI gate later (assert no failures,
  modulo a shrinking exclusion list) with no structural rewrite.

## Non-goals

- Porting the **generator** (`gen_transform_oracle.c`).
  The C generator remains the single source of truth for the golden
  values; Rust never regenerates the oracle files.
  This keeps the C reference a genuine cross-implementation baseline and
  removes any risk of the two implementations drifting on input sampling.
- Bit-for-bit reproducibility of transform output.
- Performance measurement (that is `ast-oracle-bench`'s job).
- Testing simplification *structure* (the existing `test_simplify_regression`
  already compares simplified dumps against `.simp` fixtures; this checker
  tests the *numerical output* of the transforms).
- Turning the checker into a CI gate in this work. That is deliberate
  follow-up once the divergence set is known and being driven down.

## What we consume, unchanged

Vendor these files from submodule `3a3c3e51` into
`crates/ast-core/tests/fixtures/oracle/`:

| File | Sections | Fixtures | Corpus |
| --- | --- | --- | --- |
| `simplify_fixtures.oracle` | 1327 | 676 | native `.map` / `.simp` |
| `headers.oracle` | 259 | 130 | FITS `.head` |
| `framesets.oracle` | 20 | 11 | native `.ast` FrameSets |
| `transform_oracle_overrides.txt` | — | — | per-fixture exceptions |

The oracle format (from the C design doc):

- Plain text, one section per corpus entry.
- Section header: `[<relpath>  nin=N nout=M dir=forward|inverse]`.
- Each data row is `nin` input coordinates followed by `nout` recorded
  output coordinates, at full `%.17g` precision.
- `AST__BAD` is written as the token `BAD`.
- File header comment records the tolerances (informational).

The fixture files the oracle names are already present in the Rust tree
(`crates/ast-core/tests/fixtures/`): all 676 `.map`/`.simp` stems, 129 of
130 `.head`, and 8 of 11 `.ast`.

### Missing fixtures

Four oracle-referenced fixtures are not yet vendored into the Rust tree:
`offset-sref.head`, `plot3d-test1.ast`, `sparse.ast`,
`stcschan-test1-doc3.ast`.
All four use only classes that are already ported (verified from their
`Begin <Class>` tokens), so they load through the Rust API.

Policy: **vendor all four** into `crates/ast-core/tests/fixtures/` so
oracle coverage is 100%, consistent with the project rule that fixtures
must be committed and work with the submodule absent.

General rule for vendoring any fixture: check whether it also belongs in a
test manifest (`crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt`,
`.../native/reference/simplify_tests.txt`) and add it there so the general
tests exercise it too.
For these specific four: verified that none appears in any C test manifest
(`simplify_tests.txt`, `wcsconv_tests.txt`, `grid_tests.txt`, `lsst_wcs.txt`,
`tanSipWcs.txt`) at `3a3c3e51` — they are referenced only by the oracle
files — so no manifest entry applies to them.
(The `pc-sparse` entry already in `wcsconv_tests.txt` is a different
fixture, not `sparse.ast`.)

## Architecture

One `#[ignore]` integration test:
`crates/ast-core/tests/test_transform_oracle.rs`.

- Excluded from the normal `cargo test -p ast-core` run and from CI by the
  `#[ignore]` attribute.
- Run on demand:
  `cargo test -p ast-core --test test_transform_oracle -- --ignored --nocapture`.
- Prints a human summary to stdout and writes a machine-readable JSON
  report to `target/transform_oracle_report.json`.

Rationale: the project convention (CLAUDE.md) is that regression harnesses
live under `crates/ast-core/tests/` and drive the same fixture set as the C
harness, using the public Rust API only.
`#[ignore]` gives the "not part of the normal suite" property the
investigative phase needs, and flipping it to a gate later is a localized
change (drop `#[ignore]`, assert on the findings) with no rewrite.

The test uses the public API exclusively:

- `FitsChan::from_header(text)?.read()?` for `.head`.
- `from_native_string(text)?` → `AnyObject` for `.map` / `.simp` / `.ast`.
- `AnyObject::as_mapping()` / `as_frame()`; `FrameSet` implements `Mapping`
  (base → current) and exposes `frame(FrameRef::Base|Current)`.
- `Mapping::{transform, nin, nout, has_inverse}`.
- `PointSet::{from_rows, point, npoint, ncoord}`.
- `Frame::norm` for angular-axis detection.
- `BAD_VALUE` from `ast_core::constants`.

## Checker logic

Mirrors `check_transform_oracle.c` and `transform_oracle_util.c`.

### Oracle parsing

Parse a `.oracle` file into an ordered `Vec<Section>`:

```rust
struct Section {
    relpath: String,
    nin: usize,
    nout: usize,
    forward: bool,          // dir=inverse -> false
    inputs:  Vec<Vec<f64>>, // [nin][npoint]  (column-major, as C)
    outputs: Vec<Vec<f64>>, // [nout][npoint]
}
```

- `#` and blank lines skipped.
- Header parsed for `relpath`, `nin`, `nout`, `dir`.
- Token `BAD` → `BAD_VALUE`; otherwise parse as `f64`, requiring the whole
  token to consume (reject partial parses, as C's `strtod` end-pointer
  check does).
- A malformed data row (unparseable token or wrong column count) is a
  **hard error** that fails the run — a corrupt/truncated oracle must not
  pass on reduced coverage. This matches C.

### Fixture loading

Dispatch by extension (mirrors `oracle_load_mapping`):

- `.head`: build a `FitsChan` from the header text, `read()`. If the result
  is a `FrameSet`, the mapping under test is `mapping(Base, Current)`;
  retain the base and current frames for angular detection.
- `.map` / `.simp` / `.ast`: `from_native_string`. If the object is a
  `FrameSet`, use it directly as the Mapping (it transforms base → current)
  and retain its base/current frames. Else if `as_mapping()` is `Some`, use
  that (no frames — a bare coordinate mapping). Non-mappings (KeyMap, STC
  objects that are not Regions) are skipped by the loader.
- Load failure → a `LoadFail` finding for the section (recorded, not a
  panic).

### Transform

Build a `PointSet` from the section's inputs and call
`mapping.transform(pts, section.forward)`.
An arity mismatch — `mapping.nin()`/`nout()` disagreeing with the section's
declared shape for the direction — is an `Arity` finding (a structural
change to the fixture), not a comparison.

### The four checks

Reproduce the C checks and pairing exactly.

| Check | Asserts | Tolerance (C constants) |
| --- | --- | --- |
| Golden forward | live forward output == recorded output | `rtol = atol = 1e-7` |
| Golden inverse | live inverse output == recorded output | `rtol = atol = 1e-7` |
| Equivalence (C) | live `.map` fwd == live `.simp` fwd, **gated** on the *recorded* `.map`/`.simp` values already agreeing at that point | `rtol = atol = 1e-9` |
| Round-trip | GRID corpora (`.head`, `.ast`) only: `inverse(forward(P)) ~= P` | absolute `0.05` px (`rtol = 1e-6`) |

Section pairing reproduces the C generator's emission order:

- A one-slot `prev_fwd` cache pairs an inverse section with the
  immediately-preceding forward section of the same fixture (round-trip).
- A one-slot `prev_map` + `prev_map_live` cache pairs a `.simp` forward
  section with the preceding `.map` forward section of the same stem
  (equivalence). Only `.map` forward sections update the cache; only
  `.simp` forward sections consume it, so intervening inverse sections do
  not disturb it.

Equivalence is **gated on stored agreement**: compare live `.map` vs live
`.simp` only at points where the *recorded* golden values already agree
within the equivalence tolerance. Generation-time agreement marks the
shared valid domain; out-of-domain divergence is excluded while a future
simplification regression at a previously-agreeing point is still caught.

Round-trip is **alias-aware**: a recovered pixel that differs from `P` is
accepted if it forward-maps to the same sky as `P` (handles images wider
than the projection period). Points whose forward output is `BAD` are
skipped. Round-trip comparison is never wrap-aware (pixels are not
periodic). Round-trip applies only to `.head`/`.ast`; bare `.map`/`.simp`
have no known domain.

### Comparison primitives

Ported from `transform_oracle_util.c`:

- `within_tol(got, ref, rtol, atol)`: `BAD` matches only `BAD`; any `NaN`
  never matches; else `|got - ref| <= atol + rtol*|ref|`.
- `within_tol_wrap(...)`: as above, but also accepts a difference of one
  full turn (`2*pi`) — used on angular axes only, for golden and
  equivalence (never round-trip).
- `axis_is_cyclic(frame, axis)`: generic test via `Frame::norm` — set a
  point, shift `axis` by `2*pi`, `norm` both, and treat the axis as cyclic
  iff it returns to the original value (true for a SkyFrame longitude;
  false for GRID/pixel, spectral, and other linear axes). A bare Mapping
  has no frame, so every axis is treated as angular (coordinate mappings
  with no pixel axes to protect) — matching C.

The output frame used for angular detection is the *current* frame for a
forward section and the *base* frame for an inverse section.

### Overrides

Parse `transform_oracle_overrides.txt` exactly as C does, with two
independent per-fixture entry kinds (a fixture may have both):

- Round-trip lines: `<relpath> off` (disable) or `<relpath> <rtol> <atol>`
  (loosen).
- Golden lines: `<relpath> golden off` / `golden-fwd off` / `golden-inv off`
  (skip the golden comparison for the named direction(s)).

Current entries: `tnx-cheb.head off` (its TNX Chebyshev distortion has no
inverse, so `sky->pixel` is `BAD`) and
`simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map golden-inv off`
(an architecture-unstable singularity where the residual is either exactly
zero → `BAD` or `~1e-16` rounding noise → an arbitrary unit vector).

In investigative mode the overrides are applied so the Rust divergence
counts align with what C treats as legitimately un-checkable, but a
section skipped by an override is **tallied separately** from a genuine
pass, so the report distinguishes "C says don't check this" from "Rust
agrees with the reference."

## Report artifact

Per section, the checker records an outcome:

```rust
enum Outcome {
    Pass,
    LoadFail,
    Arity,
    SkippedByOverride { reason: String },  // golden-off / rtrip-off
    Skipped { reason: String },            // non-mapping, unsupported dump
    Mismatch(Vec<AxisMiss>),               // one per failing (row, axis)
}
struct AxisMiss { check: CheckKind, row: usize, axis: usize,
                  reference: f64, got: f64, rel: f64 }
```

Outputs:

- **stdout human summary**: headline counts (total sections, passed,
  failed, skipped-by-override, other-skips, load-failed) and the top-N
  worst divergences by relative magnitude, so one run answers "how bad is
  it and where do I aim first."
- **`target/transform_oracle_report.json`**: structured for building a fix
  plan, with:
  - per-section: relpath, direction, worst check, worst relative
    divergence, failing-axis/row counts;
  - aggregations grouped by (a) fixture class mix (from the leading
    `Begin <Class>` tokens in the dump, or the WCS projection type for
    `.head`), (b) check type (golden-fwd / golden-inv / equivalence /
    round-trip), and (c) magnitude buckets: `<=1e-6` near-tolerance,
    `1e-6..1e-3`, `1e-3..1`, `>1` gross, plus `BAD-mismatch` and
    `load-fail`;
  - headline counts.

## Assertion behavior

In this investigative phase the test body does **not** assert on
divergences: it runs to completion over all 1606 sections and always
writes the report. It panics only on a broken harness — a corrupt oracle
file, a fixtures directory that cannot be read, or a report that cannot be
written. `#[ignore]` keeps it out of the normal suite and CI.

Converting to a gate later is a localized change: drop `#[ignore]` and
assert `findings.iter().all(|f| f.is_pass_or_expected())`, where "expected"
is a committed exclusion list of known-divergent sections (each with a
reason), mirroring the existing `NATIVE_READ_EXCLUDED` pattern in this
repo. Shrinking that list becomes the porting work.

## Testing the checker itself

Port the C `--selftest` coverage as ordinary (non-ignored) unit tests in
the same file, so the harness's own parsing/comparison logic is verified in
the normal suite:

- Oracle parsing: a small inline oracle string parses to the expected
  sections, including a `BAD` token and forward/inverse direction.
- Overrides parsing/lookup: round-trip lines and golden lines are
  independent for the same fixture; `off` vs loosened tolerances; the
  golden-fwd/golden-inv/both distinction.
- `within_tol` / `within_tol_wrap`: `BAD`-vs-`BAD`, `BAD`-vs-finite, `NaN`,
  the `2*pi` wrap acceptance, and the near-singularity arm64-vs-x86 pairs
  that the C `test_oracle_util.c` pins.

## File-by-file plan

- `crates/ast-core/tests/fixtures/oracle/{simplify_fixtures,headers,framesets}.oracle`
  — vendored, unchanged, from `3a3c3e51`.
- `crates/ast-core/tests/fixtures/oracle/transform_oracle_overrides.txt`
  — vendored, unchanged.
- `crates/ast-core/tests/fixtures/{...}/offset-sref.head`,
  `plot3d-test1.ast`, `sparse.ast`, `stcschan-test1-doc3.ast`
  — vendored into the locations matching their corpus.
- `crates/ast-core/tests/test_transform_oracle.rs`
  — the `#[ignore]` investigative test plus the (non-ignored) selftest
  units; parser, loader, comparison primitives, the four checks, section
  pairing, overrides, and report emission.
- `PLAN.md` — record the new harness in the test-migration tracker.

## Open implementation parameters

- Exact JSON schema field names for the report (settle during
  implementation; the grouping axes above are fixed).
- The `top-N` count for the stdout worst-divergence list (default ~40).
- How fixture "class mix" is derived for the report grouping (leading
  `Begin` tokens vs a richer walk) — start with the leading-token scan.
