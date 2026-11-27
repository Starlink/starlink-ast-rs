# Upstream sync to `4029b011`, stage 0 (pin move and XphMap) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the `reference/starlink-ast` pin from `fe26cc9b` to `4029b011` with the fixture mirror, the C-docs gate and every tier-1 test green, port the XphMap rewrite the mirrored fixtures need, and record every probe the move left stale for the stage that owns it.

**Architecture:** Mechanical sync work (pin, fixture copy including the unread `fixtures/plot3d/` oracle, package include list, `c-headers.tsv`, probe captures at both pins) plus one C-faithful port: xphmap.c's `Transform` rebuilt around the used cell that holds a position (`EdgeCells`, `HpxCellUsed`, `FindHpxCell`, `XphCellGore`, `FindXphCell`), with `testxphmap.c`'s new scans ported beside the regenerated bit-for-bit oracle.
Any Moc oracle the new boundary rule moves is regenerated here if the new C agrees, and otherwise handed to stage 1 by a named exclusion.

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes in `tools/oracle_fixtures/` built by its Makefile against the cmake build in `reference/starlink-ast/build`, bash, `audit-tools` (`c-docs`).

**Spec:** `docs/superpowers/specs/2026-11-27-upstream-sync-4029b011-design.md` (stage 0 section, "Per-stage method", "Rules settled").

## Global Constraints

- Branch `u/timj/sync-c-xphmap`, created from `main` (`bc482e9f`); the design is its first commit (`1a3c7906`). Never push. Fixups with `git commit --fixup`. Commit messages end with the two lines `Generated with AI` and `Co-Authored-By: SLAC AI`, separated by a blank line.
- Host: SDF, Linux x86_64, bash. `cargo` and `rustc` are the conda env's (memory `reference_cargo_conda_env`). Run clippy as the conda env's `cargo-clippy` binary, not `cargo clippy`, and commit with `SKIP=cargo-clippy` if the pre-commit hook's driver is stale (memory `reference_clippy_toolchain_mismatch`).
- Reference build: `reference/starlink-ast/build` is a cmake Release build (`AST_BUILD_FORTRAN=OFF`) rebuilt today at `fe26cc9b`; after the pin moves, rebuild it in place with `cmake --build reference/starlink-ast/build -j 32` (about 1.5 minutes) before any probe is run, and confirm with `ls reference/starlink-ast/build/libast_grf3d_log.a`, a library that exists only at the new pin. Never reconfigure it: the cached flags are the ones the committed oracles were captured with on this host.
- Probes: `make -C tools/oracle_fixtures <probe>` links `-last -last_pal -last_cminpack -last_err -last_grf3d -lyaml` with an rpath to the build directory, so the conda `libast.so.9` trap (memory `reference_oracle_conda_rpath_trap`) does not apply to Makefile-built probes; verify once with `ldd tools/oracle_fixtures/xphmap_probe | grep libast.so.9`.
- `<scratch>` below is a directory outside the repository, `$HOME/scratch/sync-4029b011`; create it in Task 1.
- Never modify the submodule's tracked files.
- New source files open with the two-line AURA copyright / `SPDX-License-Identifier: LGPL-3.0-or-later` header; a ported C check names its C function in a `///` comment, as `test_xphmap.rs` does today.
- Docs: one sentence per line, American spelling, no C file/line citations in rendered docs (provenance in `//` comments). C's own names (`AST__HPXTOL`, `FindHpxCell`) appear only in `//` comments.
- Rust port of C: match C's construction shape and granularity (CLAUDE.md "Before porting a class"); no threshold or margin without a C analogue (stop signal 1). The one margin, `AST__HPXTOL`, is C's.
- A user-visible change gets a `CHANGELOG.md` `Unreleased` → `Fixed` entry in the same commit.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo-clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps --all-features`, `tools/fixtures/check_mirror.sh` and `./tools/oracle_fixtures/check_c_headers_fresh.sh` all pass.

## Triage

Read from each commit's diff and the Rust site (2026-11-27).
Verdicts: **mirror** (port reproduces old C; invert), **missing** (port new C), **n/a** (no observable behavior in the port, with the reason).

| commit | verdict | what changes | Rust site | task |
|---|---|---|---|---|
| `250d9fc0` | mirror | XPHN/XPHS forward: a position beyond the grid but in line with a gore gave values; now bad | `xphmap.rs` `transform_point`, the four gore arms test `s`/`d` but never the grid's edge | 2 |
| `65c31510` | mirror | Transform rebuilt around the used cell: a cell owns its low RA edges and a position on an edge of an unused cell tries the cells across it; a position within `AST__HPXTOL` degrees of an edge is on it; the unused copy of a split-facet cell is moved to the used copy in both directions; a facet index is `floor`ed, so a position less than one facet below or left of an HPX grid is bad | `xphmap.rs` `transform_point`: `hpx_cell` truncates toward zero and has no wrap; HPX12 forward is the identity on the used area; no edge tolerance | 2 |
| `404db754`, `0a81519f`, `1c08fd7d`, `152bdde8`, `0a553bed`, `4fe05a67`, `5c990b43` | n/a | C `CLAUDE.md`, plans, `PLAN.md`, `ast.news` | — | — |
| `824308df`, `d926227a`, `a91c3190`, `746873e0`, `09f9773e`, `b8263828`, `78d4c87a` | stage 1 | Moc, sky line, `testmoccds.c` | — | — |
| `5135166e`, `8ccd17db`, `d63e13bf`, `39478966`, `8f611621`, `a2cd817d`, `1e829fb5`, `6d89ba24`, `15e7cb3c`, `1e43362d` | Plot3D | grf3d_log, testplot3dlog, plot3d.c | — | — |
| `b6d01575`, `e640543a`, `162ff48f`, `c2bb907a` | stage 2 | plot.c tick values and ForceExterior, cmpframe.c Equal | — | — |

## Review Focus

1. A position on the corner shared by four used cells in the interior of a grid must transform through the cell that owns it (its high RA side), exactly, in both directions, and the same position given to the inverse of another type must land on the same point of the sky. Pinned in Task 2 by `cell_boundaries_continue_a_used_cell` (every corner and edge center, exact comparison).
2. A position in the unused copy of a split-facet cell, which the HPX12 forward transformation passed through unchanged, must move four facets on both axes to the used copy, in both directions and for HPX0 too. Pinned in Task 2 by the `ncopy` counts of `every_pixel_appears_once_in_each_projection` and the `(5n, 5n)` points of `the_largest_order_works`.
3. A position less than one facet below or to the left of an HPX grid, whose facet index truncated toward zero into facet 0, must be bad. Pinned in Task 2 by `positions_outside_the_grid_are_bad` (the margin runs five facets below `0.5`) and `the_largest_order_works` (`(0.0, 1.5n + 1.0)` at order 27).
4. NaN and infinite inputs must give bad values and never panic in the `floor`/`ceil` arithmetic or the gore selection, for every type, order and direction. Pinned in Task 2 by `far_positions_give_bad_values`, which now requires bad everywhere.
5. A Moc whose Region's test positions lie on a facet meridian or a pole may gain or lose cells when XphMap starts transforming those positions; the port must then agree with the C library at the new pin, never with a stale oracle. Pinned in Task 3 (each Moc oracle that moves is compared with the new-pin capture before it is regenerated or excluded).

---

### Task 1: Move the pin, mirror the fixture tree, capture the probes at both pins

**Files:**
- Modify: `reference/starlink-ast` (gitlink only), `crates/starlink-ast/tests/fixtures/**` (copied from upstream; new `plot3d/`, `simplify/cap_xphmap_03.{map,simp}`, `simplify/cases.txt`, `oracle/simplify_fixtures.oracle`, `DIST_MANIFEST`), `crates/starlink-ast/Cargo.toml` (`include` list), `docs/audits/c-docs/c-headers.tsv`
- Regenerate: `crates/starlink-ast/tests/fixture_full_dump_oracle/cases.rs`, `crates/starlink-ast/tests/fixture_read_warnings_oracle/cases.rs` (the two probes that walk the fixture tree)

**Interfaces:**
- Produces: `reference/starlink-ast/build` at `4029b011`, used by every later task; `<scratch>/probes-old` and `<scratch>/probes-new`, the capture of every probe at each pin, read by Tasks 3 and 4.

- [ ] **Step 1: Capture every probe at the old pin.** The checkout and `build/` are both at `fe26cc9b` today (confirm: `git -C reference/starlink-ast rev-parse HEAD` prints `fe26cc9b…` and `build/ast.h` is dated today). Run:

```bash
mkdir -p $HOME/scratch/sync-4029b011
./tools/oracle_fixtures/check_probes_fresh.sh --capture $HOME/scratch/sync-4029b011/probes-old 2>&1 | tee $HOME/scratch/sync-4029b011/capture-old.log
```

Expected: one output file per probe in `probes-old/`; a probe that fails to build or run on this host (memory `reference_oracle_link_950_split`: some compiler-line recipes lack `-last_pal -last_cminpack`) is named in the log and fails the same way at both pins, so it drops out of the Step 7 diff and is compared by hand there.

- [ ] **Step 2: Move the checkout and stage the pin.**

```bash
git -C reference/starlink-ast fetch origin
git -C reference/starlink-ast checkout --detach 4029b011
git add reference/starlink-ast
git submodule status   # prints 4029b011… (v9.5.0-370-g4029b011)
```

- [ ] **Step 3: Rebuild the reference library in place.** `cmake --build reference/starlink-ast/build -j 32`, then `ls reference/starlink-ast/build/libast_grf3d_log.a` — expected: the file exists. Also `cmake --build reference/starlink-ast/build-fma -j 32`, kept current for the Plot3D stages' dump checks.

- [ ] **Step 4: Mirror the fixture tree.**

```bash
rsync -a --delete reference/starlink-ast/ast_tester/fixtures/ crates/starlink-ast/tests/fixtures/
tools/fixtures/check_mirror.sh
```

Expected: `fixture tree matches upstream at 4029b011`. `git status --short crates/starlink-ast/tests/fixtures | wc -l` is about 67: the 62 files of `plot3d/`, the two `cap_xphmap_03` files, `simplify/cases.txt`, `oracle/simplify_fixtures.oracle` and `DIST_MANIFEST`.

- [ ] **Step 5: Add the new manifest lines to the package include list.** In `crates/starlink-ast/Cargo.toml`, after `"/tests/fixtures/plot/expected/*.svg",`, add in the manifest's order:

```toml
    "/tests/fixtures/plot3d/cases.txt",
    "/tests/fixtures/plot3d/inputs/*.ast",
    "/tests/fixtures/plot3d/expected/*",
```

Run `cargo test -p starlink-ast --test meta test_dist_manifest::` — expected PASS (the list is the manifest entry for entry).

- [ ] **Step 6: Regenerate the C-docs header table.**

```bash
cargo run -q -p audit-tools --bin c-docs -- headers reference/starlink-ast/src > docs/audits/c-docs/c-headers.tsv
./tools/oracle_fixtures/check_c_headers_fresh.sh
cargo test -p starlink-ast --test meta test_c_docs_audit::
```

Expected: `git diff --stat docs/audits/c-docs/c-headers.tsv` shows three changed digests (`astAddRegion`, `astAddPixelMask<X>`, `MinOrder`) and no added row; both commands pass. The ledger rows are re-reviewed in stage 1 (Task 5 records this).

- [ ] **Step 7: Capture every probe at the new pin and diff.**

```bash
./tools/oracle_fixtures/check_probes_fresh.sh --capture $HOME/scratch/sync-4029b011/probes-new 2>&1 | tee $HOME/scratch/sync-4029b011/capture-new.log
diff -rq $HOME/scratch/sync-4029b011/probes-old $HOME/scratch/sync-4029b011/probes-new | tee $HOME/scratch/sync-4029b011/probes-moved.txt
```

Expected in `probes-moved.txt`: `xphmap_probe` (this stage); `fixture_full_dump_probe` and `fixture_read_warnings_probe` (the tree gained `plot3d/inputs/box3d.ast` and `frame3d.ast`; this stage); `region_moc_probe`, `region_moc_add_probe`, `region_moc_text_probe`, `mocchan_probe` and any region or SkyFrame probe the sky-line fix moves (stage 1, unless Task 3 finds the XphMap port alone accounts for a case); `plot_native_probe`, `plot_attrs_probe`, `plot_construct_probe`, `attr_strings_probe` (stage 2). Anything else is read and assigned to a stage by the commit that changed it. Keep the list for Task 5.

- [ ] **Step 8: Regenerate the two fixture-tree probes.**

```bash
./tools/oracle_fixtures/check_probes_fresh.sh --write fixture_full_dump_probe fixture_read_warnings_probe
cargo fmt
cargo test -p starlink-ast --test serial test_fixture_full_dump_oracle:: test_fixture_read_warnings_oracle::
```

Expected: `git diff --stat` on the two `cases.rs` shows only rows for `./plot3d/inputs/box3d.ast` and `./plot3d/inputs/frame3d.ast` added (a Box and a Frame, which C reads cleanly, so only the full-dump file gains rows); both tests PASS. Any other changed row belongs to a later stage: revert it here and note it for Task 5.

- [ ] **Step 9: See what the move leaves red.** `cargo test -p starlink-ast --no-fail-fast 2>&1 | grep -E "^test .* FAILED|^failures:|test result" | tee $HOME/scratch/sync-4029b011/tier1-after-pin.txt`. Expected failures, all owned by Task 2: `test_transform_oracle` on the `neg_xph_*`, `cap_xphmap_02` and `cap_xphmap_03` sections. Any other failure is read now and either assigned to Task 2 (XphMap) or Task 3 (Moc), or named in Task 5's stale table.

- [ ] **Step 10: Commit** the gitlink, the fixture tree, `Cargo.toml`, `c-headers.tsv` and the two regenerated oracles: `Move the reference/starlink-ast pin to 4029b011`. The body names the two upstream PRs (#107, #112) and the design doc, and says `fixtures/plot3d/` is mirrored unread for the Plot3D port.

### Task 2: Port the XphMap transform around the used cell that holds a position

Upstream `65c31510` and `250d9fc0`. C's `Transform` now finds the used cell for a position with `FindHpxCell` (HPX grids, both directions) or `FindXphCell` (XPH grids, forward), each built on `EdgeCells`, `HpxCellUsed` and `XphCellGore`, and transforms by that cell; the tolerance is `AST__HPXTOL * nppf / 45` grid units.

**Files:**
- Modify: `crates/starlink-ast/src/mappings/xphmap.rs` (`transform_point`, lines 129–303; module doc)
- Regenerate: `crates/starlink-ast/tests/xphmap_oracle/cases.rs`
- Modify: `crates/starlink-ast/tests/test_xphmap.rs`, `CHANGELOG.md`

**Interfaces:**
- Produces, in `xphmap.rs`: `pub(crate) const HPXTOL: f64 = 1.0e-12;` (degrees; C `AST__HPXTOL`, which stage 1's Moc `PosToNested` port also uses), and the private functions below, each a port of the C function named in its `//` comment:
  - `fn edge_cells(v: f64, tol: f64, upper: bool) -> (usize, [f64; 2], [f64; 2])` — the number of cells (1 or 2), their zero-based indices with the owner first, and the coordinate to use with each (`v`, then the edge).
  - `fn hpx_cell_used(ix: f64, iy: f64, nppf: f64, wrap: bool) -> Option<f64>` — `Some(shift)` if the cell is used, or is an unused copy and `wrap`; `shift` is the offset to the used copy, 0.0 unless an unused copy was accepted.
  - `fn find_hpx_cell(x: &mut f64, y: &mut f64, nppf: f64, tol: f64, wrap: bool) -> Option<f64>` — `Some(s)`, the sum of the used cell's indices plus one, with `x`, `y` moved onto the used cell; `None` leaves them unchanged.
  - `fn xph_cell_gore(projection: XphProjection, ix: f64, iy: f64, nppf: f64) -> Option<usize>` — the gore (0 to 3) if the cell is used.
  - `fn find_xph_cell(projection: XphProjection, x: &mut f64, y: &mut f64, nppf: f64, tol: f64) -> Option<usize>` — the gore of the used cell, two passes (same gore first), with `x`, `y` moved as in C.

- [ ] **Step 1: Regenerate the oracle.** `./tools/oracle_fixtures/check_probes_fresh.sh --write xphmap_probe`. Expected: `git diff --stat crates/starlink-ast/tests/xphmap_oracle/cases.rs` shows the `XPHN`/`XPHS` forward rows gaining `B`s beyond the grid and the HPX rows changing at facet edges and in the unused half-facets. Run `cargo test -p starlink-ast --test mappings test_xphmap::transformations_match_the_c_library_bit_for_bit` — expected FAIL.

- [ ] **Step 2: Rewrite `each_pixel_appears_once_in_each_projection`** to `testxphmap.c`'s `check_coverage` (orders 0 to 3, both directions, every type): a pixel center that comes back as itself is good; one that comes back displaced by exactly `4 * nppf` on both axes, the same way, is an unused copy and allowed only when `!forward || projection.starts_with('H')`; anything else fails with the C message's content. Then:

```rust
assert_eq!(ngood - ncopy, 12 * nppf * nppf, "{projection} order {order} forward {forward}");
let want_copies = if forward && projection.starts_with('X') { 0 } else { nppf * nppf };
assert_eq!(ncopy, want_copies, "{projection} order {order} forward {forward}");
```

- [ ] **Step 3: Replace `far_positions_give_bad_or_finite_values`** with `far_positions_give_bad_values`: the same `far` list and pairing as today, orders `[0, 3, 27]`, both directions, every type, and every output `BAD_VALUE` on both axes (C's `check_far_positions`). Drop the `bounded` branch and update the `///` comment: the forward transformation of an XPH grid now tests the grid's edge too.

- [ ] **Step 4: Add `positions_outside_the_grid_are_bad`** (C `check_outside_grid`), orders 0 to 3, both directions, every type: `nfacet = if forward && X { 4 } else { 5 }`, `lo = 0.5`, `hi = nfacet * nppf + 0.5`, `margin = 5 * nppf`, a lattice of step 0.25 from `lo - margin` to `hi + margin` on both axes (`n = ((hi - lo + 2 * margin) * 4) as usize + 1` points per row); every position with either coordinate outside `[lo, hi]` gives `BAD_VALUE` on both axes; count `ninside` (inside and transformed) and `noutside`, and `assert!(ninside >= 12 && noutside >= 12)` so the scan is not vacuous.

- [ ] **Step 5: Add `cell_boundaries_continue_a_used_cell`** (C `check_boundaries`), orders 0 to 2, both directions, every type, with every lattice and cell index an `i64` so the differences below cannot underflow: lattice index `a` at coordinate `0.25 * a - 0.5`, `nlat = 4 * (ncell + 2) + 1`; for each even `(a, b)` that is not a cell center on both axes, the one-based cells it belongs to on each axis (one if `a % 4 == 2`, else `a / 4 - 1` and `a / 4`); a cell is used if its center transforms; for each used cell among them with center lattice index `(ca, cb) = (4kx + 2, 4ky + 2)` the wanted position is

```rust
let wantx = xout[k] + (a - ca) as f64 * (xout[k + 1] - xout[k]) + (b - cb) as f64 * (xout[k + nlat] - xout[k]);
let wanty = yout[k] + (a - ca) as f64 * (yout[k + 1] - yout[k]) + (b - cb) as f64 * (yout[k + nlat] - yout[k]);
```

and the position must equal one such wanted position exactly (`==`), or be `BAD_VALUE` if it bounds no used cell; a used cell whose `k + 1` or `k + nlat` neighbor is bad fails ("not transformed as a whole"); guard `nbound >= 12 && nedge - nbound >= 12`.

- [ ] **Step 6: Add `every_pole_transforms_to_the_same_pole`** (C `check_poles`), orders `[0, 1, 3, 27]`: with `n = nppf as f64`, the HPX12 north-pole positions `ntip = (k*n + 0.5, (k+2)*n + 0.5)` and south `stip = ((k+2)*n + 0.5, k*n + 0.5)` for `k` in 0..4; an XPH grid's own pole is `(2n + 0.5, 2n + 0.5)` (four copies) and the other pole its four corners `(0.5 or 4n + 0.5, 0.5 or 4n + 0.5)`; for each type and `k`, the forward transformation of each of that type's north (south) positions is one of the four `ntip` (`stip`), and the inverse of each `ntip` (`stip`) is one of that type's four north (south) positions (exact equality on both axes).

- [ ] **Step 7: Add `a_position_within_the_rounding_margin_is_on_the_edge`** (C `check_rounding`), orders `[0, 3, 27]`, every type: `tol = 1.0e-12 * n / 45.0`, `x = n + 0.5`, `y = 2n + 0.75`; the inverse transformation of `(x, y)` is not bad; `(x - 0.5 * tol, y)` gives the same pair exactly; `(x - 100.0 * tol, y)` gives `BAD_VALUE`.

- [ ] **Step 8: Extend `the_largest_order_works`** to C's `check_max_order`: make `check` take the direction; keep the four existing points, change `(5n, 5n)` forward to `Some((n, n))` and add it inverse too; add forward `(0.0, 1.5 * n + 1.0)` → `None`; for XPHN add forward `(4n + 1.0, 1.0)` → `None`, inverse `(n + 0.5, 3n + 0.5)` → `Some((2n + 0.5, 2n + 0.5))`, forward `(2n + 0.5, 2n + 0.5)` → `Some((3n + 0.5, 5n + 0.5))`.

- [ ] **Step 9: Run the file.** `cargo test -p starlink-ast --test mappings test_xphmap::` — expected FAIL in `transformations_match_the_c_library_bit_for_bit`, `each_pixel_appears_once_in_each_projection` (no copies), `far_positions_give_bad_values` (XPH forward finite), `positions_outside_the_grid_are_bad`, `cell_boundaries_continue_a_used_cell`, `every_pole_transforms_to_the_same_pole`, `a_position_within_the_rounding_margin_is_on_the_edge` and `the_largest_order_works`; the loader tests still PASS.

- [ ] **Step 10: Implement** the constant and the five functions of the Interfaces block in `xphmap.rs`, each with C's expressions (`floor(v - 0.5)` / `ceil(v - 1.5)` for the owner, `floor(v) + 0.5` for the nearest edge, `floor(ix / nppf)` for a facet index, `s = ix + iy + 1`, `d = ix - iy`, the gore and `used` tables of `XphCellGore`, the quadrant table of `FindXphCell` with the pole in gore 1), then rewrite `transform_point`: keep the up-front `is_bad` check (NaN and infinity are bad for every Mapping here, as today), compute `tol = HPXTOL * nppf / 45.0`, and follow C's `Transform` arm for arm: HPX forward `find_hpx_cell(wrap = true)` then the HPX0 shift by `s <= n5`; XPH forward `find_xph_cell` then the gore arms; inverse `find_hpx_cell(wrap = true)` then the arms by `s`. Delete the old `hpx_cell` closure and `f`. Update the module doc: a position is on the sky if it is in a used cell or on its boundary, the unused copy of a cell is moved to the used copy, and the sentence saying Moc is not ported goes (Moc builds XphMaps through `XphMap::new`).

- [ ] **Step 11: Run the file.** `cargo test -p starlink-ast --test mappings test_xphmap::` — expected PASS, every test.

- [ ] **Step 12: Run the fixture consumers.** `cargo test -p starlink-ast --test mappings test_transform_oracle::` (the `xph` rows), `cargo test -p starlink-ast --test meta test_simplify_regression::` and `cargo test -p starlink-ast --test serial test_serialisation:: test_deferred_fixtures::` (the `cap_xphmap_03` round trip; memory `project_simplify_fixture_two_tests`) — expected PASS with no exclusion added.

- [ ] **Step 13: CHANGELOG.** Under `Unreleased` → `Fixed`, one `starlink-ast` entry: an XphMap now transforms a position on the boundary of a used HEALPix cell (a pole, a meridian between polar facets, the edge of a gore) through that cell, treats a position within the HPX projection's rounding margin of an edge as on it, moves a position in the unused copy of the split facet to the used copy in both directions, and gives bad values for every position outside the grid, as the C library now does; before, such positions were bad, or beyond an XPH grid were transformed.

- [ ] **Step 14: Commit** `xphmap.rs`, `cases.rs`, `test_xphmap.rs`, `CHANGELOG.md`: `Transform an XphMap position through the used cell that holds it`. The body names `65c31510` and `250d9fc0`.

### Task 3: Settle the Moc oracles the XphMap change moves

Moc builds its grids with `XphMap::new`, so positions on facet meridians now transform and a Moc's cells can change before the stage 1 Moc fixes land.
C at the new pin has both changes, so a Moc case that the port now matches in `probes-new` is the XphMap's doing and is regenerated here; one that matches neither capture waits for stage 1.

**Files:**
- Possibly regenerate: `crates/starlink-ast/tests/region_moc_oracle/cases.rs`, `region_moc_add_oracle/cases.rs`, `region_moc_text_oracle/cases.rs`, `mocchan_oracle/cases.rs`
- Possibly modify: `crates/starlink-ast/tests/test_region_moc_oracle.rs`, `test_region_moc_add_oracle.rs`, `test_moc.rs` (a named exclusion list)

- [ ] **Step 1: Run the Moc tests.** `cargo test -p starlink-ast --test regions test_moc:: test_mocchan:: test_region_moc_oracle:: test_region_moc_add_oracle:: test_region_moc_text_oracle:: 2>&1 | grep -E "FAILED|test result"`. If every test passes, write "no Moc oracle moved" in the Task 5 notes and skip to Task 4.

- [ ] **Step 2: For each failing case, compare three values**: the committed oracle (old C), `<scratch>/probes-new/<probe>` (new C) and the port's output (from the failure message or a `dbg!`). If the port equals the new C, the case moved with XphMap: run `./tools/oracle_fixtures/check_probes_fresh.sh --write <probe>`, `cargo fmt`, confirm `git diff` on that `cases.rs` touches only the failing cases, and keep the regeneration. If the port equals neither, the case needs a stage 1 Moc fix: add it to a `const XPHMAP_MOVED_EXCLUDED: &[(&str, &str)]` in that test file with the reason naming the stage 1 commit (`d926227a` for a position on the edge of the sky, `09f9773e` for a Region beyond its box, `a91c3190` for MaxOrder 0), skip only those names in the comparing test, and add `excluded_moc_cases_still_differ`, which fails when an excluded case matches the committed oracle again. A regenerated probe whose other cases also changed is a stage 1 probe: do not write it; exclude instead.

- [ ] **Step 3: Rerun** the Step 1 command — expected PASS.

- [ ] **Step 4: Commit** if anything changed: `Regenerate the Moc oracles the XphMap boundary rule moved` (or `Exclude the Moc cases that wait for stage 1`), the body listing each case and its verdict.

### Task 4: Sweep the corpora and run the stage-end gates

**Files:**
- None expected; any stale reference found is recorded in Task 5, not fixed here.

- [ ] **Step 1: Corpus sweeps.** `./tools/oracle_fixtures/check_corpus_fresh.sh 2>&1 | tee $HOME/scratch/sync-4029b011/corpus.txt` and `./tools/oracle_fixtures/check_simplify_fresh.sh 2>&1 | tee $HOME/scratch/sync-4029b011/simplify.txt`. Expected: no `CARD-ONLY`, `MIXED` or `card` verdict (the range changes no serializer); `NUMERIC`/`numeric` rows are platform drift and are left alone, as the README says. A `card` verdict is read and assigned to the stage whose commit caused it, or investigated now if no commit in the range explains it.

- [ ] **Step 2: Probe gate.** `./tools/oracle_fixtures/check_probes_fresh.sh xphmap_probe fixture_full_dump_probe fixture_read_warnings_probe` — expected `SAME` for all three (the ones this stage regenerated). The stage 1 and stage 2 probes still differ, by design, and are listed in Task 5.

- [ ] **Step 3: Stage-end gates** from Global Constraints: tier 1 (`cargo test -p starlink-ast --no-fail-fast`), clippy, fmt, docs, `check_mirror.sh`, `check_c_headers_fresh.sh`. Expected: all pass. Also `cargo test -p starlink-ast --test meta test_probes_table::` — expected PASS (no new generated file).

### Task 5: Record stage 0 in PLAN.md and the trackers

**Files:**
- Modify: `PLAN.md` (§7 Plot3D row at about line 346, §8, the C → Rust test migration tracker), `docs/issues/c-library-quirks.md` (one stale sentence)

- [ ] **Step 1: §8 subsection.** Before `### Done: sync to \`fe26cc9b\``, add `### In progress: sync to \`4029b011\`` (and change the section heading `## 8. Upstream syncs — done` to `## 8. Upstream syncs`): the pin move (`fe26cc9b` → `4029b011`, PRs #107 and #112, 35 commits), a link to the design, a stage table (0 pin and XphMap, `u/timj/sync-c-xphmap`, done; 1 Moc and the sky line, `u/timj/sync-c-moc`, pending; 2 Plot and CmpFrame, `u/timj/sync-c-plot`, pending), a paragraph on what stage 0 ported (the two XphMap commits, the new `testxphmap.c` scans, the tree probes) and Task 3's Moc verdicts, and a "left stale" table from Task 1 Step 7 with one row per moved probe: probe, what moved it, owning stage. Add rows for: `fixtures/plot3d/` mirrored unread (first read by Plot3D stage 1 for the dumps and stage 3 for the logs and SVGs); the three Moc ledger rows re-reviewed in stage 1; the Java port resync deferred at lower priority (the user), with the range to re-read.

- [ ] **Step 2: Retire the PR 107 pending section.** Replace `### Pending: Moc, XphMap and SkyFrame fixes from Starlink/ast PR 107` with a short note under the new subsection: its xphmap.c row is done (stage 0) and `far_positions_give_bad_or_finite_values` is now `far_positions_give_bad_values`; its six Moc and SkyFrame rows are stage 1's, and the table moves there unchanged.

- [ ] **Step 3: Plot3D row.** In the Plot (stage) row of §7, after "`ast-plugins` is still a one-line stub…", add: "`fixtures/plot3d/`, the C oracle of Starlink/ast PR 112 (logging 3D backend, 26 `testplot3dlog` cases with logs, dumps and four fixed-camera SVGs), is mirrored and unread until Plot3D".

- [ ] **Step 4: Migration tracker.** Update the `testxphmap.c` row: "the scans added upstream in PR 107 (every position outside a grid bad, every cell boundary, the poles, the rounding margin, the largest order) ported <the commit's date> in stage 0 of the `4029b011` sync". Add rows: `testmoccds.c` — `not ported — stage 1 of the 4029b011 sync, as test_moccds.rs computing the reference with the cdshealpix crate`; `testplotattr.c` — `not ported — stage 2`; `testgrf3dlog.c` and `testplot3dlog.c` — `deferred: Plot3D; their fixtures are mirrored in fixtures/plot3d/`. Note on the `testmoc.c`, `testregions.c`, `testmocchan.c` and `testcmpframe.c` rows which stage ports their new checks.

- [ ] **Step 5: Quirks file.** In the entry "`astLoadXphMap` leaks the projection-type string it reads", correct the sentence that says XphMap is not ported: it is (`test_xphmap.rs`), and the leak has no counterpart in Rust.

- [ ] **Step 6: Commit** `Record stage 0 of the 4029b011 sync in PLAN.md`.
