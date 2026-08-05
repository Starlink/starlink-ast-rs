//! FITS read-path Frame and Axis attribute plumbing.
//!
//! Each test here isolates one `WcsCelestial` / `WcsMapFrm` attribute
//! decision that a wcsconv fixture exercises only indirectly, so a
//! failure names the attribute rather than a serialised card.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;
use ast_core::frames::{SkyFrame, SkySystem};

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
/// (fitschan.c:39204-39210) and applies it as the Epoch fallback
/// unconditionally (fitschan.c:39370). Only the *Equinox* attribute is
/// restricted to equatorial and ecliptic systems (fitschan.c:39371-39373).
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
/// (fitschan.c:39378-39381).
#[test]
fn cname_supplies_skyframe_axis_labels() {
    let sky = read_sky_frame(&equatorial_header(
        "CNAME1  = 'Right Ascension'\nCNAME2  = 'Declination'\n",
    ));
    assert!(sky.test_axis_label(1).unwrap());
    assert!(sky.test_axis_label(2).unwrap());
    assert_eq!(sky.axis_label(1).unwrap(), "Right Ascension");
    assert_eq!(sky.axis_label(2).unwrap(), "Declination");
}

/// The same header without CNAME leaves both Labels unset, so the SkyFrame's
/// system-derived defaults apply. C's celestial path has no CTYPE fallback —
/// unlike the plain-Frame path, which falls back because a plain Frame has no
/// better default.
#[test]
fn absent_cname_leaves_skyframe_labels_at_their_defaults() {
    let sky = read_sky_frame(&equatorial_header(""));
    assert!(!sky.test_axis_label(1).unwrap());
    assert!(!sky.test_axis_label(2).unwrap());
    assert_eq!(sky.axis_label(1).unwrap(), "Right ascension");
    assert_eq!(sky.axis_label(2).unwrap(), "Declination");
}
