//! [`MocChan`] — a Channel that reads and writes MOCs as text.
//!
//! It reads and writes [`Moc`] objects in the string or JSON serialization
//! of the IVOA MOC recommendation, version 1.1.
//!
//! Reading takes lines from the source until it has none left and treats
//! them as one text, so a value may continue from one line to the next; the
//! lines after a malformed one are left for the next read.
//! Writing passes the text to the sink in lines of at most `MocLineLen`
//! characters.
//!
//! A native dump of a MocChan names its `MocFormat` the reverse way round,
//! `MocEnc = "JSON"` for the string serialization and `"STRING"` for JSON,
//! and its comment says which format the name means. Every version of the
//! C library writes and reads the names this way, so dumps and readers of
//! every version agree, and this port keeps to it.

// C: mocchan.c. A read goes through moc.c `astAddMocText`, here
// `Moc::add_moc_text`, and a write through `astGetMocText`, here
// `Moc::get_moc_text`.

use std::io;
use std::path::Path;
use std::sync::Arc;

use super::base::ChannelBase;
use super::source_sink::{FileSink, FileSource};
use super::{Channel, ChannelConfig, Sink, Source};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::IntraTransform;
use crate::regions::{CmpOperator, Moc};
use crate::serial::{AnyObject, Node, NodeValue, Serializable};

/// The values of the [MocFormat](MocChan::moc_format) attribute of a
/// [`MocChan`]: the serializations of the IVOA MOC recommendation, version
/// 1.1, in which a MOC is written as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MocFormat {
    /// Not known, the default; MOCs are written as strings.
    #[default]
    Unknown,
    /// The string serialization, such as `1/3 2/37,101-103`.
    String,
    /// The JSON serialization, such as `{"1":[3],"2":[37,101,102,103]}`.
    Json,
}

/// The default `MocLineLen`.
const DEFAULT_LINE_LEN: i32 = 80;

/// A [`Channel`] that reads and writes [`Moc`] objects as text, in the
/// string or JSON serialization of the IVOA MOC recommendation, version 1.1.
///
/// Writing a Moc to a MocChan with [`Channel::write`] makes a textual
/// description of it, in the serialization the
/// [MocFormat](Self::moc_format) attribute names; writing any other object
/// writes nothing and returns 0.
/// Reading from a MocChan with [`Channel::read`] makes a new Moc from all
/// the text left in its source, whichever serialization it is in.
///
/// The text is read from a [source](Self::set_source) and written to a
/// [sink](Self::set_sink), which connect the MocChan to wherever the text is
/// kept, and which should convert between any other character encoding the
/// text is kept in and ASCII.
/// [`from_string`](Self::from_string) and
/// [`with_string_sink`](Self::with_string_sink) read from and write to a
/// string, and [`from_file`](Self::from_file) and [`to_file`](Self::to_file)
/// read from and write to a file.
/// Without a source or SourceFile, a read takes the text from standard
/// input; without a sink or SinkFile, the text is written to standard
/// output.
/// A clone shares the original's source and sink.
///
/// Besides the attributes of every Channel, in [`ChannelConfig`], none of
/// which it uses, a MocChan has the [MocFormat](Self::moc_format) and
/// [MocLineLen](Self::moc_line_len) attributes, and the [Ident](Self::ident)
/// and [ID](Self::id) attributes.
///
/// # Example
///
/// ```
/// use starlink_ast::{Channel, MocChan, MocFormat};
///
/// let mut input = MocChan::from_string("1/3 2/37,101-103");
/// let moc = input.read().unwrap().expect("a Moc");
/// assert_eq!(input.moc_format(), MocFormat::String);
///
/// let mut out = MocChan::with_string_sink();
/// out.set_moc_format(MocFormat::Json);
/// assert_eq!(out.write(&moc).unwrap(), 1);
/// assert_eq!(out.into_string(), "{\"1\":[3],\"2\":[37,101,102,103]}\n");
/// ```
// C: the MocChan class (mocchan.c).
#[derive(Debug, Clone, Default)]
pub struct MocChan {
    base: ChannelBase,
    moc_format: MocFormat,
    moc_line_len: Option<i32>,
}

impl MocChan {
    /// Create a MocChan with no source or sink, and every attribute unset.
    ///
    /// Set a source with [`set_source`](Self::set_source) before reading
    /// and a sink with [`set_sink`](Self::set_sink) before writing, and the
    /// attributes with their setters; the other constructors make a MocChan
    /// with a source or sink already set.
    #[doc(alias = "astMocChan")]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a `MocChan` that reads from the given in-memory string, one
    /// line at a time.
    pub fn from_string(text: &str) -> Self {
        Self {
            base: ChannelBase::with_string_source(text),
            ..Self::new()
        }
    }

    /// Create a `MocChan` with an internal [`StringSink`](super::StringSink) for writing; see
    /// [`into_string`](Self::into_string).
    pub fn with_string_sink() -> Self {
        Self {
            base: ChannelBase::with_string_sink(),
            ..Self::new()
        }
    }

    /// Create a `MocChan` that reads from the file at `path`.
    ///
    /// # Errors
    ///
    /// Passes on the I/O error of opening the file.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let mut ch = Self::new();
        ch.set_source(Box::new(FileSource::open(path)?));
        Ok(ch)
    }

    /// Create a `MocChan` that writes to the file at `path`, replacing any
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

    /// Consume the channel and return the text written to its internal
    /// [`StringSink`](super::StringSink), one line per sink line, or an empty string if it has
    /// none.
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
    /// MocChan reads, or `None` when unset.
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

    /// The SinkFile attribute: the name of a text file to which the MocChan
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

    /// The Ident attribute: a permanent string identifying this MocChan.
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

    /// The ID attribute: a string identifying this MocChan, which the library
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

    /// The `MocFormat` attribute: the serialization in which a MOC is
    /// written.
    ///
    /// Default: [`MocFormat::Unknown`].
    ///
    /// A write uses the JSON serialization when it is [`MocFormat::Json`],
    /// and the string serialization otherwise.
    /// A read finds the serialization from the text it reads, whatever the
    /// value of MocFormat, and then sets MocFormat to it.
    #[doc(alias = "MocFormat")]
    pub fn moc_format(&self) -> MocFormat {
        self.moc_format
    }

    /// Set the [MocFormat](Self::moc_format) attribute.
    /// Setting [`MocFormat::Unknown`] is the same as clearing it.
    pub fn set_moc_format(&mut self, format: MocFormat) {
        self.moc_format = format;
    }

    /// Whether the [MocFormat](Self::moc_format) attribute is set, which is
    /// whether it is other than [`MocFormat::Unknown`].
    pub fn test_moc_format(&self) -> bool {
        self.moc_format != MocFormat::Unknown
    }

    /// Clear the [MocFormat](Self::moc_format) attribute.
    pub fn clear_moc_format(&mut self) {
        self.moc_format = MocFormat::Unknown;
    }

    /// The `MocLineLen` attribute: the most characters in each line a write
    /// passes to the sink.
    ///
    /// Default: 80.
    ///
    /// A line may be shorter, since a value is never split between lines:
    /// each line but the last ends after a space or comma.
    /// A write fails with [`ErrorCode::Smbuf`] if MocLineLen is too short to
    /// hold a value.
    #[doc(alias = "MocLineLen")]
    pub fn moc_line_len(&self) -> i32 {
        self.moc_line_len.unwrap_or(DEFAULT_LINE_LEN)
    }

    /// Set the [MocLineLen](Self::moc_line_len) attribute.
    /// A negative value is stored as zero.
    pub fn set_moc_line_len(&mut self, len: i32) {
        self.moc_line_len = Some(len.max(0));
    }

    /// Whether the [MocLineLen](Self::moc_line_len) attribute is set.
    pub fn test_moc_line_len(&self) -> bool {
        self.moc_line_len.is_some()
    }

    /// Clear the [MocLineLen](Self::moc_line_len) attribute.
    pub fn clear_moc_line_len(&mut self) {
        self.moc_line_len = None;
    }
}

impl Channel for MocChan {
    // C: mocchan.c `Read`: a Moc with no MaxOrder set from all the source
    // text, normalized; `MocFormat` is then set to the serialization read.
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.base.warnings.clear();
        let mut moc = Moc::new();
        let base = &self.base;
        let lines = std::iter::from_fn(|| base.read_line("MocChan"));
        let added = moc.add_moc_text(-1, lines, "astRead");
        // A SourceFile that could not be opened reads as empty text, so its
        // error takes the place of the failure to find any.
        self.base.take_read_error()?;
        let json = added?;
        let maxorder = moc.max_order();
        moc.moc_norm(false, CmpOperator::Or, 0, maxorder, "astRead")?;
        self.moc_format = if json {
            MocFormat::Json
        } else {
            MocFormat::String
        };
        Ok(Some(AnyObject::Moc(moc)))
    }

    // C: mocchan.c `Write`: a Moc as text in lines of at most `MocLineLen`
    // characters, returning 1; anything else writes nothing and returns 0.
    fn write(&mut self, obj: &AnyObject) -> AstResult<usize> {
        self.base.warnings.clear();
        let AnyObject::Moc(moc) = obj else {
            return Ok(0);
        };
        let json = self.moc_format == MocFormat::Json;
        let buflen = self.moc_line_len() as usize;
        let base = &self.base;
        moc.get_moc_text_for(
            json,
            buflen,
            &mut |line| base.write_line("MocChan", line),
            "astWrite",
        )?;
        base.flush();
        Ok(1)
    }

    fn register_intra(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()> {
        self.base.intra.register(transform)
    }

    fn warning_texts(&self) -> &[String] {
        &self.base.warnings
    }

    fn config(&self) -> &ChannelConfig {
        &self.base.config
    }

    fn config_mut(&mut self) -> &mut ChannelConfig {
        &mut self.base.config
    }
}

impl MocFormat {
    /// The name a dump gives it (mocchan.c `xencod`).
    ///
    /// The two format names are the reverse of what they say: a STRING
    /// format is dumped as `JSON` and a JSON one as `STRING`. C has always
    /// written and read them this way, the one reversal cancelling the
    /// other, and keeps the order so that dumps and readers of every
    /// version agree; this port does the same. The public MocFormat is
    /// unaffected.
    fn dump_name(self) -> &'static str {
        match self {
            MocFormat::Unknown => "UNKNOWN",
            MocFormat::String => "JSON",
            MocFormat::Json => "STRING",
        }
    }

    /// The comment a dump gives the name: which format the reversed name
    /// means (mocchan.c `Dump`).
    fn dump_comment(self) -> &'static str {
        match self {
            MocFormat::Unknown => "Format",
            MocFormat::String => "JSON means STRING (backwards compatibility)",
            MocFormat::Json => "STRING means JSON (backwards compatibility)",
        }
    }

    /// The format a dump's name means (mocchan.c `astLoadMocChan`), given
    /// the name as C's `astChrMatch` would match it, or `None` for a name
    /// that is neither.
    fn from_dump_name(key: &str) -> Option<Self> {
        match key {
            "JSON" => Some(MocFormat::String),
            "STRING" => Some(MocFormat::Json),
            _ => None,
        }
    }
}

impl Serializable for MocChan {
    fn class_name(&self) -> &'static str {
        "MocChan"
    }

    /// mocchan.c `Dump`, after the Object and Channel parts. The class
    /// comment is C's, copied from StcsChan.
    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("MocChan");
        node.class_comment = Some("STC-S I/O Channel".to_string());
        self.base.write_parts(&mut node, false);
        // C writes MocEnc even when unset, as a comment.
        let name = self.moc_format.dump_name();
        let comment = Some(self.moc_format.dump_comment().to_string());
        if self.test_moc_format() {
            node.push_string("MocEnc", name, comment);
        } else {
            node.push_unset("MocEnc", NodeValue::String(name.to_string()), comment);
        }
        node.write_int(
            "MocLLn",
            self.moc_line_len.is_some(),
            false,
            i64::from(self.moc_line_len()),
            "Moc line length",
        );
        Ok(node)
    }

    /// mocchan.c `astLoadMocChan`: each attribute through its setter. A
    /// MocEnc other than `UNKNOWN` must be one of the reversed format names,
    /// ignoring case, or it is AST__RDERR.
    fn from_node(node: &Node) -> AstResult<Self> {
        let mut ch = MocChan {
            base: ChannelBase::read_parts(node)?,
            ..MocChan::new()
        };
        let part = node.section("MocChan");
        if let Some(name) = part.get_string_if_set("MocEnc")
            && name != "UNKNOWN"
        {
            // mocchan.c `FindString` matches with `astChrMatch`.
            let key = crate::mappings::helpers::chr_match_key(&name);
            ch.moc_format = MocFormat::from_dump_name(&key).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Rderr,
                    format!(
                        "astRead(Channel): Illegal value '{name}' supplied for the MocChan \
                         component 'MocEnc'."
                    ),
                )
            })?;
        }
        if let Some(len) = part.get_int_any_if_set("MocLLn") {
            ch.set_moc_line_len(len as i32);
        }
        Ok(ch)
    }
}
