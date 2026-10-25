# Bench fixtures

One plain Mapping of each class, the simplest realistic instance, as C's own Native dump, with `bench.oracle` recording C's transform of 24 points sampled over each fixture's own domain.
They are the `bench` set `ast-bench` times (`../sets/bench.txt`).

Regenerate both from the reference library:

    tools/perf/build_c_engine.sh bench
    target/perf/c/bench/gen_bench_fixtures crates/ast-bench/fixtures

`tools/perf/gen_bench_fixtures.c` holds each fixture's construction and sampling domain.
It writes the oracle itself rather than running `gen_transform_oracle`, which samples every Native dump over [-1000, 1000]: a TimeMap takes MJDs, a LutMap its table range and a WcsMap small angles.

`crates/ast-bench/tests/bench_fixtures.rs` holds the port to C's outputs for every section, as `test_transform_oracle.rs` does for the fixtures in `ast_tester`.
These fixtures are not in `ast_tester` yet; moving them there in a C pull request, and folding the oracle into the main gate, is a follow-up.

DssMap is absent: C has no public constructor for one, so it is covered by the DSS headers in the FrameSet set instead.
