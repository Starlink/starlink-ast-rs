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
use crate::frames::{CmpFrame, DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap, UnitMap};

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

/// `astMatch` with the matchsub cast-retry (frame.c:15473). Tries the
/// direct per-class match first; on failure (when `match_sub` is set),
/// casts the template down into the target's class, re-fixes its
/// Domain, and retries via the base Match. This is what lets a
/// specialised template (SkyFrame, CmpFrame, ...) align with a plainer
/// target sharing its Domain.
pub(crate) fn match_with_subclass(
    template: &dyn Frame,
    target: &dyn Frame,
    match_sub: bool,
) -> AstResult<Option<MatchResult>> {
    if let Some(m) = template.match_template(target, match_sub)? {
        return Ok(Some(m));
    }
    if match_sub {
        if let Some(mut super_this) = template.cast(target)? {
            // Re-fix the Domain: the parent class may default it
            // differently (frame.c:15505-15509).
            if target.test_domain() {
                let dom = template.domain();
                if !dom.is_empty() {
                    super_this.set_domain(dom);
                }
            }
            return super_this.match_template(target, match_sub);
        }
    }
    Ok(None)
}

/// Port of C `astConvertX` (frame.c:3028): negotiate a common
/// coordinate system between `from` and `to`, returning a 2-frame
/// FrameSet (base = `from`, current = `to`) whose Mapping converts
/// between them, or `None` if no conversion is possible. `domainlist`
/// is a comma-separated priority list ("" / blank field permits any).
pub(crate) fn convert_frames(
    from: &dyn Frame,
    to: &dyn Frame,
    domainlist: &str,
) -> AstResult<Option<FrameSet>> {
    let mut result_map: Option<DynMapping> = None;

    // Loop over comma-separated domain fields until a match is found
    // (frame.c:3124-3126).
    let mut matched = false;
    for raw_field in domainlist.split(',') {
        if matched {
            break;
        }

        // Clean the domain: uppercase, strip whitespace
        // (CleanDomain, frame.c:3132).
        let domain: String = raw_field
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();

        // Score the best match for the current domain (frame.c:3137).
        let mut best_score = -1;
        let mut perfect = false;

        // Consider both "to" and "from" as the basis of a common
        // coordinate system, quitting early on a perfect match
        // (frame.c:3142-3143).
        for icom in 0..=1 {
            if perfect {
                break;
            }

            // common0 = copy of (icom ? from : to); PreserveAxes = 0;
            // Domain set when the field is non-blank
            // (frame.c:3149-3155).
            let mut common0 = if icom == 1 {
                from.snapshot()
            } else {
                to.snapshot()
            };
            common0.set_preserve_axes(false);
            if !domain.is_empty() {
                common0.set_domain(&domain);
            }

            // frame1 = (icom ? to : from) (frame.c:3158).
            let frame1: &dyn Frame = if icom == 1 { to } else { from };

            // First match: common0 (template) probes frame1, producing
            // the map frame1 -> common1 stored at map1_address
            // (frame.c:3161-3168). map1_address = icom ? &to_map :
            // &from_map.
            let Some(m1) = match_with_subclass(common0.as_ref(), frame1, true)? else {
                continue;
            };
            let map1 = m1.map;
            let mut common1 = m1.result;
            common1.set_preserve_axes(false);

            // frame2 = (icom ? from : to) (frame.c:3179).
            let frame2: &dyn Frame = if icom == 1 { from } else { to };

            // Second match: common1 (template) probes frame2, producing
            // the map frame2 -> common2 stored at map2_address
            // (frame.c:3180-3183). map2_address = icom ? &from_map :
            // &to_map.
            let Some(m2) = match_with_subclass(common1.as_ref(), frame2, true)? else {
                continue;
            };
            let map2 = m2.map;

            // map1_address / map2_address bookkeeping (frame.c:3161,
            // 3180): for icom==0, map1 = from_map and map2 = to_map;
            // for icom==1, map1 = to_map and map2 = from_map.
            let (from_map, mut to_map) = if icom == 1 {
                (map2, map1)
            } else {
                (map1, map2)
            };

            // Invert the "to" Mapping and compose from_map then the
            // inverted to_map in series, then simplify
            // (frame.c:3195-3198).
            to_map.invert();
            let composed = CmpMap::new(from_map, to_map, Composition::Series)?;
            let map = Mapping::simplified(&composed)?;

            // Score: 2*forward + inverse; keep the best
            // (frame.c:3204-3221).
            let score =
                (if map.has_forward() { 2 } else { 0 }) + (if map.has_inverse() { 1 } else { 0 });
            if score > best_score {
                matched = true;
                best_score = score;
                perfect = best_score >= 3;
                result_map = Some(map);
            }
        }
    }

    // If a result map was found, build the result FrameSet
    // (frame.c:3246-3250).
    if let Some(map) = result_map {
        let mut fs = FrameSet::new(from.snapshot());
        fs.add_frame(FrameRef::Base, map, to.snapshot())?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}

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
    let Some(m) = match_with_subclass(template, &*target, false)? else {
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

/// Overlay `template`'s explicitly-set attributes onto `result`.
///
/// Mirrors the polymorphic dispatch performed by C `astOverlay` on the
/// template's class. In Rust the trait method `overlay_template` is
/// dispatched on the *result* frame (`&mut self`), so this free
/// function inserts the missing template-side dispatch:
///
/// - When `template` is a [`CmpFrame`], the C `CmpFrame::Overlay`
///   algorithm (cmpframe.c:169) is reproduced here: build per-component
///   axis-association arrays and recurse into `apply_template_overlay`
///   with each component as a sub-template.
/// - For any other template class, fall through to
///   `result.overlay_template(template, template_axes)` so the per-class
///   override (SkyFrame, SpecFrame, …) handles the copy.
///
/// `template_axes` has length equal to `result.naxes()` and gives, for
/// each result axis, the (zero-based) *external* template axis index
/// from which axis-dependent attributes should be obtained, or `-1` for
/// "no association".
pub(crate) fn apply_template_overlay(
    template: &dyn Frame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    // Non-CmpFrame templates: defer to the per-class result-side
    // overlay implementation.
    let Some(template_cmp) = template.as_any().downcast_ref::<CmpFrame>() else {
        return result.overlay_template(template, template_axes);
    };

    // CmpFrame template: mirrors cmpframe.c:169-427.
    let perm = template_cmp.perm();
    let nc1 = template_cmp.frame1().naxes();
    let nres = result.naxes();

    // Helper: external template axis index for result external axis i.
    // Mirrors `template_axes ? template_axes[j] : j` in the C source.
    let template_ext_axis = |result_ext: usize| -> Option<usize> {
        if template_axes.is_empty() {
            Some(result_ext)
        } else {
            let v = template_axes[result_ext];
            if v < 0 { None } else { Some(v as usize) }
        }
    };

    let mut done = false;

    // Branch 1 — result is a CmpFrame (cmpframe.c:262-380). When each
    // result subframe's axes all derive from a single template
    // subframe, we recurse into the result's children with the
    // matching template child as the sub-template.
    if result.as_any().downcast_ref::<CmpFrame>().is_some() {
        // Borrow the result CmpFrame immutably first to read perm /
        // child sizes; release the borrow before mutably re-entering
        // for the recursive overlay calls.
        let (nres1, nres2, rperm) = {
            let res_cmp = result.as_any().downcast_ref::<CmpFrame>().unwrap();
            (
                res_cmp.frame1().naxes(),
                res_cmp.frame2().naxes(),
                res_cmp.perm().to_vec(),
            )
        };
        let nres_total = nres1 + nres2;
        debug_assert_eq!(nres_total, nres);

        let mut axes1 = vec![0i32; nres1];
        let mut axes2 = vec![0i32; nres2];

        // Per-internal-axis loop for result.frame1.
        let mut isfirst = false;
        let mut issecond = false;
        let mut clean = true;
        #[allow(clippy::needless_range_loop)]
        for i in 0..nres1 {
            // Find external result axis j such that rperm[j] == i.
            let Some(j) = (0..nres_total).find(|&j| rperm[j] == i) else {
                clean = false;
                break;
            };
            // Map result external axis j to a template internal axis.
            let Some(t_ext) = template_ext_axis(j) else {
                // No template association: treat as if it's outside
                // both subframes (analogous to a no-op axis). We can
                // still preserve the partition by recording -1 in the
                // axes array; we choose to mark which side it belongs
                // to based on the previously-established correspondence
                // and leave attribute overlay alone for that axis.
                // To stay strictly faithful to the C — which doesn't
                // pass NULL or -1 entries on this code path because
                // template_axes always comes from astMatch with valid
                // indices — bail out of the clean partition.
                clean = false;
                break;
            };
            let icmp = perm[t_ext];
            if icmp < nc1 {
                if issecond {
                    clean = false;
                    break;
                }
                isfirst = true;
                axes1[i] = icmp as i32;
            } else {
                if isfirst {
                    clean = false;
                    break;
                }
                issecond = true;
                axes1[i] = (icmp - nc1) as i32;
            }
        }
        let sub1_is_first = isfirst;
        let sub1_used = isfirst || issecond;

        // Per-internal-axis loop for result.frame2.
        let mut isfirst = false;
        let mut issecond = false;
        if clean {
            #[allow(clippy::needless_range_loop)]
            for i in 0..nres2 {
                let Some(j) = (0..nres_total).find(|&j| rperm[j] == i + nres1) else {
                    clean = false;
                    break;
                };
                let Some(t_ext) = template_ext_axis(j) else {
                    clean = false;
                    break;
                };
                let icmp = perm[t_ext];
                if icmp < nc1 {
                    if issecond {
                        clean = false;
                        break;
                    }
                    isfirst = true;
                    axes2[i] = icmp as i32;
                } else {
                    if isfirst {
                        clean = false;
                        break;
                    }
                    issecond = true;
                    axes2[i] = (icmp - nc1) as i32;
                }
            }
        }
        let sub2_is_first = isfirst;
        let sub2_used = isfirst || issecond;

        // Both result subframes must end up associated with *different*
        // template subframes (cmpframe.c:365 — `sub1 == sub2 → done=0`).
        let sides_distinct = sub1_used && sub2_used && sub1_is_first != sub2_is_first;

        if clean && sides_distinct {
            // Recurse with each template child as a sub-template,
            // mirroring `astOverlay(sub1, axes1, res->frame1)` etc.
            let result_cmp = result
                .as_any_mut()
                .downcast_mut::<CmpFrame>()
                .expect("result was CmpFrame above");

            // To avoid simultaneous borrows of template_cmp's children
            // and result_cmp's children we resolve template ptrs first.
            // (`template_cmp` is borrowed immutably; `result_cmp`
            // mutably — different objects, so no aliasing.)
            let (sub1_ref, sub2_ref): (&dyn Frame, &dyn Frame) = if sub1_is_first {
                (template_cmp.frame1(), template_cmp.frame2())
            } else {
                (template_cmp.frame2(), template_cmp.frame1())
            };

            apply_template_overlay(sub1_ref, &axes1, result_cmp.frame1_mut())?;
            apply_template_overlay(sub2_ref, &axes2, result_cmp.frame2_mut())?;
            done = true;
        }
    }

    // Branch 2 — fall-back (cmpframe.c:382-427). For each result axis,
    // assign it to whichever template component the corresponding
    // template axis lives in, leaving the other component's slot at -1.
    // Then call overlay with each component template into the *whole*
    // result Frame.
    if !done {
        let mut axes1 = vec![-1i32; nres];
        let mut axes2 = vec![-1i32; nres];
        for i in 0..nres {
            let Some(t_ext) = template_ext_axis(i) else {
                continue;
            };
            let icmp = perm[t_ext];
            if icmp < nc1 {
                axes1[i] = icmp as i32;
            } else {
                axes2[i] = (icmp - nc1) as i32;
            }
        }
        // Two recursive calls: each with one of the template
        // components as the sub-template. The component overlay impls
        // (SkyFrame::overlay_template, SpecFrame::overlay_template,
        // BasicFrame::overlay_template) inspect the supplied axes and
        // skip those marked -1 / out of range.
        apply_template_overlay(template_cmp.frame1(), &axes1, result)?;
        apply_template_overlay(template_cmp.frame2(), &axes2, result)?;
    }

    Ok(())
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
    // chosen axes, then overlay the target's and (if supplied) the
    // template's attributes. C overlays the target Frame onto the
    // result before the template (frame.c:11033), which transfers the
    // target's ActiveUnit flag.
    let mut result = target.pick_axes_snapshot(target_axes)?;
    result.set_active_unit(target.get_active_unit());
    if let Some(t) = template {
        apply_template_overlay(t, template_axes, result.as_mut())?;
    }

    // Mapping construction: UnitMap if no permutation/selection,
    // PermMap otherwise. Mirrors frame.c:11062-11072.
    let map: DynMapping = if unit {
        Box::new(UnitMap::new(result_naxes))
    } else {
        Box::new(PermMap::new(&inperm, &outperm, &[])?)
    };

    // ActiveUnit adaptation: when both the template and the result use
    // active units, build a parallel units Mapping so unit differences
    // between the corresponding target and result axes become real
    // conversions (frame.c:11077-11154).
    let mut map = map;
    if let Some(t) = template {
        if t.get_active_unit() && result.get_active_unit() {
            // C's astOverlay overlays the template Axis attributes onto
            // the result Axis before the Mapping is built, so the
            // result carries the template's Unit on each associated
            // axis (axis.c:1270, frame.c:11042). Reproduce that Unit
            // transfer here so the per-axis units Mapping below sees the
            // target -> template unit difference. Only `BasicFrame`
            // reaches this tail with active units (SkyFrame forces the
            // flag off; SpecFrame / FluxFrame have their own SubFrame
            // paths), so the result is always a `BasicFrame`.
            if let Some(rb) = result
                .as_any_mut()
                .downcast_mut::<crate::frames::BasicFrame>()
            {
                for (result_axis, &template_axis) in
                    template_axes.iter().enumerate().take(result_naxes)
                {
                    if template_axis >= 0 {
                        let tu = t.effective_unit(template_axis as usize + 1);
                        if !tu.is_empty() {
                            rb.set_unit(result_axis + 1, tu)?;
                        }
                    }
                }
            }

            // Accumulate a parallel CmpMap of per-axis units Mappings.
            let mut umap: Option<DynMapping> = None;
            let mut uunit = true;
            let mut matched = true;

            for (result_axis, &outperm) in target_axes.iter().enumerate().take(result_naxes) {
                // Corresponding target axis: result_axis when the
                // permutation is the identity (unit), else the
                // outperm value (frame.c:11089-11094). target_axes is
                // the 0-based outperm with -1 for unassociated.
                let target_axis = if unit { result_axis as i32 } else { outperm };

                let aumap = if target_axis >= 0 {
                    let tu = target.effective_unit(target_axis as usize + 1);
                    let ru = result.effective_unit(result_axis + 1);
                    crate::units::unit_mapper(&tu, &ru)
                } else {
                    None
                };

                match aumap {
                    None => {
                        // No units mapping: the units could not be
                        // matched (frame.c:11113-11117).
                        matched = false;
                        break;
                    }
                    Some(am) => {
                        if !is_unit_map(am.as_ref()) {
                            uunit = false;
                        }
                        umap = Some(match umap {
                            None => am,
                            Some(prev) => Box::new(crate::mappings::CmpMap::new(
                                prev,
                                am,
                                crate::mappings::Composition::Parallel,
                            )?),
                        });
                    }
                }
            }

            // If the units could not be matched, annul the returned
            // Mapping (frame.c:11141: match=0 => *map annulled).
            if !matched {
                return Ok(None);
            }

            // If the assembled units CmpMap is not just a UnitMap, add it
            // in series after the permutation Mapping (frame.c:11147-11153).
            if !uunit {
                if let Some(um) = umap {
                    map = Box::new(crate::mappings::CmpMap::new(
                        map,
                        um,
                        crate::mappings::Composition::Series,
                    )?);
                }
            }
        }
    }

    Ok(Some((map, result)))
}

/// Whether `m` is a `UnitMap`.
fn is_unit_map(m: &dyn crate::mappings::Mapping) -> bool {
    m.as_any()
        .downcast_ref::<crate::mappings::UnitMap>()
        .is_some()
}

/// The AST class name for a `&dyn Frame`, used by the conversion-based
/// equality test's same-class gate.
///
/// Mirrors `astGetClass`: the string is the concrete AST class
/// (`"Frame"` for `BasicFrame`, `"SkyFrame"`, …). Returns `"Frame"` for
/// any unrecognised type so a future class still gates conservatively
/// against the base class rather than spuriously matching.
pub(crate) fn frame_class_name(frame: &dyn Frame) -> &'static str {
    use crate::frames::{DSBSpecFrame, FluxFrame, SkyFrame, SpecFluxFrame, SpecFrame, TimeFrame};
    let any = frame.as_any();
    if any.is::<crate::frames::BasicFrame>() {
        "Frame"
    } else if any.is::<SkyFrame>() {
        "SkyFrame"
    } else if any.is::<DSBSpecFrame>() {
        "DSBSpecFrame"
    } else if any.is::<SpecFrame>() {
        "SpecFrame"
    } else if any.is::<TimeFrame>() {
        "TimeFrame"
    } else if any.is::<FluxFrame>() {
        "FluxFrame"
    } else if any.is::<SpecFluxFrame>() {
        "SpecFluxFrame"
    } else if any.is::<CmpFrame>() {
        "CmpFrame"
    } else if any.is::<FrameSet>() {
        "FrameSet"
    } else {
        "Frame"
    }
}

/// Conversion-based Frame equality — the shared body of every leaf
/// Frame class's `equal`.
///
/// Mirrors C `Frame::Equal` (frame.c:3549): the two Frames are equal iff
/// they have the same class name, `astConvert(that, this, "")` succeeds,
/// and the resulting base→current Mapping simplifies to a UnitMap. No
/// Frame subclass overrides `Equal` in C, so Sky/Spec/Time/Flux/
/// SpecFlux/DSBSpec/CmpFrame all route through here; FrameSet and Region
/// keep their structural overrides.
///
/// This terminates: frame conversion never emits a `NormMap` (the only
/// mapping that re-enters `Frame::equal` during simplification), so the
/// `simplified()` call below cannot recurse back into `frames_equal`.
pub(crate) fn frames_equal(this: &dyn Frame, that: &dyn Frame) -> bool {
    // Same class check (astGetClass strcmp, frame.c:3605).
    if frame_class_name(this) != frame_class_name(that) {
        return false;
    }

    // astConvert( that, this, "" ): map from `that` to `this`
    // (frame.c:3612). On any error or no-conversion, the Frames are not
    // equal.
    let Ok(Some(fs)) = convert_frames(that, this, "") else {
        return false;
    };

    // Get the base->current Mapping and simplify it; equal iff UnitMap
    // (frame.c:3614-3616).
    let Ok(map) = fs.mapping(FrameRef::Base, FrameRef::Current) else {
        return false;
    };
    let Ok(simplified) = Mapping::simplified(map.as_ref()) else {
        return false;
    };
    is_unit_map(simplified.as_ref())
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
