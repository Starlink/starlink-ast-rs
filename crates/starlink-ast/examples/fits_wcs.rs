// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Read the WCS of one HDU of a FITS file and report it.
//!
//! Usage: `cargo run -p starlink-ast --example fits_wcs -- [--dump] <file.fits> [<hdu>]`
//!
//! The header of HDU `<hdu>` (0, the default, is the primary HDU) is read
//! straight from the file and handed to a [`FitsChan`], which reads a
//! [`FrameSet`] from it.
//! Without `--dump` the program describes the base and current Frames and
//! transforms the corners of the image, GRID position 0.5 and `NAXISn + 0.5`
//! on every axis, and its centre into the current Frame and back again.
//! With `--dump` it prints the FrameSet's Native description instead.
//! A tile-compressed image is presented as CFITSIO and astropy present it,
//! with the image's BITPIX, NAXIS and NAXISn in place of the binary table's.
//! `tools/fits_wcs/fits_wcs.py` does the same with starlink-pyast, so the
//! two outputs can be diffed to compare this crate with the C library.
//!
//! The few lines that find the HDU and collect its cards are written here
//! rather than taken from a FITS crate such as `fitsrs`.
//! An example's dependencies are dev-dependencies of the crate, so a FITS
//! reader would bring its own dependency tree into starlink-ast purely for
//! this example, and the WCS library and a FITS library should stay
//! independent of each other.

use std::error::Error;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::process::ExitCode;

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::error::AstResult;
use starlink_ast::frames::FrameSet;
use starlink_ast::prelude::*;
use starlink_ast::{AnyObject, FitsChan};

/// A FITS header is written in blocks of this many bytes.
const BLOCK: usize = 2880;
/// Each header card is this many bytes.
const CARD: usize = 80;

/// The cards of one HDU's header, 80 characters each, without the END card.
struct Header {
    cards: Vec<String>,
}

impl Header {
    /// The first card named `key`, if there is one.
    fn card(&self, key: &str) -> Option<&str> {
        self.cards
            .iter()
            .map(String::as_str)
            .find(|c| c[..8].trim_end() == key && &c[8..10] == "= ")
    }

    /// The value of the first card named `key`, before any comment.
    fn value(&self, key: &str) -> Option<&str> {
        Some(self.card(key)?[10..].split('/').next()?.trim())
    }

    /// The integer value of the first card named `key`, if there is one.
    fn int(&self, key: &str) -> Option<i64> {
        self.value(key)?.parse().ok()
    }

    /// Whether the first card named `key` holds the logical value T.
    fn flag(&self, key: &str) -> bool {
        self.value(key) == Some("T")
    }

    /// The length of every data axis, NAXIS1 first; a missing NAXISn counts
    /// as 0.
    fn axis_lengths(&self) -> Vec<i64> {
        let naxis = self.int("NAXIS").unwrap_or(0).max(0);
        (1..=naxis)
            .map(|i| self.int(&format!("NAXIS{i}")).unwrap_or(0))
            .collect()
    }

    /// The size of the data that follows this header, rounded up to whole
    /// blocks.
    fn data_bytes(&self) -> u64 {
        let lengths = self.axis_lengths();
        if lengths.is_empty() {
            return 0;
        }
        // Random-groups data has NAXIS1 = 0 and the other axes give the
        // size of one group.
        let axes = if lengths[0] == 0 {
            &lengths[1..]
        } else {
            &lengths
        };
        let pixels: u64 = axes.iter().map(|&n| n.max(0) as u64).product();
        let bitpix = self.int("BITPIX").unwrap_or(0).unsigned_abs();
        let gcount = self.int("GCOUNT").unwrap_or(1).max(0) as u64;
        let pcount = self.int("PCOUNT").unwrap_or(0).max(0) as u64;
        let bytes = bitpix / 8 * gcount * (pcount + pixels);
        bytes.div_ceil(BLOCK as u64) * BLOCK as u64
    }

    /// The header as CFITSIO and astropy present it: for a tile-compressed
    /// image, which is stored as a binary table, BITPIX, NAXIS and NAXISn
    /// take the image's values from ZBITPIX, ZNAXIS and ZNAXISn, which are
    /// dropped. Any other header is returned unchanged.
    ///
    /// A FrameSet read from the table's own NAXISn would be wrong: AST
    /// takes them as the image size when it chooses the region over which
    /// to fit a SIP inverse.
    fn into_image(self) -> Self {
        if !self.flag("ZIMAGE") {
            return self;
        }
        let Some(znaxis) = self.int("ZNAXIS") else {
            return self;
        };
        let zkeys: Vec<String> = ["ZBITPIX", "ZNAXIS"]
            .into_iter()
            .map(String::from)
            .chain((1..=znaxis).map(|i| format!("ZNAXIS{i}")))
            .collect();
        // The Z cards renamed, keeping their values and comments.
        let image: Vec<String> = zkeys
            .iter()
            .filter_map(|k| self.card(k).map(|c| format!("{:<8}{}", &k[1..], &c[8..])))
            .collect();
        let describes_shape = |key: &str| {
            key == "BITPIX"
                || key == "NAXIS"
                || key
                    .strip_prefix("NAXIS")
                    .is_some_and(|n| n.parse::<u32>().is_ok())
        };
        let mut cards = Vec::with_capacity(self.cards.len());
        let mut placed = false;
        for card in self.cards {
            let key = card[..8].trim_end().to_string();
            if describes_shape(&key) {
                if !placed {
                    cards.extend(image.iter().cloned());
                    placed = true;
                }
            } else if !zkeys.contains(&key) {
                cards.push(card);
            }
        }
        Self { cards }
    }
}

/// The header of HDU `hdu`, 0 being the primary HDU, read block by block
/// past the data of the HDUs before it.
fn read_header(file: &mut File, hdu: usize) -> Result<Header, Box<dyn Error>> {
    let mut block = [0u8; BLOCK];
    let mut index = 0;
    loop {
        let mut cards = Vec::new();
        let mut end = false;
        while !end {
            match file.read_exact(&mut block) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                    let what = if cards.is_empty() {
                        format!("the file has fewer than {} HDUs", hdu + 1)
                    } else {
                        format!("the file ends inside the header of HDU {index}")
                    };
                    return Err(what.into());
                }
                Err(e) => return Err(e.into()),
            }
            for raw in block.chunks(CARD) {
                if raw.starts_with(b"END     ") {
                    end = true;
                    break;
                }
                cards.push(
                    raw.iter()
                        .map(|&b| if b.is_ascii() { b as char } else { '?' })
                        .collect(),
                );
            }
        }
        let header = Header { cards };
        if index == hdu {
            return Ok(header);
        }
        file.seek(SeekFrom::Current(header.data_bytes() as i64))?;
        index += 1;
    }
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
fn report(path: &str, hdu: usize, header: &Header, fs: &FrameSet) -> Result<(), Box<dyn Error>> {
    let lengths = header.axis_lengths();
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

    let mut file = File::open(&path).map_err(|e| format!("{path}: {e}"))?;
    let header = read_header(&mut file, hdu)?.into_image();
    let mut fc = FitsChan::new()?;
    fc.put_cards(&header.cards.concat())?;
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
        report(&path, hdu, &header, &fs)?;
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
