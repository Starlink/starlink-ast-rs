//! Masking data arrays with Regions: C's `astMask<X>`.
//!
//! A Region class decides which pixels of a grid a mask sets, through
//! [`Region::mask_plan`]: region.c `Mask<X>` for most classes, and its own
//! method for PointList (pointlist.c `Mask<X>`), which sets the pixels its
//! points fall in. [`mask`] then sets those pixels of a data array of any
//! element type, as each C `Mask<X>` does for its own type.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::BasicFrameBuilder;
use crate::mappings::Mapping;
use crate::mappings::helpers::is_bad;
use crate::regions::{DynRegion, Region};

/// The pixels a mask sets, in the order of the vectorised array (first axis
/// varying fastest), and the number C's `astMask<X>` returns.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskPlan {
    pub(crate) set: Vec<bool>,
    pub(crate) count: usize,
}

/// Set to `val` the pixels of `data` that lie inside `region` (`inside`
/// true) or outside it, as C's `astMask<X>` does, and return the number of
/// pixels it reports setting.
///
/// `data` is a grid with pixel index bounds `lbnd..=ubnd` on each axis,
/// vectorised with the first axis varying fastest, whose pixel centres are
/// at integer grid coordinates. `map` takes positions in the Region's
/// Frame to grid coordinates; with no `map`, the Region is in grid
/// coordinates already. The grid must have as many axes as `map` has
/// outputs (or, with no `map`, as the Region), and `map` as many inputs as
/// the Region (AST__NGDIN); a lower bound above its upper bound is
/// AST__GBDIN. A data array of the wrong length is AST__DIMIN.
pub fn mask<T: Copy>(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
    data: &mut [T],
    val: T,
) -> AstResult<usize> {
    if ubnd.len() != lbnd.len() {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "astMask: {} lower bounds but {} upper bounds supplied",
                lbnd.len(),
                ubnd.len()
            ),
        ));
    }
    let plan = region.mask_plan(map, inside, lbnd, ubnd)?;
    if data.len() != plan.set.len() {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "astMask: the data array has {} elements but the grid has {}",
                data.len(),
                plan.set.len()
            ),
        ));
    }
    for (d, &s) in data.iter_mut().zip(&plan.set) {
        if s {
            *d = val;
        }
    }
    Ok(plan.count)
}

/// The Region in grid coordinates and the number of pixels in the grid,
/// after the checks every `Mask<X>` makes (region.c and pointlist.c).
fn grid_region(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    lbnd: &[i64],
    ubnd: &[i64],
) -> AstResult<(DynRegion, usize)> {
    let ndim = lbnd.len();
    let nax = region.naxes();
    let used = match map {
        Some(map) => {
            if map.nin() != nax {
                return Err(AstError::new(
                    ErrorCode::Ngdin,
                    format!(
                        "astMask: Bad number of mapping inputs ({}). The Region given \
                         requires {nax} coordinate values to specify a position.",
                        map.nin()
                    ),
                ));
            }
            if map.nout() != ndim {
                return Err(AstError::new(
                    ErrorCode::Ngdin,
                    format!(
                        "astMask: Bad number of mapping outputs ({}). The pixel grid \
                         requires {ndim} coordinate values to specify a position.",
                        map.nout()
                    ),
                ));
            }
            let grid = BasicFrameBuilder::new(ndim).domain("GRID").build()?;
            region.map_region(map, &grid)?
        }
        None => {
            if ndim != nax || ndim < 1 {
                return Err(AstError::new(
                    ErrorCode::Ngdin,
                    format!(
                        "astMask: Bad number of input grid dimensions ({ndim}). The \
                         Region given requires {nax} coordinate values to specify an \
                         input position."
                    ),
                ));
            }
            region.clone_region()
        }
    };
    for idim in 0..ndim {
        if lbnd[idim] > ubnd[idim] {
            return Err(AstError::new(
                ErrorCode::Gbdin,
                format!(
                    "astMask: Lower bound of input grid ({}) exceeds corresponding upper \
                     bound ({}). Error in input dimension {}.",
                    lbnd[idim],
                    ubnd[idim],
                    idim + 1
                ),
            ));
        }
    }
    let npix = lbnd
        .iter()
        .zip(ubnd)
        .map(|(l, u)| (u - l + 1) as usize)
        .product();
    Ok((used, npix))
}

/// region.c `Mask<X>`, the default of [`Region::mask_plan`].
///
/// Every pixel outside the Region's grid-coordinate bounding box (widened by
/// two pixels) is on the same side of the Region, so those are set together
/// when the side being masked is the side the box's outside is on. Within
/// the box, C resamples the data through the Region, negated when the
/// inside is masked, with nearest-neighbour interpolation and the mask value
/// as the bad value: a Region maps a position to itself or to bad, so a
/// pixel is set exactly when the Region maps its centre to bad, which is how
/// it is found here, and the count includes each such pixel.
pub(crate) fn region_mask_plan(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
) -> AstResult<MaskPlan> {
    let (used, npix) = grid_region(region, map, lbnd, ubnd)?;
    let ndim = lbnd.len();

    // The bounding box in grid coordinates, as pixel bounds two pixels
    // wider each way and kept within the grid.
    let (lbndgd, ubndgd) = used.get_region_bounds()?;
    let mut lbndg = lbnd.to_vec();
    let mut ubndg = ubnd.to_vec();
    let mut npixg: i64 = 1;
    for idim in 0..ndim {
        if !is_bad(lbndgd[idim]) && !is_bad(ubndgd[idim]) {
            lbndg[idim] = lbnd[idim].max(lbndgd[idim].round() as i64 - 2);
            ubndg[idim] = ubnd[idim].min(ubndgd[idim].round() as i64 + 2);
        }
        if npixg >= 0 {
            npixg *= ubndg[idim] - lbndg[idim] + 1;
        }
    }

    let fill_outside = inside == used.is_negated();
    if npixg <= 0 {
        return Ok(MaskPlan {
            set: vec![fill_outside; npix],
            count: if fill_outside { npix } else { 0 },
        });
    }

    let mut set = vec![fill_outside; npix];
    let mut count = if fill_outside {
        npix - npixg as usize
    } else {
        0
    };
    let tester: DynRegion = if inside { used.negated_copy() } else { used };

    // The pixels of the box, a row along the first axis at a time.
    let row_len = (ubndg[0] - lbndg[0] + 1) as usize;
    let mut index = lbndg.clone();
    loop {
        let rows: Vec<Vec<f64>> = (0..row_len)
            .map(|px| {
                let mut p: Vec<f64> = index.iter().map(|&i| i as f64).collect();
                p[0] = (lbndg[0] + px as i64) as f64;
                p
            })
            .collect();
        let pts = PointSet::from_rows(rows.iter().map(|r| r.as_slice()));
        let out = tester.transform(&pts, true)?;
        // Offset of the row's first pixel in the vectorised array.
        let mut offset = 0usize;
        let mut stride = 1usize;
        for idim in 0..ndim {
            let i = if idim == 0 { lbndg[0] } else { index[idim] };
            offset += (i - lbnd[idim]) as usize * stride;
            stride *= (ubnd[idim] - lbnd[idim] + 1) as usize;
        }
        for px in 0..row_len {
            let bad = out.point(px).contains(&BAD_VALUE);
            set[offset + px] = bad;
            if bad {
                count += 1;
            }
        }
        // Next row.
        let mut idim = 1;
        while idim < ndim {
            index[idim] += 1;
            if index[idim] <= ubndg[idim] {
                break;
            }
            index[idim] = lbndg[idim];
            idim += 1;
        }
        if idim == ndim {
            break;
        }
    }
    Ok(MaskPlan { set, count })
}

/// pointlist.c `Mask<X>`: the pixels the PointList's points fall in are
/// set when masking its inside (or, if it is negated, its outside), and
/// every other pixel otherwise. A point whose pixel is outside the grid is
/// ignored, as C does since upstream ba5a6199 (before which C wrote beyond
/// the array). The count is the number of points inside the grid, or the
/// number of pixels less that.
pub(crate) fn pointlist_mask_plan(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
) -> AstResult<MaskPlan> {
    let (used, npix) = grid_region(region, map, lbnd, ubnd)?;
    let ndim = lbnd.len();
    let points = used.to_current_frame(used.get_region_points()?)?;

    // The index in the vectorised array of each point's pixel, if inside.
    let iv: Vec<Option<usize>> = (0..points.npoint())
        .map(|ip| {
            let mut ii = 0usize;
            let mut stride = 1usize;
            for (idim, &v) in points.point(ip).iter().enumerate().take(ndim) {
                let g = v.round();
                if is_bad(v) || g < lbnd[idim] as f64 || g > ubnd[idim] as f64 {
                    return None;
                }
                ii += (g as i64 - lbnd[idim]) as usize * stride;
                stride *= (ubnd[idim] - lbnd[idim] + 1) as usize;
            }
            Some(ii)
        })
        .collect();
    let nvalid = iv.iter().flatten().count();

    if inside != used.is_negated() {
        let mut set = vec![false; npix];
        for &i in iv.iter().flatten() {
            set[i] = true;
        }
        Ok(MaskPlan { set, count: nvalid })
    } else {
        let mut set = vec![true; npix];
        for &i in iv.iter().flatten() {
            set[i] = false;
        }
        Ok(MaskPlan {
            set,
            count: npix - nvalid,
        })
    }
}
