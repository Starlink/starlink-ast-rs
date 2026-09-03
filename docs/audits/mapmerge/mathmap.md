# `MapMerge` audit — MathMap

C source: reference/starlink-ast/src/mathmap.c
Rust: crates/ast-core/src/mappings/mathmap/mod.rs
Audited: 2026-08-24 against pin d0346c0d

C's body runs `mathmap.c:4029-4182` and does one thing: cancel a MathMap against
the MathMap that immediately follows it in series, when the two are inverses of
each other and both permit the cancellation, replacing the pair with a UnitMap.
There is no replace-with-simpler step and no lower-neighbour case.

`MathMap::map_merge` (`mathmap/mod.rs:421-432`) calls `default_map_merge`
(`mappings/simplify.rs:1336-1431`), whose neighbour attempt reaches
`MathMap::try_merge` (`mathmap/mod.rs:451-524`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | mathmap.c:4029-4030 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | mathmap.c:4032-4033 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Initialise the locals to silence uninitialised-variable warnings | mathmap.c:4035-4045 | -- | n/a: Rust locals are bound where they are computed, so none can be read before assignment |
| 4 | MathMaps are only worth simplifying in series | mathmap.c:4047-4048 | mathmap/mod.rs:458-460 | ported |
| 5 | Take the nominated index and the one after it, and check that a Mapping exists there | mathmap.c:4050-4057 | mappings/simplify.rs:1337-1341 | ported |
| 6 | Check that both Mappings are of class MathMap | mathmap.c:4059-4066 | mathmap/mod.rs:461 | ported |
| 7 | Obtain pointers to the two MathMaps and their associated invert flags | mathmap.c:4068-4074 | mappings/simplify.rs:1348-1349 | ported |
| 8 | Take SimpFI or SimpIF from each according to its invert flag, and require both to permit simplification | mathmap.c:4076-4083 | mathmap/mod.rs:463-477 | ported |
| 9 | Compare the first MathMap's effective input count with the second's effective output count | mathmap.c:4085-4098 | mathmap/mod.rs:479-484 | ported |
| 10 | Compare the first's effective number of forward functions with the second's effective number of inverse functions | mathmap.c:4100-4111 | mathmap/mod.rs:497-499 | ported |
| 11 | Compare the two sets of function strings, forward of the first against inverse of the second | mathmap.c:4113-4126 | mathmap/mod.rs:486-502 | ported |
| 12 | Repeat that comparison for the inverse of the first against the forward of the second | mathmap.c:4128-4143 | mathmap/mod.rs:504-520 | ported |
| 13 | If the two can be merged, create a UnitMap as the replacement | mathmap.c:4145-4148 | mathmap/mod.rs:522-523 | ported |
| 14 | Annul the pointers to the two original MathMaps | mathmap.c:4150-4153 | -- | n/a: owned values are dropped |
| 15 | Store the UnitMap in place of the first of the pair, with a cleared invert flag | mathmap.c:4155-4158 | mappings/simplify.rs:1422-1427 | ported |
| 16 | Shuffle the following Mappings and invert flags down to close the gap | mathmap.c:4160-4165 | mappings/simplify.rs:1428 | ported |
| 17 | Clear the final entry in each array | mathmap.c:4167-4169 | -- | n/a: the Rust list is a `Vec`, so removing the second of the pair shortens it; there is no vacated slot to clear |
| 18 | Decrement the Mapping count and return the index of the first modified element | mathmap.c:4171-4174 | mappings/simplify.rs:1430 | ported |
| 19 | Clear the returned value if an error occurred | mathmap.c:4178-4179 | -- | n/a: an error is an `Err`, not a value that has to be scrubbed on the way out |
| 20 | Return the result | mathmap.c:4181-4182 | mathmap/mod.rs:431 | ported |
| 21 | Setting Seed, SimpFI or SimpIF clears the simplified record | mathmap.c:5616-5629, :5698-5705, :5771-5778 | mathmap/mod.rs:182-188, :198-200, :211-214, :227 | ported |

## Row 8 — the flag pairing is asymmetric

C reads `SimpIF` from the first MathMap when its invert flag is set and `SimpFI`
when it is clear, and the *opposite* pair from the second
(`mathmap.c:4079-4082`). The two attributes say which composition order the
author certified as cancelling, so which one applies depends on the side of the
pair as well as the direction. `mathmap/mod.rs:463-473` keeps both halves of that
pairing.

## Row 13 — one UnitMap, not the pair of them

C replaces the first entry with the UnitMap and closes the gap, so the pair
becomes a single Mapping. `MathMap::try_merge` returns that one Mapping and
`default_map_merge` performs the same splice, which is why rows 15 to 18 all
point into the shared helper.

## A lone MathMap has no reduction of its own

Every rule above needs a neighbour. `mathmap.c` has no `Simplify` vtab slot, so
a MathMap on its own is returned unchanged, and the base `Simplify` reaches this
`MapMerge` by nominating it in a list of one (`mapping.c:16871-16883`). The port
matches that by implementing `map_merge` and leaving `Mapping::simplified` at its
default, which means a lone MathMap can never be stamped as simplified in
isolation.

## The upper neighbour only

C never looks at the Mapping *before* the nominated one: `imap1 = where` and
`imap2 = imap1 + 1` (`mathmap.c:4053-4055`). The lower-neighbour case is covered
by the driver instead, which nominates every element of the list in turn, so the
pair `(where-1, where)` is examined when `where-1` is nominated.
`default_map_merge` does additionally offer the reversed pair to the second
Mapping's own `try_merge` (`mappings/simplify.rs:1360-1369`), which for two MathMaps asks
the same question with the roles swapped; it reaches the same fixed point,
because the SimpFI / SimpIF pairing in row 8 is what decides the answer either
way.
