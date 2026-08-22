# `MapMerge` / `Simplify` audit checklists

One file per class, named for its Rust module: `shift.md` audits
`crates/ast-core/src/mappings/shift.rs`.
Region modules take a `region_` prefix, so `region_interval.md` audits
`crates/ast-core/src/regions/interval.rs`.

Each row is one of C's own comment-delimited steps in that class's `MapMerge`
or `Simplify` body.
AST's C names its steps in comments, so the granularity is C's, not ours —
typically 5 to 20 rows per class, never line by line.

## Required front matter

```
C source: reference/starlink-ast/src/<file>.c
Rust: crates/ast-core/src/<dir>/<module>.rs
Audited: YYYY-MM-DD against pin <sha>
```

## The table

| column | meaning |
|---|---|
| `#` | row number, from 1 |
| `C step` | C's own wording for the step |
| `C site` | `file.c:start-end` |
| `Rust site` | `path.rs:start-end`, or `--` when the verdict is `n/a` |
| `verdict` | exactly one of the three below |

## Verdicts

- `ported` — the Rust code performs this step.
- `n/a: <reason>` — the step cannot apply.
  A reason is required and must say why, not that it is unnecessary.
  "The Rust type cannot be inverted in place" is a reason; "not needed" is not.
- `gap: PLAN.md <section>` — not ported.
  The named `PLAN.md` section must exist and must say what would expose the
  gap, not merely what is missing.

## Which classes need a checklist

The list is derived from the source tree, not written down here, so a new class
cannot arrive without one.

- Every module under `crates/ast-core/src/mappings/` that is not in the test's
  helper list.
- Every module under `crates/ast-core/src/regions/` that overrides one of this
  audit's Region seams — `map_merge`, `simplify_remapped` or `get_def_unc` —
  plus `mod`, which carries the generic `Region::Simplify` driver.

`test_mapmerge_audit.rs` enforces all of this.
A class in the audit universe with no checklist fails, and so does a row whose
verdict is outside the set.
