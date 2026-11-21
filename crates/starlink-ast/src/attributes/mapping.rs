// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The Mapping attributes of a Frame: Invert and Report, and the read-only
//! Nin, Nout, IsLinear, IsSimple, TranForward and TranInverse.
//!
//! In C every Frame is a Mapping, the unit transformation between its own
//! axes, so it has these attributes. A Frame here is not a Mapping and
//! stores no Mapping flags, so the read-only ones are given the values C
//! gives for a Frame, Invert and Report read as unset, and setting or
//! clearing either of those is an error. A FrameSet or a Plot is a Mapping,
//! and handles these names itself before they reach this file.
// C: mapping.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, whose
// parent is Object.

use super::object::{object_clear, object_get, object_set, object_test};
use super::{fmt_int, int_value, is_read_only, not_stored, read_only_clear, read_only_set};
use crate::error::AstResult;
use crate::frames::Frame;

/// The read-only attributes of a Mapping.
const READ_ONLY: [&str; 6] = [
    "nin",
    "nout",
    "islinear",
    "issimple",
    "tranforward",
    "traninverse",
];

/// Mapping's `SetAttrib` for a Frame.
pub(crate) fn mapping_set<F: Frame + ?Sized>(frame: &mut F, setting: &str) -> AstResult<()> {
    if int_value(setting, "invert").is_some() {
        Err(not_stored("astSet", "Invert", frame.attr_class()))
    } else if int_value(setting, "report").is_some() {
        Err(not_stored("astSet", "Report", frame.attr_class()))
    } else if READ_ONLY.iter().any(|name| is_read_only(setting, name)) {
        Err(read_only_set(setting, frame.attr_class()))
    } else {
        object_set(frame, setting)
    }
}

/// Mapping's `GetAttrib` for a Frame.
pub(crate) fn mapping_get<F: Frame + ?Sized>(frame: &F, name: &str) -> AstResult<String> {
    // A Frame's transformation is the identity between its axes: linear,
    // defined both ways, and never marked as simplified.
    let naxes = i32::try_from(frame.naxes()).unwrap_or(i32::MAX);
    match name {
        "invert" | "report" | "issimple" => Ok(fmt_int(0)),
        "islinear" | "tranforward" | "traninverse" => Ok(fmt_int(1)),
        "nin" | "nout" => Ok(fmt_int(naxes)),
        _ => object_get(frame, name),
    }
}

/// Mapping's `TestAttrib` for a Frame.
pub(crate) fn mapping_test<F: Frame + ?Sized>(frame: &F, name: &str) -> AstResult<bool> {
    match name {
        "invert" | "report" => Ok(false),
        _ if READ_ONLY.contains(&name) => Ok(false),
        _ => object_test(frame, name),
    }
}

/// Mapping's `ClearAttrib` for a Frame.
pub(crate) fn mapping_clear<F: Frame + ?Sized>(frame: &mut F, name: &str) -> AstResult<()> {
    match name {
        "invert" => Err(not_stored("astClear", "Invert", frame.attr_class())),
        "report" => Err(not_stored("astClear", "Report", frame.attr_class())),
        _ if READ_ONLY.contains(&name) => Err(read_only_clear(name, frame.attr_class())),
        _ => object_clear(frame, name),
    }
}
