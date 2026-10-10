//! Channel I/O abstraction for reading and writing AST objects.
//!
//! A [`Channel`] connects a [`Source`] (text line supplier) and a [`Sink`]
//! (text line consumer) to the serialisation machinery, allowing AST objects
//! to be read from and written to external storage.
//!
//! The built-in [`NativeChannel`] uses the AST Native text format, and
//! [`MocChan`] the IVOA MOC serializations.

pub(crate) mod base;
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

/// The Channel attributes that control I/O (channel.c): each is set or
/// not, with C's default when not, as a dump records only those set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelConfig {
    comment: Option<bool>,
    full: Option<i32>,
    skip: Option<bool>,
    strict: Option<bool>,
    indent: Option<i32>,
    report_level: Option<i32>,
}

impl ChannelConfig {
    /// `Comment`: whether output includes comments. The default is true.
    pub fn comment(&self) -> bool {
        self.comment.unwrap_or(true)
    }

    /// Set `Comment`.
    pub fn set_comment(&mut self, comment: bool) {
        self.comment = Some(comment);
    }

    /// Whether `Comment` is set.
    pub fn test_comment(&self) -> bool {
        self.comment.is_some()
    }

    /// Clear `Comment`.
    pub fn clear_comment(&mut self) {
        self.comment = None;
    }

    /// `Full`: -1 for minimal output, 0 for normal and 1 for verbose. The
    /// default is 0.
    pub fn full(&self) -> i32 {
        self.full.unwrap_or(0)
    }

    /// Set `Full`; any positive value is stored as 1 and any negative one as
    /// -1, as C does.
    pub fn set_full(&mut self, full: i32) {
        self.full = Some(full.signum());
    }

    /// Whether `Full` is set.
    pub fn test_full(&self) -> bool {
        self.full.is_some()
    }

    /// Clear `Full`.
    pub fn clear_full(&mut self) {
        self.full = None;
    }

    /// `Skip`: whether data between objects is ignored. The default is
    /// false.
    pub fn skip(&self) -> bool {
        self.skip.unwrap_or(false)
    }

    /// Set `Skip`.
    pub fn set_skip(&mut self, skip: bool) {
        self.skip = Some(skip);
    }

    /// Whether `Skip` is set.
    pub fn test_skip(&self) -> bool {
        self.skip.is_some()
    }

    /// Clear `Skip`.
    pub fn clear_skip(&mut self) {
        self.skip = None;
    }

    /// `Strict`: whether unexpected input is an error rather than a
    /// warning. The default is false.
    pub fn strict(&self) -> bool {
        self.strict.unwrap_or(false)
    }

    /// Set `Strict`.
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = Some(strict);
    }

    /// Whether `Strict` is set.
    pub fn test_strict(&self) -> bool {
        self.strict.is_some()
    }

    /// Clear `Strict`.
    pub fn clear_strict(&mut self) {
        self.strict = None;
    }

    /// `Indent`: the indentation added for each level of nesting. The
    /// default is 3.
    pub fn indent(&self) -> i32 {
        self.indent.unwrap_or(3)
    }

    /// Set `Indent`. C stores any value, negative ones included.
    pub fn set_indent(&mut self, indent: i32) {
        self.indent = Some(indent);
    }

    /// Whether `Indent` is set.
    pub fn test_indent(&self) -> bool {
        self.indent.is_some()
    }

    /// Clear `Indent`.
    pub fn clear_indent(&mut self) {
        self.indent = None;
    }

    /// `ReportLevel`: how much is reported about objects read. The default
    /// is 1.
    pub fn report_level(&self) -> i32 {
        self.report_level.unwrap_or(1)
    }

    /// Set `ReportLevel`.
    pub fn set_report_level(&mut self, level: i32) {
        self.report_level = Some(level);
    }

    /// Whether `ReportLevel` is set.
    pub fn test_report_level(&self) -> bool {
        self.report_level.is_some()
    }

    /// Clear `ReportLevel`.
    pub fn clear_report_level(&mut self) {
        self.report_level = None;
    }
}

/// Provides lines of text for Channel reading.
///
/// A Channel and its clones share one source, so it must be `Send` for the
/// Channel to be.
pub trait Source: Send {
    /// Return the next line of text, or `None` at end-of-input.
    fn read_line(&mut self) -> Option<String>;
}

/// Receives lines of text from Channel writing.
///
/// A Channel and its clones share one sink, so it must be `Send` for the
/// Channel to be.
pub trait Sink: Send {
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
