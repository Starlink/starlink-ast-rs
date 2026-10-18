# Roman benchmark fixtures

The Mappings of `perf/bench_simd.c` in [Starlink/ast pull request 51](https://github.com/Starlink/ast/pull/51), as C's own Native dumps, for `ast-bench roman` (`../../README.md`).
`tools/perf/gen_roman_fixtures.c` holds their construction; regenerate them with

    tools/perf/build_c_engine.sh bench
    target/perf/c/bench/gen_roman_fixtures crates/ast-bench/fixtures/roman
