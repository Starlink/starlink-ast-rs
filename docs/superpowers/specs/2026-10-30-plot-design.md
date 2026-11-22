# Plot and Grf design

Status: draft for review, 2026-10-30.
Ticket: not yet created; one ticket, one stacked PR per stage, as DM-56839 did.
Parent design: [`2026-04-20-rust-port-design.md`](2026-04-20-rust-port-design.md), delivery step 10 ("Channel / Plot split" and "Plugin system").

## Intent

Port the 2D `Plot` class so the Rust crate can draw annotated coordinate grids, and give it a drawing-backend interface that a third party can implement.
There is no Rust plotting library equivalent to PLplot or PGPLOT, so the crate ships its own backends: a call log and SVG.
A demonstration window shows a plugin author how to write a backend, the way pyast's matplotlib backend does for Python.

`Plot` is 32k lines of C (`src/plot.c`) and is the largest absent class.
Porting it also unblocks the two Region deferrals that wait on it, `RegTrace` and `RegSplit` (PLAN.md §1, RT-TRACE and RS-SPLIT).

## Goals

- A Rust-native `Grf` trait: the interface between `Plot` and a drawing backend.
- Two backends in the crate, as separate types:
  - `LogGrf`, which writes C `grf_log.c`'s call log.
  - `SvgGrf`, which writes C `grf_log.c`'s SVG.
- `Plot` with its attributes, Native save and load, and every 2D drawing method: `grid`, `grid_line`, `curve`, `gen_curve`, `poly_curve`, `text`, `mark`, `border`, `bounding_box`, `clip`, `region_outline`.
- `RegTrace` and `RegSplit` on Regions, which `region_outline` needs.
- An egui demonstration window that draws a grid and axes through its own out-of-crate `Grf`.

## Non-goals

- `Plot3D` and `Grf3d`.
  Deferred to a later ticket, which starts by adding a 3D logging backend to the C library so there is an oracle to compare against; today C's only 3D test runs through PLplot.
- The C-ABI side of graphics: `astGrfSet`, `astGrfPush`/`astGrfPop`, the `Grf` attribute's switch between registered and link-time functions, the `grfcon` context, and link-time `astG*` symbols.
  These belong to a `CGrf` adapter in `starlink-ast-capi`, which is unstarted (PLAN.md §7); that ticket builds it.
- PLplot or PGPLOT backends.
- An image viewer.
  The demo draws a grid and axes with no image underneath.

## Anti-goals

- A `Grf` trait shaped around one backend's convenience in a way that a C function-pointer table cannot implement.
- Label placement that differs from C because the trait or a backend changed precision (see "Single precision" below).
- A GUI or font crate becoming a dependency of `starlink-ast`.
- Collapsing Plot's per-class extension points into a central `match` (CLAUDE.md, "Before porting a class"; hazard H1).

## Decisions made with the user

### Two backends, two types

C's `grf_log.c` does two jobs in one file: it logs each call to a FILE, and it optionally writes SVG.
The Rust port splits them into `LogGrf` and `SvgGrf`, each implementing `Grf` independently.
A test that wants both drives the same plot twice.

### SVG output options

`SvgGrf` takes an options struct from the start.
Its first fields are the ones C already has, the page width and height in pixels, defaulting to C's 720 and 540.
Everything else C hardcodes (black background, the color table, the 14-pixel base font size, the text-width model) stays at C's value in this ticket.
The struct is built so that later options (transparent or white background, colors, real font metrics) are additive: new fields with C's value as the default, no API break.
The text-width model sits behind one function so it can be swapped for real font metrics later.

### A Rust-native trait, wrapped for C later

The trait is idiomatic Rust: slices for coordinate arrays, enums for justification, primitive and attribute kinds, typed attribute values, `AstResult` returns.
The C-ABI layer will implement it with a `CGrf` type that owns the unsafe function pointers.
Nothing in `starlink-ast` knows about C graphics.

### Plot3D later, oracle first

See Non-goals.

## Constraints

### Single precision

C's graphics interface is `float` throughout (`src/grf.h`), and Plot rounds to `float` before every call.
`grf_log.c` computes text bounding boxes in `float`, and Plot uses those boxes to place labels.
The trait therefore uses `f32` for every coordinate, scale and bounding box.
Attribute values are `f64`, as in C's `astGAttr`.

### The trait follows C's granularity

One method per C function in `grf.h`:

| C | Purpose |
|---|---|
| `astGLine` | polyline |
| `astGMark` | markers |
| `astGText` | text at a position, with justification and up-vector |
| `astGTxExt` | bounding box of a string that `GText` would draw |
| `astGQch` | character height in world units, both axes |
| `astGScales` | world-to-physical scale factors |
| `astGAttr` | set and query style, width, size, font, color per primitive |
| `astGCap` | capability query: escapes, `M` justification, scales |
| `astGBBuf` / `astGEBuf` / `astGFlush` | buffering |

`GCap` stays a real method: Plot asks it whether the backend handles escape sequences, `M` justification and scales, and changes what it draws.
`LogGrf` and `SvgGrf` answer as `grf_log.c` does (scales only).

The attribute method keeps C's contract: it returns the previous value and sets a new one unless the new value is bad, so one call both queries and sets.
Plot saves and restores backend attributes around each drawing operation exactly as C does, through that method; the trait has no separate save and restore.
A backend holds its own attribute state, as `grf_log.c`'s static arrays do; Plot holds none of it between calls.

### Plot's extension points

Plot3D, which comes later, overrides these Plot methods: `Border`, `BoundingBox`, `Clip`, `Curve`, `GenCurve`, `GetGrfContext`, `GrfPop`, `GrfPush`, `GrfSet`, `Grid`, `GridLine`, `Mark`, `PolyCurve`, `SetTickValues`, `Text` (from `plot3d.c`'s vtab set-up).
They are Plot's architecture, not ceremony.
The Rust Plot must leave those seams at C's granularity, so that Plot3D can override them later without restructuring Plot.
The Grf-registration members among them (`GrfSet`, `GrfPush`, `GrfPop`, `GetGrfContext`) are C-ABI only and have no `starlink-ast` counterpart.

The seam is a trait holding the remaining overridable drawing methods (`border`, `bounding_box`, `clip`, `curve`, `gen_curve`, `grid`, `grid_line`, `mark`, `poly_curve`, `set_tick_values`, `text`), implemented by `Plot`, and later by a `Plot3D` that wraps a `Plot`.
Plot's internal helpers that C reaches through the vtab call these through the trait, not as inherent calls, so Plot3D's overrides take effect when the generic code runs.
(Claude's call; the user has not reviewed the mechanism.)

### Plot is a FrameSet

In C, Plot is a FrameSet subclass and serializes as one, with its own attributes added.
The Rust Plot implements `Mapping` and `Frame` as FrameSet does, and its Native dump must be byte-exact with C's.
Drawing takes `&mut dyn Grf` per call (parent spec, "Channel / Plot split"); the Plot holds configuration, not the output stream.
`Plot` is `Clone`.
Plot holds its FrameSet by composition and forwards `Mapping` and `Frame` to it, so a Frame attribute set through a Plot goes through FrameSet integrity as it does in C, where Plot inherits FrameSet's attribute handling.
Plot's own attributes (`Edge`, `Labelling`, `Gap` and the rest) are Plot fields and touch no Frame, except where C's Plot code itself forwards one to a Frame; follow C case by case.

### Where the code lives

`LogGrf` and `SvgGrf` live in `starlink-ast`: they have no dependencies, the crate's own tests need them, and a crates.io user gets SVG without extra crates.
This departs from the parent spec, which puts SVG and logging in `ast-plugins`; amend the parent spec in the same change.
The egui demo lives in its own unpublished crate.
The `ast-plugins` stub has no remaining purpose and is removed, or repurposed for the demo; the builder's call.

## Verification

### What C's own test checks

`ast_tester/testgrid.c` reads a FITS header, builds a Plot inside a 0–1 virtual viewport inset to 0.1–0.9 with an aspect correction, applies attributes, calls `astGrid`, and writes SVG through `grf_log.c`.
`ast_tester/fixtures/plot/cases.txt` lists 20 cases with their headers, attributes and box, and `fixtures/plot/expected/*.svg` holds their references.
`cmake/run_grid_test.cmake` compares only the sorted `<text>` lines, so label content and position are checked and line tessellation is not.
`cobe` is smoke-only: a degree symbol lands on a half-pixel boundary, so its position differs between FMA and non-FMA builds.

### The gates for the port

1. **Labels against C's references.**
   A Rust port of `testgrid.c` runs every case in `cases.txt` and compares sorted `<text>` lines with the committed reference SVGs, matching C's own check, including the `cobe` exception.
2. **Whole drawing against a C capture.**
   For each case, a `grf_log` call log captured from the local reference build (`-ffp-contract=off`, which Rust's arithmetic matches) is committed as a fixture, and `LogGrf`'s output must equal it.
   This covers the lines that gate 1 does not.
   No C change is needed: `testgrid` writes the call log when its fourth argument names a file not ending in `.svg`, and the reference CMake build already builds `testgrid` and `grf_log.c`.
   A capture script under `tools/oracle_fixtures/` runs it over `cases.txt`.
   `cobe` is expected to match here, since both sides avoid fused multiply-add.
   A case that cannot match goes in a named exclusion list with a reason and a companion test that fails when it starts matching; the list starts empty.
   Where a case's log differs only by platform drift, follow the existing corpus rules (CLAUDE.md, "Upstream sync protocol") rather than loosening the comparison.
3. **Backends alone.**
   A C probe drives `grf_log.c` with a fixed sequence of calls covering every function and attribute, and `LogGrf` and `SvgGrf` must reproduce its log and SVG byte for byte.
4. **Native round trip.**
   Plots dumped by C with assorted attributes set must parse and re-dump byte-exactly.
5. **Methods C does not test.**
   `curve`, `gen_curve`, `poly_curve`, `text`, `mark`, `border`, `bounding_box`, `clip` and `region_outline` have no C fixtures.
   Capture new ones from C through `grf_log` (call log and SVG), commit them, and offer them upstream as port-only fixtures.
   `ast_tester/test_bbox_values.c` is the one existing test of `bounding_box`.
   Minimum coverage: every method at least once on a sky and a non-sky Plot; `region_outline` once per Region class that overrides `RegTrace`, plus a disjoint CmpRegion for `RegSplit`; `clip` with each `ClipOp`.
   The builder adds cases beyond that where the C code branches.
6. **Frame methods and `RegTrace`/`RegSplit` directly.**
   Each stage-1 Frame method and each `RegTrace`/`RegSplit` override is compared with C through a probe built on the reference library (`docs/issues/c-oracle-probe-recipe.md`, which also covers calling protected functions such as `astRegTrace`), before any drawing depends on it.

No skip modes, and every relaxation named per case (CLAUDE.md, "Comparators and skip modes").

## Stages

Each stage is one PR on a branch stacked on the previous one.

1. **Frame methods Plot needs.**
   `Gap`, `Centre`, `Abbrev`, `Fields`, `FrameGrid`, the `LineDef`/`LineContains`/`LineCrossing`/`LineOffset` family with the SkyFrame Cartesian side-channel and line ownership (SK-18), `NormalPoints`, and the SkyAxis, TimeFrame and other per-axis overrides of `Gap`/`Abbrev`/`Fields`.
   Sources: `docs/issues/frame-mapping-c-port-gaps.md` (the "Plot-support methods" entry and the SkyAxis and TimeFrame remainders) and `docs/audits/frame-family-c-parity.md` (SK-18).
   Gate 6.
   This stage is a hard prerequisite, since `grid` places labels with `Gap`, `Fields` and `Abbrev`.
2. **`Grf`, `LogGrf`, `SvgGrf`.**
   Gate 3.
3. **Plot as an object.**
   Construction from a FrameSet with a graphics box and a plot box, every attribute, Native save and load.
   Gate 4.
4. **`grid`.**
   Gates 1 and 2.
5. **`RegTrace` and `RegSplit`.**
   The base method and its six class overrides, and CmpRegion's `RegSplit` (`docs/audits/region-class-coverage-audit.md`, RT-TRACE and RS-SPLIT).
   Before stage 6, because `region_outline` calls both.
   Gate 6.
6. **The other drawing methods.**
   Gate 5.
7. **egui demo.**
   A window that lets the user pick one of the `cases.txt` headers and its attributes, and draws the grid through a `Grf` written against egui's painter, using egui's real font metrics for `GTxExt`.
   It redraws on resize.
   It runs natively on macOS, Linux and Windows; a browser build via WebAssembly is welcome but not required.
   It is a visual example with no automated drawing check; CI only builds it and runs its unit tests, if any.

## Calls made by Claude, not yet reviewed by the user

- `LogGrf` and `SvgGrf` live in `starlink-ast`, not `ast-plugins`.
- The Plot3D seam is a trait implemented by `Plot`.
- Plot holds its FrameSet by composition.
- Gate 2, full call logs captured from the local C build, in addition to C's labels-only check.
- The minimum coverage for gate 5.
- The demo is a visual example only, built by CI but not checked.

## Left to the builder

- Exact Rust names and signatures for the trait and backends, following the crate's conventions.
- How Plot's attributes are grouped into Rust types (per-element colors, widths and sizes are indexed by graphical element in C).
- Module layout under `crates/starlink-ast/src/`.
- Whether the demo replaces `ast-plugins` or sits beside it.
- The PR boundaries within a stage, if a stage proves too large for one PR.
