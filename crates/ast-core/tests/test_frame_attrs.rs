//! Every attribute of a plain Frame, checked against the C library.
//!
//! The expected values come from `frame_oracle/cases.rs`, which
//! `tools/oracle_fixtures/frame_attrs_probe.c` generates by setting each
//! attribute on a 2-axis C Frame and reading every attribute back, with its
//! `astTest` result, and dumping the Frame. [`frame`] applies the same
//! setting string through the public `BasicFrame` API, so the test also shows
//! which method corresponds to each C attribute.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "frame_oracle/cases.rs"]
mod cases;

use cases::V;

use ast_core::frames::{BasicFrame, BasicFrameBuilder, Frame, FrameSystem};
use ast_core::{AnyObject, from_native_string};

/// Degrees in a sexagesimal or decimal string such as `10:20:30`, `-30`.
fn degrees(text: &str) -> f64 {
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text),
    };
    let mut value = 0.0;
    let mut scale = 1.0;
    for field in body.split(':') {
        value += field.parse::<f64>().unwrap() * scale;
        scale /= 60.0;
    }
    sign * value
}

/// Radians from a sexagesimal or decimal angle in degrees, with C's own
/// arithmetic: C reads ObsLat and ObsLon through a SkyAxis, which sums each
/// field times 1, 1/60 or 1/3600 times radians per degree and then applies
/// the sign (skyaxis.c `AxisUnformat`), so the result matches C's bit for
/// bit and the dumps can be compared exactly.
fn c_sky_radians(text: &str) -> f64 {
    const DEG2RAD: f64 = 3600.0 * 4.848_136_811_095_36e-6;
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text),
    };
    let scale = [1.0, 1.0 / 60.0, 1.0 / 3600.0];
    let mut value = 0.0;
    for (i, field) in body.split(':').enumerate() {
        value += field.parse::<f64>().unwrap() * scale[i] * DEG2RAD;
    }
    sign * value
}

/// Build the Frame a setting string describes, as the probe does.
fn frame(setting: &str) -> BasicFrame {
    let (permute, attrs) = match setting.strip_prefix("PERM:") {
        Some(rest) => (true, rest),
        None => (false, setting),
    };
    let mut f = BasicFrameBuilder::new(2).build().unwrap();
    for s in attrs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, value) = s.split_once('=').expect("attr=value");
        let (attr, axis) = match name.split_once('(') {
            Some((attr, rest)) => (attr, Some(rest.trim_end_matches(')').parse().unwrap())),
            None => (name, None),
        };
        let int = || value.parse::<i32>().unwrap();
        let dbl = || value.parse::<f64>().unwrap();
        match (attr, axis) {
            ("Title", None) => f.set_title(value),
            ("Domain", None) => f.set_domain(value),
            ("Epoch", None) => match value.split_at(1) {
                ("B" | "J", year) => f.set_epoch_year(year.parse().unwrap()),
                _ => f.set_epoch_year(dbl()),
            },
            ("System", None) => f.set_system(FrameSystem::Cartesian),
            ("AlignSystem", None) => f.set_align_system(FrameSystem::Cartesian),
            ("Digits", None) => f.set_digits(int()),
            ("Digits", Some(a)) => f.set_axis_digits(a, int()).unwrap(),
            ("Direction", Some(a)) => f.set_direction(a, int() != 0).unwrap(),
            ("Format", Some(a)) => f.set_format_str(a, value).unwrap(),
            ("Label", Some(a)) => f.set_label(a, value).unwrap(),
            ("Symbol", Some(a)) => f.set_symbol(a, value).unwrap(),
            ("Unit", Some(a)) => f.set_unit(a, value).unwrap(),
            ("Top", Some(a)) => f.set_top(a, dbl()).unwrap(),
            ("Bottom", Some(a)) => f.set_bottom(a, dbl()).unwrap(),
            ("MatchEnd", None) => f.set_match_end(int() != 0).unwrap(),
            ("MaxAxes", None) => f.set_max_axes(int() as usize).unwrap(),
            ("MinAxes", None) => f.set_min_axes(int() as usize).unwrap(),
            ("Permute", None) => f.set_permute(int() != 0).unwrap(),
            ("PreserveAxes", None) => f.set_preserve_axes(int() != 0).unwrap(),
            ("ObsLat", None) => f.set_obs_lat(c_sky_radians(value)),
            ("ObsLon", None) => f.set_obs_lon(c_sky_radians(value)),
            ("ObsAlt", None) => f.set_obs_alt(dbl()),
            ("Dtai", None) => f.set_dtai(dbl()),
            ("Dut1", None) => f.set_dut1(dbl()),
            other => panic!("unsupported setting {other:?}"),
        }
    }
    if permute {
        f.perm_axes(&[2, 1]).unwrap();
    }
    f
}

/// C's formatted ObsLat / ObsLon (`N10:20:30.00`, `W70:30:00.00`) in
/// radians.
fn c_obs_angle(text: &str) -> f64 {
    let (sign, rest) = match text.split_at(1) {
        ("N" | "E", rest) => (1.0, rest),
        ("S" | "W", rest) => (-1.0, rest),
        _ => panic!("unexpected {text}"),
    };
    sign * degrees(rest).to_radians()
}

/// The value the Rust API gives for an attribute, as the probe records it.
fn value(f: &BasicFrame, attr: &str, axis: usize) -> V {
    let s = |v: String| V::S(Box::leak(v.into_boxed_str()));
    match attr {
        "Title" => s(f.title()),
        "Domain" => s(f.domain().to_string()),
        "Epoch" => V::D(f.epoch_year()),
        "System" => s(f.system().as_str().to_string()),
        "AlignSystem" => s(f.align_system().as_str().to_string()),
        "Digits" if axis == 0 => V::I(f.digits()),
        "Digits" => V::I(f.axis_digits(axis).unwrap()),
        "Direction" => V::I(i32::from(f.direction(axis).unwrap())),
        "Format" => s(f.format_str(axis).unwrap()),
        "Label" => s(f.label(axis).unwrap()),
        "Symbol" => s(f.symbol(axis).unwrap()),
        "Unit" => s(f.unit(axis).unwrap().to_string()),
        "Top" => V::D(f.top(axis).unwrap()),
        "Bottom" => V::D(f.bottom(axis).unwrap()),
        "MatchEnd" => V::I(i32::from(f.match_end())),
        "MaxAxes" => V::I(f.max_axes() as i32),
        "MinAxes" => V::I(f.min_axes() as i32),
        "Permute" => V::I(i32::from(f.permute())),
        "PreserveAxes" => V::I(i32::from(f.preserve_axes())),
        "ObsLat" => V::D(BasicFrame::obs_lat(f)),
        "ObsLon" => V::D(BasicFrame::obs_lon(f)),
        "ObsAlt" => V::D(BasicFrame::obs_alt(f)),
        "Dtai" => V::D(BasicFrame::dtai(f)),
        "Dut1" => V::D(BasicFrame::dut1(f)),
        "Naxes" => V::I(f.naxes() as i32),
        "NormUnit" => s(f.norm_unit(axis).unwrap()),
        "InternalUnit" => s(f.internal_unit(axis).unwrap()),
        other => panic!("unexpected attribute {other}"),
    }
}

fn test_flag(f: &BasicFrame, attr: &str, axis: usize) -> bool {
    match attr {
        "Title" => f.test_title(),
        "Domain" => f.test_domain(),
        "Epoch" => f.test_epoch(),
        "System" => f.test_system(),
        "AlignSystem" => f.test_align_system(),
        "Digits" if axis == 0 => f.test_digits(),
        "Digits" => f.test_axis_digits(axis).unwrap(),
        "Direction" => f.test_direction(axis).unwrap(),
        "Format" => f.test_format_str(axis).unwrap(),
        "Label" => f.test_label(axis).unwrap(),
        "Symbol" => f.test_symbol(axis).unwrap(),
        "Unit" => f.test_unit(axis).unwrap(),
        "Top" => f.test_top(axis).unwrap(),
        "Bottom" => f.test_bottom(axis).unwrap(),
        "MatchEnd" => f.test_match_end(),
        "MaxAxes" => f.test_max_axes(),
        "MinAxes" => f.test_min_axes(),
        "Permute" => f.test_permute(),
        "PreserveAxes" => f.test_preserve_axes(),
        "ObsLat" => f.test_obs_lat(),
        "ObsLon" => f.test_obs_lon(),
        "ObsAlt" => f.test_obs_alt(),
        "Dtai" => f.test_dtai(),
        "Dut1" => f.test_dut1(),
        other => panic!("unexpected attribute {other}"),
    }
}

#[test]
fn every_attribute_matches_the_c_library() {
    let mut failures = Vec::new();
    for &(setting, attr, axis, expected, test) in cases::VALUE_CASES {
        let f = frame(setting);
        let got = value(&f, attr, axis);
        let matches = match (expected, got) {
            // C formats ObsLat and ObsLon to 0.01 arcsec.
            (V::S(c), V::D(rad)) if attr.starts_with("Obs") => {
                (c_obs_angle(c) - rad).abs() < 0.006 / 206264.806
            }
            (e, g) => e == g,
        };
        if !matches {
            failures.push(format!(
                "{setting:?} {attr}({axis}): got {got:?}, C gives {expected:?}"
            ));
        }
        if let Some(expected_test) = test {
            let got_test = test_flag(&f, attr, axis);
            if got_test != expected_test {
                failures.push(format!(
                    "{setting:?} test {attr}({axis}): got {got_test}, C gives {expected_test}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases::VALUE_CASES.len(),
        failures.join("\n")
    );
}

/// The dump the port writes is C's, and reading C's dump back gives a Frame
/// that writes it again and reports the same attributes.
#[test]
fn dump_matches_the_c_library() {
    for &(setting, expected) in cases::DUMP_CASES {
        let f = frame(setting);
        let written = native_dump(&AnyObject::BasicFrame(f.clone())).unwrap();
        assert_eq!(written, expected, "dump for {setting:?}");

        let AnyObject::BasicFrame(read) = from_native_string(expected).unwrap() else {
            panic!("not a Frame");
        };
        let rewritten = native_dump(&AnyObject::BasicFrame(read.clone())).unwrap();
        assert_eq!(
            rewritten, expected,
            "round trip of C's dump for {setting:?}"
        );

        for &(s, attr, axis, _, _) in cases::VALUE_CASES.iter().filter(|c| c.0 == setting) {
            let _ = s;
            assert_eq!(
                value(&read, attr, axis),
                value(&f, attr, axis),
                "{attr}({axis}) after reading the dump for {setting:?}"
            );
        }
    }
}

/// Clearing every attribute returns the Frame to its defaults.
#[test]
fn clearing_restores_every_default() {
    let (full, _) = cases::DUMP_CASES
        .iter()
        .find(|(s, _)| s.starts_with("Title=T,"))
        .expect("the combined case");
    let mut f = frame(full);
    f.clear_title();
    f.clear_domain();
    f.clear_epoch();
    f.clear_digits();
    f.clear_match_end();
    f.clear_max_axes();
    f.clear_min_axes();
    f.clear_permute();
    f.clear_preserve_axes();
    f.clear_obs_lat();
    f.clear_obs_lon();
    f.clear_obs_alt();
    f.clear_dtai();
    f.clear_dut1();
    for axis in 1..=2 {
        f.clear_axis_digits(axis).unwrap();
        f.clear_direction(axis).unwrap();
        f.clear_format_str(axis).unwrap();
        f.clear_label(axis).unwrap();
        f.clear_symbol(axis).unwrap();
        f.clear_unit(axis).unwrap();
        f.clear_top(axis).unwrap();
        f.clear_bottom(axis).unwrap();
    }
    let (_, default_dump) = cases::DUMP_CASES
        .iter()
        .find(|(s, _)| *s == " ")
        .expect("the default case");
    assert_eq!(
        native_dump(&AnyObject::BasicFrame(f)).unwrap(),
        *default_dump
    );
}
