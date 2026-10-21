# `Simplify` / `GetDefUnc` audit — Region base class

C source: reference/starlink-ast/src/region.c
Rust: crates/ast-core/src/regions/mod.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- region.c:11116 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- region.c:3547 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

Two C bodies, both generic drivers every Region subclass reaches through the
vtab: `Simplify` (`region.c:11146-11355 (fn Simplify)`), which simplifies the
encapsulated FrameSet and then the uncertainty Region if that stays stable
across the Region's extent; and `GetDefUnc`
(`region.c:3557-3612 (fn GetDefUnc)`), the default uncertainty.

The Rust counterparts are `parent_region_simplify`
(`regions/mod.rs:1896-2012 (fn parent_region_simplify)`) — which every concrete
Region's `simplified` calls first, matching C where every subclass `Simplify`
calls the parent — and `default_bounding_box_unc`
(`regions/mod.rs:1338-1365 (fn default_bounding_box_unc)`) behind `get_def_unc`
(`regions/mod.rs:1375 (fn get_def_unc)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned pointer | region.c:11146-11148 (fn Simplify) | -- | ceremony | n/a: the Rust returns `Option<DynRegion>`, `None` meaning nothing changed |
| 2 | Check the global error status and bail out | region.c:11149-11151 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a pointer to the Region structure | region.c:11152-11154 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Take a deep copy of the supplied Region, so the returned pointer differs from the supplied one when anything simplifies | region.c:11155-11159 (fn Simplify) | regions/mod.rs:1899 (fn parent_region_simplify) | architecture | ported |
| 5 | Simplify the encapsulated FrameSet, noting whether any simplification took place | region.c:11160-11164 (fn Simplify) | regions/mod.rs:1907-1912 (fn parent_region_simplify) | architecture | ported |
| 6 | If so, annul the existing FrameSet and use the simpler one | region.c:11165-11171 (fn Simplify) | regions/mod.rs:1910-1911 (fn parent_region_simplify) | architecture | ported |
| 7 | A default uncertainty is simplified by deleting it, since it is regenerated on demand from the simplified Region | region.c:11172-11176 (fn Simplify) | -- | ceremony | n/a: C annuls its cached `defunc`; the port caches nothing, so `get_def_unc` already regenerates the default from whatever the Region now is |
| 8 | An uncertainty supplied explicitly is simplified in place instead | region.c:11177-11180 (fn Simplify) | regions/mod.rs:1917 (fn parent_region_simplify) | architecture | ported |
| 9 | Obtain the Region's uncertainty | region.c:11181-11183 (fn Simplify) | regions/mod.rs:1919 (fn parent_region_simplify) | architecture | ported |
| 10 | Get the base->current Mapping from `this` | region.c:11184-11186 (fn Simplify) | regions/mod.rs:1922-1924 (fn parent_region_simplify) | architecture | ported |
| 11 | Where that Mapping has different numbers of inputs and outputs — a PermMap taking a slice through a Region — the uncertainty has to be centred on the slice | region.c:11187-11193 (fn Simplify) | regions/mod.rs:1932 (fn parent_region_simplify) | architecture | ported |
| 12 | Get the current centre of the uncertainty in its current Frame, which is the base Frame of `this` | region.c:11194-11197 (fn Simplify) | regions/mod.rs:1933 (fn parent_region_simplify) | architecture | ported |
| 13 | Store that centre in a PointSet so it can be transformed | region.c:11198-11202 (fn Simplify) | regions/mod.rs:1935 (fn parent_region_simplify) | architecture | ported |
| 14 | Transform it into the current Frame of `this` and then back into the base Frame | region.c:11203-11207 (fn Simplify) | regions/mod.rs:1936-1937 (fn parent_region_simplify) | architecture | ported |
| 15 | Re-centre the uncertainty at that position | region.c:11208-11210 (fn Simplify) | regions/mod.rs:1938-1939 (fn parent_region_simplify) | architecture | ported |
| 16 | Free the PointSets | region.c:11211-11216 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 17 | Free the centre array and the Mapping pointer | region.c:11217-11219 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 18 | Try simplifying the uncertainty, proceeding only if it changed | region.c:11220-11224 (fn Simplify) | regions/mod.rs:1948-1949 (fn parent_region_simplify) | architecture | ported |
| 19 | A simplifiable uncertainty means its base->current Mapping is linear enough for the shape to survive the transformation, but that has been tested only at the uncertainty's centre; it has to hold across the whole of `this`, which is what re-centring at opposite corners of a test box measures | region.c:11225-11239 (fn Simplify) | regions/mod.rs:1950-1952 (fn parent_region_simplify) | architecture | ported |
| 20 | Note the original base-Frame centre of the simplified uncertainty | region.c:11240-11242 (fn Simplify) | regions/mod.rs:1953 (fn parent_region_simplify) | architecture | ported |
| 21 | Take the base-Frame axis count and allocate the test-box bounds | region.c:11243-11247 (fn Simplify) | regions/mod.rs:1954 (fn parent_region_simplify) | architecture | ported |
| 22 | Where `this` is bounded, use its base-Frame bounding box as the test box | region.c:11248-11252 (fn Simplify) | regions/mod.rs:1959-1960 (fn parent_region_simplify) | architecture | ported |
| 23 | Otherwise centre the test box on the uncertainty's own centre, sized from the uncertainty's bounding box | region.c:11253-11264 (fn Simplify) | regions/mod.rs:1962-1971 (fn parent_region_simplify) | architecture | ported |
| 24 | Re-centre the uncertainty at the test box's lower bounds, which are in the base Frame of `this` and so the current Frame of `sunc` | region.c:11265-11268 (fn Simplify) | regions/mod.rs:1976 (fn parent_region_simplify) | architecture | ported |
| 25 | Take the bounding box of the re-centred uncertainty within its current Frame | region.c:11269-11274 (fn Simplify) | regions/mod.rs:1977 (fn parent_region_simplify) | architecture | ported |
| 26 | Re-centre the uncertainty at the test box's upper bounds | region.c:11275-11278 (fn Simplify) | regions/mod.rs:1981 (fn parent_region_simplify) | architecture | ported |
| 27 | Take its bounding box again | region.c:11279-11283 (fn Simplify) | regions/mod.rs:1982 (fn parent_region_simplify) | architecture | ported |
| 28 | Get a pointer to the base Frame of `this` | region.c:11284-11286 (fn Simplify) | -- | ceremony | n/a: C fetches `bfrm` here and annuls it at `region.c:11327 (fn Simplify)` without ever reading it |
| 29 | Initialise the `ok` flag to indicate the simplified uncertainty should not be used | region.c:11287-11290 (fn Simplify) | -- | ceremony | n/a: the flag starts at zero only to cover the error path, and there is no ambient status here to fail |
| 30 | Check pointers can be referenced safely | region.c:11291-11293 (fn Simplify) | -- | ceremony | n/a: a failed step is an `Err`, so the code below it does not run |
| 31 | Now indicate that the simplified uncertainty should be used | region.c:11294-11296 (fn Simplify) | regions/mod.rs:1987 (fn parent_region_simplify) | architecture | ported |
| 32 | Loop round all axes of the base Frame of `this` | region.c:11297-11299 (fn Simplify) | regions/mod.rs:1988 (fn parent_region_simplify) | architecture | ported |
| 33 | Get the width of the two bounding boxes on this axis | region.c:11300-11303 (fn Simplify) | regions/mod.rs:1989-1990 (fn parent_region_simplify) | architecture | ported |
| 34 | Reject the simplified uncertainty where the widths differ by more than the arbitrary figure C calls 0.1% | region.c:11304-11314 (fn Simplify) | regions/mod.rs:1991-1994 (fn parent_region_simplify) | architecture | ported |
| 35 | Reinstate the original base-Frame centre of the simplified uncertainty | region.c:11315-11317 (fn Simplify) | regions/mod.rs:1998-2000 (fn parent_region_simplify) | architecture | ported |
| 36 | Free the centre and bounds arrays and the base Frame pointer | region.c:11318-11327 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 37 | Where it can be used, record a simplification and store the new uncertainty | region.c:11328-11335 (fn Simplify) | regions/mod.rs:2004-2007 (fn parent_region_simplify) | architecture | ported |
| 38 | Free the uncertainty pointers | region.c:11336-11340 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 39 | Return the new Region if anything simplified, otherwise a clone of the supplied pointer | region.c:11341-11349 (fn Simplify) | regions/mod.rs:2011 (fn parent_region_simplify) | architecture | ported |
| 40 | Annul the returned pointer if an error occurred | region.c:11350-11352 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err` |
| 41 | Return the result | region.c:11353-11355 (fn Simplify) | regions/mod.rs:2011 (fn parent_region_simplify) | ceremony | ported |
| 42 | Initialise the returned pointer | region.c:3557-3559 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no pointer to pre-clear |
| 43 | Check the global error status and bail out | region.c:3560-3562 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 44 | Get a pointer to the base Frame of the supplied Region | region.c:3563-3565 (fn GetDefUnc) | regions/mod.rs:1341 (fn default_bounding_box_unc) | architecture | ported |
| 45 | Get the number of base-Frame axes | region.c:3566-3568 (fn GetDefUnc) | regions/mod.rs:1340 (fn default_bounding_box_unc) | architecture | ported |
| 46 | Get the base-Frame bounding box through `astRegBaseBox2`, so the un-negated assumption reaches the component Regions of a Prism or CmpRegion | region.c:3569-3578 (fn GetDefUnc) | regions/mod.rs:1339 (fn default_bounding_box_unc) | architecture | ported |
| 47 | Create a Box covering 1.0E-6 of that bounding box, centred on its centre, with an unbounded axis contributing zero and a degenerate axis a half-width scaled from its centre | region.c:3579-3601 (fn GetDefUnc) | regions/mod.rs:1342-1364 (fn default_bounding_box_unc) | architecture | ported |
| 48 | Free the base Frame pointer and the bounds arrays | region.c:3602-3606 (fn GetDefUnc) | -- | ceremony | n/a: owned values are dropped |
| 49 | Return NULL if an error occurred | region.c:3607-3609 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err` |
| 50 | Return the required pointer | region.c:3610-3612 (fn GetDefUnc) | regions/mod.rs:1364 (fn default_bounding_box_unc) | ceremony | ported |

## Rows 21 and 45 — the axis count comes from the box

C asks for the base-Frame axis count directly: `astGetNin( this->frameset )` in
`Simplify` (`region.c:11244 (fn Simplify)`) and `astGetNaxes( bfrm )` in
`GetDefUnc` (`region.c:3567 (fn GetDefUnc)`), each followed by an `astMalloc` of
that many doubles. The port takes the count from the bounding box it already
holds — one bound per base-Frame axis — and its owned vectors need no separate
allocation step, so C's two lines become one in each body.

## Row 34 — the tolerance is C's, and C's comment understates it

C rejects the simplified uncertainty when
`|w1 - w2| > 0.005 * (|w1| + |w2|)`
(`region.c:11308 (fn Simplify)`). Its own comment calls that 0.1% and adds that
the figure is arbitrary; 0.005 of the sum of two widths is nearer 0.5% of their
mean. The port uses `0.005` and records both the constant and C's wording, so
the arithmetic follows C rather than C's comment.

## Row 47 — the guard has to test the two bounds asymmetrically

C skips an axis whose upper bound is `+DBL_MAX` or whose lower bound is
`-DBL_MAX` (`region.c:3587 (fn GetDefUnc)`), setting both output bounds to zero.
`is_bad(v)` is `v == BAD_VALUE || !v.is_finite()`
(`mappings/helpers/mod.rs:17-19 (fn is_bad)`) with `BAD_VALUE == -f64::MAX`, so
an `is_bad` test on its own covers only the lower half; `+f64::MAX` is an
ordinary finite number and takes the bounded branch, where it earns a half-width
of about `0.5e-6 * f64::MAX`.

The port carries C's test in the same asymmetric form
(`regions/mod.rs:1348 (fn default_bounding_box_unc)`). A magnitude test would
diverge for `lbnd == +DBL_MAX`, where C is bounded.

## Row 11 — re-centring is what makes a sliced Region's uncertainty usable

When the base->current Mapping has unequal input and output counts — a PermMap
taking a slice through a Region — the uncertainty must be re-centred inside the
Region's extent before it is simplified, or the stability test measures it
somewhere the Region does not reach. C does that by transforming the uncertainty
centre into the current Frame and back, and the port does the same.

## Row 23 — the fallback test box is the uncertainty's own width

C's comment calls this box "100 times the size of the uncertainty region", but
the arithmetic sets `delta` to half the width of the uncertainty's bounding box
and then spans `centre ± delta` (`region.c:11258-11262 (fn Simplify)`), which is
that bounding box's width exactly. The port mirrors the arithmetic. The
consequence matters: for an unbounded Region the stability test moves the
uncertainty by one of its own widths rather than a hundred, so it is a weaker
test than C's comment describes — and a Region that passes here in C passes here
in the port for the same reason.

C also indexes `orig_cen` directly in that loop, relying on every uncertainty
Region having a centre. `reg_centre_query` returns an `Option`, and the port
falls back to zeros (`regions/mod.rs:1963 (fn parent_region_simplify)`) where it
is `None` — unreachable for the bounded uncertainty Regions this branch handles,
and a defined result rather than a null dereference if it ever were not.

## Row 22 — the test box is the base-Frame box, not the current-Frame one

C takes the bounded branch's test box from `astRegBaseBox`
(`region.c:11251 (fn Simplify)`) and the unbounded branch's from
`astGetRegionBounds` (`region.c:11257 (fn Simplify)`). The two are different
boxes: the first is in the Region's base Frame, the second in the current Frame
of `sunc`, which is the base Frame of `this`. Both branches therefore produce
bounds in the base Frame of `this`, which is what rows 24 and 26 then pass to
`astRegCentre` with `AST__CURRENT`. The port keeps the same split —
`region_base_box` in the bounded branch, `get_region_bounds` in the other — so
the two Frames cannot be confused by a reader who sees only one branch.
