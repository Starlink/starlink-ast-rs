# `Simplify` audit — Circle

C source: reference/starlink-ast/src/circle.c
Rust: crates/ast-core/src/regions/circle.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- circle.c:1813 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Circle has no `MapMerge`; C gives it a `Simplify` body
(`circle.c:1829-1937 (fn Simplify)`) that calls the parent `Region::Simplify` and then
tries to re-express the circle in its current Frame. Transforming a circle need
not leave a circle, so C fits a new Circle to a boundary mesh, falls back to
fitting an Ellipse in two dimensions, and adopts a fit only when `astRegPins`
places every mesh point on the fitted boundary.

`Circle::simplified` (`circle.rs:1080-1158 (fn simplified)`) is the port of it.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the result | circle.c:1829-1831 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 2 | Check the global error status and bail out | circle.c:1832-1834 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a pointer to the supplied Region structure | circle.c:1835-1837 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | circle.c:1838-1841 (fn Simplify) | circle.rs:1080-1089 (fn simplified) | architecture | ported |
| 5 | Note that simplification took place if the parent returned a different pointer | circle.c:1842-1846 (fn Simplify) | circle.rs:1090 (fn simplified) | architecture | ported |
| 6 | Skip the whole re-definition when the base-to-current Mapping is a UnitMap | circle.c:1847-1854 (fn Simplify) | circle.rs:1099-1103 (fn simplified) | architecture | ported |
| 7 | Get a mesh of points covering the Circle in its current Frame | circle.c:1855-1857 (fn Simplify) | circle.rs:1107-1108 (fn simplified) | architecture | ported |
| 8 | Get the uncertainty Region in the current Frame | circle.c:1858-1861 (fn Simplify) | circle.rs:1111 (fn simplified) | architecture | ported |
| 9 | Transform the centre into the current Frame and copy its axis values out | circle.c:1862-1869 (fn Simplify) | circle.rs:1115-1118 (fn simplified) | architecture | ported |
| 10 | Clear `ok` if any transformed centre axis is `AST__BAD` | circle.c:1870-1875 (fn Simplify) | circle.rs:1119 (fn simplified) | architecture | ported |
| 11 | Fit the best Circle through the mesh, or nothing if the centre is bad | circle.c:1876-1879 (fn Simplify) | circle.rs:1124-1125 (fn simplified) | architecture | ported |
| 12 | See whether every mesh point falls on that Circle's boundary within the uncertainty | circle.c:1880-1883 (fn Simplify) | circle.rs:1126 (fn simplified) | architecture | ported |
| 13 | If so, adopt the fitted Circle — without recording a simplification | circle.c:1884-1887 (fn Simplify) | circle.rs:1129-1130 (fn simplified) | architecture | ported |
| 14 | Otherwise, in two dimensions, see whether an Ellipse can represent the mesh | circle.c:1888-1891 (fn Simplify) | circle.rs:1137 (fn simplified) | architecture | ported |
| 15 | Discard the Circle and fit the best Ellipse through the mesh | circle.c:1892-1896 (fn Simplify) | circle.rs:1138 (fn simplified) | architecture | ported |
| 16 | See whether every mesh point falls on that Ellipse's boundary | circle.c:1897-1900 (fn Simplify) | circle.rs:1139 (fn simplified) | architecture | ported |
| 17 | If so, adopt the Ellipse and record a simplification | circle.c:1901-1907 (fn Simplify) | circle.rs:1140-1141 (fn simplified) | architecture | ported |
| 18 | Free the fitted Region | circle.c:1908-1911 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 19 | Free the transformed centre, the centre array, the mesh, the uncertainty and the Mapping | circle.c:1912-1918 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 20 | On simplification, overlay the supplied Region's attributes onto the result and return it | circle.c:1919-1926 (fn Simplify) | circle.rs:1147-1149 (fn simplified) | architecture | ported |
| 21 | Otherwise return a clone of the supplied Region | circle.c:1927-1931 (fn Simplify) | circle.rs:1151-1152 (fn simplified) | architecture | ported |
| 22 | Annul the returned pointer if an error occurred | circle.c:1932-1934 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 23 | Return the result | circle.c:1935-1937 (fn Simplify) | circle.rs:1149 (fn simplified),1135 (fn simplified) | architecture | ported |

## Row 13 — adopting a fitted Circle does not count as a simplification

The Ellipse branch sets `simpler = 1` (`circle.c:1904 (fn Simplify)`); the Circle branch does
not (`circle.c:1885-1886 (fn Simplify)`). So a Circle that is re-fitted to a *Circle* is
adopted into `new` and then thrown away, because the `if( simpler )` test at
`circle.c:1923 (fn Simplify)` sees only whatever the parent Simplify reported. If the parent
simplified nothing, C returns the original Circle and the fit is discarded.

Ellipse's own body sets the flag in both branches
(`ellipse.c:1999 (fn Simplify)`, `ellipse.c:2017 (fn Simplify)`), so this is Circle-specific. The port
mirrors it, with the omission spelled out at `circle.rs:1127-1128 (fn simplified)` so it reads
as deliberate rather than dropped. Nothing in the corpus distinguishes the two
behaviours, because reaching the branch at all requires a non-UnitMap
base-to-current Mapping that still leaves a circle a circle — and such a Mapping
is normally simplified by the parent, which sets the flag anyway.

## Rows 13 and 14 — the second fit is reached only when the first was not adopted

C's structure is `if( circle fits ) { adopt } else if( ok && nc == 2 ) { fit an
ellipse }`, so a successful Circle ends the attempt. The port has two separate
`if` blocks rather than a chain, and carries `adopted`
(`circle.rs:1123 (fn simplified)`,976) to reproduce the `else`. It also tests `ok` before
calling `best_circle` (`circle.rs:1124 (fn simplified)`), which C gets for free: `newreg` is
`NULL` when `ok` is clear, so C's `if( newreg && … )` cannot fire.

## Row 9 — the centre is transformed through the simplified Mapping

C calls `astRegTransform( this, this->points, 1, NULL, NULL )` on the *supplied*
Region, using its original base-to-current Mapping. The port transforms the
parent-simplified copy's points through the copy's Mapping
(`circle.rs:1116 (fn simplified)`).

The two agree: `Region::Simplify` alters only the encapsulated FrameSet's
Mapping and the uncertainty (see `region_mod.md` rows 5-8), never the defining
points or the base Frame, and a simplified Mapping computes the same
transformation. The difference is which of two equal-valued Mappings does the
arithmetic, so results can differ only in the last bits, and only when the
parent actually simplified something.
