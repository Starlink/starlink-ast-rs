//! Least-squares fitting of a replacement PolyMap transformation.
//!
//! Port of polymap.c `ReplaceTransformation` / `SamplePoly1D` /
//! `SamplePoly2D` / `FitPoly1D` / `FitPoly2D` (backing `astPolyTran`).
//!
//! The requested transformation is fitted by sampling the *other*
//! transformation on a regular grid of `2*order` points per axis over
//! the supplied rectangle, normalising every sampled column to unit
//! RMS, and minimising the sum of squared residuals between the grid
//! coordinates and the polynomial applied to the transformed
//! coordinates. The polynomial order increases from 2 until the
//! target accuracy `acc` is met or `maxorder` is reached; the
//! max-order fit is accepted if it is better than `maxacc`.
//!
//! The residuals are linear in the coefficients, so the minimisation
//! has a unique minimum; C reaches it with cminpack's `lmder1`
//! (tolerance `sqrt(DBL_EPSILON)`) and this port with the
//! `levenberg_marquardt` crate.

use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, OMatrix, OVector, VecStorage};

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::polymap::PolyFit;
use crate::mappings::{Mapping, PolyTerm};
use crate::serial::Serializable;

/// Fit a replacement transformation for `map` (polymap.c
/// `ReplaceTransformation`).
///
/// `forward` selects which (effective-orientation) transformation to
/// replace. `map` is the copy that will receive the fit: its class's
/// [`PolyFit`] initialisation may update it for each order tried. Returns
/// the fitted terms in the map's effective orientation: the term `powers`
/// index the replaced transformation's inputs. Returns `Ok(None)` if no fit
/// achieves `maxacc` (mirrors C's NULL return without error).
pub(crate) fn replace_transformation<M: PolyFit + Serializable + ?Sized>(
    map: &mut M,
    forward: bool,
    acc: f64,
    maxacc: f64,
    maxorder: usize,
    lbnd: &[f64],
    ubnd: &[f64],
) -> AstResult<Option<Vec<PolyTerm>>> {
    let ndim = map.nin();
    let class = map.class_name();

    // The Mapping can be used (polymap.c:5132-5158). C tests the coefficient
    // counts directly, so an iterative inverse does not count.
    if map.nout() != ndim {
        return Err(AstError::new(
            ErrorCode::Badni,
            format!(
                "astPolyTran({class}): supplied {class} has different number of inputs \
                 ({ndim}) and outputs ({})",
                map.nout()
            ),
        ));
    }
    if ndim > 2 {
        return Err(AstError::new(
            ErrorCode::Badni,
            format!(
                "astPolyTran({class}): supplied {class} has too many inputs and outputs \
                 ({ndim}) - must be 1 or 2"
            ),
        ));
    }
    if forward != map.is_inverted() {
        if !map.has_coeffs(false) {
            return Err(AstError::new(
                ErrorCode::Nodef,
                format!("astPolyTran({class}): supplied {class} has no inverse transformation"),
            ));
        }
    } else if !map.has_coeffs(true) {
        return Err(AstError::new(
            ErrorCode::Nodef,
            format!("astPolyTran({class}): supplied {class} has no forward transformation"),
        ));
    }

    // Bound checks (polymap.c:5160-5172). Dimensional checks are the
    // caller's job since they need PolyMap-specific state.
    if lbnd.len() != ndim || ubnd.len() != ndim {
        return Err(AstError::new(
            ErrorCode::Nodef,
            format!(
                "astPolyTran(PolyMap): bounds have length {}/{} but the PolyMap has {} axes",
                lbnd.len(),
                ubnd.len(),
                ndim
            ),
        ));
    }
    for i in 0..ndim {
        if lbnd[i] >= ubnd[i] {
            return Err(AstError::new(
                ErrorCode::Nodef,
                format!(
                    "astPolyTran(PolyMap): supplied upper bound for axis {} ({}) is less than \
                     or equal to the supplied lower bound ({})",
                    i + 1,
                    ubnd[i],
                    lbnd[i]
                ),
            ));
        }
    }

    // Loop over increasing polynomial orders ("order" is one more than
    // the maximum power, so 2 = linear). polymap.c:5130-5159.
    let maxorder = maxorder.max(2);
    for order in 2..=maxorder {
        let npoint = 2 * order;
        let fit = if ndim == 2 {
            let (table, mut scales) = sample_poly_2d(map, !forward, lbnd, ubnd, npoint)?;
            let (xp1, xp2) = map.fit_poly_2d_init(forward, &table, order, &mut scales)?;
            fit_poly_2d(order, acc, &table, &scales, &xp1, &xp2)
        } else {
            let (table, mut scales) = sample_poly_1d(map, !forward, lbnd[0], ubnd[0], npoint)?;
            let xp1 = map.fit_poly_1d_init(forward, &table, order, &mut scales)?;
            fit_poly_1d(order, acc, &table, &scales, &xp1)
        };

        // Accept when the target accuracy is met, or the max-order fit
        // beats maxacc (polymap.c:5152-5156).
        if let Some((terms, racc)) = fit {
            if racc < acc || (racc < maxacc && order == maxorder) {
                return Ok(Some(terms));
            }
        }
    }

    Ok(None)
}

/// Sample the requested transformation of `map` at `npoint` regularly
/// spaced 1-D positions over `[lbnd, ubnd]` (polymap.c SamplePoly1D).
///
/// Returns `(table, scales)` where `table[0]` holds the transformed
/// values and `table[1]` the grid positions, each normalised to unit
/// RMS, and `scales[i]` is the RMS that restores column `i`.
fn sample_poly_1d<M: Mapping + ?Sized>(
    map: &M,
    forward: bool,
    lbnd: f64,
    ubnd: f64,
    npoint: usize,
) -> AstResult<(Vec<Vec<f64>>, Vec<f64>)> {
    // The grid; the final point is set explicitly to the upper bound
    // to avoid rounding drift.
    let delta = (ubnd - lbnd) / (npoint - 1) as f64;
    let mut grid = vec![0.0; npoint];
    let mut v = lbnd;
    for g in grid.iter_mut().take(npoint - 1) {
        *g = v;
        v += delta;
    }
    grid[npoint - 1] = ubnd;

    let pts = PointSet::from_columns(&[&grid])?;
    let out = map.transform(&pts, forward)?;
    let transformed: Vec<f64> = (0..npoint).map(|i| out.point(i)[0]).collect();

    let mut table = vec![transformed, grid];
    let scales = normalise_columns(&mut table);
    Ok((table, scales))
}

/// Sample the requested transformation of `map` on an `npoint`×`npoint`
/// grid over the rectangle (polymap.c SamplePoly2D).
///
/// Returns `(table, scales)` where `table[0..2]` hold the transformed
/// values and `table[2..4]` the grid positions.
fn sample_poly_2d<M: Mapping + ?Sized>(
    map: &M,
    forward: bool,
    lbnd: &[f64],
    ubnd: &[f64],
    npoint: usize,
) -> AstResult<(Vec<Vec<f64>>, Vec<f64>)> {
    let nsamp = npoint * npoint;
    let mut g1 = Vec::with_capacity(nsamp);
    let mut g2 = Vec::with_capacity(nsamp);
    let d1 = (ubnd[0] - lbnd[0]) / (npoint - 1) as f64;
    let d2 = (ubnd[1] - lbnd[1]) / (npoint - 1) as f64;
    for j in 0..npoint {
        let x2 = if j == npoint - 1 {
            ubnd[1]
        } else {
            lbnd[1] + j as f64 * d2
        };
        for i in 0..npoint {
            let x1 = if i == npoint - 1 {
                ubnd[0]
            } else {
                lbnd[0] + i as f64 * d1
            };
            g1.push(x1);
            g2.push(x2);
        }
    }

    let pts = PointSet::from_columns(&[&g1, &g2])?;
    let out = map.transform(&pts, forward)?;
    let t1: Vec<f64> = (0..nsamp).map(|i| out.point(i)[0]).collect();
    let t2: Vec<f64> = (0..nsamp).map(|i| out.point(i)[1]).collect();

    let mut table = vec![t1, t2, g1, g2];
    let scales = normalise_columns(&mut table);
    Ok((table, scales))
}

/// Normalise each column to unit RMS, returning the original RMS values.
fn normalise_columns(table: &mut [Vec<f64>]) -> Vec<f64> {
    table
        .iter_mut()
        .map(|col| {
            let n = col.len() as f64;
            let rms = (col.iter().map(|v| v * v).sum::<f64>() / n).sqrt();
            for v in col.iter_mut() {
                *v /= rms;
            }
            rms
        })
        .collect()
}

/// Linear least-squares problem over polynomial coefficients: the
/// residual for each (output, sample) pair is `P(basis) - target`,
/// with a constant Jacobian given by the basis powers.
struct FitProblem {
    /// Coefficients being fitted, length `nout_fit * ncof`.
    params: DVector<f64>,
    /// Per-sample basis power products: `basis[j]` has one row of
    /// length `ncof` per sample, shared by every output.
    basis: Vec<Vec<f64>>,
    /// Fit targets, one Vec of length nsamp per fitted output.
    targets: Vec<Vec<f64>>,
}

impl FitProblem {
    fn nsamp(&self) -> usize {
        self.basis.len()
    }
    fn ncof(&self) -> usize {
        self.basis[0].len()
    }
    fn nout_fit(&self) -> usize {
        self.targets.len()
    }
}

impl LeastSquaresProblem<f64, Dyn, Dyn> for FitProblem {
    type ParameterStorage = VecStorage<f64, Dyn, nalgebra::U1>;
    type ResidualStorage = VecStorage<f64, Dyn, nalgebra::U1>;
    type JacobianStorage = VecStorage<f64, Dyn, Dyn>;

    fn set_params(&mut self, p: &OVector<f64, Dyn>) {
        self.params.copy_from(p);
    }

    fn params(&self) -> OVector<f64, Dyn> {
        self.params.clone()
    }

    fn residuals(&self) -> Option<OVector<f64, Dyn>> {
        let (nsamp, ncof) = (self.nsamp(), self.ncof());
        let mut r = DVector::zeros(self.nout_fit() * nsamp);
        for (iout, target) in self.targets.iter().enumerate() {
            for (k, basis_row) in self.basis.iter().enumerate() {
                let mut v = -target[k];
                for (w, b) in basis_row.iter().enumerate() {
                    v += self.params[iout * ncof + w] * b;
                }
                r[iout * nsamp + k] = v;
            }
        }
        Some(r)
    }

    fn jacobian(&self) -> Option<OMatrix<f64, Dyn, Dyn>> {
        let (nsamp, ncof) = (self.nsamp(), self.ncof());
        let nout = self.nout_fit();
        let mut jac = DMatrix::zeros(nout * nsamp, nout * ncof);
        for iout in 0..nout {
            for (k, basis_row) in self.basis.iter().enumerate() {
                for (w, b) in basis_row.iter().enumerate() {
                    jac[(iout * nsamp + k, iout * ncof + w)] = *b;
                }
            }
        }
        Some(jac)
    }
}

/// Run the minimisation from the C initial guess (a unit transform in
/// normalised coordinates) and return the fitted coefficients plus the
/// per-residual values at the solution.
fn solve(problem: FitProblem) -> Option<(Vec<f64>, Vec<f64>)> {
    let (result, report) = LevenbergMarquardt::new()
        .with_ftol(f64::EPSILON)
        .with_xtol(f64::EPSILON)
        .with_gtol(f64::EPSILON)
        .with_patience(200)
        .minimize(problem);
    if !report.termination.was_successful() {
        return None;
    }
    let coeffs: Vec<f64> = result.params.iter().copied().collect();
    let residuals: Vec<f64> = result.residuals()?.iter().copied().collect();
    Some((coeffs, residuals))
}

/// Fit a 1-D polynomial of the given order (polymap.c FitPoly1D).
///
/// Returns the fitted terms (in unscaled units) and the achieved RMS
/// accuracy, or `None` if the minimisation failed.
fn fit_poly_1d(
    order: usize,
    acc: f64,
    table: &[Vec<f64>],
    scales: &[f64],
    xp1: &[f64],
) -> Option<(Vec<PolyTerm>, f64)> {
    let nsamp = table[0].len();

    // Basis: the values from the class's FitPoly1DInit, one row per sample.
    let basis: Vec<Vec<f64>> = xp1.chunks(order).map(|row| row.to_vec()).collect();

    // Initial guess: unit transformation in normalised coordinates.
    let mut p0 = DVector::zeros(order);
    p0[1] = 1.0;

    let (mut coeffs, residuals) = solve(FitProblem {
        params: p0,
        basis: basis.clone(),
        targets: vec![table[1].clone()],
    })?;

    // Achieved accuracy: scaled RMS of the normalised residuals.
    let tv: f64 = residuals.iter().map(|r| r * r).sum();
    let racc = scales[1] * (tv / nsamp as f64).sqrt();

    // Zero insignificant coefficients and convert the survivors to
    // unscaled units (polymap.c:1140-1176).
    let f1 = scales[1];
    let mut f2 = 1.0;
    for (w, c) in coeffs.iter_mut().enumerate() {
        let maxterm = basis
            .iter()
            .map(|row| (*c * row[w]).abs())
            .fold(0.0, f64::max);
        if maxterm * f1 < acc {
            *c = 0.0;
        } else {
            *c *= f1 / f2;
        }
        f2 *= scales[0];
    }

    let terms: Vec<PolyTerm> = coeffs
        .iter()
        .enumerate()
        .filter(|(_, c)| **c != 0.0)
        .map(|(w, c)| PolyTerm {
            coeff: *c,
            powers: vec![w],
            out_axis: 1,
        })
        .collect();
    Some((terms, racc))
}

/// Fit a pair of 2-D polynomials of the given order (polymap.c
/// FitPoly2D). Cross terms with total power >= order are excluded, so
/// each output has `order*(order+1)/2` candidate coefficients, ordered
/// (w1,w2) = (0,0), (1,0), (0,1), (2,0), (1,1), (0,2), ...
fn fit_poly_2d(
    order: usize,
    acc: f64,
    table: &[Vec<f64>],
    scales: &[f64],
    xp1: &[f64],
    xp2: &[f64],
) -> Option<(Vec<PolyTerm>, f64)> {
    let nsamp = table[0].len();
    let ncof = order * (order + 1) / 2;

    // Enumerate the (w1, w2) pairs in coefficient order.
    let mut pairs = Vec::with_capacity(ncof);
    for w12 in 0..order {
        for w2 in 0..=w12 {
            pairs.push((w12 - w2, w2));
        }
    }

    // Basis: products of the values from the class's FitPoly2DInit for
    // each input.
    let basis: Vec<Vec<f64>> = (0..nsamp)
        .map(|k| {
            let p1 = &xp1[k * order..(k + 1) * order];
            let p2 = &xp2[k * order..(k + 1) * order];
            pairs.iter().map(|&(w1, w2)| p1[w1] * p2[w2]).collect()
        })
        .collect();

    // Initial guess: unit transformation — output 1 follows x1, output
    // 2 follows x2. (C sets the literal elements 1 and 5, which is the
    // unit transform only for order 2; the minimum of this linear
    // problem does not depend on the guess.)
    let mut p0 = DVector::zeros(2 * ncof);
    p0[1] = 1.0;
    p0[ncof + 2] = 1.0;

    let (mut coeffs, residuals) = solve(FitProblem {
        params: p0,
        basis: basis.clone(),
        targets: vec![table[2].clone(), table[3].clone()],
    })?;

    // Achieved accuracy (polymap.c:1452-1455).
    let tv: f64 = residuals.iter().map(|r| r * r).sum();
    let facc = 1.0 / (scales[2] * scales[2]) + 1.0 / (scales[3] * scales[3]);
    let racc = (tv / (2.0 * nsamp as f64 * facc)).sqrt();

    // Zero insignificant coefficients and unscale the survivors: each
    // coefficient is multiplied by f1 / (scales[0]^w1 * scales[1]^w2)
    // where f1 is the target column's scale (polymap.c:1466-1498).
    for iout in 0..2 {
        let f1 = scales[2 + iout];
        for (idx, &(w1, w2)) in pairs.iter().enumerate() {
            let c = &mut coeffs[iout * ncof + idx];
            let maxterm = basis
                .iter()
                .map(|row| (*c * row[idx]).abs())
                .fold(0.0, f64::max);
            if maxterm * f1 < acc {
                *c = 0.0;
            } else {
                *c *= f1 / (scales[0].powi(w1 as i32) * scales[1].powi(w2 as i32));
            }
        }
    }

    let mut terms = Vec::new();
    for iout in 0..2 {
        for (idx, &(w1, w2)) in pairs.iter().enumerate() {
            let c = coeffs[iout * ncof + idx];
            if c != 0.0 {
                terms.push(PolyTerm {
                    coeff: c,
                    powers: vec![w1, w2],
                    out_axis: iout + 1,
                });
            }
        }
    }
    Some((terms, racc))
}
