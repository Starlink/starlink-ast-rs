// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Every input the fuzz targets in `fuzz/` found a fault with is read again
//! through the same entry point, and must finish within ten seconds without
//! panicking.
//!
//! The inputs are in `tests/regressions/<target>/`, one file each, as
//! `cargo fuzz tmin` left them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use crate::fuzz_entry::{fitschan_read_write, mocchan_read, native_read};

/// The inputs found by one fuzz target, sorted by name.
fn inputs(target: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/regressions")
        .join(target);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n != ".gitkeep"))
        .collect();
    paths.sort();
    paths
}

/// Run `entry` on each input of `target`, failing for any that panics or
/// takes more than ten seconds.
fn replay(target: &str, entry: fn(&[u8])) {
    let mut problems = Vec::new();
    for path in inputs(target) {
        let data = fs::read(&path).unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            entry(&data);
            let _ = tx.send(());
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {
                problems.push(format!("{}: took more than 10 s", path.display()))
            }
            Err(RecvTimeoutError::Disconnected) => {
                problems.push(format!("{}: panicked", path.display()))
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn native_read_findings() {
    replay("native_read", native_read);
}

#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn fitschan_read_write_findings() {
    replay("fitschan_read_write", fitschan_read_write);
}

#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn mocchan_read_findings() {
    replay("mocchan_read", mocchan_read);
}
