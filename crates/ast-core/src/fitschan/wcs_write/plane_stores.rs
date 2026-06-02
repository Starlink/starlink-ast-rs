//! Per-plane FitsStore collection for FITS-WCS encoders.
//!
//! Mirrors `fitschan.c::FsetToStore` (12169-12259): given a FrameSet,
//! enumerate the primary and every alt-version frame, run the
//! `fset_to_store` pipeline once per (letter, frame), and return the
//! per-plane FitsStores ready for the encoder to emit per-axis cards
//! with the version-letter suffix.

use crate::error::AstResult;
use crate::fitschan::Encoding;
use crate::fitschan::fits_store::FitsStore;
use crate::fitschan::fset_to_store::fset_to_store;
use crate::frames::{FrameRef, FrameSet};

/// One plane in the writer's per-letter loop.
///
/// `letter == ' '` for the primary plane; `'A'..'Z'` for alt planes.
/// Plane order in the returned `Vec<PlaneStore>` is: primary first,
/// then alt planes in alphabetical letter order. The last element is
/// the plane whose **global** keyword values land in the emitted
/// header — see [`crate::fitschan::fits_store::FitsStore::mjdobs`]
/// for the contract.
#[allow(dead_code)] // wired into encoders in Group 5
pub(crate) struct PlaneStore {
    pub letter: char,
    pub store: FitsStore,
}

/// Build per-plane FitsStores for every coordinate-version letter
/// the FrameSet carries.
///
/// The returned Vec begins with the primary `(letter=' ', store)`
/// and continues with alt planes in alphabetical order. Mirrors
/// `fitschan.c::FsetToStore`'s outer loop.
#[allow(dead_code)] // wired in Group 5
pub(crate) fn collect_plane_stores(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
    primary_crvals: &[Option<f64>],
) -> AstResult<Vec<PlaneStore>> {
    let mut planes = Vec::new();

    // Primary plane (letter ' ').
    let primary_store = fset_to_store(fs, dim, encoding, ' ', primary_crvals)?;
    planes.push(PlaneStore {
        letter: ' ',
        store: primary_store,
    });

    // Alt planes — assigned via the existing
    // `frames_with_version_letters` helper (Ident-first, then A..Z
    // in frame-index order).
    let alt_assignments = super::frames_with_version_letters(fs);

    for (frame_idx_1based, letter) in alt_assignments {
        // Switch the FrameSet's current to the alt frame, run the
        // pipeline, then we discard the clone (caller's fs is
        // unchanged).
        let mut alt_fs = fs.clone();
        alt_fs.set_current(FrameRef::Index(frame_idx_1based))?;
        // Alt-plane crvals: caller-managed (the writer extracts
        // CRVAL{i}{letter} from the input chan). For now pass an
        // empty slice — `fset_to_store` will fall back to the
        // central-pixel default. A future commit can plumb
        // alt_crvals through the encode_wcs entry point.
        let alt_store = fset_to_store(&alt_fs, dim, encoding, letter, &[])?;
        planes.push(PlaneStore {
            letter,
            store: alt_store,
        });
    }

    Ok(planes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::BasicFrameBuilder;
    use crate::mappings::UnitMap;

    #[test]
    fn primary_only_yields_one_plane() {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let world = BasicFrameBuilder::new(2).domain("WORLD").build().unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(world))
            .unwrap();
        let dim = vec![Some(100.0), Some(100.0)];
        let planes = collect_plane_stores(&fs, &dim, Encoding::FitsWcs, &[]).unwrap();
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0].letter, ' ');
    }

    #[test]
    fn alt_frame_with_ident_a_yields_two_planes() {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let primary = BasicFrameBuilder::new(2).domain("WORLD").build().unwrap();
        let mut alt = BasicFrameBuilder::new(2).domain("WORLD2").build().unwrap();
        alt.set_ident("A");

        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(primary))
            .unwrap();
        // After add_frame, current == newly added frame (frame 2).
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(alt))
            .unwrap();
        // Reset current to primary (frame 2).
        fs.set_current(FrameRef::Index(2)).unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let planes = collect_plane_stores(&fs, &dim, Encoding::FitsWcs, &[]).unwrap();
        assert_eq!(planes.len(), 2);
        assert_eq!(planes[0].letter, ' ');
        assert_eq!(planes[1].letter, 'A');
    }
}
