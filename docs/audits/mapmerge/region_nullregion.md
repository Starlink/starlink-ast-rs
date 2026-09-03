# `Simplify` / `MapMerge` / `GetDefUnc` audit — NullRegion

C source: reference/starlink-ast/src/nullregion.c
Rust: crates/ast-core/src/regions/nullregion.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Three short C bodies. `MapMerge` (`nullregion.c:338-568`) is the Region shape.
`Simplify` (`nullregion.c:1365-1407`) calls the parent and, when the
base-to-current Mapping is not a UnitMap, rebuilds the NullRegion on the current
Frame. `GetDefUnc` (`nullregion.c:218-235`) is a zero-radius Circle at the origin.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | `MapMerge`: simplify the NullRegion, assuming a changed pointer means something happened | nullregion.c:338-460 | mappings/simplify.rs:1288-1297 | ported |
| 2 | Replace it in the list with the new Region, marking it forward | nullregion.c:461-480 | mappings/simplify.rs:1294-1295 | ported |
| 3 | Otherwise merge with a neighbouring Region, in parallel only | nullregion.c:481-520 | mappings/simplify.rs:1299-1301 | ported |
| 4 | Try the lower neighbour, then the upper | nullregion.c:521-545 | mappings/simplify.rs:1302-1326 | ported |
| 5 | On success replace the pair, close the gap, and return the first index | nullregion.c:546-565 | mappings/simplify.rs:1327-1341 | ported |
| 6 | Return the result | nullregion.c:566-568 | mappings/simplify.rs:1345 | ported |
| 7 | `Simplify`: initialise the returned pointer | nullregion.c:1365-1366 | -- | n/a: the Rust returns `AstResult<DynMapping>` |
| 8 | Check the global error status and bail out | nullregion.c:1368-1369 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 9 | Get a pointer to the supplied Region structure | nullregion.c:1371-1373 | -- | n/a: the Rust method already has `&self` |
| 10 | Invoke the parent Region simplify | nullregion.c:1374-1377 | nullregion.rs:501-504 | ported |
| 11 | If the base-to-current Mapping is a UnitMap, no further simplification is possible | nullregion.c:1378-1385 | nullregion.rs:519-528 | ported |
| 12 | Otherwise create a NullRegion on the current Frame | nullregion.c:1386-1389 | nullregion.rs:529-535 | ported |
| 13 | Free resources | nullregion.c:1390-1396 | -- | n/a: owned values are dropped |
| 14 | If anything simplified, overlay the original Region's attributes and return it | nullregion.c:1397-1402 | nullregion.rs:536-545 | ported |
| 15 | Annul the returned pointer if an error occurred | nullregion.c:1403-1405 | -- | n/a: an error is a `Result::Err` |
| 16 | Return the result | nullregion.c:1406-1407 | nullregion.rs:545 | ported |
| 17 | `GetDefUnc`: initialise | nullregion.c:218-220 | -- | n/a: the Rust returns `AstResult<DynRegion>` |
| 18 | Check the inherited status | nullregion.c:221-223 | -- | n/a: errors travel in `AstResult` |
| 19 | Create a Circle centred on the origin with zero radius | nullregion.c:224-233 | nullregion.rs:296-303 | ported |
| 20 | Return the default uncertainty Region | nullregion.c:234-235 | nullregion.rs:303 | ported |

## Row 19 — a Circle, not the bounding box

A NullRegion has no extent, so the base-class bounding-box rule has nothing to
measure. C overrides it with a degenerate Circle at the base Frame origin, and
the port does the same. That also means this class is not affected by the
base-class guard defect in PLAN.md §10.12.

## Row 12 — the current Frame, carrying the parent's uncertainty

The rebuilt NullRegion is created on the *parent-simplified* current Frame and
carries the parent's uncertainty, not the original's, so the FrameSet and
uncertainty simplification the parent performed is not discarded by the rebuild.
