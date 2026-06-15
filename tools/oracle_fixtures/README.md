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
