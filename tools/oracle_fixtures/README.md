# Oracle fixture generators

Each `.c` file here builds against `reference/starlink-ast` and prints
captured outputs as Rust constant declarations. The output is copied
into the corresponding Rust unit test file once and committed alongside
the `.c` source — tests then run without requiring the oracle.

## Building

```
make
```

Builds every `.c` here and links against `reference/starlink-ast/build/libast.so`.

## Capturing fixtures

```
make linear_approx
./linear_approx > /tmp/linear_approx.rs
# inspect output, then paste into the corresponding test file
```

The output is formatted as a Rust `const` block intended to be pasted
into the relevant `#[cfg(test)] mod tests` in the ported function's
source file.

## Native-serialization round-trip probes

Two probes generate / cross-check the `.ast` FrameSet round-trip corpus
(`crates/ast-core/tests/fixtures/native/reference/frameset_fixtures/`):

- `head2ast <input.head> [attrs]` — reads a FITS header into a FitsChan,
  `astRead`s the WCS, and dumps it via an `astChannel` text sink
  (`Comment=1,Full=0`), mirroring `wcsconverter.c`'s AST branch. Used by
  `gen_frameset_fixtures.sh` to produce the committed fixtures.
- `ast2ast <input.ast>` — reads a native `.ast` object through a source
  Channel and re-dumps it through a sink Channel at the same verbosity.
  This exercises the C oracle's *own* native round-trip, isolated from
  FITS, so a Rust round-trip can be diffed against the authoritative
  C round-trip rather than only against the first dump.

## Provenance rule

When a fixture's expected value changes (e.g., a captured number drifts
across reference rebuilds), regenerate ALL fixtures in the same module
and re-run the affected tests. Don't update one number by hand.

## `build_issimp_instrumented.sh` — watching C stamp `IsSimp`

The `IsSimp` card records that a Mapping was, at some instant, the entire
result of a simplification. When the port and C disagree about one, reasoning
about which call site stamped it has been wrong more often than right; the
script builds a copy of the pinned reference library that logs every
`astSetIsSimple` with a resolved backtrace, plus a `gen_simp` driver that reads
a native `.map`, simplifies it, and writes the result the way
`gen_simplify_fixtures.c` does.

```shell
./tools/oracle_fixtures/build_issimp_instrumented.sh /tmp/ast-instr
AST_LOG_ISSIMPLE=1 /tmp/ast-instr/gen_simp in.map out.simp 2> stamps.txt
```

`STAMP` lines carry the object address, class, shape and call stack, so a frame
reading `MapMerge + 216` inside `Simplify` identifies `cmpmap.c`'s self-simplify
head without guessing. `DUMPTAG` lines name the objects still carrying the flag
when the Channel writes them, which is how a stamp is matched to the card it
produces.

The submodule is never modified — the script copies the tracked files out and
patches the copy. Dropping the two instrumentation patches reproduces the stock
library, which is the build to use when generating oracle data: both produce
identical output over the whole `simplify_fixtures` corpus.

## `check_corpus_fresh.sh` — is the native corpus still what C produces?

Moving the `reference/starlink-ast` pin can invalidate committed references
without anything in this repo changing, and upstream will not notice because
their suite compares with `astEqual` rather than byte for byte.

```shell
./tools/oracle_fixtures/check_corpus_fresh.sh
```

Each drifting case prints as `<name> <verdict> <keyword-diffs> <value-diffs>`.
Cards and values are classified independently, so a stale card is never hidden
by a platform difference in the same file:

- `CARD-ONLY` — differs only in `IsSimp` cards. The reference predates a
  library change and must be regenerated. Exits non-zero.
- `MIXED` — `IsSimp` cards differ *and* values differ. Both causes are present:
  the cards are stale, and the reference was captured on another platform.
  Take the cards from a fresh capture and keep the committed values, which is
  what `merge_card_hunks.py` does. Exits non-zero.
- `NUMERIC` — identical keywords and identical `IsSimp` cards, last-bits value
  differences only. The reference was captured on another platform. Reported,
  tolerated.
- `STRUCTURAL` — the keyword sequence itself differs. Should never happen from
  an `IsSimp` or KeyMap change; investigate before regenerating anything.

Run this whenever the submodule pin moves. It belongs in the upstream-sync
protocol in `CLAUDE.md`.

## `build_simplify_trace.sh` — watch C simplify

When a question is "does C reach this node, and in what state?", reasoning about
the call graph has been wrong more often than right. Three dispatchers in
`mapping.c` cover every class, so one instrumented build answers it for any
Mapping:

```shell
./tools/oracle_fixtures/build_simplify_trace.sh /tmp/ast-trace
AST_TRACE=1 /tmp/ast-trace/gen_simp in.map out.simp 2> trace.txt
```

- `SIMPLIFY-ENTER <class> issimp= dns= inv= nin= nout=` — every `astSimplify_`,
  with the state it was entered in. A following `SIMPLIFY-WALLED` means the
  `IsSimple` bit short-circuited it.
- `MAPMERGE <class> where= series= nmap= issimp= inv=` — every nomination, so
  which class was offered to `MapMerge`, at which index and in which mode.
- `MAPLIST <class> series= invert= nmap_in=` — every decomposition.

Output is indented by simplify depth, so a subtree's whole route reads as a
block. This settled PLAN.md §10.14 after three wrong diagnoses from reading
dumps: the trace showed C nominating the node everyone assumed it skipped, and
entering it at a different orientation, which was the real difference.

The submodule is never modified — the script copies the tracked files out and
patches the copy.
