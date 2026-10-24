# Vendored C Dependencies

C sources compiled by `starlink-ast`'s `build.rs`. These are owned by this
repository — they do not come from the reference submodule at build time.

## Source

All files copied from `reference/starlink-ast/` at submodule commit
`7c12422fbd652e10f57fb0783a8160ced89fdad3` (v9.2.10-300-g7c12422f)
on 2026-04-27.

## pal/

PAL positional-astronomy library routines used by AST.

- Origin: `reference/starlink-ast/pal/`
- Files: 37 `.c` sources, 3 headers (`pal.h`, `pal1sofa.h`, `palmac.h`)
- Modifications: None (AST-specific symbol renaming is applied via
  `pal2ast.h` at compile time, not by modifying sources)

## erfa/

ERFA (Essential Routines for Fundamental Astronomy) snapshot used by PAL.

- Origin: `reference/starlink-ast/erfa/`
- Files: 232 `.c` sources, 2 headers (`erfa.h`, `erfam.h`)
- Modifications: None (symbol renaming via `erfa2ast.h` at compile time)

## wcslib/

AST's fork of wcslib v2.9 (~2002). This fork diverged significantly from
upstream wcslib: AST-prefixed symbols, dynamic `p`/`p2` arrays in the
`prjprm` struct, and an added TPN projection.

- Origin: `reference/starlink-ast/wcslib/`
- Files: 3 `.c` sources (`proj.c`, `tpn.c`, `wcstrig.c`),
  4 headers (`proj.h`, `wcsmath.h`, `wcstrig.h`, `memory.h`)
- Additional: `ast_memory_stub.c` — stub `astSizeOf_` implementation
  for standalone compilation
- Modifications applied during copy:
  - `proj.c` line 286: changed `#include "../src/memory.h"` to
    `#include "memory.h"` to remove AST source tree dependency
  - Added `memory.h` stub providing `astSizeOf` macro and declaration
  - Added `ast_memory_stub.c` providing minimal `astSizeOf_` implementation
- **Note:** This directory will be deleted when the Rust port of wcslib
  lands (implementation step 6a).

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
