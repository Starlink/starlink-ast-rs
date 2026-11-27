// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! What the fuzz targets in `fuzz/` do with each input, shared with the test
//! that replays the inputs they found.
//!
//! Each reads every Object it can from the input and then uses each one as
//! the C fuzz harness of the loader-hardening work did: a Mapping transforms
//! a few points each way and is simplified, and every Object is written back
//! and copied.
//! Errors are expected and ignored; a panic, a hang or a huge allocation is
//! the finding.

use starlink_ast::coords::PointSet;
use starlink_ast::{AnyObject, Channel, Encoding, FitsChan, MocChan, MocFormat, NativeChannel};

/// The most Objects read from one input, so that a reader that returns
/// Objects without consuming its input cannot loop for ever.
const MAX_OBJECTS: usize = 16;

/// The coordinate values of the points each Mapping transforms.
const VALUES: [f64; 6] = [0.0, 1.0, -1.0, 0.5, 100.0, -1.0e10];

/// Use `obj` as the C harness did: transform, simplify, write and copy.
fn exercise(obj: &AnyObject) {
    if let Some(map) = obj.as_mapping() {
        for (forward, n, ok) in [
            (true, map.nin(), map.has_forward()),
            (false, map.nout(), map.has_inverse()),
        ] {
            if ok && n > 0 {
                let pts = PointSet::from_rows(VALUES.iter().map(|v| vec![*v; n]));
                let _ = map.transform(&pts, forward);
            }
        }
        let _ = map.simplify();
    }
    let _ = obj.to_native_string();
    drop(obj.clone());
}

/// Read every Object from `data` as a Native dump.
pub fn native_read(data: &[u8]) {
    let mut ch = NativeChannel::from_string(&String::from_utf8_lossy(data));
    for _ in 0..MAX_OBJECTS {
        match ch.read() {
            Ok(Some(obj)) => exercise(&obj),
            _ => break,
        }
    }
}

/// `data` as FITS header cards: each line, cut into 80-character cards.
fn cards(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    let mut cards = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            continue;
        }
        cards.extend(chars.chunks(80).map(|c| c.iter().collect()));
    }
    cards
}

/// Read every Object from `data` as a FITS header with the default Encoding,
/// and write each back through a FitsChan of the Encoding that read it.
pub fn fitschan_read_write(data: &[u8]) {
    let Ok(mut fc) = FitsChan::new() else {
        return;
    };
    for card in cards(data) {
        let _ = fc.put_fits(&card, false);
    }
    if fc.clear_card().is_err() {
        return;
    }
    let encoding = fc.encoding().unwrap_or(Encoding::Native);
    for _ in 0..MAX_OBJECTS {
        match fc.read() {
            Ok(Some(obj)) => {
                exercise(&obj);
                if let Ok(mut out) = FitsChan::new() {
                    out.set_encoding(encoding);
                    let _ = out.write(&obj);
                }
            }
            _ => break,
        }
    }
}

/// Read every Object from `data` through a MocChan, and write each back in
/// both MOC formats.
pub fn mocchan_read(data: &[u8]) {
    let mut ch = MocChan::from_string(&String::from_utf8_lossy(data));
    for _ in 0..MAX_OBJECTS {
        match ch.read() {
            Ok(Some(obj)) => {
                exercise(&obj);
                for format in [MocFormat::String, MocFormat::Json] {
                    let mut out = MocChan::with_string_sink();
                    out.set_moc_format(format);
                    let _ = out.write(&obj);
                }
            }
            _ => break,
        }
    }
}
