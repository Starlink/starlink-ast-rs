# AST -- World Coordinate Systems Library... in Rust

The Starlink [AST library](https://starlink.eao.hawaii.edu/starlink/AST) provides
a comprehensive range of facilities for
attaching world coordinate systems to astronomical data, for
retrieving and interpreting that information in a variety of formats,
including FITS-WCS, and for generating graphical output based on it.

This library should be of interest to anyone writing astronomical
applications which need to manipulate coordinate system data,
especially celestial or spectral coordinate systems.
AST is portable and environment-independent.

This repository is a project to create a from-scratch rewrite of the C library in Rust, retaining a thin C wrapper that will enable existing users to link against it without changing their code.

## Building

Requires a C compiler (for vendored PAL, ERFA, and wcslib) and a Rust toolchain. The minimum supported Rust version is pinned in `rust-toolchain.toml`.

```shell
cargo build                    # build the default crates
cargo build --workspace        # build everything including oracle test/bench stubs
```

## Running Tests

Tests are organised into tiers. Tiers 1 and 2 run on every build with no extra setup:

```shell
cargo test -p ast-core                       # tier 1+2: all Rust unit and integration tests
cargo test -p ast-core --test test_skyframe  # run a single test file
```

Tiers 3 and 4 require the reference C library submodule:

```shell
git submodule update --init reference/starlink-ast
cargo test -p ast-ffi-tests                  # tier 3: C tests vs Rust libast
cargo test -p ast-oracle-tests               # tier 4: bit/ULP comparison
```

### Lints and Formatting

Both are CI gates:

```shell
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Generating Documentation

```shell
cargo doc --workspace --no-deps --open
```

This builds rustdoc for all crates and opens the result in a browser. The `ast-core` crate contains the public Rust API.

## Repository Layout

```
starlink-astrs/
├── crates/
│   ├── ast-core/          # pure-Rust idiomatic API
│   ├── ast-capi/          # cdylib; re-exports ast.h symbols
│   ├── ast-plugins/       # Grf / Grf3d backends
│   ├── ast-ffi-tests/     # tier 3: C tests vs Rust libast
│   ├── ast-oracle-tests/  # tier 4: bit/ULP comparison
│   └── ast-oracle-bench/  # tier 5: Rust vs C performance
├── vendor/                # vendored C sources (pal, erfa, wcslib)
├── reference/             # git submodule (optional, for tier 3+4)
└── docs/                  # design specs and implementation plans
```

## Documentation for the Classical Interface

Full documentation of the C and Fortran versions is available in the Starlink User Notes:

- [SUN/210](https://starlink.eao.hawaii.edu/devdocs/sun210.htx/sun210.html) -- Fortran documentation
- [SUN/211](https://starlink.eao.hawaii.edu/devdocs/sun211.htx/sun211.html) -- C documentation
