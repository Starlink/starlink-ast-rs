//! Gates on the user guide in `ast_core::guide`
//! (`docs/superpowers/specs/2026-10-23-user-guide-design.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Sections of SUN/211 not yet ported. It may only go down; it reaches zero
/// when the guide is complete.
const GUIDE_SECTIONS_PENDING: usize = 160;

fn guide_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/guide")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(guide_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The guide's chapter files: every `*.md` but the README.
fn chapter_files() -> BTreeSet<String> {
    std::fs::read_dir(guide_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect()
}

#[test]
fn every_chapter_file_is_declared() {
    let declared: BTreeSet<String> = declared_chapters()
        .into_iter()
        .map(|name| format!("{name}.md"))
        .collect();
    assert_eq!(chapter_files(), declared);
}

/// The chapters `guide/mod.rs` declares.
fn declared_chapters() -> BTreeSet<String> {
    read("mod.rs")
        .lines()
        .filter_map(|l| l.strip_prefix("chapter!(")?.strip_suffix(");"))
        .map(str::to_string)
        .collect()
}

/// A link to a chapter not yet written is kept as its text followed by
/// `<!-- link: crate::guide::<chapter>... -->`; once that chapter is declared,
/// the marker has to become a link.
#[test]
fn no_link_marker_names_a_declared_chapter() {
    let declared = declared_chapters();
    let mut stale = Vec::new();
    for name in chapter_files() {
        for (i, line) in read(&name).lines().enumerate() {
            for marker in line.split("<!-- link: crate::guide::").skip(1) {
                let chapter: String = marker
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '_')
                    .collect();
                if declared.contains(&chapter) {
                    stale.push(format!("{name}:{}: {chapter}", i + 1));
                }
            }
        }
    }
    assert!(
        stale.is_empty(),
        "link markers to declared chapters: {stale:?}"
    );
}

/// The anchor rustdoc gives a Markdown heading: lower case, with each space
/// made a `-` and anything but letters, digits, `-` and `_` dropped.
fn anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// The anchors of a chapter's headings, outside code blocks.
fn anchors(chapter: &str) -> BTreeSet<String> {
    let mut fenced = false;
    let mut out = BTreeSet::new();
    for line in read(&format!("{chapter}.md")).lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.starts_with('#') {
            out.insert(anchor(line.trim_start_matches('#')));
        }
    }
    out
}

/// rustdoc checks the item an intra-doc link names but not its `#fragment`,
/// so every fragment of a link to a guide chapter, or of a marker for one
/// not yet written, has to name one of that chapter's headings once it exists.
#[test]
fn every_guide_fragment_names_a_heading() {
    let declared = declared_chapters();
    let mut bad = Vec::new();
    for name in chapter_files() {
        let this = name.trim_end_matches(".md");
        for (i, line) in read(&name).lines().enumerate() {
            let mut targets: Vec<(String, String)> = line
                .split("crate::guide::")
                .skip(1)
                .filter_map(|rest| {
                    let end = rest.find([')', ' ']).unwrap_or(rest.len());
                    let (chapter, frag) = rest[..end].split_once('#')?;
                    Some((chapter.to_string(), frag.to_string()))
                })
                .collect();
            targets.extend(line.split("](#").skip(1).map(|rest| {
                let end = rest.find(')').unwrap_or(rest.len());
                (this.to_string(), rest[..end].to_string())
            }));
            for (chapter, frag) in targets {
                if declared.contains(&chapter) && !anchors(&chapter).contains(&frag) {
                    bad.push(format!("{name}:{}: {chapter}#{frag}", i + 1));
                }
            }
        }
    }
    assert!(bad.is_empty(), "fragments naming no heading: {bad:?}");
}

#[test]
fn guide_mentions_no_unported_class() {
    const UNPORTED: [&str; 5] = ["XmlChan", "StcsChan", "YamlChan", "Plot", "Plot3D"];
    let mut found = Vec::new();
    for name in chapter_files() {
        for (i, line) in read(&name).lines().enumerate() {
            let words = line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            for word in words.filter(|w| UNPORTED.contains(w)) {
                found.push(format!("{name}:{}: {word}", i + 1));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn intro_carries_the_copyright() {
    assert!(read("intro.md").contains("Copyright (C) 2026 East Asian Observatory and AURA"));
}

/// Whether a README fate is one of the forms the README describes.
fn fate_is_well_formed(fate: &str) -> bool {
    fate == "pending"
        || fate.starts_with("guide::")
        || fate.starts_with("merged into guide::")
        || fate
            .strip_prefix("dropped: ")
            .is_some_and(|r| !r.is_empty())
}

#[test]
fn readme_records_every_section() {
    let sections: BTreeMap<usize, String> = read("sections.tsv")
        .lines()
        .map(|l| {
            let mut f = l.split('\t');
            let line = f.next().unwrap().parse().unwrap();
            (line, f.nth(1).unwrap().to_string())
        })
        .collect();
    let readme = read("README.md");
    let rows: BTreeMap<usize, String> = readme
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.strip_prefix('|')?.split('|').map(str::trim).collect();
            let line = cells.first()?.parse().ok()?;
            Some((line, cells.get(2)?.to_string()))
        })
        .collect();
    let missing: Vec<_> = sections.keys().filter(|l| !rows.contains_key(l)).collect();
    assert!(
        missing.is_empty(),
        "sections with no README row: {missing:?}"
    );
    let extra: Vec<_> = rows.keys().filter(|l| !sections.contains_key(l)).collect();
    assert!(extra.is_empty(), "README rows with no section: {extra:?}");
    let bad: Vec<_> = rows
        .iter()
        .filter(|(_, f)| !fate_is_well_formed(f))
        .collect();
    assert!(bad.is_empty(), "malformed fates: {bad:?}");
    let declared = declared_chapters();
    let unknown: Vec<_> = rows
        .iter()
        .filter_map(|(line, fate)| {
            let target = fate.strip_prefix("merged into ").unwrap_or(fate);
            let (chapter, frag) = match target.strip_prefix("guide::")?.split_once('#') {
                Some((c, f)) => (c, Some(f)),
                None => (target.strip_prefix("guide::")?, None),
            };
            let known =
                declared.contains(chapter) && frag.is_none_or(|f| anchors(chapter).contains(f));
            (!known).then_some((line, fate))
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "fates naming no chapter or heading: {unknown:?}"
    );
    let pending = rows.values().filter(|f| *f == "pending").count();
    assert_eq!(
        pending, GUIDE_SECTIONS_PENDING,
        "lower GUIDE_SECTIONS_PENDING to {pending} when sections are ported; it may never rise"
    );
}
