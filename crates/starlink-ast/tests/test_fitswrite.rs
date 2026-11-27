// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Writing FrameSets that a FitsChan can describe only in part, or not at
//! all.
//!
//! Ancestor: `ast_tester/testfitswrite.c`. Each case is a FrameSet for
//! which a FitsChan write used to describe it wrongly, report an error, or
//! read or write memory it should not have. The case names are C's.
//!
//! Divergences from the C test:
//! - Where C accepts any return value from the write, the case requires the
//!   value the C library returns at the pinned commit.
//! - Each write runs under `catch_unwind`, so a panic fails its case.

use starlink_ast::attributes::Attributes;
use starlink_ast::frames::{
    BasicFrameBuilder, CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkySystem,
};
use starlink_ast::mappings::{
    CmpMap, Composition, DynMapping, LutMap, Mapping, PermMap, PolyMap, PolyTerm, ShiftMap,
    UnitMap, WinMap, ZoomMap,
};
use starlink_ast::{AnyObject, Channel, Encoding, FitsChan};

fn frame(n: usize, attrs: &str) -> Box<dyn Frame> {
    let mut f = BasicFrameBuilder::new(n).build().unwrap();
    if !attrs.is_empty() {
        f.set_attributes(attrs).unwrap();
    }
    Box::new(f)
}

/// C's `read_cards`: a FrameSet read from a FITS-WCS header.
fn read_cards(cards: &[&str]) -> FrameSet {
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    for c in cards {
        fc.put_fits(c, false).unwrap();
    }
    fc.clear_card().unwrap();
    match fc.read().unwrap() {
        Some(AnyObject::FrameSet(fs)) => fs,
        _ => panic!("no FrameSet"),
    }
}

/// A 100 by 200 pixel image of the sky with a gnomonic projection.
fn tan_frameset() -> FrameSet {
    read_cards(&[
        "NAXIS   = 2",
        "NAXIS1  = 100",
        "NAXIS2  = 200",
        "CRPIX1  = 50",
        "CRPIX2  = 100",
        "CRVAL1  = 10",
        "CRVAL2  = 20",
        "CTYPE1  = 'RA---TAN'",
        "CTYPE2  = 'DEC--TAN'",
        "CDELT1  = -0.001",
        "CDELT2  = 0.001",
    ])
}

fn fmap(fs: &FrameSet) -> DynMapping {
    fs.mapping(FrameRef::Base, FrameRef::Current).unwrap()
}

fn base(fs: &FrameSet) -> Box<dyn Frame> {
    fs.frame(FrameRef::Base).unwrap().snapshot()
}

fn current(fs: &FrameSet) -> Box<dyn Frame> {
    fs.frame(FrameRef::Current).unwrap().snapshot()
}

fn fitschan(settings: &str, cards: &[&str]) -> FitsChan {
    let mut fc = FitsChan::new().unwrap();
    fc.set_attributes(settings).unwrap();
    for c in cards {
        fc.put_fits(c, false).unwrap();
    }
    fc
}

fn two_frame(base: Box<dyn Frame>, map: DynMapping, cur: Box<dyn Frame>) -> FrameSet {
    let mut fs = FrameSet::new(base);
    fs.add_frame(FrameRef::Base, map, cur).unwrap();
    fs
}

/// C's `check_write`: the write reports no error and returns `expect`.
fn check_write(fc: &mut FitsChan, fs: FrameSet, expect: usize) -> Result<(), String> {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fc.write(&AnyObject::FrameSet(fs))
    }));
    match r {
        Err(_) => Err("the write panicked".to_string()),
        Ok(Err(e)) => Err(format!(
            "the write reported {:?}: {}",
            e.code(),
            e.message()
        )),
        Ok(Ok(n)) if n != expect => Err(format!("the write returned {n}, not {expect}")),
        Ok(Ok(_)) => Ok(()),
    }
}

/// C's `check_card`: `name` is present or absent and, if `value` is given,
/// its value starts with it.
fn check_card(
    fc: &mut FitsChan,
    name: &str,
    present: bool,
    value: Option<&str>,
) -> Result<(), String> {
    fc.clear_card().unwrap();
    let card = fc.find_fits(name, false).unwrap_or(None);
    match (card, present) {
        (Some(c), false) => Err(format!("unexpected card \"{c}\"")),
        (None, true) => Err(format!("no {name} card")),
        (Some(c), true) if value.is_some_and(|v| !c[10..].starts_with(v)) => Err(format!(
            "card \"{c}\" does not have a value starting {}",
            value.unwrap()
        )),
        _ => Ok(()),
    }
}

fn list_cards(fc: &mut FitsChan) -> Vec<String> {
    fc.clear_card().unwrap();
    let mut v = Vec::new();
    while let Some(c) = fc.find_fits("%f", true).unwrap() {
        v.push(c);
    }
    v
}

const LUT1: [f64; 5] = [0.0, 0.001, 0.004, 0.009, 0.016];
const LUT2: [f64; 5] = [0.0, 1.0, 4.0, 9.0, 16.0];
const LUT3: [f64; 3] = [0.0, 1.0, 0.0];
const LUT4: [f64; 5] = [-1.0, 0.0, 1.0, 8.0, 27.0];
const COEFFS1: [f64; 12] = [
    2.0, 1.0, 1.0, 0.0, 1.0e-6, 1.0, 2.0, 0.0, 2.0, 2.0, 0.0, 1.0,
];
const COEFFS2: [f64; 12] = [
    1.0, 1.0, 1.0, 0.0, 1.0e-3, 1.0, 2.0, 0.0, 1.0, 2.0, 0.0, 1.0,
];
const SIN_CARDS: [&str; 21] = [
    "NAXIS   = 2",
    "NAXIS1  = 100",
    "NAXIS2  = 200",
    "CRPIX1  = 50",
    "CRPIX2  = 100",
    "CRVAL1  = 10",
    "CRVAL2  = 20",
    "CTYPE1  = 'RA---TAN'",
    "CTYPE2  = 'DEC--TAN'",
    "CDELT1  = -0.001",
    "CDELT2  = 0.001",
    "CRPIX1A = 50",
    "CRPIX2A = 100",
    "CRVAL1A = 10",
    "CRVAL2A = 20",
    "CTYPE1A = 'RA---SIN'",
    "CTYPE2A = 'DEC--SIN'",
    "CDELT1A = -0.001",
    "CDELT2A = 0.001",
    "PV2_1A  = 0.1",
    "PV2_2A  = 0.2",
];
const TPN_CARDS: [&str; 13] = [
    "NAXIS   = 2",
    "NAXIS1  = 100",
    "NAXIS2  = 200",
    "CRPIX1  = 50",
    "CRPIX2  = 100",
    "CRVAL1  = 10",
    "CRVAL2  = 20",
    "CTYPE1  = 'RA---TPN'",
    "CTYPE2  = 'DEC--TPN'",
    "CDELT1  = -0.001",
    "CDELT2  = 0.001",
    "PV1_1   = 1.0",
    "PV2_1   = 1.0",
];

fn lut(v: &[f64]) -> DynMapping {
    Box::new(LutMap::new(v, 1.0, 1.0).unwrap())
}

fn series(a: DynMapping, b: DynMapping) -> DynMapping {
    Box::new(CmpMap::new(a, b, Composition::Series).unwrap())
}

fn parallel(a: DynMapping, b: DynMapping) -> DynMapping {
    Box::new(CmpMap::new(a, b, Composition::Parallel).unwrap())
}

/// A PolyMap of two inputs and outputs from C's coefficient layout
/// (coefficient, output, power of each input), with an iterative inverse.
fn poly(c: &[f64]) -> DynMapping {
    let terms: Vec<PolyTerm> = c
        .chunks(4)
        .map(|t| PolyTerm {
            coeff: t[0],
            out_axis: t[1] as usize,
            powers: vec![t[2] as usize, t[3] as usize],
        })
        .collect();
    let mut p = PolyMap::new(2, 2, &terms, None).unwrap();
    p.set_iter_inverse(true).unwrap();
    Box::new(p)
}

/// C's `poly_sky`: the polynomial about the reference pixel of `fs`, then
/// its sky Mapping.
fn poly_sky(fs: &FrameSet, c: &[f64]) -> DynMapping {
    series(
        series(
            Box::new(ShiftMap::new(vec![-50.0, -100.0]).unwrap()),
            poly(c),
        ),
        series(
            Box::new(ShiftMap::new(vec![50.0, 100.0]).unwrap()),
            fmap(fs),
        ),
    )
}

/// A primary description with no celestial axes and a secondary one that
/// is a sky coordinate system.
fn sky_and_prim() -> FrameSet {
    let sky = tan_frameset();
    let mut fs = FrameSet::new(base(&sky));
    fs.add_frame(FrameRef::Base, fmap(&sky), current(&sky))
        .unwrap();
    fs.add_frame(
        FrameRef::Base,
        Box::new(UnitMap::new(2).unwrap()),
        frame(2, "Domain=PRIM"),
    )
    .unwrap();
    fs
}

fn offset_sky() -> FrameSet {
    let mut fs = tan_frameset();
    fs.set_attribute("SkyRef", "0:40:00, 20:00:00").unwrap();
    fs.set_attribute("SkyRefIs", "Origin").unwrap();
    fs
}

type Case = fn() -> Result<(), String>;

/// Every case of the C test, by C's description.
const CASES: &[(&str, Case)] = &[
    ("a CmpMap with an Ident", || {
        let fs = tan_frameset();
        let mut map = CmpMap::new(
            fmap(&fs),
            Box::new(UnitMap::new(2).unwrap()),
            Composition::Series,
        )
        .unwrap();
        map.set_ident("X");
        let fs2 = two_frame(base(&fs), Box::new(map), current(&fs));
        check_write(&mut fitschan("Encoding=FITS-WCS", &[]), fs2, 0)
    }),
    ("sky axes tabulated against two pixel axes each", || {
        let pm = PermMap::new(&[1, 1, 2, 2], &[1, 3], &[]).unwrap();
        let map = series(Box::new(pm), parallel(lut(&LUT1), lut(&LUT1)));
        let fs = two_frame(
            frame(4, "Domain=GRID"),
            map,
            Box::new(SkyFrame::new(SkySystem::Icrs)),
        );
        let mut fc = fitschan(
            "Encoding=FITS-WCS,TabOK=1,FitsAxisOrder=<copy>",
            &["NAXIS1  = 5", "NAXIS2  = 5", "NAXIS3  = 5", "NAXIS4  = 5"],
        );
        check_write(&mut fc, fs, 0)
    }),
    ("an axis tabulated against two pixel axes", || {
        let pm = PermMap::new(&[1, 1, 2], &[1, 3], &[]).unwrap();
        let map = series(
            Box::new(pm),
            parallel(lut(&LUT2), Box::new(UnitMap::new(1).unwrap())),
        );
        let fs = two_frame(frame(3, "Domain=GRID"), map, frame(2, "Domain=WORLD"));
        let mut fc = fitschan(
            "Encoding=FITS-WCS,TabOK=1,FitsAxisOrder=<copy>",
            &["NAXIS1  = 5", "NAXIS2  = 5", "NAXIS3  = 5"],
        );
        check_write(&mut fc, fs, 0)
    }),
    ("two axes that depend on one pixel axis", || {
        let pm = PermMap::new(&[-1, -1, 1], &[3, 3], &[1.0]).unwrap();
        let map = series(
            Box::new(pm),
            parallel(lut(&LUT3), Box::new(UnitMap::new(1).unwrap())),
        );
        let fs = two_frame(frame(3, "Domain=GRID"), map, frame(2, "Domain=WORLD"));
        check_write(&mut fitschan("Encoding=FITS-WCS,TabOK=1", &[]), fs, 0)
    }),
    ("an axis that is not linear", || {
        let fs = two_frame(
            frame(1, "Domain=GRID"),
            lut(&LUT4),
            frame(1, "Domain=WORLD"),
        );
        check_write(&mut fitschan("Encoding=FITS-WCS", &["NAXIS1  = 5"]), fs, 0)
    }),
    ("a polynomial that ends with a linear term", || {
        let fs = tan_frameset();
        let fs2 = two_frame(base(&fs), series(poly(&COEFFS1), fmap(&fs)), current(&fs));
        let mut fc = fitschan("Encoding=FITS-WCS", &["NAXIS1  = 100", "NAXIS2  = 200"]);
        check_write(&mut fc, fs2, 1)
    }),
    ("distorted sky axes and a third axis", || {
        let fs = tan_frameset();
        let map = parallel(poly_sky(&fs, &COEFFS2), Box::new(UnitMap::new(1).unwrap()));
        let cur = CmpFrame::new(current(&fs), frame(1, "Domain=OTHER")).unwrap();
        let fs2 = two_frame(frame(3, "Domain=GRID"), map, Box::new(cur));
        let mut fc = fitschan(
            "Encoding=FITS-WCS",
            &["NAXIS1  = 100", "NAXIS2  = 200", "NAXIS3  = 10"],
        );
        check_write(&mut fc, fs2, 1)?;
        check_card(&mut fc, "A_ORDER", true, None)?;
        check_card(&mut fc, "CTYPE1", true, Some("'RA---TAN-SIP'"))
    }),
    (
        "distorted sky axes and two axes that share a pixel axis",
        || {
            let fs = tan_frameset();
            let pm = PermMap::new(&[1, 2, 3, -1], &[1, 2, 3, 3], &[1.0]).unwrap();
            let map = series(
                Box::new(pm),
                parallel(poly_sky(&fs, &COEFFS2), Box::new(UnitMap::new(2).unwrap())),
            );
            let cur = CmpFrame::new(current(&fs), frame(2, "Domain=OTHER")).unwrap();
            let fs2 = two_frame(frame(4, "Domain=GRID"), map, Box::new(cur));
            let mut fc = fitschan(
                "Encoding=FITS-WCS",
                &[
                    "NAXIS1  = 100",
                    "NAXIS2  = 200",
                    "NAXIS3  = 10",
                    "NAXIS4  = 1",
                ],
            );
            check_write(&mut fc, fs2, 0)
        },
    ),
    ("more Frames than version letters", || {
        let mut fs = tan_frameset();
        let icur = fs.current_index();
        for i in 0..27 {
            fs.add_frame(
                FrameRef::Base,
                Box::new(ZoomMap::new(2, 2.0 + f64::from(i)).unwrap()),
                frame(2, "Domain=EXTRA"),
            )
            .unwrap();
        }
        fs.set_current(FrameRef::Index(icur)).unwrap();
        let mut fc = fitschan("Encoding=FITS-WCS", &[]);
        check_write(&mut fc, fs, 1)?;
        check_card(&mut fc, "CDELT1A", true, Some("                 2.0"))?;
        check_card(&mut fc, "CDELT1Z", true, Some("                27.0"))
    }),
    ("an offset sky system and an Ident", || {
        let mut fs = offset_sky();
        let icur = fs.current_index();
        fs.add_frame(
            FrameRef::Base,
            Box::new(ZoomMap::new(2, 3.0).unwrap()),
            frame(2, "Domain=FOO,Ident=A"),
        )
        .unwrap();
        fs.set_current(FrameRef::Index(icur)).unwrap();
        let mut fc = fitschan("Encoding=FITS-WCS", &[]);
        check_write(&mut fc, fs, 1)?;
        check_card(&mut fc, "CTYPE1A", true, Some("'FOO1"))?;
        check_card(&mut fc, "RADESYSA", false, None)?;
        check_card(&mut fc, "CTYPE1B", true, Some("'RA---TAN'"))
    }),
    ("a secondary sky system with no primary one", || {
        let mut fc = fitschan("Encoding=FITS-PC", &["NAXIS1  = 100", "NAXIS2  = 200"]);
        check_write(&mut fc, sky_and_prim(), 1)?;
        check_card(&mut fc, "C1YPE1", false, None)
    }),
    (
        "secondary projection parameters with no primary ones",
        || {
            let mut fc = fitschan("Encoding=FITS-PC", &["NAXIS1  = 100", "NAXIS2  = 200"]);
            check_write(&mut fc, read_cards(&SIN_CARDS), 1)?;
            check_card(&mut fc, "C1YPE1", false, None)
        },
    ),
    ("a secondary description with an extra axis", || {
        let mut fs = FrameSet::new(frame(2, "Domain=GRID"));
        fs.add_frame(
            FrameRef::Base,
            Box::new(PermMap::new(&[1, 2], &[1, 2, -1], &[4.0]).unwrap()),
            frame(3, "Domain=ALT"),
        )
        .unwrap();
        fs.add_frame(
            FrameRef::Base,
            Box::new(UnitMap::new(2).unwrap()),
            frame(2, "Domain=PRIM"),
        )
        .unwrap();
        let mut fc = fitschan("Encoding=FITS-PC", &["NAXIS1  = 10", "NAXIS2  = 10"]);
        check_write(&mut fc, fs, 1)?;
        check_card(&mut fc, "C1YPE1", false, None)
    }),
    ("a four axis cube with one spatial pixel", || {
        let sky = read_cards(&[
            "NAXIS   = 2",
            "CRPIX1  = 5",
            "CRPIX2  = 5",
            "CRVAL1  = 10",
            "CRVAL2  = 20",
            "CDELT1  = -0.001",
            "CDELT2  = 0.001",
            "CTYPE1  = 'RA---TAN'",
            "CTYPE2  = 'DEC--TAN'",
            "RADESYS = 'FK5'",
            "EQUINOX = 2000",
        ]);
        let freq = frame(1, "Symbol(1)=FREQ,Unit(1)=Hz,Label(1)=Frequency");
        let win = WinMap::from_corners(&[0.0], &[1.0], &[1.0e11], &[1.0e11 + 1.0e6]).unwrap();
        let map = parallel(
            parallel(Box::new(win), fmap(&sky)),
            Box::new(UnitMap::new(1).unwrap()),
        );
        let wcs = CmpFrame::new(
            Box::new(CmpFrame::new(freq, current(&sky)).unwrap()),
            frame(1, "Symbol(1)=STOKES"),
        )
        .unwrap();
        let fs = two_frame(frame(4, "Domain=GRID"), map, Box::new(wcs));
        let mut fc = fitschan(
            "Encoding=FITS-CLASS",
            &[
                "NAXIS   = 4",
                "NAXIS1  = 10",
                "NAXIS2  = 1",
                "NAXIS3  = 1",
                "NAXIS4  = 1",
            ],
        );
        check_write(&mut fc, fs, 0)
    }),
    ("a long Symbol for an offset sky axis", || {
        let mut fs = offset_sky();
        fs.set_attribute(
            "Symbol(1)",
            "AVERYLONGSYMBOLNAMEAVERYLONGSYMBOLNAMEAVERYLONGSYMBOLNAMEAVERYLONGSYMBOLNAME\
             AVERYLONGSYMBOLNAME",
        )
        .unwrap();
        let mut fc = fitschan("Encoding=FITS-WCS", &[]);
        check_write(&mut fc, fs, 1)?;
        check_card(&mut fc, "CTYPE1", true, Some("'OFLN-TAN'"))
    }),
    ("a Domain that is nearly the axis Symbol", || {
        let fs = two_frame(
            frame(1, "Domain=GRID"),
            Box::new(UnitMap::new(1).unwrap()),
            frame(1, "Symbol(1)=ABCD,Domain=ABCX"),
        );
        let mut fc = fitschan("Encoding=FITS-WCS", &[]);
        check_write(&mut fc, fs, 1)?;
        check_card(&mut fc, "WCSNAME", true, Some("'ABCX"))
    }),
    ("a Domain that is the axis Symbol", || {
        let fs = two_frame(
            frame(1, "Domain=GRID"),
            Box::new(UnitMap::new(1).unwrap()),
            frame(1, "Symbol(1)=ABCD,Domain=abcd"),
        );
        let mut fc = fitschan("Encoding=FITS-WCS", &[]);
        check_write(&mut fc, fs, 1)?;
        check_card(&mut fc, "WCSNAME", false, None)
    }),
    ("a blank Symbol over a blank CTYPE", || {
        let mut f = BasicFrameBuilder::new(1).build().unwrap();
        f.set_attribute("Symbol(1)", " ").unwrap();
        let fs = two_frame(
            frame(1, "Domain=GRID"),
            Box::new(UnitMap::new(1).unwrap()),
            Box::new(f),
        );
        let mut fc = fitschan(
            "Encoding=FITS-WCS",
            &["CTYPE1  = ' '                  / old"],
        );
        check_write(&mut fc, fs, 1)
    }),
    ("a projection that FITS-PC cannot describe", || {
        let mut fc = fitschan(
            "Encoding=FITS-PC",
            &[
                "NAXIS1  = 100",
                "CRVAL1  = 5 / mine",
                "NAXIS2  = 200",
                "CRPIX2  = 7.5 / also mine",
            ],
        );
        let before = list_cards(&mut fc);
        fc.clear_card().unwrap();
        check_write(&mut fc, read_cards(&TPN_CARDS), 0)?;
        let after = list_cards(&mut fc);
        if after != before {
            return Err(format!(
                "the failed write left {after:?} where there were {before:?}"
            ));
        }
        Ok(())
    }),
    (
        "a secondary description that FITS-PC cannot describe",
        || {
            let mut fc = fitschan(
                "Encoding=FITS-PC",
                &["NAXIS1  = 100", "NAXIS2  = 200", "C1VAL1  = 7 / mine"],
            );
            fc.clear_card().unwrap();
            check_write(&mut fc, sky_and_prim(), 1)?;
            check_card(&mut fc, "C1VAL1", true, Some("                   7 "))?;
            check_card(&mut fc, "CRVAL1", true, None)
        },
    ),
];

#[test]
fn fitschan_writes_match_the_c_library() {
    let failures: Vec<String> = CASES
        .iter()
        .filter_map(|(name, case)| case().err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Not in the C test: `testcorruptinput.c`'s `spec2` header, a cube with two
/// spectral axes. C's MakeFitsFrameSet gives no FrameSet for it, so no FITS
/// encoding writes anything (values from the C library at the pinned commit).
#[test]
fn two_spectral_axes_are_written_by_no_fits_encoding() {
    let fs = read_cards(&[
        "CTYPE1  = 'FREQ'",
        "CTYPE2  = 'PIXEL'",
        "CTYPE3  = 'FREQ-F2W'",
        "CRVAL1  =             1.4204E9",
        "CRVAL2  =                  0.0",
        "CRVAL3  =                  0.0",
        "CRPIX1  =                513.0",
        "CRPIX2  =                 50.0",
        "CRPIX3  =                 50.0",
        "CDELT1  =           2147483648",
        "CDELT2  =                  1.0",
        "CDELT3  =                  1.0",
        "CUNIT1  = 'Hz'",
        "SPECSYS = 'BARYCENT'",
        "RESTFRQ =             1.4204E9",
    ]);
    for encoding in [
        "FITS-WCS",
        "FITS-PC",
        "FITS-IRAF",
        "FITS-AIPS",
        "FITS-AIPS++",
        "FITS-CLASS",
        "DSS",
    ] {
        let mut fc = fitschan(&format!("Encoding={encoding}"), &[]);
        check_write(&mut fc, fs.clone(), 0).unwrap_or_else(|e| panic!("{encoding}: {e}"));
        assert!(list_cards(&mut fc).is_empty(), "{encoding} left cards");
    }
}

/// Not in the C test: a FITS description treats a SkyFrame as celestial only
/// when its Domain is SKY. Beside a celestial SkyFrame, a second SkyFrame with
/// Domain POLANAL is written as two ordinary axes, while a second celestial
/// one leaves nothing written (values from the C library at the pinned
/// commit).
#[test]
fn a_skyframe_outside_the_sky_domain_is_an_ordinary_axis() {
    let sky = tan_frameset();
    for (domain, expect) in [("SKY", 0), ("POLANAL", 1)] {
        let mut second = SkyFrame::new(SkySystem::Icrs);
        second.set_attribute("Domain", domain).unwrap();
        let fs = two_frame(
            frame(4, "Domain=GRID"),
            parallel(fmap(&sky), Box::new(UnitMap::new(2).unwrap())),
            Box::new(CmpFrame::new(current(&sky), Box::new(second)).unwrap()),
        );
        for encoding in ["FITS-WCS", "FITS-PC"] {
            let mut fc = fitschan(&format!("Encoding={encoding}"), &[]);
            check_write(&mut fc, fs.clone(), expect)
                .unwrap_or_else(|e| panic!("{domain} {encoding}: {e}"));
            if expect == 1 {
                check_card(&mut fc, "CTYPE3", true, Some("'RA      '")).unwrap();
                check_card(&mut fc, "CTYPE4", true, Some("'Dec     '")).unwrap();
                check_card(&mut fc, "CRVAL3", true, Some("  57.295779513082323")).unwrap();
            }
        }
    }
}

/// Not in the C test: a degenerate axis of a SkyFrame outside the SKY Domain
/// keeps the unit scale of an ordinary axis, not the scale of its partner
/// axis (values from the C library at the pinned commit).
#[test]
fn a_degenerate_axis_outside_the_sky_domain_has_a_unit_scale() {
    let sky = tan_frameset();
    let mut second = SkyFrame::new(SkySystem::Icrs);
    second.set_attribute("Domain", "POLANAL").unwrap();
    let fs = two_frame(
        frame(3, "Domain=GRID"),
        parallel(
            fmap(&sky),
            series(
                Box::new(ZoomMap::new(1, 0.01).unwrap()),
                Box::new(PermMap::new(&[1], &[1, -1], &[0.5]).unwrap()),
            ),
        ),
        Box::new(CmpFrame::new(current(&sky), Box::new(second)).unwrap()),
    );
    for encoding in ["FITS-WCS", "FITS-PC"] {
        let mut fc = fitschan(&format!("Encoding={encoding}"), &[]);
        check_write(&mut fc, fs.clone(), 1).unwrap_or_else(|e| panic!("{encoding}: {e}"));
        check_card(&mut fc, "CDELT3", true, Some(" 0.57295779513082312")).unwrap();
        check_card(&mut fc, "CDELT4", true, Some("                 1.0 ")).unwrap();
        check_card(&mut fc, "CRVAL4", true, Some("  28.647889756541161")).unwrap();
    }
}

/// Not in the C test: FITS-CLASS describes a cube only when its SkyFrame is
/// in the SKY Domain; with the same SkyFrame in the POLANAL Domain C writes
/// nothing (values from the C library at the pinned commit).
#[test]
fn class_needs_a_skyframe_in_the_sky_domain() {
    let fs = read_cards(&[
        "NAXIS   = 3",
        "NAXIS1  = 10",
        "NAXIS2  = 1",
        "NAXIS3  = 1",
        "CTYPE1  = 'FREQ'",
        "CTYPE2  = 'RA---SIN'",
        "CTYPE3  = 'DEC--SIN'",
        "CRPIX1  = 1",
        "CRPIX2  = 5",
        "CRPIX3  = 5",
        "CRVAL1  = 1.0E11",
        "CRVAL2  = 10",
        "CRVAL3  = 20",
        "CDELT1  = 1.0E6",
        "CDELT2  = -0.1",
        "CDELT3  = 0.1",
        "SPECSYS = 'LSRK'",
        "RESTFREQ= 1.0E11",
    ]);
    let naxes = ["NAXIS1  = 10", "NAXIS2  = 1", "NAXIS3  = 1"];

    let mut fc = fitschan("Encoding=FITS-CLASS", &naxes);
    check_write(&mut fc, fs.clone(), 1).unwrap();

    let cur = current(&fs);
    let cmp = cur.as_any().downcast_ref::<CmpFrame>().unwrap();
    let mut sky = cmp.frame2().snapshot();
    sky.set_domain("POLANAL").unwrap();
    let polanal = two_frame(
        base(&fs),
        fmap(&fs),
        Box::new(CmpFrame::new(cmp.frame1().snapshot(), sky).unwrap()),
    );
    let mut fc = fitschan("Encoding=FITS-CLASS", &naxes);
    check_write(&mut fc, polanal, 0).unwrap();
    assert_eq!(list_cards(&mut fc).len(), 3);
}

/// Not in the C test: a failed write leaves a FitsChan with the cards and
/// the current card it had, when the write replaced cards on both sides of
/// the current card (values from the C library at the pinned commit).
#[test]
fn a_failed_write_keeps_the_current_card_and_replaced_cards() {
    let tpn = read_cards(&[
        "NAXIS   = 2",
        "NAXIS1  = 100",
        "NAXIS2  = 200",
        "CRPIX1  = 50",
        "CRPIX2  = 100",
        "CRVAL1  = 10",
        "CRVAL2  = 20",
        "CTYPE1  = 'RA---TPN'",
        "CTYPE2  = 'DEC--TPN'",
        "CDELT1  = -0.001",
        "CDELT2  = 0.001",
        "PV1_1   = 1.0",
        "PV2_1   = 1.0",
    ]);
    let mut fc = fitschan(
        "Encoding=FITS-PC",
        &[
            "NAXIS1  = 100",
            "CRVAL1  = 5 / mine",
            "NAXIS2  = 200",
            "CRPIX2  = 7.5 / also mine",
            "CDELT1  = 3 / x",
            "FOO     = 1",
        ],
    );
    fc.set_card(3).unwrap();
    check_write(&mut fc, tpn, 0).unwrap();
    assert_eq!(fc.card().unwrap(), 3);
    assert_eq!(
        list_cards(&mut fc)
            .iter()
            .map(|c| c.trim_end())
            .collect::<Vec<_>>(),
        [
            "NAXIS1  =                  100",
            "CRVAL1  =                    5 / mine",
            "NAXIS2  =                  200",
            "CRPIX2  =                  7.5 / also mine",
            "CDELT1  =                    3 / x",
            "FOO     =                    1",
        ]
    );
}
