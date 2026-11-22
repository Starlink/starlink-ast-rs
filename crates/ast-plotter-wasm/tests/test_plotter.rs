// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The plotter's three outputs against `tools/oracle_fixtures/fits_plotter_probe.c`,
//! which runs the steps of `fplottest.f` on the C library.
//!
//! Each fixture holds the output text, or a first line `ERROR` and then the
//! message lines that `PlotResult` carries in `Err`.

use std::path::{Path, PathBuf};

use ast_plotter::{Example, PlotRequest, PlotResult, parse_examples, plot};

/// Outputs that do not match the C library yet: the case, the output and
/// the reason. Each is a port defect found by this comparison, recorded in
/// `PLAN.md`; `exclusions_still_differ` fails when one starts to match.
const PLOTTER_EXCLUDED: &[(&str, &str, &str)] = &[
    ("solar", "fits", PV_IN_PLACE),
    ("solar_version_a", "fits", PV_IN_PLACE),
    ("solar_version_pixel", "fits", PV_IN_PLACE),
    ("solar", "ast", ROTATION_INVERSE),
    ("solar_version_a", "ast", ROTATION_INVERSE),
    ("solar_version_pixel", "ast", ROTATION_INVERSE),
    ("cobe", "svg", COBE_CURVE),
];

const PV_IN_PLACE: &str = "C's FITS-WCS write puts PV2_1B over the card the read used, \
    after CRVAL2B and with the header's comment; the port writes it as a new card after \
    the CDELTs";
const ROTATION_INVERSE: &str = "the heliographic 3x3 rotation MatrixMap: C's inverse \
    values IM0, IM6 and IM8 differ from the forward matrix's transpose by 1 to 2 ULP, \
    the port's are the transpose exactly";
const COBE_CURVE: &str = "COBE's grid on this case's plot box, pixels 0.5 to 300.5: 12 \
    polylines and 2 labels differ from C's, closed parallels starting at other points \
    around their circles and some tick marks drawn differently; the grid tests' COBE \
    case, with nearly the same settings on the plot box 10 -10 300 280, matches C's \
    call log";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn examples() -> Vec<Example> {
    let text = std::fs::read_to_string(root().join("web/plotter/examples.txt")).unwrap();
    parse_examples(&text)
}

fn example(name: &str) -> Example {
    examples()
        .into_iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no example {name}"))
}

fn request(
    header: &str,
    plot_settings: &str,
    fitschan_settings: &str,
    version: char,
) -> PlotRequest {
    PlotRequest {
        header: header.to_string(),
        plot_settings: plot_settings.to_string(),
        fitschan_settings: fitschan_settings.to_string(),
        width: 600.0,
        height: 600.0,
        version,
        page_look: false,
    }
}

/// Every case the probe writes, with its inputs, as the probe makes them.
fn cases() -> Vec<(String, PlotRequest)> {
    let mut cases: Vec<(String, PlotRequest)> = examples()
        .iter()
        .map(|e| {
            (
                e.name.to_lowercase(),
                request(&e.header, &e.settings, "", e.version),
            )
        })
        .collect();
    let derive = |from: &str, settings: Option<&str>, fsettings: &str, version: char| {
        let e = example(from);
        request(
            &e.header,
            settings.unwrap_or(&e.settings),
            fsettings,
            version,
        )
    };
    cases.push(("solar_version_a".into(), derive("SOLAR", None, "", 'A')));
    cases.push(("solar_version_pixel".into(), derive("SOLAR", None, "", '$')));
    cases.push(("bad_version".into(), derive("AITOFF", None, "", 'Z')));
    cases.push((
        "bad_plot_setting".into(),
        derive("AITOFF", Some("Fred=1"), "", ' '),
    ));
    cases.push((
        "bad_fitschan_setting".into(),
        derive("AITOFF", None, "Encoding=nonsense", ' '),
    ));
    cases.push((
        "no_wcs".into(),
        request("SIMPLE  =                    T\n", "", "", ' '),
    ));
    // Headers the probe makes: AITOFF with no pixels, and a ZoomMap in the
    // Native encoding.
    let made = |case: &str| {
        let path = root().join(format!(
            "crates/ast-plotter-wasm/tests/fixtures/{case}.head"
        ));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    };
    cases.push((
        "zero_naxis".into(),
        request(&made("zero_naxis"), &example("AITOFF").settings, "", ' '),
    ));
    cases.push((
        "native_zoommap".into(),
        request(&made("native_zoommap"), "", "", ' '),
    ));
    cases
}

/// A fixture as the `Result` the plotter returns.
fn fixture(case: &str, ext: &str) -> Result<String, String> {
    let path = root().join(format!(
        "crates/ast-plotter-wasm/tests/fixtures/{case}.{ext}"
    ));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match text.strip_prefix("ERROR\n") {
        Some(messages) => Err(messages.strip_suffix('\n').unwrap_or(messages).to_string()),
        None => Ok(text),
    }
}

fn outputs(result: &PlotResult) -> [(&'static str, &Result<String, String>); 3] {
    [
        ("fits", &result.fits),
        ("ast", &result.ast),
        ("svg", &result.svg),
    ]
}

/// The outputs of a case that differ from the C library's: each output's
/// name and the two texts.
fn differences(case: &str, request: &PlotRequest) -> Vec<(&'static str, String)> {
    let result = plot(request);
    outputs(&result)
        .into_iter()
        .filter(|(ext, got)| **got != fixture(case, ext))
        .map(|(ext, got)| {
            let shown = format!(
                "{case}.{ext}:\n--- port\n{}\n--- C\n{}",
                show(got),
                show(&fixture(case, ext))
            );
            (ext, shown)
        })
        .collect()
}

fn excluded(case: &str, ext: &str) -> bool {
    PLOTTER_EXCLUDED
        .iter()
        .any(|(c, e, _)| *c == case && *e == ext)
}

fn show(r: &Result<String, String>) -> String {
    match r {
        Ok(text) => text.chars().take(2000).collect(),
        Err(messages) => format!("ERROR\n{messages}"),
    }
}

#[test]
fn outputs_match_the_c_library() {
    let cases = cases();
    assert_eq!(cases.len(), 16);
    let failures: Vec<String> = cases
        .iter()
        .flat_map(|(name, request)| {
            differences(name, request)
                .into_iter()
                .filter(|(ext, _)| !excluded(name, ext))
                .map(|(_, shown)| shown)
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn exclusions_still_differ() {
    let cases = cases();
    for (name, ext, reason) in PLOTTER_EXCLUDED {
        let (_, request) = cases
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("excluded case {name} does not exist"));
        assert!(
            differences(name, request).iter().any(|(e, _)| e == ext),
            "{name}.{ext} now matches C; remove it from PLOTTER_EXCLUDED ({reason})"
        );
    }
}

#[test]
fn a_header_with_no_wcs_reports_no_object() {
    let result = plot(&request("SIMPLE  =                    T\n", "", "", ' '));
    for (ext, got) in outputs(&result) {
        assert_eq!(
            *got,
            Err("!!! No object read from file!!!".to_string()),
            "{ext}"
        );
    }
}

#[test]
fn markup_in_a_title_is_escaped() {
    let e = example("AITOFF");
    let settings = format!("{}, Title=a<b", e.settings);
    let svg = plot(&request(&e.header, &settings, "", ' ')).svg.unwrap();
    assert!(svg.contains(">a&lt;b</text>"), "{svg}");
}

#[test]
fn absurd_sizes_are_errors() {
    let e = example("AITOFF");
    let good = plot(&request(&e.header, &e.settings, "", ' '));
    let range = "must be a whole number of pixels from 16 to 4096";
    let cases = [
        (0.0, 600.0, format!("The image width {range}, not 0.")),
        (-5.0, 600.0, format!("The image width {range}, not -5.")),
        (600.5, 600.0, format!("The image width {range}, not 600.5.")),
        (4097.0, 600.0, format!("The image width {range}, not 4097.")),
        (
            1e12,
            600.0,
            format!("The image width {range}, not 1000000000000."),
        ),
        (
            f64::NAN,
            600.0,
            format!("The image width is not a number; it {range}."),
        ),
        (600.0, 15.0, format!("The image height {range}, not 15.")),
        (
            0.0,
            -1.0,
            format!("The image width {range}, not 0.\nThe image height {range}, not -1."),
        ),
    ];
    for (width, height, message) in cases {
        let mut r = request(&e.header, &e.settings, "", ' ');
        r.width = width;
        r.height = height;
        let result = plot(&r);
        assert_eq!(result.svg, Err(message), "{width} by {height}");
        assert_eq!(result.fits, good.fits);
        assert_eq!(result.ast, good.ast);
    }
    let mut r = request(&e.header, &e.settings, "", ' ');
    r.width = 16.0;
    r.height = 4096.0;
    assert!(plot(&r).svg.is_ok(), "16 by 4096 is allowed");
}

#[test]
fn the_examples_file_has_the_eight_examples() {
    let names: Vec<String> = examples().into_iter().map(|e| e.name).collect();
    assert_eq!(
        names,
        [
            "AITOFF", "COBE", "HPX", "ORIGIN", "SCP", "SPEC2D", "SOLAR", "NCP"
        ]
    );
}

/// Drawn as the page draws it, a negative label's minus sign is U+2212, as
/// long as a digit, and the hyphens of the title are left alone; drawn as
/// C's logging backend draws it, the sign is an ASCII hyphen, as in C's own
/// SVG.
#[test]
fn the_page_draws_minus_signs_as_minus_signs() {
    let e = example("ORIGIN");
    let mut req = request(&e.header, &e.settings, "", e.version);
    req.page_look = true;
    let svg = plot(&req).svg.unwrap();
    assert!(svg.contains(">\u{2212}0</text>"), "{svg}");
    assert!(!svg.contains(">-0</text>"), "{svg}");
    let e = example("AITOFF");
    let mut req = request(&e.header, &e.settings, "", e.version);
    req.page_look = true;
    let svg = plot(&req).svg.unwrap();
    assert!(svg.contains(">\u{2212}30</text>"), "{svg}");
    assert!(svg.contains("Hammer-Aitoff"), "{svg}");
    req.page_look = false;
    let svg = plot(&req).svg.unwrap();
    assert!(svg.contains(">-30</text>"), "{svg}");
    assert!(!svg.contains('\u{2212}'), "{svg}");
}
