# `Simplify` / `MapMerge` audit — PointList

C source: reference/starlink-ast/src/pointlist.c
Rust: crates/ast-core/src/regions/pointlist.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Two C bodies. `MapMerge` (`pointlist.c:972-1202`) is the Region shape. `Simplify`
(`pointlist.c:2223-2288`) calls the parent, then — unless the base-to-current
Mapping is already a UnitMap — transforms the defining points into the current
Frame and rebuilds the PointList there, so the result needs only a UnitMap.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | `MapMerge`: simplify the PointList, assuming a changed pointer means something happened | pointlist.c:972-1100 | simplify.rs:1292-1301 | ported |
| 2 | Replace it in the list with the new Region, marking it forward | pointlist.c:1101-1130 | simplify.rs:1298-1299 | ported |
| 3 | Otherwise merge with a neighbouring Region, in parallel only | pointlist.c:1131-1160 | simplify.rs:1303-1305 | ported |
| 4 | Try the lower neighbour, then the upper | pointlist.c:1161-1180 | simplify.rs:1306-1330 | ported |
| 5 | On success replace the pair, close the gap, and return the first index | pointlist.c:1181-1199 | simplify.rs:1331-1345 | ported |
| 6 | Return the result | pointlist.c:1200-1202 | simplify.rs:1349 | ported |
| 7 | `Simplify`: obtain a pointer to the Region structure | pointlist.c:2223-2225 | -- | n/a: the Rust method already has `&self` |
| 8 | Invoke the parent Region simplify | pointlist.c:2226-2229 | pointlist.rs:758-761 | ported |
| 9 | Note whether that simplified anything | pointlist.c:2230-2234 | pointlist.rs:761-775 | ported |
| 10 | Get the base-to-current Mapping; the PointList can be modified so a UnitMap suffices | pointlist.c:2235-2241 | pointlist.rs:776-790 | ported |
| 11 | Get the current Region Frame | pointlist.c:2242-2244 | pointlist.rs:791-795 | ported |
| 12 | Get the PointSet of base-Frame positions defining the PointList | pointlist.c:2245-2248 | pointlist.rs:791-795 | ported |
| 13 | Transform it through the Mapping | pointlist.c:2249-2251 | pointlist.rs:791-795 | ported |
| 14 | Get the positional uncertainty in the current Frame | pointlist.c:2252-2255 | pointlist.rs:796-802 | ported |
| 15 | Create a new PointList and use it in place of the original | pointlist.c:2256-2261 | pointlist.rs:803-815 | ported |
| 16 | Free resources | pointlist.c:2262-2270 | -- | n/a: owned values are dropped |
| 17 | If anything simplified, overlay the original Region's attributes and return it | pointlist.c:2271-2283 | pointlist.rs:816-830 | ported |
| 18 | Annul the returned pointer if an error occurred | pointlist.c:2284-2286 | -- | n/a: an error is a `Result::Err` |
| 19 | Return the result | pointlist.c:2287-2288 | pointlist.rs:830 | ported |

## Row 10 — why the transform is always worth doing

C's comment gives the reason: rebuilding the PointList in the current Frame means
the simplified Region needs only a UnitMap, which lets the rest of the simplifier
treat it as already reduced. The port keeps the same short-circuit when the
Mapping is already a UnitMap (`pointlist.rs:781`), so the rebuild happens exactly
when it buys something.

## `cap_pointlist_01`

That fixture's two diverging mesh coordinates are a numeric disagreement in the
transformed defining points, not a missing step: every step of both C bodies is
performed here. It is tracked as cause F in `PLAN.md` §9, not as a gap in this
checklist.
