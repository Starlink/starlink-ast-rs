// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The full sweep: every section of every transform oracle, at two point
//! counts, on both engines.

use crate::cases::Case;
use crate::oracle::{BENCH_FIXTURES, BENCH_ORACLE, CORE_FIXTURES, CORE_ORACLES, parse_oracle};
use std::path::PathBuf;

/// The point counts the sweep times: one where per-call cost shows and one
/// where the per-point loop does.
pub const SWEEP_NPOINTS: [usize; 2] = [100, 100_000];

/// Every section of the core oracles and the bench oracle, as cases.
pub fn all_cases() -> Result<Vec<Case>, String> {
    let mut files: Vec<(PathBuf, PathBuf)> = CORE_ORACLES
        .iter()
        .map(|o| {
            (
                PathBuf::from(CORE_FIXTURES),
                PathBuf::from(CORE_FIXTURES).join("oracle").join(o),
            )
        })
        .collect();
    files.push((
        PathBuf::from(BENCH_FIXTURES),
        PathBuf::from(BENCH_FIXTURES).join(BENCH_ORACLE),
    ));
    let mut cases = Vec::new();
    for (root, file) in files {
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        for section in parse_oracle(&text)? {
            cases.push(Case {
                root: root.clone(),
                section,
            });
        }
    }
    Ok(cases)
}
