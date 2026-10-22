//! The Node tree -- intermediate representation for serialization.

use std::fmt;

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
        }
    }

    /// Add an integer attribute.
    pub fn push_int(&mut self, key: &str, value: i64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Int(value),
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
                comment,
                read: ReadFlag::default(),
                helpful: true,
            });
        } else {
            self.items.push(NodeItem {
                key: key.to_string(),
                value: NodeValue::Unset(Box::new(value)),
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

    /// Look up an integer attribute (set or unset), returning None if absent.
    pub fn get_int_any(&self, key: &str) -> Option<i64> {
        match self.get_any(key) {
            Some(NodeValue::Int(v)) => Some(*v),
            Some(NodeValue::Double(v)) => Some(*v as i64),
            _ => None,
        }
    }

    /// Look up an integer attribute, returning a default if absent.
    ///
    /// If the stored value is a `Double`, it is truncated to `i64`.
    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        match self.get(key) {
            Some(NodeValue::Int(v)) => *v,
            Some(NodeValue::Double(v)) => *v as i64,
            _ => default,
        }
    }

    /// Look up a double attribute, returning a default if absent.
    ///
    /// If the stored value is an `Int`, it is promoted to `f64`.
    pub fn get_double(&self, key: &str, default: f64) -> f64 {
        match self.get(key) {
            Some(NodeValue::Double(v)) => *v,
            Some(NodeValue::Int(v)) => *v as f64,
            _ => default,
        }
    }

    /// Look up a string attribute, returning a default if absent.
    pub fn get_string(&self, key: &str, default: &str) -> String {
        match self.get(key) {
            Some(NodeValue::String(v)) => v.clone(),
            _ => default.to_string(),
        }
    }

    /// Look up a nested object attribute.
    pub fn get_object(&self, key: &str) -> Option<&Node> {
        match self.get(key) {
            Some(NodeValue::Object(n)) => Some(n),
            _ => None,
        }
    }

    /// Look up an integer attribute, returning `None` if absent or `Unset`.
    ///
    /// Unlike [`Node::get_int_any`], a commented-out default does not count
    /// as present — this is for callers that must tell "the card was
    /// written" apart from "the card was written but commented out".
    pub fn get_int_any_if_set(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(NodeValue::Int(v)) => Some(*v),
            Some(NodeValue::Double(v)) => Some(*v as i64),
            _ => None,
        }
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
        match self.get(key) {
            Some(NodeValue::String(v)) => Some(v.clone()),
            _ => None,
        }
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.class)
    }
}
