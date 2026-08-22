# `Simplify` / `GetDefUnc` audit — Region base class

C source: reference/starlink-ast/src/region.c
Rust: crates/ast-core/src/regions/mod.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Two C bodies, both generic drivers every Region subclass reaches: `Simplify`
(`region.c:11119-11327`), which simplifies the encapsulated FrameSet and then the
uncertainty Region if that stays stable across the Region's extent; and
`GetDefUnc` (`region.c:3538-3587`), the default uncertainty.

The Rust counterparts are `parent_region_simplify` (`regions/mod.rs:890-1003`) —
which every concrete Region's `simplified` calls first, matching C where every
subclass `Simplify` calls the parent — and `default_bounding_box_unc`
(`regions/mod.rs:461-483`) behind `get_def_unc` (`regions/mod.rs:489`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned pointer | region.c:11119-11120 | -- | n/a: the Rust returns `Option<DynRegion>`, `None` meaning nothing changed |
| 2 | Check the global error status and bail out | region.c:11122-11123 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a pointer to the Region structure | region.c:11125-11127 | -- | n/a: the Rust method already has `&self` |
| 4 | Deep-copy the supplied Region, so the returned pointer differs from the supplied one when anything changes | region.c:11128-11132 | regions/mod.rs:891-892 | ported |
| 5 | Simplify the encapsulated FrameSet, noting whether anything changed | region.c:11133-11137 | regions/mod.rs:896-900 | ported |
| 6 | If so, replace the FrameSet with the simpler one | region.c:11138-11144 | regions/mod.rs:901-906 | ported |
| 7 | A default uncertainty is simplified by deleting it, since it is regenerated on demand | region.c:11145-11149 | regions/mod.rs:908-911 | ported |
| 8 | An explicit uncertainty is simplified in place | region.c:11150-11153 | regions/mod.rs:912 | ported |
| 9 | Obtain the Region's uncertainty | region.c:11154-11156 | regions/mod.rs:913-914 | ported |
| 10 | Get the base-to-current Mapping of the original Region | region.c:11157-11159 | regions/mod.rs:915-916 | ported |
| 11 | When that Mapping has unequal input and output counts, re-centre the uncertainty so it is centred within the Region's extent | region.c:11160-11192 | regions/mod.rs:925-936 | ported |
| 12 | Try simplifying the uncertainty, proceeding only if it changed | region.c:11193-11197 | regions/mod.rs:937-941 | ported |
| 13 | A simplifiable uncertainty means its base-to-current Mapping is linear enough to absorb, but that has to hold across the whole Region | region.c:11198-11212 | regions/mod.rs:942-947 | ported |
| 14 | Note the simplified uncertainty's original base-Frame centre | region.c:11213-11215 | regions/mod.rs:942-947 | ported |
| 15 | Allocate the test-box bounds | region.c:11216-11220 | -- | n/a: owned vectors |
| 16 | Use this Region's base-Frame bounding box as the test box when it is available | region.c:11221-11225 | regions/mod.rs:948-950 | ported |
| 17 | Otherwise use a box 100 times the size of the uncertainty, centred on it | region.c:11226-11237 | regions/mod.rs:948-955 | ported |
| 18 | Re-centre the uncertainty at the test box's lower bounds and take its bounding box | region.c:11238-11247 | regions/mod.rs:966-971 | ported |
| 19 | Do the same at the upper bounds | region.c:11248-11256 | regions/mod.rs:972-976 | ported |
| 20 | Get this Region's base Frame | region.c:11257-11259 | regions/mod.rs:894 | ported |
| 21 | Assume at first that the simplified uncertainty will not be used | region.c:11260-11266 | regions/mod.rs:977-983 | ported |
| 22 | Compare the two bounding-box widths on each base-Frame axis | region.c:11270-11276 | regions/mod.rs:977-983 | ported |
| 23 | Reject the simplified uncertainty when the widths differ by more than the arbitrary 0.1% figure | region.c:11277-11287 | regions/mod.rs:977-983 | ported |
| 24 | Reinstate the simplified uncertainty's original centre | region.c:11288-11300 | regions/mod.rs:989-991 | ported |
| 25 | If it can be used, record a simplification and store the new uncertainty | region.c:11301-11313 | regions/mod.rs:993-999 | ported |
| 26 | Return the new Region if anything simplified, otherwise a clone | region.c:11314-11322 | regions/mod.rs:1000-1002 | ported |
| 27 | Annul the returned pointer if an error occurred | region.c:11323-11325 | -- | n/a: an error is a `Result::Err` |
| 28 | Return the result | region.c:11326-11327 | regions/mod.rs:1002 | ported |
| 29 | `GetDefUnc`: get the base Frame and its axis count | region.c:3544-3549 | regions/mod.rs:463-464 | ported |
| 30 | Get the base-Frame bounding box, honouring that `astRegBaseBox` assumes an un-negated Region | region.c:3550-3559 | regions/mod.rs:462 | ported |
| 31 | Build a Box covering 1.0E-6 of that bounding box, centred on its centre | region.c:3560-3566 | regions/mod.rs:465-473 | ported |
| 32 | Fall back to a half-width of `c * 0.5E-6` when the box is degenerate on an axis | region.c:3567 | regions/mod.rs:469-471 | ported |
| 33 | Set both bounds to zero on an axis whose bound is `±DBL_MAX` | region.c:3568-3573 | regions/mod.rs:466,474-477 | gap: PLAN.md §10.12 |
| 34 | Create the Box in the base Frame | region.c:3574-3577 | regions/mod.rs:479-482 | ported |
| 35 | Free resources and return NULL on error | region.c:3578-3585 | -- | n/a: owned values are dropped and an error is a `Result::Err` |
| 36 | Return the Box | region.c:3586-3587 | regions/mod.rs:482 | ported |

## Row 23 — the tolerance is C's, and C's comment understates it

C rejects the simplified uncertainty when
`|w1 - w2| > 0.005 * (|w1| + |w2|)`. Its own comment calls that 0.1% and adds
that the figure is arbitrary; 0.005 of the sum of two widths is nearer 0.5% of
their mean. The port uses `0.005` and records both the constant and C's wording,
so the arithmetic follows C rather than C's comment.

## Row 33 — the upper-bound guard does not catch an unbounded axis

C skips an axis whose bound is `±DBL_MAX`, setting both output bounds to zero.
The port's guard is `!is_bad(ubnd[i]) && !is_bad(lbnd[i])`, and
`is_bad(v)` is `v == BAD_VALUE || !v.is_finite()` (`mappings/helpers/mod.rs:11-13`)
with `BAD_VALUE == -f64::MAX`.

So the *lower* case is caught, because an unbounded lower bound is `-f64::MAX`
which is `BAD_VALUE`. The *upper* case is not: an unbounded upper bound is
`f64::MAX`, which is finite and is not `BAD_VALUE`, so the axis takes the bounded
branch and gets a half-width of about `0.5e-6 * f64::MAX`. See PLAN.md §10.12.

## Row 11 — re-centring is what makes a sliced Region's uncertainty usable

When the base-to-current Mapping has unequal input and output counts — a PermMap
taking a slice through a Region — the uncertainty must be re-centred inside the
Region's extent before it is simplified, or the stability test measures it
somewhere the Region does not reach. C does that by transforming the uncertainty
centre into the current Frame and back, and the port does the same.
