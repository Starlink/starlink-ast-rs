// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The Object-class `ID` attribute.

/// The `ID` attribute of an object: a string identifying it, unset by
/// default. Unlike `Ident`, a clone does not inherit it, as C's `astCopy`
/// does not copy it (object.c `Copy`): a clone is a new object. Moving an
/// object keeps it.
///
/// C also keeps the ID of an object it shares with `astClone` and drops that
/// of one it copies internally, such as a Frame added to a FrameSet or the
/// object a simplification that changes nothing returns. The port follows
/// the plain rule instead: ID is a label for the user, which the library
/// does not use, and those internal choices are not part of its meaning.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct ObjectId(Option<String>);

impl Clone for ObjectId {
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl ObjectId {
    /// The value, blank unless set.
    pub(crate) fn get(&self) -> &str {
        self.0.as_deref().unwrap_or("")
    }

    /// The value, `None` unless set.
    pub(crate) fn as_option(&self) -> Option<&str> {
        self.0.as_deref()
    }

    pub(crate) fn set(&mut self, id: &str) {
        self.0 = Some(id.to_string());
    }

    pub(crate) fn test(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn clear(&mut self) {
        self.0 = None;
    }
}

impl ObjectId {
    /// The ID held by the Object part of a dump, as object.c
    /// `astLoadObject` reads it.
    pub(crate) fn from_object_part(object: &crate::serial::Node) -> Self {
        Self(object.get_string_if_set("ID"))
    }
}
