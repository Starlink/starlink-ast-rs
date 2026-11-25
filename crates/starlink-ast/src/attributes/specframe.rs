// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a SpecFrame (StdOfRest, RestFreq, SourceVel, RefRA and
//! the rest) and of a DSBSpecFrame (DSBCentre, IF, SideBand, AlignSideBand
//! and the read-only ImagFreq).
//!
//! The values are in the units the C library's strings use, not the SI units
//! of the typed accessors: RestFreq and IF in GHz unless a unit follows the
//! number, SourceVel in km/s for a velocity, SpecOrigin and DSBCentre in the
//! Frame's current Unit.
// C: specframe.c and dsbspecframe.c SetAttrib, GetAttrib, TestAttrib and
// ClearAttrib; SpecFrame's parent is Frame and DSBSpecFrame's is SpecFrame.

use super::frame::{FrameClass, frame_clear, frame_get, frame_set, frame_test};
use super::{
    ShortcutGuard, double_value, fmt_double, fmt_int, int_value, is_read_only, keyword_value,
    read_only_clear, read_only_set, scan_whole, tran1, unknown_set, word_value, word_value_whole,
};
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::chr_match;
use crate::frames::StdOfRest;
use crate::frames::axis_format::sprintf_double;
use crate::frames::{DSBSpecFrame, Frame, SideBand, SkyFrame, SkySystem, SpecFrame, SpecSystem};
use crate::units::unit_mapper;

/// The speed of light in m/s and Planck's constant in J.s, with the values
/// the C library converts a rest wavelength or energy with.
// C: AST__C and AST__H.
const SPEED_OF_LIGHT: f64 = 2.99792458e8;
const PLANCK: f64 = 6.6260755e-34;

/// A class that is a SpecFrame: the SpecFrame itself, and a DSBSpecFrame,
/// which holds one.
pub(crate) trait SpecClass: FrameClass {
    /// The SpecFrame part of the object.
    fn spec_part(&self) -> &SpecFrame;
    /// The SpecFrame part of the object, for changing.
    fn spec_part_mut(&mut self) -> &mut SpecFrame;
}

impl SpecClass for SpecFrame {
    fn spec_part(&self) -> &SpecFrame {
        self
    }
    fn spec_part_mut(&mut self) -> &mut SpecFrame {
        self
    }
}

impl SpecClass for DSBSpecFrame {
    fn spec_part(&self) -> &SpecFrame {
        self.spec()
    }
    fn spec_part_mut(&mut self) -> &mut SpecFrame {
        self.spec_mut()
    }
}

/// System and AlignSystem by name, for both classes.
macro_rules! spec_frame_class {
    ($class:ty) => {
        impl FrameClass for $class {
            fn set_system_text(&mut self, text: &str) -> AstResult<bool> {
                match SpecSystem::parse(text) {
                    Some(sys) => self.spec_part_mut().set_system(sys).map(|()| true),
                    None => Ok(false),
                }
            }
            fn set_align_system_text(&mut self, text: &str) -> AstResult<bool> {
                Ok(SpecSystem::parse(text)
                    .map(|sys| self.spec_part_mut().set_align_system(sys))
                    .is_some())
            }
            fn system_text(&self) -> AstResult<String> {
                Ok(self.spec_part().system().as_str().to_string())
            }
            fn align_system_text(&self) -> AstResult<String> {
                Ok(self.spec_part().align_system().as_str().to_string())
            }
            fn test_system_attr(&self) -> bool {
                self.spec_part().test_system()
            }
            fn test_align_system_attr(&self) -> bool {
                self.spec_part().test_align_system()
            }
            fn clear_system_attr(&mut self) -> AstResult<()> {
                self.spec_part_mut().clear_system()
            }
            fn clear_align_system_attr(&mut self) -> AstResult<()> {
                self.spec_part_mut().clear_align_system();
                Ok(())
            }
        }
    };
}
spec_frame_class!(SpecFrame);
spec_frame_class!(DSBSpecFrame);

/// The axis attributes a Frame of one spectral or time axis takes without
/// an index.
pub(crate) const AXIS_SHORTCUTS: [&str; 7] = [
    "direction",
    "bottom",
    "top",
    "format",
    "label",
    "symbol",
    "unit",
];

/// The FK5 SkyFrame C reads and formats RefRA and RefDec with.
// C: the static "skyframe" of specframe.c,
// astSkyFrame( "system=FK5,equinox=J2000" ).
fn fk5_frame() -> SkyFrame {
    SkyFrame::new(SkySystem::Fk5)
}

/// C's `StdOfRestCode`, with the error `SetAttrib` gives for a name it does
/// not know.
fn std_of_rest(text: &str, class: &str) -> AstResult<StdOfRest> {
    StdOfRest::parse(text).ok_or_else(|| {
        AstError::new(
            ErrorCode::Attin,
            format!("astSetAttrib({class}): Invalid standard of rest description \"{text}\"."),
        )
    })
}

/// A value in the default units of the SpecFrame's System, from `unit`.
// C: specframe.c ToUnits, for the method "astSetSpecOrigin".
fn to_default_units<F: SpecClass + ?Sized>(frame: &F, unit: &str, value: f64) -> AstResult<f64> {
    let def = frame.spec_part().system().default_display_units();
    let map = unit_mapper(unit, def)?.ok_or_else(|| {
        AstError::new(
            ErrorCode::Badun,
            format!(
                "astSetSpecOrigin({}): Cannot convert the supplied attribute value from units \
                 of {unit} to {def}.",
                frame.attr_class()
            ),
        )
    })?;
    tran1(map.as_ref(), value)
}

/// A rest frequency in Hz from a number and the unit that follows it: a
/// frequency, a wavelength or an energy.
fn rest_freq_hz(value: f64, unit: &str, class: &str) -> AstResult<f64> {
    let shown = || format!("{} {unit}", sprintf_double("%g", value));
    if let Some(map) = unit_mapper(unit, "Hz")? {
        tran1(map.as_ref(), value)
    } else if let Some(map) = unit_mapper(unit, "m")? {
        let metres = tran1(map.as_ref(), value)?;
        if metres != BAD_VALUE && metres != 0.0 {
            Ok(SPEED_OF_LIGHT / metres)
        } else {
            Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetAttrib({class}): Invalid rest wavelength \"{}\" supplied.",
                    shown()
                ),
            ))
        }
    } else if let Some(map) = unit_mapper(unit, "J")? {
        let joules = tran1(map.as_ref(), value)?;
        if joules != BAD_VALUE {
            Ok(joules * (1.0 / PLANCK))
        } else {
            Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetAttrib({class}): Invalid rest energy \"{}\" supplied.",
                    shown()
                ),
            ))
        }
    } else {
        Err(AstError::new(
            ErrorCode::Attin,
            format!(
                "astSetAttrib({class}): Rest frequency given in an unsupported system of units \
                 \"{}\".",
                shown()
            ),
        ))
    }
}

/// C's numeric code for a spectral system, which an error quotes.
fn system_code(system: SpecSystem) -> i32 {
    match system {
        SpecSystem::Freq => 1,
        SpecSystem::Energy => 2,
        SpecSystem::Wavenum => 3,
        SpecSystem::Wavelen => 4,
        SpecSystem::Airwave => 5,
        SpecSystem::Vradio => 6,
        SpecSystem::Voptical => 7,
        SpecSystem::Redshift => 8,
        SpecSystem::Beta => 9,
        SpecSystem::Vrel => 10,
    }
}

/// SpecFrame's `SetAttrib`.
pub(crate) fn specframe_set<F: SpecClass + ?Sized>(frame: &mut F, setting: &str) -> AstResult<()> {
    let class = frame.attr_class();
    if let Some(name) = AXIS_SHORTCUTS.iter().find(|name| {
        setting
            .strip_prefix(**name)
            .is_some_and(|r| r.starts_with('='))
    }) {
        // The setting with "(1)" after the name goes straight to the Frame
        // class. C's Frame class hands a setting whose value it cannot read
        // back to this function without the index, which adds it again,
        // without end (c-library-quirks.md); the port stops at the second
        // visit, so that the Axis object's error is reported.
        let Some(_guard) = ShortcutGuard::enter() else {
            return Err(unknown_set(setting, class));
        };
        frame_set(frame, &format!("{name}(1){}", &setting[name.len()..]))
    } else if let Some(v) = word_value_whole(setting, "alignstdofrest") {
        let sor = std_of_rest(v, class)?;
        frame.spec_part_mut().set_align_std_of_rest(sor);
        Ok(())
    } else if word_value(setting, "geolat", 7).is_some()
        || word_value(setting, "geolon", 7).is_some()
    {
        // GeoLat and GeoLon are the old names of ObsLat and ObsLon.
        frame.attr_set(&format!("obs{}", &setting[3..]))
    } else if let Some(v) = word_value(setting, "refdec", 7) {
        let (dec, nc) = fk5_frame().unformat(2, v)?;
        if nc == super::chr_len(v) {
            frame.spec_part_mut().set_ref_dec(dec);
            Ok(())
        } else {
            Err(AstError::new(
                ErrorCode::Attin,
                format!("astSetAttrib({class}): Invalid reference declination \"{v}\"."),
            ))
        }
    } else if let Some(v) = word_value(setting, "refra", 6) {
        let (ra, nc) = fk5_frame().unformat(1, v)?;
        if nc == super::chr_len(v) {
            frame.spec_part_mut().set_ref_ra(ra);
            Ok(())
        } else {
            Err(AstError::new(
                ErrorCode::Attin,
                format!("astSetAttrib({class}): Invalid reference right ascension \"{v}\"."),
            ))
        }
    } else if let Some(v) = int_value(setting, "alignspecoffset") {
        frame.spec_part_mut().set_align_spec_offset(v != 0);
        Ok(())
    } else if let Some(v) = double_value(setting, "restfreq") {
        // A number alone is in GHz.
        frame.spec_part_mut().set_rest_freq(v * 1.0e9);
        Ok(())
    } else if let Some(scan) = scan_whole(setting, "restfreq= %lg %n%*s %n", 1) {
        let hz = rest_freq_hz(scan.double(0), &setting[scan.count(1)..], class)?;
        frame.spec_part_mut().set_rest_freq(hz);
        Ok(())
    } else if let Some(v) = double_value(setting, "sourcevel") {
        frame.spec_part_mut().set_source_vel_cur(v);
        Ok(())
    } else if let Some(v) = word_value_whole(setting, "sourcevrf") {
        let sor = std_of_rest(v, class)?;
        frame.spec_part_mut().set_source_vrf(sor)
    } else if let Some(v) = word_value_whole(setting, "sourcesys") {
        // C passes on whatever code the name gives, and astSetSourceSys
        // rejects one that is not a velocity, redshift or beta.
        match SpecSystem::parse(v) {
            Some(
                sys @ (SpecSystem::Vrel
                | SpecSystem::Beta
                | SpecSystem::Vradio
                | SpecSystem::Redshift
                | SpecSystem::Voptical),
            ) => frame.spec_part_mut().set_source_sys(sys),
            other => Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetSourceSys({class}): Bad value ({}) given for SourceSys attribute.",
                    other.map_or(-1, system_code)
                ),
            )),
        }
    } else if let Some(v) = word_value_whole(setting, "stdofrest") {
        let sor = std_of_rest(v, class)?;
        frame.spec_part_mut().set_std_of_rest(sor)
    } else if let Some(v) = double_value(setting, "specorigin") {
        // A number alone is in the Frame's current Unit.
        let unit = frame.unit(1)?;
        let origin = to_default_units(frame, &unit, v)?;
        frame.spec_part_mut().set_spec_origin(origin);
        Ok(())
    } else if let Some(scan) = scan_whole(setting, "specorigin= %lg %n%*s %n", 1) {
        let origin = to_default_units(frame, &setting[scan.count(1)..], scan.double(0))?;
        frame.spec_part_mut().set_spec_origin(origin);
        Ok(())
    } else {
        frame_set(frame, setting)
    }
}

/// SpecFrame's `GetAttrib`.
pub(crate) fn specframe_get<F: SpecClass + ?Sized>(frame: &F, name: &str) -> AstResult<String> {
    let spec = frame.spec_part();
    if AXIS_SHORTCUTS.contains(&name) {
        return frame_get(frame, &format!("{name}(1)"));
    }
    match name {
        "alignstdofrest" => Ok(spec.align_std_of_rest().as_str().to_string()),
        "alignspecoffset" => Ok(fmt_int(i32::from(spec.align_spec_offset()))),
        "geolat" => frame.attr_get("obslat"),
        "geolon" => frame.attr_get("obslon"),
        "refdec" => fk5_frame().format(2, spec.ref_dec()),
        "refra" => fk5_frame().format(1, spec.ref_ra()),
        "restfreq" => Ok(fmt_double(spec.rest_freq() * 1.0e-9)),
        "sourcevel" => Ok(fmt_double(spec.source_vel_cur())),
        "specorigin" => match spec.spec_origin_cur() {
            Ok(origin) => Ok(fmt_double(origin)),
            // The typed getter's error, in C's words.
            Err(e) if e.code() == ErrorCode::Badun => Err(AstError::new(
                ErrorCode::Badun,
                format!(
                    "astGetSpecOrigin({class}): The current units ({}) are not suitable for a \
                     {class}.",
                    frame.unit(1)?,
                    class = frame.attr_class()
                ),
            )),
            Err(e) => Err(e),
        },
        "sourcevrf" => Ok(spec.source_vrf().as_str().to_string()),
        "sourcesys" => Ok(spec.source_sys().as_str().to_string()),
        "stdofrest" => Ok(spec.std_of_rest().as_str().to_string()),
        _ => frame_get(frame, name),
    }
}

/// SpecFrame's `TestAttrib`.
pub(crate) fn specframe_test<F: SpecClass + ?Sized>(frame: &F, name: &str) -> AstResult<bool> {
    let spec = frame.spec_part();
    if AXIS_SHORTCUTS.contains(&name) {
        return frame_test(frame, &format!("{name}(1)"));
    }
    match name {
        "alignstdofrest" => Ok(spec.test_align_std_of_rest()),
        "geolat" => frame.attr_test("obslat"),
        "geolon" => frame.attr_test("obslon"),
        "refdec" => Ok(spec.test_ref_dec()),
        "refra" => Ok(spec.test_ref_ra()),
        "alignspecoffset" => Ok(spec.test_align_spec_offset()),
        "restfreq" => Ok(spec.test_rest_freq()),
        "sourcevel" => Ok(spec.test_source_vel()),
        "sourcevrf" => Ok(spec.test_source_vrf()),
        "sourcesys" => Ok(spec.test_source_sys()),
        "stdofrest" => Ok(spec.test_std_of_rest()),
        "specorigin" => Ok(spec.test_spec_origin()),
        _ => frame_test(frame, name),
    }
}

/// SpecFrame's `ClearAttrib`.
pub(crate) fn specframe_clear<F: SpecClass + ?Sized>(frame: &mut F, name: &str) -> AstResult<()> {
    if AXIS_SHORTCUTS.contains(&name) {
        return frame_clear(frame, &format!("{name}(1)"));
    }
    match name {
        "geolat" => return frame.attr_clear("obslat"),
        "geolon" => return frame.attr_clear("obslon"),
        _ => {}
    }
    let spec = frame.spec_part_mut();
    match name {
        "alignstdofrest" => spec.clear_align_std_of_rest(),
        "refdec" => spec.clear_ref_dec(),
        "refra" => spec.clear_ref_ra(),
        "alignspecoffset" => spec.clear_align_spec_offset(),
        "restfreq" => spec.clear_rest_freq(),
        "sourcevel" => spec.clear_source_vel(),
        "sourcevrf" => return spec.clear_source_vrf(),
        "sourcesys" => return spec.clear_source_sys(),
        "stdofrest" => return spec.clear_std_of_rest(),
        "specorigin" => spec.clear_spec_origin(),
        _ => return frame_clear(frame, name),
    }
    Ok(())
}

/// The error DSBSpecFrame gives for a setting it recognises and cannot use.
fn dsb_invalid(setting: &str) -> AstError {
    AstError::new(
        ErrorCode::Attin,
        format!(
            "astSetAttrib(DSBSpecFrame): The setting \"{setting}\" is invalid for a DSBSpecFrame."
        ),
    )
}

/// DSBSpecFrame's `SetAttrib`.
pub(crate) fn dsbspecframe_set(frame: &mut DSBSpecFrame, setting: &str) -> AstResult<()> {
    if setting.contains("dsbcentre=") {
        // A number alone is in the Frame's current Unit; one followed by a
        // unit is converted to it.
        let value = if let Some(v) = double_value(setting, "dsbcentre") {
            Some(v)
        } else if let Some(scan) = scan_whole(setting, "dsbcentre= %lg %n%*s %n", 1) {
            let unit = &setting[scan.count(1)..];
            match unit_mapper(unit, &frame.unit(1)?)? {
                Some(map) => Some(tran1(map.as_ref(), scan.double(0))?).filter(|v| *v != BAD_VALUE),
                None => {
                    return Err(AstError::new(
                        ErrorCode::Attin,
                        format!(
                            "astSetAttrib(DSBSpecFrame): Value supplied for attribute \
                             \"DSBCentre\" ({}) uses units which are inappropriate for the \
                             current spectral system ({}).",
                            setting.get(10..).unwrap_or(""),
                            frame.title()?
                        ),
                    ));
                }
            }
        } else {
            None
        };
        match value {
            Some(v) => frame.set_dsb_centre_cur(v).map_err(|e| match e.code() {
                ErrorCode::Attin => dsb_invalid(setting),
                _ => e,
            }),
            None => Err(dsb_invalid(setting)),
        }
    } else if let Some(v) = double_value(setting, "if") {
        // A number alone is in GHz.
        frame.set_if_ghz(v);
        Ok(())
    } else if let Some(scan) = scan_whole(setting, "if= %lg %n%*s %n", 1) {
        let unit = &setting[scan.count(1)..];
        match unit_mapper(unit, "Hz")? {
            Some(map) => {
                frame.set_ifr(tran1(map.as_ref(), scan.double(0))?);
                Ok(())
            }
            None => Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetAttrib(DSBSpecFrame): Intermediate frequency given in an \
                     inappropriate system of units \"{} {unit}\".",
                    sprintf_double("%g", scan.double(0))
                ),
            )),
        }
    } else if let Some(v) = keyword_value(setting, "sideband") {
        // "observed" and "image" name the sideband by the sign of IF.
        let sideband = if chr_match("usb", v) {
            SideBand::USB
        } else if chr_match("lsb", v) {
            SideBand::LSB
        } else if chr_match("lo", v) {
            SideBand::LO
        } else if chr_match("observed", v) {
            if frame.ifr() > 0.0 {
                SideBand::LSB
            } else {
                SideBand::USB
            }
        } else if chr_match("image", v) {
            if frame.ifr() <= 0.0 {
                SideBand::LSB
            } else {
                SideBand::USB
            }
        } else {
            return Err(dsb_invalid(setting));
        };
        frame.set_sideband(sideband);
        Ok(())
    } else if let Some(v) = int_value(setting, "alignsideband") {
        frame.set_align_sideband(v != 0);
        Ok(())
    } else if is_read_only(setting, "imagfreq") {
        Err(read_only_set(setting, "DSBSpecFrame"))
    } else {
        specframe_set(frame, setting)
    }
}

/// DSBSpecFrame's `GetAttrib`.
pub(crate) fn dsbspecframe_get(frame: &DSBSpecFrame, name: &str) -> AstResult<String> {
    match name {
        "dsbcentre" => Ok(fmt_double(frame.dsb_centre_cur()?)),
        "if" => Ok(fmt_double(frame.if_ghz())),
        "imagfreq" => Ok(fmt_double(frame.imag_freq_ghz()?)),
        "sideband" => Ok(match frame.sideband() {
            SideBand::USB => "USB",
            SideBand::LO => "LO",
            SideBand::LSB => "LSB",
        }
        .to_string()),
        "alignsideband" => Ok(fmt_int(i32::from(frame.align_sideband()))),
        _ => specframe_get(frame, name),
    }
}

/// DSBSpecFrame's `TestAttrib`.
pub(crate) fn dsbspecframe_test(frame: &DSBSpecFrame, name: &str) -> AstResult<bool> {
    match name {
        "dsbcentre" => Ok(frame.test_dsb_centre()),
        "if" => Ok(frame.test_if()),
        "sideband" => Ok(frame.test_sideband()),
        "alignsideband" => Ok(frame.test_align_sideband()),
        "imagfreq" => Ok(false),
        _ => specframe_test(frame, name),
    }
}

/// DSBSpecFrame's `ClearAttrib`.
pub(crate) fn dsbspecframe_clear(frame: &mut DSBSpecFrame, name: &str) -> AstResult<()> {
    match name {
        "dsbcentre" => frame.clear_dsb_centre(),
        "if" => frame.clear_if(),
        "sideband" => frame.clear_sideband(),
        "alignsideband" => frame.clear_align_sideband(),
        "imagfreq" => return Err(read_only_clear(name, "DSBSpecFrame")),
        _ => return specframe_clear(frame, name),
    }
    Ok(())
}
