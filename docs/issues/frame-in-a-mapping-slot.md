# A bare Frame where a Mapping is expected

## The question

Is a bare Frame in a Mapping slot a side effect of C having made Frame a
Mapping in order to support Region, or is there a real use case for it?

The Rust port refuses one construct C accepts, and the answer decides whether
that is a bug to fix or a faithful narrowing to record.

## What is settled, and is not being asked

A Region is a Frame *and* a Mapping in this port
(`crates/ast-core/src/regions/mod.rs:93`), so both of these are read, written
and byte-compared today:

| Construct | Components | C | This port |
|---|---|---|---|
| `CmpFrame` holding Frames | Frames | accepted | accepted |
| `CmpFrame` holding a Region | Frames — a Region is a Frame | accepted | accepted; `crates/ast-core/tests/fixtures/region_in_cmpframe.ast` |
| `CmpMap` holding a Region | Mappings — a Region is a Mapping | accepted | accepted, 33 fixtures |
| `CmpFrame` holding a general Mapping | — | **rejected** | rejected |
| `CmpMap` holding a bare Frame | Mappings — a bare Frame is a Mapping in C only | accepted | **rejected — the question** |

The Region-in-`CmpFrame` row has no simplify fixture behind it because every
`CmpFrame` in that corpus is a Region's own Frame — the opposite nesting.
The reference product for it is a dump the C library wrote, added alongside the
failing fixture so the refusal below cannot widen to catch it.

`CmpFrame` refusing a general Mapping is not a mirror gap: C refuses it too.
`AstCmpFrame`'s fields are `AstFrame *frame1` and `AstFrame *frame2`
(`cmpframe.h:301-302`) and `astCmpFrame_` calls `astCheckFrame` on both
arguments (`cmpframe.c:10634-10635`). The asymmetry with `CmpMap` is C's, and
the port reproduces it: `CmpFrame` holds two `DynFrame`
(`crates/ast-core/src/frames/cmpframe.rs:25-26`), `CmpMap` two `DynMapping`
(`crates/ast-core/src/mappings/cmpmap.rs:102-103`).

## Why C accepts it

`Frame::Transform` (`frame.c:11739`) transforms through
`astUnitMap( astGetNaxes( this ) )` (`frame.c:11806`), so a bare Frame used as
a Mapping is an exact copy.
`CmpFrame::Transform` (`cmpframe.c:9898`) relies on that: it builds
`astCmpMap( this->frame1, this->frame2, 0, ... )` at `cmpframe.c:9975` — a
CmpMap whose components are Frames — and wraps it in PermMaps at
`cmpframe.c:9997-10018`.
C's own comment on that function says the result "will normally be a UnitMap
but may not be if the CmpFrame contains any Regions" (`cmpframe.c:9921-9923`),
which is the reading this question tests.

C accommodated the arrangement deliberately, not accidentally: `frame.c:6233`
installs a `DoNotSimplify` override returning zero, so a Frame with `Ident` set
stays simplifiable as a Mapping where the base rule at `mapping.c:1152` would
have blocked it.
The port mirrors that override
(`crates/ast-core/src/mappings/mod.rs:674`).

## The artifact

`crates/ast-core/tests/fixtures/frame_in_map_slot.ast` is a dump the C library
wrote: a FrameSet whose `Map3` slot holds `Begin CmpFrame`.
`astAddFrame` accepted it, `astTran2` transformed through it, and the Channel
dumped it without error.
`crates/ast-core/tests/fixtures/region_in_cmpframe.ast` is the settled nesting
from the same probe, written by the same library run.
Reproduce both with `tools/oracle_fixtures/frame_in_map_slot_probe.c`.

`crates/ast-core/tests/test_frame_in_map_slot.rs` pins the current refusal and
keeps it narrow.
Its first test reads the FrameSet's `Map<n>` slots structurally, requires one to
hold a Frame class, and requires the read to fail naming that class, so the
test cannot pass because the fixture stopped containing the construct.
Its other two require the 33 Region-in-`CmpMap` nestings and the
Region-in-`CmpFrame` dump to keep reading, so the gap cannot be closed by
refusing Frames in Mapping slots wholesale, nor the refusal widened to catch the
settled cases.

## What each answer costs

**If there is a real use case:** implement `Mapping` for `CmpFrame` — one
class, not seven.
`Transform` follows `cmpframe.c:9975-10018`, using each Region component
directly and `UnitMap::new(naxes)` for a non-Region component; `Simplify`
follows `cmpframe.c:8963`.
Substituting a `UnitMap` is faithful rather than a shortcut, because it is the
object C constructs at `frame.c:11806`, and the downcast that distinguishes the
cases is already in the tree (`regions::any_as_region`,
`crates/ast-core/src/regions/mod.rs:1340`, already applied to a Frame at
`crates/ast-core/src/regions/data.rs:145`).
One further both-traits class alongside `FrameSet` and the nine Regions sits
inside the existing design; the port design spec needs no amendment.

**If there is none:** the exclusion stays, and this document records the answer
as its reason.

## Evidence available today

No fixture in the simplify corpus puts a bare Frame in a Mapping slot, and the
Region case — the one with obvious meaning — already works.
That is the whole of the evidence for the second reading, which is why the
question goes to the team rather than being answered here.
