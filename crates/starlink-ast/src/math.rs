// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The floating-point library functions `starlink-ast` calls, routed through one
//! place so the `call-counts` feature can count them by their C names. The
//! bare `f64` methods are banned elsewhere in this crate by `clippy.toml`.
//!
//! Each function is the `f64` method it wraps, so results are bit-identical.
#![allow(clippy::disallowed_methods)]
// One wrapper per method `clippy.toml` bans, whether or not the library
// calls it today, so a new call site always has its counted form.
#![allow(dead_code)]

use crate::call_counts::hit;

#[inline(always)]
pub(crate) fn sin(x: f64) -> f64 {
    hit("sin");
    x.sin()
}

#[inline(always)]
pub(crate) fn cos(x: f64) -> f64 {
    hit("cos");
    x.cos()
}

#[inline(always)]
pub(crate) fn tan(x: f64) -> f64 {
    hit("tan");
    x.tan()
}

#[inline(always)]
pub(crate) fn asin(x: f64) -> f64 {
    hit("asin");
    x.asin()
}

#[inline(always)]
pub(crate) fn acos(x: f64) -> f64 {
    hit("acos");
    x.acos()
}

#[inline(always)]
pub(crate) fn atan(x: f64) -> f64 {
    hit("atan");
    x.atan()
}

#[inline(always)]
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    hit("atan2");
    y.atan2(x)
}

#[inline(always)]
pub(crate) fn exp(x: f64) -> f64 {
    hit("exp");
    x.exp()
}

#[inline(always)]
pub(crate) fn ln(x: f64) -> f64 {
    hit("log");
    x.ln()
}

#[inline(always)]
pub(crate) fn log10(x: f64) -> f64 {
    hit("log10");
    x.log10()
}

#[inline(always)]
pub(crate) fn powf(x: f64, y: f64) -> f64 {
    hit("pow");
    x.powf(y)
}

#[inline(always)]
pub(crate) fn powi(x: f64, n: i32) -> f64 {
    hit("powi");
    x.powi(n)
}

#[inline(always)]
pub(crate) fn sqrt(x: f64) -> f64 {
    hit("sqrt");
    x.sqrt()
}

/// The square root of `x`, counted as a call only when `counted`: for a
/// branchless kernel that takes a root C skips and then discards it.
#[inline(always)]
pub(crate) fn sqrt_counted_if(x: f64, counted: bool) -> f64 {
    if counted {
        hit("sqrt");
    }
    x.sqrt()
}

#[inline(always)]
pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    hit("hypot");
    x.hypot(y)
}

#[inline(always)]
pub(crate) fn sinh(x: f64) -> f64 {
    hit("sinh");
    x.sinh()
}

#[inline(always)]
pub(crate) fn cosh(x: f64) -> f64 {
    hit("cosh");
    x.cosh()
}

#[inline(always)]
pub(crate) fn tanh(x: f64) -> f64 {
    hit("tanh");
    x.tanh()
}

/// Both `sin` and `cos`, counted as one call of each as C makes them.
#[inline(always)]
pub(crate) fn sin_cos(x: f64) -> (f64, f64) {
    hit("sin");
    hit("cos");
    x.sin_cos()
}

/// The sine and cosine of `x`, as an optimizing C compiler computes them for
/// C code that calls `sin(x)` and `cos(x)`.
///
/// Optimizing compilers turn the pair into the platform's combined function,
/// which can differ from the separate calls in the last bit: clang into
/// `__sincos_stret` on Apple platforms, GCC into glibc's `sincos` on Linux.
/// Calling it explicitly matches the C library built with those compilers,
/// and makes the result the same in every build profile, where leaving it to
/// LLVM would give different bits in debug and release builds (and LLVM does
/// not combine them at all on GNU platforms, where `sin` and `cos` set
/// `errno`). Elsewhere the separate calls are used.
#[inline(always)]
pub(crate) fn sincos_as_c_uncounted(x: f64) -> (f64, f64) {
    #[cfg(target_vendor = "apple")]
    {
        #[repr(C)]
        struct Double2 {
            sin: f64,
            cos: f64,
        }
        unsafe extern "C" {
            fn __sincos_stret(x: f64) -> Double2;
        }
        // SAFETY: a pure libm function of one double, returning both values.
        let r = unsafe { __sincos_stret(x) };
        (r.sin, r.cos)
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn sincos(x: f64, s: *mut f64, c: *mut f64);
        }
        let (mut s, mut c) = (0.0, 0.0);
        // SAFETY: a pure libm function writing one double through each
        // pointer, both valid for the call.
        unsafe { sincos(x, &mut s, &mut c) };
        (s, c)
    }
    #[cfg(not(any(target_vendor = "apple", all(target_os = "linux", target_env = "gnu"))))]
    {
        (x.sin(), x.cos())
    }
}

/// [`sincos_as_c_uncounted`], counted as one call of each of `sin` and `cos`
/// as C makes them.
#[inline(always)]
pub(crate) fn sincos_as_c(x: f64) -> (f64, f64) {
    hit("sin");
    hit("cos");
    sincos_as_c_uncounted(x)
}

#[cfg(all(test, feature = "call-counts"))]
mod tests {
    #[test]
    fn counts_by_c_name() {
        crate::call_counts::reset();
        let _ = super::sin(1.0) + super::ln(2.0) + super::powf(2.0, 0.5);
        let _ = super::sin_cos(0.5);
        let c = crate::call_counts::snapshot();
        assert_eq!(c.get("sin"), Some(&2));
        assert_eq!(c.get("cos"), Some(&1));
        assert_eq!(c.get("log"), Some(&1));
        assert_eq!(c.get("pow"), Some(&1));
    }
}
