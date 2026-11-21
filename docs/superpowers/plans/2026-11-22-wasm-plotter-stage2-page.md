# Browser Plotter Stage 2: Binding, Page and Build — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One self-contained HTML file that reproduces the AST FITS plotter CGI page in the browser: a FITS header and settings in, the annotated grid (SVG), the header rewritten as FITS-WCS and the FrameSet as Native text out, computed by `starlink-ast` compiled to WebAssembly, with the three outputs checked against the C library byte for byte.

**Architecture:** A new unpublished crate `ast-plotter-wasm` holds `plot()`, plain Rust that follows `fplottest.f` step for step on `starlink-ast` (attribute strings from stage 1, `FitsChan`, `Plot`, `SvgGrf`), tested natively against a C probe that follows the same program; a thin `wasm-bindgen` wrapper, compiled only for `wasm32`, exposes it to JavaScript. `SvgGrf` gains XML escaping and two opt-in settings the page turns on. A shell script builds the wasm, runs `wasm-bindgen --target no-modules`, and writes one HTML file with the page, the glue and the base64 wasm inline.

**Tech Stack:** Rust 2024 (`starlink-ast` with `attribute-strings`), `wasm-bindgen` (pinned with `=`), `wasm-bindgen-cli` of the same version, plain HTML/CSS/JavaScript, a C probe against the reference `libast` with `grf_log.c`, Node for one smoke test.

**Spec:** [`docs/superpowers/specs/2026-11-21-wasm-fits-plotter-design.md`](../specs/2026-11-21-wasm-fits-plotter-design.md), stage 2. The program the page follows is `fplottest.f`, fetched 2026-11-22 from <https://starlink.eao.hawaii.edu/cgi-bin/ast/file?file=fplottest.f>; Task 2 records its steps in the spec.

## Global Constraints

- Branch `u/timj/wasm-plotter-2-page`, stacked on `u/timj/wasm-plotter-1-attrs`; one PR.
- Spec anti-goal: "A page that needs a server, a network connection, or anything installed in the browser." The output is one HTML file that works from `file://`.
- Spec anti-goal: "A page that goes blank or freezes on bad input: every error is shown, in AST's words."
- Spec: `SvgGrf`'s two new settings are "each off by default so that `SvgGrf` still reproduces `grf_log.c`"; the page turns both on.
- Spec: the binding is "narrow and unpublished" (`publish = false`); it does not start issue #12's API.
- Spec: "The binding's outputs for the same inputs must match byte for byte; a case that cannot goes in a named exclusion list with its reason and a companion test that fails when it starts matching."
- Spec: "CI builds the binding for `wasm32-unknown-unknown` and runs its native tests; the page itself is checked by eye." No browser automation.
- Spec: the build "uses rustup's `cargo` explicitly, since a Homebrew `rustc` earlier on `PATH` hides rustup's targets" (`$(dirname $(rustup which cargo))`).
- User, 2026-11-21: the port handles Unicode where C is byte-oriented; labels and titles with `µ`, `μ`, `Å` and accents pass to the backend unchanged.
- Every C oddity found goes in `docs/issues/c-library-quirks.md` in the same commit.
- Commit messages end with the `Co-Authored-By` line only. Strip conda for cargo and probe builds: `env -u CC -u LDFLAGS -u CFLAGS`.
- `wasm-bindgen-cli` is not installed on this machine; Task 4 installs the version matching the pinned crate with `cargo install wasm-bindgen-cli --version <v> --locked`.

## Decisions

- **Names:** crate `crates/ast-plotter-wasm` (lib name `ast_plotter`); page sources `web/plotter/` (`index.html`, `plotter.css`, `plotter.js`, `examples.txt`); build script `tools/build_plotter.sh`; output `target/plotter/fits-plotter.html`.
- **One source for the examples:** `web/plotter/examples.txt`, read by the C probe, the Rust tests and the build script (which embeds it as a JSON array). Format, one block per example: a line `== NAME`, a line `-- settings: <Plot settings>`, a line `-- version: <character or empty>`, then the header lines verbatim, the block ending at the next `==` or end of file. The eight blocks are the spec appendix's, verbatim.
- **Page defaults:** width and height 600 pixels; redraw 400 ms after the last edit; drawing on the main thread (the grids of the eight examples draw well under a second natively; a worker is the later fix if a header proves slow).
- **Errors** are the `AstError` message and its context lines, one per line, as C reports its message stack; the probe records C's stack the same way (`astPutErr`, as `attr_strings_probe.c` does).
- **A wasm panic** cannot unwind on `wasm32-unknown-unknown`: it traps. `plotter.js` catches the trap, shows "Internal error: the plotter stopped on this input; this is a bug" with the trap message, and instantiates the module afresh for the next redraw.

## Review Focus

1. A header with no WCS at all (`SIMPLE = T` and nothing else): FITS and AST outputs show C's message, and the grid output shows the same, not a blank panel (Task 3, `a_header_with_no_wcs_reports_no_object`).
2. Settings or text with `<`, `&` or `"` (`Title=a<b & c`): the SVG stays well formed and shows the text (Task 1, `text_is_escaped_for_xml`; Task 3, `markup_in_a_title_is_escaped`).
3. A version character that names no Frame (`Z`): C's message for the grid, the other two outputs still shown (Task 3, from the probe's `bad_version` case).
4. Width or height of 0, negative, or 100000: an error message, not a trap or a giant allocation (Task 3, `absurd_sizes_are_errors`; limits 16 to 4096).
5. A panic inside the library (a trap): the page reports it and the next redraw works (Task 5, `plotter.js` re-instantiates; checked in the Node smoke test by calling a deliberately trapping export `__trap_for_test`, compiled only with the crate's `smoke-test` feature).

---

### Task 1: SvgGrf escaping and the page's two settings

**Files:**
- Modify: `crates/starlink-ast/src/grf/svg.rs`, `crates/starlink-ast/tests/test_grf.rs`, `CHANGELOG.md`, `docs/issues/c-library-quirks.md`

**Interfaces:**
- Produces: `SvgOptions { width, height, erase_under_text: bool, equal_aspect: bool }` (both new fields default `false`; `SvgOptions` stays `#[non_exhaustive]`).

- [ ] **Step 1: Write the failing tests** in `tests/test_grf.rs`:
  - `text_is_escaped_for_xml`: `text("a<b & \"c\"", 0.5, 0.5, CC, [0,1])` writes `a&lt;b &amp; &quot;c&quot;` and the SVG parses as XML (check no raw `<b`).
  - `erase_under_text_fills_the_text_box`: with `erase_under_text`, a text call writes, before its `<text`, a `<polygon fill="black" points="…"/>` whose four corners are the `text_extent` box of the same call in SVG pixels; without it, no polygon (the existing C comparison already pins that).
  - `equal_aspect_draws_on_the_centred_square`: 800×600 with `equal_aspect`: `line(&[0.0,1.0],&[0.0,1.0])` gives `points="100,600 700,0 "`; text at size 1 has `font-size="12"` (0.02 × 600).
- [ ] **Step 2: Run** `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_grf::` — Expected: the three new tests FAIL (unknown fields, unescaped text).
- [ ] **Step 3: Implement** in `svg.rs`: escape `&`, `<`, `>`, `"` in text; `erase_under_text` draws the polygon from `self.page`'s text extent before the text; `equal_aspect` maps x through `side * x + (width - side) / 2` and y likewise with `side = min(width, height)`, and sets the font size to `round(0.02 * side * size)` rather than `round(14 * size)`, and a marker's radius to `2 * 0.02 * side / 14`, rounded, scaled likewise (the spec: "text, and markers, are then drawn at the size the Plot laid them out for"). Defaults unchanged, so `svg_grf_matches_the_c_library` still passes. Quirk entry: `grf_log.c` writes text into its SVG without escaping, so `<` or `&` in a label makes invalid SVG; the port escapes. CHANGELOG `Added`: the two options; `Fixed`: escaping.
- [ ] **Step 4: Run** the Step 2 command — Expected: PASS, including `svg_grf_matches_the_c_library`.
- [ ] **Step 5: Commit** — `git commit -m "Escape SVG text, and add SvgGrf's erase-under-text and equal-aspect settings"`

### Task 2: The C probe of fplottest, and the examples file

**Files:**
- Create: `web/plotter/examples.txt`, `tools/oracle_fixtures/fits_plotter_probe.c`, `crates/ast-plotter-wasm/tests/fixtures/<case>.{svg,fits,ast}` (generated)
- Modify: `docs/superpowers/specs/2026-11-21-wasm-fits-plotter-design.md` ("What `fplottest.f` does")

**Interfaces:**
- Produces: for each case `<case>`, three files; each holds either the output text or, if that output failed, a first line `ERROR` followed by C's messages one per line. The cases are the eight examples (`aitoff` … `hpx`, lower case) and four error cases: `no_wcs` (`SIMPLE = T` only), `bad_version` (AITOFF with version `Z`), `bad_plot_setting` (AITOFF with `Fred=1`), `bad_fitschan_setting` (AITOFF with FitsChan settings `Encoding=nonsense`).

- [ ] **Step 1: Write `examples.txt`** from the spec appendix, verbatim, in the format of Decisions.
- [ ] **Step 2: Write the probe**, following `fplottest.f` exactly: `astFitsChan` with the FitsChan settings; each header line through `astPutFits`; `astClear( fc, "Card" )` then `astFindFits( fc, "NAXIS1", card, 1 )` and the value read from column 11 as Fortran's list-directed `READ` does (100 if absent), likewise NAXIS2; `pbox = { 0.5, 0.5, NAXIS1+0.5, NAXIS2+0.5 }`; `astClear Card`, `astRead`; none read → every output is `!!! No object read from file!!!`. FITS output: `astClear Card`, `astSetC Encoding FITS-WCS`, `astWrite` (0 → `Failed to create new FITS headers`), `astClear Card`, every `astFindFits( fc, "%f", card, 1 )` card, one per line, as C returns it. AST output: `astWrite` to a `Channel` whose sink collects lines. Grid: version `$` sets Current to Base; another non-blank character searches Frames 1..Nframe for `Ident` equal to it, else `The supplied FITS headers do not define a coordinate system with the requested version character 'X'.`; gbox `{0.05, 0.05, 0.95, 0.95}` in `float`, then `asp = fabsf( (float)(pbox[3]-pbox[1]) / (float)(pbox[2]-pbox[0]) )`, `asp < 0.2 || asp > 5.0` → `1.0`, and fplottest's two `DELTA` branches, all in `float`; `astPlot( fs, gbox, pbox, "%s", settings )`, `astGrid`, through `grf_log.c` at 600×600 (`astGrfLogSetSvg`). Every AST error goes to the failing output as `ERROR` and the message stack. Build and run as `attr_strings_probe.c`'s header describes; the probe writes the fixtures directory given as its argument.
- [ ] **Step 3: Run the probe**; check by eye that `aitoff.svg` draws a grid and `no_wcs.fits` holds the message.
- [ ] **Step 4: Amend the spec's** "What `fplottest.f` does" step 4: the FitsChan is rewound (`AST_CLEAR( FC, 'CARD' )`), not emptied, so the normalised header is the new FITS-WCS cards written at the start of the cards the read left behind; add the error texts and the gbox arithmetic above.
- [ ] **Step 5: Commit** — `git commit -m "Probe the FITS plotter program against the C library"`

### Task 3: The binding crate, natively

**Files:**
- Create: `crates/ast-plotter-wasm/Cargo.toml` (`publish = false`; `starlink-ast` with `attribute-strings`; feature `smoke-test`), `src/lib.rs`, `src/examples.rs`, `tests/test_plotter.rs`
- Modify: `Cargo.toml` (workspace member), `PLAN.md` (a row for the plotter)

**Interfaces:**
- Produces, in `ast_plotter`:
  - `pub struct PlotRequest { pub header: String, pub plot_settings: String, pub fitschan_settings: String, pub width: u32, pub height: u32, pub version: char, pub page_look: bool }` — `version` is `' '` for primary axes; `page_look` turns on both `SvgGrf` settings.
  - `pub struct PlotResult { pub svg: Result<String, String>, pub fits: Result<String, String>, pub ast: Result<String, String> }` — `Err` holds the message lines joined with `\n`.
  - `pub fn plot(request: &PlotRequest) -> PlotResult`.
  - `pub struct Example { pub name: String, pub settings: String, pub version: char, pub header: String }`, `pub fn parse_examples(text: &str) -> Vec<Example>`.

- [ ] **Step 1: Write the failing tests** in `tests/test_plotter.rs`:
  - `outputs_match_the_c_library`: for every fixture case, `plot()` with the case's inputs at 600×600, `page_look: false`, compares each output with the fixture, `ERROR` files against `Err` lines; a named exclusion list (`PLOTTER_EXCLUDED`, empty to start) with `exclusions_still_differ`.
  - `a_header_with_no_wcs_reports_no_object`, `markup_in_a_title_is_escaped` (`Title=a<b` on AITOFF: SVG contains `a&lt;b`), `absurd_sizes_are_errors` (0, 15, 4097 → `Err` for the grid, other outputs unaffected), `the_examples_file_has_the_eight_examples` (names in the appendix's order).
- [ ] **Step 2: Run** `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plotter-wasm` — Expected: FAIL to compile (no crate items).
- [ ] **Step 3: Implement `plot`** following the probe's steps on the Rust API (`FitsChan::new`, `set_attributes`, `put_fits`, `clear_card`, `find_fits`, `Channel::read`/`write`, the cards listed with `find_fits("%f", true)` from the first as the probe lists them, `NativeChannel::with_string_sink`, `Plot::new`, `set_attributes`, `Draw::draw_grid`, `SvgGrf`); the gbox arithmetic in `f32` exactly as the probe. A case that cannot match goes in `PLOTTER_EXCLUDED` with its reason, counted in `test_verification_ledger.rs` and `PLAN.md` §6.
- [ ] **Step 4: Run** the Step 2 command and `cargo clippy -p ast-plotter-wasm --all-targets -- -D warnings` — Expected: PASS.
- [ ] **Step 5: Commit** — `git commit -m "Add ast-plotter-wasm: the FITS plotter's work in plain Rust, checked against C"`

### Task 4: The wasm wrapper

**Files:**
- Modify: `crates/ast-plotter-wasm/Cargo.toml` (`[target.'cfg(target_arch = "wasm32")'.dependencies] wasm-bindgen = "=<newest 0.2.x>"`, `crate-type = ["cdylib", "rlib"]`), `src/lib.rs` (`mod wasm;` under `cfg(target_arch = "wasm32")`)
- Create: `crates/ast-plotter-wasm/src/wasm.rs`

**Interfaces:**
- Consumes: Task 3's `plot`, `PlotRequest`, `PlotResult`.
- Produces, to JavaScript: `plot(header, plot_settings, fitschan_settings, width, height, version, page_look) -> PlotOutput` with getters `svg()`, `svg_error()`, `fits()`, `fits_error()`, `ast()`, `ast_error()` (each `Option<String>`, the unused one `undefined`); with feature `smoke-test`, `__trap_for_test()`, which panics.

- [ ] **Step 1: Install the CLI** matching the pinned crate: `cargo install wasm-bindgen-cli --version <v> --locked` with rustup's cargo.
- [ ] **Step 2: Write `wasm.rs`** with the exports above, each a thin conversion to and from Task 3's types.
- [ ] **Step 3: Build** `PATH=$(dirname $(rustup which cargo)):$PATH cargo build -p ast-plotter-wasm --target wasm32-unknown-unknown --release` and the native `cargo test -p ast-plotter-wasm` — Expected: both succeed.
- [ ] **Step 4: Commit** — `git commit -m "Expose the FITS plotter to JavaScript through wasm-bindgen"`

### Task 5: The page, the build script and the smoke test

**Files:**
- Create: `web/plotter/index.html`, `web/plotter/plotter.css`, `web/plotter/plotter.js`, `tools/build_plotter.sh`, `tools/plotter_smoke.mjs`, `web/plotter/README.md`

**Interfaces:**
- Consumes: Task 4's exports; `examples.txt`.
- Produces: `target/plotter/fits-plotter.html`.

- [ ] **Step 1: Write the smoke test** `tools/plotter_smoke.mjs` (Node): loads a `--target nodejs` build of the binding from a path given as its argument, runs `plot` on each example of `examples.txt`, fails if any example's grid output is an error, and checks that after `__trap_for_test` throws, re-instantiating gives a working `plot`.
- [ ] **Step 2: Write `build_plotter.sh`**: builds release for `wasm32-unknown-unknown` with rustup's cargo (and, with `--smoke`, the `smoke-test` feature and `wasm-bindgen --target nodejs` then runs the smoke test); otherwise `wasm-bindgen --target no-modules`, base64-encodes the wasm, and writes `target/plotter/fits-plotter.html` from `index.html` with `plotter.css`, the glue, `plotter.js`, the examples as JSON and the wasm inlined; fails with a message naming the install command if `wasm-bindgen` is missing or its version differs from the crate's.
- [ ] **Step 3: Run** `tools/build_plotter.sh --smoke` — Expected: FAIL (no page sources yet for the HTML step; the smoke step passes once Task 4 is in).
- [ ] **Step 4: Write the page**: layout C of the spec (example menu, header box, Plot settings, FitsChan settings, width, height, version character on the left; the grid on the right; normalised header and FrameSet text side by side below); redraw 400 ms after the last edit; each output panel shows its text or its error; light and dark through `prefers-color-scheme`; `page_look: true`; trap handling as in Decisions. Choosing an example fills header, Plot settings and version, as the CGI page's `loadExample` does.
- [ ] **Step 5: Run** `tools/build_plotter.sh --smoke && tools/build_plotter.sh` — Expected: both succeed; open `target/plotter/fits-plotter.html` from Finder and check by eye that each example draws, that typing redraws, and that `Fred=1` shows C's message.
- [ ] **Step 6: Commit** — `git commit -m "Add the browser FITS plotter page and its build"`

### Task 6: CI and the record

**Files:**
- Modify: `.github/workflows/ci.yml` (a job: `rustup target add wasm32-unknown-unknown`, `cargo build -p ast-plotter-wasm --target wasm32-unknown-unknown --release`, `cargo test -p ast-plotter-wasm`), `PLAN.md` (the plotter row: stage 2 done, hosting and issue #12 left), `docs/superpowers/specs/2026-11-21-wasm-fits-plotter-design.md` (status line)

- [ ] **Step 1: Run** the CI job's commands locally with rustup's cargo — Expected: success.
- [ ] **Step 2: Run** the whole suite: `env -u CC -u LDFLAGS -u CFLAGS cargo test --workspace --exclude ast-ffi-tests --exclude ast-oracle-tests`, `cargo clippy --workspace --all-targets -- -D warnings` — Expected: pass.
- [ ] **Step 3: Commit** — `git commit -m "Build the plotter binding for wasm32 in CI"`
