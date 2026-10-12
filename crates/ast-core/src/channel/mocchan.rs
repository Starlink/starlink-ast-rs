//! [`MocChan`] — a Channel that reads and writes MOCs as text.
//!
//! A port of C `mocchan.c`. It reads and writes [`Moc`] objects in the
//! string or JSON serialization of the IVOA MOC recommendation, through
//! `Moc::add_moc_text` and [`Moc::get_moc_text`], which are `astAddMocText`
//! and `astGetMocText` in C.
//!
//! Reading takes lines from the source until it has none left and treats
//! them as one text, so a value may continue from one line to the next; the
//! lines after a malformed one are left for the next read. Writing passes
//! the text to the sink in lines of at most `MocLineLen` characters.
//!
//! C's MocChan Dump and Load are not ported: no Channel is a serializable
//! object in this port.

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

/// The `MocFormat` attribute of a [`MocChan`]: the serialization of the
/// last MOC read, and the one used to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MocFormat {
    /// Not known; MOCs are written as strings. The default.
    #[default]
    Unknown,
    /// The string serialization, such as `"1/3 2/37,101-103"`.
    String,
    /// The JSON serialization, such as `{"1":[3],"2":[37,101,102,103]}`.
    Json,
}

/// The default `MocLineLen`.
const DEFAULT_LINE_LEN: i32 = 80;

/// A Channel that reads and writes MOCs in the string or JSON serialization
/// of the IVOA MOC recommendation (C's `MocChan`).
///
/// Writing an object other than a [`Moc`] writes nothing and returns 0.
/// Reading makes a Moc from all the remaining source text; reading when
/// there is none, or text that is not a MOC, is `ErrorCode::Inmoc`.
#[derive(Debug, Clone, Default)]
pub struct MocChan {
    base: ChannelBase,
    moc_format: MocFormat,
    moc_line_len: Option<i32>,
}

impl MocChan {
    /// Create a `MocChan` with no source or sink.
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
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let mut ch = Self::new();
        ch.set_source(Box::new(FileSource::open(path)?));
        Ok(ch)
    }

    /// Create a `MocChan` that writes to the file at `path`.
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

    /// The `MocFormat` attribute. A read sets it to the serialization it
    /// found; a write uses JSON if it is [`MocFormat::Json`] and strings
    /// otherwise.
    pub fn moc_format(&self) -> MocFormat {
        self.moc_format
    }

    /// Set the `MocFormat` attribute. Setting [`MocFormat::Unknown`] is the
    /// same as clearing it.
    pub fn set_moc_format(&mut self, format: MocFormat) {
        self.moc_format = format;
    }

    /// Whether the `MocFormat` attribute is set (to other than
    /// [`MocFormat::Unknown`]).
    pub fn test_moc_format(&self) -> bool {
        self.moc_format != MocFormat::Unknown
    }

    /// Clear the `MocFormat` attribute.
    pub fn clear_moc_format(&mut self) {
        self.moc_format = MocFormat::Unknown;
    }

    /// The `MocLineLen` attribute: the most characters written in one line.
    /// The default is 80.
    pub fn moc_line_len(&self) -> i32 {
        self.moc_line_len.unwrap_or(DEFAULT_LINE_LEN)
    }

    /// Set the `MocLineLen` attribute; a negative value is stored as zero.
    /// A length shorter than a value in the MOC makes writing it
    /// `ErrorCode::Smbuf`.
    pub fn set_moc_line_len(&mut self, len: i32) {
        self.moc_line_len = Some(len.max(0));
    }

    /// Whether the `MocLineLen` attribute is set.
    pub fn test_moc_line_len(&self) -> bool {
        self.moc_line_len.is_some()
    }

    /// Clear the `MocLineLen` attribute.
    pub fn clear_moc_line_len(&mut self) {
        self.moc_line_len = None;
    }
}

impl Channel for MocChan {
    /// mocchan.c `Read`: a Moc with no MaxOrder set from all the source
    /// text, normalized; `MocFormat` is then set to the serialization read.
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.base.warnings.clear();
        let mut moc = Moc::new();
        let base = &self.base;
        let lines = std::iter::from_fn(|| base.read_line());
        let json = moc.add_moc_text(-1, lines, "astRead")?;
        let maxorder = moc.max_order();
        moc.moc_norm(false, CmpOperator::Or, 0, maxorder, "astRead")?;
        self.moc_format = if json {
            MocFormat::Json
        } else {
            MocFormat::String
        };
        Ok(Some(AnyObject::Moc(moc)))
    }

    /// mocchan.c `Write`: a Moc as text in lines of at most `MocLineLen`
    /// characters, returning 1; anything else writes nothing and returns 0.
    fn write(&mut self, obj: &AnyObject) -> AstResult<usize> {
        self.base.warnings.clear();
        let AnyObject::Moc(moc) = obj else {
            return Ok(0);
        };
        let json = self.moc_format == MocFormat::Json;
        let buflen = self.moc_line_len() as usize;
        let base = &self.base;
        moc.get_moc_text(
            json,
            buflen,
            &mut |line| {
                base.write_line(line);
                Ok(())
            },
            "astWrite",
        )?;
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
    fn dump_name(self) -> &'static str {
        match self {
            MocFormat::Unknown => "UNKNOWN",
            MocFormat::String => "STRING",
            MocFormat::Json => "JSON",
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
        let comment = Some("Format".to_string());
        if self.test_moc_format() {
            node.push_string("MocEnc", name, comment);
        } else {
            node.push_unset("MocEnc", NodeValue::String(name.to_string()), comment);
        }
        if let Some(len) = self.moc_line_len {
            node.push_int(
                "MocLLn",
                i64::from(len),
                Some("Moc line length".to_string()),
            );
        }
        Ok(node)
    }

    /// mocchan.c `astLoadMocChan`: each attribute through its setter. A
    /// MocEnc other than `UNKNOWN` must name a format, ignoring case, or it
    /// is AST__RDERR.
    fn from_node(node: &Node) -> AstResult<Self> {
        let mut ch = MocChan {
            base: ChannelBase::read_parts(node)?,
            ..MocChan::new()
        };
        let part = node.section("MocChan");
        if let Some(name) = part.get_string_if_set("MocEnc")
            && name != "UNKNOWN"
        {
            ch.moc_format = if name.eq_ignore_ascii_case("STRING") {
                MocFormat::String
            } else if name.eq_ignore_ascii_case("JSON") {
                MocFormat::Json
            } else {
                return Err(AstError::new(
                    ErrorCode::Rderr,
                    format!(
                        "astRead(Channel): Illegal value '{name}' supplied for the MocChan \
                         component 'MocEnc'."
                    ),
                ));
            };
        }
        if let Some(len) = part.get_int_any_if_set("MocLLn") {
            ch.set_moc_line_len(len as i32);
        }
        Ok(ch)
    }
}
