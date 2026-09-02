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

## `.rt` round-trip references in `simplify_fixtures/`

A `.map` / `.simp` reference is C's dump of a *simplified* object, so reading
one back and re-dumping it is not the identity.
A loader that validates an attribute re-applies it through the private setter,
which for a clear-on-set class discards the simplified record `astLoadMapping`
installed from the `IsSimp` card — see `docs/issues/c-library-quirks.md`.

The reference for a round-trip is therefore C's dump of the *loaded* object.
For 43 of the 790 fixtures that differs from the fixture itself, and the
difference is captured beside it as `<fixture>.rt` by
`tools/oracle_fixtures/gen_roundtrip_refs.sh`.
The generator refuses to capture a difference in anything but an `IsSimp` line,
so an unexplained divergence has to be read rather than blessed.
