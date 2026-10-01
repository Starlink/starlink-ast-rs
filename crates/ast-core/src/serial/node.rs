//! The Node tree -- intermediate representation for serialisation.

use std::fmt;

/// A single typed value in a serialised AST object.
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
}

/// A serialised AST object as a tree of named attributes.
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
}

impl Node {
    /// Create a new empty Node with the given class name.
    pub fn new(class: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            class_comment: None,
            items: Vec::new(),
        }
    }

    /// Add an integer attribute.
    pub fn push_int(&mut self, key: &str, value: i64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Int(value),
            comment: comment.into(),
        });
    }

    /// Add a double attribute.
    pub fn push_double(&mut self, key: &str, value: f64, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Double(value),
            comment: comment.into(),
        });
    }

    /// Add a string attribute.
    pub fn push_string(&mut self, key: &str, value: &str, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::String(value.to_string()),
            comment: comment.into(),
        });
    }

    /// Add a nested object attribute.
    pub fn push_object(&mut self, key: &str, node: Node, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Object(node),
            comment: comment.into(),
        });
    }

    /// Add an IsA marker.
    pub fn push_isa(&mut self, class: &str, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: "IsA".to_string(),
            value: NodeValue::String(class.to_string()),
            comment: comment.into(),
        });
    }

    /// Add a commented-out (unset) attribute.
    pub fn push_unset(&mut self, key: &str, value: NodeValue, comment: impl Into<Option<String>>) {
        self.items.push(NodeItem {
            key: key.to_string(),
            value: NodeValue::Unset(Box::new(value)),
            comment: comment.into(),
        });
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
        for item in &self.items {
            if item.key.eq_ignore_ascii_case(key) {
                if let NodeValue::Unset(_) = &item.value {
                    return None;
                }
                return Some(&item.value);
            }
        }
        None
    }

    /// Remove every `Unset` item from this node and from the objects nested
    /// inside it.
    ///
    /// This is C's `Full = -1`, which a class sets on the Channel around a
    /// nested write and restores afterwards (`cmpframe.c:10485-10500`). A
    /// Channel attribute is in force for the whole of the nested object's
    /// dump, however deep it goes, so the removal recurses.
    pub fn strip_unset(&mut self) {
        self.items
            .retain(|item| !matches!(&item.value, NodeValue::Unset(_)));
        for item in &mut self.items {
            if let NodeValue::Object(child) = &mut item.value {
                child.strip_unset();
            }
        }
    }

    /// Look up the first item with the given key, returning its value
    /// whether set or unset. Unwraps `Unset(inner)` to return `inner`.
    ///
    /// Key comparison is **case-insensitive** (ASCII); see [`Node::get`].
    pub fn get_any(&self, key: &str) -> Option<&NodeValue> {
        for item in &self.items {
            if item.key.eq_ignore_ascii_case(key) {
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
    /// Use when forwarding a value to another node so its set/unset status is
    /// not silently promoted to "set".
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
