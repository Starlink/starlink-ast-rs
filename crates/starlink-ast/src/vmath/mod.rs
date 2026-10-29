// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Branchless math kernels that a compiler vectorizes when they are inlined
//! into a loop over whole arrays.
//!
//! A kernel is valid on a stated domain, and each comes with a `*_covers`
//! predicate that is true exactly on that domain; a caller runs the kernel
//! over every element and then gives the elements its predicate rejects to
//! the scalar code the kernel replaces. Within its domain a kernel is within
//! 1 ULP of the exact result, rounded, which its tests check against a
//! reference computed in double-double arithmetic; a result built from
//! several kernels, such as a product of a sine and a cosine, can be
//! correspondingly further. Kernels never call libm, so their results are
//! the same on every platform. A platform's libm can itself be further from
//! the exact result than a kernel: macOS's on x86-64 is 2.65 ULP from the
//! sine at -6.452961993231616e5.

mod atan2;
mod sincos;

pub(crate) use atan2::{atan2, atan2_covers};

/// Whether this build may use fused multiply-adds everywhere: always on
/// aarch64, and on x86-64 only when built for a CPU that has them. The slice
/// entries that call the kernels pick a build of their loop for the CPU at
/// run time instead.
pub(crate) const HAS_FMA: bool = cfg!(any(target_arch = "aarch64", target_feature = "fma"));

/// Whether the x86-64 CPU running this has AVX2 and FMA, for the slice
/// entries that pick a build of their loop at run time. The standard library
/// caches the CPU query, so this is cheap to ask per call.
#[cfg(target_arch = "x86_64")]
#[inline]
pub(crate) fn x86_has_avx2_fma() -> bool {
    std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
}
pub(crate) use sincos::{sin_cos, sin_cos_covers};
