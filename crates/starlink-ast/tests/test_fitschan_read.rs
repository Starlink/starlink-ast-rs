//! FITS read-path Frame and Axis attribute plumbing.
//!
//! Each test here isolates one `WcsCelestial` / `WcsMapFrm` attribute
//! decision that a wcsconv fixture exercises only indirectly, so a
//! failure names the attribute rather than a serialized card.

use starlink_ast::AnyObject;
use starlink_ast::channel::Channel;
use starlink_ast::fitschan::FitsChan;
use starlink_ast::frames::{BasicFrame, DSBSpecFrame, Frame, SkyFrame, SkySystem};

/// A 2-axis Galactic header carrying EQUINOX and no observation date.
const GALACTIC_EQUINOX_ONLY: &str = "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                  100
CTYPE1  = 'GLON-TAN'
CTYPE2  = 'GLAT-TAN'
CRVAL1  =                 10.0
CRVAL2  =                 20.0
CRPIX1  =                 50.5
CRPIX2  =                 50.5
CDELT1  =                -0.01
CDELT2  =                 0.01
EQUINOX =               2000.0
";

/// C computes `eqmjd` from EQUINOX with no system guard
/// (fitschan.c:39333-39339) and applies it as the Epoch fallback
/// unconditionally (fitschan.c:39503). Only the *Equinox* attribute is
/// restricted to equatorial and ecliptic systems (fitschan.c:39504-39506).
/// A Galactic header with EQUINOX but no MJD-OBS/DATE-OBS therefore gets
/// an Epoch but no Equinox.
#[test]
fn galactic_header_takes_its_epoch_from_equinox() {
    let obj = FitsChan::from_header(GALACTIC_EQUINOX_ONLY)
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame");

    assert_eq!(sky.system(), SkySystem::Galactic);
    // Epoch is set — J2000.0 taken from EQUINOX.
    assert!(sky.test_epoch(), "Galactic Epoch must fall back to EQUINOX");
    assert!((sky.epoch_year() - 2000.0).abs() < 1e-9);
    // Equinox is NOT set — Galactic is neither EQU nor ECL.
    assert!(
        !sky.test_equinox(),
        "Galactic must not carry an Equinox attribute"
    );
}

/// A plain 2-axis equatorial header, with `{cname}` substituted for the pair
/// of `CNAME<i>` cards or the empty string.
fn equatorial_header(cname: &str) -> String {
    format!(
        "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                  100
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
CRVAL1  =                 10.0
CRVAL2  =                 20.0
CRPIX1  =                 50.5
CRPIX2  =                 50.5
CDELT1  =                -0.01
CDELT2  =                 0.01
{cname}"
    )
}

/// Read a header and return its current frame as a SkyFrame.
fn read_sky_frame(header: &str) -> SkyFrame {
    let obj = FitsChan::from_header(header)
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    fs.current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame")
        .clone()
}

/// CNAME<i> supplies the axis Label on a celestial frame, exactly as it
/// already does on a plain linear frame. C reads the store at the FITS
/// longitude/latitude axis and writes the SkyFrame's physical axes 0 and 1
/// (fitschan.c:39511-39514).
#[test]
fn cname_supplies_skyframe_axis_labels() {
    let sky = read_sky_frame(&equatorial_header(
        "CNAME1  = 'Right Ascension'\nCNAME2  = 'Declination'\n",
    ));
    assert!(sky.test_label(1).unwrap());
    assert!(sky.test_label(2).unwrap());
    assert_eq!(sky.label(1).unwrap(), "Right Ascension");
    assert_eq!(sky.label(2).unwrap(), "Declination");
}

/// The same header without CNAME leaves both Labels unset, so the SkyFrame's
/// system-derived defaults apply. C's celestial path has no CTYPE fallback —
/// unlike the plain-Frame path, which falls back because a plain Frame has no
/// better default.
#[test]
fn absent_cname_leaves_skyframe_labels_at_their_defaults() {
    let sky = read_sky_frame(&equatorial_header(""));
    assert!(!sky.test_label(1).unwrap());
    assert!(!sky.test_label(2).unwrap());
    assert_eq!(sky.label(1).unwrap(), "Right ascension");
    assert_eq!(sky.label(2).unwrap(), "Declination");
}

/// C `WcsSkyFrame` builds every SkyFrame from an explicit attribute string —
/// `astSkyFrame("System=ICRS")` for an ICRS header, exactly as it uses
/// `astSkyFrame("System=FK5")` for an FK5 one (fitschan.c:39425-39454). So
/// `astTestSystem` is true on a SkyFrame read from FITS whatever the system,
/// and the Dump writes an active card. ICRS is the only value for which
/// `SkyFrame::new` leaves the flag clear, so it is the only value that
/// exposes the difference.
#[test]
fn fits_read_sets_system_even_when_the_value_is_the_icrs_default() {
    let sky = read_sky_frame(&equatorial_header("RADESYS = 'ICRS    '\n"));
    assert_eq!(sky.system(), SkySystem::Icrs);
    assert!(
        sky.test_system(),
        "System must be explicitly set on a SkyFrame built by the FITS reader"
    );
}

/// The FK5 companion. It pins the other side: the unconditional `set_system`
/// must not change behavior for the systems that were already correct.
#[test]
fn fits_read_sets_system_for_a_non_default_value_too() {
    let sky = read_sky_frame(&equatorial_header(
        "RADESYS = 'FK5     '\nEQUINOX =               2000.0\n",
    ));
    assert_eq!(sky.system(), SkySystem::Fk5);
    assert!(sky.test_system());
}

/// Read a header and return its current frame's Domain plus whether the
/// attribute is set.
fn read_current_domain(header: &str) -> (String, bool) {
    let obj = FitsChan::from_header(header)
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    let frame = fs.current();
    (frame.domain().to_string(), frame.test_domain())
}

/// A 1-axis linear header with `{ctype}` substituted for its CTYPE card.
fn one_axis_header(ctype: &str) -> String {
    format!(
        "\
NAXIS   =                    1
NAXIS1  =                  100
CRVAL1  =                  1.0
CRPIX1  =                  1.0
CDELT1  =                  0.1
{ctype}"
    )
}

/// C `WcsOthers` gives the leftover plain Frame a Domain built from the CTYPE
/// values of the axes it could not classify, joined with "-"
/// (fitschan.c:39016-39074). A single unrecognized spectral-looking code
/// therefore yields that code verbatim as the Domain.
#[test]
fn unclassified_axis_domain_comes_from_ctype() {
    let (domain, is_set) = read_current_domain(&one_axis_header("CTYPE1  = 'XXXX-WAV'\n"));
    assert_eq!(domain, "XXXX-WAV");
    assert!(
        is_set,
        "Domain must be explicitly set, not merely defaulted"
    );
}

/// An absent CTYPE is replaced by the `AXIS<n>` default before the Domain is
/// built (fitschan.c:38995-38999), so the Domain is that default rather than
/// nothing. Confirmed against the C library.
#[test]
fn absent_ctype_falls_back_to_the_axis_number() {
    let (domain, is_set) = read_current_domain(&one_axis_header(""));
    assert_eq!(domain, "AXIS1");
    assert!(is_set);
}

/// A *blank* CTYPE is the case that reaches `astClearDomain`: the value is
/// present, so the `AXIS<n>` default does not apply, but `astChrLen` is zero
/// so nothing is appended and `newdom` stays NULL (fitschan.c:39017-39073).
/// The `AST_FITSCHAN` sentinel must never escape into the read object.
#[test]
fn blank_ctype_leaves_domain_unset_and_never_leaks_the_sentinel() {
    let (domain, is_set) = read_current_domain(&one_axis_header("CTYPE1  = '        '\n"));
    assert!(!is_set);
    assert_eq!(domain, "");
    assert_ne!(domain, "AST_FITSCHAN");
}

/// Two unclassified axes join with "-", in axis order. C builds `newdom` by
/// prefixing each value after the first with "-" (fitschan.c:39019-39026).
#[test]
fn two_unclassified_axes_join_their_ctypes_with_a_dash() {
    let header = "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                   10
CTYPE1  = 'AAAA    '
CTYPE2  = 'BBBB    '
CRVAL1  =                  1.0
CRVAL2  =                  2.0
CRPIX1  =                  1.0
CRPIX2  =                  1.0
CDELT1  =                  0.1
CDELT2  =                  0.2
";
    let (domain, _) = read_current_domain(header);
    assert_eq!(domain, "AAAA-BBBB");
}

/// C strips a trailing "-LOG" from each CTYPE before appending it
/// (fitschan.c:39004-39014); the stripped value is also the axis Symbol.
#[test]
fn log_suffix_is_stripped_from_the_domain_and_symbol() {
    let obj = FitsChan::from_header(&one_axis_header("CTYPE1  = 'ZZZZ-LOG'\n"))
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("-LOG header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    let frame = fs.current();
    assert_eq!(frame.domain(), "ZZZZ");
    let basic = frame
        .as_any()
        .downcast_ref::<BasicFrame>()
        .expect("a linear axis yields a BasicFrame");
    assert_eq!(basic.symbol(1).unwrap(), "ZZZZ");
}

/// `SkyFrame::unset_system` keeps its "genuinely unset" contract —
/// `enc_class.rs` relies on it to build a template a later set can override.
/// This is why the reader sets System at its own construction site rather
/// than in `SkyFrame::new`.
#[test]
fn unset_system_still_leaves_system_unset() {
    let sky = SkyFrame::unset_system();
    assert_eq!(sky.system(), SkySystem::Icrs);
    assert!(!sky.test_system());
}

/// A topocentric FREQ axis with IMAGFREQ, which makes a DSBSpecFrame.
const DSB_FREQ: &str = "\
NAXIS   =                    1
NAXIS1  =                   10
CTYPE1  = 'FREQ'
CUNIT1  = 'Hz'
CRPIX1  =                    1
CRVAL1  =              2.30E11
CDELT1  =               1.0E6
SPECSYS = 'TOPOCENT'
IMAGFREQ=              2.40E11
";

/// The Dsbcenter of the DSBSpecFrame read from `header`, in Hz.
fn dsb_centre_of(header: &str) -> f64 {
    let obj = FitsChan::from_header(header)
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    fs.current()
        .as_any()
        .downcast_ref::<DSBSpecFrame>()
        .expect("current frame should be a DSBSpecFrame")
        .dsb_centre_cur()
        .unwrap()
}

/// C's `WcsSpectral` takes the observation center from AXREF when the header
/// has one, and from CRVAL otherwise (fitschan.c:40121-40130). The values are
/// those the C library reports for the same two headers.
#[test]
fn dsb_centre_prefers_axref_over_crval() {
    assert_eq!(dsb_centre_of(DSB_FREQ), 2.30e11);
    let with_axref = format!("{DSB_FREQ}AXREF1  =              2.32E11\n");
    assert_eq!(dsb_centre_of(&with_axref), 2.32e11);
}
