//! `astFindFrame` / `astMatch` / `astSubFrame` port (frame.c:3735, 7073, 10838).
//!
//! The triplet answers "is there a coordinate system reachable from
//! this Frame that matches a template?".
//!
//! - `match_template` is the per-class predicate; mirrors C
//!   `astMatch` (frame.c:7073).
//! - `sub_frame` is the per-class workhorse that selects axes from
//!   the target and overlays template attributes; mirrors C
//!   `astSubFrame` (frame.c:10838).
//! - `find_frame_template` is the public entry point that wraps both
//!   plus the comma-separated domain-list filter; mirrors C
//!   `astFindFrame` (frame.c:3735) for plain Frames and
//!   `FrameSet::FindFrame` (frameset.c:3790) for FrameSet targets.

use crate::error::AstResult;
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::DynMapping;

/// Result of a successful `match_template` call.
///
/// Mirrors the four out-parameters of C `astMatch`:
/// `template_axes`, `target_axes`, `map`, `result`.
pub struct MatchResult {
    /// For each axis in the result Frame, the (zero-based) index of
    /// the *template* Frame axis it derives from, or `-1` if it has
    /// no template association.
    #[allow(dead_code)]
    pub template_axes: Vec<i32>,
    /// For each axis in the result Frame, the (zero-based) index of
    /// the *target* Frame axis it derives from, or `-1` if it has
    /// no target association.
    #[allow(dead_code)]
    pub target_axes: Vec<i32>,
    /// Mapping from `target` to `result`. Apply forward to convert
    /// target coordinates into the result coordinate system.
    pub map: DynMapping,
    /// Result Frame: combines target attribute values with any
    /// attributes the template explicitly set.
    pub result: DynFrame,
}

/// Result of a successful `find_frame_template` call. Identical shape
/// to C `AstFrameSet *astFindFrame` (a 2-frame FrameSet, base = target,
/// current = result).
pub(crate) type FindFrameResult = FrameSet;

/// Default `find_frame_template` body — mirrors the public C
/// `astFindFrame` (frame.c:3735) without the FrameSet override.
///
/// Calls `template.match_template(target, false)`, gates the result
/// on the supplied `domainlist` (comma-separated, case-insensitive,
/// blank-permits-any), and packages the matched mapping + result
/// Frame into a 2-frame FrameSet (base = target, current = result).
///
/// `target` is the snapshot of the calling Frame; this two-step
/// indirection avoids the `&Self` → `&dyn Frame` coercion issue that
/// would otherwise force every concrete class to implement an
/// upcaster.
pub(crate) fn default_find_frame(
    target: DynFrame,
    template: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<FindFrameResult>> {
    // Domain-list pre-processing: comma-separated, case-insensitive,
    // whitespace-stripped, with an extra leading + trailing comma so
    // substring tests match exactly. Mirrors frame.c:4112-4124.
    let mut domainlist_canonical = String::with_capacity(domainlist.len() + 2);
    domainlist_canonical.push(',');
    for c in domainlist.chars() {
        if !c.is_whitespace() {
            domainlist_canonical.push(c.to_ascii_uppercase());
        }
    }
    domainlist_canonical.push(',');

    // Per the C contract, astMatch is invoked on the template (with
    // target as second arg). Our trait dispatches this on the
    // template via `template.match_template(target, ...)`.
    let Some(m) = template.match_template(&*target, false)? else {
        return Ok(None);
    };

    // Gate on domain. Mirrors frame.c:4136-4154. The result Frame's
    // domain (already overlaid with the template's domain when the
    // template had one explicitly set) must appear in the
    // canonicalised domain list.
    let mut domain_canonical = String::with_capacity(m.result.domain().len() + 2);
    domain_canonical.push(',');
    for c in m.result.domain().chars() {
        domain_canonical.push(c.to_ascii_uppercase());
    }
    domain_canonical.push(',');

    let domain_ok =
        domainlist_canonical.contains(&domain_canonical[..]) || domainlist_canonical.contains(",,");
    if !domain_ok {
        return Ok(None);
    }

    // Simplify the mapping (frame.c:4157-4159).
    let map = crate::mappings::Mapping::simplified(m.map.as_ref())?;

    // Build the 2-frame result FrameSet (base = target, current = result).
    let mut fs = FrameSet::new(target);
    fs.add_frame(FrameRef::Base, map, m.result)?;
    Ok(Some(fs))
}
