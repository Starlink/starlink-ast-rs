# `MapMerge` audit — SlaMap

C source: reference/starlink-ast/src/slamap.c
Rust: crates/ast-core/src/mappings/slamap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `slamap.c:2701-3197`. It gathers the conversion steps of a whole
run of adjacent SlaMaps into one list, normalising each SlaMap's steps for its
applied direction, then simplifies that list until no further improvement is
possible, then rebuilds one SlaMap (or a UnitMap) from what remains.

`SlaMap::map_merge` (`slamap.rs:1238-1245`) delegates to `sla_map_merge`
(`slamap.rs:577-629`); the step list is normalised by `sla_normalized_steps`
(`slamap.rs:467-497`) and reduced by `sla_simplify_steps` (`slamap.rs:499-575`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | slamap.c:2701-2702 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | slamap.c:2704-2705 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | SlaMaps merge only in series, or when there is a single Mapping | slamap.c:2707-2711 | slamap.rs:582-584 | ported |
| 4 | Initialise the step count from the nominated SlaMap | slamap.c:2712-2715 | slamap.rs:598-599 | ported |
| 5 | Search adjacent lower-numbered Mappings for SlaMaps, accumulating their step counts | slamap.c:2716-2726 | slamap.rs:589-592 | ported |
| 6 | Search adjacent higher-numbered Mappings the same way | slamap.c:2727-2735 | slamap.rs:593-597 | ported |
| 7 | Remember the initial number of steps | slamap.c:2736-2738 | slamap.rs:599,608 | ported |
| 8 | Allocate a list to accumulate every step from every SlaMap found | slamap.c:2739-2744 | slamap.rs:598 | ported |
| 9 | Take each SlaMap in the run and note whether it is applied inverted | slamap.c:2745-2753 | slamap.rs:601-604 | ported |
| 10 | Scan its steps forward or in reverse as the direction requires | slamap.c:2754-2760 | slamap.rs:467-493 | ported |
| 11 | Copy each step's type code and its arguments into the accumulated list | slamap.c:2761-2777 | slamap.rs:605-607 | ported |
| 12 | For an inverted SlaMap, also invert each step, not merely reverse their order | slamap.c:2778-2804 | slamap.rs:467-493 | ported |
| 13 | Exchange addition and subtraction of the E-terms of aberration | slamap.c:2806-2810 | slamap.rs:467-493 | ported |
| 14 | Exchange the Besselian starting and ending epochs for the FK4 precession model | slamap.c:2811-2815 | slamap.rs:483-493 | ported |
| 15 | Exchange the starting and ending epochs for the FK5 precession model | slamap.c:2816-2820 | slamap.rs:483-493 | ported |
| 16 | Exchange FK4-to-FK5 for its inverse and back | slamap.c:2821-2825 | slamap.rs:467-493 | ported |
| 17 | For geocentric apparent, exchange the code and also the order of the date and equinox arguments | slamap.c:2826-2833 | slamap.rs:449-457 | ported |
| 18 | Exchange the code for its inverse for ecliptic, horizon, galactic, ICRS, supergalactic, the three helioprojective conversions, dynamical J2000 and HA-to-RA | slamap.c:2834-2885 | slamap.rs:467-493 | ported |
| 19 | Count the accumulated transformation steps | slamap.c:2890-2894 | slamap.rs:608 | ported |
| 20 | Loop, simplifying the step sequence until no further improvement is possible | slamap.c:2895-2899 | slamap.rs:499-504 | ported |
| 21 | Examine each remaining step in turn, assuming at first it will be kept | slamap.c:2900-2906 | slamap.rs:499-575 | ported |
| 22 | Drop a precession correction whose starting and ending epochs are the same | slamap.c:2907-2916 | slamap.rs:546-560 | ported |
| 23 | Apply the remaining, pairwise rules only while at least two steps remain | slamap.c:2917-2927 | slamap.rs:504-505 | ported |
| 24 | Combine two adjacent precession corrections sharing an equinox into one | slamap.c:2928-2941 | slamap.rs:516-545 | ported |
| 25 | Eliminate a matching pair of E-term corrections with the same argument | slamap.c:2942-2953 | slamap.rs:429-460 | ported |
| 26 | Eliminate a matching pair of FK4/FK5 conversions with the same argument | slamap.c:2954-2964 | slamap.rs:429-460 | ported |
| 27 | Eliminate a matching pair of ICRS/FK5 conversions | slamap.c:2965-2975 | slamap.rs:429-460 | ported |
| 28 | Eliminate a matching pair of geocentric apparent conversions, whose argument order reverses between directions | slamap.c:2976-2989 | slamap.rs:449-457 | ported |
| 29 | Eliminate a matching pair of ecliptic conversions | slamap.c:2990-2999 | slamap.rs:429-460 | ported |
| 30 | Eliminate a matching pair of AzEl conversions | slamap.c:3000-3010 | slamap.rs:429-460 | ported |
| 31 | Eliminate a matching pair of galactic conversions, which take no arguments | slamap.c:3011-3019 | slamap.rs:429-460 | ported |
| 32 | Eliminate a matching pair of supergalactic conversions | slamap.c:3020-3027 | slamap.rs:429-460 | ported |
| 33 | Eliminate matching pairs of the three helioprojective conversions | slamap.c:3028-3066 | slamap.rs:429-460 | ported |
| 34 | Eliminate a matching pair of dynamical J2000 conversions | slamap.c:3067-3073 | slamap.rs:429-460 | ported |
| 35 | Eliminate a matching pair of Hour Angle conversions | slamap.c:3074-3084 | slamap.rs:429-460 | ported |
| 36 | Compact the surviving steps down over the eliminated ones | slamap.c:3089-3106 | slamap.rs:499-575 | ported |
| 37 | Stop iterating when a pass reduced nothing | slamap.c:3107-3111 | slamap.rs:499-504 | ported |
| 38 | Note how many steps remain | slamap.c:3112-3115 | slamap.rs:612-613 | ported |
| 39 | Count how many Mappings condensing the run eliminates | slamap.c:3116-3120 | slamap.rs:610 | ported |
| 40 | Decide whether the replacement can be a UnitMap, when no steps remain | slamap.c:3121-3125 | slamap.rs:612 | ported |
| 41 | Decide whether anything simplified: Mappings eliminated, or fewer steps than before | slamap.c:3126-3134 | slamap.rs:613 | ported |
| 42 | Do nothing more unless something simplified | slamap.c:3135-3137 | slamap.rs:614-616 | ported |
| 43 | Create the UnitMap replacement if that is the answer | slamap.c:3138-3142 | slamap.rs:619-621 | ported |
| 44 | Otherwise create a replacement SlaMap and add each surviving step to it | slamap.c:3143-3152 | slamap.rs:621-625 | ported |
| 45 | Annul the eliminated Mappings and insert the replacement with its invert value | slamap.c:3153-3162 | slamap.rs:627-640 | ported |
| 46 | Close the gap, blank the tail, and return the index of the first eliminated Mapping | slamap.c:3163-3179 | slamap.rs:627-640 | ported |
| 47 | Annul the new Mapping if an error occurred | slamap.c:3180-3186 | -- | n/a: an error is a `Result::Err`, and owned values are dropped |
| 48 | Free the step list | slamap.c:3187-3192 | -- | n/a: owned values are dropped |
| 49 | Clear the returned value if an error occurred | slamap.c:3193-3195 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 50 | Return the result | slamap.c:3196-3197 | slamap.rs:660 | ported |

## Rows 25 to 35 — one generic rule where C writes eleven

C spells out a separate elimination rule per conversion family, each testing that
the two codes are a matching pair and that their arguments agree.
`try_cancel_pair` (`slamap.rs:429-460`) does it once, from the type's own
`inverse()`, with the two argument conventions C's families actually use: matching
in order for most, and reversed for the Amp/Map pair, which is C's geocentric
apparent case at `:2976-2989`. Precession is handled separately because its
inverse is itself with the epochs swapped.

That is a collapse of ceremony, not of architecture: the rule is one rule, and
adding a conversion type gets the elimination for free rather than needing a new
branch. What it costs is that a family C deliberately excluded would also be
eliminated here — no such family exists in `slamap.c` today.

## Row 41 — `where_inv` counts as simplification

C treats a nominated SlaMap applied inverted as simplified even when nothing else
changed, because the replacement is stored forward. `slamap.rs:613` includes
`where_inv` in the `simpler` disjunction for that reason.

## A lone SlaMap reaches the same body

C gives SlaMap no `Simplify` vtab slot, so a SlaMap nominated on its own arrives
here with `imap1 == imap2 == where_`: the run is one element, `ngone` is zero, and
the `simpler` disjunction reduces to "the step list shortened, emptied, or the
SlaMap is applied inverted". That is every reduction a lone SlaMap has — cancelling
adjacent inverse steps, combining precessions, dropping identity precessions, and
rebuilding forward — so there is nothing it can do that the run case does not.
