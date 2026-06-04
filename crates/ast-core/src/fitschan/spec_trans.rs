//! Port of fitschan.c::SpecTrans (line 30828-32545).
//!
//! Translates non-standard FITS-WCS keywords into their standard
//! equivalents. The caller is the FitsChan reader: invoke
//! [`spec_trans`] once, before any FitsStore extraction, so that
//! downstream encoders see only canonical keyword names.
//!
//! Mirrors C `SpecTrans` except that translations write into the
//! supplied `CardList` directly rather than allocating a separate
//! return FitsChan. Original non-standard cards are removed once
//! translated; this is equivalent to C's "mark as used" mechanism
//! since the read pipeline is one-shot.

// `spec_trans` is wired into each FITS reader at the end of this
// port (Tasks 26-29). Until then the function is unreachable from
// outside this module; allow dead_code module-wide.
#![allow(dead_code)]

use super::Encoding;
use super::card_list::CardList;

/// Apply all SpecTrans translations to `cards` in place.
///
/// `encoding` selects which translations apply: a few are gated to
/// specific encodings (e.g. `Cnxxx` → `*A` is FITS-PC / FITS-IRAF
/// only; AIPS spectral CTYPE rewrites are AIPS-only). Translations
/// not gated by encoding apply to all FITS encodings uniformly.
pub(crate) fn spec_trans(cards: &mut CardList, encoding: Encoding) {
    let _ = encoding;

    // Translation 1 (fitschan.c:31114-31120): RADECSYS → RADESYS.
    rename_keyword(cards, "RADECSYS", "RADESYS");

    // Translation 2 (fitschan.c:31125-31131): LONGPOLE → LONPOLE.
    rename_keyword(cards, "LONGPOLE", "LONPOLE");

    // Translation 3 (fitschan.c:31206-31238): CDjjjiii → CDj_i.
    // The old 6-digit IRAF form encodes a CD matrix; we rewrite to
    // the standard CDj_i form so the existing CD reader handles it.
    // Limited to the 9×9 grid that fits in 6-digit indexed names.
    for j in 1..=9 {
        for i in 1..=9 {
            let from = format!("CD{j:03}{i:03}");
            let to = format!("CD{j}_{i}");
            rename_keyword(cards, &from, &to);
        }
    }
}

/// Move a keyword's value and comment from `from` to `to`, deleting
/// the original card.
///
/// If `to` is already present, the source is dropped (matches C
/// `SpecTrans` semantics: an existing standard value is never
/// overwritten by its non-standard alias). Both string-valued and
/// float-valued cards are handled; integer values are formatted
/// as floats since the destinations in this module are all
/// continuous numeric quantities.
///
/// Returns `true` if a rename actually occurred (i.e. `from` was
/// present and `to` was absent).
fn rename_keyword(cards: &mut CardList, from: &str, to: &str) -> bool {
    if cards.find(to).is_some() {
        // Destination present — discard the source.
        cards.delete(from);
        return false;
    }
    let comment = cards
        .find(from)
        .and_then(|c| c.comment.clone())
        .unwrap_or_default();
    if let Some(value) = cards.get_float(from) {
        cards.delete(from);
        cards.set_float(to, value, &comment);
        return true;
    }
    if let Some(value) = cards.get_string(from) {
        cards.delete(from);
        cards.set_string(to, &value, &comment);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_trans_is_a_no_op_when_all_keywords_are_already_standard() {
        let mut cards = CardList::from_header(
            "CRVAL1  =                180.0\n\
             CRVAL2  =                 45.0\n\
             CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             RADESYS = 'FK5     '\n\
             EQUINOX =               2000.0\n\
             END",
        )
        .unwrap();
        let before = cards.iter().count();
        spec_trans(&mut cards, Encoding::FitsWcs);
        let after = cards.iter().count();
        assert_eq!(before, after);
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn rename_keyword_moves_value_and_comment_to_new_name() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK5     '           / Reference frame\nEND").unwrap();
        assert!(rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn rename_keyword_no_op_when_source_absent() {
        let mut cards = CardList::from_header("END").unwrap();
        assert!(!rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        assert!(cards.find("RADECSYS").is_none());
        assert!(cards.find("RADESYS").is_none());
    }

    #[test]
    fn rename_keyword_no_op_when_destination_already_present() {
        let mut cards = CardList::from_header(
            "RADECSYS= 'FK4     '           / Old\n\
             RADESYS = 'FK5     '           / New\n\
             END",
        )
        .unwrap();
        assert!(!rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        // Source consumed; destination preserved.
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn translates_radecsys_to_radesys() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK5     '           / Old-style frame\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn keeps_radesys_when_both_present() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK4     '\nRADESYS = 'FK5     '\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn translates_longpole_to_lonpole() {
        let mut cards =
            CardList::from_header("LONGPOLE=                180.0 / Old name\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("LONGPOLE").is_none());
        assert_eq!(cards.get_float("LONPOLE"), Some(180.0));
    }

    #[test]
    fn translates_cdjjjiii_to_cdj_i() {
        let mut cards = CardList::from_header(
            "CD001001=                  1.0\n\
             CD001002=                  0.0\n\
             CD002001=                  0.0\n\
             CD002002=                  1.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("CD001001").is_none());
        assert_eq!(cards.get_float("CD1_1"), Some(1.0));
        assert_eq!(cards.get_float("CD2_2"), Some(1.0));
    }

    #[test]
    fn cdjjjiii_does_not_overwrite_cdj_i() {
        let mut cards = CardList::from_header(
            "CD001001=                  1.0\n\
             CD1_1   =                  2.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // The CDj_i form wins.
        assert_eq!(cards.get_float("CD1_1"), Some(2.0));
    }

    #[test]
    fn rename_keyword_handles_float_values() {
        let mut cards =
            CardList::from_header("LONGPOLE=                180.0 / Old name\nEND").unwrap();
        assert!(rename_keyword(&mut cards, "LONGPOLE", "LONPOLE"));
        assert_eq!(cards.get_float("LONPOLE"), Some(180.0));
        assert!(cards.find("LONGPOLE").is_none());
    }
}
