// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Branchless math kernels that a compiler vectorizes when they are inlined
//! into a loop over whole arrays.
//!
//! A kernel is valid on a stated domain, and each comes with a `*_covers`
//! predicate that is true exactly on that domain; a caller runs the kernel
//! over every element and then gives the elements its predicate rejects to
//! the scalar code the kernel replaces. Within its domain a kernel is within
//! 1 ULP of the platform's libm, or 2 ULP for `atan2` (its docs say why); a
//! result built from several kernels, such as
//! a product of a sine and a cosine, can be correspondingly further. Kernels never call libm, so their results
//! are the same on every platform.

mod atan2;
mod sincos;

pub(crate) use atan2::{atan2, atan2_covers};
pub(crate) use sincos::{sin_cos, sin_cos_covers};
