//! Linear-approximation fitting for `Mapping`s.
//!
//! Backs the [`Mapping::linear_approx`] default trait method. Mirrors
//! C `mapping.c::astLinearApprox` (ref starlink-ast, ~line 6459):
//! given a `Mapping` and a sample box `[lbnd, ubnd]`, returns the
//! best-fit affine map `y = matrix · x + shift` if the fit is good
//! (max Cartesian deviation per test point ≤ `tol`), `None` otherwise.

// The body uses index-style loops to mirror the C source line-for-line;
// converting them to iterators would obscure the parallel structure.
#![allow(clippy::needless_range_loop, clippy::manual_memcpy)]

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::Mapping;
use crate::mappings::helpers::is_bad;

/// Affine fit to a Mapping over a sample box.
///
/// `shift[i]` is the constant offset for output axis `i`, and
/// `matrix[i * nin + j]` is the gradient ∂y_i/∂x_j (row-major
/// `nout × nin`).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearFit {
    /// Constant offset, length `nout`.
    pub shift: Vec<f64>,
    /// Linear matrix, row-major `nout × nin`.
    pub matrix: Vec<f64>,
}

impl LinearFit {
    /// Number of output axes.
    pub fn nout(&self) -> usize {
        self.shift.len()
    }

    /// Number of input axes.
    pub fn nin(&self) -> usize {
        if self.shift.is_empty() {
            0
        } else {
            self.matrix.len() / self.shift.len()
        }
    }

    /// Flatten to the C-style fit array layout: `nout` shifts followed
    /// by `nin*nout` row-major gradients.
    pub fn as_c_layout(&self) -> Vec<f64> {
        let mut v = self.shift.clone();
        v.extend_from_slice(&self.matrix);
        v
    }
}

/// Generic body of [`Mapping::linear_approx`]'s default implementation.
///
/// Mirrors `mapping.c::astLinearApprox`. Public to the crate so the
/// trait default in `mappings/mod.rs` can dispatch through here;
/// external callers should reach for [`Mapping::linear_approx`]
/// directly rather than naming this function.
///
/// Returns `Some(fit)` when the fit reproduces every test point
/// within `tol` (Cartesian distance, output space). Returns `None`
/// if `map` is non-linear over the box, or if every output is BAD
/// at every test point. A transformation error is returned, as C's
/// errors propagate from `astLinearApprox`, and so is bounds of the
/// wrong length (`ErrorCode::Ncpin`).
///
/// Outputs that produce BAD values at any test point have their
/// shift set to `BAD_VALUE` and corresponding matrix rows filled
/// with `BAD_VALUE`; the fit may still succeed for the surviving
/// outputs (mirrors the C semantics).
pub(crate) fn linear_approx_default<M: Mapping + ?Sized>(
    map: &M,
    lbnd: &[f64],
    ubnd: &[f64],
    tol: f64,
) -> AstResult<Option<LinearFit>> {
    let ndim_in = map.nin();
    let ndim_out = map.nout();
    if lbnd.len() != ndim_in || ubnd.len() != ndim_in {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "astLinearApprox: the bounds give {} and {} values; the Mapping has \
                 {ndim_in} inputs",
                lbnd.len(),
                ubnd.len()
            ),
        ));
    }
    if ndim_in == 0 || ndim_out == 0 {
        return Ok(None);
    }

    // Initialise fit array to BAD. fit layout: [zero[0..nout], grad[0..nin*nout]]
    let nc = (ndim_in + 1) * ndim_out;
    let mut fit = vec![BAD_VALUE; nc];

    // Step 1: build initial sample points — the centre of each face of the box.
    // C stores them column-major; we use PointSet rows here. There are
    // 2*ndim_in face-centre points.
    let nfaces = 2 * ndim_in;
    let mut pts_in_f = PointSet::zeros(nfaces, ndim_in);
    for face in 0..nfaces {
        let mut row = pts_in_f.point_mut(face);
        for coord_in in 0..ndim_in {
            row[coord_in] = 0.5 * (lbnd[coord_in] + ubnd[coord_in]);
        }
        let axis = face / 2;
        row[axis] = if face % 2 == 1 {
            ubnd[axis]
        } else {
            lbnd[axis]
        };
    }

    // Forward-transform the face centres.
    let pts_out_f = map.transform(&pts_in_f, true)?;

    // Step 2: extract per-output gradients and zero points from face pairs.
    // grad lives in fit[ndim_out..], zero lives in fit[..ndim_out].
    // The first pass treats inputs as if centred on the box midpoint.
    {
        let (zero_slice, grad_slice) = fit.split_at_mut(ndim_out);
        for coord_out in 0..ndim_out {
            let mut bad_output = false;
            let mut z = 0.0_f64;
            for coord_in in 0..ndim_in {
                let face1 = 2 * coord_in;
                let face2 = face1 + 1;
                let in1 = pts_in_f.point(face1)[coord_in];
                let in2 = pts_in_f.point(face2)[coord_in];
                let out1 = pts_out_f.point(face1)[coord_out];
                let out2 = pts_out_f.point(face2)[coord_out];

                if is_bad(out1) || is_bad(out2) {
                    bad_output = true;
                    break;
                }

                let indiff = in2 - in1;
                grad_slice[ndim_in * coord_out + coord_in] = if indiff != 0.0 {
                    (out2 - out1) / indiff
                } else {
                    0.0
                };
                z += out1 + out2;
            }

            if !bad_output {
                zero_slice[coord_out] = z / (2.0 * ndim_in as f64);
            }
            // bad_output: zero_slice[coord_out] keeps its BAD_VALUE init.
        }
    }

    // Step 3: correct zero points (they were computed assuming origin =
    // box centre). Subtract grad·centre to express the fit relative to
    // the actual input coordinate system. Also detect the all-BAD case.
    let mut linear = false;
    {
        let (zero_slice, grad_slice) = fit.split_at_mut(ndim_out);
        let mut ii = 0;
        for coord_out in 0..ndim_out {
            if zero_slice[coord_out] != BAD_VALUE {
                linear = true;
                for coord_in in 0..ndim_in {
                    let x0 = 0.5 * (lbnd[coord_in] + ubnd[coord_in]);
                    zero_slice[coord_out] -= grad_slice[ii] * x0;
                    ii += 1;
                }
            } else {
                for _ in 0..ndim_in {
                    grad_slice[ii] = BAD_VALUE;
                    ii += 1;
                }
            }
        }
    }

    if !linear {
        // All outputs were bad at every face centre. C fills `fit` with a
        // unit transformation (BAD zero points) at mapping.c:6952-6966, but
        // then the trailing `if( !astOK || !linear )` at mapping.c:6972
        // overwrites the whole array with BAD and leaves result = 0. So the
        // function reports failure with an all-BAD fit; we mirror that as
        // `None`. (Oracle-verified: astLinearApprox on an all-BAD MathMap
        // returns 0 with fit all AST__BAD, contradicting the SUN/211 note
        // about returning the unit transform.)
        return Ok(None);
    }

    // Step 4: build test points and verify the fit reproduces the
    // mapping within `tol` (Cartesian distance) at every test point.
    // Test-point count: 1 + 2 * (ndim_in + 2^ndim_in).
    let nverts = 1usize << ndim_in;
    let npoint = 1 + 2 * (ndim_in + nverts);
    let mut pts_in_t = PointSet::zeros(npoint, ndim_in);

    if ndim_in == 1 {
        // 1-D: distribute uniformly avoiding the endpoints.
        for point in 0..npoint {
            let frac = (point + 1) as f64 / (npoint + 1) as f64;
            pts_in_t.point_mut(point)[0] = (1.0 - frac) * lbnd[0] + frac * ubnd[0];
        }
    } else {
        // Centre point, slightly offset from exact centre.
        {
            let mut row = pts_in_t.point_mut(0);
            for coord_in in 0..ndim_in {
                row[coord_in] = 0.49 * lbnd[coord_in] + 0.51 * ubnd[coord_in];
            }
        }

        // Half-way between centre and each face centre. The face axis is
        // pulled 0.51:0.49 toward the face boundary.
        let mut point = 1;
        for face in 0..(2 * ndim_in) {
            // Capture point-0 axis values *before* taking a mut borrow to
            // pts_in_t.point_mut(point) for the new row.
            let centre0_axis_value = pts_in_t.point(0)[face / 2];
            {
                let mut row = pts_in_t.point_mut(point);
                for coord_in in 0..ndim_in {
                    row[coord_in] = 0.48 * lbnd[coord_in] + 0.52 * ubnd[coord_in];
                }
                let axis = face / 2;
                let face_bound = if face % 2 == 1 {
                    ubnd[axis]
                } else {
                    lbnd[axis]
                };
                row[axis] = 0.51 * face_bound + 0.49 * centre0_axis_value;
            }
            point += 1;
        }

        // Per vertex: vertex itself + halfway-from-centre to vertex.
        let mut vertex = vec![0_u8; ndim_in];
        loop {
            // Snapshot point-0 values for the halfway-to-centre row
            // before mutably borrowing further rows.
            let mut centre_snapshot = vec![0.0_f64; ndim_in];
            for coord_in in 0..ndim_in {
                centre_snapshot[coord_in] = pts_in_t.point(0)[coord_in];
            }

            for coord_in in 0..ndim_in {
                let v = if vertex[coord_in] != 0 {
                    ubnd[coord_in]
                } else {
                    lbnd[coord_in]
                };
                pts_in_t.point_mut(point)[coord_in] = v;
                pts_in_t.point_mut(point + 1)[coord_in] =
                    0.52 * v + 0.48 * centre_snapshot[coord_in];
            }
            point += 2;

            // Increment vertex flag (binary counter).
            let mut coord_in = 0;
            let done = loop {
                if vertex[coord_in] == 0 {
                    vertex[coord_in] = 1;
                    break false;
                } else {
                    vertex[coord_in] = 0;
                    coord_in += 1;
                    if coord_in == ndim_in {
                        break true;
                    }
                }
            };
            if done {
                break;
            }
        }
    }

    // Transform the test points and verify every point is within tol.
    let pts_out_t = map.transform(&pts_in_t, true)?;

    let (zero_slice, grad_slice) = fit.split_at_mut(ndim_out);
    let mut linear_ok = true;
    for point in 0..npoint {
        let inp = pts_in_t.point(point);
        let outp = pts_out_t.point(point);
        let mut err = 0.0_f64;
        let mut bad_output = 0_usize;
        let mut ii = 0_usize;
        for coord_out in 0..ndim_out {
            let y = outp[coord_out];
            if is_bad(y) || zero_slice[coord_out] == BAD_VALUE {
                // Mark this output as BAD across both shift and matrix row,
                // and continue to the next test point.
                zero_slice[coord_out] = BAD_VALUE;
                for _ in 0..ndim_in {
                    grad_slice[ii] = BAD_VALUE;
                    ii += 1;
                }
                bad_output += 1;
                // C: `break;` — abandon this point's accuracy check, leave
                // remaining outputs as-is for this iteration.
                break;
            }

            let mut yfit = zero_slice[coord_out];
            for coord_in in 0..ndim_in {
                yfit += grad_slice[ii] * inp[coord_in];
                ii += 1;
            }
            let diff = y - yfit;
            err += diff * diff;
        }

        // Tolerance is reduced in proportion to the number of bad outputs.
        let allowed = tol * (ndim_out - bad_output) as f64 / ndim_out as f64;
        if crate::math::sqrt(err) > allowed {
            linear_ok = false;
            break;
        }
    }

    if !linear_ok {
        return Ok(None);
    }

    // Pack into LinearFit.
    let shift: Vec<f64> = fit[..ndim_out].to_vec();
    let matrix: Vec<f64> = fit[ndim_out..].to_vec();
    Ok(Some(LinearFit { shift, matrix }))
}

#[cfg(test)]
mod tests {
    use super::LinearFit;
    use crate::mappings::{
        DynMapping, Mapping, MatrixMap, PolyMap, PolyTerm, ShiftMap, WcsMap, WcsProjection, ZoomMap,
    };
    use ndarray::Array2;

    // Captured from tools/oracle_fixtures/linear_approx.c via
    // `make linear_approx > captured.rs`. Regenerate when the
    // upstream reference C library is rebuilt. See
    // tools/oracle_fixtures/README.md for the provenance rule.

    /// Fixture: LINEAR_APPROX_IDENTITY_POLY
    /// nin=2, nout=2
    pub const LINEAR_APPROX_IDENTITY_POLY: Option<&[f64]> = Some(&[2.0, 0.0, 1.0, 1.0, 0.0, 3.0]);

    /// Fixture: LINEAR_APPROX_SHIFT_3D
    /// nin=3, nout=3
    pub const LINEAR_APPROX_SHIFT_3D: Option<&[f64]> = Some(&[
        10.0, 20.0, 30.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
    ]);

    /// Fixture: LINEAR_APPROX_TAN_TINY
    /// nin=2, nout=2
    pub const LINEAR_APPROX_TAN_TINY: Option<&[f64]> = None;

    /// Fixture: LINEAR_APPROX_TAN_LARGE
    /// nin=2, nout=2
    pub const LINEAR_APPROX_TAN_LARGE: Option<&[f64]> = None;

    /// Fixture: LINEAR_APPROX_ZOOM_1D
    /// nin=1, nout=1
    pub const LINEAR_APPROX_ZOOM_1D: Option<&[f64]> = Some(&[0.0, 7.5]);

    /// Fixture: LINEAR_APPROX_MATRIX_2X3
    /// nin=2, nout=3
    pub const LINEAR_APPROX_MATRIX_2X3: Option<&[f64]> =
        Some(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

    fn assert_close(actual: &[f64], expected: &[f64], tol: f64, label: &str) {
        assert_eq!(actual.len(), expected.len(), "{} length mismatch", label);
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!(
                (a - e).abs() < tol,
                "{}: index {} actual={} expected={}",
                label,
                i,
                a,
                e
            );
        }
    }

    fn term(coeff: f64, out_axis: usize, powers: &[usize]) -> PolyTerm {
        PolyTerm {
            coeff,
            powers: powers.to_vec(),
            out_axis,
        }
    }

    fn identity_polymap() -> DynMapping {
        // Forward: out1 = x + y + 2, out2 = 3y. Mirrors the C oracle fixture.
        let fwd = vec![
            term(1.0, 1, &[1, 0]),
            term(1.0, 1, &[0, 1]),
            term(2.0, 1, &[0, 0]),
            term(3.0, 2, &[0, 1]),
        ];
        Box::new(PolyMap::new(2, 2, &fwd, None).expect("polymap"))
    }

    #[test]
    fn identity_polymap_2x2() {
        let pm = identity_polymap();
        let fit = pm
            .linear_approx(&[-1.0, -1.0], &[1.0, 1.0], 0.001)
            .unwrap()
            .expect("rust returned a fit");
        let expected = LINEAR_APPROX_IDENTITY_POLY.expect("oracle returned a fit");
        assert_close(&fit.as_c_layout(), expected, 1e-12, "identity polymap");
    }

    #[test]
    fn shift_3d() {
        let sm = ShiftMap::new(vec![10.0, 20.0, 30.0]);
        let fit = sm
            .linear_approx(&[0.0, 0.0, 0.0], &[100.0, 100.0, 100.0], 0.001)
            .unwrap()
            .expect("rust returned a fit");
        let expected = LINEAR_APPROX_SHIFT_3D.expect("oracle returned a fit");
        assert_close(&fit.as_c_layout(), expected, 1e-12, "shift 3d");
    }

    #[test]
    fn zoom_1d() {
        let zm = ZoomMap::new(1, 7.5).expect("zoommap");
        let fit = zm
            .linear_approx(&[-2.0], &[2.0], 0.001)
            .unwrap()
            .expect("rust returned a fit");
        let expected = LINEAR_APPROX_ZOOM_1D.expect("oracle returned a fit");
        assert_close(&fit.as_c_layout(), expected, 1e-12, "zoom 1d");
    }

    #[test]
    fn matrix_2x3() {
        // 3-row × 2-col matrix → nout=3, nin=2. C oracle layout (row-major):
        //   row 0: 1 2  → out0 = x + 2y
        //   row 1: 3 4  → out1 = 3x + 4y
        //   row 2: 5 6  → out2 = 5x + 6y
        let m = Array2::from_shape_vec((3, 2), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
            .expect("array2 shape");
        let mm = MatrixMap::new(m).expect("matrixmap");
        let fit = mm
            .linear_approx(&[-5.0, -5.0], &[5.0, 5.0], 0.001)
            .unwrap()
            .expect("rust returned a fit");
        let expected = LINEAR_APPROX_MATRIX_2X3.expect("oracle returned a fit");
        assert_close(&fit.as_c_layout(), expected, 1e-12, "matrix 2x3");
    }

    #[test]
    fn tan_tiny_box_fits() {
        // TAN over [-1e-3, 1e-3] radians — locally linear.
        // The C oracle marked this as None (the projection is not
        // *exactly* identity even at this scale; with tol=1e-9 it fails).
        // Verify we agree with the C oracle.
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).expect("wcsmap");
        let fit = wm
            .linear_approx(&[-0.001, -0.001], &[0.001, 0.001], 1.0e-9)
            .unwrap();
        assert_eq!(LINEAR_APPROX_TAN_TINY, None);
        assert!(
            fit.is_none(),
            "expected None to match C oracle at tol=1e-9, got Some"
        );
    }

    #[test]
    fn tan_too_large_refuses() {
        // TAN over [-1, 1] rad is highly non-linear → C refuses.
        let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).expect("wcsmap");
        let fit = wm.linear_approx(&[-1.0, -1.0], &[1.0, 1.0], 0.001).unwrap();
        assert_eq!(LINEAR_APPROX_TAN_LARGE, None);
        assert!(fit.is_none(), "expected non-linear refusal");
    }

    #[test]
    fn dimension_mismatch_is_an_error() {
        let zm = ZoomMap::new(2, 1.0).expect("zoommap");
        let err = zm.linear_approx(&[0.0], &[1.0, 2.0], 1e-3).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::Ncpin);
    }

    #[test]
    fn transform_error_is_reported() {
        // mapping.c astLinearApprox: an error from astTransform propagates.
        // A forward-only RateMap inverted has no forward transformation.
        let mut rm =
            crate::mappings::RateMap::new(Box::new(ZoomMap::new(1, 2.0).unwrap()), 1, 1).unwrap();
        rm.invert();
        let err = rm.linear_approx(&[0.0], &[1.0], 1e-3).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::Trnnd);
    }

    #[test]
    fn returns_correct_type() {
        // Type assertion: trait method returns AstResult<Option<LinearFit>>.
        let zm = ZoomMap::new(1, 2.0).expect("zoommap");
        let _: crate::error::AstResult<Option<LinearFit>> = zm.linear_approx(&[0.0], &[1.0], 1e-3);
    }
}
