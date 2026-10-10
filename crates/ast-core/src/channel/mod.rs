//! Channel I/O abstraction for reading and writing AST objects.
//!
//! A [`Channel`] connects a [`Source`] (text line supplier) and a [`Sink`]
//! (text line consumer) to the serialisation machinery, allowing AST objects
//! to be read from and written to external storage.
//!
//! The built-in [`NativeChannel`] uses the AST Native text format, and
//! [`MocChan`] the IVOA MOC serializations.

pub mod mocchan;
pub mod native;
pub mod source_sink;

use std::sync::Arc;

use crate::error::AstResult;
use crate::keymap::KeyMap;
use crate::mappings::IntraTransform;
use crate::serial::AnyObject;

pub use mocchan::{MocChan, MocFormat};
pub use native::NativeChannel;
pub use source_sink::{FileSink, FileSource, StringSink, StringSource};

/// Configuration controlling Channel I/O behaviour.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Include comments in output (default: true).
    pub comment: bool,
    /// Output verbosity: -1=minimal, 0=normal, 1=verbose (default: 0).
    pub full: i8,
    /// Skip non-object data between objects (default: false).
    pub skip: bool,
    /// Report unexpected input data as errors (default: false).
    pub strict: bool,
    /// Indentation increment per nesting level (default: 3).
    pub indent: usize,
    /// Error reporting level (default: 0).
    pub report_level: i32,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            comment: true,
            full: 0,
            skip: false,
            strict: false,
            indent: 3,
            report_level: 0,
        }
    }
}

/// Provides lines of text for Channel reading.
pub trait Source {
    /// Return the next line of text, or `None` at end-of-input.
    fn read_line(&mut self) -> Option<String>;
}

/// Receives lines of text from Channel writing.
pub trait Sink {
    /// Accept one line of output text.
    fn write_line(&mut self, line: &str);
}

/// I/O abstraction for reading and writing AST objects.
pub trait Channel {
    /// Read the next object from the source.
    ///
    /// Returns `Ok(None)` at end-of-input.
    fn read(&mut self) -> AstResult<Option<AnyObject>>;

    /// Write an object to the sink.
    ///
    /// Returns the number of objects written: 1 normally, 0 when the
    /// channel's encoding cannot describe the object. Mirrors C `astWrite`,
    /// which returns a count rather than reporting an error, because
    /// "this encoding cannot represent this object" is an ordinary answer
    /// for the foreign FITS encodings — a FrameSet whose Mapping has no
    /// adequate linear approximation is simply not a FITS-WCS header. An
    /// `Err` is reserved for a malformed request or an I/O failure.
    fn write(&mut self, obj: &AnyObject) -> AstResult<usize>;

    /// Make `transform` available to the IntraMaps this channel reads, which
    /// find their transformation by name, as C's `astIntraReg` does for the
    /// whole process. A name already registered here is refused with
    /// AST__MRITF unless it is the same transformation.
    fn register_intra(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()>;

    /// Return the accumulated warning texts from the most recent I/O
    /// operation, in the order they were issued.
    ///
    /// This is the idiomatic Rust accessor and has no direct C counterpart —
    /// C exposes only the [`Channel::warnings`] KeyMap view.
    fn warning_texts(&self) -> &[String];

    /// Return the warnings from the most recent read or write as a [`KeyMap`],
    /// mirroring C's `astWarnings` (`channel.c:1582`).
    ///
    /// Keys are `Warning_1`, `Warning_2`, … in issue order, each a string
    /// entry. Returns `None` when no warnings were issued, matching C's
    /// `NULL` return for `nwarn == 0`.
    ///
    /// The KeyMap is built on demand and returned owned, because that is what
    /// C does: `astWarnings` allocates a fresh KeyMap per call from the
    /// channel's internal string list rather than caching one. Returning a
    /// borrow would oblige every channel to hold a KeyMap in lockstep with
    /// that list.
    fn warnings(&self) -> Option<KeyMap> {
        let texts = self.warning_texts();
        if texts.is_empty() {
            return None;
        }
        let mut map = KeyMap::with_size_guess(texts.len());
        for (i, text) in texts.iter().enumerate() {
            // C passes a single-space comment (`astMapPut0C(result, key,
            // (this->warnings)[i], " ")`), which its dump layer then treats as
            // absent; pass None so the Rust dump matches.
            map.put_0c(&format!("Warning_{}", i + 1), text, None)
                .expect("Warning_<n> keys are valid and distinct");
        }
        Some(map)
    }

    /// Return a shared reference to the channel configuration.
    fn config(&self) -> &ChannelConfig;

    /// Return a mutable reference to the channel configuration.
    fn config_mut(&mut self) -> &mut ChannelConfig;
}
