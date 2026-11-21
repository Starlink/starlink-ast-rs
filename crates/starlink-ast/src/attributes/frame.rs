// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of the Frame class, which every Frame class inherits:
//! Title, Domain, Epoch, System and the rest, the per-axis Label, Unit,
//! Format and so on, and the way a name with an axis index that Frame does
//! not own is handed to the axis's Axis object and then to the Frame the
//! axis belongs to.
// C: frame.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, whose parent
// is Mapping.
//
// Each of the four tries the Frame's own names; then, for a name of the form
// "name(axis)", the generic axis path; then, for a Frame of one axis and a
// name without an index, the Frame's own names again with "(1)" added; and
// last its parent class, with the name as given.
//
// C's typed functions validate an axis index themselves and name themselves
// in the error ("astSetLabel(Frame): Axis index (3) invalid ..."). The port's
// typed setters word that error differently, so each arm here validates the
// index first, with the name C's function gives.

use super::axis::{axis_clear, axis_get, axis_set, axis_test};
use super::mapping::{mapping_clear, mapping_get, mapping_set, mapping_test};
use super::{
    axis_double_value, axis_int_value, axis_invalid, axis_name, axis_string_value, double_value,
    fmt_decimal_yr, fmt_double, fmt_int, int_value, is_read_only, keyword_value, read_only_clear,
    read_only_set, scan_whole, string_value, word_value,
};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::date_time::read_date_time_checked;
use crate::frames::{Frame, SkyFrame, SkySystem};
use crate::pal;
use crate::serial::native_write::DBL_DIG;

/// What a Frame class supplies for its System and AlignSystem attributes,
/// whose values are the class's own.
// C: the virtual astSystemCode and astSystemString, and astSetSystem,
// astGetSystem and the rest, which each class over-rides.
pub(crate) trait FrameClass: Frame {
    /// Set System from its name; `Ok(false)` if the class has no system of
    /// that name.
    fn set_system_text(&mut self, text: &str) -> AstResult<bool>;
    /// Set AlignSystem from its name; `Ok(false)` if the class has no
    /// system of that name.
    fn set_align_system_text(&mut self, text: &str) -> AstResult<bool>;
    /// The name of the System.
    fn system_text(&self) -> AstResult<String>;
    /// The name of the AlignSystem.
    fn align_system_text(&self) -> AstResult<String>;
    /// Whether System is set.
    fn test_system_attr(&self) -> bool;
    /// Whether AlignSystem is set.
    fn test_align_system_attr(&self) -> bool;
    /// Clear System.
    fn clear_system_attr(&mut self) -> AstResult<()>;
    /// Clear AlignSystem.
    fn clear_align_system_attr(&mut self) -> AstResult<()>;
}

/// The 1-based axis an attribute string gives, checked against the Frame's
/// axes as C's `astValidateAxis` checks it for the function `method`.
pub(crate) fn validate_axis<F: Frame + ?Sized>(
    frame: &F,
    axis: i32,
    method: &str,
) -> AstResult<usize> {
    let naxes = frame.naxes();
    match usize::try_from(axis) {
        Ok(a) if a >= 1 && a <= naxes => Ok(a),
        _ => Err(axis_invalid(method, frame.attr_class(), axis, naxes)),
    }
}

/// The axis index, as the primary Frame's attribute strings number its
/// axes (1-based, external), of the axis `primary_frame` gives as `paxis`.
// C: astValidateAxis( pfrm, paxis, 0, ... ) + 1, which turns the internal
// index astPrimaryFrame gives into an external one. Of the Rust classes only
// SkyFrame's `primary_frame` gives an internal index; the others give the
// external one.
pub(crate) fn primary_axis_index(primary: &dyn Frame, paxis: usize) -> usize {
    match primary.as_any().downcast_ref::<SkyFrame>() {
        Some(sky) => {
            sky.axis_permutation()
                .iter()
                .position(|&p| p == paxis)
                .unwrap_or(paxis)
                + 1
        }
        None => paxis + 1,
    }
}

/// The FK5 SkyFrame C formats and reads an observer's latitude and
/// longitude with, as degrees, arcminutes and arcseconds to two places.
// C: the static "skyframe" of frame.c,
// astSkyFrame( "system=FK5,equinox=J2000,format(2)=dms.2" ).
fn dms_frame() -> AstResult<SkyFrame> {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_format_str(2, "dms.2")?;
    Ok(sky)
}

/// C's reading of an ObsLat or ObsLon value: an optional letter giving the
/// sign (`pos` or `neg`, in either case), then an angle the whole of the
/// rest must be. `None` if it cannot be read.
fn read_geodetic(value: &str, pos: u8, neg: u8) -> AstResult<Option<f64>> {
    let first = value.bytes().next().map(|b| b.to_ascii_uppercase());
    let (sign, text) = if first == Some(pos) {
        (1.0, &value[1..])
    } else if first == Some(neg) {
        (-1.0, &value[1..])
    } else {
        (1.0, value)
    };
    // C compares the number of characters astUnformat read with the length
    // of the text without its trailing white space.
    let wanted = text.trim_end_matches(|c: char| c.is_ascii() && super::is_space(c as u8));
    // With nothing after the sign letter C reads no characters, finds that
    // the count matches, and stores a value it never assigned
    // (c-library-quirks.md); the port stores zero.
    if wanted.is_empty() {
        return Ok(Some(0.0));
    }
    let (angle, nc) = dms_frame()?.unformat(2, text)?;
    Ok((nc == wanted.len()).then_some(angle * sign))
}

/// An ObsLat or ObsLon value as text: the angle without its sign, after the
/// letter that gives it.
fn write_geodetic(angle: f64, pos: char, neg: char) -> AstResult<String> {
    let sky = dms_frame()?;
    Ok(if angle < 0.0 {
        format!("{neg}{}", sky.format(2, -angle)?)
    } else {
        format!("{pos}{}", sky.format(2, angle)?)
    })
}

/// The name of the read-only attributes Frame recognises by how the text
/// starts: NormUnit and InternalUnit, with or without an axis.
fn is_unit_read_only(text: &str) -> bool {
    text.starts_with("normunit") || text.starts_with("internalunit")
}

/// The Frame's own names for a setting: `None` if it is none of them.
fn frame_set_own<F: FrameClass + ?Sized>(frame: &mut F, setting: &str) -> Option<AstResult<()>> {
    let class = frame.attr_class();
    Some(if let Some(v) = int_value(setting, "digits") {
        frame.set_digits(v)
    } else if let Some((axis, v)) = axis_int_value(setting, "digits") {
        validate_axis(frame, axis, "astSetDigits(axis)")
            .and_then(|axis| frame.set_axis_digits(axis, v))
    } else if let Some((axis, v)) = axis_int_value(setting, "direction") {
        validate_axis(frame, axis, "astSetDirection")
            .and_then(|axis| frame.set_direction(axis, v != 0))
    } else if let Some(v) = string_value(setting, "epoch") {
        match read_date_time_checked(v) {
            Ok(mjd) => frame.set_epoch_mjd(mjd),
            // C reports the reader's message and then its own.
            Err(first) => Err(AstError::new(ErrorCode::Attin, first).with_context(format!(
                "astSetAttrib({class}): Invalid epoch value \"{v}\" given for coordinate system."
            ))),
        }
    } else if let Some((axis, v)) = axis_double_value(setting, "top") {
        validate_axis(frame, axis, "astSetTop").and_then(|axis| frame.set_top(axis, v))
    } else if let Some((axis, v)) = axis_double_value(setting, "bottom") {
        validate_axis(frame, axis, "astSetBottom").and_then(|axis| frame.set_bottom(axis, v))
    } else if let Some(v) = string_value(setting, "domain") {
        frame.set_domain(v)
    } else if let Some((axis, v)) = axis_string_value(setting, "format") {
        validate_axis(frame, axis, "astSetFormat").and_then(|axis| frame.set_format_str(axis, v))
    } else if let Some((axis, v)) = axis_string_value(setting, "label") {
        validate_axis(frame, axis, "astSetLabel").and_then(|axis| frame.set_label(axis, v))
    } else if let Some(v) = int_value(setting, "matchend") {
        frame.set_match_end(v != 0)
    } else if let Some(v) = int_value(setting, "maxaxes") {
        frame.set_max_axes(usize::try_from(v).unwrap_or(0))
    } else if let Some(v) = int_value(setting, "minaxes") {
        frame.set_min_axes(usize::try_from(v).unwrap_or(0))
    } else if let Some(v) = int_value(setting, "permute") {
        frame.set_permute(v != 0)
    } else if let Some(v) = int_value(setting, "preserveaxes") {
        frame.set_preserve_axes(v != 0)
    } else if let Some((axis, v)) = axis_string_value(setting, "symbol") {
        validate_axis(frame, axis, "astSetSymbol").and_then(|axis| frame.set_symbol(axis, v))
    } else if let Some(v) = keyword_value(setting, "alignsystem") {
        frame.set_align_system_text(v).and_then(|known| {
            if known {
                Ok(())
            } else {
                Err(AstError::new(
                    ErrorCode::Attin,
                    format!("astSetAttrib({class}): Invalid AlignSystem description \"{v}\"."),
                ))
            }
        })
    } else if let Some(v) = keyword_value(setting, "system") {
        frame.set_system_text(v).and_then(|known| {
            if known {
                Ok(())
            } else {
                Err(AstError::new(
                    ErrorCode::Attin,
                    format!("astSetAttrib({class}): Invalid System description \"{v}\"."),
                ))
            }
        })
    } else if let Some(v) = string_value(setting, "title") {
        frame.set_title(v)
    } else if let Some((axis, v)) = axis_string_value(setting, "unit") {
        validate_axis(frame, axis, "astSetUnit").and_then(|axis| frame.set_unit(axis, v))
    } else if let Some(v) = word_value(setting, "obslat", 7) {
        match read_geodetic(v, b'N', b'S') {
            Ok(Some(angle)) => frame.set_obs_lat(angle),
            Ok(None) => Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetAttrib({class}): Invalid value for ObsLat (observers latitude) \
                     \"{v}\"."
                ),
            )),
            Err(e) => Err(e),
        }
    } else if let Some(v) = word_value(setting, "obslon", 7) {
        match read_geodetic(v, b'E', b'W') {
            Ok(Some(angle)) => frame.set_obs_lon(angle),
            Ok(None) => Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetAttrib({class}): Invalid value for ObsLon (observers longitude) \
                     \"{v}\"."
                ),
            )),
            Err(e) => Err(e),
        }
    } else if let Some(v) = double_value(setting, "obsalt") {
        frame.set_obs_alt(v)
    } else if let Some(v) = double_value(setting, "dtai") {
        frame.set_dtai(v)
    } else if let Some(v) = double_value(setting, "dut1") {
        frame.set_dut1(v)
    } else if is_read_only(setting, "naxes") || is_unit_read_only(setting) {
        Err(read_only_set(setting, class))
    } else {
        return None;
    })
}

/// Frame's `SetAttrib`.
pub(crate) fn frame_set<F: FrameClass + ?Sized>(frame: &mut F, setting: &str) -> AstResult<()> {
    let equals = setting.find('=');
    let has_axis = matches!((setting.find('('), equals), (Some(op), Some(eq)) if op < eq);
    if let Some(result) = frame_set_own(frame, setting) {
        return result;
    }
    if let Some(result) = axis_path_set(frame, setting) {
        return result;
    }
    // A Frame of one axis takes an axis attribute without its index.
    if let (false, 1, Some(eq)) = (has_axis, frame.naxes(), equals) {
        let indexed = format!("{}(1){}", &setting[..eq], &setting[eq..]);
        if let Some(result) = frame_set_own(frame, &indexed) {
            return result;
        }
    }
    mapping_set(frame, setting)
}

/// The generic axis path of `SetAttrib`: a setting `name(axis)=value` that
/// is none of the Frame's own goes to the Axis object as `name=value`; if
/// the Axis does not know the name, to the Frame the axis belongs to, with
/// that Frame's axis index and then without an index; and if nothing takes
/// it, the Axis object's error is the one reported.
/// `None` if the setting is not of that form.
fn axis_path_set<F: FrameClass + ?Sized>(frame: &mut F, setting: &str) -> Option<AstResult<()>> {
    let scan = scan_whole(setting, "%*[^()=]%n(%d)%n=%*[^\n]%n", 1)?;
    let (name, axis, value) = (
        &setting[..scan.count(0)],
        scan.int(1),
        &setting[scan.count(2)..],
    );
    Some((|| {
        let axis = validate_axis(frame, axis, "astSet")?;
        let axis_setting = format!("{name}{value}");
        match axis_set(frame.attr_axis_mut(axis)?.0, &axis_setting) {
            Err(e) if e.code() == ErrorCode::Badat => {}
            other => return other,
        }
        let this = std::ptr::from_mut(frame).cast::<()>();
        let (primary, paxis) = frame.primary_frame_mut(axis - 1);
        let mut used = false;
        if !std::ptr::eq(std::ptr::from_mut(primary).cast::<()>(), this) {
            let index = primary_axis_index(primary, paxis);
            used = primary.attr_set(&format!("{name}({index}){value}")).is_ok();
        }
        if !used {
            used = primary.attr_set(&axis_setting).is_ok();
        }
        if used {
            Ok(())
        } else {
            axis_set(frame.attr_axis_mut(axis)?.0, &axis_setting)
        }
    })())
}

/// The Frame's own names for a get: `None` if it is none of them.
fn frame_get_own<F: FrameClass + ?Sized>(frame: &F, name: &str) -> Option<AstResult<String>> {
    let flag = |v: bool| Ok(fmt_int(i32::from(v)));
    let count = |v: usize| Ok(fmt_int(i32::try_from(v).unwrap_or(i32::MAX)));
    Some(if name == "digits" {
        Ok(fmt_int(frame.digits()))
    } else if let Some(axis) = axis_name(name, "digits") {
        // The Axis's own Digits if it is set, else the Frame's.
        validate_axis(frame, axis, "astGetDigits(axis)").and_then(|axis| {
            Ok(fmt_int(if frame.test_axis_digits(axis)? {
                frame.axis_digits(axis)?
            } else {
                frame.digits()
            }))
        })
    } else if let Some(axis) = axis_name(name, "direction") {
        validate_axis(frame, axis, "astGetDirection").and_then(|axis| flag(frame.direction(axis)?))
    } else if name == "epoch" {
        // A Besselian epoch before 1984.0 and a Julian one from then on.
        let epoch = frame.epoch_mjd();
        let year = if epoch < pal::epj2d(1984.0) {
            pal::epb(epoch)
        } else {
            pal::epj(epoch)
        };
        Ok(fmt_decimal_yr(year, DBL_DIG))
    } else if let Some(axis) = axis_name(name, "top") {
        validate_axis(frame, axis, "astGetTop").and_then(|axis| Ok(fmt_double(frame.top(axis)?)))
    } else if let Some(axis) = axis_name(name, "bottom") {
        validate_axis(frame, axis, "astGetBottom")
            .and_then(|axis| Ok(fmt_double(frame.bottom(axis)?)))
    } else if name == "domain" {
        Ok(frame.domain().to_string())
    } else if let Some(axis) = axis_name(name, "format") {
        validate_axis(frame, axis, "astGetFormat").and_then(|axis| frame.format_str(axis))
    } else if let Some(axis) = axis_name(name, "label") {
        validate_axis(frame, axis, "astGetLabel").and_then(|axis| frame.label(axis))
    } else if name == "matchend" {
        flag(frame.match_end())
    } else if name == "maxaxes" {
        count(frame.max_axes())
    } else if name == "minaxes" {
        count(frame.min_axes())
    } else if name == "naxes" {
        count(frame.naxes())
    } else if name == "permute" {
        flag(frame.permute())
    } else if name == "preserveaxes" {
        flag(frame.preserve_axes())
    } else if let Some(axis) = axis_name(name, "symbol") {
        validate_axis(frame, axis, "astGetSymbol").and_then(|axis| frame.symbol(axis))
    } else if name == "alignsystem" {
        frame.align_system_text()
    } else if name == "system" {
        frame.system_text()
    } else if name == "title" {
        frame.title()
    } else if let Some(axis) = axis_name(name, "unit") {
        validate_axis(frame, axis, "astGetUnit").and_then(|axis| frame.unit(axis))
    } else if let Some(axis) = axis_name(name, "normunit") {
        validate_axis(frame, axis, "astGetNormUnit").and_then(|axis| frame.norm_unit(axis))
    } else if let Some(axis) = axis_name(name, "internalunit") {
        validate_axis(frame, axis, "astGetInternalUnit").and_then(|axis| frame.internal_unit(axis))
    } else if name == "obslat" {
        write_geodetic(frame.obs_lat(), 'N', 'S')
    } else if name == "obslon" {
        write_geodetic(pal::drange(frame.obs_lon()), 'E', 'W')
    } else if name == "obsalt" {
        Ok(fmt_double(frame.obs_alt()))
    } else if name == "dtai" {
        Ok(fmt_double(frame.dtai()))
    } else if name == "dut1" {
        Ok(fmt_double(frame.dut1()))
    } else {
        return None;
    })
}

/// Frame's `GetAttrib`.
pub(crate) fn frame_get<F: FrameClass + ?Sized>(frame: &F, name: &str) -> AstResult<String> {
    if let Some(result) = frame_get_own(frame, name) {
        return result;
    }
    if let Some(result) = axis_path_get(frame, name) {
        return result;
    }
    if !name.contains('(') && frame.naxes() == 1 {
        if let Some(result) = frame_get_own(frame, &format!("{name}(1)")) {
            return result;
        }
    }
    mapping_get(frame, name)
}

/// The name and axis of an attribute of the form `name(axis)`, for the
/// generic axis path of a get, test or clear: C's `"%*[^()]%n(%d)%n"`.
fn axis_form(name: &str) -> Option<(&str, i32)> {
    let scan = scan_whole(name, "%*[^()]%n(%d)%n", 1)?;
    Some((&name[..scan.count(0)], scan.int(1)))
}

/// The generic axis path of `GetAttrib`; see [`axis_path_set`].
fn axis_path_get<F: FrameClass + ?Sized>(frame: &F, name: &str) -> Option<AstResult<String>> {
    let (attr, axis) = axis_form(name)?;
    Some((|| {
        let axis = validate_axis(frame, axis, "astGet")?;
        match axis_get(frame.attr_axis(axis)?.0, attr) {
            Err(e) if e.code() == ErrorCode::Badat => {}
            other => return other,
        }
        let (primary, paxis) = frame.primary_frame(axis - 1);
        let this = std::ptr::from_ref(frame).cast::<()>();
        if !std::ptr::eq(std::ptr::from_ref(primary).cast::<()>(), this) {
            let index = primary_axis_index(primary, paxis);
            if let Ok(value) = primary.attr_get(&format!("{attr}({index})")) {
                return Ok(value);
            }
        }
        match primary.attr_get(attr) {
            Ok(value) => Ok(value),
            Err(_) => axis_get(frame.attr_axis(axis)?.0, attr),
        }
    })())
}

/// The Frame's own names for a test: `None` if it is none of them.
fn frame_test_own<F: FrameClass + ?Sized>(frame: &F, name: &str) -> Option<AstResult<bool>> {
    Some(if name == "digits" {
        Ok(frame.test_digits())
    } else if let Some(axis) = axis_name(name, "digits") {
        validate_axis(frame, axis, "astTestDigits(axis)")
            .and_then(|axis| frame.test_axis_digits(axis))
    } else if let Some(axis) = axis_name(name, "direction") {
        validate_axis(frame, axis, "astTestDirection").and_then(|axis| frame.test_direction(axis))
    } else if name == "epoch" {
        Ok(frame.test_epoch())
    } else if let Some(axis) = axis_name(name, "bottom") {
        validate_axis(frame, axis, "astTestBottom").and_then(|axis| frame.test_bottom(axis))
    } else if let Some(axis) = axis_name(name, "top") {
        validate_axis(frame, axis, "astTestTop").and_then(|axis| frame.test_top(axis))
    } else if name == "domain" {
        Ok(frame.test_domain())
    } else if let Some(axis) = axis_name(name, "format") {
        validate_axis(frame, axis, "astTestFormat").and_then(|axis| frame.test_format_str(axis))
    } else if let Some(axis) = axis_name(name, "label") {
        validate_axis(frame, axis, "astTestLabel").and_then(|axis| frame.test_label(axis))
    } else if name == "matchend" {
        Ok(frame.test_match_end())
    } else if name == "maxaxes" {
        Ok(frame.test_max_axes())
    } else if name == "minaxes" {
        Ok(frame.test_min_axes())
    } else if name == "permute" {
        Ok(frame.test_permute())
    } else if name == "preserveaxes" {
        Ok(frame.test_preserve_axes())
    } else if let Some(axis) = axis_name(name, "symbol") {
        validate_axis(frame, axis, "astTestSymbol").and_then(|axis| frame.test_symbol(axis))
    } else if name == "alignsystem" {
        Ok(frame.test_align_system_attr())
    } else if name == "system" {
        Ok(frame.test_system_attr())
    } else if name == "title" {
        Ok(frame.test_title())
    } else if let Some(axis) = axis_name(name, "unit") {
        validate_axis(frame, axis, "astTestUnit").and_then(|axis| frame.test_unit(axis))
    } else if name == "obslat" {
        Ok(frame.test_obs_lat())
    } else if name == "obslon" {
        Ok(frame.test_obs_lon())
    } else if name == "obsalt" {
        Ok(frame.test_obs_alt())
    } else if name == "dtai" {
        Ok(frame.test_dtai())
    } else if name == "dut1" {
        Ok(frame.test_dut1())
    } else if name == "naxes" || is_unit_read_only(name) {
        Ok(false)
    } else {
        return None;
    })
}

/// Frame's `TestAttrib`.
pub(crate) fn frame_test<F: FrameClass + ?Sized>(frame: &F, name: &str) -> AstResult<bool> {
    if let Some(result) = frame_test_own(frame, name) {
        return result;
    }
    if let Some(result) = axis_path_test(frame, name) {
        return result;
    }
    if !name.contains('(') && frame.naxes() == 1 {
        if let Some(result) = frame_test_own(frame, &format!("{name}(1)")) {
            return result;
        }
    }
    mapping_test(frame, name)
}

/// The generic axis path of `TestAttrib`; see [`axis_path_set`].
fn axis_path_test<F: FrameClass + ?Sized>(frame: &F, name: &str) -> Option<AstResult<bool>> {
    let (attr, axis) = axis_form(name)?;
    Some((|| {
        let axis = validate_axis(frame, axis, "astTest")?;
        match axis_test(frame.attr_axis(axis)?.0, attr) {
            Err(e) if e.code() == ErrorCode::Badat => {}
            other => return other,
        }
        let (primary, paxis) = frame.primary_frame(axis - 1);
        let this = std::ptr::from_ref(frame).cast::<()>();
        if !std::ptr::eq(std::ptr::from_ref(primary).cast::<()>(), this) {
            let index = primary_axis_index(primary, paxis);
            if let Ok(value) = primary.attr_test(&format!("{attr}({index})")) {
                return Ok(value);
            }
        }
        match primary.attr_test(attr) {
            Ok(value) => Ok(value),
            Err(_) => axis_test(frame.attr_axis(axis)?.0, attr),
        }
    })())
}

/// The Frame's own names for a clear: `None` if it is none of them.
fn frame_clear_own<F: FrameClass + ?Sized>(frame: &mut F, name: &str) -> Option<AstResult<()>> {
    Some(if name == "digits" {
        frame.clear_digits()
    } else if let Some(axis) = axis_name(name, "digits") {
        validate_axis(frame, axis, "astClearDigits(axis)")
            .and_then(|axis| frame.clear_axis_digits(axis))
    } else if let Some(axis) = axis_name(name, "direction") {
        validate_axis(frame, axis, "astClearDirection").and_then(|axis| frame.clear_direction(axis))
    } else if name == "epoch" {
        frame.clear_epoch()
    } else if let Some(axis) = axis_name(name, "top") {
        validate_axis(frame, axis, "astClearTop").and_then(|axis| frame.clear_top(axis))
    } else if let Some(axis) = axis_name(name, "bottom") {
        validate_axis(frame, axis, "astClearBottom").and_then(|axis| frame.clear_bottom(axis))
    } else if name == "domain" {
        frame.clear_domain()
    } else if let Some(axis) = axis_name(name, "format") {
        validate_axis(frame, axis, "astClearFormat").and_then(|axis| frame.clear_format_str(axis))
    } else if let Some(axis) = axis_name(name, "label") {
        validate_axis(frame, axis, "astClearLabel").and_then(|axis| frame.clear_label(axis))
    } else if name == "matchend" {
        frame.clear_match_end()
    } else if name == "maxaxes" {
        frame.clear_max_axes()
    } else if name == "minaxes" {
        frame.clear_min_axes()
    } else if name == "permute" {
        frame.clear_permute()
    } else if name == "preserveaxes" {
        frame.clear_preserve_axes()
    } else if let Some(axis) = axis_name(name, "symbol") {
        validate_axis(frame, axis, "astClearSymbol").and_then(|axis| frame.clear_symbol(axis))
    } else if name == "system" {
        frame.clear_system_attr()
    } else if name == "alignsystem" {
        frame.clear_align_system_attr()
    } else if name == "title" {
        frame.clear_title()
    } else if let Some(axis) = axis_name(name, "unit") {
        // C's astClearUnit names astSetUnit in its error
        // (c-library-quirks.md).
        validate_axis(frame, axis, "astSetUnit").and_then(|axis| frame.clear_unit(axis))
    } else if name == "obslat" {
        frame.clear_obs_lat()
    } else if name == "obslon" {
        frame.clear_obs_lon()
    } else if name == "obsalt" {
        frame.clear_obs_alt()
    } else if name == "dtai" {
        frame.clear_dtai()
    } else if name == "dut1" {
        frame.clear_dut1()
    } else if name == "naxes" || is_unit_read_only(name) {
        Err(read_only_clear(name, frame.attr_class()))
    } else {
        return None;
    })
}

/// Frame's `ClearAttrib`.
pub(crate) fn frame_clear<F: FrameClass + ?Sized>(frame: &mut F, name: &str) -> AstResult<()> {
    if let Some(result) = frame_clear_own(frame, name) {
        return result;
    }
    if let Some(result) = axis_path_clear(frame, name) {
        return result;
    }
    if !name.contains('(') && frame.naxes() == 1 {
        if let Some(result) = frame_clear_own(frame, &format!("{name}(1)")) {
            return result;
        }
    }
    mapping_clear(frame, name)
}

/// The generic axis path of `ClearAttrib`; see [`axis_path_set`].
fn axis_path_clear<F: FrameClass + ?Sized>(frame: &mut F, name: &str) -> Option<AstResult<()>> {
    let (attr, axis) = axis_form(name)?;
    Some((|| {
        let axis = validate_axis(frame, axis, "astClear")?;
        match axis_clear(frame.attr_axis_mut(axis)?.0, attr) {
            Err(e) if e.code() == ErrorCode::Badat => {}
            other => return other,
        }
        let this = std::ptr::from_mut(frame).cast::<()>();
        let (primary, paxis) = frame.primary_frame_mut(axis - 1);
        let mut used = false;
        if !std::ptr::eq(std::ptr::from_mut(primary).cast::<()>(), this) {
            let index = primary_axis_index(primary, paxis);
            used = primary.attr_clear(&format!("{attr}({index})")).is_ok();
        }
        if !used {
            used = primary.attr_clear(attr).is_ok();
        }
        if used {
            Ok(())
        } else {
            axis_clear(frame.attr_axis_mut(axis)?.0, attr)
        }
    })())
}
