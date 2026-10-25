// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Coordinate array types.

mod pointlist;
mod points;
mod pointset;

pub(crate) use points::PointShape;

pub use pointlist::PointList;
pub use points::{INLINE_AXES, PointBuf, PointMut, Points, PointsMut, WritePoint};
pub use pointset::PointSet;
