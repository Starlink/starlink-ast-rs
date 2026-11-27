// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Line-level mutations of Native dumps and FITS headers, after `fuzzlib.py`
//! of the C fuzz campaign against the C library's loaders (see `README.md`).
//!
//! libFuzzer's own mutations change bytes, which mostly makes a dump fail to
//! parse at all.
//! These change one line as a person editing a file might: an item value
//! replaced by an extreme one, a line deleted, duplicated or swapped, the
//! dump cut short, or an Object's class replaced.
//! Half of the mutations are still libFuzzer's own.

/// The kind of input a target reads.
#[derive(Clone, Copy)]
pub enum Kind {
    /// A Native dump.
    Native,
    /// A FITS header, one card per line.
    Fits,
}

/// Values put in place of a Native item value.
const NATIVE_VALUES: &[&str] = &[
    "\"abc\"",
    "-1",
    "0",
    "1",
    "2",
    "3",
    "99999999999",
    "2147483647",
    "-2147483648",
    "2147483648",
    "-2147483649",
    "1e999",
    "-1e308",
    "1e308",
    "nan",
    "inf",
    "-inf",
    "<bad>",
    "",
    "0x7fffffff",
    "1e-320",
    "100000",
    "65536",
    "-0",
    "1.5",
];

/// Class names put in place of the class on a `Begin` line.
const CLASSES: &[&str] = &[
    "FrameSet",
    "Frame",
    "SkyFrame",
    "SpecFrame",
    "TimeFrame",
    "CmpFrame",
    "CmpMap",
    "ZoomMap",
    "WinMap",
    "MatrixMap",
    "PermMap",
    "UnitMap",
    "ShiftMap",
    "PolyMap",
    "ChebyMap",
    "LutMap",
    "WcsMap",
    "SphMap",
    "MathMap",
    "SpecMap",
    "TimeMap",
    "SkyAxis",
    "Axis",
    "TranMap",
    "SlaMap",
    "FluxFrame",
    "SpecFluxFrame",
    "Region",
    "Box",
    "Circle",
    "Polygon",
    "Interval",
    "NullRegion",
    "CmpRegion",
    "KeyMap",
    "Plot",
    "Plot3D",
    "SplineMap",
    "SelectorMap",
    "SwitchMap",
    "UnitNormMap",
    "RateMap",
    "NormMap",
    "PcdMap",
    "GrismMap",
    "IntraMap",
    "Moc",
    "DSBSpecFrame",
    "Stc",
    "XphMap",
    "DssMap",
    "Ellipse",
    "PointList",
    "Prism",
    "FitsTable",
    "Table",
    "MocChan",
    "Channel",
    "FitsChan",
    "NoSuchClass",
];

/// Values put in place of a FITS card value.
const FITS_VALUES: &[&str] = &[
    "'abc'",
    "1.0E99999",
    "   ",
    "-0",
    "NaN",
    "Inf",
    "-Inf",
    "1e-320",
    "-1",
    "0",
    "1",
    "3",
    "99",
    "100000",
    "2147483647",
    "-2147483648",
    "2147483648",
    "1e300",
    "-1e300",
    "T",
    "'",
    "''",
    "1.5",
];

/// Values put in place of an axis count.
const AXIS_COUNTS: &[&str] = &[
    "2147483647",
    "100000",
    "99",
    "100",
    "-1",
    "0",
    "1",
    "1000",
    "-2147483648",
    "2147483648",
];

/// Values put in place of a WCS parameter value.
const WCS_VALUES: &[&str] = &["'abc'", "NaN", "T", "'1.0'", "1.0E99999", "   ", "1e300"];

/// Cards inserted into a FITS header.
const EXTRA_CARDS: &[&str] = &[
    "WCSAXES =                    9",
    "A_ORDER =                    9",
    "B_ORDER =                    9",
    "A_ORDER =               100000",
    "A_2_0   =                 1e-5",
    "PV2_99  =                    1",
    "PV1_3   =                  400",
    "LONPOLE =                1e300",
    "LATPOLE =                  999",
    "CTYPE1  = 'RA---TPV'",
    "CTYPE1  = 'RA---TAN-SIP'",
    "CTYPE2  = 'DEC--TAN-SIP'",
    "WCSAXES =                    0",
    "NAXIS   =                    0",
    "CTYPE3  = 'RA---TAN'",
    "CD1_1   =                    0",
    "CDELT1  =                    0",
    "PC1_1   =                    0",
    "PC2_2   =                    0",
    "CROTA2  =                1e300",
    "EQUINOX =                   -1",
    "MJD-OBS =                1e300",
    "DATE-OBS= 'garbage'",
    "RADESYS = 'XYZ'",
    "PROJP1  =                    1",
    "CUNIT1  = 'furlong'",
    "WCSAXES =           2147483647",
    "NAXIS   =           2147483647",
    "WCSAXES =                  100",
    "NAXIS1  =           2147483647",
    "NAXIS   =                  999",
    "CTYPE1  = 'RA---TAB'",
    "PS1_0   = 'WCS-TAB'",
    "PV1_0   =           2147483647",
    "CTYPE1  = 'RA---ZPN'",
    "PV2_1   =                 1e10",
    "CTYPE1  = 'RA---HPX'",
    "CTYPE1  = 'RA---XPH'",
    "CTYPE3  = 'FREQ-F2W'",
    "SPECSYS = 'BADSYS'",
    "TIMESYS = 'XXX'",
    "MJDREF  =                  nan",
    "CTYPE1  = 'GLON-TAN'",
    "CTYPE2  = 'DEC--XXX'",
    "CDELT1A =                    1",
    "CTYPE1A = 'RA---TAN'",
    "NAXIS3  =                   -1",
    "WCSNAME = 'x'",
    "CTYPE1  = 'RA---CAR'",
    "CTYPE1  = ''",
    "CTYPE2  = 'RA---TAN'",
];

/// CTYPE values, mostly malformed.
const BAD_CTYPES: &[&str] = &[
    "'RA---XXX'",
    "'RA--TAN'",
    "''",
    "'GLON-TAN-SIP'",
    "'FREQ-F2W'",
    "'RA---TAN-TAB'",
    "'DEC--RA'",
    "'RA---TPN'",
    "'VELO-XXX'",
    "'RA---TAN-SIP-X'",
    "'X'",
    "'RA---ZPN'",
    "'RA---HPX'",
    "'RA---XPH'",
    "'DEC--TAN'",
    "'RA---TAN'",
    "'UTC'",
    "'STOKES'",
    "'COMPLEX'",
    "'RA---AZP'",
    "'RA---SZP'",
];

/// SplitMix64, seeded from libFuzzer's seed so a mutation can be repeated.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [0, n), or 0 when `n` is 0.
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

type Line = Vec<u8>;

fn trimmed(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(line.len());
    &line[start..]
}

fn starts(line: &[u8], word: &str) -> bool {
    trimmed(line).starts_with(word.as_bytes())
}

/// Indices of the lines for which `keep` holds.
fn indices(lines: &[Line], keep: impl Fn(&[u8]) -> bool) -> Vec<usize> {
    (0..lines.len()).filter(|&i| keep(&lines[i])).collect()
}

/// `line` with everything after its `=` replaced by ` value`.
fn with_value(line: &[u8], value: &str) -> Line {
    match line.iter().position(|&b| b == b'=') {
        Some(eq) => [&line[..=eq], b" ", value.as_bytes()].concat(),
        None => line.to_vec(),
    }
}

/// The integer after the `=` of `line`, if that is all its value is.
fn integer_value(line: &[u8]) -> Option<i64> {
    let eq = line.iter().position(|&b| b == b'=')?;
    let text = std::str::from_utf8(&line[eq + 1..]).ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// Delete, duplicate or truncate at a line: the mutations both kinds share.
fn structural(lines: &mut Vec<Line>, r: &mut Rng, k: f64) {
    let n = lines.len();
    let i = r.below(n);
    if k < 0.5 {
        lines.remove(i);
    } else if k < 0.8 {
        let copy = lines[r.below(n)].clone();
        lines.insert(i, copy);
    } else {
        lines.truncate(i);
    }
}

fn mutate_native(lines: &mut Vec<Line>, r: &mut Rng) {
    let values = indices(lines, |l| {
        l.contains(&b'=')
            && !starts(l, "#")
            && !starts(l, "Begin")
            && !starts(l, "End")
            && !starts(l, "IsA")
    });
    let begins = indices(lines, |l| starts(l, "Begin "));
    let ends = indices(lines, |l| starts(l, "End "));
    let k = r.unit();
    if k < 0.45 && !values.is_empty() {
        let i = *r.pick(&values);
        lines[i] = with_value(&lines[i], r.pick(NATIVE_VALUES));
    } else if k < 0.60 && !values.is_empty() {
        let i = *r.pick(&values);
        let value = match integer_value(&lines[i]) {
            Some(v) => {
                let options = [
                    v.wrapping_sub(1),
                    v.wrapping_add(1),
                    v.wrapping_mul(2),
                    v.wrapping_neg(),
                    v.wrapping_add(100),
                    0,
                ];
                r.pick(&options).to_string()
            }
            None => r.pick(NATIVE_VALUES).to_string(),
        };
        lines[i] = with_value(&lines[i], &value);
    } else if k < 0.84 {
        structural(lines, r, (k - 0.60) / 0.24);
    } else if k < 0.90 && !begins.is_empty() && !ends.is_empty() {
        let (i, j) = (*r.pick(&begins), *r.pick(&ends));
        lines.swap(i, j);
    } else if !begins.is_empty() {
        let i = *r.pick(&begins);
        let line = &lines[i];
        let at = line.windows(6).position(|w| w == b"Begin ").unwrap() + 6;
        let rest = &line[at..];
        let end = rest
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(rest.len());
        lines[i] = [&line[..at], r.pick(CLASSES).as_bytes(), &rest[end..]].concat();
    } else {
        structural(lines, r, 0.0);
    }
}

/// `line` with its value, from column 11, replaced by `value`: a string
/// value starts there, any other is right-justified to column 30.
fn with_card_value(line: &[u8], value: &str) -> Line {
    let head = &line[..10.min(line.len())];
    if value.starts_with('\'') {
        [head, value.as_bytes()].concat()
    } else {
        [head, format!("{value:>20}").as_bytes()].concat()
    }
}

fn mutate_fits(lines: &mut Vec<Line>, r: &mut Rng) {
    let valued = indices(lines, |l| l.len() > 10 && &l[8..10] == b"= ");
    let numeric: Vec<usize> = valued
        .iter()
        .copied()
        .filter(|&i| {
            let v = trimmed(&lines[i][10..]);
            v.first().is_some_and(|b| b"-+0123456789.".contains(b))
        })
        .collect();
    let keyed = |prefixes: &[&str]| -> Vec<usize> {
        valued
            .iter()
            .copied()
            .filter(|&i| prefixes.iter().any(|p| lines[i].starts_with(p.as_bytes())))
            .collect()
    };
    let k = r.unit();
    let (targets, values): (Vec<usize>, &[&str]) = if k < 0.35 {
        (numeric, FITS_VALUES)
    } else if k < 0.45 {
        (valued.clone(), FITS_VALUES)
    } else if k < 0.55 {
        (keyed(&["NAXIS", "WCSAXES"]), AXIS_COUNTS)
    } else if k < 0.62 {
        (keyed(&["CTYPE"]), BAD_CTYPES)
    } else if k < 0.70 {
        let wcs = [
            "CRVAL", "CDELT", "PC", "CD", "CRPIX", "PV", "PS", "LONPOLE", "LATPOLE",
        ];
        (keyed(&wcs), WCS_VALUES)
    } else {
        (Vec::new(), &[])
    };
    if !targets.is_empty() {
        let i = *r.pick(&targets);
        lines[i] = with_card_value(&lines[i], r.pick(values));
        return;
    }
    let n = lines.len();
    let i = r.below(n);
    if k < 0.82 {
        structural(lines, r, if k < 0.76 { 0.6 } else { 0.0 });
    } else if k < 0.90 {
        lines.insert(i, r.pick(EXTRA_CARDS).as_bytes().to_vec());
    } else if k < 0.93 {
        if let Some(end) = lines.iter().position(|l| l.starts_with(b"END")) {
            lines.remove(end);
        }
    } else if k < 0.97 {
        let cut = r.below(lines[i].len().max(1));
        lines[i].truncate(cut);
    } else {
        let at = r.below(lines[i].len().max(1));
        if at < lines[i].len() {
            lines[i][at] = 32 + r.below(95) as u8;
        }
    }
}

/// Mutate the first `size` bytes of `data` in place, as a `fuzz_mutator!`
/// does, returning the new size, at most `max_size`.
pub fn mutate(data: &mut [u8], size: usize, max_size: usize, seed: u32, kind: Kind) -> usize {
    let mut r = Rng(u64::from(seed));
    if r.unit() < 0.5 {
        return libfuzzer_sys::fuzzer_mutate(data, size, max_size);
    }
    let mut lines: Vec<Line> = data[..size]
        .split(|&b| b == b'\n')
        .map(<[u8]>::to_vec)
        .collect();
    if lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        return libfuzzer_sys::fuzzer_mutate(data, size, max_size);
    }
    match kind {
        Kind::Native => mutate_native(&mut lines, &mut r),
        Kind::Fits => mutate_fits(&mut lines, &mut r),
    }
    let mut out: Vec<u8> = Vec::with_capacity(size);
    for line in &lines {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    let n = out.len().min(max_size);
    data[..n].copy_from_slice(&out[..n]);
    n
}
