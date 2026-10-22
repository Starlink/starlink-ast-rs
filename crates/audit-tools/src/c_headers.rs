//! The public documentation headers of the C library.
//!
//! Each class, function and attribute the C library documents has a comment
//! header in its source, delimited by marker lines. The same header carries
//! the C and the Fortran edition of the text, the editions distinguished by a
//! leading `c` or `f` on a line. This module extracts the C edition of every
//! header, with a normalized name, so documentation audits can enumerate what
//! the C library documents.

use std::fmt;

/// What a header documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderKind {
    /// A class (`*class++`).
    Class,
    /// A function or method (`*++` or `c++`).
    Function,
    /// An attribute (`*att++`).
    Attribute,
}

impl fmt::Display for HeaderKind {
    /// The spelling the TSV and the audit ledger use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HeaderKind::Class => "class",
            HeaderKind::Function => "function",
            HeaderKind::Attribute => "attribute",
        })
    }
}

impl HeaderKind {
    fn parse(s: &str) -> Option<HeaderKind> {
        match s {
            "class" => Some(HeaderKind::Class),
            "function" => Some(HeaderKind::Function),
            "attribute" => Some(HeaderKind::Attribute),
            _ => None,
        }
    }
}

/// One documentation header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CHeader {
    /// What the header documents.
    pub kind: HeaderKind,
    /// The documented name with any `(…)` suffix and `/…` alternatives
    /// removed; a `<X>` type placeholder is kept.
    pub name: String,
    /// The name with every `<…>` removed.
    pub alias: String,
    /// Source file the header is in, such as `permmap.c`.
    pub file: String,
    /// The C edition of the header body, comment markers removed and relative
    /// indentation kept.
    pub text: String,
}

/// Columns of leading space that follow a comment marker before the text starts.
const TEXT_COLUMN: usize = 2;

/// The kind and closing marker for a line that opens a header, if it is one.
fn opener(line: &str) -> Option<(Option<HeaderKind>, &'static str)> {
    match line {
        "*class++" => Some((Some(HeaderKind::Class), "*class--")),
        "*++" => Some((Some(HeaderKind::Function), "*--")),
        "*att++" => Some((Some(HeaderKind::Attribute), "*att--")),
        "c++" => Some((Some(HeaderKind::Function), "c--")),
        "f++" => Some((None, "f--")),
        _ => None,
    }
}

/// The text of a kept line with its marker removed, or `None` for a line of
/// the Fortran edition or a non-comment line.
fn kept(line: &str) -> Option<String> {
    // A `c` marker is a one-column marker like `*`, and counts only when the
    // text after it is separated by a space or absent.
    let rest = if let Some(r) = line.strip_prefix('*') {
        r
    } else if let Some(r) = line.strip_prefix('c')
        && (r.is_empty() || r.starts_with(' '))
    {
        r
    } else {
        return None;
    };
    let spaces = rest.len() - rest.trim_start_matches(' ').len();
    Some(rest[spaces.min(TEXT_COLUMN)..].to_string())
}

/// Cut `name` at the first `/`, then at the first `(`, and trim it.
fn normalize(name: &str) -> String {
    let name = name.split('/').next().unwrap_or("");
    name.split('(').next().unwrap_or("").trim().to_string()
}

/// `name` with every `<…>` removed.
fn alias_of(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The first non-empty line after the `Name:` line.
fn header_name(lines: &[String]) -> Option<&str> {
    let at = lines.iter().position(|l| l.trim() == "Name:")?;
    lines[at + 1..]
        .iter()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
}

/// Every header of the C edition in `src`, in source order.
///
/// `file` is recorded on each header.
///
/// A header of the C edition with no `Name:` entry, and a header still open at
/// the end of `src`, are errors rather than omissions: the error lists each one
/// as `file:line: …`, with the line of the header's opening marker.
pub fn c_headers(src: &str, file: &str) -> Result<Vec<CHeader>, Vec<String>> {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut open: Option<(Option<HeaderKind>, &str, Vec<String>, usize)> = None;
    for (index, line) in src.lines().enumerate() {
        let line = line.trim_end();
        if let Some((kind, close, body, opened)) = open.as_mut() {
            if line == *close {
                if let Some(kind) = *kind {
                    if let Some(raw) = header_name(body) {
                        let name = normalize(raw);
                        out.push(CHeader {
                            kind,
                            alias: alias_of(&name),
                            name,
                            file: file.to_string(),
                            text: body.join("\n"),
                        });
                    } else {
                        problems.push(format!("{file}:{opened}: header has no Name: entry",));
                    }
                }
                open = None;
            } else if kind.is_some()
                && let Some(text) = kept(line)
            {
                body.push(text);
            } else if kind.is_some() && line.is_empty() {
                body.push(String::new());
            }
        } else if let Some((kind, close)) = opener(line) {
            open = Some((kind, close, Vec::new(), index + 1));
        }
    }
    if let Some((_, close, _, opened)) = open {
        problems.push(format!(
            "{file}:{opened}: header is not closed by {close} before the end of the file"
        ));
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// Sections of a header that record its provenance rather than document the
/// item; [`text_digest`] leaves them out.
const UNDIGESTED_SECTIONS: [&str; 4] = ["Authors:", "Copyright:", "History:", "Licence:"];

/// A stable digest of what a header body documents: FNV-1a 64 of its UTF-8
/// bytes, as 16 lower-case hex digits.
///
/// A section heading is a line with no leading space that ends in `:`; the
/// sections named in [`UNDIGESTED_SECTIONS`] are skipped, so a new `History:`
/// entry does not change the digest but any change to the documentation does.
/// It identifies the text; it is not a cryptographic hash.
pub fn text_digest(text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut skipping = false;
    for line in text.split('\n') {
        if !line.starts_with(' ') && line.ends_with(':') {
            skipping = UNDIGESTED_SECTIONS.contains(&line);
        }
        if skipping {
            continue;
        }
        for b in line.bytes().chain(std::iter::once(b'\n')) {
            hash = (hash ^ u64::from(b)).wrapping_mul(PRIME);
        }
    }
    format!("{hash:016x}")
}

/// One row of [`headers_tsv`], as read back by [`parse_headers_tsv`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderRow {
    /// What the header documents.
    pub kind: HeaderKind,
    /// The normalized name, as [`CHeader::name`].
    pub name: String,
    /// The name with every `<…>` removed, as [`CHeader::alias`].
    pub alias: String,
    /// Source file the header is in.
    pub file: String,
    /// [`text_digest`] of the header body.
    pub digest: String,
}

/// A TSV of `headers`, one row per header, after a
/// `kind\tname\talias\tfile\tdigest` line. Rows are sorted by file, then by
/// position. The digest column is [`text_digest`] of the body, so a header
/// whose text changes shows as a changed row.
pub fn headers_tsv(headers: &[CHeader]) -> String {
    let mut order: Vec<usize> = (0..headers.len()).collect();
    order.sort_by(|&a, &b| headers[a].file.cmp(&headers[b].file).then(a.cmp(&b)));
    let mut out = String::from("kind\tname\talias\tfile\tdigest\n");
    for i in order {
        let h = &headers[i];
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            h.kind,
            h.name,
            h.alias,
            h.file,
            text_digest(&h.text)
        ));
    }
    out
}

/// Parse the output of [`headers_tsv`] into rows.
///
/// The header line and any row that does not have five fields with a known
/// kind are skipped.
pub fn parse_headers_tsv(text: &str) -> Vec<HeaderRow> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let kind = HeaderKind::parse(f.next()?)?;
            let name = f.next()?.to_string();
            let alias = f.next()?.to_string();
            let file = f.next()?.to_string();
            let digest = f.next()?.to_string();
            f.next().is_none().then_some(HeaderRow {
                kind,
                name,
                alias,
                file,
                digest,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
/*
*class++
*  Name:
*     PermMap

*  Purpose:
*     Coordinate permutation Mapping.
*class--
*/
/*
*++
*  Name:
c     astPermMap
f     AST_PERMMAP

*  Purpose:
*     Create a PermMap.
f  Fortran only line.
c  C only line.
*--
*/
/*
*att++
*  Name:
*     Digits/Digits(axis)
*att--
*/
/*
*att++
*  Name:
*     Label(axis)
*att--
*/
/*
*++
*  Name:
c     astMapGet0<X>
f     AST_MAPGET0<X>
*--
*/
/*
c++
*  Name:
*     astToString
c--
*/
/*
f++
*  Name:
*     AST_FORTRAN_ONLY
f--
*/
";

    #[test]
    fn every_c_edition_header_is_found() {
        let h = c_headers(SRC, "x.c").unwrap();
        let names: Vec<_> = h.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "PermMap",
                "astPermMap",
                "Digits",
                "Label",
                "astMapGet0<X>",
                "astToString"
            ]
        );
        assert_eq!(h[0].kind, HeaderKind::Class);
        assert_eq!(h[1].kind, HeaderKind::Function);
        assert_eq!(h[2].kind, HeaderKind::Attribute);
        assert_eq!(h[5].kind, HeaderKind::Function);
    }

    #[test]
    fn names_are_normalized() {
        let h = c_headers(SRC, "x.c").unwrap();
        assert_eq!(h[4].alias, "astMapGet0");
        assert_eq!(h[2].alias, "Digits");
    }

    #[test]
    fn the_text_is_the_c_edition() {
        let h = c_headers(SRC, "x.c").unwrap();
        assert!(h[1].text.contains("C only line."));
        assert!(!h[1].text.contains("Fortran only line."));
        assert!(!h[1].text.contains("AST_PERMMAP"));
    }

    #[test]
    fn c_only_lines_align_with_starred_lines() {
        let h = c_headers(SRC, "x.c").unwrap();
        assert!(h[1].text.contains("\nC only line."), "{:?}", h[1].text);
        assert!(
            h[1].text.starts_with("Name:\n   astPermMap\n"),
            "{:?}",
            h[1].text
        );
    }

    #[test]
    fn a_header_with_no_name_is_an_error() {
        let src = "*\n*++\n*  Purpose:\n*     Nothing.\n*--\n";
        let e = c_headers(src, "x.c").unwrap_err();
        assert_eq!(e.len(), 1);
        assert!(
            e[0].starts_with("x.c:2:") && e[0].contains("Name:"),
            "{e:?}"
        );
    }

    #[test]
    fn a_header_open_at_the_end_of_the_file_is_an_error() {
        let src = "*\n*class++\n*  Name:\n*     Box\n";
        let e = c_headers(src, "x.c").unwrap_err();
        assert_eq!(e.len(), 1);
        assert!(
            e[0].starts_with("x.c:2:") && e[0].contains("*class--"),
            "{e:?}"
        );
        let src = "f++\n*  Name:\n*     AST_BOX\n";
        assert!(c_headers(src, "x.c").is_err());
    }

    #[test]
    fn a_fortran_header_with_no_name_is_not_an_error() {
        let src = "f++\n*  Purpose:\n*     x\nf--\n";
        assert_eq!(c_headers(src, "x.c").unwrap(), []);
    }

    #[test]
    fn indentation_is_relative() {
        let h = c_headers(SRC, "x.c").unwrap();
        assert!(h[0].text.starts_with("Name:\n   PermMap\n"));
    }

    #[test]
    fn key_includes_the_file() {
        let a = c_headers(SRC, "a.c").unwrap();
        let b = c_headers(SRC, "b.c").unwrap();
        let tsv = headers_tsv(&[a.clone(), b].concat());
        let rows = parse_headers_tsv(&tsv);
        assert_eq!(rows.len(), 12);
        let permmaps: Vec<_> = rows.iter().filter(|r| r.name == "PermMap").collect();
        assert_eq!(permmaps.len(), 2);
        assert_eq!(permmaps[0].file, "a.c");
        assert_eq!(permmaps[1].file, "b.c");
        assert_eq!(permmaps[0].digest, text_digest(&a[0].text));
    }

    #[test]
    fn the_digest_follows_the_body() {
        let before = headers_tsv(&c_headers(SRC, "x.c").unwrap());
        assert_eq!(before, headers_tsv(&c_headers(SRC, "x.c").unwrap()));
        let edited = SRC.replace("Create a PermMap.", "Create a new PermMap.");
        let after = headers_tsv(&c_headers(&edited, "x.c").unwrap());
        let (old, new) = (parse_headers_tsv(&before), parse_headers_tsv(&after));
        assert_eq!(old.len(), new.len());
        let changed: Vec<_> = old
            .iter()
            .zip(&new)
            .filter(|(o, n)| o != n)
            .map(|(o, _)| o.name.as_str())
            .collect();
        assert_eq!(changed, ["astPermMap"]);
    }

    #[test]
    fn text_digest_is_fnv1a_64_of_the_lines() {
        // FNV-1a 64 of "a\n".
        assert_eq!(text_digest("a"), "089bdc07b544e7b2");
    }

    #[test]
    fn provenance_sections_are_not_digested() {
        let doc = "Name:\n   Box\n\nPurpose:\n   A box.\n";
        let history = format!("{doc}History:\n   1-JAN-2000 (DSB):\n      Original.\n");
        let more = format!("{history}   2-JAN-2000 (DSB):\n      Fixed.\n");
        assert_eq!(text_digest(&history), text_digest(&more));
        let copyright = |year: &str| format!("{doc}Copyright:\n   Copyright (C) {year} X\n");
        assert_eq!(
            text_digest(&copyright("2000")),
            text_digest(&copyright("2026"))
        );
        let after = format!("{history}Notes:\n   - A note.\n");
        assert_ne!(text_digest(&after), text_digest(doc));
    }
}
