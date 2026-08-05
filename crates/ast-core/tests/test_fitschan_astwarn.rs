//! ASTWARN card placement on the FITS read path.
//!
//! C `Warn` (fitschan.c:34990) inserts its cards in front of the current
//! card, and the current card during a read is the last `NAXIS<i>` keyword
//! `DistortMaps` looked up (fitschan.c:8809-8813) — the only read on that
//! path that goes through `astGetFits<X>` rather than `GetValue`, and so the
//! only one that leaves the card cursor moved.

use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

/// The keyword of every card in the chan, in order.
fn keywords(fc: &FitsChan) -> Vec<String> {
    fc.to_header()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take(8).collect::<String>().trim().to_string())
        .collect()
}

/// A 2-axis header with an unrecognised celestial system: the port already
/// generates the `badcel` warning text (`wcs_celestial.rs`) for this, so it
/// isolates card *position* from text generation. C parks its cursor on
/// `NAXIS2` here, so the block lands between `NAXIS1` and `NAXIS2`.
///
/// Same shape as the `unknown-cel` fixture header
/// (`tests/fixtures/wcsconv/inputs/unknown-cel.head`).
const TWO_AXIS_BADCEL: &str = "\
NAXIS1  =                  100 / length of data axis 1
NAXIS2  =                  100 / length of data axis 2
CTYPE1  = 'XLON-TAN'           / Unknown longitude axis
CTYPE2  = 'XLAT-TAN'           / Unknown latitude axis
CRVAL1  =                180.0
CRVAL2  =                 45.0
CRPIX1  =                 50.0
CRPIX2  =                 50.0
CDELT1  =                 -0.1
CDELT2  =                  0.1
";

#[test]
fn astwarn_block_precedes_the_last_naxis_card_for_two_axes() {
    let mut fc = FitsChan::from_header(TWO_AXIS_BADCEL).expect("header parses");
    let _ = fc.read().expect("read succeeds");
    let kws = keywords(&fc);

    let first_warn = kws
        .iter()
        .position(|k| k == "ASTWARN")
        .unwrap_or_else(|| panic!("a badcel ASTWARN block is expected, got {kws:?}"));
    let naxis1 = kws.iter().position(|k| k == "NAXIS1").expect("NAXIS1");
    let naxis2 = kws.iter().position(|k| k == "NAXIS2").expect("NAXIS2");
    assert!(
        naxis1 < first_warn && first_warn < naxis2,
        "ASTWARN block must sit between NAXIS1 and NAXIS2, got {kws:?}"
    );
}
