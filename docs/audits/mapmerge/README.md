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
Both are mandatory: `every_checklist_declares_both_markers` fails on a file that
lacks either, so a checklist arrives gated rather than opting in later.
Adding a marker to a file whose rows do not hold up makes a test fail rather than
a claim pass.

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
`anchor-citations` only appends a missing `(fn …)`; it never moves a number.

Moving a number is the other tool's job, and only at a pin move.
`cargo run -p audit-tools --bin reanchor-citations -- docs/audits/mapmerge/<file>.md`
diffs each cited line's own baseline against the source now — `git blame` names
the commit that wrote the row, and that commit's tree, or the submodule pin it
recorded, is what the citation was written against — and follows a line the diff
carried through unchanged onto where it landed.
A line the diff replaced or deleted it refuses and reports, because the content
the row describes is no longer there: re-anchor that row against the code it now
describes rather than remapping its number, and check whether the prose around it
described a C behaviour the pin move changed.
Run it once against the tree the numbers belong to; against an already-relocated
tree its baselines are the wrong side of the move.
Re-state the pin in `REPAIRED` in `crates/ast-core/tests/test_audit_citations.rs`
and in every `Citations:` marker in the same change as the re-anchors.

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
verdict is `ported` must cite a Rust site inside an `impl Trait for Type` block or
a `trait Trait` definition, or inside code one of the class's trait methods calls.
A default method on the trait counts, which is how the base modules declare their
seams — `regions/mod.rs` puts the shared `Region` bodies in `pub trait Region`
rather than in an implementation of it, and a generic driver dispatches to one of
those per class exactly as it does to an implemented override.
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
- `divergence: <reason>` — the step applies, and the port deliberately does not
  perform it, because performing it would reproduce a defect in the C step.
  The reason must cite `docs/issues/c-library-quirks.md`, where the finding and
  the evidence for it live, and the verdict is invalid without that citation.
  A divergence is work refused, not work owed.

Those three are the whole grammar, and `every_checklist_row_has_a_verdict`
rejects anything else.
Four further forms were withdrawn once no row used them: `gap: PLAN.md
<section>`, because every gap is closed and readmitting one should take a
deliberate change to the test rather than a new table cell;
`unreachable: <reason>`, which no row ever carried; and `deferred: <class>` and
`driver: <path>`, which describe how a whole C seam is covered rather than what
a step's port does, and are the vocabulary of `C_SIMPLIFY_SEAMS` in
`test_mapmerge_audit.rs`.
A seam whose port waits on an open design question is recorded there as
`undecided: <issue path>`, which requires the document to state the question and
name the fixture that demonstrates it.

## `Uncited C steps:`

The rows are not trusted to be complete: `c_step_comments` enumerates the steps
of the C body from the source, and `test_audit_citations.rs` requires each one to
fall inside some row's `C site`.
The bodies it enumerates are the ones the table rows cite in the file the
`C source:` header names, so prose elsewhere in the checklist can cite another
class's code as evidence without taking on its steps.

AST's own prologue block is skipped — it documents the function rather than
naming a step in it.
Anything else the checklist does not enumerate is named, with a reason, under an
optional heading in the header block:

```
Uncited C steps:
- polygon.c:5335 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
```

A step that is a step belongs in a row, with its own verdict, even when the
verdict is `n/a`.
The heading is for comment blocks that name no step at all.

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
