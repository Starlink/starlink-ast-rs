//! FITS-WCS encoder pipeline.
//!
//! Converts an AST FrameSet into FITS header cards by:
//! 1. Decomposing the FrameSet into a FitsStore (axis identification + linear extraction)
//! 2. Writing the FitsStore contents as FITS keyword cards
//!
//! This mirrors the C library's `FsetToStore()` → `WcsFromStore()` pipeline.

mod plane_stores;
pub(crate) use plane_stores::{PlaneStore, collect_plane_stores};

use super::card_list::{CardList, NewScope};
use super::fits_store::FitsStore;
use super::fset_to_store::WriteAttrs;
use super::wcs_decompose::split_cd;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{FrameRef, FrameSet, TimeScale};
use crate::mappings::helpers::ast_equal;
use crate::pal;
use crate::serial::AnyObject;

// ---------------------------------------------------------------------------
// Key-formatting helpers
// ---------------------------------------------------------------------------

/// Format an axis-indexed FITS key with optional version-letter suffix.
///
/// `s == ' '` returns the bare key (primary plane); other letters
/// append the suffix. Mirrors fitschan.c::FormatKey.
fn fmt_axis_key(name: &str, axis: usize, s: char) -> String {
    if s == ' ' {
        format!("{name}{axis}")
    } else {
        format!("{name}{axis}{s}")
    }
}

/// Format a matrix-element key (e.g. CD1_2A) with optional version letter.
fn fmt_matrix_key(name: &str, i: usize, j: usize, s: char) -> String {
    if s == ' ' {
        format!("{name}{i}_{j}")
    } else {
        format!("{name}{i}_{j}{s}")
    }
}

/// Format a non-axis FITS key (e.g. WCSAXES, RADESYS) with optional version letter.
fn fmt_global_key(name: &str, s: char) -> String {
    if s == ' ' {
        name.to_string()
    } else {
        format!("{name}{s}")
    }
}

/// Encode a FrameSet as FITS-WCS keywords into a CardList.
///
/// `dim` is the per-axis NAXIS{i} dimension array (in WCS axis order)
/// extracted from the input header. `None` entries indicate unknown
/// dimensions; the encoder uses `(1, …, 1)` as the FindBasisVectors
/// fallback in that case (mirrors fitschan.c:10526).
///
/// This is the top-level encoder entry point.
pub(crate) fn encode_wcs(
    obj: &AnyObject,
    cards: &mut CardList,
    use_cd: bool,
    dim: &[Option<f64>],
    attrs: WriteAttrs,
) -> AstResult<bool> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    // Extract primary CRVAL{i} values from the input cards. These
    // feed the per-axis CRVAL choice in `fset_to_store` (mirrors
    // C `ReadCrval` plumbed through `SpectralAxes` /
    // `CelestialAxes`). When a CRVAL is in the input, the writer
    // prefers it over the central-pixel fallback so that any
    // half-pixel-bumped CRPIX/CRVAL handling (`NearestPix`) sees
    // the user-supplied reference rather than a synthesised one.
    let nbase = frameset.base().naxes();
    let primary_crvals = extract_input_crvals(cards, ' ', nbase);

    let mut planes = collect_plane_stores(
        frameset,
        dim,
        crate::fitschan::Encoding::FitsWcs,
        &primary_crvals,
        attrs,
    )?;

    // Last-plane-wins: the alphabetically-last plane's global keyword
    // values (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DTAI, DUT1, etc.)
    // overwrite the primary's. Mirrors C's
    // `SetItem(&store->mjdobs, 0, 0, ' ', ...)` pattern at
    // fitschan.c:29805 — see FitsStore::mjdobs doc comment.
    propagate_last_plane_globals_into_primary(&mut planes);

    // Emit the primary plane (which now carries the last-plane
    // globals) followed by each alt plane (per-axis cards only —
    // their global emissions are gated by `s == ' '`).
    //
    // Each version either writes a complete axis description or none at
    // all: the cards a failed version emitted before the defect was
    // found are removed again, as C does with `FixNew( this, NEW2, !ok,
    // ... )` at the tail of every pass of its version loop
    // (fitschan.c:37798). An alternate description is only meaningful
    // alongside a primary one, so a failed primary abandons the whole
    // header rather than leaving alternates behind — C's `if( !primok )
    // break;` at fitschan.c:37812.
    let mut primok = false;
    let mut ret = false;
    for (i, plane) in planes.iter().enumerate() {
        let (s, frame_name) = if i == 0 {
            (' ', None)
        } else {
            (plane.letter, alt_plane_domain(frameset, plane.letter))
        };
        let ok = write_store_to_cards(&plane.store, cards, use_cd, s, frame_name.as_deref())?;
        if ok {
            ret = true;
        }
        if i == 0 {
            primok = ok;
        }
        cards.fix_new(NewScope::Version, !ok);
        if !primok {
            break;
        }
    }

    // SIP block, emitted only for a primary description that succeeded.
    // C emits these coefficients inside its version loop, before the
    // `FixNew` that discards a failed version's cards; here the block is
    // emitted after the loop because the alt-plane axis cards must land
    // between the primary axis block and the coefficients to match the
    // reference headers. Gating on `primok` gives the same outcome, since
    // only the primary plane carries SIP.
    if primok {
        if let Some(primary) = planes.first() {
            write_sip_block(primary.store.sip_a_order, &primary.store.sip_a, "A", cards);
            write_sip_block(primary.store.sip_b_order, &primary.store.sip_b, "B", cards);
            write_sip_block(
                primary.store.sip_ap_order,
                &primary.store.sip_ap,
                "AP",
                cards,
            );
            write_sip_block(
                primary.store.sip_bp_order,
                &primary.store.sip_bp,
                "BP",
                cards,
            );
        }
    }
    Ok(ret)
}

/// Propagate observation-global keyword values from the
/// alphabetically-last plane back into the primary `PlaneStore` so
/// that when `write_store_to_cards` emits the primary plane (with
/// `s = ' '`, which is the only branch that emits global cards), the
/// values reflect the C library's last-plane-wins overwrite rule.
///
/// Mirrors C `SkySys`'s hard-coded
/// `SetItem(&store->mjdobs, 0, 0, ' ', ...)` writes.
fn propagate_last_plane_globals_into_primary(planes: &mut [PlaneStore]) {
    if planes.len() <= 1 {
        return;
    }

    // MJD-OBS / DATE-OBS follow last-plane-ran-SkySys-wins semantics
    // (C fitschan.c:29807): SkySys calls SetItem(mjdobs, ' ', ep)
    // unconditionally on the !defdate branch — including with
    // AST__BAD when the plane has no Equinox. Find the last plane
    // whose `sky_sys` actually executed; whatever it computed
    // (Some(v) or None) is what primary should carry. Frames that
    // didn't run SkySys (non-SkyFrames, or SkySys's defdate branch)
    // leave the slot untouched.
    let last_mjdobs_plane = planes.iter().rposition(|p| p.store.mjdobs_skysys_ran);
    if let Some(idx) = last_mjdobs_plane {
        let mjdobs = planes[idx].store.mjdobs;
        let dateobs = planes[idx].store.dateobs.clone();
        planes[0].store.mjdobs = mjdobs;
        planes[0].store.dateobs = dateobs;
    }

    // Other observation-globals: last-plane-with-Some-value wins.
    // These come from SpectralAxes (which doesn't have the
    // AST__BAD-overwrite quirk) and don't need the SkySys-ran flag.
    fn last_some<T: Clone>(
        planes: &[PlaneStore],
        get: impl Fn(&FitsStore) -> Option<T>,
    ) -> Option<T> {
        for plane in planes.iter().rev() {
            if let Some(v) = get(&plane.store) {
                return Some(v);
            }
        }
        None
    }
    let obsgeox = last_some(planes, |s| s.obsgeox);
    let obsgeoy = last_some(planes, |s| s.obsgeoy);
    let obsgeoz = last_some(planes, |s| s.obsgeoz);
    let timesys = last_some(planes, |s| s.timesys.clone());
    let dtai = last_some(planes, |s| s.dtai);
    let dut1 = last_some(planes, |s| s.dut1);
    let primary = &mut planes[0].store;
    primary.obsgeox = obsgeox.or(primary.obsgeox);
    primary.obsgeoy = obsgeoy.or(primary.obsgeoy);
    primary.obsgeoz = obsgeoz.or(primary.obsgeoz);
    primary.timesys = timesys.or_else(|| primary.timesys.clone());
    primary.dtai = dtai.or(primary.dtai);
    primary.dut1 = dut1.or(primary.dut1);
}

/// Look up the domain string for the alt frame whose Ident-derived
/// version letter equals `letter`. Returns None for the primary plane,
/// for an empty domain, or for the per-class default domain (which C
/// suppresses via `astTestDomain` at fitschan.c:29959-29961).
///
/// Used to populate the per-version `WCSNAME{S}` cards.
fn alt_plane_domain(fs: &FrameSet, letter: char) -> Option<String> {
    if letter == ' ' {
        return None;
    }
    for (idx, l) in frames_with_version_letters(fs) {
        if l == letter
            && let Ok(frame) = fs.frame(FrameRef::Index(idx))
        {
            // Suppress the per-class default domain (the SkyFrame's
            // "SKY", SpecFrame's "SPECTRUM", TimeFrame's "TIME",
            // BasicFrame's empty default). C's `astTestDomain` returns
            // false for those.
            if !frame.test_domain() {
                return None;
            }
            let d = frame.domain();
            if !d.is_empty() {
                return Some(d.to_string());
            }
        }
    }
    None
}

/// Read the input chan's `CRVAL{i}{s}` cards into a per-axis array.
/// Mirrors fitschan.c::ReadCrval (line 25775+) — the C library extracts
/// these *before* per-axis processing so that `OtherAxes`/`SpectralAxes`
/// can adopt them as the WCS→IWC ShiftMap offsets, and `MakeIntWorld`
/// then back-solves CRPIX to match. Without an input chan (e.g. `.ast`
/// inputs), all entries are `None` and the writer falls back to
/// `world(g0)` per axis.
fn extract_input_crvals(cards: &CardList, s: char, naxis: usize) -> Vec<Option<f64>> {
    (0..naxis)
        .map(|i| {
            let key = if s == ' ' {
                format!("CRVAL{}", i + 1)
            } else {
                format!("CRVAL{}{s}", i + 1)
            };
            cards.get_float(&key)
        })
        .collect()
}

/// Encode a FrameSet via FitsStore with a custom card writer.
///
/// Used by alternative encodings that share the FrameSet→FitsStore
/// decomposition but write different keywords.
pub(crate) fn encode_wcs_to_store(
    obj: &AnyObject,
    cards: &mut CardList,
    dim: &[Option<f64>],
    attrs: WriteAttrs,
    writer: impl FnOnce(&FitsStore, &mut CardList) -> AstResult<()>,
) -> AstResult<bool> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    let input_crvals = extract_input_crvals(cards, ' ', frameset.base().naxes());
    let (store, ok) = store_from_frameset(frameset, dim, &input_crvals, attrs)?;
    // A FrameSet with no describable primary axes yields no store at all in
    // C, so `Write` has nothing to encode and reports failure
    // (fitschan.c:12358, 40716).
    if !ok {
        return Ok(false);
    }
    writer(&store, cards)?;
    Ok(true)
}

/// Variant of `encode_wcs_to_store` that drives a per-plane writer.
///
/// The writer callback is invoked once per plane in
/// `collect_plane_stores` order — primary first, then alt planes
/// alphabetically. The encoder's writer is responsible for
/// applying the version-letter suffix to per-axis card names; the
/// `is_last` flag tells the writer whether this plane is the
/// alphabetically-last one (so it should also emit observation-
/// global keywords like MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DTAI,
/// DUT1 — last-plane-wins per the C-library overwrite contract).
#[allow(dead_code)] // wired into encoders below in Tasks 5.2-5.5
pub(crate) fn encode_wcs_per_plane(
    obj: &AnyObject,
    cards: &mut CardList,
    dim: &[Option<f64>],
    encoding: crate::fitschan::Encoding,
    attrs: WriteAttrs,
    mut writer: impl FnMut(&FitsStore, &mut CardList, char, bool) -> AstResult<()>,
) -> AstResult<bool> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "Per-plane FITS encoding requires a FrameSet object",
            ));
        }
    };
    let primary_crvals = extract_input_crvals(cards, ' ', frameset.base().naxes());
    let mut planes = collect_plane_stores(frameset, dim, encoding, &primary_crvals, attrs)?;

    // Last-plane-wins for observation-global keywords (MJD-OBS,
    // DATE-OBS, OBSGEO, TIMESYS, DTAI, DUT1). Mirrors C's `SkySys`
    // contract — every plane's sky_sys writes unconditionally to
    // the primary slot, so the alphabetically-last plane's values
    // win even when the encoder is primary-axis-only.
    propagate_last_plane_globals_into_primary(&mut planes);

    // Restrict to the primary plane for encodings that the C library
    // documents as primary-axis-only. Mirrors the per-encoder header
    // notes:
    //   - FITS-IRAF: "Only primary axis descriptions are written
    //     out." (fitschan.c:18864)
    //   - FITS-PC:   "Only primary axis descriptions are written
    //     out." (fitschan.c:5xxx — same restriction)
    //   - FITS-AIPS / FITS-AIPS++: same.
    //   - FITS-CLASS: same.
    //
    // FITS-WCS is the only encoding that supports the full A..Z
    // letter loop, and it goes through `encode_wcs` (not this
    // helper).
    use crate::fitschan::Encoding;
    let primary_only = matches!(
        encoding,
        Encoding::FitsIraf
            | Encoding::FitsPc
            | Encoding::FitsAips
            | Encoding::FitsAipsPP
            | Encoding::FitsClass
    );

    // `collect_plane_stores` returns no plane at all when the primary axes
    // could not be described, which is C discarding the whole FitsStore
    // (fitschan.c:12358). `Write` then has nothing to encode and reports
    // failure (fitschan.c:40716).
    if primary_only {
        let Some(primary) = planes.first() else {
            return Ok(false);
        };
        // For primary-only encoders, the single plane is the
        // last (and only) plane — pass `is_last = true`.
        writer(&primary.store, cards, primary.letter, true)?;
        return Ok(true);
    }

    if planes.is_empty() {
        return Ok(false);
    }
    let last_idx = planes.len() - 1;
    for (i, plane) in planes.iter().enumerate() {
        let is_last = i == last_idx;
        writer(&plane.store, cards, plane.letter, is_last)?;
    }
    Ok(true)
}

/// Decompose a FrameSet into a FitsStore using the current frame as the
/// WCS target (pixel→current mapping).
///
/// `input_crvals` carries per-axis CRVAL{i} values from the input
/// chan (matching the primary plane); see [`extract_input_crvals`]
/// for the rationale.
pub(crate) fn store_from_frameset(
    fs: &FrameSet,
    dim: &[Option<f64>],
    input_crvals: &[Option<f64>],
    attrs: WriteAttrs,
) -> AstResult<(FitsStore, bool)> {
    store_from_frameset_to(fs, fs.current_idx(), dim, input_crvals, attrs)
}

/// Decompose a FrameSet into a FitsStore using `target_idx` (0-based) as
/// the WCS target frame (pixel→target_frame mapping).
///
/// `dim` carries per-axis NAXIS{i} values (or `None` if unknown); used
/// to choose the FindBasisVectors root grid position.
///
/// Mirrors fitschan.c::AddVersion (line 12241) which takes a frame index
/// and builds the axis/keyword store for that frame.
fn store_from_frameset_to(
    fs: &FrameSet,
    target_idx: usize,
    dim: &[Option<f64>],
    input_crvals: &[Option<f64>],
    attrs: WriteAttrs,
) -> AstResult<(FitsStore, bool)> {
    // The new path (fset_to_store) reads the FrameSet's current frame
    // directly; for alt-plane callers (target_idx != current_idx) we
    // clone the FrameSet and bump its current pointer to the requested
    // frame before dispatching. The legacy path used a `target_idx`
    // parameter to thread this through every helper; the new path
    // localises it here.
    //
    // `input_crvals` is forwarded to `fset_to_store` so the per-axis
    // CRVAL choice can prefer the input header's CRVAL over the
    // central-pixel fallback (mirrors C `ReadCrval` plumbed through
    // `SpectralAxes` / `CelestialAxes`, fitschan.c:2867+).
    let encoding = crate::fitschan::Encoding::FitsWcs;
    if target_idx == fs.current_idx() {
        return crate::fitschan::fset_to_store::fset_to_store(
            fs,
            dim,
            encoding,
            ' ',
            input_crvals,
            attrs,
        );
    }
    let mut alt_fs = fs.clone();
    alt_fs.set_current(FrameRef::Index(target_idx + 1))?;
    crate::fitschan::fset_to_store::fset_to_store(&alt_fs, dim, encoding, ' ', input_crvals, attrs)
}

/// Write FitsStore contents as FITS keyword cards.
///
/// `s` is the FITS version letter: `' '` = primary plane (bare keys),
/// `'A'`–`'Z'` = alt plane (suffixed keys). `frame_name` drives
/// WCSNAME{S} emission for alt planes; ignored for the primary plane.
fn write_store_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    use_cd: bool,
    s: char,
    frame_name: Option<&str>,
) -> AstResult<bool> {
    let naxis = store.naxis;

    // A version must have a complete axis description: without CRPIX1 and
    // CRVAL1 there is nothing for the other keywords to qualify, so the
    // whole version is abandoned. C tests this before emitting anything
    // (fitschan.c:37296-37305) and applies it to the primary as well as
    // the alternates.
    if naxis == 0 || store.crpix[0].is_none() || store.crval[0].is_none() {
        return Ok(false);
    }

    // WCSAXES{S} — mirrors fitschan.c:36987-37000. Emit when:
    //   1. the store explicitly carries a WCSAXES value (the
    //      MakeIntWorld degenerate-axis case at fitschan.c:22263), OR
    //   2. the input chan's NAXIS differs from `naxis` — including
    //      "no NAXIS at all" (e.g. `.ast` inputs whose chan starts
    //      empty, where the C library treats naxis as -1 and emits).
    let input_naxis = cards.get_int("NAXIS").map(|v| v as usize);
    let emit_wcsaxes = store.wcsaxes.is_some()
        || match input_naxis {
            Some(n) => n != naxis,
            None => true,
        };
    if emit_wcsaxes {
        let key = fmt_global_key("WCSAXES", s);
        cards.set_int(&key, naxis as i64, "Number of WCS axes");
    }

    // WCSNAME{S}: store.wcsname (set by `sky_sys` from the SkyFrame's
    // Domain) takes precedence over the FrameSet's frame_name. When
    // either is present, emit on both primary and alt planes. Mirrors
    // fitschan.c::WcsFromStore lines 37005-37011 — the C library
    // unconditionally emits WCSNAME (with a 1-axis special case to
    // suppress when WCSNAME == CTYPE1).
    let wcsname_to_emit: Option<&str> = store
        .wcsname
        .as_deref()
        .or_else(|| frame_name.filter(|n| !n.is_empty()));
    if let Some(name) = wcsname_to_emit {
        // 1-axis suppression: if naxis == 1 and CTYPE0 equals WCSNAME
        // (case-insensitive), the C library skips the redundant
        // WCSNAME card. Mirrors the `Similar` check at C:37008.
        let suppress = naxis == 1
            && store
                .ctype
                .first()
                .and_then(|c| c.as_deref())
                .is_some_and(|c| c.eq_ignore_ascii_case(name));
        if !suppress {
            cards.set_string(
                &fmt_global_key("WCSNAME", s),
                name,
                "Reference name for the coord. frame",
            );
        }
    }

    // CRPIXi{S}. Every axis must have one; C abandons the version
    // otherwise (fitschan.c:37341-37352).
    for i in 0..naxis {
        let Some(val) = store.crpix[i] else {
            return Ok(false);
        };
        let key = fmt_axis_key("CRPIX", i + 1, s);
        let comment = format!("Reference pixel on axis {}", i + 1);
        cards.set_float(&key, val, &comment);
    }

    // CRVALi{S}. Likewise required on every axis
    // (fitschan.c:37354-37366).
    for i in 0..naxis {
        let Some(val) = store.crval[i] else {
            return Ok(false);
        };
        let key = fmt_axis_key("CRVAL", i + 1, s);
        let comment = format!("Value at ref. pixel on axis {}", i + 1);
        cards.set_float(&key, val, &comment);
    }

    // CTYPEi{S}. If the projection is the AST-specific "TPN" code,
    // convert it to the standard FITS-WCS "TAN" and remember to emit
    // its projection parameters with the "QV" prefix instead of "PV".
    // AST does the inverse conversion on read; non-AST software ignores
    // the QV terms and reads a plain TAN projection. Mirrors
    // `WcsFromStore` (fitschan.c:37066-37077).
    //
    // A CTYPE is required on every axis; C abandons the version otherwise
    // (fitschan.c:37371-37377).
    let mut axis_par_qv = vec![false; naxis];
    for (i, qv) in axis_par_qv.iter_mut().enumerate() {
        let Some(ref val) = store.ctype[i] else {
            return Ok(false);
        };
        let mut out = val.clone();
        if val.len() >= 4 && val[val.len() - 4..].eq_ignore_ascii_case("-TPN") {
            let base = &val[..val.len() - 4];
            out = format!("{base}-TAN");
            *qv = true;
        }
        let key = fmt_axis_key("CTYPE", i + 1, s);
        let comment = store.ctype_com[i]
            .clone()
            .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
        cards.set_string(&key, &out, &comment);
    }

    // CD matrix or PC+CDELT.
    // Emitted before CUNIT to match the C library's WcsFromStore order
    // (fitschan.c:36867+: CRPIX, CRVAL, CTYPE, CNAME, CD, CDELT, PC, CUNIT).
    if use_cd {
        write_cd_matrix(store, cards, s)?;
    } else {
        write_pc_cdelt(store, cards, s)?;
    }

    // CUNITi{S}
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = fmt_axis_key("CUNIT", i + 1, s);
            let comment = format!("Units for axis {}", i + 1);
            cards.set_string(&key, val, &comment);
        }
    }

    // Plane-shared cards: only emit on primary plane.
    // Mirrors C library WcsFromStore order (fitschan.c:36867+):
    // CD matrix, then MJD-OBS / DATE-OBS, then RADESYS / EQUINOX.
    //
    // If the input chan supplied MJD-AVG, also preserve it: the C
    // library carries it through via ignore-used semantics, but our
    // sweep would drop it. We emit MJD-AVG before MJD-OBS so the
    // position matches the input (MJD-AVG was originally next to the
    // other time-related cards, before the encoder-added MJD-OBS).
    // Likewise for TIMESYS — the C library doesn't re-emit it but it
    // survives in the input chan.
    if s == ' ' {
        if cards.find("MJD-AVG").is_some()
            && let Some(mjdobs) = store.mjdobs
        {
            cards.set_float("MJD-AVG", mjdobs, "Average MJD of observation");
        }
        if let Some(ts) = cards.get_string("TIMESYS") {
            cards.set_string("TIMESYS", &ts, "Time scale");
        }
        if let Some(mjdobs) = store.mjdobs {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
        // DATE-OBS companion to MJD-OBS. Mirrors fitschan.c:3610-3613.
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
        // DTAI / DUT1: not emitted by FitsWcs. The C library reads these
        // when present (fitschan.c:36495-36506) but never writes them —
        // see fitschan.c:773-778 which explicitly notes "DUT1 is not
        // written out as yet when writing a FrameSet to a foreign FITS
        // header". The store carries them on the read path so that
        // round-tripping through SkyFrame/Frame attributes preserves
        // the values, but the writer suppresses them.
    }

    // PV{i}_{m} numerical projection parameters. Mirrors WcsFromStore
    // (fitschan.c:37257-37284) — emitted between TIMESYS/PV (if any)
    // and RADESYS/EQUINOX. The store's `pv` is keyed by 0-based axis
    // index, m index; FITS uses 1-based axis numbering.
    for (i, &qv) in axis_par_qv.iter().enumerate() {
        // TPN axes (rewritten to "TAN" above) emit their parameters with
        // the "QV" prefix; all other axes use "PV" (fitschan.c:37074,
        // 37278 `parprefix`).
        let prefix = if qv { "QV" } else { "PV" };
        for (m, val) in store.pv[i].iter().enumerate() {
            if let Some(v) = val {
                let key = if s == ' ' {
                    format!("{prefix}{}_{m}", i + 1)
                } else {
                    format!("{prefix}{}_{m}{s}", i + 1)
                };
                cards.set_float(&key, *v, "Projection parameter");
            }
        }
    }

    // Ancillary keywords. Emit BEFORE the SIP block so any new
    // cards (VELOSYS, MJD-OBS, ...) land between the per-axis WCS
    // section and the SIP coefficients, matching C's WcsFromStore
    // ordering (fitschan.c:37200-37348).
    if let Some(ref radesys) = store.radesys {
        cards.set_string(
            &fmt_global_key("RADESYS", s),
            radesys,
            "Reference frame for RA/DEC values",
        );
    }
    if let Some(equinox) = store.equinox {
        cards.set_float(
            &fmt_global_key("EQUINOX", s),
            equinox,
            "[yr] Epoch of reference equinox",
        );
    }
    // LONPOLE: only emit when non-default. FITS-WCS Paper II §2.4: the
    // default φp is 180° for zenithal projections with native fiducial
    // θ₀ ≥ 0° (TAN/SIN/ZEA/etc.). The C library suppresses the redundant
    // default value; emitting it would diverge from the canonical output.
    if let Some(lonpole) = store.lonpole {
        if lonpole != 180.0 {
            cards.set_float(
                &fmt_global_key("LONPOLE", s),
                lonpole,
                "Native longitude of celestial pole",
            );
        }
    }
    if let Some(latpole) = store.latpole {
        cards.set_float(
            &fmt_global_key("LATPOLE", s),
            latpole,
            "Native latitude of celestial pole",
        );
    }
    // Spectral block. Mirrors fitschan.c::WcsFromStore order
    // (C:37325-37361): SPECSYS, SSYSSRC, ZSOURCE, VELOSYS, RESTFRQ,
    // RESTWAV, IMAGFREQ.
    if let Some(ref specsys) = store.specsys {
        cards.set_string(
            &fmt_global_key("SPECSYS", s),
            specsys,
            "Standard of rest for spectral axis",
        );
    }
    if let Some(ref ssyssrc) = store.ssyssrc {
        cards.set_string(
            &fmt_global_key("SSYSSRC", s),
            ssyssrc,
            "Standard of rest for source redshift",
        );
    }
    if let Some(zsource) = store.zsource {
        cards.set_float(
            &fmt_global_key("ZSOURCE", s),
            zsource,
            "[] Redshift of source",
        );
    }
    if let Some(velosys) = store.velosys {
        cards.set_float(
            &fmt_global_key("VELOSYS", s),
            velosys,
            "[m/s] Topo. apparent velocity of rest frame",
        );
    }
    if let Some(restfrq) = store.restfrq {
        cards.set_float(
            &fmt_global_key("RESTFRQ", s),
            restfrq,
            "[Hz] Rest frequency",
        );
    }
    if let Some(restwav) = store.restwav {
        cards.set_float(
            &fmt_global_key("RESTWAV", s),
            restwav,
            "[m] Rest wavelength",
        );
    }

    // IMAGFREQ — DSB image-sideband frequency (AST extension, JACH).
    // Mirrors C:37358-37361. Primary-only in the C source (no
    // FormatKey suffix variant exists on this card).
    if s == ' ' {
        if let Some(imagfreq) = store.imagfreq {
            cards.set_float("IMAGFREQ", imagfreq, "[Hz] Image frequency");
        }
    }

    // OBSGEO-X/Y/Z — observer geocentric coordinates. FITS-WCS
    // standard (Calabretta+ 2002 §3.1). Always primary-only — these
    // describe the observatory, not a coord-version property.
    // Mirrors C:37363-37372.
    if s == ' ' {
        if let Some(x) = store.obsgeox {
            cards.set_float("OBSGEO-X", x, "[m] Observatory geocentric X");
        }
        if let Some(y) = store.obsgeoy {
            cards.set_float("OBSGEO-Y", y, "[m] Observatory geocentric Y");
        }
        if let Some(z) = store.obsgeoz {
            cards.set_float("OBSGEO-Z", z, "[m] Observatory geocentric Z");
        }
    }

    // SREFIS / SREF{i} / SREFP{i} — AST-extension SkyFrame
    // offset-coords metadata. Note the AST keyword names (8-char
    // budget: SREFIS / SREF / SREFP) — not SKYREFIS / SKYREF /
    // SKYREFP. Mirrors C:37185-37216.
    if let Some(ref skyrefis) = store.skyrefis {
        cards.set_string(
            &fmt_global_key("SREFIS", s),
            skyrefis,
            "Is SkyRef used as pole or origin?",
        );
    }
    for i in 0..naxis {
        if let Some(v) = store.skyref[i] {
            let key = fmt_axis_key("SREF", i + 1, s);
            let comment = format!("Sky reference position on axis {}", i + 1);
            cards.set_float(&key, v, &comment);
        }
    }
    for i in 0..naxis {
        if let Some(v) = store.skyrefp[i] {
            let key = fmt_axis_key("SREFP", i + 1, s);
            let comment = format!("Sky primary meridian position on axis {}", i + 1);
            cards.set_float(&key, v, &comment);
        }
    }

    // SIP distortion coefficients are emitted by the caller after
    // the per-plane axis loop (so alt-plane axis cards land between
    // the primary axis block and the SIP coefficient block, matching
    // C `WcsFromStore`'s tail emission at fitschan.c:37378-37413).

    Ok(true)
}

/// Write CD matrix keywords.
fn write_cd_matrix(store: &FitsStore, cards: &mut CardList, s: char) -> AstResult<()> {
    let naxis = store.naxis;
    for i in 0..naxis {
        for j in 0..naxis {
            if let Some(val) = store.cd[i][j] {
                if val != 0.0 || i == j {
                    let key = fmt_matrix_key("CD", i + 1, j + 1, s);
                    cards.set_float(&key, val, "Transformation matrix element");
                }
            }
        }
    }
    Ok(())
}

/// Emit one SIP coefficient block (A_*, B_*, AP_*, BP_*).
///
/// Mirrors `SIPIntWorld`'s store loops (fitschan.c:28939-28986) and
/// the C writer at fitschan.c:37378-37413. The store table is
/// initialised with `AST__BAD` for every (i, jm) pair `≤ (imax,
/// jmmax)`, then real coefficients overwrite some slots. The C
/// `WcsFromStore` writer iterates `p ∈ [0, pmax]` × `q ∈ [0, qmax]`
/// and emits a card only when `GetItem` returns a non-`AST__BAD`
/// value. The order keyword `<prefix>_ORDER` is emitted only when
/// at least one good coefficient was emitted, with the value being
/// the maximum `p + q` across emitted cards.
///
/// The comments are those of C's `WcsFromStore`: "SIP forward distortion
/// coeff" and "SIP max order" for the forward blocks, "SIP inverse
/// distortion coeff" and "SIP inverse max order" for the inverse ones.
fn write_sip_block(order: Option<usize>, coeffs: &[Vec<f64>], prefix: &str, cards: &mut CardList) {
    let Some(_order) = order else { return };
    if coeffs.is_empty() {
        return;
    }
    let (order_comment, coeff_comment) = match prefix {
        "AP" | "BP" => ("SIP inverse max order", "SIP inverse distortion coeff"),
        _ => ("SIP max order", "SIP forward distortion coeff"),
    };

    let pmax = coeffs.len().saturating_sub(1);
    let qmax = coeffs
        .iter()
        .map(|r| r.len())
        .max()
        .unwrap_or(0)
        .saturating_sub(1);

    let mut emitted_order: usize = 0;
    let mut any = false;
    for p in 0..=pmax {
        let row = match coeffs.get(p) {
            Some(r) => r,
            None => continue,
        };
        for q in 0..=qmax {
            let val = row.get(q).copied().unwrap_or(BAD_VALUE);
            if val == BAD_VALUE {
                continue;
            }
            cards.set_float(&format!("{prefix}_{p}_{q}"), val, coeff_comment);
            if p + q > emitted_order {
                emitted_order = p + q;
            }
            any = true;
        }
    }
    if any {
        cards.set_int(
            &format!("{prefix}_ORDER"),
            emitted_order as i64,
            order_comment,
        );
    }
}

/// Write PC matrix + CDELT keywords.
fn write_pc_cdelt(store: &FitsStore, cards: &mut CardList, s: char) -> AstResult<()> {
    let naxis = store.naxis;

    let mut cd_array = ndarray::Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cd_array[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }
    let (cdelt, pc) = split_cd(&cd_array);

    // CDELTi{S}
    for (i, &val) in cdelt.iter().enumerate() {
        let key = fmt_axis_key("CDELT", i + 1, s);
        cards.set_float(&key, val, &format!("Pixel size on axis {}", i + 1));
    }

    // PCi_j{S}, omitting elements equal to the unit matrix's
    // (fitschan.c `WcsFromStore`).
    for i in 0..naxis {
        for j in 0..naxis {
            let default = if i == j { 1.0 } else { 0.0 };
            if !ast_equal(pc[[i, j]], default) {
                let key = fmt_matrix_key("PC", i + 1, j + 1, s);
                cards.set_float(&key, pc[[i, j]], "Transformation matrix element");
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Calendar helpers
// ---------------------------------------------------------------------------

/// Difference TDB − TT in seconds at the geocentre.
///
/// SkyFrame.Epoch in the C library is stored on the TDB scale (see
/// fitschan.c:29718 "The epoch is in TDB"). TDB and TT differ by a
/// small periodic offset of amplitude ~1.66 ms — comparable to FITS
/// DATE-OBS millisecond precision, so it cannot be ignored.
///
/// Delegates to [`pal::dtdb_geocentre`], which wraps ERFA's full
/// Fairhead & Bretagnon (1990) series (≤±3 ns absolute, 1950-2050).
/// AST's `timemap.c::Rcc` is the same algorithm.
pub(crate) fn tdb_minus_tt_seconds(mjd: f64) -> f64 {
    pal::dtdb_geocentre(mjd)
}

/// Convert TDB (Barycentric Dynamical Time) MJD to UTC MJD.
///
/// Mirrors `fitschan.c::TDBConv` (lines 33894-33976): build TDB and
/// UTC `TimeFrame`s and use `astConvert` to compute the mapping
/// between them — exactly like the C library's `TDBConv` helper.
pub(crate) fn tdb_mjd_to_utc(tdb_mjd: f64) -> f64 {
    convert_time_mjd(tdb_mjd, TimeScale::Tdb, TimeScale::Utc)
}

/// Convert UTC MJD to TDB (Barycentric Dynamical Time) MJD.
///
/// Mirrors `fitschan.c::TDBConv` for the UTC→TDB direction.
pub(crate) fn utc_mjd_to_tdb(utc_mjd: f64) -> f64 {
    convert_time_mjd(utc_mjd, TimeScale::Utc, TimeScale::Tdb)
}

/// Generic timescale conversion via the `TimeFrame` `astConvert`
/// pathway. Both source and destination are MJD; only the timescale
/// differs. Returns the input unchanged on conversion failure (the
/// C library reports an error in that case; we fall through to the
/// raw value, which is the historic Rust behaviour and gives the
/// nearest sensible fallback for tests).
fn convert_time_mjd(mjd: f64, from: TimeScale, to: TimeScale) -> f64 {
    use crate::frames::{Frame, TimeFrame, TimeSystem};
    // An MJD TimeFrame with no TimeOrigin accepts any timescale.
    let mut from_tf = TimeFrame::new(TimeSystem::Mjd);
    from_tf
        .set_timescale(from)
        .expect("an MJD TimeFrame accepts any timescale");
    let mut to_tf = TimeFrame::new(TimeSystem::Mjd);
    to_tf
        .set_timescale(to)
        .expect("an MJD TimeFrame accepts any timescale");
    let Ok(Some(fs)) = Frame::convert_to(&from_tf, &to_tf, &[]) else {
        return mjd;
    };
    let Ok(mapping) = fs.mapping(
        crate::frames::FrameRef::Base,
        crate::frames::FrameRef::Current,
    ) else {
        return mjd;
    };
    let pts = crate::coords::PointSet::from_rows([[mjd]]);
    match mapping.transform(&pts, true) {
        Ok(out) => {
            let v = out.point(0)[0];
            if v.is_finite() { v } else { mjd }
        }
        Err(_) => mjd,
    }
}

/// Decompose an MJD (UTC, Modified Julian Date) into a Gregorian
/// calendar date + time tuple `(year, month, day, hour, min, sec_frac)`.
///
/// Uses the standard Meeus algorithm (Astronomical Algorithms, §7).
/// MJD's day boundary is at midnight (JD's day boundary is at noon, so
/// the algorithm shifts by 0.5).
#[allow(dead_code)] // wired into the writer's DATE-OBS path in Group 6
pub(crate) fn mjd_to_gregorian(mjd: f64) -> (i32, u32, u32, u32, u32, f64) {
    let jd = mjd + 2_400_000.5_f64;
    let z = (jd + 0.5).floor() as i64;
    let frac = jd + 0.5 - z as f64; // 0.0 = midnight

    // Gregorian calendar conversion (Meeus §7).
    let a = if z < 2_299_161 {
        z
    } else {
        let alpha = ((z as f64 - 1_867_216.25) / 36_524.25).floor() as i64;
        z + 1 + alpha - alpha / 4
    };
    let b = a + 1524;
    let c = ((b as f64 - 122.1) / 365.25).floor() as i64;
    let d = (365.25 * c as f64).floor() as i64;
    let e = ((b - d) as f64 / 30.6001).floor() as i64;

    let day = (b - d - (30.6001 * e as f64).floor() as i64) as u32;
    let month = if e < 14 {
        (e - 1) as u32
    } else {
        (e - 13) as u32
    };
    let year = if month > 2 {
        (c - 4716) as i32
    } else {
        (c - 4715) as i32
    };

    // Time of day from fractional day.
    let total_secs = frac * 86400.0;
    let hour = (total_secs / 3600.0).floor() as u32;
    let rem = total_secs - hour as f64 * 3600.0;
    let min = (rem / 60.0).floor() as u32;
    let sec_frac = rem - min as f64 * 60.0;

    (year, month, day, hour, min, sec_frac)
}

/// Format an MJD (UTC) as a FITS DATE-OBS string.
///
/// Mirrors fitschan.c:3594-3608. Cutoff is MJD 51179 (1999-01-01);
/// modern dates use ISO-8601 `YYYY-MM-DDTHH:MM:SS.SSS`, legacy dates
/// use `DD/MM/YY`.
#[allow(dead_code)] // wired into the writer's DATE-OBS path in Group 6
pub(crate) fn mjd_to_date_str(mjd: f64) -> String {
    const CUTOFF_MJD: f64 = 51179.0; // palCaldj(99, 1, 1)
    let (year, month, day, hour, min, sec_frac) = mjd_to_gregorian(mjd);
    if mjd >= CUTOFF_MJD {
        // Modern ISO-8601, 3-decimal-place seconds.
        let int_sec = sec_frac.floor() as u32;
        let ms = ((sec_frac - int_sec as f64) * 1000.0).round() as u32;
        // Handle rounding overflow (e.g. 59.9996 → ms=1000). Roll up.
        let (int_sec, ms, min, hour, day, month, year) = if ms == 1000 {
            // Defer: simplest is to add 1 second and re-format.
            let mjd_bumped = mjd + 1.0 / 86400.0;
            let (y2, mo2, d2, h2, mn2, sf2) = mjd_to_gregorian(mjd_bumped);
            let s2 = sf2.floor() as u32;
            (s2, 0, mn2, h2, d2, mo2, y2)
        } else {
            (int_sec, ms, min, hour, day, month, year)
        };
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{int_sec:02}.{ms:03}")
    } else {
        // Legacy DD/MM/YY: two-digit year (year % 100 with century strip).
        let yy = year.unsigned_abs() % 100;
        format!("{day:02}/{month:02}/{yy:02}")
    }
}

/// Parse a FITS DATE-OBS string back to UTC MJD.
///
/// Inverse of [`mjd_to_date_str`]. Accepts:
/// - ISO-8601 `YYYY-MM-DDTHH:MM:SS[.SSS]` (modern format)
/// - ISO-8601 `YYYY-MM-DD` (date-only; assumes 00:00:00)
/// - Legacy `DD/MM/YY` (assumes 00:00:00; 2-digit year mapped to
///   1900–1999 if YY >= 50, else 2000–2049)
///
/// Returns `None` if the string doesn't parse.
///
/// Mirrors fitschan.c::DateObs at line 8352.
pub(crate) fn date_str_to_mjd(s: &str) -> Option<f64> {
    let s = s.trim();
    // Legacy DD/MM/YY format.
    if let Some(parts) = parse_legacy_date(s) {
        let (year, month, day) = parts;
        return gregorian_to_mjd(year, month, day, 0, 0, 0, "");
    }
    // ISO-8601: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS[.SSS][Z]
    let (date_part, time_part) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let date_segs: Vec<&str> = date_part.split('-').collect();
    if date_segs.len() != 3 {
        return None;
    }
    let year: i32 = date_segs[0].parse().ok()?;
    let month: u32 = date_segs[1].parse().ok()?;
    let day: u32 = date_segs[2].parse().ok()?;

    let (hour, min, sec, frac_digits) = if let Some(tp) = time_part {
        let tp = tp.trim_end_matches('Z');
        let time_segs: Vec<&str> = tp.split(':').collect();
        if time_segs.len() != 3 {
            return None;
        }
        let h: u32 = time_segs[0].parse().ok()?;
        let m: u32 = time_segs[1].parse().ok()?;
        // Keep the whole and fractional seconds apart. C scans them as a
        // `%2d` and a separate `%d` whose field width comes from the
        // bracketing `%n` markers, then recombines them arithmetically; see
        // gregorian_to_mjd.
        let (whole, frac) = match time_segs[2].split_once('.') {
            Some((w, f)) => (w, f),
            None => (time_segs[2], ""),
        };
        let s: u32 = whole.parse().ok()?;
        (h, m, s, frac)
    } else {
        (0, 0, 0, "")
    };

    gregorian_to_mjd(year, month, day, hour, min, sec, frac_digits)
}

fn parse_legacy_date(s: &str) -> Option<(i32, u32, u32)> {
    let segs: Vec<&str> = s.split('/').collect();
    if segs.len() != 3 {
        return None;
    }
    let day: u32 = segs[0].parse().ok()?;
    let month: u32 = segs[1].parse().ok()?;
    let yy: i32 = segs[2].parse().ok()?;
    if !(0..=99).contains(&yy) {
        return None;
    }
    let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
    Some((year, month, day))
}

/// Convert (Gregorian Y/M/D + H/M/S) to MJD, following the arithmetic of
/// `fitschan.c::DateObs` (fitschan.c:8512-8539) exactly.
///
/// `frac_digits` is the run of digits after the decimal point in the seconds
/// field, as written; its length is significant because it supplies the power
/// of ten, so `"087"` gives 0.087 rather than 0.87.
///
/// Two details carry the precision, and departing from either costs about a
/// ULP in the resulting MJD:
///
/// - The day number and the time of day are combined at MJD magnitude
///   (~5e4), not Julian Date magnitude (~2.45e6). Adding the day fraction to
///   a JD first and subtracting the 2400000.5 offset afterwards rounds the
///   fraction onto a grid ~64x coarser than the MJD's own.
/// - The seconds are rebuilt as `fsc / 10^width + sc`, which is not
///   bit-identical to parsing `"SS.fff"` as one number: the two disagree for
///   about 1.6% of second/fraction pairs.
fn gregorian_to_mjd(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    frac_digits: &str,
) -> Option<f64> {
    // C rejects the date on palCaldj's status rather than range-checking the
    // fields itself; palCldj is the same routine for a full year number (the
    // only difference, palCaldj's 2-digit-year window, is applied by
    // parse_legacy_date before we get here).
    let (mjd0, j) = crate::pal::cldj(year, month as i32, day as i32);
    if j != 0 {
        return None;
    }

    // C: secs = fsc / pow( 10.0, width ); secs += sc.
    let mut secs = 0.0;
    if !frac_digits.is_empty() {
        if !frac_digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // C scans this with "%d" into an int, so anything past ~10 digits has
        // already overflowed there; u64 covers every width C handles.
        let fsc: u64 = frac_digits.parse().ok()?;
        if fsc > 0 {
            secs = fsc as f64 / 10.0_f64.powi(frac_digits.len() as i32);
        }
    }
    secs += f64::from(sec);

    let (days, j) = crate::pal::dtf2d(hour as i32, min as i32, secs);
    if j != 0 {
        return None;
    }
    Some(mjd0 + days)
}

/// Identify which frames in the FrameSet are eligible for FITS alt-plane
/// emission, and assign each a version letter ('A'..='Z').
///
/// Mirrors the loop at fitschan.c:12172-12230:
///   - Skip base and current frames (they're emitted as the primary plane).
///   - Skip frames with domain "IWC" (intermediate world coords; not externalised).
///   - First pass: if a frame's `ident()` is a single uppercase letter and
///     not yet used, use it.
///   - Second pass: assign remaining letters A, B, C, ... in frame-index
///     order, skipping any already used in the first pass.
///
/// Returns a Vec of (1-based frame index, version letter) pairs.
pub(crate) fn frames_with_version_letters(fs: &FrameSet) -> Vec<(usize, char)> {
    let nframe = fs.nframe();
    let base_idx = fs.base_idx() + 1; // 1-based to match C
    let curr_idx = fs.current_idx() + 1;

    let mut used = [false; 26]; // index 0 = 'A', 25 = 'Z'
    let mut assignment: Vec<(usize, Option<char>)> = Vec::new();

    // First pass: explicit Ident.
    for ifrm in 1..=nframe {
        if ifrm == base_idx || ifrm == curr_idx {
            continue;
        }
        let frame = match fs.frame(FrameRef::Index(ifrm)) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if frame.domain().eq_ignore_ascii_case("IWC") {
            continue;
        }
        let ident = frame.ident();
        let letter = if ident.len() == 1 {
            let c = ident.chars().next().unwrap();
            if c.is_ascii_uppercase() && !used[(c as u8 - b'A') as usize] {
                used[(c as u8 - b'A') as usize] = true;
                Some(c)
            } else {
                None
            }
        } else {
            None
        };
        assignment.push((ifrm, letter));
    }

    // Second pass: assign remaining letters in order.
    let mut next = b'A';
    for (_, opt_letter) in assignment.iter_mut() {
        if opt_letter.is_none() {
            while next <= b'Z' && used[(next - b'A') as usize] {
                next += 1;
            }
            if next > b'Z' {
                break; // out of letters; remaining frames don't get planes
            }
            *opt_letter = Some(next as char);
            used[(next - b'A') as usize] = true;
            next += 1;
        }
    }

    assignment
        .into_iter()
        .filter_map(|(idx, opt)| opt.map(|c| (idx, c)))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::card_list::CardList;

    /// Helper: build a minimal store with the AST-extension fields
    /// populated and emit it via `write_store_to_cards`.
    fn write_extension_keywords(s: char) -> CardList {
        let mut store = FitsStore::new(2);
        // Required for the ID/CTYPE bookkeeping in write_store_to_cards.
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(0.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        // AST-extension fields under test.
        store.wcsname = Some("MYDOMAIN".to_string());
        store.obsgeox = Some(1234567.0);
        store.obsgeoy = Some(2345678.0);
        store.obsgeoz = Some(3456789.0);
        store.ssyssrc = Some("LSRK".to_string());
        store.zsource = Some(0.025);
        store.imagfreq = Some(2.3e11);
        store.skyrefis = Some("ORIGIN".to_string());
        store.skyref[0] = Some(1.5);
        store.skyref[1] = Some(0.25);
        store.skyrefp[0] = Some(1.6);
        store.skyrefp[1] = Some(0.5);

        let mut cards = CardList::new();
        write_store_to_cards(&store, &mut cards, false, s, None).expect("write");
        cards
    }

    #[test]
    fn primary_plane_emits_ast_extension_keywords() {
        let cards = write_extension_keywords(' ');

        assert_eq!(cards.get_string("WCSNAME").as_deref(), Some("MYDOMAIN"));
        assert!((cards.get_float("OBSGEO-X").unwrap() - 1234567.0).abs() < 1e-9);
        assert!((cards.get_float("OBSGEO-Y").unwrap() - 2345678.0).abs() < 1e-9);
        assert!((cards.get_float("OBSGEO-Z").unwrap() - 3456789.0).abs() < 1e-9);
        assert_eq!(cards.get_string("SSYSSRC").as_deref(), Some("LSRK"));
        assert!((cards.get_float("ZSOURCE").unwrap() - 0.025).abs() < 1e-12);
        assert!((cards.get_float("IMAGFREQ").unwrap() - 2.3e11).abs() < 1.0);
        assert_eq!(cards.get_string("SREFIS").as_deref(), Some("ORIGIN"));
        assert!((cards.get_float("SREF1").unwrap() - 1.5).abs() < 1e-12);
        assert!((cards.get_float("SREF2").unwrap() - 0.25).abs() < 1e-12);
        assert!((cards.get_float("SREFP1").unwrap() - 1.6).abs() < 1e-12);
        assert!((cards.get_float("SREFP2").unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn alt_plane_emits_suffixed_versions_and_no_obsgeo() {
        let cards = write_extension_keywords('A');

        // Per-plane keys: suffix 'A'.
        assert_eq!(cards.get_string("WCSNAMEA").as_deref(), Some("MYDOMAIN"));
        assert_eq!(cards.get_string("SSYSSRCA").as_deref(), Some("LSRK"));
        assert!(cards.get_float("ZSOURCEA").is_some());
        assert_eq!(cards.get_string("SREFISA").as_deref(), Some("ORIGIN"));
        assert!(cards.get_float("SREF1A").is_some());
        assert!(cards.get_float("SREF2A").is_some());
        assert!(cards.get_float("SREFP1A").is_some());
        assert!(cards.get_float("SREFP2A").is_some());

        // OBSGEO-{X,Y,Z} and IMAGFREQ are primary-only — not emitted on alt.
        assert!(cards.find("OBSGEO-X").is_none());
        assert!(cards.find("OBSGEO-Y").is_none());
        assert!(cards.find("OBSGEO-Z").is_none());
        assert!(cards.find("IMAGFREQ").is_none());
    }
    #[test]
    fn mjd_to_gregorian_y2k() {
        // 2000-01-01 00:00:00 UTC = MJD 51544.0
        let (y, mo, d, h, mn, s) = mjd_to_gregorian(51544.0);
        assert_eq!(y, 2000);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mn, 0);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn mjd_to_gregorian_j2000_noon() {
        // 2000-01-01 12:00:00 UTC = MJD 51544.5
        let (y, mo, d, h, mn, s) = mjd_to_gregorian(51544.5);
        assert_eq!((y, mo, d, h, mn), (2000, 1, 1, 12, 0));
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn mjd_to_gregorian_pre_1900() {
        // 1858-11-17 00:00:00 UTC = MJD 0
        let (y, mo, d, _, _, _) = mjd_to_gregorian(0.0);
        assert_eq!((y, mo, d), (1858, 11, 17));
    }

    #[test]
    fn mjd_to_date_str_modern_iso() {
        // MJD ≥ 51179 (1999-01-01) → ISO-8601 with millisecond precision.
        let s = mjd_to_date_str(51544.5);
        assert_eq!(s, "2000-01-01T12:00:00.000");
    }

    #[test]
    fn mjd_to_date_str_modern_with_ms_precision() {
        let s = mjd_to_date_str(52414.59);
        assert_eq!(&s[0..10], "2002-05-20");
        assert!(s.starts_with("2002-05-20T"));
        assert_eq!(&s[11..16], "14:09");
    }

    #[test]
    fn mjd_to_date_str_legacy_format() {
        // MJD < 51179 (1999-01-01) → DD/MM/YY.
        // MJD 40000 = 1968-05-24
        let s = mjd_to_date_str(40000.0);
        assert_eq!(s, "24/05/68");
    }

    #[test]
    fn mjd_to_date_str_cutoff_boundary() {
        // MJD 51179 = 1999-01-01 → modern format.
        let s = mjd_to_date_str(51179.0);
        assert!(s.starts_with("1999-01-01T"));

        // MJD 51178 = 1998-12-31 → legacy format.
        let s = mjd_to_date_str(51178.0);
        assert_eq!(s, "31/12/98");
    }

    #[test]
    fn date_str_to_mjd_iso_modern() {
        let mjd = date_str_to_mjd("2000-01-01T12:00:00.000").unwrap();
        assert!((mjd - 51544.5).abs() < 1e-6);
    }

    /// The MJD must be bit-identical to C's `DateObs`, which composes it as
    /// `palCaldj` (whole days) + `palDtf2d` (day fraction) and builds the
    /// seconds as `fsc / 10^width + sc`. These are the DATE-OBS strings from
    /// the `azel`, `date-frac` and `serpens` wcsconv fixtures; the expected
    /// values were taken from the C library on the same inputs.
    #[test]
    fn date_str_to_mjd_matches_c_bit_for_bit() {
        for (text, want) in [
            ("2017-07-21T08:38:39.087", 57955.36017461806_f64),
            ("2020-06-15T10:30:00.123Z", 59015.437501423614_f64),
            ("2007-04-30T15:13:53.717", 54220.63464950232_f64),
        ] {
            let got = date_str_to_mjd(text).expect("parses");
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{text}: got {got:.17}, want {want:.17}"
            );
        }
    }

    /// A fractional-seconds field keeps its leading zeros: the digit count is
    /// the power of ten, so ".087" is 87 milliseconds, not 870.
    #[test]
    fn date_str_to_mjd_fraction_width_is_significant() {
        // Differencing against the day number quantizes the residue to a few
        // times 1e-7 s, so this checks the scale, not the last bits; the
        // bit-exactness of the whole value is pinned by the test above.
        let a = (date_str_to_mjd("2000-01-01T00:00:00.087").unwrap() - 51544.0) * 86400.0;
        let b = (date_str_to_mjd("2000-01-01T00:00:00.87").unwrap() - 51544.0) * 86400.0;
        assert!((a - 0.087).abs() < 1e-6, "{a}");
        assert!((b - 0.87).abs() < 1e-6, "{b}");
    }

    #[test]
    fn date_str_to_mjd_iso_no_time() {
        let mjd = date_str_to_mjd("2000-01-01").unwrap();
        assert!((mjd - 51544.0).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_iso_with_trailing_z() {
        let mjd = date_str_to_mjd("2000-01-01T12:00:00.000Z").unwrap();
        assert!((mjd - 51544.5).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_legacy() {
        // 24/05/68 → 1968-05-24 = MJD 40000
        let mjd = date_str_to_mjd("24/05/68").unwrap();
        assert!((mjd - 40000.0).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_roundtrip_modern() {
        let original = 52414.59f64;
        let s = mjd_to_date_str(original);
        let parsed = date_str_to_mjd(&s).unwrap();
        assert!((parsed - original).abs() < 1e-3);
    }

    #[test]
    fn date_str_to_mjd_roundtrip_legacy() {
        let original = 40000.0f64;
        let s = mjd_to_date_str(original);
        let parsed = date_str_to_mjd(&s).unwrap();
        assert!((parsed - original).abs() < 1e-6);
    }

    #[test]
    fn utc_tdb_roundtrip() {
        let utc = 51544.0_f64;
        let tdb = utc_mjd_to_tdb(utc);
        let utc_back = tdb_mjd_to_utc(tdb);
        assert!((utc - utc_back).abs() < 1e-12);
    }

    #[test]
    fn tdb_tt_principal_term_amplitude() {
        // The Fairhead-Bretagnon principal term of TDB-TT has amplitude
        // ~1.66 ms. Sample one year of MJDs and check the bound holds.
        let mut max_abs: f64 = 0.0;
        for d in 0..365 {
            let mjd = 52000.0 + d as f64;
            max_abs = max_abs.max(tdb_minus_tt_seconds(mjd).abs());
        }
        assert!(max_abs > 0.0015, "amplitude too small: {max_abs}");
        assert!(max_abs < 0.002, "amplitude too large: {max_abs}");
    }

    #[test]
    fn frames_with_version_letters_timj() {
        use crate::serial::AnyObject;
        // timj.ast has 4 frames: GRID(base), PIXEL, AXIS, SKY(current).
        // Expect: PIXEL → A, AXIS → B (both letters, in frame-index order).
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/wcsconv/inputs/timj.ast"
        ))
        .expect("read timj.ast");
        let obj = crate::serial::from_native_string(&text).expect("parse timj");
        let fs = match obj {
            AnyObject::FrameSet(fs) => fs,
            _ => panic!("expected FrameSet"),
        };
        let assignments = frames_with_version_letters(&fs);
        assert!(
            !assignments.is_empty(),
            "timj should have at least 1 alt plane, got {}",
            assignments.len()
        );
        // First two letters should be A then B (or whichever first two
        // alphabetically given Ident may override).
        let letters: Vec<char> = assignments.iter().map(|(_, c)| *c).collect();
        eprintln!("timj alt assignments: {:?}", assignments);
        assert!(
            letters.contains(&'A') || letters.contains(&'B'),
            "letters: {:?}",
            letters
        );
    }
}
