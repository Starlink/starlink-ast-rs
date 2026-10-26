// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Read the WCS of one HDU of a FITS file and report it.
//!
//! Usage: `cargo run -p starlink-ast --example fits_wcs -- [--dump] <file.fits> [<hdu>]`
//!
//! The header of HDU `<hdu>` (0, the default, is the primary HDU) is read
//! with the `fitsrs` crate and its value cards are stored one by one, with
//! their types, in a [`FitsChan`], which reads a [`FrameSet`] from them.
//! Without `--dump` the program describes the base and current Frames and
//! transforms the corners of the image, GRID position 0.5 and `NAXISn + 0.5`
//! on every axis, and its centre into the current Frame and back again.
//! With `--dump` it prints the FrameSet's Native description instead.
//! A tile-compressed image is presented as CFITSIO and astropy present it,
//! with the image's BITPIX, NAXIS and NAXISn in place of the binary table's.
//! `tools/fits_wcs/fits_wcs.py` does the same with starlink-pyast, so the
//! two outputs can be diffed to compare this crate with the C library.

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;

use fitsrs::card::{Card, Value};
use fitsrs::{Fits, HDU};
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::error::AstResult;
use starlink_ast::frames::FrameSet;
use starlink_ast::prelude::*;
use starlink_ast::{AnyObject, FitsChan};

/// The value cards of a header, keyword and value, in header order.
type Cards = Vec<(String, Value)>;

/// The value cards of HDU `index`, 0 being the primary HDU.
fn read_cards(path: &str, index: usize) -> Result<Cards, Box<dyn Error>> {
    let file = File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let hdu = Fits::from_reader(BufReader::new(file))
        .nth(index)
        .ok_or_else(|| format!("the file has fewer than {} HDUs", index + 1))??;
    let cards: Vec<Card> = match hdu {
        HDU::Primary(h) | HDU::XImage(h) => h.get_header().cards().cloned().collect(),
        HDU::XBinaryTable(h) => h.get_header().cards().cloned().collect(),
        HDU::XASCIITable(h) => h.get_header().cards().cloned().collect(),
    };
    Ok(cards
        .into_iter()
        .filter_map(|card| match card {
            Card::Value { name, value } => Some((name, value)),
            _ => None,
        })
        .collect())
}

/// The integer value of the card named `key`, if there is one.
fn int(cards: &Cards, key: &str) -> Option<i64> {
    cards.iter().find_map(|(name, value)| match value {
        Value::Integer { value, .. } if name == key => Some(*value),
        _ => None,
    })
}

/// Whether the card named `key` holds the logical value T.
fn flag(cards: &Cards, key: &str) -> bool {
    cards
        .iter()
        .any(|(name, value)| name == key && matches!(value, Value::Logical { value: true, .. }))
}

/// The length of every data axis, NAXIS1 first; a missing NAXISn counts as 0.
fn axis_lengths(cards: &Cards) -> Vec<i64> {
    let naxis = int(cards, "NAXIS").unwrap_or(0).max(0);
    (1..=naxis)
        .map(|i| int(cards, &format!("NAXIS{i}")).unwrap_or(0))
        .collect()
}

/// The cards as CFITSIO and astropy present them: for a tile-compressed
/// image, which is stored as a binary table, BITPIX, NAXIS and NAXISn take
/// the image's values from ZBITPIX, ZNAXIS and ZNAXISn, which are dropped.
/// Any other header is returned unchanged.
///
/// A FrameSet read from the table's own NAXISn would be wrong: AST takes
/// them as the image size when it chooses the region over which to fit a
/// SIP inverse.
fn as_image(cards: Cards) -> Cards {
    if !flag(&cards, "ZIMAGE") {
        return cards;
    }
    let Some(znaxis) = int(&cards, "ZNAXIS") else {
        return cards;
    };
    let zkeys: Vec<String> = ["ZBITPIX", "ZNAXIS"]
        .into_iter()
        .map(String::from)
        .chain((1..=znaxis).map(|i| format!("ZNAXIS{i}")))
        .collect();
    let image: Cards = zkeys
        .iter()
        .filter_map(|k| cards.iter().find(|(name, _)| name == k))
        .map(|(name, value)| (name[1..].to_string(), value.clone()))
        .collect();
    let describes_shape = |name: &str| {
        name == "BITPIX"
            || name == "NAXIS"
            || name
                .strip_prefix("NAXIS")
                .is_some_and(|n| n.parse::<u32>().is_ok())
    };
    let mut out = Vec::with_capacity(cards.len());
    let mut placed = false;
    for (name, value) in cards {
        if describes_shape(&name) {
            if !placed {
                out.extend(image.iter().cloned());
                placed = true;
            }
        } else if !zkeys.contains(&name) {
            out.push((name, value));
        }
    }
    out
}

/// Store the cards in `fc`, each with its type, as `astSetFits<X>` does
/// in C, then rewind so that a read starts at the first card. A card whose
/// value is not legal FITS is left out.
fn store_cards(fc: &mut FitsChan, cards: &Cards) -> AstResult<()> {
    for (name, value) in cards {
        match value {
            Value::Integer { value, comment } => {
                let comment = comment.as_deref().unwrap_or("");
                match i32::try_from(*value) {
                    Ok(v) => fc.set_fits_i(name, v, comment, false)?,
                    Err(_) => fc.set_fits_k(name, *value, comment, false)?,
                }
            }
            Value::Float { value, comment } => {
                fc.set_fits_f(name, *value, comment.as_deref().unwrap_or(""), false)?;
            }
            Value::Logical { value, comment } => {
                fc.set_fits_l(name, *value, comment.as_deref().unwrap_or(""), false)?;
            }
            Value::String { value, comment } => {
                fc.set_fits_s(name, value, comment.as_deref().unwrap_or(""), false)?;
            }
            Value::Undefined => fc.set_fits_u(name, "", false)?,
            Value::Invalid(_) => {}
        }
    }
    fc.clear_card()
}

/// The value with 17 significant digits in C's `%.16e` form, which is what
/// Python prints too, so that every bit of a double is visible.
fn sci(v: f64) -> String {
    let text = format!("{v:.16e}");
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent.parse().expect("the exponent is an integer");
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

/// The values separated by spaces, each written by `write`, the bad value
/// as `<bad>`.
fn numbers(values: &[f64], write: fn(f64) -> String) -> String {
    values
        .iter()
        .map(|&v| {
            if v == BAD_VALUE {
                "<bad>".to_string()
            } else {
                write(v)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Print the Frame's identity and its axes.
fn describe(which: &str, frame: &dyn Frame) -> AstResult<()> {
    println!(
        "{which} frame: Domain=\"{}\" Naxes={} Title=\"{}\"",
        frame.domain(),
        frame.naxes(),
        frame.title()?
    );
    for axis in 1..=frame.naxes() {
        println!(
            "  axis {axis}: Label=\"{}\" Symbol=\"{}\" Unit=\"{}\"",
            frame.label(axis)?,
            frame.symbol(axis)?,
            frame.unit(axis)?
        );
    }
    Ok(())
}

/// Describe the FrameSet and transform the image corners through it.
fn report(path: &str, hdu: usize, cards: &Cards, fs: &FrameSet) -> Result<(), Box<dyn Error>> {
    let lengths = axis_lengths(cards);
    println!("file: {path}");
    println!("hdu: {hdu}");
    let dims: Vec<String> = lengths.iter().map(i64::to_string).collect();
    println!(
        "data array: {}",
        if dims.is_empty() {
            "none".to_string()
        } else {
            dims.join(" x ")
        }
    );
    println!(
        "frames: {}  base: {}  current: {}",
        fs.nframe(),
        fs.base_index(),
        fs.current_index()
    );
    describe("base", fs.base())?;
    describe("current", fs.current())?;

    // The image corners in GRID coordinates, axis 1 varying fastest, then
    // the centre. A grid axis the data array does not have is one pixel
    // long.
    let nin = fs.base().naxes();
    let size = |axis: usize| lengths.get(axis).copied().unwrap_or(1).max(1) as f64;
    let mut points: Vec<Vec<f64>> = (0..1usize << nin)
        .map(|k| {
            (0..nin)
                .map(|a| {
                    if (k >> a) & 1 == 1 {
                        size(a) + 0.5
                    } else {
                        0.5
                    }
                })
                .collect()
        })
        .collect();
    points.push((0..nin).map(|a| (size(a) + 1.0) / 2.0).collect());
    let columns: Vec<Vec<f64>> = (0..nin)
        .map(|a| points.iter().map(|p| p[a]).collect())
        .collect();
    let columns: Vec<&[f64]> = columns.iter().map(Vec::as_slice).collect();
    let grid = PointSet::from_columns(&columns)?;

    let world = fs.transform(&grid, true)?;
    let back = fs.transform(&world, false)?;
    let current = fs.current();
    println!("corners: grid -> current [formatted] -> grid");
    for p in 0..grid.npoint() {
        let mut normed = world.point(p).to_vec();
        current.norm(&mut normed)?;
        let formatted = (1..=current.naxes())
            .map(|axis| current.format(axis, normed[axis - 1]))
            .collect::<AstResult<Vec<_>>>()?;
        println!(
            "  {} -> {} [{}] -> {}",
            numbers(&grid.point(p), |v| format!("{v:.1}")),
            numbers(&world.point(p), sci),
            formatted.join(", "),
            numbers(&back.point(p), sci)
        );
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut dump = false;
    let mut positional = Vec::new();
    for arg in std::env::args().skip(1) {
        if arg == "--dump" {
            dump = true;
        } else {
            positional.push(arg);
        }
    }
    let (path, hdu) = match positional.as_slice() {
        [path] => (path.clone(), 0),
        [path, hdu] => (
            path.clone(),
            hdu.parse::<usize>()
                .map_err(|_| format!("the HDU must be a non-negative integer, not {hdu:?}"))?,
        ),
        _ => return Err("usage: fits_wcs [--dump] <file.fits> [<hdu>]".into()),
    };

    let cards = as_image(read_cards(&path, hdu)?);
    let mut fc = FitsChan::new()?;
    store_cards(&mut fc, &cards)?;
    let fs = match fc.read()? {
        Some(AnyObject::FrameSet(fs)) => fs,
        Some(other) => {
            return Err(
                format!("the header holds a {}, not a FrameSet", other.class_name()).into(),
            );
        }
        None => return Err("no FrameSet could be read from the header".into()),
    };
    if dump {
        fs.show()?;
    } else {
        report(&path, hdu, &cards, &fs)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fits_wcs: {e}");
            ExitCode::FAILURE
        }
    }
}
