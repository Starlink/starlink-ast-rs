// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The Node tree -- intermediate representation for serialization.

use std::cell::RefCell;
use std::fmt;

use crate::error::{AstError, ErrorCode};

/// A single typed value in a serialized AST object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeValue {
    /// Integer attribute.
    Int(i64),
    /// Floating-point attribute.
    Double(f64),
    /// String attribute.
    String(String),
    /// Nested object.
    Object(Node),
    /// Commented-out default -- preserves `# Key = Value` lines
    /// from C output for byte-exact round-trip.
    Unset(Box<NodeValue>),
}

/// One item in a Node's ordered attribute list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeItem {
    /// Attribute key (e.g. "Zoom", "Nin"). For IsA markers, key is "IsA".
    pub key: String,
    /// The value.
    pub value: NodeValue,
    /// The value as the C library holds it, quotes removed, for a value read
    /// from text: what C's `astRead<Type>` functions decode, and quote when
    /// they cannot. `None` for an item made in memory or holding an Object.
    pub text: Option<String>,
    /// Comment text (after `#` on the same line).
    pub comment: Option<String>,
    /// Whether a loader has read this item, which a Channel reports as
    /// unrecognized if not (see [`ReadFlag`]).
    pub read: ReadFlag,
    /// For an `Unset` value, whether C calls it helpful: one a Channel
    /// writes, as a comment, unless its Full is negative. A value that is
    /// not helpful is written only when Full is positive (channel.c `Use`).
    pub helpful: bool,
}

/// Whether a loader has read an item or object of a description read from
/// a Channel, shared by the copies [`Node::section`] makes. Every flag
/// compares equal, so it takes no part in comparing descriptions.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReadFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl ReadFlag {
    /// Record a read.
    pub fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether a read has been recorded.
    pub fn get(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl PartialEq for ReadFlag {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// A serialized AST object as a tree of named attributes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    /// AST class name (e.g. "ZoomMap", "FrameSet").
    pub class: String,
    /// Comment on the Begin line (e.g. "Compound Mapping").
    pub class_comment: Option<String>,
    /// Ordered list of items: attributes, IsA markers, and commented
    /// defaults. IsA markers use key `"IsA"` and value
    /// `NodeValue::String(class_name)`.
    pub items: Vec<NodeItem>,
    /// Whether the class writing this object as part of its own sets the
    /// Channel's Full to -1 around it unless Full is 1, as C's Frame does
    /// for its Axes (frame.c `Dump`) and CmpFrame for its two Frames
    /// (cmpframe.c `Dump`). Full applies to the whole of the object's dump,
    /// however deep it goes.
    pub full_capped: bool,
    /// Whether a loader has looked anything up in this object, shared by
    /// the copies [`Node::section`] makes.
    pub accessed: ReadFlag,
    /// The lower-cased name of the item this object was read as the value
    /// of, which a failure to load it names; `None` for a top-level object
    /// or one made in memory.
    pub item: ItemName,
}

/// The name of the item a nested object was read under. Two Nodes are equal
/// whatever their item names, as a Node read from text and one made in
/// memory describe the same object.
#[derive(Debug, Clone, Default)]
pub(crate) struct ItemName(pub Option<String>);

impl PartialEq for ItemName {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Node {
    /// Create a new empty Node with the given class name.
    pub fn new(class: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            class_comment: None,
            items: Vec::new(),
            full_capped: false,
            accessed: ReadFlag::default(),
            item: ItemName::default(),
        }
    }

    /// Add an integer attribute.
    pub fn push_int(&mut self, key: &str, value: i64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Int(value),
            text: None,
            comment: comment.into(),
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Add a double attribute.
    pub fn push_double(&mut self, key: &str, value: f64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Double(value),
            text: None,
            comment: comment.into(),
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Add a string attribute.
    pub fn push_string(&mut self, key: &str, value: &str, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::String(value.to_string()),
            text: None,
            comment: comment.into(),
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Add a nested object attribute.
    pub fn push_object(&mut self, key: &str, node: Node, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Object(node),
            text: None,
            comment: comment.into(),
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Insert a nested object attribute before the item at `index`.
    pub fn insert_object(
        &mut self,
        index: usize,
        key: &str,
        node: Node,
        comment: impl Into<Option<String>>,
    ) {
        self.items.insert(
            index,
            NodeItem {
                key: key.to_string(),
                value: NodeValue::Object(node),
                text: None,
                comment: comment.into(),
                read: ReadFlag::default(),
                helpful: true,
            },
        );
    }

    /// Add a nested object that its parent writes with Full set to -1
    /// unless it is 1 (see [`Node::full_capped`]).
    pub fn push_full_capped_object(
        &mut self,
        key: &str,
        mut node: Node,
        comment: impl Into<Option<String>>,
    ) {
        node.full_capped = true;
        self.push_object(key, node, comment);
    }

    /// Add an IsA marker.
    pub fn push_isa(&mut self, class: &str, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: "IsA".to_string(),
            value: NodeValue::String(class.to_string()),
            text: None,
            comment: comment.into(),
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Add a commented-out (unset) attribute.
    pub fn push_unset(&mut self, key: &str, value: NodeValue, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Unset(Box::new(value)),
            text: None,
            comment: comment.into(),
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Add a value as C's `astWriteInt`, `astWriteDouble`, `astWriteString`
    /// and `astWriteObject` do: active if `set`, otherwise as a comment
    /// that is written when Full is zero only if `helpful`, and always when
    /// Full is positive (channel.c `Use`). An unset Object is written as
    /// its name alone.
    pub fn write(&mut self, key: &str, set: bool, helpful: bool, value: NodeValue, comment: &str) {
        let comment = Some(comment.to_string());
        if set {
            self.items.push(NodeItem {
                key: key.to_string(),
                value,
                text: None,
                comment,
                read: ReadFlag::default(),
                helpful: true,
            });
        } else {
            self.items.push(NodeItem {
                key: key.to_string(),
                value: NodeValue::Unset(Box::new(value)),
                text: None,
                comment,
                read: ReadFlag::default(),
                helpful,
            });
        }
    }

    /// [`Node::write`] for an integer.
    pub fn write_int(&mut self, key: &str, set: bool, helpful: bool, value: i64, comment: &str) {
        self.write(key, set, helpful, NodeValue::Int(value), comment);
    }

    /// [`Node::write`] for a double.
    pub fn write_double(&mut self, key: &str, set: bool, helpful: bool, value: f64, comment: &str) {
        self.write(key, set, helpful, NodeValue::Double(value), comment);
    }

    /// [`Node::write`] for a string.
    pub fn write_string(
        &mut self,
        key: &str,
        set: bool,
        helpful: bool,
        value: &str,
        comment: &str,
    ) {
        self.write(
            key,
            set,
            helpful,
            NodeValue::String(value.to_string()),
            comment,
        );
    }

    /// Replace the value of the first item matching `key`, or push a new
    /// item if none exists. Comment is preserved on replace, omitted on push.
    /// Key comparison is case-insensitive.
    pub fn replace_or_push(&mut self, key: &str, value: NodeValue) {
        for item in &mut self.items {
            if item.key.eq_ignore_ascii_case(key) {
                item.value = value;
                return;
            }
        }
        self.items.push(NodeItem {
            key: key.to_string(),
            value,
            text: None,
            comment: None,
            read: ReadFlag::default(),
            helpful: true,
        });
    }

    /// Look up the first item with the given key, returning its value.
    /// Returns `None` if not found or if the item is `Unset`.
    ///
    /// Key comparison is **case-insensitive** (ASCII), mirroring the C AST
    /// channel reader which uses `strcasecmp`-style matching for attribute
    /// names. This allows the keyword-card FITS path (which produces
    /// all-uppercase or capitalize-first key names derived from FITS keywords)
    /// to find attributes whose canonical names in native text have mid-word
    /// uppercase letters (e.g. `MapA`, `InvB`, `SimpFI`).
    pub fn get(&self, key: &str) -> Option<&NodeValue> {
        self.accessed.set();
        for item in &self.items {
            if item.key.eq_ignore_ascii_case(key) {
                if let NodeValue::Unset(_) = &item.value {
                    return None;
                }
                item.read.set();
                return Some(&item.value);
            }
        }
        None
    }

    /// The part of this object's description that belongs to `class`, as
    /// C's `astReadClassData` gives it to that class's loader
    /// (channel.c `ReadClassData`): the items between the previous `IsA`
    /// marker and `IsA <class>`, or, for the object's own class, those after
    /// the last `IsA` marker. A part that is not there is empty, as is the
    /// part of a class whose `IsA` marker was left out because nothing
    /// preceded it: those items belong to the class named by the next
    /// marker.
    pub fn section(&self, class: &str) -> Node {
        let mut chunks: Vec<(&str, Vec<&NodeItem>)> = Vec::new();
        let mut current: Vec<&NodeItem> = Vec::new();
        for item in &self.items {
            if item.key == "IsA" {
                if let NodeValue::String(name) = &item.value {
                    chunks.push((name.as_str(), std::mem::take(&mut current)));
                }
            } else {
                current.push(item);
            }
        }
        chunks.push((self.class.as_str(), current));
        let mut node = Node::new(self.class.clone());
        node.class_comment.clone_from(&self.class_comment);
        node.accessed = self.accessed.clone();
        node.item = self.item.clone();
        if let Some((_, items)) = chunks.into_iter().find(|(name, _)| *name == class) {
            node.items = items.into_iter().cloned().collect();
        }
        node
    }

    /// Look up the first item with the given key, returning its value
    /// whether set or unset. Unwraps `Unset(inner)` to return `inner`.
    ///
    /// Key comparison is **case-insensitive** (ASCII); see [`Node::get`].
    pub fn get_any(&self, key: &str) -> Option<&NodeValue> {
        self.accessed.set();
        for item in &self.items {
            if item.key.eq_ignore_ascii_case(key) {
                item.read.set();
                if let NodeValue::Unset(inner) = &item.value {
                    return Some(inner.as_ref());
                }
                return Some(&item.value);
            }
        }
        None
    }

    /// Look up the first item's raw value with the given key, preserving the
    /// `Unset(inner)` wrapper (unlike [`Node::get_any`], which unwraps it).
    #[cfg(test)]
    pub fn get_raw(&self, key: &str) -> Option<&NodeValue> {
        for item in &self.items {
            if item.key.eq_ignore_ascii_case(key) {
                return Some(&item.value);
            }
        }
        None
    }

    /// The first item called `key` that is set, marking it read, as
    /// [`Node::get`] finds it.
    fn set_item(&self, key: &str) -> Option<&NodeItem> {
        self.accessed.set();
        let item = self
            .items
            .iter()
            .find(|i| i.key.eq_ignore_ascii_case(key))?;
        if let NodeValue::Unset(_) = &item.value {
            return None;
        }
        item.read.set();
        Some(item)
    }

    /// Look up an integer attribute (set or unset), returning None if absent.
    ///
    /// A set value read from text is decoded as C's `astReadInt` decodes it
    /// (see [`Node::get_int`]).
    pub fn get_int_any(&self, key: &str) -> Option<i64> {
        if let Some(item) = self.set_item(key) {
            return int_of(item);
        }
        match self.get_any(key) {
            Some(NodeValue::Int(v)) => Some(*v),
            Some(NodeValue::Double(v)) => Some(*v as i64),
            _ => None,
        }
    }

    /// Look up an integer attribute, returning a default if absent.
    ///
    /// A value read from text is decoded as C's `astReadInt` decodes it: a
    /// decimal integer within the range of a C `int`, with white space
    /// around it. Any other value, or an Object, records C's error for the
    /// load ([`record_read_error`]) and gives 0, as C does.
    /// A value made in memory that is a `Double` is truncated to `i64`.
    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        match self.set_item(key) {
            Some(item) => int_of(item).unwrap_or(default),
            None => default,
        }
    }

    /// Look up a 64-bit integer attribute, returning a default if absent.
    ///
    /// As [`Node::get_int`], but a value read from text may be any integer
    /// a C `int64_t` holds, as for C's `astReadInt64`.
    pub fn get_int64(&self, key: &str, default: i64) -> i64 {
        let Some(item) = self.set_item(key) else {
            return default;
        };
        match (&item.value, &item.text) {
            (NodeValue::Object(n), _) => {
                record_object_error(&item.key, &n.class, "a 64 bit integer");
                0
            }
            (_, Some(text)) => c_int64(text).unwrap_or_else(|| {
                record_value_error(&item.key, text, "a 64 bit integer");
                0
            }),
            (NodeValue::Int(v), None) => *v,
            (NodeValue::Double(v), None) => *v as i64,
            _ => default,
        }
    }

    /// Look up a double attribute, returning a default if absent.
    ///
    /// A value read from text is decoded as C's `astReadDouble` decodes it:
    /// `<bad>` is [`BAD_VALUE`](crate::constants::BAD_VALUE), anything else
    /// must be a whole number as `sscanf`'s `%lf` reads it (hexadecimal
    /// included) and finite. Any other value, or an Object, records C's
    /// error for the load and gives 0.
    /// A value made in memory that is an `Int` is promoted to `f64`.
    pub fn get_double(&self, key: &str, default: f64) -> f64 {
        let Some(item) = self.set_item(key) else {
            return default;
        };
        match (&item.value, &item.text) {
            (NodeValue::Object(n), _) => {
                record_object_error(
                    &item.key,
                    &n.class,
                    "a double precision floating point number",
                );
                0.0
            }
            (_, Some(text)) => match c_double(text) {
                Ok(v) => v,
                Err(DoubleError::Unreadable) => {
                    record_value_error(&item.key, text, "a double precision floating point number");
                    0.0
                }
                Err(DoubleError::NotFinite) => {
                    record_read_error(format!(
                        "Illegal double precision floating point value \"{text}\" read for \"{}\".",
                        item.key.to_ascii_lowercase()
                    ));
                    0.0
                }
            },
            (NodeValue::Double(v), None) => *v,
            (NodeValue::Int(v), None) => *v as f64,
            _ => default,
        }
    }

    /// Look up a string attribute, returning a default if absent.
    ///
    /// A value read from text is its text, as C's `astReadString` gives it,
    /// whether or not it looks like a number. An Object records C's error
    /// for the load and gives the default.
    pub fn get_string(&self, key: &str, default: &str) -> String {
        self.string_of(key).unwrap_or_else(|| default.to_string())
    }

    fn string_of(&self, key: &str) -> Option<String> {
        let item = self.set_item(key)?;
        match (&item.value, &item.text) {
            (NodeValue::Object(n), _) => {
                record_object_error(&item.key, &n.class, "a string");
                None
            }
            (_, Some(text)) => Some(text.clone()),
            (NodeValue::String(v), None) => Some(v.clone()),
            _ => None,
        }
    }

    /// Look up a nested object attribute.
    ///
    /// A value read from text that is not an Object records C's
    /// `astReadObject` error for the load and gives `None`.
    pub fn get_object(&self, key: &str) -> Option<&Node> {
        let item = self.set_item(key)?;
        match (&item.value, &item.text) {
            (NodeValue::Object(n), _) => Some(n),
            (_, Some(text)) => {
                record_value_error(&item.key, text, "an Object");
                None
            }
            _ => None,
        }
    }

    /// Look up an integer attribute, returning `None` if absent or `Unset`.
    ///
    /// Unlike [`Node::get_int_any`], a commented-out default does not count
    /// as present — this is for callers that must tell "the card was
    /// written" apart from "the card was written but commented out".
    /// A value read from text is decoded as by [`Node::get_int`].
    pub fn get_int_any_if_set(&self, key: &str) -> Option<i64> {
        int_of(self.set_item(key)?)
    }

    /// Record that the loader has read the items called `key`, which C's
    /// loader reads but whose values the port takes from elsewhere, so that
    /// a Channel does not report them as unrecognized.
    pub fn mark_read(&self, key: &str) {
        self.accessed.set();
        for item in &self.items {
            if item.key.eq_ignore_ascii_case(key) {
                item.read.set();
            }
        }
    }

    /// Look up a string attribute, returning `None` if absent or `Unset`.
    pub fn get_string_if_set(&self, key: &str) -> Option<String> {
        self.string_of(key)
    }
}

/// The value of a set integer `item`, decoded as C's `astReadInt` decodes
/// it; `Some(0)` with C's error recorded for a value it cannot read.
fn int_of(item: &NodeItem) -> Option<i64> {
    match (&item.value, &item.text) {
        (NodeValue::Object(n), _) => {
            record_object_error(&item.key, &n.class, "an integer");
            Some(0)
        }
        (_, Some(text)) => Some(
            c_int64(text)
                .filter(|v| i32::try_from(*v).is_ok())
                .unwrap_or_else(|| {
                    record_value_error(&item.key, text, "an integer");
                    0
                }),
        ),
        (NodeValue::Int(v), None) => Some(*v),
        (NodeValue::Double(v), None) => Some(*v as i64),
        _ => None,
    }
}

/// C's `astChanStr2Int64`: a decimal integer, with an optional sign and
/// white space around it, that an `int64_t` holds.
fn c_int64(text: &str) -> Option<i64> {
    let t = text.trim_matches(|c: char| c.is_ascii_whitespace());
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.strip_prefix('+').unwrap_or(t).parse().ok()
}

/// Why C's `astReadDouble` refuses a value.
enum DoubleError {
    /// `sscanf`'s `%lf` does not read the whole value.
    Unreadable,
    /// It reads an infinity or a NaN.
    NotFinite,
}

/// A value as C's `astReadDouble` reads it: `<bad>`, or the whole text read
/// by `sscanf`'s `%lf`, which takes decimal and hexadecimal numbers,
/// infinities and NaNs, the last two refused.
fn c_double(text: &str) -> Result<f64, DoubleError> {
    let t = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if t == "<bad>" {
        return Ok(crate::constants::BAD_VALUE);
    }
    let v = t
        .parse::<f64>()
        .ok()
        .or_else(|| hex_double(t))
        .ok_or(DoubleError::Unreadable)?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(DoubleError::NotFinite)
    }
}

/// A C hexadecimal floating point number, `0x` digits with an optional
/// fraction and binary exponent (`0x1.8p3`), with an optional sign.
fn hex_double(t: &str) -> Option<f64> {
    let (negative, t) = match t.as_bytes().first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))?;
    let (mantissa, exponent) = match t.find(['p', 'P']) {
        Some(i) => (&t[..i], Some(&t[i + 1..])),
        None => (t, None),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut value = 0.0_f64;
    for c in int_part.chars() {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_part.chars() {
        value += f64::from(c.to_digit(16)?) * scale;
        scale /= 16.0;
    }
    if let Some(e) = exponent {
        let e = c_int64(e).filter(|_| !e.starts_with(char::is_whitespace))?;
        value *= crate::math::powi(
            2.0,
            i32::try_from(e).unwrap_or(if e < 0 { i32::MIN } else { i32::MAX }),
        );
    }
    Some(if negative { -value } else { value })
}

/// What the current load has recorded: the first error a typed read met,
/// and the class of the Channel reading, which C's messages name.
#[derive(Default)]
struct LoadState {
    error: Option<AstError>,
    channel: Option<String>,
}

thread_local! {
    static LOAD: RefCell<LoadState> = RefCell::default();
}

/// Run `f` with `class` as the class of the Channel reading, which the
/// errors of typed reads name (`astRead(<class>)`). The previous class is
/// restored afterwards.
pub(crate) fn with_read_channel<T>(class: &str, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            LOAD.with(|l| l.borrow_mut().channel = previous);
        }
    }
    let previous = LOAD.with(|l| l.borrow_mut().channel.replace(class.to_string()));
    let _restore = Restore(previous);
    f()
}

/// Record `message` as an AST__BADIN error of the current load, unless one
/// is recorded already: as in C, where a failed read sets the status and
/// later reads do nothing, the first error is the one reported.
pub(crate) fn record_read_error(message: String) {
    let channel = LOAD.with(|l| l.borrow().channel.clone());
    let channel = channel.as_deref().unwrap_or("Channel");
    record_load_error(AstError::new(
        ErrorCode::Badin,
        format!("astRead({channel}): {message}"),
    ));
}

/// Record `error` as the error of the current load, unless one is recorded
/// already, for a check C's loader makes that the port's loader, which
/// cannot fail at that point, makes without stopping: the load then
/// reports it ahead of anything the loader goes on to do.
pub(crate) fn record_load_error(error: AstError) {
    LOAD.with(|l| {
        let mut l = l.borrow_mut();
        if l.error.is_none() {
            l.error = Some(error);
        }
    });
}

fn record_value_error(key: &str, text: &str, what: &str) {
    record_read_error(format!(
        "The value \"{} = {text}\" cannot be read as {what}.",
        key.to_ascii_lowercase()
    ));
}

fn record_object_error(key: &str, class: &str, what: &str) {
    record_read_error(format!(
        "The Object \"{} = <{class}>\" cannot be read as {what}.",
        key.to_ascii_lowercase()
    ));
}

/// Take the error recorded by typed reads, leaving none.
pub(crate) fn take_read_error() -> Option<AstError> {
    LOAD.with(|l| l.borrow_mut().error.take())
}

/// Put back an error taken with [`take_read_error`].
pub(crate) fn restore_read_error(error: Option<AstError>) {
    LOAD.with(|l| l.borrow_mut().error = error);
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.class)
    }
}
