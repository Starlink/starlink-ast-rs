//! C references in rendered documentation.
//!
//! The documentation of the Rust API should describe the Rust API.
//! A C source file name (`permmap.c:2849`, `frame.h:646`) or an internal C
//! function (`astInitPermMap`) in a rendered page points a reader at code they
//! are not using, and line numbers in the C source change with every upstream
//! sync.

use crate::{code_lines, is_ident};
use std::collections::BTreeSet;
use std::path::Path;

/// Tags that style or link a run of words inside a line, such as the
/// highlighting rustdoc puts around part of a name.
/// Removing one leaves no space, so `<span>permmap</span>.c` reads as `permmap.c`.
const INLINE_TAGS: [&str; 8] = ["a", "b", "code", "em", "i", "span", "strong", "u"];

/// Whether `tag`, the text between `<` and `>`, opens or closes an inline tag.
fn is_inline_tag(tag: &str) -> bool {
    let name: String = tag
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    INLINE_TAGS.contains(&name.to_ascii_lowercase().as_str())
}

/// The visible text of `html`: tags removed (an inline tag leaves nothing, any
/// other leaves a space) and the five entities rustdoc emits in prose decoded.
fn visible_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut tag: Option<String> = None;
    for c in html.chars() {
        match (c, tag.as_mut()) {
            ('<', None) => tag = Some(String::new()),
            ('>', Some(t)) => {
                if !is_inline_tag(t) {
                    text.push(' ');
                }
                tag = None;
            }
            (_, None) => text.push(c),
            (_, Some(t)) => t.push(c),
        }
    }
    decode_entities(&text)
}

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// The `href` targets in `html`, entities decoded, in order of appearance.
fn href_targets(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("href=\"") {
        rest = &rest[at + 6..];
        let Some(end) = rest.find('"') else { break };
        out.push(decode_entities(&rest[..end]));
        rest = &rest[end..];
    }
    out
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
/// The `href` targets are scanned the same way after the visible text, so a
/// link to a C file (`https://example.org/src/permmap.c`) is found although
/// its text may not name it.
/// References are returned in order of appearance, the visible text first.
pub fn c_references(html: &str, public: &BTreeSet<String>) -> Vec<String> {
    let mut text = visible_text(html);
    for target in href_targets(html) {
        // A newline separates targets from the text and from each other, so a
        // reference cannot run across them.
        text.push('\n');
        text.push_str(&target);
    }
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

/// The names of the public modules declared in `src`, a crate root.
///
/// A declaration is found wherever `pub mod name` appears in code, so
/// `#[cfg(unix)] pub mod name;` on one line counts, and one in a comment or a
/// string does not.
pub fn public_modules(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for code in code_lines(src) {
        let mut rest = code.as_str();
        while let Some(at) = rest.find("pub mod ") {
            let before = rest[..at].chars().next_back();
            let after = &rest[at + "pub mod ".len()..];
            let name: String = after
                .trim_start()
                .chars()
                .take_while(|c| is_ident(*c))
                .collect();
            if !before.is_some_and(is_ident) && !name.is_empty() {
                out.push(name);
            }
            rest = after;
        }
    }
    out
}

/// The rendered documentation directories under the cargo target directory
/// `target`: `target/doc` and `target/<triple>/doc`, where they exist.
fn doc_dirs(target: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![target.join("doc")];
    if let Ok(entries) = std::fs::read_dir(target) {
        dirs.extend(entries.flatten().map(|e| e.path().join("doc")));
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// Every `.html` and `.js` file under `dir`, except the content-hashed
/// `static.files` that rustdoc shares between builds.
fn page_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "static.files") {
                page_files(&p, out)?;
            }
        } else if p.extension().is_some_and(|x| x == "html" || x == "js") {
            out.push(p);
        }
    }
    Ok(())
}

/// Delete the rendered pages and scripts under the cargo target directory
/// `target` that a documentation build started at `since` did not write.
///
/// An incremental `cargo doc` leaves pages and `type.impl/*.js` files that no
/// source produces any more, which a scan would read as if they were current.
/// When the build wrote nothing (cargo found the docs fresh) nothing is stale
/// and nothing is deleted, so an incremental build stays incremental.
/// Returns the number of files deleted.
pub fn clear_stale_pages(target: &Path, since: std::time::SystemTime) -> std::io::Result<usize> {
    let mut files = Vec::new();
    for d in doc_dirs(target) {
        page_files(&d, &mut files)?;
    }
    let mut stale = Vec::new();
    let mut rewritten = false;
    for f in files {
        if std::fs::metadata(&f)?.modified()? >= since {
            rewritten = true;
        } else {
            stale.push(f);
        }
    }
    if !rewritten {
        return Ok(0);
    }
    for f in &stale {
        std::fs::remove_file(f)?;
    }
    Ok(stale.len())
}

#[cfg(test)]
mod tests {
    use super::{c_references, clear_stale_pages, public_modules};
    use std::collections::BTreeSet;

    #[test]
    fn public_modules_include_a_one_line_cfg_declaration() {
        let src = "pub mod a;\n#[cfg(unix)] pub mod b;\npub(crate) mod c;\nmod d;\n\
                   // pub mod e;\nlet _ = \"pub mod f;\";\n    pub mod g {\n}\n/* pub mod h; */\n";
        assert_eq!(public_modules(src), ["a", "b", "g"]);
    }

    #[test]
    fn only_pages_the_build_did_not_write_are_cleared() {
        use std::time::{Duration, SystemTime};
        let root = std::env::temp_dir().join(format!("audit-tools-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let since = SystemTime::now();
        let old = since - Duration::from_secs(3600);
        let put = |rel: &str, mtime: SystemTime| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(mtime)
                .unwrap();
        };
        put(
            "doc/starlink_ast/fresh.html",
            since + Duration::from_secs(5),
        );
        put("doc/starlink_ast/stale.html", old);
        put("doc/type.impl/starlink_ast/stale.js", old);
        put("aarch64/doc/starlink_ast/stale.html", old);
        put("doc/static.files/shared-1.js", old);
        put("doc/starlink_ast/notes.txt", old);
        put("debug/deps/lib.rlib", old);
        assert_eq!(clear_stale_pages(&root, since).unwrap(), 3);
        assert!(root.join("doc/starlink_ast/fresh.html").exists());
        assert!(!root.join("doc/starlink_ast/stale.html").exists());
        assert!(!root.join("doc/type.impl/starlink_ast/stale.js").exists());
        assert!(!root.join("aarch64/doc/starlink_ast/stale.html").exists());
        for kept in [
            "doc/static.files/shared-1.js",
            "doc/starlink_ast/notes.txt",
            "debug/deps/lib.rlib",
        ] {
            assert!(root.join(kept).exists(), "{kept}");
        }
        // A build that wrote nothing left nothing stale: keep everything.
        let later = SystemTime::now() + Duration::from_secs(3600);
        assert_eq!(clear_stale_pages(&root, later).unwrap(), 0);
        assert!(root.join("doc/starlink_ast/fresh.html").exists());
        // A target that does not exist yet is fine.
        assert_eq!(clear_stale_pages(&root.join("absent"), since).unwrap(), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }

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
        let html = "<a href=\"permmap.html\" class=\"astx\">starlink_ast</a><p>AstError</p>";
        assert!(c_references(html, &public()).is_empty());
    }

    #[test]
    fn an_inline_tag_inside_a_file_name_does_not_split_it() {
        let html = "<p>See <span class=\"x\">permmap</span>.c:12 and <code>frame</code>.h.</p>";
        assert_eq!(c_references(html, &public()), ["permmap.c:12", "frame.h"]);
        // Block tags still separate words.
        let html = "<p>permmap</p><p>.c</p>";
        assert!(c_references(html, &public()).is_empty());
    }

    #[test]
    fn a_link_to_a_c_file_is_flagged_by_its_target() {
        let html = "<a href=\"https://example.org/src/permmap.c#L12\">source</a>\
                    <a href=\"https://example.org/frame.h:646\">header</a>";
        assert_eq!(c_references(html, &public()), ["permmap.c", "frame.h:646"]);
    }

    #[test]
    fn a_link_to_an_internal_c_function_is_flagged() {
        let html = "<a href=\"https://example.org/?q=astInitPermMap&amp;x=1\">x</a>";
        assert_eq!(c_references(html, &public()), ["astInitPermMap"]);
    }

    #[test]
    fn ordinary_links_are_not_flagged() {
        let html = "<a href=\"../starlink_ast/struct.PermMap.html#method.new\">a</a>\
                    <a href=\"https://www.starlink.ac.uk/docs/sun211.htx/sun211.html\">b</a>\
                    <a href=\"https://docs.rs/ndarray/latest/ndarray/index.html\">c</a>\
                    <a href=\"#astPermMap\">d</a>";
        assert!(c_references(html, &public()).is_empty());
    }
}
