// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palPrenut (pal/palPrenut.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__MJD0;
use crate::erfa::{self, M3};

/// The combined precession and nutation matrix from the mean equator and
/// equinox of Julian epoch `epoch` to the true equator and equinox of the TT
/// date `date` (a Modified Julian Date), IAU 2006 precession and IAU 2000A
/// nutation.
pub(crate) fn prenut(epoch: f64, date: f64) -> M3 {
    // Specified Julian epoch as a 2-part JD.
    let (d1, d2) = erfa::epj2jd(epoch);

    // P matrix, from specified epoch to J2000.0.
    let [
        eps0,
        psia,
        oma,
        _bpa,
        _bqa,
        _pia,
        _bpia,
        _epsa,
        chia,
        _za,
        _zetaa,
        _thetaa,
        _pa,
        _gam,
        _phi,
        _psi,
    ] = erfa::p06e(d1, d2);
    let r1: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let r1 = erfa::rz(-chia, r1);
    let r1 = erfa::rx(oma, r1);
    let r1 = erfa::rz(psia, r1);
    let r1 = erfa::rx(-eps0, r1);

    // NPB matrix, from J2000.0 to date.
    let r2 = erfa::pnm06a(PAL__MJD0, date);

    // NPB matrix, from specified epoch to date.
    erfa::rxr(&r2, &r1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same3;
    use crate::test_inputs::{SplitMix64, specials};

    #[test]
    fn prenut_matches_vendored_c_bit_for_bit() {
        let mut epochs = vec![1000.0, 1900.0, 1950.0, 2000.0, 2100.0, 3000.0];
        let mut rng = SplitMix64::new(0x5eed_9e17);
        epochs.extend((0..200).map(|_| 1000.0 + rng.next_f64() * 2000.0));
        epochs.extend(specials());
        let mut dates: Vec<f64> = epochs
            .iter()
            .filter(|e| e.is_finite())
            .map(|e| 51544.5 + (e - 2000.0) * 365.25)
            .collect();
        dates.extend(specials());
        let mut pairs = Vec::new();
        for (k, &e) in epochs.iter().enumerate() {
            pairs.push((e, dates[k % dates.len()]));
            pairs.push((e, 51544.5));
            pairs.push((2000.0, dates[(k * 7 + 3) % dates.len()]));
        }
        for (e, d) in pairs {
            let mut c = [[0.0; 3]; 3];
            // SAFETY: nine outputs.
            unsafe { super::super::astPalPrenut(e, d, c.as_mut_ptr()) };
            let r = prenut(e, d);
            assert!((0..3).all(|i| same3(&r[i], &c[i])), "{e:e} {d:e}");
        }
    }
}
