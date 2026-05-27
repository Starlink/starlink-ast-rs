//! Port of `fitschan.c::OtherAxes` (~24030).
//!
//! Catch-all builder for WCS-frame axes not handled by `celestial_axes`
//! or `spectral_axes`. Stores `CTYPE` / `CTYPE` comment / `CUNIT` /
//! `CNAME` / `CRVAL` for each remaining axis and returns a `wcs→iwc`
//! mapping that subtracts the chosen CRVAL on each axis (identity on
//! the already-handled ones).
//!
//! ## Deliberate deferrals (mirrors plan §3.2)
//!
//! The C source has three special-case paths around line 24239:
//! `IsMapLinear` → linear ShiftMap, `LogAxis` → `-LOG` algorithm
//! mapping, `IsMapTab1D` → `-TAB` algorithm mapping. The plan
//! explicitly punts on `LogAxis` and `-TAB` and treats every axis as
//! linear (the same fallback C takes when none of the three match).
//! This is correct for any axis that is genuinely linear; it produces
//! a non-linear `pixel→IWC` for log/tab axes that `make_int_world`
//! will reject downstream — the same outcome the C library reaches
//! when it falls through to the "pretend linear" path. `IsMapLinear`
//! is similarly deferred: linearity is asserted by construction
//! rather than verified, and any failure surfaces later. Track these
//! deferrals as Group-8 follow-ups if they show up in wcsconv-failed
//! fixtures.

// Wired up by `fset_to_store` in Group 7; until then nothing in this
// module is reachable from outside the cfg(test) block, and rustc's
// dead-code lint would otherwise fire on every item. Index-style
// loops mirror the C control flow line-for-line; they are revisited
// in the post-validation idiomatic-Rust pass (see
// `feedback_c_first_then_idiomatic`).
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{CmpFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap};

/// Arguments threaded through the axis-classifier sequence
/// (`celestial_axes` → `spectral_axes` → `other_axes`). Each builder
/// updates `store` / `crvals` / `axis_done` in place; the returned
/// mapping is the WCS→IWC contribution from that builder alone.
pub(crate) struct OtherAxesArgs<'a> {
    /// FrameSet whose base frame is FITS pixel coords and whose
    /// current frame is FITS WCS coords.
    pub fs: &'a FrameSet,
    /// Image dimensions in pixels per base-frame axis. `None` for
    /// unknown dimensions (mirrors `dim[i] == AST__BAD` in C).
    pub dim: &'a [Option<f64>],
    /// 0-based FITS-axis index for each WCS-frame axis. Populated by
    /// the writer's axis-permutation logic before this function runs.
    pub wperm: &'a mut [usize],
    /// FITS keyword store; updated in place.
    pub store: &'a mut FitsStore,
    /// Per-axis default CRVAL; `Some` if a higher-priority builder
    /// (`celestial_axes`, `spectral_axes`) chose a value, else
    /// `None`. Read here.
    pub crvals: &'a mut [Option<f64>],
    /// Per-axis "this axis has been described" flags; updated in
    /// place.
    pub axis_done: &'a mut [bool],
}

/// Returns the `wcs→iwc` mapping for unclaimed (non-celestial,
/// non-spectral) axes. Stores CTYPE / CTYPE-comment / CUNIT / CNAME /
/// CRVAL into `args.store`. Mirrors `fitschan.c::OtherAxes`.
pub(crate) fn other_axes(args: OtherAxesArgs<'_>) -> AstResult<DynMapping> {
    let wcsfrm = args.fs.current();
    let nwcs = wcsfrm.naxes();

    // Count remaining axes; if all are claimed, the contribution is
    // identity. Mirrors fitschan.c:24165-24172.
    let nother = args.axis_done.iter().filter(|&&done| !done).count();
    if nother == 0 {
        return Ok(Box::new(UnitMap::new(nwcs)));
    }

    // pixel→wcs mapping and central-pixel WCS coordinates.
    // Mirrors fitschan.c:24178-24204.
    let map = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let npix = map.nin();

    let centre_pixel: Vec<f64> = (0..npix)
        .map(|i| match args.dim.get(i).copied().flatten() {
            Some(d) => (0.5 * d).floor(),
            None => 1.0,
        })
        .collect();
    let centre_in = PointSet::from_rows([centre_pixel]);
    let centre_out = map.transform(&centre_in, true)?;
    let centre_wcs: Vec<f64> = centre_out.point(0)[..nwcs].to_vec();

    // Walk the WCS axes, building a per-axis WCS→IWC ShiftMap and
    // composing the running result. Mirrors fitschan.c:24208-24398.
    let mut ret: Option<DynMapping> = None;
    for iax in 0..nwcs {
        if args.axis_done[iax] {
            continue;
        }

        let fits_i = args.wperm[iax];

        // Choose CRVAL: caller's override (if non-BAD) wins, else the
        // central-pixel WCS value. If both are BAD, abort the loop —
        // C: fitschan.c:24217-24223 (`break`).
        let chosen_crval = match args.crvals.get(iax).copied().flatten() {
            Some(v) if !is_bad(v) => Some(v),
            _ => {
                let v = centre_wcs[iax];
                if is_bad(v) { None } else { Some(v) }
            }
        };
        let Some(crval) = chosen_crval else {
            break;
        };

        args.store.crval[fits_i] = Some(crval);

        // Build the per-axis WCS→IWC mapping: ShiftMap(-crval) on the
        // single axis, embedded into a nwcs-axis Mapping by
        // parallel-composing with UnitMaps before/after. Mirrors
        // C OtherAxes lines 24240-24245 (the non-log non-tab path that
        // we always take — see module-level deferral note).
        let shift = ShiftMap::new(vec![-crval]);
        let axmap = add_unit_maps(Box::new(shift), iax, nwcs)?;

        // Compose with the running result. Mirrors C:24350-24356.
        ret = Some(match ret.take() {
            None => axmap,
            Some(prev) => Box::new(CmpMap::new(prev, axmap, Composition::Series)?),
        });

        // Set CTYPE / CTYPE comment / CUNIT / CNAME for this axis.
        // Mirrors C:24358-24390 — the per-axis attribute lookup uses
        // the WCS frame, with CmpFrame transparently routing to the
        // right sub-frame.
        let (sub_frame, sub_axis) = resolve_subframe(wcsfrm, iax);
        populate_axis_keywords(sub_frame, sub_axis, fits_i, args.store);

        args.axis_done[iax] = true;
    }

    // Simplify a non-trivial result; otherwise return a UnitMap.
    // Mirrors C:24411-24417.
    match ret {
        Some(m) => m.simplified(),
        None => Ok(Box::new(UnitMap::new(nwcs))),
    }
}

/// Build a Mapping with `nax` inputs/outputs in which `map`'s axes
/// occupy positions `iax..iax+map.nin()`, padded by UnitMaps on either
/// side. Mirrors `fitschan.c::AddUnitMaps` (~2983).
fn add_unit_maps(map: DynMapping, iax: usize, nax: usize) -> AstResult<DynMapping> {
    let nmap = map.nin();
    let mut ret = map;
    if iax > 0 {
        ret = Box::new(CmpMap::new(
            Box::new(UnitMap::new(iax)),
            ret,
            Composition::Parallel,
        )?);
    }
    if iax + nmap < nax {
        ret = Box::new(CmpMap::new(
            ret,
            Box::new(UnitMap::new(nax - iax - nmap)),
            Composition::Parallel,
        )?);
    }
    Ok(ret)
}

/// Resolve a WCS-frame axis index to the (sub-frame, 1-based axis)
/// pair that holds its label/symbol/unit. For a `CmpFrame`, recurses
/// into the appropriate child; for any other frame, returns it
/// unchanged with `iax + 1`. Mirrors how C's `astGetSymbol(wcsfrm, iax)`
/// dispatches through CmpFrame's class methods.
fn resolve_subframe(frame: &dyn Frame, iax: usize) -> (&dyn Frame, usize) {
    if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
        let n1 = cmp.naxes1();
        if iax < n1 {
            resolve_subframe(cmp.frame1(), iax)
        } else {
            resolve_subframe(cmp.frame2(), iax - n1)
        }
    } else {
        (frame, iax + 1)
    }
}

/// Populate `CTYPE` / `CTYPE` comment / `CUNIT` / `CNAME` for one
/// axis. Mirrors `fitschan.c::OtherAxes` lines 24359-24390 — symbol
/// becomes CTYPE (with `AXIS{n}` fallback), label becomes the comment
/// when explicitly set and distinct from symbol, unit becomes CUNIT
/// when set.
fn populate_axis_keywords(frame: &dyn Frame, axis: usize, fits_i: usize, store: &mut FitsStore) {
    use crate::frames::BasicFrame;

    let basic = match frame.as_any().downcast_ref::<BasicFrame>() {
        Some(b) => b,
        None => {
            // Frames without explicit symbol/label support fall back to
            // the generic AXISn naming. Real FITS-WCS-relevant frames
            // (Sky/Spec/Time) are claimed by the celestial/spectral
            // builders before reaching here; the fallback covers
            // user-defined frame subclasses.
            store.ctype[fits_i] = Some(format!("AXIS{axis}"));
            return;
        }
    };

    let symbol = basic.symbol(axis).ok().unwrap_or("");
    let label = basic.label(axis).ok().unwrap_or("");
    let label_set = basic.test_label(axis).ok().unwrap_or(false);

    let ctype = if !symbol.is_empty() {
        symbol.to_string()
    } else {
        format!("AXIS{axis}")
    };
    store.ctype[fits_i] = Some(ctype);

    if label_set && !label.is_empty() && label != symbol {
        store.ctype_com[fits_i] = Some(label.to_string());
        if store.cname[fits_i].is_none() {
            store.cname[fits_i] = Some(label.to_string());
        }
    }

    if basic.test_unit(axis).ok().unwrap_or(false)
        && let Ok(u) = basic.unit(axis)
        && !u.is_empty()
        && store.cunit[fits_i].is_none()
    {
        store.cunit[fits_i] = Some(u.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::BasicFrameBuilder;
    use crate::mappings::UnitMap;

    fn build_2axis_basic_frameset() -> FrameSet {
        let pixel = BasicFrameBuilder::new(2)
            .domain("GRID")
            .label(1, "pixel x")
            .label(2, "pixel y")
            .build()
            .unwrap();
        let wcs = BasicFrameBuilder::new(2)
            .label(1, "world x")
            .label(2, "world y")
            .symbol(1, "WX")
            .symbol(2, "WY")
            .unit(1, "m")
            .unit(2, "m")
            .build()
            .unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(wcs))
            .unwrap();
        fs
    }

    #[test]
    fn two_axes_basic_frame_unit_pixel_to_wcs() {
        let fs = build_2axis_basic_frameset();
        let dim = vec![Some(100.0), Some(200.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut crvals: Vec<Option<f64>> = vec![None, None];
        let mut axis_done = vec![false; 2];

        let mapping = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Both axes done.
        assert!(axis_done[0] && axis_done[1]);
        // CTYPE = symbol, CUNIT = unit, CTYPE-comment = label.
        assert_eq!(store.ctype[0].as_deref(), Some("WX"));
        assert_eq!(store.ctype[1].as_deref(), Some("WY"));
        assert_eq!(store.cunit[0].as_deref(), Some("m"));
        assert_eq!(store.cunit[1].as_deref(), Some("m"));
        assert_eq!(store.ctype_com[0].as_deref(), Some("world x"));
        assert_eq!(store.cname[0].as_deref(), Some("world x"));

        // CRVAL came from central-pixel WCS coords. With identity
        // pixel→wcs and dim=(100,200), centre is (floor(50), floor(100)) =
        // (50, 100), and identity transforms to (50, 100).
        assert_eq!(store.crval[0], Some(50.0));
        assert_eq!(store.crval[1], Some(100.0));

        // Mapping shape: 2-in / 2-out, and applying it to the centre
        // returns (0, 0) (i.e. the IWC origin lies at CRVAL).
        assert_eq!(mapping.nin(), 2);
        assert_eq!(mapping.nout(), 2);
        let pts = PointSet::from_rows([[50.0_f64, 100.0]]);
        let out = mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 0.0).abs() < 1e-12);
        assert!((p[1] - 0.0).abs() < 1e-12);
    }

    #[test]
    fn no_unclaimed_axes_returns_unit_map() {
        let fs = build_2axis_basic_frameset();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut crvals: Vec<Option<f64>> = vec![None, None];
        let mut axis_done = vec![true, true];

        let mapping = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Nothing was modified.
        assert!(store.ctype[0].is_none() && store.ctype[1].is_none());
        // Returned mapping is a UnitMap of the right size.
        assert_eq!(mapping.nin(), 2);
        assert_eq!(mapping.nout(), 2);
        assert!(mapping.as_any().is::<UnitMap>());
    }

    #[test]
    fn override_crval_takes_precedence() {
        let fs = build_2axis_basic_frameset();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut crvals: Vec<Option<f64>> = vec![Some(7.5), None];
        let mut axis_done = vec![false; 2];

        let _ = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Axis 0: input override.
        assert_eq!(store.crval[0], Some(7.5));
        // Axis 1: derived from central-pixel WCS (50, since dim=100).
        assert_eq!(store.crval[1], Some(50.0));
    }

    #[test]
    fn partial_pre_claimed_axes_produces_one_axis_mapping() {
        let fs = build_2axis_basic_frameset();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut crvals: Vec<Option<f64>> = vec![None, None];
        // Axis 0 already done by an earlier (hypothetical) builder.
        let mut axis_done = vec![true, false];

        let mapping = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Only axis 1 was processed.
        assert!(axis_done[0] && axis_done[1]);
        assert!(store.ctype[0].is_none(), "axis 0 was already done");
        assert_eq!(store.ctype[1].as_deref(), Some("WY"));
        assert_eq!(store.crval[1], Some(50.0));

        // The mapping is identity on axis 0 and shift(-50) on axis 1.
        let pts = PointSet::from_rows([[10.0_f64, 50.0]]);
        let out = mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 10.0).abs() < 1e-12);
        assert!((p[1] - 0.0).abs() < 1e-12);
    }

    #[test]
    fn unknown_dim_uses_pixel_one() {
        let fs = build_2axis_basic_frameset();
        let dim = vec![None, Some(200.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut crvals: Vec<Option<f64>> = vec![None, None];
        let mut axis_done = vec![false; 2];

        let _ = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Axis 0: dim unknown → centre = 1.0 → CRVAL = 1.0.
        assert_eq!(store.crval[0], Some(1.0));
        // Axis 1: dim=200 → centre = floor(100) = 100.
        assert_eq!(store.crval[1], Some(100.0));
    }

    #[test]
    fn fallback_axis_label_when_no_symbol() {
        let pixel = BasicFrameBuilder::new(1).build().unwrap();
        let wcs = BasicFrameBuilder::new(1).build().unwrap(); // no label, no symbol, no unit
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(1)), Box::new(wcs))
            .unwrap();

        let dim = vec![Some(50.0)];
        let mut wperm = vec![0_usize];
        let mut store = FitsStore::new(1);
        let mut crvals: Vec<Option<f64>> = vec![None];
        let mut axis_done = vec![false];

        let _ = other_axes(OtherAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            store: &mut store,
            crvals: &mut crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        assert_eq!(store.ctype[0].as_deref(), Some("AXIS1"));
        assert!(store.cunit[0].is_none());
        assert!(store.cname[0].is_none());
    }
}
