//! [`NativeChannel`] — Channel implementation for AST's Native text format.

use std::io;
use std::path::Path;
use std::sync::Arc;

use super::base::ChannelBase;
use super::source_sink::{FileSink, FileSource};
use super::{Channel, ChannelConfig, Sink, Source};
use crate::error::AstResult;
use crate::mappings::{IntraRegistry, IntraTransform};
use crate::serial::{self, AnyObject, Node, Serializable};

// ---------------------------------------------------------------------------
// NativeChannel
// ---------------------------------------------------------------------------

/// A [`Channel`] that reads and writes AST objects in AST's native text
/// format.
///
/// Writing an object to a NativeChannel with [`Channel::write`] makes a
/// textual description of it, and reading from one with [`Channel::read`]
/// makes a new object from such a description.
/// The native format can describe any AST object, and the descriptions of
/// several objects can follow one another, to be read back in the same
/// order.
///
/// The text is read from a [source](Self::set_source) and written to a
/// [sink](Self::set_sink), which connect the NativeChannel to wherever the
/// text is kept.
/// [`from_string`](Self::from_string) and
/// [`with_string_sink`](Self::with_string_sink) read from and write to a
/// string, and [`from_file`](Self::from_file) and [`to_file`](Self::to_file)
/// read from and write to a file.
/// Without a source or SourceFile, a read takes the text from standard
/// input; without a sink or SinkFile, the text is written to standard
/// output.
/// A clone shares the original's source and sink.
///
/// Besides the attributes of every Channel, in [`ChannelConfig`], a
/// NativeChannel has the [Ident](Self::ident) and [ID](Self::id) attributes.
///
/// # Example
///
/// ```
/// use starlink_ast::mappings::ZoomMap;
/// use starlink_ast::{AnyObject, Channel, NativeChannel};
///
/// let mut out = NativeChannel::with_string_sink();
/// let written = out.write(&AnyObject::ZoomMap(ZoomMap::new(2, 3.5).unwrap()));
/// assert_eq!(written.unwrap(), 1);
/// let text = out.into_string();
/// assert!(text.starts_with(" Begin ZoomMap"));
///
/// let mut input = NativeChannel::from_string(&text);
/// let object = input.read().unwrap().expect("one object");
/// assert_eq!(object.as_mapping().unwrap().nin(), 2);
/// assert!(input.read().unwrap().is_none());
/// ```
// C: the Channel class (channel.c), whose text this is; C's StdChannel
// writes the same.
#[doc(alias = "Channel")]
#[derive(Debug, Clone, Default)]
pub struct NativeChannel {
    pub(crate) base: ChannelBase,
}

impl NativeChannel {
    /// Create a NativeChannel with no source or sink, and every attribute
    /// unset.
    ///
    /// Set a source with [`set_source`](Self::set_source) before reading
    /// and a sink with [`set_sink`](Self::set_sink) before writing, and the
    /// attributes with their setters; the other constructors make a
    /// NativeChannel with a source or sink already set.
    ///
    /// # See also
    ///
    /// `astChannel`, which takes the source and sink functions and the
    /// attribute settings as arguments.
    #[doc(alias = "astChannel")]
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
    ///
    /// # Errors
    ///
    /// Passes on the I/O error of opening the file.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let mut ch = Self::new();
        ch.set_source(Box::new(FileSource::open(path)?));
        Ok(ch)
    }

    /// Create a `NativeChannel` that writes to the file at `path`, replacing any
    /// file of that name.
    ///
    /// # Errors
    ///
    /// Passes on the I/O error of creating the file.
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

    /// The SourceFile attribute: the name of a text file from which the
    /// NativeChannel reads, or `None` when unset.
    ///
    /// Default: unset, when the text comes from the [source](Self::set_source),
    /// or from standard input if there is none.
    ///
    /// When it is set, the file is used in place of the source.
    /// It is opened by the first read after the attribute is set, which
    /// fails if the file cannot be opened.
    /// Setting the attribute again or clearing it closes the file.
    /// A clone does not inherit it.
    #[doc(alias = "SourceFile")]
    pub fn source_file(&self) -> Option<&str> {
        self.base.source_file()
    }

    /// Set [`source_file`](Self::source_file), closing any file open for the
    /// last value.
    pub fn set_source_file(&mut self, source_file: &str) {
        self.base.set_source_file(source_file);
    }

    /// Whether [`source_file`](Self::source_file) is set.
    pub fn test_source_file(&self) -> bool {
        self.base.source_file().is_some()
    }

    /// Clear [`source_file`](Self::source_file), closing any file open for it.
    /// Reads then return to the source, or to standard input if there is
    /// none.
    pub fn clear_source_file(&mut self) {
        self.base.clear_source_file();
    }

    /// The SinkFile attribute: the name of a text file to which the NativeChannel
    /// writes, or `None` when unset.
    ///
    /// Default: unset, when the text goes to the [sink](Self::set_sink), or
    /// to standard output if there is none.
    ///
    /// When it is set, the file is used in place of the sink.
    /// It is created, replacing any file of that name, by the first write
    /// after the attribute is set, which fails if it cannot be created.
    /// Setting the attribute again or clearing it closes the file.
    /// A clone does not inherit it.
    #[doc(alias = "SinkFile")]
    pub fn sink_file(&self) -> Option<&str> {
        self.base.sink_file()
    }

    /// Set [`sink_file`](Self::sink_file), closing any file open for the
    /// last value.
    pub fn set_sink_file(&mut self, sink_file: &str) {
        self.base.set_sink_file(sink_file);
    }

    /// Whether [`sink_file`](Self::sink_file) is set.
    pub fn test_sink_file(&self) -> bool {
        self.base.sink_file().is_some()
    }

    /// Clear [`sink_file`](Self::sink_file), closing any file open for it.
    /// Writes then return to the sink, or to standard output if there is
    /// none.
    pub fn clear_sink_file(&mut self) {
        self.base.clear_sink_file();
    }

    /// The Ident attribute: a permanent string identifying this Channel.
    ///
    /// A clone keeps it, and it is written to and read back from a Native
    /// dump.
    /// Default: an empty string.
    #[doc(alias = "Ident")]
    pub fn ident(&self) -> &str {
        self.base.ident.as_deref().unwrap_or("")
    }

    /// Set [`ident`](Self::ident).
    pub fn set_ident(&mut self, ident: &str) {
        self.base.ident = Some(ident.to_string());
    }

    /// Whether [`ident`](Self::ident) is set.
    pub fn test_ident(&self) -> bool {
        self.base.ident.is_some()
    }

    /// Clear [`ident`](Self::ident).
    pub fn clear_ident(&mut self) {
        self.base.ident = None;
    }

    /// The ID attribute: a string identifying this Channel, which the library
    /// does not use.
    ///
    /// It is written to and read back from a Native dump, but unlike
    /// [`ident`](Self::ident) a clone does not keep it.
    /// Default: an empty string.
    // C's `astCopy` does not copy the ID.
    #[doc(alias = "ID")]
    pub fn id(&self) -> &str {
        self.base.id.get()
    }

    /// Set [`id`](Self::id).
    pub fn set_id(&mut self, id: &str) {
        self.base.id.set(id);
    }

    /// Whether [`id`](Self::id) is set.
    pub fn test_id(&self) -> bool {
        self.base.id.test()
    }

    /// Clear [`id`](Self::id).
    pub fn clear_id(&mut self) {
        self.base.id.clear();
    }
}

impl Channel for NativeChannel {
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.base.read_native("Channel")
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<usize> {
        self.base.warnings.clear();
        let node = serial::any_object_to_node(obj)?;
        let options = serial::native_write::RenderOptions::from_config(&self.base.config);
        let text = serial::native_write::render_native_with_options(&node, &options)?;

        for line in text.lines() {
            self.base.write_line("Channel", line)?;
        }
        self.base.flush();
        // The native encoding can represent every AST class, so a write that
        // did not error wrote the object, as C's Channel `Write` returns 1.
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
