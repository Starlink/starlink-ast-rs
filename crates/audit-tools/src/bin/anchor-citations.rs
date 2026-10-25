// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Rewrite a checklist's citations into `file:line (fn name)` form.
//!
//! Reports every row it cannot anchor instead of relocating it. A citation that
//! was wrong when it was written stays wrong under any mechanical remap, so the
//! rows this reports need a per-row re-anchor against the code they describe.
//!
//! Usage: anchor-citations <checklist.md>...
//!   --write   rewrite in place; without it, print what would change

use audit_tools::Resolution;
use std::path::PathBuf;

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

fn main() {
    let root = repo_root();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let write = args.iter().any(|a| a == "--write");
    let mut unanchored = 0usize;
    for arg in args.iter().filter(|a| !a.starts_with("--")) {
        let path = PathBuf::from(arg);
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        let mut out = String::with_capacity(text.len());
        for (i, line) in text.lines().enumerate() {
            let mut rebuilt = line.to_string();
            // Right to left, so an earlier citation's offsets stay valid.
            for (cit, range) in audit_tools::citations(line).into_iter().rev() {
                if cit.item.is_some() {
                    continue;
                }
                let src_path = match audit_tools::resolve_source(&root, &cit.file) {
                    Resolution::Found(p) => p,
                    Resolution::Ambiguous(paths) => {
                        unanchored += 1;
                        println!(
                            "{}:{}: {} names {} files — qualify the stem",
                            path.display(),
                            i + 1,
                            cit.file,
                            paths.len()
                        );
                        continue;
                    }
                    Resolution::Missing => continue,
                };
                let src = std::fs::read_to_string(&src_path).expect("source readable");
                let item = if cit.file.ends_with(".rs") {
                    audit_tools::enclosing_rust_item(&src, cit.start)
                } else {
                    audit_tools::enclosing_c_function(&src, cit.start)
                };
                match item {
                    Some(name) => rebuilt.insert_str(range.end, &format!(" (fn {name})")),
                    None => {
                        unanchored += 1;
                        println!(
                            "{}:{}: {}:{} is not inside any item — re-anchor by hand",
                            path.display(),
                            i + 1,
                            cit.file,
                            cit.start
                        );
                    }
                }
            }
            out.push_str(&rebuilt);
            out.push('\n');
        }
        if write {
            std::fs::write(&path, out).expect("checklist writable");
        } else {
            print!("{out}");
        }
    }
    if unanchored > 0 {
        eprintln!("{unanchored} citations could not be anchored");
        std::process::exit(1);
    }
}
