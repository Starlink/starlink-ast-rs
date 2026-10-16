//! Transforming every position of a regular grid.
//!
//! Backs [`Mapping::tran_grid`], a port of C `mapping.c` `TranGrid` and its
//! helpers `TranGridAdaptively`, `TranGridWithBlocking` and
//! `TranGridSection`. The grid is split recursively along its longest axis
//! until either a section is small enough that a linear fit is not worth
//! trying, or `astLinearApprox` finds the Mapping linear within `tol` over the
//! section. Each section is then transformed in blocks of at most 2048
//! positions, either through the Mapping or by stepping the linear fit across
//! the block.
//!
//! The linear-fit positions are accumulated by repeated addition, starting
//! afresh at each block, so the blocking and the 1-D, 2-D and general paths
//! are kept exactly as C has them: each gives different rounding.

// The section loops mirror the C source index for index.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, Mapping, substitute_nan_to_bad_in_place};

/// The most positions transformed at once (`mxpix` in `TranGridWithBlocking`).
const MXPIX: i64 = 2 * 1024;

/// Body of [`Mapping::tran_grid`]; `mapping.c` `TranGrid`.
pub(crate) fn tran_grid(
    this: &dyn Mapping,
    lbnd: &[i64],
    ubnd: &[i64],
    tol: f64,
    maxpix: usize,
    forward: bool,
) -> AstResult<PointSet> {
    let ncoord_in = lbnd.len();
    if ubnd.len() != ncoord_in {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "astTranGrid: the bounds give {} and {} values.",
                lbnd.len(),
                ubnd.len()
            ),
        ));
    }

    // The number of grid points, checking each pair of bounds.
    let mut mpix: i64 = 1;
    for idim in 0..ncoord_in {
        if lbnd[idim] > ubnd[idim] {
            return Err(AstError::new(
                ErrorCode::Gbdin,
                format!(
                    "astTranGrid: Lower bound of input grid ({}) exceeds corresponding \
                     upper bound ({}). Error in input dimension {}.",
                    lbnd[idim],
                    ubnd[idim],
                    idim + 1
                ),
            ));
        }
        mpix = (ubnd[idim] - lbnd[idim])
            .checked_add(1)
            .and_then(|d| mpix.checked_mul(d))
            .ok_or_else(|| {
                AstError::new(
                    ErrorCode::Exspix,
                    "astTranGrid: Supplied grid contains too many points.",
                )
            })?;
    }
    let npoint = mpix;

    // ValidateMapping.
    let defined = if forward {
        this.has_forward()
    } else {
        this.has_inverse()
    };
    if !defined {
        return Err(AstError::new(
            ErrorCode::Trnnd,
            format!(
                "astTranGrid: {} coordinate transformation is not defined by the \
                 Mapping supplied.",
                if forward { "A forward" } else { "An inverse" }
            ),
        ));
    }
    let (nin, nout) = if forward {
        (this.nin(), this.nout())
    } else {
        (this.nout(), this.nin())
    };
    if ncoord_in != nin {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "astTranGrid: Bad number of input coordinate values ({ncoord_in}). The \
                 Mapping given requires {nin} coordinate value{} for each input point.",
                if nin == 1 { "" } else { "s" }
            ),
        ));
    }

    if tol < 0.0 {
        return Err(AstError::new(
            ErrorCode::Patin,
            format!(
                "astTranGrid: Invalid positional accuracy tolerance ({tol} pixel). This \
                 value should not be less than zero."
            ),
        ));
    }

    // Simplify only when there are enough points to make it worthwhile.
    let mut simple: DynMapping = if npoint > 1024 {
        let simple = this.simplify()?;
        let defined = if forward {
            simple.has_forward()
        } else {
            simple.has_inverse()
        };
        if !defined {
            return Err(AstError::new(
                ErrorCode::Trnnd,
                format!(
                    "astTranGrid: {} coordinate transformation is not defined by the \
                     Mapping supplied.",
                    if forward { "A forward" } else { "An inverse" }
                ),
            ));
        }
        simple
    } else {
        this.clone_box()
    };
    if !forward {
        simple.invert();
    }

    let npoint = usize::try_from(npoint).map_err(|_| {
        AstError::new(
            ErrorCode::Exspix,
            "astTranGrid: Supplied grid contains too many points.",
        )
    })?;
    let mut out = PointSet::zeros(npoint, nout);
    let grid = Grid {
        lbnd_in: lbnd,
        ubnd_in: ubnd,
        ncoord_out: nout,
    };
    let maxpix = i64::try_from(maxpix).unwrap_or(i64::MAX);
    grid.adaptively(&*simple, lbnd, ubnd, tol, maxpix, &mut out)?;
    Ok(out)
}

/// The whole grid being transformed, which every section indexes into.
struct Grid<'a> {
    lbnd_in: &'a [i64],
    ubnd_in: &'a [i64],
    ncoord_out: usize,
}

impl Grid<'_> {
    /// `TranGridAdaptively`: transform the section `[lbnd, ubnd]`, dividing it
    /// in two along its longest axis while the Mapping is not linear enough
    /// over it.
    fn adaptively(
        &self,
        this: &dyn Mapping,
        lbnd: &[i64],
        ubnd: &[i64],
        tol: f64,
        maxpix: i64,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let ncoord_in = lbnd.len();
        let mut npix: i64 = 1;
        let mut mxdim: i64 = 0;
        let mut dimx = 1;
        let mut nvertex: i64 = 1;
        for coord_in in 0..ncoord_in {
            let dim = ubnd[coord_in] - lbnd[coord_in] + 1;
            npix *= dim;
            if dim > mxdim {
                mxdim = dim;
                dimx = coord_in;
            }
            nvertex *= 2;
        }

        // The number of sample points astLinearApprox needs. A section with
        // fewer than four times as many positions is not worth fitting.
        let npoint = 1 + 4 * ncoord_in as i64 + 2 * nvertex;
        let toosmall = npix < 4 * npoint;
        let toobig = maxpix < mxdim;

        let mut linear_fit: Option<Vec<f64>> = None;
        let divide = if toosmall || tol == 0.0 {
            false
        } else if toobig {
            true
        } else {
            // Fit over the cell edges rather than the centres, so an axis
            // spanned by one cell still has distinct bounds.
            let flbnd: Vec<f64> = lbnd.iter().map(|&l| l as f64 - 0.5).collect();
            let fubnd: Vec<f64> = ubnd.iter().map(|&u| u as f64 + 0.5).collect();
            linear_fit = this
                .linear_approx(&flbnd, &fubnd, tol)?
                .map(|fit| fit.as_c_layout())
                .filter(|fit| !fit.contains(&crate::constants::BAD_VALUE));
            linear_fit.is_none()
        };

        if !divide {
            self.with_blocking(this, linear_fit.as_deref(), lbnd, ubnd, out)
        } else {
            let mut lo = lbnd.to_vec();
            let mut hi = ubnd.to_vec();
            hi[dimx] = (0.5 * (lbnd[dimx] + ubnd[dimx]) as f64).floor() as i64;
            self.adaptively(this, &lo, &hi, tol, maxpix, out)?;
            lo[dimx] = hi[dimx] + 1;
            hi[dimx] = ubnd[dimx];
            if lo[dimx] <= hi[dimx] {
                self.adaptively(this, &lo, &hi, tol, maxpix, out)?;
            }
            Ok(())
        }
    }

    /// `TranGridWithBlocking`: transform the section `[lbnd, ubnd]` in
    /// blocks of at most [`MXPIX`] positions, shaped like the section.
    fn with_blocking(
        &self,
        this: &dyn Mapping,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
        out: &mut PointSet,
    ) -> AstResult<()> {
        let ndim_in = lbnd.len();

        // Limit the longest block dimension until a block holds few enough
        // positions, by a binary chop.
        let mut npix: i64 = 1;
        let mut mxdim_block: i64 = 0;
        for idim in 0..ndim_in {
            let dim = ubnd[idim] - lbnd[idim] + 1;
            npix *= dim;
            if mxdim_block < dim {
                mxdim_block = dim;
            }
        }
        if npix > MXPIX {
            let mut lolim: i64 = 1;
            let mut hilim = mxdim_block;
            while hilim - lolim > 1 {
                mxdim_block = (hilim + lolim) / 2;
                npix = 1;
                for idim in 0..ndim_in {
                    let dim = ubnd[idim] - lbnd[idim] + 1;
                    npix *= dim.min(mxdim_block);
                }
                if npix <= MXPIX {
                    lolim = mxdim_block;
                } else {
                    hilim = mxdim_block;
                }
            }
            mxdim_block = lolim;
        }
        if mxdim_block < 2 {
            mxdim_block = 2;
        }

        let mut dim_block = vec![0_i64; ndim_in];
        let mut lbnd_block = vec![0_i64; ndim_in];
        let mut ubnd_block = vec![0_i64; ndim_in];
        for idim in 0..ndim_in {
            let dim = ubnd[idim] - lbnd[idim] + 1;
            dim_block[idim] = dim.min(mxdim_block);
            lbnd_block[idim] = lbnd[idim];
            ubnd_block[idim] = (lbnd[idim] + dim_block[idim] - 1).min(ubnd[idim]);
        }

        loop {
            self.section(this, linear_fit, &lbnd_block, &ubnd_block, out)?;

            // Step the block along the least significant axis that has room,
            // returning the axes below it to their start.
            let mut idim = 0;
            loop {
                if ubnd_block[idim] < ubnd[idim] {
                    lbnd_block[idim] = (lbnd_block[idim] + dim_block[idim]).min(ubnd[idim]);
                    ubnd_block[idim] = (lbnd_block[idim] + dim_block[idim] - 1).min(ubnd[idim]);
                    break;
                }
                lbnd_block[idim] = lbnd[idim];
                ubnd_block[idim] = (lbnd[idim] + dim_block[idim] - 1).min(ubnd[idim]);
                idim += 1;
                if idim == ndim_in {
                    return Ok(());
                }
            }
        }
    }

    /// `TranGridSection`: transform the block `[lbnd, ubnd]` and store each
    /// result at its position in the whole grid.
    fn section(
        &self,
        this: &dyn Mapping,
        linear_fit: Option<&[f64]>,
        lbnd: &[i64],
        ubnd: &[i64],
        out: &mut PointSet,
    ) -> AstResult<()> {
        let ndim_in = lbnd.len();
        let ndim_out = self.ncoord_out;
        let (lbnd_in, ubnd_in) = (self.lbnd_in, self.ubnd_in);

        let mut npoint = 1_usize;
        for coord_in in 0..ndim_in {
            npoint *= (ubnd[coord_in] - lbnd[coord_in] + 1) as usize;
        }
        let mut offset = vec![0_i64; npoint];
        let mut stride = vec![0_i64; ndim_in];
        let mut s: i64 = 1;
        for coord_in in 0..ndim_in {
            stride[coord_in] = s;
            s *= ubnd_in[coord_in] - lbnd_in[coord_in] + 1;
        }

        let ptr_out = if let Some(linear_fit) = linear_fit {
            let zero = &linear_fit[..ndim_out];
            let grad = &linear_fit[ndim_out..];
            let mut ptr_out = PointSet::zeros(npoint, ndim_out);
            let mut v = ptr_out.view_mut();
            let mut point = 0;

            if ndim_in == 1 && ndim_out == 1 {
                let mut off = lbnd[0] - lbnd_in[0];
                let mut xx1 = zero[0] + grad[0] * lbnd[0] as f64;
                for _ in lbnd[0]..=ubnd[0] {
                    v[[0, point]] = xx1;
                    xx1 += grad[0];
                    offset[point] = off;
                    point += 1;
                    off += 1;
                }
            } else if ndim_in == 2 && ndim_out == 2 {
                let mut x1 = zero[0] + grad[1] * (lbnd[1] - 1) as f64;
                let mut y1 = zero[1] + grad[3] * (lbnd[1] - 1) as f64;
                let mut off1 = stride[1] * (lbnd[1] - lbnd_in[1] - 1) - lbnd_in[0];
                for _ in lbnd[1]..=ubnd[1] {
                    x1 += grad[1];
                    y1 += grad[3];
                    off1 += stride[1];
                    let mut xx1 = x1 + grad[0] * lbnd[0] as f64;
                    let mut yy1 = y1 + grad[2] * lbnd[0] as f64;
                    let mut off = off1 + lbnd[0];
                    for _ in lbnd[0]..=ubnd[0] {
                        v[[0, point]] = xx1;
                        xx1 += grad[0];
                        v[[1, point]] = yy1;
                        yy1 += grad[2];
                        offset[point] = off;
                        point += 1;
                        off += 1;
                    }
                }
            } else {
                // Accumulate partial sums of the matrix product for the more
                // significant input axes, recomputing only those whose index
                // has changed since the previous position.
                let mut accum = vec![0.0_f64; ndim_in * ndim_out];
                let mut dim = lbnd.to_vec();
                let mut off: i64 = 0;
                for coord_in in 0..ndim_in {
                    off += stride[coord_in] * (dim[coord_in] - lbnd_in[coord_in]);
                }
                for coord_out in 0..ndim_out {
                    accum[(coord_out + 1) * ndim_in - 1] = zero[coord_out];
                }
                let mut coord_in = ndim_in - 1;
                loop {
                    for coord_out in 0..ndim_out {
                        let i1 = coord_out * ndim_in;
                        for idim in (1..=coord_in).rev() {
                            let i2 = i1 + idim;
                            accum[i2 - 1] = accum[i2] + dim[idim] as f64 * grad[i2];
                        }
                        v[[coord_out, point]] = accum[i1] + dim[0] as f64 * grad[i1];
                    }
                    offset[point] = off;
                    point += 1;
                    if !next_index(&mut dim, &mut off, &mut coord_in, lbnd, ubnd, &stride) {
                        break;
                    }
                }
            }
            ptr_out
        } else {
            let mut ptr_in = PointSet::zeros(npoint, ndim_in);
            let mut v = ptr_in.view_mut();
            let mut point = 0;
            if ndim_in == 1 && ndim_out == 1 {
                for ix in lbnd[0]..=ubnd[0] {
                    v[[0, point]] = ix as f64;
                    offset[point] = ix - lbnd_in[0];
                    point += 1;
                }
            } else if ndim_in == 2 && ndim_out == 2 {
                let mut off1 = stride[1] * (lbnd[1] - lbnd_in[1] - 1) - lbnd_in[0];
                for iy in lbnd[1]..=ubnd[1] {
                    off1 += stride[1];
                    let mut off2 = off1 + lbnd[0];
                    for ix in lbnd[0]..=ubnd[0] {
                        v[[0, point]] = ix as f64;
                        v[[1, point]] = iy as f64;
                        offset[point] = off2;
                        point += 1;
                        off2 += 1;
                    }
                }
            } else {
                let mut dim = lbnd.to_vec();
                let mut off: i64 = 0;
                for coord_in in 0..ndim_in {
                    off += stride[coord_in] * (dim[coord_in] - lbnd_in[coord_in]);
                }
                let mut coord_in = 0;
                loop {
                    for c in 0..ndim_in {
                        v[[c, point]] = dim[c] as f64;
                    }
                    offset[point] = off;
                    point += 1;
                    if !next_index(&mut dim, &mut off, &mut coord_in, lbnd, ubnd, &stride) {
                        break;
                    }
                }
            }
            let mut ptr_out = this.transform(&ptr_in, true)?;
            substitute_nan_to_bad_in_place(&mut ptr_out);
            ptr_out
        };

        // Copy the results to their positions in the whole grid.
        let src = ptr_out.view();
        let mut dst = out.view_mut();
        for coord_out in 0..ndim_out {
            for point in 0..npoint {
                dst[[coord_out, offset[point] as usize]] = src[[coord_out, point]];
            }
        }
        Ok(())
    }
}

/// Step the pixel indices `dim` to the next position of the block
/// `[lbnd, ubnd]`, first axis fastest, keeping `off` its offset in the whole
/// grid. `coord_in` is left as the most significant axis that changed.
/// Returns false once every position has been visited.
fn next_index(
    dim: &mut [i64],
    off: &mut i64,
    coord_in: &mut usize,
    lbnd: &[i64],
    ubnd: &[i64],
    stride: &[i64],
) -> bool {
    *coord_in = 0;
    loop {
        let c = *coord_in;
        if dim[c] < ubnd[c] {
            dim[c] += 1;
            *off += stride[c];
            return true;
        }
        dim[c] = lbnd[c];
        *off -= stride[c] * (ubnd[c] - lbnd[c]);
        *coord_in += 1;
        if *coord_in == dim.len() {
            return false;
        }
    }
}
