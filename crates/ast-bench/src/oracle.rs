// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Read the committed transform oracles.
//!
//! The parser mirrors `crates/starlink-ast/tests/test_transform_oracle.rs`
//! (`Section`, `parse_double`, `parse_oracle`), which integration-test code
//! cannot share.

use starlink_ast::constants::BAD_VALUE;

/// The fixture tree mirrored from `ast_tester/fixtures`.
pub const CORE_FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../starlink-ast/tests/fixtures"
);

/// The fixtures written for benchmarking, one plain Mapping per class
/// (`tools/perf/gen_bench_fixtures.c`).
pub const BENCH_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");

/// The oracle of [`BENCH_FIXTURES`], in that directory.
pub const BENCH_ORACLE: &str = "bench.oracle";

/// The three transform oracles under `CORE_FIXTURES/oracle/`.
pub const CORE_ORACLES: [&str; 3] = [
    "simplify_fixtures.oracle",
    "headers.oracle",
    "framesets.oracle",
];

/// One `[relpath nin= nout= dir=]` section of an oracle.
#[derive(Clone, Debug)]
pub struct Section {
    /// Fixture path relative to the fixture root.
    pub relpath: String,
    /// Input coordinates per row.
    pub nin: usize,
    /// Output coordinates per row.
    pub nout: usize,
    /// Forward (true) or inverse transformation.
    pub forward: bool,
    /// Column-major: `inputs[axis][row]`, `axis in 0..nin`.
    pub inputs: Vec<Vec<f64>>,
    /// Column-major: `outputs[axis][row]`, `axis in 0..nout`.
    pub outputs: Vec<Vec<f64>>,
}

impl Section {
    /// Number of rows.
    pub fn npoint(&self) -> usize {
        self.inputs.first().map_or(0, Vec::len)
    }
}

/// Port of C `oracle_parse_double`: the `BAD` token maps to `BAD_VALUE`;
/// otherwise the whole token must parse as an `f64`.
fn parse_double(tok: &str) -> Option<f64> {
    if tok == "BAD" {
        return Some(BAD_VALUE);
    }
    tok.parse::<f64>().ok()
}

/// Parse a whole `.oracle` file. Section headers look like
/// `[<relpath>  nin=N nout=M dir=forward]`; data rows are `nin` inputs then
/// `nout` outputs. A malformed data row is a hard error (a truncated oracle
/// must not pass on reduced coverage), matching the C checker.
pub fn parse_oracle(text: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let inside = rest.strip_suffix(']').unwrap_or(rest);
            let mut it = inside.split_whitespace();
            let relpath = it.next().ok_or("empty section header")?.to_string();
            let mut nin = None;
            let mut nout = None;
            let mut forward = true;
            for tok in it {
                if let Some(v) = tok.strip_prefix("nin=") {
                    nin = v.parse().ok();
                } else if let Some(v) = tok.strip_prefix("nout=") {
                    nout = v.parse().ok();
                } else if let Some(v) = tok.strip_prefix("dir=") {
                    forward = v != "inverse";
                }
            }
            let nin = nin.ok_or_else(|| format!("bad section header: {line}"))?;
            let nout = nout.ok_or_else(|| format!("bad section header: {line}"))?;
            sections.push(Section {
                relpath,
                nin,
                nout,
                forward,
                inputs: vec![Vec::new(); nin],
                outputs: vec![Vec::new(); nout],
            });
            continue;
        }
        let cur = sections
            .last_mut()
            .ok_or("data row before any section header")?;
        let vals: Vec<f64> = line
            .split_whitespace()
            .map(|t| parse_double(t).ok_or_else(|| format!("unparseable token {t:?}")))
            .collect::<Result<_, _>>()?;
        if vals.len() != cur.nin + cur.nout {
            return Err(format!(
                "malformed data row in [{}]: got {} of {} columns",
                cur.relpath,
                vals.len(),
                cur.nin + cur.nout
            ));
        }
        for (axis, v) in vals.iter().take(cur.nin).enumerate() {
            cur.inputs[axis].push(*v);
        }
        for (axis, v) in vals.iter().skip(cur.nin).enumerate() {
            cur.outputs[axis].push(*v);
        }
    }
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_core_oracles_with_known_section_count() {
        let mut total = 0;
        for name in CORE_ORACLES {
            let text = std::fs::read_to_string(format!("{CORE_FIXTURES}/oracle/{name}")).unwrap();
            total += parse_oracle(&text).unwrap().len();
        }
        // The total `every_oracle_section_is_gated` asserts in starlink-ast.
        assert_eq!(total, 2261);
    }

    #[test]
    fn malformed_row_is_hard_error() {
        let text = "[a.map  nin=2 nout=2 dir=forward]\n1 2 3\n";
        assert!(parse_oracle(text).is_err());
    }
}
