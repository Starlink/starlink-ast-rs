# C Documentation Audit Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix every parity gap and deferred defect the C documentation audit recorded, so that none of its `PLAN.md` §2 rows stays open.

**Architecture:** Each task closes a group of `PLAN.md` §2 rows (those whose status says "found by the C documentation audit") or deferred minors, one subagent at a time.
Each gap is fixed the way C does it, read from `reference/starlink-ast/src/` and confirmed with a C-oracle probe where the source alone does not settle it.
The Rust API is unreleased: where C can raise an error the Rust returns `AstResult`, and signature changes ripple through the call sites in the same commit.

**Tech Stack:** Rust 2024 (`starlink-ast`, `audit-tools`), the reference C library for probes.

**Spec:** `docs/superpowers/specs/2026-10-19-c-docs-audit-design.md` (the audit that found these), and `docs/superpowers/specs/2026-04-20-rust-port-design.md` (API shape).

## Global Constraints

- Port C faithfully: read the C code path, including macros; no heuristics, no tolerances C lacks (CLAUDE.md "Stop signals").
- Every behavior fix: a test that fails first, using the public API, in the area test program that owns the class (`crates/starlink-ast/tests/<area>/main.rs`); then the fix; each gap in its own commit.
- Before claiming C's behavior where the source is ambiguous, run a C-oracle probe (`docs/issues/c-oracle-probe-recipe.md`; library at `reference/starlink-ast/build/libast.dylib`; build in the scratchpad, with `AST_REPLACE_NAN=1` where NaN outputs matter).
- A closed gap's `PLAN.md` §2 row is removed in the commit that closes it; a gap found not to be one gets its row rewritten with the evidence.
- New public items get rustdoc that stands on its own for the Rust, `#[doc(alias)]` with the C name and a ledger row in `docs/audits/c-docs/ledger.tsv` where a C header maps to them; no C line numbers or internal C names in rendered docs; public C names only in `# See also`.
- No `unsafe` raw pointers; no `Arc`, `Mutex` or `Sync` in `starlink-ast`; the SkyFrame setter hot path stays O(1), allocation-free and lock-free.
- One sentence per line in prose and new rustdoc; American spelling.
- Verification per code commit: the failing test, then the area test program, `--test meta`, and `cargo clippy --workspace --all-targets -- -D warnings` with and without `--features ast-bench/c-oracle`. Docs-only commits: the doc build with `RUSTDOCFLAGS="-D warnings"` and `cargo test -p starlink-ast --test meta test_c_docs_audit::`. One full `cargo test -p starlink-ast` at the end of the plan.
- Commit through the clean-env wrapper so hooks run; re-add files the re-anchor hook rewrites.

## Review Focus

1. A signature change from a plain value to `AstResult` (`map_length`, `internal_unit`, `norm_unit`, the FitsChan getters): every caller must handle the error rather than `unwrap` it where C would carry on.
2. An inverted FrameSet given to any changed FrameSet method (`find_frame`, `convert_to`, `FrameSet::new`, the index getters): the effective Base and Current must follow C.
3. A Native dump written by C and read by the port after a change to dumping (PermMap `InCpy`/`OutCpy`, FrameSet Invert): it must round trip byte-exact.
4. A NativeChannel or MocChan whose SourceFile is set, then cleared, then set again: the file opened for the last value must be closed and the source function restored.
5. A MathMap expression mixing the bitwise operators with negative or non-integer values: results must match C's fixed-point rule, not the IEEE bit pattern.

---

### Task 1: KeyMap

- `KeyMap::map_length` returns `AstResult<usize>` and reports `ErrorCode::Bigkey` as C's `astMapLength` does (PLAN §2 row "`KeyMap::map_length` gives 0 for a key it cannot use"); update its call sites.
- The `get_elem_*` out-of-range error message names the method as C's literal message does (`astMapGetElem<X>`); confirm C's exact text first.

### Task 2: MathMap

- `1.or.0` reports `ErrorCode::Miopr` (PLAN §2 row "MathMap reports a number followed by a dotted operator as an undefined variable").
- Port C's `BIT_OPER` for `&`, `|` and `^` (PLAN §2 row "MathMap bitwise operators reinterpret the IEEE bits").
- Port C's `ExtractVariables` name validation: AST__MISVN, AST__VARIN, AST__DUVAR (PLAN §2 row "`MathMap::new` does not validate variable names").

### Task 3: Mapping behavior

- Report: `transform` and `transform_into` report as C's `astTranN`/`astTranP` do, and a clone clears Report (PLAN §2 row "`Report` differs from C's in two ways").
- `simplify` clears AST__NODEF / AST__TRNND raised during a reduction and returns the Mapping unsimplified (PLAN §2 row "`simplify` returns AST__NODEF / AST__TRNND errors").
- `ResampleFlags` carries C's AST__URESAMP1–4 user flags to a caller's interpolator or kernel (PLAN §2 row "`ResampleFlags` has no user flags").

### Task 4: Mapping API

- `PermMap` null-permutation shorthand, dumped as `InCpy` / `OutCpy` (PLAN §2 row "`PermMap::new` has no null-permutation shorthand").
- `ChebyMap::coeffs`, as C's `astPolyCoeffs` on a ChebyMap (PLAN §2 row "`ChebyMap` has no `coeffs`").
- A public counterpart of `astDecompose` for CmpMap and TranMap, with their components and Invert values (PLAN §2 row "`astDecompose` has no counterpart").

### Task 5: Frame

- `norm_points` reports `ErrorCode::Dimin` for a wrong coordinate count (PLAN §2 row "`norm_points` reports Ncpin ...").
- `internal_unit` and `norm_unit` return `AstResult` and report `ErrorCode::Axiin` (PLAN §2 row "`internal_unit` and `norm_unit` accept an axis out of range"); update every override and caller.
- A public counterpart of `astSkyOffsetMap` (PLAN §2 row "`astSkyOffsetMap` has no counterpart").
- `SkySystem::parse` ignores trailing white space, as C's `astChrMatch` does.

### Task 6: FrameSet search and indices

- `find_frame` sets the Current of a FrameSet target to the Frame matched, and `convert_to` sets the Base of a FrameSet argument to the Frame used (PLAN §2 row "`find_frame` and `convert_to` leave a FrameSet's Current and Base unchanged"); update `docs/issues/frame-mapping-c-port-gaps.md`.
- `convert_to` searches an inverted FrameSet's intermediate Frames in C's order (PLAN §2 row "`convert_to` searches an inverted FrameSet's other Frames in ascending order").
- Integer Base and Current getters, and setting either by Domain name (PLAN §2 row "FrameSet has no getter for the Base and Current indices").

### Task 7: FrameSet construction and variants

- `FrameSet::new` given a FrameSet takes its Frames, Mappings, Base, Current and Invert, and the Region builders take a FrameSet's current Frame as C's `astInitRegion` does (PLAN §2 row "`FrameSet::new` holds a FrameSet as one Frame").
- `set_variant`: a test of the root-node fallback, and the per-link Invert of the simplified Mapping recorded as C does (probe first).

### Task 8: FitsChan

- Encoding, CDMatrix and DefB1950 read a source not yet read (PLAN §2 row "A FitsChan's Encoding, CDMatrix and DefB1950 defaults ...").
- `FITSCHAN_OPTIONS` applied to every new FitsChan (PLAN §2 row "`FITSCHAN_OPTIONS` is not read").
- With Strict set, a warning is an AST__BADIN error at the point C raises it (PLAN §2 row "A FitsChan warning is not an error when Strict is set").
- `CardList::put` overwrite semantics match `astPutFits`.

### Task 9: Channels

- SourceFile and SinkFile on NativeChannel and MocChan (PLAN §2 row "A NativeChannel and a MocChan have no SourceFile or SinkFile attribute").
- Standard input and output when a Channel has no source or sink (PLAN §2 row "A Channel with no source or sink does not use standard input or output").

### Task 10: Region API shape

- `Moc::get_moc_text` without its C-shaped `method: &str` parameter, and `Stc::stc_set_obs` (protected in C) made crate-private or given its public C-equivalent shape.

### Task 11: Audit tooling

- Every deferred audit-tools minor: header with no `Name:` or unterminated at end of file reported, not dropped; `href` targets scanned, with URL tests; a second alias in one attribute reported; `mentions_alias` skips strings and comments; the literal stripper handles block comments and `cr#""`; nested ledger item paths; duplicate aliases kept distinct; ledger header columns checked and the "expected 7" message corrected; `c-docs` survives a closed pipe; stale pages in the gate's target directory cleared; one-line `#[cfg] pub mod` found; the repeated reason string; `show` column alignment; `pub mod c_headers` placement; `check_c_headers_fresh.sh` reads the pinned commit, not the checkout; stale `type.impl/*.js` text.

### Task 12: Docs and tests

- `docs/issues/c-oracle-probe-recipe.md` says that on macOS a probe linked against `reference/starlink-ast/build` needs `-last_err -last_grf3d -last_grf_5.6`, or it crashes on the first error.
- `MatrixMap::diagonal` doctest checks the inverse zero-fill; `CmpMap::new`'s second `# Errors` bullet names its error; `// C:` comments placed after the `///` blocks in `xphmap.rs`; British spellings in `//` comments; C line numbers in `//` comments refreshed against the pinned C source (with a tool if the re-anchor tool can do it).

### Task 13: Errors the merge code drops

- Every place in the simplifier and the per-class merge code that drops an error C would pass up (listed in the Task 3 review): the WcsMap and WinMap matrix transforms, nested simplify `.ok()` calls, `if let Ok` on a component simplify in `cmp_map_merge`, and constructor failures; each confirmed against C with a probe, with the simplify corpus kept byte-exact.
