# `Simplify` audit — Ellipse

C source: reference/starlink-ast/src/ellipse.c
Rust: crates/ast-core/src/regions/ellipse.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin ca610685 (re-derived from file content at the
fixture-reorg sync; the numbers before that were correct against pin 3e60abaa)
Extension points: classified

Uncited C steps:
- ellipse.c:1923 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Ellipse has no `MapMerge`; C gives it a `Simplify` body
(`ellipse.c:1939-2044 (fn Simplify)`) that calls the parent `Region::Simplify` and then
tries to re-express the ellipse in its current Frame. It fits a Circle to a
boundary mesh first — an ellipse can transform into a circle — and falls back to
fitting an Ellipse in two dimensions, adopting a fit only when `astRegPins`
places every mesh point on the fitted boundary.

`Ellipse::simplified` (`ellipse.rs:1040-1118 (fn simplified)`) is the port of it. The body
is Circle's with two differences: there is no UnitMap short-circuit, and both
branches count as a simplification. See `region_circle.md`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the result | ellipse.c:1939-1941 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 2 | Check the global error status and bail out | ellipse.c:1942-1944 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a pointer to the supplied Region structure | ellipse.c:1945-1947 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | ellipse.c:1948-1951 (fn Simplify) | ellipse.rs:1040-1053 (fn simplified) | architecture | ported |
| 5 | Note that simplification took place if the parent returned a different pointer | ellipse.c:1952-1956 (fn Simplify) | ellipse.rs:1054 (fn simplified) | architecture | ported |
| 6 | Get the base-to-current Mapping, and attempt the re-definition whatever it is | ellipse.c:1957-1962 (fn Simplify) | ellipse.rs:1064-1067 (fn simplified) | architecture | ported |
| 7 | Get a mesh of points covering the Ellipse in its current Frame | ellipse.c:1963-1965 (fn Simplify) | ellipse.rs:1070-1071 (fn simplified) | architecture | ported |
| 8 | Get the uncertainty Region in the current Frame | ellipse.c:1966-1969 (fn Simplify) | ellipse.rs:1074 (fn simplified) | architecture | ported |
| 9 | Transform the centre into the current Frame and copy its axis values out | ellipse.c:1970-1975 (fn Simplify) | ellipse.rs:1078-1081 (fn simplified) | architecture | ported |
| 10 | Clear `ok` if any transformed centre axis is `AST__BAD` | ellipse.c:1976-1982 (fn Simplify) | ellipse.rs:1082 (fn simplified) | architecture | ported |
| 11 | Fit the best Circle through the mesh, or nothing if the centre is bad | ellipse.c:1983-1986 (fn Simplify) | ellipse.rs:1087-1088 (fn simplified) | architecture | ported |
| 12 | Adopt that Circle, and record a simplification, if every mesh point falls on its boundary within the uncertainty | ellipse.c:1987-1995 (fn Simplify) | ellipse.rs:1089-1093 (fn simplified) | architecture | ported |
| 13 | Otherwise, in two dimensions, see whether an Ellipse can represent the mesh | ellipse.c:1996-1999 (fn Simplify) | ellipse.rs:1098 (fn simplified) | architecture | ported |
| 14 | Discard the Circle and fit the best Ellipse through the mesh | ellipse.c:2000-2004 (fn Simplify) | ellipse.rs:1099 (fn simplified) | architecture | ported |
| 15 | See whether every mesh point falls on that Ellipse's boundary | ellipse.c:2005-2008 (fn Simplify) | ellipse.rs:1100 (fn simplified) | architecture | ported |
| 16 | If so, adopt the Ellipse and record a simplification | ellipse.c:2009-2015 (fn Simplify) | ellipse.rs:1101-1102 (fn simplified) | architecture | ported |
| 17 | Free the fitted Region | ellipse.c:2016-2019 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 18 | Free the transformed centre, the centre array, the mesh, the uncertainty and the Mapping | ellipse.c:2020-2025 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 19 | On simplification, overlay the supplied Region's attributes onto the result and return it | ellipse.c:2026-2033 (fn Simplify) | ellipse.rs:1107-1109 (fn simplified) | architecture | ported |
| 20 | Otherwise return a clone of the supplied Region | ellipse.c:2034-2038 (fn Simplify) | ellipse.rs:1111-1112 (fn simplified) | architecture | ported |
| 21 | Annul the returned pointer if an error occurred | ellipse.c:2039-2041 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 22 | Return the result | ellipse.c:2042-2044 (fn Simplify) | ellipse.rs:1109 (fn simplified),1112 | architecture | ported |

## Row 6 — no UnitMap short-circuit, and that is deliberate in C

Circle wraps the whole re-definition in `if( !astIsAUnitMap( map ) )`
(`circle.c:1853 (fn Simplify)`). Ellipse does not: it fetches the Mapping, uses it to
transform the centre, and annuls it (`ellipse.c:1961 (fn Simplify)`, `:2024`), but never
tests it. So an Ellipse whose base-to-current Mapping is a UnitMap still gets a
Circle fitted to its own mesh, and is replaced by that Circle if the fit holds —
which is the reduction an ellipse with equal axes should undergo, and the reason
C does not short-circuit here. The port mirrors it and says so at
`ellipse.rs:1060-1063 (fn simplified)`.

## Rows 12 and 13 — the second fit is reached only when the first was not adopted

C's structure is `if( circle fits ) { adopt } else if( ok && nc == 2 ) { fit an
ellipse }`, so a successful Circle ends the attempt. The port has two separate
`if` blocks rather than a chain, and carries `adopted`
(`ellipse.rs:1086 (fn simplified)`,1098) to reproduce the `else`. The distinction matters:
without it, a mesh that both a Circle and an Ellipse fit would be replaced by the
Ellipse, and C replaces it by the Circle.

The Circle branch also requires `ok`, which C gets for free — `newreg` is `NULL`
when `ok` is clear, so its `if( newreg && … )` cannot fire. The port tests `ok`
explicitly (`ellipse.rs:1087 (fn simplified)`) because `best_circle` is only called inside
that test.

## Row 9 — the centre is transformed through the simplified Mapping

As in `region_circle.md`: C transforms the *supplied* Region's points through its
original Mapping, the port transforms the parent-simplified copy's points through
the copy's Mapping. The parent alters neither the points nor the base Frame, so
the two agree.
