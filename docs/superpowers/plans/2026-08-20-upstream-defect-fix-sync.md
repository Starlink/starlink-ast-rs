# Upstream sync: the C defect-fix branch (`50c2b21b` → `f6762005`)

Plan for the sync PLAN.md §8 warned about.
The pin has moved; this plan turns that move into a port that conforms to the
fixed C library instead of to the bugs it used to have.

## What arrived

41 commits, 25 `src/` files, +872/−235, plus 103 changed files under
`ast_tester/`.
Nineteen defect fixes.
Six of them change serialized output, one changes read-side card consumption,
and the rest are behavioural fixes whose Rust counterpart is a conformance test
rather than a code change.

Every fix in the range corresponds to an entry in
[`docs/issues/c-library-quirks.md`](../../issues/c-library-quirks.md).
The sync is therefore driven by that document: for each entry, either the port
**mirrors** the bug and the mirror must go, or the port already did the right
thing and needs a test that pins the agreement.

## The four mirrors to retire

| Quirk entry | Rust mirror | Upstream fix |
|---|---|---|
| `EncodeFloat` strips the exponent's leading zero by shifting right | `strip_exponent_leading_zero`, `fitschan/format.rs` | `68bcb7db` |
| Duplicate-keyword consumption inconsistent between `WcsFcRead` and `SpecTrans` | `CardList::mark_card` first-match on the `GetValue` path | `6301311f` |
| `CDjjjiii` cards consumed under encodings that discard their values | `spec_trans::translate_pc_family` | `fd92f9d2` |
| `astMapCopyEntry` normalises the key with the source KeyMap's `KeyCase` | `keymap` copy path | `d8edc072` |

A faithful mirror of a fixed bug is a bug.

## The two licences to retire

Both are in the wcsconv gate and both exist because of C bugs now fixed:

- `ALIASING_TAG_LICENSED` (5 cases) drops every `ISSIMP<xx>` / `INVERT<xx>`
  card from both sides.
- `ignore_is_simp_line` in `test_simplify_regression.rs` drops the `IsSimp`
  line from all 132 negative fixtures.

Neither can go until the port reproduces C's new, deterministic tag placement.

## Work order

Five commits.
The `.native` fixtures each carry three independent upstream changes, so the
three fitschan un-mirrors and the fixture copy have to land together or the
gate is ambiguous.

### 1. fitschan un-mirrors + `.native` corpus

- Drop the length-preserving shift in `strip_exponent_leading_zero`: post-fix C
  removes the redundant exponent zero and right-justifies, with no compensating
  space, so the Rust rule is "strip the zero, then right-justify".
- Port `MarkAllCards`: `GetValue2`'s second-FitsChan path marks *every* card for
  the keyword, not the first. Value still comes from the first match.
- Respect the MJD-OBS exception: `translate_date_obs` tests for MJD-OBS without
  marking it.
- Move the `CDjjjiii` scan inside the `Encoding::FitsIraf` guard; invert the
  `cdjjjiii_consumed_without_translation_for_non_iraf_encoding` unit test.
- Copy the 69 changed `.native` files and `gappt.fits-wcs` into
  `crates/ast-core/tests/fixtures/wcsconv/expected/`.

The `WcsNative` PermMap-invert fix (`f54ff1b7`) needs no code: the port's shape
was already the corrected one.

### 2. `IsSimple` per orientation + `.simp` corpus + both licences

The load-bearing change. C now keeps two records — `AST__ISSIMPLE_FLAG` and
`AST__ISSIMPLEINV_FLAG` — and selects between them on the current `Invert`
value; `astSetInvert` no longer clears anything.
Placement of the `IsSimp` card becomes a property of the Mapping tree alone.

- Add a shared per-orientation record to `mappings/mod.rs` and replace the
  `has_been_simplified: bool` field in all 27 classes.
- `invert()` stops clearing; it just flips the orientation, which changes which
  record `has_been_simplified()` reads.
- Delete `CmpMap::is_simple_serial` and `clear_child_simple_tag` — the
  `C-ARTIFACT` dual-flag emulation exists only to reproduce the aliasing
  artifact, and there is no longer an artifact to reproduce.
- Port the discard-on-mutation rule (`astClearIsSimple` in the class setters):
  any attribute change that feeds the class's own simplification drops both
  records. `ZoomMap::Zoom`, `PcdMap::Disco`, `PolyMap::IterInverse` and
  `astPolyTran`'s result are the cases the new C tests pin.
- Copy the 27 changed `.simp` fixtures.
- Retire `ALIASING_TAG_LICENSED` and `is_aliasing_tag_card`.
- Retire `ignore_is_simp_line` and `strip_is_simp`.
- Re-check the three `skip_string_compare` entries that were IsSimp-driven
  (`cap_cmpmap_01`, `cap_cmpmap_04`, `cap_specmap_02`).

### 3. Port the new C tests

| C test | Rust home |
|---|---|
| `testmapping.c`: `testequaldumpstable`, `testsimplevsinvert`, `testsimplevsinvertdump`, `testmutationclearssimple`, `testselfsimplifyafterset` | new `crates/ast-core/tests/test_mapping.rs` |
| `testfitschan.c`: `testgrismorder` | `crates/ast-core/tests/test_grismmap.rs` |
| `testkeymap.c`: nine new functions | extend `crates/ast-core/tests/test_keymap.rs` |
| `testimmutable.c` (new file, 239 lines) | **not portable** — `AST__IMMUT` guards a reference-counted sharing condition that owned values cannot construct. Belongs to `ast-capi`. |

### 4. KeyMap conformance

Un-mirror the `astMapCopyEntry` KeyCase behaviour, then verify the rest of the
KeyMap fixes are already satisfied and pin each with a test: key-order dump
stability, `KyCas` unset sentinel, `MapIterate` under `SortBy`, `ConvertValue`
range and overflow, NaN conversion, negative-value rounding, zero-length vector
rejection, `astMapGetElem<X>` on an undefined entry, `astMapRename` refusal,
`MpLck` ordering, and the absent `Mem%d` card.

### 5. Documentation

- `docs/issues/c-library-quirks.md`: move every entry fixed in this range to
  `RESOLVED (upstream <sha>)` with the Rust-side outcome recorded.
- `PLAN.md` §8: replace the pending-sync section with the completed record.
- `PLAN.md` §6 ledger: drop the retired relaxation rows; the numbers are the
  claim.
- `PLAN.md` test-migration tracker: rows for `testmapping.c` and
  `testimmutable.c`.
- `docs/issues/2026-08-09-issimple-tag-placement.md`: record that the proposal
  it made was accepted upstream and is now ported.

## Out of scope

- `CmpMap` `InvA`/`InvB` (quirk entry at `:203`). Not fixed in this range. The
  local commits `0a46009`…`62f949d` record why it is blocked on `IsSimp`;
  re-assess after commit 2 lands, but do not widen this sync to cover it.
- `ast-capi` work implied by `AST__IMMUT`. The crate is unstarted.
- `grf_log` SvgX/SvgY rounding and `serpens.svg` (`72f09a2e`). No Rust plotting
  backend consumes it yet.
