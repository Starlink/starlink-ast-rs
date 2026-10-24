// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Quadratic least-squares fit to a 2-input Mapping.
//!
//! Backs [`Mapping::quad_approx`], a port of C `mapping.c` `QuadApprox`
//! (`astQuadApprox`). Each Mapping output is fitted as
//! `a0 + a1*x + a2*y + a3*x*y + a4*x*x + a5*y*y` to a regular grid of
//! `nx` × `ny` positions spanning the box, by solving the 6×6 normal equations
//! with `palDmat`.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{Mapping, substitute_nan_to_bad_in_place};
use crate::pal;

/// The result of [`Mapping::quad_approx`].
#[derive(Debug, Clone, PartialEq)]
pub struct QuadFit {
    /// Six coefficients per Mapping output, in the order
    /// `[a0, a1 (x), a2 (y), a3 (x*y), a4 (x*x), a5 (y*y)]`: output `i` is
    /// `coeffs[6*i..6*i + 6]`.
    ///
    /// When `fitted` is false, the output whose fit failed and every output after it hold `BAD_VALUE`.
    /// Outputs fitted before the failure keep their coefficients.
    // This is what C leaves in its `fit` array.
    pub coeffs: Vec<f64>,
    /// The RMS residual between the fit and the Mapping, over every fitted
    /// output, or `BAD_VALUE` when no output was fitted.
    pub rms: f64,
    /// Whether every output was fitted.
    // C's returned value.
    pub fitted: bool,
}

/// Body of [`Mapping::quad_approx`]; `mapping.c` `QuadApprox`.
pub(crate) fn quad_approx<M: Mapping + ?Sized>(
    map: &M,
    lbnd: [f64; 2],
    ubnd: [f64; 2],
    nx: usize,
    ny: usize,
) -> AstResult<QuadFit> {
    let nin = map.nin();
    let nout = map.nout();
    if nin != 2 {
        return Err(AstError::new(
            ErrorCode::Badni,
            format!(
                "astQuadApprox: Input Mapping has {nin} {} - it must have 2 inputs.",
                if nin == 1 { "input" } else { "inputs" }
            ),
        ));
    }

    // Use at least 3 points on each input axis.
    let nx = nx.max(3);
    let ny = ny.max(3);
    let np = nx * ny;

    // A regular grid of input positions, x varying fastest.
    let dx = (ubnd[0] - lbnd[0]) / (nx - 1) as f64;
    let dy = (ubnd[1] - lbnd[1]) / (ny - 1) as f64;
    let mut pset1 = PointSet::zeros(np, 2);
    let mut i = 0;
    for iy in 0..ny {
        let mut x = lbnd[0];
        let y = lbnd[1] + iy as f64 * dy;
        for _ in 0..nx {
            let mut p = pset1.point_mut(i);
            p[0] = x;
            p[1] = y;
            x += dx;
            i += 1;
        }
    }

    let mut pset2 = map.transform(&pset1, true)?;
    substitute_nan_to_bad_in_place(&mut pset2);

    let mut coeffs = vec![BAD_VALUE; 6 * nout];
    let mut fitted = true;
    let mut rms = 0.0_f64;
    let mut ntot = 0_usize;

    for iout in 0..nout {
        // Form the required sums over the positions with a good output.
        let mut n = 0_usize;
        let (mut sx, mut sy, mut sxy, mut sx2, mut sy2) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut sx2y, mut sx3, mut sxy2, mut sy3) = (0.0, 0.0, 0.0, 0.0);
        let (mut sx2y2, mut sx3y, mut sxy3, mut sx4, mut sy4) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut sz, mut sz2, mut szx, mut szy) = (0.0, 0.0, 0.0, 0.0);
        let (mut szxy, mut szx2, mut szy2) = (0.0, 0.0, 0.0);
        for ip in 0..np {
            let p = pset1.point(ip);
            let (x, y) = (p[0], p[1]);
            let z = pset2.point(ip)[iout];
            if z != BAD_VALUE {
                let xx = x * x;
                let yy = y * y;
                let xy = x * y;
                n += 1;
                sx += x;
                sy += y;
                sxy += xy;
                sx2 += xx;
                sy2 += yy;
                sx2y += xx * y;
                sx3 += xx * x;
                sxy2 += x * yy;
                sy3 += yy * y;
                sx2y2 += xx * yy;
                sx3y += xx * xy;
                sxy3 += xy * yy;
                sx4 += xx * xx;
                sy4 += yy * yy;
                sz += z;
                sz2 += z * z;
                szx += z * x;
                szy += z * y;
                szxy += z * xy;
                szx2 += z * xx;
                szy2 += z * yy;
            }
        }

        // The normal equations M.X = V; palDmat overwrites V with X.
        let nf = n as f64;
        #[rustfmt::skip]
        let mut mat = [
            nf,   sx,   sy,   sxy,   sx2,   sy2,
            sx,   sx2,  sxy,  sx2y,  sx3,   sxy2,
            sy,   sxy,  sy2,  sxy2,  sx2y,  sy3,
            sxy,  sx2y, sxy2, sx2y2, sx3y,  sxy3,
            sx2,  sx3,  sx2y, sx3y,  sx4,   sx2y2,
            sy2,  sxy2, sy3,  sxy3,  sx2y2, sy4,
        ];
        let mut o = [sz, szx, szy, szxy, szx2, szy2];
        if !pal::dmat(6, &mut mat, &mut o) {
            fitted = false;
            break;
        }
        coeffs[6 * iout..6 * iout + 6].copy_from_slice(&o);

        // Add this output's sum of squared residuals.
        ntot += n;
        rms += o[0] * o[0] * nf
            + 2.0 * o[0] * o[1] * sx
            + 2.0 * o[0] * o[2] * sy
            + 2.0 * (o[0] * o[3] + o[1] * o[2]) * sxy
            + (2.0 * o[0] * o[4] + o[1] * o[1]) * sx2
            + (2.0 * o[0] * o[5] + o[2] * o[2]) * sy2
            + 2.0 * o[1] * o[4] * sx3
            + 2.0 * (o[1] * o[3] + o[2] * o[4]) * sx2y
            + 2.0 * (o[1] * o[5] + o[2] * o[3]) * sxy2
            + 2.0 * o[2] * o[5] * sy3
            + o[4] * o[4] * sx4
            + 2.0 * o[3] * o[4] * sx3y
            + (2.0 * o[4] * o[5] + o[3] * o[3]) * sx2y2
            + 2.0 * o[3] * o[5] * sxy3
            + o[5] * o[5] * sy4
            + sz2
            - 2.0 * (o[0] * sz + o[1] * szx + o[2] * szy + o[3] * szxy + o[4] * szx2 + o[5] * szy2);
    }

    // No output was fitted: either there are none or the first one failed.
    if ntot == 0 {
        return Ok(QuadFit {
            coeffs,
            rms: BAD_VALUE,
            fitted: false,
        });
    }
    let rms = if rms > 0.0 {
        crate::math::sqrt(rms / ntot as f64)
    } else {
        0.0
    };
    Ok(QuadFit {
        coeffs,
        rms,
        fitted,
    })
}
