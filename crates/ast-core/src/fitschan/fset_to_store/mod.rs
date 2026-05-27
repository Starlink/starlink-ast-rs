//! Port of `astFsetToStore` and its callees (`CelestialAxes`,
//! `SpectralAxes`, `OtherAxes`, `MakeIntWorld`, `SIPIntWorld`).
//!
//! Builds a [`FitsStore`](super::fits_store::FitsStore) from a
//! `FrameSet` by constructing the `wcs→iwc` and `pixel→iwc` mappings
//! from chosen header values, then extracting CRPIX / CD / PC / CDELT
//! via a least-squares fit. Mirrors `fitschan.c` (function references
//! on each item).
//!
//! See `docs/superpower/specs/2026-05-25-makeintworld-port-design.md`
//! for the design.
//!
//! Naming note: the upstream-equivalent module name `wcs_keywords/`
//! collides with the existing `fitschan/wcs_keywords.rs` reading-side
//! helper; we use `fset_to_store/` (matching the C entry-point name)
//! to keep the read/write halves of the pipeline distinguishable.

mod celestial_axes;
mod fiducial;
mod grism;
mod invertable;
mod is_map_linear;
mod log_axis;
mod make_int_world;
mod orth;
mod other_axes;
mod sky_pole;
mod sky_sys;
mod spectral_axes;
mod world_axes;
// mod spectral_axes;         // lands in Group 5 (Task 5.4)
// mod sip_int_world;         // lands in Group 6

// Re-exports for Group-7 callers (`fset_to_store::fset_to_store`)
// landing later. Marked `unused_imports`-allowed so the re-export
// stays in place while no in-crate caller exists yet.
#[allow(unused_imports)]
pub(crate) use make_int_world::make_int_world;
#[allow(unused_imports)]
pub(crate) use other_axes::{OtherAxesArgs, other_axes};

// Top-level entry — added in Group 7.
// pub(crate) fn fset_to_store(...) -> AstResult<FitsStore> { ... }
