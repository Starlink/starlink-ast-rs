// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Raw C declarations of the vendored PAL and ERFA symbols (`astPal`- and
//! `astEra`-prefixed) that the wrappers in the parent module call.

use std::ffi::c_char;

unsafe extern "C" {
    // -- Epoch conversions --
    pub(super) fn astPalEpj(date: f64) -> f64;
    pub(super) fn astPalEpj2d(epj: f64) -> f64;
    pub(super) fn astPalEpb(date: f64) -> f64;
    pub(super) fn astPalEpb2d(epb: f64) -> f64;

    // -- Vector / matrix --
    pub(super) fn astPalDmxv(dm: *const [f64; 3], va: *const f64, vb: *mut f64);
    pub(super) fn astPalDimxv(dm: *const [f64; 3], va: *const f64, vb: *mut f64);
    pub(super) fn astPalDmxm(da: *const [f64; 3], db: *const [f64; 3], dc: *mut [f64; 3]);
    pub(super) fn astPalDvxv(va: *const f64, vb: *const f64, vc: *mut f64);
    pub(super) fn astPalDvdv(va: *const f64, vb: *const f64) -> f64;
    pub(super) fn astPalDvn(v: *const f64, uv: *mut f64, vm: *mut f64);
    pub(super) fn astPalDmat(
        n: i32,
        a: *mut f64,
        y: *mut f64,
        d: *mut f64,
        jf: *mut i32,
        iw: *mut i32,
    );

    // -- Coordinate conversions --
    #[cfg(test)]
    pub(super) fn astPalGaleq(dl: f64, db: f64, dr: *mut f64, dd: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalEqgal(dr: f64, dd: f64, dl: *mut f64, db: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalGalsup(dl: f64, db: f64, dsl: *mut f64, dsb: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalSupgal(dsl: f64, dsb: f64, dl: *mut f64, db: *mut f64);
    #[cfg(test)]
    pub(super) fn astEraFk45z(
        r1950: f64,
        d1950: f64,
        bepoch: f64,
        r2000: *mut f64,
        d2000: *mut f64,
    );
    #[cfg(test)]
    pub(super) fn astEraFk54z(
        r2000: f64,
        d2000: f64,
        bepoch: f64,
        r1950: *mut f64,
        d1950: *mut f64,
        dr1950: *mut f64,
        dd1950: *mut f64,
    );
    #[cfg(test)]
    pub(super) fn astPalFk5hz(r5: f64, d5: f64, epoch: f64, rh: *mut f64, dh: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalHfk5z(
        rh: f64,
        dh: f64,
        epoch: f64,
        r5: *mut f64,
        d5: *mut f64,
        dr5: *mut f64,
        dd5: *mut f64,
    );

    // -- Precession matrices --
    pub(super) fn astPalPrec(ep0: f64, ep1: f64, rmatp: *mut [f64; 3]);
    pub(super) fn astPalPrebn(bep0: f64, bep1: f64, rmatp: *mut [f64; 3]);
    pub(super) fn astPalEcmat(date: f64, rmat: *mut [f64; 3]);

    // -- Euler rotation --
    pub(super) fn astPalDeuler(
        order: *const c_char,
        phi: f64,
        theta: f64,
        psi: f64,
        rmat: *mut [f64; 3],
    );

    // -- Aberration --
    #[cfg(test)]
    pub(super) fn astPalEtrms(ep: f64, ev: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalAddet(rm: f64, dm: f64, eq: f64, rc: *mut f64, dc: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalSubet(rc: f64, dc: f64, eq: f64, rm: *mut f64, dm: *mut f64);
    pub(super) fn astPalMappa(eq: f64, date: f64, amprms: *mut f64);
    pub(super) fn astPalAmpqk(ra: f64, da: f64, amprms: *const f64, rm: *mut f64, dm: *mut f64);
    pub(super) fn astPalMapqkz(rm: f64, dm: f64, amprms: *const f64, ra: *mut f64, da: *mut f64);

    // -- Angular normalization --
    pub(super) fn astPalDrange(angle: f64) -> f64;
    pub(super) fn astPalDranrm(angle: f64) -> f64;
    pub(super) fn astPalDsep(al: f64, ap: f64, bl: f64, bp: f64) -> f64;
    pub(super) fn astPalDbear(a1: f64, b1: f64, a2: f64, b2: f64) -> f64;

    // -- Earth / observer --
    pub(super) fn astPalEvp(
        date: f64,
        deqx: f64,
        dvb: *mut f64,
        dpb: *mut f64,
        dvh: *mut f64,
        dph: *mut f64,
    );
    pub(super) fn astPalGeoc(p: f64, h: f64, r: *mut f64, z: *mut f64);

    // -- Miscellaneous --
    pub(super) fn astPalDav2m(axvec: *const f64, rmat: *mut [f64; 3]);

    // -- Time / sidereal --
    pub(super) fn astPalDat(dju: f64) -> f64;
    pub(super) fn astPalGmst(ut1: f64) -> f64;
    pub(super) fn astPalEqeqx(date: f64) -> f64;

    // -- Observer / velocity --
    pub(super) fn astPalPvobs(p: f64, h: f64, stl: f64, pv: *mut f64);
    #[cfg(test)]
    pub(super) fn astPalRvgalc(r2000: f64, d2000: f64) -> f64;
    #[cfg(test)]
    pub(super) fn astPalRvlg(r2000: f64, d2000: f64) -> f64;
    #[cfg(test)]
    pub(super) fn astPalRvlsrd(r2000: f64, d2000: f64) -> f64;
    #[cfg(test)]
    pub(super) fn astPalRvlsrk(r2000: f64, d2000: f64) -> f64;

    // -- Calendar / time-of-day conversions --
    pub(super) fn astPalDjcl(
        djm: f64,
        iy: *mut i32,
        im: *mut i32,
        id: *mut i32,
        fd: *mut f64,
        j: *mut i32,
    );
    pub(super) fn astPalCldj(iy: i32, im: i32, id: i32, djm: *mut f64, j: *mut i32);
    pub(super) fn astPalDd2tf(ndp: i32, days: f64, sign: *mut c_char, ihmsf: *mut i32);
    pub(super) fn astPalDtf2d(ihour: i32, imin: i32, sec: f64, days: *mut f64, j: *mut i32);
}
