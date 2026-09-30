//! SkyFrame axis formatting, Units and serialisation, checked against the C
//! library.
//!
//! Every expected value comes from `skyframe_oracle/cases.rs`, which
//! `tools/oracle_fixtures/skyframe_format_probe.c` generates by running the
//! same settings through the C library at the submodule pin. Each case names
//! an attribute setting string as `astSet` takes it; [`sky_frame`] applies it
//! through the public Rust API, so the tests also show which setter
//! corresponds to each C attribute.
//!
//! It extends the SkyFrame block of `ast_tester/testframe.c`, ported in
//! `test_frame.rs`, which checks the Unit and NormUnit of a SkyFrame with six
//! Formats; the tables here cover more Formats, Digits and AsTime settings
//! and the values they format.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "skyframe_oracle/cases.rs"]
mod cases;

use ast_core::frames::{Frame, SkyFrame, SkySystem};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Build a SkyFrame from a System name and an `astSet`-style setting string
/// such as `"Digits(2)=10,Format(2)=dms.*"`.
fn sky_frame(system: &str, attrs: &str) -> SkyFrame {
    let mut sky = SkyFrame::new(match system {
        "ICRS" => SkySystem::Icrs,
        "GALACTIC" => SkySystem::Galactic,
        other => panic!("unexpected System {other}"),
    });
    apply(&mut sky, attrs);
    sky
}

fn apply(sky: &mut SkyFrame, attrs: &str) {
    for setting in attrs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, value) = setting.split_once('=').expect("attr=value");
        let (attr, axis) = match name.split_once('(') {
            Some((attr, rest)) => (attr, Some(rest.trim_end_matches(')').parse().unwrap())),
            None => (name, None),
        };
        match (attr, axis) {
            ("System", None) => sky.set_system(match value.to_ascii_uppercase().as_str() {
                "GALACTIC" => SkySystem::Galactic,
                "ICRS" => SkySystem::Icrs,
                other => panic!("unexpected System {other}"),
            }),
            ("Digits", None) => sky.set_digits(value.parse().unwrap()).unwrap(),
            ("Digits", Some(axis)) => sky.set_axis_digits(axis, value.parse().unwrap()).unwrap(),
            ("Format", Some(axis)) => sky.set_format_str(axis, value).unwrap(),
            ("Unit", Some(axis)) => sky.set_unit(axis, value).unwrap(),
            ("AsTime", Some(axis)) => sky.set_as_time(axis, value == "1").unwrap(),
            ("Label", Some(axis)) => sky.set_axis_label(axis, value).unwrap(),
            other => panic!("unsupported setting {other:?}"),
        }
    }
}

#[test]
fn format_matches_the_c_library() {
    let mut failures = Vec::new();
    for &(system, attrs, axis, value, expected) in cases::FORMAT_CASES {
        let got = sky_frame(system, attrs).format(axis, value).unwrap();
        if got != expected {
            failures.push(format!(
                "{system} {attrs:?} axis {axis} value {value}: got {got:?}, C gives {expected:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases::FORMAT_CASES.len(),
        failures.join("\n")
    );
}

#[test]
fn format_unit_and_norm_unit_match_the_c_library() {
    let mut failures = Vec::new();
    for &(system, attrs, axis, format, unit, norm) in cases::UNIT_CASES {
        let sky = sky_frame(system, attrs);
        let got = (
            sky.format_str(axis).unwrap(),
            sky.unit(axis).unwrap(),
            sky.norm_unit(axis),
        );
        if got != (format.to_string(), unit.to_string(), norm.to_string()) {
            failures.push(format!(
                "{system} {attrs:?} axis {axis}: got {got:?}, C gives {:?}",
                (format, unit, norm)
            ));
        }
        assert_eq!(sky.effective_unit(axis), sky.unit(axis).unwrap());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The dump the port writes for each setting is the one C writes, and
/// reading C's dump back gives a SkyFrame that writes it again.
#[test]
fn dump_matches_the_c_library() {
    for &(attrs, expected) in cases::DUMP_CASES {
        let mut sky = SkyFrame::new(SkySystem::Icrs);
        apply(&mut sky, attrs);
        let written = to_native_string(&AnyObject::SkyFrame(sky)).unwrap();
        assert_eq!(written, expected, "dump for {attrs:?}");

        let read = from_native_string(expected).unwrap();
        let rewritten = to_native_string(&read).unwrap();
        assert_eq!(rewritten, expected, "round trip of C's dump for {attrs:?}");
    }
}

/// The attributes a C dump carries are the ones the SkyFrame reads back.
#[test]
fn a_read_dump_keeps_its_attributes() {
    let (_, dump) = cases::DUMP_CASES
        .iter()
        .find(|(attrs, _)| attrs.starts_with("Label(1)=RA"))
        .expect("the combined case");
    let AnyObject::SkyFrame(sky) = from_native_string(dump).unwrap() else {
        panic!("not a SkyFrame");
    };
    assert_eq!(sky.format_str(1).unwrap(), "bhms");
    assert!(sky.test_format_str(1).unwrap());
    assert_eq!(sky.unit(1).unwrap(), "hh mm ss");
    assert!(sky.test_unit(1).unwrap());
    assert_eq!(sky.axis_digits(1).unwrap(), 8);
    assert!(sky.test_axis_digits(1).unwrap());
    assert!(sky.as_time(1).unwrap());
    assert!(sky.test_as_time(1).unwrap());
    assert!(!sky.test_format_str(2).unwrap());
    assert_eq!(sky.norm_unit(1), "hh mm ss");
}

/// Clearing an attribute returns the axis to the default the System and
/// the remaining attributes give it.
#[test]
fn clearing_restores_the_default() {
    let mut sky = sky_frame("ICRS", "Format(1)=bhms,Unit(1)=deg,Digits(1)=9,AsTime(1)=0");
    sky.clear_format_str(1).unwrap();
    sky.clear_unit(1).unwrap();
    sky.clear_axis_digits(1).unwrap();
    sky.clear_as_time(1).unwrap();
    assert_eq!(sky.format_str(1).unwrap(), "hms.1");
    assert_eq!(sky.unit(1).unwrap(), "hh:mm:ss.s");

    sky.set_digits(3).unwrap();
    assert_eq!(sky.format_str(2).unwrap(), "d");
    sky.clear_digits();
    assert_eq!(sky.format_str(2).unwrap(), "dms");
}

/// Attributes follow the axis they were set on when the axes are permuted.
#[test]
fn attributes_follow_a_permutation() {
    let mut sky = sky_frame("ICRS", "Format(1)=bhms,Digits(2)=9");
    sky.perm_axes(&[2, 1]).unwrap();
    assert_eq!(sky.format_str(2).unwrap(), "bhms");
    assert_eq!(sky.unit(2).unwrap(), "hh mm ss");
    assert_eq!(sky.format_str(1).unwrap(), "dms.2");
    assert_eq!(sky.format(2, 1.0).unwrap(), "3 49 11");
}

#[test]
fn bad_and_nan_format_as_bad() {
    let sky = sky_frame("ICRS", " ");
    assert_eq!(
        sky.format(1, ast_core::constants::BAD_VALUE).unwrap(),
        "<bad>"
    );
    assert_eq!(sky.format(1, f64::NAN).unwrap(), "<bad>");
    assert!(sky.format(3, 0.0).is_err());
}
