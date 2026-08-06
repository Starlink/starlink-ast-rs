# KeyMap Port Design

**Date:** 2026-08-06
**Branch:** `tickets/DM-54793`
**C source:** `reference/starlink-ast/src/keymap.c` (11,484 lines),
`reference/starlink-ast/src/keymap.h`

## Problem

`KeyMap` is unported.
It is the keyed typed-value container that AST uses wherever a variable set of named values must cross an API boundary: `astWarnings`, `Table`/`FitsTable` column storage, the Object-associated user KeyMap, and the internal state of every remaining `Channel` subclass (`XmlChan`, `YamlChan`, `StcsChan`).
Nothing downstream of it can be ported until it exists.

A prototype implementation exists at `../starlink-astrs-proto/src/keymap`.
It is a useful reference for API shape but not a basis for the port: it hand-rolls its own text serialiser rather than using this repo's `Serializable`/`Node` tree, and it does not carry the type-conversion matrix.

## Goals

1. Port `keymap.c` to `crates/ast-core/src/keymap/` — entry storage, `ConvertValue`, all supported type codes, `SortBy`, and a `Serializable` round-trip that is byte-exact against the C reference for the fixtures we hold, `MapSz` excepted.
2. Add `AnyObject::KeyMap` so KeyMaps deserialise from and serialise into `.ast` files.
3. Port `ast_tester/testkeymap.c` and `ast_tester/testobjectkeymap.c`.
4. Shape the struct so `Table` and `FitsTable` can embed it later without redesign.

## Non-goals

- `Table` / `FitsTable` (`table.h:98` embeds `AstKeyMap` by value; designed-for, not built).
- The Object-associated retained KeyMap (`object.c:1736`, `object.c:2480`).
- The C-ABI shim. `crates/ast-capi/src/lib.rs` is a one-line stub; there is nothing to extend yet.
- Patching the C library. Three C bugs are recorded below and in `docs/issues/c-library-quirks.md`; the C patches are a separate follow-up.
- Deciding whether the `Channel` subclasses carry `KeyMap` or `Node`. See "Deferred decisions".

## Section A: Storage model

### Storage: `std::collections::HashMap` owns the entries

C's chained bucket table is **not** ported as a storage structure.
`HashMap<String, MapEntry>` holds the entries, and the only piece of the C table that survives is the arithmetic needed to compute `MapSz` at dump time (below).

The C table has no property we need that `HashMap` lacks:

- **Key equivalence reduces to exact string equality.** `KeyCmp` (`keymap.c:3921`) ignores trailing spaces and `HashFun` (`keymap.c:3604`) ignores all spaces, which looks like it rules out a `String` key. It does not: C normalises the key at *insertion*, not at comparison. `ConvertKey` (`keymap.c:1573`) uppercases when `KeyCase=0`, and `MAKE_MAPPUT0` null-terminates away trailing spaces before hashing (`keymap.c:4706-4716`). Apply the same normalisation to the lookup key and `HashMap`'s `Eq` is the right predicate.
- **Lookup is at least as fast.** Both are O(1), and `HashMap` is the better-tuned constant factor.
- **Observable order does not come from the table.** `SortBy` ordering is computed from `member` / `keymember` / `KeyCmp`, none of which involve bucket layout. C's `SORTBY_NONE` order *is* bucket order, and that is precisely Bug 1; see "Ordering".
- **Nothing else reads `mapsize`.** `astMapSize` sums `nentry[itab]` (`keymap.c:7801`) — a count `HashMap` already has. `mapsize` reaches the outside world through exactly one path: the `MapSz` card.

### `MapSz` is computed at dump time, exactly

`Dump` writes `MapSz` unconditionally (`keymap.c:10510`) and `LoadKeyMap` feeds it to `NewTable` (`keymap.c:11026`), so the card must be emitted.
`MapSz` cannot be read off a `HashMap`, but it does not have to be *retained* either — it can be reconstructed by replaying C's own sizing loop over the keys in the order we are about to dump them:

```
mapsize = 16                                  # MIN_TABLE_SIZE (keymap.c:232)
if sizeguess set and sizeguess/10 > 16:       # SetSizeGuess (keymap.c:9520)
    mapsize = next_pow2(sizeguess / 10)       # NewTable rounding (keymap.c:8760)
for each key in dump order:
    counts[ djb2(key) & (mapsize - 1) ] += 1
    if that bucket's count > 10:              # MAX_ENTRIES_PER_TABLE_ENTRY (keymap.c:708)
        mapsize *= 2                          # DoubleTableSize (keymap.c:2626)
        recompute counts for the keys seen so far
```

This is a faithful port of `AddTableEntry`'s doubling check and `DoubleTableSize`'s redistribution, not an estimate of them.
`NewTable` writes its power-of-two rounding in floating point — `(int)(0.5 + pow(2.0, ceil(log(size)/log(2.0))))` (`keymap.c:8764`) — where an integer doubling loop is used instead; the two agree on every exact power from 2^4 to 2^24 and on every size from 17 to 200000, so this is an equivalent form rather than a substitution.
It needs djb2 plus a scratch count array sized `mapsize`; no bucket table, no per-entry hash field, and nothing maintained across mutations.
`SizeGuess` folds in because `SetSizeGuess` refuses to run on a non-empty map (`AST__NOWRT`, `keymap.c:9463`), so the seed is always fixed before any key arrives.

Verified against `reference/starlink-ast/build/libast.so`:

- The loop reproduces C's own `MapSz` on 14/14 key sets — the four fixture keys, spreads up to 2000 keys, up to 300 deliberately all-colliding keys, and mixtures — and on 12/12 `SizeGuess` combinations.
- Feeding hand-synthesised dumps (alphabetical keys, `MapSz` from the loop) to `astFromString`, C ends at **exactly** the written value in 10/10 scenarios, with zero doublings during the load.

The second result holds because we dump in `KeyCmp` order and `LoadKeyMap` re-inserts in dump order, so C's insertion sequence is ours.
That matters: the doubling loop is order-sensitive in general (105 of 4000 randomly generated adversarial key sets gave different `mapsize` under different insertion orders), and fixing the dump order is what makes the computed value the one C will arrive at.

### Where this diverges from C, and why it is safe

C's `MapSz` is *history*, not content: `DoubleTableSize` is the only thing that changes `mapsize` after construction and nothing halves it (`RemoveTableEntry` decrements `nentry[itab]` at `keymap.c:9036` and stops).
Grow a map to 11 colliding keys and remove 9 and it still reports `MapSz = 32`; build those same 2 keys fresh and it reports `16`.
So the value is not a function of the entry set, and two C KeyMaps with identical contents can disagree on it.

Rust's value is a function of the entry set. The consequences:

- **A C dump carrying a history-inflated `MapSz` does not round-trip byte-exactly through Rust** — we re-emit the content-derived value. This is the one divergence, and it is unavoidable without retaining C's collision history verbatim.
- **A C reader is unaffected.** Every `MapSz` accepted by `LoadKeyMap` loads with `status=0` and correct contents; probing −8, 0, 1, 3, 7, 17, 100, 1024 and 1000000 produced no errors. An undersized value self-heals during the load, because the doubling check runs on each re-put: a 500-entry map whose C `MapSz` is 128 converges to 128 whether the file says 1, 16, 32 or 128. Since our value is the one C's own loop reaches, the reader does zero extra work.
- **If the value ever has to be approximated, err low.** Oversized values are *retained*: `NewTable` rounds up to the next power of two and keeps it (256/1024/65536 verbatim, 100→128, 17→32), and nothing later shrinks the table, so an over-estimate bloats the table for the object's lifetime in every downstream consumer. An under-estimate costs a bounded number of rehash passes during one load and then lands on the right size.

### Oracle comparison ignores the `MapSz` card

Because C's value is history-dependent and ours is content-dependent, byte comparison against C output excludes the `MapSz` card from both sides.
This follows the established precedent for `ISSIMP<xx>` / `INVERT<xx>` — `common::is_aliasing_tag_card` and `ALIASING_TAG_LICENSED` in `crates/ast-core/tests/test_wcsconv.rs` — and is licensed the same way: per-card, named, and documented, not a blanket tolerance.
Every other card, `MemCnt` included, is compared byte-exactly.

`HashFun` is still ported verbatim; we write no hash algorithm of our own, and `MapSz` fidelity is not traded away.

### The struct

```rust
pub struct KeyMap {
    entries: HashMap<String, MapEntry>,  // key already normalised by ConvertKey
    member_count: u64,                   // source of both age counters
    sizeguess: Option<usize>,            // seeds the dump-time MapSz loop
    key_case: Option<bool>,              // None = attribute unset
    key_error: Option<bool>,
    map_locked: Option<bool>,
    sort_by: Option<SortBy>,             // None = unset; default SortBy::None
}

struct MapEntry {
    comment: Option<String>,
    member: u64,                         // bumped on every put of a value
    keymember: u64,                      // retained when an existing key is re-put
    value: EntryValue,
}
```

The key lives in the `HashMap` key, not in `MapEntry`, and there is no retained `hash` field — C stores one (`AstMapEntry.hash`, `keymap.h:114`) only so `DoubleTableSize` can rehash without re-reading keys, and the dump-time loop recomputes hashes from the keys it is already walking.
`astMapSize` reads `entries.len()`.

Rust's `Option<T>` replaces C's unset sentinels.
C uses three different ones: `-1` for `KeyCase` (`astMAKE_TEST` at `keymap.c:10161`), `-INT_MAX` for `KeyError` (`10201`), `MapLocked` (`10242`) and `SortBy` (`10306`), and `INT_MAX` for `SizeGuess` (`keymap.c:3599`, whose getter defaults to `MIN_TABLE_SIZE*MAX_ENTRIES_PER_TABLE_ENTRY` = 160).
The inconsistency is the cause of Bug 2 below, and `Option` makes it unrepresentable.
`sizeguess: None` therefore behaves as 160, which divided by 10 is `MIN_TABLE_SIZE` — the seed the loop already starts from, so unset and default coincide.

Ported verbatim: `HashFun` (djb2, skipping **all** spaces), `KeyCmp` (`keymap.c:3921`, ignoring **trailing** spaces only), `ConvertKey` (`keymap.c:1573`, uppercasing when `KeyCase=0`), `MIN_TABLE_SIZE = 16` (`keymap.c:232`), `MAX_ENTRIES_PER_TABLE_ENTRY = 10` (`keymap.c:236`), `DoubleTableSize`'s `newmapsize = 2*mapsize` redistribution rule (`keymap.c:2626`), `NewTable`'s power-of-two rounding (`keymap.c:8720`), `SetSizeGuess`'s `sizeguess/MAX_ENTRIES_PER_TABLE_ENTRY` sizing and its `AST__NOWRT` refusal on a non-empty map (`keymap.c:9463`), and the same refusal in `SetKeyCase` (`keymap.c:9234`).
The four table constants and rules are consumed only by the dump-time `MapSz` loop; `HashFun` is additionally exercised by tier-1 tests that lock its values against the C oracle.

`AST__MXKEYLEN = 200` is enforced with `ErrorCode::Bigkey` when `KeyCase=0`, as C does.

### Dropped: the Object entry list

C threads all `AST__OBJECTTYPE` entries onto a second linked list via `firstA` (`keymap.h:120`), maintained by `AddToObjectList` (`keymap.c:716`) and `RemoveFromObjectList`.
Its only consumer is `ManageLock` (`keymap.c:4088`), which propagates `astLock`/`astUnlock` into contained Objects.
This port is `Send`-not-`Sync` and has no `astLock`, so the list has no reader and is not built.

### Ordering

**`SortBy::None` read-out returns keys in `KeyCmp` order.**

C head-inserts (`keymap.c:675`: `entry->next = this->table[itab]; this->table[itab] = entry;`) and treats the resulting bucket order as the observable order for `astMapKey`/`astMapIterate` under `SORTBY_NONE` and for `Dump` unconditionally.
That makes the observable order a function of insertion history, which is the root of Bug 1.

Rust sorts by `KeyCmp` at read-out time in `map_key`, `map_iterate` and `to_node` when `SortBy` is `None`.
`HashMap` iteration order is unspecified and randomised per process, so an ordered read-out is required rather than merely preferable — every path that exposes an order sorts, and none of them may iterate the map directly.

Sorting happens at read-out, not on put.
C's documented reason for `SORTBY_NONE` being the default is that it "avoids the need for a sorted list of keys to be maintained"; that property is preserved — puts stay O(1), and only the ordered-read paths pay.

Consequences: dumps are idempotent, dumps are independent of insertion order, Rust never reproduces C's bucket-reversal, and the dump order is the order the `MapSz` loop simulates.

### SortBy

The six explicit modes port as C has them, driven by `CompareEntries` (`keymap.c:1463`):

| Mode | Key | Field |
|---|---|---|
| `None` (default) | — | `KeyCmp` order at read-out (Rust); bucket order (C) |
| `AgeUp` / `AgeDown` | age of the *value* | `member` |
| `KeyAgeUp` / `KeyAgeDown` | age of the *key* | `keymember` |
| `KeyUp` / `KeyDown` | key text | `KeyCmp` |

The dual counters are load-bearing and are ported exactly (`keymap.c:686-696`): `member` is `(this->member_count)++` on every put, while `keymember` is reused when a key that already exists is re-put.
So re-putting a value moves an entry under `AgeDown` but not under `KeyAgeDown` — an assertion `testkeymap.c` makes directly.

C maintains a circular doubly-linked sorted list (`first`/`snext`/`sprev`/`nsorted`), built by `SortEntries` (`keymap.c:9819`) via `qsort` and maintained incrementally by `AddToSortedList`'s binary chop (`keymap.c:802`).
Rust computes an ordered `Vec<usize>` of entry positions instead.
The linked list exists only to serve ordered read-out and the chop insert; a sort over a slice does both, and it eliminates Bug 3 structurally.

## Section B: Three C bugs

All three were reproduced against `reference/starlink-ast/build/libast.so` with standalone C probes, not inferred from reading the source.

Per the agreed scope, these are **specified only** this cycle.
Each is written up in `docs/issues/c-library-quirks.md` with its location and proposed patch.
Rust implements the correct behaviour and mirrors none of them.

### Bug 1 — dump order reverses within a bucket, and never stabilises

`Dump` walks the table bucket by bucket, head-first within each bucket (`keymap.c:10516-10525`).
`LoadKeyMap` re-`MapPut`s entries in dump order (`keymap.c:11035`), and every put head-inserts (`keymap.c:675`).
So two keys that collide into the same bucket swap places on every round trip:

```
put order: SIZE, FRAME     (djb2 puts both in bucket 0)
dump 1:  Key1 = "FRAME"   Key2 = "SIZE"
dump 2:  Key1 = "SIZE"    Key2 = "FRAME"
dump 3:  Key1 = "FRAME"   Key2 = "SIZE"
dump1 == dump2 ? NO      dump1 == dump3 ? NO (differs in MemCnt)
```

The dump is not idempotent and does not converge.
Real fixtures hit this: `reference/starlink-ast/ast_tester/stcschan-test1-doc3-props.ast` has `SIZE` and `FRAME` both in bucket 0, and `TIME` and `REFPOS` both in bucket 4.

**Consequence.** A KeyMap's serialised form depends on how many times it has been written and read, which makes byte-exact comparison of any KeyMap-bearing file order-of-operations dependent.

**Proposed C patch.** Make `Dump` emit entries in `KeyCmp` order rather than bucket order (it already ignores `SortBy` entirely, so this changes no documented behaviour). Alphabetical output is idempotent under any insertion order.

### Bug 2 — a spurious `KyCas = 1` appears on the first round trip

`LoadKeyMap` reads `kycas` with a default of `-INT_MAX` (`keymap.c:11001`):

```c
new->keycase = astReadInt( channel, "kycas", -INT_MAX );
if ( TestKeyCase( new, status ) ) SetKeyCase( new, new->keycase, status );
```

But `KeyCase`'s unset sentinel is `-1`, not `-INT_MAX` (`keymap.c:10161`):

```c
astMAKE_TEST(KeyMap,KeyCase,( this->keycase != -1 ))
```

Every other KeyMap attribute uses `-INT_MAX` and is consistent.
So when `KyCas` is absent from the file, `keycase` becomes `-INT_MAX`, which `!= -1`, so `TestKeyCase` reports the attribute as *set* and the next dump writes it out.

Reproduced on a single-entry map where no collision is possible:

```
dump 1:  Begin KeyMap / MapSz = 16 / MemCnt = 2 / Key1 = "ONLY" ...
dump 2:  Begin KeyMap / KyCas = 1 / MapSz = 16 / MemCnt = 2 / Key1 = "ONLY" ...
```

It stabilises after one round (`KyCas` is then present and read normally), so unlike Bug 1 it is convergent — but it means no freshly-written KeyMap matches its own re-dump.

This is visible in this repository: `cap_timemap_02.simp:218` carries `KyCas = 1` while `cap_timemap_02.map` does not, for the same logical KeyMap.

**Proposed C patch.** One character: change the default at `keymap.c:11001` from `-INT_MAX` to `-1`. Alternatively, change the sentinel at `keymap.c:10160-10161` to `-INT_MAX` for consistency with the other three attributes.

### Bug 3 — `astMapIterate` never terminates when `SortBy` is set

`AddToSortedList` builds a **circular** doubly-linked list — the empty case sets `entry->snext = entry->sprev = entry` (`keymap.c:864-866`), and both the insert-at-front and insert-at-back cases wrap the ends together (`keymap.c:872-885`).

`MapKey` knows this and guards for it (`keymap.c:3530`):

```c
for( istep = 0; entry && istep < index; istep++ ) {
   entry = entry->snext;
   if( entry == this->first ) { entry = NULL; break; }
}
```

`MapIterate`'s sorted branch does not (`keymap.c:8690-8706`):

```c
   } else {
      if( reset ) { entry = this->first; } else { entry = this->iter_entry; }
      if( entry ) {
         key = entry->key;
         this->iter_entry = entry->snext;
      }
   }
```

`entry->snext` is never NULL, so the iterator wraps forever.
Reproduced with a 3-entry map and `SortBy = KeyUp`:

```
A B C A B C A B ...    terminated = NO -- ran past the end
```

The `SORTBY_NONE` branch terminates correctly, because it walks the bucket chains where `next` really does reach NULL.

**Consequence.** Any caller of `astMapIterate` on a KeyMap with a non-default `SortBy` loops forever.
Latent in practice only because `table.c:3747` is the sole caller in the tree and never sets `SortBy`.

**Proposed C patch.** Add the same wrap guard `MapKey` already uses: after `entry->snext`, if the result is `this->first`, store NULL in `iter_entry`.

Rust's `map_iterate` returns `Option<&str>` from a bounded index into the ordered position vector, so it cannot express this failure.

### Fixture regeneration

Only two files in this repository contain KeyMaps:

- `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/stcresourceprofile/cap_timemap_02.simp` — `KyCas = 1`, `MapSz = 16`, `MemCnt = 11`
- `.../cap_timemap_02.map` — `MapSz = 16`, `MemCnt = 8`, no `KyCas`

Both hold the same four keys (`Size`, `Name`, `Resolution`, `Error`) which hash to buckets 0/6/9/15 — monotonic, no collision, so Bug 1 does not currently bite them.
Under this design they are regenerated with keys in `KeyCmp` order (`Error`, `Name`, `Resolution`, `Size`) and the spurious `KyCas` reconciled.
`MapSz` is unchanged: four non-colliding keys give 16 from the sizing loop, which is what both files already carry.
Mechanical; four keys each.

## Section C: Types and conversion

### Supported type codes

Ten of C's eleven, using C's numbering because it appears in the `Typ%d` dump line:

| Code | Type | Rust |
|---|---|---|
| 0 | `AST__BADTYPE` | absent-entry marker only |
| 1 | `AST__INTTYPE` | `i32` |
| 2 | `AST__DOUBLETYPE` | `f64` |
| 3 | `AST__STRINGTYPE` | `String` |
| 4 | `AST__OBJECTTYPE` | `AnyObject` |
| 5 | `AST__FLOATTYPE` | `f32` |
| 7 | `AST__SINTTYPE` | `i16` |
| 8 | `AST__UNDEFTYPE` | no value |
| 9 | `AST__BYTETYPE` | `u8` |
| 10 | `AST__KINTTYPE` | `i64` |

Each entry is either scalar (`nel == 0`) or a 1-D vector (`nel > 0`).
The distinction is observable through `map_length` and the presence of the `Nel%d` dump line, so it is a property of the entry, not merely of Rust's `Vec`:

```rust
enum EntryValue {
    Int(Scalar1D<i32>),
    Double(Scalar1D<f64>),
    // ... one per type
    Undef { nel: usize },
}

enum Scalar1D<T> { Scalar(T), Vector(Vec<T>) }
```

### `AST__POINTERTYPE` (code 6) is omitted

C's own round trip cannot read it.
`DumpEntry` writes the pointer as a `%p` string (`keymap.c:2911-2914`), but `LoadKeyMap` has **no `AST__POINTERTYPE` branch** — a file containing `Typ = 6` fails to load.
`ConvertValue` also refuses every conversion into or out of the type except pointer→pointer (`keymap.c:2217`).
So the type is write-only and unloadable in C, and omitting it costs no round-trip fidelity.

On the question of the Rust equivalent of a stored `void *`: there isn't a good one, because the C semantics are "an untyped, non-owning borrow with no lifetime", which is precisely what Rust exists to reject.

- `Box<dyn Any>` is *owning* where `void *` borrows, and is not `Clone`, so it would break KeyMap's deep-copy contract.
- A generic `KeyMap<T>` puts a type parameter on every signature that touches a KeyMap.
- `usize` is the raw pointer under a different name, with no safety gained.

The only in-tree C consumer is `yamlchan.c`'s `PARENT_KM` back-reference.
When `YamlChan` is ported, that becomes a parent index or an explicit traversal stack — the Rust port has no need for the `void *` itself, so the type is not implemented speculatively for it.

### `ConvertValue`

`ConvertValue` (`keymap.c:1655`, ~840 lines) is ported as one central function, because every get and put in the class routes through it and splitting it would obscure which conversions are legal.

Its heuristics are C's own, are ported as-is, and get an explicit comment saying that the heuristic is in the original:

- **double → string.** C formats with `%.*g` at `AST__DBL_DIG-3` significant digits, then retries at `AST__DBL_DIG`, and reverts to the shorter form unless the longer one is more than 9 characters longer.
- **`AST__BAD` → `"<bad>"`** (`AST__BAD_STRING`) on conversion to string.
- **double → int** as `(int)(dval + 0.5)`, not a `round` call.

### API surface

Typed methods, one per C macro expansion, snake_case:

| C macro | C expansions | Ported | Rust |
|---|---|---|---|
| `MAKE_MAPPUT0` (`keymap.c:4769-4777`) | 9 | 8 | `put_0i`, `put_0d`, `put_0c`, `put_0a`, `put_0f`, `put_0s`, `put_0k`, `put_0b` |
| `MAKE_MAPPUT1` (`keymap.c:5007-5015`) | 9 | 8 | `put_1i`, `put_1d`, … |
| `MAKE_MAPGET0` (`keymap.c:5554-5562`) | 9 | 8 | `get_0i`, `get_0d`, … |
| `MAKE_MAPGET1` (`keymap.c:6044-6051`) | 8 | 7 | `get_1i`, `get_1d`, … |
| `MAKE_MAPGETELEM` (`keymap.c:6940-6947`) | 8 | 7 | `get_elem_i`, … |
| `MAKE_MAPPUTELEM` (`keymap.c:8473-8481`) | 9 | 8 | `put_elem_i`, … |

Each family loses exactly one expansion — the `P` (pointer) variant, per the
section above. `MAKE_MAPGET1` and `MAKE_MAPGETELEM` have no `C` (string)
expansion because C hand-writes those to carry the caller's buffer length
(`astMapGet1C_` at `keymap.c:11349`, `astMapGetElemC_` at `keymap.c:11355`);
Rust returns owned `String`s and needs no length argument, so `get_1c` and
`get_elem_c` are ordinary members of their families.

That is 46 typed accessors, plus `put_u` (`astMapPutU_`, `keymap.c:11378`) for
undefined entries and `get_0c`'s hand-written C counterpart `astMapGetC_`
(`keymap.c:11311`), whose buffer-lifetime contract Rust does not need.
1:1 with the C source, so the port is auditable against it line by line.
This matches the repo's existing typed-setter convention: no string-typed `get_attr(name)` layer in `ast-core`.

Also ported:

- `map_length` — `nel`, or 1 for a scalar.
- `map_len_c` (`keymap.c:7808`) — the longest formatted element length across the entry, minus one.
- `map_type`, `map_has_key`, `map_defined`, `map_remove`, `map_rename`, `map_size`, `map_key`, `map_iterate`.
- `put_elem_*`'s out-of-range append: `elem >= nel || elem < 0` grows the vector rather than erroring.
- `map_copy` (`keymap.c:4112`) — recursive merge when both source and destination hold a KeyMap at the same key; otherwise remove, copy, re-add retaining `keymember`.
- `map_copy_entry` (`keymap.c:4264`).
- `set_map_locked` / `clear_map_locked` (`keymap.c:9381`) — propagates recursively into nested KeyMap entries, both scalar and vector.
- `CheckCircle` (`keymap.c:951`) — rejects self-insertion and any cycle with `ErrorCode::Kycir`.

`Clone` is the deep copy. C's copy constructor uses `astCopy` on entry Objects, so contained Objects are deep-copied — unlike `astMapPut0A`, which shares by refcount via `astClone`. Rust's owned-value model gives the deep-copy semantics for free and has no refcount to share.

### Errors

Every code KeyMap needs is already generated from `ast_err.msg`: `Mpger`, `Mpker`, `Mpvin`, `Mpper`, `Badkey`, `Bigkey`, `Kycir`, `Nowrt`, `Inter`.
`AstError` is the existing struct; no new variants.

`KeyError` selects between returning `Ok(None)`-style absence and raising `Mpger`, as in C.

## Section D: Serialisation

`KeyMap` implements the existing `pub(crate) trait Serializable` (`crates/ast-core/src/serial/mod.rs:28`), replacing the prototype's hand-rolled emitter.

`to_node` mirrors `Dump` (`keymap.c:10480`):

1. `SzGss`, `SortBy`, `KyCas`, `KyErr`, `MpLck` — written conditionally on their test predicates.
2. `MapSz` and `MemCnt` — unconditional (`keymap.c:10510`, `10513`). `MapSz` comes from the dump-time sizing loop in Section A, run over the same `KeyCmp`-ordered key sequence that step 3 emits.
3. Per entry, in `KeyCmp` order (the Bug 1 fix): `Key%d`; `Com%d` only when the comment is non-blank; `Typ%d` with C's per-type comment string; `Nel%d` only when `nel > 0`; then `Val%d` for a scalar or `V%d_%d` per element, with the comment `"Item values"` on the first element only (`keymap.c:2725-2935`).

C omits the value line entirely for a double equal to `AST__BAD`; that is ported.

`from_node` mirrors `LoadKeyMap` (`keymap.c:10844`), including the asymmetry that **`Mem%d` is read but never written** (`keymap.c:11068` reads it and assigns `member_count`; `DumpEntry` has no counterpart).
Both halves are mirrored: we read `Mem%d` if present, and do not write it.

The final `member_count = astReadInt(channel,"memcnt",0)` is ported.

`MapSz` is read and discarded — it sizes a table we do not build, and re-emitting it verbatim would contradict the content-derived value the dump computes.
`SzGss` is *not* discarded: it is a user-set attribute that C restores (`keymap.c:10996`), it round-trips, and it seeds the sizing loop.

`AnyObject` gains `KeyMap(Box<KeyMap>)` — boxed because a KeyMap may contain KeyMaps — wired into both `any_object_from_node` (`serial/mod.rs:1672`) and `any_object_to_node`.

## Section E: Module layout

```
crates/ast-core/src/keymap/
├── mod.rs        # pub struct KeyMap, attributes, public typed methods
├── entry.rs      # MapEntry, EntryValue, Scalar1D, type codes
├── table.rs      # HashFun, KeyCmp, ConvertKey, and the dump-time
│                 # MapSz sizing loop (NewTable / DoubleTableSize rules)
├── convert.rs    # ConvertValue
├── sort.rs       # SortBy, CompareEntries, ordered read-out
└── serial.rs     # Serializable: to_node / from_node
```

Following the multi-file pattern already used by `regions/`.

## Section F: Testing

- **`crates/ast-core/tests/test_keymap.rs`** — full port of `ast_tester/testkeymap.c` (1419 lines), including:
  - `testsorting()` across all six explicit modes, and its assertion that re-putting a value moves the entry under `AgeDown` but not under `KeyAgeDown`;
  - the `KeyCase` cases — `map_key(0) == "FREDS"` under `KeyCase=0`, `"fReDs"` found, `" fReDs"` not found (leading space is significant, trailing is not);
  - the `map_len_c` expectations 4 / 0 / 5;
  - the `ConvertValue` conversion cases the C test exercises.
- **`crates/ast-core/tests/test_objectkeymap.rs`** — port of `ast_tester/testobjectkeymap.c` (190 lines).
- **Round-trip property tests** asserting the two properties C lacks:
  - **idempotence** — `dump(read(dump(km))) == dump(km)`;
  - **insertion-order independence** — two KeyMaps built from the same entries in different orders dump identically.

  Both fail against current C by construction, and are the regression guard for the ordering fix.
- **`MapSz` sizing tests**, since the loop is the one piece of table arithmetic retained and has no other coverage:
  - `HashFun` values locked against the C oracle for a fixed key list, so a djb2 typo cannot pass;
  - computed `MapSz` locked against C's own value for the key sets the probes used — the four fixture keys, spreads at 100/500/2000, all-colliding sets at 11/40/300, a collide-plus-spread mixture, and the `SizeGuess` combinations;
  - a growth-table test pinning the doubling boundaries (`MIN_TABLE_SIZE` until a bucket passes 10, then powers of two).
- **Byte-exact `.ast` round trip** over the two regenerated fixtures, via the existing auto-discovering round-trip harness.
- **`MapSz` excluded from oracle byte comparison**, implemented alongside `common::is_aliasing_tag_card` and listed in its licensed-exclusion table with the reason.

Both simplify-fixture consumers must be run when the fixtures change: the manifest-driven simplify test and the auto-discovering byte-exact round-trip test.

## Deferred decisions

**Channel subclasses: `KeyMap` or `Node`?**
Recorded as open; settled when the first subclass is actually ported, not now.

The observation that motivated the question is correct — the base C `Channel` deliberately streams output rather than retaining a KeyMap copy — but the subclasses are not a free swap:

- `yamlchan.c:11391` and `:11531` construct KeyMaps with `SortBy = KeyAgeDown`; `:12191` uses `AgeDown`; `:9814` sets `KeyUp` to force alphabetical `astMapKey`. The choice of ordering is load-bearing algorithm state.
- `Node` is `pub(crate)` (`serial/node.rs`), has no ordering control, and has no typed-vector values.

So it is a per-subclass algorithm decision, not a container substitution.

**`Channel::warnings` signature.**
`crates/ast-core/src/channel/mod.rs:71` declares `fn warnings(&self) -> &[String]`, where `docs/superpower/specs/2026-04-20-rust-port-design.md:412` specifies `Option<&KeyMap>`.
The Rust code is right and the spec is amended: C stores warnings internally as a string array and `astWarnings` (`channel.c:1690-1709`) only materialises a throwaway KeyMap at the API boundary:

```c
if( this->nwarn && this->warnings ) {
   result = astKeyMap( "", status );
   for( i = 0; i < this->nwarn; i++ ){
      sprintf( key, "Warning_%d", i + 1 );
      astMapPut0C( result, key, (this->warnings)[ i ], " " );
   }
}
```

Returning `&[String]` exposes the same information without the allocation.

## Risks

- **`ConvertValue` is large and centrally load-bearing.** A wrong cell in the matrix is a silent wrong value, not a compile error. Mitigated by porting it as one function against the C source and by `testkeymap.c`'s conversion assertions.
- **The ordering divergence is intentional and must not be mistaken for a port defect** by a later reader. Mitigated by the quirks-doc entries and by the two property tests, which state the intended invariant in executable form.
- **Fixture regeneration could mask an unrelated drift.** Only two files, four keys each; each changed line is accounted for by either the key reordering or the `KyCas` fix.
- **The `MapSz` exclusion could grow into a habit.** It is licensed for one card on one class, for a stated reason, in the same table as the `ISSIMP`/`INVERT` exclusions. Any further card added to that table needs the same standard of evidence.
- **A wrong `MapSz` would be invisible**, since the oracle comparison ignores it. Mitigated by testing the value directly against C rather than relying on the dump comparison.

## Success criteria

- `KeyMap` ported to `crates/ast-core/src/keymap/`, storing entries in a `HashMap`, with `ConvertValue`, all ten supported types, and `SortBy`.
- `MapSz` computed at dump time and matching C's value on the probed key sets and `SizeGuess` combinations; the card excluded from oracle byte comparison with its reason recorded.
- `AnyObject::KeyMap` wired into both serialisation dispatch functions.
- `testkeymap.c` and `testobjectkeymap.c` ported and green.
- The two KeyMap fixtures regenerated; both simplify-fixture consumers green.
- Idempotence and insertion-order-independence property tests green.
- Three C bugs recorded in `docs/issues/c-library-quirks.md` with locations and proposed patches.
- `PLAN.md` updated: KeyMap, the two test rows, and the deferred Channel-subclass question.
- `cargo fmt` and `cargo clippy --workspace -- -D warnings` clean.
