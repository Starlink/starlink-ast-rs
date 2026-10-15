//! Transform one transform-oracle section twice and print the library calls
//! the second (steady-state) call makes, with the `call-counts` feature:
//!
//!     cargo run -p ast-core --features call-counts --example count_one -- \
//!         simplify/cap_interval_01.map forward
//!
//! The path is relative to `tests/fixtures`. Set `AST_COUNT_BACKTRACE=<name>`
//! to print where the first call to `<name>` comes from.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (relpath, forward) = match args.as_slice() {
        [p, d] => (p.as_str(), d == "forward"),
        _ => {
            eprintln!("usage: count_one <relpath> forward|inverse");
            std::process::exit(2);
        }
    };
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let mut oracle = None;
    for name in ["simplify_fixtures", "headers", "framesets"] {
        let text = std::fs::read_to_string(format!("{root}/oracle/{name}.oracle")).unwrap();
        if let Some(i) = text.find(&format!("[{relpath}  ")) {
            oracle = Some(text[i..].to_string());
            break;
        }
    }
    let section = oracle.expect("section in an oracle");
    let mut lines = section.lines();
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
    let want_dir = if forward {
        "dir=forward"
    } else {
        "dir=inverse"
    };
    let body: Vec<&str> = section
        .split("\n\n")
        .find(|b| b.starts_with(&format!("[{relpath}  ")) && b.contains(want_dir))
        .expect("section for that direction")
        .lines()
        .skip(1)
        .collect();
    let rows: Vec<Vec<f64>> = body
        .iter()
        .map(|l| {
            l.split_whitespace()
                .take(nin)
                .map(|t| {
                    if t == "BAD" {
                        ast_core::constants::BAD_VALUE
                    } else {
                        t.parse().unwrap()
                    }
                })
                .collect()
        })
        .collect();
    let text = std::fs::read_to_string(format!("{root}/{relpath}")).unwrap();
    let obj = if relpath.ends_with(".head") {
        use ast_core::channel::Channel;
        ast_core::FitsChan::from_header(&text)
            .unwrap()
            .read()
            .unwrap()
            .unwrap()
    } else {
        ast_core::from_native_string(&text).unwrap()
    };
    let map = obj.as_mapping().expect("a Mapping");
    let pts = ast_core::coords::PointSet::from_rows(rows);
    let mut out = ast_core::coords::PointSet::zeros(pts.npoint(), nout);
    map.transform_into(&pts, forward, &mut out).unwrap();
    ast_core::call_counts::reset();
    map.transform_into(&pts, forward, &mut out).unwrap();
    for (k, v) in ast_core::call_counts::snapshot() {
        println!("{k}\t{v}");
    }
}
