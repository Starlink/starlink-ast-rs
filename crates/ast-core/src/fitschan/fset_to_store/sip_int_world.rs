//! Port of `fitschan.c::SIPIntWorld` (~line 28495).
//!
//! Receives a pixel→IWC `Mapping` that contains a 2-in/2-out SIP
//! `PolyMap` somewhere inside its celestial-axis sub-chain, and:
//!
//! 1. Splits out the celestial-axis sub-mapping using
//!    [`Mapping::map_split`] (mirrors `astMapSplit` of the inverted
//!    map on the lon/lat outputs).
//! 2. Walks the simplified series chain looking for a 2-in/2-out
//!    `PolyMap`.
//! 3. Verifies the pre-PolyMap mapping is a pure shift (with optional
//!    isotropic scaling) and the post-PolyMap mapping is purely
//!    linear with no shift.
//! 4. Factors any scaling into the PolyMap via [`scale_poly_inputs`]
//!    and decomposes the result via [`analyse_poly`] into shift /
//!    canonical SIP / matrix segments.
//! 5. Stores the resulting CRPIX, CD matrix, and SIP A/B/AP/BP
//!    coefficients in the supplied [`FitsStore`]. Appends `-SIP` to
//!    the celestial CTYPE values.
//! 6. Returns a fresh `Mapping` equivalent to the input but with the
//!    PolyMap replaced by its linear flank, so `make_int_world` can
//!    re-extract CRPIX / CD on the celestial axes from the linear
//!    chain.
//!
//! Returns `None` if any of the structural checks fail (wrong shape,
//! rotation in the lower flank, shift in the upper flank, no PolyMap
//! present, etc.). In that case the FitsStore is left untouched.

#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::mappings::helpers::analyse_poly::analyse_poly;
use crate::mappings::helpers::scale_poly_inputs::scale_poly_inputs;
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, PermMap, PolyMap, ShiftMap, UnitMap,
};

use crate::constants::BAD_VALUE;

/// Outputs returned by [`sip_int_world`] on success.
///
/// `inaxes` are the two 0-based input indices of the *original* map
/// that feed the celestial output pair (mirrors C's returned
/// `inaxes[2]`). `crpix` is in pixel coords (the inverse of the
/// stripped chain at IWC origin). `cd[0..4]` is row-major
/// `(CDlon_j1, CDlon_j2, CDlat_j1, CDlat_j2)` where `j1 = inaxes[0]`
/// and `j2 = inaxes[1]`.
#[derive(Debug)]
pub(crate) struct SipResult {
    /// The supplied mapping with its celestial-axis PolyMap removed
    /// and replaced by the linear flanks (and any non-celestial axes
    /// preserved). Caller can re-fit CRPIX / CD on this stripped form.
    pub stripped: DynMapping,
    /// The two 0-based input indices that feed the celestial outputs.
    pub in_axes: [usize; 2],
    /// CRPIX values for `in_axes[0]` and `in_axes[1]` (pixel coords).
    pub crpix: [f64; 2],
    /// CD matrix elements: `[CDlon_j1, CDlon_j2, CDlat_j1, CDlat_j2]`.
    pub cd: [f64; 4],
}

/// Linear-fit gates from `astLinearApprox`.
const LINFIT_TOL: f64 = 1.0e-7;

/// Default sample-box edge when `dim` is unknown — mirrors C's
/// `if (ubnd == AST__BAD) ubnd = 1000.0`.
const DEFAULT_DIM: f64 = 1000.0;

/// Port of `fitschan.c::SIPIntWorld`.
///
/// `map` — pixel→IWC mapping.
/// `tol` — pixel-space tolerance for the linear-approximation gate.
/// `lonax` / `latax` — 0-based output indices of celestial lon / lat.
/// `dim` — pixel-axis lengths (`None` for unknown).
/// `store` — FitsStore to write SIP A/B/AP/BP coefficients into and
///   to which the `-SIP` CTYPE suffix is appended.
///
/// On success returns `Some(SipResult)`. On any structural failure
/// returns `None`; the store is mutated only when `Some` is returned.
pub(crate) fn sip_int_world(
    map: &dyn Mapping,
    tol: f64,
    lonax: usize,
    latax: usize,
    store: &mut FitsStore,
    dim: &[Option<f64>],
) -> AstResult<Option<SipResult>> {
    let nin = map.nin();
    // C bug: uses Nin twice. We mirror the bug to keep semantics
    // bit-identical.
    let nout = nin;

    // Both transformations must be defined (mirrors C:28663).
    if !map.has_forward() || !map.has_inverse() {
        return Ok(None);
    }

    // Step 1: invert the supplied map and split on the celestial output
    // pair (which after inversion are inputs lonax/latax of the inverse).
    // Then invert it back.
    //
    // We can do this without mutating the supplied map by working on a
    // clone. Mirrors C's astInvert/astMapSplit/astInvert dance, but
    // structural rather than stateful.
    let mut inverted = map.clone_box();
    inverted.invert();
    // 0-based axis indices throughout this port (matches C's protected
    // astMapSplit), so call the zero-based method directly.
    let split = inverted.map_split_zero_based(&[lonax, latax]);
    drop(inverted); // free clone before next step.

    let (inax1, mut tmap1) = match split {
        Some((axes, m)) if m.nout() == 2 => (axes, m),
        _ => return Ok(None),
    };
    // tmap1 is currently the inverse direction; flip it back so
    // tmap1 maps inax1 inputs (pixel) → (lon, lat) outputs.
    tmap1.invert();

    if inax1.len() != 2 {
        return Ok(None);
    }
    let in_axes = [inax1[0], inax1[1]];

    // Step 2: simplify and flatten the celestial-axis sub-mapping
    // into a list of series mappings.
    //
    // C reaches this through `astSimplify` before `astMapList`, and the
    // reduction that matters here — consolidating a PolyMap's duplicate terms
    // so the SIP identity term is folded into one coefficient — happens in the
    // merge loop.
    let smap = tmap1.simplify()?;
    let chain = crate::mappings::simplify::flatten(smap.as_ref(), false, true)?;

    // Step 3: locate the first 2-in/2-out PolyMap in the chain.
    let mut polymap: Option<PolyMap> = None;
    let mut imap_pm: usize = 0;
    for (i, (m, inv, _)) in chain.iter().enumerate() {
        if let Some(pm) = m.as_any().downcast_ref::<PolyMap>() {
            if pm.nin() == 2 && pm.nout() == 2 {
                let mut clone = pm.clone();
                if *inv {
                    clone.invert();
                }
                polymap = Some(clone);
                imap_pm = i;
                break;
            }
        }
    }
    let mut polymap = match polymap {
        Some(p) => p,
        None => return Ok(None),
    };

    // Step 4: assemble pre-PolyMap chain into map_lower (UnitMap if empty)
    // and post-PolyMap chain into map_upper (UnitMap if empty). Each
    // sub-chain may itself need its inv flag honoured.
    let mut map_lower: Option<DynMapping> = None;
    for (m, inv, _) in chain.iter().take(imap_pm) {
        let mut piece = m.clone_box();
        if *inv {
            piece.invert();
        }
        map_lower = Some(match map_lower {
            None => piece,
            Some(prev) => Box::new(CmpMap::new(prev, piece, Composition::Series)?),
        });
    }
    let mut map_upper: Option<DynMapping> = None;
    for (m, inv, _) in chain.iter().skip(imap_pm + 1) {
        let mut piece = m.clone_box();
        if *inv {
            piece.invert();
        }
        map_upper = Some(match map_upper {
            None => piece,
            Some(prev) => Box::new(CmpMap::new(prev, piece, Composition::Series)?),
        });
    }
    let mut map_lower: DynMapping = map_lower.unwrap_or_else(|| Box::new(UnitMap::new(2)));
    let mut map_upper: DynMapping = map_upper.unwrap_or_else(|| Box::new(UnitMap::new(2)));

    // Sanity-check shapes (mirrors C:28759).
    if map_lower.nin() != 2
        || map_lower.nout() != 2
        || map_upper.nin() != 2
        || map_upper.nout() != 2
    {
        return Ok(None);
    }

    // Step 5: linear fit of map_lower over [(0, 0), (dim_x, dim_y)].
    let (ubnd_x, ubnd_y) = sample_ubnd(dim);
    let lbnd = vec![0.0_f64, 0.0];
    let ubnd = vec![ubnd_x, ubnd_y];

    let lower_fit = match map_lower.linear_approx(&lbnd, &ubnd, tol) {
        Some(f) => f,
        None => return Ok(None),
    };
    // fit layout: shift[0..2] | matrix[0..4] (row-major nout×nin).
    let lower_shift_x = lower_fit.shift[0];
    let lower_shift_y = lower_fit.shift[1];
    // matrix[i, j] at row-major lower_fit.matrix[i*nin + j]; with
    // nin=2, lower_matrix [m00, m01, m10, m11] = [fit[2], fit[3],
    // fit[4], fit[5]] in C's `fit` indexing.
    let m00 = lower_fit.matrix[0];
    let m01 = lower_fit.matrix[1];
    let m10 = lower_fit.matrix[2];
    let m11 = lower_fit.matrix[3];

    let scaling = (m00 - 1.0).abs() > LINFIT_TOL || (m11 - 1.0).abs() > LINFIT_TOL;
    let rotation = m01.abs() > LINFIT_TOL || m10.abs() > LINFIT_TOL;
    if rotation {
        return Ok(None);
    }

    let mut shift_x = lower_shift_x;
    let mut shift_y = lower_shift_y;
    if scaling {
        // Replace map_lower with a pure ShiftMap; absorb the scaling
        // into a re-built PolyMap via scale_poly_inputs. Mirrors
        // fitschan.c:28798-28815.
        if m00 == 0.0 || m11 == 0.0 {
            return Ok(None);
        }
        shift_x /= m00;
        shift_y /= m11;
        map_lower = Box::new(ShiftMap::new(vec![shift_x, shift_y]));

        let scales = [m00, m11];
        polymap = match scale_poly_inputs(&polymap, &scales)? {
            Some(p) => p,
            None => return Ok(None),
        };
    }

    // Step 6: AnalysePoly factors leading shift / canonical SIP /
    // trailing matrix from the PolyMap. Mirrors fitschan.c:28825.
    if let Some(decomp) = analyse_poly(&polymap)? {
        // map_lower ∘ map1 (shift)
        let map1: DynMapping = Box::new(decomp.map1);
        map_lower = Box::new(CmpMap::new(map_lower, map1, Composition::Series)?);

        // map3 (matrix or unit) ∘ map_upper
        let map3: DynMapping = match decomp.map3_matrix {
            Some(mm) => Box::new(mm),
            None => Box::new(UnitMap::new(2)),
        };
        map_upper = Box::new(CmpMap::new(map3, map_upper, Composition::Series)?);

        // Replace polymap with the canonical-form middle.
        polymap = decomp.map2;
    }

    // Step 7: linear fit of map_upper over [-ubnd, ubnd]. Mirrors
    // fitschan.c:28837. Must have negligible shift.
    let lbnd_u = vec![-ubnd_x, -ubnd_y];
    let upper_fit = match map_upper.linear_approx(&lbnd_u, &ubnd, tol) {
        Some(f) => f,
        None => return Ok(None),
    };
    if upper_fit.shift[0].abs() > LINFIT_TOL || upper_fit.shift[1].abs() > LINFIT_TOL {
        return Ok(None);
    }

    // Step 8: handle non-celestial trailing axes. Mirrors
    // fitschan.c:28852-28877. When the supplied mapping has more than
    // the two celestial outputs, split out a sub-mapping that produces
    // the remaining (non-celestial) outputs so it can be put in
    // parallel with the (cleaned) celestial 2-in/2-out chain when the
    // result mapping is rebuilt.
    //
    // Note: per the C bug at fitschan.c:28673-28674 (`nout = astGetNin(map)`)
    // the local `nout` here equals `nin`; we mirror the bug.
    let mut inax2: Option<Vec<usize>> = None;
    let mut tmap2: Option<DynMapping> = None;
    if nout > 2 {
        let mut outrem: Vec<usize> = Vec::with_capacity(nout - 2);
        for iout in 0..nout {
            if iout != lonax && iout != latax {
                outrem.push(iout);
            }
        }

        // Mirrors C's astInvert/astMapSplit/astInvert dance: invert
        // map, split on `outrem` (now inputs of the inverse), invert
        // map back. We work on a clone so we don't mutate the caller's
        // mapping.
        let mut inverted2 = map.clone_box();
        inverted2.invert();
        match inverted2.map_split_zero_based(&outrem) {
            Some((axes, mut t2)) => {
                t2.invert();
                inax2 = Some(axes);
                tmap2 = Some(t2);
            }
            None => return Ok(None),
        }
    } else if nout != 2 || nin != 2 {
        return Ok(None);
    }

    // Step 9: CRPIX = pixel coords of IWC origin via the simplified
    // chain (smap, before stripping). Mirrors fitschan.c:29180-29185.
    let zero_in = PointSet::from_rows([[0.0_f64, 0.0]]);
    let crpix_pts = smap.transform(&zero_in, false)?;
    let crpix_arr = [crpix_pts.point(0)[0], crpix_pts.point(0)[1]];

    // The inverse transformation may be undefined at the IWC origin, in
    // which case there is no reference pixel and so the SIP conventions
    // cannot describe the celestial axes. Mirrors
    // fitschan.c:29187-29191.
    if crpix_arr[0] == BAD_VALUE || crpix_arr[1] == BAD_VALUE {
        return Ok(None);
    }

    // CD matrix from the upper fit.
    let cd_arr = [
        upper_fit.matrix[0],
        upper_fit.matrix[1],
        upper_fit.matrix[2],
        upper_fit.matrix[3],
    ];

    // Step 10: emit SIP A/B/AP/BP from the canonical PolyMap. Mirrors
    // fitschan.c:28893-28960.
    emit_sip_coeffs(&polymap, &in_axes, store);

    // Step 11: append `-SIP` to celestial CTYPE. Mirrors C:28966-28983.
    append_sip_suffix(&mut store.ctype[latax]);
    append_sip_suffix(&mut store.ctype[lonax]);

    // Step 12: build the stripped result Mapping.
    // result = map_lower ∘ map_upper, then sandwich PermMaps to
    // restore the original input/output axis order.
    let mut result: DynMapping = Box::new(CmpMap::new(map_lower, map_upper, Composition::Series)?);

    if let Some(t2) = tmap2 {
        result = Box::new(CmpMap::new(result, t2, Composition::Parallel)?);
    }

    // Output PermMap (forward: from sub-mapping's output order back
    // to original). Mirrors fitschan.c:29022-29050.
    let inax2_slice: &[usize] = inax2.as_deref().unwrap_or(&[]);
    let pm_out = build_perm_outputs(nout, lonax, latax, inax2_slice)?;
    result = Box::new(CmpMap::new(result, Box::new(pm_out), Composition::Series)?);

    // Input PermMap. Mirrors fitschan.c:29052-29080.
    let pm_in = build_perm_inputs(nin, &in_axes, inax2_slice)?;
    result = Box::new(CmpMap::new(Box::new(pm_in), result, Composition::Series)?);

    Ok(Some(SipResult {
        stripped: result,
        in_axes,
        crpix: crpix_arr,
        cd: cd_arr,
    }))
}

/// Pick the upper-bound corners for the linear-approximation sample
/// box. `dim[0]` / `dim[1]` are the celestial pixel-axis lengths
/// (`None` when unknown — falls back to the C default of 1000).
fn sample_ubnd(dim: &[Option<f64>]) -> (f64, f64) {
    let x = dim.first().and_then(|d| *d).unwrap_or(DEFAULT_DIM);
    let y = dim.get(1).and_then(|d| *d).unwrap_or(DEFAULT_DIM);
    (x, y)
}

/// Append "-SIP" to the supplied CTYPE, padding/truncating to leave
/// the suffix at offset 8 (matches the C `strcpy(buf+8, "-SIP")`
/// idiom). If the slot is `None`, leaves it `None`.
fn append_sip_suffix(slot: &mut Option<String>) {
    if let Some(orig) = slot {
        let mut buf = orig.clone();
        if buf.len() > 8 {
            buf.truncate(8);
        }
        // C overwrites index 8 with '\0' first, then strcpy "-SIP" —
        // i.e. CTYPE[..8] stays as-is even if shorter than 8 (with
        // trailing zero padding the buffer in C). We match the
        // observable behaviour: keep the prefix exactly, regardless of
        // length, then append "-SIP".
        buf.push_str("-SIP");
        *orig = buf;
    }
}

/// Iterate the polymap's forward (and explicit-inverse, if any) terms
/// and write SIP A/B/AP/BP coefficients into the FitsStore.
///
/// The branch on `inaxes[0] < inaxes[1]` mirrors fitschan.c:28907-28933:
/// when the celestial axes were swapped during `astMapSplit` the
/// per-output and per-input role of (i, jm) flips.
fn emit_sip_coeffs(polymap: &PolyMap, inaxes: &[usize; 2], store: &mut FitsStore) {
    // Forward terms first (drives A / B), then explicit inverse if
    // present (drives AP / BP). When iter_inverse is on but no
    // explicit inverse coefficients are stored, AP / BP are not
    // emitted — matching C's `astPolyCoeffs(polymap, 0, ...)`
    // returning ncoeff = 0 when no inverse was supplied.
    let lon_first = inaxes[0] < inaxes[1];

    // Forward — A / B.
    emit_one_direction(
        polymap.forward_terms(),
        lon_first,
        true, // forward
        store,
    );

    // Inverse — AP / BP. Skip if no explicit inverse coeffs.
    if let Some(inv) = polymap.inverse_terms() {
        emit_one_direction(inv, lon_first, false, store);
    }
}

/// One direction of the SIP coefficient emission loop. `forward`
/// distinguishes A/B vs AP/BP. `lon_first` is `true` when the
/// celestial axes are in their natural lon-then-lat order; `false`
/// flips the role of the two output axes (mirroring C's
/// `inaxes[0] < inaxes[1]` switch).
fn emit_one_direction(
    terms: &[crate::mappings::PolyTerm],
    lon_first: bool,
    forward: bool,
    store: &mut FitsStore,
) {
    if terms.is_empty() {
        return;
    }

    // Per-output (i, jm) max powers.
    let mut a_imax = 0_usize;
    let mut a_jmmax = 0_usize;
    let mut b_imax = 0_usize;
    let mut b_jmmax = 0_usize;

    for t in terms {
        let (i, jm) = if lon_first {
            (t.powers[0], t.powers[1])
        } else {
            (t.powers[1], t.powers[0])
        };
        let to_a = if lon_first {
            t.out_axis == 1
        } else {
            t.out_axis == 2
        };
        if to_a {
            if i > a_imax {
                a_imax = i;
            }
            if jm > a_jmmax {
                a_jmmax = jm;
            }
        } else {
            if i > b_imax {
                b_imax = i;
            }
            if jm > b_jmmax {
                b_jmmax = jm;
            }
        }
    }

    // Initialise the storage tables with BAD_VALUE for every (i, jm)
    // ≤ (imax, jmmax), then overwrite with real values.
    let a_size = a_imax + 1;
    let a_jsize = a_jmmax + 1;
    let b_size = b_imax + 1;
    let b_jsize = b_jmmax + 1;

    let (a_table, b_table) = if forward {
        (&mut store.sip_a, &mut store.sip_b)
    } else {
        (&mut store.sip_ap, &mut store.sip_bp)
    };
    *a_table = vec![vec![BAD_VALUE; a_jsize]; a_size];
    *b_table = vec![vec![BAD_VALUE; b_jsize]; b_size];

    // Overwrite with real values, subtracting 1.0 from the linear
    // identity terms (per fitschan.c:28958: SIP is an additive
    // correction, so the explicit `1*x_i` linear term gets dropped).
    for t in terms {
        let to_a = if lon_first {
            t.out_axis == 1
        } else {
            t.out_axis == 2
        };
        let (i, jm) = if lon_first {
            (t.powers[0], t.powers[1])
        } else {
            (t.powers[1], t.powers[0])
        };

        let mut val = t.coeff;
        // Subtract the +1 identity terms.
        if (t.out_axis == 1 && i == 1 && jm == 0) || (t.out_axis == 2 && i == 0 && jm == 1) {
            val -= 1.0;
        }
        if val == 0.0 || val == BAD_VALUE {
            continue;
        }

        if to_a {
            a_table[i][jm] = val;
        } else {
            b_table[i][jm] = val;
        }
    }

    // Compute the FITS ORDER for each populated table — the maximum
    // i+jm across non-zero / non-BAD entries.
    let a_order = compute_fits_order(a_table);
    let b_order = compute_fits_order(b_table);

    if forward {
        store.sip_a_order = a_order;
        store.sip_b_order = b_order;
    } else {
        store.sip_ap_order = a_order;
        store.sip_bp_order = b_order;
    }
}

/// FITS ORDER = max(i + j) over non-zero, non-BAD entries.
fn compute_fits_order(table: &[Vec<f64>]) -> Option<usize> {
    let mut order = None::<usize>;
    let mut any = false;
    for (i, row) in table.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            if v == BAD_VALUE || v == 0.0 {
                continue;
            }
            any = true;
            let s = i + j;
            order = Some(order.map_or(s, |o| o.max(s)));
        }
    }
    if any { order } else { Some(0) }
}

/// Build the output-side PermMap for the stripped Mapping. Mirrors
/// fitschan.c:29024-29044. The PermMap permutes the sub-mapping's
/// outputs back into the original `nout`-sized output order.
///
/// `lonax` / `latax` are the original 0-based output indices of the
/// celestial axes. `outrem` lists the other 0-based output indices
/// (only non-empty when nout > 2; empty for the 2D fixture).
fn build_perm_outputs(
    nout: usize,
    lonax: usize,
    latax: usize,
    outrem: &[usize],
) -> AstResult<PermMap> {
    // inperm: nout entries, drives the inverse direction. Following the
    // C code's external (1-based) convention, position pos in the
    // stripped sub-mapping's output corresponds to original output
    // index `inperm1[pos]`.
    let mut inperm: Vec<i64> = vec![0; nout];
    let mut outperm: Vec<i64> = vec![0; nout];

    // First two slots: lon/lat.
    inperm[0] = (lonax as i64) + 1;
    inperm[1] = (latax as i64) + 1;
    outperm[lonax] = 1;
    outperm[latax] = 2;

    for (k, &orig_out) in outrem.iter().enumerate() {
        inperm[k + 2] = (orig_out as i64) + 1;
        outperm[orig_out] = (k + 2) as i64 + 1;
    }

    PermMap::new(&inperm, &outperm, &[])
}

/// Build the input-side PermMap. Mirrors fitschan.c:29052-29078.
///
/// `inaxes` are the 0-based input indices of the original mapping
/// that produce the celestial outputs. `inax2` lists the 0-based
/// input indices for the trailing-output sub-mapping (empty for 2D).
fn build_perm_inputs(nin: usize, inaxes: &[usize; 2], inax2: &[usize]) -> AstResult<PermMap> {
    let mut inperm: Vec<i64> = vec![0; nin];
    let mut outperm: Vec<i64> = vec![0; nin];

    outperm[0] = (inaxes[0] as i64) + 1;
    outperm[1] = (inaxes[1] as i64) + 1;
    inperm[inaxes[0]] = 1;
    inperm[inaxes[1]] = 2;

    for (k, &orig_in) in inax2.iter().enumerate() {
        outperm[k + 2] = (orig_in as i64) + 1;
        inperm[orig_in] = (k + 2) as i64 + 1;
    }

    PermMap::new(&inperm, &outperm, &[])
}

#[cfg(test)]
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;
    use crate::mappings::{MatrixMap, PolyTerm, ShiftMap};
    use ndarray::Array2;

    /// Captured fixtures from
    /// `tools/oracle_fixtures/sip_int_world.c`. Regenerate by running
    /// `make sip_int_world && ./sip_int_world > /tmp/captured.rs`
    /// from `tools/oracle_fixtures/` and copying the output here.
    mod oracle {
        pub const BAD: f64 = -f64::MAX;

        pub mod sip_2d {
            use super::BAD;
            pub const HAS_RESULT: bool = true;
            pub const NIN: usize = 2;
            pub const NOUT: usize = 2;
            pub const LONAX: usize = 0;
            pub const LATAX: usize = 1;
            pub const TOL: f64 = 0.001;
            pub const IN_AXES: [usize; 2] = [0, 1];
            pub const CRPIX: [f64; 2] = [128.0, 128.0];
            pub const CD: [f64; 4] = [
                0.00024975688027235502,
                0.000230177809743655,
                0.00023042851926541701,
                -0.00024996577057658702,
            ];

            pub const SIP_A_ORDER: Option<usize> = Some(3);
            pub const SIP_A: &[(usize, usize, f64)] = &[
                (0, 2, 2.9656e-06),
                (0, 3, 3.7745999999999998e-09),
                (1, 1, 2.1886e-05),
                (1, 2, -1.6847e-07),
                (2, 0, -2.3862999999999998e-05),
                (2, 1, -8.5609999999999997e-09),
                (3, 0, -1.4172e-07),
            ];
            pub const SIP_B_ORDER: Option<usize> = Some(3);
            pub const SIP_B: &[(usize, usize, f64)] = &[
                (0, 2, 2.3099999999999999e-05),
                (0, 3, -1.6168e-07),
                (1, 1, -2.4386e-05),
                (1, 2, -5.7813000000000003e-09),
                (2, 0, 2.1196999999999998e-06),
                (2, 1, -1.6583e-07),
                (3, 0, -2.0249000000000002e-08),
            ];
            pub const SIP_AP_ORDER: Option<usize> = Some(3);
            pub const SIP_AP: &[(usize, usize, f64)] = &[
                (0, 1, -6.4275000000000001e-07),
                (0, 2, -2.9425000000000002e-06),
                (0, 3, -3.5819999999999998e-09),
                (1, 0, -1.4897000000013705e-05),
                (1, 1, -2.2249999999999999e-05),
                (1, 2, 1.7195e-07),
                (2, 0, 2.4145999999999999e-05),
                (2, 1, 6.7089999999999999e-09),
                (3, 0, 1.4492e-07),
            ];
            pub const SIP_BP_ORDER: Option<usize> = Some(3);
            pub const SIP_BP: &[(usize, usize, f64)] = &[
                (0, 1, -1.6588000000039571e-05),
                (0, 2, -2.3424000000000001e-05),
                (0, 3, 1.6509999999999999e-07),
                (1, 0, -2.6782999999999998e-06),
                (1, 1, 2.4752999999999998e-05),
                (1, 2, 3.8916999999999997e-09),
                (2, 0, -2.1509999999999998e-06),
                (2, 1, 1.6999999999999999e-07),
                (3, 0, 2.0482000000000001e-08),
            ];
            pub const SAMPLE_FORWARD: &[([f64; 2], [f64; 2])] = &[
                ([1.0, 1.0], [-0.060951705632033275, 0.0024812309165185911]),
                ([64.0, 64.0], [-0.030715820161024643, 0.0012503840839148805]),
                ([128.0, 128.0], [0.0, 0.0]),
                (
                    [200.0, 200.0],
                    [0.034555297681152716, -0.0014066820944042407],
                ),
                (
                    [256.0, 256.0],
                    [0.061431640322049286, -0.0025007681678297609],
                ),
            ];

            // Silence unused warnings for fixtures not yet exercised
            // by a specific test.
            #[allow(dead_code)]
            pub const _UNUSED: f64 = BAD;
        }
    }

    /// SIP coefficients from the oracle fixture, packaged as
    /// PolyTerm sets for use in test-side construction of the
    /// synthetic chain. The forward polymap is `1*x_i + Σ A/B_i_j`,
    /// matching the fixture's `sip_fwd[]` layout in the .c source.
    fn sip_2d_fwd_terms() -> Vec<PolyTerm> {
        let mut t = vec![
            PolyTerm {
                coeff: 1.0,
                powers: vec![1, 0],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 1.0,
                powers: vec![0, 1],
                out_axis: 2,
            },
        ];
        // A coefficients: out_axis 1, powers (i, j).
        let a: &[(usize, usize, f64)] = &[
            (0, 2, 2.9656e-06),
            (0, 3, 3.7746e-09),
            (1, 1, 2.1886e-05),
            (1, 2, -1.6847e-07),
            (2, 0, -2.3863e-05),
            (2, 1, -8.561e-09),
            (3, 0, -1.4172e-07),
        ];
        for &(i, j, c) in a {
            t.push(PolyTerm {
                coeff: c,
                powers: vec![i, j],
                out_axis: 1,
            });
        }
        let b: &[(usize, usize, f64)] = &[
            (0, 2, 2.31e-05),
            (0, 3, -1.6168e-07),
            (1, 1, -2.4386e-05),
            (1, 2, -5.7813e-09),
            (2, 0, 2.1197e-06),
            (2, 1, -1.6583e-07),
            (3, 0, -2.0249e-08),
        ];
        for &(i, j, c) in b {
            t.push(PolyTerm {
                coeff: c,
                powers: vec![i, j],
                out_axis: 2,
            });
        }
        t
    }

    fn sip_2d_inv_terms() -> Vec<PolyTerm> {
        let mut t = vec![
            PolyTerm {
                coeff: 1.0,
                powers: vec![1, 0],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 1.0,
                powers: vec![0, 1],
                out_axis: 2,
            },
        ];
        let ap: &[(usize, usize, f64)] = &[
            (0, 1, -6.4275e-07),
            (0, 2, -2.9425e-06),
            (0, 3, -3.582e-09),
            (1, 0, -1.4897e-05),
            (1, 1, -2.225e-05),
            (1, 2, 1.7195e-07),
            (2, 0, 2.4146e-05),
            (2, 1, 6.709e-09),
            (3, 0, 1.4492e-07),
        ];
        for &(i, j, c) in ap {
            t.push(PolyTerm {
                coeff: c,
                powers: vec![i, j],
                out_axis: 1,
            });
        }
        let bp: &[(usize, usize, f64)] = &[
            (0, 1, -1.6588e-05),
            (0, 2, -2.3424e-05),
            (0, 3, 1.651e-07),
            (1, 0, -2.6783e-06),
            (1, 1, 2.4753e-05),
            (1, 2, 3.8917e-09),
            (2, 0, -2.151e-06),
            (2, 1, 1.7e-07),
            (3, 0, 2.0482e-08),
        ];
        for &(i, j, c) in bp {
            t.push(PolyTerm {
                coeff: c,
                powers: vec![i, j],
                out_axis: 2,
            });
        }
        t
    }

    /// CD matrix from the oracle fixture.
    fn cd_2d() -> [f64; 4] {
        [
            0.000249756880272355,
            0.000230177809743655,
            0.000230428519265417,
            -0.000249965770576587,
        ]
    }

    /// Build the synthetic SIP chain matching the oracle fixture:
    /// ShiftMap(-crpix) → PolyMap(SIP) → MatrixMap(CD).
    fn build_sip_2d_chain() -> DynMapping {
        let crpix = 128.0;
        let sm = ShiftMap::new(vec![-crpix, -crpix]);
        let pm = PolyMap::new(2, 2, &sip_2d_fwd_terms(), Some(&sip_2d_inv_terms())).unwrap();
        let cd = cd_2d();
        let mm = MatrixMap::new(
            Array2::from_shape_vec((2, 2), vec![cd[0], cd[1], cd[2], cd[3]]).unwrap(),
        )
        .unwrap();
        let s_then_p = CmpMap::new(Box::new(sm), Box::new(pm), Composition::Series).unwrap();
        let chain = CmpMap::new(Box::new(s_then_p), Box::new(mm), Composition::Series).unwrap();
        Box::new(chain)
    }

    #[test]
    fn sip_2d_extracts_oracle_values() {
        let chain = build_sip_2d_chain();
        let mut store = FitsStore::new(2);
        // Pre-populate CTYPE so the -SIP suffix has something to
        // append to, mirroring how the FsetToStore upstream call
        // populates these before MakeIntWorld dispatches into us.
        store.ctype[0] = Some("RA---TAN".into());
        store.ctype[1] = Some("DEC--TAN".into());

        let dim = vec![Some(256.0), Some(256.0)];
        let result = sip_int_world(chain.as_ref(), 0.001, 0, 1, &mut store, &dim)
            .unwrap()
            .expect("oracle says HAS_RESULT=true");

        assert_eq!(result.in_axes, oracle::sip_2d::IN_AXES);
        // CRPIX: the oracle returned [128, 128].
        approx_eq(result.crpix[0], oracle::sip_2d::CRPIX[0], 1e-6, "crpix[0]");
        approx_eq(result.crpix[1], oracle::sip_2d::CRPIX[1], 1e-6, "crpix[1]");
        // CD matrix: the oracle returned the original CD bit-for-bit.
        for k in 0..4 {
            approx_eq(
                result.cd[k],
                oracle::sip_2d::CD[k],
                1e-12,
                &format!("cd[{k}]"),
            );
        }

        // SIP coefficients.
        assert_eq!(store.sip_a_order, oracle::sip_2d::SIP_A_ORDER);
        assert_eq!(store.sip_b_order, oracle::sip_2d::SIP_B_ORDER);
        assert_eq!(store.sip_ap_order, oracle::sip_2d::SIP_AP_ORDER);
        assert_eq!(store.sip_bp_order, oracle::sip_2d::SIP_BP_ORDER);
        check_sip_table(&store.sip_a, oracle::sip_2d::SIP_A, "SIP_A");
        check_sip_table(&store.sip_b, oracle::sip_2d::SIP_B, "SIP_B");
        check_sip_table(&store.sip_ap, oracle::sip_2d::SIP_AP, "SIP_AP");
        check_sip_table(&store.sip_bp, oracle::sip_2d::SIP_BP, "SIP_BP");

        // CTYPE got -SIP suffix.
        assert_eq!(store.ctype[0].as_deref(), Some("RA---TAN-SIP"));
        assert_eq!(store.ctype[1].as_deref(), Some("DEC--TAN-SIP"));

        // Sample-forward equivalence on the stripped mapping.
        for &(input, expected) in oracle::sip_2d::SAMPLE_FORWARD {
            let pts = PointSet::from_rows([input]);
            let out = result.stripped.transform(&pts, true).unwrap();
            let p = out.point(0);
            approx_eq(
                p[0],
                expected[0],
                1e-9,
                &format!("stripped({:?}) [0]", input),
            );
            approx_eq(
                p[1],
                expected[1],
                1e-9,
                &format!("stripped({:?}) [1]", input),
            );
        }
    }

    #[test]
    fn non_sip_returns_none() {
        // Same chain without a PolyMap.
        let crpix = 128.0;
        let sm = ShiftMap::new(vec![-crpix, -crpix]);
        let cd = cd_2d();
        let mm = MatrixMap::new(
            Array2::from_shape_vec((2, 2), vec![cd[0], cd[1], cd[2], cd[3]]).unwrap(),
        )
        .unwrap();
        let chain: DynMapping =
            Box::new(CmpMap::new(Box::new(sm), Box::new(mm), Composition::Series).unwrap());

        let mut store = FitsStore::new(2);
        let dim = vec![Some(256.0), Some(256.0)];
        let result = sip_int_world(chain.as_ref(), 0.001, 0, 1, &mut store, &dim).unwrap();
        assert!(result.is_none(), "non-SIP chain should return None");
    }

    fn approx_eq(actual: f64, expected: f64, tol: f64, label: &str) {
        let diff = (actual - expected).abs();
        let rel = if expected.abs() > 1e-15 {
            diff / expected.abs()
        } else {
            diff
        };
        assert!(
            diff <= tol || rel <= tol,
            "{label}: actual={actual:e} expected={expected:e} diff={diff:e}",
        );
    }

    /// Verify each (i, j, val) in `expected` matches the corresponding
    /// `table[i][j]`, treating BAD_VALUE entries as "absent".
    fn check_sip_table(table: &[Vec<f64>], expected: &[(usize, usize, f64)], label: &str) {
        for &(i, j, want) in expected {
            let got = table
                .get(i)
                .and_then(|row| row.get(j))
                .copied()
                .unwrap_or(BAD_VALUE);
            assert!(
                (got - want).abs() < 1e-12 || ((got - want).abs() / want.abs()) < 1e-9,
                "{label}[{i}][{j}]: got {got:e}, want {want:e}",
            );
        }
    }
}
