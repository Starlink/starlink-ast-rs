//! The `#[doc(alias = "…")]` attributes in a Rust source file.
//!
//! The alias gate compares them with the documentation ledger, so the name
//! of the item an alias sits on and the type or trait that owns it are
//! derived here once.

use crate::{code_lines, enclosing_rust_block};
use std::collections::BTreeMap;

/// Every `#[doc(alias = "…")]` in `src`: (alias, item name, owner).
///
/// An attribute that lists several aliases, `#[doc(alias = "a", alias = "b")]`,
/// yields one entry per alias. A `doc(alias …)` inside a string literal or a
/// comment is not an attribute and is skipped.
///
/// The item is the next `fn`/`struct`/`enum`/`trait`/`type`/`const` declared
/// after the attribute; the owner is the subject of the enclosing `impl` or
/// `trait` block (`"PermMap"` for `impl PermMap` and `impl Mapping for
/// PermMap`, `"Mapping"` for `trait Mapping`), or the item's own name when
/// there is no enclosing block.
///
/// An alias attribute the parser cannot read, or whose declaration it cannot
/// resolve, is returned with an item of `<unparsed: line N>` or
/// `<unresolved: line N>`, which no ledger row can name, so the gate fails on
/// it rather than missing it.
pub fn doc_aliases(src: &str) -> Vec<(String, String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let code = code_lines(src);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !mentions_alias(&code[i]) {
            continue;
        }
        let unparsed = || {
            (
                "<unparsed>".to_string(),
                format!("<unparsed: line {}>", i + 1),
                "<unparsed>".to_string(),
            )
        };
        let Some(aliases) = aliases_of(line) else {
            out.push(unparsed());
            continue;
        };
        let declaration = lines.iter().enumerate().skip(i + 1).find(|(_, l)| {
            let l = l.trim_start();
            !(l.is_empty() || l.starts_with("#[") || l.starts_with("//"))
        });
        let resolved = declaration.and_then(|(at, text)| Some((at, declared_name(text)?)));
        let Some((at, item)) = resolved else {
            for alias in aliases {
                out.push((
                    alias,
                    format!("<unresolved: line {}>", i + 1),
                    "<unresolved>".to_string(),
                ));
            }
            continue;
        };
        let owner = match enclosing_rust_block(src, at + 1) {
            Some(h) => match h.split_once(" for ") {
                Some((_, ty)) => ty.trim().to_string(),
                // A trait's supertraits follow a `:` in its header.
                None => match h.strip_prefix("trait ") {
                    Some(t) => t.split(':').next().unwrap_or(t).trim().to_string(),
                    None => h.trim().to_string(),
                },
            },
            None => item.clone(),
        };
        for alias in aliases {
            out.push((alias, item.clone(), owner.clone()));
        }
    }
    out
}

/// The entries of `aliases` (from [`doc_aliases`]) that occur more than once.
///
/// An alias set compared as a set hides a second identical attribute on the
/// same item, so the gate counts them here.
pub fn duplicate_aliases(aliases: &[(String, String, String)]) -> Vec<(String, String, String)> {
    let mut counts: BTreeMap<&(String, String, String), usize> = BTreeMap::new();
    for a in aliases {
        *counts.entry(a).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(a, _)| a.clone())
        .collect()
}

/// The names of the public types and traits `src` declares at the top level
/// of the file.
///
/// A declaration counts only when it starts in column 0 with `pub ` (not
/// `pub(crate)` or another restricted visibility): rustfmt indents anything
/// inside an `impl`, `trait`, `mod` or `fn` body, so column 0 is the file's
/// own scope.
pub fn top_level_types(src: &str) -> Vec<String> {
    src.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("pub ")?;
            let kind = rest.split_whitespace().find(|w| *w != "unsafe")?;
            if !matches!(kind, "trait" | "struct" | "enum" | "type") {
                return None;
            }
            declared_name(l)
        })
        .collect()
}

/// Whether a ledger row may go without a `#[doc(alias)]` because its Rust
/// item already has the C name.
///
/// rustdoc rejects a `#[doc(alias)]` equal to the item's own name, so a row
/// whose Rust item is a public top-level type or trait (one of `types`, from
/// [`top_level_types`]) named exactly as the C name cannot carry the
/// attribute; the name itself is what a search finds. Every other row,
/// including a type named differently from the C name, needs its alias.
pub fn alias_satisfied_by_name(
    rust_item: &str,
    alias: &str,
    types: &std::collections::BTreeSet<String>,
) -> bool {
    !rust_item.contains("::") && rust_item == alias && types.contains(rust_item)
}

/// Whether a line carries, or opens, a `doc(alias …)` attribute.
///
/// Every such line has to surface in the result, parsed or not, so an alias
/// the parser cannot read still reaches the gate instead of being invisible.
fn mentions_alias(line: &str) -> bool {
    let compact: String = line.split_whitespace().collect();
    compact.contains("doc(alias") || compact.ends_with("#[doc(")
}

/// The aliases in a line that is a `#[doc(alias = "…", alias = "…")]`
/// attribute, or `None` when the line is not of that form.
fn aliases_of(line: &str) -> Option<Vec<String>> {
    let rest = line.trim_start().strip_prefix("#[")?.trim_start();
    let rest = rest.strip_prefix("doc")?.trim_start();
    let mut rest = rest.strip_prefix('(')?.trim_start();
    let mut out = Vec::new();
    loop {
        let r = rest.strip_prefix("alias")?.trim_start();
        let r = r.strip_prefix('=')?.trim_start();
        let r = r.strip_prefix('"')?;
        let end = r.find('"')?;
        out.push(r[..end].to_string());
        let r = r[end + 1..].trim_start();
        match r.strip_prefix(',') {
            Some(more) if !more.trim_start().starts_with(')') => rest = more.trim_start(),
            _ => {
                let closed = r.trim_start_matches(',').trim_start().starts_with(')');
                return closed.then_some(out);
            }
        }
    }
}

/// The name a declaration line introduces, past visibility and qualifiers.
fn declared_name(line: &str) -> Option<String> {
    let mut words = line.split_whitespace().peekable();
    while let Some(w) = words.next() {
        // `pub(crate)` and `pub(in path)` are visibility, not a name.
        let w = w.split('(').next().unwrap_or(w);
        match w {
            "pub" | "unsafe" | "async" | "extern" | "default" => {}
            // `const fn` qualifies a function; `const NAME: T` is the item.
            "const" if words.peek().is_some_and(|n| is_qualifier(n)) => {}
            "const" | "fn" | "struct" | "enum" | "trait" | "type" | "static" | "union" => {
                return ident(words.next()?);
            }
            _ => return None,
        }
    }
    None
}

fn is_qualifier(w: &str) -> bool {
    matches!(w, "fn" | "unsafe" | "async" | "extern")
}

/// The leading identifier of `s`.
fn ident(s: &str) -> Option<String> {
    let name: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{alias_satisfied_by_name, doc_aliases, duplicate_aliases, top_level_types};

    fn t(a: &str, i: &str, o: &str) -> (String, String, String) {
        (a.into(), i.into(), o.into())
    }

    #[test]
    fn finds_aliases_on_methods_types_and_trait_items() {
        let src = r#"
/// A map.
#[doc(alias = "PermMap")]
pub struct PermMap;
impl PermMap {
    /// New.
    #[doc(alias = "astPermMap")]
    pub fn new() -> Self { PermMap }
}
pub trait Mapping {
    #[doc(alias = "Invert")]
    fn is_inverted(&self) -> bool;
}
impl Mapping for PermMap {
    fn is_inverted(&self) -> bool { false }
}
"#;
        let a = doc_aliases(src);
        assert!(a.contains(&t("PermMap", "PermMap", "PermMap")));
        assert!(a.contains(&t("astPermMap", "new", "PermMap")));
        assert!(a.contains(&t("Invert", "is_inverted", "Mapping")));
    }

    #[test]
    fn an_alias_on_a_type_alias_passes() {
        let src = "#[doc(alias = \"StcResourceProfile\")]\npub type StcResourceProfile = Stc<ResourceProfile>;\n";
        assert_eq!(
            doc_aliases(src),
            [t(
                "StcResourceProfile",
                "StcResourceProfile",
                "StcResourceProfile"
            )]
        );
    }

    #[test]
    fn other_attributes_between_alias_and_item_are_skipped() {
        let src = "impl K {\n    #[doc(alias = \"astMapGet0\")]\n    #[must_use]\n    pub fn map_get0_d(&self) {}\n}\n";
        assert_eq!(doc_aliases(src), [t("astMapGet0", "map_get0_d", "K")]);
    }

    #[test]
    fn a_family_alias_on_each_member_passes() {
        let src = "impl KeyMap {\n    #[doc(alias = \"astMapGet0\")]\n    pub fn map_get0_d(&self) {}\n    #[doc(alias = \"astMapGet0\")]\n    pub fn map_get0_i(&self) {}\n}\n";
        assert_eq!(
            doc_aliases(src),
            [
                t("astMapGet0", "map_get0_d", "KeyMap"),
                t("astMapGet0", "map_get0_i", "KeyMap")
            ]
        );
    }

    #[test]
    fn visibility_and_qualifiers_do_not_hide_the_item_name() {
        let src = "impl K {\n    #[doc(alias = \"a\")]\n    pub(crate) fn one(&self) {}\n    #[doc(alias = \"b\")]\n    pub const fn two(&self) {}\n    #[doc(alias = \"c\")]\n    pub(crate) const unsafe fn three(&self) {}\n}\n";
        assert_eq!(
            doc_aliases(src),
            [t("a", "one", "K"), t("b", "two", "K"), t("c", "three", "K")]
        );
    }

    #[test]
    fn an_alias_inside_a_trait_impl_is_owned_by_the_type() {
        let src = "impl Mapping for Zoom {\n    #[doc(alias = \"x\")]\n    fn f(&self) {}\n}\n";
        assert_eq!(doc_aliases(src), [t("x", "f", "Zoom")]);
    }

    #[test]
    fn whitespace_in_the_attribute_is_tolerated() {
        let src = "  #[ doc( alias = \"Lbl\" ) ]\nenum E {}\n";
        assert_eq!(doc_aliases(src), [t("Lbl", "E", "E")]);
    }

    #[test]
    fn a_trait_with_supertraits_owns_its_methods_aliases() {
        let src = "pub trait Mapping: AsAny + Send + fmt::Debug {\n    #[doc(alias = \"Nin\")]\n    fn nin(&self) -> usize;\n}\n";
        assert_eq!(doc_aliases(src), [t("Nin", "nin", "Mapping")]);
    }

    #[test]
    fn top_level_types_are_public_items_outside_every_block() {
        let src = "pub trait Mapping {\n    type Out;\n}\npub struct S;\nimpl S {\n    fn f() {}\n}\npub enum E {}\npub fn g() {}\n";
        assert_eq!(top_level_types(src), ["Mapping", "S", "E"]);
    }

    #[test]
    fn a_crate_private_type_is_not_top_level() {
        let src = "pub(crate) struct Axis {\n    x: f64,\n}\nstruct Hidden;\n";
        assert!(
            top_level_types(src).is_empty(),
            "{:?}",
            top_level_types(src)
        );
    }

    #[test]
    fn a_type_inside_a_mod_or_fn_body_is_not_top_level() {
        let src = "mod inner {\n    pub struct Axis;\n}\nfn f() {\n    pub struct Local;\n}\n";
        assert!(
            top_level_types(src).is_empty(),
            "{:?}",
            top_level_types(src)
        );
    }

    #[test]
    fn the_same_name_exception_needs_a_public_type_of_the_alias_name() {
        let types = ["Mapping".to_string()].into_iter().collect();
        assert!(alias_satisfied_by_name("Mapping", "Mapping", &types));
        // A Rust item named differently from the C name still needs its alias.
        let types = ["BoxRegion".to_string()].into_iter().collect();
        assert!(!alias_satisfied_by_name("BoxRegion", "Box", &types));
        // A method row never takes the exception.
        let types = ["nin".to_string()].into_iter().collect();
        assert!(!alias_satisfied_by_name("Mapping::nin", "nin", &types));
        // A same-name item that is not a public top-level type is refused.
        let src = "pub(crate) struct Axis;\n";
        let types = top_level_types(src).into_iter().collect();
        assert!(!alias_satisfied_by_name("Axis", "Axis", &types));
    }

    #[test]
    fn an_alias_on_an_unresolvable_declaration_is_reported_not_dropped() {
        let src =
            "impl K {\n    #[doc(alias = \"astX\")]\n    #[cfg(unix)] pub fn f(&self) {}\n}\n";
        let a = doc_aliases(src);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].0, "astX");
        assert!(a[0].1.starts_with("<unresolved"), "{a:?}");
    }

    #[test]
    fn a_continued_string_does_not_hide_the_owner() {
        // The continuation line opens with `'{`, inside the string, which is
        // neither a char literal nor a brace.
        let src = r#"impl FitsChan {
    pub fn set_source_file(&mut self, source_file: &str) {
        let e = 1;
        let _ = format!("Failed to open input SourceFile \
            '{source_file}' - {e}.");
    }

    #[doc(alias = "SinkFile")]
    pub fn sink_file(&self) {}
}
"#;
        assert_eq!(doc_aliases(src), [t("SinkFile", "sink_file", "FitsChan")]);
    }

    #[test]
    fn a_raw_string_spanning_lines_does_not_hide_the_owner() {
        let src = r##"impl K {
    pub fn f(&self) {
        let _ = r#"a "quoted" {
            line with } and "#;
    }

    #[doc(alias = "astX")]
    pub fn g(&self) {}
}
"##;
        assert_eq!(doc_aliases(src), [t("astX", "g", "K")]);
    }

    #[test]
    fn a_multi_line_alias_attribute_is_reported_not_dropped() {
        let src = "impl K {\n    #[doc(alias =\n        \"astX\")]\n    pub fn f(&self) {}\n}\n";
        let a = doc_aliases(src);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!(a[0].1.starts_with("<unparsed"), "{a:?}");
        let src = "#[doc(\n    alias = \"astX\"\n)]\nfn f() {}\n";
        let a = doc_aliases(src);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!(a[0].1.starts_with("<unparsed"), "{a:?}");
    }

    #[test]
    fn a_second_alias_in_one_attribute_is_found() {
        let src = "impl K {\n    #[doc(alias = \"a\", alias = \"b\")]\n    pub fn f(&self) {}\n    #[doc(alias = \"c\",)]\n    pub fn g(&self) {}\n}\n";
        assert_eq!(
            doc_aliases(src),
            [t("a", "f", "K"), t("b", "f", "K"), t("c", "g", "K")]
        );
        let src = "#[doc(alias = \"a\", hidden)]\nfn f() {}\n";
        assert!(doc_aliases(src)[0].1.starts_with("<unparsed"));
    }

    #[test]
    fn an_alias_in_a_string_or_comment_is_not_an_attribute() {
        let src = "fn f() {\n    let _ = \"#[doc(alias = \\\"x\\\")]\";\n    /* #[doc(alias = \"y\")]\n       #[doc(alias = \"z\")] */\n    // #[doc(alias = \"w\")]\n}\n";
        assert_eq!(doc_aliases(src), []);
        let src = "const S: &str = r#\"\n#[doc(alias = \"x\")]\n\"#;\n";
        assert_eq!(doc_aliases(src), []);
    }

    #[test]
    fn duplicates_are_reported() {
        let a = [t("x", "f", "K"), t("y", "f", "K"), t("x", "f", "K")];
        assert_eq!(duplicate_aliases(&a), [t("x", "f", "K")]);
        assert!(duplicate_aliases(&a[..2]).is_empty());
    }
}
