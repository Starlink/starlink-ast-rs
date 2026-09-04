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

## Native-serialization round-trip corpus

`crates/ast-core/tests/fixtures/wcsconv/framesets/` is generated upstream, by
`reference/starlink-ast/ast_tester/gen_frameset_fixtures.c`, and mirrored here by
copying the tree. It used to be generated on this side by `gen_frameset_fixtures.sh`
driving two local probes, `head2ast` and `ast2ast`; both are gone with it, because
a corpus generated twice from two sources is a corpus that can disagree with
itself. `tools/fixtures/check_mirror.sh` is what asserts the two trees match.

The round-trip references that used to sit beside each simplify fixture as `.rt`
are gone too, along with `gen_roundtrip_refs.sh` that captured them. They existed
because reading a dump was not the identity in C: a subclass loader re-applied a
validated attribute through a clear-on-set setter and discarded the `IsSimp` card
`astLoadMapping` had just installed. Upstream `07d0cd2d` guards each clear on the
value actually differing, so a fixture is now its own round-trip reference.

## `unitnorm_probe` — what does C on *this* host compute here?

```shell
unitnorm_probe <fixture.map> <x1> ... <xN>
```

Reads one native-encoded Mapping, runs a single point through its inverse
transform (`N` = the Mapping's `Nout`), and prints the result with `BAD` for
`AST__BAD`.
Used to settle whether a recorded oracle value is a property of the C algorithm
or of the generator host's floating point: the reference corpus was captured on
arm64 macOS, and a value that this host's C library disagrees with is a
platform artefact rather than a port defect.
Its first use was the zero-norm case of
`simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map`
(`docs/issues/c-library-quirks.md`, "`UnitNormMap`'s zero-norm guard is defeated
by compiler multiply-add contraction").

## `gen_simp` — what does the pinned C emit for a simplify fixture?

```shell
gen_simp [-m] <in> <out> [comment] [indent]
```

Reads one native-encoded Object, calls `astSimplify` on it, and writes the
result. `comment` and `indent` are passed to the output Channel and default to
the Channel's own defaults (1 and 3), so a reference captured with either turned
off is reproducible. No row needs that now: upstream regenerated `lsst.simp`,
the one reference that had been captured with `Comment` and `Indent` off, with the
Channel's defaults, and the manifest's render-options column went with it.

`-m` replaces a FrameSet by its base->current Mapping before simplifying. Most
of the corpus's FrameSet fixtures were captured that way, so their reference is
a Mapping; a fixture whose reference is itself a FrameSet was captured without
it. `test_simplify_regression.rs` applies the same rule, keyed on the
reference's class.

This is the instrument behind every re-baseline in the simplify corpus, and
`check_corpus_fresh_simplify.sh` drives it over the whole set.

## `check_simplify_fresh.sh` — is the simplify corpus still the pinned C's output?

Captures every row of `crates/ast-core/tests/fixtures/simplify/cases.txt` from
the pinned oracle with `gen_simp` and classifies the difference. `gen_simp` links
upstream's `oracle_register_intramaps`, so the corpus's IntraMap fixtures load and
are compared like any other rather than being reported as structural drift:

- **card** — the reference has a different set of cards. It predates a library
  change; regenerate it with `gen_simp` and commit the regeneration in the same
  range as the pin move. Exits non-zero.
- **numeric** — the same cards with different values. The reference was captured
  on another platform. Leave the values alone: regenerating here would replace a
  cross-platform reference with this machine's libm. Where a residue falls
  outside the harness's token tolerance, license it per case in
  `test_simplify_regression.rs` and count it in `PLAN.md` §6.
- **structural** — a different class or nesting. Should never happen; treat as a
  bug in the driver or the manifest.

A negative fixture names its own input as its reference, and the input was never
simplified, so every `IsSimp` card the fresh capture carries is a stamp the
simplifier put on a Mapping it handed back. Those lines are dropped from both
sides for those rows and nothing else is —
`test_simplify_regression.rs` compares them the same way and gates the card
counts separately.

Run it after every move of the `reference/starlink-ast` pin, alongside
`check_corpus_fresh.sh`.

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

## Diffing the port against C

`build_simplify_trace.sh` instruments the C library. The port emits the same
three events as `tracing` events under its `tracing` feature, from
`simplify::trace` in `crates/ast-core/src/mappings/simplify.rs`, on the target
`ast::simplify`. The field names match the C trace's, so one `awk` recipe reads
both.

```shell
AST_TRACE=1 /tmp/ast-trace/gen_simp in.map /dev/null 2> c.txt
cargo run -q --features tracing --example simp_one -- in.map 2> port.txt >/dev/null

grep MAPMERGE c.txt    | awk '{print $2, $3, $4, $5}'          > c_mm.txt
grep MAPMERGE port.txt | sed 's/.*class="\([^"]*\)".*where_=\([0-9]*\) series=\([0-9]*\) nmap=\([0-9]*\).*/\1 where=\2 series=\3 nmap=\4/' > p_mm.txt
diff c_mm.txt p_mm.txt
```

`MAPMERGE-RESULT` says what each nomination returned, so a productive one can be
told from one that declined. Counting those two on each side is what identified
PLAN.md §10.3: C's nominations succeed 23 times in 96, the port's 15 in 149, and
the whole difference is WinMap.

`RUST_LOG` selects the level: `ast::simplify=debug` (the default the example
installs) gives nominations and per-class reductions; `ast::simplify=trace` adds
the higher-volume decomposition lines, matching C's `MAPLIST`.
`SIMPLIFY-ENTER` carries a `site` field naming the call site, so a divergence
names the function to look at.

This is how PLAN.md §10.3 was narrowed from "CmpMap under-tags" to "the port
makes 149 nominations where C makes 96".

Both sides are permanent. Re-adding instrumentation each time a question of this
shape came up is what let three wrong diagnoses of §10.14 stand.
