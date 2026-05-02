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

    /// Look up the first item with the given key, returning its value.
    /// Returns `None` if not found or if the item is `Unset`.
    pub fn get(&self, key: &str) -> Option<&NodeValue> {
        for item in &self.items {
            if item.key == key {
                if let NodeValue::Unset(_) = &item.value {
                    return None;
                }
                return Some(&item.value);
            }
        }
        None
    }

    /// Look up an integer attribute, returning a default if absent.
    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        match self.get(key) {
            Some(NodeValue::Int(v)) => *v,
            _ => default,
        }
    }

    /// Look up a double attribute, returning a default if absent.
    pub fn get_double(&self, key: &str, default: f64) -> f64 {
        match self.get(key) {
            Some(NodeValue::Double(v)) => *v,
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
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.class)
    }
}
