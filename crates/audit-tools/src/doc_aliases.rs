//! The `#[doc(alias = "…")]` attributes in a Rust source file.
//!
//! The alias gate compares them with the documentation ledger, so the name
//! of the item an alias sits on and the type or trait that owns it are
//! derived here once.

use crate::enclosing_rust_block;

/// Every `#[doc(alias = "…")]` in `src`: (alias, item name, owner).
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
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") || !mentions_alias(line) {
            continue;
        }
        let unparsed = || {
            (
                "<unparsed>".to_string(),
                format!("<unparsed: line {}>", i + 1),
                "<unparsed>".to_string(),
            )
        };
        let Some(alias) = alias_of(line) else {
            out.push(unparsed());
            continue;
        };
        let declaration = lines.iter().enumerate().skip(i + 1).find(|(_, l)| {
            let l = l.trim_start();
            !(l.is_empty() || l.starts_with("#[") || l.starts_with("//"))
        });
        let resolved = declaration.and_then(|(at, text)| Some((at, declared_name(text)?)));
        let Some((at, item)) = resolved else {
            out.push((
                alias,
                format!("<unresolved: line {}>", i + 1),
                "<unresolved>".to_string(),
            ));
            continue;
        };
        let owner = match enclosing_rust_block(src, at + 1) {
            Some(h) => match h.split_once(" for ") {
                Some((_, ty)) => ty.trim().to_string(),
                None => h.strip_prefix("trait ").unwrap_or(&h).trim().to_string(),
            },
            None => item.clone(),
        };
        out.push((alias, item, owner));
    }
    out
}

/// Whether a line carries, or opens, a `doc(alias …)` attribute.
///
/// Every such line has to surface in the result, parsed or not, so an alias
/// the parser cannot read still reaches the gate instead of being invisible.
fn mentions_alias(line: &str) -> bool {
    let compact: String = line.split_whitespace().collect();
    compact.contains("doc(alias") || compact.ends_with("#[doc(")
}

/// The alias in a line that is a `#[doc(alias = "…")]` attribute.
fn alias_of(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("#[")?.trim_start();
    let rest = rest.strip_prefix("doc")?.trim_start();
    let rest = rest.strip_prefix('(')?.trim_start();
    let rest = rest.strip_prefix("alias")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
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
    use super::doc_aliases;

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
    fn an_alias_on_an_unresolvable_declaration_is_reported_not_dropped() {
        let src =
            "impl K {\n    #[doc(alias = \"astX\")]\n    #[cfg(unix)] pub fn f(&self) {}\n}\n";
        let a = doc_aliases(src);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].0, "astX");
        assert!(a[0].1.starts_with("<unresolved"), "{a:?}");
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
}
