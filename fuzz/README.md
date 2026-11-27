# Fuzzing the readers

Fuzz targets, run with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), for the readers of the port that take text from outside: a Native dump, a FITS header, and a MOC.
Each reads every Object it can from its input and then uses each one: a Mapping transforms a few points each way and is simplified, and every Object is written back and copied.
An error is a correct result; a panic, a run longer than 10 seconds, or more than 2 GB of memory is a finding.

| Target | Reads | Then |
|---|---|---|
| `native_read` | A Native dump, through a `NativeChannel` | uses each Object |
| `fitschan_read_write` | A FITS header, one card per line, with the default Encoding | uses each Object and writes it through a FitsChan of that Encoding |
| `mocchan_read` | A MOC, through a `MocChan` | uses each Object and writes it in both MOC formats |

What a target does with an input is in `crates/starlink-ast/tests/fuzz_entry/mod.rs`, which the regression test shares.

## Running

The fuzzer needs a nightly toolchain, and this crate is not part of the repository's workspace for that reason.

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz
./fuzz/seed.sh   # copy the test fixtures into fuzz/corpus/<target>/
./fuzz/run.sh    # every target at once for an hour, 4 processes each
```

`run.sh` takes `SECONDS_TO_RUN` and `JOBS` from the environment.
It records each crash, timeout and out-of-memory in `fuzz/artifacts/<target>/` and carries on.
Where Homebrew's `rustc` is first on the `PATH`, `rustup run nightly` is not enough, so `run.sh` puts the nightly toolchain's directory first itself.

To run one target by hand, put the same toolchain first and pass the same options:

```sh
cargo fuzz run --debug-assertions native_read -- -dict=fuzz/dict/native.dict -timeout=10 -rss_limit_mb=2048
```

The build keeps debug assertions and overflow checks, so either is a finding.

## A finding

1. Minimize it: `cargo fuzz tmin --debug-assertions <target> fuzz/artifacts/<target>/<file>`.
   `tmin` keeps any input that still fails, which may fail somewhere else; check that the minimized input fails where the original did.
2. Copy it into `crates/starlink-ast/tests/regressions/<target>/` with a name that says what it holds.
   `test_fuzz_regressions` reads every input there through the same entry point and fails if one panics or takes more than 10 seconds.
   It runs on stable, in the ordinary test suite.
3. Fix the port, test first.
   If the C library fails on the same input, record it in `docs/issues/c-library-quirks.md`.

## Open findings

`open/<target>/` holds inputs that still fail the time or memory bound, each named for what it holds, with the `PLAN.md` row that tracks it:

| Input | What happens | `PLAN.md` |
|---|---|---|
| `fitschan_read_write/slow-write-of-nine-axes` | Writing the FrameSet back takes about 9 s in the port and 28 s in C, each axis WCSAXES adds multiplying the time by about eight | §5, waiting on the C library |
| `native_read/three-frames-of-65536-axes-written-back` | A FrameSet of three Frames of 65536 axes loads in 50 MB, but writing it back peaks at 2.1 GB, the description built before the text | §5 |
| `native_read/polymap-niterinverse-of-two-billion` | A PolyMap's inverse iterates up to 2147483647 times a point, in C as in the port | §2, waiting on the C library |
| `fitschan_read_write/undefined-cd-value-*` | C fails the read for a card with an undefined value; the port reads on, into the slow write above | §5 |
| `fitschan_read_write/invalid-alternate-wcsaxes` | C fails the read for an invalid WCSAXES in an alternate description; the port reads on, into the slow write above | §5 |

When one is fixed, move it to `crates/starlink-ast/tests/regressions/<target>/`, where the replay test holds it.

## Mutations

libFuzzer's own mutations change bytes, which mostly makes a dump or header fail to parse at all.
Half of the mutations of `native_read` and `fitschan_read_write` are instead line edits (`src/lib.rs`), and the dictionaries in `dict/` give libFuzzer's own the tokens these formats are made of.
Both come from the campaign that fuzzed the C library's loaders for Starlink/ast pull request 104, "More checks against corrupt input files" (its `fuzzlib.py`):

- **Native dump:** an item's value replaced by one of a list of extreme values (`2147483647`, `-2147483649`, `99999999999`, `1e999`, `nan`, `inf`, `<bad>`, an empty value, a string), or an integer value changed by one, doubled, negated or zeroed; a line deleted or duplicated; the dump cut short; a `Begin` line swapped with an `End` line; or the class on a `Begin` line replaced by another class, or by one that does not exist.
- **FITS header:** a value replaced by an extreme or malformed one (`1.0E99999`, `NaN`, `''`, a lone quote); an axis count (`NAXIS`, `WCSAXES`) set to an extreme; a `CTYPE` replaced by a malformed one; a WCS parameter given a value of the wrong type; one of a list of awkward cards inserted (a SIP order of 100000, `WCSAXES = 2147483647`, `-TAB` with no table, an unknown `SPECSYS`); a card deleted, duplicated, cut short or given one wrong character; or the `END` card removed.

That campaign's lessons, which these targets follow:

- Read every Object in an input, and use each one: most of the C faults were in transforming, simplifying or writing an Object that had loaded, not in loading it.
- Bound the time of each input: a count read from a dump drives a loop or an allocation, and a slow input is as much a fault as a crash.
- Keep every input reproducible from its seed and mutations, and bucket the failures by where they happen, before minimizing one of each.
