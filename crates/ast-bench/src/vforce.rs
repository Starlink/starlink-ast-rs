// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! SphMap's math through Apple's vForce, timed beside the port, as a
//! yardstick for how fast a vectorized `eraC2s` and `eraS2c` can be on this
//! machine.
//!
//! vForce takes whole arrays, is macOS-only and needs `unsafe` calls, so the
//! port does not use it; this module measures it only. The vForce versions
//! compute what `eraC2s` and `eraS2c` compute for points off the poles, which
//! is every point of the Roman inputs, and skip SphMap's bad-value and pole
//! handling, which costs the port little on those inputs.

use std::path::Path;

use starlink_ast::AnyObject;
use starlink_ast::coords::{Points, PointsMut};

use crate::load::{Loaded, load_fixture};
use crate::roman::{
    BitsCheck, FIXTURES, Inputs, N_SWEEP, compare_bits, inputs, median, time_batches,
};

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    fn vvatan2(z: *mut f64, y: *const f64, x: *const f64, n: *const i32);
    fn vvsqrt(y: *mut f64, x: *const f64, n: *const i32);
    fn vvsincos(s: *mut f64, c: *mut f64, x: *const f64, n: *const i32);
}

/// One timing: SphMap one way at `n` points, through the port and through
/// vForce, in nanoseconds per point (medians over the reps).
#[derive(Clone, Debug)]
pub struct VforceTiming {
    /// `sphmap_fwd` or `sphmap_inv`, as `ast-bench roman` names them.
    pub label: &'static str,
    /// Points per call.
    pub n: usize,
    /// The port, through `Mapping::transform_into`.
    pub port_ns: f64,
    /// The same math through vForce.
    pub vforce_ns: f64,
}

/// vForce's length argument for an array of `n` values.
fn len(n: usize) -> i32 {
    i32::try_from(n).expect("vForce takes at most i32::MAX values")
}

/// (x, y, z) to (longitude, latitude), as `eraC2s` computes it off the poles.
/// `r` is scratch of the same length.
fn forward(x: &[f64], y: &[f64], z: &[f64], lon: &mut [f64], lat: &mut [f64], r: &mut [f64]) {
    let n = len(x.len());
    for i in 0..x.len() {
        r[i] = x[i] * x[i] + y[i] * y[i];
    }
    // SAFETY: every array holds n values, and vForce allows the result to
    // overwrite its input.
    unsafe {
        vvsqrt(r.as_mut_ptr(), r.as_ptr(), &n);
        vvatan2(lon.as_mut_ptr(), y.as_ptr(), x.as_ptr(), &n);
        vvatan2(lat.as_mut_ptr(), z.as_ptr(), r.as_ptr(), &n);
    }
}

/// (longitude, latitude) to (x, y, z), as `eraS2c` computes it. `sb` and
/// `cb` are scratch of the same length.
fn inverse(lon: &[f64], lat: &[f64], x: &mut [f64], y: &mut [f64], z: &mut [f64], cb: &mut [f64]) {
    let n = len(lon.len());
    // SAFETY: every array holds n values.
    unsafe {
        vvsincos(y.as_mut_ptr(), x.as_mut_ptr(), lon.as_ptr(), &n);
        vvsincos(z.as_mut_ptr(), cb.as_mut_ptr(), lat.as_ptr(), &n);
    }
    for i in 0..lon.len() {
        x[i] *= cb[i];
        y[i] *= cb[i];
    }
}

/// SphMap one way on `axes` (one array per input axis, `n` values each)
/// through vForce, into `out` laid out one output axis after another.
fn vforce_into(fwd: bool, axes: &[Vec<f64>], n: usize, out: &mut [f64], scratch: &mut [f64]) {
    if fwd {
        let (lon, lat) = out.split_at_mut(n);
        forward(
            &axes[0][..n],
            &axes[1][..n],
            &axes[2][..n],
            lon,
            lat,
            &mut scratch[..n],
        );
    } else {
        let (x, rest) = out.split_at_mut(n);
        let (y, z) = rest.split_at_mut(n);
        inverse(&axes[0][..n], &axes[1][..n], x, y, z, &mut scratch[..n]);
    }
}

/// The Roman SphMap and its inputs for one direction.
fn setup(fwd: bool, n: usize) -> Result<(Box<AnyObject>, Vec<Vec<f64>>), String> {
    let loaded = match load_fixture(Path::new(FIXTURES), "sphmap.map")? {
        Loaded::Mapping(obj) => obj,
        Loaded::Untransformable(why) => return Err(format!("sphmap.map: {why}")),
    };
    let kind = if fwd {
        Inputs::Cartesian
    } else {
        Inputs::Spherical
    };
    Ok((loaded, inputs(kind, n)))
}

/// The port's output for `n` points of `axes`, one output axis after another.
fn port_into(
    map: &dyn starlink_ast::mappings::Mapping,
    fwd: bool,
    columns: &[f64],
    n: usize,
    out: &mut [f64],
) -> Result<(), String> {
    let fail = |e: starlink_ast::error::AstError| format!("sphmap N={n}: {e}");
    let input = Points::new(columns.chunks_exact(n)).map_err(fail)?;
    let output = PointsMut::new(out.chunks_exact_mut(n)).map_err(fail)?;
    map.transform_into(input, fwd, output).map_err(fail)
}

/// How vForce's result compares with the port's on `n` Roman points.
pub fn sphmap_check(fwd: bool, n: usize) -> Result<BitsCheck, String> {
    let (obj, axes) = setup(fwd, n)?;
    let map = obj.as_mapping().expect("loaded as a Mapping");
    let nout = if fwd { 2 } else { 3 };
    let columns: Vec<f64> = axes.iter().flat_map(|a| &a[..n]).copied().collect();
    let mut port = vec![0.0; nout * n];
    port_into(map, fwd, &columns, n, &mut port)?;
    let mut vf = vec![0.0; nout * n];
    vforce_into(fwd, &axes, n, &mut vf, &mut vec![0.0; n]);
    Ok(compare_bits(&port, &vf))
}

/// Time SphMap both ways at each `ast-bench roman` point count up to
/// `max_n`, through the port and through vForce, calling `report` with each
/// timing; returns how the two compare at the largest count.
pub fn run(
    max_n: usize,
    reps: usize,
    report: &mut dyn FnMut(&VforceTiming),
) -> Result<Vec<(&'static str, BitsCheck)>, String> {
    let sweep: Vec<usize> = N_SWEEP.iter().copied().filter(|&n| n <= max_n).collect();
    let top = *sweep.last().ok_or("no point count is within --max-n")?;
    let mut checks = Vec::new();
    for (label, fwd) in [("sphmap_fwd", true), ("sphmap_inv", false)] {
        let (obj, axes) = setup(fwd, top)?;
        let map = obj.as_mapping().expect("loaded as a Mapping");
        let nout = if fwd { 2 } else { 3 };
        let (mut port, mut vf, mut scratch) = (Vec::new(), Vec::new(), vec![0.0; top]);
        for &n in &sweep {
            let columns: Vec<f64> = axes.iter().flat_map(|a| &a[..n]).copied().collect();
            port = vec![0.0; nout * n];
            vf = vec![0.0; nout * n];
            let port_t = time_batches(reps, &mut || port_into(map, fwd, &columns, n, &mut port))?;
            let vf_t = time_batches(reps, &mut || {
                vforce_into(fwd, &axes, n, &mut vf, &mut scratch);
                Ok(())
            })?;
            report(&VforceTiming {
                label,
                n,
                port_ns: median(&port_t) / n as f64 * 1e9,
                vforce_ns: median(&vf_t) / n as f64 * 1e9,
            });
        }
        checks.push((label, compare_bits(&port, &vf)));
    }
    Ok(checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vforce_forward_is_close_to_the_port() {
        let r = sphmap_check(true, 1000).unwrap();
        assert!(r.max_ulps <= 4, "{r:?}");
    }

    #[test]
    fn vforce_inverse_is_close_to_the_port() {
        let r = sphmap_check(false, 1000).unwrap();
        assert!(r.max_ulps <= 4, "{r:?}");
    }
}
