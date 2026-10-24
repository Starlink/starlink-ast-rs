//! Transform one transform-oracle section twice and print the library calls
//! the second (steady-state) call makes, with the `call-counts` feature:
//!
//!     cargo run -p starlink-ast --features call-counts --example count_one -- \
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
                        starlink_ast::constants::BAD_VALUE
                    } else {
                        t.parse().unwrap()
                    }
                })
                .collect()
        })
        .collect();
    let text = std::fs::read_to_string(format!("{root}/{relpath}")).unwrap();
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
