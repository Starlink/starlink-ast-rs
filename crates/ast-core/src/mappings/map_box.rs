//! Bounding-box search for a Mapping over a constrained input region.
//!
//! Line-by-line port of `MapBox` and helpers (`SpecialBounds`,
//! `GlobalBounds`, `LocalMaximum`, `UphillSimplex`, `NewVertex`,
//! `MapFunction`, `Random`) from
//! `reference/starlink-ast/src/mapping.c`. The numerical sequence is
//! intended to match the C reference exactly: same algorithm, same
//! pseudo-random seed (1776655449), same convergence behaviour. The
//! intent is "C-first, idiomatic Rust later" — control flow mirrors C
//! including its use of `f64::MAX` / `-f64::MAX` as sentinels.

// The port is line-for-line C; index loops, not iterator chains, are
// the right shape for that, and rewriting them as iterators would
// obscure the correspondence with the C source.
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_memcpy)]

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::Mapping;

/// Result of [`Mapping::map_box`].
#[derive(Debug, Clone)]
pub struct MapBoxResult {
    /// Lowest value taken by the nominated output coordinate over the
    /// constrained input region.
    pub lbnd_out: f64,
    /// Highest value taken by the nominated output coordinate over the
    /// constrained input region.
    pub ubnd_out: f64,
    /// Input coordinates of a (not necessarily unique) point at which
    /// the output reaches `lbnd_out`. Length = effective `nin`.
    pub xl: Vec<f64>,
    /// Input coordinates of a (not necessarily unique) point at which
    /// the output reaches `ubnd_out`. Length = effective `nin`.
    pub xu: Vec<f64>,
}

// ---------------------------------------------------------------------------
// MapData equivalent
// ---------------------------------------------------------------------------

/// Description of the Mapping function being optimised.
///
/// Mirrors C's `MapData` (mapping.c:518-531). The `forward` flag, the
/// constrained input bounds, the output coordinate index and a `negate`
/// flag are all bundled here so they can be threaded through the
/// optimisation routines unchanged from the C structure.
struct MapData<'a> {
    mapping: &'a dyn Mapping,
    /// Lower constraints on the input coordinates, after the C-side swap
    /// so that `lbnd[i] <= ubnd[i]`.
    lbnd: Vec<f64>,
    ubnd: Vec<f64>,
    /// Effective number of input coordinates of the simplified Mapping
    /// for the chosen direction.
    nin: usize,
    /// Effective number of output coordinates.
    nout: usize,
    /// Index of the output coordinate to optimise (zero-based).
    coord: usize,
    /// Whether the forward (`true`) or inverse (`false`) transform is used.
    forward: bool,
    /// When true, `MapFunction` returns the negated value (C uses this
    /// to find a local minimum by maximising the negated function).
    negate: bool,
}

// ---------------------------------------------------------------------------
// Random
// ---------------------------------------------------------------------------

/// Pseudo-random number generator from C's `Random` (mapping.c:8809).
///
/// Numerical Recipes "minimal standard" generator with constants
/// 16807 / 2147483647 / 127773 / 2836. Operates on a `i64` seed
/// (matching C's `long int`). Returns a value in `[0, 1]`.
fn random(seed: &mut i64) -> f64 {
    // i = *seed / 127773;
    let i = *seed / 127773;
    // *seed = ( *seed - i * 127773 ) * 16807 - i * 2836;
    *seed = (*seed - i * 127773) * 16807 - i * 2836;
    if *seed < 0 {
        *seed += 2147483647;
    }
    // Return ( (double) ( *seed - 1 ) ) / (double) 2147483646;
    ((*seed - 1) as f64) / 2147483646.0
}

// ---------------------------------------------------------------------------
// MapFunction
// ---------------------------------------------------------------------------

/// Evaluate the Mapping at a single input point and return the
/// requested output coordinate (negated when `mapdata.negate`).
///
/// Mirrors C `MapFunction` (mapping.c:7385-7507).
/// Returns `BAD_VALUE` if the point is outside the input bounds, or if
/// any output coordinate is bad.
fn map_function(mapdata: &MapData, input: &[f64], ncall: &mut i32) -> AstResult<f64> {
    // See if the input point lies outside the required bounds.
    let mut outside = false;
    for coord_in in 0..mapdata.nin {
        if input[coord_in] < mapdata.lbnd[coord_in] || input[coord_in] > mapdata.ubnd[coord_in] {
            outside = true;
            break;
        }
    }
    if outside {
        return Ok(BAD_VALUE);
    }

    // Build a one-point PointSet and transform.
    let pts_in = PointSet::from_rows([input.to_vec()]);
    let pts_out = mapdata.mapping.transform(&pts_in, mapdata.forward)?;
    *ncall += 1;

    // Test if any output coordinate is bad.
    let row = pts_out.point(0);
    for &v in row.iter().take(mapdata.nout) {
        if v == BAD_VALUE {
            return Ok(BAD_VALUE);
        }
    }

    let mut result = row[mapdata.coord];
    if mapdata.negate {
        result = -result;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// NewVertex
// ---------------------------------------------------------------------------

/// Generate a new simplex vertex by reflecting/scaling the worst
/// vertex through the centroid of the opposite face. Replaces the
/// worst vertex when the new point improves on it.
///
/// Mirrors C `NewVertex` (mapping.c:8247-8381).
#[allow(clippy::too_many_arguments)]
fn new_vertex(
    mapdata: &MapData,
    lo: usize,
    scale: f64,
    x: &mut [f64],
    f: &mut [f64],
    ncall: &mut i32,
    xnew: &mut [f64],
) -> AstResult<f64> {
    let ncoord = mapdata.nin;
    let nvertex = ncoord + 1;

    // Loop to obtain each coordinate of the new vertex.
    for coord in 0..ncoord {
        // Average the coordinates of all vertices except `lo`.
        let mut xface = 0.0_f64;
        for vertex in 0..nvertex {
            if vertex != lo {
                xface += x[vertex * ncoord + coord] / ((nvertex - 1) as f64);
            }
        }
        // Magnify the lowest vertex's distance from this point.
        xnew[coord] = xface + (x[lo * ncoord + coord] - xface) * scale;
    }

    // Evaluate the Mapping function at the new vertex.
    let fnew = map_function(mapdata, xnew, ncall)?;

    // If the result is not bad and exceeds the previous value at the
    // lowest vertex, then replace the lowest vertex with this new one.
    if fnew != BAD_VALUE && fnew > f[lo] {
        for coord in 0..ncoord {
            x[lo * ncoord + coord] = xnew[coord];
        }
        f[lo] = fnew;
    }
    Ok(fnew)
}

// ---------------------------------------------------------------------------
// UphillSimplex
// ---------------------------------------------------------------------------

/// Simplex-method local maximiser.
///
/// Mirrors C `UphillSimplex` (mapping.c:22758-23066). Uses the
/// modification described in the C source for handling forbidden
/// regions (lateral contraction toward the best vertex).
#[allow(clippy::too_many_arguments)]
fn uphill_simplex(
    mapdata: &MapData,
    acc: f64,
    maxcall: i32,
    dx: &[f64],
    xmax: &mut [f64],
    err: &mut f64,
    ncall: &mut i32,
) -> AstResult<f64> {
    const FACTOR: f64 = 3.0;

    #[allow(unused_assignments)]
    let mut result = BAD_VALUE;
    *err = f64::MAX;
    *ncall = 0;

    let ncoord = mapdata.nin;
    let nvertex = ncoord + 1;

    let mut f: Vec<f64> = vec![0.0; nvertex];
    let mut x: Vec<f64> = vec![0.0; ncoord * nvertex];
    let mut xnew: Vec<f64> = vec![0.0; ncoord];

    // Set up an initial simplex.
    for vertex in 0..nvertex {
        for coord in 0..ncoord {
            let mut tmp = xmax[coord];
            // Displace all but the first vertex along one axis.
            // C: if ( coord == ( vertex - 1 ) ) tmp += dx[ coord ];
            // For vertex 0 this never triggers (no underflow because we
            // mirror C's int comparison with a guarded subtraction).
            if vertex >= 1 && coord == vertex - 1 {
                tmp += dx[coord];
            }
            x[vertex * ncoord + coord] = tmp;
        }
        f[vertex] = map_function(mapdata, &x[vertex * ncoord..(vertex + 1) * ncoord], ncall)?;
        if f[vertex] == BAD_VALUE {
            f[vertex] = -f64::MAX;
        }
    }

    let mut ncalla: i32 = nvertex as i32;

    // Loop until convergence is reached.
    loop {
        // Initialise lo / nextlo / hi.
        let mut lo: usize = if f[0] < f[1] { 0 } else { 1 };
        let mut nextlo: usize = 1 - lo;
        let mut hi: usize = 0;

        for vertex in 0..nvertex {
            if f[vertex] <= f[lo] {
                nextlo = lo;
                lo = vertex;
            } else if f[vertex] <= f[nextlo] && vertex != lo {
                nextlo = vertex;
            }
            if f[vertex] > f[hi] {
                hi = vertex;
            }
        }

        // Estimate the simplex error range.
        let range = if f[hi] == -f64::MAX || f[lo] == -f64::MAX {
            f64::MAX
        } else {
            f[hi] - f[lo]
        };

        // Convergence test.
        if range <= acc.abs() || *ncall >= maxcall || ncalla >= 3 * maxcall {
            for coord in 0..ncoord {
                xmax[coord] = x[hi * ncoord + coord];
            }
            result = if f[hi] == -f64::MAX { BAD_VALUE } else { f[hi] };
            *err = range;
            break;
        }

        // Try reflecting the worst vertex through the opposite face.
        let mut fnew = new_vertex(mapdata, lo, -1.0, &mut x, &mut f, ncall, &mut xnew)?;
        ncalla += 1;

        if fnew == BAD_VALUE {
            // Lateral-contraction fallback: contract worst toward best.
            for coord in 0..ncoord {
                let offset = x[lo * ncoord + coord] - x[hi * ncoord + coord];
                x[lo * ncoord + coord] = x[hi * ncoord + coord] + offset / FACTOR;
                if (x[lo * ncoord + coord] - x[hi * ncoord + coord]).abs() >= offset.abs() {
                    x[lo * ncoord + coord] = x[hi * ncoord + coord];
                }
            }
            f[lo] = map_function(mapdata, &x[lo * ncoord..(lo + 1) * ncoord], ncall)?;
            if f[lo] == BAD_VALUE {
                f[lo] = -f64::MAX;
            }
            ncalla += 1;
        } else if fnew >= f[hi] {
            // New maximum: try expanding.
            let _ = new_vertex(mapdata, lo, FACTOR, &mut x, &mut f, ncall, &mut xnew)?;
            ncalla += 1;
        } else if fnew <= f[nextlo] {
            // No improvement on second worst — contract worst toward
            // the opposite face.
            let fsave = f[lo];
            fnew = new_vertex(mapdata, lo, 1.0 / FACTOR, &mut x, &mut f, ncall, &mut xnew)?;
            ncalla += 1;

            if fnew <= fsave {
                // Contract entire simplex toward best vertex.
                for vertex in 0..nvertex {
                    if vertex != hi {
                        for coord in 0..ncoord {
                            let offset = x[vertex * ncoord + coord] - x[hi * ncoord + coord];
                            x[vertex * ncoord + coord] = x[hi * ncoord + coord] + offset / FACTOR;
                            if (x[vertex * ncoord + coord] - x[hi * ncoord + coord]).abs()
                                >= offset.abs()
                            {
                                x[vertex * ncoord + coord] = x[hi * ncoord + coord];
                            }
                        }
                        f[vertex] = map_function(
                            mapdata,
                            &x[vertex * ncoord..(vertex + 1) * ncoord],
                            ncall,
                        )?;
                        if f[vertex] == BAD_VALUE {
                            f[vertex] = -f64::MAX;
                        }
                        ncalla += 1;
                    }
                }
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// LocalMaximum
// ---------------------------------------------------------------------------

/// Find a local maximum of the (possibly negated) Mapping function via
/// repeated simplex restarts.
///
/// Mirrors C `LocalMaximum` (mapping.c:6982-7119).
fn local_maximum(mapdata: &MapData, acc: f64, fract_in: f64, x: &mut [f64]) -> AstResult<f64> {
    const MAXCALL: i32 = 1500;
    const MAXITER: i32 = 5;

    let mut result = BAD_VALUE;
    let mut fract = fract_in;
    let mut err = 0.0_f64;
    let mut dx: Vec<f64> = vec![0.0; mapdata.nin];

    for _iter in 0..MAXITER {
        // Set up step lengths.
        for coord in 0..mapdata.nin {
            let middle = 0.5 * (mapdata.lbnd[coord] + mapdata.ubnd[coord]);
            dx[coord] = fract * (mapdata.ubnd[coord] - mapdata.lbnd[coord]);
            if x[coord] > middle {
                dx[coord] = -dx[coord];
            }
        }

        let mut ncall: i32 = 0;
        let maximum = uphill_simplex(mapdata, acc, MAXCALL, &dx, x, &mut err, &mut ncall)?;

        if result == BAD_VALUE {
            result = maximum;
        } else if maximum >= result {
            // Converged when the improvement is no greater than acc and
            // the simplex error is also no greater than acc.
            let done = (maximum - result) <= acc && err <= acc;
            result = maximum;
            if done {
                break;
            }
        }
        // Whether the maximum improved or not, decrement the step.
        // Mirrors C: the `fract /= 1000.0` lives outside the inner
        // `else if maximum >= result` test so it always runs.
        fract /= 1000.0;
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// SpecialBounds
// ---------------------------------------------------------------------------

/// Initial estimate of the output bounds using a fixed set of "special
/// points": every corner, the centre of every face, the centroid, the
/// origin (if in bounds), plus a point slightly inside each corner.
///
/// Mirrors C `SpecialBounds` (mapping.c:17028-17343). Returns `true`
/// when the caller should continue with `GlobalBounds`, `false` when
/// no further refinement is needed (no valid points found at all).
fn special_bounds(
    mapdata: &MapData,
    lbnd: &mut f64,
    ubnd: &mut f64,
    xl: &mut [f64],
    xu: &mut [f64],
) -> AstResult<bool> {
    let ncoord = mapdata.nin;

    // Number of corners (2^ncoord).
    let mut npoint: usize = 1;
    for _ in 0..ncoord {
        npoint *= 2;
    }
    // Plus a second point near each corner.
    let ncorner = npoint;
    npoint *= 2;
    // Plus face centres and centroid.
    npoint += 2 * ncoord + 1;

    // Determine if the origin is in bounds.
    let mut origin = true;
    for coord in 0..ncoord {
        if mapdata.lbnd[coord] > 0.0 || mapdata.ubnd[coord] < 0.0 {
            origin = false;
            break;
        }
    }
    if origin {
        npoint += 1;
    }

    // Secondary bounds (used as fallback when no primary points
    // produce fully-non-bad output rows).
    let mut slbnd = *lbnd;
    let mut subnd = *ubnd;
    let mut sxl: Vec<f64> = vec![0.0; ncoord];
    let mut sxu: Vec<f64> = vec![0.0; ncoord];

    // Build the input PointSet (npoint x ncoord, row-major).
    let mut pts_in = PointSet::zeros(npoint, ncoord);

    // Populate corners with a binary counter (`limit`).
    let mut limit: Vec<i32> = vec![0; ncoord];
    let mut point: usize = 0;
    let mut done = false;
    while !done {
        // Set this corner's coordinates.
        {
            let row = pts_in.point_mut(point);
            for coord in 0..ncoord {
                row[coord] = if limit[coord] != 0 {
                    mapdata.ubnd[coord]
                } else {
                    mapdata.lbnd[coord]
                };
            }
        }
        point += 1;

        // Advance the limit array.
        let mut coord = 0;
        loop {
            if limit[coord] == 0 {
                limit[coord] = 1;
                break;
            } else {
                limit[coord] = 0;
                coord += 1;
                if coord == ncoord {
                    done = true;
                    break;
                }
            }
        }
    }

    // Centre of each face: 2 * ncoord points.
    for face in 0..(2 * ncoord) {
        {
            let row = pts_in.point_mut(point);
            for coord in 0..ncoord {
                row[coord] = 0.5 * (mapdata.lbnd[coord] + mapdata.ubnd[coord]);
            }
            row[face / 2] = if face % 2 == 1 {
                mapdata.lbnd[face / 2]
            } else {
                mapdata.ubnd[face / 2]
            };
        }
        point += 1;
    }

    // Centroid.
    let icen = point;
    {
        let row = pts_in.point_mut(point);
        for coord in 0..ncoord {
            row[coord] = 0.5 * (mapdata.lbnd[coord] + mapdata.ubnd[coord]);
        }
    }
    point += 1;

    // Offset-from-corner points: 0.999 * corner + 0.001 * centroid.
    // Need the centroid row stored above; copy from pts_in by index.
    // We read each value individually via `pts_in.point(ic)` to avoid
    // simultaneous mutable+immutable borrows.
    for ic in 0..ncorner {
        let mut tmp: Vec<f64> = vec![0.0; ncoord];
        let corner = pts_in.point(ic);
        let centroid = pts_in.point(icen);
        for coord in 0..ncoord {
            tmp[coord] = 0.999 * corner[coord] + 0.001 * centroid[coord];
        }
        let row = pts_in.point_mut(point);
        row[..ncoord].copy_from_slice(&tmp[..ncoord]);
        point += 1;
    }

    // Origin, if in bounds.
    if origin {
        let row = pts_in.point_mut(point);
        for coord in 0..ncoord {
            row[coord] = 0.0;
        }
    }

    // Transform the entire batch.
    let pts_out = mapdata.mapping.transform(&pts_in, mapdata.forward)?;

    let mut result = true;

    // Loop through each point.
    for point in 0..npoint {
        let out_row = pts_out.point(point);
        let mut bad = false;
        for &v in out_row.iter().take(mapdata.nout) {
            if v == BAD_VALUE {
                bad = true;
                break;
            }
        }
        let f = out_row[mapdata.coord];
        if !bad {
            if *lbnd == BAD_VALUE || f < *lbnd {
                *lbnd = f;
                let in_row = pts_in.point(point);
                xl[..ncoord].copy_from_slice(&in_row[..ncoord]);
            }
            if *ubnd == BAD_VALUE || f > *ubnd {
                *ubnd = f;
                let in_row = pts_in.point(point);
                xu[..ncoord].copy_from_slice(&in_row[..ncoord]);
            }
        } else if f != BAD_VALUE {
            if slbnd == BAD_VALUE || f < slbnd {
                slbnd = f;
                let in_row = pts_in.point(point);
                sxl[..ncoord].copy_from_slice(&in_row[..ncoord]);
            }
            if subnd == BAD_VALUE || f > subnd {
                subnd = f;
                let in_row = pts_in.point(point);
                sxu[..ncoord].copy_from_slice(&in_row[..ncoord]);
            }
        }
    }

    // Fall back to secondary bounds if no primary values were found.
    if *lbnd == BAD_VALUE && *ubnd == BAD_VALUE {
        *lbnd = slbnd;
        *ubnd = subnd;
        for coord in 0..ncoord {
            xu[coord] = sxu[coord];
            xl[coord] = sxl[coord];
        }
        result = !(slbnd == BAD_VALUE || subnd == BAD_VALUE);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// GlobalBounds
// ---------------------------------------------------------------------------

/// Refine an initial bound estimate with pseudo-random sampling +
/// simplex local search.
///
/// Mirrors C `GlobalBounds` (mapping.c:1872-2469). The macro
/// `FILL_POSITION_BUFFER` is reproduced here as a closure-style helper
/// that operates on the local mutable state.
#[allow(clippy::too_many_arguments)]
fn global_bounds(
    mapdata: &mut MapData,
    lbnd: &mut f64,
    ubnd: &mut f64,
    xl: &mut [f64],
    xu: &mut [f64],
) -> AstResult<()> {
    const DEFAULT_ACC: f64 = 3.0e-5;
    const MAXITER: i32 = 10000;
    const MINSAME: i32 = 5;
    const NBATCH: usize = 32;

    let ncoord = mapdata.nin;

    // Per-axis bookkeeping.
    let mut active_hi: Vec<f64> = vec![-f64::MAX; ncoord];
    let mut active_lo: Vec<f64> = vec![f64::MAX; ncoord];
    let mut sample_hi: Vec<f64> = vec![0.0; ncoord];
    let mut sample_lo: Vec<f64> = vec![0.0; ncoord];
    let mut sample_width: Vec<f64> = vec![0.0; ncoord];
    let mut x: Vec<f64> = vec![0.0; ncoord];

    // Volume oversize factor: volume ratio of 2 -> per-axis ratio of
    // 2^(1/ncoord).
    let oversize = 2.0_f64.powf(1.0 / ncoord as f64);

    for coord in 0..ncoord {
        sample_width[coord] = (mapdata.ubnd[coord] - mapdata.lbnd[coord]) * oversize;
        sample_lo[coord] = mapdata.lbnd[coord];
        sample_hi[coord] = mapdata.ubnd[coord];
    }

    let mut pts_in = PointSet::zeros(NBATCH, ncoord);
    let mut pts_out = PointSet::zeros(NBATCH, mapdata.nout);

    // batch = NBATCH means buffer is empty; trigger refill.
    #[allow(unused_assignments)]
    let mut batch: usize = NBATCH;
    let mut seed: i64 = 1776655449;

    // ---- Macro FILL_POSITION_BUFFER (mapping.c:2052-2125) ----
    // Closure form so we can call it from multiple places below.
    // We need exclusive borrows on a lot of the state, which is why
    // this is inlined where the C macro expands.
    macro_rules! fill_position_buffer {
        () => {{
            for coord in 0..ncoord {
                if active_hi[coord] >= active_lo[coord] {
                    let active_width = active_hi[coord] - active_lo[coord];

                    if active_width * oversize < sample_width[coord] {
                        sample_width[coord] /= oversize.powf(0.25);
                    } else if active_width * oversize > sample_width[coord] {
                        sample_width[coord] = active_width * oversize;
                    }

                    sample_lo[coord] =
                        (active_lo[coord] + active_hi[coord] - sample_width[coord]) * 0.5;
                    sample_hi[coord] =
                        (active_lo[coord] + active_hi[coord] + sample_width[coord]) * 0.5;

                    if sample_lo[coord] < mapdata.lbnd[coord] {
                        sample_lo[coord] = mapdata.lbnd[coord];
                    }
                    if sample_hi[coord] > mapdata.ubnd[coord] {
                        sample_hi[coord] = mapdata.ubnd[coord];
                    }
                }
            }

            for b in 0..NBATCH {
                let row = pts_in.point_mut(b);
                for coord in 0..ncoord {
                    let r = random(&mut seed);
                    row[coord] = sample_lo[coord] * r + sample_hi[coord] * (1.0 - r);
                }
            }

            mapdata
                .mapping
                .transform_into(&pts_in, mapdata.forward, &mut pts_out)?;
            batch = 0;
        }};
    }

    // Initial fill.
    fill_position_buffer!();

    let mut done_max = false;
    let mut done_min = false;
    let mut nmax: i32 = 0;
    let mut nmin: i32 = 0;
    let mut nsame_max: i32 = 0;
    let mut nsame_min: i32 = 0;

    let mut iter = 0;
    while iter < MAXITER {
        // Pick the search accuracy.
        let mut acc = DEFAULT_ACC;
        if *lbnd != BAD_VALUE && *ubnd != BAD_VALUE {
            acc = (*ubnd - *lbnd).abs() * DEFAULT_ACC;
        } else if *lbnd != BAD_VALUE {
            if lbnd.abs() > 1.0 {
                acc = lbnd.abs() * DEFAULT_ACC;
            }
        } else if *ubnd != BAD_VALUE && ubnd.abs() > 1.0 {
            acc = ubnd.abs() * DEFAULT_ACC;
        }

        // ---- Search for a new local minimum ----
        let mut bad = false;
        if !done_min {
            if iter == 0 && *lbnd != BAD_VALUE {
                x[..ncoord].copy_from_slice(&xl[..ncoord]);
            } else if *lbnd == BAD_VALUE && *ubnd != BAD_VALUE {
                x[..ncoord].copy_from_slice(&xu[..ncoord]);
            } else {
                if batch >= NBATCH {
                    fill_position_buffer!();
                }
                let out_row = pts_out.point(batch);
                bad = false;
                for &v in out_row.iter().take(mapdata.nout) {
                    if v == BAD_VALUE {
                        bad = true;
                        break;
                    }
                }
                if !bad {
                    let in_row = pts_in.point(batch);
                    x[..ncoord].copy_from_slice(&in_row[..ncoord]);
                }
                batch += 1;
            }

            if !bad {
                for coord in 0..ncoord {
                    if x[coord] < active_lo[coord] {
                        active_lo[coord] = x[coord];
                    }
                    if x[coord] > active_hi[coord] {
                        active_hi[coord] = x[coord];
                    }
                }

                mapdata.negate = true;
                let mut new_min = local_maximum(mapdata, acc, 0.01, &mut x)?;
                if new_min != BAD_VALUE {
                    new_min = -new_min;

                    for coord in 0..ncoord {
                        if x[coord] < active_lo[coord] {
                            active_lo[coord] = x[coord];
                        }
                        if x[coord] > active_hi[coord] {
                            active_hi[coord] = x[coord];
                        }
                    }

                    nmin += 1;

                    if *lbnd == BAD_VALUE {
                        nsame_min = 1;
                        *lbnd = new_min;
                        xl[..ncoord].copy_from_slice(&x[..ncoord]);
                    } else if new_min < *lbnd {
                        nsame_min = if (*lbnd - new_min) > acc {
                            1
                        } else {
                            nsame_min + 1
                        };
                        *lbnd = new_min;
                        xl[..ncoord].copy_from_slice(&x[..ncoord]);
                    } else {
                        nsame_min += 1;
                    }

                    if nsame_min >= MINSAME && nsame_min >= (0.3_f32 * nmin as f32).round() as i32 {
                        done_min = true;
                    }
                }
            }
        }

        // ---- Search for a new local maximum ----
        let mut bad = false;
        if !done_max {
            if iter == 0 && *ubnd != BAD_VALUE {
                x[..ncoord].copy_from_slice(&xu[..ncoord]);
            } else if *ubnd == BAD_VALUE && *lbnd != BAD_VALUE {
                x[..ncoord].copy_from_slice(&xl[..ncoord]);
            } else {
                if batch >= NBATCH {
                    fill_position_buffer!();
                }
                let out_row = pts_out.point(batch);
                bad = false;
                for &v in out_row.iter().take(mapdata.nout) {
                    if v == BAD_VALUE {
                        bad = true;
                        break;
                    }
                }
                if !bad {
                    let in_row = pts_in.point(batch);
                    x[..ncoord].copy_from_slice(&in_row[..ncoord]);
                }
                batch += 1;
            }

            if !bad {
                for coord in 0..ncoord {
                    if x[coord] < active_lo[coord] {
                        active_lo[coord] = x[coord];
                    }
                    if x[coord] > active_hi[coord] {
                        active_hi[coord] = x[coord];
                    }
                }

                mapdata.negate = false;
                let new_max = local_maximum(mapdata, acc, 0.01, &mut x)?;
                if new_max != BAD_VALUE {
                    for coord in 0..ncoord {
                        if x[coord] < active_lo[coord] {
                            active_lo[coord] = x[coord];
                        }
                        if x[coord] > active_hi[coord] {
                            active_hi[coord] = x[coord];
                        }
                    }

                    nmax += 1;

                    if *ubnd == BAD_VALUE {
                        nsame_max = 1;
                        *ubnd = new_max;
                        xu[..ncoord].copy_from_slice(&x[..ncoord]);
                    } else if new_max > *ubnd {
                        nsame_max = if (new_max - *ubnd) > acc {
                            1
                        } else {
                            nsame_max + 1
                        };
                        *ubnd = new_max;
                        xu[..ncoord].copy_from_slice(&x[..ncoord]);
                    } else {
                        nsame_max += 1;
                    }

                    if nsame_max >= MINSAME && nsame_max >= (0.3_f32 * nmax as f32).round() as i32 {
                        done_max = true;
                    }
                }
            }
        }

        if done_min && done_max {
            break;
        }
        iter += 1;
    }

    // Polish the global minimum and maximum at machine precision.
    if *lbnd != BAD_VALUE {
        mapdata.negate = true;
        *lbnd = local_maximum(mapdata, 0.0, f64::EPSILON.sqrt(), xl)?;
        if *lbnd != BAD_VALUE {
            *lbnd = -*lbnd;
        }
    }
    if *ubnd != BAD_VALUE {
        mapdata.negate = false;
        *ubnd = local_maximum(mapdata, 0.0, f64::EPSILON.sqrt(), xu)?;
    }

    if *lbnd == BAD_VALUE || *ubnd == BAD_VALUE {
        return Err(AstError::new(
            ErrorCode::Mbbnf,
            format!(
                "astMapBox: No valid output coordinates (after {} test points).",
                2 * MAXITER
            ),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// MapBox (public entry, called via Mapping::map_box default impl)
// ---------------------------------------------------------------------------

/// Default implementation of `Mapping::map_box`.
///
/// Mirrors C `MapBox` (mapping.c:7121-7383). The function simplifies
/// the Mapping, runs `SpecialBounds` (when the input dimensionality is
/// at most 12), then refines via `GlobalBounds`.
pub(crate) fn map_box_default<M: Mapping + ?Sized>(
    mapping: &M,
    lbnd_in: &[f64],
    ubnd_in: &[f64],
    forward: bool,
    coord_out: usize,
) -> AstResult<MapBoxResult> {
    map_box_dyn_inner(mapping, lbnd_in, ubnd_in, forward, coord_out)
}

fn map_box_dyn_inner<M: Mapping + ?Sized>(
    mapping: &M,
    lbnd_in: &[f64],
    ubnd_in: &[f64],
    forward: bool,
    coord_out: usize,
) -> AstResult<MapBoxResult> {
    // Effective input/output counts for the requested direction.
    let nin = if forward {
        mapping.nin()
    } else {
        mapping.nout()
    };
    let nout = if forward {
        mapping.nout()
    } else {
        mapping.nin()
    };

    if coord_out >= nout {
        return Err(AstError::new(
            ErrorCode::Badci,
            format!(
                "astMapBox: Output coordinate index ({}) invalid - it should be in the range 1 to {}.",
                coord_out.wrapping_add(1),
                nout
            ),
        ));
    }
    if lbnd_in.len() < nin || ubnd_in.len() < nin {
        return Err(AstError::new(
            ErrorCode::Badci,
            format!(
                "astMapBox: lbnd_in/ubnd_in length must be at least {} (got {} and {}).",
                nin,
                lbnd_in.len(),
                ubnd_in.len()
            ),
        ));
    }

    // Simplify the Mapping (mapping is queried many times below).
    let simplified: crate::mappings::DynMapping = mapping.as_dyn_mapping().simplify()?;

    // Build MapData. Swap lbnd/ubnd into ascending order if the caller
    // passed them the other way around.
    let mut lbnd_v: Vec<f64> = vec![0.0; nin];
    let mut ubnd_v: Vec<f64> = vec![0.0; nin];
    let mut x_l: Vec<f64> = vec![BAD_VALUE; nin];
    let mut x_u: Vec<f64> = vec![BAD_VALUE; nin];
    for coord in 0..nin {
        lbnd_v[coord] = if lbnd_in[coord] < ubnd_in[coord] {
            lbnd_in[coord]
        } else {
            ubnd_in[coord]
        };
        ubnd_v[coord] = if ubnd_in[coord] > lbnd_in[coord] {
            ubnd_in[coord]
        } else {
            lbnd_in[coord]
        };
    }

    let mut mapdata = MapData {
        mapping: simplified.as_ref(),
        lbnd: lbnd_v,
        ubnd: ubnd_v,
        nin,
        nout,
        coord: coord_out,
        forward,
        negate: false,
    };

    let mut lbnd: f64 = BAD_VALUE;
    let mut ubnd: f64 = BAD_VALUE;

    // SpecialBounds is only run when nin <= 12 (matches C:7334).
    let refine = if nin <= 12 {
        special_bounds(&mapdata, &mut lbnd, &mut ubnd, &mut x_l, &mut x_u)?
    } else {
        true
    };

    if refine {
        global_bounds(&mut mapdata, &mut lbnd, &mut ubnd, &mut x_l, &mut x_u)?;
    }

    Ok(MapBoxResult {
        lbnd_out: lbnd,
        ubnd_out: ubnd,
        xl: x_l,
        xu: x_u,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{ShiftMap, UnitMap, WinMap, ZoomMap};

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol.max(1e-12 * (a.abs() + b.abs()).max(1.0))
    }

    #[test]
    fn unit_map_box() {
        let m = UnitMap::new(2);
        let r = map_box_default(&m, &[1.0, 2.0], &[10.0, 20.0], true, 0).unwrap();
        assert!(approx(r.lbnd_out, 1.0, 1e-9));
        assert!(approx(r.ubnd_out, 10.0, 1e-9));
        let r = map_box_default(&m, &[1.0, 2.0], &[10.0, 20.0], true, 1).unwrap();
        assert!(approx(r.lbnd_out, 2.0, 1e-9));
        assert!(approx(r.ubnd_out, 20.0, 1e-9));
    }

    #[test]
    fn zoom_map_box() {
        let m = ZoomMap::new(2, 3.0).unwrap();
        let r = map_box_default(&m, &[1.0, -2.0], &[4.0, 5.0], true, 0).unwrap();
        assert!(approx(r.lbnd_out, 3.0, 1e-9));
        assert!(approx(r.ubnd_out, 12.0, 1e-9));
        let r = map_box_default(&m, &[1.0, -2.0], &[4.0, 5.0], true, 1).unwrap();
        assert!(approx(r.lbnd_out, -6.0, 1e-9));
        assert!(approx(r.ubnd_out, 15.0, 1e-9));
    }

    #[test]
    fn shift_map_box() {
        let m = ShiftMap::new(vec![10.0, -5.0]);
        let r = map_box_default(&m, &[0.0, 0.0], &[1.0, 2.0], true, 0).unwrap();
        assert!(approx(r.lbnd_out, 10.0, 1e-9));
        assert!(approx(r.ubnd_out, 11.0, 1e-9));
        let r = map_box_default(&m, &[0.0, 0.0], &[1.0, 2.0], true, 1).unwrap();
        assert!(approx(r.lbnd_out, -5.0, 1e-9));
        assert!(approx(r.ubnd_out, -3.0, 1e-9));
    }

    #[test]
    fn win_map_box() {
        // Map input box [0,0]..[10,10] to output box [100,200]..[110,300].
        let m = WinMap::from_corners(&[0.0, 0.0], &[10.0, 10.0], &[100.0, 200.0], &[110.0, 300.0])
            .unwrap();
        let r = map_box_default(&m, &[0.0, 0.0], &[10.0, 10.0], true, 0).unwrap();
        assert!(approx(r.lbnd_out, 100.0, 1e-9));
        assert!(approx(r.ubnd_out, 110.0, 1e-9));
        let r = map_box_default(&m, &[0.0, 0.0], &[10.0, 10.0], true, 1).unwrap();
        assert!(approx(r.lbnd_out, 200.0, 1e-9));
        assert!(approx(r.ubnd_out, 300.0, 1e-9));
    }

    #[test]
    fn swapped_bounds() {
        // Caller passes upper bound first; map_box should swap to canonical.
        let m = ZoomMap::new(1, 2.0).unwrap();
        let r = map_box_default(&m, &[5.0], &[1.0], true, 0).unwrap();
        assert!(approx(r.lbnd_out, 2.0, 1e-9));
        assert!(approx(r.ubnd_out, 10.0, 1e-9));
    }
}
