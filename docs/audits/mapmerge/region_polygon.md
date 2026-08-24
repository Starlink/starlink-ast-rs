# `Simplify` audit — Polygon

C source: reference/starlink-ast/src/polygon.c
Rust: crates/ast-core/src/regions/polygon.rs
Audited: 2026-08-24 against pin 07fbe2dc

Polygon has no `MapMerge`; C gives it a `Simplify` body
(`polygon.c:5291-5467`) that calls the parent `Region::Simplify` and then, in two
dimensions and through a non-UnitMap, re-expresses the polygon in its current
Frame by transforming its vertices. Unlike Circle and Ellipse, no shape is
re-fitted — the vertices go where the Mapping sends them — so the checks C makes
before adopting the result are the only thing standing between a transformed
polygon and a wrong one.

`Polygon::simplified` (`polygon.rs:1095-1150`) is the port of it.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the result and check the global error status | polygon.c:5354-5358 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Obtain a pointer to the supplied Region structure | polygon.c:5360-5361 | -- | n/a: the Rust method already has `&self` |
| 3 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | polygon.c:5363-5365 | polygon.rs:1099-1112 | ported |
| 4 | Note that simplification took place if the parent returned a different pointer | polygon.c:5367-5370 | polygon.rs:1113 | ported |
| 5 | Re-express the vertices only when the base-to-current Mapping is not a UnitMap and has two outputs | polygon.c:5372-5380 | polygon.rs:1115-1130 | ported |
| 6 | Get the Frame the Polygon represents | polygon.c:5382-5383 | polygon.rs:1134 | ported |
| 7 | Get the uncertainty Region in that Frame | polygon.c:5385-5386 | polygon.rs:1141 | gap: PLAN.md §10.18 |
| 8 | Transform the vertices into that Frame | polygon.c:5388-5390 | polygon.rs:1132-1138 | ported |
| 9 | Get the number of vertices | polygon.c:5392-5393 | polygon.rs:1136 | ported |
| 10 | Re-organise the vertex axis values into the order the Polygon constructor wants | polygon.c:5395-5406 | polygon.rs:1136-1138 | n/a: the Rust constructor takes a slice per vertex, so there is no axis-major buffer to build |
| 11 | Clear `ok` if any transformed axis-1 value is `AST__BAD` | polygon.c:5399-5404 | -- | gap: PLAN.md §10.18 |
| 12 | Build the new Polygon from the transformed vertices, only if `ok` | polygon.c:5408-5410 | polygon.rs:1139-1146 | gap: PLAN.md §10.18 |
| 13 | When `SimpVertices` is zero, mesh the original Polygon and require every mesh point to lie on the new Polygon's boundary, annulling the new Polygon otherwise | polygon.c:5412-5429 | -- | gap: PLAN.md §10.18 |
| 14 | If a new Polygon survives, use it in place of the original and record a simplification | polygon.c:5431-5437 | polygon.rs:1147-1149 | ported |
| 15 | Free the Frame, the uncertainty, the transformed points, the vertex buffer and the Mapping | polygon.c:5441-5447 | -- | n/a: owned values are dropped |
| 16 | On simplification, overlay the supplied Region's attributes onto the result and return it | polygon.c:5449-5455 | polygon.rs:1148-1149 | ported |
| 17 | Otherwise return the supplied Region unchanged | polygon.c:5457-5460 | polygon.rs:1123-1129 | ported |
| 18 | Annul the returned pointer if an error occurred, then return | polygon.c:5462-5467 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |

## Rows 7, 11, 12 and 13 — the port adopts the transformed polygon unconditionally

Three of C's guards are missing, and all three are recorded together in
PLAN.md §10.18 because they sit in the same branch:

- **Row 11.** C clears `ok` when a transformed vertex has `AST__BAD` on axis 1,
  and builds nothing when `ok` is clear. Note that C tests only axis 1: the axis-2
  loop at `polygon.c:5405-5406` copies without testing. That asymmetry is C's, and
  a port of the guard should reproduce it rather than tidy it.
- **Row 13.** C's `SimpVertices` attribute exists precisely because transforming
  the vertices can leave the *edges* in the wrong place: a non-linear Mapping
  carries each vertex correctly while bowing the line between them. With the
  attribute cleared, C meshes the original Polygon and discards the new one unless
  `astRegPins` puts every mesh point on its boundary. The port has no
  `SimpVertices` attribute at all — no accessor, and no `SimpVT` card on the
  serialisation path C writes at `polygon.c:6692` — so the check is unreachable
  rather than merely absent.
- **Row 7.** C passes `astGetUncFrm( new, AST__CURRENT )` to the Polygon
  constructor: the uncertainty mapped into the current Frame, where the new
  vertices live. The port passes `this.data.uncertainty.clone()`, the uncertainty
  as stored in the base Frame. When the base-to-current Mapping is not a UnitMap —
  which is the only case that reaches this code — those are different Regions.

The default `SimpVertices` is 1 (`polygon.c` `astMAKE_GET(Polygon,SimpVertices,…)`),
so row 13's check is skipped on the default and the corpus cannot see it. Rows 7
and 11 are visible without any attribute being set; what they need is a fixture
with an explicit uncertainty, and one with a vertex that transforms to `AST__BAD`.

## Row 12 — the winding order is restored, as the C constructor does

`polygon.rs:1146` calls `ensure_inside` after building the new Polygon.
That is not an addition: C's own constructor calls `EnsureInside` at
`polygon.c:7160`, inside `astInitPolygon_`, so every `astPolygon( … )` — including
the one at `polygon.c:5410` — already normalises the vertex order so the
un-negated Polygon bounds its inside. Remapping vertices can reverse that order,
which is why the step matters here.
