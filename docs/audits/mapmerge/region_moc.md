# `Simplify` / `GetDefUnc` audit — Moc

C source: reference/starlink-ast/src/moc.c
Rust: crates/starlink-ast/src/regions/moc.rs
Audited: 2026-10-09 against pin c95aa801
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- moc.c:4382 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

moc.c has neither a `MapMerge` nor a `Simplify`. A Moc inherits region.c's
`Simplify`, which simplifies the FrameSet and the uncertainty a Region carries;
`Moc::simplified` (`moc.rs:3741-3746 (fn simplified)`) reaches it through
`parent_region_simplify`, since a Rust trait has no parent method to inherit,
and `test_simplify_seams.rs` names the class in `REGION_SIMPLIFY_INHERITORS`
for that reason. Its rows are region.c's, in `region_mod.md`.

`GetDefUnc` (`moc.c:4389-4423 (fn GetDefUnc)`) is a Circle at the ICRS origin whose
radius is the resolution at MaxOrder, made once and kept.
`Moc::get_def_unc` (`moc.rs:3455-3468 (fn get_def_unc)`) is the port.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `GetDefUnc`: initialize | moc.c:4389-4390 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialize |
| 2 | Check the global error status | moc.c:4392-4393 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Moc | moc.c:4395-4396 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | If no default uncertainty is kept, make one | moc.c:4398-4400 (fn GetDefUnc) | moc.rs:3455-3457 (fn get_def_unc) | architecture | ported |
| 5 | Get the resolution at MaxOrder in radians | moc.c:4402-4403 (fn GetDefUnc) | moc.rs:3458 (fn get_def_unc) | architecture | ported |
| 6 | Make a Circle of that radius at the origin of the base Frame, ICRS | moc.c:4405-4414 (fn GetDefUnc) | moc.rs:3459-3465 (fn get_def_unc) | architecture | ported |
| 7 | Return the kept uncertainty | moc.c:4416-4417 (fn GetDefUnc) | moc.rs:3466 (fn get_def_unc) | architecture | ported |
| 8 | Annul the result if an error occurred | moc.c:4419-4420 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 9 | Return the default uncertainty Region | moc.c:4422-4423 (fn GetDefUnc) | moc.rs:3466 (fn get_def_unc) | architecture | ported |
