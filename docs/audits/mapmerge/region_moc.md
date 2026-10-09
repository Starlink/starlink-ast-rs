# `Simplify` / `GetDefUnc` audit — Moc

C source: reference/starlink-ast/src/moc.c
Rust: crates/ast-core/src/regions/moc.rs
Audited: 2026-10-09 against pin c95aa801
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- moc.c:4283 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

moc.c has neither a `MapMerge` nor a `Simplify`. A Moc inherits region.c's
`Simplify`, which simplifies the FrameSet and the uncertainty a Region carries;
`Moc::simplified` (`moc.rs:2608-2613 (fn simplified)`) reaches it through
`parent_region_simplify`, since a Rust trait has no parent method to inherit,
and `test_simplify_seams.rs` names the class in `REGION_SIMPLIFY_INHERITORS`
for that reason. Its rows are region.c's, in `region_mod.md`.

`GetDefUnc` (`moc.c:4290-4324 (fn GetDefUnc)`) is a Circle at the ICRS origin whose
radius is the resolution at MaxOrder, made once and kept.
`Moc::get_def_unc` (`moc.rs:2317-2330 (fn get_def_unc)`) is the port.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `GetDefUnc`: initialise | moc.c:4290-4291 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 2 | Check the global error status | moc.c:4293-4294 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Moc | moc.c:4296-4297 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | If no default uncertainty is kept, make one | moc.c:4299-4301 (fn GetDefUnc) | moc.rs:2318-2320 (fn get_def_unc) | architecture | ported |
| 5 | Get the resolution at MaxOrder in radians | moc.c:4303-4304 (fn GetDefUnc) | moc.rs:2321 (fn get_def_unc) | architecture | ported |
| 6 | Make a Circle of that radius at the origin of the base Frame, ICRS | moc.c:4306-4315 (fn GetDefUnc) | moc.rs:2322-2328 (fn get_def_unc) | architecture | ported |
| 7 | Return the kept uncertainty | moc.c:4317-4318 (fn GetDefUnc) | moc.rs:2329 (fn get_def_unc) | architecture | ported |
| 8 | Annul the result if an error occurred | moc.c:4320-4321 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 9 | Return the default uncertainty Region | moc.c:4323-4324 (fn GetDefUnc) | moc.rs:2329 (fn get_def_unc) | architecture | ported |
