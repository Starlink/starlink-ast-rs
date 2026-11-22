// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The system clock, on the platforms that have one.
//!
//! WebAssembly for the browser (`wasm32-unknown-unknown`) has no clock the
//! standard library can read: `SystemTime::now` panics there, which traps
//! the module. Callers get `None` on that target and decide what to do.

use std::time::Duration;

/// The time since 1970-01-01 00:00:00 UTC by the system clock, or `None`
/// on a platform with no clock. A clock set before 1970 reads as zero.
pub(crate) fn since_unix_epoch() -> Option<Duration> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        None
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
        )
    }
}
