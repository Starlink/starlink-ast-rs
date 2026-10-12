//! Keyword-name template matching.
//!
//! Port of C `Match` / `MatchFront` / `MatchChar` (fitschan.c:22619, 22826,
//! 22771). A template is a keyword name in which `%` introduces a field
//! specifier of the form `%<width><type>`:
//!
//! * width `0` matches zero or more characters, `1`-`9` matches exactly that
//!   many, and an omitted width matches one or more.
//! * type `d` matches decimal digits, `c` matches upper-case letters only, and
//!   `f` matches any character legal in a FITS keyword (letters of either case,
//!   digits, `_` and `-`).
//!
//! All other template characters must be matched by the same character in the
//! test string, ignoring case.

/// A legal FITS keyword character. Mirrors C `isFits` (fitschan.c:1394), which
/// accepts lower-case letters as well as upper.
fn is_fits(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

/// A template fault C's `MatchFront` and `MatchChar` report as AST__BDFMT.
#[derive(Clone, Copy)]
enum BadTemplate {
    /// An illegal field type.
    Type(char),
    /// A template ending within a field specifier.
    Incomplete,
}

thread_local! {
    /// The first template fault met during the current match.
    static BAD_TEMPLATE: std::cell::Cell<Option<BadTemplate>> =
        const { std::cell::Cell::new(None) };
}

fn note_bad_template(bad: BadTemplate) {
    BAD_TEMPLATE.with(|b| {
        if b.get().is_none() {
            b.set(Some(bad));
        }
    });
}

/// Does `test` belong to the character set named by `ty`?
fn match_char(test: u8, ty: char) -> bool {
    match ty {
        'd' => test.is_ascii_digit(),
        'c' => test.is_ascii_uppercase(),
        'f' => is_fits(test),
        _ => {
            note_bad_template(BadTemplate::Type(ty));
            false
        }
    }
}

/// The outcome of matching the leading field of a template.
struct Front {
    /// Field type character, or `None` when the template began with a literal.
    ty: Option<char>,
    /// Whether the field can absorb further test characters (unspecified or
    /// zero width).
    extend: bool,
    /// Characters consumed from the test string (the template minimum).
    ntest: usize,
    /// Characters consumed from the template.
    ntemp: usize,
}

/// Match the first field of `temp` against the start of `test`, returning
/// `None` when fewer than the minimum number of characters match.
fn match_front(test: &[u8], temp: &[u8]) -> Option<Front> {
    let tb = *temp.first().unwrap_or(&0);

    // A literal template character must equal the test character, bar case.
    if tb != b'%' {
        let ta = *test.first().unwrap_or(&0);
        if !tb.eq_ignore_ascii_case(&ta) {
            return None;
        }
        return Some(Front {
            ty: None,
            extend: false,
            ntest: 1,
            ntemp: 1,
        });
    }

    // A field specifier: optional width digit, then the type character.
    let mut ntemp = 3;
    let mut b = 1;
    let (ntest, extend) = match temp.get(b) {
        Some(&c) if c.is_ascii_digit() => {
            let n = (c - b'0') as usize;
            (n, n == 0)
        }
        // No width given: one or more characters match. Step back so the
        // current character is re-used as the type specifier.
        _ => {
            b -= 1;
            ntemp -= 1;
            (1, true)
        }
    };

    b += 1;
    let Some(&ty) = temp.get(b) else {
        note_bad_template(BadTemplate::Incomplete);
        return None;
    };
    let ty = ty as char;

    // C tests the string's terminating null too, so an illegal type is
    // found even when the test string has run out.
    for i in 0..ntest {
        if !match_char(test.get(i).copied().unwrap_or(0), ty) {
            return None;
        }
    }

    Some(Front {
        ty: Some(ty),
        extend,
        ntest,
        ntemp,
    })
}

/// Recursive body of [`match_keyword`]. Integer fields are pushed to `fields`
/// innermost-first (so in reverse template order) while `nfld` counts every
/// field found, matched or not stored.
fn match_inner(
    test: &[u8],
    temp: &[u8],
    maxfld: usize,
    fields: &mut Vec<i32>,
    nfld: &mut usize,
) -> bool {
    let Some(front) = match_front(test, temp) else {
        return false;
    };

    // Skip the characters used up by the leading field.
    let mut a = front.ntest;
    let b = front.ntemp;
    let mut matched = false;

    if a >= test.len() && b >= temp.len() {
        // Both strings exhausted together.
        matched = true;
    } else if b >= temp.len() && front.extend {
        // The template ran out but the field can absorb the rest of the test
        // string, provided every remaining character is of the field's type.
        let ty = front.ty.unwrap();
        while a < test.len() && match_char(test[a], ty) {
            a += 1;
        }
        if a >= test.len() {
            matched = true;
        }
    } else if match_inner(&test[a..], &temp[b..], maxfld, fields, nfld) {
        matched = true;
    } else if front.extend {
        // The remainders did not match at the minimum field width; widen the
        // leading field one character at a time and retry.
        let ty = front.ty.unwrap();
        while a < test.len() && match_char(test[a], ty) {
            a += 1;
            if match_inner(&test[a..], &temp[b..], maxfld, fields, nfld) {
                matched = true;
                break;
            }
        }
    }

    // If the strings match and the leading field is an integer, decode it.
    if matched && front.ty == Some('d') && a > 0 {
        if *nfld < maxfld {
            if let Ok(v) = std::str::from_utf8(&test[..a]).unwrap_or("").parse::<i32>() {
                fields.push(v);
            }
        }
        *nfld += 1;
    }

    matched
}

/// Does keyword name `test` match `template`?
///
/// On a match, returns the values of up to `maxfld` integer (`%d`) fields in
/// the order they occur in the template.
pub(crate) fn match_keyword(test: &str, template: &str, maxfld: usize) -> Option<Vec<i32>> {
    let mut fields = Vec::new();
    let mut nfld = 0;
    if match_inner(
        test.as_bytes(),
        template.as_bytes(),
        maxfld,
        &mut fields,
        &mut nfld,
    ) {
        fields.reverse();
        Some(fields)
    } else {
        None
    }
}

/// [`match_keyword`] for a template a caller supplied: an illegal field
/// type met while matching is AST__BDFMT, as C's `MatchChar` reports it.
pub(crate) fn match_keyword_checked(
    test: &str,
    template: &str,
    method: &str,
) -> crate::error::AstResult<bool> {
    BAD_TEMPLATE.with(|b| b.set(None));
    let matched = match_keyword(test, template, 0).is_some();
    let message = match BAD_TEMPLATE.with(|b| b.take()) {
        None => return Ok(matched),
        Some(BadTemplate::Type(ty)) => format!(
            "{method}(FitsChan): Illegal field type or width specifier '{ty}' found in \
             filter template '{template}'."
        ),
        Some(BadTemplate::Incomplete) => format!(
            "{method}(FitsChan): Incomplete field specifier found at end of filter \
             template '{template}'."
        ),
    };
    Err(crate::error::AstError::new(
        crate::error::ErrorCode::Bdfmt,
        message,
    ))
}

#[cfg(test)]
mod tests {
    use super::match_keyword;

    #[test]
    fn literal_template_matches_exactly() {
        assert_eq!(match_keyword("EQUINOX", "EQUINOX", 0), Some(vec![]));
        assert!(match_keyword("EQUINOXA", "EQUINOX", 0).is_none());
        assert!(match_keyword("EQUINO", "EQUINOX", 0).is_none());
    }

    #[test]
    fn literal_match_ignores_case() {
        assert!(match_keyword("equinox", "EQUINOX", 0).is_some());
    }

    #[test]
    fn single_digit_field_returns_index() {
        assert_eq!(match_keyword("CRVAL1", "CRVAL%d", 1), Some(vec![1]));
        assert_eq!(match_keyword("CRVAL12", "CRVAL%d", 1), Some(vec![12]));
        assert!(match_keyword("CRVAL1A", "CRVAL%d", 1).is_none());
        assert!(match_keyword("CRVAL", "CRVAL%d", 1).is_none());
    }

    #[test]
    fn version_letter_field_needs_upper_case() {
        assert_eq!(match_keyword("CRVAL1A", "CRVAL%d%1c", 1), Some(vec![1]));
        assert_eq!(match_keyword("CRVAL12Z", "CRVAL%d%1c", 1), Some(vec![12]));
        // "%1c" matches upper case only.
        assert!(match_keyword("CRVAL1a", "CRVAL%d%1c", 1).is_none());
        // Exactly one letter, no more.
        assert!(match_keyword("CRVAL1AB", "CRVAL%d%1c", 1).is_none());
    }

    #[test]
    fn two_digit_fields_come_back_in_template_order() {
        assert_eq!(match_keyword("PC1_2", "PC%d_%d", 2), Some(vec![1, 2]));
        assert_eq!(match_keyword("PC12_34", "PC%d_%d", 2), Some(vec![12, 34]));
        assert_eq!(match_keyword("PC1_2A", "PC%d_%d%1c", 2), Some(vec![1, 2]));
        assert!(match_keyword("PC1_2", "PC%d_%d%1c", 2).is_none());
        // The underscore is a literal, so the run-together AIPS form is not
        // matched by the FITS-WCS template.
        assert!(match_keyword("PC001001", "PC%d_%d", 2).is_none());
    }

    #[test]
    fn similar_prefixes_stay_distinct() {
        // SREF%d must not swallow SREFP1 / SREFIS.
        assert!(match_keyword("SREFP1", "SREF%d", 1).is_none());
        assert!(match_keyword("SREFIS", "SREF%d", 1).is_none());
        assert_eq!(match_keyword("SREFP1", "SREFP%d", 1), Some(vec![1]));
        // A_%d_%d must not swallow the AP_ form.
        assert!(match_keyword("AP_1_2", "A_%d_%d", 2).is_none());
        assert_eq!(match_keyword("AP_1_2", "AP_%d_%d", 2), Some(vec![1, 2]));
    }

    #[test]
    fn zero_width_field_matches_nothing_or_more() {
        assert!(match_keyword("CD", "CD%0d", 1).is_some());
        assert!(match_keyword("CD12", "CD%0d", 1).is_some());
        assert!(match_keyword("CDX", "CD%0d", 1).is_none());
    }

    #[test]
    fn fixed_width_field_matches_exact_count() {
        assert_eq!(match_keyword("CD001002", "CD%3d%3d", 2), Some(vec![1, 2]));
        assert!(match_keyword("CD1002", "CD%3d%3d", 2).is_none());
    }

    #[test]
    fn fits_type_accepts_hyphen_and_underscore() {
        assert!(match_keyword("OBSGEO-X", "OBSGEO%1f%1c", 0).is_some());
        assert!(match_keyword("A_1", "A%1f%1d", 1).is_some());
    }
}
