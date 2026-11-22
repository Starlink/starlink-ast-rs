# Plot Stage 7: The egui Demo — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A native window that lets the user pick one of the grid test cases and edit its attributes, and draws the Plot's grid through a `Grf` written against egui's painter, with egui's own font metrics answering the text queries, redrawing when the window is resized.

**Architecture:** A new unpublished workspace crate, `crates/ast-plot-demo`, beside `ast-plugins` rather than replacing it, since `ast-plugins` is reserved for backends other programs link against and the demo is an application.
It is a library with three egui-free or egui-only modules (the cases and attribute parser, the egui backend, the app) and a thin `main.rs`, so everything but the window is unit-tested headless through `egui::Context::run`.
The cases and FITS headers are embedded with `include_str!`, so the binary runs from anywhere and a later WebAssembly build needs no file access.

**Tech Stack:** Rust 2024, `starlink-ast` (path dependency), `eframe` 0.32 (re-exporting `egui` and `epaint` 0.32).

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 7.

## Global Constraints

- Branch `u/timj/plot-7-egui`, stacked on `u/timj/plot-6-drawing`; one PR.
- Spec: "A window that lets the user pick one of the `cases.txt` headers and its attributes, and draws the grid through a `Grf` written against egui's painter, using egui's real font metrics for `GTxExt`. It redraws on resize."
- Spec: "It runs natively on macOS, Linux and Windows; a browser build via WebAssembly is welcome but not required."
- Spec: "It is a visual example with no automated drawing check; CI only builds it and runs its unit tests, if any."
- `eframe = "0.32"`, chosen as the newest release whose `rust-version` (1.85) met the workspace's then `rust-version = "1.85"`.
  That premise was wrong: `nalgebra` 0.34 already needed 1.87, and egui 0.32's own dependencies need 1.89, so the workspace now declares 1.87 and the demo 1.89.
- No dependency is added to `starlink-ast` for the demo (memory: no example dependencies); the demo is not a default member, so a plain `cargo build` does not build egui.
- The crate is `publish = false`, so it needs no `CHANGELOG.md` entry, no `#[doc(alias)]` and no C-docs ledger row.
- Every new `.rs` file opens with the two-line AURA copyright and SPDX header (`test_source_headers`).
- `[lints] workspace = true`; `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo doc --workspace --no-deps` clean; strip conda (`env -u CC -u LDFLAGS -u CFLAGS`).
- Commit messages end with the `Co-Authored-By` line only (no session line).
- Per-task test: `cargo test -p ast-plot-demo`; at the end of Task 3 also `cargo test -p starlink-ast --no-fail-fast`, since `test_source_headers` and the other repository gates live there.

## Decisions

- **Graphics coordinates are egui points**, x to the right from the drawing rectangle's left edge and y upward from its bottom edge, so `scales` is `[1.0, 1.0]` and a resize changes only the graphics box.
  The Plot's graphics box is fixed at construction, so a resize builds a new Plot.
- **Graphics box:** the drawing rectangle less a 10% margin on each side, shrunk on one axis to keep the plot box's aspect ratio, and centered; an aspect ratio outside 0.05 to 20 counts as 1, as `testgrid.c` does.
- **Capabilities:** only `GrfCap::Scales`, as `LogGrf` answers, so the Plot interprets escape sequences and `M` justification itself.
  `text` and `text_extent` still handle all four vertical justifications.
- **Text size:** `BASE_FONT_SIZE = 12.0` points at text size 1, proportional font; `char_height` is `[r, r]` with `r` egui's row height for that font, the same height `text_extent` gives a box.
- **Styles:** every attribute of every primitive starts at 1.0.
  Color index 0 is the panel background, 1 the theme's text color, 2 to 15 PGPLOT's color table (2 red `#FF0000`, 3 green `#00FF00`, 4 blue `#0000FF`, 5 cyan `#00FFFF`, 6 magenta `#FF00FF`, 7 yellow `#FFFF00`, 8 `#FF8000`, 9 `#80FF00`, 10 `#00FF80`, 11 `#0080FF`, 12 `#8000FF`, 13 `#FF0080`, 14 `#555555`, 15 `#AAAAAA`), anything else the text color; the value is rounded to the nearest integer.
  Width is a multiple of `BASE_LINE_WIDTH = 1.0` points.
  Style 1 is solid, 2 dashed (dash 6, gap 4, times the width), 4 dotted (dash 1, gap 3, times the width), 3 and 5 dashed as 2, anything else solid.
  Marker size is a multiple of `BASE_MARKER_SIZE = 6.0` points; marker 1 a filled dot, 2 a plus, 3 an asterisk, 4 a circle, 5 a cross, 6 a square, anything else a circle.
- **Attribute strings** use C's `astSet` syntax, as `cases.txt` writes them, parsed by the demo's own copy of the names `crates/starlink-ast/tests/common/plot.rs` knows.
  The copy returns errors instead of panicking: an unknown name or element is `ErrorCode::Badat`, a value that does not parse is `ErrorCode::Attin`.
  It is a copy, not shared code: a string-typed setter does not belong on `starlink-ast`'s public API (CLAUDE.md, "What NOT to do"), and the tests cannot depend on an egui crate.
  It is also the first piece of what `starlink-ast-capi` will need for `astSet` on a Plot, but not a port of it: C's `SetAttrib` in `plot.c` decides the accepted spellings, abbreviations and errors, and the shim's version must be ported from there.
  Task 3 records this in `PLAN.md`, so that the shim starts from `plot.c` and the demo and test helper can later share its parser.
- **Caching:** the shapes of the last drawing are kept with the key they were drawn for (case index, applied attribute string, drawing rectangle rounded to whole points, dark mode), and redrawn only when the key changes.
- **Attributes apply on Enter** or when the field loses focus, not on every keystroke, since some grids take tens of milliseconds.
- **Too small:** a drawing rectangle under 100 points on either side draws nothing and says so.
- **No system libraries in CI.** On Linux, winit loads X11 or Wayland and xkbcommon at run time (its default `x11-dlopen` and `wayland-dlopen` features), glutin loads OpenGL the same way, and arboard's X11 clipboard is pure Rust, so building needs no development packages.
  macOS and Windows use their native windowing and need nothing either.
  The apt line in eframe's README predates that; add it to CI only if the Linux build fails.

## Review Focus

1. A minimized or tiny window (zero-size rectangle): no panic, a message instead of a drawing (Task 3, `draw_case_refuses_a_tiny_rectangle`).
2. A typo or bad value in the attribute field: the error is shown, not a panic (Task 1, `unknown_attribute_is_an_error`, `bad_value_is_an_error`; Task 3, `draw_case_reports_a_bad_attribute`).
3. Dark mode: color index 1 must follow the theme, or the grid vanishes on one of them (Task 2, `color_one_is_the_foreground`).
4. Labels on a vertical axis: a rotated text box must enclose the text where it is drawn, or the Plot spaces labels wrongly (Task 2, `centered_text_is_centered_at_any_angle`).
5. A HiDPI display (`pixels_per_point` 2): sizes stay in points, so the drawing looks the same (Task 2, `char_height_is_in_points_at_any_scale`).

---

### Task 1: The crate, the cases and the attribute parser

**Files:**
- Create: `crates/ast-plot-demo/Cargo.toml`, `crates/ast-plot-demo/src/lib.rs`, `crates/ast-plot-demo/src/cases.rs`, `crates/ast-plot-demo/src/settings.rs`
- Modify: `Cargo.lock` (by cargo)

**Interfaces:**
- Produces, in `ast_plot_demo::cases`:
  - `pub struct DemoCase { pub name: &'static str, pub head: &'static str, pub header: &'static str, pub attr: &'static str, pub fattr: &'static str, pub pbox: Option<[f64; 4]> }`, `head` the header's file name and `header` its text.
  - `pub fn cases() -> Vec<DemoCase>`, parsed from the embedded `crates/starlink-ast/tests/fixtures/plot/cases.txt` as `grid_cases` in `tests/common/plot.rs` parses it.
  - `pub fn plot_box(case: &DemoCase) -> [f64; 4]`: `pbox`, or the header's pixel grid as `naxis_box` in `tests/common/plot.rs` computes it.
  - `pub fn graphics_box(width: f32, height: f32, pbox: [f64; 4]) -> [f32; 4]`, per Decisions.
  - `pub fn build_plot(case: &DemoCase, attr: &str, gbox: [f32; 4]) -> AstResult<Plot>`: the header read through a FitsChan with the case's FitsChan settings (only `carlin=1` exists; anything else is `ErrorCode::Badat`), a Plot of the object read, titled "A FITS test", with `attr` applied. A header that reads no object is `ErrorCode::Badin`.
- Produces, in `ast_plot_demo::settings`: `pub fn apply_settings(plot: &mut Plot, settings: &str) -> AstResult<()>`.

- [ ] **Step 1: Create the crate**

`Cargo.toml`: `name = "ast-plot-demo"`, `version = "0.1.0"`, workspace `edition`, `rust-version`, `license`, `publish = false`, a one-line `description`; dependencies `starlink-ast = { path = "../starlink-ast" }` and `eframe = "0.32"`; `[lints] workspace = true`.
`lib.rs` declares `pub mod cases; pub mod settings;` with a crate doc saying what the demo is and how to run it.
The workspace's `members = ["crates/*"]` picks it up; leave `default-members` alone.
Embed the 20 headers as a `const HEADERS: [(&str, &str); 20]` of file name and `include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/<name>")`.

- [ ] **Step 2: Write the failing tests**

In `cases.rs`:

```rust
#[test]
fn every_case_has_its_header() {
    let cases = cases();
    assert_eq!(cases.len(), 20);
    for c in &cases {
        assert!(c.header.contains("NAXIS"), "{}", c.name);
    }
}

#[test]
fn graphics_box_keeps_the_aspect_ratio() {
    assert_eq!(graphics_box(1000.0, 500.0, [0.0, 0.0, 100.0, 100.0]), [300.0, 50.0, 700.0, 450.0]);
    assert_eq!(graphics_box(500.0, 1000.0, [0.0, 0.0, 200.0, 100.0]), [50.0, 300.0, 450.0, 500.0]);
    // An aspect ratio of 100 counts as 1.
    assert_eq!(graphics_box(1000.0, 500.0, [0.0, 0.0, 1.0, 100.0]), [300.0, 50.0, 700.0, 450.0]);
}

#[test]
fn every_case_draws_through_log_grf() {
    for c in cases() {
        let mut plot = build_plot(&c, c.attr, graphics_box(800.0, 600.0, plot_box(&c))).unwrap();
        plot.draw_grid(&mut LogGrf::new(Vec::new())).unwrap();
    }
}
```

In `settings.rs`, on a Plot of `cases()[0]` built with an empty attribute string:

```rust
#[test]
fn settings_reach_the_typed_setters() {
    apply_settings(&mut plot, "grid=1,labelling=int,Colour(axes)=3,width=2").unwrap();
    assert!(plot.grid());
    assert_eq!(plot.labelling(), Labelling::Interior);
    assert_eq!(plot.colour(PlotElement::Axis1), 3);
    assert_eq!(plot.width(PlotElement::Title), 2.0);
}

#[test]
fn unknown_attribute_is_an_error() {
    assert_eq!(apply_settings(&mut plot, "nosuch=1").unwrap_err().code(), ErrorCode::Badat);
    assert_eq!(apply_settings(&mut plot, "colour(nosuch)=2").unwrap_err().code(), ErrorCode::Badat);
}

#[test]
fn bad_value_is_an_error() {
    assert_eq!(apply_settings(&mut plot, "grid=yes").unwrap_err().code(), ErrorCode::Attin);
    assert_eq!(apply_settings(&mut plot, "tol").unwrap_err().code(), ErrorCode::Attin);
}
```

`Colour(axes)` sets both axis elements, as C's group does; use whichever `PlotElement` the group resolves to if `Axis1` is not its name.

- [ ] **Step 3: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo`
Expected: compile errors for the missing functions.

- [ ] **Step 4: Implement `cases.rs` and `settings.rs`**

Port the parsing from `tests/common/plot.rs` (`set_attr`, `set_style_attr`, `apply_settings`, `set_component_system`, `PER_AXIS`), turning each `unwrap` and `panic!` into the errors in Decisions, with messages naming the setting as written, such as `astSet(Plot): unknown attribute "nosuch=1".`.

- [ ] **Step 5: Run the tests**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo`
Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/ast-plot-demo
git commit -m "Add the ast-plot-demo crate: grid cases and attribute settings"
```

### Task 2: The egui backend

**Files:**
- Create: `crates/ast-plot-demo/src/grf.rs`
- Modify: `crates/ast-plot-demo/src/lib.rs` (add `pub mod grf;`)

**Interfaces:**
- Consumes: `cases`, `plot_box`, `graphics_box`, `build_plot` (Task 1).
- Produces, in `ast_plot_demo::grf`:
  - `pub struct EguiGrf`, implementing `starlink_ast::grf::Grf`.
  - `pub fn EguiGrf::new(ctx: &egui::Context, rect: egui::Rect, foreground: egui::Color32, background: egui::Color32) -> Self`: graphics `(0, 0)` is `rect.left_bottom()`.
  - `pub fn EguiGrf::into_shapes(self) -> Vec<egui::Shape>`, in screen coordinates.
  - `pub const BASE_FONT_SIZE: f32 = 12.0`, `BASE_LINE_WIDTH: f32 = 1.0`, `BASE_MARKER_SIZE: f32 = 6.0`.
  - It must be used inside an egui pass (`Context::run` or an app's `update`), where `Context::fonts` has fonts.

- [ ] **Step 1: Write the failing tests**

A helper `fn with_grf<R>(ppp: f32, f: impl FnOnce(&mut EguiGrf) -> R) -> R` runs `f` inside `egui::Context::run` with an 800×600 screen rectangle and `pixels_per_point` `ppp`, on an `EguiGrf` over that rectangle with a white foreground and black background.
`J(v, h)` is shorthand for `Justification::new(v, h)`.

```rust
#[test]
fn attr_returns_the_previous_value() {
    with_grf(1.0, |g| {
        assert_eq!(g.attr(GrfAttr::Colour, Some(3.0), GrfPrimitive::Line).unwrap(), 1.0);
        assert_eq!(g.attr(GrfAttr::Colour, None, GrfPrimitive::Line).unwrap(), 3.0);
        assert_eq!(g.attr(GrfAttr::Colour, None, GrfPrimitive::Text).unwrap(), 1.0);
    });
}

#[test]
fn color_one_is_the_foreground() {
    // One solid line; its stroke is the foreground, white.
}

#[test]
fn empty_text_has_a_zero_box() { /* every corner 0.0 */ }

#[test]
fn zero_up_vector_is_an_error() { /* text_extent("a", .., [0.0, 0.0]) is Grfer */ }

#[test]
fn mismatched_coordinates_are_an_error() { /* line(&[0.0, 1.0], &[0.0]) and mark likewise are Grfer */ }

#[test]
fn centered_text_is_centered_at_any_angle() {
    // For up [0,1], [-1,0], [1,1] and [0,-1], the mean of the four corners
    // of text_extent("Label", 100.0, 200.0, J(Centre, Centre), up) is
    // (100, 200) within 1e-3, and the box's sides are the galley's width
    // and height.
}

#[test]
fn justification_places_the_box() {
    // up [0,1]: J(Baseline, Left) puts corner 0 at x = 100 and below
    // y = 200, and corner 3 above it; J(Top, Right) puts corner 2 at (100, 200).
}

#[test]
fn drawn_text_starts_at_its_box() {
    // After text("Label", 100.0, 200.0, J(Baseline, Left), [-1.0, 0.0]),
    // the one shape is Shape::Text whose pos is corner 3 of the extent in
    // screen coordinates, with angle -FRAC_PI_2.
}

#[test]
fn char_height_is_in_points_at_any_scale() {
    // char_height at ppp 1 and 2 agree within 1 point; at text size 2 it is
    // between 1.8 and 2.2 times that at size 1.
}

#[test]
fn every_case_draws_through_egui_grf() {
    // Each case built with graphics_box(800.0, 600.0, ..) draws its grid
    // without error, and the shapes include at least one Shape::Text.
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo grf::`
Expected: compile errors for `EguiGrf`.

- [ ] **Step 3: Implement `EguiGrf`**

Styles and colors per Decisions; lines through `Shape::line` or `Shape::dashed_line`; text laid out with `Fonts::layout_no_wrap(text, FontId::proportional(BASE_FONT_SIZE * size), Color32::PLACEHOLDER)` and drawn as `TextShape::new(pos, galley, colour).with_angle(angle)`.
The text box, with `w` and `h` the galley's size and `a` its baseline below its top (`rows[0].pos.y + rows[0].row.glyphs[0].pos.y`):

```text
n = up / |up|                  (graphics coordinates, y up)
d = (n.y, -n.x)                the direction the text reads
top above the reference:  Top 0, Centre h/2, Baseline a, Bottom h
left of the reference:    Left 0, Centre w/2, Right w
corner 0 = ref - left*d + (top - h)*n;  corner 1 = corner 0 + w*d
corner 2 = corner 1 + h*n;              corner 3 = corner 0 + h*n
```

`text` places the galley's top left at corner 3, converted to screen coordinates, with angle `atan2(-d.y, d.x)`, egui's clockwise rotation on a y-down screen.

- [ ] **Step 4: Run the tests**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo`
Expected: 16 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-plot-demo
git commit -m "Add EguiGrf, a Grf drawing through egui's painter"
```

### Task 3: The window, CI and the record

**Files:**
- Create: `crates/ast-plot-demo/src/app.rs`, `crates/ast-plot-demo/src/main.rs`, `crates/ast-plot-demo/README.md`
- Modify: `crates/ast-plot-demo/src/lib.rs`, `.github/workflows/ci.yml`, `CLAUDE.md` (repository layout), `PLAN.md` (Plot row), `docs/superpowers/specs/2026-10-30-plot-design.md` (stage 7 done note)

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces, in `ast_plot_demo::app`:
  - `pub fn draw_case(ctx: &egui::Context, rect: egui::Rect, case: &DemoCase, attr: &str, foreground: Color32, background: Color32) -> Result<Vec<egui::Shape>, String>`: `Err("The window is too small to draw in.")` under 100 points on a side, otherwise the grid's shapes, or the error's message.
  - `pub struct DemoApp`, `impl Default`, `impl eframe::App`.

- [ ] **Step 1: Write the failing tests**

In `app.rs`, inside `egui::Context::run`:

```rust
#[test]
fn draw_case_refuses_a_tiny_rectangle() {
    // A 0×0 and a 99×500 rectangle give Err("The window is too small to draw in.").
}

#[test]
fn draw_case_reports_a_bad_attribute() {
    // attr "nosuch=1" gives an Err whose text contains "nosuch".
}

#[test]
fn draw_case_draws_a_grid() {
    // cases()[0] with its own attr in an 800×600 rectangle gives Ok with shapes.
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo app::`
Expected: compile errors for `draw_case`.

- [ ] **Step 3: Implement `draw_case`, `DemoApp` and `main`**

`DemoApp` holds the cases, the selected index, the attribute field's text, the applied attribute string and the cached drawing (Decisions, "Caching").
A top panel holds a `ComboBox` of case names (choosing one sets both strings to the case's `attr`), a single-line `TextEdit` applied per Decisions, and a "Reset" button restoring the case's `attr`; an error from the last drawing is shown under it in the theme's error color.
The central panel draws into its whole available rectangle, foreground `visuals.text_color()`, background `visuals.panel_fill`.
`main` calls `eframe::run_native("AST Plot demo", ..)` with an initial inner size of 900×700.

- [ ] **Step 4: Run the tests and the window**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo`
Expected: 19 passed.
Run: `env -u CC -u LDFLAGS -u CFLAGS cargo run -p ast-plot-demo --release` in the background for 10 seconds, then stop it.
Expected: no panic in its output. Ask the user to look at the window, pick a few cases and resize it; the drawing itself has no automated check (spec).

- [ ] **Step 5: CI**

In `build-and-test`, after the benchmark harness tests, a step `Demo tests` running `cargo test -p ast-plot-demo` (no system packages; Decisions, "No system libraries in CI").
`cargo build --workspace`, clippy and docs already cover the crate.

- [ ] **Step 6: The record**

`README.md`: what the demo shows, `cargo run -p ast-plot-demo --release`, that on Linux it needs an X11 or Wayland desktop at run time, and that it is not built by a plain `cargo build`.
`CLAUDE.md` repository layout: one line for `ast-plot-demo/`.
`PLAN.md`: a row beside the `starlink-ast-capi` rows for Plot's `astSet` string parsing, saying that `crates/ast-plot-demo/src/settings.rs` and `crates/starlink-ast/tests/common/plot.rs` hold provisional parsers of Plot attribute strings, not ported from `plot.c`'s `SetAttrib`, that the shim must port its own from `SetAttrib`, and that both copies should then call it; exposed by the first C caller of `astSet` on a Plot.
`PLAN.md` Plot row and the spec's stage 7: done, naming the crate.

- [ ] **Step 7: Full check and commit**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc --workspace --no-deps`.
Expected: all pass.

```bash
git add crates/ast-plot-demo .github/workflows/ci.yml CLAUDE.md PLAN.md docs/superpowers/specs/2026-10-30-plot-design.md
git commit -m "Add the ast-plot-demo window"
```
