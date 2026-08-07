//! `SortBy`: the order in which keys are presented.

/// The order [`crate::keymap::KeyMap::map_key`], `map_iterate` and the
/// dump present keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    /// Alphabetical, ascending. C leaves this order unspecified — it is
    /// whatever the bucket chains happen to give — but a `HashMap` has no
    /// stable equivalent, so the Rust port sorts.
    #[default]
    None,
    /// Oldest entry first.
    AgeUp,
    /// Youngest entry first.
    AgeDown,
    /// Alphabetical, ascending.
    KeyUp,
    /// Alphabetical, descending.
    KeyDown,
    /// Oldest key first, ignoring later re-puts of an existing key.
    KeyAgeUp,
    /// Youngest key first, ignoring later re-puts of an existing key.
    KeyAgeDown,
}
