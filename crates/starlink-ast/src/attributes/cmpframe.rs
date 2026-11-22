// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a CmpFrame.
//!
//! A CmpFrame owns no attributes of its own. A name goes first to the
//! Frame class, as for any Frame; failing that, to the Frames its axes
//! belong to, so that a component's attributes can be reached without
//! taking it out of the CmpFrame: a name with an axis index, `Label(2)`, to
//! the Frame holding that axis, with the index that Frame gives it; a name
//! without one to each of them. Every failure becomes the CmpFrame's own
//! error, whatever the component reported.
//!
//! A get or test of a name with an axis index goes straight to the
//! component, so `Label(2)` by string can differ from the typed
//! [`label`](crate::frames::Frame::label), which the CmpFrame answers itself.
// C: cmpframe.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, whose
// parent is Frame. C runs the attempts with error reporting off and clears
// each failure; here each attempt's error is dropped.

use super::frame::{FrameClass, frame_clear, frame_get, frame_set, frame_test, primary_axis_index};
use super::{axis_invalid, scan_whole};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::chr_match;
use crate::frames::{CmpFrame, Frame};

impl FrameClass for CmpFrame {
    // C: cmpframe.c SystemCode, which knows only "Compound".
    fn set_system_text(&mut self, text: &str) -> AstResult<bool> {
        Ok(chr_match("Compound", text) && {
            self.set_system_flag(true);
            true
        })
    }
    fn set_align_system_text(&mut self, text: &str) -> AstResult<bool> {
        Ok(chr_match("Compound", text) && {
            self.set_align_system_flag(true);
            true
        })
    }
    fn system_text(&self) -> AstResult<String> {
        Ok("Compound".to_string())
    }
    fn align_system_text(&self) -> AstResult<String> {
        Ok("Compound".to_string())
    }
    fn test_system_attr(&self) -> bool {
        self.system_flag()
    }
    fn test_align_system_attr(&self) -> bool {
        self.align_system_flag()
    }
    fn clear_system_attr(&mut self) -> AstResult<()> {
        self.set_system_flag(false);
        Ok(())
    }
    fn clear_align_system_attr(&mut self) -> AstResult<()> {
        self.set_align_system_flag(false);
        Ok(())
    }
}

/// The 0-based axis of a name with an axis index, checked as C's
/// `astPrimaryFrame` checks it.
fn primary_axis(frame: &CmpFrame, axis: i32) -> AstResult<usize> {
    let naxes = frame.naxes();
    match usize::try_from(axis) {
        Ok(a) if a >= 1 && a <= naxes => Ok(a - 1),
        _ => Err(axis_invalid("astPrimaryFrame", "CmpFrame", axis, naxes)),
    }
}

/// The error for a name nothing in the CmpFrame recognised, for a get, test
/// or clear.
fn no_attribute(method: &str, name: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("{method}: The CmpFrame given does not have an attribute called \"{name}\"."),
    )
}

/// The name and axis of `"%[^(](%d)%n"`, a name with an axis index as a get,
/// test or clear is given it.
fn indexed_name(name: &str) -> Option<(&str, i32)> {
    let scan = scan_whole(name, "%[^(](%d)%n", 2)?;
    Some((&name[..scan.text(0).len()], scan.int(1)))
}

/// CmpFrame's `SetAttrib`.
pub(crate) fn cmpframe_set(frame: &mut CmpFrame, setting: &str) -> AstResult<()> {
    if frame_set(frame, setting).is_ok() {
        return Ok(());
    }
    let mut ok = false;
    if let Some(scan) = scan_whole(setting, "%[^(=](%d)= %n%*s %n", 2) {
        let name = scan.text(0).to_string();
        let value = &setting[scan.count(2)..];
        let axis = primary_axis(frame, scan.int(1))?;
        let (primary, paxis) = frame.primary_frame_mut(axis);
        let index = primary_axis_index(primary, paxis);
        ok = primary
            .attr_set(&format!("{name}({index})={value}"))
            .is_ok()
            || primary.attr_set(&format!("{name}={value}")).is_ok();
    } else {
        // Every axis's Frame is given the setting, so a component of two
        // axes is given it twice.
        for axis in 0..frame.naxes() {
            let (primary, _) = frame.primary_frame_mut(axis);
            ok |= primary.attr_set(setting).is_ok();
        }
    }
    if ok {
        Ok(())
    } else {
        Err(AstError::new(
            ErrorCode::Badat,
            format!(
                "astSet: The attribute setting \"{setting}\" is invalid for the given CmpFrame."
            ),
        ))
    }
}

/// CmpFrame's `GetAttrib`.
pub(crate) fn cmpframe_get(frame: &CmpFrame, name: &str) -> AstResult<String> {
    if let Some((attr, axis)) = indexed_name(name) {
        let (primary, paxis) = frame.primary_frame(primary_axis(frame, axis)?);
        let index = primary_axis_index(primary, paxis);
        return primary
            .attr_get(&format!("{attr}({index})"))
            .or_else(|_| primary.attr_get(attr))
            .map_err(|_| no_attribute("astGet", name));
    }
    if let Ok(value) = frame_get(frame, name) {
        return Ok(value);
    }
    (0..frame.naxes())
        .find_map(|axis| frame.primary_frame(axis).0.attr_get(name).ok())
        .ok_or_else(|| no_attribute("astGet", name))
}

/// CmpFrame's `TestAttrib`.
pub(crate) fn cmpframe_test(frame: &CmpFrame, name: &str) -> AstResult<bool> {
    if let Ok(value) = frame_test(frame, name) {
        return Ok(value);
    }
    if let Some((attr, axis)) = indexed_name(name) {
        let (primary, paxis) = frame.primary_frame(primary_axis(frame, axis)?);
        let index = primary_axis_index(primary, paxis);
        return primary
            .attr_test(&format!("{attr}({index})"))
            .or_else(|_| primary.attr_test(attr))
            .map_err(|_| no_attribute("astTest", name));
    }
    (0..frame.naxes())
        .find_map(|axis| frame.primary_frame(axis).0.attr_test(name).ok())
        .ok_or_else(|| no_attribute("astTest", name))
}

/// CmpFrame's `ClearAttrib`.
pub(crate) fn cmpframe_clear(frame: &mut CmpFrame, name: &str) -> AstResult<()> {
    if frame_clear(frame, name).is_ok() {
        return Ok(());
    }
    let mut ok = false;
    if let Some((attr, axis)) = indexed_name(name) {
        let axis = primary_axis(frame, axis)?;
        let (primary, paxis) = frame.primary_frame_mut(axis);
        let index = primary_axis_index(primary, paxis);
        ok = primary.attr_clear(&format!("{attr}({index})")).is_ok()
            || primary.attr_clear(attr).is_ok();
    } else {
        for axis in 0..frame.naxes() {
            let (primary, _) = frame.primary_frame_mut(axis);
            ok |= primary.attr_clear(name).is_ok();
        }
    }
    if ok {
        Ok(())
    } else {
        Err(no_attribute("astClear", name))
    }
}
