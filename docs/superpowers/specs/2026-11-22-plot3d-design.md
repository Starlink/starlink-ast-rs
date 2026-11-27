# Plot3D and Grf3d design

Status: draft for review, 2026-11-22.
Ticket: not yet created; one ticket, one stacked PR per stage, as the Plot work did.
Parent designs: [`2026-04-20-rust-port-design.md`](2026-04-20-rust-port-design.md) (delivery step 10) and [`2026-10-30-plot-design.md`](2026-10-30-plot-design.md), which deferred Plot3D to this ticket ("Plot3D later, oracle first").

## Intent

Port the C `Plot3D` class, which draws an annotated coordinate grid around a 3D box, and give it a 3D drawing-backend interface that a third party can implement.

The driver is issue #12.
CARTA (CARTAvis) builds the C library with Emscripten today and wants to switch to the Rust WebAssembly build; its developers asked for Plot3D as well as Plot before they can.
CARTA does not draw 3D grids yet: its `wasm_src/ast_wrapper/grf_debug.cc` stubs out every `astG3D*` function, so Plot3D is for a 3D view it has still to build, most likely rendered with WebGL by CARTA itself.

A human must be able to check by eye that Plot3D draws correctly, independently of call logs and of CARTA.
So the crate also renders 3D grids itself: to SVG, in the egui demo, and in the web plotter page when it is given a 3D FITS header.

## Goals

- A Rust-native `Grf3d` trait: the interface between `Plot3D` and a 3D drawing backend, at the granularity of C's `grf3d.h`.
- `LogGrf3d`, writing the same call log as the C library's 3D logging backend, as the oracle.
- `Plot3D` with its attributes, Native save and load byte for byte with C, and its drawing methods: `grid`, `border`, `text`, `mark`, and `set_tick_values`.
- `set_tick_values` on `Plot`, which the Plot work deferred to this ticket.
- A recorded 3D scene and a camera projector, so that one Plot3D drawing can be rendered through any 2D `Grf` from any viewpoint.
- SVG output of a 3D grid at a fixed viewpoint.
- A 3D mode in the egui demo, with the view rotated by the mouse.
- The projector's SVG checked against the C library's 3D SVG at fixed cameras, element for element, without constraining how the projector is written.
- A 3D mode in the web plotter page (`web/plotter/`, `crates/ast-plotter-wasm`): given a FITS header whose FrameSet is 3D, it draws a Plot3D at a fixed viewpoint.
- Plot3D's attributes through the attribute-string layer (`starlink_ast::attributes`), including `Norm`, `RootCorner`, axis 3 and the `_XY`, `_XZ` and `_YZ` plane suffixes, because the web page and issue #12's JavaScript API set Plot options as strings.

## Non-goals

- The C 3D oracle itself: a C logging `grf3d` backend and a proper C test suite for Plot3D.
  The user writes these on the C side, in `reference/starlink-ast`.
  Stages that compare against C wait for them.
- WebGL rendering of the recorded scene.
  The scene is designed so that WebGL, or CARTA, can consume it later without changes to Plot3D.
- Issue #12's JavaScript API, including a JavaScript-supplied `Grf3d`.
- The C-ABI side: `astPlot3D`, the link-time `astG3D*` symbols, and a `CGrf3d` adapter in `starlink-ast-capi`.
- PLplot, PGPLOT or VTK backends.
- Stroke-font (Hershey) text projected through the camera, as `grf3d_pgplot.c` draws it.
  It can be added later as an option of the projector without changing anything else.

## Anti-goals

- A camera in the `Grf3d` trait, or anything in Plot3D's output that depends on the viewpoint.
  C's `grf3d.h` has no camera; the camera belongs to the backend.
  GAIA's VTK backend (`applications/gaia/gaia3d/generic/Grf3dVtk.C`) adds 3D lines and 3D vector text to a VTK scene and lets VTK own the camera, measuring text in the text's own plane; CARTA with WebGL would work the same way.
  A trait that needed a camera would rule both out.
- Collapsing Plot3D's overrides of Plot's methods into a central `match` (CLAUDE.md, "Before porting a class"; hazard H1).
- A GUI or font crate becoming a dependency of `starlink-ast`.
- Text placement that differs from C because the trait or a backend changed precision: `grf3d.h` is `float` throughout, as `grf.h` is.

## Background: how C builds Plot3D

`plot3d.c` (8.6k lines) is a Plot subclass.
It holds three ordinary 2D Plots, one per face of the 3D box (`plotxy`, `plotxz`, `plotyz`).
On each it registers its own 2D graphics functions (`Plot3DAttr`, `Plot3DCap`, `Plot3DFlush`, `Plot3DLine`, `Plot3DMark`, `Plot3DQch`, `Plot3DScales`, `Plot3DText`, `Plot3DTxExt`; `plot3d.c:5521`), which turn each 2D call on a face into a 3D `astG3D*` call on that face's plane, with the plane carried in the Grf context KeyMap.
Plot attributes set on the Plot3D are forwarded to the three inner Plots by macros (`plot3d.c:632` onward), and a per-axis attribute for axis 1, 2 or 3 is forwarded to the two inner Plots that have that axis, at the right inner axis index.

Its vtab overrides of Plot are `Grid`, `Text`, `SetTickValues`, `PolyCurve`, `Border`, `BoundingBox`, `GetGrfContext`, `GrfPop`, `GrfPush`, `GrfSet`, `GridLine`, `Mark`, `Curve`, `GenCurve` and `Clip` (`plot3d.c:3841`).
Of these, `BoundingBox`, `Clip`, `Curve`, `GenCurve`, `GridLine`, `PolyCurve` and the four Grf-registration methods report an error on a Plot3D (the class prologue, "Functions").
It also overrides `Transform`, FrameSet's `SetCurrent`, `ClearCurrent` and `RemoveFrame`, and the attribute and `Equal` methods.

Its Native dump (`plot3d.c:7452` onward) adds `Norm`, `RootCn`, the axis bookkeeping (`AxPlX1` … `AxInZ2`), `BasePl`, the three inner Plots as nested objects `PlotXY`, `PlotXZ` and `PlotYZ`, and `PixFrm`.

`grf3d.h` declares eight functions: `astG3DAttr`, `astG3DCap`, `astG3DFlush`, `astG3DLine`, `astG3DMark`, `astG3DQch`, `astG3DText` and `astG3DTxExt`.
Text and markers take a reference point, a justification, an up vector and a normal vector defining their plane; `astG3DTxExt` returns the text's 3D bounding box.
The camera is not part of it: `grf3d_pgplot.c` and `grf3d_plplot.c` each keep their own (`PG3DSetCamera`, `PL3DAutoCamera`, `PL3DRotateEye` and the rest).
`grf3d_pgplot.c` draws text as Hershey glyph strokes projected through the camera; `grf3d_plplot.c` (2026 port) projects the reference point and text direction and draws 2D text with `plptex`.

## Approaches considered

1. **A camera inside each visual backend**, as in C's PGPLOT and PLplot backends: an `SvgGrf3d` and an `EguiGrf3d` each projecting as they draw.
   Rejected: two 3D implementations to keep in step, egui cannot skew text so each would draw it differently, and rotating the egui view would mean redrawing the whole grid each frame.
2. **One projector over the existing 2D backends, drawing as Plot3D draws.**
   One 3D implementation, `SvgGrf` and `EguiGrf` reused unchanged.
   But measurement would have to go through the camera, which makes Plot3D's layout depend on the viewpoint, against the anti-goal.
3. **Record once, project many times.** *(Chosen by the user.)*
   Plot3D draws into a recording `Grf3d` that keeps a 3D scene, measuring text in its own plane with no camera, as VTK does.
   A projector then renders that scene through a camera onto any 2D `Grf`.
   One 3D implementation serves SVG, the web page and egui; egui rotates by re-projecting the scene, not redrawing the grid; and WebGL or CARTA can later consume the same scene.
   Cost, shared with the PLplot backend: text on a face seen obliquely is not foreshortened in the projected views, and the projected text's drawn width follows the 2D backend's font rather than the in-plane measurement.

## Design

### `Grf3d`

A trait in `starlink_ast::grf`, beside `Grf`, with one method per `grf3d.h` function, following the decisions the Plot design made for `Grf` ("A Rust-native trait, wrapped for C later", "Single precision", "The trait follows C's granularity"):
`f32` for every coordinate, vector and box; attribute values `f64`; the attribute method keeps C's query-and-set contract; slices for coordinate arrays; the existing justification, attribute, primitive and capability enums reused; `AstResult` returns; `cap` a real method.

### `LogGrf3d`

Writes the call log of the C library's 3D logging backend, `grf3d_log.c`, byte for byte.
Its format is fixed by the user's C implementation, planned in `reference/starlink-ast/docs/superpowers/plans/2026-11-22-grf3d-log.md`, not by this design.
`grf3d_log.c` also writes optional SVG through a camera ported from `grf3d_plplot.c`; that is a reference picture for the projector (below), not something `LogGrf3d` reproduces.

Text is measured as `grf_log.c` measures it, in the text's own plane: `grf_log.c`'s character height (4 mm times the text `Size`), 0.6 width factor, descender and padding, applied along the baseline and up directions in 3D, converted to world units by one scale, millimetres per world unit (default 200, so a unit cube matches `grf_log.c`'s unit page).
The text axes are built from the up and normal vectors with no camera and no flips; `grf3d_plplot.c`'s `getTextAxes` flips them towards an eye, but its flips leave the text's box unchanged.
`LogGrf3d` and `SceneGrf3d` share this measurement, one function in Rust, copied operation for operation from `grf3d_log.c`.

### `Plot3D`

Built as C builds it: a `Plot` part over the 3D FrameSet, holding Plot3D's own attributes (`Norm`, `RootCorner`) and C's bookkeeping, plus three inner `Plot`s, one per face.
The Plot design's `Draw` trait is the seam C's vtab gives (`2026-10-30-plot-design.md`, "Plot's extension points"): `Plot3D` implements it, overriding `draw_grid`, `draw_border`, `draw_text` and `draw_mark`, and returning C's error from the methods C refuses.
`Draw` as it stands takes `&mut dyn Grf` and a two-element text up vector, so it gains two associated types: the backend (`dyn Grf` for `Plot`, `dyn Grf3d` for `Plot3D`) and the up vector (`[f32; 2]` and `[f32; 3]`), the way C's one `astText` takes a 2- or 3-element `up` array.
Callers of `Plot`'s drawing methods are unchanged; the change to the trait gets a `CHANGELOG.md` entry and the bump `cargo semver-checks` asks for.
`set_tick_values` joins the trait.
`bounding_box` keeps its 2D return type, since on a `Plot3D` it only reports C's error.
`Plot3D` has its own region-outline entry point, as that design noted, because the inherent `Plot::draw_region_outline` calls Plot's `draw_border`.

Each inner Plot draws through a private adapter that implements `Grf` on top of the caller's `&mut dyn Grf3d` and a face, porting C's `Plot3DLine`, `Plot3DText` and the rest one for one.
This replaces C's registration of graphics functions and its Grf context KeyMap; nothing about it is public.

`set_tick_values` is ported on `Plot` and overridden on `Plot3D`, as in C.

`Plot3D` implements `Frame` and `Mapping` as `Plot` does, but has no `frameset_mut()` escape hatch: C's `Plot3D` overrides `SetCurrent`, `ClearCurrent` and `RemoveFrame` to keep its inner Plots in step, and a raw FrameSet handle would bypass that.
C builds the Plot part as a dummy 2D Plot and then swaps in the 3D Frames (`plot3d.c:7870`); the Rust construction follows the same steps.

Native save and load are byte for byte with C, including the nested `PlotXY`, `PlotXZ` and `PlotYZ`.
The attribute-string layer gains Plot3D: `Norm`, `RootCorner`, axis 3 on per-axis Plot attributes, and the plane suffixes.

### Recorded scene and projector

`SceneGrf3d` implements `Grf3d` by recording: polylines, markers and text, each text item with its string, reference point, justification, up and normal vectors and the attributes in force (color, width, style, size, font).
It answers `text_extent` and `char_height` with the measurement `LogGrf3d` uses, so the projected pictures show the layout that was checked against C.
The recorded scene is public, so a renderer outside the crate (WebGL, CARTA) can walk it.
Because `starlink-ast` is published, its types are `#[non_exhaustive]` and read through accessors, so fields can be added without a breaking change.

`Camera`: eye, target, up vector and screen distance, perspective or orthographic (screen distance zero), ported from `grf3d_plplot.c`'s camera with its auto-camera (fit the scene into the graphics box) and rotate-eye operations.

The projector renders a scene through a camera onto any `&mut dyn Grf`: polylines and markers by projecting their points, text as `grf3d_plplot.c` draws it, at the projected reference point, rotated to the projected baseline, at the projected character height.
It applies `getTextAxes`' flips at draw time, so text seen from behind or upside down reads normally while keeping its box.

**Checked against C's SVG, without being shaped by it.**
The projector is free to work as suits Rust: walk the scene in any order, sort by depth, batch, vectorise, keep its own float operation order.
It is not required to match `grf3d_log.c`'s SVG byte for byte, because C draws call by call as it goes and matching that would fix the projector's structure to an implementation detail of the C.
Instead a comparator parses the C reference SVGs (committed by the C test suite at fixed cameras) and the projector's SVG for the same Plot3D and camera, and matches them element for element, ignoring order: each polyline's points, each marker, and each text's string, anchor, angle and font size.
Positions may differ by one pixel, the rounding to integer pixels both sides do; angles by the matching amount.
That tolerance is the comparator's documented rule from the start, stated in its source with this reason; a case needing more goes in a named exclusion list with its reason and a companion test that fails when it starts passing (CLAUDE.md, "Comparators and skip modes").
The camera mathematics is also unit-tested against `grf3d_plplot.c`'s.

All three live in `starlink-ast`, which needs no new dependency for them.

### SVG, egui and the web page

- SVG: a Plot3D drawn into a `SceneGrf3d`, projected at a fixed camera onto `SvgGrf`.
- egui demo (`crates/ast-plot-demo`): a 3D mode that records the scene once, re-projects it onto `EguiGrf` each frame, and rotates the camera with the mouse; re-recorded only when an attribute or the case changes.
- Web plotter (`crates/ast-plotter-wasm`, `web/plotter/`): when the FrameSet read from the header has a 3D base Frame, the binding builds a Plot3D instead of a Plot and returns the SVG of its scene at a fixed camera; the example menu gains at least one 3D header.

## Stages

One ticket, one stacked PR per stage.

1. **`Plot3D` as an object.** Construction, every attribute with C's defaults, the attribute-string layer, `Transform` and the FrameSet overrides, Native save and load byte for byte with C (pinned by C probes, `plot3d-test1.ast` and `testplot3d`'s checkdump round trip).
   Needs only the existing C build, so it goes first.
2. **`Grf3d` and `LogGrf3d`.** Checked against the C 3D logging backend.
3. **Drawing.** `draw_grid`, `draw_border`, `draw_text`, `draw_mark`, `set_tick_values` on `Plot` and `Plot3D`, and the refused methods; whole call logs checked against C through `LogGrf3d`.
4. **Scene, camera, projector and SVG.**
5. **egui 3D mode.**
6. **Web plotter 3D mode.**

Stage 4 is checked against C's reference SVGs by the order-free comparator; stages 5 and 6 are checked by eye.
Stages 2 and 3 do not merge until the user's C oracle exists and they are checked against it.
Stages 4 to 6 may be built on an unmerged stage 3, but merge after it.

## Decided by the user

- Plot3D is ported for CARTA (issue #12) and must be checkable by eye without CARTA: SVG, egui and the web page all render it.
- The C oracle and a proper C test suite are written by the user on the C side.
- Record once, project many times (approach 3).
- The web page draws 3D at a fixed viewpoint; WebGL is later.
- Text is measured as `grf_log.c` measures it, in the text's own plane, by both the C 3D log and the Rust scene recorder.
- `grf3d_log.c` gets optional SVG through a camera, as `grf_log.c` has; the Rust projector is compared with it, but its performance and structure are not compromised to match the C's implementation details.

## Calls made by Claude, not yet reviewed by the user

- Stroke-font projected text is out of scope, addable later as a projector option.
- The scene, camera and projector live in `starlink-ast`, not `ast-plugins`.
- `Plot3D` holds its inner Plots by composition, and the per-face `Grf` adapter is private.
- The stage order and split above, and that stages checked against the C oracle wait for it before merging.
- `Draw` gains associated backend and up-vector types rather than `Plot3D` getting a sibling trait.
- The scene types are `#[non_exhaustive]` with accessors.
- No `frameset_mut()` on `Plot3D`.
- The web page detects 3D from the FrameSet's base Frame having three axes.

## Left to the builder

- The comparator's parsing of SVG (no new dependency in `starlink-ast`; a test-only parser of the subset both sides write is enough).
- What a zero normal vector does in the log and the scene (`grf3d.h` means "towards the camera", which the log does not have): the C plan proposes an error; the Rust side copies whatever `grf3d_log.c` does.
- The names and fields of the scene types.
- How the egui demo maps mouse drags onto camera rotations, and its default viewpoint.
- How Plot's 2D assumptions, if any remain outside the per-axis attribute arrays (already sized three), are generalised for the Plot part of a Plot3D.
- The 3D examples added to the web page and the egui demo's cases.
