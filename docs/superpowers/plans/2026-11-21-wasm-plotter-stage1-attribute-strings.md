# Browser Plotter Stage 1: Attribute Strings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set, get, test and clear attributes by name from strings in the C library's syntax, with the C library's results and errors, for Plot, FrameSet, the Frame family and FitsChan, public in `starlink-ast` behind an off-by-default Cargo feature.

**Architecture:** A port of C's attribute-string machinery: `object.c`'s splitting and normalisation, then each class's `SetAttrib`/`GetAttrib`/`TestAttrib`/`ClearAttrib` as hidden methods on the `Frame` trait, implemented per class, each handling the names it owns and handing the rest to its parent class's handler (a free function, since Rust has no parent vtab pointer), with FrameSet's integrity bracket, Frame's axis path and CmpFrame's component delegation ported as C has them.
The internals are always compiled, because the FitsChan constructor applies `FITSCHAN_OPTIONS` through the FitsChan chain; only the public trait is behind the feature.
One new C probe applies strings to objects built by the reference library and records every result, error message and the final dump; the Rust test replays it byte for byte.

**Tech Stack:** Rust 2024, `starlink-ast`; a C probe against the reference `libast`, built as the existing probes are.

**Spec:** [`docs/superpowers/specs/2026-11-21-wasm-fits-plotter-design.md`](../specs/2026-11-21-wasm-fits-plotter-design.md), stage 1.
**Facts:** [`docs/audits/2026-11-21-attribute-strings-survey.md`](../../audits/2026-11-21-attribute-strings-survey.md), the survey of every C function and Rust counterpart this plan touches, with line numbers; each task names the survey sections it ports.

## Global Constraints

- Branch `u/timj/wasm-plotter-1-attrs`, stacked on `u/timj/plot-7-egui`; one PR.
- Spec: "Attribute names, accepted values, returned text or errors that differ from the C library's for the classes covered" is an anti-goal; every result and every error message is compared with C byte for byte.
- Spec: "One central `match` over attribute names for every class" is an anti-goal; each class's names live in that class's handler, which falls through to its parent's, at C's granularity (CLAUDE.md, "Before porting a class").
- Spec: string-typed attribute access is public only behind a Cargo feature that is off by default; the feature is `attribute-strings`.
- Spec: covered classes: Plot, FrameSet, Frame, SkyFrame, SpecFrame, DSBSpecFrame, TimeFrame, CmpFrame, FitsChan, and the attributes they inherit (Object, Mapping, Channel, Axis, SkyAxis).
- Spec: a class the chain reaches before it is covered fails with an error naming the class and saying its attribute strings are not yet supported.
- Spec: Object's `RefCount`, `Nobject` and `ObjSize` fail with an error and are a named exception in the C comparison.
- No skip modes; every exception is per case, named in source, with a reason and a companion test that fails when the case starts matching (CLAUDE.md, "Comparators and skip modes"); each exception is counted in `PLAN.md` §6 and the verification ledger (`test_verification_ledger.rs`).
- Every C behaviour the port mirrors that C's authors would call a bug gets a `docs/issues/c-library-quirks.md` entry.
- The SCUBA-2 hot path is untouched: no change to any `SkyFrame::set_*` body.
- New public items: `#[doc(alias)]` for the C names, docs written for the Rust (CLAUDE.md, step 9), a `CHANGELOG.md` `Unreleased` entry.
- Commit messages end with the `Co-Authored-By` line only.
- Strip conda for cargo and probe builds: `env -u CC -u LDFLAGS -u CFLAGS`.
- Per-task test: `cargo test -p starlink-ast --test frames test_attr_strings::`; at the end of Task 9 the whole suite, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo doc -p starlink-ast --no-deps --features attribute-strings`.

## Decisions

- **Hidden `Frame` methods** (in `frames/mod.rs`, `#[doc(hidden)]`, beside `axis_object_top`), the extension seam:
  - `fn attr_class(&self) -> &'static str` — the C class name, for error messages and `Class`; default: the class name the serialiser uses, through a new `serial::frame_class_name(&dyn Frame)` built on the same downcast chain as `FrameToNode for dyn Frame` (`serial/mod.rs:586`).
  - `fn attr_vset(&mut self, settings: &str) -> AstResult<()>` — C's `VSet`; default: split (survey A.1), then `attr_set` per assignment, stopping at the first error. FrameSet overrides it with the integrity bracket.
  - `fn attr_set(&mut self, setting: &str) -> AstResult<()>` — `SetAttrib`, given one normalised `name=value`.
  - `fn attr_get(&self, name: &str) -> AstResult<String>` — `GetAttrib`, given a normalised name.
  - `fn attr_test(&self, name: &str) -> AstResult<bool>` — `TestAttrib`.
  - `fn attr_clear_list(&mut self, names: &str) -> AstResult<()>` — C's `Clear`; default: split on every comma, normalise, skip blanks, `attr_clear` each. FrameSet overrides it with the integrity bracket.
  - `fn attr_clear(&mut self, name: &str) -> AstResult<()>` — `ClearAttrib`.
  - Defaults of `attr_set`, `attr_get`, `attr_test`, `attr_clear`: the not-yet-supported error, so Regions, FluxFrame and SpecFluxFrame stay uncovered until a later stage overrides them.
- **Parent calls** are free functions in `crate::attributes`, one per class and operation (`object_set`, `mapping_set`, `frame_set`, `skyframe_set`, ...), each taking the object and the setting and returning `AstResult<()>` or the value; a class's trait method is its own function, which ends by calling its parent's.
- **Axis objects** are reached through two new hidden `Frame` methods, `fn attr_axis(&self, axis: usize) -> AstResult<AxisRef<'_>>` and `fn attr_axis_mut(&mut self, axis: usize) -> AstResult<AxisMut<'_>>` (1-based, the Frame's external axis order), with `pub(crate) enum AxisRef<'a> { Plain(&'a Axis), Sky(&'a SkyAxis) }` and `AxisMut` likewise in `frames/axis.rs`; BasicFrame and SkyFrame return their `FrameAxis`, SpecFrame and TimeFrame their plain `Axis`, CmpFrame and FrameSet delegate to the component or current Frame.
- **Primary Frames for writing:** a new hidden `fn primary_frame_mut(&mut self, axis: usize) -> (&mut dyn Frame, usize)` mirroring `primary_frame` (`frames/mod.rs:1782`), overridden by CmpFrame and FrameSet.
- **Mapping attributes on plain Frames:** C accepts them on every Frame; Rust Frames other than FrameSet and Plot store no Mapping flags. Gets of the read-only ones are computed as C computes them for a Frame (`Nin`, `Nout` the axis count; `IsLinear`, `TranForward`, `TranInverse` 1; `IsSimple` 0), `Invert` and `Report` get `0` and test false; setting or clearing `Invert` or `Report` on such a Frame fails with the not-yet-supported error and is a named exception, with a `PLAN.md` row saying what would expose it.
- **Plot's used-value gets** (survey B.12): C runs an invisible `astGrid` through the graphics module in use. The Plot's `attr_get` clones the Plot and calls `used_values` with a `LogGrf` writing to `std::io::sink()`, which is the page model C's `grf_log` gives, so a get matches C linked with `grf_log`; and, behind the feature, `Plot::get_attribute_with_grf(&mut self, name: &str, grf: &mut dyn Grf) -> AstResult<String>` gets with a caller's backend.
- **Plot's `Grf` attribute:** C's Plot has it (an int, not dumped); the Rust Plot gains a `grf: Option<bool>` attribute field with get/set/test/clear in `plot/attrs.rs`, not dumped, so the string layer can handle it as C does.
- **Integrity:** C's `RecordIntegrity`/`RestoreIntegrity` (frameset.c:8440, :9345) are ported as `pub(crate)` FrameSet methods with C's `integrity_lost` flag, used by FrameSet's `attr_vset`/`attr_clear_list` overrides and by its `Current`/`Invert` arms; `with_integrity` stays for the typed setters.
- **Errors:** C's first message is the `AstError` message and each later one a `context` line (`with_context`), so `NOWRT`'s "This is a read-only attribute." and the integrity "Unable to accommodate ..." are context; the probe records every message C reports, and the Rust test compares code, message and context.
- **Get strips graphical escapes** unless escapes are on (`crate::escapes::escapes()`), as `astGetC` does (survey A.2); `astStripEscapes` (plot.c:25659) is ported on `plot::text::find_escape`.
- **The public trait** (`src/attributes/public.rs`, `#[cfg(feature = "attribute-strings")]`, re-exported as `starlink_ast::attributes::Attributes`):

  ```rust
  pub trait Attributes {
      fn set_attributes(&mut self, settings: &str) -> AstResult<()>;   // astSet( obj, "%s", settings )
      fn set_attribute(&mut self, name: &str, value: &str) -> AstResult<()>; // astSetC
      fn get_attribute(&mut self, name: &str) -> AstResult<String>;    // astGetC
      fn test_attribute(&self, name: &str) -> AstResult<bool>;         // astTest
      fn clear_attributes(&mut self, names: &str) -> AstResult<()>;    // astClear
  }
  ```

  implemented for every `T: Frame + ?Sized` (through the hidden methods) and for `FitsChan`.
  `get_attribute` takes `&mut self` because FitsChan's gets read its cards.
- **Tests see the feature** through a dev-dependency of `starlink-ast` on itself with the feature on (checked to work with this Cargo); `cargo package` strips it, so CI's archive job passes `--features attribute-strings`.
- **The probe** `tools/oracle_fixtures/attr_strings_probe.c` writes `crates/starlink-ast/tests/attr_strings_oracle/cases.rs`, replayed by `crates/starlink-ast/tests/test_attr_strings.rs` in the `frames` area program.
  - Each case names an object, given as the Native text C writes for it (a FrameSet, Plot or Frame built by C constructors or read from a header under `tests/fixtures/wcsconv/inputs/`), or for a FitsChan as the header text; the Rust test builds the same object by reading that text.
  - Each case lists operations, `Set(settings)`, `SetC(name, value)`, `Get(name)`, `Test(name)`, `Clear(names)`, each with C's outcome: the value or `true`/`false`, or the error code's name and every message.
  - C applies strings as `astSet( obj, "%s", text )`, so `%` is not interpreted (survey A.1).
  - The probe defines its own `astPutErr` to collect messages, links `src/grf_log.c` for Plot cases, and does not link `libast_err`; its header gives the build command.
  - After the operations it records the Native dump (Frames, FrameSets, Plots), compared byte for byte.
  - Each task adds its classes' cases to the probe and regenerates `cases.rs`.

## Review Focus

1. Names written with spaces and capitals, `" Label ( 1 ) = Right ascension"`, are normalised as C normalises them, and the value keeps its spaces (Task 1, `names_are_normalised_values_are_not`).
2. A setting string with a quoted value containing commas, and one with a single `=` whose value has commas, apply as one assignment each (Task 1, `commas_split_only_between_assignments`).
3. A string whose second assignment fails leaves the first applied and the third not, and a FrameSet's Mappings untouched (Task 6, `a_failed_assignment_stops_the_string`).
4. A Frame attribute set through a FrameSet that changes the Frame's coordinates (`System=FK4` on a sky FrameSet) keeps the FrameSet's Mappings consistent, as C does (Task 6 cases from `integrity_probe.c`'s classes).
5. A used-value get on a Plot whose grid cannot be drawn returns C's error, not a panic (Task 7, `a_used_value_get_reports_the_grid_error`).

---

### Task 1: Foundation, Object and Mapping

Ports survey A.1–A.8 (splitting, normalisation, the idioms, the error templates), B.1 Object and B.2 Mapping.

**Files:**
- Create: `crates/starlink-ast/src/attributes/mod.rs` (crate-private module, the shared helpers), `attributes/public.rs` (the trait, feature-gated), `attributes/object.rs`, `attributes/mapping.rs`
- Create: `tools/oracle_fixtures/attr_strings_probe.c`, `crates/starlink-ast/tests/attr_strings_oracle/cases.rs` (generated), `crates/starlink-ast/tests/test_attr_strings.rs`
- Modify: `crates/starlink-ast/src/lib.rs` (module, feature re-export), `crates/starlink-ast/Cargo.toml` (feature, self dev-dependency), `crates/starlink-ast/src/frames/mod.rs` (hidden methods), `crates/starlink-ast/src/frames/basic.rs` (BasicFrame's `attr_*`: Mapping chain only, Frame's own arms come in Task 2), `crates/starlink-ast/src/serial/mod.rs` (`frame_class_name`), `crates/starlink-ast/tests/frames/main.rs`, `crates/starlink-ast/src/fitschan/options.rs` (its splitter moves to `attributes`, which it then calls)
- Commit with it: `docs/audits/2026-11-21-attribute-strings-survey.md`

**Interfaces:**
- Produces, in `crate::attributes`:
  - `split_settings(settings: &str) -> Vec<String>` — VSet's assignments, normalised (survey A.1 rules 3–4), blanks dropped.
  - `normalise_name(name: &str) -> String` — Get/Test: white space removed, lower case.
  - `split_names(names: &str) -> Vec<String>` — Clear: every comma, each normalised, blanks dropped.
  - Matchers for the `astSscanf` idioms, each `None` when the whole setting does not match: `int_value(setting, name) -> Option<i32>` (`"name= %d %n"`), `double_value(setting, name) -> Option<f64>` (`"name= %lg %n"`, C `strtod`), `string_value<'a>(setting: &'a str, name) -> Option<&'a str>` (`"name=%n%*[^\n]%n"`), `keyword_value<'a>(setting, name) -> Option<&'a str>` (`"name= %n%*s %n"`), `axis_qualified(setting: &str) -> Option<(&str, i32, &str)>` (name, axis, value), `is_read_only(setting, name) -> bool` (`MATCH`).
  - `full_form(options: &[&str], test: &str, attr: &str, method: &str, class: &str) -> AstResult<usize>` (Plot's `FullForm`, `AST__OPT` errors) and `abbreviates(value, name) -> bool` (FitsChan's `Ustrncmp`), moved from `fitschan/options.rs`.
  - Formatters: `fmt_int(i32) -> String`, `fmt_double(f64) -> String` (`%.*g` with 17 digits, `format_double_g_signed`), `fmt_decimal_yr(year: f64, digits: usize) -> String` (a general port of `astFmtDecimalYr`, frame.c:4207).
  - Error builders, each with C's code and text (survey "Common error templates"): `unknown_set(setting, class)`, `unknown_get(name, class)`, `unknown_test(name, class)`, `unknown_clear(name, class)`, `blank_get(class)`, `read_only_set(setting, class)`, `read_only_clear(name, class)`, `axis_invalid(method, class, axis, naxes)`, `not_supported(method, class)` (code `Badat`, message `"{method}: Attribute strings are not yet supported for a {class}."`).
  - `strip_escapes(text: &str) -> String`.
  - `object_set`, `object_get`, `object_test`, `object_clear` over a crate trait `ObjectAttrs` (ID, Ident, UseDefs, and the class name), implemented for every `T: Frame + ?Sized` and, in Task 8, FitsChan.
  - `mapping_set`, `mapping_get`, `mapping_test`, `mapping_clear` for Frames, per Decisions.
- Produces: the hidden `Frame` methods and `Attributes` trait (Decisions).

- [ ] **Step 1: Write the failing tests**

In `attributes/mod.rs` unit tests:

```rust
#[test]
fn names_are_normalised_values_are_not() {
    assert_eq!(split_settings(" Label ( 1 ) = Right ascension"), ["label(1)= Right ascension"]);
    assert_eq!(normalise_name(" Colour ( Axes ) "), "colour(axes)");
}

#[test]
fn commas_split_only_between_assignments() {
    assert_eq!(split_settings("Title=a,b"), ["title=a,b"]);
    assert_eq!(split_settings("Title=\"a,b\",Grid=1"), ["title=a,b", "grid=1"]);
    assert_eq!(split_settings("Title=a,b,Grid=1"), ["title=a", "b", "grid=1"]);
    assert_eq!(split_settings("Title="), ["title= "]);
    assert_eq!(split_names("Title, ,Grid"), ["title", "grid"]);
}

#[test]
fn decimal_years_are_formatted_as_c_formats_them() {
    assert_eq!(fmt_decimal_yr(2000.0, 17), "2000.0");
    assert_eq!(fmt_decimal_yr(1950.5, 17), "1950.5");
}
```

In `test_attr_strings.rs`: `object_and_mapping_names_match_the_c_library` replays the probe's `frame2_*` cases, and `cases_with_no_rust_value_still_differ` checks each entry of `NO_RUST_VALUE` (the named exceptions: `Nobject`, `ObjSize`, `RefCount`; `Invert`/`Report` set and clear on a plain Frame) still differs.
The probe's first cases, on a 2-axis `astFrame` and a 1-axis one: set, get, test and clear of `ID`, `Ident`, `UseDefs`; get of `Class`, `Nin`, `Nout`, `IsLinear`, `IsSimple`, `TranForward`, `TranInverse`, `Invert`, `Report`; set and clear of `Class` and `Nin` (`NOWRT`); unknown names for each operation; a blank get; `Ident=` (a space); the splitting cases above through `astSet`.

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --lib attributes:: && cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: compile errors for the missing module and trait.

- [ ] **Step 3: Implement the foundation, Object and Mapping, and the probe**

Build the probe as `plot_methods_probe.c`'s header describes, with this file and `../src/grf_log.c` and no `libast_err`; regenerate `cases.rs`.
`fitschan/options.rs` keeps its FitsChan chain until Task 8 but calls the moved splitter and matchers.

- [ ] **Step 4: Run the tests**

Run: the Step 2 commands, and `cargo test -p starlink-ast --test fitschan options` for `FITSCHAN_OPTIONS`.
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast docs/audits/2026-11-21-attribute-strings-survey.md tools/oracle_fixtures/attr_strings_probe.c
git commit -m "Add attribute strings: splitting, Object and Mapping attributes"
```

### Task 2: Frame, Axis and SkyAxis on a BasicFrame

Ports survey B.3–B.5: Frame's 26 names, the generic axis path through the Axis object and the primary Frame, the one-axis retry, Epoch, ObsLat and ObsLon parsing and formatting, and Frame's `AXIIN` wording.

**Files:**
- Create: `crates/starlink-ast/src/attributes/frame.rs`, `attributes/axis.rs`
- Modify: `frames/mod.rs` (`attr_axis`, `attr_axis_mut`, `primary_frame_mut`), `frames/axis.rs` (`AxisRef`, `AxisMut`), `frames/basic.rs`, the probe and `cases.rs`, `test_attr_strings.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces: `frame_set`, `frame_get`, `frame_test`, `frame_clear` (Frame's handler, falling through to `mapping_*`), `axis_set`, `axis_get`, `axis_test`, `axis_clear` and `skyaxis_*` over `AxisRef`/`AxisMut`; the hidden methods in Decisions.

- [ ] **Step 1: Write the failing tests**

`frame_names_match_the_c_library` replays the new cases: on a 2-axis and a 1-axis `astFrame`, each of Frame's names set, got, tested and cleared, with valid and invalid values; `Digits` and `Digits(1)`; `Label(3)` on a 2-axis Frame (`AXIIN`); `Ident(2)=x` reaching the Axis (and its dump); `Label=x` on a 1-axis Frame (the one-axis retry); `Epoch=J2000`, `Epoch=2010-03-01`, `Epoch=nonsense`; `ObsLat=N19:49:31`, `ObsLon=W155:28:22`, an invalid ObsLat; `MinAxes=3` beyond `MaxAxes`; unknown `Fred(1)=2` (the Axis's error).

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `frame_names_match_the_c_library` on the first Frame name.

- [ ] **Step 3: Implement Frame, Axis and SkyAxis**

C's int handling goes through the typed setters with C's conversions (survey C.1: Digits clamps to at least 1, MinAxes/MaxAxes through the consistency rule, booleans non-zero).

- [ ] **Step 4: Run the tests**

Run: the Step 2 command. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for Frame, Axis and SkyAxis"
```

### Task 3: SkyFrame

Ports survey B.6: SkyFrame's 13 names, its System and AlignSystem codes, SkyRef and SkyRefP in both forms, SkyRefIs, AsTime, and IsLatitude and CentreZero through the axis path.

**Files:**
- Create: `crates/starlink-ast/src/attributes/skyframe.rs`
- Modify: `frames/skyframe.rs` (its `attr_*` and axis hooks; no `set_*` body changes), the probe, `cases.rs`, `test_attr_strings.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces: `skyframe_set`, `skyframe_get`, `skyframe_test`, `skyframe_clear`.

- [ ] **Step 1: Write the failing tests**

`skyframe_names_match_the_c_library` replays cases on an `astSkyFrame` and a SkyFrame with its axes permuted: each name with valid and invalid values, `System=Galactic`, `System=nonsense`, `SkyRef=12:30:00,-30:00:00` and `SkyRef(2)=0.5`, `SkyRefIs=Origin` and `SkyRefIs=bad` (`AST__OPT`), `Equinox=B1950`, `AsTime(1)=1`, `IsLatitude(2)=0`, `CentreZero(1)=1`, `Equinox(1)=J2000` through the axis path, clearing `SkyRef`, and gets of `LatAxis`, `LonAxis`, `IsLatAxis(1)`.

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `skyframe_names_match_the_c_library`.

- [ ] **Step 3: Implement SkyFrame's handler**

- [ ] **Step 4: Run the tests**

Run: the Step 2 command. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for SkyFrame"
```

### Task 4: SpecFrame and DSBSpecFrame

Ports survey B.7–B.8: the one-axis shortcut arms, RestFreq in GHz or with units, SourceVel in km/s, SpecOrigin, RefRA and RefDec, the GeoLat and GeoLon aliases, and DSBSpecFrame's five names.

**Files:**
- Create: `crates/starlink-ast/src/attributes/specframe.rs`
- Modify: `frames/specframe.rs`, `frames/dsbspecframe.rs`, the probe, `cases.rs`, `test_attr_strings.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces: `specframe_*` and `dsbspecframe_*`.

- [ ] **Step 1: Write the failing tests**

`spectral_names_match_the_c_library` replays cases on an `astSpecFrame` and an `astDSBSpecFrame`: `System=wave`, `StdOfRest=LSRK`, `RestFreq=230.5`, `RestFreq=1.3 mm`, `RestFreq=1 parsec` (unsupported units), `SourceVel=20` with `SourceSys=VRAD`, `SpecOrigin=230 GHz`, `RefRA=10:00:00`, `GeoLat=N19:49:31`, `Unit=GHz` (the shortcut to `Unit(1)`), `SideBand=lsb`, `DSBCentre=230`, `IF=4 GHz`, an inappropriate `DSBCentre` unit, get and clear of `ImagFreq` (read-only).

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `spectral_names_match_the_c_library`.

- [ ] **Step 3: Implement the two handlers**

Units go through `crate::units` as C's `astUnitMapper` does; the Rust setters take SI values (survey C.1), so the handler converts.

- [ ] **Step 4: Run the tests**

Run: the Step 2 command. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for SpecFrame and DSBSpecFrame"
```

### Task 5: TimeFrame and CmpFrame

Ports survey B.9–B.10: TimeFrame's six names and TimeOrigin's three forms; CmpFrame's delegation (set tries Frame first, then each axis's primary Frame, spreading an unqualified name to every axis; get sends an axis-qualified name straight to the primary Frame; every failure becomes CmpFrame's own error).

**Files:**
- Create: `crates/starlink-ast/src/attributes/timeframe.rs`, `attributes/cmpframe.rs`
- Modify: `frames/timeframe.rs`, `frames/cmpframe.rs` (`attr_*`, `primary_frame_mut`, axis hooks), the probe, `cases.rs`, `test_attr_strings.rs`, `PLAN.md` (row 71: the string path now exists and differs from the typed one as C's does)

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces: `timeframe_*`, `cmpframe_*`.

- [ ] **Step 1: Write the failing tests**

`time_and_compound_names_match_the_c_library` replays cases on an `astTimeFrame` (`TimeScale=TAI`, `TimeOrigin=2010-01-01`, `TimeOrigin=55197 d`, `LTOffset=-10`, `ClockLat=N19`, `TimeScale=bad`) and on `astCmpFrame( astSpecFrame, astSkyFrame )` and `astCmpFrame( astFrame(1), astFrame(1) )`: `System(1)=wave`, `System=FK4` (accepted by the sky component), `Label(2)=x` then `Get Label(2)` (the string-versus-typed difference of `PLAN.md` row 71), `Epoch=2000`, `RestFreq=bad` (replaced by CmpFrame's error), `Fred=1`.

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `time_and_compound_names_match_the_c_library`.

- [ ] **Step 3: Implement the two handlers**

- [ ] **Step 4: Run the tests**

Run: the Step 2 command. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for TimeFrame and CmpFrame"
```

### Task 6: FrameSet and integrity

Ports survey A.6 and B.11: FrameSet's own names, the Object and Mapping names it handles itself, everything else to the current Frame, and the integrity bracket over a whole setting string or clear list.

**Files:**
- Create: `crates/starlink-ast/src/attributes/frameset.rs`
- Modify: `frames/frameset.rs` (`record_integrity`, `restore_integrity`, the overrides), the probe, `cases.rs`, `test_attr_strings.rs`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: `pub(crate) fn record_integrity(&mut self)`, `pub(crate) fn restore_integrity(&mut self, method: &str) -> AstResult<()>` on FrameSet; FrameSet's `attr_vset` and `attr_clear_list` overrides; `frameset_*`.

- [ ] **Step 1: Write the failing tests**

`frameset_names_match_the_c_library` replays cases on FrameSets read from `aitoff.head`, `specflux.head` and `timeplot.head`: `Base=2`, `Current=Base`, `Current=SKY` (by Domain), `Current=9` (invalid), `System=FK4` and `Epoch=1950` through to the sky Frame with integrity, a sky FrameSet's `System=FK4,Fred=1,Grid=1` (`a_failed_assignment_stops_the_string`: FK4 applied, the error reported, the Mappings as C leaves them), `Invert=1` then `Current=1`, `Clear System,Epoch`, `Nframe` set (`NOWRT`), `UseDefs=0` reaching the current Frame, and the `AST__ILOST` cases of `integrity_probe.c` expressed as strings.

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `frameset_names_match_the_c_library`.

- [ ] **Step 3: Implement FrameSet's handler and the integrity bracket**

- [ ] **Step 4: Run the tests**

Run: the Step 2 command, and `cargo test -p starlink-ast --test frames integrity` for the typed path. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for FrameSet, with C's integrity bracket"
```

### Task 7: Plot

Ports survey B.12: Plot's 39 names, axis-indexed with and without an index, element-indexed through `FullForm` over C's 25 element labels, the used-value gets, and the `Grf` attribute.

**Files:**
- Create: `crates/starlink-ast/src/attributes/plot.rs`
- Modify: `plot/attrs.rs` (`grf` attribute), `plot/forward.rs` (Plot's `attr_*`), `plot/mod.rs` (`get_attribute_with_grf`, feature-gated), the probe, `cases.rs`, `test_attr_strings.rs`

**Interfaces:**
- Consumes: Task 6.
- Produces: `plot_set`, `plot_get`, `plot_test`, `plot_clear`; `Plot::get_attribute_with_grf(&mut self, name: &str, grf: &mut dyn Grf) -> AstResult<String>`; `Plot::grf`, `set_grf`, `test_grf`, `clear_grf`.

- [ ] **Step 1: Write the failing tests**

`plot_names_match_the_c_library` replays cases on Plots built from `aitoff.head` and `specflux.head` with `testgrid.c`'s boxes: the eight CGI examples' Plot settings strings, applied to Plots of their own headers (the design's appendix), each Plot name with valid and invalid values, `Gap=10` and `Gap(2)=10`, `Edge(1)=t`, `Edge(1)=x`, `Labelling=int`, `Labelling=bad`, `Colour(axes)=2`, `Colour=3`, `Color(Grid1)=4`, `Style(nosuch)=1`, a 21-character element label, gets of `Gap(1)`, `Labelling`, `Edge(2)` unset (used values) and set, `Grf=1`, `Title=Logarithmic axis example`, `Label(1)=First axis` reaching the current Frame.
`a_used_value_get_reports_the_grid_error` gets `Gap(1)` on a Plot with `Gap=0` (Plot's grid fails with `AST__PLFMT`, as `test_grid.rs` records), and expects C's error.

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `plot_names_match_the_c_library`.

- [ ] **Step 3: Implement Plot's handler**

- [ ] **Step 4: Run the tests**

Run: the Step 2 command. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for Plot"
```

### Task 8: Channel and FitsChan

Ports survey B.13–B.14 on top of the set chain `fitschan/options.rs` already has: Get, Test and Clear for Channel's 8 and FitsChan's 25 names, the read-on-access ones included, and the FitsChan `Attributes` impl; `FITSCHAN_OPTIONS` then goes through it.

**Files:**
- Create: `crates/starlink-ast/src/attributes/channel.rs` (Channel and FitsChan handlers, the set chain moved from `fitschan/options.rs`)
- Modify: `fitschan/options.rs` (keeps `env_options`; `apply` calls the moved chain), `fitschan/mod.rs`, `attributes/public.rs` (`impl Attributes for FitsChan`), the probe, `cases.rs`, `test_attr_strings.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces: `fitschan_set`, `fitschan_get`, `fitschan_test`, `fitschan_clear`, `channel_*`; `impl ObjectAttrs for FitsChan`.

- [ ] **Step 1: Write the failing tests**

`fitschan_names_match_the_c_library` replays cases on FitsChans holding `aitoff.head` and the CGI SOLAR example's header (the design's appendix, inline in the probe): `Encoding=FITS-WCS`, `Encoding=fits_iraf`, `Encoding=bad`, `CarLin=1`, `Card=3` then gets of `Card`, `CardName`, `CardType`, `CardComm`, `NCard`, `NKey`, `AllWarnings`; `Warnings=Tnx Zpx`; `AltAxes=bad`; clear of `NCard` (`NOWRT`) and of `CardName` (falls to Object's error); `Comment=0`, `Full=1`.

- [ ] **Step 2: Run them to see them fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --test frames test_attr_strings::`
Expected: FAIL in `fitschan_names_match_the_c_library`.

- [ ] **Step 3: Implement the Channel and FitsChan handlers**

- [ ] **Step 4: Run the tests**

Run: the Step 2 command, and the `FITSCHAN_OPTIONS` tests in `fitschan/options.rs`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Add attribute strings for Channel and FitsChan"
```

### Task 9: Replace the stopgap parsers, and the record

**Files:**
- Modify: `crates/ast-plot-demo/Cargo.toml` (`starlink-ast` with the feature), `crates/ast-plot-demo/src/settings.rs` (calls `Attributes::set_attributes`; its own parser goes), `crates/starlink-ast/tests/common/plot.rs` (`set_attr`, `set_style_attr`, `apply_settings` call the layer), `.github/workflows/ci.yml` (the archive job's `cargo test` passes `--features attribute-strings`; the docs step builds with it), `crates/starlink-ast/Cargo.toml` (`[package.metadata.docs.rs] features = ["attribute-strings"]`), `CLAUDE.md` (the "What NOT to do" row and the CFFI note: string access is the `attribute-strings` feature, built on the typed API), `CHANGELOG.md`, `PLAN.md` (close the stopgap-parser row, the exceptions in §6), `docs/audits/c-docs/ledger.tsv` (`astSet`, `astSet<X>`, `astGet<X>`, `astTest`, `astClear` point at the trait's methods)

**Interfaces:**
- Consumes: Tasks 1–8.

- [ ] **Step 1: Write the failing test**

In `crates/ast-plot-demo/src/settings.rs`, replace the tests with:

```rust
#[test]
fn settings_use_the_c_library_rules() {
    // C's Plot accepts the unique abbreviation "int" and reports a bad one
    // with its own message; Frame attributes reach the current Frame.
    let mut plot = plot();
    apply_settings(&mut plot, "Labelling=int,System=ICRS,Label(1)=First axis").unwrap();
    assert_eq!(plot.get_attribute("Label(1)").unwrap(), "First axis");
    let err = apply_settings(&mut plot, "Labelling=sideways").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Opt);
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p ast-plot-demo settings::`
Expected: FAIL: the stopgap parser does not know `System=ICRS` on a sky Plot.

- [ ] **Step 3: Switch the demo and the test helper to the layer; update the record**

- [ ] **Step 4: Run everything**

Run: `env -u CC -u LDFLAGS -u CFLAGS cargo test -p starlink-ast --no-fail-fast`, `cargo test -p ast-plot-demo`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc -p starlink-ast --no-deps --features attribute-strings`, and the archive check: `cargo package -p starlink-ast --allow-dirty` then the archive's `cargo test --features attribute-strings` as CI's `package` job runs it.
Expected: all pass; the grid tests still match C, now set through the layer.

- [ ] **Step 5: Commit**

```bash
git commit -am "Use attribute strings in the demo and the grid tests"
```
