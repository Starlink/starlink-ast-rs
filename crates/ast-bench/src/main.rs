//! `ast-bench` command-line entry point.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") => print!("{USAGE}"),
        Some(other) => {
            eprintln!("unknown command {other:?}\n{USAGE}");
            std::process::exit(2);
        }
    }
}

const USAGE: &str = "usage: ast-bench <command> [options]
commands:
  run --set <name> [--engine rust|c|both] [--save] [--format table|tsv]
  compare --set <name> [--base <git-ref>]
  sweep
  snapshot --set <name>
";
