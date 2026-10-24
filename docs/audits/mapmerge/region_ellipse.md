# `Simplify` audit — Ellipse

C source: reference/starlink-ast/src/ellipse.c
Rust: crates/starlink-ast/src/regions/ellipse.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- ellipse.c:1928 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Ellipse has no `MapMerge`; C gives it a `Simplify` body
(`ellipse.c:1944-2049 (fn Simplify)`) that calls the parent `Region::Simplify` and then
tries to re-express the ellipse in its current Frame. It fits a Circle to a
boundary mesh first — an ellipse can transform into a circle — and falls back to
fitting an Ellipse in two dimensions, adopting a fit only when `astRegPins`
places every mesh point on the fitted boundary.

`Ellipse::simplified` (`ellipse.rs:1334-1408 (fn simplified)`) is the port of it. The body
is Circle's with two differences: there is no UnitMap short-circuit, and both
branches count as a simplification. See `region_circle.md`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the result | ellipse.c:1944-1946 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialize |
| 2 | Check the global error status and bail out | ellipse.c:1947-1949 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a pointer to the supplied Region structure | ellipse.c:1950-1952 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | ellipse.c:1953-1956 (fn Simplify) | ellipse.rs:1334-1343 (fn simplified) | architecture | ported |
| 5 | Note that simplification took place if the parent returned a different pointer | ellipse.c:1957-1961 (fn Simplify) | ellipse.rs:1344 (fn simplified) | architecture | ported |
| 6 | Get the base-to-current Mapping, and attempt the re-definition whatever it is | ellipse.c:1962-1967 (fn Simplify) | ellipse.rs:1354-1357 (fn simplified) | architecture | ported |
| 7 | Get a mesh of points covering the Ellipse in its current Frame | ellipse.c:1968-1970 (fn Simplify) | ellipse.rs:1360-1361 (fn simplified) | architecture | ported |
| 8 | Get the uncertainty Region in the current Frame | ellipse.c:1971-1974 (fn Simplify) | ellipse.rs:1364 (fn simplified) | architecture | ported |
| 9 | Transform the center into the current Frame and copy its axis values out | ellipse.c:1975-1980 (fn Simplify) | ellipse.rs:1368-1371 (fn simplified) | architecture | ported |
| 10 | Clear `ok` if any transformed center axis is `AST__BAD` | ellipse.c:1981-1987 (fn Simplify) | ellipse.rs:1372 (fn simplified) | architecture | ported |
| 11 | Fit the best Circle through the mesh, or nothing if the center is bad | ellipse.c:1988-1991 (fn Simplify) | ellipse.rs:1377-1378 (fn simplified) | architecture | ported |
| 12 | Adopt that Circle, and record a simplification, if every mesh point falls on its boundary within the uncertainty | ellipse.c:1992-2000 (fn Simplify) | ellipse.rs:1379-1383 (fn simplified) | architecture | ported |
| 13 | Otherwise, in two dimensions, see whether an Ellipse can represent the mesh | ellipse.c:2001-2004 (fn Simplify) | ellipse.rs:1388 (fn simplified) | architecture | ported |
| 14 | Discard the Circle and fit the best Ellipse through the mesh | ellipse.c:2005-2009 (fn Simplify) | ellipse.rs:1389 (fn simplified) | architecture | ported |
| 15 | See whether every mesh point falls on that Ellipse's boundary | ellipse.c:2010-2013 (fn Simplify) | ellipse.rs:1390 (fn simplified) | architecture | ported |
| 16 | If so, adopt the Ellipse and record a simplification | ellipse.c:2014-2020 (fn Simplify) | ellipse.rs:1391-1392 (fn simplified) | architecture | ported |
| 17 | Free the fitted Region | ellipse.c:2021-2024 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 18 | Free the transformed center, the center array, the mesh, the uncertainty and the Mapping | ellipse.c:2025-2030 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 19 | On simplification, overlay the supplied Region's attributes onto the result and return it | ellipse.c:2031-2038 (fn Simplify) | ellipse.rs:1397-1399 (fn simplified) | architecture | ported |
| 20 | Otherwise return a clone of the supplied Region | ellipse.c:2039-2043 (fn Simplify) | ellipse.rs:1401-1402 (fn simplified) | architecture | ported |
| 21 | Annul the returned pointer if an error occurred | ellipse.c:2044-2046 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 22 | Return the result | ellipse.c:2047-2049 (fn Simplify) | ellipse.rs:1399 (fn simplified),1385 (fn simplified) | architecture | ported |

## Row 6 — no UnitMap short-circuit, and that is deliberate in C

Circle wraps the whole re-definition in `if( !astIsAUnitMap( map ) )`
(`circle.c:1853 (fn Simplify)`). Ellipse does not: it fetches the Mapping, uses it to
transform the center, and annuls it (`ellipse.c:1966 (fn Simplify)`, `ellipse.c:2029 (fn Simplify)`), but never
tests it. So an Ellipse whose base-to-current Mapping is a UnitMap still gets a
Circle fitted to its own mesh, and is replaced by that Circle if the fit holds —
which is the reduction an ellipse with equal axes should undergo, and the reason
C does not short-circuit here. The port mirrors it and says so at
`ellipse.rs:1350-1353 (fn simplified)`.

## Rows 12 and 13 — the second fit is reached only when the first was not adopted

C's structure is `if( circle fits ) { adopt } else if( ok && nc == 2 ) { fit an
ellipse }`, so a successful Circle ends the attempt. The port has two separate
`if` blocks rather than a chain, and carries `adopted`
(`ellipse.rs:1376 (fn simplified)`,1098) to reproduce the `else`. The distinction matters:
without it, a mesh that both a Circle and an Ellipse fit would be replaced by the
Ellipse, and C replaces it by the Circle.

The Circle branch also requires `ok`, which C gets for free — `newreg` is `NULL`
when `ok` is clear, so its `if( newreg && … )` cannot fire. The port tests `ok`
explicitly (`ellipse.rs:1377 (fn simplified)`) because `best_circle` is only called inside
that test.

## Row 9 — the center is transformed through the simplified Mapping

As in `region_circle.md`: C transforms the *supplied* Region's points through its
original Mapping, the port transforms the parent-simplified copy's points through
the copy's Mapping. The parent alters neither the points nor the base Frame, so
the two agree.
