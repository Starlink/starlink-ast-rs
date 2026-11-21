# Browser FITS plotter and attribute strings design

Status: draft for review, 2026-11-21.
Branches: one per stage, the first `u/timj/wasm-plotter-1-attrs`, stacked on `u/timj/plot-7-egui`.
Related: [Starlink/starlink-ast-rs#12](https://github.com/Starlink/starlink-ast-rs/issues/12) (a JavaScript API for the WebAssembly build); the Plot design, [`2026-10-30-plot-design.md`](2026-10-30-plot-design.md).

## Intent

Reproduce the AST grid plotter at <https://starlink.eao.hawaii.edu/cgi-bin/ast/fits-plotter> as a single web page that runs entirely in the browser.
The CGI page sends a FITS header and settings to a server, which runs a Fortran program (`fplottest.f`) and returns a PGPLOT image and two text outputs.
Here the Rust library, compiled to WebAssembly, does all of it on the client and draws SVG, so the page needs no server: it is one HTML file that opens by double-clicking.

Now it is a tool for developers to experiment with the Rust plotting engine on headers and settings of their choice.
Later it could replace the CGI page on a public site, and the binding behind it will inform the JavaScript API discussed in issue #12.

The CGI page's settings fields take attribute strings, `System=ICRS, Grid=1, Label(1)=First axis`, which set Frame and FrameSet attributes through the Plot as well as Plot ones.
The Rust library has no general way to apply such a string: its API is typed setters, and the parsers that exist are partial copies (below).
So the first stage builds that layer properly, ported from the C library's attribute handling, as the shared foundation that issue #12's JavaScript API and the future C API (`starlink-ast-capi`) both need.

## Goals

- **Attribute strings.** Set, get, test and clear attributes by name, from strings in the C library's syntax, with the C library's results, for Plot, FrameSet, Frame, SkyFrame, SpecFrame, DSBSpecFrame, TimeFrame, CmpFrame and FitsChan, and the attributes they inherit (Object, Mapping, Channel, and the per-axis attributes Frames hand to their Axis, SkyAxis and similar objects).
- **The plotter page.** One self-contained HTML file with the CGI page's inputs, examples and three outputs, computed in the browser by the WebAssembly build.
- **Checked against C.** Both the attribute layer and the page's three outputs are compared with the C library, byte for byte.

## Non-goals

- Attribute strings for classes beyond those listed; they come later, class by class.
- The general JavaScript API of issue #12, an npm package, or settling any of issue #12's open questions.
  The page's binding is narrow and unpublished.
- The C-ABI shim (`starlink-ast-capi`); the attribute layer is built so it can use it, nothing more.
- Hosting the page anywhere, or embedding it in the crate's docs.rs pages (docs.rs is not expected to run a page's own scripts; linking to a hosted copy is the later route).
- The STC-S plotting demo that the AST web pages also list; a natural follow-on.
- Browser automation tests of the page.

## Anti-goals

- Attribute names, accepted values, returned text or errors that differ from the C library's for the classes covered.
  A user should be able to use the AST documentation's attribute reference unchanged.
- One central `match` over attribute names for every class.
  C's per-class attribute functions (`SetAttrib`, `GetAttrib`, `TestAttrib`, `ClearAttrib`) are an extension chain that each class extends and passes on; the port keeps that granularity (CLAUDE.md, "Before porting a class").
- String-typed attribute access on `starlink-ast`'s everyday public API: it is public only behind a Cargo feature that is off by default.
- A page that needs a server, a network connection, or anything installed in the browser.
- A page that goes blank or freezes on bad input: every error is shown, in AST's words.

## Background

### The CGI page

Inputs:

- a FITS header (text box);
- Plot settings, an attribute string applied to the Plot (and through it to the FrameSet and its current Frame);
- FitsChan settings, an attribute string applied to the FitsChan that reads the header;
- output width and height in screen pixels;
- an axis version character: a space for the primary axes, a letter `A`–`Z` for the alternate description with that letter, `$` for pixel coordinates;
- eight examples, each filling the header and the Plot settings: AITOFF, COBE, ORIGIN, SCP, SPEC2D, SOLAR, NCP, HPX.

Outputs: the annotated grid, the header rewritten as standard FITS-WCS ("normalised"), and the FrameSet as AST Native text.

The examples' headers and settings are in the page's own JavaScript (`loadExample`); for instance AITOFF's settings are `System=ICRS, Grid=1, Tickall=0, Border=1` and SPEC2D's are `System(1)=wave, LogTicks(1)=1, LabelUnits=1, MinTickLen=0.02, MajTickLen=0.05, NumLabGap=0.1, Title=Logarithmic axis example`.
They differ from the repository's grid cases of the same names, so the page carries its own copies, taken from that page.

### What `fplottest.f` does

The Fortran program behind the CGI page, available from the page's "source code" link:

1. Creates a FitsChan with the FitsChan settings as its options string, and puts the header's cards into it.
2. Finds `NAXIS1` and `NAXIS2` (100 each if absent) and makes the plot box `0.5, 0.5, NAXIS1+0.5, NAXIS2+0.5`.
3. Reads the FrameSet; reports an error if none is read.
4. For the normalised headers: clears the FitsChan, sets `Encoding=FITS-WCS`, writes the FrameSet, and lists the cards.
5. For the FrameSet text: writes the FrameSet to a plain Channel.
6. Applies the version character: `$` makes the base Frame current; a letter makes current the Frame whose `Ident` is that letter, or reports that there is none.
7. For the grid: a PGPLOT window with equal aspect (`PGWNAD(0,1,0,1)`), its world box less 5% on each side, shrunk on one axis to keep the plot box's aspect ratio; `AST_PLOT( FS, GBOX, PBOX, SETTING )`, then `AST_GRID`.

The page follows these steps; the C probe (below) follows them too, with `grf_log`'s SVG in place of PGPLOT.

### What exists in the Rust port

- Typed setters and getters for every attribute of the classes covered.
- `crates/starlink-ast/src/fitschan/options.rs`: a private port of `object.c`'s `astSet` splitting and FitsChan's `SetAttrib`, setting only, used for the `FITSCHAN_OPTIONS` environment variable, which C's FitsChan constructors read (no other class reads one).
- `crates/ast-plot-demo/src/settings.rs` and `crates/starlink-ast/tests/common/plot.rs`: provisional Plot-only parsers, not ported from C; `PLAN.md` has a row saying they should be replaced by a port of `SetAttrib`.
- `SvgGrf`, which reproduces `grf_log.c`'s SVG byte for byte: graphics coordinates 0 to 1 span the image's width and height separately, on a black background, with lines drawn through labels.
- The crate builds for `wasm32-unknown-unknown` and its library tests pass under `wasm32-wasip1` (checked 2026-10-30).

## Design

### Stage 1: attribute strings

A port of the C library's attribute-string machinery:

- `object.c`'s `astSet` splitting of a settings string into `name=value` assignments (its comma rule, quoting and whitespace handling), as `fitschan/options.rs` already ports it;
- each covered class's `SetAttrib`, `GetAttrib`, `TestAttrib` and `ClearAttrib`, at C's granularity: a class handles the names it owns and hands the rest to its parent class's function, and Plot hands names it does not own to FrameSet, FrameSet to its current Frame, CmpFrame to its component Frames, and Frame its per-axis names to its Axis objects, as C does;
- the text a get returns is what `astGetC` returns, and errors carry C's codes and messages.

DSBSpecFrame is covered because a FitsChan reading a header with `IMAGFREQ` and similar cards produces one, so a pasted header can make it the current Frame or a CmpFrame component.
When the chain reaches a class not yet covered, the operation fails with an error naming the class and saying its attribute strings are not yet supported; it is the one error with no C counterpart, and it disappears class by class.
Object's `RefCount`, `Nobject` and `ObjSize` have no value in the Rust library (`PLAN.md` records them under `starlink-ast-capi`); getting them fails with an error saying so, and they are listed as a named exception in the C comparison.

The layer lives in `starlink-ast`, because it needs the crate's internals (FrameSet integrity when a Frame attribute is set through it, the Axis objects, typed test and clear methods the public API has only for some attributes) and because the FitsChan constructor already applies `FITSCHAN_OPTIONS` through a copy of part of it.
The per-class chain is hidden trait methods, implemented per class and always compiled, which the FitsChan constructor uses.
The four operations are public, for the covered classes on the objects the Rust API already has (`Plot`, `FrameSet`, the Frame types, `FitsChan`), only with a new Cargo feature that is off by default; the new public items are additions for semver, with a `CHANGELOG.md` entry.
CLAUDE.md's rule against string-typed attribute access in `starlink-ast` ("What NOT to do" and the architecture notes) is amended in the same stage to say so.
`fitschan/options.rs`, the demo's `settings.rs` and the test helper's parser are replaced by it, and the `PLAN.md` row about them is closed.

Checked by a C probe that applies settings strings to objects built by the reference library, and records `astGetC`, `astTest` and the Native dump after each, and the error message for invalid ones; the port must match each byte for byte.
The cases include every attribute the eight examples and the grid cases set, axis-qualified names (`Label(1)`, `System(1)` on a CmpFrame), per-element Plot styles (`Colour(axes)`), abbreviated values (`Labelling=int`), and invalid names and values.

### Stage 2: the binding, the page and the build

**The binding.** A new unpublished crate with one entry point for the page: given the header text, the FitsChan settings, the Plot settings, the width, the height and the version character, it returns the SVG, the normalised header and the FrameSet text, each either its text or the error that stopped it.
It follows `fplottest.f`'s steps with the stage 1 layer, and draws through `SvgGrf`.
The work is plain Rust, testable natively; a thin `wasm-bindgen` wrapper exposes it to JavaScript.

**Two opt-in `SvgGrf` settings,** each off by default so that `SvgGrf` still reproduces `grf_log.c`:

- erase under text: fill each text's box with the background before drawing it, as the C library's PGPLOT and PLplot backends do (`grf_pgplot.c`'s `astGText`, `grf_plplot.c`), so grid lines stop short of labels;
- equal aspect: map graphics coordinates 0 to 1 onto the largest centred square of the image, as `fplottest.f`'s `PGWNAD(0,1,0,1)` does, so a non-square image does not stretch the drawing; text, and markers, are then drawn at the size the Plot laid them out for (characters 0.02 of the square's side at size 1, from the backends' 4 mm on a 200 mm page), rather than `grf_log.c`'s fixed 14 pixels, so labels fit their boxes at any image size.

The page turns both on.

**The page,** laid out with everything visible at once:

- left: an example menu (the eight CGI examples, each filling the header and Plot settings as the CGI page does), the header text box, Plot settings, FitsChan settings, width and height, and the version character;
- right of those: the grid, as SVG;
- below: the normalised header and the FrameSet text, side by side.

It redraws shortly after the user stops typing; there is no submit button.
An error replaces the output it affects, in AST's words.
It is plain and readable in light and dark mode; it does not copy the CGI page's styling.

**The build.** A script compiles the binding for `wasm32-unknown-unknown` in release, runs `wasm-bindgen` with a target that needs no ES modules, and writes one HTML file with the page's HTML, CSS and JavaScript, the generated glue and the WebAssembly (base64-encoded) inline, so it opens from `file://`.
It needs rustup's `wasm32-unknown-unknown` target and a `wasm-bindgen-cli` matching the crate's `wasm-bindgen` version; it uses rustup's `cargo` explicitly, since a Homebrew `rustc` earlier on `PATH` hides rustup's targets.

**Checked against C.** A C probe that follows `fplottest.f` writes the three outputs for each of the eight examples, the grid through `grf_log`'s SVG at a fixed size, with the new `SvgGrf` settings off.
The binding's outputs for the same inputs must match byte for byte; a case that cannot goes in a named exclusion list with its reason and a companion test that fails when it starts matching (CLAUDE.md, "Comparators and skip modes").
CI builds the binding for `wasm32-unknown-unknown` and runs its native tests; the page itself is checked by eye.

## Decided by the user

- A single page, client-side only, opened by double-clicking one HTML file; serving it over HTTP or hosting it can come later.
- All three outputs and all eight examples are kept.
- Layout C: grid beside the inputs, the two text outputs side by side underneath, no tabs.
- The attribute layer is ported properly from `SetAttrib`, not a demo-only parser.
- It lives inside `starlink-ast`, public only behind a Cargo feature that is off by default.
- It covers set, get, test and clear for the plotter's classes first; other classes later.
- The WebAssembly binding is narrow, for the page only, and does not start issue #12's API.
- Two stages, each a branch off the one before, starting from `u/timj/plot-7-egui`.

## Calls made by Claude, not yet reviewed by the user

- The binding is a separate, unpublished crate.
- The page redraws as you type, after a short pause.
- The grid is drawn by `SvgGrf`, not by a backend measuring text with the browser's fonts, so it is the backend the tests compare with C.
- The two opt-in `SvgGrf` settings, both on in the page, with equal aspect also scaling text to the layout's size.
- DSBSpecFrame joins stage 1; other classes the chain reaches before they are covered give an error saying so.
- Object's `RefCount`, `Nobject` and `ObjSize` give an error and are a named exception in the C comparison.
- The C probe for the page follows `fplottest.f` with `grf_log`'s SVG.
- The look is plain, for light and dark mode, not a copy of the CGI page.

## Left to the builder

- Names: the Cargo feature, the binding crate and the page directory (placeholders so far: `attribute-strings`, `ast-plotter-wasm`, `web/plotter/`), and the build script's name and output path.
- The Rust shape of the attribute interface, provided it keeps C's per-class chain.
- The order classes are ported in within stage 1, and how stage 1 is split into commits.
- The page's default width and height, and the redraw delay.
- Whether drawing runs on the page's main thread or in a worker, provided the page stays responsive.
- How the examples are stored in the page source.
