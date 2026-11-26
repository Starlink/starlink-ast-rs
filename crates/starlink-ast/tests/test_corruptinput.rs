// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Input that is corrupt, malicious or very large must give the C library's
//! error, and never crash, hang or exhaust memory.
//!
//! Ported from `reference/starlink-ast/ast_tester/testcorruptinput.c`.
//! Divergences from the C:
//!
//! - Every dump the C test reads is replayed by `test_hardening_oracle.rs`;
//!   this file ports its other checks.
//! - The C test checks only the status; the messages are compared too, with
//!   those the C library at the pinned commit reports.
//! - C passes counts as `int`; the Rust constructors take a `usize` or a slice,
//!   so a negative count cannot be passed.
//! - C cuts a default Domain, Title, Label or Symbol that it builds from other
//!   text to fit a buffer of 100 or 200 characters; the port keeps the text
//!   whole, so `long_default_text` checks that C's cut text is a prefix of the
//!   port's and, for a Domain, that it is not cut at 100 characters
//!   (`docs/issues/c-library-quirks.md`).
//! - `attributes` (C's `test_attributes`) leaves out its Prism and Plot3D
//!   cases: a Prism is a Region, which the attribute-strings layer does not
//!   yet support (`PLAN.md` section 1), and Plot3D is not ported.
//! - `fits_digits` (C's `test_documents`) ports only its FitsChan parts; there
//!   is no StcsChan, XmlChan or YamlChan.
//! - The "six nested logs" FluxFrame Unit of C's `test_long_text` is not ported
//!   here.
//! - C's `check_survives`, `check_dateobs` and `check_steps` run on a thread
//!   with a 10 s limit, so that a hang fails rather than stalls.
//! - `check_spline_values` is from `testloaderhardening.c`, and the permuted
//!   Frame and CmpFrame round trips from `testframeperm.c`.

use std::time::Duration;

use ndarray::Array2;
use starlink_ast::constants::{BAD_VALUE, MXAXES};
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::{
    BasicFrameBuilder, CmpFrame, DSBSpecFrame, FluxFrame, FluxSystem, Frame, SkyFrame, SpecFrame,
    SpecSystem, TimeFrame, TimeSystem,
};
use starlink_ast::mappings::{
    CmpMap, Composition, Mapping, MatrixMap, PermMap, ShiftMap, SplineMap, UnitMap, UnitNormMap,
    WcsMap, WcsProjection, WinMap, ZoomMap,
};
use starlink_ast::{AnyObject, AstType, Channel, Encoding, FitsChan, Table, from_native_string};

fn expect<T>(result: AstResult<T>, code: ErrorCode, messages: &[&str]) {
    let Err(err) = result else {
        panic!("no error");
    };
    let got: Vec<&str> = err
        .message()
        .split('\n')
        .chain(err.context().iter().map(String::as_str))
        .collect();
    assert_eq!((err.code(), got), (code, messages.to_vec()));
}

fn too_many_inputs(class: &str, n: usize) -> [String; 2] {
    [
        format!("astInitMapping({class}): Bad number of input coordinates ({n})."),
        format!("This number should not exceed {MXAXES}."),
    ]
}

fn expect_inputs<T>(result: AstResult<T>, class: &str, n: usize) {
    let [a, b] = too_many_inputs(class, n);
    expect(result, ErrorCode::Badni, &[&a, &b]);
}

/// Constructors accept the limit and refuse one more.
#[test]
fn axis_limit() {
    assert_eq!(MXAXES, 65536);
    assert!(UnitMap::new(MXAXES).is_ok());
    expect_inputs(UnitMap::new(MXAXES + 1), "UnitMap", MXAXES + 1);
    expect_inputs(UnitMap::new(2147483647), "UnitMap", 2147483647);
    expect_inputs(ShiftMap::new(vec![0.0; MXAXES + 1]), "ShiftMap", MXAXES + 1);
    expect_inputs(ZoomMap::new(MXAXES + 1, 2.0), "ZoomMap", MXAXES + 1);
    expect_inputs(
        WinMap::new(&vec![0.0; MXAXES + 1], &vec![1.0; MXAXES + 1]),
        "WinMap",
        MXAXES + 1,
    );
    expect_inputs(
        UnitNormMap::new(vec![0.0; MXAXES + 1]),
        "UnitNormMap",
        MXAXES + 1,
    );
    let perm: Vec<i64> = (1..=65537).collect();
    expect_inputs(PermMap::new(&perm, &[], &[]), "PermMap", MXAXES + 1);
    expect(
        PermMap::new(&[], &perm, &[]),
        ErrorCode::Badno,
        &[
            "astInitMapping(PermMap): Bad number of output coordinates (65537).",
            "This number should not exceed 65536.",
        ],
    );
    expect_inputs(
        CmpMap::new(
            Box::new(UnitMap::new(MXAXES).unwrap()),
            Box::new(UnitMap::new(MXAXES / 2).unwrap()),
            Composition::Parallel,
        ),
        "CmpMap",
        MXAXES + MXAXES / 2,
    );

    expect(
        BasicFrameBuilder::new(MXAXES + 1).build(),
        ErrorCode::Naxin,
        &[
            "astInitFrame(Frame): Number of axes (65537) is invalid - this number should not exceed 65536.",
        ],
    );
    expect(
        BasicFrameBuilder::new(2147483647).build(),
        ErrorCode::Naxin,
        &[
            "astInitFrame(Frame): Number of axes (2147483647) is invalid - this number should not exceed 65536.",
        ],
    );

    // Two Frames each within the limit cannot make a CmpFrame beyond it.
    let big = || Box::new(BasicFrameBuilder::new(MXAXES).build().unwrap());
    let half = || Box::new(BasicFrameBuilder::new(MXAXES / 2).build().unwrap());
    assert!(CmpFrame::new(half(), half()).is_ok());
    expect(
        CmpFrame::new(big(), half()),
        ErrorCode::Naxin,
        &[
            "astInitCmpFrame(CmpFrame): Number of axes (98304) is invalid - this number should not exceed 65536.",
        ],
    );
}

/// A permutation that is not one gives C's two messages, naming the class.
#[test]
fn invalid_permutations() {
    let messages = |class: &str, n: usize| {
        [
            format!("astPermAxes({class}): Invalid axis permutation array."),
            format!("Each axis index should lie in the range 1 to {n} and should occur only once."),
        ]
    };
    let mut frame = BasicFrameBuilder::new(3).build().unwrap();
    for perm in [[1, 1, 2], [0, 1, 2], [1, 2, 4]] {
        let [a, b] = messages("Frame", 3);
        expect(frame.perm_axes(&perm), ErrorCode::Prmin, &[&a, &b]);
    }
    let mut cmp = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    )
    .unwrap();
    let [a, b] = messages("CmpFrame", 2);
    expect(cmp.perm_axes(&[1, 1]), ErrorCode::Prmin, &[&a, &b]);
    let mut sky = SkyFrame::unset_system();
    let [a, b] = messages("SkyFrame", 2);
    expect(sky.perm_axes(&[1, 1]), ErrorCode::Prmin, &[&a, &b]);
}

/// Run `f` on its own thread, giving `Err` with what went wrong if it panics
/// or takes more than 10 s.
fn within_10s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => Ok(v),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err("took more than 10 s".into()),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err("panicked".into()),
    }
}

/// A dump made by joining `head`, `nfill` copies of `fill` and `tail`.
fn long_dump(head: &str, nfill: usize, fill: char, tail: &str) -> String {
    format!("{head}{}{tail}", fill.to_string().repeat(nfill))
}

/// A unit in which `unit` is the argument of `nlog` nested `log` functions.
fn nested_unit(unit: &str, nlog: usize) -> String {
    format!("{}{unit}{}", "log(".repeat(nlog), ")".repeat(nlog))
}

/// Check that `text` starts with `start`, the text C gives after cutting it to
/// fit a buffer, which the port does not cut.
fn check_text(what: &str, text: &str, start: &str) {
    assert!(
        text.starts_with(start),
        "{what}: \"{text}\" does not start with \"{start}\""
    );
}

/// Text that is longer than the buffers C once copied it into.
#[test]
fn long_text() {
    // An item that is not recognized is quoted in a warning.
    let dump = long_dump(
        " Begin Frame\n Naxes = 1\n Junk = \"",
        5000,
        'J',
        "\"\n End Frame\n",
    );
    let _ = within_10s(move || from_native_string(&dump).map(|_| ())).unwrap();
}

/// The name of a Table column is put in the key of each of its cells, and a
/// name of more than a hundred characters is refused when a dump is read.
#[test]
fn long_table_column_name() {
    let mut table = Table::new();
    table.add_column("COLNAME", AstType::Int, &[], " ").unwrap();
    table.put_0i("COLNAME(1)", 1, None).unwrap();
    table.put_0i("COLNAME(3)", 3, None).unwrap();
    let dump = table.to_native_string().unwrap();
    assert!(dump.contains("COLNAME"));
    let dump = dump.replace("COLNAME", &"N".repeat(150));
    expect(
        from_native_string(&dump),
        ErrorCode::Lderr,
        &[
            "astLoad(Table): The name of column 1 read for a Table is too long (it must have no more than 100 characters).",
            "Error while reading a Table from a Channel.",
        ],
    );
}

/// The default Domain, Title and Labels of a CmpFrame of Frames with long
/// Domains.
/// C cuts the Domain to the 100 characters of its buffer and the port keeps
/// it whole.
#[test]
fn long_domain_of_a_cmpframe() {
    let dump = long_dump(
        " Begin Frame\n Naxes = 1\n Domain = \"",
        300,
        'D',
        "\"\n Lbl1 = \"A long label\"\n Uni1 = \"km\"\n End Frame\n",
    );
    let load = || from_native_string(&dump).unwrap().into_frame().unwrap();
    let mut cmp = CmpFrame::new(load(), load()).unwrap();
    for _ in 0..4 {
        cmp = CmpFrame::new(Box::new(cmp), load()).unwrap();
    }
    let domain = cmp.domain().to_string();
    assert!(domain.starts_with('D'), "{domain}");
    assert!(domain.len() > 100, "the Domain was cut to {}", domain.len());
    cmp.title().unwrap();
    cmp.label(1).unwrap();
    cmp.to_native_string().unwrap();
}

/// The default Title of a SpecFluxFrame joins the Labels of its components.
#[test]
fn long_label_in_a_specfluxframe() {
    let dump = long_dump(
        " Begin SpecFluxFrame\n IsA Frame\n FrameA =\n Begin SpecFrame\n Naxes = 1\n Lbl1 = \"",
        400,
        'L',
        "\"\n IsA Frame\n End SpecFrame\n FrameB =\n Begin FluxFrame\n Naxes = 1\n IsA Frame\n \
         End FluxFrame\n IsA CmpFrame\n End SpecFluxFrame\n",
    );
    if let Ok(AnyObject::SpecFluxFrame(frame)) = from_native_string(&dump) {
        let _ = frame.title();
    }
}

/// The default Label and Symbol of a spectral, flux or time axis change with
/// its Unit, so a long Unit gives a long Label and Symbol.
/// C cuts each to fit its buffer; the port gives the whole text, which starts
/// with C's.
#[test]
fn long_default_text() {
    let unit = nested_unit("Hz", 40);
    let mut spec = SpecFrame::new(SpecSystem::Freq);
    spec.set_unit(1, &unit).unwrap();
    let label = format!(
        "{}(1.0000000000000001e-09/ Frequency )**-1",
        "log(".repeat(40)
    );
    check_text("SpecFrame Label", &spec.label(1).unwrap(), &label);
    let symbol = format!(
        "{}(1.0000000000000001e-09/ FREQ )**-1)))))",
        "log(".repeat(40)
    );
    check_text("SpecFrame Symbol", &spec.symbol(1).unwrap(), &symbol);

    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_unit(1, &unit).unwrap();
    check_text(
        "DSBSpecFrame Label",
        &dsb.label(1).unwrap(),
        &"log(".repeat(25),
    );

    let mut time = TimeFrame::new(TimeSystem::Mjd);
    time.set_unit(1, &nested_unit("d", 40)).unwrap();
    let label = format!(
        "{} Modified Julian Date {}",
        "log(".repeat(40),
        ")".repeat(18)
    );
    check_text("TimeFrame Label", &time.label(1).unwrap(), &label);
    let symbol = format!("{} MJD {}", "log(".repeat(40), ")".repeat(35));
    check_text("TimeFrame Symbol", &time.symbol(1).unwrap(), &symbol);

    let mut flux = FluxFrame::new(FluxSystem::FluxDen);
    flux.set_unit(1, &nested_unit("Jy", 4)).unwrap();
    assert_eq!(flux.label(1).unwrap(), "log(log(log(log( Flux density ))))");
    assert_eq!(flux.symbol(1).unwrap(), "log(log(log(log( S_nu ))))");

    // A wave number in units of wavelength has a Symbol that includes the
    // scale factor.
    let mut wavn = SpecFrame::new(SpecSystem::Wavenum);
    wavn.set_unit(1, "nm").unwrap();
    let symbol = wavn.symbol(1).unwrap();
    assert!(symbol.len() >= 21 && symbol.contains("WAVN"), "{symbol}");
    assert_eq!(symbol, "999999999.99999988/ WAVN ");
}

/// A FluxFrame dump includes a comment for each preferred unit that names the
/// System the unit belongs to; the longest name is that of surface brightness
/// per unit wavelength.
#[test]
fn surface_brightness_flux_frame_dump() {
    let mut flux = FluxFrame::with_spec_val(1.0e10, None);
    flux.set_system(FluxSystem::SBrightW).unwrap();
    flux.set_unit(1, "W/m^2/Angstrom/arcsec^2").unwrap();
    let dump = AnyObject::FluxFrame(flux).to_native_string().unwrap();
    assert!(matches!(
        from_native_string(&dump),
        Ok(AnyObject::FluxFrame(_))
    ));
}

/// A Format is a `printf` format for the one axis value; one that asks for
/// anything else is refused when it is used, as is one that needs more room
/// than there is.
#[test]
fn format_attribute() {
    let mut frame = BasicFrameBuilder::new(1).build().unwrap();
    frame.set_format_str(1, "%.3lf metres").unwrap();
    assert_eq!(frame.format(1, 1.5).unwrap(), "1.500 metres");
    frame.set_format_str(1, "%5d%%").unwrap();
    assert_eq!(frame.format(1, 42.0).unwrap(), "   42%");
    for bad in [
        "%s",
        "%n",
        "%s%s%s%s",
        "%.2f and %s",
        "%*d",
        "%Lf",
        "%ld",
        "%.2f %.2f",
        "%p",
    ] {
        // C accepts the setting and refuses the format when it is used.
        let _ = frame.set_format_str(1, bad);
        expect(
            frame.format(1, 1.5),
            ErrorCode::Fmter,
            &[&format!(
                "astFormat: The Format string \"{bad}\" cannot be used to format an axis value. \
                 It should have no more than one conversion specification, for a floating \
                 point or integer value."
            )],
        );
    }
    for bad in ["%.500f", "%500.1f"] {
        let _ = frame.set_format_str(1, bad);
        expect(
            frame.format(1, 1.5),
            ErrorCode::Fmter,
            &[
                "astAxisFormat(Axis): Internal buffer overflow while formatting a coordinate \
                 value of 1.5 - result exceeds 127 characters.",
                &format!("The format string was \"{bad}\"."),
            ],
        );
    }
}

/// The same Format read from a dump is refused when it is used.
#[test]
fn format_from_a_dump() {
    let frame = from_native_string(" Begin Frame\n Naxes = 1\n Fmt1 = \"%s%n\"\n End Frame\n")
        .unwrap()
        .into_frame()
        .unwrap();
    expect(
        frame.format(1, 1.5),
        ErrorCode::Fmter,
        &[
            "astFormat: The Format string \"%s%n\" cannot be used to format an axis value. \
             It should have no more than one conversion specification, for a floating point \
             or integer value.",
        ],
    );
}

/// Attribute names and values that are longer than the buffers C once copied
/// them into.
#[cfg(feature = "attribute-strings")]
#[test]
fn attributes() {
    use starlink_ast::attributes::Attributes;
    use starlink_ast::frames::FrameSet;

    // A value of 5000 characters is kept whole by each way of setting it.
    let value = "v".repeat(5000);
    let mut frame = BasicFrameBuilder::new(1).build().unwrap();
    frame.set_title(&value);
    assert_eq!(frame.title(), value);
    frame
        .set_attributes(&format!("Domain=D,Title={value}"))
        .unwrap();
    assert_eq!(frame.get_attribute("Title").unwrap(), value);
    frame.set_attribute("Title", &value).unwrap();
    assert_eq!(frame.get_attribute("Title").unwrap(), value);

    // The same value as the Label of each axis of a CmpFrame, which passes the
    // setting on to the Frame that owns the axis.
    let cmp = || {
        CmpFrame::new(
            Box::new(SkyFrame::unset_system()),
            Box::new(SpecFrame::unset_system()),
        )
        .unwrap()
    };
    let mut cmp1 = cmp();
    for i in 1..=3 {
        cmp1.set_label(i, &value).unwrap();
        assert_eq!(cmp1.label(i).unwrap(), value);
        let mut cmp2 = cmp();
        cmp2.set_attribute(&format!("Label({i})"), &value).unwrap();
        assert_eq!(cmp2.label(i).unwrap(), value);
        cmp2.set_attributes(&format!("Label({i})={value}")).unwrap();
        assert_eq!(cmp2.get_attribute(&format!("Label({i})")).unwrap(), value);
    }

    // An attribute name that ends with an axis index is split into the name
    // and the index; no attribute has a name of 300 characters.
    let name = format!("{}(1)", "x".repeat(300));
    let setting = format!("{name}=1");
    let mut cmp = cmp();
    let mut fs = FrameSet::new(Box::new(cmpframe_copy(&cmp)));
    check_badat("a CmpFrame", &mut cmp, &name, &setting);
    check_badat("a FrameSet", &mut fs, &name, &setting);
}

#[cfg(feature = "attribute-strings")]
fn cmpframe_copy(cmp: &CmpFrame) -> CmpFrame {
    cmp.clone()
}

/// Check that clearing, getting, testing and setting `name` (set as
/// `setting`) give an unknown-attribute error.
#[cfg(feature = "attribute-strings")]
fn check_badat<T: starlink_ast::attributes::Attributes>(
    what: &str,
    obj: &mut T,
    name: &str,
    setting: &str,
) {
    let code = |r: AstResult<()>| r.expect_err(what).code();
    assert_eq!(
        code(obj.clear_attributes(name)),
        ErrorCode::Badat,
        "clearing {what}"
    );
    assert_eq!(
        code(obj.get_attribute(name).map(|_| ())),
        ErrorCode::Badat,
        "getting {what}"
    );
    assert_eq!(
        code(obj.test_attribute(name).map(|_| ())),
        ErrorCode::Badat,
        "testing {what}"
    );
    assert_eq!(
        code(obj.set_attributes(setting)),
        ErrorCode::Badat,
        "setting {what}"
    );
}

/// A FITS card is formatted with the number of digits given by FitsDigits, in
/// what is left of the card.
#[test]
fn fits_digits() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_digits(-300);
    fc.set_fits_f("VAL", 1.0e-300, "", false).unwrap();
    fc.clear_card().unwrap();
    let card = fc.find_fits("VAL", false).unwrap().unwrap();
    assert!(card.contains("1.0E-300"), "{card}");

    // The two parts of a complex value share the card.
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_digits(40);
    fc.set_fits_cf("CVAL", (-1.0 / 3.0, 1.0e-300), "", false)
        .unwrap();
    fc.clear_card().unwrap();
    let _ = fc.find_fits("CVAL", false);

    // A positive value is used as it is, so a value that needs more room than
    // the card has cannot be written.
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_digits(300);
    let result = fc
        .set_fits_f("VAL", 1.0e-20 / 3.0, "", false)
        .and_then(|()| fc.clear_card())
        .and_then(|()| fc.find_fits("VAL", false));
    expect(
        result,
        ErrorCode::Bdfts,
        &[
            "astFindFits(FitsChan): Cannot encode floating point value 3.33333e-21 into a FITS header card for keyword 'VAL'.",
        ],
    );
}

/// Splitting a Mapping in which two of many axes are coupled.
#[test]
fn split() {
    const NAX: usize = 12;
    let matrix = {
        let mut m = Array2::<f64>::eye(NAX);
        m[[1, 2]] = 0.5;
        m[[2, 1]] = -0.5;
        m
    };
    let maps: [(&str, Box<dyn Mapping>); 2] = [
        ("MatrixMap", Box::new(MatrixMap::new(matrix).unwrap())),
        (
            "WcsMap",
            Box::new(WcsMap::new(NAX, WcsProjection::Tan, 2, 3).unwrap()),
        ),
    ];
    for (class, map) in maps {
        let mut inperm = [0_i64; NAX];
        inperm[1] = 1;
        inperm[2] = 2;
        let perm = PermMap::new(&inperm, &[2, 3], &[]).unwrap();
        let cmp = CmpMap::new(map, Box::new(perm), Composition::Series).unwrap();
        // Neither coupled axis can be split off alone.
        assert!(
            cmp.map_split(&[2]).unwrap().is_none(),
            "split one of two coupled axes from a {class}"
        );
        // The two can be split off together, and feed both outputs.
        let (out, part) = cmp
            .map_split(&[2, 3])
            .unwrap()
            .unwrap_or_else(|| panic!("no split of two coupled axes from a {class}"));
        assert_eq!((part.nout(), out), (2, vec![1, 2]), "{class}");
    }
}

/// Transform positions with a SplineMap of order 1 in X and order 2 in Y that
/// has a single X coefficient and Y knots of 0, 0.5, 1, 1.5 and 2.
/// The Y knots are all distinct, so the spline is defined only between the
/// second and fourth of them; positions outside that interval, but inside the
/// range of the knots, give bad values.
fn check_spline_values(what: &str, map: &dyn Mapping) {
    let xin = [0.5; 5];
    let yin = [0.25, 0.5, 0.75, 1.5, 1.75];
    let uexp = [BAD_VALUE, 10.0, 15.0, 30.0, BAD_VALUE];
    let vexp = [BAD_VALUE, 1.0, 1.5, 3.0, BAD_VALUE];
    let (u, v) = map.tran2(&xin, &yin, true).unwrap();
    for i in 0..5 {
        let ok = if uexp[i] == BAD_VALUE {
            u[i] == BAD_VALUE && v[i] == BAD_VALUE
        } else {
            (u[i] - uexp[i]).abs() <= 1e-12 && (v[i] - vexp[i]).abs() <= 1e-12
        };
        assert!(
            ok,
            "{what} at Y={} gave ({},{}), expected ({},{})",
            yin[i], u[i], v[i], uexp[i], vexp[i]
        );
    }
}

/// A SplineMap in which the first and last knots are not repeated satisfies
/// every invariant, but its spline is not defined over the whole range of its
/// knots.
/// It is evaluated only where it is defined, as constructed and as loaded from
/// its dump.
///
/// Ported from `testloaderhardening.c`.
#[test]
fn spline_values() {
    let map = SplineMap::new(
        1,
        2,
        1,
        3,
        vec![0.0, 1.0],
        vec![0.0, 0.5, 1.0, 1.5, 2.0],
        vec![10.0, 20.0, 30.0],
        vec![1.0, 2.0, 3.0],
    )
    .unwrap();
    check_spline_values("SplineMap with distinct knots", &map);
    let dump = AnyObject::SplineMap(map).to_native_string().unwrap();
    let AnyObject::SplineMap(loaded) = from_native_string(&dump).unwrap() else {
        panic!("not a SplineMap");
    };
    check_spline_values("loaded SplineMap with distinct knots", &loaded);
}

/// A valid, non-identity permutation loads and round-trips unchanged.
///
/// Ported from `testframeperm.c`.
#[test]
fn valid_permutations() {
    let mut frame = BasicFrameBuilder::new(3).build().unwrap();
    frame.perm_axes(&[3, 1, 2]).unwrap();
    let dump1 = AnyObject::BasicFrame(frame).to_native_string().unwrap();
    let loaded = from_native_string(&dump1).unwrap();
    assert!(matches!(loaded, AnyObject::BasicFrame(_)));
    assert_eq!(loaded.to_native_string().unwrap(), dump1);

    // A CmpFrame carries its own permutation array.
    let plain = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    )
    .unwrap();
    let dump = AnyObject::CmpFrame(plain).to_native_string().unwrap();
    assert!(matches!(
        from_native_string(&dump),
        Ok(AnyObject::CmpFrame(_))
    ));

    let mut cmp = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).domain("A").build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).domain("B").build().unwrap()),
    )
    .unwrap();
    cmp.perm_axes(&[2, 1]).unwrap();
    let dump1 = AnyObject::CmpFrame(cmp).to_native_string().unwrap();
    let loaded = from_native_string(&dump1).unwrap();
    assert_eq!(loaded.to_native_string().unwrap(), dump1);
    let AnyObject::CmpFrame(cmp) = loaded else {
        panic!("not a CmpFrame");
    };
    cmp.norm(&mut [0.1, 0.2]).unwrap();
    assert_eq!(cmp.naxes(), 2);
}

/// Read an object from FITS header cards.
fn read_header(cards: &[&str]) -> AstResult<Option<AnyObject>> {
    let mut fc = FitsChan::new()?;
    for card in cards {
        fc.put_fits(card, false)?;
    }
    fc.clear_card()?;
    fc.read()
}

/// A header read, then written as a dump and as a FITS header, and used to
/// transform a position, within 10 s and without panicking.
fn survives(cards: &[&str]) {
    let last = cards.last().copied().unwrap_or_default().to_string();
    let cards: Vec<String> = cards.iter().map(|c| c.to_string()).collect();
    within_10s(move || {
        let refs: Vec<&str> = cards.iter().map(String::as_str).collect();
        let Ok(Some(obj)) = read_header(&refs) else {
            return;
        };
        if let Some(map) = obj.as_mapping()
            && map.nin() == 2
            && map.nout() == 2
        {
            let _ = map.tran2(&[10.0], &[20.0], true);
            let _ = map.tran2(&[10.0], &[20.0], false);
        }
        let _ = obj.to_native_string();
        if let Ok(mut fc) = FitsChan::new() {
            let _ = fc.write(&obj);
        }
    })
    .unwrap_or_else(|why| panic!("{last}: {why}"));
}

/// The DATE-OBS card of the FITS-WCS header written for a header read from
/// `cards`.
fn date_obs(cards: &[&str]) -> Option<String> {
    let cards: Vec<String> = cards.iter().map(|c| c.to_string()).collect();
    within_10s(move || {
        let refs: Vec<&str> = cards.iter().map(String::as_str).collect();
        let Ok(Some(obj)) = read_header(&refs) else {
            return None;
        };
        let mut fc = FitsChan::new().ok()?;
        fc.set_encoding(Encoding::FitsWcs);
        fc.write(&obj).ok()?;
        fc.clear_card().ok()?;
        fc.find_fits("DATE-OBS", false).ok()?
    })
    .unwrap()
}

/// The world coordinate step for one pixel along each axis of the FrameSet
/// read from `cards`.
fn steps(cards: &[&str]) -> Option<[f64; 2]> {
    let cards: Vec<String> = cards.iter().map(|c| c.to_string()).collect();
    within_10s(move || {
        let refs: Vec<&str> = cards.iter().map(String::as_str).collect();
        let Ok(Some(AnyObject::FrameSet(fs))) = read_header(&refs) else {
            return None;
        };
        if fs.nin() != 2 || fs.nout() != 2 {
            return None;
        }
        let (x, y) = fs
            .tran2(&[10.0, 11.0, 10.0], &[20.0, 20.0, 21.0], true)
            .ok()?;
        if [x[0], x[1], y[0], y[2]].contains(&BAD_VALUE) {
            return None;
        }
        Some([x[1] - x[0], y[2] - y[0]])
    })
    .unwrap()
}

fn assert_steps(what: &str, cards: &[&str], want: [f64; 2]) {
    let got = steps(cards).unwrap_or_else(|| panic!("{what} could not be read as a FrameSet"));
    for i in 0..2 {
        assert!(
            (got[i] - want[i]).abs() <= 1.0e-6 * want[i].abs(),
            "{what} gave steps {got:?}, not {want:?}"
        );
    }
}

const SKY: [&str; 8] = [
    "CTYPE1  = 'RA---TAN'",
    "CTYPE2  = 'DEC--TAN'",
    "CRVAL1  =                180.0",
    "CRVAL2  =                 45.0",
    "CRPIX1  =                256.5",
    "CRPIX2  =                256.5",
    "CDELT1  =              -0.0003",
    "CDELT2  =               0.0003",
];

fn with<'a>(base: &[&'a str], extra: &[&'a str]) -> Vec<&'a str> {
    base.iter().chain(extra).copied().collect()
}

/// WCSAXES values that are not a number of axes that FITS can describe are
/// refused.
#[test]
fn fits_axis_counts() {
    assert!(read_header(&with(&SKY, &["WCSAXES =                    2"])).is_ok());
    for bad in [
        "WCSAXES =           2147483647",
        "WCSAXES =                 1000",
        "WCSAXES =              1.0E300",
        "WCSAXES =                    0",
        "WCSAXES =                   -3",
    ] {
        let err = read_header(&with(&SKY, &[bad])).expect_err(bad);
        assert_eq!(err.code(), ErrorCode::Bdfts, "{bad}");
    }
}

/// Hostile headers are read, written and transformed within 10 s and without
/// a panic.
#[test]
fn fits_survives() {
    survives(&[
        "WCSAXES =                    1",
        "CTYPE1  = 'RA---TAN-SIP'",
        "CTYPE2  = 'DEC--TAN-SIP'",
        "CRVAL1  =                180.0",
        "CRVAL2  =                 45.0",
        "CRPIX1  =                256.5",
        "CRPIX2  =                256.5",
        "CD1_1   =              -0.0003",
        "CD2_2   =               0.0003",
        "A_ORDER =                    2",
        "A_2_0   =               1.0E-6",
        "B_ORDER =                    2",
        "B_0_2   =               1.0E-6",
    ]);
    survives(&[
        "CTYPE1  = ''",
        "CRVAL1  =                  1.0",
        "CRPIX1  =                  1.0",
        "CDELT1  =                  1.0",
    ]);
    survives(&[
        "CTYPE1  = 'X'",
        "CRVAL1  =                  1.0",
        "CRPIX1  =                  1.0",
        "CDELT1  =                  1.0",
    ]);
    let tnx = |wat: &'static str| {
        [
            "CTYPE1  = 'RA---TNX'",
            "CTYPE2  = 'DEC--TNX'",
            "CRVAL1  =                110.5",
            "CRVAL2  =                 55.8",
            "CRPIX1  =               4189.8",
            "CRPIX2  =                -30.1",
            "CD1_1   =              -7.6E-7",
            "CD2_1   =              -1.2E-4",
            "CD1_2   =               1.2E-4",
            "CD2_2   =              -1.3E-6",
            "WAT0_001= 'system=image'",
            "WAT1_001= 'wtype=tnx axtype=ra lngcor = \"3. 2. 2. 2. 0.005 0.49 '",
            wat,
            "WAT1_003= '0.25 0.49 -0.2 0.005 -0.001 \"'",
            "WAT2_001= 'wtype=tnx axtype=dec latcor = \"3. 2. 2. 2. 0.005 0.49 '",
            "WAT2_002= '0.25 0.49 0.15 -0.003 6.5E-4 \"'",
        ]
    };
    survives(&tnx("WAT1_002="));
    survives(&tnx("WAT1_002=                    5"));
    // Two longitude axes.
    survives(&[
        "CTYPE1  = 'RA---NCP'",
        "CTYPE2  = 'DEC--NCP'",
        "CTYPE3  = 'RA---TAN'",
        "CRVAL1  =                180.0",
        "CRVAL2  =                 45.0",
        "CRPIX1  =                256.5",
        "CRPIX2  =                256.5",
        "CDELT1  =              -0.0003",
        "CDELT2  =               0.0003",
    ]);
    // A reference latitude that has 302 digits of degrees.
    survives(&[
        "CTYPE1  = 'RA---CAR'",
        "CTYPE2  = 'DEC--CAR'",
        "CRVAL1  =                  0.0",
        "CRVAL2  =              1.0E300",
        "CRPIX1  =                  1.0",
        "CRPIX2  =                  1.0",
        "CDELT1  =                 -1.0",
        "CDELT2  =                  1.0",
    ]);
    // An equinox that puts the sidereal time far out of range.
    survives(&with(
        &SKY,
        &["RADESYS = 'FK5'", "EQUINOX =           2147483647"],
    ));
    // A pixel size so small that its reciprocal is infinite: simplifying
    // the Mapping must stop when two Mappings hold the same infinite value.
    survives(&[
        "CTYPE1  = 'WAVE'",
        "CRVAL1  =               5.5E-7",
        "CRPIX1  =                513.0",
        "CDELT1  =               1e-320",
        "CUNIT1  = 'm'",
    ]);
    // More WCS axes than pixel axes.
    survives(&[
        "WCSAXES =                   20",
        "CTYPE1  = 'PIXEL'",
        "CTYPE2  = 'PIXEL'",
        "CRVAL1  =                  0.0",
        "CRVAL2  =                  0.0",
        "CRPIX1  =                 50.0",
        "CRPIX2  =                 50.0",
        "CD1_1   =                  1.0",
        "CD1_2   =                  0.5",
        "CD2_1   =                 -0.5",
        "CD2_2   =                  1.0",
    ]);
    survives(&with(&SKY, &["WCSAXES =                   20"]));
    // A logarithmic axis with a negative reference value that has a three
    // digit exponent.
    survives(&[
        "CTYPE1  = 'FREQ-LOG'",
        "CRVAL1  = -1.2345678901234567E-300",
        "CRPIX1  =                512.5",
        "CDELT1  =                  1.0",
        "CUNIT1  = 'Hz'",
    ]);
    // A spectral axis with a rest frequency so small that the first step used
    // to find a rate of change is zero.
    survives(&[
        "CTYPE1  = 'BETA-FRQ'",
        "CRVAL1  =               1.0E-6",
        "CRPIX1  =                512.5",
        "CDELT1  =               1.0E-8",
        "RESTFRQ =               1e-320",
        "SPECSYS = 'BARYCENT'",
    ]);
    // A cube with two spectral axes.
    survives(&[
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
    // A spectral axis beside an axis that has a blank CTYPE, and so a blank
    // label.
    survives(&[
        "CTYPE1  = 'WAVE-F2W'",
        "CTYPE2  = ''",
        "CTYPE3  = 'GLAT-TAN'",
        "CRVAL1  =         2.1106114E-1",
        "CRVAL2  =               45.000",
        "CRVAL3  =                5.000",
        "CRPIX1  =              513.000",
        "CRPIX2  =               50.500",
        "CRPIX3  =                  NaN",
        "CDELT1  =         1.4868E-4",
        "CDELT2  =        -0.00083333",
        "CDELT3  =         0.00083333",
        "CUNIT1  = 'm       '",
        "RESTFRQ =       1.420405752E9",
        "SPECSYS = 'BARYCENT'",
        "MJD-OBS =            52413.59",
    ]);
}

/// DATE-OBS is written for an epoch of observation that has a calendar date,
/// and not for one that is far beyond any date.
#[test]
fn fits_date_obs() {
    let card = date_obs(&with(&SKY, &["MJD-OBS =              55000.5"])).expect("DATE-OBS");
    assert!(card[10..].starts_with("'2009-06-18T1"), "{card}");
    for far in [
        "MJD-OBS =              1.0E300",
        "MJD-OBS =               1.0E10",
    ] {
        assert_eq!(date_obs(&with(&SKY, &[far])), None, "{far}");
    }
}

/// An axis with no CDELT card, or one that is not a number, has the default
/// pixel size of 1.0, and a CDELT of zero is replaced by a millionth of CRVAL,
/// or by 1.0 if CRVAL is zero or absent.
#[test]
fn fits_pixel_steps() {
    let linear = |crval1: &'static str, cdelt1: Option<&'static str>| {
        let mut cards = vec![
            "CTYPE1  = 'XPOS'",
            "CTYPE2  = 'YPOS'",
            crval1,
            "CRVAL2  =                 10.0",
            "CRPIX1  =                 10.0",
            "CRPIX2  =                 20.0",
            "CDELT2  =                  0.5",
        ];
        cards.extend(cdelt1);
        cards
    };
    let crval = "CRVAL1  =               5000.0";
    assert_steps("CDELT2 but no CDELT1", &linear(crval, None), [1.0, 0.5]);
    assert_steps(
        "CDELT1 = 'abc'",
        &linear(crval, Some("CDELT1  = 'abc'")),
        [1.0, 0.5],
    );
    let zero = Some("CDELT1  =                  0.0");
    assert_steps(
        "CDELT1 = 0.0, CRVAL1 = 5000.0",
        &linear(crval, zero),
        [0.005, 0.5],
    );
    assert_steps(
        "CDELT1 = 0.0, CRVAL1 = 0.0",
        &linear("CRVAL1  =                  0.0", zero),
        [1.0, 0.5],
    );
    assert_steps(
        "CDELT1 = 0.0 and no CRVAL1",
        &linear("COMMENT no reference value for axis 1", zero),
        [1.0, 0.5],
    );
}

/// A celestial axis with no partner is treated in the same way whatever
/// projection its CTYPE names: a CTYPE card on its own is ignored, and a
/// header that describes the whole axis is rejected.
#[test]
fn fits_lone_celestial_axis() {
    let lone = [
        "CTYPE1  = 'XPOS'",
        "CTYPE2  = 'YPOS'",
        "CRVAL1  =                 10.0",
        "CRVAL2  =                 20.0",
        "CRPIX1  =                 50.0",
        "CRPIX2  =                100.0",
        "CDELT1  =                  1.0",
        "CDELT2  =                  1.0",
    ];
    let rest = [
        "CRVAL3  =                  5.0",
        "CRPIX3  =                  1.0",
        "CDELT3  =                  1.0",
    ];
    for ctype in [
        "CTYPE3  = 'DEC--GLS'",
        "CTYPE3  = 'RA---GLS'",
        "CTYPE3  = 'DEC--ATF'",
        "CTYPE3  = 'RA---NCP'",
        "CTYPE3  = 'DEC--NCP'",
        "CTYPE3  = 'DEC--CAR'",
        "CTYPE3  = 'RA---CAR'",
        "CTYPE3  = 'DEC--ZPX'",
        "CTYPE3  = 'RA---TNX'",
        "CTYPE3  = 'DEC--TPV'",
    ] {
        let cards = with(&lone, &[ctype]);
        read_header(&cards).unwrap_or_else(|e| panic!("{ctype} gave {e}"));
        let cards = with(&cards, &rest);
        let err = read_header(&cards).expect_err(ctype);
        assert_eq!(err.code(), ErrorCode::Bdfts, "{ctype}");
    }
}

/// A floating point keyword value that an int cannot hold is not converted to
/// one.
#[test]
fn fits_integer_overflow() {
    let mut fc = FitsChan::new().unwrap();
    fc.put_fits("NAXIS1  =              -1.0E300", false)
        .unwrap();
    fc.put_fits("NAXIS2  =                100.0", false)
        .unwrap();
    fc.clear_card().unwrap();
    assert_eq!(fc.get_fits_i(Some("NAXIS2")).unwrap(), Some(100));
    assert_eq!(fc.get_fits_i(Some("NAXIS1")).ok().flatten(), None);
}

/// An image dimension that is not a number fails a read, with the C
/// library's messages, as the dimensions are read with `astGetFitsF`; one
/// that is missing does not.
#[test]
fn fits_image_dimension_that_is_not_a_number() {
    let tail = [
        "CRVAL2  =                0.000 / Reference offset lat (deg)",
        "CRPIX2  =               50.500 / Reference pixel",
    ];
    for naxis2 in ["NAXIS2  =          `      1000", "NAXIS2  = 'abc'"] {
        let err = read_header(&with(&[naxis2], &tail)).expect_err(naxis2);
        assert_eq!(err.code(), ErrorCode::Ftcnv, "{naxis2}: {err}");
        let text = err.to_string();
        for line in [
            "astGetFitsF(FitsChan): Cannot convert FITS keyword 'NAXIS2' to floating point.",
            "astGetFitsF(FitsChan): Cannot get value for FITS keyword 'NAXIS2'.",
        ] {
            assert!(text.contains(line), "{naxis2}: {text}");
        }
    }
    assert!(read_header(&tail).is_ok());
}

/// A write reads the image dimensions from the FitsChan it writes to, as C's
/// does, so one holding an image dimension that is not a number fails it.
#[test]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]
fn fits_write_with_an_image_dimension_that_is_not_a_number() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/programs/joye_car_headers/CHIPASS_Equ.head"
    ))
    .unwrap();
    let cards: Vec<&str> = text.lines().collect();
    let obj = read_header(&cards).unwrap().unwrap();
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.put_fits("NAXIS1  = 'abc'", false).unwrap();
    let err = fc.write(&obj).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Ftcnv, "{err}");
    assert!(
        err.to_string().contains(
            "astGetFitsF(FitsChan): Cannot convert FITS keyword 'NAXIS1' to floating point."
        ),
        "{err}"
    );
}

/// A line of a dump ends at a NUL, as a C string does.
#[test]
fn a_dump_line_ends_at_a_nul() {
    let obj = from_native_string(
        " Begin ZoomMap\n Nin = 2\0 = 3\n IsA Mapping\n Zoom = 4\0\u{e9}\n End ZoomMap\n",
    )
    .unwrap();
    let map = obj.as_mapping().unwrap();
    assert_eq!(map.nin(), 2);
    assert_eq!(map.tran1(&[1.0], true).ok(), None);
    let (x, _) = map.tran2(&[1.0], &[1.0], true).unwrap();
    assert_eq!(x, [4.0]);
    // An item whose name is all NULs is a blank line, so the Object after
    // it is not that item's value; C then finds no first Mapping.
    let err = from_native_string(
        " Begin CmpMap\n Nin = 1\n IsA Mapping\n MapA =\n Begin UnitMap\n Nin = 1\n \
         IsA Mapping\n End UnitMap\n \0\0\0\0 =\n Begin UnitMap\n Nin = 1\n IsA Mapping\n \
         End UnitMap\n End CmpMap\n",
    )
    .unwrap_err();
    assert!(
        err.to_string().contains(
            "astLoad(CmpMap): The first component Mapping is missing from the CmpMap read."
        ),
        "{err}"
    );
}

/// A SpecFrame read from a dump with a standard of rest it does not know
/// fails, with the C library's messages.
#[test]
fn spec_frame_dump_with_an_unknown_standard_of_rest() {
    let dump = SpecFrame::new(SpecSystem::Freq).to_native_string().unwrap();
    for (item, message) in [
        (
            "SoR = 1e-320",
            "astRead(Channel): Invalid standard of rest description \"1e-320\".",
        ),
        (
            "AlSoR = \"Nowhere\"",
            "astRead(Channel): Invalid alignment standard of rest description \"Nowhere\".",
        ),
    ] {
        let bad = dump.replacen(" End SpecFrame", &format!(" {item}\n End SpecFrame"), 1);
        let err = from_native_string(&bad).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Attin, "{item}: {err}");
        assert!(err.to_string().contains(message), "{item}: {err}");
    }
}
