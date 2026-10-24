//! Source citations for the `MapMerge` / `Simplify` audit checklists.
//!
//! A checklist row cites a source line, and a line number rots as the file
//! around it changes. Recording the enclosing item's name beside the number
//! makes the rot detectable: the number and the name have to still agree.
//! This crate holds the one derivation of that name, so the tool that writes
//! the citations and the test that checks them cannot disagree about it.

pub mod c_docs_ledger;
pub mod c_headers;
mod c_steps;
pub mod doc_aliases;
pub mod guide_authors;
mod rendered_docs;
pub use c_docs_ledger::{LedgerRow, owner_and_name, parse_ledger};
pub use c_headers::{
    CHeader, HeaderKind, HeaderRow, c_headers, c_headers_of_files, headers_tsv, parse_headers_tsv,
    text_digest, write_report,
};
pub use c_steps::c_step_comments;
pub use doc_aliases::{alias_satisfied_by_name, doc_aliases, duplicate_aliases, top_level_types};
pub use rendered_docs::{c_references, clear_stale_pages, public_modules};

/// A parsed `file:line[-line] [(fn name)]` citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// File name as written, without a directory.
    pub file: String,
    /// First line of the cited range, 1-based.
    pub start: usize,
    /// Last line of the cited range, when the citation names one.
    pub end: Option<usize>,
    /// The item name the citation claims, when it carries one.
    pub item: Option<String>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Parse the first citation in `text`.
pub fn parse_citation(text: &str) -> Option<Citation> {
    citations(text).into_iter().next().map(|(c, _)| c)
}

/// Every citation in `text`, each with the byte range it occupies.
///
/// A row may cite more than one site, and a citation that is not found is a
/// citation nothing checks, so all of them are returned rather than the first.
///
/// A citation may name further lines in the same file as a comma-separated tail,
/// as in `simplify.rs:1470 (fn region_map_merge),1482`. Each is returned as a
/// citation of its own so a number written that way is checked rather than
/// skipped. It carries the file of the citation it follows but not its item
/// name: the tail names further lines of the same file, which are often in a
/// different function, so a tail needs its own `(fn …)` to be checkable.
pub fn citations(text: &str) -> Vec<(Citation, std::ops::Range<usize>)> {
    let mut out: Vec<(Citation, std::ops::Range<usize>)> = Vec::new();
    for (colon, _) in text.match_indices(':') {
        if out.last().is_some_and(|(_, range)| colon < range.end) {
            continue;
        }
        if let Some((cit, range)) = parse_citation_at(text, colon) {
            let tail = continuations(text, range.end, &cit);
            out.push((cit, range));
            out.extend(tail);
        }
    }
    out
}

/// The `,line` and `,line-line` tail that follows a citation ending at `end`.
///
/// Each inherits the leading citation's file, which is what the shorthand
/// means, and nothing else.
fn continuations(
    text: &str,
    end: usize,
    lead: &Citation,
) -> Vec<(Citation, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut at = end;
    while let Some(rest) = text.get(at..).and_then(|t| t.strip_prefix(',')) {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let Ok(start) = digits.parse::<usize>() else {
            break;
        };
        let mut used = 1 + digits.len();
        let mut range_end = None;
        if let Some(tail) = text[at + used..].strip_prefix('-') {
            let d: String = tail.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = d.parse::<usize>() {
                range_end = Some(n);
                used += 1 + d.len();
            }
        }
        let item = text[at + used..]
            .strip_prefix(" (fn ")
            .and_then(|t| t.split_once(')'))
            .map(|(name, _)| name.trim().to_string());
        let span = match &item {
            Some(name) => used + " (fn ".len() + name.len() + 1,
            None => used,
        };
        out.push((
            Citation {
                file: lead.file.clone(),
                start,
                end: range_end,
                item,
            },
            at..at + span,
        ));
        used = span;
        at += used;
    }
    out
}

/// Parse the citation whose `file:line` colon is at `colon`.
fn parse_citation_at(text: &str, colon: usize) -> Option<(Citation, std::ops::Range<usize>)> {
    let stem: String = text[..colon]
        .chars()
        .rev()
        .take_while(|c| is_ident(*c) || *c == '.' || *c == '/')
        .collect();
    let file: String = stem.chars().rev().collect();
    let file = file.trim_start_matches('/').to_string();
    if !(file.ends_with(".rs") || file.ends_with(".c") || file.ends_with(".h")) {
        return None;
    }
    let begin = colon - file.len();
    let after = &text[colon + 1..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    let start = digits.parse::<usize>().ok()?;
    let mut used = digits.len();
    let mut end = None;
    if let Some(tail) = after[used..].strip_prefix('-') {
        let d: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = d.parse::<usize>() {
            end = Some(n);
            used += 1 + d.len();
        }
    }
    let item = after[used..]
        .strip_prefix(" (fn ")
        .and_then(|t| t.split_once(')'))
        .map(|(name, _)| name.trim().to_string());
    let last = match &item {
        Some(name) => used + " (fn ".len() + name.len() + 1,
        None => used,
    };
    Some((
        Citation {
            file,
            start,
            end,
            item,
        },
        begin..colon + 1 + last,
    ))
}

/// The string literal a line ends inside, if any, which the next line
/// continues.
#[derive(Clone, Copy, Default)]
enum OpenLiteral {
    #[default]
    None,
    /// A `"…"` or `b"…"` string, including one continued with a trailing `\`.
    Str,
    /// A raw string closed by `"` and this many `#`.
    Raw(usize),
    /// A `/* … */` comment, which nests, at this depth.
    Block(usize),
}

/// Every line of `src` with comments and string and char literal bodies
/// stripped, so their braces do not count as block structure.
///
/// A string literal or a block comment may span lines.
/// The state of each line carries into the next.
fn code_lines(src: &str) -> Vec<String> {
    let mut open = OpenLiteral::None;
    src.lines().map(|l| code_only(l, &mut open)).collect()
}

/// One line of [`code_lines`]: `open` is the literal the previous line ended
/// inside, and is updated to the one this line ends inside.
fn code_only(line: &str, open: &mut OpenLiteral) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match *open {
            OpenLiteral::Str => {
                match chars[i] {
                    '\\' => i += 2,
                    '"' => {
                        *open = OpenLiteral::None;
                        i += 1;
                    }
                    _ => i += 1,
                }
                continue;
            }
            OpenLiteral::Raw(hashes) => {
                if chars[i] == '"'
                    && chars[i + 1..]
                        .iter()
                        .take(hashes)
                        .filter(|c| **c == '#')
                        .count()
                        == hashes
                {
                    *open = OpenLiteral::None;
                    i += 1 + hashes;
                } else {
                    i += 1;
                }
                continue;
            }
            OpenLiteral::Block(depth) => {
                match (chars[i], chars.get(i + 1)) {
                    ('/', Some('*')) => {
                        *open = OpenLiteral::Block(depth + 1);
                        i += 2;
                    }
                    ('*', Some('/')) => {
                        *open = match depth {
                            1 => OpenLiteral::None,
                            _ => OpenLiteral::Block(depth - 1),
                        };
                        i += 2;
                    }
                    _ => i += 1,
                }
                continue;
            }
            OpenLiteral::None => {}
        }
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => break,
            '/' if chars.get(i + 1) == Some(&'*') => {
                *open = OpenLiteral::Block(1);
                i += 2;
            }
            '"' => {
                *open = OpenLiteral::Str;
                i += 1;
            }
            'r' if raw_string_hashes(&chars, i).is_some() => {
                let hashes = raw_string_hashes(&chars, i).expect("just tested");
                *open = OpenLiteral::Raw(hashes);
                i += 2 + hashes;
            }
            // A char literal: an escape such as `'\''` or `'\u{7b}'`, or one
            // character between quotes. Anything else is a lifetime.
            '\'' if chars.get(i + 1) == Some(&'\\') => {
                i = chars
                    .get(i + 3..)
                    .unwrap_or_default()
                    .iter()
                    .position(|c| *c == '\'')
                    .map_or(chars.len(), |at| i + 3 + at + 1);
            }
            '\'' if chars.get(i + 2) == Some(&'\'') => i += 3,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// The number of `#` in the raw string literal opening at `chars[at]`, an `r`,
/// or `None` when `r` there starts no raw string.
fn raw_string_hashes(chars: &[char], at: usize) -> Option<usize> {
    // `r` must begin a token, or follow the `b` or `c` that begins one in
    // `br"…"` and `cr"…"`.
    let starts_token = |k: usize| k == 0 || !is_ident(chars[k - 1]);
    if !(starts_token(at) || (matches!(chars[at - 1], 'b' | 'c') && starts_token(at - 1))) {
        return None;
    }
    let hashes = chars[at + 1..].iter().take_while(|c| **c == '#').count();
    (chars.get(at + 1 + hashes) == Some(&'"')).then_some(hashes)
}

fn rust_fn_name(code: &str) -> Option<String> {
    let at = code
        .find("fn ")
        .filter(|at| *at == 0 || !code[..*at].chars().next_back().is_some_and(is_ident))?;
    let name: String = code[at + 3..]
        .chars()
        .take_while(|c| is_ident(*c))
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The first line of the attribute and doc-comment block attached to the item
/// declared at `i` (0-based), as a 1-based line number.
///
/// A citation to an item's documentation designates the item, so the block
/// counts as part of it.
fn attached_start(raw: &[&str], i: usize) -> usize {
    let mut first = i;
    while first > 0 {
        let prev = raw[first - 1].trim_start();
        if prev.starts_with("///") || prev.starts_with("//") || prev.starts_with("#[") {
            first -= 1;
        } else {
            break;
        }
    }
    first + 1
}

/// Every `fn` in `src`, as `(name, first line, last line)`.
///
/// The range runs from the start of the item's doc-comment block to the line
/// closing its body, so a citation to the signature or the documentation
/// designates the same item a citation to the body does. A declaration with no
/// body, as in a trait, ends at its semicolon. A semicolon reached inside a
/// bracket pair belongs to a type rather than to the signature — the length of
/// an array parameter, or of an array inside the return type — and does not end
/// it.
pub fn rust_items(src: &str) -> Vec<(String, usize, usize)> {
    let raw: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    // Items whose body has opened, as `(name, first line, body brace depth)`.
    let mut open: Vec<(String, usize, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut pending: Option<(String, usize)> = None;
    for (i, code) in code_lines(src).iter().enumerate() {
        if let Some(name) = rust_fn_name(code) {
            pending = Some((name, attached_start(&raw, i)));
        }
        for c in code.chars() {
            match c {
                '(' => parens += 1,
                ')' => parens = parens.saturating_sub(1),
                '[' => brackets += 1,
                ']' => brackets = brackets.saturating_sub(1),
                '{' => {
                    depth += 1;
                    if let Some((name, first)) = pending.take() {
                        open.push((name, first, depth));
                    }
                }
                '}' => {
                    if open.last().is_some_and(|(_, _, d)| *d == depth) {
                        let (name, first, _) = open.pop().expect("just tested");
                        out.push((name, first, i + 1));
                    }
                    depth = depth.saturating_sub(1);
                }
                // A signature with no body, as in a trait declaration.
                ';' if parens == 0 && brackets == 0 => {
                    if let Some((name, first)) = pending.take() {
                        out.push((name, first, i + 1));
                    }
                }
                _ => {}
            }
        }
    }
    // A body left unclosed at end of file still spans what was seen of it.
    for (name, first, _) in open {
        out.push((name, first, raw.len()));
    }
    out
}

/// The `impl` or `trait` header enclosing `line` (1-based): `"Trait for Type"`
/// for a trait implementation, `"Type"` for an inherent one, and `"trait Name"`
/// for a trait definition.
///
/// This is what distinguishes a step that landed on a trait method — the port's
/// equivalent of a C vtab slot, declared either in the trait or in a per-class
/// implementation of it — from one that landed on an inherent helper. Ask
/// [`declares_trait_methods`] which of the three a header is.
pub fn enclosing_rust_block(src: &str, line: usize) -> Option<String> {
    line.checked_sub(1)
        .and_then(|i| block_headers(src).into_iter().nth(i))
        .flatten()
}

/// The header [`enclosing_rust_block`] gives for each line of `src`, in one
/// pass over the file.
fn block_headers(src: &str) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut pending: Option<String> = None;
    for code in code_lines(src) {
        if let Some(header) = block_header(&code) {
            pending = Some(header);
        }
        for c in code.chars() {
            match c {
                '{' => {
                    depth += 1;
                    if let Some(header) = pending.take() {
                        stack.push((header, depth));
                    }
                }
                '}' => {
                    if stack.last().is_some_and(|(_, d)| *d == depth) {
                        stack.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        out.push(stack.last().map(|(header, _)| header.clone()));
    }
    out
}

/// Whether a header from [`enclosing_rust_block`] declares trait methods.
///
/// A trait implementation (`impl Trait for Type`) and a trait definition
/// (`trait Trait`) both declare methods a generic driver dispatches to per
/// class; an inherent `impl Type` does not.
pub fn declares_trait_methods(header: &str) -> bool {
    header.contains(" for ") || header.starts_with("trait ")
}

/// The subject of an `impl` or `trait` line, with generics and `where` clauses
/// removed. A trait definition keeps its keyword, which is what tells it apart
/// from an inherent `impl` on a type of the same name.
fn block_header(code: &str) -> Option<String> {
    let trimmed = code.trim_start();
    let trimmed = trimmed
        .strip_prefix("pub(crate) ")
        .or_else(|| trimmed.strip_prefix("pub "))
        .unwrap_or(trimmed);
    let (keyword, rest) = match trimmed.strip_prefix("impl") {
        Some(rest) => ("", rest),
        None => ("trait ", trimmed.strip_prefix("trait")?),
    };
    if rest.starts_with(|c: char| is_ident(c)) {
        return None;
    }
    let body = rest
        .split(" where ")
        .next()
        .unwrap_or(rest)
        .split('{')
        .next()
        .unwrap_or(rest);
    let mut out = String::new();
    let mut generic = 0i32;
    for c in body.chars() {
        match c {
            '<' => generic += 1,
            '>' => generic -= 1,
            _ if generic == 0 => out.push(c),
            _ => {}
        }
    }
    let header = out.split_whitespace().collect::<Vec<_>>().join(" ");
    (!header.is_empty()).then(|| format!("{keyword}{header}"))
}

/// The name of the `fn` whose body, signature or documentation contains `line`
/// (1-based). The innermost enclosing item wins.
pub fn enclosing_rust_item(src: &str, line: usize) -> Option<String> {
    rust_items(src)
        .into_iter()
        .filter(|(_, first, last)| *first <= line && line <= *last)
        .min_by_key(|(_, first, last)| last - first)
        .map(|(name, _, _)| name)
}

/// The name of the C function whose body contains `line` (1-based).
pub fn enclosing_c_function(src: &str, line: usize) -> Option<String> {
    c_functions(src)
        .into_iter()
        .find(|(_, start, end)| *start <= line && line <= *end)
        .map(|(name, _, _)| name)
}

/// Every function definition in `src`, as `(name, first line, last line)`.
///
/// AST's house style puts a definition's declarator at column 0 and its closing
/// brace alone at column 0, so a definition is delimited without parsing C.
pub fn c_functions(src: &str) -> Vec<(String, usize, usize)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        match c_definition_name(&lines, i) {
            Some(CDefinition::Function(name)) => {
                let mut j = i + 1;
                while j < lines.len() && lines[j] != "}" {
                    j += 1;
                }
                out.push((name, i + 1, j + 1));
                i = j + 1;
            }
            Some(CDefinition::Macro(name, span)) => {
                out.push((name, i + 1, i + span));
                i += span;
            }
            None => i += 1,
        }
    }
    out
}

/// What a column-0 declarator turns out to be.
enum CDefinition {
    /// A function definition, whose body follows.
    Function(String),
    /// A macro invocation at file scope, such as the `astMAKE_SET` family that
    /// generates AST's attribute accessors, with the number of lines it spans.
    /// It is the construct a citation to those lines designates, so it is an
    /// item in its own right.
    Macro(String, usize),
}

/// The name of the function defined at `lines[i]`, if a definition starts there.
///
/// What separates a definition from the prototype of the same name earlier in
/// the file is where the declarator ends: a definition reaches `{` with its
/// parameter list balanced, a prototype reaches `;`.
fn c_definition_name(lines: &[&str], i: usize) -> Option<CDefinition> {
    if !lines[i].chars().next().is_some_and(is_ident) {
        return None;
    }
    let open = lines[i].find('(')?;
    let head = lines[i][..open].trim_end();
    let ident: String = head.chars().rev().take_while(|c| is_ident(*c)).collect();
    if ident.is_empty() {
        return None;
    }
    let ident: String = ident.chars().rev().collect();

    // An invocation names nothing before the identifier; a declarator names at
    // least a return type. So `astMAKE_SET1(…)` is an invocation and
    // `static int MapMerge(…)` is a declarator, and prose that happens to hold
    // an identifier and a `(` is neither.
    if head == ident {
        // An argument list may run over several lines; the invocation ends
        // where it balances.
        let mut depth = 0i32;
        for (offset, text) in lines.iter().skip(i).take(9).enumerate() {
            for c in text.chars() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            if depth == 0 {
                return text
                    .trim_end()
                    .ends_with(')')
                    .then_some(CDefinition::Macro(ident, offset + 1));
            }
        }
        return None;
    }

    // What separates a definition from a prototype is where the declarator
    // ends: a definition reaches `{` with its parameter list balanced, a
    // prototype reaches `;`.
    let mut depth = 0i32;
    let mut opened = false;
    for text in lines.iter().skip(i).take(9) {
        for c in text.chars() {
            match c {
                '(' => {
                    depth += 1;
                    opened = true;
                }
                ')' => depth -= 1,
                _ if !opened || depth > 0 => {}
                '{' => return Some(CDefinition::Function(ident)),
                c if c.is_whitespace() => {}
                // The declarator is balanced and what follows is neither a body
                // nor a semicolon, so this was not a declarator.
                _ => return None,
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    /// A class whose trait method delegates its algorithm to a free function in
    /// a neighboring module, which is how the port holds `MapMerge` bodies.
    const CLASS_SRC: &str = r#"
impl SphMap {
    fn helper(&self) -> u32 {
        inherent_only()
    }
}

impl Mapping for SphMap {
    fn map_merge(&self, ctx: &mut Ctx) -> bool {
        super::simplify::sph_map_merge(self, ctx)
    }
}
"#;

    const MERGE_SRC: &str = r#"
fn sph_map_merge(map: &SphMap, ctx: &mut Ctx) -> bool {
    if let Some(hit) = try_sphmap_diagonal_pattern_at(ctx) {
        return default_map_merge(ctx, hit);
    }
    false
}

fn try_sphmap_diagonal_pattern_at(ctx: &Ctx) -> Option<usize> {
    extract_3d_diagonal(ctx)
}

fn extract_3d_diagonal(ctx: &Ctx) -> Option<usize> {
    None
}

fn unrelated_helper(ctx: &Ctx) -> bool {
    false
}
"#;

    #[test]
    fn a_trait_method_owns_the_body_it_delegates_to() {
        assert_eq!(trait_method_names(CLASS_SRC), vec!["map_merge".to_string()]);
        for owned in [
            "map_merge",
            "sph_map_merge",
            "try_sphmap_diagonal_pattern_at",
            "extract_3d_diagonal",
        ] {
            assert!(
                reached_from_trait_method(CLASS_SRC, MERGE_SRC, owned),
                "{owned} hangs off the seam"
            );
        }
        for outside in ["helper", "unrelated_helper"] {
            assert!(
                !reached_from_trait_method(CLASS_SRC, MERGE_SRC, outside),
                "{outside} is not reached from a trait method"
            );
        }
    }

    const IMPLS: &str = "\
struct Zoom;

impl Zoom {
    fn helper(&self) -> u32 {
        1
    }
}

impl Mapping for Zoom {
    fn simplified(&self) -> u32 {
        2
    }
}
";

    #[test]
    fn distinguishes_a_trait_method_from_an_inherent_one() {
        assert_eq!(enclosing_rust_block(IMPLS, 5).as_deref(), Some("Zoom"));
        assert_eq!(
            enclosing_rust_block(IMPLS, 11).as_deref(),
            Some("Mapping for Zoom")
        );
        assert_eq!(enclosing_rust_block(IMPLS, 1), None);
        assert!(!declares_trait_methods("Zoom"));
        assert!(declares_trait_methods("Mapping for Zoom"));
    }

    const TRAIT_DEF: &str = "\
pub trait Region: Frame + Mapping {
    fn parent_simplify(&self) -> u32 {
        helper()
    }
}

fn helper() -> u32 {
    1
}
";

    #[test]
    fn a_default_method_on_the_trait_is_a_trait_method() {
        assert_eq!(
            enclosing_rust_block(TRAIT_DEF, 3).as_deref(),
            Some("trait Region: Frame + Mapping")
        );
        assert!(declares_trait_methods("trait Region: Frame + Mapping"));
        assert_eq!(
            trait_method_names(TRAIT_DEF),
            vec!["parent_simplify".to_string()]
        );
        assert!(reached_from_trait_method(TRAIT_DEF, TRAIT_DEF, "helper"));
    }

    use super::*;

    // Line numbers are 1-based and the assertions below depend on them:
    // 4 `fn outer`, 7 `42`, 13 `fn other`, 14 its body, 18 `fn free`, 19 its body.
    const RUST: &str = "\
struct A;

impl A {
    fn outer(&self) -> u32 {
        let s = \"} not a brace\";
        if true {
            42
        } else {
            0
        }
    }

    fn other(&self) {
        let _ = 1;
    }
}

fn free() -> u32 {
    1
}
";

    #[test]
    fn finds_the_enclosing_fn_through_nested_braces_and_strings() {
        // Inside `outer`'s `if`.
        assert_eq!(enclosing_rust_item(RUST, 7).as_deref(), Some("outer"));
        // A `}` inside a string literal must not close a block.
        assert_eq!(enclosing_rust_item(RUST, 5).as_deref(), Some("outer"));
        assert_eq!(enclosing_rust_item(RUST, 14).as_deref(), Some("other"));
        assert_eq!(enclosing_rust_item(RUST, 19).as_deref(), Some("free"));
    }

    #[test]
    fn block_comments_are_stripped_across_lines_and_nest() {
        let src = "a /* { x */ b\n/* open {\n /* inner } */ still {\n*/ c { d\n// } e\n";
        let code: Vec<String> = code_lines(src);
        assert_eq!(code, ["a  b", "", "", " c { d", ""]);
    }

    #[test]
    fn c_string_raw_literals_are_stripped() {
        let src = "let a = cr#\"} {\"#; let b = br\"}\"; let c = r\"}\"; x\n";
        assert_eq!(code_lines(src), ["let a = c; let b = b; let c = ; x"]);
        // A backslash is literal in a raw string, so it must not hide the close.
        assert_eq!(code_lines("cr#\"\\\"#; {\n"), ["c; {"]);
        let src = "let a = cr#\"\n}\n\"#; y\n";
        assert_eq!(code_lines(src), ["let a = c", "", "; y"]);
    }

    #[test]
    fn a_line_outside_every_fn_has_no_enclosing_item() {
        assert_eq!(enclosing_rust_item(RUST, 1), None);
        assert_eq!(enclosing_rust_item(RUST, 3), None);
    }

    const WRAPPED_SIGNATURE: &str = "\
fn wrapped(
    xs: &[(u32, u32); 2],
    flag: bool,
) -> bool {
    xs[0].0 == 1 && flag
}
";

    #[test]
    fn a_semicolon_inside_a_parameter_list_does_not_end_the_signature() {
        // The `;` is the array length in `xs`, not the end of a bodiless
        // declaration, so the item spans its whole body.
        for line in 1..=6 {
            assert_eq!(
                enclosing_rust_item(WRAPPED_SIGNATURE, line).as_deref(),
                Some("wrapped"),
                "line {line}"
            );
        }
    }

    const ARRAY_RETURN: &str = "\
fn split(
    xs: &[u32],
) -> Option<[(u32, bool); 2]> {
    let a = xs.first()?;
    Some([(*a, true), (*a, false)])
}
";

    #[test]
    fn a_semicolon_inside_an_array_return_type_does_not_end_the_signature() {
        // The `;` is the array length in the return type, reached after the
        // parameter list has closed, so the item still spans its whole body.
        for line in 1..=6 {
            assert_eq!(
                enclosing_rust_item(ARRAY_RETURN, line).as_deref(),
                Some("split"),
                "line {line}"
            );
        }
    }

    const C: &str = "\
/* Prototype. */
static int MapMerge( AstMapping *, int, int * );

/* Body. */
static int MapMerge( AstMapping *this, int where,
                     int *status ) {

/* Local Variables: */
   int result;

/* Check the global error status. */
   if ( !astOK ) return 0;

   return result;
}

static void Other( void ) {
   return;
}
";

    /// AST generates its attribute accessors with file-scope macro
    /// invocations, and documents each attribute in a comment block whose prose
    /// also starts at column 0. A citation to an accessor line designates the
    /// macro; prose designates nothing.
    const C_ACCESSORS: &str = "\
/* UnitRadius */
/* ---------- */
/*
*att++
c     If this condition is true (indicated by setting UnitRadius
c     non-zero), it implies that a CmpMap may be simplified.
*att--
*/
astMAKE_CLEAR1(SphMap,UnitRadius,unitradius,(astClearIsSimple(this),-1))
astMAKE_TEST(SphMap,UnitRadius,( this->unitradius != -1 ))

static void Later( void ) {
   return;
}
";

    /// An accessor whose argument list runs over two lines, as
    /// `polygon.c`'s `SimpVertices` getter does.
    const C_WRAPPED_ACCESSOR: &str = "\
astMAKE_GET(Polygon,SimpVertices,int,0,( ( this->simp_vertices != -INT_MAX ) ?
                                   this->simp_vertices : 1 ))
astMAKE_TEST(Polygon,SimpVertices,( this->simp_vertices != -INT_MAX ))
";

    #[test]
    fn an_accessor_macro_may_span_more_than_one_line() {
        for line in 1..=2 {
            assert_eq!(
                enclosing_c_function(C_WRAPPED_ACCESSOR, line).as_deref(),
                Some("astMAKE_GET"),
                "line {line}"
            );
        }
        assert_eq!(
            enclosing_c_function(C_WRAPPED_ACCESSOR, 3).as_deref(),
            Some("astMAKE_TEST")
        );
    }

    #[test]
    fn an_accessor_macro_names_itself_and_comment_prose_names_nothing() {
        assert_eq!(
            enclosing_c_function(C_ACCESSORS, 9).as_deref(),
            Some("astMAKE_CLEAR1")
        );
        assert_eq!(
            enclosing_c_function(C_ACCESSORS, 10).as_deref(),
            Some("astMAKE_TEST")
        );
        // Prose that happens to hold an identifier and a `(` is not a
        // declarator, and must not swallow the lines below it.
        assert_eq!(enclosing_c_function(C_ACCESSORS, 5), None);
        assert_eq!(
            enclosing_c_function(C_ACCESSORS, 13).as_deref(),
            Some("Later")
        );
    }

    #[test]
    fn finds_the_c_definition_not_the_prototype() {
        assert_eq!(enclosing_c_function(C, 12).as_deref(), Some("MapMerge"));
        assert_eq!(enclosing_c_function(C, 2), None);
        assert_eq!(enclosing_c_function(C, 18).as_deref(), Some("Other"));
    }

    const DOCUMENTED: &str = "\
/// Documented.
///
/// More prose.
fn documented(
    a: u32,
) -> u32 {
    a
}

trait T {
    /// A declaration with no body.
    fn declared(&self) -> u32;
}
";

    #[test]
    fn a_doc_comment_or_signature_line_designates_its_item() {
        // The doc-comment block, the signature and the body all designate the
        // same item, as a C declarator line designates its function.
        for line in 1..=8 {
            assert_eq!(
                enclosing_rust_item(DOCUMENTED, line).as_deref(),
                Some("documented"),
                "line {line}"
            );
        }
        assert_eq!(
            enclosing_rust_item(DOCUMENTED, 11).as_deref(),
            Some("declared")
        );
        assert_eq!(
            enclosing_rust_item(DOCUMENTED, 12).as_deref(),
            Some("declared")
        );
    }

    #[test]
    fn a_citation_may_carry_leading_path_components() {
        let c = parse_citation("mappings/simplify.rs:2941").unwrap();
        assert_eq!((c.file.as_str(), c.start), ("mappings/simplify.rs", 2941));
    }

    #[test]
    fn an_ambiguous_stem_resolves_to_nothing_until_it_is_qualified() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("crates/audit-tools has two ancestors");
        // Two files are named `simplify.rs`, so the bare stem designates
        // neither; enough leading components single one out.
        match resolve_source(root, "simplify.rs") {
            Resolution::Ambiguous(paths) => assert!(paths.len() >= 2, "{paths:?}"),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
        match resolve_source(root, "mappings/simplify.rs") {
            Resolution::Found(path) => assert!(path.ends_with("mappings/simplify.rs")),
            other => panic!("expected Found, got {other:?}"),
        }
        // A component must match whole: `list.rs` is not a suffix of
        // `pointlist.rs`.
        assert_eq!(resolve_source(root, "no_such_file.rs"), Resolution::Missing);
        // Eighteen files are named `mod.rs`, and one leading component is
        // enough for each of them however deep it sits.
        match resolve_source(root, "mod.rs") {
            Resolution::Ambiguous(paths) => assert_eq!(paths.len(), 18, "{paths:?}"),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
        match resolve_source(root, "mathmap/mod.rs") {
            Resolution::Found(path) => assert!(path.ends_with("mappings/mathmap/mod.rs")),
            other => panic!("expected Found, got {other:?}"),
        }
        // A unique basename needs no qualification.
        match resolve_source(root, "zoom.rs") {
            Resolution::Found(path) => assert!(path.ends_with("mappings/zoom.rs")),
            other => panic!("expected Found, got {other:?}"),
        }
        // A C name resolves in the pinned submodule, and its absence is
        // Missing rather than a panic, because tier-1 builds without it.
        let c = resolve_source(root, "zoommap.c");
        assert!(
            matches!(&c, Resolution::Found(p) if p.ends_with("starlink-ast/src/zoommap.c"))
                || c == Resolution::Missing,
            "{c:?}"
        );
        assert_eq!(resolve_source(root, "nosuchfile.c"), Resolution::Missing);
    }

    #[test]
    fn finds_every_citation_in_a_row_not_only_the_first() {
        let row = "| 14 | step | sphmap.c:829-875 | mappings/simplify.rs:2878-2886 (fn f), mappings/simplify.rs:2968-2982 | ported |";
        let found = citations(row);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].0.file, "sphmap.c");
        assert_eq!(found[1].0.item.as_deref(), Some("f"));
        assert_eq!((found[2].0.start, found[2].0.end), (2968, Some(2982)));
        // The reported range covers exactly the citation, so a rewrite can
        // splice by offset without disturbing the rest of the row.
        let (cit, range) = &found[2];
        assert_eq!(&row[range.clone()], format!("{}:2968-2982", cit.file));
    }

    #[test]
    fn a_comma_separated_tail_becomes_a_citation_of_its_own() {
        let row = "| 12 | step | box.c:1622-1624 (fn MapMerge) | mappings/simplify.rs:1470 (fn region_map_merge),1482,1490-1495 | ported |";
        let found = citations(row);
        assert_eq!(found.len(), 4);
        // Each tail number inherits the file and item of the citation it
        // follows, which is what the shorthand says, so the gate checks it the
        // same way.
        for (i, (cit, _)) in found.iter().enumerate().skip(1) {
            assert_eq!(cit.file, "mappings/simplify.rs", "citation {i}");
        }
        // A tail carries no item of its own until one is written for it: the
        // lines it names are often in a different function from the citation it
        // follows, so inheriting that name would assert something untested.
        assert_eq!(found[2].0.item, None);
        assert_eq!((found[2].0.start, found[2].0.end), (1482, None));
        assert_eq!((found[3].0.start, found[3].0.end), (1490, Some(1495)));
        // The reported range covers just the tail, so a rewrite splices it
        // without disturbing the citation it hangs off.
        assert_eq!(&row[found[2].1.clone()], ",1482");

        // A tail that carries its own `(fn …)` is checked against it.
        let row = "| 1 | step | a.rs:1 (fn f),9 (fn g) | ported |";
        let found = citations(row);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].0.item.as_deref(), Some("g"));
        assert_eq!(&row[found[1].1.clone()], ",9 (fn g)");

        // A comma that separates two whole citations is not a tail: the second
        // names its own file, and inheriting the first's would be wrong.
        let row = "| 1 | step | a.c:1 (fn F), mappings/simplify.rs:2 (fn g) | ported |";
        let found = citations(row);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].0.file, "mappings/simplify.rs");
    }

    #[test]
    fn parses_all_three_citation_forms() {
        let c = parse_citation("polygon.rs:1158").unwrap();
        assert_eq!(
            (c.file.as_str(), c.start, c.end, c.item),
            ("polygon.rs", 1158, None, None)
        );
        let c = parse_citation("polygon.rs:1158-1262").unwrap();
        assert_eq!((c.start, c.end), (1158, Some(1262)));
        let c = parse_citation("polygon.rs:1158 (fn simplified)").unwrap();
        assert_eq!(c.item.as_deref(), Some("simplified"));
        assert!(parse_citation("prose with no citation").is_none());
    }
}

/// What a citation's file name designates in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Exactly one file matches.
    Found(std::path::PathBuf),
    /// No file matches. For a C citation this means the submodule is absent.
    Missing,
    /// More than one file matches, so the citation designates nothing until it
    /// is qualified with enough leading path components to single one out.
    Ambiguous(Vec<std::path::PathBuf>),
}

/// Resolve a citation's file name against the tree.
///
/// A Rust name is matched by whole path components against `starlink-ast`'s source,
/// so `simplify.rs` matches both `units/simplify.rs` and `mappings/simplify.rs`
/// and is reported ambiguous, while `mappings/simplify.rs` matches one.
/// Matching whole components keeps `pointlist.rs` from matching
/// `regions/cmp_pointlist.rs`. A C name is looked up directly in the pinned
/// source directory.
pub fn resolve_source(root: &std::path::Path, file: &str) -> Resolution {
    use std::path::PathBuf;
    if !file.ends_with(".rs") {
        let path = root.join("reference/starlink-ast/src").join(file);
        return if path.is_file() {
            Resolution::Found(path)
        } else {
            Resolution::Missing
        };
    }
    let wanted: Vec<&str> = file.split('/').filter(|s| !s.is_empty()).collect();
    let mut found: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.join("crates/starlink-ast/src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let parts: Vec<&str> = path
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();
            if parts.len() >= wanted.len() && parts[parts.len() - wanted.len()..] == wanted[..] {
                found.push(path);
            }
        }
    }
    found.sort();
    match found.len() {
        0 => Resolution::Missing,
        1 => Resolution::Found(found.remove(0)),
        _ => Resolution::Ambiguous(found),
    }
}

/// Every `fn` in `src` declared inside an `impl <Trait> for <Type>` block or a
/// `trait <Trait>` definition.
///
/// These are the port's equivalent of C's vtab slots: the methods a generic
/// driver reaches per class. A base class's own module declares them in the
/// trait rather than in an implementation of it, and a default method there is
/// dispatched per class exactly as an implemented one is.
pub fn trait_method_names(src: &str) -> Vec<String> {
    let headers = block_headers(src);
    rust_items(src)
        .into_iter()
        .filter(|(_, _, last)| {
            headers
                .get(last - 1)
                .and_then(Option::as_ref)
                .is_some_and(|header| declares_trait_methods(header))
        })
        .map(|(name, _, _)| name)
        .collect()
}

/// A Rust source's lines and items, parsed once for the call-graph search.
struct Parsed {
    /// Each line with comments and literal bodies stripped.
    code: Vec<String>,
    items: Vec<(String, usize, usize)>,
}

impl Parsed {
    fn new(src: &str) -> Self {
        Parsed {
            code: code_lines(src),
            items: rust_items(src),
        }
    }
}

/// The names `item` calls, as identifiers that appear before a `(` in its body.
///
/// Over-collecting is harmless here — a name that calls nothing contributes
/// nothing to the reachable set — so this looks for the shape of a call rather
/// than resolving one.
fn called_names(src: &Parsed, item: &str) -> Vec<String> {
    const KEYWORDS: &[&str] = &[
        "if", "while", "for", "match", "return", "fn", "let", "as", "in", "else", "loop", "move",
    ];
    let Some((_, first, last)) = src.items.iter().find(|(name, _, _)| name == item) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for code in &src.code[first - 1..*last] {
        let chars: Vec<char> = code.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            if *c != '(' {
                continue;
            }
            let mut start = i;
            while start > 0 && is_ident(chars[start - 1]) {
                start -= 1;
            }
            let name: String = chars[start..i].iter().collect();
            if !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && !KEYWORDS.contains(&name.as_str())
            {
                out.push(name);
            }
        }
    }
    out
}

/// Whether `item` is a trait method of the class, or code such a method owns.
///
/// A class's per-class algorithm does not always sit in the trait method
/// itself: the `MapMerge` bodies live in `mappings/simplify.rs` and the trait
/// method calls one. What the audit needs to know is that the per-class seam
/// exists and the code hangs off it, so this follows calls out of the class's
/// trait methods, through `class_src` and `cited_src`, and reports whether they
/// reach `item`.
pub fn reached_from_trait_method(class_src: &str, cited_src: &str, item: &str) -> bool {
    use std::collections::BTreeSet;
    let parsed = [Parsed::new(class_src), Parsed::new(cited_src)];
    let mut frontier = trait_method_names(class_src);
    let mut seen: BTreeSet<String> = frontier.iter().cloned().collect();
    // Deep enough for the port's longest chain: the trait method calls the
    // class's merge body, which calls a pattern matcher, which calls an
    // extractor.
    for _ in 0..4 {
        if seen.contains(item) {
            return true;
        }
        let mut next = Vec::new();
        for name in frontier.drain(..) {
            for src in &parsed {
                for callee in called_names(src, &name) {
                    if seen.insert(callee.clone()) {
                        next.push(callee);
                    }
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    seen.contains(item)
}
