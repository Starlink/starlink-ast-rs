//! Port of `fitschan.c::OtherAxes` (~24030).
//!
//! Catch-all builder for WCS-frame axes not handled by `celestial_axes`
//! or `spectral_axes`. Stores `CTYPE` / `CTYPE` comment / `CUNIT` /
//! `CNAME` / `CRVAL` for each remaining axis and returns a `wcs→iwc`
//! mapping that subtracts the chosen CRVAL on each axis (identity on
//! the already-handled ones).
//!
//! ## TODO: branches not yet ported
//!
//! - **`IsMapTab1D`** (-TAB algorithm) — 1-D tabular look-up tables.
//!   Blocked on `FitsTable` infrastructure not yet ported. Tracked
//!   in [`docs/issues/fset_to_store-port-gaps.md`](../../../../../docs/issues/fset_to_store-port-gaps.md).
//!
//! `IsMapLinear` and `LogAxis` are now both wired into the
//! algorithm cascade below — see the in-line comment at the
//! per-axis decision.

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
use crate::frames::{Frame, FrameRef, FrameSet};
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap};

use super::is_map_linear::is_map_linear;
use super::log_axis::log_axis;

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
    // identity. Mirrors fitschan.c:24270-24277.
    let nother = args.axis_done.iter().filter(|&&done| !done).count();
    if nother == 0 {
        return Ok(Box::new(UnitMap::new(nwcs)));
    }

    // pixel→wcs mapping and central-pixel WCS coordinates.
    // Mirrors fitschan.c:24283-24309.
    let map = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let npix = map.nin();

    // Pixel-axis bounds for IsMapLinear / LogAxis. Mirrors C:24187-24191:
    // lbnd = 1.0, ubnd = dim[i] (500 when unknown).
    let lbnd_p: Vec<f64> = vec![1.0; npix];
    let ubnd_p: Vec<f64> = (0..npix)
        .map(|i| args.dim.get(i).copied().flatten().unwrap_or(500.0))
        .collect();

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
    // composing the running result. Mirrors fitschan.c:24313-24503.
    let mut ret: Option<DynMapping> = None;
    for iax in 0..nwcs {
        if args.axis_done[iax] {
            continue;
        }

        let fits_i = args.wperm[iax];

        // Choose CRVAL: caller's override (if non-BAD) wins, else the
        // central-pixel WCS value. If both are BAD, abort the loop —
        // C: fitschan.c:24322-24328 (`break`).
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

        // Decide which algorithm fits this axis. Mirrors the
        // priority cascade at fitschan.c:24344-24452:
        //   1. IsMapLinear → ShiftMap(-crval).
        //   2. else LogAxis → -LOG mapping.
        //   3. else -TAB (NOT YET PORTED — needs FitsTable).
        //   4. else fallback: pretend-linear ShiftMap (will produce
        //      a non-linear pixel→IWC that MakeIntWorld rejects).
        let mut log_axis_used = false;
        let axmap: DynMapping = if is_map_linear(map.as_ref(), &lbnd_p, &ubnd_p, iax)? {
            // Linear path. Mirrors C:24240-24245.
            let shift = ShiftMap::new(vec![-crval]);
            add_unit_maps(Box::new(shift), iax, nwcs)?
        } else if let Some(lm) = log_axis(map.as_ref(), iax, nwcs, &lbnd_p, &ubnd_p, crval)? {
            // -LOG algorithm path. Mirrors C:24252-24255.
            log_axis_used = true;
            lm
        } else {
            // Fallback: build the same ShiftMap as the linear case;
            // make_int_world will reject the resulting non-linear
            // pixel→IWC. Mirrors C:24341-24347.
            // -TAB algorithm not yet ported — see
            // fset_to_store-port-gaps.md.
            let shift = ShiftMap::new(vec![-crval]);
            add_unit_maps(Box::new(shift), iax, nwcs)?
        };

        // Compose with the running result. Mirrors C:24350-24356.
        ret = Some(match ret.take() {
            None => axmap,
            Some(prev) => Box::new(CmpMap::new(prev, axmap, Composition::Series)?),
        });

        // Set CTYPE / CTYPE comment / CUNIT / CNAME for this axis from
        // the WCS Frame's getters (fitschan.c `OtherAxes`).
        axis_keywords(wcsfrm, iax + 1, fits_i, args.store);

        // Append "-LOG" to CTYPE when the log path fired. Mirrors
        // C:24369-24373 (SetAlgCode "-LOG"). The C source also handles
        // "-TAB" here; that branch lands when the -TAB port goes in.
        if log_axis_used && let Some(ref ctype) = args.store.ctype[fits_i] {
            args.store.ctype[fits_i] = Some(set_alg_code(ctype, "-LOG"));
        }

        args.axis_done[iax] = true;
    }

    // Simplify a non-trivial result; otherwise return a UnitMap.
    // Mirrors C:24411-24417.
    match ret {
        Some(m) => m.simplify(),
        None => Ok(Box::new(UnitMap::new(nwcs))),
    }
}

/// Append a 4-character algorithm code (e.g. `"-LOG"`, `"-TAB"`) to a
/// CTYPE that is currently 4 characters of system name plus the dash
/// padding (or the pre-suffix string from the C source's
/// `SetAlgCode`). When the input already has a non-dash 5th-8th
/// character we replace the suffix; otherwise we append.
fn set_alg_code(ctype: &str, code: &str) -> String {
    debug_assert_eq!(code.len(), 4);
    // CTYPE is 8 characters in FITS-WCS: 4 for system + 4 for
    // algorithm. If the input is already 4 chars, append; if it's
    // 5+, replace the trailing 4 chars with the code (the C SetAlgCode
    // pads with '-' if needed).
    let mut buf: String = if ctype.len() <= 4 {
        // Pad to 4 with '-' then append.
        let mut s = ctype.to_string();
        while s.len() < 4 {
            s.push('-');
        }
        s
    } else {
        // Truncate at index 4.
        ctype.chars().take(4).collect()
    };
    buf.push_str(code);
    buf
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

/// Store CTYPE, its comment, CNAME and CUNIT for WCS axis `axis`
/// (1-based) of `wcsfrm`, as fitschan.c `OtherAxes` does: the Symbol is
/// the CTYPE, or `AXIS<n>` when it is blank; a Label that is set, not
/// blank and not the Symbol is the comment and CNAME, the comment being
/// `Type of co-ordinate on axis <n>` otherwise; and a Unit that is set is
/// the CUNIT.
fn axis_keywords(wcsfrm: &dyn Frame, axis: usize, fits_i: usize, store: &mut FitsStore) {
    let symbol = wcsfrm.symbol(axis).unwrap_or_default();
    let label = wcsfrm.label(axis).unwrap_or_default();
    store.ctype[fits_i] = Some(if symbol.is_empty() {
        format!("AXIS{axis}")
    } else {
        symbol.clone()
    });
    if !label.is_empty() && wcsfrm.test_label(axis).unwrap_or(false) && symbol != label {
        store.ctype_com[fits_i] = Some(label.clone());
        store.cname[fits_i] = Some(label);
    } else {
        store.ctype_com[fits_i] = Some(format!("Type of co-ordinate on axis {axis}"));
    }
    if wcsfrm.test_unit(axis).unwrap_or(false) {
        store.cunit[fits_i] = Some(wcsfrm.unit(axis).unwrap_or_default());
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

        // C `GetDefaultSymbol` (frame.c:5448) for a Frame with no
        // explicit Symbol and no Domain falls back to the Axis
        // class's "x" default symbol followed by `axis_index + 1`,
        // giving "x1" / "x2" / etc.
        assert_eq!(store.ctype[0].as_deref(), Some("x1"));
        assert!(store.cunit[0].is_none());
        assert!(store.cname[0].is_none());
    }
}
