// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a SkyFrame: AsTime, Equinox, NegLon, SkyTol,
//! Projection, SkyRef, SkyRefIs, SkyRefP and AlignOffset, and the read-only
//! IsLatAxis, IsLonAxis, LatAxis and LonAxis.
// C: skyframe.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, whose
// parent is Frame. System and AlignSystem are Frame's names, with SkyFrame's
// systems (`FrameClass`).

use super::frame::{FrameClass, frame_clear, frame_get, frame_set, frame_test, validate_axis};
use super::{
    axis_double_value, axis_int_value, axis_name, double_value, fmt_decimal_yr, fmt_double,
    fmt_int, int_value, is_read_only, read_only_clear, read_only_set, string_value,
};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::date_time::read_date_time_checked;
use crate::frames::{Frame, SkyFrame, SkyRefIs, SkySystem};
use crate::pal;
use crate::serial::native_write::DBL_DIG;

/// The name of a sky coordinate system as a get returns it.
// C: skyframe.c SystemString.
fn system_name(system: SkySystem) -> &'static str {
    match system {
        SkySystem::Unknown => "Unknown",
        other => other.as_str(),
    }
}

impl FrameClass for SkyFrame {
    fn set_system_text(&mut self, text: &str) -> AstResult<bool> {
        Ok(SkySystem::parse(text).map(|s| self.set_system(s)).is_some())
    }
    fn set_align_system_text(&mut self, text: &str) -> AstResult<bool> {
        Ok(SkySystem::parse(text)
            .map(|s| self.set_align_system(s))
            .is_some())
    }
    fn system_text(&self) -> AstResult<String> {
        Ok(system_name(self.system()).to_string())
    }
    fn align_system_text(&self) -> AstResult<String> {
        Ok(system_name(self.align_system()).to_string())
    }
    fn test_system_attr(&self) -> bool {
        self.test_system()
    }
    fn test_align_system_attr(&self) -> bool {
        self.test_align_system()
    }
    fn clear_system_attr(&mut self) -> AstResult<()> {
        self.clear_system();
        Ok(())
    }
    fn clear_align_system_attr(&mut self) -> AstResult<()> {
        self.clear_align_system();
        Ok(())
    }
}

/// The two values of a `SkyRef` or `SkyRefP` setting given as one string,
/// "lon, lat" in the order and formats of the SkyFrame's axes; `None` if
/// the whole of it is not two axis values with a comma between them.
// C: astUnformat on axis 1, a comma, astUnformat on axis 2, and the counts
// of characters read adding up to the length of the string.
fn axis_pair(frame: &SkyFrame, value: &str) -> AstResult<Option<(f64, f64)>> {
    let (first, nc) = frame.unformat(1, value)?;
    if value.as_bytes().get(nc) != Some(&b',') {
        return Ok(None);
    }
    let rest = &value[nc + 1..];
    let (second, nc2) = frame.unformat(2, rest)?;
    Ok((nc2 == rest.len()).then_some((first, second)))
}

/// The error for a `SkyRef` or `SkyRefP` string that is not two axis values.
fn bad_pair(value: &str, attr: &str) -> AstError {
    let shown = value.trim_end_matches(|c: char| c.is_ascii() && super::is_space(c as u8));
    AstError::new(
        ErrorCode::Badoc,
        format!(
            "astSetAttrib(SkyFrame): Invalid axis values string \"{shown}\" given for {attr} \
             attribute."
        ),
    )
}

/// The year a get returns for an Epoch or Equinox held as an MJD: Besselian
/// before 1984.0 and Julian from then on.
fn year_text(mjd: f64) -> String {
    let year = if mjd < pal::epj2d(1984.0) {
        pal::epb(mjd)
    } else {
        pal::epj(mjd)
    };
    fmt_decimal_yr(year, DBL_DIG)
}

/// Whether the text starts as one of the read-only attributes SkyFrame
/// recognises by how they start, or is one of the others.
fn is_axis_read_only(name: &str) -> bool {
    name.starts_with("islataxis") || name.starts_with("islonaxis")
}

/// SkyFrame's `SetAttrib`.
pub(crate) fn skyframe_set(frame: &mut SkyFrame, setting: &str) -> AstResult<()> {
    if let Some((axis, v)) = axis_int_value(setting, "astime") {
        let axis = validate_axis(frame, axis, "astSetAsTime")?;
        frame.set_as_time(axis, v != 0)
    } else if let Some(v) = string_value(setting, "equinox") {
        match read_date_time_checked(v) {
            Ok(mjd) => {
                frame.set_equinox_mjd(mjd);
                Ok(())
            }
            // C reports the reader's message and then its own.
            Err(first) => Err(AstError::new(ErrorCode::Attin, first).with_context(format!(
                "astSetAttrib(SkyFrame): Invalid equinox value \"{v}\" given for sky coordinate \
                 system."
            ))),
        }
    } else if let Some(v) = int_value(setting, "neglon") {
        frame.set_neg_lon(v != 0);
        Ok(())
    } else if let Some(v) = double_value(setting, "skytol") {
        frame.set_sky_tol(v);
        Ok(())
    } else if let Some(v) = string_value(setting, "projection") {
        frame.set_projection(v);
        Ok(())
    } else if let Some(v) = string_value(setting, "skyref") {
        match axis_pair(frame, v)? {
            Some((lon, lat)) => {
                frame.set_sky_ref(1, lon)?;
                frame.set_sky_ref(2, lat)
            }
            None => Err(bad_pair(v, "SkyRef")),
        }
    } else if let Some((axis, v)) = axis_double_value(setting, "skyref") {
        let axis = validate_axis(frame, axis, "astSetSkyRef")?;
        frame.set_sky_ref(axis, v)
    } else if let Some(v) = string_value(setting, "skyrefis") {
        match SkyRefIs::parse(v) {
            Some(mode) => {
                frame.set_sky_ref_is(mode);
                Ok(())
            }
            None => Err(AstError::new(
                ErrorCode::Opt,
                format!("astSet(SkyFrame): option '{v}' is unknown in '{setting}'."),
            )),
        }
    } else if let Some(v) = string_value(setting, "skyrefp") {
        match axis_pair(frame, v)? {
            Some((lon, lat)) => {
                frame.set_sky_ref_p(1, lon)?;
                frame.set_sky_ref_p(2, lat)
            }
            None => Err(bad_pair(v, "SkyRefP")),
        }
    } else if let Some((axis, v)) = axis_double_value(setting, "skyrefp") {
        let axis = validate_axis(frame, axis, "astSetSkyRefP")?;
        frame.set_sky_ref_p(axis, v)
    } else if let Some(v) = int_value(setting, "alignoffset") {
        frame.set_align_offset(v != 0);
        Ok(())
    } else if is_axis_read_only(setting)
        || is_read_only(setting, "lataxis")
        || is_read_only(setting, "lonaxis")
    {
        Err(read_only_set(setting, "SkyFrame"))
    } else {
        frame_set(frame, setting)
    }
}

/// SkyFrame's `GetAttrib`.
pub(crate) fn skyframe_get(frame: &SkyFrame, name: &str) -> AstResult<String> {
    let flag = |v: bool| Ok(fmt_int(i32::from(v)));
    let index = |v: usize| Ok(fmt_int(i32::try_from(v).unwrap_or(i32::MAX)));
    // IsLatAxis and IsLonAxis are false for an index that is not an axis.
    let on_axis = |axis: i32, is: &dyn Fn(usize) -> AstResult<bool>| {
        usize::try_from(axis).ok().and_then(|a| is(a).ok()) == Some(true)
    };
    if let Some(axis) = axis_name(name, "astime") {
        let axis = validate_axis(frame, axis, "astGetAsTime")?;
        flag(frame.as_time(axis)?)
    } else if name == "equinox" {
        Ok(year_text(frame.equinox_mjd()))
    } else if let Some(axis) = axis_name(name, "islataxis") {
        flag(on_axis(axis, &|a| frame.is_lat_axis(a)))
    } else if let Some(axis) = axis_name(name, "islonaxis") {
        flag(on_axis(axis, &|a| frame.is_lon_axis(a)))
    } else if name == "lataxis" {
        index(frame.lat_axis())
    } else if name == "lonaxis" {
        index(frame.lon_axis())
    } else if name == "neglon" {
        flag(frame.neg_lon())
    } else if name == "skytol" {
        Ok(fmt_double(frame.sky_tol()))
    } else if name == "projection" {
        Ok(frame.projection().unwrap_or("").to_string())
    } else if name == "skyref" {
        Ok(format!(
            "{}, {}",
            frame.format(1, frame.sky_ref(1)?)?,
            frame.format(2, frame.sky_ref(2)?)?
        ))
    } else if let Some(axis) = axis_name(name, "skyref") {
        let axis = validate_axis(frame, axis, "astGetSkyRef")?;
        Ok(fmt_double(frame.sky_ref(axis)?))
    } else if name == "skyrefp" {
        Ok(format!(
            "{}, {}",
            frame.format(1, frame.sky_ref_p(1)?)?,
            frame.format(2, frame.sky_ref_p(2)?)?
        ))
    } else if let Some(axis) = axis_name(name, "skyrefp") {
        let axis = validate_axis(frame, axis, "astGetSkyRefP")?;
        Ok(fmt_double(frame.sky_ref_p(axis)?))
    } else if name == "skyrefis" {
        Ok(match frame.sky_ref_is() {
            SkyRefIs::Pole => "Pole",
            SkyRefIs::Ignored => "Ignored",
            SkyRefIs::Origin => "Origin",
        }
        .to_string())
    } else if name == "alignoffset" {
        flag(frame.align_offset())
    } else {
        frame_get(frame, name)
    }
}

/// SkyFrame's `TestAttrib`.
pub(crate) fn skyframe_test(frame: &SkyFrame, name: &str) -> AstResult<bool> {
    if let Some(axis) = axis_name(name, "astime") {
        let axis = validate_axis(frame, axis, "astTestAsTime")?;
        frame.test_as_time(axis)
    } else if name == "equinox" {
        Ok(frame.test_equinox())
    } else if name == "neglon" {
        Ok(frame.test_neg_lon())
    } else if name == "skytol" {
        Ok(frame.test_sky_tol())
    } else if name == "projection" {
        Ok(frame.test_projection())
    } else if name == "skyrefis" {
        Ok(frame.test_sky_ref_is())
    } else if name == "skyref" {
        Ok(frame.test_sky_ref(1)? || frame.test_sky_ref(2)?)
    } else if let Some(axis) = axis_name(name, "skyref") {
        let axis = validate_axis(frame, axis, "astTestSkyRef")?;
        frame.test_sky_ref(axis)
    } else if name == "skyrefp" {
        Ok(frame.test_sky_ref_p(1)? || frame.test_sky_ref_p(2)?)
    } else if let Some(axis) = axis_name(name, "skyrefp") {
        let axis = validate_axis(frame, axis, "astTestSkyRefP")?;
        frame.test_sky_ref_p(axis)
    } else if name == "alignoffset" {
        Ok(frame.test_align_offset())
    } else if is_axis_read_only(name) || name == "lataxis" || name == "lonaxis" {
        Ok(false)
    } else {
        frame_test(frame, name)
    }
}

/// SkyFrame's `ClearAttrib`.
pub(crate) fn skyframe_clear(frame: &mut SkyFrame, name: &str) -> AstResult<()> {
    if let Some(axis) = axis_name(name, "astime") {
        let axis = validate_axis(frame, axis, "astClearAsTime")?;
        frame.clear_as_time(axis)
    } else if name == "equinox" {
        frame.clear_equinox();
        Ok(())
    } else if name == "neglon" {
        frame.clear_neg_lon();
        Ok(())
    } else if name == "projection" {
        frame.clear_projection();
        Ok(())
    } else if name == "skyref" {
        frame.clear_sky_ref(1)?;
        frame.clear_sky_ref(2)
    } else if name == "skytol" {
        frame.clear_sky_tol();
        Ok(())
    } else if let Some(axis) = axis_name(name, "skyref") {
        let axis = validate_axis(frame, axis, "astClearSkyRef")?;
        frame.clear_sky_ref(axis)
    } else if name == "skyrefp" {
        frame.clear_sky_ref_p(1)?;
        frame.clear_sky_ref_p(2)
    } else if let Some(axis) = axis_name(name, "skyrefp") {
        let axis = validate_axis(frame, axis, "astClearSkyRefP")?;
        frame.clear_sky_ref_p(axis)
    } else if name == "skyrefis" {
        frame.clear_sky_ref_is();
        Ok(())
    } else if name == "alignoffset" {
        frame.clear_align_offset();
        Ok(())
    } else if is_axis_read_only(name) || name == "lataxis" || name == "lonaxis" {
        Err(read_only_clear(name, "SkyFrame"))
    } else {
        frame_clear(frame, name)
    }
}
