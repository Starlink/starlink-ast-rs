# Upstream fixture-hierarchy sync — design

Status: approved 2026-09-03.
Upstream range: `3e60abaa` → `b8fcd43c` (pinned), then a new branch `u/timj/more-fixtures-2` on top.

## 1. Problem

Upstream AST has reorganized `ast_tester` so that all committed test data lives
below `ast_tester/fixtures/`, and every path a manifest or a transform-oracle
section header names is relative to that directory.
Our fixture tree grew independently and diverges from it in layout, in manifest
format, and in which fixtures exist at all.

The consequences are concrete rather than cosmetic.

- The three `.oracle` corpora now key their sections on full fixture-root-relative
  paths (`simplify/brad.map`, `programs/joye_car_headers/car1.head`), while
  `test_transform_oracle.rs::resolve_fixture` keys on basenames and searches five
  candidate directories with a basename fallback.
  Upstream now has genuinely duplicated basenames — `car1.head` through
  `car5.head` exist in both `wcsconv/inputs/` and `programs/joye_car_headers/` —
  so that fallback resolves the wrong file rather than failing.
- 46 simplify fixtures and one wcsconv case exist only here, so upstream's oracle
  does not cover them.
  In the other direction only three files are genuinely absent here —
  `cheby_linear_lone_reduce.{map,simp}` and `neg_cheby_quadratic_no_reduce.map`,
  two ChebyMap MapMerge fixtures added upstream at `86704acf`.
  Every other upstream simplify fixture is present here, relocated: the scenario
  fixtures under `native/reference/`, the unloadable ones under
  `simplify_fixtures_unsupported/`.
  `headers.oracle` additionally gains four headers upstream.
- Upstream's `simplify_fixtures.oracle` has no section for 60 of its own 781
  fixtures. Ours gates 38 of those.
- The manifests carry the same column but not the same meaning: upstream flags
  165 of 467 simplify rows and 108 of 192 wcsconv rows `skip_string_compare`,
  where our harness flags 0 and 8.
  Reading upstream's flags directly would relax roughly 265 comparisons.

The goal is one hierarchy, byte-identical in both repos, verified by a single
recursive diff, with the comparison policy meaning the same thing on both sides.

## 2. Target hierarchy

`crates/ast-core/tests/fixtures/` becomes a verbatim copy of
`reference/starlink-ast/ast_tester/fixtures/`.
Nothing Rust-local survives outside it.

```
ast_tester/fixtures/                    ==  crates/ast-core/tests/fixtures/
  simplify/          .map / .simp + cases.txt
  wcsconv/
    inputs/  expected/  cases.txt
    framesets/         C's dump of the FrameSet read from each input      NEW
  serialisation/       minimal per-class dumps + shape probes             NEW
  oracle/            *.oracle + transform_oracle_overrides.txt
                     + keymap_mapsz.txt                                   NEW
  plot/expected/     SVG / PostScript references
  programs/<prog>/   data used by individual C and Fortran programs
```

`plot/expected/` is copied although no Rust test reads it until the Plot port.
It costs 3.7 MB once and buys a mirror gate with no exclusion list, which is
worth more than the bytes: an exclusion list is one more thing that has to stay
honest.

### 2.1 What each new directory holds, and why it is a directory

`wcsconv/framesets/` — 139 `.ast` files, C's dump of the FrameSet obtained by
reading each `wcsconv/inputs/` header.
Produced by `head2ast`.
It is the corpus that proves the Native serializer round-trips FITS-derived
FrameSets, which no other corpus covers.

`serialisation/` — C dumps of programmatically built objects: 24 minimal
per-class dumps (ZoomMap, MatrixMap ×2, PermMap ×2, ShiftMap, WinMap, UnitMap,
CmpMap ×3, Frame, SkyFrame ×2, SpecFrame, TimeFrame, CmpFrame ×2, FrameSet ×6),
plus `c_dssmap.ast`, `c_variants.ast`, `jan-dsb.ast`, `frame_in_map_slot.ast`
and `region_in_cmpframe.ast`.
These pin the serializer for a *bare* object of each class — attribute set,
defaults, comment text, `IsA` boundaries — which the simplify corpus cannot,
because it holds only simplified composites, and `framesets/` cannot, because it
holds only FITS-derived FrameSets.
`specflux.ast` is byte-identical to `wcsconv/inputs/specflux.ast` and drops out.
`degen1.ast`, `lsst.ast`, `timj.ast` and `splittest1.ast` differ from their
same-named copies elsewhere in the tree — they are re-dumps through a Channel
with specific attributes — so they stay, and their provenance is confirmed
during U2 before either copy is deleted.

`oracle/keymap_mapsz.txt` — the C capture that `test_keymap_serial.rs` compares
`MapSz` card emission against, currently at `fixtures/keymap/mapsz_oracle.txt`.

### 2.2 What does not become a directory

Two Rust-only expectations stay as named lists in Rust source rather than as
directory boundaries in a shared tree.
A directory would encode a Rust-only fact as a property of both repos.

- The 24 fixtures the port cannot load (13 IntraMap, 9 XphMap,
  2 StcResourceProfile) stay in `simplify/` and are named in a `SIMPLIFY_DEFERRED`
  list, one reason string each, with a companion test that fails when a deferred
  fixture starts loading.
  This replaces `simplify_fixtures_unsupported/`.
- `frame_in_map_slot.ast` sits in `serialisation/` and is named in the existing
  `FRAME_IN_MAP_SLOT_UNDECIDED` list (`test_frame_in_map_slot.rs:29`), already
  counted in the verification ledger.
  C reads that file without complaint — C wrote it, and accepts a Frame in any
  Mapping slot because `Frame::Transform` (`frame.c:11739`) transforms through
  `astUnitMap( astGetNaxes( this ) )`.
  The refusal is this port's, and whether it is right is the open question in
  `docs/issues/frame-in-a-mapping-slot.md`.
  `region_in_cmpframe.ast` is not a refusal case at all: it is the control
  proving the refusal is not widened to catch the settled Region nestings.

### 2.3 The `.rt` override mechanism is deleted

43 files named `<fixture>.rt` currently hold the expected output of loading and
re-dumping a simplify fixture, for the fixtures where that differs from the
fixture itself.
All 43 differ in exactly one way: `IsSimp = 1` lines are missing from the
readback, 50 lines in total, never an addition and never any other card.

The cause is a defect in the C library.
`astLoadMapping` reads the `IsSimp` card and sets the flag into the correct
orientation (`mapping.c:24137`).
The subclass loader then validates the attribute it just read by re-applying it
through the private setter — `astLoadZoomMap` at `zoommap.c:2238`:

```c
new->zoom = astReadDouble( channel, "zoom", 0.0 );
if ( TestZoom( new, status ) ) SetZoom( new, new->zoom, status );
```

and that setter clears the flag (`zoommap.c:1704`):

```c
astMAKE_SET1(ZoomMap,Zoom,double,zoom,(astClearIsSimple(this), ...
```

`astClearIsSimple` is correct for a real attribute change, since changing `Zoom`
can invalidate a simplification.
It is wrong when the "change" is the loader echoing back the value it just read.
Net effect: C writes `IsSimp = 1` and cannot read it back.

The class distribution across all 50 lost cards is exactly the set of classes
whose setter clears the flag, and nothing else:

| class | cards lost | setter |
|---|---|---|
| ZoomMap | 44 | `zoommap.c:1704` |
| PcdMap | 2 | `pcdmap.c:2586` |
| MathMap | 2 | `mathmap.c:5705` |
| GrismMap | 2 | `grismmap.c:255` |

The fix is upstream (U1), and the 43 files are deleted rather than moved.
The round-trip test then has no override path at all.
This is the "faithful mirror becomes a bug once upstream fixes it" case that the
sync protocol in CLAUDE.md warns about: the port currently mirrors the defect,
with an entry in `docs/issues/c-library-quirks.md` that is retired in R4.

## 3. Manifests

Both repos read `simplify/cases.txt` and `wcsconv/cases.txt` from the shared
tree. `simplify_tests.txt` and `wcsconv_tests.txt` are deleted.

Two Rust-only columns disappear rather than being added upstream.

- `render_options` has exactly one user: the `lsst` row, whose reference was
  captured with `Comment=0,Indent=0`.
  Regenerating `lsst.simp` with default Channel attributes takes it from 211
  lines / 3.5 KB to roughly the 15 KB of `rigby.simp` and removes the column.
- `compare_mode` becomes a value in upstream's existing `skip_string_compare`
  column: `astequal` alongside `yes`, distinguishing "relax numeric tokens to
  AST's astEQUAL tolerance" from "skip the string comparison".

The flags then have to mean the same thing on both sides, which is U4.
Until the C harness compares to the same standard, upstream's 273 opt-outs
cannot be read here without silently relaxing roughly 265 comparisons.

## 4. Upstream work — branch `u/timj/more-fixtures-2`

**U1. The `IsSimp` loader fix.**
Preserve the flag across the validating `Set` in the four affected loaders:

```c
int simp = astIsSimple( new );
new->zoom = astReadDouble( channel, "zoom", 0.0 );
if ( TestZoom( new, status ) ) SetZoom( new, new->zoom, status );
if ( simp ) astSetIsSimple( new );
```

Then regenerate the simplify corpus and read the diff.
Nested `IsSimp` cards now survive load, and `astSimplify` skips nodes already
marked simplified, so a re-read tree may simplify differently than before.
No `.map` fixture's top-level object carries an `IsSimp` card, so the entry
points to the `.map` → `.simp` comparison are unaffected; nested cards are the
exposure.
Any `.simp` that changes is a finding to explain before it is blessed.

**U2. Generators become cmake targets** so every reference in the tree is
reproducible from the source tree:

| generator | status now | action |
|---|---|---|
| `gen_transform_oracle` | upstream | reuse |
| `gen_simplify_fixtures` | upstream | reuse |
| `head2ast.c` | ours only | move upstream → `wcsconv/framesets/` |
| `gen_native_fixtures.c` | source lost | rewrite, commit → `serialisation/` |
| keymap `mapsz` probe | ours only | move upstream → `oracle/keymap_mapsz.txt` |
| `gen_roundtrip_refs.sh` + `ast2ast.c` | ours only | delete — U1 removes the reason |

`gen_native_fixtures.c` was added to the submodule's working tree by `1f6b322`
and never committed upstream, so the source is gone from both repos.
Only the compiled binary survives, in the untracked
`reference/starlink-ast/build/ast_tester/gen_native_fixtures`.
It is rewritten from the 24 fixtures it produced, which fully determine its
output.

**U3. Our fixtures land upstream — with the generators that build them.**

46 simplify fixtures across 25 case stems (4 stems negative, `.map` only) are
Rust-originated, and none of them is reproducible: `gen_simplify_fixtures.c`
knows nothing about `cefit_*`, `defunc_*`, `regbasepick_*`, `setunc_*`,
`simpstab_*`, `cap_cmpmap_nested_tag` or the four `neg_*` stems, and no
committed program here builds them either.
`a96bc80` and its siblings commit the bytes with no `.c` alongside, and
`docs/audits/region-phase5-parity-spec.md` records each fixture's intent in
prose but not its constructor calls or numeric parameters.

Copying the bytes alone would leave 25 fixtures that a corpus regeneration
either orphans or deletes, and that no future C behaviour change could
re-baseline.
So U3 adds a constructor function per stem to `gen_simplify_fixtures.c`, each
calling the existing `write_fixture` or `write_negative_fixture` helper so both
halves of a pair come out of one pass.
Only the `.map` half needs reconstructing: a `.map` is the complete native
serialization of the constructed object, so every attribute is already in the
committed bytes, and the `.simp` derives from it exactly as `write_fixture`
derives it.
The gate is byte-identity — regenerating the corpus must reproduce all 46
committed files with an empty `git diff`.

U3 also lands `lsst.simp` regenerated with default Channel attributes, its input
already present as `wcsconv/inputs/lsst.ast`; `offset-sref.native`;
`pc-sparse-naxis.head`, which is our `pc-sparse.head` — it differs from
upstream's only by a leading `NAXIS = 2` card, so it goes upstream under a
distinct name rather than diverging silently; the `serialisation/` and
`wcsconv/framesets/` trees; and the matching `cases.txt` rows.

**U4. The C harness gets ULP-and-`IsSimp`-aware comparison,** and every
`skip_string_compare` flag is re-audited against it.
This is the largest piece and the one that makes the shared manifest
trustworthy.
Each flag that survives the audit carries a reason.

If U1's blast radius turns out to be large, or the 273-flag audit balloons, U4
may be split out behind U1–U3 and U5, with our stricter policy staying
Rust-side in the interim and U4 retiring it afterwards.
That is a contingency with a stated trigger, not the plan.

**U5. Regenerate the three oracles** from the finished tree, and account for
every fixture that ends with no section.
An unexplained absence is coverage lost silently, which is the failure mode the
CLAUDE.md comparator rules exist to prevent.

## 5. Rust-side work

**R1. Move the pin** to the tip of the upstream branch; run
`tools/oracle_fixtures/check_corpus_fresh.sh` and `check_simplify_fresh.sh`;
classify card-only versus numeric drift per the CLAUDE.md sync protocol.

**R2. Replace the fixture tree wholesale** from upstream, and add the mirror
gate (§6).
Three fixtures arrive that this repo has never seen —
`cheby_linear_lone_reduce.{map,simp}` and `neg_cheby_quadratic_no_reduce.map`.
ChebyMap is ported here and the corpus already holds `cap_cheby_consolidate`, so
these should pass both the simplify harness and the oracle.
If they do not, that is a real MapMerge gap in `ChebyMap` and belongs in
`docs/audits/mapmerge/chebymap.md`, not in an exclusion list.

**R3. Repoint every consumer.**

- `resolve_fixture` (`test_transform_oracle.rs:266`) becomes `ROOT.join(relpath)`.
  The five-candidate search and the basename fallback go.
  This is the change that makes upstream's duplicated basenames safe.
  `resolves_every_oracle_relpath` stays and gets stricter.
- Manifests read from `fixtures/{simplify,wcsconv}/cases.txt`.
  `test_verification_ledger.rs:174` reads both by path and asserts derived
  counts; those numbers move.
- Every relpath-keyed list and floor is rekeyed.
  All three exclusion lists — `SLAMAP_ORACLE_EXCLUDED`, `HEAD_ORACLE_EXCLUDED`,
  `SIMPLIFY_ORACLE_EXCLUDED` — are currently empty, so only the key *format* and
  the floors change: the floors 136 / 259 / 1339, `is_slamap_section`, `is_head_section`,
  `is_simplify_section`, the `framesets.oracle has 20 sections` assertion, and
  the FMA substitution table keyed on
  `simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map`.
  `is_deferred_class_fixture` currently detects a
  `simplify_fixtures_unsupported` path component and is replaced by
  `SIMPLIFY_DEFERRED`.
- `native_round_trip_fixture` loses its `.rt` lookup.
  Its three sweeps repoint at `simplify/`, `wcsconv/framesets/` and
  `serialisation/`, and the `serialisation/` sweep consults
  `FRAME_IN_MAP_SLOT_UNDECIDED`.
- Roughly 31 files carry fixture path literals in `include_str!`s and doc
  comments, plus `build.rs`, three examples and four `src/` doc references.
- `tools/oracle_fixtures/` loses `gen_roundtrip_refs.sh`, `ast2ast.c` and
  `head2ast.c`; `check_corpus_fresh.*` and `check_simplify_fresh.*` are
  repointed and switched to the shared `cases.txt`.

**R4. Three behavioural ports.**

- *The one-ULP WCS boundary clamp* (`460efce3`, `src/wcsmap.c`).
  A WinMap used by a FITS CAR mapping evaluates an expression such as
  `-0.5 deg + 181 * 0.5 deg` as exactly 90 degrees when the compiler contracts
  the multiply and add into an FMA, and as one representable double above 90
  when it does not.
  Upstream clamps rather than letting the validity of a WCS depend on the
  compiler, which turns the corner pixel of `car6.head` and `CHIPASS_Equ.head`
  from `AST__BAD` into a real position.
  Rust does not contract: rustc has no `-ffp-contract=fast` equivalent enabled
  by default, and fusion comes only from an explicit `f64::mul_add`.
  Measured on this host, `a + n*b` overshoots pi/2 where `n.mul_add(b, a)` lands
  exactly on it.
  So Rust behaves like the non-FMA C build, `HEAD_ORACLE_EXCLUDED` is empty with
  the floor at all 259 sections, and the port currently matches the old `BAD`.
  The clamp is ported faithfully, because it is what the C source does and what
  the regenerated oracle was captured from.
  Using `mul_add` at that site instead would also make the two fixtures pass but
  would diverge structurally from `wcsmap.c` and leave the clamp's other
  exposures unguarded — hazard H2.
- *`CmpMap::Equal`* — the guard for when `astMapList` declines to decompose a
  protected CmpMap.
  `astMapList` refuses to expand a CmpMap for which `astDoNotSimplify` is true
  (`cmpmap.c:1154`) and returns a list holding a clone of the CmpMap itself, so
  an `Equal` that decomposes both operands and compares element by element calls
  `astEqual` on the same pair again and recurses until the stack is exhausted.
  Any CmpMap carrying an `Ident` reaches this.
  `MapSplit` already guards the identical hazard at `cmpmap.c:2555`.
  Both C fixes (`16ed5d37`, `0f797ae7`) are already in the pinned tree; the work
  here is the Rust port plus the two tests added to `testmapping.c`,
  `testprotectedcmpequal` and `testprotectedcmpequalstable`.
- *Un-mirror the `IsSimp` loader loss* and retire its `c-library-quirks.md`
  entry.

**R5.** Green all four tiers; update `PLAN.md` §8 and the C → Rust migration
tracker; fix the docs that name old paths.

R2 through R4 are one commit range, with the gates green only at the end: the
regenerated oracle expects both the clamp and the `IsSimp` fix, so
`car6.head`, `CHIPASS_Equ.head` and the 43 former `.rt` fixtures fail in
between.
Naming those three expected intermediate failures up front is what keeps the
range honest.

## 6. The mirror gate

`tools/fixtures/check_mirror.sh` does a recursive diff of
`crates/ast-core/tests/fixtures/` against
`reference/starlink-ast/ast_tester/fixtures/` at the current pin and requires
empty output.
It catches both directions — an upstream fixture we failed to copy, and a local
fixture that never got upstreamed — which is the whole invariant in one command.

It needs the submodule, so it runs in the nightly job that already initializes
it, alongside tiers 3 and 4, and hard-fails there.
That is a deliberate exception to this repo's rule against skip modes, and the
reason is that this gate measures drift from upstream rather than test coverage:
tier-1 coverage does not depend on it, because the fixtures are committed here
and every harness reads the committed copies.
Skipping it when the submodule is absent loses nothing a tier-1 test was
proving.

A committed checksum manifest, so that a local fixture edit is caught without
the submodule, was considered and rejected: git already pins the content, the
freshness checks already cover staleness of oracle-generated references, and the
manifest would need touching on every legitimate change.

## 7. Facts established during design

Recorded so they are not re-derived.

- Only one simplify fixture file differs in content between the two trees:
  `cap_winmap_03.simp`.
- Exactly three upstream simplify files have no counterpart anywhere in our tree
  (the two ChebyMap fixtures above).
  Everything else is a relocation, not an absence, so R2 is overwhelmingly a move
  rather than an import.
- Oracle value drift between the old and new corpora, comparing sections by
  basename: `framesets.oracle` none; `headers.oracle` two sections, both
  `car6.head`, and both structural (`BAD` → a real position, from `460efce3`);
  `simplify_fixtures.oracle` 42 sections, all at the 1e-16 relative level, i.e.
  platform drift the sync protocol says to leave alone.
- `framesets.oracle` has the same 20 sections in both trees; only the paths
  changed.
- `headers.oracle` gains four headers upstream: `CAR_model`, `CHIPASS_Equ`,
  `cmap_3years_GP_D2`, `total_hi`.
- Duplicated basenames upstream: `car1.head` … `car5.head` (in both
  `wcsconv/inputs/` and `programs/joye_car_headers/`), `cases.txt` (three), and
  `lsst.fits-wcs` (in both `wcsconv/inputs/` and `wcsconv/expected/`, deliberate
  — the input copy is frozen so that regenerating the reference does not change
  what testgrid and testplotter draw).
- `native_round_trip_fixture` is read → write → compare, with no simplify step.
  It is one function run over three corpora by three `#[test]` wrappers, each
  auto-discovering its directory.

## 8. Risks

- **U3 is larger than a copy.**
  25 constructor functions have to be reverse-engineered from committed dumps
  and proven byte-identical.
  It is the second-largest piece after U4.
- **U1's blast radius is unknown until measured.**
  If preserving nested `IsSimp` changes many `.simp` outputs, U1 stops being a
  small fix and its diff review becomes the bulk of the upstream work.
- **U4 is the largest and least bounded piece**, at 273 flags to re-audit.
  It is also what makes the shared manifest trustworthy, so it cannot simply be
  dropped; §4 records the split contingency and its trigger.
- **U5 raises the question of which host captures the references.**
  The 42 ULP-drifting simplify sections are platform drift that the protocol
  says to leave alone, and regenerating everything here would re-anchor them to
  this machine.
  `tools/oracle_fixtures/merge_card_hunks.py` exists for exactly the
  stale-cards-with-drifted-values case and is the mitigation, but the choice of
  capture host has to be stated rather than defaulted.
