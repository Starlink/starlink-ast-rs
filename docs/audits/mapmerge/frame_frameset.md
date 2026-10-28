# `Simplify` audit — FrameSet

C source: reference/starlink-ast/src/frameset.c
Rust: crates/starlink-ast/src/frames/frameset.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3d3f255f
Extension points: classified

Uncited C steps:
- frameset.c:10175 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

`FrameSet` has no `MapMerge`; C gives it a `Simplify` body instead
(`frameset.c:10187-10261 (fn Simplify)`), overriding the one it inherits from
Frame. The algorithm is not "compose base->current and simplify that": it takes a
copy of the FrameSet and simplifies each internal edge Mapping *individually*,
leaving the node graph intact, and returns the copy only if at least one edge
reduced.

`FrameSet::simplified` (`frameset.rs:2651-2645 (fn simplified)`) is the whole
port of it, and `FrameSet::simplify_frame`
(`frameset.rs:2140 (fn simplify_frame)`) is what lets a Frame-level caller reach
it.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the result to NULL | frameset.c:10187-10189 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no pointer to pre-clear |
| 2 | Check the global error status and bail out | frameset.c:10190-10192 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a pointer to the FrameSet structure | frameset.c:10193-10195 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Make a copy of the FrameSet, since we may alter it -- a deep copy, which C calls a minor limitation of the current implementation | frameset.c:10196-10199 (fn Simplify) | frameset.rs:2643 (fn simplified) | architecture | ported |
| 5 | Loop to examine each of the Mappings between the Frames in the copy | frameset.c:10200-10204 (fn Simplify) | frameset.rs:2649-2653 (fn simplified) | architecture | ported |
| 6 | Obtain the Mapping pointer and the invert flag the FrameSet applies it with | frameset.c:10205-10208 (fn Simplify) | frameset.rs:2650 (fn simplified) | architecture | ported |
| 7 | Determine whether the Mapping's own Invert attribute is set, and obtain its value | frameset.c:10209-10213 (fn Simplify) | -- | ceremony | n/a: an `Edge` holds one Mapping already oriented `from`->`to`, so there is no second flag to compare against |
| 8 | Set the Mapping's Invert attribute to the value the FrameSet requires | frameset.c:10214-10216 (fn Simplify) | -- | ceremony | n/a: as row 7 — the edge Mapping is already in the orientation the edge applies |
| 9 | Simplify the Mapping | frameset.c:10217-10219 (fn Simplify) | frameset.rs:2642 (fn simplified) | architecture | ported |
| 10 | Restore the Mapping's original Invert attribute, set or cleared as it was | frameset.c:10220-10229 (fn Simplify) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 11 | Test whether simplification was performed, by pointer identity | frameset.c:10230-10233 (fn Simplify) | frameset.rs:2642 (fn simplified) | architecture | ported |
| 12 | On success, substitute the new Mapping and record its Invert value as the new flag | frameset.c:10234-10239 (fn Simplify) | frameset.rs:2643 (fn simplified) | architecture | ported |
| 13 | Note that some Mapping within the FrameSet simplified | frameset.c:10240-10244 (fn Simplify) | frameset.rs:2644 (fn simplified) | architecture | ported |
| 14 | Annul the pointer to the simplified Mapping | frameset.c:10245-10248 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 15 | Return the copy if anything simplified, otherwise the original FrameSet | frameset.c:10249-10252 (fn Simplify) | frameset.rs:2649-2640 (fn simplified) | architecture | ported |
| 16 | Annul the copy | frameset.c:10253-10255 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 17 | Annul the returned pointer if an error occurred | frameset.c:10256-10258 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 18 | Return the result | frameset.c:10259-10261 (fn Simplify) | frameset.rs:2649 (fn simplified) | ceremony | ported |

## Rows 11, 13 and 15 — the `simpler` flag decides which object is returned

C tracks a `simpler` flag and returns `astClone( simpler ? new : this )`. When no
edge reduced, the caller gets a pointer to the *original* FrameSet, and
`astSimplify_`'s own `result != this` test
(`mapping.c:24772 (fn astSimplify_)`) therefore reports no simplification and
does not stamp the result.

`FrameSet::simplified` carries the same flag and makes the same choice: an edge
whose `simplify_reporting_change` reports no change leaves `simpler` clear, and a
FrameSet with no reducible edge is returned as itself with `false`. The `bool`
half of the return is what C reads off the pointer, so the caller sees the
distinction C's caller sees — including `Frame::simplify_frame`, which turns it
into the `None` that `normmap.c:626 (fn MapMerge)`'s identity test needs.

## Rows 7, 8 and 10 — one oriented Mapping, not a Mapping plus a flag

C's FrameSet holds `map[]` and `invert[]` as parallel arrays: `invert[i]` is the
direction the FrameSet applies the Mapping in, which may disagree with the
Mapping object's own Invert attribute because the object can be shared. Rows 7,
8 and 10 exist to reconcile the two for the duration of the `astSimplify` call.

The port's `Edge` stores a single `DynMapping` oriented `from`->`to`, so the two
values cannot disagree and there is nothing to reconcile. This is the ownership
change doing what it is supposed to do, not a shortcut — but it is also why row
12 needs no separate flag update: the orientation `simplify()` returns is the
orientation the edge then applies.

## Row 4 — the copy is deep in both

C's comment calls the deep copy "a minor limitation of the current
implementation": it wanted to alter only the Mapping array and had to duplicate
the Frames too. `self.clone()` is the port's deep copy and carries the same cost.
Owned values leave no cheaper option, and the FrameSet returned when nothing
simplified is the original rather than the copy (row 15), so the duplicate is
discarded in exactly the cases C discards it.

## Row 5 — the loop runs over edges, and the node numbering is C's

C indexes `new->map[ inode - 1 ]` for `inode` from 1 to `nnode - 1`, so there is
one Mapping per node above the root and the array is one shorter than the node
count. The port's `edges` vector holds the same set with each entry carrying its
own `from` and `to` node indices, so iterating it visits exactly the Mappings C
visits, in the same order, without the off-by-one indexing.
