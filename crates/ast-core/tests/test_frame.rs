//! Rust equivalent of the `NormUnit` checks in
//! `reference/starlink-ast/ast_tester/testframe.c`.
//!
//! Divergences from the C test:
//!
//! - The `astAxAngle` block at the end of `testframe.c` is covered by
//!   `test_frame_geometry.rs`.
//! - The table of Unit / NormUnit pairs below is not in `testframe.c`. It
//!   was captured from the C library at the submodule pin with a probe
//!   that sets `Unit(1)` on a one-axis Frame and reads `NormUnit(1)`, so
//!   it pins the normaliser itself rather than only the Frame plumbing.
//!   It includes the cases upstream's `testunitnorm.c` checks through the
//!   protected `astUnitNormaliser`.

use ast_core::frames::{
    BasicFrameBuilder, CmpFrame, DSBSpecFrame, FluxFrame, FluxSystem, Frame, SkyFrame, SkySystem,
    SpecFrame, SpecSystem, TimeFrame, TimeSystem,
};

fn one_axis_frame(unit: &str) -> ast_core::frames::BasicFrame {
    BasicFrameBuilder::new(1).unit(1, unit).build().unwrap()
}

#[test]
fn unit_is_simplified() {
    assert_eq!(one_axis_frame("s*(m/s)").norm_unit(1), "m");
}

/// NormUnit is documented to equal Unit when no simplification can be
/// performed. The Frames below report a Unit of their own while leaving the
/// axis Unit unset, which is where the two are easiest to get out of step,
/// so the relationship is asserted rather than the individual strings.
#[test]
fn norm_unit_equals_unit_when_it_cannot_be_simplified() {
    let frames: Vec<(&str, Box<dyn Frame>)> = vec![
        (
            "Frame",
            Box::new(BasicFrameBuilder::new(2).build().unwrap()),
        ),
        ("SkyFrame", Box::new(SkyFrame::new(SkySystem::Icrs))),
        ("SpecFrame", Box::new(SpecFrame::new(SpecSystem::Wavelen))),
        ("TimeFrame", Box::new(TimeFrame::new(TimeSystem::Mjd))),
    ];
    for (name, frame) in &frames {
        for axis in 1..=frame.naxes() {
            assert_eq!(
                frame.norm_unit(axis),
                frame.effective_unit(axis),
                "NormUnit({axis}) of a default {name}"
            );
        }
    }
}

#[test]
fn norm_unit_follows_a_permutation() {
    let mut frame = BasicFrameBuilder::new(2)
        .unit(1, "km")
        .unit(2, "s*(m/s)")
        .build()
        .unwrap();
    frame.perm_axes(&[2, 1]).unwrap();
    assert_eq!(frame.norm_unit(1), "m");
    assert_eq!(frame.norm_unit(2), "km");
}

/// A SkyFrame axis whose Unit describes its values ("hh:mm:ss.s") reports
/// that description unchanged; one whose Unit names a unit ("degrees") has
/// it normalised. Values captured from the C library.
#[test]
fn sky_frame_norm_unit() {
    let cases = [
        (SkySystem::Icrs, ["hh:mm:ss.s", "ddd:mm:ss"]),
        (SkySystem::Fk5, ["hh:mm:ss.s", "ddd:mm:ss"]),
        (SkySystem::Galactic, ["deg", "deg"]),
        (SkySystem::Ecliptic, ["deg", "deg"]),
        (SkySystem::Azel, ["deg", "deg"]),
    ];
    for (system, expected) in cases {
        let mut sky = SkyFrame::new(system);
        assert_eq!(sky.norm_unit(1), expected[0], "{system:?} axis 1");
        assert_eq!(sky.norm_unit(2), expected[1], "{system:?} axis 2");

        sky.perm_axes(&[2, 1]).unwrap();
        assert_eq!(sky.norm_unit(1), expected[1], "permuted {system:?} axis 1");
        assert_eq!(sky.norm_unit(2), expected[0], "permuted {system:?} axis 2");
    }
}

/// A SkyFrame axis reports a description of its values rather than a unit
/// for them whenever its Format calls for more than one sexagesimal field,
/// or for a single field of a time. Those descriptions contain spaces,
/// which the units parser reads as multiplication, so they are left alone.
/// A Format that names a single field of an angle, and a Unit set
/// explicitly, both give units expressions, which are simplified.
#[test]
fn sky_frame_norm_unit_follows_the_format() {
    let cases = [
        ("bhms", None, "hh mm ss", "hh mm ss"),
        ("bdms.2", None, "ddd mm ss.ss", "ddd mm ss.ss"),
        ("btm", None, "minutes of time", "minutes of time"),
        ("bts", None, "seconds of time", "seconds of time"),
        ("bd", None, "degrees", "deg"),
        ("", Some("s*(m/s)"), "s*(m/s)", "m"),
    ];
    for (format, unit, expected_unit, expected_norm) in cases {
        let mut sky = SkyFrame::new(SkySystem::Icrs);
        if !format.is_empty() {
            sky.set_format_str(1, format).unwrap();
        }
        if let Some(unit) = unit {
            sky.set_unit(1, unit).unwrap();
        }
        assert_eq!(
            sky.unit(1).unwrap(),
            expected_unit,
            "Unit(1) with {format:?} {unit:?}"
        );
        assert_eq!(
            sky.norm_unit(1),
            expected_norm,
            "NormUnit(1) with {format:?} {unit:?}"
        );
    }
}

#[test]
fn spectral_time_and_flux_frames_normalise_the_unit_they_report() {
    let mut freq = SpecFrame::new(SpecSystem::Freq);
    assert_eq!(freq.norm_unit(1), "GHz");
    freq.set_unit("MHz*1000");
    assert_eq!(freq.norm_unit(1), "GHz");
    assert_eq!(SpecFrame::new(SpecSystem::Wavenum).norm_unit(1), "m**-1");

    let mut time = TimeFrame::new(TimeSystem::Mjd);
    assert_eq!(time.norm_unit(1), "d");
    time.set_unit("1000*s");
    assert_eq!(time.norm_unit(1), "ks");

    let mut flux = FluxFrame::new(FluxSystem::FluxDen);
    assert_eq!(flux.norm_unit(1), "(W/Hz)*m**-2");
    flux.set_unit("erg/s/cm**2/Hz");
    assert_eq!(flux.norm_unit(1), "(erg/Hz)*cm**-2/s");
    assert_eq!(
        FluxFrame::new(FluxSystem::SBright).norm_unit(1),
        "(W/Hz)*arcmin**-2*m**-2"
    );

    assert_eq!(
        DSBSpecFrame::new(SpecSystem::Wavelen).norm_unit(1),
        "Angstrom"
    );
}

#[test]
fn cmp_frame_norm_unit_comes_from_the_component() {
    let cmp = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Galactic)),
        Box::new(one_axis_frame("s*(m/s)")),
    );
    assert_eq!(cmp.norm_unit(1), "deg");
    assert_eq!(cmp.norm_unit(2), "deg");
    assert_eq!(cmp.norm_unit(3), "m");
}

/// `(Unit, NormUnit)` pairs read back from a one-axis C Frame.
///
/// A result is blank only when it is wholly a constant (`"2"`), and a
/// constant is folded into a multiplier prefix only when one is closer
/// (`"10/m"` is `"1/dm"`, `"2/m"` stays as it is). `nano m` becomes `N*m`
/// and `hh mm ss` becomes `hh*mm*s` because the parser reads the spaces as
/// multiplication; the SkyFrame avoids the latter by not normalising a
/// Unit that describes its axis values.
const C_ORACLE: &[(&str, &str)] = &[
    ("", ""),
    ("s*(m/s)", "m"),
    ("km", "km"),
    ("0.01 m", "cm"),
    ("2*m", "2*m"),
    ("1000 m", "km"),
    ("100*m", "hm"),
    ("m/100", "cm"),
    ("1/m", "m**-1"),
    ("m/2", "0.5*m"),
    ("10/m", "1/dm"),
    ("1000/m", "1/mm"),
    ("2/m", "2/m"),
    ("0.5/m", "0.5/m"),
    ("3/s", "3/s"),
    ("2/km", "2/km"),
    ("1/(2*m)", "0.5/m"),
    ("Angstrom", "Angstrom"),
    ("log(Hz)", "ln(Hz**2.3025850929940459)"),
    ("m**0.5", "sqrt(m)"),
    ("sqrt(m)", "sqrt(m)"),
    ("Jy/beam", "Jy/beam"),
    ("ddd:mm:ss", "ddd:mm:ss"),
    ("hh mm ss", "hh*mm*s"),
    ("degrees", "deg"),
    ("deg", "deg"),
    ("arcsec", "arcsec"),
    ("km/s", "km/s"),
    ("erg/s/cm**2", "erg*cm**-2/s"),
    ("0.5", ""),
    ("2", ""),
    ("m*m", "m**2"),
    ("mJy", "mJy"),
    ("1000*Jy", "kJy"),
    ("MHz*1000", "GHz"),
    ("ln(m)", "ln(m)"),
    ("exp(m)", "exp(m)"),
    ("10**(m)", "10**(m)"),
    ("m**2*s**-1", "m**2/s"),
    ("(m/s)**2", "(m**2)*s**-2"),
    ("s**-1", "s**-1"),
    ("cm/(s*K)", "cm/K/s"),
    ("furlong", "furlong"),
    ("1.5 furlong", "1.5*furlong"),
    ("nano m", "N*m"),
    ("micron", "um"),
];

#[test]
fn norm_unit_matches_the_c_library() {
    let mut failures = Vec::new();
    for &(unit, expected) in C_ORACLE {
        let got = one_axis_frame(unit).norm_unit(1);
        if got != expected {
            failures.push(format!("Unit {unit:?}: got {got:?}, C gives {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An axis picked out of a SkyFrame keeps the SkyFrame's Unit and NormUnit.
/// C sets the SkyFrame's Unit explicitly on the picked axis, and SkyAxis
/// leaves a set Unit that describes the axis values unchanged, as it does a
/// default one.
#[test]
fn picked_sky_axis_keeps_its_norm_unit() {
    let cases = [
        (SkySystem::Icrs, ["hh:mm:ss.s", "ddd:mm:ss"]),
        (SkySystem::Galactic, ["deg", "deg"]),
    ];
    for (system, expected) in cases {
        let sky = SkyFrame::new(system);
        for (axis, want) in [(0, expected[0]), (1, expected[1])] {
            let picked = sky.pick_axes_snapshot(&[axis]).unwrap();
            assert_eq!(
                picked.effective_unit(1),
                sky.effective_unit(axis as usize + 1)
            );
            assert_eq!(picked.norm_unit(1), want, "{system:?} axis {}", axis + 1);
        }
    }
}
