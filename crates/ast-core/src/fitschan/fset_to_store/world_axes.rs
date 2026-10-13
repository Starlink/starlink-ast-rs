//! Port of `fitschan.c::WorldAxes` (~39755).
//!
//! Given a `pixel→IWC` mapping, returns for each WCS output axis the
//! 0-based pixel input axis that is most nearly parallel to it. Used
//! by `celestial_axes` to detect the auto axis-swap case (a celestial
//! rotation with LONPOLE = 90° transposes lon/lat in IWC, so the
//! FITS layout should match by swapping `wperm` / CRVAL / CTYPE / etc.
//! at the lon and lat slots).
//!
//! A Mapping with only a forward transformation, which can be written only
//! with the `-TAB` algorithm, is analysed by splitting off each input in
//! turn, when TabOK is positive.

#![allow(clippy::needless_range_loop)] // mirrors C
#![allow(dead_code)] // wired in by celestial_axes; tests reach in directly

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::mappings::Mapping;

use crate::fitschan::wcs_decompose::find_basis_vectors;

/// AST's `astEQUAL` tolerance: `1e5 * DBL_EPSILON`.
const AST_EQUAL_TOL: f64 = 1.0e5 * f64::EPSILON;

fn ast_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= AST_EQUAL_TOL * a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}

/// Returns `perm[i] = pixel-axis-index` for each WCS output axis `i`,
/// or `None` if the mapping cannot be analysed (e.g. all outputs are
/// non-finite).
///
/// `fitstol` is the linearity tolerance handed to
/// [`find_basis_vectors`]. The default writer setting is `0.1` (10 %
/// of a pixel), matching the C `FitsTol` attribute default.
///
/// Mirrors `fitschan.c::WorldAxes` (~39755). `tab_ok` is the FitsChan's
/// TabOK attribute, which lets a Mapping with only a forward transformation
/// be analysed.
pub(crate) fn world_axes(
    map: &dyn Mapping,
    dim: &[Option<f64>],
    fitstol: f64,
    tab_ok: i32,
) -> AstResult<Option<Vec<usize>>> {
    if map.has_forward() && map.has_inverse() {
        world_axes_bidirectional(map, dim, fitstol)
    } else if map.has_forward() && tab_ok > 0 {
        world_axes_forward(map)
    } else {
        Ok(None)
    }
}

/// C's `WorldAxes` for a Mapping with only a forward transformation: each
/// output takes the input that alone feeds it, found by splitting off each
/// input in turn, and an input that feeds several outputs keeps one, the
/// output of the same index if it is one of them, the others taking unused
/// inputs.
fn world_axes_forward(map: &dyn Mapping) -> AstResult<Option<Vec<usize>>> {
    let (nin, nout) = (map.nin(), map.nout());
    let mut perm: Vec<Option<usize>> = vec![None; nout];
    let mut ok = true;
    for j in 0..nin {
        if let Some((outs, smap)) = map.map_split_zero_based(&[j])? {
            for &o in &outs[..smap.nout()] {
                if perm[o].is_none() {
                    perm[o] = Some(j);
                } else {
                    ok = false;
                }
            }
        }
    }
    if !ok || perm.iter().any(Option::is_none) {
        return Ok(None);
    }
    let mut perm: Vec<usize> = perm.into_iter().map(|p| p.expect("checked")).collect();

    for j in 0..nin {
        let nused = perm.iter().filter(|&&p| p == j).count();
        if nused <= 1 {
            continue;
        }
        let retain = if perm.get(j) == Some(&j) {
            j
        } else {
            perm.iter().position(|&p| p == j).expect("used")
        };
        for i in 0..nout {
            if perm[i] != j || i == retain {
                continue;
            }
            match (0..nin).find(|j2| !perm.contains(j2)) {
                Some(j2) => perm[i] = j2,
                None => return Ok(None),
            }
        }
    }
    Ok(Some(perm))
}

/// C's `WorldAxes` for a Mapping defined in both directions.
fn world_axes_bidirectional(
    map: &dyn Mapping,
    dim: &[Option<f64>],
    fitstol: f64,
) -> AstResult<Option<Vec<usize>>> {
    let nin = map.nin();
    let nout = map.nout();
    if nin == 0 || nout == 0 {
        return Ok(None);
    }

    // Find a root pixel `g0` that has finite IWC. Mirrors C:39865.
    let basis = match find_basis_vectors(map, nin, nout, dim, fitstol) {
        Some(b) => b,
        None => return Ok(None),
    };
    let mut g0 = basis.g0;

    // Probe pixel positions: g0 and g0 + (1,1,...,1). Two points.
    // Mirrors C:39901-39905.
    let row0 = g0.clone();
    let row1: Vec<f64> = g0.iter().map(|&v| v + 1.0).collect();
    let pset1 = PointSet::from_rows([row0.clone(), row1]);
    let pset2 = map.transform(&pset1, true)?;
    let view2 = pset2.view();

    // For each WCS axis: compute a small increment `dw[i]` and grab
    // `w0[i]`. Mirrors C:39906-39920.
    let mut w0 = vec![0.0_f64; nout];
    // dw[i] = None means "bad — skip this axis". Mirrors C's AST__BAD.
    let mut dw: Vec<Option<f64>> = vec![None; nout];
    for i in 0..nout {
        let a = view2[[0, i]];
        let b = view2[[1, i]];
        if a.is_finite() && b.is_finite() {
            w0[i] = a;
            let mut delta = 0.1 * (b - a).abs();
            if delta <= 0.001 * a.abs() {
                delta = if a != 0.0 { 0.001 * a.abs() } else { 1.0 };
            }
            dw[i] = Some(delta);
        }
    }

    // Refine `g0` by inverse-transforming `w0` back to pixel coords.
    // Mirrors C:39928-39931 — corrects for any PermMap inversion drift.
    let pset1_back = map.transform(&pset2, false)?;
    let view1_back = pset1_back.view();
    for j in 0..nin {
        let v = view1_back[[0, j]];
        if v.is_finite() {
            g0[j] = v;
        }
    }

    // For each WCS axis, perturb `w0` along axis i by `dw[i]`, invert,
    // and record the tan(angle) and projection-length to each pixel
    // axis. Mirrors C:39935-39992.
    //
    // tn / wt are flattened (i, j) tables of size nout*nin.
    let mut tn: Vec<Option<f64>> = vec![None; nout * nin];
    let mut wt: Vec<f64> = vec![0.0_f64; nout * nin];
    let mut perm: Vec<isize> = vec![-1; nout];
    for i in 0..nout {
        let Some(d) = dw[i] else { continue };

        // Build w0 with axis i offset.
        let mut probe_w = w0.clone();
        probe_w[i] += d;
        let pset2_probe = PointSet::from_rows([probe_w]);
        let pset1_probe = map.transform(&pset2_probe, false)?;
        let view1_probe = pset1_probe.view();

        // Read the resulting pixel position.
        let mut pix = vec![0.0_f64; nin];
        let mut all_finite = true;
        for j2 in 0..nin {
            let v = view1_probe[[0, j2]];
            if !v.is_finite() {
                all_finite = false;
                break;
            }
            pix[j2] = v;
        }
        if !all_finite {
            continue;
        }

        for j in 0..nin {
            let mut s = 0.0_f64;
            let mut sj = 0.0_f64;
            for j2 in 0..nin {
                let dg = pix[j2] - g0[j2];
                if j2 != j {
                    s += dg * dg;
                } else {
                    sj = dg.abs();
                }
            }
            if sj != 0.0 {
                tn[i * nin + j] = Some(s.sqrt() / sj);
                wt[i * nin + j] = sj;
            }
        }
    }

    // Greedy assignment: pick the smallest tan, breaking ties by
    // largest weight. Mirrors C:39995-40046.
    loop {
        let mut tnmin: Option<f64> = None;
        let mut wtmax: f64 = 0.0;
        let mut imin = 0_usize;
        let mut jmin = 0_usize;
        for i in 0..nout {
            for j in 0..nin {
                if let Some(t) = tn[i * nin + j] {
                    let w = wt[i * nin + j];
                    // Mirror C:40008-40018 — strict `<` for "smaller",
                    // astEQUAL only kicks in on the elseif when the new
                    // value is greater-or-equal but tolerantly equal.
                    match tnmin {
                        None => {
                            tnmin = Some(t);
                            wtmax = w;
                            imin = i;
                            jmin = j;
                        }
                        Some(cur) if t < cur => {
                            tnmin = Some(t);
                            wtmax = w;
                            imin = i;
                            jmin = j;
                        }
                        Some(cur) if ast_equal(t, cur) && w > wtmax => {
                            wtmax = w;
                            imin = i;
                            jmin = j;
                        }
                        _ => {}
                    }
                }
            }
        }
        if tnmin.is_none() {
            break;
        }
        perm[imin] = jmin as isize;
        // Invalidate the chosen WCS axis row and pixel axis column.
        for i in 0..nout {
            for j in 0..nin {
                if i == imin || j == jmin {
                    tn[i * nin + j] = None;
                }
            }
        }
    }

    // Fill any remaining unassigned WCS axes with the lowest unused
    // pixel axes. Mirrors C:40051-40083.
    let mut j_next: usize = 0;
    while j_next < nin && perm.contains(&(j_next as isize)) {
        j_next += 1;
    }
    // Once the pixel axes run out, C goes on numbering past them, which is
    // how a degenerate WCS axis gets a FITS axis of its own.
    for i in 0..nout {
        if perm[i] == -1 {
            perm[i] = j_next as isize;
            j_next += 1;
            while j_next < nin && perm.contains(&(j_next as isize)) {
                j_next += 1;
            }
        }
    }

    Ok(Some(perm.into_iter().map(|p| p as usize).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{CmpMap, Composition, MatrixMap, PermMap, ShiftMap, UnitMap};

    #[test]
    fn unitmap_2d_returns_identity_perm() {
        let m = UnitMap::new(2);
        let dim = vec![Some(100.0), Some(100.0)];
        let perm = world_axes(&m, &dim, 0.1, 0).unwrap().unwrap();
        assert_eq!(perm, vec![0, 1]);
    }

    #[test]
    fn permmap_swaps_axes() {
        // PermMap that maps input axis 0 → output axis 1 and vice versa.
        // Output axis 0 should be associated with pixel axis 1, and
        // output axis 1 with pixel axis 0.
        let perm_map = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let dim = vec![Some(100.0), Some(100.0)];
        let perm = world_axes(&perm_map, &dim, 0.1, 0).unwrap().unwrap();
        assert_eq!(perm, vec![1, 0]);
    }

    #[test]
    fn shiftmap_keeps_identity() {
        let s = ShiftMap::new(vec![10.0, 20.0]);
        let dim = vec![Some(50.0), Some(50.0)];
        let perm = world_axes(&s, &dim, 0.1, 0).unwrap().unwrap();
        assert_eq!(perm, vec![0, 1]);
    }

    #[test]
    fn matrix_swap_via_off_diagonal() {
        // Matrix that swaps axes: [[0, 1], [1, 0]]
        let mat = ndarray::arr2(&[[0.0, 1.0], [1.0, 0.0]]);
        let mm = MatrixMap::new(mat).unwrap();
        let dim = vec![Some(100.0), Some(100.0)];
        let perm = world_axes(&mm, &dim, 0.1, 0).unwrap().unwrap();
        assert_eq!(perm, vec![1, 0]);
    }

    #[test]
    fn three_axis_partial_swap() {
        // Permute axes (0,1,2) → (1,2,0): output 0 ↔ pixel 1,
        // output 1 ↔ pixel 2, output 2 ↔ pixel 0.
        let pm = PermMap::new(&[3, 1, 2], &[2, 3, 1], &[]).unwrap();
        let dim = vec![Some(100.0); 3];
        let perm = world_axes(&pm, &dim, 0.1, 0).unwrap().unwrap();
        assert_eq!(perm, vec![1, 2, 0]);
    }

    #[test]
    fn series_chain_of_shift_and_swap() {
        // ShiftMap then a swap-matrix in series: outputs should still
        // map to pixels 1, 0.
        let shift = ShiftMap::new(vec![3.0, 7.0]);
        let mat = ndarray::arr2(&[[0.0, 1.0], [1.0, 0.0]]);
        let swap = MatrixMap::new(mat).unwrap();
        let chain = CmpMap::new(Box::new(shift), Box::new(swap), Composition::Series).unwrap();
        let dim = vec![Some(100.0), Some(100.0)];
        let perm = world_axes(&chain, &dim, 0.1, 0).unwrap().unwrap();
        assert_eq!(perm, vec![1, 0]);
    }
}
