// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palDrange (pal/palDrange.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::{PAL__D2PI, PAL__DPI};

/// Normalizes an angle into the range -pi to +pi.
#[inline]
pub(crate) fn drange(angle: f64) -> f64 {
    // C's fmod is Rust's `%`.
    let mut result = angle % PAL__D2PI;
    if result > PAL__DPI {
        result -= PAL__D2PI;
    } else if result < -PAL__DPI {
        result += PAL__D2PI;
    }
    result
}
