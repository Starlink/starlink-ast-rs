// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

use super::math;

// `PI/180.0` and `180.0/PI` from `vendor/wcslib/wcsmath.h`, whose `PI`
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
    // vendor/wcslib/wcstrig.c, astSind; reference/starlink-ast at 3d3f255f.
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

    // vendor/wcslib/wcstrig.c, astCosd; reference/starlink-ast at 3d3f255f.
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

    // vendor/wcslib/wcstrig.c, astTand; reference/starlink-ast at 3d3f255f.
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

    // vendor/wcslib/wcstrig.c, astASind; reference/starlink-ast at 3d3f255f.
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

    // vendor/wcslib/wcstrig.c, astACosd; reference/starlink-ast at 3d3f255f.
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

    // vendor/wcslib/wcstrig.c, astATand; reference/starlink-ast at 3d3f255f.
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

    // vendor/wcslib/wcstrig.c, astATan2d; reference/starlink-ast at 3d3f255f.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_inputs::{degrees, unit_range};
    use crate::wcslib::difftest::*;

    /// Angles that are multiples of 90 beyond the range of an `int` quadrant
    /// count, where C's conversion to `int` is undefined.
    #[test]
    fn huge_multiples_of_90_match_wcstrig() {
        let mut p2 = 1.0f64;
        for _ in 0..70 {
            for m in [1.0, 3.0, 45.0, 90.0] {
                for s in [1.0, -1.0] {
                    let a = s * m * p2;
                    // C's result for |angle/90| >= 2^31 is undefined behavior that
                    // differs by architecture; the port follows aarch64 everywhere,
                    // so only aarch64 can compare it with C.
                    let beyond_int = (a / 90.0).abs() >= 2147483648.0;
                    // SAFETY: pure functions of their arguments.
                    unsafe {
                        if !beyond_int || cfg!(target_arch = "aarch64") {
                            same("cosd", a, Libm::cosd(a), astCosd(a));
                            same("sind", a, Libm::sind(a), astSind(a));
                        }
                        same("tand", a, Libm::tand(a), astTand(a));
                    }
                }
            }
            p2 *= 2.0;
        }
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

    fn same(name: &str, x: f64, rust: f64, c: f64) {
        assert_eq!(
            rust.to_bits(),
            c.to_bits(),
            "{name}({x:e}): {rust:e} vs {c:e}"
        );
    }

    #[test]
    fn libm_trig_matches_wcstrig_bit_for_bit() {
        for a in degrees(200_000) {
            // SAFETY: pure functions of their arguments.
            unsafe {
                same("cosd", a, Libm::cosd(a), astCosd(a));
                same("sind", a, Libm::sind(a), astSind(a));
                same("tand", a, Libm::tand(a), astTand(a));
                same("atand", a, Libm::atand(a), astATand(a));
            }
        }
        for v in unit_range(200_000) {
            // SAFETY: pure functions of their arguments.
            unsafe {
                same("asind", v, Libm::asind(v), astASind(v));
                same("acosd", v, Libm::acosd(v), astACosd(v));
            }
        }
        let d = degrees(1000);
        let step = (d.len() / 1000).max(1);
        let thin: Vec<f64> = d.iter().copied().step_by(step).collect();
        for &y in &thin {
            for &x in &thin {
                // SAFETY: pure functions of their arguments.
                unsafe { same("atan2d", y, Libm::atan2d(y, x), astATan2d(y, x)) };
            }
        }
    }
}
