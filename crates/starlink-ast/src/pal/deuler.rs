// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palDeuler (pal/palDeuler.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, M3};

/// The rotation matrix for up to three successive rotations about the axes
/// named by `order`.
///
/// Each of the first three bytes of `order` is `x`, `y` or `z` in either case,
/// or `1`, `2` or `3`, and pairs with the next of `phi`, `theta` and `psi`.
/// The first byte that is none of these (a NUL included) ends the sequence,
/// leaving the rotations so far applied.
pub(crate) fn deuler(order: &str, phi: f64, theta: f64, psi: f64) -> M3 {
    let mut rmat: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let rotations = [phi, theta, psi];
    for (&c, &angle) in order.as_bytes().iter().zip(&rotations) {
        rmat = match c {
            b'X' | b'x' | b'1' => erfa::rx(angle, rmat),
            b'Y' | b'y' | b'2' => erfa::ry(angle, rmat),
            b'Z' | b'z' | b'3' => erfa::rz(angle, rmat),
            _ => break,
        };
    }
    rmat
}
