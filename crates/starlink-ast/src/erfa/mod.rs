// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Rust ports of the ERFA functions the crate uses, replacing the vendored C.
//!
//! One file per ERFA function. Each carries ERFA's copyright and licence
//! notice, which its BSD terms require of redistributed source, and names
//! the C function and version it was ported from.
//!
//! The per-value form of each function keeps ERFA's operations in ERFA's
//! order, so one-off callers get ERFA's results. A function that callers
//! loop over may also have a vectorizable kernel, within 1 ULP of the
//! per-value form on a stated domain, which the slice entry point runs over
//! every element before giving the elements outside that domain to the
//! per-value form. A slice gives each element the same bits whatever the
//! slice's length, so a caller that sends every point through the slice,
//! even a single point, gets results that do not depend on how many points
//! are transformed together.
//!
//! Calls made inside these functions are past the boundary where the
//! `call-counts` feature counts calls, as the C library's counts stop at its
//! calls into ERFA, so they use the `f64` methods directly rather than the
//! counted wrappers in `crate::math`.
#![allow(clippy::disallowed_methods)]

mod c2s;
mod s2c;

pub(crate) use c2s::{c2s, c2s_kernel, c2s_needs_special};
pub(crate) use s2c::{s2c, s2c_kernel, s2c_needs_special};

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
pub(super) fn sincos(x: f64) -> (f64, f64) {
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
