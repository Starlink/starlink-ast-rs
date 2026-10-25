# Frame base C-vs-Rust port — design

Date: 2026-06-11
Scope: item 7 of `docs/issues/frame-mapping-c-port-gaps.md` (the "Frame base group").
Branch: `tickets/DM-54793`.

## 1. Goal and guiding rules

Port the remaining `frame.c` / `axis.c` functionality that the audit
(section 6 of the gaps doc) flagged as missing or wrong:

- `astConvert` / `ConvertX` (currently a per-class stub set).
- The `astMatch` matchsub cast-retry (`default_match` ignores `match_sub`).
- printf-style `Format` with the `Digits` attribute, and the C
  `Unformat` whitespace/error contract.
- The missing geometry / axis method family: `Offset2`, `AxAngle`,
  `AxDistance`, `AxOffset`, `AxNorm`, `Resolve`, `ResolvePoints`,
  `Intersect`, `NormBox`, `MatchAxes` / `MatchAxesX`, `IsUnitFrame`.
- `PrimaryFrame` as a first-class Frame method, unwinding the two
  private re-implementations in `fitschan/fset_to_store/`.
- `ActiveUnit` unit adaptation in `SubFrame` / `Overlay`.

The sub-items "2-D signed `angle`" and "`offset` BAD semantics" are
already FIXED (gaps-doc section 6, via the shared `cartesian_angle`
helper and the coincident-point branch); they are out of scope here.

Guiding rules (repo conventions, reaffirmed by the user this session):

- **Port the C logic directly; never paper over a gap with a heuristic
  the C does not have.** Where C uses a tolerance or heuristic, mirror
  it and cite the C source line. Do not substitute a "compute-then-check"
  shortcut. If a faithful port hits something genuinely ambiguous, stop
  and ask rather than guess.
- Mirror C control flow line-for-line first; the idiomatic-Rust pass
  comes after the C oracle is green.
- 1-based axis indices throughout the public API.
- `BAD_VALUE` (= `-f64::MAX`) on output; `NaN` accepted on input only.
- `cargo fmt` + `cargo clippy --workspace -- -D warnings` +
  `#![warn(missing_docs)]` clean before every commit.
- Deferred pieces get explicit gaps-doc / quirks-doc entries, never
  silent omission.

## 2. Key structural finding (drives section 3)

In the C library there are **no per-class `convert` overrides**.
`astConvertX` (frame.c:3028) is the single generic driver for *all*
frame alignment; every class-specific behavior (SkyFrame system
conversion, SpecFrame SOR chains, CmpFrame partitioning) lives entirely
in that class's `Match` / `SubFrame`. The public `astConvert` is just a
wrapper that swaps its first two args and calls `astConvertX` so the
method can be overridden on the basis of *either* frame's class — but no
Frame subclass actually overrides it.

The Rust port currently has the opposite shape: every frame class
overrides `convert` directly, duplicating logic that also exists in its
`sub_frame`. This has already produced one latent divergence:
`SkyFrame::convert` (basic.rs-style override) hardcodes alignment through
FK5, whereas `skyframe_sub_frame` correctly uses the template's
`AlignSystem`.

**Decision (user-approved): adopt C's single-driver structure.**
`Frame::convert_to` becomes one generic trait method backed by a free
function porting `astConvertX`; the per-class `convert` overrides are
retired so all alignment flows through `match_template` / `sub_frame`,
exactly as C. This is the most faithful structure and fixes the
FK5-vs-AlignSystem divergence. Risk is managed by migrating one class at
a time and re-verifying every `.convert()` consumer against the C oracle.

Naming: the public-facing trait method is **`convert_to`**
(`sky1.convert_to(&sky2, &["SKY"])`) — readable at call sites and
unambiguous for users coming from C who only know `astConvert`. The
internal generic free function is documented as the `astConvertX` port
but that name is not exposed on the public API.

## 3. ConvertX driver + `convert_to` rename

### 3.1 Rename
`Frame::convert` → `Frame::convert_to`. Mechanical update of all call
sites (≈10): `regions/{ellipse,circle,prism,interval,pointlist,
nullregion,...}.rs`, `fitschan/{enc_class,wcs_spectral}.rs`,
`fitschan/fset_to_store/mod.rs`, and the internal callers in
`skyframe.rs` (`compute_last`).

### 3.2 Generic driver
New private free function in `find_frame.rs`:

```text
fn convert_frames(from: &dyn Frame, to: &dyn Frame, domainlist: &str)
    -> AstResult<Option<FrameSet>>
```

Ports `astConvertX` (frame.c:3028-3258) verbatim:

1. Copy the domain list; loop each comma-separated field until a match
   is found (`match` flag). Clean each field: uppercase, strip
   whitespace (reuse the canonicalization already in
   `default_find_frame`).
2. `best_score = -1`. For `icom in 0..=1` (quit early on `perfect`):
   - `common0 = copy(if icom { from } else { to })`; set
     `PreserveAxes = false`; if the field is non-blank set its `Domain`.
   - `frame1 = (if icom { to } else { from })`.
   - `match1 = common0.match_template(frame1, match_sub = true)`.
   - On success: `common1 = match1.result`; set `PreserveAxes=false` on
     it; `match2 = common1.match_template(frame2, match_sub = true)`
     where `frame2 = (if icom { from } else { to })`.
   - On success: invert `to_map`, compose `from_map ∘ to_map` (series),
     simplify, score `= 2*has_forward + has_inverse`. Keep the candidate
     if `score > best_score`; `perfect = best_score >= 3`.
   - `map1_address`/`map2_address` bookkeeping: `from_map` is the match
     map when `icom==0`, the second match map when `icom==1` (and
     vice-versa for `to_map`). Mirror the C `map1_address`/`map2_address`
     pointer assignment exactly.
3. If a result map was found, build `FrameSet(base = from)` and
   `add_frame(Base, result_map, to)`; return it. Else `None`.

`Frame::convert_to` default body simply calls `convert_frames(self, to,
domainlist_joined)`. (The `domains: &[&str]` Rust slice is joined into
the comma list C expects; an empty slice ⇒ `" "` blank-permits-any.)

### 3.3 Retire per-class overrides
Delete the `fn convert` overrides in `skyframe.rs`, `specframe.rs`,
`cmpframe.rs`, `timeframe.rs`, `dsbspecframe.rs`, `fluxframe.rs`,
`specfluxframe.rs`, `basic.rs`. After this, `convert_to` is a single
trait method (no per-class override). Each class's existing
`match_template` / `sub_frame` already build the correct
system-conversion mapping (verified for SkyFrame: `skyframe_sub_frame`
constructs the full MakeSkyMapping + offset + axis-swap chain with
template `AlignSystem`).

Migration discipline: retire overrides one class at a time, running the
class's existing `convert_*` tier-1 tests (e.g.
`skyframe.rs::convert_fk5_to_galactic`, `convert_to_offset_frame`,
`convert_between_offset_frames_round_trip`) after each, plus the
fitschan/region consumers. Where a Rust test encoded the old FK5-aligned
expectation, re-derive the expected value from the C oracle rather than
adjusting it to whatever Rust now emits.

### 3.4 Acceptance test
Port `ast_tester/testconvert.c` → `crates/starlink-ast/tests/test_convert.rs`
(tier 1). It is the precise end-to-end check for this whole item:
- `astConvert(Frame[Domain=SKY], SkyFrame)` ⇒ FrameSet whose mapping is
  a UnitMap (requires the matchsub cast-retry — section 4).
- `astConvert(Frame[Domain=NOTSKY], SkyFrame)` ⇒ `None` (domain gate).
- `astConvert(Frame, CmpFrame)` ⇒ PermMap; with `Domain=SKY` the
  Frame matches the CmpFrame's SkyFrame component.
- `astConvert(SkyFrame, CmpFrame)` / reverse ⇒ PermMap.
- `astFindFrame(SkyFrame, CmpFrame)` ⇒ None unless SkyFrame Min/MaxAxes
  widened; `astFindFrame(Frame[Domain=ARDAPP], SkyFrame[GAPPT])` ⇒ None.

Build `testconvert.c` against a freshly built `libast.a` to confirm the
exact map class (UnitMap vs PermMap) expected for each pairing before
encoding it in the Rust test.

## 4. matchsub cast-retry + `Frame::cast`

### 4.1 `Frame::cast`
New trait method:

```text
fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>>
```

Default: `Ok(None)`. Ports C `astCast` semantics: return a copy of
`self` reduced to `other`'s class, or `None` if `self` is not a subclass
of `other`'s class. The C class ladder is:

```
Frame
├── SkyFrame
├── SpecFrame
│   └── DSBSpecFrame
├── TimeFrame
├── FluxFrame
└── CmpFrame
    └── SpecFluxFrame
```

Implementations (each cites the cast direction it serves):
- **Specialized leaf frames** (SkyFrame, SpecFrame, TimeFrame,
  FluxFrame): `cast(other)` returns `Some(BasicFrame snapshot)` when
  `other` is a plain `BasicFrame` (the only ancestor in Rust's model),
  carrying the same axis count + overlaid Frame/axis attributes (Domain,
  Title, per-axis Label/Unit/Symbol). `None` otherwise. Mirrors the
  Object-level `astCastCopy` reduction to the Frame base.
- **DSBSpecFrame**: `Some(SpecFrame clone)` when `other` is a SpecFrame
  (truncate to the parent), `Some(BasicFrame)` when `other` is a plain
  Frame, else `None`.
- **CmpFrame** (cmpframe.c:1245): if `other` is a CmpFrame or
  sub-CmpFrame ⇒ cast-copy of self; else build a `BasicFrame` with the
  same axis count, copy the axis metadata, overlay the CmpFrame
  attributes (Domain etc.), then `cast` that BasicFrame to `other`'s
  class (recursion bottoms out at the BasicFrame leaf case).

`cast` must not consult cosmetic differences; it is a class-reduction,
not an equality test.

### 4.2 Retry wiring
Introduce a small wrapper mirroring C `astMatch_` (frame.c:15473): the
entry point that `convert_frames` and `default_find_frame` call instead
of `template.match_template` directly. Body:

1. `m = template.match_template(target, match_sub)`.
2. If `m.is_none()` and `match_sub`:
   - `super_this = template.cast(target)?`.
   - If `Some(super_this)`: when `target.test_domain()` and the
     template's domain is non-empty, set `super_this`'s Domain to the
     template's domain; then `m = default_match(&*super_this, target,
     match_sub)` — the **base** Frame Match, not the (specialized)
     `super_this.match_template` (in C the cast result's vtab *is* the
     parent class, so its Match is the base Match; a BasicFrame's
     `match_template` already routes to `default_match`, so calling
     `super_this.match_template` is equivalent and is what we use).
3. Return `m`.

The MatchResult's `result` frame and `map` flow back unchanged.

### 4.3 Verification
`testconvert.c` Error 1-3 / 5-7 (Frame[Domain=SKY] ↔ SkyFrame ⇒
UnitMap) exercises exactly this path: the SkyFrame template fails to
match a plain Frame directly, casts down to a BasicFrame[SKY], and the
base Match succeeds with a UnitMap. Confirm against the C oracle.

## 5. printf Format + Digits + Unformat

### 5.1 Digits / Direction attributes
Add to `BasicFrame` (and trait getters with defaults):
- Frame-level `Digits`: storage `Option<i32>`; `digits()` default 7;
  `set_digits` constrains `>= 1` (frame.c:13134-13141 — `astMAKE_SET`
  clamps `value > 1 ? value : 1`, getter defaults 7, test = is-set).
- Per-axis `Digits(axis)` and `Direction(axis)` (axis.c). Per-axis
  Digits falls back to the frame Digits when unset (frame.c Format:
  `if (!digits_set) astSetAxisDigits(ax, astGetDigits(this))`).
  Direction default 1 (used by Plot; stored + round-tripped now,
  consumed later).
- Native serialization round-trip for Digits (frame-level + per-axis)
  and Direction, matching the C dump tokens.

### 5.2 axis_format module
New `frames/axis_format.rs` porting `ParseAxisFormat` (axis.c:2015) and
the formatting half of `AxisFormat` (axis.c:626):

- `parse_axis_format(fmt0: &str, digits: i32) -> ParsedFormat`
  returning the C-equivalent printf spec plus the `log` / `sign` /
  `lspace` / `integ` flags:
  - strip a `&` flag (sets `log`); when `log`, also strip `+` (sets
    `sign`) and ` ` (sets `lspace`); keep `-` / `#`.
  - locate the conversion code; `d`/`i`/`u`/`o`/`x`/`X`/`c` ⇒ `integ`.
  - `.*` precision ⇒ substitute the Digits value.
- `format_value(parsed, value, digits) -> String`: a faithful C
  `printf` for the `%[flags][width][.prec](G|g|E|e|f|F|d|i|x|...)`
  subset actually used: default `%1.<Digits>G`. The C `%G` semantics
  (fixed when exponent in `[-4, precision)`, else `E`, trailing-zero
  trimming, uppercase `E`) are reproduced exactly — this is the true C
  `%G`, **not** the FITS-compact variant in `fitschan/format.rs`
  (though the exponent-branch logic there is a useful reference). The
  `integ` path casts the value to `int` before formatting.
- `log` flag: parse it and render with C's non-Plot delimiter
  (`log_txt`, i.e. `"10**"` style) — the Plot graphical-escape path
  (`astTuneC("exdel")`) is deferred to step 10 and recorded as such in
  the gaps doc. This is the single deliberate divergence in the Format
  port and is the Plot-only rendering, not the numeric result.

### 5.3 BasicFrame::format / unformat
- `format(axis, value)`: `value == BAD` ⇒ `"<bad>"` (matches C
  `AxisFormat`, axis.c:704 — the `"<bad>"` token is now C-faithful, not
  an invention). Otherwise resolve the per-axis Format string (default
  `%1.<Digits>G`), parse, format. Remove the current
  `format_strs[..].parse::<usize>()` bare-precision branch.
- `unformat(axis, text)`: mirror `AxisUnformat` (axis.c:1284,
  `astSscanf("%lf %n")`):
  - parse the leading numeric token (with C-style leading-whitespace
    skip); on success return `(value, nchars)` where `nchars` **includes
    trailing whitespace** consumed by the `%n` after `%lf`.
  - on parse failure, return `(0.0-ish, 0)` style: nread = 0 and **no
    error** (current Rust raises `Unfer` — change it). The contract is
    "zero chars read, no coordinate" for callers like multi-field
    parsing.
  - recognize `"<bad>"` case-insensitive with embedded/leading/trailing
    whitespace ⇒ `BAD_VALUE`, nread = matched length.

The Rust trait signature is `unformat(&self, axis, text) ->
AstResult<(f64, usize)>`. The "no error, nread=0" case maps to
`Ok((BAD_VALUE-or-unspecified, 0))`; callers must check nread, matching
C. We will return `Ok((BAD_VALUE, 0))` so a misused value is still a
sentinel, and document that nread==0 means "nothing parsed".

## 6. Geometry / axis method family

All added as `Frame` trait methods with base (Cartesian) default impls,
ported verbatim. 1-based axis indices on the public-facing ones (matches
the C public interface; note C's protected AxDistance/AxOffset/AxNorm
also use 1-based — a documented C quirk we mirror).

- `offset2(point1, angle, offset) -> AstResult<(Vec<f64> point2, f64
  ret_angle)>` (frame.c:8228): error if naxes != 2; bad inputs ⇒ all
  BAD + ret BAD; else `p2[0]=p1[0]+sin(angle)*offset`,
  `p2[1]=p1[1]+cos(angle)*offset`, return angle.
- `ax_angle(a, b, axis) -> f64` (frame.c:1324): offset point a slightly
  in +axis direction (the `10000*DBL_EPSILON*...` nudge ladder, mirrored
  exactly), then `astAngle(aa, a, b)`.
- `ax_distance(axis, v1, v2) -> f64` (frame.c:1466): base = `v2 - v1`
  with BAD propagation (delegates to Axis `AxisDistance`; for BasicFrame
  that's a plain difference).
- `ax_offset(axis, v1, dist) -> f64` (frame.c:1744): base = `v1 + dist`
  with BAD propagation.
- `ax_norm(axis, oper, values: &mut [f64])` (frame.c:1560): base =
  no-op (validate `oper in {0,1}`); SkyFrame overrides later.
- `resolve(point1, point2, point3) -> (point4, d1, d2)` (frame.c:9429):
  dot-product decomposition; zero-length basis ⇒ BAD.
- `resolve_points(point1, point2, in: &PointSet) -> PointSet`
  (frame.c:9588): batch form; includes the **signed** perpendicular
  component for 2-D frames (`x1*y2 - x2*y1 < 0 ⇒ negate`) and the
  1-axis special case (`d1 = ax_distance`, `d2 = 0`). SkyFrame overrides
  later, so this is the prerequisite base.
- `intersect(a1, a2, b1, b2) -> cross` (frame.c:6243): error if
  naxes != 2; line-line intersection with vertical/parallel guards;
  BAD on bad inputs or parallel lines.
- `norm_box(lbnd, ubnd, reg)` (frame.c:7766): base = no-op.
- `match_axes(frm2, axes)` / `match_axes_x(frm1, axes)`
  (frame.c:7349 / 7438): for each axis of frm2, find its primary frame
  (section 7) and `match_template` it against frm1 (with PreserveAxes/
  Max/MinAxes temporarily widened per C); store the 1-based frm1 axis
  index or 0. Implemented via `match_axes_x` with the arg-swap wrapper
  mirroring the C trick.
- `is_unit_frame() -> bool` (frame.c:6417): base = true. (Specialized
  frames return false; wire those overrides as needed when simplify
  consumes it — tracked, low priority per gaps doc.)

`NormalPoints` / `NormPoints` PointSet-batch normalization is **deferred**
(thin per-point `Norm` loops, no current core consumer) and recorded in
the gaps doc.

The `PointSet` API for `resolve_points` is the existing
`crate::coords::PointSet` (column/row constructors already used across
the codebase).

## 7. PrimaryFrame + unwinding fitschan workarounds

- Promote `primary_frame` to a `Frame` trait method:
  `fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize)` (1-based
  in, returns leaf frame + 0-based underlying axis — matching the
  existing `primary_frame_walk` contract that callers already rely on;
  confirm the index base against current callers and keep it consistent,
  documenting the chosen base).
- Base impl (frame.c:8818): validate/permute axis, return `(self,
  axis)`. CmpFrame override: external→internal via perm, recurse into
  the owning component. SkyFrame: apply its axis permutation to return
  the underlying physical axis (0=lon, 1=lat) — exactly what
  `specframe::primary_frame_walk` does today.
- Replace the de-facto `specframe::primary_frame_walk` with the trait
  method (or have it delegate), then delete the two private copies:
  `fitschan/fset_to_store/make_int_world.rs:53` and
  `celestial_axes.rs:341`, pointing them at the trait method.
- Regression guard: the existing fitschan/wcsconv tests must stay green
  through this swap (the workarounds and the trait method must be
  behaviorally identical — verify, don't assume).

## 8. ActiveUnit unit adaptation

- Reconcile the existing `BasicFrame.act_unt` with C `GetActiveUnit`
  (frame.c:4651) / `SetActiveUnit` (frame.c:9882):
  - `get_active_unit()`: **always 0 if the frame contains any SkyAxis**;
    else the stored flag (default 0).
  - `set_active_unit(bool)`: store. SkyFrame forces 0, SpecFrame /
    FluxFrame force 1 (per the C "Applicability" notes) — wire these as
    overrides.
- `SubFrame` ActiveUnit tail (frame.c:11062-11108): in
  `default_sub_frame`, when a template is supplied and both
  `template.get_active_unit()` and `result.get_active_unit()` are true,
  build a parallel units `CmpMap` — for each result axis find the
  corresponding target axis and `units::unit_mapper(target_unit,
  result_unit)`; if any axis can't be mapped, the match fails (return
  `Ok(None)`); if the assembled units map is not all-UnitMap, compose it
  in series after the permutation map and simplify.
- `Overlay` (frame.c:8578 + NewUnit frame.c:7564): `Frame::overlay`, a
  method of the template as C's is (amended 2026-10-05; this was a
  result-side `overlay_template`, which could not dispatch on the
  template's class), transfers the ActiveUnit flag; when active and the Unit changes,
  rewrite the Label per `NewUnit` (e.g. `Hz`+`frequency` → `log(Hz)`
  gives `log( frequency )`) and clear the axis Format. Port `NewUnit`'s
  label-rewrite + format-clear.

This makes unit differences between matched frames produce real
conversions (currently silently ignored). SpecFrame already enables
active units in its own conversion path; this generalises it to the base
Match/Overlay machinery as C does.

## 9. Sequencing, scope boundaries, verification

### 9.1 Commit sequence (dependency order)
Each commit builds + tests + fmt + clippy clean before the next:

1. **Format / Digits / Direction / Unformat** (section 5) — self-contained.
2. **Geometry / axis method family + PrimaryFrame unwind** (sections 6, 7).
3. **`Frame::cast` + matchsub retry** (section 4).
4. **ActiveUnit adaptation** (section 8).
5. **ConvertX driver + retire per-class overrides + `convert_to` rename
   + `test_convert.rs`** (sections 3) — last, depends on 3 and 4 being
   in Match/SubFrame.

### 9.2 Scope boundaries
- **Core-only** (user-approved): no `starlink-ast-capi` shims / `ast.h` entries
  this round. The capi surface for `astConvert`, `astOffset2`,
  `astResolve`, `astIntersect`, `astAxAngle`, `astAxDistance`,
  `astAxOffset`, `astAxNorm`, `astMatchAxes`, `astNormBox`,
  `astPrimaryFrame`, `astGet/SetActiveUnit`, `astFormat`/`astUnformat`
  (printf), `Digits`/`Direction` is tracked as an explicit follow-up in
  the gaps doc.
- **Deferred, tracked in gaps/quirks doc:** Plot log-escape rendering in
  Format; `NormalPoints`/`NormPoints`; `testframe.c` (`NormUnit(1)` via
  the capi string layer — orthogonal to item 7); `IsUnitFrame` per-class
  overrides beyond the base; full conversion-based `Frame::equal` (its
  own gaps-doc item, still blocked behaviors noted there).

### 9.3 Verification strategy
- Tier-1 Rust tests per commit, ported from `ast_tester/` where it
  covers the area; `test_convert.rs` from `testconvert.c` is the
  acceptance gate for the whole item.
- For load-bearing numeric/structural behavior (ConvertX scoring and
  result map class, matchsub alignments, `resolve_points` signed-2D,
  printf `%G` output, Format/Unformat round-trips), build the reference
  `libast.a` and a small C probe binary and diff Rust output against C
  rather than guessing expected values — the pattern prior
  "oracle-matched" commits used.
- `PLAN.md` C→Rust migration tracker (currently empty) gets entries for
  `testconvert.c` (ported) and `testframe.c` (deferred, capi units).

### 9.4 No-corner-cutting commitment
Mirror C control flow directly. Replicate any C tolerance/heuristic with
a citation; never substitute a compute-then-check shortcut. Where a
faithful port is genuinely ambiguous, stop and ask. Record every
deferral explicitly.
