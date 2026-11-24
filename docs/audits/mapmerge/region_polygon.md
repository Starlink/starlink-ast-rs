# `Simplify` audit — Polygon

C source: reference/starlink-ast/src/polygon.c
Rust: crates/starlink-ast/src/regions/polygon.rs
Audited: 2026-08-24 against pin 07fbe2dc
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- polygon.c:5390 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Polygon has no `MapMerge`; C gives it a `Simplify` body
(`polygon.c:5346-5524 (fn Simplify)`) that calls the parent `Region::Simplify` and then, in two
dimensions and through a non-UnitMap, re-expresses the polygon in its current
Frame by transforming its vertices. Unlike Circle and Ellipse, no shape is
re-fitted — the vertices go where the Mapping sends them — so the checks C makes
before adopting the result are the only thing standing between a transformed
polygon and a wrong one.

`Polygon::simplified` (`polygon.rs:2364-2463 (fn simplified)`) is the port of it.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the result and check the global error status | polygon.c:5409-5413 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Obtain a pointer to the supplied Region structure | polygon.c:5415-5416 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 3 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | polygon.c:5418-5420 (fn Simplify) | polygon.rs:2364-2377 (fn simplified) | architecture | ported |
| 4 | Note that simplification took place if the parent returned a different pointer | polygon.c:5422-5425 (fn Simplify) | polygon.rs:2378 (fn simplified) | architecture | ported |
| 5 | Re-express the vertices only when the base-to-current Mapping is not a UnitMap and has two outputs | polygon.c:5427-5435 (fn Simplify) | polygon.rs:2396-2405 (fn simplified) | architecture | ported |
| 6 | Get the Frame the Polygon represents | polygon.c:5437-5438 (fn Simplify) | polygon.rs:2403 (fn simplified) | architecture | ported |
| 7 | Get the uncertainty Region in that Frame | polygon.c:5440-5441 (fn Simplify) | polygon.rs:2407 (fn simplified) | architecture | ported |
| 8 | Transform the vertices into that Frame | polygon.c:5443-5445 (fn Simplify) | polygon.rs:2412-2417 (fn simplified) | architecture | ported |
| 9 | Get the number of vertices | polygon.c:5447-5448 (fn Simplify) | polygon.rs:2419 (fn simplified) | architecture | ported |
| 10 | Re-organize the vertex axis values into the order the Polygon constructor wants | polygon.c:5450-5463 (fn Simplify) | polygon.rs:2419-2429 (fn simplified) | ceremony | n/a: the Rust constructor takes a slice per vertex, so there is no axis-major buffer to build |
| 11 | Clear `ok` if any transformed vertex value is `AST__BAD` | polygon.c:5454-5463 (fn Simplify) | polygon.rs:2420-2429 (fn simplified) | architecture | ported |
| 12 | Build the new Polygon from the transformed vertices, only if `ok` | polygon.c:5465-5467 (fn Simplify) | polygon.rs:2440-2443 (fn simplified) | architecture | ported |
| 13 | When `SimpVertices` is zero, mesh the original Polygon and require every mesh point to lie on the new Polygon's boundary, annulling the new Polygon otherwise | polygon.c:5469-5486 (fn Simplify) | polygon.rs:2449-2456 (fn simplified) | architecture | ported |
| 14 | If a new Polygon survives, use it in place of the original and record a simplification | polygon.c:5488-5494 (fn Simplify) | polygon.rs:2458-2462 (fn simplified) | architecture | ported |
| 15 | Free the Frame, the uncertainty, the transformed points, the vertex buffer and the Mapping | polygon.c:5498-5504 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 16 | On simplification, overlay the supplied Region's attributes onto the result and return it | polygon.c:5506-5512 (fn Simplify) | polygon.rs:2460-2462 (fn simplified) | architecture | ported |
| 17 | Otherwise return the supplied Region unchanged | polygon.c:5514-5517 (fn Simplify) | polygon.rs:2383-2390 (fn simplified) | architecture | ported |
| 18 | Annul the returned pointer if an error occurred, then return | polygon.c:5519-5524 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |

## Rows 7, 11, 12 and 13 — the guards C makes before adopting the result

All three guards are ported, along with the `SimpVertices` attribute the third
one reads:

- **Row 11.** `ok` is cleared when a transformed vertex has `AST__BAD` on either
  axis, and nothing is built when it is clear. Both copy loops test as they copy
  (`polygon.c:5457-5459 (fn Simplify)` and `polygon.c:5461-5463 (fn Simplify)`);
  C tested axis 1 only until `d27cc981`, and `docs/issues/c-library-quirks.md`
  carries the history. No fixture pins the guard, and none can: a
  Polygon cannot hold a bad vertex (`polygon.c:7193 (fn astInitPolygon_)` rejects one, as
  `PolygonBuilder::build` now does), and `astMapRegion` refuses any Mapping that
  sends a defining point to `AST__BAD` (`region.c:5671-5675 (fn MapRegion)`), so no construction
  available through either public API reaches it.
- **Row 13.** C's `SimpVertices` attribute exists precisely because transforming
  the vertices can leave the *edges* in the wrong place: a non-linear Mapping
  carries each vertex correctly while bowing the line between them. With the
  attribute cleared, the original Polygon is meshed and the new one discarded
  unless `reg_pins` puts every mesh point on its boundary. The attribute is read
  off the supplied Polygon, as C reads it (`polygon.c:5472 (fn Simplify)`), and serializes as
  the `SimpVT` card C writes at `polygon.c:6754 (fn Dump)` — written only when set, so an
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

The default `SimpVertices` is 1 (`polygon.c:6573-6574 (fn astMAKE_GET)`, where `-INT_MAX` is the unset
sentinel), so row 13's check is skipped unless a fixture clears it.

## Row 12 — the winding order is restored, as the C constructor does

`polygon.rs:2445 (fn simplified)` calls `ensure_inside` after building the new Polygon.
That is not an addition: C's own constructor calls `EnsureInside` at
`polygon.c:7222 (fn astInitPolygon_)`, inside `astInitPolygon_`, so every `astPolygon( … )` — including
the one at `polygon.c:5467 (fn Simplify)` — already normalizes the vertex order so the
un-negated Polygon bounds its inside. Remapping vertices can reverse that order,
which is why the step matters here.
