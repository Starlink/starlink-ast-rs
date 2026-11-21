<!-- Survey of the C attribute-string machinery and its Rust counterparts, the factual base for docs/superpowers/plans/2026-11-21-wasm-plotter-stage1-attribute-strings.md. Line numbers are at the submodule pin of 2026-11-21. -->

# Attribute-string machinery: survey for stage 1 of the browser FITS plotter

Factual base for planning stage 1 of `docs/superpowers/specs/2026-11-21-wasm-fits-plotter-design.md`.
C paths are relative to `reference/starlink-ast/src/`, Rust paths to `crates/starlink-ast/src/` unless stated.
Line numbers are at the current submodule pin.

## A. Generic entry points (object.c)

### A.1 `astSet` / `astVSet` (object.c:3248, :4749-5090)

`astSet_` (:3248) and the public `astSetId_` (:8549) both call `astVSet( this, settings, NULL, args )`.
FrameSet overrides `VSet` (frameset.c:~11895-11965) to bracket the whole call with integrity handling (A.6); Plot inherits that override.

The splitting rules in `VSet` (:4840-5085):

1. Empty settings: nothing happens (:4845).
2. **`%s` fast path** (:4851-4877): if the whole string is `" %s "` (`sscanf( settings, " %%s %n" )` consumes all of it), the variadic argument is expanded first with `vsnprintf` and the expanded text is used, with no further printf processing.
   Otherwise the string is copied and later passed through `vsprintf( buff2, buff1, args )` (:4925), so `%%` becomes `%` and any other `%` specifier consumes an argument (undefined behavior without one).
   A Rust layer with no varargs behaves like C's `astSet( obj, "%s", text )`; the C probe must call it that way (or use `astSetC`) for the strings to be comparable.
3. **Comma rule** (:4884-4900): commas become `\n` separators *only if the string contains at least two `=`* (`eq1 = strchr('='); if( eq1 && strchr( eq1+1, '=' ) )`).
   Inside double quotes commas are left alone (a `"` toggles `quoted`).
   So `"Title=a,b"` is one assignment with value `a,b`, but `"Title=a,b,Label(1)=x"` splits into `title=a`, `b`, `label(1)=x`, and the second fails.
4. **Per-assignment normalization** (:5008-5056):
   - Before the first `=`, white space is removed and letters are lower-cased; from the `=` on, characters are copied unchanged.
   - `\r` anywhere becomes `,` (`astSetC` encodes value commas as `\r`, :4060).
   - If the first non-space character after the first `=` is `"`, it is dropped (`tq` logic), and at the end trailing white space then one closing `"` are dropped (:5050-5055).
     Only a quote at the *start* of the value is special; white space before it is kept.
   - A blank assignment (nothing left) is skipped (:5060).
   - If the assignment ends in `=`, a space is appended (:5066-5076), so `"Title="` sets Title to `" "` (`tools/oracle_fixtures/ident_probe.c` header notes the same for Ident).
5. Each assignment goes to `astSetAttrib( this, assign )`; processing stops at the first error (:5083).
6. Errors raised by `VSet` itself: `AST__ATSER` "astVSet(%s): Error formatting an attribute setting..." and "Internal buffer overflow..." (:4935-4972); neither is reachable without varargs.

Values are never trimmed by `VSet`: `" Encoding = FITS-WCS"` passes `encoding= FITS-WCS` (value with a leading space) to `SetAttrib`.
Numeric `astSscanf` patterns tolerate surrounding white space (`"name= %d %n"`); string patterns (`"name=%n%*[^\n]%n"`) keep it.

`astSetC_` (:4032): copies the value with `,` → `\r`, builds `"<name>=%*s"` and calls `astSet( this, setting, 0, newv )` (width 0, so the value is printed whole).
The string has one `=`, so no comma splitting; VSet turns `\r` back into `,`.
The attribute name is trimmed only of trailing space (`astChrLen`) before the `=` is appended; VSet then removes all white space before `=`.

### A.2 `astGetC` / `Get` / `GetAttrib` (object.c:2750, :1979, :2085)

- `Get` (:1979-2060): copies the name with all white space removed and lower-cased (`isspace`/`tolower`).
  A blank name gives `AST__BADAT` "astGet(%s): A blank attribute name was given." (:2031).
  It calls `astGetAttrib( this, buff )`, then **unless `astEscapes(-1)` is set (default off, object.c:418), strips graphical escape sequences** with `astStripEscapes` (plot.c:25659).
  There is no Rust port of `astStripEscapes` (`escapes.rs` has only the on/off flag).
- `astGetC_` (:2750-2805): calls `Get`; a NULL result becomes `""`; the text is copied into a ring of `AST__ASTGETC_MAX_STRINGS` dynamically allocated slots (`astgetc_strings`, wrapping at max-1), so a pointer stays valid for the next ~50 calls.
  A Rust port returns an owned `String`, which removes the ring.
- `GetAttrib` implementations format into a per-thread `getattrib_buff` (`sprintf( getattrib_buff, "%d", ... )`) or return a pointer to a stored string.
- Object's `GetAttrib` (:2085-2213) names: class, id, ident, usedefs (`%d`), nobject (`%d`), objsize (`%zu`), refcount (`%d`).
  Unknown: `AST__BADAT` "astGet: The %s given does not have an attribute called \"%s\"." (:2208).

### A.3 `astTest` / `TestAttrib` (object.c:4142-4200, :4249-4330)

`astTest_` removes white space, lower-cases, and calls `astTestAttrib` with the whole string: **no comma splitting and no blank-name check** (a blank name goes through and ends in BADAT from Object).
Read-only names return 0 (Object :4308).
Unknown: `AST__BADAT` "astTest: The attribute name \"%s\" is invalid for a %s." (:4316).
The result is cleared to 0 on any error.

### A.4 `astClear` / `ClearAttrib` (object.c:1054-1162, :1164-1240)

`Clear` takes a **comma-separated list of names** (no quoting), removes white space and lower-cases each, skips blank ones, and calls `astClearAttrib` per name, stopping at the first error.
Read-only: `AST__NOWRT` "astClear: Invalid attempt to clear the \"%s\" value for a %s." plus a second message "This is a read-only attribute." (:1228-1231).
Unknown: `AST__BADAT` "astClear: The attribute name \"%s\" is invalid for a %s." (:1236).
FrameSet overrides `Clear` (frameset.c:2604-2668) with the same integrity bracket as `VSet`.

### A.5 Name normalization summary

| Entry | White space | Case | Splitting |
|---|---|---|---|
| astSet/VSet | removed before first `=`; value unchanged | lower before `=` | commas only if ≥2 `=`, quotes protect |
| astSetC | trailing removed from name, then as VSet | as VSet | none (one `=`) |
| astGetC | all removed | lower | none; blank is an error |
| astTest | all removed | lower | none; blank reaches BADAT |
| astClear | all removed per element | lower | every comma |

`SetAttrib` therefore sees `name=value` with `name` lower-case and space-free (e.g. `label(1)=Right ascension`), and Get/Test/Clear see bare lower-case names (e.g. `colour(axes)`).

### A.6 FrameSet integrity bracket

`VSet` and `Clear` overrides save `integrity_frame/_lost/_method` (re-entrancy), call `RecordIntegrity` (frameset.c:8440: copies the current Frame, clears `integrity_lost`), run the parent, then `RestoreIntegrity` (:9345-9460).
`RestoreIntegrity` acts only if `integrity_lost` is set, there is more than one Frame and status is OK: it matches Domains, sets `AST__INTFLAG` on both Frames, calls `astConvert( recorded, current, "" )`, and `astRemapFrame`s the current Frame unless the Mapping is a UnitMap.
Failure: `AST__ILOST` "%s(%s): Cannot maintain %s integrity." (method is `astSet` or `astClear`), and the bracket adds "Unable to accommodate the attribute setting \"%s\"." (VSet, :11945) or "Unable to accommodate clearing the \"%s\" attribute(s)." (Clear, :2657).
`integrity_lost` is set only when a name is passed to the current Frame (frameset.c:9736, :2826).
Setting Current or Invert inside the string does `RestoreIntegrity`, change, `RecordIntegrity` (frameset.c:9614, :9686).
So one astSet string is one integrity transaction, not one per assignment; if any assignment fails, status is bad and `RestoreIntegrity` does nothing (the Frame stays changed, the Mappings untouched).

Rust analogue: `FrameSet::with_integrity` (frames/frameset.rs:1245) snapshots the current Frame, runs a closure on it, `map_from` + `simplify`, and `remap_frame`s unless UnitMap.
It returns early on a closure error without remapping, matching C.
It does not check `nframe > 1` or an `integrity_lost` flag, and it brackets one closure, so the layer needs a "transaction" form: snapshot once, apply the whole string (including Current/Invert re-snapshots), restore once, and only if a Frame attribute was actually passed down.

### A.7 `astSscanf` (memory.c:4493)

Wraps `sscanf`: zeroes every `%n` target first, removes spaces before `%n` (macOS workaround) and handles trailing white space itself.
Every `SetAttrib` arm uses `nc = 0, ( N == astSscanf( setting, fmt, ..., &nc ) ) && ( nc >= len )`, i.e. the pattern must consume the whole string.

### A.8 What `fitschan/options.rs` already ports

`crates/starlink-ast/src/fitschan/options.rs` (445 lines, module `mod options;` private in `fitschan/mod.rs:26`):

- `env_options()` (`pub(super)`): `FITSCHAN_OPTIONS` read once per thread, `"None"` ignored (C `astEnvSet`).
- `apply(fc, settings)` (`pub(super)`): VSet's comma/quote rule and `assignment()` normalization (lower-case name, `\r`→`,`, leading-quote removal, trailing `=` gets a space). No `%` expansion (documented).
- `int_value`/`double_value`/`string_value` (private): the `"name= %d %n"`, `"name= %lg %n"`, `"name=%n%*[^\n]%n"` idioms, built on `fitschan/card_value.rs` `scan_int` (:235) and `scan_double` (:225, C `strtod` via `c_strtod`).
- `abbreviates()`: `Ustrncmp(value, name, ChrLen(value))` minimum-abbreviation match (Encoding, AltAxes).
- `set_attrib()`: the complete **set** chain FitsChan → Channel → Object (all 19 writable FitsChan names, 6 read-only, 8 Channel, ID/Ident/UseDefs, Object read-only), with C's messages for BADAT, NOWRT and the Encoding/AltAxes errors.
- No Get, Test or Clear; error class name is hard-coded "FitsChan".
- Only caller: `FitsChan::new()` (`fitschan/mod.rs:577-583`). Unit tests in the same file, from a C probe of `FITSCHAN_OPTIONS`.

## B. The C classes, in chain order

Hierarchy (parent captured in each `astInit<Class>Vtab` as `parent_*attrib = object->*Attrib`):
Object; Mapping → Object (mapping.c:2641-2647); Frame → Mapping (frame.c:6215-6221); Axis → Object (axis.c:2002-2010); SkyAxis → Axis (skyaxis.c:3459-3465); SkyFrame, SpecFrame, TimeFrame, CmpFrame → Frame; DSBSpecFrame → SpecFrame (dsbspecframe.c:937-946); FrameSet → Frame but **with no `parent_*attrib`** (it never falls through to Frame); Plot → FrameSet (plot.c:19915-19921); Channel → Object (channel.c:2024-2030); FitsChan → Channel (fitschan.c:18940-18946).
There is no SpecAxis or TimeAxis: SpecFrame and TimeFrame use plain Axis objects; only `axis.c` and `skyaxis.c` init Axis vtabs.

Function locations (Set / Get / Test / Clear):

| Class | SetAttrib | GetAttrib | TestAttrib | ClearAttrib |
|---|---|---|---|---|
| Object | object.c:3382 | :2085 | :4249 | :1164 |
| Mapping | mapping.c:16368 | :1467 | :20168 | :821 |
| Frame | frame.c:10042 | :4754 | :11364 | :2151 |
| Axis | axis.c:2251 | :1680 | :2403 | :1385 |
| SkyAxis | skyaxis.c:3747 | :2536 | :3891 | :1453 |
| SkyFrame | skyframe.c:8871 | :2583 | :10641 | :1417 |
| SpecFrame | specframe.c:3577 | :1193 | :5313 | :434 |
| DSBSpecFrame | dsbspecframe.c:1253 | :382 | :1723 | :300 |
| TimeFrame | timeframe.c:4611 | :2150 | :5742 | :916 |
| CmpFrame | cmpframe.c:8436 | :3193 | :9747 | :1519 |
| FrameSet | frameset.c:9462 | :4733 | :10776 | :2699 |
| Plot | plot.c:24917 | :15500 | :25892 | :6504 |
| Channel | channel.c:3595 | :889 | :3883 | :622 |
| FitsChan | fitschan.c:27599 | :17084 (not `static`) | :34508 | :7368 |

### Common idioms (with one example each)

Set:
- Integer/boolean: `"name= %d %n"` → typed setter; booleans are ints normalized by the setter macro (`astMAKE_SET(Mapping,Invert,int,invert,(value!=0))` mapping.c:23272; `astMAKE_SET(Plot,Grid,int,grid,( value ? 1 : 0 ))` plot.c:2453; `astMAKE_SET(Frame,Digits,int,digits,( value > 1 ? value : 1 ))` frame.c:13251; MinAxes/MaxAxes through `ConsistentMinAxes/MaxAxes`, frame.c:13370/13436). Example: `"maxaxes= %d %n"` frame.c.
- Double: `"name= %lg %n"`, e.g. `"obsalt= %lg %n"` frame.c.
- String (whole rest of line, white space kept, ≥1 char): `"name=%n%*[^\n]%n"`, value at `setting + off`, e.g. `"title=%n%*[^\n]%n"`.
- Keyword (one word, surrounding space allowed): `"name= %n%*s %n"` then a code lookup, e.g. `"system= %n%*s %n"` → `astSystemCode` (frame.c), `"stdofrest=%n%*s %n"` → `StdOfRestCode` (specframe.c).
- Axis-qualified: `"name(%d)= %d %n"`, `"label(%d)=%n%*[^\n]%n"`; the axis is passed as `axis - 1` to a setter that validates it.
- Element-qualified (Plot): `"style(%20[^()])= %d %n"`, label resolved with `FullForm( GrfLabels, ... )` and `IdFind` (plot.c:13920, :19545).
- Read-only: `#define MATCH(attrib) ( nc = 0, ( 0 == astSscanf( setting, attrib "=%*[^\n]%n", &nc ) ) && ( nc >= len ) )`, then the two-message NOWRT error.
- Case-insensitive whole-word match: `astChrMatch` (e.g. SkyRefIs, FrameSet Base/Current by Domain); Rust: `mappings::helpers::chr_match_key` used by `SkySystem::parse` (frames/sky_convert.rs:104).
- Unique minimum abbreviation: Plot `FullForm` (plot.c:13920; `AST__OPT` "%s(%s): Option '%.*s' is unknown in '%.*s'." / "... is ambiguous in ..."); FitsChan uses `Ustrncmp` (first match in list order wins, no ambiguity check).

Get:
- `"%d"` for ints and booleans (stored 0/1).
- `"%.*g", AST__DBL_DIG` for doubles; `AST__DBL_DIG` is `DBL_DECIMAL_DIG` = 17 (object.h.in:438).
  Rust: `serial::native_write::format_double_g_signed(v, 17)` reproduces `%.*g` verbatim (`format_double_g` turns -0 into "0", which is `WriteDouble`'s rule, not `sprintf`'s).
- Epoch/Equinox: `astFmtDecimalYr( year < 1984 ? palEpb(mjd) : palEpj(mjd), AST__DBL_DIG )` (frame.c:4925, skyframe.c:2687), which is `"%#.*g"` then strip trailing zeros keeping one after the point (frame.c:4207-4275).
  Rust has only `fmt_decimal_yr` with 9 digits, private to frames/skyframe.rs:3304, and it differs from C for exponent forms; a general port is needed.
- Enum-like: a fixed string per code (`StdOfRestString`, `TimeScaleString`, `astSystemString`; Plot `xlbling[]`, `xtgaptype[]`, edge names; SkyRefIs `POLE_STRING` etc.).
- Angles: `astFormat` of an FK5 SkyFrame with `format(2)=dms.2` (ObsLat/ObsLon, prefixed `N`/`S`, `E`/`W`, frame.c:5070-5110); SkyRef/SkyRefP `"%s, %s"` of `astFormat( this, axis, ...)`; SpecFrame RefRA/RefDec via a plain FK5 SkyFrame.

### Common error templates

| Condition | Code | Template |
|---|---|---|
| unknown, set | BADAT | `astSet: The attribute setting "%s" is invalid for a %s.` (object.c:3481) |
| unknown, get | BADAT | `astGet: The %s given does not have an attribute called "%s".` (:2208) |
| unknown, test | BADAT | `astTest: The attribute name "%s" is invalid for a %s.` (:4316) |
| unknown, clear | BADAT | `astClear: The attribute name "%s" is invalid for a %s.` (:1236) |
| blank get | BADAT | `astGet(%s): A blank attribute name was given.` (:2031) |
| read-only set | NOWRT | `astSet: The setting "%s" is invalid for a %s.` + `This is a read-only attribute.` |
| read-only clear | NOWRT | `astClear: Invalid attempt to clear the "%s" value for a %s.` + `This is a read-only attribute.` |
| read-only test | — | returns 0 |
| bad value | ATTIN | class-specific, mostly `astSetAttrib(%s): Invalid <thing> "%s" ...` |
| bad axis (Frame) | AXIIN | `%s(%s): Axis index (%d) invalid - it should be in the range 1 to %d.` (frame.c `ValidateAxis` ~:12040), method e.g. `astSetLabel`, `astSetDigits(axis)`, `astSet`, `astGet` |
| bad axis (Plot) | AXIIN | `%s(%s): Index (%d) is invalid for attribute <Attr> - it should be in the range 1 to %d.` (plot.c MAKE_* macros, e.g. :1222) |
| bad option | OPT | Plot FullForm (above); SkyFrame `astSet(%s): option '%s' is unknown in '%s'.` (SkyRefIs) |

The `%s` class is always `astGetClass( this )` of the object whose function raises it, so an error raised after delegation names the delegate: an unknown name set on a Plot whose current Frame is a SkyFrame reports "... is invalid for a SkyFrame.".
The Rust typed setters' AXIIN messages differ (`"axis index {axis} is out of range 1..={naxes}"`, frames/frame_attrs.rs:1412, frames/basic.rs:324), so the layer must validate and word AXIIN errors itself, with C's method names.

### B.1 Object (object.c)

- Falls through to: nothing (end of chain, raises BADAT).
- Attributes (7): string ID, Ident (`"id=%n%*[^\n]%n"`); int UseDefs (`"usedefs= %d %n"`, stored `value?1:0`, object.c:5305); read-only Class, Nobject, ObjSize (`%zu`), RefCount.
- Quirks: the design makes Nobject, ObjSize, RefCount an error in Rust (no value) and a named exception in the comparison. Class must give the C class name (`"SkyFrame"`, `"Plot"`, ...; `"Axis"`/`"SkyAxis"` for `Class(1)` on a Frame, see B.3).

### B.2 Mapping (mapping.c)

- Falls through to Object.
- Attributes (8): int Invert, Report; read-only Nin, Nout, IsLinear, IsSimple, TranForward, TranInverse (Get `%d`).
- Quirk: every Frame is a Mapping in C, so `Invert=1`, `Report=1`, `Nin`, `Nout`, `IsLinear`, `TranForward` are valid on a plain Frame, SkyFrame, CmpFrame etc.
  Rust Frames are not `Mapping`s and store no Mapping flags (`FrameDumpSource::dump_mapping_part` writes default `MappingFlags`, frames/frame_attrs.rs:169-181), so Invert/Report on a non-FrameSet Frame have no storage and the dump would not show them.

### B.3 Frame (frame.c)

- Falls through to Mapping; additionally delegates per-axis names to Axis objects, and via `astPrimaryFrame` to primary Frames.
- Attributes (26 names): int Digits (also `Digits(axis)`, per-axis through the Axis), MatchEnd, MaxAxes, MinAxes, Permute, PreserveAxes; double ObsAlt, Dtai, Dut1; string Domain, Title; Epoch (string → `astReadDateTime`); keyword System, AlignSystem (`astSystemCode`/`astSystemString`, overridden per class); ObsLat/ObsLon (sexagesimal through an FK5 SkyFrame `astUnformat`, optional N/S/E/W prefix); axis-indexed Direction, Top, Bottom, Format, Label, Symbol, Unit; read-only Naxes, NormUnit(axis), InternalUnit(axis).
  Axis-indexed: 10 names (Digits, Direction, Top, Bottom, Format, Label, Symbol, Unit, NormUnit, InternalUnit).
- Set errors: Epoch `astSetAttrib(%s): Invalid epoch value "%s" given for coordinate system.`; System/AlignSystem `astSetAttrib(%s): Invalid System description "%s".`; ObsLat `astSetAttrib(%s): Invalid value for ObsLat (observers latitude) "%s".` (all ATTIN).
- **Generic axis path** (Set frame.c ~:10470-10600; Get ~:5180-5260; Test and Clear alike, Clear :2380-2480): a name not matched above but of the form `name(n)` (`"%*[^()=]%n(%d)%n=%*[^\n]%n"`) is validated (`astValidateAxis( this, axis-1, 1, "astSet" )`), then:
  1. sent to the Axis object as `name=value` with error reporting off;
  2. on `AST__BADAT` only, sent to the primary Frame of that axis as `name(paxis)=value` if the primary Frame is not this Frame;
  3. failing that, sent to the primary Frame **without the index** (`name=value`);
  4. failing all, re-sent to the Axis with reporting on, so the user sees the Axis error ("... is invalid for a Axis.").
  Consequences: `Equinox(1)=J2000` on a SkyFrame sets Equinox; `Ident(2)=x` sets the Ident of axis 2's Axis object (written in its `Ax2` dump); `IsLatitude(1)`/`CentreZero(1)` reach the SkyAxis.
- **One-axis retry** (Set :10606-10630, Get, Test, Clear alike): an unrecognised name with no `(` on a 1-axis Frame is retried with `(1)` appended.
- Quirks: ObsLat/ObsLon use `nc >= 7` rather than `nc >= len`; `Unit(axis)` arm uses `&` not `&&` (harmless); Digits on its own sets the Frame's Digits, `Digits(n)` the Axis's; `Get Digits(n)` gives the Axis value if set else the Frame's.

### B.4 Axis (axis.c) and B.5 SkyAxis (skyaxis.c)

- Axis falls through to Object; SkyAxis to Axis.
- Axis (10): int Digits, Direction; double Top, Bottom; string Format, Label, Symbol, Unit; read-only NormUnit, InternalUnit. Default Format `"%%1.%dG"` of the axis Digits (axis.c ~:1830).
- SkyAxis (3): int AsTime, IsLatitude, CentreZero.
- Reached from the Frame only through the generic path (Frame handles `label(n)` etc. itself), so in practice the Axis chain serves Object names, IsLatitude/CentreZero, and the error text for unknown axis names.

### B.6 SkyFrame (skyframe.c)

- Falls through to Frame. System/AlignSystem reach it through Frame's arms and SkyFrame's `SystemCode`/`SystemString` overrides.
- Attributes (13 own): `AsTime(axis)` (int, axis-indexed, handled here), Equinox (`astReadDateTime`, get via FmtDecimalYr), NegLon (int), SkyTol (double), Projection (string), SkyRef and SkyRefP (two-value `astUnformat( this, 0, ...)` `,` `astUnformat( this, 1, ...)` and per-axis `skyref(%d)= %lg %n` in radians), SkyRefIs (keyword `Pole`/`Origin`/`Ignored` by `astChrMatch`), AlignOffset (int); read-only IsLatAxis(axis), IsLonAxis(axis), LatAxis, LonAxis.
- Errors: Equinox ATTIN `astSetAttrib(%s): Invalid equinox value "%s" given for sky coordinate system.`; SkyRef/SkyRefP `AST__BADOC` `astSetAttrib(%s): Invalid axis values string "%.*s" given for SkyRef attribute.`; SkyRefIs `AST__OPT` `astSet(%s): option '%s' is unknown in '%s'.`
- Quirks: clearing `SkyRef` clears both axes; get `SkyRef` formats with the Frame's own Format (so it follows Format/Digits); the SCUBA-2 hot path (Epoch, SkyRefIs setters) must stay O(1), which a string layer over the typed setters does not affect.

### B.7 SpecFrame (specframe.c)

- Falls through to Frame.
- First arm (:3672-3692): names `direction=`, `bottom=`, `top=`, `format=`, `label=`, `symbol=`, `unit=` get `(1)` appended and go to the **parent** (Frame) directly; Get/Test/Clear do the same.
- Attributes (12 own): AlignStdOfRest, StdOfRest, SourceVRF (keyword, `StdOfRestCode`); SourceSys (keyword through `SystemCode`, no error on a bad value: it sets `AST__BADSYSTEM`); RefRA, RefDec (sexagesimal through FK5 SkyFrame, `nc >= 6/7`); GeoLat, GeoLon (aliases: the name is rewritten to `obslat`/`obslon` and re-set); AlignSpecOffset (int); RestFreq (`"restfreq= %lg %n"` is **GHz**, `×1e9`; `"restfreq= %lg %n%*s %n"` takes a unit through `astUnitMapper` to Hz, m (wavelength) or J (energy)); SourceVel (km/s for VREL/VRADIO/VOPTICAL source systems, ×1e3); SpecOrigin (in the Frame's current Unit, or with an explicit unit, through `ToUnits`).
- Get: RestFreq `%.*g` of GHz; RefRA/RefDec through the FK5 SkyFrame's `astFormat`; GeoLat/GeoLon return Get of obslat/obslon.
- Errors (ATTIN): `Invalid standard of rest description "%s".`, `Invalid reference right ascension "%s".`, `Invalid reference declination "%s".`, `Invalid rest wavelength "%g %s" supplied.`, `Invalid rest energy ...`, `Rest frequency given in an unsupported system of units "%g %s".` (all `astSetAttrib(%s): ...`).

### B.8 DSBSpecFrame (dsbspecframe.c)

- Falls through to SpecFrame.
- Attributes (5): DSBCentre and IF (double in GHz, or with units, `"dsbcentre= %lg %n%*s %n"`), SideBand (keyword `usb`, `lsb`, `lo`, `observed`, `image` via `astChrMatch`), AlignSideBand (int); read-only ImagFreq.
- Errors (ATTIN): `astSetAttrib(%s): The setting "%s" is invalid for a %s.`, `Value supplied for attribute "DSBCentre" (%s) uses units which are inappropriate for the current spectral system (%s).`, `Intermediate frequency given in an inappropriate system of units "%g %s".`
- Quirk: ClearAttrib lists `imagfreq` as read-only (NOWRT).

### B.9 TimeFrame (timeframe.c)

- Falls through to Frame; same 1-axis shortcut arm as SpecFrame.
- Attributes (6): TimeScale, AlignTimeScale (keyword, ATTIN `Invalid time scale description "%s".`), ClockLat, ClockLon (aliases of ObsLat/ObsLon), LTOffset (double, hours), TimeOrigin (`"timeorigin= %lg %n"` in current units, `"... %lg %n%*s %n"` with a unit, or `"timeorigin=%n%*[^\n]%n"` a date string; ATTIN `Invalid TimeOrigin value "%s" given.`); Get TimeOrigin via `GetTimeOriginCur` in `%.*g`.

### B.10 CmpFrame (cmpframe.c)

- Owns no attributes (`if( 0 ) {}` placeholder).
- **Set** (:8436-8636): with reporting off, try the parent (Frame) first; on any error clear it, then:
  - axis-qualified (`"%[^(=](%d)= %n%*s %n"`): primary Frame of that axis gets `name(paxis)=value`, then on failure `name=value`;
  - unqualified: **every axis's primary Frame** gets the setting (a 2-axis SkyFrame component gets it twice), success if any accepts it.
  - If nothing accepted: `AST__BADAT` `astSet: The attribute setting "%s" is invalid for the given %s.` — **every underlying error, including ATTIN for a bad value, is replaced by this.**
  - Note the `%*s` makes axis-qualified values with embedded spaces fall into the unqualified branch.
- **Get** (:3193-3386): axis-qualified names skip the parent and go straight to the primary Frame (`name(paxis)`, then `name`); unqualified names try the parent, then each axis's primary Frame until one answers. Failure: `astGet: The %s given does not have an attribute called "%s".`
  This is the string-vs-function divergence recorded in `PLAN.md:71` (`Label(2)` on a plain-Frame component is `"Axis 1"` by string).
- Test and Clear follow Get's shape.
- Quirk: `System=FK5` on a CmpFrame first fails in Frame (CmpFrame's SystemCode knows only `Compound`), then is set on each component that accepts it; `Epoch`, `ObsLat` and other Frame attributes succeed in the parent and are spread by CmpFrame's own setters.

### B.11 FrameSet (frameset.c)

- Does **not** fall through to Frame/Mapping/Object; it handles the Object and Mapping names it wants itself and hands **everything else to the current Frame** (`ForceCopy` then `astSetAttrib( fr, setting )`, `integrity_lost = 1`).
- Attributes (5 own): Base, Current (int index, or a word: `AST__CURRENT`/`Current`, `AST__BASE`/`Base`, or a Frame Domain by `astChrMatch`; ATTIN `astSetAttrib(%s): Invalid index value for Base Frame "%s".`), Variant (string); read-only AllVariants, Nframe. It also handles ID, Ident, Invert (with Restore/Record integrity), Report, and read-only Class, Nin, Nout, Nobject, RefCount, TranForward, TranInverse.
- Quirks: `Base=AST__BASE` and `Current=Current` are accepted no-ops; **UseDefs, ObjSize, IsLinear, IsSimple go to the current Frame** (Rust's `FrameSet::set_use_defs` already does this, frames/frameset.rs:1980-1987); unknown-name errors name the current Frame's class.
- Get/Test of FrameSet names come before the current Frame; Get Class gives "FrameSet" (or "Plot").

### B.12 Plot (plot.c)

- Falls through to FrameSet (so Frame names reach the current Frame with integrity).
- No read-only attributes.
- Attributes (39 names): double Tol, TitleGap; int Grid, TickAll, ForceExterior, Invisible, Border, ClipOp, Clip, Grf, DrawTitle, Escape; axis-indexed (with an unindexed form that sets every axis / gets axis 1) DrawAxes, Abbrev, Gap, LogGap, NumLabGap, TextLabGap, LabelUp, LogPlot, LogTicks, LogLabel, NumLab, MinTick, TextLab, LabelUnits, MajTickLen, MinTickLen; axis-indexed only Edge (keyword `left right top bottom` by FullForm), LabelAt, Centre; element-indexed Style, Font, Colour, Color (alias), Width, Size (`"style(%20[^()])= %d %n"`, element by `FullForm( GrfLabels, ... )` + `IdFind`, 25 labels in `GrfLabels`, plot.c:1717: Border Curves Title Markers Strings Axis1-3 NumLab1-3 TextLab1-3 Ticks1-3 Grid1-3 Axes NumLab TextLab Grid Ticks; an unindexed set applies to every one of the `AST__NPID` elements; an unindexed get reads Border for Style and Width, TextLabs for Font, Colour and Size); keyword Labelling (`exterior interior`), TextGapType (`box plot`).
  19 axis-indexed names, 6 element-indexed.
- **"Used" values on Get**: Grid, Border, Labelling, Edge, LabelAt, Centre, Gap, LogGap, MinTick, TextLab, LabelUnits, LogTicks, LogLabel, MajTickLen are read through `GetUsed<Attr>` (MAKE_GET3, plot.c:1216-1250): the set value if set, else **`astSetInk(0); astGrid(this); astClearInk()`** and the value the grid chose.
  A get therefore needs a graphics backend; Rust `Plot::used_values(&mut self, grf: &mut dyn Grf)` (plot/grid.rs:670) provides the values but needs a `Grf`, and the four operations as designed take none.
  `tools/oracle_fixtures/plot_grid_values_probe.c` already records `astGetC` of these through grf_log.
- Quirks: Set uses `FullForm( GrfLabels, label, "Style", ...)` so the error text says `... is unknown in 'Style'.` while Get/Test/Clear quote the attribute name; labels over 20 characters fail the pattern and fall through to FrameSet → current Frame → BADAT naming the Frame class; `Labelling=bad` reports OPT then calls the setter with -1 under bad status; `Boxp_freeze` is set during GetAttrib (plot.c:16215); the Rust Plot has no `Grf` attribute (it passes the backend per call).

### B.13 Channel (channel.c)

- Falls through to Object.
- Attributes (8): int Comment, Full, Indent, ReportLevel, Skip, Strict; string SinkFile, SourceFile. No read-only.
- Quirk: setting SinkFile/SourceFile opens the file (FitsChan reads the cards from SourceFile when it is set).

### B.14 FitsChan (fitschan.c)

- Falls through to Channel.
- Attributes (25): int Card, FitsDigits, FitsRounding, CDMatrix, DefB1950, TabOK, ForceTab, IgnoreBadAlt, CarLin, SipReplace, PolyTan, SipOK, Iwc, Clean; double FitsTol; string FitsAxisOrder, Warnings; keyword Encoding (Ustrncmp abbreviation over NATIVE FITS-PC FITS-WCS FITS-IRAF FITS-AIPS FITS-AIPS++ FITS-CLASS DSS, `-` or `_`; BADAT `astSet(FitsChan): Unknown encoding system '%s' requested for a FitsChan.`), AltAxes (BADAT `Illegal value '%s' supplied for attribute AltAxes.`); read-only NCard, CardType, CardComm, CardName, NKey, AllWarnings.
- Quirks: Clear treats only NCard and AllWarnings as read-only (fitschan.c:7509); clearing NKey, CardType, CardComm or CardName falls through to Object's BADAT.
  Reading attributes act on the cards: Card moves the cursor, Encoding's default is guessed from the cards, CardType/CardComm/CardName/NCard/NKey read them (Rust getters take `&mut self`: `card`, `ncard`, `nkey`, `card_type`, `encoding`, fitschan/mod.rs:690-799, :2051); Clean and Warnings change what a read does; SourceFile reads cards on set.

## C. The Rust side

### C.1 Where typed accessors live, and test/clear gaps

- Object names: `Frame` trait (`ident`/`set_ident`/`test_ident`/`clear_ident`, `id`..., `use_defs`..., frames/mod.rs:~1000-1100); `Mapping` trait for Mappings (mappings/mod.rs); FitsChan inherent `set_id`, `set_ident` etc.; FitsChan UseDefs is a private field (`use_defs: Option<bool>`, fitschan/mod.rs:520) with no typed accessor.
- Mapping names: `Mapping` trait has `nin`, `nout`, `is_inverted`, `set_invert`, `test_invert`, `clear_invert`, `report`, `set_report`, `test_report`, `clear_report`, `has_forward`, `has_inverse`, `is_linear`; implemented by FrameSet and Plot (frames/frameset.rs:2528, plot/forward.rs:560) but **not by plain Frames** (no storage, B.2).
- Frame names: `Frame` trait (frames/mod.rs:415-2800) has get/set/test/clear for Title, Domain, Epoch (`set_epoch_mjd`), Digits, ObsLat/Lon/Alt, Dtai, Dut1, MinAxes, MaxAxes, MatchEnd, PreserveAxes, Permute, Label, Symbol, Unit, Format (`format_str`), axis Digits, Direction, Top, Bottom; `naxes`, `norm_unit`, `internal_unit`; `system_string` only for System.
  System/AlignSystem set/test/clear are inherent per class (BasicFrame, SkyFrame, SpecFrame, TimeFrame have them; **CmpFrame has none**, frames/cmpframe.rs public API is `frame1`, `frame2`, `frame1_mut`, `frame2_mut`, `perm`, `set_domain`, `set_ident`, `set_title`, `test_title`).
  Setter parameter types are narrower than C's int (`set_min_axes(usize)`, `set_match_end(bool)`), so the layer must reproduce C's int handling (Digits clamps to ≥1; MinAxes/MaxAxes through ConsistentMin/MaxAxes; booleans `!= 0`).
- SkyFrame: frames/skyframe.rs inherent set/test/clear for AlignOffset, AlignSystem, AsTime, Equinox (`set_equinox_mjd/year`), NegLon, Projection, SkyRef, SkyRefIs, SkyRefP, SkyTol, System; getters `is_lat_axis`, `is_lon_axis`, `lat_axis`, `lon_axis`.
  **Gap: no API for the SkyAxis IsLatitude and CentreZero** of a SkyFrame axis (fields exist on `SkyAxis`, frames/skyaxis.rs:21-34).
- SpecFrame (frames/specframe.rs): complete set/test/clear for AlignSpecOffset, AlignStdOfRest, AlignSystem, RefDec, RefRA, RestFreq, SourceSys, SourceVel (also `_cur`), SourceVRF, SpecOrigin (also `_cur`), StdOfRest, System. Units differ from C's strings (Hz, m/s, radians), so the layer converts.
- DSBSpecFrame (frames/dsbspecframe.rs): DSBCentre (`set_dsb_centre`, `_cur`), IF (`set_if_ghz`, `set_ifr`, `test_if`, `clear_if`), SideBand, AlignSideBand, `imag_freq`; the rest through `spec()`/`spec_mut()`.
- TimeFrame (frames/timeframe.rs): complete for AlignSystem, AlignTimeScale, LTOffset, System, TimeOrigin (`_cur`), TimeScale.
- FrameSet (frames/frameset.rs): Base, Current (`set_base_by_domain`, `set_current_by_domain`), Variant with set/test/clear; Nframe, AllVariants getters.
- Plot (plot/attrs.rs, plot/elements.rs): every attribute has get/set/test/clear (macros `flag_attr_rest!`, `axis_attr_rest!`, `style_rest!`), Rust names differ (`abbreviate`, `tick_gap`, `tick_centre`); element groups resolved for every accessor (plot/elements.rs header).
  **Gap: no Grf attribute.** Booleans are `bool` where C stores ints (LogPlot keeps an `i32`).
- Channel/FitsChan: complete set/test/clear for every writable attribute (fitschan/*.rs, channel/mod.rs `ChannelConfig`); getters for the read-only ones.
- Missing helpers: `astStripEscapes`; a general `astFmtDecimalYr( year, digits )`; a class-name method on `dyn Frame` (only the private `Serializable::class_name`, reached by downcast chains like `FrameToNode for dyn Frame`, serial/mod.rs:585).

### C.2 Axis objects

- `frames/axis.rs`: `pub(crate) struct Axis` (Option fields label, symbol, unit, digits, format, direction, top, bottom) and `pub(crate) enum FrameAxis { Plain(Axis), Sky(SkyAxis) }` with `axis()`/`axis_mut()`.
- BasicFrame: `axes: Vec<FrameAxis>` (frames/basic.rs:102), private `axis_object(&self, axis) -> AstResult<&FrameAxis>` (:309), `pub(crate) set_axis_object` (:290).
- SkyFrame: private `axes: [FrameAxis; 2]` in physical order with `perm: [usize; 2]` (frames/skyframe.rs:141-146).
- SpecFrame, TimeFrame: private `axis: super::axis::Axis` (plain, not `FrameAxis`).
- No trait method returns an Axis object; the only hidden per-axis hooks are `axis_object_top`/`axis_object_bottom` (frames/mod.rs:1518, :1540). `get_axis(axis) -> AxisKind` says only which kind.
  The generic axis path therefore needs a new hidden hook (e.g. an `&mut` Axis-attribute handle per axis), or per-class code.

### C.3 FrameSet integrity, CmpFrame components, primary Frames

- Integrity: `FrameSet::with_integrity<F: FnOnce(&mut dyn Frame) -> AstResult<()>>(&mut self, f)` (frames/frameset.rs:1245); the `frameset_attr!` macro (:1832-1870) routes every `Frame`-trait setter through it.
  Current Frame internally: `self.frames[self.current_idx]` (`Vec<DynFrame>`); `frame_mut(FrameRef::Current)` is the integrity escape hatch.
- CmpFrame: `frame1_mut()`/`frame2_mut()` (reset the cached default Domain), `pub(crate) frames_mut()`, private `component_axis(axis)` (frames/cmpframe.rs:181-262).
- `Frame::primary_frame(&self, axis) -> (&dyn Frame, usize)` (0-based, frames/mod.rs:1782; CmpFrame :500, FrameSet :2001). **There is no `primary_frame_mut`**, which the Frame and CmpFrame set/clear paths need.

### C.4 Where the chain can hang

- `Frame` trait already carries `#[doc(hidden)]` crate-facing methods (`frame_without_regions`, `axis_object_top`, `frame_flags`, `set_frame_flags`, `axis_perm`, `axis_overlay_source`, `apply_axis_overlay`, `overlay_system_from`, `normal_points`); `Region` has `reg_trace`, `reg_mesh`, `bnd_base_mesh` etc. (regions/mod.rs:337-929); `Mapping` has `invert_orientation` and others.
  Hidden trait methods with a default that returns the "not yet supported" error give dynamic dispatch from `dyn Frame` (FrameSet's `Vec<DynFrame>`, CmpFrame's components) to the concrete class, and leave Regions, FluxFrame, SpecFluxFrame uncovered by default.
- Parent-class calls: each class's method calls the parent's as a free function on its own data (e.g. `frame_set_attrib(self, setting)` for Frame, `mapping_set_attrib`, `object_set_attrib`), since Rust has no vtab parent pointer.
- FitsChan is not a Frame; `pub trait Channel` (channel/mod.rs:358, implemented by NativeChannel, MocChan, FitsChan) could host the same hidden methods, or FitsChan gets inherent/crate-private ones.
- Plot implements `Frame` and `Mapping` by forwarding (plot/forward.rs:16, :560).
- Cargo features today: `simd-eval`, `tracing`, `call-counts` (crates/starlink-ast/Cargo.toml:73-88); the new opt-in feature goes beside them.

### C.5 Formatting helpers for Get

- `%.*g`: `serial::native_write::format_double_g_signed(value, 17)` (verbatim, -0 kept) and `format_double_g` (WriteDouble's rule); `DBL_DIG = 17` (serial/native_write.rs:15, :38). `frames/axis_format.rs:235` `sprintf_double(fmt, value)` is a general C `sprintf` for one double.
- `%d`: `format!("{v}")`.
- FmtDecimalYr: only the private 9-digit `fmt_decimal_yr` (frames/skyframe.rs:3304).
- Date parsing: `frames/date_time.rs:49` `read_date_time` (C `astReadDateTime`).
- Keyword parsing: `SkySystem::parse` etc. (frames/sky_convert.rs:104, specframe.rs:74/:251, timeframe.rs:53/:138, dsbspecframe.rs:47) with `chr_match_key`.
- Sexagesimal: `Frame::unformat` / `format` on a SkyFrame.

## D. Probe infrastructure and existing tests

- Recipe: `docs/issues/c-oracle-probe-recipe.md` (link against the repo's cmake build, DT_RPATH, beware conda libast 9.3.1). Plot probes build from `reference/starlink-ast/build` configured with `-DCMAKE_C_FLAGS=-ffp-contract=off`, `-O3`, force-loading `libast_err.a` and the grf stub libraries (`plot_attrs_probe.c:20-35`); drawing probes compile `src/grf_log.c` in (`grf_log_probe.c:13-25`, shared helpers `plot_draw_probe.h`).
- Convention: `tools/oracle_fixtures/<name>_probe.c` prints Rust source to `crates/starlink-ast/tests/<name>_oracle/cases.rs` (header "Generated by tools/oracle_fixtures/<probe>.c against the C library at the submodule pin. Do not edit."; exempt from `test_source_headers`), consumed by `tests/test_<name>.rs` in an area program.
- Reusable probes and their oracles:
  - `frame_attrs_probe.c` → `tests/frame_oracle/cases.rs` (plain Frame, settings strings, value + astTest + dump; `tests/test_frame_attrs.rs`).
  - `frame_axis_attrs_probe.c` → `frame_axis_oracle` (per-axis attributes through the typed C functions, every Frame class; `test_frame_axis_attrs.rs`, which has its own `apply` parser at :92).
  - `frame_class_attrs_probe.c` → `frame_class_oracle` (`test_frame_class_attrs.rs`, `apply` at :98).
  - `class_attrs_probe.c` → `class_attrs_oracle` (TimeFrame, SpecFrame, DSBSpecFrame, FluxFrame with C-syntax settings in C units; `test_class_attrs.rs`, `apply_spec` at :70).
  - `skyframe_attrs_probe.c` → `skyframe_attrs_oracle` (`test_skyframe_attrs.rs`).
  - `plot_attrs_probe.c` → `plot_attrs_oracle` (Plot's own attributes via protected accessors; `test_plot.rs`).
  - `plot_grid_values_probe.c` → `plot_grid_values_oracle` (`astGetC` of the used-value attributes through grf_log).
  - `integrity_probe.c` → `integrity_oracle` (astSetC through a FrameSet, every Frame class, AST__ILOST cases).
  - `frameset_current_probe.c`, `ident_probe.c`, `object_id_probe.c`, `use_defs_probe.c`, `sky_use_defs_probe.c`, `spec_use_defs_probe.c`, `min_max_axes_probe.c`, `time_scale_set_probe.c`, `cmp_match_end_probe.c`, `escapes_probe.c`, `attr_test_clear_probe.c` (Mappings).
  - FITSCHAN_OPTIONS cases in `fitschan/options.rs` tests.
- The ad-hoc parsers to retire: `crates/ast-plot-demo/src/settings.rs` (`apply_settings` :57), `crates/starlink-ast/tests/common/plot.rs` (`set_attr` :76, `apply_settings` :181), plus the per-test `apply` helpers above; `PLAN.md:309` is the row to close and `PLAN.md:71` records the CmpFrame string/function split.
- A new probe can reuse `plot_draw_probe.h` (Plots from headers, grf_log) and the `rstr`/`flit` printers, apply strings with `astSet( obj, "%s", text )`, and record per case `astGetC`, `astTest`, the Native dump (`astShow`-style through a Channel to a buffer, as `frame_attrs_probe.c` does), and the error status and message stack (via `astSetPutErr` or `astStatus` after `astClearStatus`).

## E. Size and suggested split

### E.1 Count

Distinct attribute names (Color counted, aliases GeoLat/GeoLon/ClockLat/ClockLon counted):

| Class | Own names | Read-only | Axis-indexed | Element-indexed |
|---|---|---|---|---|
| Object | 7 | 4 | | |
| Mapping | 8 | 6 | | |
| Frame | 26 | 3 | 10 | |
| Axis | (10, same names as Frame) | 2 | | |
| SkyAxis | 3 | | | |
| SkyFrame | 12 (+AsTime shared) | 4 | 5 | |
| SpecFrame | 12 | | | |
| DSBSpecFrame | 5 | 1 | | |
| TimeFrame | 6 | | | |
| CmpFrame | 0 | | | |
| FrameSet | 5 | 2 | | |
| Plot | 39 | 0 | 19 | 6 |
| Channel | 8 | | | |
| FitsChan | 25 | 6 | | |
| **Total** | **≈156** | ≈28 | | |

With four operations and the indexed forms, that is roughly 600 match arms, plus the delegation logic of Frame (generic axis path, one-axis retry), CmpFrame and FrameSet.

### E.2 Suggested tasks (each testable against one C probe)

1. **Foundation + Object + Mapping.** Shared module: VSet splitter and Get/Test/Clear normalization (moved from `options.rs`), `astSscanf`-idiom matchers, `astChrMatch`/FullForm helpers, `%d`/`%.*g`/FmtDecimalYr formatters, `astStripEscapes`, C error builders per op; the hidden trait methods with the "not yet supported" default and a class-name hook; the Cargo feature and the four public operations; Object and Mapping chains (decide Frame Invert/Report storage). New probe skeleton (settings string → value/test/dump/error), first cases on a plain Frame for Object/Mapping names and splitting edge cases. (~25 names + infrastructure.)
2. **Frame + Axis + SkyAxis on BasicFrame.** 26 Frame names, Axis/SkyAxis chains, generic axis path with primary-Frame fallback, one-axis retry, Epoch/ObsLat/ObsLon parsing and formatting, AXIIN wording; an Axis-object hook. Check against the new probe and `frame_oracle`. (~40.)
3. **SkyFrame.** 13 names plus System/AlignSystem codes, SkyRef/SkyRefP pairs, SkyRefIs, IsLatitude/CentreZero gap. Check with `skyframe_attrs_oracle` cases re-expressed as strings. (~16.)
4. **SpecFrame + DSBSpecFrame.** 17 names, 1-axis shortcut arms, GHz/km/s defaults and unit parsing through the units module, aliases. Check with `class_attrs_oracle`. (~17, heavy on units.)
5. **TimeFrame + CmpFrame.** TimeFrame's 6 (TimeOrigin forms) and CmpFrame's delegation (parent-first set, error replacement, every-axis spreading, `primary_frame_mut`), including `System(1)` on a CmpFrame. (~6 + delegation.)
6. **FrameSet.** Own 5 names and re-handled Object/Mapping names, current-Frame delegation, integrity transaction over a whole string/clear list with Current/Invert re-snapshots and C's "Unable to accommodate" errors. Check with the probe and `integrity_oracle`. (~16 + integrity.)
7. **Plot.** 39 names, element labels (FullForm + IdFind groups), used-value gets through a Grf (decide the API), Grf attribute. Check with the probe linked to grf_log, `plot_attrs_oracle` and `plot_grid_values_oracle`. (~39, largest.)
8. **Channel + FitsChan.** Get/Test/Clear for 33 names on top of the existing set path, read-on-access behaviour, replacing `options.rs` and keeping `FITSCHAN_OPTIONS` working. (~33.)
9. **Integration.** Replace `ast-plot-demo/src/settings.rs`, `tests/common/plot.rs` and the per-test `apply` helpers; amend CLAUDE.md ("What NOT to do" row on string-typed access), CHANGELOG `Unreleased`, PLAN.md rows 71 and 309; docs and `#[doc(alias)]` for the new public items. (Could be folded into tasks 7 and 8.)

Dependency order: 1 → 2 → {3, 4, 5-TimeFrame} → 5-CmpFrame (needs leaf Frames) → 6 → 7; 8 depends only on 1 and can run in parallel with 2-7; 9 last.
