# `Simplify` audit — Ellipse

C source: reference/starlink-ast/src/ellipse.c
Rust: crates/ast-core/src/regions/ellipse.rs
Audited: 2026-08-24 against pin 07fbe2dc

Ellipse has no `MapMerge`; C gives it a `Simplify` body
(`ellipse.c:1879-2044`) that calls the parent `Region::Simplify` and then tries to
re-express the ellipse in its current Frame. It fits a Circle to a boundary mesh
first — an ellipse can transform into a circle — and falls back to fitting an
Ellipse in two dimensions, adopting a fit only when `astRegPins` places every
mesh point on the fitted boundary.

`Ellipse::simplified` (`ellipse.rs:967-1044`) is the port of it. The body is
Circle's with two differences: there is no UnitMap short-circuit, and both
branches count as a simplification. See `region_circle.md`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the result and check the global error status | ellipse.c:1939-1943 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Obtain a pointer to the supplied Region structure | ellipse.c:1945-1946 | -- | n/a: the Rust method already has `&self` |
| 3 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | ellipse.c:1948-1950 | ellipse.rs:971-984 | ported |
| 4 | Note that simplification took place if the parent returned a different pointer | ellipse.c:1952-1955 | ellipse.rs:985 | ported |
| 5 | Get the base-to-current Mapping, and attempt the re-definition whatever it is | ellipse.c:1957-1961 | ellipse.rs:995-998 | ported |
| 6 | Get a mesh of points covering the Ellipse in its current Frame | ellipse.c:1963-1964 | ellipse.rs:1001-1002 | ported |
| 7 | Get the uncertainty Region in the current Frame | ellipse.c:1966-1968 | ellipse.rs:1005 | ported |
| 8 | Transform the centre into the current Frame and copy its axis values out | ellipse.c:1970-1975 | ellipse.rs:1009-1012 | ported |
| 9 | Clear `ok` if any transformed centre axis is `AST__BAD` | ellipse.c:1977-1981 | ellipse.rs:1013 | ported |
| 10 | Fit the best Circle through the mesh, or nothing if the centre is bad | ellipse.c:1983-1985 | ellipse.rs:1018-1019 | ported |
| 11 | Adopt that Circle, and record a simplification, if every mesh point falls on its boundary within the uncertainty | ellipse.c:1987-1994 | ellipse.rs:1020-1024 | ported |
| 12 | Otherwise, in two dimensions, fit the best Ellipse through the mesh | ellipse.c:1996-2003 | ellipse.rs:1029-1030 | ported |
| 13 | Adopt that Ellipse, and record a simplification, if every mesh point falls on its boundary | ellipse.c:2005-2013 | ellipse.rs:1031-1034 | ported |
| 14 | Free the fitted Region, the centre array, the mesh, the uncertainty and the Mapping | ellipse.c:2016-2024 | -- | n/a: owned values are dropped |
| 15 | On simplification, overlay the supplied Region's attributes onto the result and return it | ellipse.c:2026-2032 | ellipse.rs:1038-1040 | ported |
| 16 | Otherwise return the supplied Region unchanged | ellipse.c:2034-2037 | ellipse.rs:1041-1042 | ported |
| 17 | Annul the returned pointer if an error occurred, then return | ellipse.c:2039-2044 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |

## Row 5 — no UnitMap short-circuit, and that is deliberate in C

Circle wraps the whole re-definition in `if( !astIsAUnitMap( map ) )`
(`circle.c:1853`). Ellipse does not: it fetches the Mapping, uses it to transform
the centre, and annuls it (`ellipse.c:1961`, `:2024`), but never tests it. So an
Ellipse whose base-to-current Mapping is a UnitMap still gets a Circle fitted to
its own mesh, and is replaced by that Circle if the fit holds — which is the
reduction an ellipse with equal axes should undergo, and the reason C does not
short-circuit here. The port mirrors it and says so at `ellipse.rs:990-993`.

## Row 8 — the centre is transformed through the simplified Mapping

As in `region_circle.md`: C transforms the *supplied* Region's points through its
original Mapping, the port transforms the parent-simplified copy's points through
the copy's Mapping. The parent alters neither the points nor the base Frame, so
the two agree.
