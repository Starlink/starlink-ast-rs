//! The part every Channel shares: C's Channel class part.
//!
//! A Channel's source and sink are held through shared handles, so a clone
//! reads from and writes to the same place as the original, as a copy made
//! by C's `astCopy` calls the same source and sink functions. A Channel read
//! from a dump has neither, as C's `astLoadChannel` sets them to NULL.

use std::cell::RefCell;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use super::source_sink::{StringSink, StringSource};
use super::{ChannelConfig, Sink, Source};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::source_sink::NotCopied;
use crate::mappings::IntraRegistry;
use crate::mappings::intramap::with_registry;
use crate::object_id::ObjectId;
use crate::serial::Node;
use crate::serial::native_stream::Reader;
use crate::serial::{AnyObject, any_object_from_node};

/// A sink that is either a [`StringSink`], whose text can be taken back,
/// or any other.
enum SinkKind {
    String(StringSink),
    Boxed(Box<dyn Sink>),
}

/// A Channel's source, with the lines a read handed back, which the next
/// read takes first.
struct LineSource {
    source: Box<dyn Source>,
    returned: std::collections::VecDeque<String>,
}

/// The text files a Channel reads from and writes to through its `SourceFile`
/// and `SinkFile` attributes, open for the names the Channel holds.
///
/// A file is opened by the first read or write that needs it and closed when
/// its attribute is set again, cleared, or the Channel is dropped.
/// A clone has neither attribute and no file open, as C's Channel `Copy`
/// clears them.
#[derive(Default)]
struct Files {
    fd_in: Option<BufReader<File>>,
    fd_out: Option<BufWriter<File>>,
    /// The error from opening a file for a read, which the read that called
    /// [`ChannelBase::read_line`] takes with [`ChannelBase::take_read_error`].
    read_error: Option<AstError>,
    /// Lines handed back to a file or standard input.
    returned: std::collections::VecDeque<String>,
}

/// [`Files`], which a clone does not inherit.
#[derive(Default)]
struct FilesCell(RefCell<Files>);

impl Clone for FilesCell {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Files {
    /// C's opening of the SourceFile in `ReadData`, which refuses the file
    /// the SinkFile has open.
    fn open_source_file(
        &mut self,
        class: &str,
        name: &str,
        sink_file: Option<&str>,
    ) -> AstResult<()> {
        if self.fd_out.is_some() && sink_file == Some(name) {
            return Err(AstError::new(
                ErrorCode::Rderr,
                format!(
                    "astRead({class}): Failed to open input SourceFile '{name}' - the file is currently being used as the output SinkFile."
                ),
            ));
        }
        let file = File::open(name).map_err(|e| {
            AstError::new(
                ErrorCode::Rderr,
                format!("astRead({class}): Failed to open input SourceFile '{name}' - {e}."),
            )
        })?;
        self.fd_in = Some(BufReader::new(file));
        Ok(())
    }

    /// C's opening of the SinkFile in `WriteData`.
    fn open_sink_file(&mut self, class: &str, name: &str) -> AstResult<()> {
        let file = File::create(name).map_err(|e| {
            AstError::new(
                ErrorCode::Wrerr,
                format!("astWrite({class}): Failed to open output SinkFile '{name}' - {e}."),
            )
        })?;
        self.fd_out = Some(BufWriter::new(file));
        Ok(())
    }
}

/// C's line read in `ReadData`: the characters up to a newline, which is
/// dropped; `None` at end-of-file with nothing read.
fn read_text_line(reader: &mut impl BufRead) -> Option<String> {
    let mut line = Vec::new();
    // C treats a read error as the end of the text.
    let n = reader.read_until(b'\n', &mut line).ok()?;
    if n == 0 {
        return None;
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    Some(String::from_utf8_lossy(&line).into_owned())
}

/// Lock `m`, recovering the value from a panic in another holder, which
/// cannot leave a line source or sink half-updated.
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The Channel class part of a Channel: its Object and Channel attributes,
/// source and sink, the warnings from its last read or write, and the
/// transformations the IntraMaps it reads can name.
#[derive(Clone, Default)]
pub(crate) struct ChannelBase {
    pub(crate) config: ChannelConfig,
    pub(crate) ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    pub(crate) id: crate::object_id::ObjectId,
    source: Option<Arc<Mutex<LineSource>>>,
    sink: Option<Arc<Mutex<SinkKind>>>,
    /// The SourceFile attribute, which a clone does not inherit.
    source_file: NotCopied<String>,
    /// The SinkFile attribute, which a clone does not inherit.
    sink_file: NotCopied<String>,
    files: FilesCell,
    pub(crate) warnings: Vec<String>,
    pub(crate) intra: IntraRegistry,
}

impl fmt::Debug for ChannelBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelBase")
            .field("config", &self.config)
            .field("ident", &self.ident)
            .field("source", &self.source.is_some())
            .field("sink", &self.sink.is_some())
            .field("source_file", &self.source_file.0)
            .field("sink_file", &self.sink_file.0)
            .field("warnings", &self.warnings)
            .finish()
    }
}

impl ChannelBase {
    /// Read from the lines of `text`.
    pub(crate) fn with_string_source(text: &str) -> Self {
        let mut base = Self::default();
        base.set_source(Box::new(StringSource::new(text)));
        base
    }

    /// Write to a [`StringSink`], whose text [`into_string`] gives.
    ///
    /// [`into_string`]: ChannelBase::into_string
    pub(crate) fn with_string_sink() -> Self {
        Self {
            sink: Some(Arc::new(Mutex::new(SinkKind::String(StringSink::new())))),
            ..Self::default()
        }
    }

    pub(crate) fn set_source(&mut self, source: Box<dyn Source>) {
        self.source = Some(Arc::new(Mutex::new(LineSource {
            source,
            returned: std::collections::VecDeque::new(),
        })));
    }

    pub(crate) fn set_sink(&mut self, sink: Box<dyn Sink>) {
        self.sink = Some(Arc::new(Mutex::new(SinkKind::Boxed(sink))));
    }

    /// The next line from the SourceFile, if set, or else the source, or
    /// else standard input; `None` at the end of the text.
    ///
    /// A SourceFile that cannot be opened ends the text, and
    /// [`take_read_error`](Self::take_read_error) gives the error.
    /// `class` names the Channel's class in the error.
    // C: channel.c `ReadData`: the SourceFile is opened by the first read
    // that needs it, and it takes precedence over the source function.
    pub(crate) fn read_line(&self, class: &str) -> Option<String> {
        let mut files = self.files.0.borrow_mut();
        if let Some(name) = &self.source_file.0
            && files.fd_in.is_none()
            && let Err(e) = files.open_source_file(class, name, self.sink_file.0.as_deref())
        {
            files.read_error.get_or_insert(e);
            return None;
        }
        if files.fd_in.is_some() || self.source.is_none() {
            if let Some(line) = files.returned.pop_front() {
                return Some(line);
            }
            return match &mut files.fd_in {
                Some(fd) => read_text_line(fd),
                None => read_text_line(&mut io::stdin().lock()),
            };
        }
        drop(files);
        let mut source = lock(self.source.as_ref()?);
        match source.returned.pop_front() {
            Some(line) => Some(line),
            None => source.source.read_line(),
        }
    }

    /// The error from the last [`read_line`](Self::read_line) that could not
    /// open the SourceFile, if any.
    pub(crate) fn take_read_error(&self) -> AstResult<()> {
        match self.files.0.borrow_mut().read_error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Hand `lines` back to the text being read, to be read again, in order,
    /// before anything else it gives.
    pub(crate) fn return_lines(&self, lines: &[String]) {
        let mut files = self.files.0.borrow_mut();
        if files.fd_in.is_some() || self.source.is_none() {
            for line in lines.iter().rev() {
                files.returned.push_front(line.clone());
            }
        } else if let Some(source) = &self.source {
            let mut source = lock(source);
            for line in lines.iter().rev() {
                source.returned.push_front(line.clone());
            }
        }
    }

    /// C's `astRead` of a Channel: the next Object in the source, `None` at
    /// its end.
    /// `class` is the Channel's class, for messages.
    /// The lines of one Object are read (see
    /// [`crate::serial::native_stream`]), and the rest left for the next
    /// read; the warnings for items no loader reads are kept in
    /// [`warnings`](Self::warnings), or with Strict set the first is an
    /// error.
    pub(crate) fn read_native(&mut self, class: &str) -> AstResult<Option<AnyObject>> {
        self.take_read_error()?;
        self.warnings.clear();
        let skip = self.config.skip();
        let strict = self.config.strict();
        let report_level = self.config.report_level();
        let this = &*self;
        let mut next_line = || this.read_line(class);
        let mut reader = Reader::new(&mut next_line, class, strict, report_level);
        let mut returned = Vec::new();
        let result = with_registry(&this.intra, || -> AstResult<Option<AnyObject>> {
            let Some(object) = reader.read(true, skip)? else {
                return Ok(None);
            };
            let loaded = any_object_from_node(&object.node)?;
            // C loads every nested Object it reads, so one no loader read
            // is loaded here for its reports. A class the port cannot load
            // on its own, such as an Axis or a PointSet, is left.
            let mut load = |node: &Node| match any_object_from_node(node) {
                Err(e) if e.code() == ErrorCode::Lderr => Ok(()),
                other => other.map(|_| ()),
            };
            match reader.report_object(&object, &mut load) {
                Ok(()) => Ok(Some(loaded)),
                Err((e, lines)) => {
                    returned = reader.lines[lines..].to_vec();
                    Err(e.with_context(format!(
                        "Error while reading a {} from a {class}.",
                        object.node.class
                    )))
                }
            }
        });
        let warnings = std::mem::take(&mut reader.warnings);
        drop(reader);
        self.return_lines(&returned);
        self.warnings = warnings;
        // A SourceFile that could not be opened looks like an empty source
        // to the reader, so its error takes the place of the result.
        self.take_read_error()?;
        result
    }

    /// Write `line` to the SinkFile, if set, or else the sink, or else
    /// standard output.
    /// `class` names the Channel's class in an error.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Wrerr`] if the SinkFile cannot be opened.
    // C: channel.c `WriteData`. The check there that the SinkFile is not the
    // SourceFile in use tests `fd_out` inside a branch where it is NULL, so
    // it never fires, and none is made here.
    pub(crate) fn write_line(&self, class: &str, line: &str) -> AstResult<()> {
        let mut files = self.files.0.borrow_mut();
        if let Some(name) = &self.sink_file.0
            && files.fd_out.is_none()
        {
            files.open_sink_file(class, name)?;
        }
        if let Some(fd) = &mut files.fd_out {
            // C's fprintf ignores a failure to write.
            let _ = writeln!(fd, "{line}");
            return Ok(());
        }
        drop(files);
        match &self.sink {
            Some(sink) => match &mut *lock(sink) {
                SinkKind::String(s) => s.write_line(line),
                SinkKind::Boxed(b) => b.write_line(line),
            },
            None => {
                // C's printf ignores a failure to write.
                let _ = writeln!(io::stdout().lock(), "{line}");
            }
        }
        Ok(())
    }

    /// Flush any output buffered for the SinkFile.
    pub(crate) fn flush(&self) {
        if let Some(fd) = &mut self.files.0.borrow_mut().fd_out {
            let _ = fd.flush();
        }
    }

    /// The name of the SourceFile, or `None` when unset.
    pub(crate) fn source_file(&self) -> Option<&str> {
        self.source_file.0.as_deref()
    }

    /// Set the SourceFile, closing any file open for the last one.
    /// The file is opened by the next read.
    pub(crate) fn set_source_file(&mut self, name: &str) {
        let files = self.files.0.get_mut();
        files.fd_in = None;
        files.returned.clear();
        self.source_file = NotCopied(Some(name.to_string()));
    }

    /// Clear the SourceFile, closing any file open for it, so that reads
    /// return to the source, or to standard input if there is none.
    pub(crate) fn clear_source_file(&mut self) {
        let files = self.files.0.get_mut();
        files.fd_in = None;
        files.returned.clear();
        self.source_file = NotCopied(None);
    }

    /// The name of the SinkFile, or `None` when unset.
    pub(crate) fn sink_file(&self) -> Option<&str> {
        self.sink_file.0.as_deref()
    }

    /// Set the SinkFile, closing any file open for the last one.
    /// The file is created by the next write.
    pub(crate) fn set_sink_file(&mut self, name: &str) {
        self.files.0.get_mut().fd_out = None;
        self.sink_file = NotCopied(Some(name.to_string()));
    }

    /// Clear the SinkFile, closing any file open for it, so that writes
    /// return to the sink, or to standard output if there is none.
    pub(crate) fn clear_sink_file(&mut self) {
        self.files.0.get_mut().fd_out = None;
        self.sink_file = NotCopied(None);
    }

    /// The text written to the [`StringSink`] of a Channel made by
    /// [`with_string_sink`](Self::with_string_sink), or an empty string.
    pub(crate) fn into_string(self) -> String {
        match self.sink {
            Some(sink) => match &*lock(&sink) {
                SinkKind::String(s) => s.as_str().to_string(),
                SinkKind::Boxed(_) => String::new(),
            },
            None => String::new(),
        }
    }

    /// Write the Object and Channel parts of a dump: the Object's `Ident`,
    /// then the Channel attributes that are set, in the order of channel.c
    /// `Dump`, each with its marker only if something came before it.
    pub(crate) fn write_parts(&self, node: &mut Node, own_class: bool) {
        write_parts(
            node,
            &self.id,
            self.ident.as_deref(),
            None,
            &self.config,
            own_class,
        );
    }

    /// Read the Object and Channel parts of a dump, applying each Channel
    /// attribute through its setter as channel.c `astLoadChannel` does.
    pub(crate) fn read_parts(node: &Node) -> AstResult<Self> {
        let mut base = Self::default();
        (base.id, base.ident) = read_parts(node, &mut base.config);
        Ok(base)
    }
}

/// Write the Object and Channel parts of the dump of a Channel of any class
/// (see [`ChannelBase::write_parts`]). `own_class` is whether the object is
/// a plain Channel, whose part needs no `IsA` marker.
pub(crate) fn write_parts(
    node: &mut Node,
    id: &ObjectId,
    ident: Option<&str>,
    use_defs: Option<bool>,
    c: &ChannelConfig,
    own_class: bool,
) {
    crate::frames::frame_attrs::write_object_part(node, id, ident, use_defs, true);
    // channel.c `Dump`: each attribute active when set and otherwise the
    // default the class's getter gives, none of them helpful.
    node.write_int(
        "Indnt",
        c.test_indent(),
        false,
        i64::from(c.indent()),
        "Indentation increment",
    );
    node.write_int(
        "RpLev",
        c.test_report_level(),
        false,
        i64::from(c.report_level()),
        "Error reporting level",
    );
    node.write_int(
        "Skip",
        c.test_skip(),
        false,
        i64::from(c.skip()),
        if c.skip() {
            "Ignore data between Objects"
        } else {
            "No data allowed between Objects"
        },
    );
    // "insead" is C's spelling, kept so that dumps match.
    node.write_int(
        "Strict",
        c.test_strict(),
        false,
        i64::from(c.strict()),
        if c.strict() {
            "Report errors insead of warnings"
        } else {
            "Report warnings instead of errors"
        },
    );
    let full = c.full();
    node.write_int(
        "Full",
        c.test_full(),
        false,
        i64::from(full),
        match full {
            f if f < 0 => "Suppress non-essential output",
            0 => "Output standard information",
            _ => "Output maximum information",
        },
    );
    node.write_int(
        "Comm",
        c.test_comment(),
        false,
        i64::from(c.comment()),
        if c.comment() {
            "Display comments"
        } else {
            "Omit comments"
        },
    );
    if !own_class {
        node.push_isa("Channel", Some("Basic I/O Channel".to_string()));
    }
}

/// Read the Object and Channel parts of the dump of a Channel of any class
/// into `c`, applying each Channel attribute through its setter as channel.c
/// `astLoadChannel` does, and give the `ID` and `Ident` it holds.
pub(crate) fn read_parts(node: &Node, c: &mut ChannelConfig) -> (ObjectId, Option<String>) {
    let object = node.section("Object");
    let id = ObjectId::from_object_part(&object);
    let ident = object.get_string_if_set("Ident");
    let part = node.section("Channel");
    if let Some(v) = part.get_int_any_if_set("Indnt") {
        c.set_indent(v as i32);
    }
    if let Some(v) = part.get_int_any_if_set("RpLev") {
        c.set_report_level(v as i32);
    }
    if let Some(v) = part.get_int_any_if_set("Skip") {
        c.set_skip(v != 0);
    }
    if let Some(v) = part.get_int_any_if_set("Strict") {
        c.set_strict(v != 0);
    }
    if let Some(v) = part.get_int_any_if_set("Full") {
        c.set_full(v as i32);
    }
    if let Some(v) = part.get_int_any_if_set("Comm") {
        c.set_comment(v != 0);
    }
    (id, ident)
}
