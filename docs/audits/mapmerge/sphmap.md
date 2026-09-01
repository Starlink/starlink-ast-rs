# `MapMerge` audit — SphMap

C source: reference/starlink-ast/src/sphmap.c
Rust: crates/ast-core/src/mappings/sphmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `sphmap.c:732-967` in two branches, both series-only: cancel a
SphMap against the SphMap that follows it in the opposite direction; and rewrite
a `SphMap(inverted), diagonal, SphMap(forward)` sandwich into
`WinMap, SphMap, SphMap`, adjusting `PolarLong`.

`SphMap::map_merge` (`sphmap.rs:263-270`) delegates to
`simplify::sph_map_merge` (`simplify.rs:2941-2960`); the sandwich body is
`try_sphmap_diagonal_pattern_at` (`simplify.rs:2854-2929`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | sphmap.c:732-733 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | sphmap.c:735-736 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Handle series only, and only the nominated SphMap with the Mapping that follows it | sphmap.c:743-747 | sphmap.rs:279-281 | ported |
| 4 | Take the indices of the two potential SphMaps | sphmap.c:748-751 | simplify.rs:1190-1285 | ported |
| 5 | Require the second Mapping to be a SphMap | sphmap.c:752-756 | sphmap.rs:286 | ported |
| 6 | An inverted SphMap followed by a forward one simplifies | sphmap.c:757-764 | sphmap.rs:309-316 | ported |
| 7 | A forward SphMap followed by an inverted one simplifies only under the extra condition C names | sphmap.c:765-774 | sphmap.rs:317-326 | ported |
| 8 | Create a UnitMap to replace the pair | sphmap.c:775-779 | sphmap.rs:327 | ported |
| 9 | Annul the two SphMap pointers | sphmap.c:780-784 | -- | n/a: owned values are dropped |
| 10 | Insert the replacement and initialise its invert flag | sphmap.c:785-789 | simplify.rs:1190-1285 | ported |
| 11 | Close the gap, clear the vacated slots, and return the first modified index | sphmap.c:790-808 | simplify.rs:1190-1285 | ported |
| 12 | Otherwise look for an inverted SphMap followed by a ZoomMap or diagonal MatrixMap and then a SphMap | sphmap.c:809-823 | simplify.rs:2854-2875 | ported |
| 13 | Require the third Mapping to be a non-inverted SphMap | sphmap.c:824-828 | simplify.rs:2865-2875 | ported |
| 14 | Require the middle Mapping to be a ZoomMap, or a diagonal MatrixMap whose diagonal elements have equal magnitude | sphmap.c:829-875 | simplify.rs:2878-2886, 2968-2982 | ported |
| 15 | Only proceed if all those conditions held | sphmap.c:876-878 | simplify.rs:2878-2886 | ported |
| 16 | Create a WinMap that modifies the longitude and latitude values | sphmap.c:879-882 | simplify.rs:2912-2914 | ported |
| 17 | Store scales and offsets that depend only on the signs of the diagonal elements | sphmap.c:883-912 | simplify.rs:2888-2902 | ported |
| 18 | The target shape is `(WinMap, SphMap, SphMap)`, leaving an inverted-then-forward SphMap pair | sphmap.c:913-926 | simplify.rs:2922-2927 | ported |
| 19 | Modify PolarLong to account for the change from `(SphMap, MatrixMap)` to `(WinMap, SphMap)` | sphmap.c:927-930 | simplify.rs:2906 | ported |
| 20 | Require the result to equal the second SphMap's PolarLong | sphmap.c:931-933 | simplify.rs:2907-2909 | ported |
| 21 | Copy the first SphMap and set its new PolarLong | sphmap.c:934-940 | simplify.rs:2919-2920 | ported |
| 22 | Annul the SphMap and the MatrixMap or ZoomMap | sphmap.c:941-944 | -- | n/a: owned values are dropped |
| 23 | Put the modified SphMap in the slot the diagonal Mapping vacated | sphmap.c:945-949 | simplify.rs:2924,2927 | ported |
| 24 | Put the new WinMap where the first SphMap was | sphmap.c:950-953 | simplify.rs:2925 | ported |
| 25 | Return the index of the first modified element | sphmap.c:954-957 | simplify.rs:2956 | ported |
| 26 | Free resources | sphmap.c:958-962 | -- | n/a: owned values are dropped |
| 27 | Clear the returned result if an error occurred | sphmap.c:963-965 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 28 | Return the result | sphmap.c:966-967 | simplify.rs:2959 | ported |
| 29 | Setting or clearing PolarLong or UnitRadius clears the simplified record | sphmap.c:1394-1396, :1434-1436 | sphmap.rs:70-76, :86-90 | ported |

## Row 14 — the Invert flag is deliberately ignored

C reads the diagonal without consulting the middle Mapping's Invert flag
(`sphmap.c:822-823`), because only the *signs* of the diagonal elements matter to
the WinMap it builds and inversion does not change them. `extract_3d_diagonal`
(`simplify.rs:2968-2982`) uses the forward matrix or factor for the same reason.

## Row 20 — the PolarLong check is what makes the rewrite legal

The rewrite is only valid if transforming the first SphMap's `PolarLong` through
the new WinMap lands on the second SphMap's `PolarLong`. The port checks exactly
that at `simplify.rs:2906-2909`, and declines otherwise, so the sandwich is not
rewritten when the two SphMaps disagree about the pole.

## Row 12 — the sandwich is tried only after the direct merge fails

C guards the second branch on `!simpler`. `sph_map_merge` reaches it only after
`default_map_merge` returned `None`, which is the same ordering.

## A lone SphMap has no reduction of its own

Both of C's branches need neighbours — a following SphMap, or a diagonal Mapping
and a second SphMap. `sphmap.c` has no `Simplify` vtab slot, so a SphMap on its
own is returned unchanged, and the base `Simplify` reaches this `MapMerge` by
nominating it in a list of one (`mapping.c:16871-16883`). The port matches that
by implementing `map_merge` and leaving `Mapping::simplified` at its default, so
a lone SphMap can never be stamped as simplified in isolation.
