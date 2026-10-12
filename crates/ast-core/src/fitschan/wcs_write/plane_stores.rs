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
use crate::fitschan::fset_to_store::sky_sys::is_sky_off;
use crate::fitschan::fset_to_store::{AltAxes, WriteAttrs, fset_to_store, fset_to_store_absolute};
use crate::frames::{FrameRef, FrameSet};

/// One plane in the writer's per-letter loop.
///
/// `letter == ' '` for the primary plane; `'A'..'Z'` for alt planes.
/// Plane order in the returned `Vec<PlaneStore>` is: primary first,
/// then alt planes in alphabetical letter order. The last element is
/// the plane whose **global** keyword values land in the emitted
/// header — see [`crate::fitschan::fits_store::FitsStore::mjdobs`]
/// for the contract.
pub(crate) struct PlaneStore {
    pub letter: char,
    pub store: FitsStore,
}

/// The version letters C's `FsetToStore` gives the Frames other than the
/// base and current Frames (fitschan.c:12316-12381), indexed by Frame
/// index (1-based, entry 0 unused): `None` for a Frame described by no
/// alternate axes, which includes every IWC Frame. A Frame whose Ident is
/// a single upper case letter takes that letter if no earlier Frame has;
/// unless AltAxes is IDENT, the others take the free letters in order.
///
/// Also returns C's counter `s`: the last letter the second pass handed
/// out, or the character before `A` when it handed out none, from which
/// the descriptions of absolute coordinates added for offset SkyFrames
/// take their letters.
pub(crate) fn version_letters(fs: &FrameSet, alt_axes: AltAxes) -> (Vec<Option<char>>, u8) {
    let nfrm = fs.nframe();
    let ibase = fs.base_idx() + 1;
    let icurr = fs.current_idx() + 1;
    let mut sid: Vec<Option<char>> = vec![None; nfrm + 1];
    let mut iwc = vec![false; nfrm + 1];
    let mut used = [false; 26];
    for ifrm in 1..=nfrm {
        if ifrm == icurr || ifrm == ibase {
            continue;
        }
        let Ok(frame) = fs.frame(FrameRef::Index(ifrm)) else {
            continue;
        };
        if crate::fitschan::chr_match_n(frame.domain(), "IWC", 3) {
            iwc[ifrm] = true;
            continue;
        }
        let id = frame.ident();
        if let [c] = id.as_bytes()
            && c.is_ascii_uppercase()
            && !used[(c - b'A') as usize]
        {
            used[(c - b'A') as usize] = true;
            sid[ifrm] = Some(*c as char);
        }
    }
    let mut s = b'A' - 1;
    if alt_axes != AltAxes::Ident {
        for ifrm in 1..=nfrm {
            if ifrm == icurr || ifrm == ibase || iwc[ifrm] || sid[ifrm].is_some() {
                continue;
            }
            // C steps on past every letter already given; past `Z` there
            // are no letters to give.
            loop {
                s += 1;
                if s > b'Z' || !used[(s - b'A') as usize] {
                    break;
                }
            }
            if s <= b'Z' {
                used[(s - b'A') as usize] = true;
                sid[ifrm] = Some(s as char);
            }
        }
    }
    (sid, s)
}

/// Build per-plane FitsStores for every coordinate-version letter
/// the FrameSet carries, in the order C's `FsetToStore` makes them
/// (fitschan.c:12301-12406): the primary description from the current
/// Frame; unless AltAxes is NONE, one of absolute coordinates if the
/// current Frame is an offset SkyFrame; then for each Frame given a
/// letter, in Frame order, its description and, for an offset SkyFrame,
/// one of absolute coordinates. Alternates are made only when the
/// primary description is. An alternate that cannot be made is left out.
pub(crate) fn collect_plane_stores(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
    attrs: WriteAttrs,
) -> AstResult<Vec<PlaneStore>> {
    let mut planes = Vec::new();
    let primary_isoff = is_sky_off(fs, fs.current_idx()) != 0;

    // An alternate axis description only qualifies a primary one, so when
    // the primary cannot be described the whole store is worthless and no
    // plane is returned, as C frees it (fitschan.c:12403).
    let (primary_store, primok) = fset_to_store(fs, dim, encoding, ' ', attrs.clone())?;
    if !primok {
        return Ok(planes);
    }
    planes.push(PlaneStore {
        letter: ' ',
        store: primary_store,
    });
    if attrs.alt_axes == AltAxes::None {
        return Ok(planes);
    }

    let (sid, mut s) = version_letters(fs, attrs.alt_axes);
    let push = |planes: &mut Vec<PlaneStore>, (store, ok): (FitsStore, bool), letter| {
        if ok {
            planes.push(PlaneStore { letter, store });
        }
    };
    if primary_isoff {
        s += 1;
        if s <= b'Z' {
            let made = fset_to_store_absolute(fs, dim, encoding, s as char, attrs.clone())?;
            push(&mut planes, made, s as char);
        }
    }
    for (ifrm, letter) in sid.iter().enumerate() {
        let Some(letter) = *letter else {
            continue;
        };
        let mut alt_fs = fs.clone();
        alt_fs.set_current(FrameRef::Index(ifrm))?;
        let made = fset_to_store(&alt_fs, dim, encoding, letter, attrs.clone())?;
        push(&mut planes, made, letter);
        if is_sky_off(fs, ifrm - 1) != 0 {
            s += 1;
            if s <= b'Z' {
                let made =
                    fset_to_store_absolute(&alt_fs, dim, encoding, s as char, attrs.clone())?;
                push(&mut planes, made, s as char);
            }
        }
    }
    Ok(planes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::fset_to_store::WriteAttrs;
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
        let planes =
            collect_plane_stores(&fs, &dim, Encoding::FitsWcs, WriteAttrs::default()).unwrap();
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
        let planes =
            collect_plane_stores(&fs, &dim, Encoding::FitsWcs, WriteAttrs::default()).unwrap();
        assert_eq!(planes.len(), 2);
        assert_eq!(planes[0].letter, ' ');
        assert_eq!(planes[1].letter, 'A');
    }
}
