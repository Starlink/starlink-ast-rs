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
use crate::mappings::{DynMapping, PermMap, UnitMap};

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

/// Default `SubFrame` body for plain Frames — mirrors C
/// `Frame::SubFrame` (frame.c:10838).
///
/// Builds a Mapping from `target` to a result Frame whose axes are
/// selected by `target_axes`. If `target_naxes == result_naxes` and
/// the selection is the identity, a `UnitMap` is returned; otherwise
/// a `PermMap` is constructed.
///
/// `target_axes` and `template_axes` are zero-based axis indices
/// (with `-1` meaning "no association"). The result Frame is built
/// from `target.pick_axes_snapshot(target_axes)` and any explicitly-
/// set template attributes are then overlaid via
/// `Frame::overlay_template`.
pub(crate) fn default_sub_frame(
    target: &dyn Frame,
    template: Option<&dyn Frame>,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    let target_naxes = target.naxes();
    let result_naxes = target_axes.len();

    // outperm (forward target -> result, length result_naxes):
    //   outperm[result_axis] = target_axis (1-based) or 0 if unassociated.
    // inperm (inverse result -> target, length target_naxes):
    //   inperm[target_axis] = result_axis (1-based) or 0 if no result
    //   axis is associated with this target axis.
    //
    // Mirrors frame.c:10987-11015 with the only difference being the
    // 1-based / 0-as-BAD encoding of our PermMap (vs the C 0-based /
    // -1-as-unassociated encoding).
    let mut outperm = vec![0i64; result_naxes];
    let mut inperm = vec![0i64; target_naxes];
    let mut unit = target_naxes == result_naxes;
    for (result_axis, &t) in target_axes.iter().enumerate() {
        if t < 0 || (t as usize) >= target_naxes {
            outperm[result_axis] = 0;
            unit = false;
        } else {
            outperm[result_axis] = (t as i64) + 1;
            inperm[t as usize] = (result_axis as i64) + 1;
            if t as usize != result_axis {
                unit = false;
            }
        }
    }

    // Build the result Frame: snapshot the target with only the
    // chosen axes, then overlay template attributes if a template
    // was supplied.
    let mut result = target.pick_axes_snapshot(target_axes)?;
    if let Some(t) = template {
        result.overlay_template(t, template_axes)?;
    }

    // Mapping construction: UnitMap if no permutation/selection,
    // PermMap otherwise. Mirrors frame.c:11062-11072.
    let map: DynMapping = if unit {
        Box::new(UnitMap::new(result_naxes))
    } else {
        Box::new(PermMap::new(&inperm, &outperm, &[])?)
    };

    Ok(Some((map, result)))
}

/// Default `Match` body — mirrors C `Frame::Match` (frame.c:7073).
///
/// Checks axis-cardinality bounds (`MinAxes` / `MaxAxes`), gates on
/// `Domain` equality if the template has its Domain explicitly set,
/// builds the `template_axes` / `target_axes` association arrays per
/// the `MatchEnd` attribute, and defers to `target.sub_frame` to
/// produce the (Mapping, result Frame) pair.
pub(crate) fn default_match(
    template: &dyn Frame,
    target: &dyn Frame,
    _match_sub: bool,
) -> AstResult<Option<MatchResult>> {
    // Cardinality check (frame.c:7244-7254).
    let target_naxes = target.naxes();
    let min_axes = template.min_axes();
    let max_axes = template.max_axes();
    if target_naxes < min_axes || target_naxes > max_axes {
        return Ok(None);
    }

    // Domain check (frame.c:7256-7281). Only applies when the
    // template has its Domain explicitly set; the target's domain may
    // be defaulted.
    if template.test_domain() && template.domain() != target.domain() {
        return Ok(None);
    }

    // PreserveAxes / MatchEnd / template axis count
    // (frame.c:7283-7293).
    let preserve_axes = template.preserve_axes();
    let match_end = template.match_end();
    let template_naxes = template.naxes();
    let result_naxes = if preserve_axes {
        target_naxes
    } else {
        template_naxes
    };

    // Build the per-result-axis association arrays
    // (frame.c:7300-7327).
    let mut template_axes = vec![0i32; result_naxes];
    let mut target_axes = vec![0i32; result_naxes];
    let template_offset = (template_naxes as i32) - (result_naxes as i32);
    let target_offset = (target_naxes as i32) - (result_naxes as i32);
    for result_axis in 0..result_naxes {
        let mut t_idx = result_axis as i32;
        let mut tg_idx = result_axis as i32;
        if match_end {
            t_idx += template_offset;
            tg_idx += target_offset;
        }
        template_axes[result_axis] = if t_idx < 0 || t_idx >= template_naxes as i32 {
            -1
        } else {
            t_idx
        };
        target_axes[result_axis] = if tg_idx < 0 || tg_idx >= target_naxes as i32 {
            -1
        } else {
            tg_idx
        };
    }

    // Defer to the target's SubFrame to build the result + Mapping
    // (frame.c:7332-7334).
    let Some((map, result)) =
        target.sub_frame(Some(template), result_naxes, &target_axes, &template_axes)?
    else {
        return Ok(None);
    };

    Ok(Some(MatchResult {
        template_axes,
        target_axes,
        map,
        result,
    }))
}
