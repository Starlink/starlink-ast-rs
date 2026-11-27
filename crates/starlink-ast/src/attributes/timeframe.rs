// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a TimeFrame: TimeScale, AlignTimeScale, LTOffset,
//! TimeOrigin, and ClockLat and ClockLon, which are ObsLat and ObsLon under
//! other names.
// C: timeframe.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, whose
// parent is Frame.

use super::frame::{FrameClass, frame_clear, frame_get, frame_set, frame_test};
use super::specframe::AXIS_SHORTCUTS;
use super::{
    ShortcutGuard, double_value, fmt_double, scan_whole, string_value, tran1, unknown_set,
    word_value, word_value_whole,
};
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::date_time::read_date_time_checked;
use crate::frames::{Frame, TimeFrame, TimeScale, TimeSystem};
use crate::mappings::{TimeCvtType, TimeMap};
use crate::units::unit_mapper;

impl FrameClass for TimeFrame {
    fn set_system_text(&mut self, text: &str) -> AstResult<bool> {
        match TimeSystem::parse(text) {
            Some(sys) => self.set_system(sys).map(|()| true),
            None => Ok(false),
        }
    }
    fn set_align_system_text(&mut self, text: &str) -> AstResult<bool> {
        Ok(TimeSystem::parse(text)
            .map(|sys| self.set_align_system(sys))
            .is_some())
    }
    fn system_text(&self) -> AstResult<String> {
        Ok(self.system().as_str().to_string())
    }
    fn align_system_text(&self) -> AstResult<String> {
        Ok(self.align_system().as_str().to_string())
    }
    fn test_system_attr(&self) -> bool {
        self.test_system()
    }
    fn test_align_system_attr(&self) -> bool {
        self.test_align_system()
    }
    fn clear_system_attr(&mut self) -> AstResult<()> {
        self.clear_system()
    }
    fn clear_align_system_attr(&mut self) -> AstResult<()> {
        self.clear_align_system();
        Ok(())
    }
}

/// C's `TimeScaleCode`, with the error `SetAttrib` gives for a name it does
/// not know.
fn time_scale(text: &str) -> AstResult<TimeScale> {
    TimeScale::parse(text).ok_or_else(|| {
        AstError::new(
            ErrorCode::Attin,
            format!("astSetAttrib(TimeFrame): Invalid time scale description \"{text}\"."),
        )
    })
}

/// A date, as an MJD, in the TimeFrame's System, which is how TimeOrigin
/// holds it.
// C: timeframe.c FromMJD, through a TimeMap.
fn from_mjd(frame: &TimeFrame, mjd: f64) -> AstResult<f64> {
    let cvt = match frame.system() {
        TimeSystem::Mjd => return Ok(mjd),
        TimeSystem::Jd => TimeCvtType::MjdToJd,
        TimeSystem::Jepoch => TimeCvtType::MjdToJep,
        TimeSystem::Bepoch => TimeCvtType::MjdToBep,
    };
    let mut map = TimeMap::new();
    map.add(cvt, &[0.0, 0.0])?;
    tran1(&map, mjd)
}

/// A TimeOrigin value with a unit, in the default units of the System, or
/// `None` if the unit does not convert to them.
// C: timeframe.c ToUnits, whose error SetAttrib discards for this form,
// the unit parser's included.
fn origin_in_default_units(frame: &TimeFrame, unit: &str, value: f64) -> AstResult<Option<f64>> {
    let Some(map) = unit_mapper(unit, frame.system().default_units())
        .ok()
        .flatten()
    else {
        return Ok(None);
    };
    let origin = tran1(map.as_ref(), value)?;
    Ok((origin != BAD_VALUE).then_some(origin))
}

/// A TimeOrigin given as a date.
fn origin_from_date(frame: &mut TimeFrame, text: &str) -> AstResult<()> {
    match read_date_time_checked(text) {
        Ok(mjd) => {
            let origin = from_mjd(frame, mjd)?;
            frame.set_time_origin(origin);
            Ok(())
        }
        // C reports the reader's message and then its own.
        Err(first) => Err(AstError::new(ErrorCode::Attin, first).with_context(format!(
            "astSetAttrib(TimeFrame): Invalid TimeOrigin value \"{text}\" given."
        ))),
    }
}

/// TimeFrame's `SetAttrib`.
pub(crate) fn timeframe_set(frame: &mut TimeFrame, setting: &str) -> AstResult<()> {
    if let Some(name) = AXIS_SHORTCUTS.iter().find(|name| {
        setting
            .strip_prefix(**name)
            .is_some_and(|r| r.starts_with('='))
    }) {
        // As SpecFrame's: the setting goes to the Frame class with "(1)"
        // after the name, and a second visit is stopped.
        let Some(_guard) = ShortcutGuard::enter() else {
            return Err(unknown_set(setting, "TimeFrame"));
        };
        frame_set(frame, &format!("{name}(1){}", &setting[name.len()..]))
    } else if let Some(v) = word_value_whole(setting, "aligntimescale") {
        let ts = time_scale(v)?;
        frame.set_align_timescale(ts);
        Ok(())
    } else if word_value(setting, "clocklat", 7).is_some()
        || word_value(setting, "clocklon", 7).is_some()
    {
        // ClockLat and ClockLon are ObsLat and ObsLon.
        frame.attr_set(&format!("obs{}", &setting[5..]))
    } else if let Some(v) = double_value(setting, "ltoffset") {
        frame.set_lt_offset(v);
        Ok(())
    } else if let Some(v) = double_value(setting, "timeorigin") {
        // A number alone is in the Frame's current Unit.
        frame.set_time_origin_cur(v)
    } else if let Some(scan) = scan_whole(setting, "timeorigin= %lg %n%*s %n", 1) {
        // A number and a unit, or failing that a date such as "2010 Jan 5".
        match origin_in_default_units(frame, &setting[scan.count(1)..], scan.double(0))? {
            Some(origin) => {
                frame.set_time_origin(origin);
                Ok(())
            }
            None => origin_from_date(frame, &setting["timeorigin=".len()..]),
        }
    } else if let Some(v) = string_value(setting, "timeorigin") {
        origin_from_date(frame, v)
    } else if let Some(v) = word_value_whole(setting, "timescale") {
        let ts = time_scale(v)?;
        frame.set_timescale(ts)
    } else {
        frame_set(frame, setting)
    }
}

/// TimeFrame's `GetAttrib`.
pub(crate) fn timeframe_get(frame: &TimeFrame, name: &str) -> AstResult<String> {
    if AXIS_SHORTCUTS.contains(&name) {
        return frame_get(frame, &format!("{name}(1)"));
    }
    match name {
        "aligntimescale" => Ok(frame.align_timescale().as_str().to_string()),
        "clocklat" => frame.attr_get("obslat"),
        "clocklon" => frame.attr_get("obslon"),
        "timeorigin" => Ok(fmt_double(frame.time_origin_cur()?)),
        "ltoffset" => Ok(fmt_double(frame.lt_offset())),
        "timescale" => Ok(frame.timescale().as_str().to_string()),
        _ => frame_get(frame, name),
    }
}

/// TimeFrame's `TestAttrib`.
pub(crate) fn timeframe_test(frame: &TimeFrame, name: &str) -> AstResult<bool> {
    if AXIS_SHORTCUTS.contains(&name) {
        return frame_test(frame, &format!("{name}(1)"));
    }
    match name {
        "aligntimescale" => Ok(frame.test_align_timescale()),
        "clocklat" => frame.attr_test("obslat"),
        "clocklon" => frame.attr_test("obslon"),
        "ltoffset" => Ok(frame.test_lt_offset()),
        "timeorigin" => Ok(frame.test_time_origin()),
        "timescale" => Ok(frame.test_timescale()),
        _ => frame_test(frame, name),
    }
}

/// TimeFrame's `ClearAttrib`.
pub(crate) fn timeframe_clear(frame: &mut TimeFrame, name: &str) -> AstResult<()> {
    if AXIS_SHORTCUTS.contains(&name) {
        return frame_clear(frame, &format!("{name}(1)"));
    }
    match name {
        "aligntimescale" => frame.clear_align_timescale(),
        "clocklat" => return frame.attr_clear("obslat"),
        "clocklon" => return frame.attr_clear("obslon"),
        "ltoffset" => frame.clear_lt_offset(),
        "timeorigin" => frame.clear_time_origin(),
        "timescale" => return frame.clear_timescale(),
        _ => return frame_clear(frame, name),
    }
    Ok(())
}
