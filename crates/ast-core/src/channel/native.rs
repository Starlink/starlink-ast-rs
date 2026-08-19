//! [`NativeChannel`] — Channel implementation for AST's Native text format.

use std::io;
use std::path::Path;

use super::source_sink::{FileSink, FileSource, StringSink, StringSource};
use super::{Channel, ChannelConfig, Sink, Source};
use crate::error::AstResult;
use crate::serial::{self, AnyObject};

// ---------------------------------------------------------------------------
// Internal sink wrapper — allows extracting string output
// ---------------------------------------------------------------------------

/// Internal enum so we can store either a generic `Box<dyn Sink>` or a
/// known `StringSink` (from which we can later extract the accumulated text).
enum SinkKind {
    /// A [`StringSink`] whose output can be extracted via [`NativeChannel::into_string`].
    String(StringSink),
    /// An opaque boxed sink (file, user-provided, etc.).
    Boxed(Box<dyn Sink>),
}

impl SinkKind {
    fn write_line(&mut self, line: &str) {
        match self {
            SinkKind::String(s) => s.write_line(line),
            SinkKind::Boxed(b) => b.write_line(line),
        }
    }
}

// ---------------------------------------------------------------------------
// NativeChannel
// ---------------------------------------------------------------------------

/// Channel implementation for AST's Native text format.
///
/// Reads and writes AST objects using the line-oriented Native
/// serialisation format (the same format produced by the C library's
/// `astWrite` with a `StdChannel`).
pub struct NativeChannel {
    config: ChannelConfig,
    source: Option<Box<dyn Source>>,
    sink: Option<SinkKind>,
    warnings: Vec<String>,
}

impl NativeChannel {
    /// Create an empty `NativeChannel` with no source or sink.
    ///
    /// You must set a source (for reading) or a sink (for writing) before
    /// performing I/O.
    pub fn new() -> Self {
        Self {
            config: ChannelConfig::default(),
            source: None,
            sink: None,
            warnings: Vec::new(),
        }
    }

    /// Create a `NativeChannel` that reads from the given in-memory string.
    pub fn from_string(text: &str) -> Self {
        Self {
            config: ChannelConfig::default(),
            source: Some(Box::new(StringSource::new(text))),
            sink: None,
            warnings: Vec::new(),
        }
    }

    /// Create a `NativeChannel` with an internal [`StringSink`] for writing.
    ///
    /// After writing, call [`into_string`](Self::into_string) to extract the
    /// accumulated text.
    pub fn with_string_sink() -> Self {
        Self {
            config: ChannelConfig::default(),
            source: None,
            sink: Some(SinkKind::String(StringSink::new())),
            warnings: Vec::new(),
        }
    }

    /// Create a `NativeChannel` that reads from the file at `path`.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let source = FileSource::open(path)?;
        Ok(Self {
            config: ChannelConfig::default(),
            source: Some(Box::new(source)),
            sink: None,
            warnings: Vec::new(),
        })
    }

    /// Create a `NativeChannel` that writes to the file at `path`.
    pub fn to_file(path: &Path) -> io::Result<Self> {
        let sink = FileSink::create(path)?;
        Ok(Self {
            config: ChannelConfig::default(),
            source: None,
            sink: Some(SinkKind::Boxed(Box::new(sink))),
            warnings: Vec::new(),
        })
    }

    /// Consume the channel and return the accumulated output string.
    ///
    /// This only works if the sink is a [`StringSink`] (i.e. the channel
    /// was created with [`with_string_sink`](Self::with_string_sink)).
    /// Returns an empty string if no `StringSink` is present.
    pub fn into_string(self) -> String {
        match self.sink {
            Some(SinkKind::String(ss)) => ss.into_string(),
            _ => String::new(),
        }
    }

    /// Set a custom source for reading.
    pub fn set_source(&mut self, source: Box<dyn Source>) {
        self.source = Some(source);
    }

    /// Set a custom sink for writing.
    pub fn set_sink(&mut self, sink: Box<dyn Sink>) {
        self.sink = Some(SinkKind::Boxed(sink));
    }

    /// Internal: collect all lines from the source into a single string.
    fn collect_source_text(&mut self) -> Option<String> {
        let source = self.source.as_mut()?;
        let mut text = String::new();
        while let Some(line) = source.read_line() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&line);
        }
        if text.is_empty() { None } else { Some(text) }
    }
}

impl Default for NativeChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl Channel for NativeChannel {
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.warnings.clear();
        let text = match self.collect_source_text() {
            Some(t) => t,
            None => return Ok(None),
        };
        let node = serial::native_read::parse_native(&text)?;
        let obj = serial::any_object_from_node(&node)?;
        Ok(Some(obj))
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<()> {
        self.warnings.clear();
        let node = serial::any_object_to_node(obj)?;
        let options = serial::native_write::RenderOptions {
            comment: self.config.comment,
            full: self.config.full,
            indent: self.config.indent,
        };
        let text = serial::native_write::render_native_with_options(&node, &options)?;

        if let Some(sink) = self.sink.as_mut() {
            for line in text.lines() {
                sink.write_line(line);
            }
        }
        Ok(())
    }

    fn warning_texts(&self) -> &[String] {
        &self.warnings
    }

    fn config(&self) -> &ChannelConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut ChannelConfig {
        &mut self.config
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::ZoomMap;

    #[test]
    fn round_trip_zoom_map() {
        // Create a ZoomMap and wrap it in AnyObject
        let zm = ZoomMap::new(2, 3.5).unwrap();
        let obj = AnyObject::ZoomMap(zm);

        // Write to a NativeChannel with string sink
        let mut ch = NativeChannel::with_string_sink();
        ch.write(&obj).expect("write should succeed");
        let text = ch.into_string();
        assert!(!text.is_empty(), "output text should not be empty");

        // Read back from the text
        let mut ch2 = NativeChannel::from_string(&text);
        let obj2 = ch2.read().expect("read should succeed");
        let obj2 = obj2.expect("should read an object");

        // Verify it's a ZoomMap with the same properties
        let mapping = obj2.as_mapping().expect("should be a Mapping");
        assert_eq!(mapping.nin(), 2);
        assert_eq!(mapping.nout(), 2);
    }

    #[test]
    fn from_string_reads_object() {
        let text = " Begin ZoomMap\n\
                          Nin = 2\n\
                       IsA Mapping\n\
                          Zoom = 3.5\n\
                       End ZoomMap\n";
        let mut ch = NativeChannel::from_string(text);
        let obj = ch.read().expect("read should succeed");
        assert!(obj.is_some(), "should read an object");
        let obj = obj.unwrap();
        let mapping = obj.as_mapping().expect("should be a Mapping");
        assert_eq!(mapping.nin(), 2);
    }

    #[test]
    fn with_string_sink_produces_output() {
        let zm = ZoomMap::new(1, 2.0).unwrap();
        let obj = AnyObject::ZoomMap(zm);

        let mut ch = NativeChannel::with_string_sink();
        ch.write(&obj).expect("write should succeed");
        let text = ch.into_string();
        assert!(text.contains("ZoomMap"), "output should mention ZoomMap");
        assert!(
            text.contains("Zoom"),
            "output should contain Zoom attribute"
        );
    }

    #[test]
    fn empty_source_returns_none() {
        let mut ch = NativeChannel::from_string("");
        let result = ch.read().expect("read should not error");
        assert!(result.is_none(), "empty source should return None");
    }
}
