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
