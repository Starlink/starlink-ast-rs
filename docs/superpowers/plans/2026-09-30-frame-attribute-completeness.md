# Frame attribute completeness — plan

Goal: every public attribute of Frame, SkyFrame, TimeFrame and SpecFrame (and
the classes built on them) is supported in `ast-core` and behaves as the C
library does: get, set, test and clear, the default, the Native dump and load,
and Overlay.
Frame first, then SkyFrame, then TimeFrame and SpecFrame.

## Design decision (2026-09-30)

The Frame-level attributes were re-implemented field by field in BasicFrame,
SkyFrame, SpecFrame, TimeFrame, FluxFrame and CmpFrame, each with a different
subset of methods, which is how they drifted.
The user chose a shared implementation:

- `FrameAttrs` holds the Frame-class attributes (Title, Domain, Epoch, Digits,
  ObsLat, ObsLon, ObsAlt, Dtai, Dut1, MatchEnd, MinAxes, MaxAxes, Permute,
  PreserveAxes, ActiveUnit, Ident, and whether System and AlignSystem are
  set), embedded in every Frame class.
- `Axis` holds the Axis-class attributes (Label, Symbol, Unit, Format,
  Digits, Direction, Top, Bottom), and `SkyAxis` wraps an `Axis` with its own
  attributes, as C's SkyAxis extends Axis.
  A Frame holds either kind on each axis, as C's does, which is what lets an
  axis picked out of a SkyFrame stay a SkyAxis.
- Per-class defaults (SkyFrame's Epoch for FK4, the Title and Domain of each
  class, Permute) come from per-class hooks, as C's subclasses override the
  Get methods; per-class side effects of a setter (SkyFrame's observer
  setters) stay in the class.

The C seams are recorded by `tools/oracle_fixtures/frame_attrs_probe.c`
(values, test flags and dumps for every Frame attribute) and by the vtab
overrides of each class: FrameSet and Region forward every Frame-level
accessor to their current Frame, CmpFrame forwards most to its components.

## Phases

1. **Frame.** `FrameAttrs` and `Axis` in BasicFrame; the missing attributes
   (System, AlignSystem, ObsLat, ObsLon, ObsAlt, Dtai, Dut1) and every
   missing test / clear; the per-axis dump cards (`Dig{n}`, `Fmt{n}`,
   `Dir{n}`, the helpful `Digits`) and C's Axis card order; Overlay per C's
   `Frame::Overlay` and `Axis::AxisOverlay`.
   Gate: `tests/test_frame_attrs.rs` against the probe's tables, byte-exact
   dumps both ways.
2. **Frame trait.** Every Frame-level and per-axis attribute reachable through
   `dyn Frame`, so FrameSet, Region and CmpFrame forward them as C does, with
   FrameSet and Region integrity on the setters.
3. **SkyFrame** on `FrameAttrs` and `SkyAxis { axis: Axis, .. }`; a picked
   SkyAxis stays a SkyAxis in a plain Frame (format, dump, NormUnit); the
   SkyAxis unformatter reads the Format.
4. **TimeFrame, SpecFrame, DSBSpecFrame, FluxFrame, SpecFluxFrame, CmpFrame**,
   each with its own probe.

Each phase ends green on the full suite and on its probe tables, and records
what it leaves in `PLAN.md` §2.

## Phase 2b design: the per-axis attributes

C reaches every per-axis attribute through `astGetAxis`, lending the Frame's
Digits to an Axis that has none (frame.c `MAKE_GET`); subclasses over-ride
only the getters that supply their own defaults, and SpecFrame, TimeFrame and
FluxFrame also the Unit setter.
`Axis` is crate-private, so the `Frame` trait does not hand one out.
Instead the trait carries get, set, test and clear for Label, Symbol, Unit,
Format, Digits(axis), Direction, Top and Bottom, each taking a 1-based axis
and returning `AstResult`, and each class implements them where C's class
does:

- The base behaviour, for a class that stores `Axis` values, comes from a
  macro over the class's axis storage, with the class supplying the getters C
  over-rides.
- SpecFrame, TimeFrame and FluxFrame gain an `Axis`, holding what they held
  in `unit` (and TimeFrame's `format_str`), so every attribute is stored.
- `SkyAxis` wraps an `Axis`, as C's SkyAxis extends Axis, and keeps its own
  Format (C's `skyformat`); SkyFrame's Label and Symbol overrides move into
  it.
  This is the storage half of phase 3.
- CmpFrame routes each call to the component, lending its own Digits to a
  component that has none, as C's `MAKE_GET` in cmpframe.c does.
  `Digits(axis)` comes from the component, because C reads it only through
  the attribute string, which a CmpFrame hands to the component
  (cmpframe.c `GetAttrib`).
  Top and Bottom come from the component's Axis object, because C reads
  them through `astGetAxis`; the hidden `axis_object_top` and
  `axis_object_bottom` trait methods are that seam, and they differ from
  `top` and `bottom` only for a SkyFrame, whose SkyAxes have no IsLatitude
  set.
- FrameSet setters go through `with_integrity`; a Region forwards to its
  FrameSet.

The probe reads each attribute through C's function (`astGetLabel` and so
on), which is what the trait mirrors.
For a CmpFrame, `astGetC` and `astSet` differ: they hand an axis-qualified
attribute straight to the primary Frame, skipping the CmpFrame's over-rides.

Gate: `tests/test_frame_axis_attrs.rs` against the tables from
`tools/oracle_fixtures/frame_axis_attrs_probe.c`.

## Phase 3 notes: SkyFrame

- SkyRef and SkyRefP are per external axis, as C's `MAKE_SET` and its
  relatives in skyframe.c map them through the permutation; the (lon, lat)
  pair accessors remain, named `*_lon_lat`, for the internal callers that
  build offset Mappings.
  SkyRefP's default is computed as `GetSkyRefP` computes it.
- `SkyAxis.is_latitude` is C's IsLatitude attribute, unset unless something
  sets it.
  A SkyFrame never sets it on its own axes, so the Axis methods C reaches
  through `astGetAxis` treat the latitude like a longitude; the port mirrors
  this, and `c-library-quirks.md` records it.
  SkyFrame's own over-rides tell the axes apart by their internal index.
- A plain Frame holds `FrameAxis::Plain(Axis)` or `FrameAxis::Sky(SkyAxis)`.
  SkyFrame's SubFrame attaches the picked SkyAxis with the attributes
  skyframe.c:10230 sets on it, so the picked axis formats, unformats,
  normalises and dumps as a SkyAxis.
- `unformat_sky_axis` is a port of `SkyAxis::AxisUnformat`
  (skyaxis.c:4068); a SkyFrame reads with its own Format, set or default.
- The C oracle must be built with `-ffp-contract=off` (see
  `docs/issues/c-oracle-probe-recipe.md`, Trap 1a): Apple clang otherwise
  fuses the unformatter's accumulation and the values differ by an ULP.

Gates: `tests/test_skyframe_attrs.rs` (`skyframe_attrs_probe.c`) and
`tests/test_picked_skyaxis.rs` (`picked_skyaxis_probe.c`).

## Phase 4 notes: TimeFrame, SpecFrame, DSBSpecFrame, FluxFrame

- Getters give C's value, set or default, in the units the class stores;
  where C's attribute is expressed in the Frame's current units
  (TimeOrigin, SpecOrigin, DSBCentre) or in km/s (SourceVel), a `*_cur`
  pair gives that form, converting as C's `ToUnits` and `GetXxxCur` do.
- Setters with side effects in C keep them: SourceVRF and SourceSys
  re-express SourceVel (`ConvertSourceVel`), StdOfRest re-expresses
  SpecOrigin, FluxFrame's Unit and System keep its UsedUnits.
- Each class writes the Frame part of its dump with `write_frame_cards`
  through `TraitDump`, which reads it through the `Frame` trait, so the
  commented defaults are the class's own, as C's subclass `Dump` gets them
  by calling the Frame class's.
  SkyFrame and CmpFrame still write their Frame parts themselves; their
  dumps already match C.
- A set Unit rewrites the default Label and Symbol through the label
  `astUnitMapper` returns (`units::label`).

Gate: `tests/test_class_attrs.rs` (`class_attrs_probe.c`). SpecFluxFrame
and CmpFrame have no attributes of their own beyond those of phases 2a and
2b.
