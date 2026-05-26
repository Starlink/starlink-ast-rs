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

## Provenance rule

When a fixture's expected value changes (e.g., a captured number drifts
across reference rebuilds), regenerate ALL fixtures in the same module
and re-run the affected tests. Don't update one number by hand.
