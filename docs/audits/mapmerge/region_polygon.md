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

`Polygon::simplified` (`polygon.rs:1151-1253`) is the port of it.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the result and check the global error status | polygon.c:5354-5358 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Obtain a pointer to the supplied Region structure | polygon.c:5360-5361 | -- | n/a: the Rust method already has `&self` |
| 3 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | polygon.c:5363-5365 | polygon.rs:1155-1168 | ported |
| 4 | Note that simplification took place if the parent returned a different pointer | polygon.c:5367-5370 | polygon.rs:1169 | ported |
| 5 | Re-express the vertices only when the base-to-current Mapping is not a UnitMap and has two outputs | polygon.c:5372-5380 | polygon.rs:1183-1192 | ported |
| 6 | Get the Frame the Polygon represents | polygon.c:5382-5383 | polygon.rs:1194 | ported |
| 7 | Get the uncertainty Region in that Frame | polygon.c:5385-5386 | polygon.rs:1198 | ported |
| 8 | Transform the vertices into that Frame | polygon.c:5388-5390 | polygon.rs:1200-1208 | ported |
| 9 | Get the number of vertices | polygon.c:5392-5393 | polygon.rs:1210 | ported |
| 10 | Re-organise the vertex axis values into the order the Polygon constructor wants | polygon.c:5395-5406 | polygon.rs:1210-1220 | n/a: the Rust constructor takes a slice per vertex, so there is no axis-major buffer to build |
| 11 | Clear `ok` if any transformed axis-1 value is `AST__BAD` | polygon.c:5399-5404 | polygon.rs:1213-1218 | ported |
| 12 | Build the new Polygon from the transformed vertices, only if `ok` | polygon.c:5408-5410 | polygon.rs:1222-1232 | ported |
| 13 | When `SimpVertices` is zero, mesh the original Polygon and require every mesh point to lie on the new Polygon's boundary, annulling the new Polygon otherwise | polygon.c:5412-5429 | polygon.rs:1234-1245 | ported |
| 14 | If a new Polygon survives, use it in place of the original and record a simplification | polygon.c:5431-5437 | polygon.rs:1247-1251 | ported |
| 15 | Free the Frame, the uncertainty, the transformed points, the vertex buffer and the Mapping | polygon.c:5441-5447 | -- | n/a: owned values are dropped |
| 16 | On simplification, overlay the supplied Region's attributes onto the result and return it | polygon.c:5449-5455 | polygon.rs:1249-1251 | ported |
| 17 | Otherwise return the supplied Region unchanged | polygon.c:5457-5460 | polygon.rs:1174-1181 | ported |
| 18 | Annul the returned pointer if an error occurred, then return | polygon.c:5462-5467 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |

## Rows 7, 11, 12 and 13 — the guards C makes before adopting the result

All three guards are ported, along with the `SimpVertices` attribute the third
one reads:

- **Row 11.** `ok` is cleared when a transformed vertex has `AST__BAD` on axis 1,
  and nothing is built when it is clear. C tests only axis 1: the axis-2 loop at
  `polygon.c:5405-5406` copies without testing, and the port reproduces the
  asymmetry rather than tidying it. No fixture pins the guard, and none can: a
  Polygon cannot hold a bad vertex (`polygon.c:7131` rejects one, as
  `PolygonBuilder::build` now does), and `astMapRegion` refuses any Mapping that
  sends a defining point to `AST__BAD` (`region.c:5546`), so no construction
  available through either public API reaches it.
- **Row 13.** C's `SimpVertices` attribute exists precisely because transforming
  the vertices can leave the *edges* in the wrong place: a non-linear Mapping
  carries each vertex correctly while bowing the line between them. With the
  attribute cleared, the original Polygon is meshed and the new one discarded
  unless `reg_pins` puts every mesh point on its boundary. The attribute is read
  off the supplied Polygon, as C reads it (`polygon.c:5415`), and serialises as
  the `SimpVT` card C writes at `polygon.c:6692` — written only when set, so an
  unset attribute leaves the round-trip untouched. `cap_poly_bent_edges` clears
  it over a quadratic MathMap and C keeps the original; `cap_poly_bend_allowed`
  is the same geometry with the attribute left at its default, where C adopts the
  transformed Polygon. The pair is what makes the branch falsifiable — a check
  that always failed would pass the first fixture alone.
- **Row 7.** `get_unc_frm(FrameRef::Current)` supplies the uncertainty mapped
  into the current Frame, where the new vertices live, matching
  `astGetUncFrm( new, AST__CURRENT )`. The call is unconditional as C's is: a
  Polygon with no explicit uncertainty gets its default here, and the
  `reg_overlay` in the tail is what restores the unset state.
  `cap_poly_unc_frame` pins it — its `Unc` block is the supplied Box scaled by
  the ZoomMap, not the base-Frame Box.

The default `SimpVertices` is 1 (`polygon.c:6511`, where `-INT_MAX` is the unset
sentinel), so row 13's check is skipped unless a fixture clears it.

## Row 12 — the winding order is restored, as the C constructor does

`polygon.rs:1232` calls `ensure_inside` after building the new Polygon.
That is not an addition: C's own constructor calls `EnsureInside` at
`polygon.c:7160`, inside `astInitPolygon_`, so every `astPolygon( … )` — including
the one at `polygon.c:5410` — already normalises the vertex order so the
un-negated Polygon bounds its inside. Remapping vertices can reverse that order,
which is why the step matters here.
