//! The Roman benchmark: `perf/bench_simd.c` from Starlink/ast pull request 51
//! (<https://github.com/Starlink/ast/pull/51>), repeated on the Rust engine
//! and, with the `c-oracle` feature, the C one.
//!
//! The pull request times six transforms its author found dominate the Roman
//! Space Telescope's WCS: SphMap both ways, a degree-5 and a degree-1 PolyMap
//! and a diagonal and a full 2x2 MatrixMap. Everything here follows it: the
//! Mappings (written by `tools/perf/gen_roman_fixtures.c` into
//! `fixtures/roman`), the inputs (`drand48` from seed 42, reproduced bit for
//! bit), the point counts (1 to 4088x4088, the full Roman WFI science area),
//! and the timing: each point count runs a batch of calls that doubles until
//! it takes at least 20 ms, then times that batch `reps` times and keeps the
//! median time per call. Inputs stay hot across a batch, so small and medium
//! N measure compute throughput and large N memory bandwidth.

use std::path::Path;
use std::time::{Duration, Instant};

use starlink_ast::coords::{Points, PointsMut};

use crate::load::{Loaded, load_fixture};

/// The fixtures `tools/perf/gen_roman_fixtures.c` writes.
pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/roman");

/// The point counts: 1 through 4088x4088.
pub const N_SWEEP: [usize; 17] = [
    1, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304,
    16711744,
];

/// The shortest a timed batch may be.
pub const MIN_BATCH: Duration = Duration::from_millis(20);

/// The most calls a batch may hold, so the smallest N do not run unbounded.
pub const MAX_BATCH_CALLS: usize = 1 << 22;

/// Timed batches per point count, of which the median is kept.
pub const DEFAULT_REPS: usize = 5;

const RAND_SEED: i64 = 42;

/// Pixel coordinates run over [-IMAGE_HALF, IMAGE_HALF).
const IMAGE_HALF: f64 = 2048.0;

/// POSIX `drand48`: a 48-bit linear congruential generator, so the inputs
/// are the pull request's, bit for bit.
#[derive(Clone, Debug)]
pub struct Drand48 {
    state: u64,
}

impl Drand48 {
    const MASK: u64 = (1 << 48) - 1;

    /// `srand48(seed)`: the state's high 32 bits are the seed's low 32 bits,
    /// the low 16 bits are 0x330E.
    pub fn new(seed: i64) -> Self {
        Drand48 {
            state: (((seed as u64) & 0xffff_ffff) << 16) | 0x330e,
        }
    }

    /// `drand48()`: the next state over 2^48, which a double holds exactly.
    pub fn next_f64(&mut self) -> f64 {
        self.state = 0x5_deec_e66d_u64.wrapping_mul(self.state).wrapping_add(0xb) & Self::MASK;
        self.state as f64 / (1u64 << 48) as f64
    }
}

/// Which inputs a transform reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inputs {
    /// Random points on the unit sphere as (x, y, z).
    Cartesian,
    /// The same points as (longitude, latitude) in radians.
    Spherical,
    /// Random pixel coordinates (x, y) about the image center.
    Pixel,
}

/// One of the pull request's transforms.
#[derive(Clone, Copy, Debug)]
pub struct Bench {
    /// Its label in the pull request's tables.
    pub label: &'static str,
    /// Its fixture under [`FIXTURES`].
    pub fixture: &'static str,
    /// Whether it times the forward transform.
    pub forward: bool,
    /// The inputs it transforms.
    pub inputs: Inputs,
}

/// The six transforms, in the pull request's order.
pub const BENCHES: [Bench; 6] = [
    Bench {
        label: "sphmap_fwd",
        fixture: "sphmap.map",
        forward: true,
        inputs: Inputs::Cartesian,
    },
    Bench {
        label: "sphmap_inv",
        fixture: "sphmap.map",
        forward: false,
        inputs: Inputs::Spherical,
    },
    Bench {
        label: "poly5_fwd",
        fixture: "poly5.map",
        forward: true,
        inputs: Inputs::Pixel,
    },
    Bench {
        label: "poly1_fwd",
        fixture: "poly1.map",
        forward: true,
        inputs: Inputs::Pixel,
    },
    Bench {
        label: "matdiag2_fwd",
        fixture: "matdiag2.map",
        forward: true,
        inputs: Inputs::Pixel,
    },
    Bench {
        label: "matfull2_fwd",
        fixture: "matfull2.map",
        forward: true,
        inputs: Inputs::Pixel,
    },
];

/// The pull request's input arrays for `n` points, one per axis.
pub fn inputs(kind: Inputs, n: usize) -> Vec<Vec<f64>> {
    let mut rng = Drand48::new(RAND_SEED);
    match kind {
        Inputs::Cartesian | Inputs::Spherical => {
            let naxes = if kind == Inputs::Cartesian { 3 } else { 2 };
            let mut axes: Vec<Vec<f64>> = (0..naxes).map(|_| Vec::with_capacity(n)).collect();
            for _ in 0..n {
                let lo = rng.next_f64() * 2.0 * std::f64::consts::PI;
                let la = (rng.next_f64() - 0.5) * std::f64::consts::PI;
                if kind == Inputs::Cartesian {
                    let cp = la.cos();
                    axes[0].push(lo.cos() * cp);
                    axes[1].push(lo.sin() * cp);
                    axes[2].push(la.sin());
                } else {
                    axes[0].push(lo);
                    axes[1].push(la);
                }
            }
            axes
        }
        Inputs::Pixel => {
            let mut axes: Vec<Vec<f64>> = (0..2).map(|_| Vec::with_capacity(n)).collect();
            for _ in 0..n {
                axes[0].push((rng.next_f64() - 0.5) * 2.0 * IMAGE_HALF);
                axes[1].push((rng.next_f64() - 0.5) * 2.0 * IMAGE_HALF);
            }
            axes
        }
    }
}

/// The pull request's timing of one transform at one point count: double a
/// batch of calls until it takes at least [`MIN_BATCH`] (or holds
/// [`MAX_BATCH_CALLS`]), then time it `reps` times. Returns the time per call
/// of each rep, in seconds, in the order taken.
pub fn time_batches(
    reps: usize,
    op: &mut dyn FnMut() -> Result<(), String>,
) -> Result<Vec<f64>, String> {
    let mut batch = |ncalls: usize| -> Result<f64, String> {
        let t = Instant::now();
        for _ in 0..ncalls {
            op()?;
        }
        Ok(t.elapsed().as_secs_f64())
    };
    let mut ncalls = 1;
    loop {
        if batch(ncalls)? >= MIN_BATCH.as_secs_f64() || ncalls >= MAX_BATCH_CALLS {
            break;
        }
        ncalls *= 2;
    }
    (0..reps)
        .map(|_| batch(ncalls).map(|dt| dt / ncalls as f64))
        .collect()
}

/// The median of `times`, as the pull request takes it: the middle of the
/// sorted values, the upper one of an even count.
pub fn median(times: &[f64]) -> f64 {
    let mut t = times.to_vec();
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

/// One engine's timing of one transform at one point count.
#[derive(Clone, Debug)]
pub struct Timing {
    /// The transform's label.
    pub label: &'static str,
    /// The point count.
    pub n: usize,
    /// "rust" or "c".
    pub engine: &'static str,
    /// The time per call of each rep, in seconds.
    pub rep_times: Vec<f64>,
}

impl Timing {
    /// Median throughput in millions of points per second.
    pub fn mpxs(&self) -> f64 {
        self.n as f64 / median(&self.rep_times) / 1e6
    }
}

/// How the two engines' outputs for one transform compare, over the largest
/// point count timed.
#[derive(Clone, Debug, Default)]
pub struct BitsCheck {
    /// Output values compared.
    pub values: usize,
    /// Values whose bits differ.
    pub differ: usize,
    /// The largest difference in units in the last place.
    pub max_ulps: u64,
}

/// The distance between two doubles in units in the last place, or
/// `u64::MAX` when their signs differ or either is NaN.
pub fn ulps(a: f64, b: f64) -> u64 {
    if a.to_bits() == b.to_bits() {
        return 0;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return u64::MAX;
    }
    a.to_bits().abs_diff(b.to_bits())
}

/// Compare two engines' outputs value by value.
pub fn compare_bits(a: &[f64], b: &[f64]) -> BitsCheck {
    let mut c = BitsCheck {
        values: a.len(),
        ..BitsCheck::default()
    };
    for (&x, &y) in a.iter().zip(b) {
        let d = ulps(x, y);
        if d != 0 {
            c.differ += 1;
            c.max_ulps = c.max_ulps.max(d);
        }
    }
    c
}

/// Which engines to time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engines {
    /// The port only.
    Rust,
    /// The reference C library only.
    C,
    /// Both, Rust first at each point count.
    Both,
}

/// Run the benchmark over the point counts up to `max_n`, calling `report`
/// with each timing as it is made. Returns, per transform timed on both
/// engines, how their outputs compare at the largest point count.
pub fn run(
    engines: Engines,
    max_n: usize,
    reps: usize,
    report: &mut dyn FnMut(&Timing),
) -> Result<Vec<(&'static str, BitsCheck)>, String> {
    let sweep: Vec<usize> = N_SWEEP.iter().copied().filter(|&n| n <= max_n).collect();
    let top = *sweep.last().ok_or("no point count is within --max-n")?;
    #[cfg(feature = "c-oracle")]
    let c_engine = match engines {
        Engines::Rust => None,
        _ => Some(crate::c_engine::CEngine::open(
            crate::c_engine::Variant::Bench,
        )?),
    };
    #[cfg(not(feature = "c-oracle"))]
    if engines != Engines::Rust {
        return Err("timing C needs ast-bench's c-oracle feature".into());
    }
    let root = Path::new(FIXTURES);
    #[cfg_attr(not(feature = "c-oracle"), allow(unused_mut))]
    let mut checks = Vec::new();
    let mut cached: Option<(Inputs, Vec<Vec<f64>>)> = None;
    for b in BENCHES {
        if cached.as_ref().is_none_or(|(k, _)| *k != b.inputs) {
            // Free the old arrays first: at full size each holds 134 MB.
            drop(cached.take());
            cached = Some((b.inputs, inputs(b.inputs, top)));
        }
        let axes = &cached.as_ref().expect("just filled").1;
        let loaded = match load_fixture(root, b.fixture)? {
            Loaded::Mapping(obj) => obj,
            Loaded::Untransformable(why) => return Err(format!("{}: {why}", b.fixture)),
        };
        let map = loaded.as_mapping().expect("loaded as a Mapping");
        let (nin, nout) = if b.forward {
            (map.nin(), map.nout())
        } else {
            (map.nout(), map.nin())
        };
        if nin != axes.len() {
            return Err(format!(
                "{}: takes {nin} inputs, not {}",
                b.label,
                axes.len()
            ));
        }
        #[cfg(feature = "c-oracle")]
        let c_map = match &c_engine {
            Some(e) => Some(e.load(root, b.fixture)?),
            None => None,
        };
        #[cfg_attr(not(feature = "c-oracle"), allow(unused_assignments))]
        let mut rust_out = Vec::new();
        #[cfg(feature = "c-oracle")]
        let mut c_out = Vec::new();
        for &n in &sweep {
            // C's layout: the first n values of each axis, one axis after
            // another. The port reads the same buffer as one slice per axis.
            let columns: Vec<f64> = axes.iter().flat_map(|a| &a[..n]).copied().collect();
            if engines != Engines::C {
                rust_out = vec![0.0; nout * n];
                let mut op = || -> Result<(), String> {
                    let fail = |e: starlink_ast::error::AstError| format!("{} N={n}: {e}", b.label);
                    let input = Points::new(columns.chunks_exact(n)).map_err(fail)?;
                    let out = PointsMut::new(rust_out.chunks_exact_mut(n)).map_err(fail)?;
                    map.transform_into(input, b.forward, out).map_err(fail)
                };
                report(&Timing {
                    label: b.label,
                    n,
                    engine: "rust",
                    rep_times: time_batches(reps, &mut op)?,
                });
            }
            #[cfg(feature = "c-oracle")]
            if let Some(cm) = &c_map {
                c_out = vec![0.0; nout * n];
                let mut op = || -> Result<(), String> {
                    cm.tran(b.forward, n, nin, &columns, nout, &mut c_out)
                        .map_err(|e| format!("{} N={n}: {e}", b.label))
                };
                report(&Timing {
                    label: b.label,
                    n,
                    engine: "c",
                    rep_times: time_batches(reps, &mut op)?,
                });
            }
        }
        #[cfg(feature = "c-oracle")]
        if engines == Engines::Both {
            checks.push((b.label, compare_bits(&rust_out, &c_out)));
        }
    }
    Ok(checks)
}

/// The summary table: per transform and point count, each engine's median
/// throughput and, with both, how many times faster Rust is than C.
pub fn summary(timings: &[Timing], markdown: bool) -> String {
    let get = |label: &str, n: usize, engine: &str| {
        timings
            .iter()
            .find(|t| t.label == label && t.n == n && t.engine == engine)
            .map(Timing::mpxs)
    };
    let mut s = String::new();
    if markdown {
        s.push_str("| Transform | N | Rust Mpx/s | C Mpx/s | Rust/C speed |\n");
        s.push_str("|-----------|--:|-----------:|--------:|-------------:|\n");
    } else {
        s.push_str(&format!(
            "{:<14} {:>9} {:>12} {:>12} {:>13}\n",
            "transform", "N", "rust Mpx/s", "c Mpx/s", "rust/c speed"
        ));
    }
    let fmt = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{v:.1}"));
    for b in BENCHES {
        for n in N_SWEEP {
            let (r, c) = (get(b.label, n, "rust"), get(b.label, n, "c"));
            if r.is_none() && c.is_none() {
                continue;
            }
            let speed = match (r, c) {
                (Some(r), Some(c)) => format!("{:.2}x", r / c),
                _ => "-".to_string(),
            };
            if markdown {
                s.push_str(&format!(
                    "| {} | {n} | {} | {} | {speed} |\n",
                    b.label,
                    fmt(r),
                    fmt(c)
                ));
            } else {
                s.push_str(&format!(
                    "{:<14} {n:>9} {:>12} {:>12} {speed:>13}\n",
                    b.label,
                    fmt(r),
                    fmt(c)
                ));
            }
        }
    }
    s
}

/// The pull request's CSV: `transform,n_points,rep,time_s`, the transform
/// labeled with the engine (`sphmap_fwd_rust`, `sphmap_fwd_c`).
pub fn csv(timings: &[Timing]) -> String {
    let mut s = String::from("transform,n_points,rep,time_s\n");
    for t in timings {
        for (rep, dt) in t.rep_times.iter().enumerate() {
            s.push_str(&format!(
                "{}_{},{},{rep},{dt:.9e}\n",
                t.label, t.engine, t.n
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `srand48(42)` then `drand48()`, from the C library on macOS aarch64.
    #[test]
    fn drand48_reproduces_c() {
        let mut r = Drand48::new(42);
        let want = [
            0x3fe7_d326_17ca_2020_u64,
            0x3fd5_eed2_2ed8_de00,
            0x3fbc_7015_c72a_2300,
            0x3fdb_0799_fb18_bc80,
        ];
        for w in want {
            assert_eq!(r.next_f64().to_bits(), w);
        }
        for _ in 4..999 {
            r.next_f64();
        }
        assert_eq!(r.next_f64().to_bits(), 0x3fd5_d764_fc27_bd80);
    }

    #[test]
    fn spherical_and_cartesian_inputs_are_the_same_points() {
        let s = inputs(Inputs::Spherical, 100);
        let c = inputs(Inputs::Cartesian, 100);
        for i in 0..100 {
            let (lo, la) = (s[0][i], s[1][i]);
            assert_eq!(c[2][i], la.sin());
            assert_eq!(c[0][i], lo.cos() * la.cos());
            assert!((0.0..2.0 * std::f64::consts::PI).contains(&lo));
        }
    }

    #[test]
    fn pixel_inputs_cover_the_image() {
        let p = inputs(Inputs::Pixel, 10_000);
        for a in &p {
            assert!(a.iter().all(|v| (-IMAGE_HALF..IMAGE_HALF).contains(v)));
            assert!(a.iter().any(|&v| v < -1900.0) && a.iter().any(|&v| v > 1900.0));
        }
    }

    #[test]
    fn every_fixture_loads_with_the_inputs_it_is_given() {
        for b in BENCHES {
            let Loaded::Mapping(obj) = load_fixture(Path::new(FIXTURES), b.fixture).unwrap() else {
                panic!("{} is not a Mapping", b.fixture);
            };
            let m = obj.as_mapping().unwrap();
            let nin = if b.forward { m.nin() } else { m.nout() };
            assert_eq!(nin, inputs(b.inputs, 1).len(), "{}", b.label);
        }
    }

    #[test]
    fn batches_report_one_time_per_rep() {
        let mut calls = 0;
        let mut op = || -> Result<(), String> {
            calls += 1;
            std::thread::sleep(Duration::from_millis(5));
            Ok(())
        };
        let t = time_batches(3, &mut op).unwrap();
        assert_eq!(t.len(), 3);
        // A 5 ms call reaches 20 ms in a batch of 4.
        assert!(t.iter().all(|&dt| (0.005..0.05).contains(&dt)), "{t:?}");
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 3.0);
    }

    #[test]
    fn ulps_counts_representable_steps() {
        assert_eq!(ulps(1.0, 1.0), 0);
        assert_eq!(ulps(1.0, f64::from_bits(1.0f64.to_bits() + 3)), 3);
        assert_eq!(ulps(1.0, -1.0), u64::MAX);
        let c = compare_bits(
            &[1.0, 2.0, 3.0],
            &[1.0, f64::from_bits(2.0f64.to_bits() + 1), 3.0],
        );
        assert_eq!((c.values, c.differ, c.max_ulps), (3, 1, 1));
    }

    /// Both engines give the same bits on the pull request's inputs.
    #[cfg(feature = "c-oracle")]
    #[test]
    fn engines_agree_bit_for_bit() {
        let checks = run(Engines::Both, 4096, 1, &mut |_| {}).unwrap();
        assert_eq!(checks.len(), BENCHES.len());
        for (label, c) in checks {
            assert_eq!(c.differ, 0, "{label}: {c:?}");
        }
    }
}
