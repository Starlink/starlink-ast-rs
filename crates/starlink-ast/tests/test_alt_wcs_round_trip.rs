//! Round-trip tests for FITS-WCS alternate planes.

use crate::common::native_dump;
use starlink_ast::AnyObject;
use starlink_ast::channel::Channel;
use starlink_ast::fitschan::FitsChan;
use starlink_ast::from_native_string;

const FITS_WCS_MULTI: &str = include_str!("fixtures/wcsconv/inputs/multi-wcs.head");

#[test]
fn fits_wcs_multi_native_round_trip_preserves_alt() {
    let mut fc = FitsChan::from_header(FITS_WCS_MULTI).unwrap();
    let obj = fc.read().unwrap().unwrap();

    // Render to native and re-parse.
    let native = native_dump(&obj).unwrap();
    let parsed = from_native_string(&native).unwrap();

    let nframe_before = match &obj {
        AnyObject::FrameSet(fs) => fs.nframe(),
        _ => panic!("expected FrameSet"),
    };
    let nframe_after = match &parsed {
        AnyObject::FrameSet(fs) => fs.nframe(),
        _ => panic!("expected FrameSet"),
    };
    assert_eq!(nframe_before, nframe_after);
    assert!(nframe_after >= 3, "expect base + primary + ≥1 alt");

    // Round-trip through native is byte-stable.
    let native2 = native_dump(&parsed).unwrap();
    assert_eq!(native, native2, "native round-trip must be byte-stable");
}
