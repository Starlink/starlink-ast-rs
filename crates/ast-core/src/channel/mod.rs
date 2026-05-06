//! Channel I/O abstraction for reading and writing AST objects.
//!
//! A [`Channel`] connects a [`Source`] (text line supplier) and a [`Sink`]
//! (text line consumer) to the serialisation machinery, allowing AST objects
//! to be read from and written to external storage.
//!
//! The built-in [`NativeChannel`] uses the AST Native text format.

pub mod native;
pub mod source_sink;

use crate::error::AstResult;
use crate::serial::AnyObject;

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
    fn write(&mut self, obj: &AnyObject) -> AstResult<()>;

    /// Return accumulated warnings from the most recent I/O operation.
    fn warnings(&self) -> &[String];

    /// Return a shared reference to the channel configuration.
    fn config(&self) -> &ChannelConfig;

    /// Return a mutable reference to the channel configuration.
    fn config_mut(&mut self) -> &mut ChannelConfig;
}
