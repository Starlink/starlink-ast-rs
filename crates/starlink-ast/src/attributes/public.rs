// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The public face of attribute strings.

use super::{blank_get, normalise_name, split_setting_c, split_settings, strip_escapes};
use crate::error::AstResult;
use crate::frames::Frame;

/// Set, get, test and clear an object's attributes by name, from strings.
///
/// The names, the syntax and the values are those of the AST documentation's
/// attribute reference, and every result and error message is the C
/// library's: a program that takes attribute settings as text can pass them
/// straight through.
/// Names are not case sensitive and may contain white space.
///
/// It is implemented for `Plot`, `FrameSet` and every Frame, including a
/// `dyn Frame`, and for `FitsChan`.
/// The classes whose attributes can be reached are listed in the
/// [module documentation](crate::attributes); on any other the methods fail with
/// [`ErrorCode::Badat`](crate::error::ErrorCode::Badat).
/// An attribute reached through an object that holds another (a Frame
/// attribute set on a `FrameSet` or a `Plot`) behaves as it does through the
/// typed setters, so a `FrameSet` keeps its Mappings consistent.
///
/// ```
/// use starlink_ast::attributes::Attributes;
/// use starlink_ast::frames::BasicFrameBuilder;
///
/// let mut frame = BasicFrameBuilder::new(2).build()?;
/// frame.set_attributes("Ident=detector, UseDefs=0")?;
/// assert_eq!(frame.get_attribute("Ident")?, "detector");
/// assert!(frame.test_attribute("UseDefs")?);
/// frame.clear_attributes("Ident, UseDefs")?;
/// assert!(!frame.test_attribute("Ident")?);
/// # Ok::<(), starlink_ast::error::AstError>(())
/// ```
pub trait Attributes {
    /// Apply a comma-separated list of `name=value` settings, in order.
    ///
    /// A comma separates settings only when the string holds more than one
    /// `=`, so `"Title=a,b"` sets one value; a value in double quotes keeps
    /// its commas in any string.
    /// A value is taken as written after the `=`, including white space.
    /// The settings before one that fails stay applied, and those after it
    /// are not applied.
    ///
    /// The C library takes its string as a `printf` format; this takes it
    /// as text, so a `%` is an ordinary character.
    ///
    /// # Errors
    ///
    /// The C library's error for the first setting that fails: a name the
    /// object does not have, a read-only attribute, or a value the
    /// attribute does not accept.
    #[doc(alias = "astSet")]
    fn set_attributes(&mut self, settings: &str) -> AstResult<()>;

    /// Set one attribute to a value given as text.
    ///
    /// The value's commas are part of it, whatever else it contains.
    ///
    /// # Errors
    ///
    /// As [`set_attributes`](Self::set_attributes).
    #[doc(alias = "astSet")]
    fn set_attribute(&mut self, name: &str, value: &str) -> AstResult<()>;

    /// The value of an attribute as text: a number in decimal, with enough
    /// digits to read back exactly, and a flag as `0` or `1`.
    ///
    /// The value of an attribute that is not set is its default.
    /// Graphical escape sequences are removed from the value, as the C
    /// library removes them.
    ///
    /// It takes the object mutably because some attributes are found by
    /// working on it: a `FitsChan`'s gets read its cards.
    ///
    /// # Errors
    ///
    /// A blank name, a name the object does not have, or the failure of
    /// the work the value needs.
    #[doc(alias = "astGet")]
    fn get_attribute(&mut self, name: &str) -> AstResult<String>;

    /// Whether an attribute has been set, and not cleared since.
    /// A read-only attribute is never set.
    ///
    /// # Errors
    ///
    /// A name the object does not have.
    #[doc(alias = "astTest")]
    fn test_attribute(&self, name: &str) -> AstResult<bool>;

    /// Clear each attribute of a comma-separated list of names, so that it
    /// takes its default value again.
    /// Clearing an attribute that is not set does nothing.
    ///
    /// # Errors
    ///
    /// The C library's error for the first name that fails: a name the
    /// object does not have, or a read-only attribute.
    #[doc(alias = "astClear")]
    fn clear_attributes(&mut self, names: &str) -> AstResult<()>;
}

impl<T: Frame + ?Sized> Attributes for T {
    fn set_attributes(&mut self, settings: &str) -> AstResult<()> {
        self.attr_vset(&split_settings(settings), settings)
    }

    fn set_attribute(&mut self, name: &str, value: &str) -> AstResult<()> {
        let (assignments, text) = split_setting_c(name, value);
        self.attr_vset(&assignments, &text)
    }

    fn get_attribute(&mut self, name: &str) -> AstResult<String> {
        let name = normalise_name(name);
        if name.is_empty() {
            return Err(blank_get(self.attr_class()));
        }
        Ok(strip_escapes(&self.attr_get(&name)?))
    }

    fn test_attribute(&self, name: &str) -> AstResult<bool> {
        self.attr_test(&normalise_name(name))
    }

    fn clear_attributes(&mut self, names: &str) -> AstResult<()> {
        self.attr_clear_list(names)
    }
}
