//! Masking data arrays with Regions: C's `astMask<X>`.
//!
//! A Region class masks a grid through [`Region::mask_data`]: region.c
//! `Mask<X>` for most classes, which resamples the data through the Region,
//! and its own method for PointList (pointlist.c `Mask<X>`), which sets the
//! pixels its points fall in. C has one `Mask<X>` per element type; the
//! trait method takes the data as a [`MaskData`], one variant per type, so
//! that it can be called on a `dyn Region`, and [`mask`] wraps a typed array
//! in it.

use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::BasicFrameBuilder;
use crate::mappings::helpers::is_bad;
use crate::mappings::{
    Interp, Mapping, ResampleFlags, ResampleInput, ResampleOptions, ResampleOutput, resample,
};
use crate::pixel::PixelValue;
use crate::regions::{DynRegion, Region};

/// A data array to mask and the value to set, one variant per element type
/// C's `astMask<X>` accepts.
#[doc(hidden)]
#[derive(Debug)]
pub enum MaskData<'a> {
    /// `double` (`astMaskD`).
    D(&'a mut [f64], f64),
    /// `float` (`astMaskF`).
    F(&'a mut [f32], f32),
    /// `INT_BIG` (`astMaskK`).
    K(&'a mut [i64], i64),
    /// `UINT_BIG` (`astMaskUK`).
    UK(&'a mut [u64], u64),
    /// `int` (`astMaskI`).
    I(&'a mut [i32], i32),
    /// `unsigned int` (`astMaskUI`).
    UI(&'a mut [u32], u32),
    /// `short int` (`astMaskS`).
    S(&'a mut [i16], i16),
    /// `unsigned short int` (`astMaskUS`).
    US(&'a mut [u16], u16),
    /// `signed char` (`astMaskB`).
    B(&'a mut [i8], i8),
    /// `unsigned char` (`astMaskUB`).
    UB(&'a mut [u8], u8),
}

/// Applies `$f::<T>($data, $val, $($arg),*)` to whichever variant `$md` is.
macro_rules! dispatch {
    ($md:expr, $f:ident, $($arg:expr),*) => {
        match $md {
            MaskData::D(d, v) => $f($($arg,)* d, v),
            MaskData::F(d, v) => $f($($arg,)* d, v),
            MaskData::K(d, v) => $f($($arg,)* d, v),
            MaskData::UK(d, v) => $f($($arg,)* d, v),
            MaskData::I(d, v) => $f($($arg,)* d, v),
            MaskData::UI(d, v) => $f($($arg,)* d, v),
            MaskData::S(d, v) => $f($($arg,)* d, v),
            MaskData::US(d, v) => $f($($arg,)* d, v),
            MaskData::B(d, v) => $f($($arg,)* d, v),
            MaskData::UB(d, v) => $f($($arg,)* d, v),
        }
    };
}

/// An element type [`mask`] accepts: one of C's `astMask<X>` types.
pub trait MaskValue: PixelValue {
    /// `data` and `val` as the [`MaskData`] variant for this type.
    #[doc(hidden)]
    fn mask_data(data: &mut [Self], val: Self) -> MaskData<'_>;
}

macro_rules! mask_value {
    ($($t:ty => $v:ident),* $(,)?) => {$(
        impl MaskValue for $t {
            fn mask_data(data: &mut [Self], val: Self) -> MaskData<'_> {
                MaskData::$v(data, val)
            }
        }
    )*};
}

mask_value!(
    f64 => D, f32 => F, i64 => K, u64 => UK, i32 => I,
    u32 => UI, i16 => S, u16 => US, i8 => B, u8 => UB,
);

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
pub fn mask<T: MaskValue>(
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
    region.mask_data(map, inside, lbnd, ubnd, T::mask_data(data, val))
}

/// AST__DIMIN unless `data` has `npix` elements.
fn check_len<T>(data: &[T], npix: usize) -> AstResult<()> {
    if data.len() != npix {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "astMask: the data array has {} elements but the grid has {npix}",
                data.len()
            ),
        ));
    }
    Ok(())
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
            region.reg_map_region(map, &grid)?
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

/// region.c `Mask<X>`, the default of [`Region::mask_data`].
pub(crate) fn region_mask(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
    data: MaskData<'_>,
) -> AstResult<usize> {
    dispatch!(data, region_mask_x, region, map, inside, lbnd, ubnd)
}

/// region.c `Mask<X>` for one element type.
///
/// Every pixel outside the Region's grid-coordinate bounding box (widened by
/// two pixels) is on the same side of the Region, so those are set together
/// when the side being masked is the side the box's outside is on. Within
/// the box the data are resampled through the Region, negated when the
/// inside is masked, with nearest-neighbour interpolation and `val` as the
/// bad value: a Region maps a position inside it to itself and any other to
/// bad, so a pixel keeps its value or is set to `val`.
fn region_mask_x<T: PixelValue>(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
    data: &mut [T],
    val: T,
) -> AstResult<usize> {
    let (used, npix) = grid_region(region, map, lbnd, ubnd)?;
    check_len(data, npix)?;
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
        if fill_outside {
            data.fill(val);
            return Ok(npix);
        }
        return Ok(0);
    }

    // The Region leaves points inside it unchanged and makes others bad, the
    // opposite of what masking the inside needs.
    let tester: DynRegion = if inside { used.negated_copy() } else { used };
    let opts = ResampleOptions {
        interp: Interp::Nearest,
        params: &[],
        flags: ResampleFlags::default(),
        tol: 0.0,
        maxpix: 100,
    };
    let resample_into = |input: &[T], out: &mut [T]| {
        resample(
            tester.as_ref() as &dyn Mapping,
            &ResampleInput {
                lbnd,
                ubnd,
                data: input,
                var: None,
            },
            &mut ResampleOutput {
                lbnd,
                ubnd,
                region_lbnd: &lbndg,
                region_ubnd: &ubndg,
                data: out,
                var: None,
            },
            &opts,
            val,
        )
    };
    if fill_outside {
        // Resample into an array already holding `val` everywhere, so that
        // the pixels outside the box are set too.
        let mut out = vec![val; npix];
        let set = resample_into(data, &mut out)?;
        data.copy_from_slice(&out);
        Ok(npix - npixg as usize + set)
    } else {
        // C resamples the array onto itself; each pixel reads only itself,
        // so resampling from a copy gives the same result.
        let input = data.to_vec();
        resample_into(&input, data)
    }
}

/// pointlist.c `Mask<X>`: the pixels the PointList's points fall in are
/// set when masking its inside (or, if it is negated, its outside), and
/// every other pixel otherwise. A point whose pixel is outside the grid is
/// ignored, as C does since upstream ba5a6199 (before which C wrote beyond
/// the array). The count is the number of points inside the grid, or the
/// number of pixels less that.
pub(crate) fn pointlist_mask(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
    data: MaskData<'_>,
) -> AstResult<usize> {
    dispatch!(data, pointlist_mask_x, region, map, inside, lbnd, ubnd)
}

/// pointlist.c `Mask<X>` for one element type.
fn pointlist_mask_x<T: Copy>(
    region: &dyn Region,
    map: Option<&dyn Mapping>,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
    data: &mut [T],
    val: T,
) -> AstResult<usize> {
    let (used, npix) = grid_region(region, map, lbnd, ubnd)?;
    check_len(data, npix)?;
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
        for &i in iv.iter().flatten() {
            data[i] = val;
        }
        Ok(nvalid)
    } else {
        let mut keep = vec![false; npix];
        for &i in iv.iter().flatten() {
            keep[i] = true;
        }
        for (d, &k) in data.iter_mut().zip(&keep) {
            if !k {
                *d = val;
            }
        }
        Ok(npix - nvalid)
    }
}
