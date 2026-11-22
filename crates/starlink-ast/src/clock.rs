// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The system clock.
//!
//! WebAssembly for the browser (`wasm32-unknown-unknown`) has no clock the
//! standard library can read: `SystemTime::now` panics there, which traps
//! the module. On that target the browser's clock is read through
//! JavaScript's `Date.now`, to the millisecond.

use std::time::Duration;

/// The time since 1970-01-01 00:00:00 UTC by the system clock. A clock set
/// before 1970 reads as zero.
pub(crate) fn since_unix_epoch() -> Duration {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        Duration::try_from_secs_f64(js_sys::Date::now() / 1000.0).unwrap_or_default()
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
    }
}
