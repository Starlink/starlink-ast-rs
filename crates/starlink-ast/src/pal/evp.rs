// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palEvp (pal/palEvp.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::{PAL__MJD0, PAL__SPD};
use crate::erfa::{self, V3};

/// The Earth's barycentric velocity (au per second) and position (au), then
/// its heliocentric velocity and position, at the TDB date `date` (a
/// Modified Julian Date), with respect to the mean equator and equinox of
/// Julian epoch `deqx`, or to the BCRS when `deqx` is zero or negative.
///
/// Returns `(dvb, dpb, dvh, dph)`.
pub(crate) fn evp(date: f64, deqx: f64) -> (V3, V3, V3, V3) {
    // BCRS PV-vectors; the date-range warning is ignored.
    let ((mut pvh, mut pvb), _) = erfa::epv00(PAL__MJD0, date);

    // Was precession to another equinox requested?
    if deqx > 0.0 {
        // Yes: compute precession matrix from J2000.0 to deqx.
        let (d1, d2) = erfa::epj2jd(deqx);
        let r = erfa::pmat06(d1, d2);

        // Rotate the PV-vectors.
        pvh = erfa::rxpv(&r, &pvh);
        pvb = erfa::rxpv(&r, &pvb);
    }

    // Return the required vectors.
    let mut dvb = [0.0; 3];
    let mut dpb = [0.0; 3];
    let mut dvh = [0.0; 3];
    let mut dph = [0.0; 3];
    for i in 0..3 {
        dvh[i] = pvh[1][i] / PAL__SPD;
        dvb[i] = pvb[1][i] / PAL__SPD;
        dph[i] = pvh[0][i];
        dpb[i] = pvb[0][i];
    }
    (dvb, dpb, dvh, dph)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::constants::BAD_VALUE;
    use crate::erfa::difftest::same3;
    use crate::test_inputs::{SplitMix64, specials};

    /// The MJD at the start of Julian epoch `e`.
    pub(in crate::pal) fn mjd(e: f64) -> f64 {
        51544.5 + (e - 2000.0) * 365.25
    }

    /// Dates (MJD) for epochs 1000 to 3000, including those outside 1900 to
    /// 2100 where the ephemeris warns, paired with equinoxes 1950, 2000,
    /// random ones, ones far from J2000, and values that ask for no
    /// precession or are no epoch; then the special dates.
    pub(in crate::pal) fn date_equinox_pairs() -> Vec<(f64, f64)> {
        let mut rng = SplitMix64::new(0x5eed_e7b0);
        let mut epochs = vec![1000.0, 1900.0, 1950.0, 2000.0, 2100.0, 3000.0];
        epochs.extend((0..60).map(|_| 1000.0 + rng.next_f64() * 2000.0));
        let mut dates: Vec<f64> = epochs.iter().map(|&e| mjd(e)).collect();
        dates.extend(specials());
        let mut equinoxes = vec![1950.0, 2000.0, 1000.0, 3000.0, 0.0, -0.0, -1.0];
        equinoxes.extend((0..4).map(|_| 1800.0 + rng.next_f64() * 400.0));
        equinoxes.extend([BAD_VALUE, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE]);
        let mut out = Vec::new();
        for &d in &dates {
            for &e in &equinoxes {
                out.push((d, e));
            }
        }
        out
    }

    #[test]
    fn evp_matches_vendored_c_bit_for_bit() {
        for (date, deqx) in date_equinox_pairs() {
            let mut c = [[0.0; 3]; 4];
            // SAFETY: four 3-vector outputs.
            unsafe {
                super::super::astPalEvp(
                    date,
                    deqx,
                    c[0].as_mut_ptr(),
                    c[1].as_mut_ptr(),
                    c[2].as_mut_ptr(),
                    c[3].as_mut_ptr(),
                )
            };
            let r = evp(date, deqx);
            assert!(
                same3(&r.0, &c[0])
                    && same3(&r.1, &c[1])
                    && same3(&r.2, &c[2])
                    && same3(&r.3, &c[3]),
                "{date:e} {deqx:e}: {r:?} vs C {c:?}"
            );
        }
    }
}
