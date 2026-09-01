# `Simplify` audit — FrameSet

C source: reference/starlink-ast/src/frameset.c
Rust: crates/ast-core/src/frames/frameset.rs
Audited: 2026-08-24 against pin 07fbe2dc

`FrameSet` has no `MapMerge`; C gives it a `Simplify` body instead
(`frameset.c:10111-10238`), overriding the one it inherits from Frame. The
algorithm is not "compose base→current and simplify that": it takes a copy of
the FrameSet and simplifies each internal edge Mapping *individually*, leaving
the node graph intact, and returns the copy only if at least one edge reduced.

`FrameSet::simplified` (`frameset.rs:1598-1621`) is the whole port of it.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the result to NULL and check the global error status | frameset.c:10164-10169 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Obtain a pointer to the FrameSet structure | frameset.c:10171-10172 | -- | n/a: the Rust method already has `&self` |
| 3 | Take a deep copy of the FrameSet, since the Mappings in it will be altered | frameset.c:10174-10176 | frameset.rs:1602 | ported |
| 4 | Loop over each Mapping between the Frames of the copy | frameset.c:10178-10182 | frameset.rs:1603 | ported |
| 5 | Obtain that Mapping and the invert flag the FrameSet applies it with | frameset.c:10184-10186 | frameset.rs:1604 | ported |
| 6 | Note whether the Mapping's own Invert attribute is set, and its value | frameset.c:10188-10190 | -- | n/a: an `Edge` holds one Mapping already oriented `from`→`to` (`frameset.rs:35-42`), so there is no second flag to compare against |
| 7 | Set the Mapping's Invert attribute to the value the FrameSet requires | frameset.c:10192-10193 | -- | n/a: as row 6 — the edge Mapping is already in the orientation the edge applies |
| 8 | Simplify the Mapping | frameset.c:10195-10196 | frameset.rs:1604 | ported |
| 9 | Restore the Mapping's original Invert attribute, set or cleared as it was | frameset.c:10198-10206 | -- | n/a: nothing was mutated, so nothing is restored |
| 10 | Test whether simplification was performed, by pointer identity | frameset.c:10208-10210 | frameset.rs:1609 | ported |
| 11 | On success, substitute the new Mapping and record its Invert value as the new flag | frameset.c:10212-10216 | frameset.rs:1610 | ported |
| 12 | Note that some Mapping in the FrameSet simplified | frameset.c:10218-10220 | frameset.rs:1611 | ported |
| 13 | Annul the simplified Mapping pointer | frameset.c:10222-10224 | -- | n/a: owned values are dropped |
| 14 | Return the copy if anything simplified, otherwise the original FrameSet | frameset.c:10226-10229 | frameset.rs:1616-1620 | ported |
| 15 | Annul the copy | frameset.c:10231-10232 | -- | n/a: owned values are dropped |
| 16 | Annul the returned pointer if an error occurred | frameset.c:10234-10235 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 17 | Return the result | frameset.c:10237-10238 | frameset.rs:1616 | ported |

## Rows 10, 12 and 14 — the port always returns the copy

C tracks a `simpler` flag and returns `astClone( simpler ? new : this )`. When no
edge reduced, the caller gets a pointer to the *original* FrameSet, and
`astSimplify_`'s own `result != this` test (`mapping.c:24772`) therefore reports
no simplification and does not stamp the result.

`FrameSet::simplified` returns the copy unconditionally. Whether that changes an
observable depends on how `simplify_entry` decides that a result is new, which is
the same question §10.17 asks about NormMap. Recorded there rather than as a
separate section because the two share the decision.

## Row 6 — one oriented Mapping, not a Mapping plus a flag

C's FrameSet holds `map[]` and `invert[]` as parallel arrays: `invert[i]` is the
direction the FrameSet applies the Mapping in, which may disagree with the
Mapping object's own Invert attribute because the object can be shared. Rows 6,
7 and 9 exist to reconcile the two for the duration of the `astSimplify` call.

The port's `Edge` stores a single `DynMapping` oriented `from`→`to`, so the two
values cannot disagree and there is nothing to reconcile. This is the ownership
change doing what it is supposed to do, not a shortcut — but it is also why row
11 needs no separate flag update: the orientation `simplify()` returns is the
orientation the edge then applies.
