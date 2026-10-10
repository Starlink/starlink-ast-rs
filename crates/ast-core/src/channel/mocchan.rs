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

use super::source_sink::{FileSink, FileSource, SinkKind, StringSink, StringSource};
use super::{Channel, ChannelConfig, Sink, Source};
use crate::error::AstResult;
use crate::mappings::{IntraRegistry, IntraTransform};
use crate::regions::{CmpOperator, Moc};
use crate::serial::AnyObject;

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
pub struct MocChan {
    config: ChannelConfig,
    source: Option<Box<dyn Source>>,
    sink: Option<SinkKind>,
    warnings: Vec<String>,
    intra: IntraRegistry,
    moc_format: MocFormat,
    moc_line_len: Option<i32>,
}

impl MocChan {
    /// Create a `MocChan` with no source or sink.
    pub fn new() -> Self {
        Self {
            config: ChannelConfig::default(),
            source: None,
            sink: None,
            warnings: Vec::new(),
            intra: IntraRegistry::new(),
            moc_format: MocFormat::Unknown,
            moc_line_len: None,
        }
    }

    /// Create a `MocChan` that reads from the given in-memory string, one
    /// line at a time.
    pub fn from_string(text: &str) -> Self {
        Self {
            source: Some(Box::new(StringSource::new(text))),
            ..Self::new()
        }
    }

    /// Create a `MocChan` with an internal [`StringSink`] for writing; see
    /// [`into_string`](Self::into_string).
    pub fn with_string_sink() -> Self {
        Self {
            sink: Some(SinkKind::String(StringSink::new())),
            ..Self::new()
        }
    }

    /// Create a `MocChan` that reads from the file at `path`.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        Ok(Self {
            source: Some(Box::new(FileSource::open(path)?)),
            ..Self::new()
        })
    }

    /// Create a `MocChan` that writes to the file at `path`.
    pub fn to_file(path: &Path) -> io::Result<Self> {
        Ok(Self {
            sink: Some(SinkKind::Boxed(Box::new(FileSink::create(path)?))),
            ..Self::new()
        })
    }

    /// Consume the channel and return the text written to its internal
    /// [`StringSink`], one line per sink line, or an empty string if it has
    /// none.
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

impl Default for MocChan {
    fn default() -> Self {
        Self::new()
    }
}

impl Channel for MocChan {
    /// mocchan.c `Read`: a Moc with no MaxOrder set from all the source
    /// text, normalized; `MocFormat` is then set to the serialization read.
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.warnings.clear();
        let mut moc = Moc::new();
        let source = &mut self.source;
        let lines = std::iter::from_fn(|| source.as_mut()?.read_line());
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
        self.warnings.clear();
        let AnyObject::Moc(moc) = obj else {
            return Ok(0);
        };
        let json = self.moc_format == MocFormat::Json;
        let buflen = self.moc_line_len() as usize;
        let sink = &mut self.sink;
        moc.get_moc_text(
            json,
            buflen,
            &mut |line| {
                if let Some(sink) = sink.as_mut() {
                    sink.write_line(line);
                }
                Ok(())
            },
            "astWrite",
        )?;
        Ok(1)
    }

    fn register_intra(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()> {
        self.intra.register(transform)
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
