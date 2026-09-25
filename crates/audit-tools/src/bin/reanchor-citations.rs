//! Follow a checklist's citations onto the lines their content now occupies.
//!
//! A citation rots in two ways. The file it names can change around it, which
//! moves the cited content to a new line; or the row can be wrong about what it
//! describes, which no amount of relocation fixes. The first is mechanical and
//! this tool does it; the second it refuses and reports, because a citation
//! relocated onto content that is not what the row claims is worse than a stale
//! one — it reads as verified.
//!
//! The mechanism is a diff between the source as it stood when a row was
//! written and the source now, so the baseline is per row rather than per file.
//! A checklist holds rows from several commits at once: a fix that changes the C
//! source re-anchors the rows it touches and leaves the rest, and one written
//! after a pin move is already on the new source while its neighbours are not.
//! `git blame` names the commit that wrote each line; for a Rust citation that
//! commit's own tree is the baseline, and for a C citation it is the submodule
//! pin that commit recorded. A cited line the diff carries through unchanged is
//! relocated to where it landed; one the diff replaced or deleted is reported.
//! The relocation is then checked the way the audit gate checks it, against
//! [`audit_tools::enclosing_rust_item`] or
//! [`audit_tools::enclosing_c_function`], and rejected unless the item at the
//! new line is the one the citation names.
//!
//! Usage: reanchor-citations <checklist.md>...
//!   --write  rewrite in place; without it, report what would change

use audit_tools::Resolution;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("cwd readable");
    while !dir.join("PLAN.md").is_file() {
        dir = dir
            .parent()
            .expect("run inside the repository")
            .to_path_buf();
    }
    dir
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The submodule holding the pinned C source.
const SUBMODULE: &str = "reference/starlink-ast";

/// The commit that wrote each line of `path`, 1-based, from `git blame`.
///
/// A checklist's rows are written by different commits, and a row's own commit
/// is the state its citation was checked against, so the baseline has to be read
/// per line rather than per file.
fn blame(root: &Path, path: &Path) -> Option<Vec<String>> {
    let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
    let out = git(root, &["blame", "--porcelain", "HEAD", "--", &rel])?;
    let mut shas = Vec::new();
    for line in out.lines() {
        // A porcelain header is "<sha> <orig-line> <final-line> [<count>]", and
        // only a header starts with a 40-character hex field.
        let mut fields = line.split(' ');
        let Some(sha) = fields.next() else { continue };
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if fields
            .next()
            .and_then(|f| f.parse::<usize>().ok())
            .is_none()
        {
            continue;
        }
        let Some(final_line) = fields.next().and_then(|f| f.parse::<usize>().ok()) else {
            continue;
        };
        if shas.len() < final_line {
            shas.resize(final_line, String::new());
        }
        shas[final_line - 1] = sha.to_string();
    }
    (!shas.is_empty()).then_some(shas)
}

/// The submodule pin `rev` recorded, which is the C source a row written at
/// `rev` was checked against.
fn pin_at(root: &Path, rev: &str) -> Option<String> {
    let out = git(root, &["rev-parse", &format!("{rev}:{SUBMODULE}")])?;
    let pin = out.trim().to_string();
    (!pin.is_empty()).then_some(pin)
}

/// Map every line of the old text that survives into the new one, as a 1-based
/// line map.
///
/// Built by trimming the common head and tail and running a longest common
/// subsequence over what is left. A source file's edits are localised, so the
/// remainder is small even for the largest C file in the tree, while the full
/// table over two 40,000-line files would not fit in memory.
///
/// A line is mapped only where the subsequence carries it through rather than
/// replacing it, so lines that repeat in the file are placed by their position
/// in the sequence and not by a content search that cannot tell two identical
/// lines apart.
fn line_map(old: &[&str], new: &[&str]) -> HashMap<usize, usize> {
    let mut out = HashMap::new();
    let head = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    for i in 0..head {
        out.insert(i + 1, i + 1);
    }
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    for k in 0..tail {
        out.insert(old.len() - k, new.len() - k);
    }
    let (old, new) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let (n, m) = (old.len(), new.len());
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if old[i] == new[j] {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old[i] == new[j] {
            out.insert(head + i + 1, head + j + 1);
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// A source file's current text, and its diff against one baseline revision.
///
/// The baseline is part of the key because rows written by different commits are
/// compared against different revisions of the same file.
struct Sources {
    /// Current text, keyed by resolved path.
    now: HashMap<PathBuf, String>,
    /// Text as of a baseline revision, keyed by path and that revision.
    then: HashMap<(PathBuf, String), String>,
    /// Line maps from a baseline revision to now, keyed the same way.
    maps: HashMap<(PathBuf, String), HashMap<usize, usize>>,
}

impl Sources {
    fn new() -> Self {
        Sources {
            now: HashMap::new(),
            then: HashMap::new(),
            maps: HashMap::new(),
        }
    }

    /// Load `path` as it stands now and as of `rev`, caching both along with the
    /// line map between them.
    ///
    /// For a file inside the submodule `rev` is a pin and the text comes from the
    /// submodule; for one in the superproject it is a superproject commit.
    fn load(&mut self, root: &Path, path: &Path, rev: &str) -> Option<()> {
        let key = (path.to_path_buf(), rev.to_string());
        if self.maps.contains_key(&key) {
            return Some(());
        }
        let now = match self.now.get(path) {
            Some(text) => text.clone(),
            None => {
                let text = std::fs::read_to_string(path).ok()?;
                self.now.insert(path.to_path_buf(), text.clone());
                text
            }
        };
        let then = match path.strip_prefix(root.join(SUBMODULE)) {
            Ok(inner) => {
                let inner = inner.to_string_lossy();
                git(&root.join(SUBMODULE), &["show", &format!("{rev}:{inner}")])?
            }
            Err(_) => {
                let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
                git(root, &["show", &format!("{rev}:{rel}")])?
            }
        };
        let map = line_map(
            &then.lines().collect::<Vec<_>>(),
            &now.lines().collect::<Vec<_>>(),
        );
        self.then.insert(key.clone(), then);
        self.maps.insert(key, map);
        Some(())
    }
}

/// Whether `text` opts into the audit gate, which is what makes its citations
/// this tool's to move.
fn opted_in(text: &str) -> bool {
    text.lines()
        .take(12)
        .any(|l| l.starts_with("Citations: verified against pin "))
}

fn main() {
    let root = repo_root();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let write = args.iter().any(|a| a == "--write");
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();

    let mut sources = Sources::new();
    let mut moved = 0usize;
    let mut stuck: Vec<String> = Vec::new();
    for arg in files {
        let path = PathBuf::from(arg);
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        if !opted_in(&text) {
            continue;
        }
        let Some(wrote) = blame(&root, &path) else {
            stuck.push(format!("{}: cannot blame the checklist", path.display()));
            continue;
        };
        let mut out = String::with_capacity(text.len());
        for (i, line) in text.lines().enumerate() {
            let mut rebuilt = line.to_string();
            // Right to left, so an earlier citation's byte offsets stay valid.
            for (cit, range) in audit_tools::citations(line).into_iter().rev() {
                let Resolution::Found(src_path) = audit_tools::resolve_source(&root, &cit.file)
                else {
                    continue;
                };
                // The commit that wrote the row is the state it was checked
                // against: its own tree for a Rust citation, and the submodule
                // pin it recorded for a C one.
                let Some(sha) = wrote.get(i).filter(|s| !s.is_empty()) else {
                    stuck.push(format!(
                        "{}:{}: no commit wrote this line, so there is no baseline",
                        path.display(),
                        i + 1
                    ));
                    continue;
                };
                let in_submodule = src_path.starts_with(root.join(SUBMODULE));
                let rev = if in_submodule {
                    match pin_at(&root, sha) {
                        Some(pin) => pin,
                        None => {
                            stuck.push(format!(
                                "{}:{}: {} recorded no submodule pin",
                                path.display(),
                                i + 1,
                                &sha[..8]
                            ));
                            continue;
                        }
                    }
                } else {
                    sha.clone()
                };
                if sources.load(&root, &src_path, &rev).is_none() {
                    stuck.push(format!(
                        "{}:{}: cannot read `{}` at {}",
                        path.display(),
                        i + 1,
                        cit.file,
                        &rev[..8.min(rev.len())]
                    ));
                    continue;
                }
                let now = &sources.now[&src_path];
                let is_rust = cit.file.ends_with(".rs");
                let item_at = |line: usize| {
                    if is_rust {
                        audit_tools::enclosing_rust_item(now, line)
                    } else {
                        audit_tools::enclosing_c_function(now, line)
                    }
                };
                // A citation with no `(fn …)` of its own is still a line number
                // that drifts, so it is relocated the same way. It carries no
                // claim for the relocation to be checked against, so the name is
                // derived from the line the content reached — the same
                // derivation `anchor-citations` uses, applied once the number is
                // right rather than before.
                let named = |line: usize| match &cit.item {
                    Some(claimed) => format!(" (fn {claimed})"),
                    None => item_at(line)
                        .map(|name| format!(" (fn {name})"))
                        .unwrap_or_default(),
                };
                let map = &sources.maps[&(src_path.clone(), rev.clone())];
                let Some(&start) = map.get(&cit.start) else {
                    stuck.push(format!(
                        "{}:{}: `{}:{}`{} — the cited line did not survive; \
                         re-anchor against the code the row describes",
                        path.display(),
                        i + 1,
                        cit.file,
                        cit.start,
                        named(cit.start)
                    ));
                    continue;
                };
                if let Some(claimed) = &cit.item
                    && item_at(start).as_deref() != Some(claimed.as_str())
                {
                    stuck.push(format!(
                        "{}:{}: `{}:{}` (fn {claimed}) — its content moved to \
                         line {start}, which is in {}; re-anchor by hand",
                        path.display(),
                        i + 1,
                        cit.file,
                        cit.start,
                        item_at(start).unwrap_or_else(|| "no item".into())
                    ));
                    continue;
                }
                let end = cit.end.and_then(|e| map.get(&e).copied());
                if let Some(last) = cit.end
                    && end.is_none()
                {
                    stuck.push(format!(
                        "{}:{}: `{}:{}-{last}`{} — the range's last line did not \
                         survive; re-anchor by hand",
                        path.display(),
                        i + 1,
                        cit.file,
                        cit.start,
                        named(start)
                    ));
                    continue;
                }
                if start == cit.start && end == cit.end {
                    continue;
                }
                // A continuation is spliced back in the form it was written:
                // `,line` names further lines of the file the citation before it
                // named, and spelling the file out again would read as a second
                // citation.
                let head = match line[range.clone()].starts_with(',') {
                    true => ",".to_string(),
                    false => format!("{}:", cit.file),
                };
                let replacement = match end {
                    Some(end) => format!("{head}{start}-{end}{}", named(start)),
                    None => format!("{head}{start}{}", named(start)),
                };
                rebuilt.replace_range(range, &replacement);
                moved += 1;
            }
            out.push_str(&rebuilt);
            out.push('\n');
        }
        if write {
            std::fs::write(&path, out).expect("checklist writable");
        }
    }
    println!("{moved} citations relocated");
    if !stuck.is_empty() {
        eprintln!("{} citations need a hand re-anchor:", stuck.len());
        for s in &stuck {
            eprintln!("  {s}");
        }
        std::process::exit(1);
    }
}
