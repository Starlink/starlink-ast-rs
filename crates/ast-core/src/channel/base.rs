//! The part every Channel shares: C's Channel class part.
//!
//! A Channel's source and sink are held through shared handles, so a clone
//! reads from and writes to the same place as the original, as a copy made
//! by C's `astCopy` calls the same source and sink functions. A Channel read
//! from a dump has neither, as C's `astLoadChannel` sets them to NULL.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use super::source_sink::{StringSink, StringSource};
use super::{ChannelConfig, Sink, Source};
use crate::error::AstResult;
use crate::error::ErrorCode;
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

    /// The next line from the source, or `None` at its end or if there is
    /// none.
    pub(crate) fn read_line(&self) -> Option<String> {
        let mut source = lock(self.source.as_ref()?);
        match source.returned.pop_front() {
            Some(line) => Some(line),
            None => source.source.read_line(),
        }
    }

    /// Hand `lines` back to the source, to be read again, in order, before
    /// anything else it gives.
    pub(crate) fn return_lines(&self, lines: &[String]) {
        if let Some(source) = &self.source {
            let mut source = lock(source);
            for line in lines.iter().rev() {
                source.returned.push_front(line.clone());
            }
        }
    }

    /// C's `astRead` of a Channel: the next Object in the source, `None` at
    /// its end. `class` is the Channel's class, for messages. The lines of
    /// one Object are read (see [`crate::serial::native_stream`]), and the
    /// rest left for the next read; the warnings for items no loader reads
    /// are kept in [`warnings`](Self::warnings), or with Strict set the
    /// first is an error.
    pub(crate) fn read_native(&mut self, class: &str) -> AstResult<Option<AnyObject>> {
        self.warnings.clear();
        let skip = self.config.skip();
        let strict = self.config.strict();
        let report_level = self.config.report_level();
        let this = &*self;
        let mut next_line = || this.read_line();
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
        result
    }

    /// Pass `line` to the sink, if there is one.
    pub(crate) fn write_line(&self, line: &str) {
        if let Some(sink) = &self.sink {
            match &mut *lock(sink) {
                SinkKind::String(s) => s.write_line(line),
                SinkKind::Boxed(b) => b.write_line(line),
            }
        }
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
    c: &ChannelConfig,
    own_class: bool,
) {
    crate::frames::frame_attrs::write_object_part(node, id, ident, None);
    let before = node.items.len();
    if c.test_indent() {
        node.push_int(
            "Indnt",
            i64::from(c.indent()),
            Some("Indentation increment".to_string()),
        );
    }
    if c.test_report_level() {
        node.push_int(
            "RpLev",
            i64::from(c.report_level()),
            Some("Error reporting level".to_string()),
        );
    }
    if c.test_skip() {
        node.push_int(
            "Skip",
            i64::from(c.skip()),
            Some(
                if c.skip() {
                    "Ignore data between Objects"
                } else {
                    "No data allowed between Objects"
                }
                .to_string(),
            ),
        );
    }
    if c.test_strict() {
        // "insead" is C's spelling, kept so that dumps match.
        node.push_int(
            "Strict",
            i64::from(c.strict()),
            Some(
                if c.strict() {
                    "Report errors insead of warnings"
                } else {
                    "Report warnings instead of errors"
                }
                .to_string(),
            ),
        );
    }
    if c.test_full() {
        let full = c.full();
        node.push_int(
            "Full",
            i64::from(full),
            Some(
                match full {
                    f if f < 0 => "Suppress non-essential output",
                    0 => "Output standard information",
                    _ => "Output maximum information",
                }
                .to_string(),
            ),
        );
    }
    if c.test_comment() {
        node.push_int(
            "Comm",
            i64::from(c.comment()),
            Some(
                if c.comment() {
                    "Display comments"
                } else {
                    "Omit comments"
                }
                .to_string(),
            ),
        );
    }
    if !own_class && node.items.len() > before {
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
