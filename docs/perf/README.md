# Transform performance records

Timings of the port against the reference C library, recorded with `ast-bench` (`crates/ast-bench/README.md`) so that the effect of each round of optimisation can be shown against where it started.

Each file is one run on one machine: a header of `#` lines names the commit, the date, the machine, the Rust compiler and the C build, and the body is `ast-bench run --format tsv` output with both engines.
Absolute times belong to that machine; the Rust/C ratios travel better, but libm, vector width and caches still differ between platforms, so a ratio from another machine is a guide, not a gate.

| File | Commit | What it records |
|---|---|---|
| `2026-10-15-macos-aarch64-pre-phase0.tsv` | `048e9317` | The `bench` set before the call-count parity fixes: the starting point. |
| `2026-10-15-macos-aarch64-post-phase0.tsv` | `74348ecd` | The `bench` set after them (WcsMap set-up, SlaMap, SpecMap, MathMap and PolyMap constants, Region caches). |

The pre-Phase-0 run uses that commit's `ast-core` with the current `ast-bench` harness, built in a git worktree, since the harness did not exist yet; it uses no `ast-core` API newer than that commit.

To add a record, run `ast-bench run --set bench --engine both --format tsv` and prepend the header lines.
