//! The Frame trait and associated types.

mod basic;
mod cmpframe;
mod dsbspecframe;
mod fluxframe;
mod frameset;
pub(crate) mod sky_convert;
pub(crate) mod skyaxis;
mod skyframe;
mod specfluxframe;
mod specframe;
mod timeframe;
pub use basic::{BasicFrame, BasicFrameBuilder};
pub use cmpframe::CmpFrame;
pub use dsbspecframe::{DSBSpecFrame, SideBand};
pub use fluxframe::{FluxFrame, FluxSystem};
pub use frameset::{FrameRef, FrameSet};
pub use sky_convert::{SkyRefIs, SkySystem};
pub use skyframe::SkyFrame;
pub use specfluxframe::SpecFluxFrame;
pub use specframe::{SpecFrame, SpecSystem, StdOfRest};
pub use timeframe::{TimeFrame, TimeScale, TimeSystem};

use std::fmt;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{AsAny, DynMapping};

/// An owned, dynamically-dispatched Frame.
pub type DynFrame = Box<dyn Frame>;

/// A coordinate frame defining the meaning of coordinate values.
///
/// All concrete frame classes implement this trait. Frames are `Send`
/// but not `Sync`.
pub trait Frame: AsAny + Send + fmt::Debug {
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

    /// Find a mapping to convert coordinates from this frame to `to`.
    ///
    /// `domains` is a priority-ordered list of domain names to try when
    /// searching for an alignment path. Returns a [`FrameSet`] connecting
    /// `self` (base) to `to` (current) if conversion is possible, or
    /// `None` if the frames are incompatible.
    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>>;

    /// Observer geodetic latitude in radians.
    fn obs_lat(&self) -> Option<f64> {
        None
    }

    /// Observer geodetic longitude in radians.
    fn obs_lon(&self) -> Option<f64> {
        None
    }

    /// Observer altitude in metres.
    fn obs_alt(&self) -> Option<f64> {
        None
    }

    /// TAI-UTC in seconds.
    fn dtai(&self) -> Option<f64> {
        None
    }

    /// UT1-UTC in seconds.
    fn dut1(&self) -> Option<f64> {
        None
    }

    /// Coordinate system name (e.g., "FK5", "GALACTIC"), if applicable.
    fn system_string(&self) -> Option<&str> {
        None
    }

    /// Permute the axes of this frame.
    ///
    /// `perm` is a 1-based permutation array of length `naxes`. After the
    /// call, external axis `i` corresponds to what was previously axis
    /// `perm[i-1]`. For example, `perm = &[2, 1]` swaps two axes.
    ///
    /// The default implementation validates the permutation and returns Ok
    /// for 1-axis frames (where the only valid permutation is `[1]`).
    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        validate_permutation(perm, self.naxes())?;
        Ok(())
    }

    /// Test whether this frame is equal to another.
    ///
    /// Uses AST's standard floating-point tolerance for numeric comparisons.
    fn equal(&self, other: &dyn Frame) -> bool;
}

/// Validate that `perm` is a valid 1-based permutation of length `naxes`.
pub fn validate_permutation(perm: &[usize], naxes: usize) -> AstResult<()> {
    if perm.len() != naxes {
        return Err(AstError::new(
            ErrorCode::Prmin,
            format!(
                "permutation array has {} elements but frame has {} axes",
                perm.len(),
                naxes
            ),
        ));
    }
    let mut seen = vec![false; naxes];
    for (i, &p) in perm.iter().enumerate() {
        if p < 1 || p > naxes {
            return Err(AstError::new(
                ErrorCode::Prmin,
                format!(
                    "permutation element {} (at position {}) is out of range 1..={}",
                    p,
                    i + 1,
                    naxes
                ),
            ));
        }
        if seen[p - 1] {
            return Err(AstError::new(
                ErrorCode::Prmin,
                format!("axis {} appears more than once in permutation", p),
            ));
        }
        seen[p - 1] = true;
    }
    Ok(())
}
