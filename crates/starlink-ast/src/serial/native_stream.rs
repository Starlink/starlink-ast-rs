// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A Channel's reader: one Object at a time from a source of text lines, as
//! channel.c `Read`, `ReadClassData`, `GetNextData`, `InputTextItem` and
//! `ClearValues` read it.
//!
//! C builds an Object while it reads. Each class loader, from `Object` down
//! to the Object's own class, asks `ReadClassData` for its own part of the
//! description: the items up to the next `IsA` (or the `End`). Those items
//! are a "values list" from which the loader takes the ones it knows; when
//! the next class asks, or the Object is finished, `ClearValues` reports
//! each item left as not recognized, a warning or, with `Strict` set, an
//! error. A nested Object is read, in full, when its item is met.
//!
//! The port reads in two passes. The first is C's reading, line for line:
//! it consumes exactly the lines C consumes, raises the errors C raises
//! while reading, and records each values list with the number of lines
//! read when it was cleared. The second loads the description built and
//! then reports the items no loader looked up (see [`ReadFlag`]), in the
//! order C's `ClearValues` reports them: a nested Object's reports when it
//! is read, then each values list in turn, its items in the order of C's
//! hash table. With `Strict` set the first is an error, and the lines read
//! after the values list it was in are handed back to the source, where C
//! stops reading.
//!
//! Two things are not known to the port. When a read fails partway, C's
//! loaders for the classes before the failure have already taken their
//! items, so the items they left are reported; the port, which loads only
//! a whole description, reports only the items of values lists no loader
//! could see. And when a loader fails, C has read only up to that class's
//! part, where the port has read the whole Object.
//!
//! [`ReadFlag`]: super::node::ReadFlag

use super::native_read::{parse_scalar_value, split_comment, try_parse_commented_default};
use super::node::{NodeItem, ReadFlag};
use super::{Node, NodeValue};
use crate::error::{AstError, AstResult, ErrorCode};

/// The parent of each class C can load, whose loader each class's loader
/// calls first (`astLoad<Parent>` in each `astLoad<Class>`), `Object` having
/// none. This is the parent `astMAKE_ISA` names, except for ChebyMap,
/// which chebymap.c declares a Mapping although its loader is PolyMap's
/// and its dumps hold an `IsA PolyMap`.
const PARENTS: &[(&str, &str)] = &[
    ("Axis", "Object"),
    ("Box", "Region"),
    ("Channel", "Object"),
    ("ChebyMap", "PolyMap"),
    ("Circle", "Region"),
    ("CmpFrame", "Frame"),
    ("CmpMap", "Mapping"),
    ("CmpRegion", "Region"),
    ("DSBSpecFrame", "SpecFrame"),
    ("DssMap", "Mapping"),
    ("Ellipse", "Region"),
    ("FitsChan", "Channel"),
    ("FitsTable", "Table"),
    ("FluxFrame", "Frame"),
    ("Frame", "Mapping"),
    ("FrameSet", "Frame"),
    ("GrismMap", "Mapping"),
    ("Interval", "Region"),
    ("IntraMap", "Mapping"),
    ("KeyMap", "Object"),
    ("LutMap", "Mapping"),
    ("Mapping", "Object"),
    ("MathMap", "Mapping"),
    ("MatrixMap", "Mapping"),
    ("Moc", "Region"),
    ("MocChan", "Channel"),
    ("NormMap", "Mapping"),
    ("NullRegion", "Region"),
    ("PcdMap", "Mapping"),
    ("PermMap", "Mapping"),
    ("Plot", "FrameSet"),
    ("Plot3D", "Plot"),
    ("PointList", "Region"),
    ("PointSet", "Object"),
    ("Polygon", "Region"),
    ("PolyMap", "Mapping"),
    ("Prism", "Region"),
    ("RateMap", "Mapping"),
    ("Region", "Frame"),
    ("SelectorMap", "Mapping"),
    ("ShiftMap", "Mapping"),
    ("SkyAxis", "Axis"),
    ("SkyFrame", "Frame"),
    ("SlaMap", "Mapping"),
    ("SpecFluxFrame", "CmpFrame"),
    ("SpecFrame", "Frame"),
    ("SpecMap", "Mapping"),
    ("SphMap", "Mapping"),
    ("SplineMap", "Mapping"),
    ("Stc", "Region"),
    ("StcCatalogEntryLocation", "Stc"),
    ("StcObsDataLocation", "Stc"),
    ("StcResourceProfile", "Stc"),
    ("StcsChan", "Channel"),
    ("StcSearchLocation", "Stc"),
    ("SwitchMap", "Mapping"),
    ("Table", "KeyMap"),
    ("TimeFrame", "Frame"),
    ("TimeMap", "Mapping"),
    ("TranMap", "Mapping"),
    ("UnitMap", "Mapping"),
    ("UnitNormMap", "Mapping"),
    ("WcsMap", "Mapping"),
    ("WinMap", "Mapping"),
    ("XmlChan", "Channel"),
    ("XphMap", "Mapping"),
    ("YamlChan", "Channel"),
    ("ZoomMap", "Mapping"),
];

/// The classes whose loaders build an Object of class `class`, in the order
/// they read their parts of its description (`Object` first), or `None`
/// for a class C cannot load (loader.c `astGetLoader`).
fn class_chain(class: &str) -> Option<Vec<&'static str>> {
    let mut chain = Vec::new();
    let mut name: &'static str = if class == "Object" {
        "Object"
    } else {
        PARENTS.iter().find(|(c, _)| *c == class)?.0
    };
    loop {
        chain.push(name);
        match PARENTS.iter().find(|(c, _)| *c == name) {
            Some((_, parent)) => name = parent,
            None => break,
        }
    }
    chain.reverse();
    Some(chain)
}

/// The bucket of C's values hash table that holds an item called `name`
/// (channel.c `HashFun`, the djb2 hash, over `HASHMAP_SIZE` 128 buckets).
fn bucket(name: &str) -> u64 {
    let mut hash: u64 = 5381;
    for c in name.bytes() {
        hash = (hash << 5).wrapping_add(hash).wrapping_add(u64::from(c));
    }
    hash & 127
}

/// C's `InputTextItem`: the line without any comment (text after the first
/// `#` outside quotes) or leading and trailing white space.
fn clean(line: &str) -> &str {
    let mut quoted = false;
    let mut end = line.len();
    for (i, b) in line.bytes().enumerate() {
        if b == b'"' {
            quoted = !quoted;
        }
        if b == b'#' && !quoted {
            end = i;
            break;
        }
    }
    line[..end].trim_matches(|c: char| c.is_ascii_whitespace())
}

/// C's `Unquote`: remove the quotes of each quoted string, turning a doubled
/// quote within one into a single quote.
fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut quoted = false;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' {
            out.push(b[i]);
        } else if !quoted {
            quoted = true;
        } else if b.get(i + 1) == Some(&b'"') {
            out.push(b'"');
            i += 1;
        } else {
            quoted = false;
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_space(b: u8) -> bool {
    b.is_ascii_whitespace()
}

/// The end of the white space starting at `i`.
fn skip_space(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    i
}

/// Match `" %n%*8[^ \t=]%n = %n"`: a name of one to eight characters other
/// than a space, tab or `=`, then `=`. Gives the end of the name and the
/// start of what follows the `=` and any white space after it.
fn match_name_equals(b: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < b.len() && !matches!(b[i], b' ' | b'\t' | b'=') {
        i += 1;
    }
    if i == 0 || i > 8 {
        return None;
    }
    let name_end = i;
    let i = skip_space(b, i);
    if b.get(i) != Some(&b'=') {
        return None;
    }
    Some((name_end, skip_space(b, i + 1)))
}

/// Match `" %*1[Kk]...%n%*s%n %n"` for `keyword` (`begin`, `isa` or
/// `end`), ignoring case, followed by one word and nothing else. Gives the
/// word.
fn match_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let b = line.as_bytes();
    let n = keyword.len();
    if b.len() < n || !line[..n].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let start = skip_space(b, n);
    let mut end = start;
    while end < b.len() && !is_space(b[end]) {
        end += 1;
    }
    (end > start && skip_space(b, end) == b.len()).then(|| &line[start..end])
}

/// One data item, as C's `GetNextData` gives it.
struct Datum {
    /// The name, in lower case: `begin`, `isa`, `end` or an item's name.
    name: String,
    /// The class of a `begin`, `isa` or `end`, or an item's value with its
    /// quotes removed; `None` for an item holding an Object.
    val: Option<String>,
    /// The item's name as written.
    key: String,
    /// The item's value as written, quotes included.
    raw: String,
    /// The comment after the item.
    comment: Option<String>,
}

/// An item of a values list: its index in the Object's description, its
/// name in lower case, and C's report of it if no loader reads it.
type Value = (usize, String, String);

/// A values list that a loader could look in, as `ClearValues` finds it.
struct Values {
    items: Vec<Value>,
    /// The number of lines read when it was cleared.
    lines: usize,
}

/// What happened while an Object was read, in order.
enum Event {
    /// An Object held by item `item` was read.
    Nested { item: usize, events: Vec<Event> },
    /// A values list was cleared.
    Clear(Values),
}

/// An Object's description and what happened while it was read.
pub(crate) struct ReadObject {
    pub(crate) node: Node,
    events: Vec<Event>,
}

/// The reader of one Channel read.
pub(crate) struct Reader<'a> {
    next_line: &'a mut dyn FnMut() -> Option<String>,
    /// The class of the Channel, for messages.
    class: &'a str,
    /// Every line read, so that those after an error can be handed back.
    pub(crate) lines: Vec<String>,
    /// C's `astreadclassdata_msg`: whether the failure of a nested Object
    /// has been reported.
    nested_msg: bool,
    /// The Channel's `Strict`.
    strict: bool,
    /// The Channel's `ReportLevel`.
    report_level: i32,
    /// The warnings made, as C's `astAddWarning` stores them.
    pub(crate) warnings: Vec<String>,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(
        next_line: &'a mut dyn FnMut() -> Option<String>,
        class: &'a str,
        strict: bool,
        report_level: i32,
    ) -> Self {
        Self {
            next_line,
            class,
            lines: Vec::new(),
            nested_msg: false,
            strict,
            report_level,
            warnings: Vec::new(),
        }
    }

    /// C's `astAddWarning` at level 1: store `msg` as a warning, or with
    /// Strict set raise it as an error, if ReportLevel is at least 1.
    fn report(&mut self, msg: &str) -> AstResult<()> {
        if self.report_level < 1 {
            Ok(())
        } else if self.strict {
            Err(self.error(ErrorCode::Badin, msg.to_string()))
        } else {
            self.warnings.push(msg.to_string());
            Ok(())
        }
    }

    /// Report each item of a values list that no loader read, in the order
    /// of C's hash table (`ClearValues`).
    fn report_unread(
        &mut self,
        mut items: Vec<Value>,
        unread: impl Fn(usize) -> bool,
    ) -> AstResult<()> {
        items.sort_by_key(|(_, name, _)| bucket(name));
        for (index, _, msg) in items {
            if unread(index) {
                self.report(&msg)?;
            }
        }
        Ok(())
    }

    /// Report the items of `object` and the Objects in it that no loader
    /// read, in the order C reports them, once the description has been
    /// loaded. `load` loads a nested Object that no loader read, as C loads
    /// every nested Object it reads. An error carries the number of lines
    /// C had read when it raised it.
    pub(crate) fn report_object(
        &mut self,
        object: &ReadObject,
        load: &mut dyn FnMut(&Node) -> AstResult<()>,
    ) -> Result<(), (AstError, usize)> {
        self.report_events(&object.node, &object.events, load)
    }

    fn report_events(
        &mut self,
        node: &Node,
        events: &[Event],
        load: &mut dyn FnMut(&Node) -> AstResult<()>,
    ) -> Result<(), (AstError, usize)> {
        for event in events {
            match event {
                Event::Nested { item, events } => {
                    let NodeValue::Object(child) = &node.items[*item].value else {
                        unreachable!("a nested Object's item holds it");
                    };
                    if !child.accessed.get() {
                        load(child).map_err(|e| (e, self.lines.len()))?;
                    }
                    self.report_events(child, events, load)?;
                }
                Event::Clear(values) => {
                    let lines = values.lines;
                    self.report_unread(values.items.clone(), |i| !node.items[i].read.get())
                        .map_err(|e| (e, lines))?;
                }
            }
        }
        Ok(())
    }

    fn error(&self, code: ErrorCode, message: String) -> AstError {
        AstError::new(code, format!("astRead({}): {message}", self.class))
    }

    /// C's `GetNextData`, giving also the commented defaults met on the way,
    /// which C ignores and the port keeps in the description.
    fn next_datum(&mut self, skip: bool) -> AstResult<(Option<Datum>, Vec<NodeItem>)> {
        let mut defaults = Vec::new();
        loop {
            let Some(raw) = (self.next_line)() else {
                return Ok((None, defaults));
            };
            self.lines.push(raw);
            let raw = self.lines.last().unwrap().as_str();
            let line = clean(raw);
            if line.is_empty() {
                if let Some(after) = raw.trim_start().strip_prefix('#')
                    && let Some(item) = try_parse_commented_default(after)
                {
                    defaults.push(item);
                }
                continue;
            }
            let comment = split_comment(raw).1.map(str::to_string);
            let b = line.as_bytes();
            let datum = |name: String, val: Option<String>, key: &str, raw: &str| Datum {
                name,
                val,
                key: key.to_string(),
                raw: raw.to_string(),
                comment: comment.clone(),
            };
            if !skip && let Some((name_end, value_start)) = match_name_equals(b) {
                let key = &line[..name_end];
                let name = key.to_ascii_lowercase();
                let value = &line[value_start..];
                return Ok((
                    Some(if value.is_empty() {
                        datum(name, None, key, "")
                    } else {
                        datum(name, Some(unquote(value)), key, value)
                    }),
                    defaults,
                ));
            }
            if let Some(class) = match_keyword(line, "begin") {
                return Ok((
                    Some(datum("begin".into(), Some(class.into()), "", "")),
                    defaults,
                ));
            }
            if !skip {
                for keyword in ["isa", "end"] {
                    if let Some(class) = match_keyword(line, keyword) {
                        return Ok((
                            Some(datum(keyword.into(), Some(class.into()), "", "")),
                            defaults,
                        ));
                    }
                }
                let shown = if line.len() > 50 {
                    format!("{}...", &line[..50])
                } else {
                    line.to_string()
                };
                return Err(self.error(
                    ErrorCode::Badin,
                    format!("Cannot interpret the input data: \"{shown}\"."),
                ));
            }
        }
    }

    /// C's `Read`: the next Object, `None` at the end of the data. `top` is
    /// whether it is a top-level Object, before which other data are
    /// skipped when `skip` is set.
    pub(crate) fn read(&mut self, top: bool, skip: bool) -> AstResult<Option<ReadObject>> {
        let (datum, _) = self.next_datum(top && skip)?;
        let Some(datum) = datum else {
            if top {
                return Ok(None);
            }
            return Err(self.error(
                ErrorCode::Eochn,
                "End of input encountered while trying to read an AST Object.".into(),
            ));
        };
        if datum.name != "begin" {
            return Err(self.error(
                ErrorCode::Badin,
                "Missing \"Begin\" when expecting an Object.".into(),
            ));
        }
        let class = datum.val.unwrap_or_default();
        let Some(chain) = class_chain(&class) else {
            return Err(AstError::new(
                ErrorCode::Ocluk,
                format!("astGetLoader: Object of unknown class \"{class}\" cannot be loaded."),
            ));
        };
        let mut node = Node::new(class.clone());
        node.class_comment = datum.comment;
        let result = self.read_classes(&mut node, &class, &chain);
        let result = result.map(|events| ReadObject { node, events });
        if top {
            result.map(Some).map_err(|e| {
                e.with_context(format!(
                    "Error while reading a {class} from a {}.",
                    self.class
                ))
            })
        } else {
            result.map(Some)
        }
    }

    /// The `ReadClassData` of each class in `chain`, then `Read`'s closing
    /// `ClearValues` and check for the `End`.
    fn read_classes(
        &mut self,
        node: &mut Node,
        class: &str,
        chain: &[&str],
    ) -> AstResult<Vec<Event>> {
        let mut events = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let mut values_class: Option<String> = None;
        let mut values_ok = false;
        let mut end_of_object = false;
        for &want in chain {
            if values_ok {
                events.push(Event::Clear(Values {
                    items: std::mem::take(&mut values),
                    lines: self.lines.len(),
                }));
                values_class = None;
            }
            if let Some(have) = &values_class {
                values_ok = have == want;
            } else if end_of_object {
                return Err(self
                    .error(
                        ErrorCode::Lderr,
                        format!("Invalid attempt to read further {want} data following an end of {class}."),
                    )
                    .with_context("Perhaps the wrong class loader has been invoked?"));
            } else {
                loop {
                    let (datum, defaults) = self.next_datum(false)?;
                    node.items.extend(defaults);
                    let Some(datum) = datum else {
                        return Err(self.error(
                            ErrorCode::Eochn,
                            format!("Unexpected end of input (missing end of {class})."),
                        ));
                    };
                    match datum.name.as_str() {
                        "isa" => {
                            let isa = datum.val.unwrap_or_default();
                            node.push_isa(&isa, datum.comment);
                            values_ok = isa == want;
                            values_class = Some(isa);
                            break;
                        }
                        "end" => {
                            end_of_object = true;
                            let end = datum.val.unwrap_or_default();
                            if end != class {
                                return Err(self
                                    .error(
                                        ErrorCode::Badin,
                                        "Bad class structure in input data.".into(),
                                    )
                                    .with_context(format!(
                                        "End of {end} read when expecting end of {class}."
                                    )));
                            }
                            values_ok = want == end;
                            values_class = Some(end);
                            break;
                        }
                        _ => {}
                    }
                    let index = node.items.len();
                    match datum.val {
                        Some(val) => {
                            let msg = format!(
                                "The value \"{} = {val}\" was not recognised as valid input.",
                                datum.name
                            );
                            values.push((index, datum.name.clone(), msg));
                            node.items.push(NodeItem {
                                key: datum.key,
                                value: parse_scalar_value(&datum.raw),
                                comment: datum.comment,
                                read: ReadFlag::default(),
                                helpful: true,
                            });
                        }
                        None => {
                            self.nested_msg = false;
                            let nested = match self.read(false, false) {
                                Ok(nested) => nested.expect("a nested read gives an Object"),
                                Err(e) => {
                                    if self.nested_msg {
                                        return Err(e);
                                    }
                                    self.nested_msg = true;
                                    return Err(e.with_context(format!(
                                        "Failed to read the \"{}\" Object value.",
                                        datum.name
                                    )));
                                }
                            };
                            let msg = format!(
                                "The Object \"{} = <{}>\" was not recognised as valid input.",
                                datum.name, nested.node.class
                            );
                            values.push((index, datum.name.clone(), msg));
                            events.push(Event::Nested {
                                item: index,
                                events: nested.events,
                            });
                            node.push_object(&datum.key, nested.node, datum.comment);
                        }
                    }
                }
            }
        }
        // `Read`'s own `ClearValues`, after the loader. A values list no
        // loader could look in is reported now, the read having failed.
        match values_class {
            Some(values_class) if !values_ok => {
                if self.strict {
                    return Err(self
                        .error(
                            ErrorCode::Badin,
                            "Invalid class structure in input data.".into(),
                        )
                        .with_context(format!(
                            "Class \"{values_class}\" is invalid or out of order within a {class}."
                        )));
                }
                self.report_unread(values, |_| true)?;
            }
            _ => events.push(Event::Clear(Values {
                items: values,
                lines: self.lines.len(),
            })),
        }
        if !end_of_object {
            return Err(self.error(
                ErrorCode::Badin,
                format!("Unexpected end of input (missing end of {class})."),
            ));
        }
        Ok(events)
    }
}
