//! The conditions C's `Warn` reports, and the Warnings attribute that selects
//! them (fitschan.c:35431, 43585).
//!
//! Every warning a read or write raises carries the lower-case name of its
//! condition. It is stored, as `ASTWARN` cards and in the Channel's warning
//! list, only if the FitsChan's Warnings attribute names that condition.

use crate::error::{AstError, AstResult, ErrorCode};

/// C's `ALLWARNINGS`: every condition the Warnings attribute accepts, lower
/// case, with a space before and after each name.
const ALL_WARNINGS: &str = " distortion noequinox noradesys nomjd-obs nolonpole nolatpole tnx zpx badcel noctype badlat badmat badval badctype badpv badkeyname badkeyvalue badalt ";

/// The Warnings attribute's default.
pub(crate) const DEFAULT_WARNINGS: &str =
    "BadKeyName BadKeyValue Tnx Zpx BadPV BadCel BadMat BadCTYPE";

/// A warning and the condition that raised it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Warning {
    /// The lower-case condition name, one of those in `ALL_WARNINGS`.
    pub condition: &'static str,
    /// The text of the warning.
    pub text: String,
    /// The method an error for it names, where C names one of its own
    /// rather than the one the list was made for.
    pub method: Option<&'static str>,
}

impl Warning {
    pub(crate) fn new(condition: &'static str, text: impl Into<String>) -> Self {
        debug_assert!(ALL_WARNINGS.contains(&format!(" {condition} ")));
        Self {
            condition,
            text: text.into(),
            method: None,
        }
    }

    /// The warning, with an error for it naming `method`.
    pub(crate) fn named_by(mut self, method: &'static str) -> Self {
        self.method = Some(method);
        self
    }
}

/// The warnings a read or write has raised, in the order raised.
///
/// With Strict in force a warning the Warnings attribute names is not added:
/// [`push`](Self::push) returns it as the error C's `astAddWarning` reports,
/// so the work that raised it stops there, as C's does.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Warnings {
    list: Vec<Warning>,
    strict: Option<Strict>,
}

/// What makes a warning an error: the Warnings attribute and the name of the
/// method C reports the error from.
#[derive(Debug, Clone, PartialEq)]
struct Strict {
    conditions: String,
    method: &'static str,
}

impl Warnings {
    /// An empty list, to which every warning is added.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// An empty list for a FitsChan with Strict in force, whose Warnings
    /// attribute is `conditions`, raised by `method`.
    pub(crate) fn strict(conditions: &str, method: &'static str) -> Self {
        Self {
            list: Vec::new(),
            strict: Some(Strict {
                conditions: conditions.to_string(),
                method,
            }),
        }
    }

    /// An empty list that treats warnings as this one does.
    pub(crate) fn child(&self) -> Self {
        Self {
            list: Vec::new(),
            strict: self.strict.clone(),
        }
    }

    /// The error `warning` is with Strict in force and its condition
    /// reported, or `Ok` when it is only a warning.
    pub(crate) fn check(&self, warning: &Warning) -> AstResult<()> {
        match &self.strict {
            Some(s) if !warning.text.is_empty() && reports(&s.conditions, warning.condition) => {
                Err(strict_error(
                    warning.method.unwrap_or(s.method),
                    &warning.text,
                ))
            }
            _ => Ok(()),
        }
    }

    /// Add `warning`, or with Strict in force and its condition reported,
    /// return it as an error.
    pub(crate) fn push(&mut self, warning: Warning) -> AstResult<()> {
        self.check(&warning)?;
        self.list.push(warning);
        Ok(())
    }

    /// Add the warnings of `other`, which were checked as they were added
    /// to it.
    pub(crate) fn append(&mut self, other: Warnings) {
        self.list.extend(other.list);
    }
}

impl std::ops::Deref for Warnings {
    type Target = [Warning];
    fn deref(&self) -> &[Warning] {
        &self.list
    }
}

impl IntoIterator for Warnings {
    type Item = Warning;
    type IntoIter = std::vec::IntoIter<Warning>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

/// The error C's `astAddWarning` reports for a warning with Strict set.
pub(crate) fn strict_error(method: &str, text: &str) -> AstError {
    AstError::new(ErrorCode::Badin, format!("{method}(FitsChan): {text}"))
}

/// C's `GetAllWarnings`: every condition name, space separated.
pub(crate) fn all_warnings() -> &'static str {
    ALL_WARNINGS
}

/// C's `GoodWarns`: check that `value` is a list of known conditions,
/// separated by white space and in any case.
pub(crate) fn good_warns(value: &str) -> AstResult<()> {
    for word in value.split(|c: char| c.is_ascii_whitespace() || c == '\x0b') {
        if word.is_empty() {
            continue;
        }
        let word = word.to_ascii_lowercase();
        if !ALL_WARNINGS.contains(&format!(" {word} ")) {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetWarnings(fitschan): Unknown condition '{word}' specified when \
                     setting the Warnings attribute."
                ),
            ));
        }
    }
    Ok(())
}

/// Whether the Warnings value `list` names `condition`: C's
/// `FullForm( list, condition, 0 ) >= 0`, which splits `list` at spaces only
/// and compares each word, without its trailing blanks, ignoring case.
pub(crate) fn reports(list: &str, condition: &str) -> bool {
    list.split(' ')
        .filter(|option| !option.is_empty())
        .any(|option| option[..super::card::chr_len(option)].eq_ignore_ascii_case(condition))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_list_names_its_conditions_only() {
        for c in [
            "badkeyname",
            "badkeyvalue",
            "tnx",
            "zpx",
            "badpv",
            "badcel",
            "badmat",
            "badctype",
        ] {
            assert!(reports(DEFAULT_WARNINGS, c), "{c}");
        }
        for c in ["badalt", "badval", "distortion", "noctype", "badlat"] {
            assert!(!reports(DEFAULT_WARNINGS, c), "{c}");
        }
    }

    #[test]
    fn reports_splits_at_spaces_only() {
        assert!(reports("  BADVAL  ", "badval"));
        assert!(!reports("badval\tbadlat", "badval"));
        assert!(!reports("badv", "badval"));
    }

    #[test]
    fn good_warns_accepts_any_case_and_white_space() {
        assert!(good_warns("").is_ok());
        assert!(good_warns(" BadVal\tnoCTYPE\n").is_ok());
        let e = good_warns("badval bogus").unwrap_err();
        assert_eq!(e.code(), ErrorCode::Attin);
        assert!(e.to_string().contains("'bogus'"));
    }
}
