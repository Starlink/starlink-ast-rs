# Vendored C Dependencies

C sources compiled by `starlink-ast`'s `build.rs`. These are owned by this
repository — they do not come from the reference submodule at build time.

## Source

All files copied from `reference/starlink-ast/` at submodule commit
`3d3f255f433906417e81d3783537b42d91d49e38` (v9.5.0-165-g3d3f255f)
on 2026-10-28.

## pal/

PAL positional-astronomy library routines used by AST.

- Origin: `reference/starlink-ast/pal/`
- Files: 29 `.c` sources, 3 headers (`pal.h`, `pal1sofa.h`, `palmac.h`);
  `pal/` is identical to upstream PAL v0.9.10-9-g75017db, less the
  routines AST does not call
- Modifications: None (AST-specific symbol renaming is applied via
  `pal2ast.h` at compile time, not by modifying sources)

## erfa/

ERFA (Essential Routines for Fundamental Astronomy) 2.0.1, used by PAL
and by AST directly.

- Origin: `reference/starlink-ast/erfa/`
- Files: 236 `.c` sources, 4 headers (`erfa.h`, `erfam.h`,
  `erfaextra.h`, `erfadatextra.h`), copied unmodified from ERFA
  v2.0.1-12-g1a8044c
- Modifications: None (symbol renaming via `erfa2ast.h` at compile time)

## wcslib

Ported to Rust in `crates/starlink-ast/src/wcslib/`; the vendored C was deleted in the commit "Delete the vendored wcslib".

## palwrap.c

Single-translation-unit wrapper that `#include`s all PAL and ERFA `.c`
files after applying symbol-renaming macros. Compiled as one object file.

- Origin: `reference/starlink-ast/palwrap.c`

## pal2ast.h

Macro definitions renaming `palXxx` → `astPalXxx` to avoid symbol clashes.

- Origin: `reference/starlink-ast/pal2ast.h`

## erfa2ast.h

Macro definitions renaming `eraXxx` → `astEraXxx` to avoid symbol clashes.

- Origin: `reference/starlink-ast/erfa2ast.h`

## ast_err.msg

Starlink message file defining AST error codes. Parsed by `starlink-ast`'s
`build.rs` to generate the Rust `ErrorCode` enum with MESSGEN-compatible
numeric values.

- Origin: `reference/starlink-ast/ast_err.msg`
