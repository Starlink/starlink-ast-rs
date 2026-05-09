# Native Serialization Round-Trip Gaps

Status: 21/24 C-generated fixtures pass byte-exact round-trip.
Date: 2026-05-09

## Test infrastructure

The C fixture generator (`reference/starlink-ast/ast_tester/gen_native_fixtures.c`)
produces 24 fixtures using the C AST library with C11/default Channel settings.
The Rust test (`test_serialisation::native_byte_exact_round_trip_generated_fixtures`)
reads each fixture, writes it back via `to_native_string`, and compares byte-for-byte.

Build: `cmake -B build -DAST_C_STANDARD=11 && cmake --build build --target gen_native_fixtures`
Run: `./build/ast_tester/gen_native_fixtures <output_dir> <ast_tester_dir>`

Fixtures live in: `crates/ast-core/tests/fixtures/native/generated/`

## Remaining 3 failures

### 1. lsst.ast — CmpMap InvA with non-square SphMap (line 274)

**Problem**: When a CmpMap has `InvA = 1` and the child is a non-square
mapping (SphMap: 3→2 forward, 2→3 inverse), the round-trip produces
an extra `Invert = 1` on the child that shouldn't be there.

**Root cause**: Our `from_node` applies InvA by XORing with the child's
own Invert flag and calling `invert()` on the child. This changes
`is_inverted()` to true. Then `to_node` writes BOTH `InvA = 1` (from
`self.first.is_inverted()`) AND `Invert = 1` on the child (because
the child's internal flag is now set). The C format only writes InvA
on the parent; the child is stored in its native (non-inverted) state.

**The difficulty**: For square mappings (Nin==Nout), inversion doesn't
change dimensions, so writing the child with `Invert = 1` and NOT
writing InvA would also round-trip correctly. But for non-square
mappings, the child's Nin/Nout swap on inversion, making the
serialized Nin/Nout wrong.

**Fix approach**: The CmpMap needs to separate "child's own native
Invert flag" from "the InvA flag applied by this CmpMap". Store both
independently:
- `child_inv_a_native: bool` — the child's Invert flag as read from the file
- `inv_a: bool` — the InvA flag on the CmpMap

During `to_node`:
- Write child in its native state (with `child_inv_a_native` as its Invert)
- Write `InvA = 1` only when InvA differs from `child_inv_a_native`

During transform:
- Effective inversion = `child_inv_a_native XOR inv_a`

This requires adding `child_inv_a_native`/`child_inv_b_native` fields
to CmpMap and adjusting from_node to NOT call `invert()` on children
(instead, track both flags separately and compute effective inversion
at transform time).

**Files**: `crates/ast-core/src/mappings/cmpmap.rs`

**Impact**: All CmpMaps with InvA/InvB and non-square children
(SphMap is the main case: it's 3→2 forward, 2→3 inverse).

### 2. timj.ast — Nnode for tree-structured FrameSet (line 18)

**Problem**: The file has `Nframe = 4` and `Nnode = 5`. Our FrameSet
only supports linear chains where nnode == nframe. The C library
supports tree-structured FrameSets where multiple frames can share
a node (or nodes can exist without a frame).

**Context**: `timj.ast` has this structure:
```
Nframe = 4
Nnode = 5
Lnk2 = 1   # Node 2 derived from node 1
Lnk3 = 1   # Node 3 derived from node 1
Lnk4 = 1   # Node 4 derived from node 1
Lnk5 = 1   # Node 5 derived from node 1
```

Node 1 is shared — all other nodes derive from it (star topology).
With 4 frames and 5 nodes, one frame maps to a different node than
its index.

**Fix approach**: Add a `node_index: Vec<usize>` field to FrameSet
that maps each frame (0-based) to its node index (0-based). When
`nnode != nframe`, also store `nnode` and write both `Nnode` and
per-frame `Nd{i}` entries (frame-to-node mappings). During read,
parse `Nd{i}` to build the mapping.

The mapping affects which `Map{i}` connects to which frame — the
link/map structure uses NODE indices, not frame indices.

**Files**: `crates/ast-core/src/frames/frameset.rs`

**Impact**: Any FrameSet created by the C library with remapped nodes
(common in FITS WCS with multiple alternate coordinate systems).

### 3. degen1.ast — Same Nnode issue (line 19)

Same as timj.ast. Has `Nframe = 4, Nnode = 5` with star topology.

## What was fixed in this session

Starting from 10/24 passing:
- **SkyFrame**: unset attrs (Title, Domain, Epoch, Lbl, Uni, Dir, Bot,
  Top, SkyTol), projection in title, AlSys, SRefIs tracking, SRef
  comments with formatted coordinates, is_latitude inference
- **BasicFrame**: unset attrs (Title, Lbl, Uni), class comment
- **SpecFrame**: unset attrs (Title, Domain, Epoch, Lbl, Uni),
  sor_set tracking, unit preference
- **TimeFrame**: unset attrs (Title, Domain, Epoch, Lbl, Uni)
- **CmpFrame**: proxy attrs from children (Title, Naxes, Domain, Lbl,
  Uni, Dir), strip_unset for nested children
- **FrameSet**: proxy attrs from current frame, Base commented,
  Epoch/System injection for CmpFrame current, skip class-specific
  attrs from proxy
- **All frames**: ActUnt and Ident attributes
- **Mappings**: IsSimp/Invert/Fwd/Inv attribute ordering, has_forward/
  has_inverse on Mapping trait, MappingBaseAttrs struct refactor
- **LutMap**: Incr default, has_inverse tracking
- **PolyMap/ChebyMap**: preserve zero-coefficient terms, fix power
  comment formatting
- **CmpMap**: inv_a/inv_b field tracking
- **SkyAxis**: don't write IsLat (inferred from position)
