// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Transform benchmarking for the AST port.
//!
//! The Rust engine is always present. The reference C library is an optional
//! second engine behind the `c-oracle` feature, used while Rust is being
//! brought to parity with C; regression checking needs only Rust.
pub mod base;
pub mod bits;
#[cfg(feature = "c-oracle")]
pub mod c_engine;
pub mod cases;
pub mod compare;
pub mod fingerprint;
pub mod load;
pub mod oracle;
pub mod points;
pub mod report;
pub mod roman;
pub mod run;
pub mod snapshot;
pub mod sweep;
pub mod timing;
