//! Numeric rate-of-change computation via adaptive finite differences.
//!
//! Based on the algorithm in C `mapping.c` (lines 8859–9152 and 1255–1386).
//! The approach evaluates the mapping at multiple points across different
//! interval sizes, choosing the interval that gives the most consistent
//! (lowest range) gradient estimate.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;

use super::Mapping;
use super::helpers::is_bad;

/// Number of sub-intervals used when computing gradient consistency.
/// The C code uses `RATE_ORDER = 8`, giving 9 evaluation points per interval.
const RATE_ORDER: usize = 8;

/// Maximum number of interval sizes to try in each direction (larger/smaller).
const NN: usize = 50;

/// Compute the numeric rate of change of mapping output `ax1` with respect
/// to input `ax2`, evaluated at the position `at`.
///
/// Axis indices are 1-based. Returns `BAD_VALUE` if the derivative cannot
/// be reliably estimated.
///
/// The algorithm tests multiple interval sizes and selects the one where
/// sub-interval gradients are most consistent (smallest range between
/// max and min gradients), following the C AST library's approach.
pub fn numeric_rate(mapping: &dyn Mapping, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
    let ax2_idx = ax2 - 1;
    let ax1_idx = ax1 - 1;

    // Central value on the input axis being varied.
    let x0 = at[ax2_idx];
    if is_bad(x0) {
        return Ok(BAD_VALUE);
    }

    // Initial interval size (mapping.c Rate, commit e782c220): a small
    // fraction of |x0|, or the same small absolute step when x0 is zero.
    // Seeding with 1.0 for x0==0 pushed the first sampled interval outside
    // the domain of narrow-domain Mappings (e.g. a ChebyMap on [-1,1]),
    // making astRate return BAD at x0=0. The loop below grows the interval
    // as needed for both restricted and unrestricted Mappings.
    let mut h0 = if x0 != 0.0 {
        f64::EPSILON * 1e9 * x0.abs()
    } else {
        f64::EPSILON * 1e9
    };

    // Find an initial working interval by increasing h0 until FindGradient
    // succeeds (or we reach an absurdly large interval).
    let mut mean0 = BAD_VALUE;
    let mut range0 = 0.0;
    while is_bad(mean0) && h0 < 1e-10 * f64::MAX {
        h0 *= 10.0;
        let (m, r) = find_gradient(mapping, at, ax1_idx, ax2_idx, x0, h0)?;
        mean0 = m;
        range0 = r;
    }

    // If no working interval was found, return BAD.
    if is_bad(mean0) {
        return Ok(BAD_VALUE);
    }

    // Arrays to store gradient ranges and means for different interval sizes.
    // Index NN is our starting point (h0). Indices > NN are larger intervals,
    // indices < NN are smaller intervals.
    let mut ranges = vec![0.0f64; 2 * NN + 1];
    let mut means = vec![0.0f64; 2 * NN + 1];

    ranges[NN] = range0;
    means[NN] = mean0;
    let mut min_range = range0;
    let mut iret = NN;
    let mut itop = NN;

    // Search larger intervals (multiply by 4 each step). A BAD interval
    // (FindGradient failure) is skipped but does NOT abandon the search:
    // the loop continues to larger sizes (mapping.c:9044-9081).
    let mut h = h0;
    for _ in (NN + 1)..=(2 * NN) {
        h *= 4.0;

        let (mean, range) = find_gradient(mapping, at, ax1_idx, ax2_idx, x0, h)?;
        if is_bad(mean) {
            continue;
        }

        itop += 1;
        means[itop] = mean;
        ranges[itop] = range;

        if range < min_range {
            min_range = range;
            iret = itop;
        } else if range > min_range {
            // Range increasing — we've passed the minimum.
            break;
        } else if range == 0.0 && ranges[itop - 1] == 0.0 {
            // Zero range confirmed by the previous interval — the function
            // is linear here. Upstream c1a88658 indexes the range array by
            // the store index `itop`, not the loop counter `iin`; the two
            // diverge when an interval was skipped (FindGradient failed).
            iret = itop;
            break;
        }

        // Stop if range grows too large relative to our first estimate.
        if range > 100.0 * range0 {
            break;
        }
    }

    // Record the minimum range found in the upward search.
    range0 = min_range;

    let mut ibot = NN;

    // Search smaller intervals (divide by 4 each step). As above, a BAD
    // interval is skipped rather than abandoning the search
    // (mapping.c:9093-9113).
    h = h0;
    for _ in (0..NN).rev() {
        h /= 4.0;

        let (mean, range) = find_gradient(mapping, at, ax1_idx, ax2_idx, x0, h)?;
        if is_bad(mean) {
            continue;
        }

        ibot -= 1;
        means[ibot] = mean;
        ranges[ibot] = range;

        if range < min_range {
            min_range = range;
            iret = ibot;
        } else if range > min_range {
            break;
        } else if range == 0.0 && ranges[ibot + 1] == 0.0 {
            // Upstream c1a88658: index by the store index `ibot`, not the
            // loop counter `iin` (see the upward-search branch above).
            iret = ibot;
            break;
        }

        if range > 100.0 * range0 {
            break;
        }
    }

    // Select the best result (mapping.c:9116-9141). `ret` starts as BAD
    // and is only assigned when a trustworthy interval is found.
    let mut ret = BAD_VALUE;
    if min_range == 0.0 {
        // Zero range — only believe it if an adjacent interval also has
        // zero range. Otherwise it is probably a numerical fluke.
        if (iret > ibot && ranges[iret - 1] == 0.0) || (iret < itop && ranges[iret + 1] == 0.0) {
            ret = means[iret];
        } else {
            // Search for the smallest strictly-positive range, ignoring the
            // exact zeros. If none exists, `ret` stays BAD (as in C).
            let mut minrange = 0.0;
            for i in ibot..=itop {
                if ranges[i] > 0.0 && (minrange == 0.0 || ranges[i] < minrange) {
                    minrange = ranges[i];
                    ret = means[i];
                }
            }
        }
    } else {
        // Non-zero minimum range — return the corresponding mean gradient.
        ret = means[iret];
    }

    Ok(ret)
}

/// Compute the mean gradient over an interval `h` centered on `x0`, and
/// the range (max - min) of sub-interval gradients.
///
/// The interval is divided into `RATE_ORDER` sub-intervals (giving
/// `RATE_ORDER + 1` evaluation points). The mean gradient is computed
/// from the first to last good point. The range measures gradient
/// consistency within the interval.
///
/// Returns `(mean_gradient, range)`. If the gradient cannot be computed,
/// returns `(BAD_VALUE, 0.0)`.
fn find_gradient(
    mapping: &dyn Mapping,
    at: &[f64],
    ax1_idx: usize,
    ax2_idx: usize,
    x0: f64,
    h: f64,
) -> AstResult<(f64, f64)> {
    let npts = RATE_ORDER + 1;
    let i0 = RATE_ORDER / 2;
    let dh = h / RATE_ORDER as f64;

    // Build x values at evenly spaced points over the interval.
    let x_vals: Vec<f64> = (0..npts)
        .map(|i| x0 + (i as f64 - i0 as f64) * dh)
        .collect();

    // Evaluate the mapping at all points in a single batch call.
    let y_vals = evaluate_batch(mapping, at, ax1_idx, ax2_idx, &x_vals)?;

    // Find gradient range and mean over the interval.
    let mut gmax = BAD_VALUE;
    let mut gmin = BAD_VALUE;
    let mut ngood = 0usize;
    let mut x1 = 0.0f64;
    let mut y1 = BAD_VALUE;
    let mut x2 = 0.0f64;
    let mut y2 = BAD_VALUE;

    for i in 0..RATE_ORDER {
        if !is_bad(y_vals[i + 1]) && !is_bad(y_vals[i]) && x_vals[i + 1] != x_vals[i] {
            ngood += 1;

            let g = (y_vals[i + 1] - y_vals[i]) / (x_vals[i + 1] - x_vals[i]);

            if ngood == 1 {
                gmax = g;
                gmin = g;
            } else if g < gmin {
                gmin = g;
            } else if g > gmax {
                gmax = g;
            }

            if is_bad(y1) {
                y1 = y_vals[i];
                x1 = x_vals[i];
            }
            y2 = y_vals[i + 1];
            x2 = x_vals[i + 1];
        }
    }

    // Need at least two good sub-intervals for a reliable estimate.
    if ngood > 1 {
        let mean = (y2 - y1) / (x2 - x1);
        let range = gmax - gmin;
        Ok((mean, range))
    } else {
        Ok((BAD_VALUE, 0.0))
    }
}

/// Evaluate the mapping at multiple x values (varying `ax2_idx`) while
/// keeping other axes at the values in `at`. Returns the output values
/// for axis `ax1_idx`.
fn evaluate_batch(
    mapping: &dyn Mapping,
    at: &[f64],
    ax1_idx: usize,
    ax2_idx: usize,
    x_vals: &[f64],
) -> AstResult<Vec<f64>> {
    let nin = at.len();
    let npts = x_vals.len();

    // Build column data: each axis gets a vec of npts values.
    let columns: Vec<Vec<f64>> = (0..nin)
        .map(|ax| {
            if ax == ax2_idx {
                x_vals.to_vec()
            } else {
                vec![at[ax]; npts]
            }
        })
        .collect();

    let col_refs: Vec<&[f64]> = columns.iter().map(|c| c.as_slice()).collect();
    let pts = PointSet::from_columns(&col_refs)?;
    let out = mapping.transform(&pts, true)?;

    // Extract the ax1 output values.
    let result: Vec<f64> = (0..npts).map(|i| out.point(i)[ax1_idx]).collect();
    Ok(result)
}
