// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Cross-lane SIMD prototype for [`PolyMap`] forward evaluation (2D).
//!
//! Process `LANES` points per iteration using [`wide::f64x4`]. The pipeline is:
//!
//! 1. A copy of the input's two axes, combined with a `BAD_VALUE →
//!    f64::NAN` substitution so the kernel can run branchlessly. NaN propagates
//!    through arithmetic at no extra cost.
//! 2. SIMD kernel evaluates the polynomial 4 points at a time, walking the
//!    same `forward_compact` term layout as the scalar path. Inner loop is
//!    pure FMA (`coeff * x^p0 * y^p1` accumulated into the output lane).
//! 3. A copy back into the output's axes, combined with `NaN → BAD_VALUE`
//!    substitution.
//!
//! The two copy passes are O(N) memory-bandwidth-bound; the kernel is
//! O(N · t) where `t` is the term count. The threshold [`SIMD_MIN_POINTS`]
//! controls when SIMD beats scalar for a given PolyMap shape — below that
//! the setup cost (copy passes + heap power-table) dominates. Tune via
//! `cargo run --example bench_polymap_simd --release --features simd-eval`.
//!
//! **Specialization**: this prototype implements `nin = nout = 2` only,
//! since that's the LSST WCS workload and the only shape benchmarked so far.
//! Generalising to `nin = nout = N` is straightforward (axis loop becomes
//! dynamic) but should be motivated by an actual workload first.
//!
//! Gated behind `feature = "simd-eval"`. Not used by default builds.

use wide::{CmpLt, f64x4};

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};

use super::Mapping;
use super::cmpmap::CmpMap;
use super::polymap::{PolyMap, PolyTerm};

/// Recursively descend a Mapping looking for a [`PolyMap`]. Returns a
/// reference to the first one found (left-to-right walk through any
/// [`CmpMap`] nesting). Used by the SIMD bench example to extract the inner
/// PolyMap from a simplified WCS FrameSet so the benchmark measures the
/// polynomial kernel in isolation.
pub fn find_polymap(m: &dyn Mapping) -> Option<&PolyMap> {
    if let Some(p) = m.as_any().downcast_ref::<PolyMap>() {
        return Some(p);
    }
    if let Some(cm) = m.as_any().downcast_ref::<CmpMap>() {
        if let Some(p) = find_polymap(cm.first()) {
            return Some(p);
        }
        if let Some(p) = find_polymap(cm.second()) {
            return Some(p);
        }
    }
    None
}

const LANES: usize = 4;

/// Minimum point count for SIMD path to be worth taking. Below this, the
/// The copy passes + heap power-table setup dominates the kernel time.
///
/// Picked from a single-host bench-run on Apple M-series; tune for other
/// hardware. The scalar path is only ~98 ns/pt on this workload, so the
/// SIMD win is in the 30-50% range and the crossover is fairly low.
pub const SIMD_MIN_POINTS: usize = 128;

/// Did the SIMD path run, or did we bail out to scalar? Lets benchmarks
/// distinguish a "SIMD result" from a "fell-back result".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdPath {
    /// SIMD kernel ran end-to-end.
    Took,
    /// Fell back (shape mismatch, too few points, etc.) — output is unchanged.
    Fell,
}

impl PolyMap {
    /// Forward-transform `input` into `output` using the cross-lane SIMD
    /// prototype, falling back to the scalar `transform_into` path if the
    /// PolyMap shape isn't supported by this prototype or `npoint` is below
    /// [`SIMD_MIN_POINTS`].
    ///
    /// Returns [`SimdPath::Took`] if the SIMD path actually ran. Bench code
    /// uses this to attribute timings correctly.
    pub fn forward_simd_2d_into(
        &self,
        input: &Points<'_>,
        output: &mut PointsMut<'_>,
    ) -> AstResult<SimdPath> {
        let npoint = input.npoint();
        if self.nin() != 2 || self.nout() != 2 {
            return Ok(SimdPath::Fell);
        }
        if input.ncoord() != 2 || output.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!(
                    "forward_simd_2d_into: input/output must have ncoord = 2 \
                     (got {}/{})",
                    input.ncoord(),
                    output.ncoord()
                ),
            ));
        }
        if output.npoint() != npoint {
            return Err(AstError::new(
                ErrorCode::Badin,
                "forward_simd_2d_into: input/output npoint mismatch",
            ));
        }
        if npoint < SIMD_MIN_POINTS {
            return Ok(SimdPath::Fell);
        }

        // Allocate SoA staging buffers. Single allocation per axis, reused
        // for input and output (output overwrites after kernel completes).
        let mut xin = vec![0.0_f64; npoint];
        let mut yin = vec![0.0_f64; npoint];
        let mut xout = vec![0.0_f64; npoint];
        let mut yout = vec![0.0_f64; npoint];

        // Pass 1: copy the axes with BAD → NaN substitution.
        let (fx, fy) = (input.axis(0), input.axis(1));
        for ip in 0..npoint {
            let x = fx[ip];
            let y = fy[ip];
            xin[ip] = if x == BAD_VALUE { f64::NAN } else { x };
            yin[ip] = if y == BAD_VALUE { f64::NAN } else { y };
        }

        // Pass 2: SIMD kernel evaluates the polynomial 4 points at a time.
        // PolyMap's existing layout is index-based (`forward_by_output`); we
        // gather per-output term references once here so the kernel walks
        // contiguous reference slices instead of indexing back through
        // `forward[idx]` per term per chunk.
        let (out0_refs, out1_refs, max_pows) = self.simd_forward_layout_2d();
        eval_forward_2d_soa(
            &out0_refs, &out1_refs, max_pows, &xin, &yin, &mut xout, &mut yout,
        );

        // Pass 3: copy back with NaN → BAD substitution.
        for ip in 0..npoint {
            let x = xout[ip];
            let y = yout[ip];
            output.axis_mut(0)[ip] = if x.is_nan() { BAD_VALUE } else { x };
            output.axis_mut(1)[ip] = if y.is_nan() { BAD_VALUE } else { y };
        }

        Ok(SimdPath::Took)
    }

    /// Protocol-internal forward SIMD eval (2D).
    ///
    /// Same SIMD pipeline as [`forward_simd_2d_into`], but assumes the
    /// caller is inside the NaN-as-bad protocol (i.e. invocations come
    /// from `transform_into_nan`). Therefore:
    ///   - input substitution `BAD_VALUE → NaN` is kept (cheap defense
    ///     against accidental BAD slipping in; no-op for the canonical
    ///     NaN inputs the protocol promises);
    ///   - output substitution `NaN → BAD_VALUE` is **dropped**, because
    ///     the protocol's contract is "NaN stays inside the pipeline" —
    ///     converting back to BAD here would leak `-1.8e308` to the next
    ///     stage's NaN-aware kernel and produce arithmetic garbage.
    ///
    /// The boundary-substitution pass at the chain's public entry
    /// ([`dyn Mapping::transform_nan_protocol`]) handles NaN → BAD on
    /// final exit, once per pipeline run.
    pub fn forward_simd_2d_into_nan(
        &self,
        input: &Points<'_>,
        output: &mut PointsMut<'_>,
    ) -> AstResult<SimdPath> {
        let npoint = input.npoint();
        if self.nin() != 2 || self.nout() != 2 {
            return Ok(SimdPath::Fell);
        }
        if input.ncoord() != 2 || output.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!(
                    "forward_simd_2d_into_nan: input/output must have ncoord = 2 \
                     (got {}/{})",
                    input.ncoord(),
                    output.ncoord()
                ),
            ));
        }
        if output.npoint() != npoint {
            return Err(AstError::new(
                ErrorCode::Badin,
                "forward_simd_2d_into_nan: input/output npoint mismatch",
            ));
        }
        if npoint < SIMD_MIN_POINTS {
            return Ok(SimdPath::Fell);
        }

        let mut xin = vec![0.0_f64; npoint];
        let mut yin = vec![0.0_f64; npoint];
        let mut xout = vec![0.0_f64; npoint];
        let mut yout = vec![0.0_f64; npoint];

        // Input: copy through, lifting any BAD that slipped past the
        // boundary into NaN. Cheap and defensive.
        let (fx, fy) = (input.axis(0), input.axis(1));
        for ip in 0..npoint {
            let x = fx[ip];
            let y = fy[ip];
            xin[ip] = if x == BAD_VALUE { f64::NAN } else { x };
            yin[ip] = if y == BAD_VALUE { f64::NAN } else { y };
        }

        let (out0_refs, out1_refs, max_pows) = self.simd_forward_layout_2d();
        eval_forward_2d_soa(
            &out0_refs, &out1_refs, max_pows, &xin, &yin, &mut xout, &mut yout,
        );

        // Output: straight copy — leave NaN as NaN so downstream
        // protocol-aware stages keep the canonical sentinel.
        output.axis_mut(0)[..npoint].copy_from_slice(&xout);
        output.axis_mut(1)[..npoint].copy_from_slice(&yout);

        Ok(SimdPath::Took)
    }
}

/// 4-points-at-a-time forward evaluator for `nin = nout = 2` PolyMaps,
/// working on SoA inputs/outputs. NaN inputs propagate to NaN outputs for
/// free; the caller handles BAD/NaN substitution outside this kernel.
///
/// `out0` and `out1` are slices of references into the PolyMap's forward
/// terms, gathered per-output-axis by the caller so the inner loop walks
/// references contiguously without indexing through `forward_by_output`.
fn eval_forward_2d_soa(
    out0: &[&PolyTerm],
    out1: &[&PolyTerm],
    max_pows: [usize; 2],
    xin: &[f64],
    yin: &[f64],
    xout: &mut [f64],
    yout: &mut [f64],
) {
    let npoint = xin.len();
    debug_assert_eq!(yin.len(), npoint);
    debug_assert_eq!(xout.len(), npoint);
    debug_assert_eq!(yout.len(), npoint);

    let max_p0 = max_pows[0];
    let max_p1 = max_pows[1];

    // Power tables: one entry per power level, each entry is a 4-lane
    // vector (4 points worth of x^p / y^p). Heap-allocated once; reused
    // across all chunks of the chunk loop.
    let mut x_pow: Vec<f64x4> = vec![f64x4::splat(1.0); max_p0 + 1];
    let mut y_pow: Vec<f64x4> = vec![f64x4::splat(1.0); max_p1 + 1];

    let chunks = npoint / LANES;
    for ck in 0..chunks {
        let base = ck * LANES;
        let x = f64x4::new([xin[base], xin[base + 1], xin[base + 2], xin[base + 3]]);
        let y = f64x4::new([yin[base], yin[base + 1], yin[base + 2], yin[base + 3]]);

        // x^0 = 1 (already set at allocation), x^1 = x, x^p = x^(p-1) * x.
        if max_p0 >= 1 {
            x_pow[1] = x;
            for p in 2..=max_p0 {
                x_pow[p] = x_pow[p - 1] * x;
            }
        }
        if max_p1 >= 1 {
            y_pow[1] = y;
            for p in 2..=max_p1 {
                y_pow[p] = y_pow[p - 1] * y;
            }
        }

        // Evaluate output 0: y0 = Σ coeff * x^p0 * y^p1.
        let mut yo0 = f64x4::splat(0.0);
        for &term in out0 {
            let coeff = f64x4::splat(term.coeff);
            let p0 = term.powers[0];
            let p1 = term.powers[1];
            // mul_add lowers to FMA on platforms that have it (e.g. NEON FMA,
            // x86 FMA3). On platforms without hardware FMA, wide falls back
            // to separate mul + add.
            yo0 = (coeff * x_pow[p0]).mul_add(y_pow[p1], yo0);
        }
        let mut yo1 = f64x4::splat(0.0);
        for &term in out1 {
            let coeff = f64x4::splat(term.coeff);
            let p0 = term.powers[0];
            let p1 = term.powers[1];
            yo1 = (coeff * x_pow[p0]).mul_add(y_pow[p1], yo1);
        }

        let yo0_arr = yo0.to_array();
        let yo1_arr = yo1.to_array();
        xout[base..base + LANES].copy_from_slice(&yo0_arr);
        yout[base..base + LANES].copy_from_slice(&yo1_arr);
    }

    // Scalar tail (last 0..LANES-1 points).
    let tail_start = chunks * LANES;
    if tail_start < npoint {
        let mut xs = vec![1.0_f64; max_p0 + 1];
        let mut ys = vec![1.0_f64; max_p1 + 1];
        for ip in tail_start..npoint {
            let x = xin[ip];
            let y = yin[ip];
            for p in 1..=max_p0 {
                xs[p] = if p == 1 { x } else { xs[p - 1] * x };
            }
            for p in 1..=max_p1 {
                ys[p] = if p == 1 { y } else { ys[p - 1] * y };
            }
            let mut o0 = 0.0;
            for &term in out0 {
                o0 += term.coeff * xs[term.powers[0]] * ys[term.powers[1]];
            }
            xout[ip] = o0;
            let mut o1 = 0.0;
            for &term in out1 {
                o1 += term.coeff * xs[term.powers[0]] * ys[term.powers[1]];
            }
            yout[ip] = o1;
        }
    }
}

// ---------------------------------------------------------------------------
// SIMD Newton-Raphson inverse (2D)
// ---------------------------------------------------------------------------

impl PolyMap {
    /// Inverse-transform `target` into `result` using the cross-lane SIMD
    /// Newton-Raphson prototype, falling back to the scalar path when the
    /// PolyMap shape isn't supported or `npoint` is below
    /// [`SIMD_MIN_POINTS`].
    ///
    /// Behaves exactly like the scalar iterative inverse semantically: same
    /// `niter_inverse`/`tol_inverse` limits, same convergence criterion, same
    /// BAD-value propagation. The win comes from evaluating forward + the
    /// full 2x2 Jacobian for 4 points at a time in a single pass over the
    /// forward terms (no separate "Jacobian polynomial" data structure), then
    /// doing the 2x2 Cramer solve and convergence check in SIMD.
    ///
    /// Returns [`SimdPath::Took`] if the SIMD path actually ran. Bench code
    /// uses this to attribute timings correctly.
    pub fn inverse_simd_2d_into(
        &self,
        target: &Points<'_>,
        result: &mut PointsMut<'_>,
    ) -> AstResult<SimdPath> {
        let npoint = target.npoint();
        if self.nin() != 2 || self.nout() != 2 {
            return Ok(SimdPath::Fell);
        }
        if target.ncoord() != 2 || result.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!(
                    "inverse_simd_2d_into: input/output must have ncoord = 2 \
                     (got {}/{})",
                    target.ncoord(),
                    result.ncoord()
                ),
            ));
        }
        if result.npoint() != npoint {
            return Err(AstError::new(
                ErrorCode::Badin,
                "inverse_simd_2d_into: input/output npoint mismatch",
            ));
        }
        if npoint < SIMD_MIN_POINTS {
            return Ok(SimdPath::Fell);
        }

        // Pass 1: copy the target axes with BAD → NaN substitution. SoA result
        // buffers start empty; the linear-guess step fills them.
        let mut xt = vec![0.0_f64; npoint];
        let mut yt = vec![0.0_f64; npoint];
        let mut xr = vec![0.0_f64; npoint];
        let mut yr = vec![0.0_f64; npoint];

        let (fx, fy) = (target.axis(0), target.axis(1));
        for ip in 0..npoint {
            let x = fx[ip];
            let y = fy[ip];
            xt[ip] = if x == BAD_VALUE { f64::NAN } else { x };
            yt[ip] = if y == BAD_VALUE { f64::NAN } else { y };
        }

        // Pass 2: build the linear-truncation initial guess. We extract the
        // constant and linear coefficients from forward once (scalar), invert
        // the 2x2 linear matrix, then apply the inverse in SIMD over all
        // points. If the linear part is singular, fall back to copying target
        // as the guess (mirrors scalar `compute_linear_guess`).
        let (a_inv, b) = match linear_inverse_2d(&self.forward_terms_2d()) {
            Some(pair) => pair,
            None => {
                // Singular linear part: identity-guess (target as input).
                xr.copy_from_slice(&xt);
                yr.copy_from_slice(&yt);
                ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0])
            }
        };
        linear_guess_2d_soa(a_inv, b, &xt, &yt, &mut xr, &mut yr);

        // Pass 3: SIMD Newton-Raphson loop.
        let (out0_refs, out1_refs, max_pows) = self.simd_forward_layout_2d();
        nr_inverse_2d_soa(
            &out0_refs,
            &out1_refs,
            max_pows,
            self.niter_inverse(),
            self.tol_inverse(),
            &xt,
            &yt,
            &mut xr,
            &mut yr,
        );

        // Pass 4: copy back into the result with NaN → BAD substitution.
        for ip in 0..npoint {
            let x = xr[ip];
            let y = yr[ip];
            result.axis_mut(0)[ip] = if x.is_nan() { BAD_VALUE } else { x };
            result.axis_mut(1)[ip] = if y.is_nan() { BAD_VALUE } else { y };
        }

        Ok(SimdPath::Took)
    }

    /// Protocol-internal inverse SIMD eval (2D).
    ///
    /// See [`forward_simd_2d_into_nan`](Self::forward_simd_2d_into_nan)
    /// for rationale. Same inverse pipeline as `inverse_simd_2d_into`,
    /// with the output NaN → BAD substitution dropped so downstream
    /// NaN-aware stages keep receiving NaN as the canonical "invalid"
    /// sentinel rather than the finite `BAD_VALUE = -1.8e308`.
    pub fn inverse_simd_2d_into_nan(
        &self,
        target: &Points<'_>,
        result: &mut PointsMut<'_>,
    ) -> AstResult<SimdPath> {
        let npoint = target.npoint();
        if self.nin() != 2 || self.nout() != 2 {
            return Ok(SimdPath::Fell);
        }
        if target.ncoord() != 2 || result.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!(
                    "inverse_simd_2d_into_nan: input/output must have ncoord = 2 \
                     (got {}/{})",
                    target.ncoord(),
                    result.ncoord()
                ),
            ));
        }
        if result.npoint() != npoint {
            return Err(AstError::new(
                ErrorCode::Badin,
                "inverse_simd_2d_into_nan: input/output npoint mismatch",
            ));
        }
        if npoint < SIMD_MIN_POINTS {
            return Ok(SimdPath::Fell);
        }

        let mut xt = vec![0.0_f64; npoint];
        let mut yt = vec![0.0_f64; npoint];
        let mut xr = vec![0.0_f64; npoint];
        let mut yr = vec![0.0_f64; npoint];

        // Defensive BAD → NaN lift on input; no-op when the caller is
        // already inside the protocol (canonical NaN already).
        let (fx, fy) = (target.axis(0), target.axis(1));
        for ip in 0..npoint {
            let x = fx[ip];
            let y = fy[ip];
            xt[ip] = if x == BAD_VALUE { f64::NAN } else { x };
            yt[ip] = if y == BAD_VALUE { f64::NAN } else { y };
        }

        let (a_inv, b) = match linear_inverse_2d(&self.forward_terms_2d()) {
            Some(pair) => pair,
            None => {
                xr.copy_from_slice(&xt);
                yr.copy_from_slice(&yt);
                ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0])
            }
        };
        linear_guess_2d_soa(a_inv, b, &xt, &yt, &mut xr, &mut yr);

        let (out0_refs, out1_refs, max_pows) = self.simd_forward_layout_2d();
        nr_inverse_2d_soa(
            &out0_refs,
            &out1_refs,
            max_pows,
            self.niter_inverse(),
            self.tol_inverse(),
            &xt,
            &yt,
            &mut xr,
            &mut yr,
        );

        // Output: straight copy; leave NaN as NaN.
        result.axis_mut(0)[..npoint].copy_from_slice(&xr);
        result.axis_mut(1)[..npoint].copy_from_slice(&yr);

        Ok(SimdPath::Took)
    }

    /// Forward terms grouped per output axis as `(&[&PolyTerm], &[&PolyTerm])`.
    /// Helper for the inverse path's linear-guess setup, which needs to scan
    /// the constant + linear coefficients of the forward polynomial.
    fn forward_terms_2d(&self) -> [Vec<&PolyTerm>; 2] {
        let (a, b, _) = self.simd_forward_layout_2d();
        [a, b]
    }
}

/// Extract the linear part of a 2-output forward polynomial and invert it.
///
/// For each output axis, scans terms that are pure constants (all powers 0)
/// or pure linear (exactly one power = 1, rest = 0) and accumulates them
/// into a 2x2 matrix `A` and a 2-vector `b` such that
/// `y ≈ A x + b` near the linear truncation. Returns `(A^{-1}, b)` if `A`
/// is non-singular, else `None`.
fn linear_inverse_2d(terms_by_output: &[Vec<&PolyTerm>; 2]) -> Option<([f64; 4], [f64; 2])> {
    let mut a = [0.0_f64; 4]; // row-major 2x2: a[r*2 + c]
    let mut b = [0.0_f64; 2];

    for (out, axis_terms) in terms_by_output.iter().enumerate() {
        for &term in axis_terms.iter() {
            let p0 = term.powers[0];
            let p1 = term.powers[1];
            match (p0, p1) {
                (0, 0) => b[out] += term.coeff,
                (1, 0) => a[out * 2] += term.coeff,
                (0, 1) => a[out * 2 + 1] += term.coeff,
                _ => {}
            }
        }
    }

    let det = a[0] * a[3] - a[1] * a[2];
    if det.abs() < 1e-30 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some((
        [
            a[3] * inv_det,
            -a[1] * inv_det,
            -a[2] * inv_det,
            a[0] * inv_det,
        ],
        b,
    ))
}

/// SIMD initial guess `x = A^{-1} * (target - b)` applied across all points.
fn linear_guess_2d_soa(
    a_inv: [f64; 4],
    b: [f64; 2],
    xt: &[f64],
    yt: &[f64],
    xr: &mut [f64],
    yr: &mut [f64],
) {
    let npoint = xt.len();
    let ai00 = f64x4::splat(a_inv[0]);
    let ai01 = f64x4::splat(a_inv[1]);
    let ai10 = f64x4::splat(a_inv[2]);
    let ai11 = f64x4::splat(a_inv[3]);
    let b0 = f64x4::splat(b[0]);
    let b1 = f64x4::splat(b[1]);

    let chunks = npoint / LANES;
    for ck in 0..chunks {
        let base = ck * LANES;
        let tx = f64x4::new([xt[base], xt[base + 1], xt[base + 2], xt[base + 3]]);
        let ty = f64x4::new([yt[base], yt[base + 1], yt[base + 2], yt[base + 3]]);
        let dx = tx - b0;
        let dy = ty - b1;
        let x = ai00 * dx + ai01 * dy;
        let y = ai10 * dx + ai11 * dy;
        xr[base..base + LANES].copy_from_slice(&x.to_array());
        yr[base..base + LANES].copy_from_slice(&y.to_array());
    }
    // Scalar tail.
    for ip in (chunks * LANES)..npoint {
        let dx = xt[ip] - b[0];
        let dy = yt[ip] - b[1];
        xr[ip] = a_inv[0] * dx + a_inv[1] * dy;
        yr[ip] = a_inv[2] * dx + a_inv[3] * dy;
    }
}

/// Cross-lane SIMD Newton-Raphson inverse kernel (nin = nout = 2).
///
/// Reads `xtarget`/`ytarget` (sky-domain SoA inputs), uses `xresult`/`yresult`
/// as both initial guess (from `linear_guess_2d_soa`) and final result. Each
/// chunk of `LANES` points carries a "this chunk has any unconverged lane"
/// flag — once every lane in a chunk passes the convergence test (or has
/// gone NaN, which we treat as "done with no answer"), we skip that chunk on
/// subsequent iterations. The outer loop early-exits once every chunk is done.
fn nr_inverse_2d_soa(
    out0: &[&PolyTerm],
    out1: &[&PolyTerm],
    max_pows: [usize; 2],
    max_iter: usize,
    tol: f64,
    xtarget: &[f64],
    ytarget: &[f64],
    xresult: &mut [f64],
    yresult: &mut [f64],
) {
    let npoint = xtarget.len();
    debug_assert_eq!(ytarget.len(), npoint);
    debug_assert_eq!(xresult.len(), npoint);
    debug_assert_eq!(yresult.len(), npoint);

    let chunks = npoint / LANES;
    let tail_start = chunks * LANES;
    let tol_sq = f64x4::splat(tol * tol);

    let mut x_pow: Vec<f64x4> = vec![f64x4::splat(1.0); max_pows[0] + 1];
    let mut y_pow: Vec<f64x4> = vec![f64x4::splat(1.0); max_pows[1] + 1];

    // Per-chunk "all 4 lanes are done" flag.
    let mut chunk_done = vec![false; chunks];

    for _iter in 0..max_iter {
        let mut any_active = false;

        for ck in 0..chunks {
            if chunk_done[ck] {
                continue;
            }
            any_active = true;

            let base = ck * LANES;
            let x = f64x4::new([
                xresult[base],
                xresult[base + 1],
                xresult[base + 2],
                xresult[base + 3],
            ]);
            let y = f64x4::new([
                yresult[base],
                yresult[base + 1],
                yresult[base + 2],
                yresult[base + 3],
            ]);
            let tx = f64x4::new([
                xtarget[base],
                xtarget[base + 1],
                xtarget[base + 2],
                xtarget[base + 3],
            ]);
            let ty = f64x4::new([
                ytarget[base],
                ytarget[base + 1],
                ytarget[base + 2],
                ytarget[base + 3],
            ]);

            // Build per-axis power tables for these 4 points.
            if max_pows[0] >= 1 {
                x_pow[1] = x;
                for p in 2..=max_pows[0] {
                    x_pow[p] = x_pow[p - 1] * x;
                }
            }
            if max_pows[1] >= 1 {
                y_pow[1] = y;
                for p in 2..=max_pows[1] {
                    y_pow[p] = y_pow[p - 1] * y;
                }
            }

            // Single pass through forward terms producing both the forward
            // output and the Jacobian columns (d/dx, d/dy). Each term's
            // derivative is computed on the fly from coefficient and powers.
            let mut yo0 = f64x4::splat(0.0);
            let mut j00 = f64x4::splat(0.0); // d(y0)/d(x)
            let mut j01 = f64x4::splat(0.0); // d(y0)/d(y)
            for &term in out0 {
                let coeff = f64x4::splat(term.coeff);
                let p0 = term.powers[0];
                let p1 = term.powers[1];
                yo0 = (coeff * x_pow[p0]).mul_add(y_pow[p1], yo0);
                if p0 > 0 {
                    let dc = f64x4::splat(term.coeff * (p0 as f64));
                    j00 = (dc * x_pow[p0 - 1]).mul_add(y_pow[p1], j00);
                }
                if p1 > 0 {
                    let dc = f64x4::splat(term.coeff * (p1 as f64));
                    j01 = (dc * x_pow[p0]).mul_add(y_pow[p1 - 1], j01);
                }
            }
            let mut yo1 = f64x4::splat(0.0);
            let mut j10 = f64x4::splat(0.0); // d(y1)/d(x)
            let mut j11 = f64x4::splat(0.0); // d(y1)/d(y)
            for &term in out1 {
                let coeff = f64x4::splat(term.coeff);
                let p0 = term.powers[0];
                let p1 = term.powers[1];
                yo1 = (coeff * x_pow[p0]).mul_add(y_pow[p1], yo1);
                if p0 > 0 {
                    let dc = f64x4::splat(term.coeff * (p0 as f64));
                    j10 = (dc * x_pow[p0 - 1]).mul_add(y_pow[p1], j10);
                }
                if p1 > 0 {
                    let dc = f64x4::splat(term.coeff * (p1 as f64));
                    j11 = (dc * x_pow[p0]).mul_add(y_pow[p1 - 1], j11);
                }
            }

            // Residual and 2x2 Cramer's-rule solve, all SIMD.
            let r0 = tx - yo0;
            let r1 = ty - yo1;
            let det = j00 * j11 - j01 * j10;
            let dx_step = (j11 * r0 - j01 * r1) / det;
            let dy_step = (j00 * r1 - j10 * r0) / det;
            let x_new = x + dx_step;
            let y_new = y + dy_step;

            // Per-lane convergence: |Δ|² < tol² · |x|² OR a NaN appeared
            // (NaN tracks an input that was BAD or a singular Jacobian — we
            // treat both as "this lane is done; output stays NaN").
            let vlensq = dx_step * dx_step + dy_step * dy_step;
            let xlensq = x_new * x_new + y_new * y_new;
            let conv = vlensq.cmp_lt(tol_sq * xlensq);
            let nan = x_new.is_nan() | y_new.is_nan();
            let done_mask = conv | nan;

            // Reduce mask to "all 4 lanes done?" (wide masks use the
            // all-bits-set / zero convention).
            let arr = done_mask.to_array();
            chunk_done[ck] = arr.iter().all(|&v| v.to_bits() != 0);

            xresult[base..base + LANES].copy_from_slice(&x_new.to_array());
            yresult[base..base + LANES].copy_from_slice(&y_new.to_array());
        }

        if !any_active {
            break;
        }
    }

    // Scalar tail: reuse the scalar NR loop for the last 0..3 points.
    // For an LSST-shaped workload this is at most 3 points; per-point cost
    // is independent of the SIMD savings so we just do it inline rather
    // than refactoring shared scalar code.
    if tail_start < npoint {
        let tol_sq_scalar = tol * tol;
        let mut xs = vec![1.0_f64; max_pows[0] + 1];
        let mut ys = vec![1.0_f64; max_pows[1] + 1];
        for ip in tail_start..npoint {
            for _iter in 0..max_iter {
                let mut x = xresult[ip];
                let mut y = yresult[ip];
                for p in 1..=max_pows[0] {
                    xs[p] = if p == 1 { x } else { xs[p - 1] * x };
                }
                for p in 1..=max_pows[1] {
                    ys[p] = if p == 1 { y } else { ys[p - 1] * y };
                }
                let (mut yo0, mut yo1) = (0.0_f64, 0.0_f64);
                let (mut a00, mut a01, mut a10, mut a11) = (0.0_f64, 0.0, 0.0, 0.0);
                for &t in out0 {
                    let p0 = t.powers[0];
                    let p1 = t.powers[1];
                    yo0 += t.coeff * xs[p0] * ys[p1];
                    if p0 > 0 {
                        a00 += t.coeff * (p0 as f64) * xs[p0 - 1] * ys[p1];
                    }
                    if p1 > 0 {
                        a01 += t.coeff * (p1 as f64) * xs[p0] * ys[p1 - 1];
                    }
                }
                for &t in out1 {
                    let p0 = t.powers[0];
                    let p1 = t.powers[1];
                    yo1 += t.coeff * xs[p0] * ys[p1];
                    if p0 > 0 {
                        a10 += t.coeff * (p0 as f64) * xs[p0 - 1] * ys[p1];
                    }
                    if p1 > 0 {
                        a11 += t.coeff * (p1 as f64) * xs[p0] * ys[p1 - 1];
                    }
                }
                let r0 = xtarget[ip] - yo0;
                let r1 = ytarget[ip] - yo1;
                let det = a00 * a11 - a01 * a10;
                if det.abs() < 1e-30 || !det.is_finite() {
                    xresult[ip] = f64::NAN;
                    yresult[ip] = f64::NAN;
                    break;
                }
                let dx = (a11 * r0 - a01 * r1) / det;
                let dy = (a00 * r1 - a10 * r0) / det;
                x += dx;
                y += dy;
                xresult[ip] = x;
                yresult[ip] = y;
                let v = dx * dx + dy * dy;
                let xn = x * x + y * y;
                if v < tol_sq_scalar * xn || xn == 0.0 {
                    break;
                }
            }
        }
    }
}
