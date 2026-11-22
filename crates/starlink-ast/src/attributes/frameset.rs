// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a FrameSet, and the integrity of its Mappings while a
//! string of settings changes its current Frame.
//!
//! A FrameSet handles its own names (Base, Current, Variant, the read-only
//! Nframe and AllVariants) and the Object and Mapping names it keeps itself
//! (ID, Ident, Invert, Report and the read-only ones), and hands every other
//! name to its current Frame. It does not fall through to the Frame class.
//!
//! A setting that reaches the current Frame changes it without touching the
//! Mappings. Once the whole string has been applied, the FrameSet finds the
//! Mapping from the current Frame as it was to the Frame as it is, and
//! remaps the current Frame with it, so that the FrameSet still joins the
//! same positions: one setting string, or one list of names to clear, is one
//! change of integrity. If a setting fails, the string stops there and the
//! Mappings are left as they were.
// C: frameset.c SetAttrib, GetAttrib, TestAttrib, ClearAttrib, and the VSet
// and Clear over-rides with RecordIntegrity and RestoreIntegrity. C keeps the
// recorded Frame, the "integrity lost" flag and the method name in
// per-thread globals, which VSet and Clear save and restore around their
// work so that a nested call cannot disturb them; the thread-local below is
// that state.

use std::cell::RefCell;

use super::{
    fmt_int, int_value, is_read_only, keyword_value, no_value, read_only_clear, read_only_set,
    string_value,
};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::chr_match;
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::{Mapping, UnitMap};

/// A class that is a FrameSet: the FrameSet itself, and a Plot, which holds
/// one. Errors name the object's own class.
pub(crate) trait FrameSetClass: Frame {
    /// The FrameSet part of the object.
    fn fs(&self) -> &FrameSet;
    /// The FrameSet part of the object, for changing.
    fn fs_mut(&mut self) -> &mut FrameSet;
}

impl FrameSetClass for FrameSet {
    fn fs(&self) -> &FrameSet {
        self
    }
    fn fs_mut(&mut self) -> &mut FrameSet {
        self
    }
}

/// The state C keeps between `RecordIntegrity` and `RestoreIntegrity`.
struct Integrity {
    /// A copy of the current Frame as it was when recorded.
    frame: Option<DynFrame>,
    /// Whether a setting has reached the current Frame since.
    lost: bool,
    /// The C function an error names: `astSet` or `astClear`.
    method: &'static str,
}

thread_local! {
    static INTEGRITY: RefCell<Integrity> = const {
        RefCell::new(Integrity { frame: None, lost: false, method: "" })
    };
}

/// Replace the integrity state, returning the old one.
fn swap_state(state: Integrity) -> Integrity {
    INTEGRITY.with(|s| s.replace(state))
}

/// Note that a setting has reached the current Frame.
fn mark_lost() {
    INTEGRITY.with(|s| s.borrow_mut().lost = true);
}

/// Record a copy of the current Frame.
// C: frameset.c RecordIntegrity.
fn record_integrity<F: FrameSetClass + ?Sized>(obj: &F) {
    let frame = obj.fs().current().snapshot();
    INTEGRITY.with(|s| {
        let mut s = s.borrow_mut();
        s.frame = Some(frame);
        s.lost = false;
    });
}

/// If the current Frame has changed since it was recorded, remap it with
/// the Mapping from the recorded copy, then forget the copy.
// C: frameset.c RestoreIntegrity.
fn restore_integrity<F: FrameSetClass + ?Sized>(obj: &mut F) -> AstResult<()> {
    let (old, lost, method) = INTEGRITY.with(|s| {
        let mut s = s.borrow_mut();
        let lost = std::mem::replace(&mut s.lost, false);
        (s.frame.take(), lost, s.method)
    });
    let Some(old) = old else { return Ok(()) };
    if !lost || obj.fs().nframe() <= 1 {
        return Ok(());
    }
    let map = obj
        .fs()
        .current()
        .map_from(old.as_ref())
        .map_err(|e| match e.code() {
            ErrorCode::Ilost => {
                let class = obj.attr_class();
                AstError::new(
                    ErrorCode::Ilost,
                    format!("{method}({class}): Cannot maintain {class} integrity."),
                )
            }
            _ => e,
        })?;
    let map = map.simplify()?;
    // C remaps only when the Mapping is not a UnitMap.
    if map.as_ref().as_any().is::<UnitMap>() {
        return Ok(());
    }
    obj.fs_mut().remap_frame(FrameRef::Current, map)
}

/// Run `work`, a string of settings or a list of names to clear, as one
/// change of integrity, adding `context` to an error that only the
/// restoring of integrity gives.
fn bracket<F: FrameSetClass + ?Sized>(
    obj: &mut F,
    method: &'static str,
    context: impl FnOnce() -> String,
    work: impl FnOnce(&mut F) -> AstResult<()>,
) -> AstResult<()> {
    let _saved = SavedState(Some(swap_state(Integrity {
        frame: None,
        lost: false,
        method,
    })));
    record_integrity(obj);
    match work(obj) {
        // After a failure C restores nothing.
        Err(e) => Err(e),
        Ok(()) => restore_integrity(obj).map_err(|e| e.with_context(context())),
    }
}

/// The integrity state from before a bracket, put back when the bracket
/// ends, however it ends.
struct SavedState(Option<Integrity>);

impl Drop for SavedState {
    fn drop(&mut self) {
        if let Some(state) = self.0.take() {
            swap_state(state);
        }
    }
}

/// FrameSet's `VSet`: the assignments of one setting string, as one change
/// of integrity.
pub(crate) fn frameset_vset<F: FrameSetClass + ?Sized>(
    obj: &mut F,
    assignments: &[String],
    text: &str,
) -> AstResult<()> {
    if text.is_empty() {
        return Ok(());
    }
    bracket(
        obj,
        "astSet",
        || format!("Unable to accommodate the attribute setting \"{text}\"."),
        |obj| assignments.iter().try_for_each(|a| obj.attr_set(a)),
    )
}

/// FrameSet's `Clear`: the names of one list, as one change of integrity.
pub(crate) fn frameset_clear_list<F: FrameSetClass + ?Sized>(
    obj: &mut F,
    names: &str,
) -> AstResult<()> {
    bracket(
        obj,
        "astClear",
        || format!("Unable to accommodate clearing the \"{names}\" attribute(s)."),
        |obj| {
            super::split_names(names)
                .iter()
                .try_for_each(|n| obj.attr_clear(n))
        },
    )
}

/// The Frame a Base or Current setting of `index` names: C's
/// `astValidateFrameIndex`, for the function `method`.
fn frame_ref<F: FrameSetClass + ?Sized>(obj: &F, index: i32, method: &str) -> AstResult<FrameRef> {
    let nframe = obj.fs().nframe();
    match index {
        0 => Ok(FrameRef::Base),
        -1 => Ok(FrameRef::Current),
        n if n >= 1 && usize::try_from(n).is_ok_and(|n| n <= nframe) => {
            Ok(FrameRef::Index(n as usize))
        }
        n => Err(AstError::new(
            ErrorCode::Frmin,
            format!(
                "{method}({}): Invalid Frame index ({n}) given.",
                obj.attr_class()
            ),
        )
        .with_context(format!(
            "This value should be in the range 1 to {nframe} (or AST__CURRENT or AST__BASE)."
        ))),
    }
}

/// The Frame a Base or Current setting names by a word: `None` for the
/// Frame that is already that one, else the Frame, or an error.
fn frame_by_word<F: FrameSetClass + ?Sized>(
    obj: &F,
    word: &str,
    same: [&str; 2],
    other: [&str; 2],
    attr: &str,
) -> AstResult<Option<FrameRef>> {
    if same.iter().any(|w| chr_match(w, word)) {
        return Ok(None);
    }
    if other.iter().any(|w| chr_match(w, word)) {
        return Ok(Some(if attr == "Base" {
            FrameRef::Current
        } else {
            FrameRef::Base
        }));
    }
    // A Frame named by its Domain: the first that matches.
    let fs = obj.fs();
    for i in 1..=fs.nframe() {
        if chr_match(fs.frame(FrameRef::Index(i))?.domain(), word) {
            return Ok(Some(FrameRef::Index(i)));
        }
    }
    Err(AstError::new(
        ErrorCode::Attin,
        format!(
            "astSetAttrib({}): Invalid index value for {attr} Frame \"{word}\".",
            obj.attr_class()
        ),
    ))
}

/// The read-only attributes a FrameSet handles itself.
const READ_ONLY: [&str; 9] = [
    "allvariants",
    "class",
    "nframe",
    "nin",
    "nobject",
    "nout",
    "refcount",
    "tranforward",
    "traninverse",
];

/// Make Current another Frame: the integrity of the current Frame is
/// restored first and recorded afresh after.
fn change_current<F: FrameSetClass + ?Sized>(
    obj: &mut F,
    change: impl FnOnce(&mut FrameSet) -> AstResult<()>,
) -> AstResult<()> {
    restore_integrity(obj)?;
    change(obj.fs_mut())?;
    record_integrity(obj);
    Ok(())
}

/// FrameSet's `SetAttrib`.
pub(crate) fn frameset_set<F: FrameSetClass + ?Sized>(obj: &mut F, setting: &str) -> AstResult<()> {
    if let Some(v) = int_value(setting, "base") {
        let which = frame_ref(obj, v, "astSetBase")?;
        obj.fs_mut().set_base(which)
    } else if let Some(v) = keyword_value(setting, "base") {
        match frame_by_word(
            obj,
            v,
            ["AST__BASE", "Base"],
            ["AST__CURRENT", "Current"],
            "Base",
        )? {
            Some(which) => obj.fs_mut().set_base(which),
            None => Ok(()),
        }
    } else if let Some(v) = int_value(setting, "current") {
        let which = frame_ref(obj, v, "astSetCurrent")?;
        change_current(obj, |fs| fs.set_current(which))
    } else if let Some(v) = keyword_value(setting, "current") {
        let found = frame_by_word(
            obj,
            v,
            ["AST__CURRENT", "Current"],
            ["AST__BASE", "Base"],
            "Current",
        )?;
        match found {
            Some(which) => change_current(obj, |fs| fs.set_current(which)),
            None => Ok(()),
        }
    } else if let Some(v) = string_value(setting, "id") {
        obj.set_id(v)
    } else if let Some(v) = string_value(setting, "ident") {
        obj.set_ident(v)
    } else if let Some(v) = int_value(setting, "invert") {
        change_current(obj, |fs| {
            Mapping::set_invert(fs, v != 0);
            Ok(())
        })
    } else if let Some(v) = int_value(setting, "report") {
        Mapping::set_report(obj.fs_mut(), v != 0);
        Ok(())
    } else if let Some(v) = string_value(setting, "variant") {
        obj.fs_mut().set_variant(v)
    } else if READ_ONLY.iter().any(|name| is_read_only(setting, name)) {
        Err(read_only_set(setting, obj.attr_class()))
    } else {
        let result = obj.fs_mut().frame_mut(FrameRef::Current)?.attr_set(setting);
        mark_lost();
        result
    }
}

/// FrameSet's `GetAttrib`.
pub(crate) fn frameset_get<F: FrameSetClass + ?Sized>(obj: &F, name: &str) -> AstResult<String> {
    let fs = obj.fs();
    let count = |v: usize| fmt_int(i32::try_from(v).unwrap_or(i32::MAX));
    let flag = |v: bool| fmt_int(i32::from(v));
    Ok(match name {
        "allvariants" => fs.all_variants(),
        "base" => count(fs.base_index()),
        "class" => obj.attr_class().to_string(),
        "current" => count(fs.current_index()),
        "id" => obj.id().to_string(),
        "ident" => obj.ident().to_string(),
        "invert" => flag(Mapping::is_inverted(fs)),
        "nframe" => count(fs.nframe()),
        "nin" => count(Mapping::nin(fs)),
        "nobject" => return Err(no_value("Nobject", obj.attr_class())),
        "nout" => count(Mapping::nout(fs)),
        "refcount" => return Err(no_value("RefCount", obj.attr_class())),
        "report" => flag(Mapping::report(fs)),
        "tranforward" => flag(Mapping::has_forward(fs)),
        "traninverse" => flag(Mapping::has_inverse(fs)),
        "variant" => fs.variant(),
        _ => return fs.current().attr_get(name),
    })
}

/// FrameSet's `TestAttrib`.
pub(crate) fn frameset_test<F: FrameSetClass + ?Sized>(obj: &F, name: &str) -> AstResult<bool> {
    let fs = obj.fs();
    Ok(match name {
        "base" => fs.test_base(),
        "current" => fs.test_current(),
        "id" => obj.test_id(),
        "ident" => obj.test_ident(),
        "invert" => Mapping::test_invert(fs),
        "report" => Mapping::test_report(fs),
        "variant" => fs.test_variant(),
        _ if READ_ONLY.contains(&name) => false,
        _ => return fs.current().attr_test(name),
    })
}

/// FrameSet's `ClearAttrib`.
pub(crate) fn frameset_clear<F: FrameSetClass + ?Sized>(obj: &mut F, name: &str) -> AstResult<()> {
    match name {
        "base" => obj.fs_mut().clear_base(),
        "current" => {
            return change_current(obj, |fs| {
                fs.clear_current();
                Ok(())
            });
        }
        "id" => return obj.clear_id(),
        "ident" => return obj.clear_ident(),
        "invert" => {
            return change_current(obj, |fs| {
                Mapping::clear_invert(fs);
                Ok(())
            });
        }
        "report" => Mapping::clear_report(obj.fs_mut()),
        "variant" => obj.fs_mut().clear_variant(),
        _ if READ_ONLY.contains(&name) => return Err(read_only_clear(name, obj.attr_class())),
        _ => {
            let result = obj.fs_mut().frame_mut(FrameRef::Current)?.attr_clear(name);
            mark_lost();
            return result;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{BasicFrameBuilder, SkyFrame, SkySystem};
    use crate::mappings::UnitMap;

    /// The integrity state of the thread: the method and whether a Frame
    /// is recorded.
    fn state() -> (&'static str, bool) {
        INTEGRITY.with(|s| {
            let s = s.borrow();
            (s.method, s.frame.is_some())
        })
    }

    // A setting that panics inside a string must not leave the string's
    // integrity state behind for the thread's later work.
    #[test]
    fn a_panic_inside_a_string_restores_the_state_before_it() {
        let mut fs = FrameSet::new(Box::new(BasicFrameBuilder::new(2).build().unwrap()));
        fs.add_frame(
            FrameRef::Base,
            Box::new(UnitMap::new(2)),
            Box::new(SkyFrame::new(SkySystem::Icrs)),
        )
        .unwrap();
        assert_eq!(state(), ("", false));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bracket(&mut fs, "astSet", String::new, |_| {
                panic!("a setting panics")
            })
        }));
        assert!(caught.is_err());
        assert_eq!(state(), ("", false));
    }
}
