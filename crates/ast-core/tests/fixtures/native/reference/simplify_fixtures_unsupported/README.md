# Unsupported simplify fixtures

These `.map`/`.simp` fixtures were imported from the upstream AST
`ast_tester/simplify_fixtures/` corpus but exercise Mapping/Region
classes the Rust port has not yet implemented. They are staged here,
grouped by the missing class, so they are excluded from both fixture
consumers:

- `test_simplify_regression.rs` (manifest-driven — no entries reference
  these files), and
- `test_serialisation.rs::native_byte_exact_round_trip_simplify_fixtures`
  and `test_native_roundtrip_all.rs` (auto-discover `.map`/`.simp` in
  `simplify_fixtures/` via a non-recursive `read_dir`, so a subdirectory
  is naturally skipped).

## Layout

- `intramap/` — fixtures containing `IntraMap` (not ported).
- `xphmap/` — fixtures containing `XphMap` (not ported).
- `stcresourceprofile/` — fixtures containing `StcResourceProfile` (not
  ported).

## Re-enabling

When one of these classes lands:

1. Move its fixtures back into `../simplify_fixtures/`.
2. Add the corresponding entries to `../simplify_tests.txt`.
3. Confirm both the simplify regression and the byte-exact round-trip
   corpus pass for them.
