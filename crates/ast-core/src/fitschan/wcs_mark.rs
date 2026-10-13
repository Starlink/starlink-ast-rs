//! Read-consumption marking for FITS-WCS-family headers.
//!
//! Port of the card-marking side effect of C `WcsFcRead` (fitschan.c:36336).
//! `WcsFcRead` walks **every** card in the FitsChan and pattern-matches the
//! card's keyword name against a fixed list of templates. A card whose name
//! matches, and whose value converts to the type the matching branch asks for,
//! is passed to `MarkCard` — flagging it consumed so it is excluded from
//! subsequent output. A value that does not convert raises a `badval`
//! warning, unless `SpecTrans` translated the keyword.
//!
//! Because the C function is a card *sweep* rather than a keyword *lookup*,
//! every copy of a repeated keyword is marked. A header holding two HDUs' worth
//! of `CRVAL1` loses both cards, not just the first. That is the behaviour this
//! module reproduces; value extraction into a [`FitsStore`] lives in
//! [`super::wcs_keywords`], which reads through the first match only, as C's
//! `GetValue` does.
//!
//! [`FitsStore`]: super::fits_store::FitsStore

use super::card::{Consumption, FitsValue};
use super::card_list::CardList;
use super::card_value;
use super::fits_match::match_keyword;
use super::warning::Warning;
use crate::error::{AstError, AstResult, ErrorCode};

/// The FITS value type a template's branch converts its card to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    Float,
    String,
}

/// A keyword-name template whose cards `WcsFcRead` consumes.
struct Template {
    /// Name template in `Match` syntax (see [`super::fits_match`]).
    pattern: &'static str,
    /// Type the branch converts the value to.
    want: Want,
    /// Whether an undefined (valueless) card is acceptable. Set only for
    /// `VELOSYS`, which C reads purely to strip it from the header.
    undef: bool,
}

const fn f(pattern: &'static str) -> Template {
    Template {
        pattern,
        want: Want::Float,
        undef: false,
    }
}

const fn s(pattern: &'static str) -> Template {
    Template {
        pattern,
        want: Want::String,
        undef: false,
    }
}

const fn f_undef(pattern: &'static str) -> Template {
    Template {
        pattern,
        want: Want::Float,
        undef: true,
    }
}

/// Every template whose branch in C `WcsFcRead` leaves `mark` at its initial
/// value of 1, listed in source order (fitschan.c:36438-36995).
///
/// The branches that clear `mark` are deliberately absent: `DUT1`, `MJD-OBS`,
/// `MJD-AVG`, `OBSGEO-X`, `OBSGEO-Y`, `OBSGEO-Z`, `TIMESYS`, and `DTAI` supply
/// observation details independent of the pixel→WCS mapping, so C reads them
/// but leaves them in the header. (`DTAI` is read at all only when `TELESCOP`
/// names JCMT or UKIRT; either way it is never marked.) None of those names
/// matches a template below, so omitting them preserves C's if/else-if
/// precedence.
///
/// Also absent are `CD%d_%d`, `RADECSYS`, `CROTA%d`, `PROJP%d`, `EPOCH`,
/// `LONGPOLE` and `DATE-OBS`: `WcsFcRead` has no branch for them. They reach
/// the store through [`super::spec_trans`], whose `GetValue2` marks the single
/// first matching card rather than every copy.
const TEMPLATES: &[Template] = &[
    f("CRVAL%d"),
    f("CRVAL%d%1c"),
    f("CRPIX%d"),
    f("CRPIX%d%1c"),
    f("CDELT%d"),
    f("CDELT%d%1c"),
    s("CTYPE%d"),
    s("CTYPE%d%1c"),
    s("CNAME%d"),
    s("CNAME%d%1c"),
    s("CUNIT%d"),
    s("CUNIT%d%1c"),
    f("PC%d_%d"),
    f("PC%d_%d%1c"),
    f("PV%d_%d"),
    f("PV%d_%d%1c"),
    s("PS%d_%d"),
    s("PS%d_%d%1c"),
    s("RADESYS"),
    s("RADESYS%1c"),
    f("EQUINOX"),
    f("EQUINOX%1c"),
    f("LATPOLE"),
    f("LATPOLE%1c"),
    f("LONPOLE"),
    f("LONPOLE%1c"),
    f("WCSAXES"),
    f("WCSAXES%1c"),
    s("WCSNAME"),
    s("WCSNAME%1c"),
    s("SPECSYS"),
    s("SPECSYS%1c"),
    s("SSYSSRC"),
    s("SSYSSRC%1c"),
    f("ZSOURCE"),
    f("ZSOURCE%1c"),
    f_undef("VELOSYS"),
    f_undef("VELOSYS%1c"),
    f("RESTFRQ"),
    f("RESTFRQ%1c"),
    f("RESTWAV"),
    f("RESTWAV%1c"),
    f("IMAGFREQ"),
    f("SREF%d"),
    f("SREF%d%1c"),
    f("SREFP%d"),
    f("SREFP%d%1c"),
    s("SREFIS"),
    s("SREFIS%1c"),
    f("AXREF%d"),
    f("AXREF%d%1c"),
    f("A_%d_%d"),
    f("A_%d_%d%1c"),
    f("B_%d_%d"),
    f("B_%d_%d%1c"),
    f("AP_%d_%d"),
    f("AP_%d_%d%1c"),
    f("BP_%d_%d"),
    f("BP_%d_%d%1c"),
];

/// Outcome of the conversion C performs via `CnvValue` before marking.
enum Convert {
    /// The value converts; the card is marked.
    Ok,
    /// The value does not convert; C leaves the card unmarked.
    No,
    /// The card has no value and the branch does not permit that, which C
    /// reports as `AST__FUNDEF` (fitschan.c:7666-7672).
    Undefined,
}

/// Whether `keyword` is one `WcsFcRead` reads, so that the sweep alone
/// decides whether each of its cards is consumed.
pub(crate) fn is_wcs_keyword(keyword: &str) -> bool {
    TEMPLATES
        .iter()
        .any(|t| match_keyword(keyword, t.pattern, 0).is_some())
}

/// Can `value` be converted to `want`? C's `CnvValue`, which hands the card to
/// `CnvType` (fitschan.c:7550) unless an undefined value is acceptable.
fn convert(value: &FitsValue, want: Want, undef: bool) -> Convert {
    if matches!(value, FitsValue::Undef) {
        return if undef {
            Convert::Ok
        } else {
            Convert::Undefined
        };
    }
    let want = match want {
        Want::Float => card_value::Want::Float,
        Want::String => card_value::Want::String,
    };
    // The rounding only shapes the text a float converts to.
    match card_value::cnv_type(Some(value), want, 0, "") {
        Ok(Some(_)) => Convert::Ok,
        Ok(None) => Convert::No,
        Err(_) => Convert::Undefined,
    }
}

/// The axis descriptions a FITS-WCS read builds Frames for, and its number
/// of pixel axes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WcsVersions {
    /// The number of pixel axes: one more than the highest CRPIX index of
    /// the primary axis description or any alternate one up to the highest
    /// with a CRVAL, as C's `FitsToStore` takes it (fitschan.c:11670-11675).
    pub npix: usize,
    /// Whether the primary axis description has a CRPIX, without which
    /// `MakeFitsFrameSet` adds no Frame for it (fitschan.c:12182).
    pub primary: bool,
    /// The alternate axis descriptions `MakeFitsFrameSet` adds Frames for
    /// (fitschan.c:12196-12213): each letter up to the highest with a CRVAL
    /// that has a CRVAL, CRPIX or CTYPE for an axis below `npix`.
    pub alternates: Vec<char>,
}

/// Find the axis descriptions and pixel axes a read of `cards` gives, from
/// the CRVAL, CRPIX and CTYPE values C's `WcsFcRead` stores: a card counts
/// if the sweep would store it, its value converting to the type its
/// template asks for, and cards already consumed are skipped.
pub(crate) fn wcs_versions(cards: &CardList) -> WcsVersions {
    const VERSIONS: usize = 27;
    // For each version (0 the primary, 1 `A`, ...), the highest CRPIX index
    // stored, and the (1-based) axes with a CRVAL or CTYPE.
    let mut crpix_max = [0usize; VERSIONS];
    let mut crval_axes: Vec<Vec<usize>> = vec![Vec::new(); VERSIONS];
    let mut crpix_axes: Vec<Vec<usize>> = vec![Vec::new(); VERSIONS];
    let mut ctype_axes: Vec<Vec<usize>> = vec![Vec::new(); VERSIONS];
    for card in cards.iter() {
        if card.flags.consumption == Consumption::Used {
            continue;
        }
        let Some(tmpl) = TEMPLATES
            .iter()
            .find(|t| match_keyword(&card.keyword, t.pattern, 0).is_some())
        else {
            continue;
        };
        let item = match tmpl.pattern {
            "CRVAL%d" | "CRVAL%d%1c" => &mut crval_axes,
            "CRPIX%d" | "CRPIX%d%1c" => &mut crpix_axes,
            "CTYPE%d" | "CTYPE%d%1c" => &mut ctype_axes,
            _ => continue,
        };
        if !matches!(convert(&card.value, tmpl.want, tmpl.undef), Convert::Ok) {
            continue;
        }
        let Some(&[axis]) = match_keyword(&card.keyword, tmpl.pattern, 1).as_deref() else {
            continue;
        };
        let Ok(axis) = usize::try_from(axis) else {
            continue;
        };
        if axis == 0 {
            continue;
        }
        let si = if tmpl.pattern.ends_with("%1c") {
            match card.keyword.chars().last() {
                Some(c @ 'A'..='Z') => c as usize - 'A' as usize + 1,
                Some(c @ 'a'..='z') => c as usize - 'a' as usize + 1,
                _ => continue,
            }
        } else {
            0
        };
        item[si].push(axis);
        if tmpl.pattern.starts_with("CRPIX") {
            crpix_max[si] = crpix_max[si].max(axis);
        }
    }
    // `GetMaxS`: the highest version with a CRVAL, or the primary.
    let smax = (1..VERSIONS)
        .rev()
        .find(|&si| !crval_axes[si].is_empty())
        .unwrap_or(0);
    let npix = (0..=smax).map(|si| crpix_max[si]).max().unwrap_or(0);
    let alternates = (1..=smax)
        .filter(|&si| {
            [&crval_axes[si], &crpix_axes[si], &ctype_axes[si]]
                .iter()
                .any(|axes| axes.iter().any(|&a| a <= npix))
        })
        .map(|si| (b'A' + (si - 1) as u8) as char)
        .collect();
    WcsVersions {
        npix,
        primary: crpix_max[0] > 0,
        alternates,
    }
}

/// Flag every card whose keyword names a FITS-WCS item as provisionally
/// consumed, so a successful read excludes it from output.
///
/// Already-consumed cards are skipped, matching C `astClearCard` plus the
/// `CARDUSED` test in `MoveCard` with the default `ignore_used` of 1. Protected
/// cards are visited but left unmarked, as in C `MarkCard`.
///
/// Returns `AST__FUNDEF` when a card that must carry a value has none.
pub(crate) fn mark_wcs_cards(
    cards: &mut CardList,
    translated: &[String],
) -> (Vec<(usize, Warning)>, AstResult<()>) {
    let mut warnings = Vec::new();
    for (index, card) in cards.iter_mut().enumerate() {
        if card.flags.consumption == Consumption::Used {
            continue;
        }
        let Some(tmpl) = TEMPLATES
            .iter()
            .find(|t| match_keyword(&card.keyword, t.pattern, 0).is_some())
        else {
            continue;
        };
        match convert(&card.value, tmpl.want, tmpl.undef) {
            Convert::Ok => {
                if !card.flags.protected {
                    card.flags.consumption = Consumption::Provisional;
                }
            }
            // C warns of a value it cannot convert, in front of its card,
            // unless the translations hold the keyword.
            Convert::No => {
                if !translated.contains(&card.keyword) {
                    warnings.push((
                        index,
                        Warning::new(
                            "badval",
                            format!(
                                "The original FITS header contained a value for keyword {} \
                                 which could not be converted to a {}.",
                                card.keyword,
                                match tmpl.want {
                                    Want::Float => "floating point number",
                                    Want::String => "character string",
                                }
                            ),
                        ),
                    ));
                }
            }
            Convert::Undefined => {
                return (
                    warnings,
                    Err(AstError::new(
                        ErrorCode::Fundef,
                        format!("the FITS keyword '{}' has an undefined value", card.keyword),
                    )),
                );
            }
        }
    }
    (warnings, Ok(()))
}

#[cfg(test)]
mod tests {
    use super::mark_wcs_cards;
    use crate::fitschan::card_list::CardList;

    /// Each card's keyword paired with whether the sweep marked it.
    fn consumed(header: &str) -> Vec<(String, bool)> {
        use crate::fitschan::card::Consumption;
        let mut cards = CardList::from_header(header).unwrap();
        mark_wcs_cards(&mut cards, &[]).1.unwrap();
        cards
            .iter()
            // The structural END card is never a WCS keyword; exclude it so
            // these assertions stay focused on WCS-card marking.
            .filter(|c| c.keyword != "END")
            .map(|c| {
                (
                    c.keyword.clone(),
                    c.flags.consumption == Consumption::Provisional,
                )
            })
            .collect()
    }

    #[test]
    fn every_copy_of_a_repeated_wcs_keyword_is_marked() {
        let got = consumed(
            "CRVAL1  =                 1.0\n\
             CRVAL1  =                 1.0\n\
             END",
        );
        assert_eq!(got, vec![("CRVAL1".into(), true), ("CRVAL1".into(), true)]);
    }

    #[test]
    fn observation_detail_keywords_survive() {
        let got = consumed(
            "MJD-OBS =               50000.0\n\
             DUT1    =                   0.1\n\
             OBSGEO-X=                1000.0\n\
             TIMESYS = 'UTC'\n\
             DTAI    =                  37.0\n\
             END",
        );
        assert!(got.iter().all(|(_, marked)| !*marked), "{got:?}");
    }

    #[test]
    fn keywords_handled_only_by_spec_trans_survive() {
        // CD, RADECSYS, CROTA, PROJP, EPOCH, LONGPOLE and DATE-OBS have no
        // WcsFcRead branch.
        let got = consumed(
            "CD1_1   =                 1.0\n\
             RADECSYS= 'FK5'\n\
             CROTA1  =                 0.0\n\
             PROJP1  =                 0.0\n\
             EPOCH   =                1950.0\n\
             LONGPOLE=                 0.0\n\
             DATE-OBS= '2000-01-01'\n\
             END",
        );
        assert!(got.iter().all(|(_, marked)| !*marked), "{got:?}");
    }

    #[test]
    fn alternate_axis_descriptions_are_marked() {
        let got = consumed(
            "CRVAL1A =                 1.0\n\
             CTYPE2B = 'FREQ'\n\
             PV1_2C  =                 0.0\n\
             END",
        );
        assert!(got.iter().all(|(_, marked)| *marked), "{got:?}");
    }

    #[test]
    fn sip_coefficients_are_marked() {
        let got = consumed(
            "A_1_2   =                 1e-6\n\
             BP_0_3  =                 1e-7\n\
             END",
        );
        assert!(got.iter().all(|(_, marked)| *marked), "{got:?}");
    }

    #[test]
    fn similar_names_are_not_swallowed() {
        // SREF%d must not match SREFP1 or SREFIS by prefix; all three have
        // their own branch, so all three end up marked, but via the right one.
        let got = consumed(
            "SREF1   =                 1.0\n\
             SREFP1  =                 2.0\n\
             SREFIS  = 'Pole'\n\
             END",
        );
        assert!(got.iter().all(|(_, marked)| *marked), "{got:?}");
    }

    #[test]
    fn a_string_that_is_not_a_number_leaves_a_float_card_unmarked() {
        let got = consumed("CRVAL1  = 'not a number'\nEND");
        assert_eq!(got, vec![("CRVAL1".into(), false)]);
    }

    #[test]
    fn a_numeric_string_converts_and_marks() {
        let got = consumed("CRVAL1  = '1.5'\nEND");
        assert_eq!(got, vec![("CRVAL1".into(), true)]);
    }

    #[test]
    fn an_undefined_velosys_is_accepted() {
        let mut cards = CardList::from_header("VELOSYS =\nEND").unwrap();
        assert!(mark_wcs_cards(&mut cards, &[]).1.is_ok());
    }

    #[test]
    fn an_undefined_crval_is_an_error() {
        let mut cards = CardList::from_header("CRVAL1  =\nEND").unwrap();
        assert!(mark_wcs_cards(&mut cards, &[]).1.is_err());
    }
}
