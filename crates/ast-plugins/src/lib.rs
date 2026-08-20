//! Graphics plugin backends (`Grf`, `Grf3d`) — **not yet implemented**.
//!
//! When built, this crate provides the PLplot, SVG and logging backends behind
//! the `Grf` / `Grf3d` traits that `Plot` and `Plot3D` draw through.
//!
//! Nothing here is started, and it is blocked behind `Plot` itself (delivery
//! step 10). It also gates the `RegTrace` and `RegSplit` region deferrals
//! recorded in `PLAN.md` §1.
