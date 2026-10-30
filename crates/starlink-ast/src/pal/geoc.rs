// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palGeoc (pal/palGeoc.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, Ellipsoid};

/// Geodetic latitude and height to the distance from the Earth's axis and
/// from the equatorial plane, in au (WGS84 ellipsoid, zero longitude).
///
/// The geocentric latitude is `atan2(z, r)`.
pub(crate) fn geoc(p: f64, h: f64) -> (f64, f64) {
    // Use zero longitude.
    let elong = 0.0;
    // This is not `ERFA_DAU`: PAL writes the au as 1.49597870E11 m.
    let au = 1.49597870E11;
    // ERFA's status is ignored, as in PAL; on failure ERFA zeroes the vector.
    let xyz = erfa::gd2gc(Ellipsoid::Wgs84, elong, p, h).unwrap_or([0.0; 3]);
    // PAL divides by `cos(elong)` for a longitude that is the constant 0.
    (xyz[0] / (au * f64::cos(elong)), xyz[2] / au)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::{geodetic_heights, geodetic_latitudes, same};

    #[test]
    fn geoc_matches_vendored_c_bit_for_bit() {
        let lats = geodetic_latitudes(3000);
        let heights = geodetic_heights();
        for &p in &lats {
            for &h in &heights {
                let (mut r, mut z) = (0.0, 0.0);
                // SAFETY: both outputs are one double.
                unsafe { super::super::astPalGeoc(p, h, &mut r, &mut z) };
                let (pr, pz) = geoc(p, h);
                assert!(
                    same(pr, r) && same(pz, z),
                    "{p:e} {h:e}: ({pr:e}, {pz:e}) vs C ({r:e}, {z:e})"
                );
            }
        }
    }
}
