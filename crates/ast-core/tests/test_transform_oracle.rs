//! Investigative transform-output oracle checker.
//!
//! Ported from the C `check_transform_oracle` / `transform_oracle_util`
//! (reference/starlink-ast `ast_tester/`, submodule commit 3a3c3e51). Reads
//! the committed `.oracle` files, reloads each named fixture through the
//! public Rust API, re-transforms the recorded inputs, and reports how far
//! the Rust outputs diverge from the C reference values. This harness never
//! regenerates the oracle: the C values are the frozen baseline.

// Constants and helpers below are consumed by later tasks in this oracle suite.
#![allow(dead_code)]

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::constants::BAD_VALUE;
use ast_core::fitschan::FitsChan;
use ast_core::frames::{Frame, FrameRef};
use ast_core::from_native_string;
use ast_core::mappings::Mapping;

/// Golden tolerance: live outputs vs recorded oracle values. Wide enough to
/// absorb cross-architecture libm differences (~5e-9 near singularities)
/// while still catching real algorithmic change (>= ~1e-6 relative).
const GOLDEN_RTOL: f64 = 1e-7;
const GOLDEN_ATOL: f64 = 1e-7;
/// Equivalence tolerance: live `.map` vs live `.simp`, same run/machine.
const EQUIV_RTOL: f64 = 1e-9;
const EQUIV_ATOL: f64 = 1e-9;
/// Round-trip accuracy: absolute pixel tolerance for GRID-domain corpora.
const RTRIP_RTOL: f64 = 1e-6;
const RTRIP_ATOL: f64 = 0.05;

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// Port of C `oracle_within_tol`: `|got-ref| <= atol + rtol*|ref|`.
/// `BAD_VALUE` matches only `BAD_VALUE`; any NaN never matches.
fn within_tol(got: f64, reference: f64, rtol: f64, atol: f64) -> bool {
    let got_bad = got == BAD_VALUE;
    let ref_bad = reference == BAD_VALUE;
    if got_bad || ref_bad {
        return got_bad && ref_bad;
    }
    if got.is_nan() || reference.is_nan() {
        return false;
    }
    (got - reference).abs() <= atol + rtol * reference.abs()
}

/// Port of C `oracle_within_tol_wrap`: as `within_tol`, but also accepts a
/// difference of one full turn (2*pi) — a longitude may emerge as 0 vs 2*pi
/// or +pi vs -pi across architectures. Angular axes only.
fn within_tol_wrap(got: f64, reference: f64, rtol: f64, atol: f64) -> bool {
    if within_tol(got, reference, rtol, atol) {
        return true;
    }
    if got == BAD_VALUE || reference == BAD_VALUE || got.is_nan() || reference.is_nan() {
        return false;
    }
    ((got - reference).abs() - TWO_PI).abs() <= atol + rtol * reference.abs()
}

/// One corpus entry: recorded inputs and outputs for one fixture+direction.
struct Section {
    relpath: String,
    nin: usize,
    nout: usize,
    forward: bool,
    /// Column-major: `inputs[axis][row]`, `axis in 0..nin`.
    inputs: Vec<Vec<f64>>,
    /// Column-major: `outputs[axis][row]`, `axis in 0..nout`.
    outputs: Vec<Vec<f64>>,
}

impl Section {
    fn npoint(&self) -> usize {
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
fn parse_oracle(text: &str) -> Result<Vec<Section>, String> {
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

use std::path::PathBuf;

/// A loaded fixture: the owned object plus whether it is a FrameSet (so the
/// checker knows to pull base/current frames for angular detection).
struct Loaded {
    obj: AnyObject,
    is_frameset: bool,
}

impl Loaded {
    /// The mapping under test. For a FrameSet this is the object itself
    /// (it implements `Mapping` as base->current); for a bare Mapping,
    /// itself; `None` for a non-Mapping object.
    fn mapping(&self) -> Option<&dyn Mapping> {
        self.obj.as_mapping()
    }

    /// The output frame for a given direction: current frame (forward) or
    /// base frame (inverse). `None` unless the fixture is a FrameSet.
    fn frame(&self, forward: bool) -> Option<&dyn Frame> {
        if !self.is_frameset {
            return None;
        }
        let AnyObject::FrameSet(fs) = &self.obj else {
            return None;
        };
        let which = if forward {
            FrameRef::Current
        } else {
            FrameRef::Base
        };
        fs.frame(which).ok()
    }
}

/// Load a fixture named by an oracle relpath. Dispatches by extension:
/// `.head` through a `FitsChan`, everything else through
/// `from_native_string`. Returns `Ok(None)` for a resolved-but-non-Mapping
/// object (KeyMap, non-Region STC), `Err` for a resolved-but-unreadable one.
fn load_fixture(relpath: &str) -> Result<Option<Loaded>, String> {
    let path = match resolve_fixture(relpath) {
        Some(p) => p,
        None => return Err(format!("fixture not found: {relpath}")),
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {relpath}: {e}"))?;

    let obj = if relpath.ends_with(".head") {
        let mut fc =
            FitsChan::from_header(&text).map_err(|e| format!("from_header {relpath}: {e}"))?;
        match fc.read().map_err(|e| format!("read {relpath}: {e}"))? {
            Some(o) => o,
            None => return Err(format!("FitsChan read returned None: {relpath}")),
        }
    } else {
        from_native_string(&text).map_err(|e| format!("from_native_string {relpath}: {e}"))?
    };

    let is_frameset = matches!(obj, AnyObject::FrameSet(_));
    if obj.as_mapping().is_none() {
        return Ok(None); // resolved but not a Mapping — legitimately skipped
    }
    Ok(Some(Loaded { obj, is_frameset }))
}

/// Resolve an oracle relpath (rooted at the C `ast_tester/` dir) to the
/// on-disk Rust fixture. Searches a fixed, ordered list of the directories
/// the corpus was vendored into, trying the full relpath then the basename
/// in each. `native/generated/` is intentionally excluded: it holds
/// Rust-produced dumps that differ from the C-provenance source.
fn resolve_fixture(relpath: &str) -> Option<PathBuf> {
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    const DIRS: [&str; 8] = [
        "wcsconv/inputs",
        "native/reference",
        "native/reference/simplify_fixtures",
        "native/reference/simplify_fixtures_unsupported/xphmap",
        "native/reference/simplify_fixtures_unsupported/intramap",
        "native/reference/simplify_fixtures_unsupported/stcresourceprofile",
        "wcsconv/expected",
        "fits/headers",
    ];
    let basename = relpath.rsplit('/').next().unwrap_or(relpath);
    for dir in DIRS {
        for candidate in [
            format!("{ROOT}/{dir}/{relpath}"),
            format!("{ROOT}/{dir}/{basename}"),
        ] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn resolves_every_oracle_relpath() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
        let mut missing = Vec::new();
        for name in [
            "simplify_fixtures.oracle",
            "headers.oracle",
            "framesets.oracle",
        ] {
            let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
            for s in parse_oracle(&text).unwrap() {
                if resolve_fixture(&s.relpath).is_none() {
                    missing.push(s.relpath);
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "unresolved fixtures: {missing:?}");
    }

    #[test]
    fn never_resolves_into_generated() {
        // lsst.ast exists in native/generated (Rust output) and in
        // native/reference + wcsconv/inputs (C source). The resolver must
        // pick a non-generated copy.
        let p = resolve_fixture("lsst.ast").expect("lsst.ast resolves");
        assert!(!p.to_string_lossy().contains("native/generated"));
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parses_header_bad_token_and_direction() {
        let text = "# header\n\n\
                    [foo.map  nin=2 nout=1 dir=forward]\n\
                    \x201 2  3\n\
                    \x204 5  BAD\n\n\
                    [bar.head  nin=1 nout=1 dir=inverse]\n\
                    \x207  8\n";
        let secs = parse_oracle(text).expect("parse ok");
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].relpath, "foo.map");
        assert_eq!((secs[0].nin, secs[0].nout, secs[0].forward), (2, 1, true));
        assert_eq!(secs[0].npoint(), 2);
        assert_eq!(secs[0].inputs[0][0], 1.0);
        assert_eq!(secs[0].inputs[1][1], 5.0);
        assert_eq!(secs[0].outputs[0][0], 3.0);
        assert_eq!(secs[0].outputs[0][1], BAD_VALUE);
        assert!(!secs[1].forward);
    }

    #[test]
    fn malformed_row_is_hard_error() {
        let text = "[foo.map  nin=2 nout=1 dir=forward]\n  1 2\n";
        assert!(parse_oracle(text).is_err());
    }

    #[test]
    fn parses_all_three_committed_oracles() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
        let mut total = 0;
        for name in [
            "simplify_fixtures.oracle",
            "headers.oracle",
            "framesets.oracle",
        ] {
            let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
            total += parse_oracle(&text).expect("committed oracle parses").len();
        }
        assert_eq!(total, 1606);
    }
}

#[cfg(test)]
mod primitive_tests {
    use super::*;

    #[test]
    fn within_tol_matches_c_semantics() {
        assert!(within_tol(1.0, 1.0, 0.0, 0.0));
        assert!(within_tol(1.0 + 4e-16, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(1.0001, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol(5e-13, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol(BAD_VALUE, BAD_VALUE, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(BAD_VALUE, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(1.0, BAD_VALUE, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol(f64::NAN, 1.0, 1e9, 1e9));
    }

    #[test]
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    fn within_tol_wrap_accepts_full_turn_and_ci_pairs() {
        assert!(within_tol_wrap(1.0, 1.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(TWO_PI, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(0.0, TWO_PI, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(within_tol_wrap(
            std::f64::consts::PI,
            -std::f64::consts::PI,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(!within_tol_wrap(1.0, 2.0, GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(!within_tol_wrap(BAD_VALUE, 0.0, GOLDEN_RTOL, GOLDEN_ATOL));
        // Exact arm64-ref vs x86-64-got pairs observed in C CI.
        assert!(within_tol_wrap(
            5.0999352672935785e-17,
            6.2831853071795862,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(within_tol_wrap(
            1.9478449064398693e-20,
            6.2831853071795862,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(within_tol_wrap(
            3.1415926535897931,
            -3.1415926535897931,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
        assert!(within_tol_wrap(
            1.0,
            1.0000000047018207,
            GOLDEN_RTOL,
            GOLDEN_ATOL
        ));
    }
}

#[cfg(test)]
mod loader_tests {
    use super::*;

    #[test]
    fn loads_native_mapping() {
        let l = load_fixture("simplify_fixtures/matrix_diagonal_to_zoom.map")
            .expect("no error")
            .expect("is a mapping");
        let m = l.mapping().expect("mapping");
        assert_eq!(m.nin(), 2);
        assert!(l.frame(true).is_none(), "bare mapping has no frame");
    }

    #[test]
    fn loads_head_frameset_with_frames() {
        let l = load_fixture("cobe.head")
            .expect("no error")
            .expect("is a mapping");
        assert!(l.is_frameset);
        let m = l.mapping().expect("mapping");
        assert_eq!(m.nin(), 2);
        assert!(l.frame(true).is_some(), "forward -> current frame");
        assert!(l.frame(false).is_some(), "inverse -> base frame");
    }
}
