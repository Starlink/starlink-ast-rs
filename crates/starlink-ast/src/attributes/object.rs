// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes every object has: ID, Ident and UseDefs, and the
//! read-only Class, Nobject, ObjSize and RefCount.
// C: object.c SetAttrib, GetAttrib, TestAttrib and ClearAttrib, the end of
// every class's chain.

use super::{
    fmt_int, int_value, is_read_only, no_value, read_only_clear, read_only_set, string_value,
    unknown_clear, unknown_get, unknown_set, unknown_test,
};
use crate::error::AstResult;
#[cfg(feature = "attribute-strings")]
use crate::frames::Frame;

/// The Object attributes of a class as the functions of this file read
/// them.
pub(crate) trait ObjectRead {
    /// The C class name, which is the Class attribute and the name errors
    /// give.
    fn obj_class(&self) -> &'static str;
    /// The ID attribute.
    fn obj_id(&self) -> String;
    /// Whether the ID attribute is set.
    fn obj_test_id(&self) -> bool;
    /// The Ident attribute.
    fn obj_ident(&self) -> String;
    /// Whether the Ident attribute is set.
    fn obj_test_ident(&self) -> bool;
    /// The UseDefs attribute.
    fn obj_use_defs(&self) -> bool;
    /// Whether the UseDefs attribute is set.
    fn obj_test_use_defs(&self) -> bool;
}

/// The Object attributes of a class as the functions of this file change
/// them.
pub(crate) trait ObjectAttrs: ObjectRead {
    /// Set the ID attribute.
    fn obj_set_id(&mut self, id: &str) -> AstResult<()>;
    /// Clear the ID attribute.
    fn obj_clear_id(&mut self) -> AstResult<()>;
    /// Set the Ident attribute.
    fn obj_set_ident(&mut self, ident: &str) -> AstResult<()>;
    /// Clear the Ident attribute.
    fn obj_clear_ident(&mut self) -> AstResult<()>;
    /// Set the UseDefs attribute.
    fn obj_set_use_defs(&mut self, value: bool) -> AstResult<()>;
    /// Clear the UseDefs attribute.
    fn obj_clear_use_defs(&mut self) -> AstResult<()>;
}

#[cfg(feature = "attribute-strings")]
impl<T: Frame + ?Sized> ObjectRead for T {
    fn obj_class(&self) -> &'static str {
        self.attr_class()
    }
    fn obj_id(&self) -> String {
        self.id().to_string()
    }
    fn obj_test_id(&self) -> bool {
        self.test_id()
    }
    fn obj_ident(&self) -> String {
        self.ident().to_string()
    }
    fn obj_test_ident(&self) -> bool {
        self.test_ident()
    }
    fn obj_use_defs(&self) -> bool {
        self.use_defs()
    }
    fn obj_test_use_defs(&self) -> bool {
        self.test_use_defs()
    }
}

#[cfg(feature = "attribute-strings")]
impl<T: Frame + ?Sized> ObjectAttrs for T {
    fn obj_set_id(&mut self, id: &str) -> AstResult<()> {
        self.set_id(id)
    }
    fn obj_clear_id(&mut self) -> AstResult<()> {
        self.clear_id()
    }
    fn obj_set_ident(&mut self, ident: &str) -> AstResult<()> {
        self.set_ident(ident)
    }
    fn obj_clear_ident(&mut self) -> AstResult<()> {
        self.clear_ident()
    }
    fn obj_set_use_defs(&mut self, value: bool) -> AstResult<()> {
        self.set_use_defs(value)
    }
    fn obj_clear_use_defs(&mut self) -> AstResult<()> {
        self.clear_use_defs()
    }
}

/// The read-only attributes of an Object.
const READ_ONLY: [&str; 4] = ["class", "nobject", "objsize", "refcount"];

/// Object's `SetAttrib`: any other setting is an error.
pub(crate) fn object_set<O: ObjectAttrs + ?Sized>(obj: &mut O, setting: &str) -> AstResult<()> {
    if let Some(v) = string_value(setting, "id") {
        obj.obj_set_id(v)
    } else if let Some(v) = string_value(setting, "ident") {
        obj.obj_set_ident(v)
    } else if let Some(v) = int_value(setting, "usedefs") {
        obj.obj_set_use_defs(v != 0)
    } else if READ_ONLY.iter().any(|name| is_read_only(setting, name)) {
        Err(read_only_set(setting, obj.obj_class()))
    } else {
        Err(unknown_set(setting, obj.obj_class()))
    }
}

/// Object's `GetAttrib`: any other name is an error.
pub(crate) fn object_get<O: ObjectRead + ?Sized>(obj: &O, name: &str) -> AstResult<String> {
    match name {
        "class" => Ok(obj.obj_class().to_string()),
        "id" => Ok(obj.obj_id()),
        "ident" => Ok(obj.obj_ident()),
        "usedefs" => Ok(fmt_int(i32::from(obj.obj_use_defs()))),
        // Counts of the C library's handles and allocations.
        "nobject" => Err(no_value("Nobject", obj.obj_class())),
        "objsize" => Err(no_value("ObjSize", obj.obj_class())),
        "refcount" => Err(no_value("RefCount", obj.obj_class())),
        _ => Err(unknown_get(name, obj.obj_class())),
    }
}

/// Object's `TestAttrib`: a read-only attribute is never set, and any
/// other name is an error.
pub(crate) fn object_test<O: ObjectRead + ?Sized>(obj: &O, name: &str) -> AstResult<bool> {
    match name {
        "id" => Ok(obj.obj_test_id()),
        "ident" => Ok(obj.obj_test_ident()),
        "usedefs" => Ok(obj.obj_test_use_defs()),
        _ if READ_ONLY.contains(&name) => Ok(false),
        _ => Err(unknown_test(name, obj.obj_class())),
    }
}

/// Object's `ClearAttrib`: any other name is an error.
pub(crate) fn object_clear<O: ObjectAttrs + ?Sized>(obj: &mut O, name: &str) -> AstResult<()> {
    match name {
        "id" => obj.obj_clear_id(),
        "ident" => obj.obj_clear_ident(),
        "usedefs" => obj.obj_clear_use_defs(),
        _ if READ_ONLY.contains(&name) => Err(read_only_clear(name, obj.obj_class())),
        _ => Err(unknown_clear(name, obj.obj_class())),
    }
}
