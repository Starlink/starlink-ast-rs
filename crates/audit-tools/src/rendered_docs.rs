//! C references in rendered documentation.
//!
//! The documentation of the Rust API should describe the Rust API.
//! A C source file name (`permmap.c:2849`, `frame.h:646`) or an internal C
//! function (`astInitPermMap`) in a rendered page points a reader at code they
//! are not using, and line numbers in the C source change with every upstream
//! sync.

use std::collections::BTreeSet;

/// The visible text of `html`: tags removed (each leaves a space) and the
/// five entities rustdoc emits in prose decoded.
fn visible_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Length of `digits[-digits]` at the start of `s`, or zero when `s` does not
/// start with a digit.
fn line_range_len(s: &[u8]) -> usize {
    let digits = s.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return 0;
    }
    if s.get(digits) == Some(&b'-') {
        let more = s[digits + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if more > 0 {
            return digits + 1 + more;
        }
    }
    digits
}

/// The C references in one rendered page's visible text: C source and header
/// file names (`permmap.c`, `frame.h`, with or without `:line`) and
/// `ast<Name>` words not in `public`.
///
/// A file name is a lowercase word followed by `.c` or `.h`, where the word is
/// not itself preceded by a word character or `.` and the extension is not
/// followed by a word character.
/// A path such as `reference/starlink-ast/src/table.c` is flagged by its file
/// name.
/// References are returned in order of appearance.
pub fn c_references(html: &str, public: &BTreeSet<String>) -> Vec<String> {
    let text = visible_text(html);
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !is_word_char(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_word_char(bytes[i]) {
            i += 1;
        }
        let word = &text[start..i];
        let wb = word.as_bytes();
        if wb.len() > 3 && word.starts_with("ast") && wb[3].is_ascii_uppercase() {
            if !public.contains(word) {
                out.push(word.to_string());
            }
            continue;
        }
        let rest = &bytes[i..];
        let after_dot_c = (rest.starts_with(b".c") || rest.starts_with(b".h"))
            && !rest.get(2).is_some_and(|b| is_word_char(*b));
        let lower = wb[0].is_ascii_lowercase()
            && wb
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
        let prev_ok = start == 0 || bytes[start - 1] != b'.';
        // `self.h` is a Rust field access, not a header.
        if after_dot_c && lower && prev_ok && word != "self" {
            let mut end = i + 2;
            if bytes.get(end) == Some(&b':') {
                let n = line_range_len(&bytes[end + 1..]);
                if n > 0 {
                    end += 1 + n;
                }
            }
            out.push(text[start..end].to_string());
            i = end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::c_references;
    use std::collections::BTreeSet;

    fn public() -> BTreeSet<String> {
        ["astPermMap".to_string()].into()
    }

    #[test]
    fn a_c_file_name_anywhere_in_text_is_flagged() {
        let html = "<p>As in permmap.c:2849 and <a href=\"x\">frame.c</a>.</p>";
        assert_eq!(c_references(html, &public()), ["permmap.c:2849", "frame.c"]);
    }

    #[test]
    fn a_c_header_name_is_flagged_with_its_line() {
        let html = "<p>See frame.h:646 and permmap.h.</p>";
        assert_eq!(c_references(html, &public()), ["frame.h:646", "permmap.h"]);
    }

    #[test]
    fn rust_text_that_looks_like_a_header_is_not_flagged() {
        let html = "<a href=\"x.html\">page</a><p>foo.hash and self.h and self.c</p>";
        assert!(c_references(html, &public()).is_empty());
    }

    #[test]
    fn internal_c_names_are_flagged_and_public_ones_are_not() {
        let html = "<p>Like astPermMap; built by astInitPermMap.</p>";
        assert_eq!(c_references(html, &public()), ["astInitPermMap"]);
    }

    #[test]
    fn a_c_file_path_is_flagged_by_its_file_name() {
        let html = "<p>See reference/starlink-ast/src/table.c and vendor/wcslib/proj.c:12.</p>";
        assert_eq!(c_references(html, &public()), ["table.c", "proj.c:12"]);
    }

    #[test]
    fn markup_and_rust_names_are_not_text() {
        let html = "<a href=\"permmap.c.html\" class=\"astx\">ast_core</a><p>AstError</p>";
        assert!(c_references(html, &public()).is_empty());
    }
}
