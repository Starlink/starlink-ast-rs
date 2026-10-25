# Step 0: Project Bootstrap — Design Spec

## Summary

Scaffold the `starlink-astrs` Rust workspace so that `cargo build` and
`cargo test` pass on a fresh clone. This establishes the build
infrastructure that all subsequent implementation steps build on.

## Deliverables

1. Workspace `Cargo.toml` with all 6 crates as members.
2. Vendored C sources (`pal/`, `erfa/`, `wcslib/`, `palwrap.c`,
   `pal2ast.h`, `erfa2ast.h`, `ast_err.msg`) copied into `vendor/`.
3. `starlink-ast`'s `build.rs` compiles vendored C and generates the
   `ErrorCode` enum from `ast_err.msg`.
4. All crates compile; `cargo test -p starlink-ast` passes a trivial
   smoke test.
5. CI via GitHub Actions (build/test/clippy/fmt on PRs).
6. `vendor/README.md` with provenance documentation.

## Workspace Structure

```
starlink-astrs/
├── Cargo.toml                    # workspace root
├── rust-toolchain.toml           # channel = "stable"
├── .gitignore                    # /target, *.o, etc.
├── crates/
│   ├── starlink-ast/
│   │   ├── Cargo.toml            # [lib], build-dep: cc
│   │   ├── build.rs              # compiles vendor C; generates ErrorCode
│   │   └── src/
│   │       ├── lib.rs            # re-exports error module
│   │       └── error.rs          # AstError, AstResult, include! generated enum
│   ├── starlink-ast-capi/
│   │   ├── Cargo.toml            # crate-type = ["cdylib", "staticlib"], dep: starlink-ast
│   │   └── src/lib.rs            # placeholder
│   ├── ast-plugins/
│   │   ├── Cargo.toml            # dep: starlink-ast
│   │   └── src/lib.rs            # placeholder
│   ├── ast-ffi-tests/
│   │   ├── Cargo.toml            # dev-dep: starlink-ast-capi
│   │   └── src/lib.rs            # placeholder
│   ├── ast-oracle-tests/
│   │   ├── Cargo.toml            # dev-dep: starlink-ast
│   │   └── src/lib.rs            # placeholder
│   └── ast-oracle-bench/
│       ├── Cargo.toml            # dep: criterion, starlink-ast
│       └── src/lib.rs            # placeholder
├── vendor/
│   ├── pal/                      # from reference/starlink-ast/pal/
│   ├── erfa/                     # from reference/starlink-ast/erfa/
│   ├── wcslib/                   # from reference/starlink-ast/wcslib/
│   ├── palwrap.c                 # single-TU wrapper
│   ├── pal2ast.h                 # PAL symbol renaming (palXxx → astPalXxx)
│   ├── erfa2ast.h                # ERFA symbol renaming (eraXxx → astEraXxx)
│   ├── ast_err.msg               # error code definitions
│   └── README.md                 # provenance
└── .github/
    └── workflows/
        └── ci.yml                # PR: build, test, clippy, fmt
```

### Default-members

Submodule-dependent crates are excluded from default workspace members
so the everyday build works without the submodule:

```toml
[workspace]
members = ["crates/*"]
default-members = [
    "crates/starlink-ast",
    "crates/starlink-ast-capi",
    "crates/ast-plugins",
]
```

`ast-ffi-tests`, `ast-oracle-tests`, and `ast-oracle-bench` must be
targeted explicitly (e.g. `cargo test -p ast-ffi-tests`).

## Vendored C Compilation (build.rs)

### pal + erfa via palwrap.c

The original C library compiles all PAL and ERFA sources through a
single translation unit (`palwrap.c`) that `#include`s every `.c`
file after applying symbol-renaming macros. We replicate this exactly.

Paths are relative to the `crates/starlink-ast/` manifest directory (the
`cc` crate resolves relative to `CARGO_MANIFEST_DIR`):

```rust
cc::Build::new()
    .file("../../vendor/palwrap.c")
    .include("../../vendor")       // finds pal2ast.h, erfa2ast.h
    .include("../../vendor/pal")   // pal source headers
    .include("../../vendor/erfa")  // erfa source headers
    .warnings(false)
    .compile("ast_pal_erfa");
```

`HAVE_CONFIG_H` is intentionally **not** defined — this causes
`palwrap.c` to skip `#include <config.h>` and since `EXTERNAL_PAL`
is also undefined, all sources compile in.

### wcslib (3 files)

```rust
cc::Build::new()
    .files(&[
        "../../vendor/wcslib/proj.c",
        "../../vendor/wcslib/wcstrig.c",
        "../../vendor/wcslib/tpn.c",
    ])
    .include("../../vendor/wcslib")
    .warnings(false)
    .compile("ast_wcslib");
```

## ErrorCode Generation (build.rs)

### Source

`vendor/ast_err.msg` — the Starlink message file format.

### Parsing

The `build.rs` parser tracks directives (`.FACILITY`, `.SEVERITY`,
`.BASE`) and message definitions (`NAME <description>`).

### Formula

Error code values use the Starlink MESSGEN formula:

```
errcode = ((facility_code | 0x800) << 16) + ((message_number | 0x1000) << 3) + severity
```

For AST: `facility_code = 1521`, `severity = 2` (ERROR), `base = 300`.

Example: `ATGER` is message 300, so:
```
((1521 | 2048) << 16) + ((300 | 4096) << 3) + 2 = 233933154
```

### Generated Output

Written to `$OUT_DIR/error_codes.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ErrorCode {
    /// attribute getting error
    Atger = 233933154,
    /// attribute setting error
    Atser = 233933162,
    // ... one variant per message in ast_err.msg
}
```

Plus:
- `impl Display for ErrorCode` — prints the description text.
- `ErrorCode::as_raw(self) -> u32` — returns the numeric value.
- `ErrorCode::from_raw(u32) -> Option<ErrorCode>` — for CFFI use.

### Usage in Rust

```rust
use starlink_ast::error::{AstError, AstResult, ErrorCode};

// Raising
return Err(AstError::new(ErrorCode::Atger, "failed to get attribute Foo"));

// Matching
match err.code() {
    ErrorCode::Atger => { /* handle */ }
    ErrorCode::Axiin => { /* handle */ }
    _ => { /* propagate */ }
}

// Raw value for CFFI
let raw: u32 = ErrorCode::Atger.as_raw();
```

### Error types (src/error.rs)

```rust
pub struct AstError {
    code: ErrorCode,
    message: String,
}

pub type AstResult<T> = Result<T, AstError>;
```

## CI Configuration

Single GitHub Actions workflow (`.github/workflows/ci.yml`):

**Trigger**: push to `main`, pull requests.

**Steps**:
1. Checkout (no submodules)
2. Install stable Rust via `dtolnay/rust-toolchain`
3. `cargo build --workspace`
4. `cargo test -p starlink-ast`
5. `cargo clippy --workspace -- -D warnings`
6. `cargo fmt --all -- --check`

A nightly parity-test job (submodule-dependent) will be added when
`starlink-ast-capi` and `ast-ffi-tests` have content to test.

## rust-toolchain.toml

```toml
[toolchain]
channel = "stable"
```

MSRV (1.85+, required for edition 2024) is enforced by CI, not by
pinning a specific version in the toolchain file. Developers use
whatever stable they have installed.

## Vendor Provenance (vendor/README.md)

Records for each copied directory:
- Source location (reference/starlink-ast/ path)
- Submodule commit: `7c12422fbd652e10f57fb0783a8160ced89fdad3`
- Date copied
- List of AST-specific modifications (if any; for this initial copy,
  the modifications are upstream in Starlink/starlink-ast already)
- Note that wcslib will be replaced by a Rust port at step 6a

## What This Step Does NOT Include

- No trait definitions (`Mapping`, `Frame`, `Region`) — deferred to Step 1
- No coordinate array types (`PointSet`, `PointList`) — Step 1
- No CFFI symbols in `starlink-ast-capi` — Step 1+
- No nightly CI job for parity tests — added when there's content
- No `cminpack` — replaced by `levenberg-marquardt` crate (later step)
