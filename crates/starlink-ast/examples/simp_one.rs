//! Simplify one native-format Mapping fixture and print the result.
//!
//! Usage: `cargo run -p starlink-ast --example simp_one -- <path-to-.map>`
//!
//! A FrameSet input is reduced to its base→current Mapping first, matching what
//! the simplify regression harness feeds to `simplify`.

use starlink_ast::{AnyObject, from_native_string};

/// Print the `ast::simplify` events the `tracing` feature emits, filtered by
/// `RUST_LOG` (default `ast::simplify=debug`, which gives the nomination and
/// per-class-reduction lines without the higher-volume decomposition ones).
///
/// A library never installs a subscriber, so an example that wants the events
/// has to do it.
#[cfg(feature = "tracing")]
fn install_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ast::simplify=debug"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

fn main() {
    #[cfg(feature = "tracing")]
    install_tracing();

    let path = std::env::args().nth(1).expect("usage: simp_one <file.map>");
    let text = std::fs::read_to_string(&path).expect("read fixture");
    let obj = from_native_string(&text).expect("parse fixture");
    let mapping = match obj {
        AnyObject::FrameSet(ref fs) => fs
            .mapping(
                starlink_ast::frames::FrameRef::Base,
                starlink_ast::frames::FrameRef::Current,
            )
            .expect("frameset mapping"),
        other => other.into_mapping().expect("mapping"),
    };
    let simplified = mapping.simplify().expect("simplify");
    match AnyObject::from_mapping(simplified.as_ref()) {
        Some(obj) => obj.show().expect("serialize"),
        None => println!("(result class is not one this example can serialize)"),
    }
}
