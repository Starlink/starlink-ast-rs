// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of the Axis object of one axis of a Frame.
//!
//! In C an axis is an object of class Axis or SkyAxis that the Frame holds,
//! with its own ID, Ident and UseDefs. A Frame hands a name with an axis
//! index that it does not own, such as `Ident(2)`, to that object without
//! the index, which is how these functions are reached.
//! The Frame's own functions take every form of the Axis attributes proper
//! (`Label(1)`, `Top(2)` and the rest) first, so those arms here see only a
//! setting whose value the Frame's arm could not read.
// C: axis.c and skyaxis.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib;
// Axis's parent is Object and SkyAxis's is Axis.

use super::object::{ObjectAttrs, ObjectRead, object_clear, object_get, object_set, object_test};
use super::{double_value, fmt_double, fmt_int, int_value, is_read_only, string_value};
use super::{read_only_clear, read_only_set};
use crate::error::AstResult;
use crate::frames::axis::{AxisMut, AxisRef};

impl ObjectRead for AxisRef<'_> {
    fn obj_class(&self) -> &'static str {
        match self {
            AxisRef::Plain(_) => "Axis",
            AxisRef::Sky(_) => "SkyAxis",
        }
    }
    fn obj_id(&self) -> String {
        self.axis().id.get().to_string()
    }
    fn obj_test_id(&self) -> bool {
        self.axis().id.test()
    }
    fn obj_ident(&self) -> String {
        self.axis().ident.clone().unwrap_or_default()
    }
    fn obj_test_ident(&self) -> bool {
        self.axis().ident.is_some()
    }
    fn obj_use_defs(&self) -> bool {
        self.axis().use_defs.unwrap_or(true)
    }
    fn obj_test_use_defs(&self) -> bool {
        self.axis().use_defs.is_some()
    }
}

impl ObjectRead for AxisMut<'_> {
    fn obj_class(&self) -> &'static str {
        self.as_ref().obj_class()
    }
    fn obj_id(&self) -> String {
        self.as_ref().obj_id()
    }
    fn obj_test_id(&self) -> bool {
        self.as_ref().obj_test_id()
    }
    fn obj_ident(&self) -> String {
        self.as_ref().obj_ident()
    }
    fn obj_test_ident(&self) -> bool {
        self.as_ref().obj_test_ident()
    }
    fn obj_use_defs(&self) -> bool {
        self.as_ref().obj_use_defs()
    }
    fn obj_test_use_defs(&self) -> bool {
        self.as_ref().obj_test_use_defs()
    }
}

impl ObjectAttrs for AxisMut<'_> {
    fn obj_set_id(&mut self, id: &str) -> AstResult<()> {
        self.axis_mut().id.set(id);
        Ok(())
    }
    fn obj_clear_id(&mut self) -> AstResult<()> {
        self.axis_mut().id.clear();
        Ok(())
    }
    fn obj_set_ident(&mut self, ident: &str) -> AstResult<()> {
        self.axis_mut().ident = Some(ident.to_string());
        Ok(())
    }
    fn obj_clear_ident(&mut self) -> AstResult<()> {
        self.axis_mut().ident = None;
        Ok(())
    }
    fn obj_set_use_defs(&mut self, value: bool) -> AstResult<()> {
        self.axis_mut().use_defs = Some(value);
        Ok(())
    }
    fn obj_clear_use_defs(&mut self) -> AstResult<()> {
        self.axis_mut().use_defs = None;
        Ok(())
    }
}

/// The Format slot of the object: a SkyAxis keeps its own.
fn format_slot<'a>(ax: &'a mut AxisMut<'_>) -> &'a mut Option<String> {
    match ax {
        AxisMut::Plain(a) => &mut a.format,
        AxisMut::Sky(s) => &mut s.format,
    }
}

/// SkyAxis's then Axis's `SetAttrib`.
pub(crate) fn axis_set(mut ax: AxisMut<'_>, setting: &str) -> AstResult<()> {
    if let AxisMut::Sky(sky) = &mut ax {
        if let Some(v) = int_value(setting, "astime") {
            sky.as_time = Some(v != 0);
            return Ok(());
        } else if let Some(v) = int_value(setting, "islatitude") {
            sky.is_latitude = Some(v != 0);
            return Ok(());
        } else if let Some(v) = int_value(setting, "centrezero") {
            sky.centre_zero = Some(v != 0);
            return Ok(());
        }
    }
    if let Some(v) = int_value(setting, "digits") {
        ax.axis_mut().digits = Some(v.max(1));
    } else if let Some(v) = int_value(setting, "direction") {
        ax.axis_mut().direction = Some(v != 0);
    } else if let Some(v) = double_value(setting, "top") {
        ax.axis_mut().top = Some(v);
    } else if let Some(v) = double_value(setting, "bottom") {
        ax.axis_mut().bottom = Some(v);
    } else if let Some(v) = string_value(setting, "format") {
        *format_slot(&mut ax) = Some(v.to_string());
    } else if let Some(v) = string_value(setting, "label") {
        ax.axis_mut().label = Some(v.to_string());
    } else if let Some(v) = string_value(setting, "symbol") {
        ax.axis_mut().symbol = Some(v.to_string());
    } else if let Some(v) = string_value(setting, "unit") {
        ax.axis_mut().unit = Some(v.to_string());
    } else if is_read_only(setting, "normunit") || is_read_only(setting, "internalunit") {
        return Err(read_only_set(setting, ax.obj_class()));
    } else {
        return object_set(&mut ax, setting);
    }
    Ok(())
}

/// SkyAxis's then Axis's `GetAttrib`.
pub(crate) fn axis_get(ax: AxisRef<'_>, name: &str) -> AstResult<String> {
    if let AxisRef::Sky(sky) = ax {
        match name {
            "astime" => return Ok(fmt_int(i32::from(sky.as_time_value()))),
            "islatitude" => return Ok(fmt_int(i32::from(sky.is_latitude_value()))),
            "centrezero" => return Ok(fmt_int(i32::from(sky.centre_zero_value()))),
            _ => {}
        }
    }
    // The values of the Axis attributes proper are the Frame's to give: it
    // answers `Label(1)` and the rest itself, with its own defaults, and
    // never asks the Axis object for them. What is set is all this object
    // can add.
    let a = ax.axis();
    let stored = match name {
        "digits" => a.digits.map(fmt_int),
        "direction" => a.direction.map(|v| fmt_int(i32::from(v))),
        "top" => a.top.map(fmt_double),
        "bottom" => a.bottom.map(fmt_double),
        "format" => match ax {
            AxisRef::Plain(p) => p.format.clone(),
            AxisRef::Sky(s) => s.format.clone(),
        },
        "label" => a.label.clone(),
        "symbol" => a.symbol.clone(),
        "unit" => a.unit.clone(),
        _ => None,
    };
    match stored {
        Some(value) => Ok(value),
        None => object_get(&ax, name),
    }
}

/// SkyAxis's then Axis's `TestAttrib`.
pub(crate) fn axis_test(ax: AxisRef<'_>, name: &str) -> AstResult<bool> {
    if let AxisRef::Sky(sky) = ax {
        match name {
            "astime" => return Ok(sky.as_time.is_some()),
            "islatitude" => return Ok(sky.is_latitude.is_some()),
            "centrezero" => return Ok(sky.centre_zero.is_some()),
            _ => {}
        }
    }
    let a = ax.axis();
    match name {
        "digits" => Ok(a.digits.is_some()),
        "direction" => Ok(a.direction.is_some()),
        "top" => Ok(a.top.is_some()),
        "bottom" => Ok(a.bottom.is_some()),
        "format" => Ok(match ax {
            AxisRef::Plain(p) => p.format.is_some(),
            AxisRef::Sky(s) => s.format.is_some(),
        }),
        "label" => Ok(a.label.is_some()),
        "symbol" => Ok(a.symbol.is_some()),
        "unit" => Ok(a.unit.is_some()),
        "normunit" | "internalunit" => Ok(false),
        _ => object_test(&ax, name),
    }
}

/// SkyAxis's then Axis's `ClearAttrib`.
pub(crate) fn axis_clear(mut ax: AxisMut<'_>, name: &str) -> AstResult<()> {
    if let AxisMut::Sky(sky) = &mut ax {
        match name {
            "astime" => {
                sky.as_time = None;
                return Ok(());
            }
            "islatitude" => {
                sky.is_latitude = None;
                return Ok(());
            }
            "centrezero" => {
                sky.centre_zero = None;
                return Ok(());
            }
            _ => {}
        }
    }
    match name {
        "digits" => ax.axis_mut().digits = None,
        "direction" => ax.axis_mut().direction = None,
        "top" => ax.axis_mut().top = None,
        "bottom" => ax.axis_mut().bottom = None,
        "format" => *format_slot(&mut ax) = None,
        "label" => ax.axis_mut().label = None,
        "symbol" => ax.axis_mut().symbol = None,
        "unit" => ax.axis_mut().unit = None,
        "normunit" | "internalunit" => return Err(read_only_clear(name, ax.obj_class())),
        _ => return object_clear(&mut ax, name),
    }
    Ok(())
}
