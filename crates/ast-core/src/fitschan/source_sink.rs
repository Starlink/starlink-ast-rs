//! A FitsChan's source and sink: C's source and sink functions, and the
//! `SourceFile` and `SinkFile` attributes (fitschan.c `ReadFromSource`,
//! `ReadFits`, `WriteToSink`, `WriteFits`, `ShowFits`, `SetSourceFile` and
//! `Delete`).
//!
//! C reads the source function on the first use of the FitsChan, not when
//! it is created, and writes the cards to the sink when the FitsChan is
//! deleted. Here the source is read by the first method C reads it in, and
//! the cards are written to the sink when the FitsChan is dropped. A drop
//! cannot report an error, so [`FitsChan::write_fits`] is the way to write
//! with one.
//!
//! A clone has no source, sink, `SourceFile` or `SinkFile`: C's FitsChan
//! `Copy` sets the source and sink functions to NULL and Channel `Copy` the
//! file names, so a copy never writes the cards a second time. C's
//! `astCopy` reads the source before copying, through `GetObjSize`, so the
//! copy holds the source's cards. A clone cannot change the FitsChan it is
//! made from, so it reads the source into the copy and keeps the cards it
//! read in the [`SourceCell`], for the FitsChan to store on its next use
//! exactly as it would have stored them from the source.

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};

use super::FitsChan;
use super::card::FITS_CARD_LEN;
use crate::channel::{Sink, Source};
use crate::error::{AstError, AstResult, ErrorCode};

/// A FitsChan field that a clone does not inherit.
pub(crate) struct NotCopied<T>(pub(crate) Option<T>);

impl<T> Clone for NotCopied<T> {
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl<T> Default for NotCopied<T> {
    fn default() -> Self {
        Self(None)
    }
}

/// A FitsChan's source, and the cards a clone has read from it.
#[derive(Default)]
pub(crate) struct SourceState {
    /// The source, until it is read (C's `source`).
    source: Option<Box<dyn Source>>,
    /// The source once read, which [`FitsChan::read_fits`] reads again (C's
    /// `saved_source`).
    saved: Option<Box<dyn Source>>,
    /// The cards a clone read from `source`, which the FitsChan has still to
    /// store.
    pending: Option<Vec<String>>,
}

/// A FitsChan's [`SourceState`], which a clone reads but does not inherit.
#[derive(Default)]
pub(crate) struct SourceCell(RefCell<SourceState>);

impl Clone for SourceCell {
    fn clone(&self) -> Self {
        Self::default()
    }
}

/// C's `fgets(buf, max + 1, fd)`: the next line of at most `max` bytes,
/// including its newline; a longer line is returned in pieces. `None` at
/// end-of-file.
fn fgets(reader: &mut impl BufRead, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    while line.len() < max {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        let chunk = &buf[..buf.len().min(max - line.len())];
        if let Some(i) = chunk.iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&chunk[..=i]);
            reader.consume(i + 1);
            return Ok(Some(line));
        }
        let n = chunk.len();
        line.extend_from_slice(chunk);
        reader.consume(n);
    }
    Ok((!line.is_empty()).then_some(line))
}

/// C's `astChrLen`: the length of `s` without its trailing white space and
/// non-printable characters.
fn chr_len(s: &[u8]) -> usize {
    s.iter()
        .rposition(|&b| b.is_ascii_graphic())
        .map_or(0, |i| i + 1)
}

impl FitsChan {
    /// Read cards from `source` the first time the FitsChan is used.
    ///
    /// `source` gives one card per line, such as the cards of a FITS file,
    /// and `None` when there are no more.
    /// The source is read by the first method that uses the cards, not by
    /// this one: the cards are then appended, and the first of them made
    /// current.
    /// [`read_fits`](Self::read_fits) reads it again.
    /// Without a source the FitsChan holds only the cards put in it.
    pub fn set_source(&mut self, source: Box<dyn Source>) {
        *self.source.0.get_mut() = SourceState {
            source: Some(source),
            ..SourceState::default()
        };
    }

    /// Write the cards to `sink` when the FitsChan is dropped, or by
    /// [`write_fits`](Self::write_fits).
    ///
    /// Each card a read has not consumed is given to the sink as one line.
    /// No `END` card is added: if the header needs one, write it after the
    /// FitsChan is dropped, unless the FitsChan holds one.
    /// The [SinkFile](Self::sink_file) attribute, when set, takes the sink's
    /// place.
    /// Without a sink or SinkFile, the cards are lost when the FitsChan is
    /// dropped.
    pub fn set_sink(&mut self, sink: Box<dyn Sink>) {
        self.sink = NotCopied(Some(sink));
    }

    /// C's `ReadFromSource`: if the source has not been read, append its
    /// cards and make the first of them current.
    pub(crate) fn read_from_source(&mut self) -> AstResult<()> {
        let state = self.source.0.get_mut();
        if let Some(cards) = state.pending.take() {
            let mut cards = cards.into_iter();
            return self.store_source_cards(|| cards.next());
        }
        let Some(mut source) = state.source.take() else {
            return Ok(());
        };
        let stored = self.store_source_cards(|| source.read_line());
        self.source.0.get_mut().saved = Some(source);
        stored
    }

    /// The body of C's `ReadFromSource`: append each card `next` gives until
    /// it gives `None` or one cannot be stored, then make the first of them
    /// current.
    fn store_source_cards(&mut self, mut next: impl FnMut() -> Option<String>) -> AstResult<()> {
        self.cards.set_card(i64::MAX);
        let icard = self.cards.get_card();
        while let Some(card) = next() {
            self.put_fits(&card, false)?;
        }
        self.cards.set_card(icard as i64);
        Ok(())
    }

    /// C's FitsChan `Copy`, after `astCopy` has read the source: a FitsChan
    /// with this one's cards and attributes and no source or sink. A card
    /// the copy cannot store ends the read, as in C, and the error is
    /// reported when this FitsChan stores the same cards.
    pub(crate) fn copy_reading_source(&self, mut copy: FitsChan) -> FitsChan {
        let mut state = self.source.0.borrow_mut();
        if let Some(cards) = &state.pending {
            let mut cards = cards.clone().into_iter();
            let _ = copy.store_source_cards(|| cards.next());
        } else if let Some(mut source) = state.source.take() {
            let mut read = Vec::new();
            let _ = copy.store_source_cards(|| {
                let card = source.read_line();
                read.extend(card.clone());
                card
            });
            state.saved = Some(source);
            state.pending = Some(read);
        }
        copy
    }

    /// Read the [source](Self::set_source) again, appending any cards it now
    /// gives and making the first of them current.
    ///
    /// The source is read anyway on first use, so this is needed only to
    /// read cards that have reached it since, such as cards newly added to
    /// an external store.
    /// Without a source this does nothing.
    /// The [SourceFile](Self::source_file) is not read again; cards are read
    /// from it whenever it is set.
    ///
    /// # Errors
    ///
    /// Passes on the error of storing a card from the source, as for
    /// [`put_fits`](Self::put_fits).
    #[doc(alias = "astReadFits")]
    pub fn read_fits(&mut self) -> AstResult<()> {
        // Cards a clone read are those C stored before this call.
        if self.source.0.get_mut().pending.is_some() {
            self.read_from_source()?;
        }
        let state = self.source.0.get_mut();
        if state.source.is_none() {
            state.source = state.saved.take();
        }
        self.read_from_source()
    }

    /// Write the cards to the [SinkFile](Self::sink_file), if set, or else
    /// to the [sink](Self::set_sink), then delete every card.
    ///
    /// With neither a SinkFile nor a sink, this only empties the FitsChan,
    /// as [`empty_fits`](Self::empty_fits) does.
    /// Unlike a drop, this reports a failure.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Wrerr`] if the SinkFile cannot be created.
    /// - [`ErrorCode::Bdfts`] if a card cannot be formatted, as for
    ///   [`to_header`](Self::to_header).
    /// - As for [`card`](Self::card).
    ///
    /// The cards are deleted even when writing them fails.
    #[doc(alias = "astWriteFits")]
    pub fn write_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        let written = self.write_to_sink();
        self.empty_fits_inner();
        written
    }

    /// Print each card a read has not consumed to standard output, one per
    /// line, leaving the current card as it was.
    ///
    /// The cards are formatted as by [`find_fits`](Self::find_fits).
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bdfts`] if a card cannot be formatted, as for
    ///   [`to_header`](Self::to_header); nothing is printed.
    /// - As for [`card`](Self::card).
    #[doc(alias = "astShowFits")]
    pub fn show_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for line in self.card_lines()? {
            // C's printf ignores a failure to write.
            let _ = writeln!(out, "{line}");
        }
        Ok(())
    }

    /// The text of each card a read has not consumed, formatted as C's
    /// `astFindFits` with `%f` formats it.
    fn card_lines(&self) -> AstResult<Vec<String>> {
        self.cards
            .iter()
            .filter(|card| !card.is_consumed())
            .map(|card| {
                card.format_with_digits_and_rounding(
                    self.fits_digits,
                    self.fits_rounding() as usize,
                )
            })
            .collect()
    }

    /// C's `WriteToSink`: write each card a read has not consumed to the
    /// `SinkFile`, if set, or else to the sink.
    pub(crate) fn write_to_sink(&mut self) -> AstResult<()> {
        let mut fd = match &self.sink_file.0 {
            Some(name) => Some(File::create(name).map_err(|e| {
                AstError::new(
                    ErrorCode::Wrerr,
                    format!("astDelete(FitsChan): Failed to open output SinkFile '{name}' - {e}."),
                )
            })?),
            None => None,
        };
        if fd.is_none() && self.sink.0.is_none() {
            return Ok(());
        }
        self.read_from_source()?;
        let lines = self.card_lines()?;
        for line in lines {
            match (&mut fd, &mut self.sink.0) {
                // C's fprintf ignores a failure to write.
                (Some(fd), _) => {
                    let _ = writeln!(fd, "{line}");
                }
                (None, Some(sink)) => sink.write_line(&line),
                (None, None) => unreachable!("a SinkFile or sink is present"),
            }
        }
        Ok(())
    }

    /// The `SourceFile` attribute: the name of a text file from which cards
    /// have been read, or `None` when unset.
    ///
    /// Default: unset.
    ///
    /// The file holds one header card per line; it is a text file, not a
    /// FITS file.
    /// Setting the attribute reads the file at once, appending its cards to
    /// any already in the FitsChan, in addition to those of any
    /// [source](Self::set_source).
    /// Clearing it only forgets the name.
    /// A clone does not inherit it.
    #[doc(alias = "SourceFile")]
    pub fn source_file(&self) -> Option<&str> {
        self.source_file.0.as_deref()
    }

    /// Set the [SourceFile](Self::source_file) attribute, and append the
    /// cards in the file, one per line, leaving the FitsChan at end-of-file.
    ///
    /// A line longer than a card is read as several cards.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Rderr`] if the file cannot be opened; the attribute is
    ///   set anyway.
    /// - Passes on the error of storing a card, as for
    ///   [`put_fits`](Self::put_fits).
    // C: FitsChan's `SetSourceFile`; a long line is split as `fgets` splits
    // it.
    pub fn set_source_file(&mut self, source_file: &str) -> AstResult<()> {
        self.source_file = NotCopied(Some(source_file.to_string()));
        let file = File::open(source_file).map_err(|e| {
            AstError::new(
                ErrorCode::Rderr,
                format!(
                    "{} '{source_file}' - {e}.",
                    "astSetSourceFile(FitsChan): Failed to open input SourceFile"
                ),
            )
        })?;
        self.read_from_source()?;
        self.cards.set_card(i64::MAX);
        let mut reader = BufReader::new(file);
        // C's fgets ends the read at an error as at end-of-file.
        while let Some(line) = fgets(&mut reader, FITS_CARD_LEN + 1).ok().flatten() {
            let card = String::from_utf8_lossy(&line[..chr_len(&line)]).into_owned();
            self.put_fits(&card, false)?;
        }
        Ok(())
    }

    /// Whether the [SourceFile](Self::source_file) attribute is set.
    pub fn test_source_file(&self) -> bool {
        self.source_file.0.is_some()
    }

    /// Clear the [SourceFile](Self::source_file) attribute.
    /// The cards read from the file are kept.
    pub fn clear_source_file(&mut self) {
        self.source_file = NotCopied(None);
    }

    /// The `SinkFile` attribute: the name of the text file to which the
    /// cards are written when the FitsChan is dropped or
    /// [`write_fits`](Self::write_fits) is called, or `None` when unset.
    ///
    /// Default: unset, when the cards go to the [sink](Self::set_sink), if
    /// there is one.
    ///
    /// When it is set, the file takes the place of the sink.
    /// It holds one header card per line; it is a text file, not a FITS
    /// file.
    /// The file is created when the cards are written, replacing any file
    /// of that name; [`write_fits`](Self::write_fits) reports a file that
    /// cannot be created, which a drop cannot.
    /// A clone does not inherit it.
    #[doc(alias = "SinkFile")]
    pub fn sink_file(&self) -> Option<&str> {
        self.sink_file.0.as_deref()
    }

    /// Set the [SinkFile](Self::sink_file) attribute.
    /// The file is created only when the cards are written.
    pub fn set_sink_file(&mut self, sink_file: &str) {
        self.sink_file = NotCopied(Some(sink_file.to_string()));
    }

    /// Whether the [SinkFile](Self::sink_file) attribute is set.
    pub fn test_sink_file(&self) -> bool {
        self.sink_file.0.is_some()
    }

    /// Clear the [SinkFile](Self::sink_file) attribute.
    pub fn clear_sink_file(&mut self) {
        self.sink_file = NotCopied(None);
    }
}

impl Drop for FitsChan {
    /// Write the cards to the SinkFile, if set, or else the sink.
    /// A failure cannot be reported here; [`FitsChan::write_fits`] reports
    /// it.
    // C: FitsChan's `Delete`.
    fn drop(&mut self) {
        let _ = self.write_to_sink();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fgets_splits_long_lines_as_c_does() {
        let mut r = BufReader::new(&b"abc\ndefgh\n\nxy"[..]);
        let got: Vec<Vec<u8>> = std::iter::from_fn(|| fgets(&mut r, 4).unwrap()).collect();
        assert_eq!(
            got,
            vec![
                b"abc\n".to_vec(),
                b"defg".to_vec(),
                b"h\n".to_vec(),
                b"\n".to_vec(),
                b"xy".to_vec()
            ]
        );
    }

    #[test]
    fn chr_len_drops_trailing_space_and_control() {
        assert_eq!(chr_len(b"AB C  \r\n"), 4);
        assert_eq!(chr_len(b" \n"), 0);
    }
}
