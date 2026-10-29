// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

use super::math;
use crate::vmath;

// `PI/180.0` and `180.0/PI` from `reference/starlink-ast/wcslib/wcsmath.h`, whose `PI`
// literal rounds to `std::f64::consts::PI`.
use std::f64::consts::PI;
pub(super) const D2R: f64 = PI / 180.0;
pub(super) const R2D: f64 = 180.0 / PI;

/// How close to the end of its range an argument of `acosd` or `asind` may
/// be, beyond the range, and still be taken as the end (`wcstrig.h`).
pub(crate) const WCSTRIG_TOL: f64 = 1e-10;

/// The trigonometry the projections use, with angles in degrees.
///
/// The projections are written once over this trait.
/// [`Libm`] evaluates every function exactly as wcslib does, including the
/// exact results at multiples of 45 and 90 degrees and the snapping of
/// inverse arguments within [`WCSTRIG_TOL`] of the ends of their range.
pub(crate) trait Trig {
    /// Whether a projection body computes a value it may not use rather than
    /// branch around it: true for trigonometry run in a vectorized loop,
    /// whose body must not branch, and false for [`Libm`], whose calls are
    /// made one point at a time and would pay for the unused value.
    const BRANCH_FREE: bool;
    /// Sine of an angle in degrees.
    fn sind(a: f64) -> f64;
    /// Cosine of an angle in degrees.
    fn cosd(a: f64) -> f64;
    /// Tangent of an angle in degrees.
    fn tand(a: f64) -> f64;
    /// Arc sine, in degrees.
    fn asind(v: f64) -> f64;
    /// Arc cosine, in degrees.
    fn acosd(v: f64) -> f64;
    /// Arc tangent, in degrees.
    fn atand(v: f64) -> f64;
    /// Arc tangent of `y / x`, in degrees, in the quadrant of `(x, y)`.
    fn atan2d(y: f64, x: f64) -> f64;
}

/// [`Trig`] by the platform's math library: bit for bit what wcslib returns.
pub(crate) struct Libm;

/// The quadrant `abs((int)floor(x)) % 4` of C, for `x` already floored.
///
/// C's conversion to `int` is undefined once `x` is outside the range of an
/// `int`, and an angle that is a multiple of 90.0 can be that large. The
/// result here does not depend on the platform: the conversion saturates, as
/// arm64's does, and `abs` wraps `INT_MIN` to itself, so `x` beyond 2^31
/// gives `INT_MAX % 4` = 3 and `x` below -2^31 gives `INT_MIN % 4` = 0.
/// Rust's saturating `as` and `wrapping_abs` give exactly those results
/// without panicking.
#[inline(always)]
fn quadrant(floored: f64) -> i32 {
    (floored as i32).wrapping_abs() % 4
}

impl Trig for Libm {
    const BRANCH_FREE: bool = false;

    // reference/starlink-ast/wcslib/wcstrig.c, astSind; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn sind(angle: f64) -> f64 {
        if angle % 90.0 == 0.0 {
            match quadrant((angle / 90.0 - 0.5).floor()) {
                0 => return 1.0,
                1 => return 0.0,
                2 => return -1.0,
                3 => return 0.0,
                _ => {}
            }
        }
        math::sin(angle * D2R)
    }

    // reference/starlink-ast/wcslib/wcstrig.c, astCosd; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn cosd(angle: f64) -> f64 {
        if angle % 90.0 == 0.0 {
            match quadrant((angle / 90.0 + 0.5).floor()) {
                0 => return 1.0,
                1 => return 0.0,
                2 => return -1.0,
                3 => return 0.0,
                _ => {}
            }
        }
        math::cos(angle * D2R)
    }

    // reference/starlink-ast/wcslib/wcstrig.c, astTand; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn tand(angle: f64) -> f64 {
        let resid = angle % 360.0;
        if resid == 0.0 || resid.abs() == 180.0 {
            return 0.0;
        } else if resid == 45.0 || resid == 225.0 {
            return 1.0;
        } else if resid == -135.0 || resid == -315.0 {
            return -1.0;
        }
        math::tan(angle * D2R)
    }

    // reference/starlink-ast/wcslib/wcstrig.c, astASind; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn asind(v: f64) -> f64 {
        if v <= -1.0 {
            if v + 1.0 > -WCSTRIG_TOL {
                return -90.0;
            }
        } else if v == 0.0 {
            return 0.0;
        } else if v >= 1.0 && v - 1.0 < WCSTRIG_TOL {
            return 90.0;
        }
        math::asin(v) * R2D
    }

    // reference/starlink-ast/wcslib/wcstrig.c, astACosd; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn acosd(v: f64) -> f64 {
        if v >= 1.0 {
            if v - 1.0 < WCSTRIG_TOL {
                return 0.0;
            }
        } else if v == 0.0 {
            return 90.0;
        } else if v <= -1.0 && v + 1.0 > -WCSTRIG_TOL {
            return 180.0;
        }
        math::acos(v) * R2D
    }

    // reference/starlink-ast/wcslib/wcstrig.c, astATand; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn atand(v: f64) -> f64 {
        if v == -1.0 {
            return -45.0;
        } else if v == 0.0 {
            return 0.0;
        } else if v == 1.0 {
            return 45.0;
        }
        math::atan(v) * R2D
    }

    // reference/starlink-ast/wcslib/wcstrig.c, astATan2d; reference/starlink-ast at 3d3f255f.
    #[inline(always)]
    fn atan2d(y: f64, x: f64) -> f64 {
        if y == 0.0 {
            if x >= 0.0 {
                return 0.0;
            } else if x < 0.0 {
                return 180.0;
            }
        } else if x == 0.0 {
            if y > 0.0 {
                return 90.0;
            } else if y < 0.0 {
                return -90.0;
            }
        }
        math::atan2(y, x) * R2D
    }
}

/// [`Trig`] by the branchless kernels of [`crate::vmath`], which vectorize
/// when a projection body is inlined into a loop over whole arrays.
///
/// `sind`, `cosd`, `atan2d` and `atand` are kernels, valid where their
/// predicates ([`sincosd_covers`], [`atan2d_covers`], [`atand_covers`])
/// hold; the predicates reject wcslib's exact special cases and the kernels'
/// invalid inputs, and a caller redoes the elements they reject with
/// [`Libm`]. There `sind` and `cosd` are within 1 ULP of the exact sine and
/// cosine of their argument in radians, rounded, on every platform, since
/// they call no library. [`Libm`]'s are the platform libm's, which can
/// itself be further from the exact value: macOS's on x86-64 is 2.65 ULP
/// from the sine of -6.452961993231616e5 radians. Over angles within two
/// turns the two were within 2 ULP of each other on every platform
/// measured, and every drift bound derived for a projection's slice takes 2
/// ULP per `sind` or `cosd`.
/// `atan2d` and `atand` are an arctangent within 1 ULP of libm's times
/// `R2D`, whose rounding can take them 2 ULP from [`Libm`]'s. `tand`,
/// `asind` and `acosd` are [`Libm`]'s.
///
/// `FMA` is passed to [`crate::vmath::atan2`]: whether the code this is
/// inlined into may use fused multiply-adds. The result is the same either
/// way.
pub(crate) struct Kernel<const FMA: bool>;

impl<const FMA: bool> Trig for Kernel<FMA> {
    const BRANCH_FREE: bool = true;

    #[inline(always)]
    fn sind(angle: f64) -> f64 {
        // The argument is rounded as wcslib rounds the one it gives libm.
        vmath::sin_cos(angle * D2R).0
    }

    #[inline(always)]
    fn cosd(angle: f64) -> f64 {
        vmath::sin_cos(angle * D2R).1
    }

    #[inline(always)]
    fn tand(angle: f64) -> f64 {
        Libm::tand(angle)
    }

    #[inline(always)]
    fn asind(v: f64) -> f64 {
        Libm::asind(v)
    }

    #[inline(always)]
    fn acosd(v: f64) -> f64 {
        Libm::acosd(v)
    }

    #[inline(always)]
    fn atand(v: f64) -> f64 {
        vmath::atan2::<FMA>(v, 1.0) * R2D
    }

    #[inline(always)]
    fn atan2d(y: f64, x: f64) -> f64 {
        vmath::atan2::<FMA>(y, x) * R2D
    }
}

/// Whether [`Kernel`]'s `sind` and `cosd` are valid for `angle`: not a
/// multiple of 90 degrees, which [`Libm`] answers exactly, and within the
/// sine and cosine kernel's domain, which excludes infinities and NaN.
///
/// Whether `angle` is a multiple of 90 is decided without a remainder or a
/// rounding to an integer, which are library calls on some targets, so the
/// predicate vectorizes with the kernels: the nearest integer `k` to
/// `angle / 90` is exact, and so is `90 * k`, for any `angle` in the
/// kernel's domain, so `90 * k == angle` exactly when `angle` is a multiple
/// of 90. Outside the domain, where that can fail, the second test rejects
/// the angle anyway.
// `k` is `(q + TOINT) - TOINT` for `q = angle / 90`. Where |q| <= 2^51,
// `q + TOINT` lies in [2^52, 2^53], where the doubles are the integers, so it
// rounds to TOINT plus the nearest integer to `q`, ties to even since TOINT
// is even, and subtracting TOINT is exact: `k` is `q.round_ties_even()`, but
// for the sign of a zero, which `90 * k == angle` does not see. The kernel's
// domain, |angle * D2R| <= 1e6, keeps |q| below 2^20. Outside it, `k` can be
// anything, NaN for a NaN or infinite `angle`, but the second test is false
// there, NaN, infinities and zeros included, so the predicate is false
// whatever the first test gives. The predicate is therefore the same for
// every input as with `round_ties_even`, which
// `sincosd_covers_is_the_rounding_test_for_every_input` checks.
#[inline(always)]
pub(crate) fn sincosd_covers(angle: f64) -> bool {
    let q = angle / 90.0;
    ((q + TOINT - TOINT) * 90.0 != angle) & vmath::sin_cos_covers(angle * D2R)
}

/// 1.5 * 2^52: adding and subtracting it rounds a value of magnitude at
/// most 2^51 to the nearest integer.
const TOINT: f64 = 1.5 / f64::EPSILON;

/// Whether [`Kernel`]'s `atan2d` is valid for (`y`, `x`): neither zero, which
/// [`Libm`] answers exactly, and both finite.
#[cfg_attr(not(test), allow(dead_code))] // used by the projections' slice kernels
#[inline(always)]
pub(crate) fn atan2d_covers(y: f64, x: f64) -> bool {
    y != 0.0 && x != 0.0 && vmath::atan2_covers(y, x)
}

/// Whether [`Kernel`]'s `atand` is valid for `v`: finite and none of -1, 0
/// and 1, which [`Libm`] answers exactly.
#[inline(always)]
pub(crate) fn atand_covers(v: f64) -> bool {
    v.is_finite() && v != -1.0 && v != 0.0 && v != 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_inputs::{angle_pairs, degrees, ulps, unit_range};
    use crate::vmath::HAS_FMA;

    type K = Kernel<HAS_FMA>;

    /// How far `atan2d` and `atand` may be from [`Libm`]'s, in ULP. Each is an
    /// arctangent within 1 ULP of libm's, asserted separately below, times
    /// `R2D`. The product rounds once more, and two radian values 1 ULP apart
    /// can land 2 ULP apart in degrees, since `R2D` is about 57.3 and the
    /// product's ULP is 32 or 64 times the radian value's. The bound is per
    /// elementary function; 2 is the most measured.
    const R2D_PRODUCT_ULPS: u64 = 2;

    /// The largest ULP distance between two functions over `inputs`, and how
    /// many values differ.
    fn worst<T: Copy>(
        inputs: &[T],
        kernel: impl Fn(T) -> f64,
        libm: impl Fn(T) -> f64,
    ) -> (u64, usize) {
        let mut max = 0;
        let mut differ = 0;
        for &x in inputs {
            let d = ulps(kernel(x), libm(x));
            max = max.max(d);
            differ += usize::from(d != 0);
        }
        (max, differ)
    }

    /// How far `sind` and `cosd` may be from [`Libm`]'s over this test's
    /// angles, within two turns, in ULP: the kernel is within 1 ULP of the
    /// exact value, rounded, on every platform, and the two were within 2
    /// ULP of each other there on every platform measured (macOS on arm64
    /// and x86-64).
    const SINCOS_LIBM_ULPS: u64 = 2;

    /// `sind` and `cosd` within [`SINCOS_LIBM_ULPS`] of [`Libm`]'s, and
    /// `atan2d` and `atand` within [`R2D_PRODUCT_ULPS`], their arctangents
    /// in radians within 1 ULP of libm's.
    #[test]
    fn kernel_trig_is_within_two_ulp_of_libm_where_it_covers() {
        let angles: Vec<f64> = degrees(2_000_000)
            .into_iter()
            .filter(|&a| sincosd_covers(a))
            .collect();
        let pairs: Vec<(f64, f64)> = angle_pairs(1_000_000)
            .into_iter()
            .filter(|&(y, x)| atan2d_covers(y, x))
            .collect();
        let mut values = unit_range(2_000_000);
        values.extend([5e-324, -1e-300, 1e10, -1e300, f64::MAX]);
        let values: Vec<f64> = values.into_iter().filter(|&v| atand_covers(v)).collect();

        let in_degrees = [
            (
                "sind",
                worst(&angles, K::sind, Libm::sind),
                SINCOS_LIBM_ULPS,
            ),
            (
                "cosd",
                worst(&angles, K::cosd, Libm::cosd),
                SINCOS_LIBM_ULPS,
            ),
            (
                "atan2d",
                worst(
                    &pairs,
                    |(y, x)| K::atan2d(y, x),
                    |(y, x)| Libm::atan2d(y, x),
                ),
                R2D_PRODUCT_ULPS,
            ),
            (
                "atand",
                worst(&values, K::atand, Libm::atand),
                R2D_PRODUCT_ULPS,
            ),
        ];
        // The arctangents before the product with R2D.
        let in_radians = [
            (
                "atan2d",
                worst(
                    &pairs,
                    |(y, x)| vmath::atan2::<HAS_FMA>(y, x),
                    |(y, x)| math::atan2(y, x),
                ),
            ),
            (
                "atand",
                worst(&values, |v| vmath::atan2::<HAS_FMA>(v, 1.0), math::atan),
            ),
        ];
        for (name, (max, differ), bound) in in_degrees {
            assert!(max <= bound, "{name}: {max} ULP from libm");
            assert!(
                differ > 0,
                "{name}: no value differs from libm; is the kernel in use?"
            );
        }
        for (name, (max, _)) in in_radians {
            assert!(max <= 1, "{name}: {max} ULP from libm before the product");
        }
    }

    /// [`sincosd_covers`] is `angle % 90.0 != 0.0`, as [`Libm`] tests for
    /// its exact cases, together with the kernel's domain.
    #[test]
    fn sincosd_covers_is_the_remainder_test_within_the_domain() {
        let mut angles = degrees(1_000_000);
        for k in [
            1e5,
            6.3e5,
            6.4e5,
            1e6,
            70368744177664.0,
            9007199254740992.0,
            1e300f64,
        ] {
            for a in [90.0 * k, 90.0 * k + 0.5, 90.0 * k - 90.0] {
                angles.extend([a, -a, f64::from_bits(a.to_bits() + 1)]);
            }
        }
        angles.extend([5e-324, -5e-324, 1e-310, 1e-300]);
        for a in angles {
            let by_remainder = a % 90.0 != 0.0 && vmath::sin_cos_covers(a * D2R);
            assert_eq!(sincosd_covers(a), by_remainder, "{a:e}");
        }
    }

    /// [`sincosd_covers`] with the nearest integer taken by
    /// `round_ties_even`, a library call on x86-64 without SSE4.1.
    fn sincosd_covers_by_rounding(angle: f64) -> bool {
        ((angle / 90.0).round_ties_even() * 90.0 != angle) & vmath::sin_cos_covers(angle * D2R)
    }

    /// [`sincosd_covers`] gives the same answer as
    /// [`sincosd_covers_by_rounding`] for every input: random angles, every
    /// multiple of 90 near zero and near the ends of the kernel's domain
    /// with its neighbours, the angles either side of those ends, the
    /// magnitudes where adding TOINT stops rounding to an integer, and the
    /// zeros, subnormals, infinities and NaN. Within the domain the nearest
    /// integers themselves agree.
    #[test]
    fn sincosd_covers_is_the_rounding_test_for_every_input() {
        let step = |a: f64, n: i64| f64::from_bits(a.to_bits().wrapping_add_signed(n));
        let mut angles = degrees(1_000_000);
        // The largest multiple of 90 inside the domain is near 636619.77 * 90.
        let edge = 1.0e6 * R2D;
        let ks = (-2000i64..=2000)
            .chain(636_000..=636_700)
            .chain(-636_700..=-636_000);
        for k in ks {
            let a = 90.0 * k as f64;
            for n in -4i64..=4 {
                angles.push(step(a, n));
            }
            angles.extend([a + 45.0, a - 45.0, a + 0.5, a - 0.5]);
        }
        for a in [edge, -edge] {
            for n in -4096i64..=4096 {
                angles.push(step(a, n));
            }
        }
        for e in 50u64..=54 {
            // 2^e.
            let q = f64::from_bits((1023 + e) << 52);
            for a in [q * 90.0, -q * 90.0] {
                for n in -4i64..=4 {
                    angles.push(step(a, n));
                }
            }
        }
        angles.extend([
            0.0,
            -0.0,
            5e-324,
            -5e-324,
            f64::MIN_POSITIVE,
            -f64::MIN_POSITIVE,
            1e-300,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            -f64::NAN,
        ]);
        for a in angles {
            assert_eq!(
                sincosd_covers(a),
                sincosd_covers_by_rounding(a),
                "{a:e} ({:#x})",
                a.to_bits()
            );
            if vmath::sin_cos_covers(a * D2R) {
                let q = a / 90.0;
                assert_eq!(q + TOINT - TOINT, q.round_ties_even(), "{a:e}");
            }
        }
    }

    #[test]
    fn covers_rejects_wcstrig_exact_arguments() {
        for a in [0.0, -0.0, 90.0, -90.0, 180.0, 270.0, 360.0, -720.0] {
            assert!(!sincosd_covers(a), "{a}");
        }
        assert!(!atan2d_covers(0.0, -1.0) && !atan2d_covers(1.0, 0.0));
        assert!(!atand_covers(1.0) && !atand_covers(-1.0) && !atand_covers(0.0));
        assert!(sincosd_covers(45.0) && atan2d_covers(1.0, 1.0));
    }

    /// The port's own values beyond the range of an `int` quadrant count,
    /// the same on every platform.
    #[test]
    fn huge_multiples_of_90_have_the_aarch64_values() {
        let a = 90.0 * 1099511627776.0; // 90 * 2^40
        assert_eq!(Libm::cosd(a).to_bits(), 0.0f64.to_bits());
        assert_eq!(Libm::sind(a).to_bits(), 0.0f64.to_bits());
        assert_eq!(Libm::cosd(-a).to_bits(), 1.0f64.to_bits());
        assert_eq!(Libm::sind(-a).to_bits(), 1.0f64.to_bits());
    }
}
