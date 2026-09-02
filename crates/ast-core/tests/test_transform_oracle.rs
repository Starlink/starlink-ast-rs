//! Investigative transform-output oracle checker.
//!
//! Ported from the C `check_transform_oracle` / `transform_oracle_util`
//! (reference/starlink-ast `ast_tester/`, submodule commit 3a3c3e51). Reads
//! the committed `.oracle` files, reloads each named fixture through the
//! public Rust API, re-transforms the recorded inputs, and reports how far
//! the Rust outputs diverge from the C reference values. This harness never
//! regenerates the oracle: the C values are the frozen baseline.

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

/// A loaded fixture, or the reason the corpus cannot transform it.
enum LoadOutcome {
    /// Boxed to keep the two variants a similar size: an `AnyObject` is
    /// several hundred bytes, a reason is a fat pointer.
    Transformable(Box<Loaded>),
    /// Resolved and read, but has no transform to compare against the
    /// oracle. The string is the reason, reported verbatim as the skip.
    Untransformable(&'static str),
}
use LoadOutcome::{Transformable, Untransformable};

impl LoadOutcome {
    /// The loaded fixture, panicking with the recorded reason otherwise.
    /// For tests that name a fixture known to be transformable.
    fn expect_transformable(self) -> Loaded {
        match self {
            Transformable(l) => *l,
            Untransformable(reason) => panic!("fixture is not transformable: {reason}"),
        }
    }
}

/// Load a fixture named by an oracle relpath. Dispatches by extension:
/// `.head` through a `FitsChan`, everything else through
/// `from_native_string`.
///
/// `Err` means resolved but unreadable. `Ok(Untransformable(reason))` means
/// resolved to an object this corpus cannot transform, and names which case
/// it is: a bare Frame, or an object that is not a Mapping in either
/// library (KeyMap, non-Region STC).
///
/// A bare Frame is a Mapping in C, so C's `oracle_load_mapping`
/// (`transform_oracle_util.c:136`) accepts one and the generator recorded
/// real sections for it — `specflux.ast` is a CmpFrame whose recorded
/// output columns equal its inputs. The Rust port makes `Frame` and
/// `Mapping` sibling traits, so a Frame has no transform to call, and these
/// sections are skipped rather than checked.
///
/// Nothing is lost: used as a Mapping, a Frame implements a unit transform
/// in both directions (`frame.c:115-119`), so the recorded values are an
/// identity by construction rather than a result worth re-deriving. The one
/// Frame class that overrides `Transform` is CmpFrame, and it departs from
/// the identity only when a component is a Region (`cmpframe.c:9920-9922`),
/// which the port would load as a Region — a Mapping — and so transform
/// normally.
fn load_fixture(relpath: &str) -> Result<LoadOutcome, String> {
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
        let reason = if obj.as_frame().is_some() {
            "bare Frame: not a Mapping in the Rust port, and a Frame's C \
             transform is a unit transform, so there is no math to check"
        } else {
            "not a Mapping in C either, so the oracle records no transform"
        };
        return Ok(Untransformable(reason));
    }
    Ok(Transformable(Box::new(Loaded { obj, is_frameset })))
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
        assert_eq!(total, 1752);
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
            .expect_transformable();
        let m = l.mapping().expect("mapping");
        assert_eq!(m.nin(), 2);
        assert!(l.frame(true).is_none(), "bare mapping has no frame");
    }

    #[test]
    fn loads_head_frameset_with_frames() {
        let l = load_fixture("cobe.head")
            .expect("no error")
            .expect_transformable();
        assert!(l.is_frameset);
        let m = l.mapping().expect("mapping");
        assert_eq!(m.nin(), 2);
        assert!(l.frame(true).is_some(), "forward -> current frame");
        assert!(l.frame(false).is_some(), "inverse -> base frame");
    }

    /// A bare Frame loads but is untransformable, and says why. C checks
    /// these sections because a Frame is a Mapping there; the reason is
    /// carried into the report so the skip does not have to be re-derived.
    #[test]
    fn bare_frame_is_untransformable_and_says_why() {
        match load_fixture("specflux.ast").expect("loads") {
            Transformable(_) => panic!("a bare Frame has no transform in the Rust port"),
            Untransformable(reason) => {
                assert!(reason.contains("bare Frame"), "reason names the case");
                assert!(
                    reason.contains("unit transform"),
                    "reason says why it is safe"
                );
            }
        }
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
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug)]
struct RtripRule {
    off: bool,
    rtol: f64,
    atol: f64,
}
#[derive(Clone, Copy, Debug, Default)]
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
        // The committed file declares nothing: no fixture may switch a check
        // off wholesale. Per-row licences live in the harness, where they are
        // named and asserted.
        assert!(
            o.rtrip.is_empty() && o.golden.is_empty(),
            "transform_oracle_overrides.txt must declare no overrides; found \
             rtrip={:?} golden={:?}",
            o.rtrip,
            o.golden,
        );
        // The parser still works, so a future entry would take effect.
        let probe = parse_overrides("a.head off\nb.map golden-inv off\n");
        assert_eq!(probe.rtrip("a.head"), None);
        assert!(probe.golden_off("b.map", false));
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn skyframe_longitude_is_cyclic_pixel_is_not() {
        let l = load_fixture("cobe.head").unwrap().expect_transformable();
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
            .expect_transformable();
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
            .expect_transformable();
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

/// Rows where C's own recorded inverse does not recover the recorded forward
/// input, keyed by fixture. `compare_rtrip` excuses exactly these, and only
/// after the live result has failed both the direct and the aliased
/// comparison, so a row C round-trips is never excused here.
///
/// This is a statement about the corpus, and
/// `reference_rtrip_gaps_are_exactly_the_recorded_set` checks it against the
/// `.oracle` files with no fixture loaded and no live transform involved. It
/// assumes the default round-trip tolerances, which is safe while
/// `transform_oracle_overrides.txt` declares none.
const RTRIP_REFERENCE_GAPS_LICENSED: &[(&str, &[usize])] = &[
    // Every row. The TNX Chebyshev distortion builds a ChebyMap, whose
    // storage cannot hold its own Jacobian (the derivative of a 1st-kind
    // Chebyshev T_n is a 2nd-kind U_(n-1)), so PolyMap's iterative inverse is
    // unavailable and C's sky->pixel lands nowhere near the pixel it came
    // from: row 0 goes 1,1 -> 2375.93,-2531.65, and row 22 is BAD
    // (`headers.oracle:5542-5600`). The golden checks still pin both
    // directions against C, which is what proves the port reproduces C's
    // wrong inverse rather than a different one.
    (
        "tnx-cheb.head",
        &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
        ],
    ),
    // One row on the tangential spherical cube: the image is wider than the
    // quad-cube period, so C's inverse returns the aliased preimage
    // 418.3068181819074 for pixel 1400.125. The port recovers the pixel
    // itself, so this row passes through `pixel_ok` and never reaches the
    // reference gate — it is listed because the census is of the corpus, not
    // of what the gate excuses.
    ("tsc.head", &[10]),
];

/// Oracle rows whose recorded values were produced by the generator host's
/// compiler contracting a multiply-add, where the unfused arithmetic C's own
/// source prescribes gives a different answer. Each entry replaces the recorded
/// row with the unfused expectation for the golden comparison only, keyed by
/// `(relpath, forward, row, expected outputs)`.
///
/// This is a strengthening, not a relaxation: the substituted row is asserted
/// exactly, at the same tolerance as every other row.
/// `fma_substitutions_replace_rows_that_really_differ` keeps each entry honest
/// by requiring the recorded values to still be the ones being replaced.
const FMA_SUBSTITUTIONS_LICENSED: &[(&str, bool, usize, &[f64])] = &[
    // UnitNormMap's inverse writes `in*norm + centre[coord]`
    // (`unitnormmap.c:1079`). This fixture's rows 2 and 3 feed the transform an
    // input whose distance from the centre is zero, which C detects by
    // `max_relin == 0` at `unitnormmap.c:1024` and answers with BAD on every
    // axis but the last and 0 on that one. The arm64 generator's compiler fused
    // the multiply-add, leaving 3.4075347438225656e-16 instead of an exact
    // zero, so the guard never fired and the row recorded a noise direction:
    // 0.75853844164804163 0.65162829323331306 3.4075347438225656e-16. Both
    // fused values are reproducible as fma(u, d, 5.0) and fma(u, d, 6.0).
    // Any non-contracting build -- Rust anywhere, and C on this repo's x86-64
    // host, confirmed with `tools/oracle_fixtures/unitnorm_probe` -- takes the
    // guard. See `docs/issues/c-library-quirks.md`.
    (
        "simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map",
        false,
        2,
        &[BAD_VALUE, BAD_VALUE, 0.0],
    ),
    (
        "simplify_fixtures/neg_unitnormmap_inv_fwd_diffcentre.map",
        false,
        3,
        &[BAD_VALUE, BAD_VALUE, 0.0],
    ),
];

/// The reference a golden comparison should use: the recorded outputs, with any
/// `FMA_SUBSTITUTIONS_LICENSED` row replaced. Borrows when nothing applies,
/// which is every section but one.
fn substituted_reference<'a>(
    relpath: &str,
    forward: bool,
    recorded: &'a [Vec<f64>],
) -> std::borrow::Cow<'a, [Vec<f64>]> {
    let subs: Vec<_> = FMA_SUBSTITUTIONS_LICENSED
        .iter()
        .filter(|(rp, fwd, _, _)| *rp == relpath && *fwd == forward)
        .collect();
    if subs.is_empty() {
        return std::borrow::Cow::Borrowed(recorded);
    }
    let mut patched = recorded.to_vec();
    for (_, _, row, expect) in subs {
        assert_eq!(
            expect.len(),
            patched.len(),
            "FMA substitution for {relpath} row {row} has {} values but the \
             section has {} axes",
            expect.len(),
            patched.len(),
        );
        for (axis, col) in patched.iter_mut().enumerate() {
            col[*row] = expect[axis];
        }
    }
    std::borrow::Cow::Owned(patched)
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

/// Does C's own recorded inverse recover the recorded forward input at this
/// row? When it does not, `inverse(forward(P)) == P` is not a property of the
/// fixture, and requiring it of the port would assert something about the
/// corpus. Decided from stored data alone — the live transform plays no part —
/// which is the gate `compare_equiv` applies to the shared domain.
fn reference_round_trips(
    fwd: &Section,
    inv_ref: &[Vec<f64>],
    row: usize,
    rtol: f64,
    atol: f64,
) -> bool {
    (0..fwd.nin).all(|a| within_tol(inv_ref[a][row], fwd.inputs[a][row], rtol, atol))
}

/// Round-trip accuracy (GRID corpora): inverse(forward(P)) ~= P, BAD-skipping
/// and alias-aware (a different recovered pixel is accepted if it
/// forward-maps to the same sky). `inv_live` is the live output of the
/// inverse section (i.e. inverse(forward(P))); `inv_ref` is C's recorded
/// output for the same section, which excuses a row C does not round-trip
/// either — checked last, so it can only excuse a row the live comparison has
/// already failed.
fn compare_rtrip(
    map: &dyn Mapping,
    fwd: &Section,
    inv_live: &[Vec<f64>],
    inv_ref: &[Vec<f64>],
    rtol: f64,
    atol: f64,
) -> Vec<AxisMiss> {
    let mut misses = Vec::new();
    let npoint = fwd.npoint();
    // Forward-transform the recovered pixels to detect aliased preimages.
    let recovered = Section {
        relpath: fwd.relpath.clone(),
        nin: fwd.nin,
        nout: fwd.nout,
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
        // Last: C's own recorded inverse does not recover this pixel either,
        // so the identity is not a property of the fixture.
        if !reference_round_trips(fwd, inv_ref, row, rtol, atol) {
            continue;
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

    #[test]
    fn rtrip_excuses_only_rows_the_reference_also_fails() {
        // A 1-axis identity mapping, so the alias escape is easy to reason
        // about: a recovered pixel forward-maps to itself.
        let map = ast_core::mappings::UnitMap::new(1);
        let fwd = Section {
            relpath: "synthetic.head".into(),
            nin: 1,
            nout: 1,
            forward: true,
            inputs: vec![vec![10.0, 20.0, 30.0, 40.0]],
            outputs: vec![vec![10.0, 20.0, 30.0, 40.0]],
        };
        //            row 0: live recovers                     -> pass
        //            row 1: live wrong, reference recovers    -> MISS
        //            row 2: live wrong, reference also wrong  -> excused
        //            row 3: live recovers, reference wrong    -> pass (order)
        let inv_live = vec![vec![10.0, 99.0, 99.0, 40.0]];
        let inv_ref = vec![vec![10.0, 20.0, 77.0, 55.0]];
        let misses = compare_rtrip(&map, &fwd, &inv_live, &inv_ref, RTRIP_RTOL, RTRIP_ATOL);
        assert_eq!(misses.len(), 1, "only row 1 may be reported: {misses:?}");
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
            Ok(Transformable(l)) => *l,
            Ok(Untransformable(reason)) => {
                reports.push(SectionReport {
                    relpath: s.relpath.clone(),
                    forward: s.forward,
                    outcome: Outcome::Skipped(reason.into()),
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
            let reference = substituted_reference(&s.relpath, s.forward, &s.outputs);
            misses.extend(compare_golden(kind, &live, &reference, &angular));
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
                    misses.extend(compare_rtrip(map, fwd, &live, &s.outputs, rtol, atol));
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

fn check_name(c: CheckKind) -> &'static str {
    match c {
        CheckKind::GoldenFwd => "golden-fwd",
        CheckKind::GoldenInv => "golden-inv",
        CheckKind::Equivalence => "equivalence",
        CheckKind::RoundTrip => "round-trip",
    }
}

/// Coarse magnitude bucket for a mismatch's worst relative divergence.
fn magnitude_bucket(rel: f64, bad_mismatch: bool) -> &'static str {
    if bad_mismatch {
        "bad-mismatch"
    } else if rel <= 1e-6 {
        "<=1e-6"
    } else if rel <= 1e-3 {
        "1e-6..1e-3"
    } else if rel <= 1.0 {
        "1e-3..1"
    } else {
        ">1"
    }
}

/// Minimal JSON string escaping (quotes, backslashes, control chars).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// A mismatch's worst (highest-rel) axis, and whether it is a BAD mismatch.
fn worst_miss(misses: &[AxisMiss]) -> Option<(&AxisMiss, bool)> {
    misses
        .iter()
        .map(|m| {
            let bad = m.got == BAD_VALUE || m.reference == BAD_VALUE;
            (m, bad)
        })
        .max_by(|a, b| {
            a.0.rel
                .partial_cmp(&b.0.rel)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Write the machine-readable divergence report as JSON (no serde: this crate
/// has no serde dependency, and the schema is tiny and fixed).
fn write_json_report(path: &str, reports: &[SectionReport]) -> std::io::Result<()> {
    let mut pass = 0;
    let mut load_fail = 0;
    let mut skipped = 0;
    let mut skipped_override = 0;
    let mut arity = 0;
    let mut failed = 0;

    let mut sections_json = String::new();
    for r in reports {
        let (status, detail) = match &r.outcome {
            Outcome::Pass => {
                pass += 1;
                ("pass".to_string(), String::new())
            }
            Outcome::LoadFail(e) => {
                load_fail += 1;
                ("load-fail".to_string(), e.clone())
            }
            Outcome::Skipped(e) => {
                skipped += 1;
                ("skipped".to_string(), e.clone())
            }
            Outcome::SkippedByOverride(e) => {
                skipped_override += 1;
                ("skipped-override".to_string(), e.clone())
            }
            Outcome::Arity(e) => {
                arity += 1;
                ("arity".to_string(), e.clone())
            }
            Outcome::Mismatch(m) => {
                failed += 1;
                let (worst, bad) = worst_miss(m).unwrap();
                (
                    format!("mismatch:{}", check_name(worst.check)),
                    format!(
                        "bucket={} worst_rel={:.3e} nmiss={} ref={:.17e} got={:.17e}",
                        magnitude_bucket(worst.rel, bad),
                        worst.rel,
                        m.len(),
                        worst.reference,
                        worst.got
                    ),
                )
            }
        };
        let _ = writeln!(
            sections_json,
            "    {{\"relpath\":\"{}\",\"forward\":{},\"status\":\"{}\",\"detail\":\"{}\"}},",
            json_escape(&r.relpath),
            r.forward,
            status,
            json_escape(&detail)
        );
    }
    // Trim trailing comma+newline for valid JSON.
    let sections_json = sections_json.trim_end().trim_end_matches(',');

    let json = format!(
        "{{\n  \"totals\": {{\"sections\":{},\"pass\":{},\"failed\":{},\"skipped\":{},\"skipped_override\":{},\"load_fail\":{},\"arity\":{}}},\n  \"sections\": [\n{}\n  ]\n}}\n",
        reports.len(),
        pass,
        failed,
        skipped,
        skipped_override,
        load_fail,
        arity,
        sections_json
    );
    std::fs::write(path, json)?;

    // stdout human summary: headline + worst divergences.
    println!(
        "transform-oracle: {} sections | pass={} fail={} skip={} skip-override={} load-fail={} arity={}",
        reports.len(),
        pass,
        failed,
        skipped,
        skipped_override,
        load_fail,
        arity
    );
    let mut worst: Vec<(&str, bool, f64, &AxisMiss)> = reports
        .iter()
        .filter_map(|r| match &r.outcome {
            Outcome::Mismatch(m) => {
                worst_miss(m).map(|(w, _bad)| (r.relpath.as_str(), r.forward, w.rel, w))
            }
            _ => None,
        })
        .collect();
    worst.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    println!("worst {} divergences:", worst.len().min(40));
    for (relpath, forward, rel, m) in worst.iter().take(40) {
        println!(
            "  {:<52} {:<7} {:<11} rel={:.3e} row={} axis={} ref={:.6e} got={:.6e}",
            relpath,
            if *forward { "fwd" } else { "inv" },
            check_name(m.check),
            rel,
            m.row,
            m.axis,
            m.reference,
            m.got
        );
    }
    Ok(())
}

/// SlaMap-family oracle sections not yet matching C, each with a one-line
/// reason. The goal is to keep this list empty; it only ever shrinks. A
/// section is identified by `(relpath, forward)`. Reasons must name the
/// divergence class (e.g. `"simplifier: normal-form"` for a `.simp`
/// equivalence gap that belongs to Phase 2, not to SlaMap transform math). A
/// precession-only divergence is never a legitimate entry: palPrec is the
/// same C code on both sides, so C and Rust must agree there.
///
/// After the Phase 1 transform fixes (scalar carrier, AMP arg order,
/// per-step longitude normalization, bad-args guard) every SlaMap section
/// matches the C oracle, so this list is empty.
const SLAMAP_ORACLE_EXCLUDED: &[(&str, bool, &str)] = &[
    // Empty: SlaMap transform parity with C is proven for the whole *sla*
    // simplify corpus. Add a row only for a genuinely out-of-Phase-1 gap,
    // with a specific reason.
];

/// Number of SlaMap-family oracle section-directions currently in the corpus.
/// The gate asserts it examined at least this many, so a future rename or
/// deletion of the fixtures cannot make the gate pass vacuously.
const SLAMAP_SECTION_FLOOR: usize = 136;

/// Is this oracle section owned by SlaMap transform math (a *sla* fixture in
/// the simplify corpus)?
fn is_slamap_section(relpath: &str) -> bool {
    let base = relpath.rsplit('/').next().unwrap_or(relpath);
    base.contains("sla") // cap_slamap_*, sla_*, slamap_*, neg_sla_*
}

/// Regression gate: every SlaMap-family oracle section matches C, except the
/// explicitly-excluded ones. Mirrors the `NATIVE_READ_EXCLUDED` discipline in
/// `test_wcsconv.rs` — the list only shrinks; an empty list means parity is
/// proven for this class. The two invariants: no un-excluded SlaMap section
/// diverges, and no excluded section secretly passes (a stale entry).
#[test]
fn slamap_sections_match_oracle() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let text =
        std::fs::read_to_string(format!("{dir}/simplify_fixtures.oracle")).expect("oracle present");
    let reports = run_oracle(&text, &ov);

    let excluded: std::collections::HashSet<(&str, bool)> = SLAMAP_ORACLE_EXCLUDED
        .iter()
        .map(|(rp, fwd, _)| (*rp, *fwd))
        .collect();

    // A section fails the gate when its transform outputs diverge from C, the
    // fixture won't load, or its arity no longer matches. Pass /
    // SkippedByOverride / Skipped are all acceptable ("not a divergence").
    let is_fail = |o: &Outcome| {
        matches!(
            o,
            Outcome::Mismatch(_) | Outcome::LoadFail(_) | Outcome::Arity(_)
        )
    };

    let mut unexpected_failures: Vec<String> = Vec::new();
    // Every excluded key that never turned up as a live failure is stale.
    let mut stale_exclusions: std::collections::HashSet<(&str, bool)> = excluded.clone();
    let mut checked = 0usize;

    for r in &reports {
        if !is_slamap_section(&r.relpath) {
            continue;
        }
        checked += 1;
        let key = (r.relpath.as_str(), r.forward);
        if excluded.contains(&key) {
            if is_fail(&r.outcome) {
                stale_exclusions.remove(&key);
            }
            // An excluded section that now passes stays in `stale_exclusions`.
        } else if is_fail(&r.outcome) {
            unexpected_failures.push(format!(
                "{} (forward={}): {:?}",
                r.relpath,
                r.forward,
                describe_outcome(&r.outcome),
            ));
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "SlaMap oracle sections diverged from C (not excluded):\n{}",
        unexpected_failures.join("\n"),
    );
    assert!(
        stale_exclusions.is_empty(),
        "SLAMAP_ORACLE_EXCLUDED entries now pass — remove them:\n{:?}",
        stale_exclusions,
    );
    assert!(
        checked >= SLAMAP_SECTION_FLOOR,
        "gate examined only {checked} SlaMap sections (expected >= {SLAMAP_SECTION_FLOOR}); \
         fixtures may have been renamed or removed",
    );
}

/// Short human description of a non-pass outcome for gate failure messages.
fn describe_outcome(o: &Outcome) -> String {
    match o {
        Outcome::Pass => "pass".to_string(),
        Outcome::LoadFail(e) => format!("load-fail: {e}"),
        Outcome::Skipped(e) => format!("skipped: {e}"),
        Outcome::SkippedByOverride(e) => format!("skipped-override: {e}"),
        Outcome::Arity(e) => format!("arity: {e}"),
        Outcome::Mismatch(m) => {
            let (worst, bad) = worst_miss(m).expect("mismatch has >=1 miss");
            format!(
                "mismatch:{} bucket={} worst_rel={:.3e} nmiss={} ref={:.6e} got={:.6e}",
                check_name(worst.check),
                magnitude_bucket(worst.rel, bad),
                worst.rel,
                m.len(),
                worst.reference,
                worst.got,
            )
        }
    }
}

/// `.head` oracle sections not yet matching C, each with a one-line reason
/// naming the unported C read-path branch. The goal is to keep this list
/// empty; it only ever shrinks. A section is identified by `(relpath,
/// forward)`. These are FITS-read structural gaps (the decoded object has a
/// different shape/attribute than C), never serialisation or simplifier
/// artifacts — the `.ast`/`.simp` corpus is already byte-exact.
const HEAD_ORACLE_EXCLUDED: &[(&str, bool, &str)] = &[];

/// Number of `.head` oracle section-directions currently in the corpus. The
/// gate asserts it examined at least this many, so a rename or deletion of a
/// fixture cannot make the gate pass vacuously.
const HEAD_SECTION_FLOOR: usize = 259;

/// Is this oracle section a `.head` FITS-read fixture?
fn is_head_section(relpath: &str) -> bool {
    relpath.ends_with(".head")
}

/// Regression gate: every `.head` oracle section matches C, except the
/// explicitly-excluded ones. The list only shrinks; an empty list means parity
/// is proven for all FITS-read fixtures. The two invariants: no un-excluded
/// head section diverges, and no excluded section secretly passes (a stale
/// entry).
#[test]
fn head_sections_match_oracle() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let text = std::fs::read_to_string(format!("{dir}/headers.oracle")).expect("oracle present");
    let reports = run_oracle(&text, &ov);

    let excluded: std::collections::HashSet<(&str, bool)> = HEAD_ORACLE_EXCLUDED
        .iter()
        .map(|(rp, fwd, _)| (*rp, *fwd))
        .collect();

    // A section fails the gate when its transform outputs diverge from C, the
    // fixture won't load, or its arity no longer matches. Pass /
    // SkippedByOverride / Skipped are all acceptable ("not a divergence").
    let is_fail = |o: &Outcome| {
        matches!(
            o,
            Outcome::Mismatch(_) | Outcome::LoadFail(_) | Outcome::Arity(_)
        )
    };

    let mut unexpected_failures: Vec<String> = Vec::new();
    // Every excluded key that never turned up as a live failure is stale.
    let mut stale_exclusions: std::collections::HashSet<(&str, bool)> = excluded.clone();
    let mut checked = 0usize;

    for r in &reports {
        if !is_head_section(&r.relpath) {
            continue;
        }
        checked += 1;
        let key = (r.relpath.as_str(), r.forward);
        if excluded.contains(&key) {
            if is_fail(&r.outcome) {
                stale_exclusions.remove(&key);
            }
            // An excluded section that now passes stays in `stale_exclusions`.
        } else if is_fail(&r.outcome) {
            unexpected_failures.push(format!(
                "{} (forward={}): {:?}",
                r.relpath,
                r.forward,
                describe_outcome(&r.outcome),
            ));
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "head oracle sections diverged from C (not excluded):\n{}",
        unexpected_failures.join("\n"),
    );
    assert!(
        stale_exclusions.is_empty(),
        "HEAD_ORACLE_EXCLUDED entries now pass — remove them:\n{:?}",
        stale_exclusions,
    );
    assert!(
        checked >= HEAD_SECTION_FLOOR,
        "gate examined only {checked} head sections (expected >= {HEAD_SECTION_FLOOR}); \
         fixtures may have been renamed or removed",
    );
}

/// Simplify-corpus oracle sections not yet matching C, each with a one-line
/// reason. Same discipline as `HEAD_ORACLE_EXCLUDED` and
/// `SLAMAP_ORACLE_EXCLUDED`: identified by `(relpath, forward)`, only ever
/// shrinks, and empty means parity is proven.
///
/// A `LoadFail` on a fixture staged under `simplify_fixtures_unsupported/`
/// does not belong here — the gate licenses those structurally, since they
/// name a class the port has not implemented (`PLAN.md` §4, enforced by
/// `test_deferred_fixtures.rs`). Everything else is a defect to fix.
const SIMPLIFY_ORACLE_EXCLUDED: &[(&str, bool, &str)] = &[];

/// Number of simplify-corpus oracle section-directions currently gated. The
/// gate asserts it examined at least this many, so a rename or deletion of a
/// fixture cannot make the gate pass vacuously.
/// Of the 1752 sections, 259 are `.head` and 136 are SlaMap, leaving 1357
/// here, of which 18 are the deferred-class load-fails licensed below.
const SIMPLIFY_SECTION_FLOOR: usize = 1339;

/// Is this oracle section part of the simplify corpus (a `simplify_fixtures/`
/// relpath or a top-level `.ast`/`.map`/`.simp` fixture)? This is everything
/// the `.head` and SlaMap gates do not already cover.
fn is_simplify_section(relpath: &str) -> bool {
    !is_head_section(relpath) && !is_slamap_section(relpath)
}

/// Does this section's fixture resolve to the deferred-class staging area?
///
/// Those fixtures are kept in the corpus so the coverage they represent stays
/// visible, but they instantiate a class the port has not implemented, so they
/// cannot load. A `LoadFail` on one is expected; any other failure, and any
/// failure on a fixture outside that area, is a defect.
fn is_deferred_class_fixture(relpath: &str) -> bool {
    resolve_fixture(relpath).is_some_and(|p| {
        p.components()
            .any(|c| c.as_os_str() == "simplify_fixtures_unsupported")
    })
}

/// Regression gate over every oracle section the `.head` and SlaMap gates do
/// not cover — the simplify corpus, which is the bulk of the 1752 sections.
///
/// Without this the majority of the corpus was read only by
/// `transform_oracle_report`, which is `#[ignore]` and does not assert, so a
/// divergence there failed nothing.
#[test]
fn simplify_sections_match_oracle() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let mut reports = Vec::new();
    for name in [
        "simplify_fixtures.oracle",
        "headers.oracle",
        "framesets.oracle",
    ] {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).expect("oracle present");
        reports.extend(run_oracle(&text, &ov));
    }

    let excluded: std::collections::HashSet<(&str, bool)> = SIMPLIFY_ORACLE_EXCLUDED
        .iter()
        .map(|(rp, fwd, _)| (*rp, *fwd))
        .collect();

    // A section fails the gate when its transform outputs diverge from C, the
    // fixture won't load, or its arity no longer matches. Pass /
    // SkippedByOverride / Skipped are all acceptable ("not a divergence").
    let is_fail = |o: &Outcome| {
        matches!(
            o,
            Outcome::Mismatch(_) | Outcome::LoadFail(_) | Outcome::Arity(_)
        )
    };

    let mut unexpected_failures: Vec<String> = Vec::new();
    let mut stale_exclusions: std::collections::HashSet<(&str, bool)> = excluded.clone();
    let mut checked = 0usize;

    for r in &reports {
        if !is_simplify_section(&r.relpath) {
            continue;
        }
        // A deferred-class fixture may fail to load, and only that way.
        if matches!(r.outcome, Outcome::LoadFail(_)) && is_deferred_class_fixture(&r.relpath) {
            continue;
        }
        checked += 1;
        let key = (r.relpath.as_str(), r.forward);
        if excluded.contains(&key) {
            if is_fail(&r.outcome) {
                stale_exclusions.remove(&key);
            }
            // An excluded section that now passes stays in `stale_exclusions`.
        } else if is_fail(&r.outcome) {
            unexpected_failures.push(format!(
                "{} (forward={}): {:?}",
                r.relpath,
                r.forward,
                describe_outcome(&r.outcome),
            ));
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "simplify oracle sections diverged from C (not excluded):\n{}",
        unexpected_failures.join("\n"),
    );
    assert!(
        stale_exclusions.is_empty(),
        "SIMPLIFY_ORACLE_EXCLUDED entries now pass — remove them:\n{:?}",
        stale_exclusions,
    );
    assert!(
        checked >= SIMPLIFY_SECTION_FLOOR,
        "gate examined only {checked} simplify sections (expected >= \
         {SIMPLIFY_SECTION_FLOOR}); fixtures may have been renamed or removed",
    );
}

/// Every oracle section is covered by one of the three asserting gates, so a
/// divergence anywhere in the corpus fails a test rather than only showing up
/// in the `#[ignore]` report.
#[test]
fn every_oracle_section_is_gated() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let mut ungated = Vec::new();
    let mut total = 0usize;
    for name in [
        "simplify_fixtures.oracle",
        "headers.oracle",
        "framesets.oracle",
    ] {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).expect("oracle present");
        for s in parse_oracle(&text).expect("oracle parses") {
            total += 1;
            if !is_head_section(&s.relpath)
                && !is_slamap_section(&s.relpath)
                && !is_simplify_section(&s.relpath)
            {
                ungated.push(s.relpath);
            }
        }
    }
    assert!(
        ungated.is_empty(),
        "oracle sections covered by no gate:\n{ungated:?}",
    );
    assert_eq!(total, 1752, "gates must span every section");
}

/// The set of rows where C's own recorded inverse does not recover the pixel is
/// exactly `RTRIP_REFERENCE_GAPS_LICENSED`. Corpus-only: no fixture is loaded
/// and no transform runs, so this cannot be satisfied by a change in the port.
/// A regenerated corpus, a new fixture whose reference does not round-trip, or
/// a stale entry all fail here.
#[test]
fn reference_rtrip_gaps_are_exactly_the_recorded_set() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let mut found: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for name in [
        "simplify_fixtures.oracle",
        "headers.oracle",
        "framesets.oracle",
    ] {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).expect("oracle present");
        let sections = parse_oracle(&text).expect("oracle parses");
        // Same pairing as `run_oracle`: an inverse section of a GRID fixture
        // with the immediately-preceding forward section of the same fixture.
        let mut prev_fwd: Option<&Section> = None;
        for s in &sections {
            let grid = has_ext(&s.relpath, ".head") || has_ext(&s.relpath, ".ast");
            if !s.forward && grid {
                if let Some(fwd) = prev_fwd.filter(|f| {
                    f.relpath == s.relpath && f.nin == s.nout && f.npoint() == s.npoint()
                }) {
                    for row in 0..fwd.npoint() {
                        // `compare_rtrip` skips a row whose forward output was
                        // BAD before any comparison happens.
                        if (0..fwd.nout).any(|b| fwd.outputs[b][row] == BAD_VALUE) {
                            continue;
                        }
                        if !reference_round_trips(fwd, &s.outputs, row, RTRIP_RTOL, RTRIP_ATOL) {
                            found.entry(s.relpath.clone()).or_default().push(row);
                        }
                    }
                }
            }
            if s.forward {
                prev_fwd = Some(s);
            }
        }
    }

    let expected: std::collections::BTreeMap<String, Vec<usize>> = RTRIP_REFERENCE_GAPS_LICENSED
        .iter()
        .map(|(rp, rows)| ((*rp).to_string(), rows.to_vec()))
        .collect();
    assert_eq!(
        found, expected,
        "the rows where C's own inverse does not recover the pixel have \
         changed. Update RTRIP_REFERENCE_GAPS_LICENSED, the ledger and \
         PLAN.md §6 together, with a reason per fixture",
    );
}

/// Every FMA substitution must name a row that exists and whose recorded values
/// actually differ from the substituted ones. A regenerated corpus that agrees
/// with the unfused arithmetic makes the entry dead weight, and this fails
/// rather than letting it sit there unnoticed.
#[test]
fn fma_substitutions_replace_rows_that_really_differ() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    for (relpath, forward, row, expect) in FMA_SUBSTITUTIONS_LICENSED {
        let name = if has_ext(relpath, ".head") {
            "headers.oracle"
        } else if has_ext(relpath, ".ast") {
            "framesets.oracle"
        } else {
            "simplify_fixtures.oracle"
        };
        let text = std::fs::read_to_string(format!("{dir}/{name}")).expect("oracle present");
        let sections = parse_oracle(&text).expect("oracle parses");
        let s = sections
            .iter()
            .find(|s| s.relpath == *relpath && s.forward == *forward)
            .unwrap_or_else(|| panic!("no section for {relpath} (forward={forward})"));
        assert!(
            *row < s.npoint(),
            "{relpath} row {row} is past the section's {} rows",
            s.npoint(),
        );
        assert_eq!(
            expect.len(),
            s.outputs.len(),
            "{relpath} row {row}: axis count"
        );
        let differs = (0..s.outputs.len())
            .any(|a| !within_tol(s.outputs[a][*row], expect[a], GOLDEN_RTOL, GOLDEN_ATOL));
        assert!(
            differs,
            "{relpath} row {row}: the recorded values already agree with the \
             substitution, so the entry is dead. Delete it from \
             FMA_SUBSTITUTIONS_LICENSED, the ledger and PLAN.md §6",
        );
    }
}

/// Investigative harness. Not part of the normal suite or CI: run with
///   cargo test -p ast-core --test test_transform_oracle -- --ignored --nocapture
/// It runs every oracle section, prints a summary, and writes a JSON report
/// to `target/transform_oracle_report.json`. It never asserts on divergence
/// (the whole point is to measure it); it panics only on a broken harness.
#[test]
#[ignore = "investigative: measures Rust-vs-C transform divergence; run with --ignored"]
fn transform_oracle_report() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let mut reports = Vec::new();
    for name in [
        "simplify_fixtures.oracle",
        "headers.oracle",
        "framesets.oracle",
    ] {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).expect("oracle present");
        reports.extend(run_oracle(&text, &ov));
    }
    assert_eq!(reports.len(), 1752, "harness must cover every section");
    let out = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/transform_oracle_report.json"
    );
    write_json_report(out, &reports).expect("report writes");
    println!("wrote {out}");
}
