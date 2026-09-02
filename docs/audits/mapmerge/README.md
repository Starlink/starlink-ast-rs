# `MapMerge` / `Simplify` audit checklists

One file per class, named for its Rust module: `shift.md` audits
`crates/ast-core/src/mappings/shift.rs`.
Region modules take a `region_` prefix, so `region_interval.md` audits
`crates/ast-core/src/regions/interval.rs`; Frame modules take a `frame_` prefix,
so `frame_frameset.md` audits `crates/ast-core/src/frames/frameset.rs`.

Each row is one of C's own comment-delimited steps in that class's `MapMerge`
or `Simplify` body.
AST's C names its steps in comments, so the granularity is C's, not ours —
typically 5 to 20 rows per class, never line by line.

## Required front matter

```
C source: reference/starlink-ast/src/<file>.c
Rust: crates/ast-core/src/<dir>/<module>.rs
Audited: YYYY-MM-DD against pin <sha>
Citations: verified against pin <sha>
Extension points: classified
```

The last two lines are what the gates in `test_audit_citations.rs` look for, in
the first twelve lines of the file.
`Citations:` says every citation in the file carries the name of the item it
lands in and was checked against that pin; `Extension points:` says every row
classifies its C step.
A file without them is skipped by those gates and counted in `PLAN.md` §6, so
adding a marker to a file whose rows do not hold up makes a test fail rather
than a claim pass.

## The table

| column | meaning |
|---|---|
| `#` | row number, from 1 |
| `C step` | C's own wording for the step |
| `C site` | `file.c:start-end` |
| `Rust site` | `path.rs:start-end`, or `--` when the verdict is `n/a` |
| `extension point` | `architecture` or `ceremony` — see below |
| `verdict` | exactly one of the forms below |

A citation may name the item it lands in, as `sphmap.c:743-747 (fn MapMerge)`.
The name is what makes a stale line number detectable, and
`crates/audit-tools` derives it for both the tool that writes the citations and
the test that checks them.
Anchor a repaired file with `cargo run -p audit-tools --bin anchor-citations --
docs/audits/mapmerge/<file>.md --write`, then read what it wrote: it names the
item at the line, which is not always the item the row means.

## The extension-point column

Each row says whether its C step is `architecture` or `ceremony`, applying the
mechanical test in [`docs/porting-hazards.md`](../../porting-hazards.md) H1: does
a generic driver in another file dispatch to this through the vtab?

- `ceremony` — the step exists only because of C's conventions and asks nothing
  of the port: an error-status check, a pointer annulled or freed, a result
  sentinel initialised or cleared, an axis-major buffer marshalled for a C
  constructor, an accessor the `astMAKE_SET` family generates.
- `architecture` — the step does work the port must also do, and so has to hang
  off the same per-class seam C reaches it through.

`test_audit_citations.rs` checks the second one: an `architecture` row whose
verdict is `ported` must cite a Rust site inside an `impl Trait for Type` block,
or inside code one of the class's trait methods calls.
The second form is what this port does with `MapMerge`: the trait method is a
few lines that call a `<class>_map_merge` body in `mappings/simplify.rs`, and
the rows cite the body.
A row that lands anywhere else is the shape of H1 — a per-class extension point
collapsed into a central `match` — and the row's Rust site should be corrected
to the code the seam actually owns, or the collapse fixed.

## Verdicts

- `ported` — the Rust code performs this step.
- `n/a: <reason>` — the step cannot apply.
  A reason is required and must say why, not that it is unnecessary.
  "The Rust type cannot be inverted in place" is a reason; "not needed" is not.
- `gap: PLAN.md <section>` — not ported.
  The named `PLAN.md` section must exist and must say what would expose the
  gap, not merely what is missing.
- `deferred: <class>` — the step belongs to a class this port has not started,
  counted in `test_deferred_fixtures.rs::DEFERRED_CLASSES`.
- `driver: <path>` — the step is performed by the shared driver at that path
  rather than by this class.
- `unreachable: <reason>` — the step is ported, and no construction available
  through the public API reaches it. The reason must say what makes it
  unreachable.
- `divergence: <reason>` — the step applies, and the port deliberately does not
  perform it, because performing it would reproduce a defect in the C step.
  The reason must cite `docs/issues/c-library-quirks.md`, where the finding and
  the evidence for it live, and the verdict is invalid without that citation.
  This is not a softer `gap`: a gap is work owed, a divergence is work refused.

## Which classes need a checklist

The list is derived from the source tree, not written down here, so a new class
cannot arrive without one.

- Every module under `crates/ast-core/src/mappings/` that is not in the test's
  helper list.
- Every module under `crates/ast-core/src/regions/` that overrides one of this
  audit's Region seams — `map_merge`, `simplify_remapped`, `get_def_unc` or
  `simplified` — plus `mod`, which carries the generic `Region::Simplify` driver.
- Every module under `crates/ast-core/src/frames/` that overrides `simplified`.
  A Frame class may also implement `Mapping`; `FrameSet` does, and C gives it a
  `Simplify` body of its own (`frameset.c:10111`).

`simplified` is in that list because C's per-class `Simplify` vtab slot is an
extension point on the same footing as `MapMerge`: `astSimplify_`
(`mapping.c:24772`) is its only caller, and a class that overrides it owns a
whole simplification algorithm. Such a body belongs in this audit whichever
directory the class lives in.

The Rust tree cannot be the only authority, because a C class whose simplify
seam was never ported has no Rust module and so produces no checklist to be
missing. `C_SIMPLIFY_SEAMS` in the test lists every C file that installs a
`MapMerge` or `Simplify` vtab entry against what covers it — a checklist stem,
the base driver module, a `DEFERRED_CLASSES` row, or a `PLAN.md` gap section —
and when the submodule is present the list is re-derived from the C source and
required to match, so a pin move that adds a seam fails there.

`test_mapmerge_audit.rs` enforces all of this.
A class in the audit universe with no checklist fails, and so does a row whose
verdict is outside the set, and so does a C seam nothing covers.

## Which seam a reduction belongs in

C has one public entry, `astSimplify`, and one private dispatch behind it — the
`Simplify` vtab slot, whose only caller is `mapping.c:24772`. Eleven classes
override that slot. For every other class **all** reduction lives in `MapMerge`,
and the base `Simplify` reaches it by nominating the Mapping in a list of one
(`mapping.c:16871-16883`).

So a row whose Rust site is a `simplified()` body, in a class whose C counterpart
does not override the slot, is not `ported`: it is in a seam C does not have.
`crates/ast-core/tests/test_simplify_seams.rs` holds the list of 11.

The companion rule — that nothing but `simplify()` reaches the seam — is enforced
by the compiler, not by a checklist: `simplified()` takes a `ViaSimplify` and
only `mappings::simplify_entry` can construct one.
