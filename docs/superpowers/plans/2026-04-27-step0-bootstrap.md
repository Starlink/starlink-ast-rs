# Step 0: Project Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scaffold the `starlink-astrs` Rust workspace so that `cargo build` and `cargo test -p starlink-ast` pass, with vendored C compilation and ErrorCode generation working.

**Architecture:** A Cargo workspace with 6 crates. `starlink-ast` uses a `build.rs` that compiles vendored PAL+ERFA (via the existing `palwrap.c` single-TU wrapper) and wcslib (3 files directly), and generates a Rust `ErrorCode` enum from `ast_err.msg`. Submodule-dependent crates are excluded from default-members.

**Tech Stack:** Rust 2024 edition, `cc` crate for C compilation, GitHub Actions CI.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Workspace root; defines members and default-members |
| `rust-toolchain.toml` | Pins stable channel |
| `crates/starlink-ast/Cargo.toml` | Library crate with `cc` build-dependency |
| `crates/starlink-ast/build.rs` | Compiles vendor C; generates `error_codes.rs` |
| `crates/starlink-ast/src/lib.rs` | Crate root; declares `error` module |
| `crates/starlink-ast/src/error.rs` | `AstError`, `AstResult`; includes generated enum |
| `crates/starlink-ast-capi/Cargo.toml` | cdylib + staticlib; depends on starlink-ast |
| `crates/starlink-ast-capi/src/lib.rs` | Placeholder |
| `crates/ast-plugins/Cargo.toml` | Depends on starlink-ast |
| `crates/ast-plugins/src/lib.rs` | Placeholder |
| `crates/ast-ffi-tests/Cargo.toml` | Depends on starlink-ast-capi |
| `crates/ast-ffi-tests/src/lib.rs` | Placeholder |
| `crates/ast-oracle-tests/Cargo.toml` | Depends on starlink-ast |
| `crates/ast-oracle-tests/src/lib.rs` | Placeholder |
| `crates/ast-oracle-bench/Cargo.toml` | Depends on criterion + starlink-ast |
| `crates/ast-oracle-bench/src/lib.rs` | Placeholder |
| `vendor/pal/` | Copied PAL sources |
| `vendor/erfa/` | Copied ERFA sources |
| `vendor/wcslib/` | Copied wcslib sources + stub `memory.h` |
| `vendor/palwrap.c` | Single-TU compilation wrapper |
| `vendor/pal2ast.h` | PAL symbol renaming macros |
| `vendor/erfa2ast.h` | ERFA symbol renaming macros |
| `vendor/ast_err.msg` | Error code definitions |
| `vendor/README.md` | Provenance documentation |
| `.github/workflows/ci.yml` | PR CI: build, test, clippy, fmt |

---

### Task 1: Copy Vendored C Sources

**Files:**
- Create: `vendor/pal/` (37 `.c` files + 3 `.h` files)
- Create: `vendor/erfa/` (232 `.c` files + 2 `.h` files)
- Create: `vendor/wcslib/` (3 `.c` files + 4 `.h` files)
- Create: `vendor/palwrap.c`
- Create: `vendor/pal2ast.h`
- Create: `vendor/erfa2ast.h`
- Create: `vendor/ast_err.msg`

- [ ] **Step 1: Copy PAL sources**

```bash
cp -r reference/starlink-ast/pal vendor/pal
```

- [ ] **Step 2: Copy ERFA sources**

```bash
cp -r reference/starlink-ast/erfa vendor/erfa
```

- [ ] **Step 3: Copy wcslib sources**

```bash
cp -r reference/starlink-ast/wcslib vendor/wcslib
```

- [ ] **Step 4: Copy wrapper and renaming headers**

```bash
cp reference/starlink-ast/palwrap.c vendor/palwrap.c
cp reference/starlink-ast/pal2ast.h vendor/pal2ast.h
cp reference/starlink-ast/erfa2ast.h vendor/erfa2ast.h
```

- [ ] **Step 5: Copy ast_err.msg**

```bash
cp reference/starlink-ast/ast_err.msg vendor/ast_err.msg
```

- [ ] **Step 6: Create stub memory.h for wcslib**

`vendor/wcslib/proj.c` includes `../src/memory.h` from the AST source tree, but
uses no symbols from it. Provide a minimal stub at `vendor/wcslib/memory.h` and
patch `proj.c` to include it locally instead.

Create `vendor/wcslib/memory.h`:

```c
#ifndef MEMORY_INCLUDED
#define MEMORY_INCLUDED
/* Minimal stub: proj.c includes this but uses no symbols from it.
   The full AST memory.h is not needed for standalone wcslib compilation. */
#endif
```

Then edit `vendor/wcslib/proj.c` line 286 — change:
```c
#include "../src/memory.h"
```
to:
```c
#include "memory.h"
```

- [ ] **Step 7: Commit vendored sources**

```bash
git add vendor/
git commit -m "Copy vendored C sources (pal, erfa, wcslib) from starlink-ast

Source: reference/starlink-ast at 7c12422fbd652e10f57fb0783a8160ced89fdad3
- pal/: 37 source files, 3 headers (PAL astrometry routines)
- erfa/: 232 source files, 2 headers (ERFA routines)
- wcslib/: 3 source files, 4 headers (WCS projections, AST fork)
- palwrap.c: single-TU compilation wrapper
- pal2ast.h, erfa2ast.h: symbol renaming macros
- ast_err.msg: error code definitions for build.rs
- wcslib/memory.h: stub replacing ../src/memory.h dependency"
```

---

### Task 2: Create Workspace Cargo.toml and rust-toolchain.toml

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`

- [ ] **Step 1: Create workspace Cargo.toml**

Create `Cargo.toml`:

```toml
[workspace]
members = ["crates/*"]
default-members = [
    "crates/starlink-ast",
    "crates/starlink-ast-capi",
    "crates/ast-plugins",
]
resolver = "2"

[workspace.package]
edition = "2024"
rust-version = "1.85"
license = "LGPL-3.0-or-later"
repository = "https://github.com/Starlink/starlink-ast-rs"

[workspace.dependencies]
cc = "1"
```

- [ ] **Step 2: Create rust-toolchain.toml**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml rust-toolchain.toml
git commit -m "Add workspace Cargo.toml and rust-toolchain.toml"
```

---

### Task 3: Create starlink-ast Crate Skeleton

**Files:**
- Create: `crates/starlink-ast/Cargo.toml`
- Create: `crates/starlink-ast/src/lib.rs`

- [ ] **Step 1: Create crate directory**

```bash
mkdir -p crates/starlink-ast/src
```

- [ ] **Step 2: Create starlink-ast Cargo.toml**

Create `crates/starlink-ast/Cargo.toml`:

```toml
[package]
name = "starlink-ast"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Pure-Rust port of the Starlink AST world coordinate system library"

[build-dependencies]
cc.workspace = true
```

- [ ] **Step 3: Create placeholder lib.rs**

Create `crates/starlink-ast/src/lib.rs`:

```rust
#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod error;
```

- [ ] **Step 4: Create placeholder error.rs**

Create `crates/starlink-ast/src/error.rs`:

```rust
//! Error types for the AST library.

use std::fmt;

include!(concat!(env!("OUT_DIR"), "/error_codes.rs"));

/// An error produced by the AST library.
#[derive(Debug, Clone)]
pub struct AstError {
    code: ErrorCode,
    message: String,
}

impl AstError {
    /// Create a new AST error with the given code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Return the error code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Return the error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AstError {}

/// A Result type alias for AST operations.
pub type AstResult<T> = Result<T, AstError>;
```

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/
git commit -m "Add starlink-ast crate skeleton with error module"
```

---

### Task 4: Write starlink-ast build.rs

**Files:**
- Create: `crates/starlink-ast/build.rs`

- [ ] **Step 1: Write build.rs**

Create `crates/starlink-ast/build.rs`:

```rust
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("../../vendor");

    compile_pal_erfa(&vendor_dir);
    compile_wcslib(&vendor_dir);
    generate_error_codes(&vendor_dir);
}

fn compile_pal_erfa(vendor_dir: &PathBuf) {
    println!("cargo::rerun-if-changed={}", vendor_dir.join("palwrap.c").display());

    cc::Build::new()
        .file(vendor_dir.join("palwrap.c"))
        .include(vendor_dir)
        .include(vendor_dir.join("pal"))
        .include(vendor_dir.join("erfa"))
        .warnings(false)
        .compile("ast_pal_erfa");
}

fn compile_wcslib(vendor_dir: &PathBuf) {
    let wcslib_dir = vendor_dir.join("wcslib");

    println!("cargo::rerun-if-changed={}", wcslib_dir.join("proj.c").display());
    println!("cargo::rerun-if-changed={}", wcslib_dir.join("tpn.c").display());
    println!("cargo::rerun-if-changed={}", wcslib_dir.join("wcstrig.c").display());

    cc::Build::new()
        .file(wcslib_dir.join("proj.c"))
        .file(wcslib_dir.join("tpn.c"))
        .file(wcslib_dir.join("wcstrig.c"))
        .include(&wcslib_dir)
        .warnings(false)
        .compile("ast_wcslib");
}

fn generate_error_codes(vendor_dir: &PathBuf) {
    let msg_path = vendor_dir.join("ast_err.msg");
    println!("cargo::rerun-if-changed={}", msg_path.display());

    let contents = fs::read_to_string(&msg_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", msg_path.display(), e));

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("error_codes.rs");
    let mut out = fs::File::create(&out_path).unwrap();

    let mut facility_code: u64 = 0;
    let mut severity: u64 = 0;
    let mut message_number: u64 = 0;

    // Collect all error codes first
    let mut variants: Vec<(String, u64, String)> = Vec::new(); // (name, value, description)

    for line in contents.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('!') {
            continue;
        }

        if let Some(rest) = line.strip_prefix(".FACILITY") {
            // Parse: AST,1521/PREFIX=AST__
            let rest = rest.trim();
            if let Some(comma_pos) = rest.find(',') {
                let after_comma = &rest[comma_pos + 1..];
                if let Some(slash_pos) = after_comma.find('/') {
                    facility_code = after_comma[..slash_pos].parse().unwrap();
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix(".SEVERITY") {
            let sev_name = rest.trim();
            severity = match sev_name {
                "WARNING" => 0,
                "SUCCESS" => 1,
                "ERROR" => 2,
                "INFORMATIONAL" => 3,
                "SEVERE" => 4,
                _ => 0,
            };
            continue;
        }

        if let Some(rest) = line.strip_prefix(".BASE") {
            message_number = rest.trim().parse().unwrap();
            continue;
        }

        if line.starts_with(".TITLE") || line.starts_with(".END") {
            continue;
        }

        // Parse message definition: NAME <description>
        if let Some(angle_start) = line.find('<') {
            if let Some(angle_end) = line.find('>') {
                let name = line[..angle_start].trim();
                let description = &line[angle_start + 1..angle_end];

                // Starlink MESSGEN formula
                let errcode = ((facility_code | 0x800) << 16)
                    + ((message_number | 0x1000) << 3)
                    + severity;

                // Convert name to PascalCase: "ATGER" -> "Atger", "3DFSET" -> "ThreeDfset"
                let variant_name = to_pascal_case(name);

                variants.push((variant_name, errcode, description.to_string()));
                message_number += 1;
            }
        }
    }

    // Write the enum
    writeln!(out, "/// Error codes defined by the AST library.").unwrap();
    writeln!(out, "///").unwrap();
    writeln!(out, "/// Values match the Starlink MESSGEN formula for binary compatibility").unwrap();
    writeln!(out, "/// with the C library's `AST__*` error constants.").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "#[repr(u32)]").unwrap();
    writeln!(out, "#[non_exhaustive]").unwrap();
    writeln!(out, "pub enum ErrorCode {{").unwrap();
    for (name, value, desc) in &variants {
        writeln!(out, "    /// {desc}").unwrap();
        writeln!(out, "    {name} = {value},").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Write Display impl
    writeln!(out, "impl fmt::Display for ErrorCode {{").unwrap();
    writeln!(out, "    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {{").unwrap();
    writeln!(out, "        let desc = match self {{").unwrap();
    for (name, _, desc) in &variants {
        writeln!(out, "            ErrorCode::{name} => \"{desc}\",").unwrap();
    }
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        write!(f, \"{{desc}}\")").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Write as_raw method
    writeln!(out, "impl ErrorCode {{").unwrap();
    writeln!(out, "    /// Return the raw numeric error code value.").unwrap();
    writeln!(out, "    pub fn as_raw(self) -> u32 {{").unwrap();
    writeln!(out, "        self as u32").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    // Write from_raw method
    writeln!(out, "    /// Convert a raw numeric value to an ErrorCode, if valid.").unwrap();
    writeln!(out, "    pub fn from_raw(value: u32) -> Option<Self> {{").unwrap();
    writeln!(out, "        match value {{").unwrap();
    for (name, value, _) in &variants {
        writeln!(out, "            {value} => Some(ErrorCode::{name}),").unwrap();
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}

/// Convert an AST error name to PascalCase.
///
/// Rules:
/// - All-uppercase names: capitalize first letter, lowercase rest ("ATGER" -> "Atger")
/// - Names starting with a digit: prefix with word ("3DFSET" -> "ThreeDfset")
fn to_pascal_case(name: &str) -> String {
    let name = if name.starts_with(|c: char| c.is_ascii_digit()) {
        let digit_word = match name.chars().next().unwrap() {
            '0' => "Zero",
            '1' => "One",
            '2' => "Two",
            '3' => "Three",
            '4' => "Four",
            '5' => "Five",
            '6' => "Six",
            '7' => "Seven",
            '8' => "Eight",
            '9' => "Nine",
            _ => unreachable!(),
        };
        format!("{}{}", digit_word, &name[1..])
    } else {
        name.to_string()
    };

    let mut result = String::new();
    let mut chars = name.chars();
    if let Some(first) = chars.next() {
        result.push(first.to_ascii_uppercase());
    }
    for c in chars {
        result.push(c.to_ascii_lowercase());
    }
    result
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p starlink-ast
```

Expected: successful build producing `libast_pal_erfa.a`, `libast_wcslib.a`, and the `error_codes.rs` generated file.

- [ ] **Step 3: Commit**

```bash
git add crates/starlink-ast/build.rs
git commit -m "Add build.rs: vendor C compilation and ErrorCode generation"
```

---

### Task 5: Add Smoke Test for ErrorCode

**Files:**
- Create: `crates/starlink-ast/tests/error_codes.rs`

- [ ] **Step 1: Write the test**

Create `crates/starlink-ast/tests/error_codes.rs`:

```rust
use starlink_ast::error::ErrorCode;

#[test]
fn atger_has_correct_raw_value() {
    // MESSGEN formula: ((facility | 0x800) << 16) + ((msg_num | 0x1000) << 3) + severity
    // AST__ATGER: facility=1521, msg_num=300, severity=2 (ERROR)
    // Verified against cmake-generated ast_err.h: AST__ATGER = 233933154
    assert_eq!(ErrorCode::Atger.as_raw(), 233_933_154);
}

#[test]
fn from_raw_round_trips() {
    let code = ErrorCode::Atger;
    let raw = code.as_raw();
    assert_eq!(ErrorCode::from_raw(raw), Some(code));
}

#[test]
fn from_raw_returns_none_for_invalid() {
    assert_eq!(ErrorCode::from_raw(0), None);
    assert_eq!(ErrorCode::from_raw(u32::MAX), None);
}

#[test]
fn display_shows_description() {
    let desc = format!("{}", ErrorCode::Atger);
    assert_eq!(desc, "attribute getting error");
}

#[test]
fn error_code_3dfset_accessible() {
    // Verify that the "3DFSET" error code (starts with digit) is accessible
    // as "ThreeDfset" in Rust
    let _ = ErrorCode::ThreeDfset;
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test -p starlink-ast
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/starlink-ast/tests/
git commit -m "Add smoke tests for ErrorCode generation"
```

---

### Task 6: Create Stub Crates (starlink-ast-capi, ast-plugins, ast-ffi-tests, ast-oracle-tests, ast-oracle-bench)

**Files:**
- Create: `crates/starlink-ast-capi/Cargo.toml`, `crates/starlink-ast-capi/src/lib.rs`
- Create: `crates/ast-plugins/Cargo.toml`, `crates/ast-plugins/src/lib.rs`
- Create: `crates/ast-ffi-tests/Cargo.toml`, `crates/ast-ffi-tests/src/lib.rs`
- Create: `crates/ast-oracle-tests/Cargo.toml`, `crates/ast-oracle-tests/src/lib.rs`
- Create: `crates/ast-oracle-bench/Cargo.toml`, `crates/ast-oracle-bench/src/lib.rs`

- [ ] **Step 1: Create starlink-ast-capi**

Create `crates/starlink-ast-capi/Cargo.toml`:

```toml
[package]
name = "starlink-ast-capi"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "C-ABI compatible shim for the Rust AST library"

[lib]
crate-type = ["cdylib", "staticlib"]

[dependencies]
starlink-ast = { path = "../starlink-ast" }
```

Create `crates/starlink-ast-capi/src/lib.rs`:

```rust
#![allow(missing_docs)]
```

- [ ] **Step 2: Create ast-plugins**

Create `crates/ast-plugins/Cargo.toml`:

```toml
[package]
name = "ast-plugins"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Graphics and I/O plugin backends for the AST library"

[dependencies]
starlink-ast = { path = "../starlink-ast" }
```

Create `crates/ast-plugins/src/lib.rs`:

```rust
#![warn(missing_docs)]

//! Plugin backends (Grf, Grf3d) for the AST library.
```

- [ ] **Step 3: Create ast-ffi-tests**

Create `crates/ast-ffi-tests/Cargo.toml`:

```toml
[package]
name = "ast-ffi-tests"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "FFI parity tests: C ast_tester programs against Rust-built libast"

[dependencies]
starlink-ast-capi = { path = "../starlink-ast-capi" }
```

Create `crates/ast-ffi-tests/src/lib.rs`:

```rust
#![allow(missing_docs)]
```

- [ ] **Step 4: Create ast-oracle-tests**

Create `crates/ast-oracle-tests/Cargo.toml`:

```toml
[package]
name = "ast-oracle-tests"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Oracle correctness tests: Rust starlink-ast vs reference C libast"

[dependencies]
starlink-ast = { path = "../starlink-ast" }
```

Create `crates/ast-oracle-tests/src/lib.rs`:

```rust
#![allow(missing_docs)]
```

- [ ] **Step 5: Create ast-oracle-bench**

Create `crates/ast-oracle-bench/Cargo.toml`:

```toml
[package]
name = "ast-oracle-bench"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Performance benchmarks: Rust starlink-ast vs reference C libast"

[dependencies]
starlink-ast = { path = "../starlink-ast" }
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "oracle"
harness = false
```

Create `crates/ast-oracle-bench/src/lib.rs`:

```rust
#![allow(missing_docs)]
```

- [ ] **Step 6: Verify workspace builds**

```bash
cargo build --workspace
```

Expected: all 6 crates compile successfully. Note: `starlink-ast-capi` produces a cdylib
which will have unresolved symbols at this point (that's expected for a placeholder).

If the cdylib link fails due to no exported symbols, add a dummy export to
`crates/starlink-ast-capi/src/lib.rs`:

```rust
#![allow(missing_docs)]

#[unsafe(no_mangle)]
pub extern "C" fn ast_version() -> i32 {
    0
}
```

- [ ] **Step 7: Commit**

```bash
git add crates/starlink-ast-capi/ crates/ast-plugins/ crates/ast-ffi-tests/ crates/ast-oracle-tests/ crates/ast-oracle-bench/
git commit -m "Add stub crates: starlink-ast-capi, ast-plugins, ast-ffi-tests, ast-oracle-tests, ast-oracle-bench"
```

---

### Task 7: Write vendor/README.md

**Files:**
- Create: `vendor/README.md`

- [ ] **Step 1: Write provenance documentation**

Create `vendor/README.md`:

```markdown
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
- Modifications applied during copy:
  - `proj.c` line 286: changed `#include "../src/memory.h"` to
    `#include "memory.h"` to remove AST source tree dependency
  - Added stub `memory.h` (proj.c includes it but uses no symbols from it)
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
```

- [ ] **Step 2: Commit**

```bash
git add vendor/README.md
git commit -m "Add vendor/README.md documenting provenance"
```

---

### Task 8: Add GitHub Actions CI

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create CI workflow**

```bash
mkdir -p .github/workflows
```

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: "-D warnings"

jobs:
  build-and-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - name: Build
        run: cargo build --workspace

      - name: Test
        run: cargo test -p starlink-ast

      - name: Clippy
        run: cargo clippy --workspace -- -D warnings

      - name: Format check
        run: cargo fmt --all -- --check
```

- [ ] **Step 2: Commit**

```bash
git add .github/
git commit -m "Add GitHub Actions CI workflow"
```

---

### Task 9: Update .gitignore and Final Verification

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Update .gitignore**

Add to the existing `.gitignore`:

```
# C build artifacts from vendor compilation
*.o
*.a
*.so
*.dylib
```

- [ ] **Step 2: Full verification**

```bash
cargo build --workspace
cargo test -p starlink-ast
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Expected: all four commands pass with zero warnings and zero errors.

- [ ] **Step 3: Commit**

```bash
git add .gitignore
git commit -m "Update .gitignore for C build artifacts"
```

---

## Verification Checklist

After all tasks are complete, verify:

1. `cargo build --workspace` — all 6 crates compile
2. `cargo test -p starlink-ast` — error code smoke tests pass
3. `cargo clippy --workspace -- -D warnings` — no warnings
4. `cargo fmt --all -- --check` — properly formatted
5. `vendor/README.md` exists with provenance info
6. `.github/workflows/ci.yml` exists
7. `git status` is clean (no untracked files that should be committed)
