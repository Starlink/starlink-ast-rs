//! The 1-d kernels of mapping.c (`Sinc`, `SincCos`, `SincGauss`,
//! `SincSinc`, `Somb`, `SombCos` and `Gauss`) and `J1Bessel`, which `Somb`
//! and `SombCos` use, shared by resampling and rebinning.
//!
//! Each takes the offset of a contributing pixel from the interpolation
//! point and the parameters [`setup`] derives from the user's. C computes pi
//! as `acos(-1.0)` and half pi as `0.5 * pi`, which are [`PI`] and
//! [`FRAC_PI_2`] exactly.

use std::f64::consts::{FRAC_PI_2, PI};

/// A 1-d kernel: its value at an offset, given the parameters [`setup`]
/// derives.
pub(crate) type KernelFn = fn(f64, &[f64]) -> f64;

/// A kernel of mapping.c.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Builtin {
    Sinc,
    SincSinc,
    SincCos,
    SincGauss,
    Gauss,
    Somb,
    SombCos,
}

impl Builtin {
    /// The number of `params` the set-up reads.
    pub(crate) fn nparam(self) -> usize {
        match self {
            Builtin::Sinc | Builtin::Somb => 1,
            _ => 2,
        }
    }
}

/// The set-up `ResampleSection` and `RebinSection` share: the kernel
/// function, the number of neighbouring pixels either side, and the
/// parameters the kernel is given, from the user's `params`.
pub(crate) fn setup(kernel: Builtin, params: &[f64]) -> (KernelFn, i64, Vec<f64>) {
    // C's `MaxD`, `MaxI` and `MinI`.
    let max_d = |a: f64, b: f64| if a > b { a } else { b };
    let max_i = |a: i32, b: i32| if a > b { a } else { b };
    let min_i = |a: i32, b: i32| if a < b { a } else { b };
    let nearest_int = |v: f64| (v + 0.5).floor() as i32;
    // Sinc and Somb: two neighbours by default.
    let default_two = || {
        let n = nearest_int(params[0]);
        if n <= 0 { 2 } else { max_i(1, n) }
    };
    // The kernels cut off at the first zero of a factor whose offset is
    // params[1]: at most that many neighbours, and that many by default.
    let to_zero = || {
        let mut n = nearest_int(params[0]);
        if n <= 0 {
            n = i32::MAX;
        }
        min_i(n, max_d(1.0, params[1]).ceil() as i32)
    };
    let k_zero = || vec![0.5 / max_d(1.0, params[1])];
    // The Gaussian kernels: out to 1% of the peak by default.
    let gaussian = || {
        let fwhm = max_d(0.1, params[1]);
        let k = 4.0 * 2.0_f64.ln() / (fwhm * fwhm);
        let mut n = nearest_int(params[0]);
        if n <= 0 {
            n = (-(0.01_f64.ln()) / k).sqrt().ceil() as i32;
        }
        (n, vec![k])
    };
    let (f, n, par): (KernelFn, i32, Vec<f64>) = match kernel {
        Builtin::Sinc => (sinc, default_two(), Vec::new()),
        Builtin::Somb => (somb, default_two(), Vec::new()),
        Builtin::SincCos => (sinc_cos, to_zero(), k_zero()),
        Builtin::SincSinc => (sinc_sinc, to_zero(), k_zero()),
        Builtin::SombCos => (somb_cos, to_zero(), k_zero()),
        Builtin::SincGauss => {
            let (n, par) = gaussian();
            (sinc_gauss, n, par)
        }
        Builtin::Gauss => {
            let (n, par) = gaussian();
            (gauss, n, par)
        }
    };
    (f, i64::from(n), par)
}

/// mapping.c `Sinc`: `sinc(pi * offset)`.
pub(crate) fn sinc(offset: f64, _params: &[f64]) -> f64 {
    let offset = offset * PI;
    if offset != 0.0 {
        offset.sin() / offset
    } else {
        1.0
    }
}

/// mapping.c `SincCos`: `sinc(pi * offset) * cos(k * pi * offset)`, out to
/// the first zero of the cosine, with `k` in `params[0]`.
pub(crate) fn sinc_cos(offset: f64, params: &[f64]) -> f64 {
    let offset = PI * offset.abs();
    let offset_k = offset * params[0];
    if offset_k < FRAC_PI_2 {
        (if offset != 0.0 {
            offset.sin() / offset
        } else {
            1.0
        }) * offset_k.cos()
    } else {
        0.0
    }
}

/// mapping.c `SincGauss`: `sinc(pi * offset) * exp(-k * offset * offset)`,
/// with `k` in `params[0]`.
pub(crate) fn sinc_gauss(offset: f64, params: &[f64]) -> f64 {
    let offset_pi = PI * offset;
    (if offset_pi != 0.0 {
        offset_pi.sin() / offset_pi
    } else {
        1.0
    }) * (-params[0] * offset * offset).exp()
}

/// mapping.c `SincSinc`: `sinc(pi * offset) * sinc(k * pi * offset)`, out
/// to the first zero of the second factor, with `k` in `params[0]`.
pub(crate) fn sinc_sinc(offset: f64, params: &[f64]) -> f64 {
    let offset = PI * offset.abs();
    let offset_k = offset * params[0];
    if offset_k < FRAC_PI_2 {
        (if offset != 0.0 {
            offset.sin() / offset
        } else {
            1.0
        }) * (if offset_k != 0.0 {
            offset_k.sin() / offset_k
        } else {
            1.0
        })
    } else {
        0.0
    }
}

/// mapping.c `Somb`: `somb(pi * offset)`, that is `2 * J1(x) / x`.
pub(crate) fn somb(offset: f64, _params: &[f64]) -> f64 {
    let offset = offset * PI;
    if offset != 0.0 {
        2.0 * j1_bessel(offset) / offset
    } else {
        1.0
    }
}

/// mapping.c `SombCos`: `somb(pi * offset) * cos(k * pi * offset)`, out to
/// the first zero of the cosine, with `k` in `params[0]`.
///
/// C forms the first factor as `J1(x) / x`, without the factor of 2 `Somb`
/// has, but still takes it as 1 at zero offset, where `J1(x) / x` tends to
/// a half; the port does the same.
pub(crate) fn somb_cos(offset: f64, params: &[f64]) -> f64 {
    let offset = PI * offset.abs();
    let offset_k = offset * params[0];
    if offset_k < FRAC_PI_2 {
        (if offset != 0.0 {
            j1_bessel(offset) / offset
        } else {
            1.0
        }) * offset_k.cos()
    } else {
        0.0
    }
}

/// mapping.c `Gauss`: `exp(-k * offset * offset)`, with `k` in `params[0]`.
pub(crate) fn gauss(offset: f64, params: &[f64]) -> f64 {
    (-params[0] * offset * offset).exp()
}

/// mapping.c `J1Bessel`: the Bessel function of the first kind of order
/// one, by C's rational and asymptotic approximations.
// C's truncated 2/pi and 3pi/4 are part of the approximation.
#[allow(clippy::approx_constant)]
fn j1_bessel(x: f64) -> f64 {
    const P1: f64 = 1.0;
    const P2: f64 = 0.183105E-2;
    const P3: f64 = -0.3516396496E-4;
    const P4: f64 = 0.2457520174E-5;
    const P5: f64 = -0.240337019E-6;

    const Q1: f64 = 0.04687499995;
    const Q2: f64 = -0.2002690873E-3;
    const Q3: f64 = 0.8449199096E-5;
    const Q4: f64 = -0.88228987E-6;
    const Q5: f64 = 0.105787412E-6;

    const R1: f64 = 72362614232.0;
    const R2: f64 = -7895059235.0;
    const R3: f64 = 242396853.1;
    const R4: f64 = -2972611.439;
    const R5: f64 = 15704.48260;
    const R6: f64 = -30.16036606;

    const S1: f64 = 144725228442.0;
    const S2: f64 = 2300535178.0;
    const S3: f64 = 18583304.74;
    const S4: f64 = 99447.43394;
    const S5: f64 = 376.9991397;
    const S6: f64 = 1.0;

    let ax = x.abs();
    if ax < 8.0 {
        let y = x * x;
        x * (R1 + y * (R2 + y * (R3 + y * (R4 + y * (R5 + y * R6)))))
            / (S1 + y * (S2 + y * (S3 + y * (S4 + y * (S5 + y * S6)))))
    } else {
        let s = if x >= 0.0 { 1.0 } else { -1.0 };
        let z = 8.0 / ax;
        let y = z * z;
        let xx = ax - 2.356194491;
        (0.636619772 / ax).sqrt()
            * (xx.cos() * (P1 + y * (P2 + y * (P3 + y * (P4 + y * P5))))
                - z * xx.sin() * (Q1 + y * (Q2 + y * (Q3 + y * (Q4 + y * Q5)))))
            * s
    }
}
