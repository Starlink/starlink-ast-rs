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

`cargo test -p starlink-ast --release` was green at `93adfea`: 1765 tests across 40
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

None. Item J and the `testtabspectrum` row landed on 2026-09-25; what follows
records what they found, since two of the three corrections this note originally
queued for item J turned out to be wrong.

### J — reconcile the two FitsChan gap docs against the code

Done.
Both gap docs carry a re-checked header, `PLAN.md` §5's two rows and the `-TAB`
blocker paragraph are rewritten, and the write-path gaps found alongside
`c384289f` are recorded in `fitschan-c-port-gaps.md` with the fixture that would
expose each.

Two of the three corrections this note queued for the item were wrong, which is
the part worth keeping:

1. **The spec's paths were not wrong.** It cites the two SIP files by bare
   filename and both are where it says. What *was* stale is its
   plumbed-but-unreachable bullet: item F made `FitsTol` and `SipOK` reachable,
   so that bullet is now marked resolved in the spec rather than corrected.
2. **The `c-oracle-probe-recipe.md` findings queued for this item were wrong,
   and the correct ones were already committed** in `93adfea`.
   `grf_stubs.c` does exist, shipped beside the doc as
   `c-oracle-probe-grf_stubs.c`, and `libast_grf_5.6.so` is still in the build.
   The real 9.5.0 breakage is that `libast_pal` and `libast_cminpack` are now
   separate shared libraries, so `-last` alone leaves `astPal*`, `astEra*` and
   the cminpack symbols undefined; both recipes were also macOS-only.
3. **`c384289f`'s remaining gaps did need recording**, and now are — all six
   verified against `fitschan.c` at the new pin rather than transcribed. Two are
   worth carrying forward as real defects rather than mere divergences:
   `write_cd_matrix`'s guard is `val != 0.0 || i == j` where C's is
   `val != 0.0`, so an explicit zero on the CD diagonal gains a card C omits;
   and `write_pc_cdelt` compares against a literal `1e-15` where C uses
   `astEQUAL`, which is a tolerance with no analogue in the C source and should
   become `ast_equal`.

### The `testtabspectrum` row

Done, but not as a new row: `testtabspectrum` is a function inside
`testfitschan.c` (`:256`, called at `:1444`), not a standalone test file, so it
belongs on the existing `testfitschan.c` tracker row and is recorded there.
A `testxphmap.c` row was added at the same time so its deliberate absence does
not read as an oversight.

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
