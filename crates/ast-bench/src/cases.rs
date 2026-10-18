//! Benchmark cases: an oracle section of a fixture, named by a set file.

//! Benchmark cases: an oracle section of a fixture, named by a set file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::oracle::{BENCH_FIXTURES, BENCH_ORACLE, CORE_FIXTURES, Section, parse_oracle};

/// The directory of set files.
pub const SETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/sets");

/// One fixture and direction to time: its oracle section and the fixture
/// root its relpath is under.
#[derive(Clone, Debug)]
pub struct Case {
    /// The fixture root.
    pub root: PathBuf,
    /// The oracle section, giving the fixture, the direction and the points.
    pub section: Section,
}

/// The cases of set `name`, `sets/<name>.txt`.
pub fn load_set(name: &str) -> Result<Vec<Case>, String> {
    load_set_file(&Path::new(SETS).join(format!("{name}.txt")))
}

/// The cases of a set file: lines `<oracle> <relpath> # reason`, where
/// `<oracle>` is `bench` for the bench fixtures' oracle or the name of one
/// of the core oracles without `.oracle`. Each relpath gives every section
/// its oracle has for it (forward and inverse).
pub fn load_set_file(path: &Path) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut oracles: HashMap<String, (PathBuf, Vec<Section>)> = HashMap::new();
    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(oracle), Some(relpath)) = (it.next(), it.next()) else {
            return Err(format!("{}: bad line {line:?}", path.display()));
        };
        if !oracles.contains_key(oracle) {
            let (root, file) = if oracle == "bench" {
                (
                    PathBuf::from(BENCH_FIXTURES),
                    PathBuf::from(BENCH_FIXTURES).join(BENCH_ORACLE),
                )
            } else {
                (
                    PathBuf::from(CORE_FIXTURES),
                    PathBuf::from(CORE_FIXTURES)
                        .join("oracle")
                        .join(format!("{oracle}.oracle")),
                )
            };
            let text =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            oracles.insert(oracle.to_string(), (root, parse_oracle(&text)?));
        }
        let (root, sections) = &oracles[oracle];
        let found: Vec<Case> = sections
            .iter()
            .filter(|s| s.relpath == relpath)
            .map(|s| Case {
                root: root.clone(),
                section: s.clone(),
            })
            .collect();
        if found.is_empty() {
            return Err(format!(
                "{}: {relpath} is not in the {oracle} oracle",
                path.display()
            ));
        }
        cases.extend(found);
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_set_covers_every_bench_section() {
        let cases: Vec<_> = load_set("bench")
            .unwrap()
            .into_iter()
            .filter(|c| c.root == Path::new(crate::oracle::BENCH_FIXTURES))
            .collect();
        let text = std::fs::read_to_string(format!(
            "{}/{}",
            crate::oracle::BENCH_FIXTURES,
            crate::oracle::BENCH_ORACLE
        ))
        .unwrap();
        assert_eq!(
            cases.len(),
            crate::oracle::parse_oracle(&text).unwrap().len()
        );
    }

    #[test]
    fn an_unknown_relpath_is_an_error() {
        let dir = std::env::temp_dir().join(format!("ast-bench-set-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bad.txt");
        std::fs::write(&file, "bench no_such.map # nothing\n").unwrap();
        assert!(load_set_file(&file).unwrap_err().contains("no_such.map"));
    }
}
