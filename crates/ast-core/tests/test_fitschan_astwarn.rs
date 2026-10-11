//! ASTWARN card placement on the FITS read path.
//!
//! C `Warn` (fitschan.c:34990) inserts its cards in front of the current
//! card, and the current card during a read is the last `NAXIS<i>` keyword
//! `DistortMaps` looked up (fitschan.c:8809-8813) — the only read on that
//! path that goes through `astGetFits<X>` rather than `GetValue`, and so the
//! only one that leaves the card cursor moved.

use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

/// The string value of every `ASTWARN` card, in order.
fn astwarn_values(fc: &FitsChan) -> Vec<String> {
    fc.to_header()
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("ASTWARN"))
        .map(|l| {
            // `KEYWORD = 'value'` — take what is between the first pair of
            // quotes, keeping the trailing spaces C's wrapping produces.
            let start = l.find('\'').expect("ASTWARN card holds a string") + 1;
            let end = start + l[start..].find('\'').expect("closing quote");
            l[start..end].to_string()
        })
        .collect()
}

/// The keyword of every card in the chan, in order.
fn keywords(fc: &FitsChan) -> Vec<String> {
    fc.to_header()
        .unwrap()
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

/// `projp`-shaped: `PROJP0/1/2` translate to `PV2_0/PV2_1/PV2_2`, but `-TAN`
/// accepts only `PV2_0`. C warns once per unusable parameter, latitude axis
/// before longitude at each index, in ascending `m` order.
///
/// Same shape as the `projp` fixture header
/// (`tests/fixtures/wcsconv/inputs/projp.head`).
const TWO_AXIS_BADPV: &str = "\
NAXIS1  =                  512 / length of data axis 1
NAXIS2  =                  512 / length of data axis 2
CTYPE1  = 'RA---TAN'           / Gnomonic projection
CTYPE2  = 'DEC--TAN'           / Gnomonic projection
CRVAL1  =              180.000 / Reference RA (deg)
CRVAL2  =               45.000 / Reference Dec (deg)
CRPIX1  =              256.500 / Reference pixel on axis 1
CRPIX2  =              256.500 / Reference pixel on axis 2
CD1_1   =        -8.3332205E-4 / CD matrix
CD1_2   =                  0.0 / CD matrix
CD2_1   =                  0.0 / CD matrix
CD2_2   =        0.00083332205 / CD matrix
RADECSYS= 'FK5     '           / Old-style reference frame keyword
EQUINOX =               2000.0 / Equinox
PROJP0  =                  0.0 / Legacy projection parameter 0
PROJP1  =                  0.0 / Legacy projection parameter 1
PROJP2  =                  0.0 / Legacy projection parameter 2
LONGPOLE=              180.000 / Native longitude of celestial pole
";

#[test]
fn badpv_warning_names_each_unusable_parameter_in_ascending_order() {
    let mut fc = FitsChan::from_header(TWO_AXIS_BADPV).expect("header parses");
    let _ = fc.read().expect("read succeeds");
    // Text and wrapping copied from `expected/projp.native:134-141`, including
    // the trailing space where C's 60-character wrap breaks the line.
    assert_eq!(
        astwarn_values(&fc),
        vec![
            "        ",
            "Projection parameter PV2_1 found, but is not used by -TAN ",
            "projections.",
            "        ",
            "        ",
            "Projection parameter PV2_2 found, but is not used by -TAN ",
            "projections.",
            "        ",
        ]
    );
}

/// `awav-a2f`-shaped: an air-wavelength axis whose algorithm code says the
/// values are expressed as frequency. C keeps the mapping — it interprets the
/// illegal `P` character correctly — and warns.
///
/// Same shape as the `awav-a2f` fixture header
/// (`tests/fixtures/wcsconv/inputs/awav-a2f.head`).
const ONE_AXIS_BADCTYPE: &str = "\
SIMPLE  =                    T / Fits standard
BITPIX  =                  -32 / Bits per pixel
NAXIS   =                    1 / Number of dimensions
NAXIS1  =                 1024 / Length of axis
CTYPE1  = 'AWAV-A2F'           / Air wavelength sampled, expressed as frequency
CRVAL1  =            5.000E-07 / Reference air wavelength (metres)
CRPIX1  =              512.500 / Reference pixel
CDELT1  =            1.000E-10 / Wavelength increment (metres)
CUNIT1  = 'm       '           / Axis units
RESTFRQ =           5.996E+14  / Rest frequency (Hz)
SPECSYS = 'BARYCENT'           / Spectral reference frame
";

#[test]
fn badctype_warning_names_the_legal_ctype_and_precedes_naxis1() {
    let mut fc = FitsChan::from_header(ONE_AXIS_BADCTYPE).expect("header parses");
    let _ = fc.read().expect("read succeeds");
    // Text and wrapping copied from `expected/awav-a2f.native:52-55`.
    assert_eq!(
        astwarn_values(&fc),
        vec![
            "        ",
            "The spectral CTYPE value AWAV-A2F is not legal - using ",
            "AWAV-A2A instead.",
            "        ",
        ]
    );

    // A 1-axis header parks C's card cursor on NAXIS1, so the block precedes
    // it rather than sitting between two NAXIS cards.
    let kws = keywords(&fc);
    let first_warn = kws.iter().position(|k| k == "ASTWARN").expect("ASTWARN");
    let naxis1 = kws.iter().position(|k| k == "NAXIS1").expect("NAXIS1");
    assert!(
        first_warn < naxis1,
        "ASTWARN block must precede NAXIS1, got {kws:?}"
    );
}

/// The `tan-zeropv` fixture header: a `-TAN` projection carrying `PVi_m`
/// distortion coefficients that are all zero on the longitude axis.
fn tan_zeropv_header() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/tan-zeropv.head",
    ))
    .expect("tan-zeropv.head should be readable")
}

#[test]
fn all_zero_longitude_pv_warns_and_demotes_to_plain_tan() {
    // C `GetUsedPolyTan` (fitschan.c:14071-14080): the latitude axis has a
    // non-zero coefficient (PV2_1 = 1.0) so it passes, and the longitude check
    // in the `else` branch then finds every coefficient zero, which raises
    // `badpv`, keeps the standard TAN convention and deletes the longitude PVs.
    let mut fc = FitsChan::from_header(&tan_zeropv_header()).expect("header parses");
    let _ = fc.read().expect("read succeeds").expect("an object");

    let warns = astwarn_values(&fc);
    assert!(
        warns.iter().any(|w| w.contains("longitude axis are zero")),
        "expected the longitude-axis badpv warning, got: {warns:#?}"
    );
    assert!(
        !warns.iter().any(|w| w.contains("latitude axis are zero")),
        "the latitude axis has PV2_1 = 1.0 and must not warn: {warns:#?}"
    );
}

#[test]
fn all_zero_latitude_pv_warns_about_the_latitude_axis() {
    // The other side of the same branch: zero every latitude coefficient and C
    // reports the latitude axis instead, never reaching the longitude check
    // (fitschan.c:14030-14039). The header is edited in memory — the fixture on
    // disk is C-generated ground truth.
    let text = tan_zeropv_header()
        .replace(
            "PV2_1   =                  1.0",
            "PV2_1   =                  0.0",
        )
        .replace(
            "PV2_2   =            0.0001000",
            "PV2_2   =            0.0000000",
        );

    let mut fc = FitsChan::from_header(&text).expect("header parses");
    let _ = fc.read().expect("read succeeds").expect("an object");

    let warns = astwarn_values(&fc);
    assert!(
        warns.iter().any(|w| w.contains("latitude axis are zero")),
        "expected the latitude-axis badpv warning, got: {warns:#?}"
    );
}

// ===========================================================================
// The astWarnings KeyMap view
// ===========================================================================

#[test]
fn warnings_keymap_mirrors_c_astwarnings() {
    // C `astWarnings` (channel.c:1582) builds a fresh KeyMap keyed
    // `Warning_1`, `Warning_2`, … in issue order, one string entry per
    // warning, and returns NULL when none were issued.
    use ast_core::AstType;

    let mut fc = FitsChan::from_header(&tan_zeropv_header()).expect("header parses");
    let _ = fc.read().expect("read succeeds").expect("an object");

    let texts = fc.warning_texts().to_vec();
    assert!(
        !texts.is_empty(),
        "tan-zeropv.head must raise at least one warning"
    );

    let map = fc.warnings().expect("warnings were issued, so not None");
    assert_eq!(map.map_size(), texts.len(), "one entry per warning");

    for (i, text) in texts.iter().enumerate() {
        let key = format!("Warning_{}", i + 1);
        assert_eq!(
            map.map_type(&key).expect("key present"),
            AstType::String,
            "{key} must be a string entry"
        );
        assert_eq!(
            map.get_0c(&key).expect("readable").as_deref(),
            Some(text.as_str()),
            "{key} must carry the warning text in issue order"
        );
    }

    // Keys are 1-based, so there is no Warning_0 and no entry past the count.
    assert_eq!(map.get_0c("Warning_0").expect("readable"), None);
    let past_end = format!("Warning_{}", texts.len() + 1);
    assert_eq!(map.get_0c(&past_end).expect("readable"), None);
}
