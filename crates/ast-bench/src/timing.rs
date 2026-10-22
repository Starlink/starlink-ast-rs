//! Time a transform: repeat it until a time budget is filled and report the
//! median and minimum time per point.

//! Time a transform: repeat it until a time budget is filled and report the
//! median and minimum time per point.

use std::time::{Duration, Instant};

/// Per-point timing of one engine on one case, direction and point count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    /// The median over samples of the time per point, in nanoseconds.
    pub median_ns_per_point: f64,
    /// The fastest sample's time per point, in nanoseconds.
    pub min_ns_per_point: f64,
    /// How many samples were taken.
    pub samples: usize,
}

/// One timed operation: a single transform call.
pub type Op<'a> = &'a mut dyn FnMut() -> Result<(), String>;

/// The shortest a sample may be; calls are repeated within a sample until
/// it is at least this long, so the clock's resolution does not dominate.
const MIN_SAMPLE: Duration = Duration::from_millis(1);

/// Time each of `ops` on `npoint` points, taking samples in rounds (every op
/// once per round, in order) until `budget` has passed and every op has at
/// least `min_samples` samples. Rounds keep drift from favoring one op.
pub fn measure(
    budget: Duration,
    min_samples: usize,
    npoint: usize,
    ops: &mut [Op<'_>],
) -> Result<Vec<Stats>, String> {
    let mut reps = vec![1usize; ops.len()];
    for (k, op) in ops.iter_mut().enumerate() {
        loop {
            let t = Instant::now();
            for _ in 0..reps[k] {
                op()?;
            }
            if t.elapsed() >= MIN_SAMPLE || reps[k] >= 1 << 24 {
                break;
            }
            reps[k] *= 2;
        }
    }
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); ops.len()];
    let start = Instant::now();
    while samples.iter().any(|s| s.len() < min_samples) || start.elapsed() < budget {
        for (k, op) in ops.iter_mut().enumerate() {
            let t = Instant::now();
            for _ in 0..reps[k] {
                op()?;
            }
            samples[k].push(t.elapsed().as_nanos() as f64 / (reps[k] * npoint.max(1)) as f64);
        }
    }
    Ok(samples
        .into_iter()
        .map(|mut s| {
            s.sort_by(f64::total_cmp);
            Stats {
                median_ns_per_point: s[s.len() / 2],
                min_ns_per_point: s[0],
                samples: s.len(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn measure_reports_per_point_time() {
        let mut f = || -> Result<(), String> {
            std::thread::sleep(Duration::from_micros(100));
            Ok(())
        };
        let s = &measure(Duration::from_millis(20), 5, 1000, &mut [&mut f]).unwrap()[0];
        assert!(s.samples >= 5);
        // 100 µs over 1000 points is about 100 ns per point; sleep only overshoots.
        assert!(
            s.min_ns_per_point >= 100.0 && s.min_ns_per_point < 1000.0,
            "{s:?}"
        );
    }

    #[test]
    fn measure_propagates_an_error() {
        let mut f = || -> Result<(), String> { Err("boom".into()) };
        assert!(measure(Duration::from_millis(5), 1, 1, &mut [&mut f]).is_err());
    }

    #[test]
    fn measure_alternates_engines() {
        let order = std::cell::RefCell::new(Vec::new());
        let mut a = || -> Result<(), String> {
            order.borrow_mut().push('a');
            Ok(())
        };
        let mut b = || -> Result<(), String> {
            order.borrow_mut().push('b');
            Ok(())
        };
        let stats = measure(Duration::from_millis(5), 3, 1, &mut [&mut a, &mut b]).unwrap();
        assert_eq!(stats.len(), 2);
        let o = order.borrow();
        // Both run in every round, a then b.
        assert!(o.windows(2).any(|w| w == ['a', 'b']) && o.windows(2).any(|w| w == ['b', 'a']));
    }
}
