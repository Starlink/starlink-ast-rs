// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Transform one transform-oracle section twice and print the library calls
//! the second (steady-state) call makes, with the `call-counts` feature:
//!
//!     cargo run -p starlink-ast --features call-counts --example count_one -- \
//!         simplify/cap_interval_01.map forward
//!
//! The path is relative to `tests/fixtures`. A third argument names one oracle
//! file to search, and the Mapping file is then read from beside that file:
//!
//!     cargo run -p starlink-ast --features call-counts --example count_one -- \
//!         slamap_amp.map forward crates/ast-bench/fixtures/bench.oracle
//!
//! Set `AST_COUNT_BACKTRACE=<name>`
//! to print where the first call to `<name>` comes from.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (relpath, forward, oracle_path) = match args.as_slice() {
        [p, d] => (p.as_str(), d == "forward", None),
        [p, d, o] => (p.as_str(), d == "forward", Some(o.as_str())),
        _ => {
            eprintln!("usage: count_one <relpath> forward|inverse [oracle-file]");
            std::process::exit(2);
        }
    };
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let mut oracle = None;
    let files: Vec<String> = match oracle_path {
        Some(o) => vec![o.to_string()],
        None => ["simplify_fixtures", "headers", "framesets"]
            .iter()
            .map(|name| format!("{root}/oracle/{name}.oracle"))
            .collect(),
    };
    let mut dir = format!("{root}/");
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        if let Some(i) = text.find(&format!("[{relpath}  ")) {
            oracle = Some(text[i..].to_string());
            if oracle_path.is_some() {
                dir = format!(
                    "{}/",
                    std::path::Path::new(&file).parent().unwrap().display()
                );
            }
            break;
        }
    }
    let section = oracle.expect("section in an oracle");
    let want_dir = if forward {
        "dir=forward"
    } else {
        "dir=inverse"
    };
    let block = section
        .split("\n\n")
        .find(|b| b.starts_with(&format!("[{relpath}  ")) && b.contains(want_dir))
        .expect("section for that direction");
    let mut lines = block.lines();
    let header = lines.next().unwrap();
    let field = |k: &str| -> usize {
        header
            .split_whitespace()
            .find_map(|t| t.strip_prefix(k))
            .unwrap()
            .trim_end_matches(']')
            .parse()
            .unwrap()
    };
    let (nin, nout) = (field("nin="), field("nout="));
    let body: Vec<&str> = lines.collect();
    let rows: Vec<Vec<f64>> = body
        .iter()
        .map(|l| {
            l.split_whitespace()
                .take(nin)
                .map(|t| {
                    if t == "BAD" {
                        starlink_ast::constants::BAD_VALUE
                    } else {
                        t.parse().unwrap()
                    }
                })
                .collect()
        })
        .collect();
    let text = std::fs::read_to_string(format!("{dir}{relpath}")).unwrap();
    let obj = if relpath.ends_with(".head") {
        use starlink_ast::channel::Channel;
        starlink_ast::FitsChan::from_header(&text)
            .unwrap()
            .read()
            .unwrap()
            .unwrap()
    } else {
        starlink_ast::from_native_string(&text).unwrap()
    };
    let map = obj.as_mapping().expect("a Mapping");
    let pts = starlink_ast::coords::PointSet::from_rows(rows);
    let mut out = starlink_ast::coords::PointSet::zeros(pts.npoint(), nout);
    map.transform_into(pts.as_points(), forward, out.as_points_mut())
        .unwrap();
    starlink_ast::call_counts::reset();
    map.transform_into(pts.as_points(), forward, out.as_points_mut())
        .unwrap();
    for (k, v) in starlink_ast::call_counts::snapshot() {
        println!("{k}\t{v}");
    }
}
