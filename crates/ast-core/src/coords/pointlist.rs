//! Frame-aware point collection.

use std::fmt;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame};

use super::PointSet;

/// Frame-aware point collection.
///
/// Wraps a [`PointSet`] with a [`Frame`] that gives meaning to the coordinate
/// values. Will implement `Region` (and thus `Frame + Mapping`) once
/// BasicFrame and the integrity mechanism are available (Step 3+).
pub struct PointList {
    frame: DynFrame,
    points: PointSet,
}

impl PointList {
    /// Create a new PointList.
    ///
    /// The PointSet's `ncoord` must equal the frame's `naxes`.
    pub fn new(frame: DynFrame, points: PointSet) -> AstResult<Self> {
        if points.ncoord() != frame.naxes() {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!(
                    "PointSet has {} coordinates but frame has {} axes",
                    points.ncoord(),
                    frame.naxes()
                ),
            ));
        }
        Ok(Self { frame, points })
    }

    /// Access the frame.
    pub fn frame(&self) -> &dyn Frame {
        &*self.frame
    }

    /// Access the points.
    pub fn points(&self) -> &PointSet {
        &self.points
    }
}

impl fmt::Debug for PointList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointList")
            .field("naxes", &self.frame.naxes())
            .field("npoint", &self.points.npoint())
            .finish()
    }
}
