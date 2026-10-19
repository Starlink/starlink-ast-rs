//! Every attribute of a SkyFrame, checked against the C library.
//!
//! The expected values come from `skyframe_attrs_oracle/cases.rs`, which
//! `tools/oracle_fixtures/skyframe_attrs_probe.c` generates by applying each
//! setting to a default SkyFrame, reading every attribute back with its
//! `astTest` result, and dumping the SkyFrame. [`sky_frame`] applies the same
//! settings through the public API, so the test also shows which method
//! corresponds to each C attribute.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "skyframe_attrs_oracle/cases.rs"]
mod cases;

use cases::V;

use ast_core::frames::{Frame, SkyFrame, SkyRefIs, SkySystem};
use ast_core::{AnyObject, from_native_string};

/// A default SkyFrame with each `;`-separated setting applied in turn, as
/// the probe applies them.
fn sky_frame(settings: &str) -> SkyFrame {
    let mut f = SkyFrame::unset_system();
    for setting in settings.split(';').filter(|s| !s.is_empty()) {
        if setting == "PERM" {
            f.perm_axes(&[2, 1]).unwrap();
            continue;
        }
        let (name, value) = setting.split_once('=').unwrap();
        let (name, axis) = match name.split_once('(') {
            Some((n, rest)) => (n, rest.trim_end_matches(')').parse().unwrap()),
            None => (name, 0),
        };
        let dbl = || value.parse::<f64>().unwrap();
        match name {
            "System" => f.set_system(SkySystem::parse(value).unwrap()),
            "AlignSystem" => f.set_align_system(SkySystem::parse(value).unwrap()),
            "Equinox" => f.set_equinox_year(dbl()),
            "Epoch" => f.set_epoch_year(dbl()),
            "NegLon" => f.set_neg_lon(value != "0"),
            "Projection" => f.set_projection(value),
            "SkyRef" => f.set_sky_ref(axis, dbl()).unwrap(),
            "SkyRefP" => f.set_sky_ref_p(axis, dbl()).unwrap(),
            "SkyRefIs" => f.set_sky_ref_is(SkyRefIs::parse(value).unwrap()),
            "SkyTol" => f.set_sky_tol(dbl()),
            "AlignOffset" => f.set_align_offset(value != "0"),
            "AsTime" => f.set_as_time(axis, value != "0").unwrap(),
            other => panic!("unexpected setting {other}"),
        }
    }
    f
}

/// C's name for a SkyRefIs value.
fn sky_ref_is_name(v: SkyRefIs) -> &'static str {
    match v {
        SkyRefIs::Origin => "Origin",
        SkyRefIs::Pole => "Pole",
        SkyRefIs::Ignored => "Ignored",
    }
}

fn value(f: &SkyFrame, attr: &str, axis: usize) -> V {
    let s = |v: String| V::S(Box::leak(v.into_boxed_str()));
    let b = |v: bool| V::I(i32::from(v));
    match attr {
        "System" => s(f.system().dump_str().to_string()),
        "AlignSystem" => s(f.align_system().dump_str().to_string()),
        "Equinox" => V::D(f.equinox_year()),
        "Epoch" => V::D(f.epoch_year()),
        "NegLon" => b(f.neg_lon()),
        "Projection" => s(f.projection().unwrap_or("").to_string()),
        "SkyRef" => V::D(f.sky_ref(axis).unwrap()),
        "SkyRefIs" => s(sky_ref_is_name(f.sky_ref_is()).to_string()),
        "SkyRefP" => V::D(f.sky_ref_p(axis).unwrap()),
        "SkyTol" => V::D(f.sky_tol()),
        "AlignOffset" => b(f.align_offset()),
        "AsTime" => b(f.as_time(axis).unwrap()),
        "IsLatAxis" => b(f.is_lat_axis(axis).unwrap()),
        "IsLonAxis" => b(f.is_lon_axis(axis).unwrap()),
        "LatAxis" => V::I(f.lat_axis() as i32),
        "LonAxis" => V::I(f.lon_axis() as i32),
        "Title" => s(Frame::title(f).unwrap()),
        "Label" => s(Frame::label(f, axis).unwrap()),
        "Symbol" => s(Frame::symbol(f, axis).unwrap()),
        "Unit" => s(Frame::unit(f, axis).unwrap()),
        "Format" => s(Frame::format_str(f, axis).unwrap()),
        "Direction" => b(Frame::direction(f, axis).unwrap()),
        "Top" => V::D(Frame::top(f, axis).unwrap()),
        "Bottom" => V::D(Frame::bottom(f, axis).unwrap()),
        other => panic!("unexpected attribute {other}"),
    }
}

fn test_flag(f: &SkyFrame, attr: &str, axis: usize) -> bool {
    match attr {
        "System" => f.test_system(),
        "AlignSystem" => f.test_align_system(),
        "Equinox" => f.test_equinox(),
        "Epoch" => Frame::test_epoch(f),
        "NegLon" => f.test_neg_lon(),
        "Projection" => f.test_projection(),
        "SkyRef" => f.test_sky_ref(axis).unwrap(),
        "SkyRefIs" => f.test_sky_ref_is(),
        "SkyRefP" => f.test_sky_ref_p(axis).unwrap(),
        "SkyTol" => f.test_sky_tol(),
        "AlignOffset" => f.test_align_offset(),
        "AsTime" => f.test_as_time(axis).unwrap(),
        "Title" => Frame::test_title(f),
        "Label" => Frame::test_label(f, axis).unwrap(),
        "Symbol" => Frame::test_symbol(f, axis).unwrap(),
        "Unit" => Frame::test_unit(f, axis).unwrap(),
        "Format" => Frame::test_format_str(f, axis).unwrap(),
        "Direction" => Frame::test_direction(f, axis).unwrap(),
        "Top" => Frame::test_top(f, axis).unwrap(),
        "Bottom" => Frame::test_bottom(f, axis).unwrap(),
        other => panic!("unexpected attribute {other}"),
    }
}

/// Whether a value matches C's. Epoch and Equinox are compared to 1e-9
/// years, since C and the port convert them from an MJD by the same
/// formula but not necessarily with the same rounding.
fn matches(attr: &str, expected: V, got: V) -> bool {
    match (expected, got) {
        (V::D(e), V::D(g)) if attr == "Epoch" || attr == "Equinox" => (e - g).abs() < 1e-9,
        (e, g) => e == g,
    }
}

#[test]
fn every_attribute_matches_the_c_library() {
    let mut failures = Vec::new();
    let mut last: Option<(&str, SkyFrame)> = None;
    for &(setting, attr, axis, expected, expected_test) in cases::VALUE_CASES {
        if last.as_ref().is_none_or(|(s, _)| *s != setting) {
            last = Some((setting, sky_frame(setting)));
        }
        let f = &last.as_ref().unwrap().1;
        let got = value(f, attr, axis);
        let got_test = expected_test.map(|_| test_flag(f, attr, axis));
        if !matches(attr, expected, got) || got_test != expected_test {
            failures.push(format!(
                "{setting:?} {attr}({axis}): got {got:?} test {got_test:?}, C gives {expected:?} test {expected_test:?}"
            ));
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

/// The dump the port writes is C's, and reading C's dump back gives a
/// SkyFrame that writes it again.
#[test]
fn dump_matches_the_c_library() {
    let mut failures = Vec::new();
    for &(setting, expected) in cases::DUMP_CASES {
        let f = sky_frame(setting);
        let written = native_dump(&AnyObject::SkyFrame(f)).unwrap();
        if written != expected {
            failures.push(format!(
                "dump for {setting:?}:\n--- port\n{written}--- C\n{expected}"
            ));
            continue;
        }
        let read = from_native_string(expected).unwrap();
        let rewritten = native_dump(&read).unwrap();
        if rewritten != expected {
            failures.push(format!(
                "round trip of C's dump for {setting:?}:\n--- port\n{rewritten}--- C\n{expected}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} dumps differ:\n{}",
        failures.len(),
        cases::DUMP_CASES.len(),
        failures.join("\n")
    );
}

/// A SkyFrame's own SkyAxes have no IsLatitude set, so the Axis methods C
/// reaches through `astGetAxis` treat its latitude axis as a longitude:
/// `astAxOffset` wraps -0.1 into [0, 2pi] on either axis, and a CmpFrame
/// holding the SkyFrame reports no Top or Bottom limit on its latitude.
/// Recorded in `docs/issues/c-library-quirks.md`.
#[test]
fn own_axes_have_no_is_latitude() {
    let sky = SkyFrame::unset_system();
    for axis in [1, 2] {
        let v = sky.ax_offset(axis, -0.1, 0.0).unwrap();
        assert!(
            (v - (2.0 * std::f64::consts::PI - 0.1)).abs() < 1e-12,
            "axis {axis}: {v}"
        );
    }
    assert!(
        (sky.ax_distance(2, 0.1, 6.2).unwrap() - (6.1 - 2.0 * std::f64::consts::PI)).abs() < 1e-12
    );
    let cmp = ast_core::frames::CmpFrame::new(
        Box::new(sky.clone()),
        Box::new(ast_core::frames::BasicFrameBuilder::new(1).build().unwrap()),
    );
    assert_eq!(Frame::top(&cmp, 2).unwrap(), f64::MAX);
    assert_eq!(Frame::top(&sky, 2).unwrap(), std::f64::consts::FRAC_PI_2);
}
