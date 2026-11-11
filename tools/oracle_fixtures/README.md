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

`crates/starlink-ast/tests/fixtures/wcsconv/framesets/` is generated upstream, by
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

Captures every row of `crates/starlink-ast/tests/fixtures/simplify/cases.txt` from
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

## `recapture_wcslib_golden.sh` — are the wcslib golden values still what C computes?

`crates/starlink-ast/tests/wcslib_golden/projections.tsv` holds the results of the reference library's wcslib projections, which `src/wcslib/golden_tests.rs` compares the native projections against bit for bit.
The file carries its own test vectors, so it is the tool's input as well as its output.
The shared inputs (`FI`, `RI`), the parameter sets (`S`) and the inputs of the `X` lines are kept, and `recapture_wcslib_golden.c` computes every `A`, `E`, `F` and `R` line and every `X` result afresh, whatever the file held for it.
The header comment that names `reference/starlink-ast at <commit>` gets the commit the reference checkout is at.

```shell
./tools/oracle_fixtures/recapture_wcslib_golden.sh --check          # fail, showing the first differences, if the file is not a fresh capture
./tools/oracle_fixtures/recapture_wcslib_golden.sh                  # rewrite the file in place
./tools/oracle_fixtures/recapture_wcslib_golden.sh --output <path>  # write the capture to another file
```

Run it after moving the `reference/starlink-ast` pin if the range touches `wcslib/proj.c`, `wcslib/tpn.c`, `wcslib/wcstrig.c` or their headers (`proj.h`, `wcstrig.h`, `wcsmath.h`), and commit the rewritten file in the same commit range as the pin move.
After a pin move that touches none of them, only the commit in the header changes.

The script copies those six files to a scratch directory and builds them there with `-O3 -ffp-contract=off`, the flags the file's header records; the submodule is never modified.
`proj.c` includes the library's `src/memory.h` for `astSizeOf`, and that header needs the configured library, so the scratch copy has a three-line `memory.h` in its place and the tool defines `astSizeOf_`: every parameter array it hands to a projection holds `WCSLIB_MXPAR + 1` values.
The script stops if the checkout's `wcslib/` has uncommitted changes, because the commit in the header would not describe the capture.
`AST_REFERENCE` names a reference checkout other than `reference/starlink-ast`, and `--golden <path>` a file to read, and to check or rewrite, other than the committed one.

The values are those of the capture platform's libm, and the committed ones are those of macOS on aarch64.
On any other platform the script neither rewrites nor checks the file: only `--output <path>` works there, and it prints the platform it captured on.
The file it writes keeps the committed header, which still names macOS aarch64.

The tool stops, naming the line, at a line of an unknown type, an unknown projection code, a malformed input or a line out of place.
A parameter set whose set-up starts or stops failing changes shape, between one `E` line and its `F`, `R` and `X` lines; the tool says so, and it cannot invent the `X` lines of a set that had only an `E` line.

## `build_simplify_trace.sh` — watch C simplify

When a question is "does C reach this node, and in what state?", reasoning about
the call graph has been wrong more often than right.
One instrumented build answers it for any Mapping:

```shell
./tools/oracle_fixtures/build_simplify_trace.sh <scratch>/ast-trace
AST_TRACE=1 <scratch>/ast-trace/gen_simp in.map out.simp 2> trace.txt
```

Three dispatchers in `mapping.c` cover every class:

- `SIMPLIFY-ENTER class= issimp= dns= inv= nin= nout=` — every `astSimplify_`,
  with the state it was entered in. A following `SIMPLIFY-WALLED` means the
  `IsSimple` bit short-circuited it.
- `MAPMERGE class= where_= series= nmap= issimp= inv=` — every nomination: which
  class was offered to `MapMerge`, at which index and in which mode.
- `MAPMERGE-RESULT class= result= nmap=` — what the nomination returned (`-1`
  for nothing) and the list length after it.
- `MAPLIST class= series= invert= nmap_in=` — every decomposition.

The `cmpmap.c` merge loop and `MapMerge` head add the list itself:

- `MERGE-LIST depth= nominated= series= nmap= list=[ Class(eff[,s]/oN) ... ]` —
  the whole list as each element is nominated: each entry's class, the direction
  it is applied in, `,s` if it carries `IsSimple`, and its own `Invert`.
- `NOMINATE-SKIP depth= nominated= frozen=` — an element passed over, frozen or
  (with `frozen=0`) excluded by a restricted simplify.
- `FREEZE depth= modified= class=` — the oscillation check froze an element.
- `CMPMAP-HEAD depth= where_= simpler= new=` — a nominated CmpMap simplified on
  its own, and whether that changed it.
- `CMPMAP-SPLIT depth= where_=` — a nominated CmpMap was replaced in the list by
  its two components.

`depth` counts the nominations enclosing this one, so an inner merge loop run by
simplifying a nominated CmpMap reads as one level deeper. A CmpMap's class is
written `CmpMap[series]` or `CmpMap[parallel]`.
Output is also indented by simplify depth, so a subtree's whole route reads as a
block.
This settled PLAN.md §10.14 after three wrong diagnoses from reading dumps: the
trace showed C nominating the node everyone assumed it skipped, and entering it
at a different orientation, which was the real difference.

The submodule is never modified — the script copies the tracked files out and
patches the copy.

## Diffing the port against C

The port emits the same events as `tracing` events under its `tracing` feature,
from `simplify::trace` in `crates/starlink-ast/src/mappings/simplify.rs`, on the
target `ast::simplify`, with the same names and fields in the same order.
The `simp_one` example installs a subscriber that prints them, so once each
side's prefix is stripped the merge events diff line for line:

```shell
AST_TRACE=1 <scratch>/ast-trace/gen_simp in.map /dev/null 2> c.txt
cargo run -q -p starlink-ast --features tracing --example simp_one -- in.map 2> port.txt >/dev/null

EV='MERGE-LIST|MAPMERGE|CMPMAP-|NOMINATE-SKIP|FREEZE'
grep -E "$EV" c.txt    | sed -E 's/^ +//'                 > c_ev.txt
grep -E "$EV" port.txt | sed -E 's/^ *(DEBUG|TRACE) +//'  > p_ev.txt
diff c_ev.txt p_ev.txt
```

The first differing `MERGE-LIST` is usually the answer: a divergence in list
order, orientation or tags shows there before it shows in any result.
The stored orientation (`/oN` and `MAPMERGE inv=`) differs wherever C's
shared-pointer `Invert` history does, which the simplify corpus licenses per
case in `SHARED_INVERT_HISTORY_LICENSED`; strip it with
`sed -E 's#/o[01]\)#)#g; s/ inv=[01]$//'` to compare the rest.
`SIMPLIFY-ENTER` is not part of the diff: the port's carries a `site` field
naming the call site rather than C's `dns`, so a divergence names the function to
look at.

`MAPMERGE-RESULT` tells a productive nomination from one that declined. Counting
those on each side is what identified PLAN.md §10.3: C's nominations succeed 23
times in 96, the port's 15 in 149, and the whole difference is WinMap.
The `MERGE-LIST` diff is what found the cmpmap.c:1677-1734 split, the per-object
`Frozen` flag and the WinMap distribution shortcut together, after porting any
one of them alone had regressed the corpus.

`RUST_LOG` selects the level: `ast::simplify=debug` (the default the example
installs) gives everything above; `ast::simplify=trace` adds the higher-volume
decomposition lines, matching C's `MAPLIST`.

The port emits `MAPMERGE` and `MAPMERGE-RESULT` in `dispatch_map_merge`, its counterpart of C's `astMapMerge` dispatcher.
A merge that hands off to another class's `MapMerge` must go through `dispatch_map_merge` wherever C goes through `astMapMerge`, as `ShiftMap`'s delegation to `WinMap` does (shiftmap.c:706).
Calling the class's merge directly skips the dispatcher's `DoNotSimplify` check and both events, so the two streams drift apart at that point.

Both sides are permanent. Add an event to both rather than a temporary print:
re-adding instrumentation each time a question of this shape came up is what let
three wrong diagnoses of §10.14 stand.
