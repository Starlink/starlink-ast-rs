# Vendored Files

Files copied from `reference/starlink-ast/` for `starlink-ast`'s build script.
These are owned by this repository; they do not come from the reference submodule at build time.
The crate compiles no C.

## Source

Copied from `reference/starlink-ast/` at submodule commit `3d3f255f433906417e81d3783537b42d91d49e38` (v9.5.0-165-g3d3f255f) on 2026-10-28.

## pal and erfa

PAL and ERFA are ported to Rust in `crates/starlink-ast/src/pal/` and `crates/starlink-ast/src/erfa/`; the vendored C was deleted in the commit "Delete the vendored PAL and ERFA".
`palwrap.c`, `pal2ast.h` and `erfa2ast.h` went with it.

## wcslib

Ported to Rust in `crates/starlink-ast/src/wcslib/`; the vendored C was deleted in the commit "Delete the vendored wcslib".

## ast_err.msg

Starlink message file defining AST error codes.
Parsed by `starlink-ast`'s `build.rs` to generate the Rust `ErrorCode` enum with MESSGEN-compatible numeric values.

- Origin: `reference/starlink-ast/ast_err.msg`
