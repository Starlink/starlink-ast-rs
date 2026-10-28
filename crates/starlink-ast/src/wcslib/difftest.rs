// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The vendored wcslib functions the Rust port is checked against, bit for
//! bit.

unsafe extern "C" {
    pub(super) fn astCosd(angle: f64) -> f64;
    pub(super) fn astSind(angle: f64) -> f64;
    pub(super) fn astTand(angle: f64) -> f64;
    pub(super) fn astACosd(v: f64) -> f64;
    pub(super) fn astASind(v: f64) -> f64;
    pub(super) fn astATand(v: f64) -> f64;
    pub(super) fn astATan2d(y: f64, x: f64) -> f64;
}
