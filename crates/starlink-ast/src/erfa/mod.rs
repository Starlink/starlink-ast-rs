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
//! loop over may also have a vectorizable kernel, within a stated bound of
//! the per-value form on a stated domain, which the slice entry point runs
//! over every element before giving the elements outside that domain to the
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
#[cfg(test)]
pub(crate) use s2c::{S2C_PRODUCT_ULPS, s2c_reference};
pub(crate) use s2c::{s2c, s2c_kernel, s2c_needs_special};

/// The sine and cosine of `x`, as an optimizing C compiler computes them for
/// C code that calls `sin(x)` and `cos(x)`; see [`crate::math::sincos_as_c_uncounted`].
#[inline(always)]
pub(super) fn sincos(x: f64) -> (f64, f64) {
    crate::math::sincos_as_c_uncounted(x)
}
