//! [`NativeChannel`] — Channel implementation for AST's Native text format.

use std::io;
use std::path::Path;
use std::sync::Arc;

use super::base::ChannelBase;
use super::source_sink::{FileSink, FileSource};
use super::{Channel, ChannelConfig, Sink, Source};
use crate::error::AstResult;
use crate::mappings::intramap::with_registry;
use crate::mappings::{IntraRegistry, IntraTransform};
use crate::serial::{self, AnyObject, Node, Serializable};

// ---------------------------------------------------------------------------
// NativeChannel
// ---------------------------------------------------------------------------

/// Channel implementation for AST's Native text format (C's `Channel`).
///
/// Reads and writes AST objects using the line-oriented Native
/// serialisation format (the same format produced by the C library's
/// `astWrite` with a `StdChannel`). A clone shares the original's source
/// and sink.
#[derive(Debug, Clone, Default)]
pub struct NativeChannel {
    pub(crate) base: ChannelBase,
}

impl NativeChannel {
    /// Create an empty `NativeChannel` with no source or sink.
    ///
    /// You must set a source (for reading) or a sink (for writing) before
    /// performing I/O.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a `NativeChannel` that reads from the given in-memory string
    /// and gives the IntraMaps it reads the transformations in `intra`.
    pub(crate) fn from_string_with(text: &str, intra: IntraRegistry) -> Self {
        let mut ch = Self::from_string(text);
        ch.base.intra = intra;
        ch
    }

    /// Create a `NativeChannel` that reads from the given in-memory string.
    pub fn from_string(text: &str) -> Self {
        Self {
            base: ChannelBase::with_string_source(text),
        }
    }

    /// Create a `NativeChannel` with an internal [`StringSink`](super::StringSink) for writing.
    ///
    /// After writing, call [`into_string`](Self::into_string) to extract the
    /// accumulated text.
    pub fn with_string_sink() -> Self {
        Self {
            base: ChannelBase::with_string_sink(),
        }
    }

    /// Create a `NativeChannel` that reads from the file at `path`.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let mut ch = Self::new();
        ch.set_source(Box::new(FileSource::open(path)?));
        Ok(ch)
    }

    /// Create a `NativeChannel` that writes to the file at `path`.
    pub fn to_file(path: &Path) -> io::Result<Self> {
        let mut ch = Self::new();
        ch.set_sink(Box::new(FileSink::create(path)?));
        Ok(ch)
    }

    /// Consume the channel and return the accumulated output string.
    ///
    /// This only works if the sink is a [`StringSink`](super::StringSink) (i.e. the channel
    /// was created with [`with_string_sink`](Self::with_string_sink)).
    /// Returns an empty string if no `StringSink` is present.
    pub fn into_string(self) -> String {
        self.base.into_string()
    }

    /// Set a custom source for reading.
    pub fn set_source(&mut self, source: Box<dyn Source>) {
        self.base.set_source(source);
    }

    /// Set a custom sink for writing.
    pub fn set_sink(&mut self, sink: Box<dyn Sink>) {
        self.base.set_sink(sink);
    }

    /// The Object-class `Ident` attribute, blank unless set.
    pub fn ident(&self) -> &str {
        self.base.ident.as_deref().unwrap_or("")
    }

    /// Set `Ident`.
    pub fn set_ident(&mut self, ident: &str) {
        self.base.ident = Some(ident.to_string());
    }

    /// Whether `Ident` is set.
    pub fn test_ident(&self) -> bool {
        self.base.ident.is_some()
    }

    /// Clear `Ident`.
    pub fn clear_ident(&mut self) {
        self.base.ident = None;
    }

    /// The Object-class `ID` attribute, blank unless set. Unlike `Ident`, a
    /// clone does not inherit it, as C's `astCopy` does not copy it.
    pub fn id(&self) -> &str {
        self.base.id.get()
    }

    /// Set `ID`.
    pub fn set_id(&mut self, id: &str) {
        self.base.id.set(id);
    }

    /// Whether `ID` is set.
    pub fn test_id(&self) -> bool {
        self.base.id.test()
    }

    /// Clear `ID`.
    pub fn clear_id(&mut self) {
        self.base.id.clear();
    }

    /// Internal: collect all lines from the source into a single string.
    fn collect_source_text(&mut self) -> Option<String> {
        let mut text = String::new();
        while let Some(line) = self.base.read_line() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&line);
        }
        if text.is_empty() { None } else { Some(text) }
    }
}

impl Channel for NativeChannel {
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.base.warnings.clear();
        let text = match self.collect_source_text() {
            Some(t) => t,
            None => return Ok(None),
        };
        let node = serial::native_read::parse_native(&text)?;
        let obj = with_registry(&self.base.intra, || serial::any_object_from_node(&node))?;
        Ok(Some(obj))
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<usize> {
        self.base.warnings.clear();
        let node = serial::any_object_to_node(obj)?;
        let options = serial::native_write::RenderOptions {
            comment: self.base.config.comment(),
            full: self.base.config.full() as i8,
            // C's indentation loops run no times for a negative Indent, so
            // it writes what an Indent of 0 would.
            indent: self.base.config.indent().max(0) as usize,
        };
        let text = serial::native_write::render_native_with_options(&node, &options)?;

        for line in text.lines() {
            self.base.write_line(line);
        }
        // The native encoding can represent every AST class, so a write that
        // did not error wrote the object. Mirrors C `Write`'s `ret = 1;` on
        // the NATIVE_ENCODING branch (fitschan.c:40690).
        Ok(1)
    }

    fn warning_texts(&self) -> &[String] {
        &self.base.warnings
    }

    fn register_intra(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()> {
        self.base.intra.register(transform)
    }

    fn config(&self) -> &ChannelConfig {
        &self.base.config
    }

    fn config_mut(&mut self) -> &mut ChannelConfig {
        &mut self.base.config
    }
}

impl Serializable for NativeChannel {
    fn class_name(&self) -> &'static str {
        "Channel"
    }

    /// channel.c `Dump`: the Object part, then the Channel attributes that
    /// are set.
    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("Channel");
        node.class_comment = Some("Basic I/O Channel".to_string());
        self.base.write_parts(&mut node, true);
        Ok(node)
    }

    /// channel.c `astLoadChannel`: no source or sink, and each attribute
    /// through its setter.
    fn from_node(node: &Node) -> AstResult<Self> {
        Ok(Self {
            base: ChannelBase::read_parts(node)?,
        })
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
