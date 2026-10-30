# `MapMerge` audit — SlaMap

C source: reference/starlink-ast/src/slamap.c
Rust: crates/starlink-ast/src/mappings/slamap.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3d3f255f
Extension points: classified

Uncited C steps:
- slamap.c:2680 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `slamap.c:2709-3205 (fn MapMerge)`, the same architecture as SpecMap and
TimeMap: gather the conversion steps of a run of adjacent SlaMaps, normalize
each for its applied direction, simplify the accumulated list until nothing more
goes, and rebuild one SlaMap or a UnitMap.

`SlaMap::map_merge` (`slamap.rs:1504-1511 (fn map_merge)`) delegates to `sla_map_merge`
(`slamap.rs:734-786 (fn sla_map_merge)`); the helpers are `sla_normalized_steps`
(`slamap.rs:622-649 (fn sla_normalized_steps)`), `sla_simplify_steps`
(`slamap.rs:656-724 (fn sla_simplify_steps)`) and `try_cancel_pair`
(`slamap.rs:582-614 (fn try_cancel_pair)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | slamap.c:2709-2711 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | slamap.c:2712-2714 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | SlaMaps merge only in series, or when only one Mapping is present | slamap.c:2715-2719 (fn MapMerge) | slamap.rs:739-741 (fn sla_map_merge) | architecture | ported |
| 4 | Initialize the step count to the number in the nominated SlaMap | slamap.c:2720-2723 (fn MapMerge) | slamap.rs:756 (fn sla_map_merge),761 (fn sla_map_merge) | architecture | ported |
| 5 | Search adjacent lower-numbered Mappings until one is not an SlaMap, accumulating step counts | slamap.c:2724-2734 (fn MapMerge) | slamap.rs:746-749 (fn sla_map_merge) | architecture | ported |
| 6 | Search adjacent higher-numbered Mappings the same way | slamap.c:2735-2743 (fn MapMerge) | slamap.rs:750-753 (fn sla_map_merge) | architecture | ported |
| 7 | Remember the initial number of transformation steps | slamap.c:2744-2746 (fn MapMerge) | slamap.rs:756 (fn sla_map_merge),761 (fn sla_map_merge) | architecture | ported |
| 8 | Allocate memory for the accumulated step list | slamap.c:2747-2752 (fn MapMerge) | -- | ceremony | n/a: a `Vec` grows as the steps are appended |
| 9 | Loop to obtain the transformation data for each SlaMap being merged | slamap.c:2753-2756 (fn MapMerge) | slamap.rs:758-765 (fn sla_map_merge) | architecture | ported |
| 10 | Take each SlaMap and note whether it is being applied in its inverse direction | slamap.c:2757-2761 (fn MapMerge) | slamap.rs:759-761 (fn sla_map_merge) | architecture | ported |
| 11 | Set loop limits and an increment so the steps are scanned forward or in reverse as that direction dictates | slamap.c:2762-2768 (fn MapMerge) | slamap.rs:623-628 (fn sla_normalized_steps) | architecture | ported |
| 12 | Loop through each transformation step in the SlaMap | slamap.c:2769-2771 (fn MapMerge) | slamap.rs:626-629 (fn sla_normalized_steps) | architecture | ported |
| 13 | Free any extra information cached with the conversion step | slamap.c:2772-2775 (fn MapMerge) | slamap.rs:1158-1167 (fn new) | ceremony | n/a: the star-independent parameters are rebuilt on each transform rather than stored on the step, and the per-thread `palMappa` cache is keyed by the arguments, so a merge has no cache to discard |
| 14 | Store the step's type code, obtain its argument count from `CvtString`, and copy the arguments | slamap.c:2776-2785 (fn MapMerge) | slamap.rs:630 (fn sla_normalized_steps),633-643 (fn sla_normalized_steps) | architecture | ported |
| 15 | An inverted SlaMap must not only accumulate its steps in reverse but apply each in reverse; define macros for the changes that needs | slamap.c:2786-2792 (fn MapMerge) | slamap.rs:623-625 (fn sla_normalized_steps),626 (fn sla_normalized_steps) | architecture | ported |
| 16 | Macro swapping two nominated arguments where the type code matches | slamap.c:2793-2801 (fn MapMerge) | slamap.rs:635-639 (fn sla_normalized_steps) | architecture | ported |
| 17 | Macro exchanging a transformation type code for its inverse | slamap.c:2802-2810 (fn MapMerge) | slamap.rs:630 (fn sla_normalized_steps) | architecture | ported |
| 18 | Apply the macros only where the SlaMap is inverted | slamap.c:2811-2813 (fn MapMerge) | slamap.rs:623-625 (fn sla_normalized_steps) | architecture | ported |
| 19 | Exchange addition and subtraction of the E-terms of aberration | slamap.c:2814-2818 (fn MapMerge) | slamap.rs:248-249 (fn inverse) | architecture | ported |
| 20 | Exchange the starting and ending Besselian epochs of the FK4 precession model | slamap.c:2819-2823 (fn MapMerge) | slamap.rs:631-639 (fn sla_normalized_steps) | architecture | ported |
| 21 | Exchange the starting and ending epochs of the FK5 precession model | slamap.c:2824-2828 (fn MapMerge) | slamap.rs:631-639 (fn sla_normalized_steps) | architecture | ported |
| 22 | Exchange FK4-to-FK5 for its inverse, and vice versa | slamap.c:2829-2833 (fn MapMerge) | slamap.rs:252-253 (fn inverse) | architecture | ported |
| 23 | For geocentric apparent, exchange the code and also the order of the date and equinox arguments | slamap.c:2834-2841 (fn MapMerge) | slamap.rs:254-255 (fn inverse),629-637 (fn sla_normalized_steps) | architecture | ported |
| 24 | Exchange the code for its inverse for the ten remaining families: ecliptic, horizon, galactic, ICRS, supergalactic, the two helioprojective conversions, helio-ecliptic, dynamical J2000 and hour angle | slamap.c:2842-2893 (fn MapMerge) | slamap.rs:256-276 (fn inverse) | architecture | ported |
| 25 | Undefine the local macros | slamap.c:2894-2897 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 26 | Count the accumulated transformation steps | slamap.c:2898-2902 (fn MapMerge) | slamap.rs:763-764 (fn sla_map_merge) | architecture | ported |
| 27 | Loop, simplifying the step sequence until no further improvement is possible | slamap.c:2903-2907 (fn MapMerge) | slamap.rs:658-722 (fn sla_simplify_steps) | architecture | ported |
| 28 | Examine each remaining transformation step in turn | slamap.c:2908-2911 (fn MapMerge) | slamap.rs:661 (fn sla_simplify_steps) | architecture | ported |
| 29 | Initially assume the current step will be retained | slamap.c:2912-2914 (fn MapMerge) | -- | ceremony | n/a: the Rust pass removes the eliminated steps instead of marking the survivors |
| 30 | Drop a precession correction whose starting and ending epochs are the same | slamap.c:2915-2924 (fn MapMerge) | slamap.rs:704-717 (fn sla_simplify_steps) | architecture | ported |
| 31 | The remaining simplifications combine adjacent steps, so apply them only while two steps remain | slamap.c:2925-2929 (fn MapMerge) | slamap.rs:661 (fn sla_simplify_steps) | architecture | ported |
| 32 | Macro testing whether two adjacent type codes have given values | slamap.c:2930-2935 (fn MapMerge) | slamap.rs:596 (fn try_cancel_pair) | architecture | ported |
| 33 | Combine two adjacent precession corrections sharing an epoch into one | slamap.c:2936-2944 (fn MapMerge) | slamap.rs:674-682 (fn sla_simplify_steps) | architecture | ported |
| 34 | Retain the second correction with the first's starting epoch, and eliminate the first | slamap.c:2945-2949 (fn MapMerge) | slamap.rs:683-693 (fn sla_simplify_steps) | architecture | ported |
| 35 | Eliminate a matching pair of E-term corrections with the same argument | slamap.c:2950-2961 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 36 | Eliminate a matching pair of FK4/FK5 conversions with the same argument | slamap.c:2962-2972 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 37 | Eliminate a matching pair of ICRS/FK5 conversions | slamap.c:2973-2983 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 38 | Eliminate a matching pair of geocentric apparent conversions, whose argument order reverses between the two directions | slamap.c:2984-2997 (fn MapMerge) | slamap.rs:602-610 (fn try_cancel_pair) | architecture | ported |
| 39 | Eliminate a matching pair of ecliptic conversions | slamap.c:2998-3007 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 40 | Eliminate a matching pair of AzEl conversions, comparing both arguments | slamap.c:3008-3018 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 41 | Eliminate a matching pair of galactic conversions, which take no arguments | slamap.c:3019-3027 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 42 | Eliminate a matching pair of supergalactic conversions | slamap.c:3028-3035 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 43 | Eliminate a matching pair of helioprojective-Cartesian conversions, comparing four arguments | slamap.c:3036-3050 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 44 | The same for helioprojective-Radial | slamap.c:3051-3065 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 45 | Eliminate a matching pair of helio-ecliptic conversions | slamap.c:3066-3074 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 46 | Eliminate a matching pair of dynamical J2000 conversions | slamap.c:3075-3081 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 47 | Eliminate a matching pair of hour angle conversions | slamap.c:3082-3092 (fn MapMerge) | slamap.rs:596-613 (fn try_cancel_pair) | architecture | ported |
| 48 | Undefine the local macro | slamap.c:3093-3096 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 49 | A step being kept advances the index identifying its new location | slamap.c:3097-3102 (fn MapMerge) | slamap.rs:700 (fn sla_simplify_steps) | architecture | ported |
| 50 | Copy the step's data down when its new location differs from its current one | slamap.c:3103-3114 (fn MapMerge) | slamap.rs:664-665 (fn sla_simplify_steps),690 (fn sla_simplify_steps) | architecture | ported |
| 51 | Quit when a pass reduced no steps | slamap.c:3115-3119 (fn MapMerge) | slamap.rs:719-721 (fn sla_simplify_steps) | architecture | ported |
| 52 | Note how many transformation steps now remain | slamap.c:3120-3123 (fn MapMerge) | -- | ceremony | n/a: the surviving steps are the `Vec`'s contents, so its length is the count |
| 53 | Determine how many Mappings condensing the run eliminates | slamap.c:3124-3128 (fn MapMerge) | slamap.rs:767 (fn sla_map_merge) | architecture | ported |
| 54 | Determine whether the replacement can be a UnitMap, which needs every step eliminated | slamap.c:3129-3133 (fn MapMerge) | slamap.rs:769 (fn sla_map_merge) | architecture | ported |
| 55 | Determine whether simplification is possible, on any of the four clauses | slamap.c:3134-3142 (fn MapMerge) | slamap.rs:770 (fn sla_map_merge) | architecture | ported |
| 56 | Do nothing more unless simplification is possible | slamap.c:3143-3145 (fn MapMerge) | slamap.rs:771-773 (fn sla_map_merge) | architecture | ported |
| 57 | Create the UnitMap replacement where that is the answer | slamap.c:3146-3150 (fn MapMerge) | slamap.rs:775-776 (fn sla_map_merge) | architecture | ported |
| 58 | Otherwise create a replacement SlaMap and add each surviving step to it | slamap.c:3151-3160 (fn MapMerge) | slamap.rs:778-782 (fn sla_map_merge) | architecture | ported |
| 59 | Annul the pointers to the Mappings being eliminated | slamap.c:3161-3166 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 60 | Insert the replacement pointer with a cleared invert value | slamap.c:3167-3170 (fn MapMerge) | slamap.rs:784 (fn sla_map_merge) | architecture | ported |
| 61 | Move the subsequent Mapping information down to close the gap | slamap.c:3171-3176 (fn MapMerge) | slamap.rs:784 (fn sla_map_merge) | architecture | ported |
| 62 | Blank out the information left at the end of the arrays | slamap.c:3177-3182 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entries leave no slots to blank |
| 63 | Decrement the Mapping count and return the index of the first eliminated Mapping | slamap.c:3183-3187 (fn MapMerge) | slamap.rs:784-785 (fn sla_map_merge) | architecture | ported |
| 64 | Annul the new Mapping if an error occurred | slamap.c:3188-3194 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 65 | Free the memory used for the transformation steps | slamap.c:3195-3200 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 66 | Clear the returned value if an error occurred | slamap.c:3201-3203 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 67 | Return the result | slamap.c:3204-3205 (fn MapMerge) | slamap.rs:785 (fn sla_map_merge) | ceremony | ported |

## Row 23 — inverting a geocentric apparent step exchanges its arguments too

`AST__SLA_AMP( DATE, EQ )` and `AST__SLA_MAP( EQ, DATE )` carry the same pair of
numbers in opposite orders (`slamap.c:698 (fn AddSlaCvt)`,692), and the transform reads
them that way round: the port's `pal::mappa` call takes `(args[0], args[1])` for
`Map` and `(args[1], args[0])` for `Amp` (`slamap.rs:1161 (fn new)`,1040). So
inverting such a step means exchanging its two arguments as well as its type
code, which is why C pairs `SWAP_CODES( AST__SLA_AMP, AST__SLA_MAP )` with
`SWAP_ARGS` on both codes. Those are two of only four `SWAP_ARGS` invocations in
the class; the other two are the precession models of rows 20 and 21.

`sla_normalized_steps` swaps the first two arguments for all four
(`slamap.rs:631-639 (fn sla_normalized_steps)`). It previously swapped them for the
precession models alone, which transposed the date and equinox of an inverted
`Amp` or `Map` step — changing both the dumped SlaMap and the transform the
replacement applies.

## Rows 35 to 47 — one generic rule where C writes thirteen

C spells out a separate elimination rule per conversion family, each testing that
the two codes are a matching pair and that as many arguments as that family takes
agree. `try_cancel_pair` (`slamap.rs:582-614 (fn try_cancel_pair)`) does it once, from the
type's own `inverse()`, comparing every argument the step stores — which is the
count `nargs` gives for its type (`slamap.rs:143-165 (fn nargs)`), and so is C's
per-family count: one for the E-terms, FK4/FK5, ICRS/FK5, ecliptic,
helio-ecliptic and hour angle; two for AzEl; four for the two helioprojective
families; none for galactic, supergalactic and dynamical J2000.

Two conventions survive as special cases because C uses two. The geocentric
apparent pair is compared crosswise, `a.args[0]` against `b.args[1]` and back
(`slamap.rs:602-610 (fn try_cancel_pair)`), which is C's note at `slamap.c:2986 (fn MapMerge)` that the argument
order reverses between the directions. Precession is compared crosswise too
(`slamap.rs:588-593 (fn try_cancel_pair)`) because its inverse is itself with the epochs
swapped, so `PREC(e1,e2)` followed by `PREC(e2,e1)` cancels — C reaches that same
elimination as a combination (row 33) leaving `PREC(e1,e1)`, which row 30 then
drops.

## Rows 33 and 34 — which of the two precession steps survives

C keeps the second correction, copies the first's starting epoch into it, and
lets the first fall away. The port rewrites the first step's ending epoch and
removes the second (`slamap.rs:683-693 (fn sla_simplify_steps)`). Both leave one
`PREC(e1,e3)` where the pair was, in the same position in the sequence, so the
step lists agree.

The port also treats `e1 == e3` as a cancellation of both steps rather than
producing `PREC(e1,e1)`. C produces the identity step and eliminates it on its
next pass through row 30, and both loops run to a fixed point, so the two reach
the same surviving list. The port keeps the row 30 sweep as well
(`slamap.rs:704-717 (fn sla_simplify_steps)`), for an identity precession that arrives in
the accumulated list rather than being made by a combination.

## Rows 29, 49 and 50 — compaction in place, or removal

C rewrites the step array in place: a surviving step is copied down over the
eliminated ones and `ikeep` tracks where the next survivor lands, so the pass
ends with the survivors packed at the front and `nstep` reset to `ikeep + 1`.
`sla_simplify_steps` removes the eliminated steps from the `Vec` instead, which
packs the survivors by construction. The two agree on the surviving sequence
because both keep the steps in their original relative order and both consume the
second step of a cancelling pair — C by incrementing `istep`, the port by
removing both entries at once.

## Row 55 — `where_inv` counts as simplification

C treats a nominated SlaMap applied inverted as simplified even when nothing else
changed, because the replacement is always stored forward. `slamap.rs:770 (fn sla_map_merge)`
includes `where_inv` in the `simpler` disjunction for that reason. It is the
list's flag, `(*invert_list)[where]`, not the object's, since `flatten` resets
every leaf to forward.

## Row 13 — the cached mean-to-apparent parameters

C stores the `palMappa` parameter array alongside the conversion step and frees it
here, because the merge may change the step's arguments and would otherwise leave
a cache that no longer matches them. The port builds that array per transform
call (`slamap.rs:1158-1167 (fn new)`), reusing a per-thread copy only while the
arguments repeat, so there is nothing held across a merge to invalidate.

## Row 3 — the `*nmap == 1` disjunct is unreachable

C's guard admits a parallel nomination when the list holds a single Mapping. The
only caller that passes `series == 0` is a parallel CmpMap's own `MapMerge`, whose
list always holds both components, and the base `Simplify` nominates with
`series = 1` (`mapping.c:16871-16883 (fn Simplify)`). So the disjunct cannot be reached, and
`sla_map_merge` delegating every parallel nomination to `default_map_merge` is
equivalent rather than narrower.

## A lone SlaMap reaches the same body

C gives SlaMap no `Simplify` vtab slot, so a SlaMap nominated on its own arrives
here as a run of one: `ngone` is zero and the `simpler` test reduces to "the step
list shortened, emptied, or the SlaMap is applied inverted". Cancelling adjacent
inverse steps, combining precessions, dropping identity precessions and
rebuilding forward is every reduction a lone SlaMap has.
