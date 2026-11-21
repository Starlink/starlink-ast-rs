// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The grid test cases, with their FITS headers built into the program.

use starlink_ast::AnyObject;
use starlink_ast::channel::Channel;
use starlink_ast::error::{AstError, AstResult, ErrorCode};
use starlink_ast::fitschan::FitsChan;
use starlink_ast::frames::Frame;
use starlink_ast::plot::Plot;

use crate::settings::apply_settings;

/// The grid test cases, one per line: name, header path, Plot attributes,
/// FitsChan attributes, plot box, reference SVG and whether only the run is
/// checked, separated by `|`.
const CASES: &str = include_str!("../../starlink-ast/tests/fixtures/plot/cases.txt");

/// The FITS headers the cases name, by file name.
const HEADERS: [(&str, &str); 20] = [
    (
        "aitoff.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/aitoff.head"),
    ),
    (
        "car1.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/car1.head"),
    ),
    (
        "car2.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/car2.head"),
    ),
    (
        "car3.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/car3.head"),
    ),
    (
        "car4.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/car4.head"),
    ),
    (
        "car5.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/car5.head"),
    ),
    (
        "car6.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/car6.head"),
    ),
    (
        "cobe.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/cobe.head"),
    ),
    (
        "hpx.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/hpx.head"),
    ),
    (
        "origin.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/origin.head"),
    ),
    (
        "polco.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/polco.head"),
    ),
    (
        "polco2.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/polco2.head"),
    ),
    (
        "scp.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/scp.head"),
    ),
    (
        "serpens.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/serpens.head"),
    ),
    (
        "specflux.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/specflux.head"),
    ),
    (
        "timeplot.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/timeplot.head"),
    ),
    (
        "tnx.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/tnx.head"),
    ),
    (
        "tsc.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/tsc.head"),
    ),
    (
        "zpn.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/zpn.head"),
    ),
    (
        "zpx.head",
        include_str!("../../starlink-ast/tests/fixtures/wcsconv/inputs/zpx.head"),
    ),
];

/// One grid test case.
#[derive(Debug, Clone, Copy)]
pub struct DemoCase {
    /// The case's name.
    pub name: &'static str,
    /// The FITS header's file name.
    pub head: &'static str,
    /// The FITS header's text.
    pub header: &'static str,
    /// Plot attributes, in `astSet` syntax.
    pub attr: &'static str,
    /// FitsChan attributes for reading the header.
    pub fattr: &'static str,
    /// The plot box, or `None` for the header's pixel grid.
    pub pbox: Option<[f64; 4]>,
}

/// The grid test cases, in the order the case list gives them.
///
/// # Panics
///
/// When the built-in case list names a header that is not built in, or
/// has a malformed plot box; the unit tests check that neither happens.
pub fn cases() -> Vec<DemoCase> {
    CASES
        .lines()
        .filter(|l| !l.starts_with('#') && l.contains('|'))
        .map(|l| {
            let f: Vec<&str> = l.split('|').map(str::trim).collect();
            let head = f[1].rsplit('/').next().unwrap_or(f[1]);
            let header = HEADERS
                .iter()
                .find(|(name, _)| *name == head)
                .map(|(_, text)| *text)
                .unwrap_or_else(|| panic!("no built-in header {head}"));
            let pbox = (!f[4].is_empty()).then(|| {
                let v: Vec<f64> = f[4]
                    .split_whitespace()
                    .map(|x| x.parse().expect("a plot box value"))
                    .collect();
                [v[0], v[1], v[2], v[3]]
            });
            DemoCase {
                name: f[0],
                head,
                header,
                attr: f[2],
                fattr: f[3],
                pbox,
            }
        })
        .collect()
}

/// The case's plot box: the one it gives, or else the header's pixel grid,
/// from `NAXIS1` and `NAXIS2`, each 100 if absent.
pub fn plot_box(case: &DemoCase) -> [f64; 4] {
    if let Some(pbox) = case.pbox {
        return pbox;
    }
    let naxis = |key: &str| {
        case.header
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.get(10..)?.split_whitespace().next()?.parse::<i32>().ok())
            .unwrap_or(100)
    };
    [
        0.5,
        0.5,
        f64::from(naxis("NAXIS1  =")) + 0.5,
        f64::from(naxis("NAXIS2  =")) + 0.5,
    ]
}

/// The graphics box for a drawing area `width` by `height`: the area less a
/// tenth of it on each side, shrunk on one axis to keep the aspect ratio of
/// `pbox`, and centered.
///
/// An aspect ratio outside 0.05 to 20 counts as 1.
pub fn graphics_box(width: f32, height: f32, pbox: [f64; 4]) -> [f32; 4] {
    let (x0, y0) = (0.1 * width, 0.1 * height);
    let (w, h) = (0.8 * width, 0.8 * height);
    let mut asp = ((pbox[3] - pbox[1]) / (pbox[2] - pbox[0])) as f32;
    if !(0.05..=20.0).contains(&asp) {
        asp = 1.0;
    }
    if h > asp * w {
        let gh = asp * w;
        let y = y0 + 0.5 * (h - gh);
        [x0, y, x0 + w, y + gh]
    } else {
        let gw = h / asp;
        let x = x0 + 0.5 * (w - gw);
        [x, y0, x + gw, y0 + h]
    }
}

/// The Plot of a case: its header read through a FitsChan with the case's
/// FitsChan attributes, plotted in `gbox` with the title "A FITS test", and
/// `attr` applied.
///
/// # Errors
///
/// - `ErrorCode::Badat` for a FitsChan attribute other than `carlin=1`.
/// - `ErrorCode::Badin` when the header holds no coordinate system.
/// - Any error reading the header, building the Plot or applying `attr`.
pub fn build_plot(case: &DemoCase, attr: &str, gbox: [f32; 4]) -> AstResult<Plot> {
    let mut fc = FitsChan::from_header(case.header)?;
    for setting in case.fattr.split(',').filter(|s| !s.is_empty()) {
        match setting.trim().to_lowercase().as_str() {
            "carlin=1" => fc.set_car_lin(true),
            _ => {
                return Err(AstError::new(
                    ErrorCode::Badat,
                    format!("unknown FitsChan setting \"{setting}\""),
                ));
            }
        }
    }
    let no_object = || {
        AstError::new(
            ErrorCode::Badin,
            format!("{} holds no coordinate system", case.head),
        )
    };
    let object: AnyObject = fc.read()?.ok_or_else(no_object)?;
    let frame = object.as_frame().ok_or_else(no_object)?;
    let mut plot = Plot::new(frame, gbox, plot_box(case))?;
    plot.set_title("A FITS test")?;
    apply_settings(&mut plot, attr)?;
    Ok(plot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use starlink_ast::grf::LogGrf;
    use starlink_ast::plot::Draw;

    #[test]
    fn every_case_has_its_header() {
        let cases = cases();
        assert_eq!(cases.len(), 20);
        for c in &cases {
            assert_eq!(c.head, format!("{}.head", c.name));
            assert!(!c.header.is_empty(), "{}", c.name);
        }
    }

    #[test]
    fn graphics_box_keeps_the_aspect_ratio() {
        assert_eq!(
            graphics_box(1000.0, 500.0, [0.0, 0.0, 100.0, 100.0]),
            [300.0, 50.0, 700.0, 450.0]
        );
        assert_eq!(
            graphics_box(500.0, 1000.0, [0.0, 0.0, 200.0, 100.0]),
            [50.0, 400.0, 450.0, 600.0]
        );
        // An aspect ratio of 100 counts as 1.
        assert_eq!(
            graphics_box(1000.0, 500.0, [0.0, 0.0, 1.0, 100.0]),
            [300.0, 50.0, 700.0, 450.0]
        );
    }

    #[test]
    fn every_case_draws_through_log_grf() {
        for c in cases() {
            let gbox = graphics_box(800.0, 600.0, plot_box(&c));
            let mut plot = build_plot(&c, c.attr, gbox).unwrap();
            plot.draw_grid(&mut LogGrf::new(Vec::new())).unwrap();
        }
    }
}
