//! Built-in [`Source`] and [`Sink`] implementations.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use super::{Sink, Source};

// ---------------------------------------------------------------------------
// StringSource
// ---------------------------------------------------------------------------

/// A [`Source`] that reads lines from an in-memory string.
pub struct StringSource {
    lines: Vec<String>,
    pos: usize,
}

impl StringSource {
    /// Create a new `StringSource` from the given text.
    ///
    /// The text is split on newline characters; each resulting segment
    /// becomes one line delivered by [`Source::read_line`].
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.lines().map(|l| l.to_string()).collect(),
            pos: 0,
        }
    }
}

impl Source for StringSource {
    fn read_line(&mut self) -> Option<String> {
        if self.pos < self.lines.len() {
            let line = self.lines[self.pos].clone();
            self.pos += 1;
            Some(line)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// StringSink
// ---------------------------------------------------------------------------

/// A [`Sink`] that accumulates written lines into an in-memory string.
pub struct StringSink {
    buf: String,
}

impl StringSink {
    /// Create a new empty `StringSink`.
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    /// Consume the sink and return the accumulated text.
    pub fn into_string(self) -> String {
        self.buf
    }

    /// Return a reference to the accumulated text.
    pub fn as_str(&self) -> &str {
        &self.buf
    }
}

impl Default for StringSink {
    fn default() -> Self {
        Self::new()
    }
}

impl Sink for StringSink {
    fn write_line(&mut self, line: &str) {
        self.buf.push_str(line);
        self.buf.push('\n');
    }
}

// ---------------------------------------------------------------------------
// FileSource
// ---------------------------------------------------------------------------

/// A [`Source`] that reads lines from a file via buffered I/O.
pub struct FileSource {
    reader: BufReader<File>,
}

impl FileSource {
    /// Open the file at `path` for reading.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
        })
    }
}

impl Source for FileSource {
    fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => {
                // Strip the trailing newline, if any.
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Some(line)
            }
            Err(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// FileSink
// ---------------------------------------------------------------------------

/// A [`Sink`] that writes lines to a file via buffered I/O.
pub struct FileSink {
    writer: BufWriter<File>,
}

impl FileSink {
    /// Create (or truncate) the file at `path` for writing.
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }
}

impl Sink for FileSink {
    fn write_line(&mut self, line: &str) {
        // Ignore write errors here; they will surface when the file is flushed.
        let _ = writeln!(self.writer, "{}", line);
    }
}

impl Drop for FileSink {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}

// ---------------------------------------------------------------------------
// Internal sink wrapper — allows extracting string output
// ---------------------------------------------------------------------------

/// Internal enum so we can store either a generic `Box<dyn Sink>` or a
/// known `StringSink` (from which we can later extract the accumulated text).
pub(crate) enum SinkKind {
    /// A [`StringSink`] whose output can be extracted via `into_string`.
    String(StringSink),
    /// An opaque boxed sink (file, user-provided, etc.).
    Boxed(Box<dyn Sink>),
}

impl SinkKind {
    pub(crate) fn write_line(&mut self, line: &str) {
        match self {
            SinkKind::String(s) => s.write_line(line),
            SinkKind::Boxed(b) => b.write_line(line),
        }
    }
}
