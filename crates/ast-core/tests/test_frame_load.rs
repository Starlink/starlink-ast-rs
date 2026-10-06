//! Loading SpecFrames and TimeFrames from Native dumps reads the
//! Frame-class attributes as C's `astLoadFrame` does, and the observer
//! position under the names older versions of AST wrote.

use ast_core::frames::{Frame, SpecFrame, SpecSystem, TimeFrame, TimeSystem};
use ast_core::{AnyObject, from_native_string, to_native_string};

fn round_trip(f: &dyn Frame, obj: AnyObject) -> Box<dyn Frame> {
    let text = to_native_string(&obj).unwrap();
    let back = from_native_string(&text).unwrap();
    let frame = back.as_frame().expect("a Frame");
    assert_eq!(frame.naxes(), f.naxes());
    frame.snapshot()
}

fn set_frame_keys(f: &mut dyn Frame) {
    f.set_digits(10).unwrap();
    f.set_max_axes(3).unwrap();
    f.set_min_axes(1).unwrap();
    f.set_match_end(true).unwrap();
    f.set_permute(false).unwrap();
    f.set_preserve_axes(true).unwrap();
}

fn assert_frame_keys(f: &dyn Frame) {
    assert_eq!(f.digits(), 10);
    assert_eq!((f.test_max_axes(), f.max_axes()), (true, 3));
    assert_eq!((f.test_min_axes(), f.min_axes()), (true, 1));
    assert!(f.test_match_end() && f.match_end());
    assert!(f.test_permute() && !f.permute());
    assert!(f.test_preserve_axes() && f.preserve_axes());
}

/// Digits, MinAxes, MaxAxes, MatchEnd, Permute and PreserveAxes survive a
/// Native round trip of a SpecFrame and a TimeFrame.
#[test]
fn frame_keys_survive_a_round_trip() {
    let mut s = SpecFrame::new(SpecSystem::Freq);
    set_frame_keys(&mut s);
    assert_frame_keys(round_trip(&s, AnyObject::SpecFrame(s.clone())).as_ref());

    let mut t = TimeFrame::new(TimeSystem::Mjd);
    set_frame_keys(&mut t);
    assert_frame_keys(round_trip(&t, AnyObject::TimeFrame(t.clone())).as_ref());
}

/// GeoLat / GeoLon (SpecFrame, specframe.c:7097) and ClLat / ClLon
/// (TimeFrame, timeframe.c:7248) give the observer position when ObsLat and
/// ObsLon are absent.
#[test]
fn legacy_observer_keys_are_read() {
    let spec = "Begin SpecFrame\n Naxes = 1\n Domain = \"SPECTRUM\"\n System = \"FREQ\"\n \
                GeoLat = 0.3\n GeoLon = 1.2\nEnd SpecFrame\n";
    let s = from_native_string(spec).unwrap();
    let s = s.as_frame().unwrap();
    assert_eq!((s.obs_lat(), s.obs_lon()), (0.3, 1.2));

    let time = "Begin TimeFrame\n Naxes = 1\n Domain = \"TIME\"\n System = \"MJD\"\n \
                ClLat = -0.4\n ClLon = 2.5\nEnd TimeFrame\n";
    let t = from_native_string(time).unwrap();
    let t = t.as_frame().unwrap();
    assert_eq!((t.obs_lat(), t.obs_lon()), (-0.4, 2.5));
}
