//! Writing and reading FITS-WCS headers that use the `-TAB` algorithm,
//! ported from `testtabspectrum`, `checktab` and `checktab2` in
//! `reference/starlink-ast/ast_tester/testfitschan.c`.
//!
//! Divergences from the C ancestor:
//!
//! - C's `astCopy` of the FitsChan is `Clone`, and the table source is a
//!   closure rather than a callback reading globals.
//!
//! The C error numbers are kept in the assertion messages.

use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{
    BasicFrameBuilder, Frame, FrameRef, FrameSet, SkyFrame, SpecFrame, SpecSystem,
};
use ast_core::mappings::{CmpMap, Composition, LutMap, Mapping, MathMap, ShiftMap};
use ast_core::{AnyObject, Channel, Encoding, FitsChan, FitsTable, KeyMap, from_native_string};

/// A FitsChan with the given attributes and `NAXIS` cards.
fn fitschan(encoding: Encoding, tab_ok: i32, naxis: &[usize]) -> FitsChan {
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(encoding);
    fc.set_tab_ok(tab_ok);
    fc.put_fits(&format!("NAXIS   = {}", naxis.len()), false)
        .unwrap();
    for (i, n) in naxis.iter().enumerate() {
        fc.put_fits(&format!("NAXIS{}  = {n}", i + 1), false)
            .unwrap();
    }
    fc
}

/// The value of the string keyword `name`, searching from the first card.
fn fits_s(fc: &mut FitsChan, name: &str) -> Option<String> {
    fc.clear_card().unwrap();
    fc.get_fits_s(Some(name)).unwrap()
}

/// The FitsTable stored under `name` in `tables`.
fn table(tables: &KeyMap, name: &str) -> FitsTable {
    match tables.get_0a(name).unwrap() {
        Some(Some(AnyObject::FitsTable(t))) => *t,
        _ => panic!("no FitsTable stored under {name}"),
    }
}

/// The doubles in the first row of `column`.
fn column(t: &FitsTable, column: &str) -> Vec<f64> {
    match t
        .get_column_data(column, 0.0, ast_core::constants::BAD_VALUE, None)
        .unwrap()
    {
        ast_core::ColumnData::Double(v) => v,
        other => panic!("{column} is not a double column: {other:?}"),
    }
}

/// The FrameSet read from `fc`, from its first card.
fn read_frameset(fc: &mut FitsChan) -> Option<FrameSet> {
    fc.clear_card().unwrap();
    match fc.read().unwrap() {
        Some(AnyObject::FrameSet(fs)) => Some(fs),
        Some(_) => panic!("read an object that is not a FrameSet"),
        None => None,
    }
}

/// The checks C's `checkft` makes on the `y = 1/x^2` table with its index.
fn check_ft(coords: &[f64], index: &[f64]) {
    let n = coords.len();
    assert_eq!(index[0], 1.0, "2001");
    assert_eq!(coords[0], 1.0, "2002");
    assert_eq!(index[n - 1], 1.0e2, "2003");
    assert_eq!(coords[n - 1], 1.0e-4, "2004");
    assert!(
        (coords[n / 2 - 1] - index[n / 2 - 1].powf(-2.0)).abs() <= 1e-20,
        "2005"
    );
}

/// The checks `checktab` makes on a table of the `y = 1/x^2` MathMap: two
/// 197-element columns, coordinates and index.
fn check_mathmap_table(tables: &KeyMap, base: u32) {
    assert_eq!(tables.map_size(), 1, "{}", base + 2);
    assert_eq!(tables.map_key(0).unwrap(), "WCS-TAB", "{}", base + 3);
    let t = table(tables, "WCS-TAB");
    let tt = t.as_table();
    assert_eq!(tt.ncolumn(), 2, "{}", base + 6);
    assert_eq!(tt.nrow(), 1, "{}", base + 7);
    assert_eq!(tt.column_length("coords1").unwrap(), 197, "{}", base + 8);
    assert_eq!(tt.column_length("index1").unwrap(), 197, "{}", base + 9);
    assert_eq!(t.column_size("COORDS1").unwrap(), 8 * 197, "{}", base + 10);
    assert_eq!(t.column_size("INDEX1").unwrap(), 8 * 197, "{}", base + 12);
    let coords = column(&t, "Coords1");
    let index = column(&t, "inDex1");
    assert_eq!(coords.len(), 197, "{}", base + 11);
    assert_eq!(index.len(), 197, "{}", base + 13);
    check_ft(&coords, &index);
}

/// The Mapping from the base Frame of `fs` to the base Frame of `fs2`
/// through their current Frames, applied to `x`.
fn round_trip(fs: &FrameSet, fs2: &FrameSet, x: &[f64]) -> Vec<f64> {
    let fwd = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let inv = fs2.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    inv.tran1(&fwd.tran1(x, true).unwrap(), false).unwrap()
}

/// `y = 1/x^2` with its inverse.
fn inverse_square() -> MathMap {
    MathMap::new(1, 1, &["y=1/(x*x)"], &["x=1/sqrt(y)"]).unwrap()
}

/// A one-axis Frame with Domain `domain`.
fn frame1(domain: &str) -> Box<dyn Frame> {
    Box::new(BasicFrameBuilder::new(1).domain(domain).build().unwrap())
}

/// `testtabspectrum`: a LutMap spectral axis is written as `WAVE-TAB`, with
/// its look-up table stored unchanged as the only column of `WCS-TAB`.
#[test]
fn tab_spectrum() {
    let lut = [0.0, 0.5, 1.5, 3.0, 5.0];
    let mut wave = SpecFrame::new(SpecSystem::Wavelen);
    wave.set_unit(1, "nm").unwrap();
    let mut fs = FrameSet::new(frame1("PIXELS"));
    fs.add_frame(
        FrameRef::Base,
        Box::new(LutMap::new(&lut, 1.0, 1.0).unwrap()),
        Box::new(wave),
    )
    .unwrap();

    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_cd_matrix(true);
    fc.set_tab_ok(1);
    assert_eq!(fc.write(&AnyObject::FrameSet(fs)).unwrap(), 1, "12430");

    assert_eq!(
        fits_s(&mut fc, "CTYPE1").as_deref(),
        Some("WAVE-TAB"),
        "12431"
    );
    // PS1_0 names the binary table extension, PS1_1 the column holding the
    // coordinate array.
    assert_eq!(
        fits_s(&mut fc, "PS1_0").as_deref(),
        Some("WCS-TAB"),
        "12432"
    );
    assert_eq!(
        fits_s(&mut fc, "PS1_1").as_deref(),
        Some("COORDS1"),
        "12433"
    );

    // The table travels alongside the header rather than in it.
    let tables = fc.get_tables().expect("12434");
    let mut t = table(&tables, "WCS-TAB");
    let mut header = t.get_table_header().unwrap();
    assert_eq!(
        fits_s(&mut header, "TDIM1").as_deref(),
        Some("(1,5)"),
        "12435"
    );
    assert_eq!(t.as_table().nrow(), 1, "12436");
    assert_eq!(t.as_table().ncolumn(), 1, "12436");
    let name = t.as_table().column_name(1).unwrap();
    assert_eq!(name, "COORDS1", "12437");
    assert_eq!(t.as_table().column_shape(&name).unwrap(), [1, 5], "12438");
    assert_eq!(t.column_size(&name).unwrap(), 5 * 8, "12439");
    // The column carries the units of the SpecFrame the axis came from.
    assert_eq!(
        fits_s(&mut header, "TUNIT1").as_deref(),
        Some("nm"),
        "12440"
    );
    // Table cells are addressed as "column(row)".
    let cell = t.as_table().get_1d(&format!("{name}(1)")).unwrap();
    assert_eq!(cell.as_deref(), Some(&lut[..]), "12441, 12442");
}

/// `checktab`, all but its `sparse.ast` section, which is
/// [`check_tab_sparse`]: a MathMap axis written
/// with an index column, a LutMap axis without one, the FrameSets read back
/// from them with the tables in the FitsChan and from a table source, the
/// errors a read without the tables raises, a non-celestial axis, and an
/// axis whose index is not the unit index.
#[test]
fn check_tab() {
    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit(1, "MHz").unwrap();
    let mut fs = FrameSet::new(frame1("GRID"));
    fs.add_frame(
        FrameRef::Base,
        Box::new(inverse_square()),
        Box::new(sf.clone()),
    )
    .unwrap();

    // Without TabOK the axis cannot be written, and no table is made.
    let mut fc = fitschan(Encoding::FitsWcs, 0, &[100]);
    assert_eq!(
        fc.write(&AnyObject::FrameSet(fs.clone())).unwrap(),
        0,
        "1000"
    );
    assert!(fc.get_tables().is_none(), "1001");

    fc.set_tab_ok(1);
    assert_eq!(
        fc.write(&AnyObject::FrameSet(fs.clone())).unwrap(),
        1,
        "1002"
    );
    check_mathmap_table(&fc.get_tables().expect("1003"), 1003);

    // Replace the MathMap with a LutMap giving wavelengths, shifted by 50
    // pixels, and make the current Frame a frequency again. The SpecFrame
    // is the one used above, which remembers MHz as the unit last used for
    // frequency, so the table holds MHz.
    let lut: Vec<f64> = (1..=100).map(|i| 1.0 / f64::from(i * i)).collect();
    sf.set_system(SpecSystem::Wavelen).unwrap();
    sf.set_unit(1, "m").unwrap();
    fs.remove_frame(FrameRef::Current).unwrap();
    fs.add_frame(
        FrameRef::Base,
        Box::new(LutMap::new(&lut, -49.0, 1.0).unwrap()),
        Box::new(sf),
    )
    .unwrap();
    fs.with_integrity(|f| {
        f.as_any_mut()
            .downcast_mut::<SpecFrame>()
            .unwrap()
            .set_system(SpecSystem::Freq)
    })
    .unwrap();
    fs.remap_frame(FrameRef::Base, Box::new(ShiftMap::new(vec![50.0])))
        .unwrap();

    // Remove the table and the header from the first write.
    fc.remove_tables("WCS-TAB").unwrap();
    fc.purge_wcs().unwrap();
    assert_eq!(
        fc.write(&AnyObject::FrameSet(fs.clone())).unwrap(),
        1,
        "1013"
    );
    let tables = fc.get_tables().expect("1014");
    assert_eq!(tables.map_size(), 1, "1016");
    assert_eq!(tables.map_key(0).unwrap(), "WCS-TAB", "1017");
    let t = table(&tables, "WCS-TAB");
    assert_eq!(t.as_table().ncolumn(), 1, "1020");
    assert_eq!(t.as_table().nrow(), 1, "1021");
    assert_eq!(t.as_table().column_length("coords1").unwrap(), 100, "1022");
    assert_eq!(t.column_size("COORDS1").unwrap(), 8 * 100, "1024");
    let coords = column(&t, "Coords1");
    assert_eq!(coords.len(), 100, "1025");
    assert!((coords[0] - 299.792458).abs() <= 1.0e-5, "3002");
    assert!((coords[99] - 2997924.58).abs() <= 1.0e-1, "3004");

    // Read back with the tables in the FitsChan; the copy is taken without
    // them.
    fc.remove_tables(" ").unwrap();
    let mut fc2 = fc.clone();
    fc.put_tables(&tables).unwrap();
    let fs2 = read_frameset(&mut fc).expect("1028");
    assert!(fs.current().equal(fs2.current()), "1029");
    let x = [1.0, 50.0, 100.0];
    for (y, x) in round_trip(&fs, &fs2, &x).iter().zip(x) {
        assert!((y - x).abs() <= 1.0e-4, "1030: {y} != {x}");
    }

    // Without the tables the read fails, with AST__NOTAB if TabOK is set
    // and AST__BDFTS if it is not, and consumes no cards.
    assert!(fc2.tab_ok() > 0, "1031");
    let ncard = fc2.ncard().unwrap();
    fc2.clear_card().unwrap();
    let err = fc2.read().expect_err("1032");
    assert_eq!(err.code(), ErrorCode::Notab, "1032");
    fc2.set_tab_ok(0);
    fc2.clear_card().unwrap();
    let err = fc2.read().expect_err("1032");
    assert_eq!(err.code(), ErrorCode::Bdfts, "1032");
    fc2.set_tab_ok(1);
    assert_eq!(fc2.ncard().unwrap(), ncard, "1034");

    // A table source supplies the table when the header asks for it.
    let source_table = table(&tables, "WCS-TAB");
    fc2.set_table_source(move |extnam: &str, extver: i32, extlevel: i32| {
        assert_eq!(extnam, "WCS-TAB", "1035");
        assert_eq!(extver, 1, "1065");
        assert_eq!(extlevel, 1, "1066");
        Ok(Some(source_table.clone()))
    });
    let fs2 = read_frameset(&mut fc2).expect("1035");
    assert!(fs.current().equal(fs2.current()), "1036");
    for (y, x) in round_trip(&fs, &fs2, &x).iter().zip(x) {
        assert!((y - x).abs() <= 1.0e-4, "1037: {y} != {x}");
    }

    // A Frame with no special class is written as a -TAB axis named after
    // its Domain.
    let mut fsv = FrameSet::new(frame1("GRID"));
    let volt = BasicFrameBuilder::new(1)
        .domain("voltage")
        .unit(1, "V")
        .build()
        .unwrap();
    fsv.add_frame(FrameRef::Base, Box::new(inverse_square()), Box::new(volt))
        .unwrap();
    fc.empty_fits().unwrap();
    fc.put_fits("NAXIS   = 1", false).unwrap();
    fc.put_fits("NAXIS1  = 100", false).unwrap();
    assert_eq!(
        fc.write(&AnyObject::FrameSet(fsv.clone())).unwrap(),
        1,
        "1043"
    );
    assert_eq!(
        fits_s(&mut fc, "CTYPE1").as_deref(),
        Some("VOLT-TAB"),
        "1059"
    );
    check_mathmap_table(&fc.get_tables().expect("1044"), 1044);

    let mut fs2v = read_frameset(&mut fc).expect("1058");
    fsv.invert();
    fs2v.invert();
    let fs3v = fs2v.convert_to(&mut fsv, &[""]).unwrap().expect("1061");
    assert_eq!(fsv.base().domain(), "VOLTAGE", "1062");
    assert_eq!(fs2v.base().domain(), "VOLTAGE", "1063");
    let x = [1.0, 10.0, 100.0];
    for (y, x) in fs3v.tran1(&x, true).unwrap().iter().zip(x) {
        assert!((y - x).abs() <= 1.0e-2, "1064: {y} != {x}");
    }

    // A non-linear Mapping in front of the LutMap makes an index that is
    // not the unit index, so the table has an INDEX column.
    let nilut: Vec<f64> = (0..20).map(|i| 100.0 + f64::from(i) * 5.0).collect();
    let nicm = CmpMap::new(
        Box::new(MathMap::new(1, 1, &["y=x*x"], &["x=sqrt(y)"]).unwrap()),
        Box::new(LutMap::new(&nilut, 1.0, 1.0).unwrap()),
        Composition::Series,
    )
    .unwrap();
    let volt = BasicFrameBuilder::new(1)
        .domain("VOLTAGE")
        .unit(1, "V")
        .build()
        .unwrap();
    let mut nifs = FrameSet::new(frame1("GRID"));
    nifs.add_frame(FrameRef::Base, Box::new(nicm), Box::new(volt))
        .unwrap();
    let mut nifc = fitschan(Encoding::FitsWcs, 1, &[20]);
    assert_eq!(nifc.write(&AnyObject::FrameSet(nifs)).unwrap(), 1, "1067");
    let nitables = nifc.get_tables().expect("1068");
    assert!(
        table(&nitables, "WCS-TAB").as_table().ncolumn() >= 2,
        "1068"
    );
    // Reading back uses the index column of the table stored in the
    // FitsChan.
    assert!(read_frameset(&mut nifc).is_some(), "1069");
}

/// `checktab`'s `sparse.ast` section: a FrameSet with -TAB celestial and
/// DSB spectral axes is written and read back, and the two must convert
/// through their `SKY-DSBSPECTRUM` Frames and transform alike.
#[test]
fn check_tab_sparse() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/programs/testfitschan/sparse.ast"
    ))
    .unwrap();
    let AnyObject::FrameSet(mut fssp) = from_native_string(&text).unwrap() else {
        panic!("1038: sparse.ast is not a FrameSet");
    };
    let mut fcsp = fitschan(Encoding::FitsWcs, 1, &[2000, 1]);
    assert_eq!(
        fcsp.write(&AnyObject::FrameSet(fssp.clone())).unwrap(),
        1,
        "1038"
    );
    let mut fs2sp = read_frameset(&mut fcsp).expect("1038");

    fssp.invert();
    fs2sp.invert();
    assert!(
        fssp.convert_to(&mut fs2sp, &["SKY-DSBSPECTRUM"])
            .unwrap()
            .is_some(),
        "1039"
    );
    assert_eq!(fssp.base().domain(), "SKY-DSBSPECTRUM", "1040");
    assert_eq!(fs2sp.base().domain(), "SKY-DSBSPECTRUM", "1041");
    fssp.invert();
    fs2sp.invert();

    for x in [[1.0, 1.0, 1.0], [10.0, 1.0, 1000.0]] {
        let pts = PointSet::from_rows([x]);
        let y = fssp.transform(&pts, true).unwrap();
        let y2 = fs2sp.transform(&pts, true).unwrap();
        for (a, b) in y.point(0).iter().zip(y2.point(0)) {
            assert!((a - b).abs() <= 1.0e-8, "1042: {a} != {b}");
        }
    }
}

/// `checktab2`: a celestial axis pair with a SkyRef is written as -TAB and
/// read back with the same SkyRef.
#[test]
fn check_tab2() {
    let mut sf = SkyFrame::unset_system();
    sf.set_sky_ref(1, 0.0001).unwrap();
    sf.set_sky_ref(2, 0.0001).unwrap();
    let mm = MathMap::new(1, 1, &["y=(x+50)**(-2)"], &["x=-50+1/sqrt(y)"]).unwrap();
    let mmm = CmpMap::new(Box::new(mm.clone()), Box::new(mm), Composition::Parallel).unwrap();
    let mut fs = FrameSet::new(Box::new(
        BasicFrameBuilder::new(2).domain("GRID").build().unwrap(),
    ));
    fs.add_frame(FrameRef::Base, Box::new(mmm), Box::new(sf))
        .unwrap();

    let mut fc = fitschan(Encoding::FitsWcs, 1, &[100, 100]);
    assert_eq!(fc.write(&AnyObject::FrameSet(fs)).unwrap(), 1, "2000");
    let fs2 = read_frameset(&mut fc).expect("2001");
    let sky = fs2
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("2001: the current Frame is a SkyFrame");
    assert!((sky.sky_ref(1).unwrap() - 0.0001).abs() <= 1.0e-7, "2001");
    assert!((sky.sky_ref(2).unwrap() - 0.0001).abs() <= 1.0e-7, "2001");
}

/// With Strict set, a `-TAB` header holding a value the read cannot convert
/// fails as the cards are swept, before the FrameSet is made, so the table
/// source is never asked for a table and no table is kept.
// C: `FitsToStore` sweeps the cards in `WcsFcRead` and frees the store on
// the strict `badval` error, so `FsetFromStore`, which fetches the tables,
// does nothing.
#[test]
fn strict_sweep_error_asks_no_table_source() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit(1, "MHz").unwrap();
    let mut fs = FrameSet::new(frame1("GRID"));
    fs.add_frame(FrameRef::Base, Box::new(inverse_square()), Box::new(sf))
        .unwrap();
    let mut fc = fitschan(Encoding::FitsWcs, 1, &[100]);
    assert_eq!(fc.write(&AnyObject::FrameSet(fs)).unwrap(), 1);
    let source_table = table(&fc.get_tables().unwrap(), "WCS-TAB");
    let header = fc.to_header().unwrap();
    assert!(header.contains("-TAB"));

    let mut fc = FitsChan::new().unwrap();
    fc.set_tab_ok(1);
    fc.set_warning_conditions("badval").unwrap();
    fc.config_mut().set_strict(true);
    for line in header.lines() {
        let line = if line.starts_with("CRVAL1  =") {
            "CRVAL1  = 'abc'"
        } else {
            line
        };
        fc.put_fits(line, false).unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    fc.set_table_source(move |_: &str, _: i32, _: i32| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(Some(source_table.clone()))
    });
    fc.clear_card().unwrap();
    let err = fc.read().expect_err("a strict badval");
    assert_eq!(err.code(), ErrorCode::Badin);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(fc.get_tables().is_none());
}
