// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Plot test helpers: FITS headers read into FrameSets, and C's attribute
//! setting strings applied through the attribute-string layer.

use std::path::PathBuf;

use starlink_ast::AnyObject;
use starlink_ast::attributes::Attributes;
use starlink_ast::channel::Channel;
use starlink_ast::error::AstResult;
use starlink_ast::fitschan::FitsChan;
use starlink_ast::frames::{Frame, FrameSet};
use starlink_ast::plot::{Plot, PlotElement};

/// The FrameSet a FITS header under `tests/fixtures/wcsconv/inputs` reads as.
pub fn read_head(name: &str) -> FrameSet {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut fc = FitsChan::from_header(&text).unwrap();
    match fc.read().unwrap().unwrap() {
        AnyObject::FrameSet(fs) => fs,
        other => panic!("{name}: expected a FrameSet, got {other:?}"),
    }
}

/// An attribute name as C writes it, split into its lower-case name and
/// its axis, if any.
pub fn split_name(name: &str) -> (String, Option<usize>) {
    match name.split_once('(') {
        Some((n, rest)) => (
            n.to_lowercase(),
            Some(rest.trim_end_matches(')').parse().unwrap()),
        ),
        None => (name.to_lowercase(), None),
    }
}

/// A style attribute name as C writes it, `Colour(Axes)`, split into its
/// lower-case attribute name and its element, `None` for a name C does
/// not know.
pub fn split_style(name: &str) -> (String, Option<PlotElement>) {
    let (attr, element) = name.split_once('(').unwrap();
    let element = element.trim_end_matches(')');
    let found = PlotElement::ALL
        .into_iter()
        .find(|e| e.name().eq_ignore_ascii_case(element));
    (attr.to_lowercase(), found)
}

/// Apply an `astSet` string of Plot attributes.
pub fn apply_settings(plot: &mut Plot, settings: &str) -> AstResult<()> {
    plot.set_attributes(settings)
}

/// One row of `fixtures/plot/cases.txt`, the grid test cases.
pub struct GridCase {
    pub name: String,
    /// The header's file name under `fixtures/wcsconv/inputs`.
    pub head: String,
    /// Plot attributes, as C's `astSet` takes them.
    pub attr: String,
    /// FitsChan attributes for reading the header.
    pub fattr: String,
    /// The plot box, or `None` for the header's pixel grid.
    pub pbox: Option<[f64; 4]>,
    /// The reference SVG's path under `fixtures`.
    pub reference: String,
    /// Whether C compares only that the case runs, not its labels.
    pub skip_text_compare: bool,
}

/// The grid test cases of `fixtures/plot/cases.txt`.
pub fn grid_cases() -> Vec<GridCase> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plot/cases.txt");
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && l.contains('|'))
        .map(|l| {
            let f: Vec<&str> = l.split('|').map(str::trim).collect();
            let pbox = (!f[4].is_empty()).then(|| {
                let v: Vec<f64> = f[4]
                    .split_whitespace()
                    .map(|x| x.parse().unwrap())
                    .collect();
                [v[0], v[1], v[2], v[3]]
            });
            GridCase {
                name: f[0].to_string(),
                head: f[1].rsplit('/').next().unwrap().to_string(),
                attr: f[2].to_string(),
                fattr: f[3].to_string(),
                pbox,
                reference: f[5].to_string(),
                skip_text_compare: f.get(6).is_some_and(|s| *s == "yes"),
            }
        })
        .collect()
}

/// The plot box `testgrid.c` uses for a header when none is given: its
/// pixel grid, from `NAXIS1` and `NAXIS2`, each 100 if absent.
fn naxis_box(text: &str) -> [f64; 4] {
    let naxis = |key: &str| {
        text.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l[10..].split_whitespace().next()?.parse::<i32>().ok())
            .unwrap_or(100)
    };
    [
        0.5,
        0.5,
        f64::from(naxis("NAXIS1  =")) + 0.5,
        f64::from(naxis("NAXIS2  =")) + 0.5,
    ]
}

/// The graphics box `testgrid.c` uses for a plot box: 0.1 to 0.9, shrunk
/// on one axis to keep the plot box's aspect ratio, in single precision as
/// C computes it.
pub fn testgrid_gbox(pbox: [f64; 4]) -> [f32; 4] {
    let mut gbox = [0.1f32, 0.1, 0.9, 0.9];
    let mut asp = ((pbox[3] - pbox[1]) / (pbox[2] - pbox[0])) as f32;
    if !(0.05..=20.0).contains(&asp) {
        asp = 1.0;
    }
    if asp > 1.0 {
        let delta = 0.5f32 * ((gbox[2] - gbox[0]) - (gbox[3] - gbox[1]) / asp);
        gbox[2] -= delta;
        gbox[0] += delta;
    } else {
        let delta = 0.5f32 * ((gbox[3] - gbox[1]) - asp * (gbox[2] - gbox[0]));
        gbox[3] -= delta;
        gbox[1] += delta;
    }
    gbox
}

/// The Plot of a grid test case, as `testgrid.c` builds it: the header read
/// through a FitsChan with the case's attributes, a Plot of it titled
/// "A FITS test", and the case's attributes set.
pub fn testgrid_plot(case: &GridCase) -> Plot {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(&case.head);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_attributes(&case.fattr).unwrap();
    // The Native encoding can hold any Frame, which astPlot takes as it
    // takes a FrameSet.
    let object = fc.read().unwrap().unwrap();
    let frame = object.as_frame().expect("a Frame or FrameSet");
    let pbox = case.pbox.unwrap_or_else(|| naxis_box(&text));
    let mut plot = Plot::new(frame, testgrid_gbox(pbox), pbox).unwrap();
    plot.set_title("A FITS test").unwrap();
    apply_settings(&mut plot, &case.attr).unwrap();
    plot
}

/// The 64-bit FNV-1a hash of a text, which the drawing probes record for
/// long SVGs.
pub fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// A backend that reports a failure from its `n`th polyline on.
pub struct FailingGrf {
    pub inner: starlink_ast::grf::LogGrf<Vec<u8>>,
    pub lines_left: usize,
}

impl starlink_ast::grf::Grf for FailingGrf {
    fn line(&mut self, x: &[f32], y: &[f32]) -> starlink_ast::error::AstResult<()> {
        if self.lines_left == 0 {
            return Err(starlink_ast::error::AstError::new(
                starlink_ast::error::ErrorCode::Grfer,
                "out of ink",
            ));
        }
        self.lines_left -= 1;
        self.inner.line(x, y)
    }
    fn mark(&mut self, x: &[f32], y: &[f32], m: i32) -> starlink_ast::error::AstResult<()> {
        self.inner.mark(x, y, m)
    }
    fn text(
        &mut self,
        t: &str,
        x: f32,
        y: f32,
        j: starlink_ast::grf::Justification,
        up: [f32; 2],
    ) -> starlink_ast::error::AstResult<()> {
        self.inner.text(t, x, y, j, up)
    }
    fn text_extent(
        &mut self,
        t: &str,
        x: f32,
        y: f32,
        j: starlink_ast::grf::Justification,
        up: [f32; 2],
    ) -> starlink_ast::error::AstResult<starlink_ast::grf::TextBox> {
        self.inner.text_extent(t, x, y, j, up)
    }
    fn char_height(&mut self) -> starlink_ast::error::AstResult<[f32; 2]> {
        self.inner.char_height()
    }
    fn scales(&mut self) -> starlink_ast::error::AstResult<[f32; 2]> {
        self.inner.scales()
    }
    fn attr(
        &mut self,
        a: starlink_ast::grf::GrfAttr,
        v: Option<f64>,
        p: starlink_ast::grf::GrfPrimitive,
    ) -> starlink_ast::error::AstResult<f64> {
        self.inner.attr(a, v, p)
    }
    fn cap(&mut self, c: starlink_ast::grf::GrfCap, v: i32) -> bool {
        self.inner.cap(c, v)
    }
}
