// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palMappa (pal/palMappa.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::evp::evp;
use super::prenut::prenut;
use super::{PAL__CR, PAL__MJD0};
use crate::erfa;

/// The gravitational radius of the Sun times 2 (2 mu / c^2, au).
const GR2: f64 = 2.0 * 9.87063e-9;

/// The star-independent parameters for mean-to-apparent place
/// transformations from the mean equator and equinox of Julian epoch `eq` to
/// geocentric apparent place at the TDB date `date` (a Modified Julian
/// Date):
///
/// - 0: the time interval for proper motion (Julian years);
/// - 1 to 3: the barycentric position of the Earth (au);
/// - 4 to 6: the heliocentric direction of the Earth (unit vector);
/// - 7: the light deflection parameter, twice the Sun's gravitational
///   radius over the Earth-Sun distance;
/// - 8 to 10: the barycentric Earth velocity in units of c;
/// - 11: sqrt(1 - v^2), v being the magnitude of that velocity;
/// - 12 to 20: the precession and nutation matrix, row by row.
pub(crate) fn mappa(eq: f64, date: f64) -> [f64; 21] {
    let mut amprms = [0.0; 21];

    // Time interval for proper motion correction.
    amprms[0] = erfa::epj(PAL__MJD0, date) - eq;

    // Get Earth barycentric and heliocentric position and velocity.
    let (ebd, dpb, _ehd, eh) = evp(date, eq);
    amprms[1..4].copy_from_slice(&dpb);

    // Heliocentric direction of Earth (normalized) and modulus.
    let (e, ehn) = erfa::pn(&eh);
    amprms[4..7].copy_from_slice(&ehn);

    // Light deflection parameter.
    amprms[7] = GR2 / e;

    // Aberration parameters.
    for i in 0..3 {
        amprms[i + 8] = ebd[i] * PAL__CR;
    }
    let (vm, _vn) = erfa::pn(&[amprms[8], amprms[9], amprms[10]]);
    amprms[11] = (1.0 - vm * vm).sqrt();

    // NPB matrix.
    let npb = prenut(eq, date);
    for (i, row) in npb.iter().enumerate() {
        amprms[12 + 3 * i..15 + 3 * i].copy_from_slice(row);
    }
    amprms
}

/// The precession and nutation matrix held in `amprms[12..21]`.
pub(super) fn npb_matrix(amprms: &[f64; 21]) -> erfa::M3 {
    [
        [amprms[12], amprms[13], amprms[14]],
        [amprms[15], amprms[16], amprms[17]],
        [amprms[18], amprms[19], amprms[20]],
    ]
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::constants::BAD_VALUE;
    use crate::erfa::difftest::same;
    use crate::pal::evp::tests::{date_equinox_pairs, mjd};

    /// C's parameters at five epochs from 1000 to 3000, with equinoxes 1950
    /// and 2000, and at a date that is no date.
    pub(in crate::pal) fn c_amprms() -> Vec<[f64; 21]> {
        [
            (2000.0, mjd(1000.0)),
            (1950.0, mjd(1900.0)),
            (2000.0, mjd(2000.0)),
            (2000.0, mjd(2100.0)),
            (1950.0, mjd(3000.0)),
            (2000.0, BAD_VALUE),
        ]
        .iter()
        .map(|&(eq, date)| {
            let mut c = [0.0; 21];
            // SAFETY: 21 outputs.
            unsafe { super::super::astPalMappa(eq, date, c.as_mut_ptr()) };
            c
        })
        .collect()
    }

    #[test]
    fn mappa_matches_vendored_c_bit_for_bit() {
        for (date, eq) in date_equinox_pairs() {
            let mut c = [0.0; 21];
            // SAFETY: 21 outputs.
            unsafe { super::super::astPalMappa(eq, date, c.as_mut_ptr()) };
            let r = mappa(eq, date);
            assert!(
                (0..21).all(|k| same(r[k], c[k])),
                "{eq:e} {date:e}: {r:?} vs C {c:?}"
            );
        }
    }
}
