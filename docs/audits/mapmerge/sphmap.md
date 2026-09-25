# `MapMerge` audit — SphMap

C source: reference/starlink-ast/src/sphmap.c
Rust: crates/ast-core/src/mappings/sphmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- sphmap.c:716 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `sphmap.c:732-967 (fn MapMerge)` in two branches, both series-only: cancel a
SphMap against the SphMap that follows it in the opposite direction; and rewrite
a `SphMap(inverted), diagonal, SphMap(forward)` sandwich into
`WinMap, SphMap, SphMap`, adjusting `PolarLong`.

`SphMap::map_merge` (`sphmap.rs:283-290 (fn map_merge)`) delegates to
`simplify::sph_map_merge` (`mappings/simplify.rs:3099-3118 (fn sph_map_merge)`); the sandwich body is
`try_sphmap_diagonal_pattern_at` (`mappings/simplify.rs:3012-3087 (fn try_sphmap_diagonal_pattern_at)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 2 | Initialise the returned result to -1 | sphmap.c:732-733 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 3 | Check the global error status and bail out | sphmap.c:735-736 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Further initialisation | sphmap.c:738-741 (fn MapMerge) | -- | ceremony | n/a: the sentinels C clears here are C's own — a null replacement Mapping, a "simplified" flag and an axis count — and the Rust code has no state to clear before the search begins |
| 4 | Handle series only, and only the nominated SphMap with the Mapping that follows it | sphmap.c:743-747 (fn MapMerge) | sphmap.rs:299-301 (fn try_merge) | architecture | ported |
| 5 | Take the indices of the two potential SphMaps | sphmap.c:748-751 (fn MapMerge) | mappings/simplify.rs:1321-1395 (fn default_map_merge) | architecture | ported |
| 6 | Require the second Mapping to be a SphMap | sphmap.c:752-756 (fn MapMerge) | sphmap.rs:306 (fn try_merge) | architecture | ported |
| 7 | An inverted SphMap followed by a forward one simplifies | sphmap.c:757-764 (fn MapMerge) | sphmap.rs:329-336 (fn try_merge) | architecture | ported |
| 8 | A forward SphMap followed by an inverted one simplifies only under the extra condition C names | sphmap.c:765-774 (fn MapMerge) | sphmap.rs:337-346 (fn try_merge) | architecture | ported |
| 9 | Create a UnitMap to replace the pair | sphmap.c:775-779 (fn MapMerge) | sphmap.rs:347 (fn try_merge) | architecture | ported |
| 10 | Annul the two SphMap pointers | sphmap.c:780-784 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 11 | Insert the replacement and initialise its invert flag | sphmap.c:785-789 (fn MapMerge) | mappings/simplify.rs:1321-1395 (fn default_map_merge) | architecture | ported |
| 12 | Close the gap, clear the vacated slots, and return the first modified index | sphmap.c:790-808 (fn MapMerge) | mappings/simplify.rs:1321-1395 (fn default_map_merge) | architecture | ported |
| 13 | Otherwise look for an inverted SphMap followed by a ZoomMap or diagonal MatrixMap and then a SphMap | sphmap.c:809-823 (fn MapMerge) | mappings/simplify.rs:3017-3027 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 14 | Require the third Mapping to be a non-inverted SphMap | sphmap.c:824-828 (fn MapMerge) | mappings/simplify.rs:3029-3033 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 15 | Require the middle Mapping to be a ZoomMap, or a diagonal MatrixMap whose diagonal elements have equal magnitude | sphmap.c:829-875 (fn MapMerge) | mappings/simplify.rs:3035-3044 (fn try_sphmap_diagonal_pattern_at), mappings/simplify.rs:3126-3140 (fn extract_3d_diagonal) | architecture | ported |
| 16 | Only proceed if all those conditions held | sphmap.c:876-878 (fn MapMerge) | mappings/simplify.rs:3042-3044 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 17 | Create a WinMap that modifies the longitude and latitude values | sphmap.c:879-882 (fn MapMerge) | mappings/simplify.rs:3069-3072 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 18 | Store scales and offsets that depend only on the signs of the diagonal elements | sphmap.c:883-912 (fn MapMerge) | mappings/simplify.rs:3046-3060 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 19 | The target shape is `(WinMap, SphMap, SphMap)`, leaving an inverted-then-forward SphMap pair | sphmap.c:913-926 (fn MapMerge) | mappings/simplify.rs:3074-3085 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 20 | Modify PolarLong to account for the change from `(SphMap, MatrixMap)` to `(WinMap, SphMap)` | sphmap.c:927-930 (fn MapMerge) | mappings/simplify.rs:3064 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 21 | Require the result to equal the second SphMap's PolarLong | sphmap.c:931-933 (fn MapMerge) | mappings/simplify.rs:3065-3067 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 22 | Copy the first SphMap and set its new PolarLong | sphmap.c:934-940 (fn MapMerge) | mappings/simplify.rs:3077-3078 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 23 | Annul the SphMap and the MatrixMap or ZoomMap | sphmap.c:941-944 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 24 | Put the modified SphMap in the slot the diagonal Mapping vacated | sphmap.c:945-949 (fn MapMerge) | mappings/simplify.rs:3082 (fn try_sphmap_diagonal_pattern_at), mappings/simplify.rs:3085 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 25 | Put the new WinMap where the first SphMap was | sphmap.c:950-953 (fn MapMerge) | mappings/simplify.rs:3083 (fn try_sphmap_diagonal_pattern_at) | architecture | ported |
| 26 | Return the index of the first modified element | sphmap.c:954-957 (fn MapMerge) | mappings/simplify.rs:3114 (fn sph_map_merge) | architecture | ported |
| 27 | Free resources | sphmap.c:958-962 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 28 | Clear the returned result if an error occurred | sphmap.c:963-965 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 29 | Return the result | sphmap.c:966-967 (fn MapMerge) | mappings/simplify.rs:3117 (fn sph_map_merge) | architecture | ported |
| 30 | Setting or clearing PolarLong or UnitRadius clears the simplified record | sphmap.c:1394-1396 (fn astMAKE_CLEAR1), sphmap.c:1436-1438 (fn astMAKE_CLEAR1) | sphmap.rs:70-77 (fn set_unit_radius), sphmap.rs:100-105 (fn set_polar_long) | ceremony | ported |

## Row 15 — the Invert flag is deliberately ignored

C reads the diagonal without consulting the middle Mapping's Invert flag
(`sphmap.c:822-823 (fn MapMerge)`), because only the *signs* of the diagonal elements matter to
the WinMap it builds and inversion does not change them. `extract_3d_diagonal`
(`mappings/simplify.rs:3126-3140 (fn extract_3d_diagonal)`) uses the forward matrix or factor for the same reason.

## Row 21 — the PolarLong check is what makes the rewrite legal

The rewrite is only valid if transforming the first SphMap's `PolarLong` through
the new WinMap lands on the second SphMap's `PolarLong`. The port checks exactly
that at `mappings/simplify.rs:3064-3067 (fn try_sphmap_diagonal_pattern_at)`, and declines otherwise, so the sandwich is not
rewritten when the two SphMaps disagree about the pole.

## Row 13 — the sandwich is tried only after the direct merge fails

C guards the second branch on `!simpler`. `sph_map_merge` reaches it only after
`default_map_merge` returned `None`, which is the same ordering.

## A lone SphMap has no reduction of its own

Both of C's branches need neighbours — a following SphMap, or a diagonal Mapping
and a second SphMap. `sphmap.c` has no `Simplify` vtab slot, so a SphMap on its
own is returned unchanged, and the base `Simplify` reaches this `MapMerge` by
nominating it in a list of one (`mapping.c:16871-16883 (fn Simplify)`). The port matches that
by implementing `map_merge` and leaving `Mapping::simplified` at its default, so
a lone SphMap can never be stamped as simplified in isolation.
