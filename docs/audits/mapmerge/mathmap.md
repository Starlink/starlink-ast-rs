# `MapMerge` audit — MathMap

C source: reference/starlink-ast/src/mathmap.c
Rust: crates/ast-core/src/mappings/mathmap/mod.rs
Audited: 2026-08-24 against pin d0346c0d
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- mathmap.c:4006 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `mathmap.c:4029-4182 (fn MapMerge)` and does one thing: cancel a MathMap against
the MathMap that immediately follows it in series, when the two are inverses of
each other and both permit the cancellation, replacing the pair with a UnitMap.
There is no replace-with-simpler step and no lower-neighbour case.

`MathMap::map_merge` (`mathmap/mod.rs:483-494 (fn map_merge)`) calls `default_map_merge`
(`mappings/simplify.rs:1361-1456 (fn default_map_merge)`), whose neighbour attempt reaches
`MathMap::try_merge` (`mathmap/mod.rs:514-587 (fn try_merge)`).

The last row is not part of `MapMerge` but of the rule that governs it: an
attribute setter whose value feeds the class's own `MapMerge` must discard the
simplified record.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | mathmap.c:4029-4030 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | mathmap.c:4032-4033 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Initialise the locals to silence uninitialised-variable warnings | mathmap.c:4035-4045 (fn MapMerge) | -- | ceremony | n/a: Rust locals are bound where they are computed, so none can be read before assignment |
| 4 | MathMaps are only worth simplifying in series | mathmap.c:4047-4048 (fn MapMerge) | mathmap/mod.rs:521-523 (fn try_merge) | architecture | ported |
| 5 | Take the nominated index and the one after it, and check that a Mapping exists there | mathmap.c:4050-4057 (fn MapMerge) | mappings/simplify.rs:1362-1366 (fn default_map_merge) | architecture | ported |
| 6 | Check that both Mappings are of class MathMap | mathmap.c:4059-4066 (fn MapMerge) | mathmap/mod.rs:524 (fn try_merge) | architecture | ported |
| 7 | Obtain pointers to the two MathMaps and their associated invert flags | mathmap.c:4068-4074 (fn MapMerge) | mappings/simplify.rs:1373-1374 (fn default_map_merge) | architecture | ported |
| 8 | Take SimpFI or SimpIF from each according to its invert flag, and require both to permit simplification | mathmap.c:4076-4083 (fn MapMerge) | mathmap/mod.rs:526-540 (fn try_merge) | architecture | ported |
| 9 | Compare the first MathMap's effective input count with the second's effective output count | mathmap.c:4085-4098 (fn MapMerge) | mathmap/mod.rs:542-547 (fn try_merge) | architecture | ported |
| 10 | Compare the first's effective number of forward functions with the second's effective number of inverse functions | mathmap.c:4100-4111 (fn MapMerge) | mathmap/mod.rs:560-562 (fn try_merge) | architecture | ported |
| 11 | Compare the two sets of function strings, forward of the first against inverse of the second | mathmap.c:4113-4126 (fn MapMerge) | mathmap/mod.rs:549-565 (fn try_merge) | architecture | ported |
| 12 | Repeat that comparison for the inverse of the first against the forward of the second | mathmap.c:4128-4143 (fn MapMerge) | mathmap/mod.rs:567-583 (fn try_merge) | architecture | ported |
| 13 | If the two can be merged, create a UnitMap as the replacement | mathmap.c:4145-4148 (fn MapMerge) | mathmap/mod.rs:585-586 (fn try_merge) | architecture | ported |
| 14 | Annul the pointers to the two original MathMaps | mathmap.c:4150-4153 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 15 | Store the UnitMap in place of the first of the pair, with a cleared invert flag | mathmap.c:4155-4158 (fn MapMerge) | mappings/simplify.rs:1447-1452 (fn default_map_merge) | architecture | ported |
| 16 | Shuffle the following Mappings and invert flags down to close the gap | mathmap.c:4160-4165 (fn MapMerge) | mappings/simplify.rs:1453 (fn default_map_merge) | architecture | ported |
| 17 | Clear the final entry in each array | mathmap.c:4167-4169 (fn MapMerge) | -- | ceremony | n/a: the Rust list is a `Vec`, so removing the second of the pair shortens it; there is no vacated slot to clear |
| 18 | Decrement the Mapping count and return the index of the first modified element | mathmap.c:4171-4174 (fn MapMerge) | mappings/simplify.rs:1453-1455 (fn default_map_merge) | architecture | ported |
| 19 | Clear the returned value if an error occurred | mathmap.c:4178-4179 (fn MapMerge) | -- | ceremony | n/a: an error is an `Err`, not a value that has to be scrubbed on the way out |
| 20 | Return the result | mathmap.c:4181-4182 (fn MapMerge) | mathmap/mod.rs:493 (fn map_merge) | ceremony | ported |
| 21 | Setting or clearing Seed, SimpFI or SimpIF clears the simplified record | mathmap.c:5616-5629 (fn astMAKE_CLEAR1), mathmap.c:5701-5708 (fn astMAKE_CLEAR1), mathmap.c:5776-5783 (fn astMAKE_CLEAR1) | mathmap/mod.rs:187-197 (fn set_simp_fi), mathmap/mod.rs:205-235 (fn set_simp_if), mathmap/mod.rs:244-259 (fn set_seed), mathmap/mod.rs:268-272 (fn clear_seed) | ceremony | ported |

## Row 8 — the flag pairing is asymmetric

C reads `SimpIF` from the first MathMap when its invert flag is set and `SimpFI`
when it is clear, and the *opposite* pair from the second
(`mathmap.c:4079-4082 (fn MapMerge)`). The two attributes say which composition order the
author certified as cancelling, so which one applies depends on the side of the
pair as well as the direction. `mathmap/mod.rs:526-536 (fn try_merge)` keeps both halves of that
pairing.

## Row 13 — one UnitMap, not the pair of them

C replaces the first entry with the UnitMap and closes the gap, so the pair
becomes a single Mapping. `MathMap::try_merge` returns that one Mapping and
`default_map_merge` performs the same splice, which is why rows 15 to 18 all
point into the shared helper.

## A lone MathMap has no reduction of its own

Every rule above needs a neighbour. `mathmap.c` has no `Simplify` vtab slot, so
a MathMap on its own is returned unchanged, and the base `Simplify` reaches this
`MapMerge` by nominating it in a list of one (`mapping.c:16871-16883 (fn Simplify)`). The port
matches that by implementing `map_merge` and leaving `Mapping::simplified` at its
default, which means a lone MathMap can never be stamped as simplified in
isolation.

## The upper neighbour only

C never looks at the Mapping *before* the nominated one: `imap1 = where` and
`imap2 = imap1 + 1` (`mathmap.c:4053-4055 (fn MapMerge)`). The lower-neighbour case is covered
by the driver instead, which nominates every element of the list in turn, so the
pair `(where-1, where)` is examined when `where-1` is nominated.
`default_map_merge` does additionally offer the reversed pair to the second
Mapping's own `try_merge` (`mappings/simplify.rs:1385-1394 (fn default_map_merge)`), which for two MathMaps asks
the same question with the roles swapped; it reaches the same fixed point,
because the SimpFI / SimpIF pairing in row 8 is what decides the answer either
way.
