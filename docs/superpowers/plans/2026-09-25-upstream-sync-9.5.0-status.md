# Upstream 9.5.0 sync — status at 2026-09-25

Handover note for resuming the sync described in
[`../specs/2026-09-24-upstream-sync-9.5.0-design.md`](../specs/2026-09-24-upstream-sync-9.5.0-design.md).
The spec is authoritative for *what* each item requires; this file records only
which items are finished, which are not, and what a resumption needs to know
that the spec does not say.

## Where the branch stands

Branch `tickets/DM-54793`, HEAD `93adfea`.
Twenty commits are unpushed, from `7fbc8e3` (the `astEQUALS` regrouping) to
`93adfea` (the performance addendum).
The working tree is clean apart from the `reference/starlink-ast` submodule
marker, which is the new pin and is meant to be there.

`cargo test -p ast-core --release` was green at `93adfea`: 1765 tests across 40
targets, no failures.
The citation audit's five gates were green at `445cdb3` (7 passed, 593 s); they
have not been re-run since, and nothing committed after that touches a checklist
or a cited line.

Six `fixup!` commits earlier on the branch are **already pushed** and are out of
scope for squashing — the standing instruction was to squash only new unpushed
commits, and that was done.

## Items finished

| Item | What it was | Landed as |
|---|---|---|
| A | Invert the ten mirrors upstream fixed | `7fbc8e3`, `34325c6`, `5402b40`, `1ea9099`, `abcc001`, `05c18db`, `62e0974`, `ff1c9e1`, `3501257`, `2f00bff` |
| C | Fixture mirror back to green | `307ef20`, `8fb64fc`, `e64df72` |
| D | Re-anchor the citation audit onto the new pin | `445cdb3` |
| E | Port the new upstream regression tests | landed with each fix in A, see below |
| F | The three FitsChan SIP fixes | `c2cd151`, `2e2061b` |
| G | Restructure the quirks file, open items first | `ad65eaf` |
| H | Record the `NormUnit` gap in PLAN.md | `c58e15b` |
| I | Performance check | `93adfea` |

Item E landed pairwise with item A rather than as its own commit range, which is
what the spec asked for ("paired so each inversion lands with its gate").
Eight of its nine rows are in place; the ninth, `testtabspectrum`, is out of
scope and is the one piece of item E still owed a PLAN.md row — see below.
The Polygon case (`abcc001`) carries no test by design: a Polygon cannot hold a
bad vertex, so the guard is unpinnable, and `docs/audits/mapmerge/region_polygon.md`
records why rather than leaving the absence unexplained.

Item F is more complete than its two commit titles suggest, and the third
upstream fix is easy to mistake for missing: no commit subject names `121d12d2`,
because it landed inside `2e2061b` along with `c384289f`. Both are named as
ancestors of the `905`-`912` cases in `test_fitschan_wcs_write.rs:429`, and the
box derivation the spec asks for is really there — `sip_int_world.rs:266` pushes
a 3x3 grid spanning the image through both Mappings, and `:306` scales the
tolerance by the displacement a one-pixel step produces. Six gates cover it,
none `#[ignore]`d.

## Items outstanding

### J — reconcile the two FitsChan gap docs against the code

Not started.
The spec's item J is a complete statement of the work and should be followed as
written; it is documentation only and gates nothing.

Three corrections to make to the spec itself while doing it, all verified
against the tree:

1. **The spec's own file paths are wrong.** It names
   `sip_int_world.rs:311` and `make_int_world.rs:68` as if they sat under
   `wcs_write/`. Both live under `fset_to_store/`. Fix the spec in the same
   change — it has a revision history for exactly this.
2. **The `c-oracle-probe-recipe.md` findings queued for this item were wrong,
   and the correct ones are already committed** in `93adfea`. Do not re-apply
   the queued versions. `grf_stubs.c` does exist (shipped beside the doc as
   `c-oracle-probe-grf_stubs.c`), and `libast_grf_5.6.so` still exists in the
   build. The real 9.5.0 breakage is that `libast_pal` and `libast_cminpack`
   are now separate shared libraries, so `-last` alone leaves `astPal*`,
   `astEra*` and the cminpack symbols undefined. Both recipes were also
   macOS-only. All of this is already corrected in the probe doc, in
   `mapping-perf-vs-c-oracle.md`, and in `bench_per_class.rs`'s header, each
   verified by linking and running.
3. **`c384289f`'s remaining gaps still need recording** in the FitsChan gap
   docs: the SIP block's position relative to the per-version loop; `AltAxes`
   unmodelled; `write_cd_matrix`'s `|| i == j` and `write_pc_cdelt`'s `1e-15`
   against C's `val != 0.0` / `astEQUAL`; `write_pc_cdelt` cannot fail where
   C's CDELT branch can; and C's `PCFromStore` loops version letters where the
   port marks FitsPc primary-only.

### The `testtabspectrum` PLAN.md row

Item E's last row. `testtabspectrum` needs `-TAB`, which is three unported
things (`Table`, `FitsTable`, and the algorithm), so it is out of scope for this
sync and needs a migration-tracker row naming the FitsTable blocker and
deferring to PLAN.md §3, where that blocker already lives correctly.
`PLAN.md` has no `testtabspectrum` row today.

## Things worth not rediscovering

- **The toolchain prefix is required for every `cargo` call and every
  `git commit`**, because the pre-commit hook runs fmt, clippy and doc:
  `RUSTC=$HOME/.cargo/bin/rustc RUSTDOC=$HOME/.cargo/bin/rustdoc CARGO_TARGET_DIR=/lscratch/timj/rustup-target-1981`.
  Without it the hook fails with `E0514` on a `cc` crate built by conda's
  rustc 1.97.1 against rustup's 1.98.1.
- **`bench_per_class`'s binary is under the host-triple subdirectory** of the
  target dir (`.../x86_64-unknown-linux-gnu/release/examples/`), not directly
  under `release/`.
- **The 2026-05-19 perf table is an Apple M4 Max snapshot.** Its Rust-vs-C
  ratios are not comparable with a run on this Linux host. The addendum in
  `93adfea` is a same-host before/after and deliberately does not re-baseline
  those columns.
- **Sub-2 ns/pt bench cells need more than three reps.** At three, one cell
  read 1.061 purely from noise; the single-side rep-to-rep spread has a p90 of
  11.6%. Seven interleaved reps per side settled it.
