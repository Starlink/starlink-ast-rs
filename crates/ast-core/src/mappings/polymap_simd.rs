//! Cross-lane SIMD prototype for [`PolyMap`] forward evaluation (2D).
//!
//! Process `LANES` points per iteration using [`wide::f64x4`]. The pipeline is:
//!
//! 1. AOS→SoA transpose of the input `PointSet`, combined with a `BAD_VALUE →
//!    f64::NAN` substitution so the kernel can run branchlessly. NaN propagates
//!    through arithmetic at no extra cost.
//! 2. SIMD kernel evaluates the polynomial 4 points at a time, walking the
//!    same `forward_compact` term layout as the scalar path. Inner loop is
//!    pure FMA (`coeff * x^p0 * y^p1` accumulated into the output lane).
//! 3. SoA→AOS transpose of the output, combined with `NaN → BAD_VALUE`
//!    substitution.
//!
//! The two transpose passes are O(N) memory-bandwidth-bound; the kernel is
//! O(N · t) where `t` is the term count. The threshold [`SIMD_MIN_POINTS`]
//! controls when SIMD beats scalar for a given PolyMap shape — below that
//! the setup cost (transpose passes + heap power-table) dominates. Tune via
//! `cargo run --example bench_polymap_simd --release --features simd-eval`.
//!
//! **Specialization**: this prototype implements `nin = nout = 2` only,
//! since that's the LSST WCS workload and the only shape benchmarked so far.
//! Generalising to `nin = nout = N` is straightforward (axis loop becomes
//! dynamic) but should be motivated by an actual workload first.
//!
//! Gated behind `feature = "simd-eval"`. Not used by default builds.

use wide::f64x4;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
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
/// SoA transpose + heap power-table setup dominates the kernel time.
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
        input: &PointSet,
        output: &mut PointSet,
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

        // Pass 1: AOS → SoA with BAD → NaN substitution.
        // The PointSet underlying buffer is contiguous in row-major order:
        // [x0, y0, x1, y1, ...]. Walk it in pairs.
        let view = input.view();
        let flat: &[f64] = view.to_slice().expect("PointSet is C-contiguous");
        debug_assert_eq!(flat.len(), npoint * 2);
        for ip in 0..npoint {
            let x = flat[ip * 2];
            let y = flat[ip * 2 + 1];
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

        // Pass 3: SoA → AOS with NaN → BAD substitution.
        let mut out_view = output.view_mut();
        let out_flat: &mut [f64] = out_view.as_slice_mut().expect("PointSet is C-contiguous");
        for ip in 0..npoint {
            let x = xout[ip];
            let y = yout[ip];
            out_flat[ip * 2] = if x.is_nan() { BAD_VALUE } else { x };
            out_flat[ip * 2 + 1] = if y.is_nan() { BAD_VALUE } else { y };
        }

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
