# Native Serialization Round-Trip Gaps

Status: 24/24 C-generated fixtures pass byte-exact round-trip.
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

### 1. ~~lsst.ast — CmpMap InvA with non-square SphMap (line 274)~~ FIXED

**Fix**: Aligned with C model — children keep their native Invert
state; `inv_a`/`inv_b` record the expected child state (snapshot at
construction/load). Effective transform direction computed at runtime
as `forward XOR (inv_a != child.is_inverted())`. `from_node` no longer
calls `invert()` on children; `to_node` writes `inv_a`/`inv_b` directly.

### 2. ~~timj.ast — Nnode for tree-structured FrameSet (line 18)~~ FIXED

**Fix**: Added `nnode` and `node_map` fields to FrameSet. Edges now
connect **nodes** (not frames). `from_node` reads `Nnode`, `Nod{i}`,
`Lnk{i}`, `Map{i}` correctly for tree-structured FrameSets. `to_node`
writes `Nnode` and `Nod{i}` entries when `nnode != nframe`. BFS
path-finding operates on the node graph, with frame→node translation
in `mapping()`.

### 3. ~~degen1.ast — Same Nnode issue~~ FIXED

Same fix as timj.ast.

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
- **CmpMap**: inv_a/inv_b field tracking; children keep native Invert
  state with effective direction computed at runtime (fixes non-square
  InvA/InvB round-trip)
- **FrameSet**: tree-structured node graphs (Nnode != Nframe) with
  frame-to-node mapping via Nod{i} entries
- **SkyAxis**: don't write IsLat (inferred from position)
