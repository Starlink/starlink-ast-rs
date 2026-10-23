//! Print the commit author and co-author for a user-guide chapter.
//!
//! Usage:
//!   guide-authors <start> <end>
//!       blame lines `<start>..=<end>` of `reference/starlink-ast/sun_master.tex`
//!       and print `author: …` and, if there is one, `co-author: …`, followed by
//!       each blame author's share

use audit_tools::guide_authors::{credit, shares};
use std::process::{Command, ExitCode};

const SUBMODULE: &str = "reference/starlink-ast";
const SOURCE: &str = "sun_master.tex";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [start, end] = args.as_slice() else {
        eprintln!("usage: guide-authors <start> <end>");
        return ExitCode::from(2);
    };
    let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) else {
        eprintln!("guide-authors: <start> and <end> must be line numbers");
        return ExitCode::from(2);
    };
    let output = match Command::new("git")
        .args([
            "-C",
            SUBMODULE,
            "blame",
            "-w",
            "-M",
            "-C",
            "--line-porcelain",
            SOURCE,
        ])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            eprintln!("git blame failed: {}", String::from_utf8_lossy(&o.stderr));
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("git blame failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let blame = String::from_utf8_lossy(&output);
    let shares = shares(&blame, start, end);
    let (author, co_author) = credit(&shares);
    match author {
        Some(a) => println!("author: {a}"),
        None => println!("author: (none credited)"),
    }
    if let Some(c) = co_author {
        println!("co-author: {c}");
    }
    for (name, share) in &shares {
        println!("  {:5.1}%  {name}", share * 100.0);
    }
    ExitCode::SUCCESS
}
