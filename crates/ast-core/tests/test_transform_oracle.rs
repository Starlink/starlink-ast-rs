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
use ast_core::coords::PointSet;
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

/// Port of C `axis_is_cyclic`: is output `axis` (0-based slice index) of
/// `frame` 2*pi-periodic?  Normalizes a test point and a copy shifted by
/// 2*pi on that axis; the axis is cyclic iff the normalized values agree to
/// within 1e-6.
fn axis_is_cyclic(frame: &dyn Frame, axis: usize) -> bool {
    let naxes = frame.naxes();
    if axis >= naxes {
        return false;
    }
    let mut p = vec![0.1_f64; naxes];
    let mut q = vec![0.1_f64; naxes];
    q[axis] += TWO_PI;
    if frame.norm(&mut p).is_err() || frame.norm(&mut q).is_err() {
        return false;
    }
    (p[axis] - q[axis]).abs() < 1e-6
}

/// One angular flag per output axis (`section.nout` long).  For a bare
/// Mapping (no frame) every axis is treated as angular — the C fallback.
fn angular_flags(loaded: &Loaded, section: &Section) -> Vec<bool> {
    match loaded.frame(section.forward) {
        Some(frame) => (0..section.nout)
            .map(|a| axis_is_cyclic(frame, a))
            .collect(),
        None => vec![true; section.nout],
    }
}

/// Run the recorded inputs through the live mapping.  Returns column-major
/// `[nout][npoint]`.  Returns `Err` on arity mismatch (a structural fixture
/// change) or on a transform error.
fn transform_section(map: &dyn Mapping, s: &Section) -> Result<Vec<Vec<f64>>, String> {
    let (want_in, want_out) = if s.forward {
        (s.nin, s.nout)
    } else {
        (s.nout, s.nin)
    };
    if map.nin() != want_in || map.nout() != want_out {
        return Err(format!(
            "arity: fixture nin/nout {}/{} != mapping {}/{}",
            want_in,
            want_out,
            map.nin(),
            map.nout()
        ));
    }
    let npoint = s.npoint();
    let rows: Vec<Vec<f64>> = (0..npoint)
        .map(|r| (0..s.nin).map(|a| s.inputs[a][r]).collect())
        .collect();
    let pts = PointSet::from_rows(rows);
    let out = map
        .transform(&pts, s.forward)
        .map_err(|e| format!("transform: {e}"))?;
    let cols: Vec<Vec<f64>> = (0..s.nout)
        .map(|a| (0..out.npoint()).map(|r| out.point(r)[a]).collect())
        .collect();
    Ok(cols)
}

use std::collections::HashMap;

#[derive(Clone, Copy)]
struct RtripRule {
    off: bool,
    rtol: f64,
    atol: f64,
}
#[derive(Clone, Copy, Default)]
struct GoldenRule {
    fwd_off: bool,
    inv_off: bool,
}

/// Parsed `transform_oracle_overrides.txt`. Round-trip and golden rules are
/// looked up independently, matching the C checker.
#[derive(Default)]
struct Overrides {
    rtrip: HashMap<String, RtripRule>,
    golden: HashMap<String, GoldenRule>,
}

fn parse_overrides(text: &str) -> Overrides {
    let mut o = Overrides::default();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(rel), Some(what)) = (it.next(), it.next()) else {
            continue;
        };
        if let Some(kind) = what.strip_prefix("golden") {
            let entry = o.golden.entry(rel.to_string()).or_default();
            let (fwd, inv) = match kind {
                "" => (true, true), // "golden"
                "-fwd" => (true, false),
                "-inv" => (false, true),
                _ => (false, false),
            };
            entry.fwd_off |= fwd;
            entry.inv_off |= inv;
        } else if what == "off" {
            o.rtrip.insert(
                rel.to_string(),
                RtripRule {
                    off: true,
                    rtol: 0.0,
                    atol: 0.0,
                },
            );
        } else {
            // "<rtol> <atol>" loosened tolerances (fall back to defaults).
            let rtol = what.parse().unwrap_or(RTRIP_RTOL);
            let atol = it.next().and_then(|t| t.parse().ok()).unwrap_or(RTRIP_ATOL);
            o.rtrip.insert(
                rel.to_string(),
                RtripRule {
                    off: false,
                    rtol,
                    atol,
                },
            );
        }
    }
    o
}

impl Overrides {
    /// `None` = round-trip disabled for this fixture; `Some((rtol, atol))`
    /// = enabled with these tolerances.
    fn rtrip(&self, relpath: &str) -> Option<(f64, f64)> {
        match self.rtrip.get(relpath) {
            Some(r) if r.off => None,
            Some(r) => Some((r.rtol, r.atol)),
            None => Some((RTRIP_RTOL, RTRIP_ATOL)),
        }
    }

    fn golden_off(&self, relpath: &str, forward: bool) -> bool {
        self.golden
            .get(relpath)
            .is_some_and(|g| if forward { g.fwd_off } else { g.inv_off })
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;

    #[test]
    fn parses_independent_rtrip_and_golden_lines() {
        let text = "# comment\n\
                    rtoff.head off\n\
                    loose.head 2e-3 4e-2\n\
                    noisy.map golden-inv off\n\
                    both.map golden off\n\
                    fwdonly.map golden-fwd off\n";
        let o = parse_overrides(text);
        assert_eq!(o.rtrip("rtoff.head"), None);
        assert_eq!(o.rtrip("loose.head"), Some((2e-3, 4e-2)));
        // golden-only fixture keeps default round-trip enabled
        assert_eq!(o.rtrip("noisy.map"), Some((RTRIP_RTOL, RTRIP_ATOL)));
        assert!(!o.golden_off("rtoff.head", true) && !o.golden_off("rtoff.head", false));
        assert!(!o.golden_off("noisy.map", true) && o.golden_off("noisy.map", false));
        assert!(o.golden_off("both.map", true) && o.golden_off("both.map", false));
        assert!(o.golden_off("fwdonly.map", true) && !o.golden_off("fwdonly.map", false));
    }

    #[test]
    fn parses_committed_overrides_file() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/oracle/transform_oracle_overrides.txt"
        );
        let o = parse_overrides(&std::fs::read_to_string(path).unwrap());
        // tnx-cheb.head has round-trip disabled.
        assert_eq!(o.rtrip("tnx-cheb.head"), None);
        // neg_unitnormmap_inv_fwd_diffcentre.map skips golden inverse only.
        let rel = "simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map";
        assert!(o.golden_off(rel, false) && !o.golden_off(rel, true));
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn skyframe_longitude_is_cyclic_pixel_is_not() {
        let l = load_fixture("cobe.head").unwrap().unwrap();
        let flags = angular_flags(
            &l,
            &Section {
                relpath: "cobe.head".into(),
                nin: 2,
                nout: 2,
                forward: true,
                inputs: vec![vec![], vec![]],
                outputs: vec![vec![], vec![]],
            },
        );
        assert!(
            flags.iter().any(|&f| f),
            "at least one axis cyclic (sky lon)"
        );
    }

    #[test]
    fn bare_mapping_all_axes_angular() {
        let l = load_fixture("simplify_fixtures/matrix_diagonal_to_zoom.map")
            .unwrap()
            .unwrap();
        let s = Section {
            relpath: "x.map".into(),
            nin: 2,
            nout: 2,
            forward: true,
            inputs: vec![vec![], vec![]],
            outputs: vec![vec![], vec![]],
        };
        assert_eq!(angular_flags(&l, &s), vec![true, true]);
    }

    #[test]
    fn transform_reproduces_oracle_shape() {
        let l = load_fixture("simplify_fixtures/matrix_diagonal_to_zoom.map")
            .unwrap()
            .unwrap();
        let s = Section {
            relpath: "x.map".into(),
            nin: 2,
            nout: 2,
            forward: true,
            inputs: vec![vec![1.0, 2.0], vec![3.0, 4.0]],
            outputs: vec![vec![0.0, 0.0], vec![0.0, 0.0]],
        };
        let out = transform_section(l.mapping().unwrap(), &s).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 2);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckKind {
    GoldenFwd,
    GoldenInv,
    Equivalence,
    RoundTrip,
}

#[derive(Clone, Copy, Debug)]
struct AxisMiss {
    check: CheckKind,
    row: usize,
    axis: usize,
    reference: f64,
    got: f64,
    rel: f64,
}

fn rel_diff(got: f64, reference: f64) -> f64 {
    let d = (got - reference).abs();
    if reference != 0.0 {
        d / reference.abs()
    } else {
        d
    }
}

/// Wrap-aware on angular axes, plain otherwise (C `axis_match`).
fn axis_match(got: f64, reference: f64, rtol: f64, atol: f64, angular: bool) -> bool {
    if angular {
        within_tol_wrap(got, reference, rtol, atol)
    } else {
        within_tol(got, reference, rtol, atol)
    }
}

/// Golden check A/B: live output vs recorded output, per axis.
fn compare_golden(
    check: CheckKind,
    live: &[Vec<f64>],
    recorded: &[Vec<f64>],
    angular: &[bool],
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = recorded.first().map_or(0, Vec::len);
    for row in 0..npoint {
        for (axis, rec_col) in recorded.iter().enumerate() {
            let (got, reference) = (live[axis][row], rec_col[row]);
            if !axis_match(got, reference, GOLDEN_RTOL, GOLDEN_ATOL, angular[axis]) {
                misses.push(AxisMiss {
                    check,
                    row,
                    axis,
                    reference,
                    got,
                    rel: rel_diff(got, reference),
                });
            }
        }
    }
    misses
}

/// Equivalence check C, gated on stored agreement: compare live `.map` vs
/// live `.simp` only where the recorded values already agree.
fn compare_equiv(
    map_live: &[Vec<f64>],
    simp_live: &[Vec<f64>],
    map_ref: &[Vec<f64>],
    simp_ref: &[Vec<f64>],
    angular: &[bool],
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = map_ref.first().map_or(0, Vec::len);
    for row in 0..npoint {
        for axis in 0..map_ref.len() {
            if !axis_match(
                map_ref[axis][row],
                simp_ref[axis][row],
                EQUIV_RTOL,
                EQUIV_ATOL,
                angular[axis],
            ) {
                continue; // out of shared domain at generation time
            }
            let (a, b) = (map_live[axis][row], simp_live[axis][row]);
            if !axis_match(a, b, EQUIV_RTOL, EQUIV_ATOL, angular[axis]) {
                misses.push(AxisMiss {
                    check: CheckKind::Equivalence,
                    row,
                    axis,
                    reference: b,
                    got: a,
                    rel: rel_diff(a, b),
                });
            }
        }
    }
    misses
}

/// Round-trip accuracy (GRID corpora): inverse(forward(P)) ~= P, BAD-skipping
/// and alias-aware (a different recovered pixel is accepted if it
/// forward-maps to the same sky). `inv_live` is the live output of the
/// inverse section (i.e. inverse(forward(P))).
fn compare_rtrip(
    map: &dyn Mapping,
    fwd: &Section,
    inv_live: &[Vec<f64>],
    rtol: f64,
    atol: f64,
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = fwd.npoint();
    // Forward-transform the recovered pixels to detect aliased preimages.
    let recovered = Section {
        relpath: fwd.relpath.clone(),
        nin: fwd.nin,
        nout: fwd.nin,
        forward: true,
        inputs: inv_live.to_vec(),
        outputs: Vec::new(),
    };
    let sky2 = transform_section(map, &recovered).ok();

    for row in 0..npoint {
        // Skip points whose forward output was BAD (no inverse).
        if (0..fwd.nout).any(|b| fwd.outputs[b][row] == BAD_VALUE) {
            continue;
        }
        let pixel_ok =
            (0..fwd.nin).all(|a| within_tol(inv_live[a][row], fwd.inputs[a][row], rtol, atol));
        if pixel_ok {
            continue;
        }
        // Different pixel: accept if it forward-maps to the same sky.
        if let Some(sky2) = &sky2 {
            let alias = (0..fwd.nout).all(|b| {
                within_tol_wrap(sky2[b][row], fwd.outputs[b][row], GOLDEN_RTOL, GOLDEN_ATOL)
            });
            if alias {
                continue;
            }
        }
        for (a, (inv_col, inp_col)) in inv_live.iter().zip(fwd.inputs.iter()).enumerate() {
            let (got, reference) = (inv_col[row], inp_col[row]);
            if !within_tol(got, reference, rtol, atol) {
                misses.push(AxisMiss {
                    check: CheckKind::RoundTrip,
                    row,
                    axis: a,
                    reference,
                    got,
                    rel: rel_diff(got, reference),
                });
            }
        }
    }
    misses
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn golden_flags_out_of_tolerance_axis() {
        let live = vec![vec![1.0, 2.0]];
        let recorded = vec![vec![1.0, 2.5]];
        let misses = compare_golden(CheckKind::GoldenFwd, &live, &recorded, &[false]);
        assert_eq!(misses.len(), 1);
        assert_eq!((misses[0].row, misses[0].axis), (1, 0));
    }

    #[test]
    fn equiv_skips_points_where_recorded_disagree() {
        // recorded map/simp disagree at row 0 (out of shared domain) but the
        // live values also disagree there — must be skipped. Row 1 agrees in
        // the record but diverges live — must be flagged.
        let map_ref = vec![vec![10.0, 1.0]];
        let simp_ref = vec![vec![20.0, 1.0]];
        let map_live = vec![vec![10.0, 1.0]];
        let simp_live = vec![vec![20.0, 9.0]];
        let misses = compare_equiv(&map_live, &simp_live, &map_ref, &simp_ref, &[false]);
        assert_eq!(misses.len(), 1);
        assert_eq!(misses[0].row, 1);
    }
}

enum Outcome {
    Pass,
    LoadFail(String),
    Skipped(String),
    SkippedByOverride(String),
    Arity(String),
    Mismatch(Vec<AxisMiss>),
}

struct SectionReport {
    relpath: String,
    forward: bool,
    outcome: Outcome,
}

/// Does relpath end with the given extension?
fn has_ext(relpath: &str, ext: &str) -> bool {
    relpath.rsplit('.').next().map(|e| format!(".{e}")) == Some(ext.to_string())
}

/// Stem (path minus final extension).
fn path_stem(relpath: &str) -> &str {
    match relpath.rfind('.') {
        Some(i) => &relpath[..i],
        None => relpath,
    }
}

/// Run every section of one oracle file. Never asserts; records an outcome
/// per section. Section pairing mirrors the C generator's emission order.
fn run_oracle(oracle_text: &str, overrides: &Overrides) -> Vec<SectionReport> {
    let sections = parse_oracle(oracle_text).expect("oracle parses");
    let mut reports = Vec::with_capacity(sections.len());

    // One-slot caches keyed on the C emission order.
    let mut prev_fwd: Option<Section> = None; // for round-trip
    let mut prev_map: Option<(Section, Vec<Vec<f64>>)> = None; // (.map fwd, its live outputs) for equiv

    for s in sections {
        let loaded = match load_fixture(&s.relpath) {
            Ok(Some(l)) => l,
            Ok(None) => {
                reports.push(SectionReport {
                    relpath: s.relpath.clone(),
                    forward: s.forward,
                    outcome: Outcome::Skipped("not a mapping".into()),
                });
                continue;
            }
            Err(e) => {
                reports.push(SectionReport {
                    relpath: s.relpath.clone(),
                    forward: s.forward,
                    outcome: Outcome::LoadFail(e),
                });
                continue;
            }
        };
        let map = loaded.mapping().expect("load_fixture guaranteed a mapping");
        let live = match transform_section(map, &s) {
            Ok(v) => v,
            Err(e) => {
                reports.push(SectionReport {
                    relpath: s.relpath.clone(),
                    forward: s.forward,
                    outcome: Outcome::Arity(e),
                });
                continue;
            }
        };
        let angular = angular_flags(&loaded, &s);
        let mut misses: Vec<AxisMiss> = Vec::new();
        let mut skipped_by_override = false;

        // Golden check (fwd or inv), unless disabled by an override.
        if overrides.golden_off(&s.relpath, s.forward) {
            skipped_by_override = true;
        } else {
            let kind = if s.forward {
                CheckKind::GoldenFwd
            } else {
                CheckKind::GoldenInv
            };
            misses.extend(compare_golden(kind, &live, &s.outputs, &angular));
        }

        // Round-trip: an inverse section of a GRID fixture paired with the
        // immediately-preceding forward section of the same fixture.
        let grid = has_ext(&s.relpath, ".head") || has_ext(&s.relpath, ".ast");
        if !s.forward && grid {
            if let Some(fwd) = prev_fwd
                .as_ref()
                .filter(|f| f.relpath == s.relpath && f.nin == s.nout && f.npoint() == s.npoint())
            {
                if let Some((rtol, atol)) = overrides.rtrip(&s.relpath) {
                    misses.extend(compare_rtrip(map, fwd, &live, rtol, atol));
                }
            }
        }

        // Equivalence: a .simp forward whose stem matches the cached .map fwd.
        if s.forward && has_ext(&s.relpath, ".simp") {
            if let Some((mmap, mlive)) = prev_map.as_ref().filter(|(m, _)| {
                m.nout == s.nout
                    && m.npoint() == s.npoint()
                    && path_stem(&m.relpath) == path_stem(&s.relpath)
            }) {
                misses.extend(compare_equiv(
                    mlive,
                    &live,
                    &mmap.outputs,
                    &s.outputs,
                    &angular,
                ));
            }
        }

        // Update caches (before moving `s` into the report).
        if s.forward {
            prev_fwd = Some(clone_section_meta(&s));
        }
        if s.forward && has_ext(&s.relpath, ".map") {
            prev_map = Some((clone_section_meta(&s), live.clone()));
        }

        let outcome = if skipped_by_override && misses.is_empty() {
            Outcome::SkippedByOverride(format!("golden-{}", if s.forward { "fwd" } else { "inv" }))
        } else if misses.is_empty() {
            Outcome::Pass
        } else {
            Outcome::Mismatch(misses)
        };
        reports.push(SectionReport {
            relpath: s.relpath,
            forward: s.forward,
            outcome,
        });
    }
    reports
}

/// Cheap copy of a section including its data columns (the caches need the
/// recorded inputs/outputs for later pairing).
fn clone_section_meta(s: &Section) -> Section {
    Section {
        relpath: s.relpath.clone(),
        nin: s.nin,
        nout: s.nout,
        forward: s.forward,
        inputs: s.inputs.clone(),
        outputs: s.outputs.clone(),
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;

    #[test]
    fn runs_framesets_oracle_without_panicking() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
        let oracle = std::fs::read_to_string(format!("{dir}/framesets.oracle")).unwrap();
        let ov = parse_overrides(
            &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt")).unwrap(),
        );
        let reports = run_oracle(&oracle, &ov);
        assert_eq!(reports.len(), 20, "framesets.oracle has 20 sections");
    }
}
