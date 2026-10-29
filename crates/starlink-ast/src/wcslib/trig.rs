// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

use super::math;

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

#[cfg(test)]
mod tests {
    use super::*;

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
