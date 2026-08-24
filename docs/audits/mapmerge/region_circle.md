# `Simplify` audit — Circle

C source: reference/starlink-ast/src/circle.c
Rust: crates/ast-core/src/regions/circle.rs
Audited: 2026-08-24 against pin 07fbe2dc

Circle has no `MapMerge`; C gives it a `Simplify` body
(`circle.c:1769-1937`) that calls the parent `Region::Simplify` and then tries to
re-express the circle in its current Frame. Transforming a circle need not leave
a circle, so C fits a new Circle to a boundary mesh, falls back to fitting an
Ellipse in two dimensions, and adopts a fit only when `astRegPins` places every
mesh point on the fitted boundary.

`Circle::simplified` (`circle.rs:892-974`) is the port of it.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the result and check the global error status | circle.c:1829-1833 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Obtain a pointer to the supplied Region structure | circle.c:1835-1836 | -- | n/a: the Rust method already has `&self` |
| 3 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | circle.c:1838-1840 | circle.rs:896-909 | ported |
| 4 | Note that simplification took place if the parent returned a different pointer | circle.c:1842-1845 | circle.rs:910 | ported |
| 5 | Skip the whole re-definition when the base-to-current Mapping is a UnitMap | circle.c:1847-1853 | circle.rs:919-923 | ported |
| 6 | Get a mesh of points covering the Circle in its current Frame | circle.c:1855-1856 | circle.rs:927-928 | ported |
| 7 | Get the uncertainty Region in the current Frame | circle.c:1858-1860 | circle.rs:931 | ported |
| 8 | Transform the centre into the current Frame and copy its axis values out | circle.c:1862-1867 | circle.rs:935-938 | ported |
| 9 | Clear `ok` if any transformed centre axis is `AST__BAD` | circle.c:1870-1874 | circle.rs:939 | ported |
| 10 | Fit the best Circle through the mesh, or nothing if the centre is bad | circle.c:1876-1878 | circle.rs:944-945 | ported |
| 11 | Adopt that Circle if every mesh point falls on its boundary within the uncertainty | circle.c:1880-1886 | circle.rs:946-951 | ported |
| 12 | Otherwise, in two dimensions, fit the best Ellipse through the mesh | circle.c:1888-1895 | circle.rs:957-958 | ported |
| 13 | Adopt that Ellipse, and record a simplification, if every mesh point falls on its boundary | circle.c:1897-1905 | circle.rs:959-962 | ported |
| 14 | Free the fitted Region, the centre array, the mesh, the uncertainty and the Mapping | circle.c:1908-1917 | -- | n/a: owned values are dropped |
| 15 | On simplification, overlay the supplied Region's attributes onto the result and return it | circle.c:1919-1925 | circle.rs:967-969 | ported |
| 16 | Otherwise return the supplied Region unchanged | circle.c:1927-1930 | circle.rs:970-971 | ported |
| 17 | Annul the returned pointer if an error occurred, then return | circle.c:1932-1937 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |

## Row 11 — adopting a fitted Circle does not count as a simplification

The Ellipse branch sets `simpler = 1` (`circle.c:1904`); the Circle branch does
not (`circle.c:1885-1886`). So a Circle that is re-fitted to a *Circle* is
adopted into `new` and then thrown away, because the `if( simpler )` test at
`circle.c:1923` sees only whatever the parent Simplify reported. If the parent
simplified nothing, C returns the original Circle and the fit is discarded.

Ellipse's own body sets the flag in both branches
(`ellipse.c:1997`, `ellipse.c:2015`), so this is Circle-specific. The port
mirrors it, with the omission spelled out at `circle.rs:947-948` so it reads as
deliberate rather than dropped. Nothing in the corpus distinguishes the two
behaviours, because reaching the branch at all requires a non-UnitMap
base-to-current Mapping that still leaves a circle a circle — and such a Mapping
is normally simplified by the parent, which sets the flag anyway.

## Row 8 — the centre is transformed through the simplified Mapping

C calls `astRegTransform( this, this->points, 1, NULL, NULL )` on the *supplied*
Region, using its original base-to-current Mapping. The port transforms the
parent-simplified copy's points through the copy's Mapping (`circle.rs:936`).

The two agree: `Region::Simplify` alters only the encapsulated FrameSet's
Mapping and the uncertainty (see `region_mod.md` rows 5-8), never the defining
points or the base Frame, and a simplified Mapping computes the same
transformation. The difference is which of two equal-valued Mappings does the
arithmetic, so results can differ only in the last bits, and only when the
parent actually simplified something.
