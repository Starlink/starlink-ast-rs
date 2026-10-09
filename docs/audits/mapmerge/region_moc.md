# `Simplify` / `GetDefUnc` audit — Moc

C source: reference/starlink-ast/src/moc.c
Rust: crates/ast-core/src/regions/moc.rs
Audited: 2026-10-09 against pin c95aa801
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- moc.c:4265 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

moc.c has neither a `MapMerge` nor a `Simplify`. A Moc inherits region.c's
`Simplify`, which simplifies the FrameSet and the uncertainty a Region carries;
`Moc::simplified` (`moc.rs:1876-1881 (fn simplified)`) reaches it through
`parent_region_simplify`, since a Rust trait has no parent method to inherit,
and `test_simplify_seams.rs` names the class in `REGION_SIMPLIFY_INHERITORS`
for that reason. Its rows are region.c's, in `region_mod.md`.

`GetDefUnc` (`moc.c:4272-4306 (fn GetDefUnc)`) is a Circle at the ICRS origin whose
radius is the resolution at MaxOrder, made once and kept.
`Moc::get_def_unc` (`moc.rs:1585-1598 (fn get_def_unc)`) is the port.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `GetDefUnc`: initialise | moc.c:4272-4273 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 2 | Check the global error status | moc.c:4275-4276 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Moc | moc.c:4278-4279 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | If no default uncertainty is kept, make one | moc.c:4281-4283 (fn GetDefUnc) | moc.rs:1586-1588 (fn get_def_unc) | architecture | ported |
| 5 | Get the resolution at MaxOrder in radians | moc.c:4285-4286 (fn GetDefUnc) | moc.rs:1589 (fn get_def_unc) | architecture | ported |
| 6 | Make a Circle of that radius at the origin of the base Frame, ICRS | moc.c:4288-4297 (fn GetDefUnc) | moc.rs:1590-1596 (fn get_def_unc) | architecture | ported |
| 7 | Return the kept uncertainty | moc.c:4299-4300 (fn GetDefUnc) | moc.rs:1597 (fn get_def_unc) | architecture | ported |
| 8 | Annul the result if an error occurred | moc.c:4302-4303 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 9 | Return the default uncertainty Region | moc.c:4305-4306 (fn GetDefUnc) | moc.rs:1597 (fn get_def_unc) | architecture | ported |
