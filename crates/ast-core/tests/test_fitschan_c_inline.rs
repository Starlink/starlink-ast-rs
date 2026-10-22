//! Checks made inline in `main()` of `testfitschan.c` that no other Rust test
//! makes, each named by C's error codes.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testfitschan.c`; the list is
//! `docs/issues/testfitschan-coverage.md`. Each test follows its C block,
//! with C's values. [`roundtrip`] is C's helper of the same name.

use std::path::{Path, PathBuf};

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::error::ErrorCode;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::frames::{
    BasicFrameBuilder, Frame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, SpecSystem,
    StdOfRest,
};
use ast_core::mappings::{Mapping, ZoomMap};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn put(fc: &mut FitsChan, cards: &[&str]) {
    for card in cards {
        fc.put_fits(card, false).unwrap();
    }
}

fn read_fs(fc: &mut FitsChan) -> FrameSet {
    fc.clear_card().unwrap();
    match fc.read().unwrap() {
        Some(AnyObject::FrameSet(fs)) => fs,
        other => panic!("expected a FrameSet, got {other:?}"),
    }
}

/// C's `roundtrip` result: the write or read failed (0), the keyword did not
/// have the expected value (-2), the coordinates disagreed (-1), or all is
/// well (1).
#[derive(Debug, PartialEq)]
enum Rt {
    Failed,
    WrongKey,
    Disagree,
    Ok,
}

/// C's `roundtrip`: write `fs` with `encoding`, check that `expect_key`
/// (any `CTYPEn` for `"CTYPE"`) has `expect_val`, read it back, and compare
/// the two FrameSets' world coordinates at pixel offsets 50, 50.5, 51, 55
/// and 45 on every axis, to `tol` absolute or relative.
fn roundtrip(fs: &FrameSet, encoding: Encoding, tol: f64, expect: (&str, &str)) -> Rt {
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(encoding);
    if fc.write(&AnyObject::FrameSet(fs.clone())).unwrap_or(0) != 1 {
        return Rt::Failed;
    }
    let (key, val) = expect;
    let keys: Vec<String> = if key == "CTYPE" {
        (1..=9).map(|i| format!("CTYPE{i}")).collect()
    } else {
        vec![key.to_string()]
    };
    let found = keys.iter().any(|k| {
        fc.clear_card().unwrap();
        fc.get_fits_s(Some(k)).ok().flatten().as_deref() == Some(val)
    });
    if !found {
        return Rt::WrongKey;
    }
    fc.clear_card().unwrap();
    let Ok(Some(AnyObject::FrameSet(fs2))) = fc.read() else {
        return Rt::Failed;
    };
    if tol < 0.0 {
        return Rt::Ok;
    }

    let map1 = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let map2 = fs2.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let curfrm2 = fs2.current();
    let nin = fs.nin();
    let cvt = fs
        .current()
        .snapshot()
        .convert_to(curfrm2.snapshot().as_mut(), &[""])
        .unwrap()
        .expect("the current Frames align");
    let pts = ast_core::coords::PointSet::from_rows(
        [50.0, 50.5, 51.0, 55.0, 45.0]
            .iter()
            .map(|&p| vec![p; nin])
            .collect::<Vec<_>>(),
    );
    let wcs1 = map1.transform(&pts, true).unwrap();
    let wcs2 = map2.transform(&pts, true).unwrap();
    let wcs1c = cvt.transform(&wcs1, true).unwrap();
    for ip in 0..pts.npoint() {
        for iax in 0..fs.nout() {
            let (v1, v2) = (wcs1c.point(ip)[iax], wcs2.point(ip)[iax]);
            if v1 == ast_core::constants::BAD_VALUE || v2 == ast_core::constants::BAD_VALUE {
                continue;
            }
            let diff = curfrm2.ax_distance(iax + 1, v1, v2).unwrap().abs();
            let scale = v1.abs().max(v2.abs());
            if diff > tol && scale > 0.0 && diff / scale > tol {
                return Rt::Disagree;
            }
        }
    }
    Rt::Ok
}

/// The FITS-WCS header C puts into a FitsChan before its GetFits tests.
const TAN_CARDS: [&str; 9] = [
    "CRPIX1  =                   45",
    "CRPIX2  =                   45",
    "CRVAL1  =                   45",
    "CRVAL2  =                 89.9",
    "MYNAME  =                     ",
    "CDELT1  =                -0.01",
    "CDELT2  =                 0.01",
    "CTYPE1  = 'RA---TAN'",
    "CTYPE2  = 'DEC--TAN'",
];

/// 7777-7778: GetFitsK with no keyword reads the card at the cursor.
#[test]
fn get_fits_k_at_the_cursor() {
    let mut fc = FitsChan::new().unwrap();
    put(&mut fc, &TAN_CARDS);
    fc.set_card(2).unwrap();
    assert_eq!(fc.get_fits_k(None).unwrap(), Some(45), "Error 7777-7778");
}

/// 12-13: with SipOK off, a SIP header reads but is too non-linear to write
/// as FITS-WCS.
#[test]
fn sip_header_does_not_write_as_fits_wcs() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_sip_ok(false);
    fc.set_source_file(fixture("wcsconv/inputs/sip.head").to_str().unwrap())
        .unwrap();
    let fs = read_fs(&mut fc);
    fc.set_encoding(Encoding::FitsWcs);
    let nw = fc.write(&AnyObject::FrameSet(fs)).unwrap_or(0);
    assert!(nw == 0, "Error 13: wrote {nw}");
}

/// 14-19: a header whose alternate description C is malformed fails to read
/// unless IgnoreBadAlt is set, when it gives the other four Frames, and
/// with the BadAlt warning enabled the read warns once.
#[test]
fn alternate_axes_with_a_bad_set() {
    let alt = fixture("programs/testfitschan/alt.header");
    let alt = alt.to_str().unwrap();
    let mut fc = FitsChan::new().unwrap();
    fc.set_clean(false);
    assert!(!fc.ignore_bad_alt(), "Error 14");
    fc.set_source_file(alt).unwrap();
    fc.clear_card().unwrap();
    let err = fc.read().expect_err("Error 15: the read succeeded");
    assert_eq!(err.code(), ErrorCode::Bdfts, "Error 15: {err}");

    fc.set_ignore_bad_alt(true);
    assert!(fc.ignore_bad_alt(), "Error 16");
    fc.empty_fits().unwrap();
    fc.set_source_file(alt).unwrap();
    let fs = read_fs(&mut fc);
    assert_eq!(fs.nframe(), 4, "Error 18");

    fc.empty_fits().unwrap();
    fc.set_source_file(alt).unwrap();
    fc.set_warning_conditions("BadAlt").unwrap();
    let _ = read_fs(&mut fc);
    assert_eq!(fc.warnings().map(|km| km.map_size()), Some(1), "Error 19");
}

/// 115-117, 133-135: ForceTab and TabOK are set, read back and cleared.
#[test]
fn force_tab_and_tab_ok_set_and_clear() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_force_tab(1);
    assert!(fc.test_force_tab(), "Error 115");
    assert_eq!(fc.force_tab(), 1, "Error 116");
    fc.clear_force_tab();
    assert!(!fc.test_force_tab(), "Error 117");
    fc.set_tab_ok(1);
    assert!(fc.test_tab_ok(), "Error 133");
    assert_eq!(fc.tab_ok(), 1, "Error 134");
    fc.clear_tab_ok();
    assert!(!fc.test_tab_ok(), "Error 135");
}

/// 313: ShowFits succeeds.
#[test]
fn show_fits_succeeds() {
    let mut fc = FitsChan::new().unwrap();
    put(&mut fc, &TAN_CARDS);
    fc.clear_card().unwrap();
    fc.show_fits().expect("Error 313");
}

/// 630-633: an FK5 header with an observation date and place, changed to
/// AZEL, writes `AZ---TAN` as FITS-WCS and reads back.
#[test]
fn azel_writes_as_fits_wcs() {
    let mut fc = FitsChan::new().unwrap();
    put(
        &mut fc,
        &[
            "SIMPLE  =                    T",
            "BITPIX  =                  -32",
            "NAXIS   =                    2",
            "NAXIS1  =                  100",
            "NAXIS2  =                  100",
            "CTYPE1  = 'RA---TAN'",
            "CTYPE2  = 'DEC--TAN'",
            "CRVAL1  =              180.000",
            "CRVAL2  =               45.000",
            "CRPIX1  =               50.500",
            "CRPIX2  =               50.500",
            "CDELT1  =              -0.0100",
            "CDELT2  =               0.0100",
            "RADESYS = 'FK5'",
            "EQUINOX =               2000.0",
            "DATE-OBS= '2017-07-21T08:38:39.087'",
            "OBSGEO-X=  -5464586.5949660344",
            "OBSGEO-Y=  -2492996.5580556658",
            "OBSGEO-Z=   2150654.3760909005",
            "END",
        ],
    );
    let mut fs = read_fs(&mut fc);
    set_sky_system(&mut fs, SkySystem::Azel);
    assert_eq!(
        roundtrip(&fs, Encoding::FitsWcs, 1e-3, ("CTYPE", "AZ---TAN")),
        Rt::Ok,
        "Error 630-633"
    );
}

/// Set the System of a FrameSet's current SkyFrame, as C's `astSetC(fs,
/// "System", ...)` does.
fn set_sky_system(fs: &mut FrameSet, sys: SkySystem) {
    fs.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SkyFrame>()
            .unwrap()
            .set_system(sys);
        Ok(())
    })
    .unwrap();
}

/// A one-axis FrameSet from a GRID Frame to a barycentric SpecFrame with a
/// rest frequency of 1.4204e9 Hz, through a ZoomMap, then changed to the
/// System and Unit C sets.
fn spec_frameset(
    sys: SpecSystem,
    unit: &str,
    zoom: f64,
    new_sys: SpecSystem,
    new_unit: Option<&str>,
) -> FrameSet {
    let mut spec = SpecFrame::new(sys);
    spec.set_unit(1, unit).unwrap();
    spec.set_std_of_rest(StdOfRest::Barycentric).unwrap();
    spec.set_rest_freq(1.4204e9);
    let mut pixel = BasicFrameBuilder::new(1).build().unwrap();
    Frame::set_domain(&mut pixel, "GRID").unwrap();
    let mut fs = FrameSet::new(Box::new(pixel));
    fs.add_frame(
        FrameRef::Current,
        Box::new(ZoomMap::new(1, zoom).unwrap()),
        Box::new(spec),
    )
    .unwrap();
    fs.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SpecFrame>()
            .unwrap()
            .set_system(new_sys)
    })
    .unwrap();
    if let Some(unit) = new_unit {
        Frame::set_unit(&mut fs, 1, unit).unwrap();
    }
    fs
}

/// 650-655, 660-665: SpecFrames whose pixel axis is linear in one System,
/// written in another, give C's non-linear FITS-WCS algorithm codes and
/// read back.
#[test]
fn non_linear_spectral_writes() {
    use SpecSystem::*;
    let cases = [
        (
            Wavelen,
            "m",
            1.0e-10,
            Wavenum,
            Some("1/m"),
            1e-3,
            "WAVN-W2F",
            650,
        ),
        (Freq, "Hz", 1.0e6, Airwave, Some("m"), 1e-3, "AWAV-F2A", 652),
        (Vrel, "m/s", 1000.0, Beta, None, 1e-3, "BETA", 654),
        (Freq, "Hz", 1.0e6, Vrel, Some("m/s"), 1e-3, "VELO-F2V", 660),
        (
            Airwave,
            "m",
            1.0e-10,
            Freq,
            Some("Hz"),
            0.1,
            "FREQ-A2F",
            662,
        ),
        (Vrel, "m/s", 1000.0, Freq, Some("Hz"), 1e-3, "FREQ-V2F", 664),
    ];
    for (sys, unit, zoom, new_sys, new_unit, tol, ctype, err) in cases {
        let fs = spec_frameset(sys, unit, zoom, new_sys, new_unit);
        assert_eq!(
            roundtrip(&fs, Encoding::FitsWcs, tol, ("CTYPE1", ctype)),
            Rt::Ok,
            "Error {err}-{}: {ctype}",
            err + 1
        );
    }
}

/// 804-805: a bad floating-point value reads back as the string `<bad>`.
#[test]
fn bad_float_reads_as_the_bad_string() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_f("BADFLT", ast_core::constants::BAD_VALUE, "bad float", false)
        .unwrap();
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_s(Some("BADFLT")).unwrap().as_deref(),
        Some("<bad>"),
        "Error 804-805"
    );
}

/// 836: an FK5 header remapped to Supergalactic writes `SLON` as FITS-WCS.
#[test]
fn supergalactic_writes_slon() {
    let mut fc = FitsChan::new().unwrap();
    put(
        &mut fc,
        &[
            "NAXIS1  =                  100",
            "NAXIS2  =                  100",
            "CTYPE1  = 'RA---TAN'",
            "CTYPE2  = 'DEC--TAN'",
            "CRVAL1  =              180.000",
            "CRVAL2  =               45.000",
            "CRPIX1  =               50.000",
            "CRPIX2  =               50.000",
            "CDELT1  =              -0.0100",
            "CDELT2  =               0.0100",
            "RADESYS = 'FK5'",
            "EQUINOX =               2000.0",
            "END",
        ],
    );
    let mut fs = read_fs(&mut fc);
    let mut tgt = SkyFrame::new(SkySystem::Supergalactic);
    let cvt = fs
        .current()
        .snapshot()
        .convert_to(&mut tgt, &[""])
        .unwrap()
        .unwrap();
    fs.remap_frame(
        FrameRef::Current,
        cvt.mapping(FrameRef::Base, FrameRef::Current).unwrap(),
    )
    .unwrap();
    set_sky_system(&mut fs, SkySystem::Supergalactic);

    let mut wfc = FitsChan::new().unwrap();
    wfc.set_encoding(Encoding::FitsWcs);
    put(
        &mut wfc,
        &[
            "NAXIS1  =                  100",
            "NAXIS2  =                  100",
        ],
    );
    assert_eq!(wfc.write(&AnyObject::FrameSet(fs)).unwrap(), 1);
    wfc.clear_card().unwrap();
    let mut found = false;
    while let Some(card) = wfc.find_fits("%f", true).unwrap() {
        found |= card.contains("SLON");
    }
    assert!(found, "Error 836: SkySys Supergalactic: CTYPE missing SLON");
}

/// 876, 880, 882: each Encoding reads back by name once set, and a set
/// Encoding and Clean test as set.
#[test]
fn encoding_and_clean_set_and_test() {
    let mut fc = FitsChan::new().unwrap();
    for name in [
        "FITS-WCS",
        "FITS-PC",
        "FITS-IRAF",
        "FITS-AIPS",
        "FITS-AIPS++",
        "FITS-CLASS",
        "DSS",
        "NATIVE",
    ] {
        fc.set_encoding(Encoding::from_name(name).unwrap());
        assert_eq!(fc.encoding().unwrap().name(), name, "Error 876");
    }
    assert!(fc.test_encoding(), "Error 880");
    fc.set_clean(true);
    assert!(fc.test_clean(), "Error 882");
    fc.clear_clean();
    fc.clear_encoding();
    assert!(!fc.test_clean() && !fc.test_encoding());
}

/// 4-11: a read fails on a CRPIX2 that is a string, leaving the cards in
/// place, except that with Clean set it removes every card it used but the
/// one RetainFits marked.
#[test]
fn failed_read_retains_cards() {
    let cards = [
        "CRPIX1  = 45",
        "CRPIX2  = 'fred'",
        "CRVAL1  = 45",
        "CRVAL2  = 89.9",
        "CDELT1  = -0.01",
        "CDELT2  = 0.01",
        "CTYPE1  = 'RA---TAN'",
        "CTYPE2  = 'DEC--TAN'",
    ];
    for (clean, ctype2_kept, errs) in [(false, true, (4, 6, 7)), (true, false, (8, 10, 11))] {
        let mut fc = FitsChan::new().unwrap();
        fc.set_clean(clean);
        put(&mut fc, &cards);
        fc.clear_card().unwrap();
        if fc.find_fits("CTYPE1", false).unwrap().is_some() {
            fc.retain_fits().unwrap();
        }
        fc.clear_card().unwrap();
        assert!(fc.read().is_err(), "Error {}-{}", errs.0, errs.0 + 1);
        fc.clear_card().unwrap();
        assert!(
            fc.find_fits("CTYPE1", false).unwrap().is_some(),
            "Error {}: CTYPE1 has not been retained",
            errs.1
        );
        fc.clear_card().unwrap();
        assert_eq!(
            fc.find_fits("CTYPE2", false).unwrap().is_some(),
            ctype2_kept,
            "Error {}: CTYPE2",
            errs.2
        );
    }
}

/// 500-534: a Galactic TAN header changed to each sky System and written in
/// each of five encodings. As in C, an encoding may refuse the write, but a
/// write it makes must have C's longitude CTYPE and read back to the same
/// sky positions.
#[test]
fn sky_systems_through_five_encodings() {
    let systems = [
        (SkySystem::Fk5, "RA---TAN"),
        (SkySystem::Fk4, "RA---TAN"),
        (SkySystem::Fk4NoE, "RA---TAN"),
        (SkySystem::Icrs, "RA---TAN"),
        (SkySystem::Galactic, "GLON-TAN"),
        (SkySystem::Supergalactic, "SLON-TAN"),
        (SkySystem::Ecliptic, "ELON-TAN"),
    ];
    let encodings = [
        Encoding::FitsWcs,
        Encoding::FitsPc,
        Encoding::FitsIraf,
        Encoding::FitsAips,
        Encoding::FitsAipsPP,
    ];
    for (is, (sys, ctype)) in systems.iter().enumerate() {
        for (ie, &enc) in encodings.iter().enumerate() {
            let mut fc = FitsChan::new().unwrap();
            put(
                &mut fc,
                &[
                    "SIMPLE  =                    T",
                    "BITPIX  =                  -32",
                    "NAXIS   =                    2",
                    "NAXIS1  =                  100",
                    "NAXIS2  =                  100",
                    "CTYPE1  = 'GLON-TAN'",
                    "CTYPE2  = 'GLAT-TAN'",
                    "CRVAL1  =              180.000",
                    "CRVAL2  =                0.000",
                    "CRPIX1  =               50.500",
                    "CRPIX2  =               50.500",
                    "CDELT1  =              -0.0100",
                    "CDELT2  =               0.0100",
                    "END",
                ],
            );
            let mut fs = read_fs(&mut fc);
            set_sky_system(&mut fs, *sys);
            let rt = roundtrip(&fs, enc, 1e-3, ("CTYPE", ctype));
            assert!(
                rt == Rt::Ok || rt == Rt::Failed,
                "Error {}: {sys:?} with {}: {rt:?}",
                500 + is * encodings.len() + ie,
                enc.name()
            );
        }
    }
}

/// 600-609: linear SpecFrames in six Systems, and pixel axes linear in FREQ
/// written as VOPT and WAVE, round-trip through FITS-WCS with C's CTYPE1.
#[test]
fn spectral_systems_through_fits_wcs() {
    use SpecSystem::*;
    let linear = [
        (Freq, "Hz", StdOfRest::Barycentric, 1.4204e9, 1.0e6, "FREQ"),
        (
            Wavelen,
            "m",
            StdOfRest::Barycentric,
            5.996e14,
            1.0e-10,
            "WAVE",
        ),
        (Vradio, "m/s", StdOfRest::Lsrk, 1.4204e9, 1000.0, "VRAD"),
        (
            Voptical,
            "m/s",
            StdOfRest::Barycentric,
            1.4204e9,
            1000.0,
            "VOPT",
        ),
        (
            Redshift,
            "",
            StdOfRest::Barycentric,
            1.4204e9,
            1.0e-6,
            "ZOPT",
        ),
        (Beta, "", StdOfRest::Barycentric, 1.4204e9, 1.0e-6, "BETA"),
    ];
    for (i, (sys, unit, sor, rest, zoom, ctype)) in linear.into_iter().enumerate() {
        let mut spec = SpecFrame::new(sys);
        spec.set_unit(1, unit).unwrap();
        spec.set_std_of_rest(sor).unwrap();
        spec.set_rest_freq(rest);
        let mut pixel = BasicFrameBuilder::new(1).build().unwrap();
        Frame::set_domain(&mut pixel, "GRID").unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(
            FrameRef::Current,
            Box::new(ZoomMap::new(1, zoom).unwrap()),
            Box::new(spec),
        )
        .unwrap();
        assert_eq!(
            roundtrip(&fs, Encoding::FitsWcs, 1e-3, ("CTYPE1", ctype)),
            Rt::Ok,
            "Error {}: {ctype}",
            600 + i
        );
    }
    for (new_sys, new_unit, ctype, err) in [
        (Voptical, None, "VOPT-F2W", 606),
        (Wavelen, Some("m"), "WAVE-F2W", 608),
    ] {
        let fs = spec_frameset(Freq, "Hz", 1.0e6, new_sys, new_unit);
        assert_eq!(
            roundtrip(&fs, Encoding::FitsWcs, 1e-3, ("CTYPE1", ctype)),
            Rt::Ok,
            "Error {err}-{}: {ctype}",
            err + 1
        );
    }
}

/// 680-684: a SpecFrame logarithmic in pixels writes `FREQ-LOG` and reads
/// back to 1e-6 relative at pixel 512.
#[test]
fn log_spectral_axis_writes_freq_log() {
    use ast_core::mappings::MathMap;
    let logmap = MathMap::new(
        1,
        1,
        &["s = 1.4204e9 * exp( p * 1.0e-4 )"],
        &["p = 1.0e4 * log( s / 1.4204e9 )"],
    )
    .unwrap();
    let mut spec = SpecFrame::new(SpecSystem::Freq);
    spec.set_unit(1, "Hz").unwrap();
    spec.set_std_of_rest(StdOfRest::Barycentric).unwrap();
    spec.set_rest_freq(1.4204e9);
    let mut pixel = BasicFrameBuilder::new(1).build().unwrap();
    Frame::set_domain(&mut pixel, "GRID").unwrap();
    let mut fs = FrameSet::new(Box::new(pixel));
    fs.add_frame(FrameRef::Base, Box::new(logmap), Box::new(spec))
        .unwrap();

    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    put(&mut fc, &["NAXIS   = 1", "NAXIS1  = 1024"]);
    assert_eq!(
        fc.write(&AnyObject::FrameSet(fs.clone())).unwrap(),
        1,
        "Error 680"
    );
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_s(Some("CTYPE1")).unwrap().as_deref(),
        Some("FREQ-LOG"),
        "Error 681-682"
    );
    let fs2 = read_fs(&mut fc);
    let pin = ast_core::coords::PointSet::from_rows([[512.0]]);
    let a = fs.transform(&pin, true).unwrap().point(0)[0];
    let b = fs2.transform(&pin, true).unwrap().point(0)[0];
    assert!((a - b).abs() <= (1e-6 * a).abs(), "Error 683: {a} vs {b}");
}

/// 650-659 (DATE-OBS block): an FK5 header with MJD-OBS and OBSGEO writes
/// them back as FITS-WCS within 1e-3 days and 1 m.
#[test]
// C's values, to the digits of its header cards.
#[allow(clippy::excessive_precision)]
fn date_and_observatory_write_back() {
    let mut fc = FitsChan::new().unwrap();
    put(
        &mut fc,
        &[
            "SIMPLE  =                    T",
            "BITPIX  =                  -32",
            "NAXIS   =                    2",
            "NAXIS1  =                  100",
            "NAXIS2  =                  100",
            "CTYPE1  = 'RA---TAN'",
            "CTYPE2  = 'DEC--TAN'",
            "CRVAL1  =              180.000",
            "CRVAL2  =               45.000",
            "CRPIX1  =               50.500",
            "CRPIX2  =               50.500",
            "CDELT1  =              -0.0100",
            "CDELT2  =               0.0100",
            "RADESYS = 'FK5'",
            "EQUINOX =               2000.0",
            "MJD-OBS =   57955.360174621412",
            "DATE-OBS= '2017-07-21T08:38:39.087'",
            "OBSGEO-X=  -5464586.5949660344",
            "OBSGEO-Y=  -2492996.5580556658",
            "OBSGEO-Z=   2150654.3760909005",
            "END",
        ],
    );
    let fs = read_fs(&mut fc);
    let mut wfc = FitsChan::new().unwrap();
    wfc.set_encoding(Encoding::FitsWcs);
    assert_eq!(wfc.write(&AnyObject::FrameSet(fs)).unwrap(), 1, "Error 651");
    for (key, want, tol, err) in [
        ("MJD-OBS", 57955.360174621412, 1e-3, 652),
        ("OBSGEO-X", -5464586.5949660344, 1.0, 654),
        ("OBSGEO-Y", -2492996.5580556658, 1.0, 656),
        ("OBSGEO-Z", 2150654.3760909005, 1.0, 658),
    ] {
        wfc.clear_card().unwrap();
        let got = wfc
            .get_fits_f(Some(key))
            .unwrap()
            .unwrap_or_else(|| panic!("Error {err}: {key} not written"));
        assert!(
            (got - want).abs() <= tol,
            "Error {}: {key} = {got}",
            err + 1
        );
    }
}

/// 740-747: string values read as logicals.
#[test]
fn strings_read_as_logicals() {
    let mut fc = FitsChan::new().unwrap();
    let cases = [
        ("STRT", "T", true),
        ("STRY", "Y", true),
        ("STRF", "F", false),
        ("STRN", "N", false),
        ("STRDOTT", ".TRUE.", true),
        ("STRDOTF", ".FALSE.", false),
        ("STR1", "1", true),
        ("STR0", "0", false),
    ];
    for (key, val, _) in cases {
        fc.set_fits_s(key, val, "", false).unwrap();
    }
    for (i, (key, val, want)) in cases.into_iter().enumerate() {
        fc.clear_card().unwrap();
        assert_eq!(
            fc.get_fits_l(Some(key)).unwrap(),
            Some(want),
            "Error {}: STRING '{val}' -> LOGICAL",
            740 + i
        );
    }
}

/// 808-809: PutFits overwriting a card of each value type with the same
/// value, then an integer too large for 32 bits with a different one. The
/// steps are C's, in C's order: an overwrite replaces the card at the
/// cursor, which each Get leaves on the card it found.
#[test]
fn put_fits_overwrites_each_type() {
    let mut fc = FitsChan::new().unwrap();
    fc.put_fits("KTEST   = 9999999999 / original", true)
        .unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("KTEST   = 9999999999 / same value", true)
        .unwrap();
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_k(Some("KTEST")).unwrap(),
        Some(9999999999),
        "Error 808: KINT"
    );

    fc.put_fits("CITEST  = 10 20 / original ci", true).unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("CITEST  = 10 20 / same ci", true).unwrap();
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_ci(Some("CITEST")).unwrap(),
        Some((10, 20)),
        "Error 808: COMPLEXI"
    );

    fc.put_fits("CFTEST  = 1.5 2.5 / original cf", true)
        .unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("CFTEST  = 1.5 2.5 / same cf", true).unwrap();
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_cf(Some("CFTEST")).unwrap(),
        Some((1.5, 2.5)),
        "Error 808: COMPLEXF"
    );

    fc.put_fits("LTEST   = T / original logical", true).unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("LTEST   = T / same logical", true).unwrap();
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_l(Some("LTEST")).unwrap(),
        Some(true),
        "Error 808: LOGICAL"
    );

    fc.put_fits("UTEST   =   / original undef", true).unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("UTEST   =   / same undef", true).unwrap();

    fc.put_fits("CONTINUE  'a long continuation value'", true)
        .unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("CONTINUE  'a long continuation value'", true)
        .unwrap();
    fc.clear_card().unwrap();
    assert!(
        fc.get_fits_cn(Some("CONTINUE")).unwrap().is_some(),
        "Error 808: CONTINUE"
    );

    fc.put_fits("COMMENT   This is a comment", true).unwrap();
    fc.clear_card().unwrap();
    fc.put_fits("COMMENT   This is a comment", true).unwrap();

    fc.clear_card().unwrap();
    fc.put_fits("KTEST   = 8888888888 / changed", true).unwrap();
    fc.clear_card().unwrap();
    assert_eq!(
        fc.get_fits_k(Some("KTEST")).unwrap(),
        Some(8888888888),
        "Error 809"
    );
}

/// 810-821: every TIMESYS value reads, and TDB and TCB give an Epoch
/// different from UTC's.
#[test]
fn every_timesys_reads() {
    let mut epoch_utc = 0.0;
    for (its, ts) in ["UTC", "UT", "IAT", "ET", "TT", "TDT", "TDB", "TCG", "TCB"]
        .iter()
        .enumerate()
    {
        let mut fc = FitsChan::new().unwrap();
        put(
            &mut fc,
            &[
                "NAXIS1  = 100",
                "NAXIS2  = 100",
                "CTYPE1  = 'RA---TAN'",
                "CTYPE2  = 'DEC--TAN'",
                "CRVAL1  = 180.0",
                "CRVAL2  = 45.0",
                "CRPIX1  = 50.5",
                "CRPIX2  = 50.5",
                "CDELT1  = -0.001",
                "CDELT2  = 0.001",
                "RADESYS = 'FK5'",
                "EQUINOX = 2000.0",
                "MJD-OBS = 55000.0",
                &format!("TIMESYS = '{ts:<8}'"),
            ],
        );
        fc.clear_card().unwrap();
        let Ok(Some(AnyObject::FrameSet(fs))) = fc.read() else {
            panic!("Error {}: TIMESYS='{ts}' read failed", 810 + its);
        };
        let epoch = fs.epoch_mjd();
        match *ts {
            "UTC" => epoch_utc = epoch,
            "TDB" => assert!((epoch - epoch_utc).abs() >= 1.0e-8, "Error 820"),
            "TCB" => assert!((epoch - epoch_utc).abs() >= 1.0e-8, "Error 821"),
            _ => {}
        }
    }
}
