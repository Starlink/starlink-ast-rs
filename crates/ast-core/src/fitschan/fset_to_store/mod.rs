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

// mod other_axes;            // lands in Task 3.2
// mod celestial_axes;        // lands in Group 5
// mod spectral_axes;         // lands in Group 5
// mod make_int_world;        // lands in Group 4
// mod sip_int_world;         // lands in Group 6
// mod orth;                  // lands in Group 4
// mod invertable;            // lands in Group 4
// mod sky_sys;               // lands in Group 5

// pub(crate) use other_axes::{OtherAxesArgs, other_axes};

// Top-level entry — added in Group 7.
// pub(crate) fn fset_to_store(...) -> AstResult<FitsStore> { ... }
