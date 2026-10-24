// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Every test file in `tests/` is a module of exactly one test program.
//!
//! The package sets `autotests = false` and builds the files as modules of
//! the area programs `tests/<area>/main.rs`, so a file no program names is
//! never compiled and its tests never run. This fails when that happens, or
//! when a program names a file that does not exist or is named twice.
//! Helpers in subdirectories, such as `common/mod.rs`, are shared and may be
//! named by several programs.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn every_test_file_is_in_one_area_program() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut named: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in fs::read_dir(&tests).unwrap() {
        let dir = entry.unwrap().path();
        let main = dir.join("main.rs");
        if !main.is_file() {
            continue;
        }
        let area = dir.file_name().unwrap().to_string_lossy().into_owned();
        for line in fs::read_to_string(&main).unwrap().lines() {
            if let Some(rest) = line.strip_prefix("#[path = \"../")
                && let Some(file) = rest.strip_suffix("\"]")
            {
                named
                    .entry(file.to_string())
                    .or_default()
                    .push(area.clone());
            }
        }
    }

    let mut problems = Vec::new();
    for entry in fs::read_dir(&tests).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            match named.remove(&file) {
                None => problems.push(format!("{file} is in no area program")),
                Some(areas) if areas.len() > 1 => {
                    problems.push(format!("{file} is in {}", areas.join(", ")))
                }
                Some(_) => {}
            }
        }
    }
    for (file, areas) in named {
        if tests.join(&file).is_file() {
            continue;
        }
        problems.push(format!(
            "{} names {file}, which does not exist",
            areas.join(", ")
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
