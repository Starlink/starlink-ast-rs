//! The Frame trait and associated types.

mod basic;
mod cmpframe;
pub use basic::{BasicFrame, BasicFrameBuilder};
pub use cmpframe::CmpFrame;

use crate::error::AstResult;
use crate::mappings::DynMapping;

/// An owned, dynamically-dispatched Frame.
pub type DynFrame = Box<dyn Frame>;

/// A coordinate frame defining the meaning of coordinate values.
///
/// All concrete frame classes implement this trait. Frames are `Send`
/// but not `Sync`.
pub trait Frame: Send {
    /// Number of axes in this frame.
    fn naxes(&self) -> usize;

    /// Format a coordinate value for display.
    ///
    /// `axis` is 1-based.
    fn format(&self, axis: usize, value: f64) -> AstResult<String>;

    /// Parse a formatted coordinate string back to a numeric value.
    ///
    /// `axis` is 1-based. Returns `(value, characters_consumed)`.
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)>;

    /// Geodesic distance between two points in this frame.
    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64>;

    /// Angle at point `b` in the triangle a-b-c.
    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64>;

    /// Move along the geodesic from `from` toward `to` by distance `dist`.
    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>>;

    /// Normalise coordinates to their canonical range.
    fn norm(&self, coords: &mut [f64]) -> AstResult<()>;

    /// Deep-clone this frame for integrity delta computation.
    ///
    /// Used by FrameSet and Region integrity: a snapshot is taken before
    /// mutation, then `map_from` computes the correction mapping.
    fn snapshot(&self) -> DynFrame;

    /// Compute the mapping from a previous state of this frame to the current state.
    ///
    /// After mutating a frame attribute, this returns the transformation that
    /// maps coordinates in the old frame to coordinates in the new frame.
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;
}
