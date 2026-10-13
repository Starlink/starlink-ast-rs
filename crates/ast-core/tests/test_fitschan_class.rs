//! CLASS-encoding FITS read tests.
//!
//! CLASS headers reach the reader through `enc_class.rs`, whose projection
//! handling differs from the FITS-WCS path: a blank projection code is
//! rewritten rather than rejected.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;
use ast_core::frames::{CmpFrame, Frame, SkyFrame};

/// The first `SkyFrame` in `frame`, descending through `CmpFrame` components.
/// A CLASS header produces `CmpFrame(SkyFrame, SpecFrame)`, so the SkyFrame is
/// a component rather than the current frame. Both components are searched, so
/// the helper holds whichever order `enc_class.rs` composes them in.
fn find_skyframe(frame: &dyn Frame) -> Option<&SkyFrame> {
    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        return Some(sky);
    }
    let cmp = frame.as_any().downcast_ref::<CmpFrame>()?;
    find_skyframe(cmp.frame1()).or_else(|| find_skyframe(cmp.frame2()))
}

/// A CLASS header whose celestial CTYPEs carry no projection code.
///
/// `DELTAV` plus a `VELO-*` keyword is what identifies the encoding as CLASS
/// (`guess_encoding::is_class`, mirroring C `astGetEncoding`), so both are
/// required for the header to reach the CLASS reader at all.
const CLASS_BLANK_PROJECTION: &str = "\
SIMPLE  =                    T
BITPIX  =                  -32
NAXIS   =                    3
NAXIS1  =                 1024
NAXIS2  =                    1
NAXIS3  =                    1
CTYPE1  = 'FREQ    '
CTYPE2  = 'RA--    '
CTYPE3  = 'DEC-    '
CRVAL1  =                  0.0
CRVAL2  =               40.738
CRVAL3  =               62.933
CRPIX1  =              512.500
CRPIX2  =                  0.5
CRPIX3  =                  0.5
CDELT1  =              39062.5
CDELT2  =              -0.0100
CDELT3  =               0.0100
EQUINOX =               2000.0
RADESYS = 'FK5     '
RESTFREQ=       1.102013543E11
VELO-OBS=               2500.0
DELTAV  =             -106.266
";

/// The same celestial axes with an explicit `-GLS` projection code.
const EXPLICIT_GLS: &str = "\
NAXIS   =                    2
NAXIS1  =                  100
NAXIS2  =                  100
CTYPE1  = 'GLON-GLS'
CTYPE2  = 'GLAT-GLS'
CRVAL1  =                 10.0
CRVAL2  =                 20.0
CRPIX1  =                 50.5
CRPIX2  =                 50.5
CDELT1  =                -0.01
CDELT2  =                 0.01
";

/// A CLASS header with a blank projection code gets `-GLS` from `ClassTrans`
/// (fitschan.c:7188-7209), and that `-GLS` survives: the `-GLS` → `-SFL`
/// rewrite at fitschan.c:31927 is gated on the projection code captured at the
/// *top* of the SpecTrans loop (fitschan.c:31248-31271), which for a blank
/// CTYPE is blank, not `-GLS`. So the frame reports the GLS description
/// "sinusoidal", not the SFL description "Sanson-Flamsteed". The two are the
/// same projection math (wcsmap.c:698, :712 share `astSFLfwd`/`astSFLrev`) and
/// differ only in name.
#[test]
fn blank_class_projection_keeps_the_gls_description() {
    let obj = FitsChan::from_header(CLASS_BLANK_PROJECTION)
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("CLASS header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    let sky = find_skyframe(fs.current()).expect("a SkyFrame component");
    assert_eq!(sky.projection(), Some("sinusoidal"));
}

/// The companion: an *explicit* `-GLS` does hit the fitschan.c:31927 rewrite,
/// becomes `-SFL`, and reports the SFL description. This is the other side of
/// the same guard, and what `inputs/gls.head` covers in the wcsconv corpus.
#[test]
fn explicit_gls_projection_becomes_sfl() {
    let obj = FitsChan::from_header(EXPLICIT_GLS)
        .expect("header parses")
        .read()
        .expect("read succeeds")
        .expect("header should produce an object");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("read should produce a FrameSet");
    };
    let sky = find_skyframe(fs.current()).expect("a SkyFrame component");
    assert_eq!(sky.projection(), Some("Sanson-Flamsteed"));
}
