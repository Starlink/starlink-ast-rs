//! Inspect the public documentation headers of the C library.
//!
//! Usage:
//!   c-docs headers <src-dir>
//!       print the TSV of every header in every `*.c` file of `<src-dir>`
//!   c-docs show <file.c> [--src <src-dir>]
//!       print the C text of each header in one file; `<src-dir>` defaults to
//!       `reference/starlink-ast/src`

use audit_tools::{c_headers, headers_tsv};
use std::path::Path;
use std::process::ExitCode;

const DEFAULT_SRC: &str = "reference/starlink-ast/src";

fn usage() -> ExitCode {
    eprintln!("usage: c-docs headers <src-dir>");
    eprintln!("       c-docs show <file.c> [--src <src-dir>]");
    ExitCode::from(2)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn headers(dir: &Path) -> Result<(), String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "c"))
        .collect();
    files.sort();
    let mut all = Vec::new();
    for path in files {
        let name = path.file_name().expect("file has a name").to_string_lossy();
        all.extend(c_headers(&read(&path)?, &name));
    }
    print!("{}", headers_tsv(&all));
    Ok(())
}

fn show(file: &str, src: &Path) -> Result<(), String> {
    let path = if Path::new(file).is_file() {
        Path::new(file).to_path_buf()
    } else {
        src.join(file)
    };
    let name = path.file_name().expect("file has a name").to_string_lossy();
    for h in c_headers(&read(&path)?, &name) {
        println!("== {} {}", h.kind, h.name);
        println!("{}", h.text);
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["headers", dir] => headers(Path::new(dir)),
        ["show", file] => show(file, Path::new(DEFAULT_SRC)),
        ["show", file, "--src", dir] => show(file, Path::new(dir)),
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("c-docs: {e}");
            ExitCode::FAILURE
        }
    }
}
