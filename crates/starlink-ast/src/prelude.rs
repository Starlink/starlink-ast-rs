// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The traits whose methods are called on Mappings, Frames, Regions and
//! Channels, for one `use`.
//!
//! Rust lets a trait's methods be called only where the trait is in scope, so
//! transforming points through a FrameSet needs [`Mapping`], reading an axis
//! value of a SkyFrame needs [`Frame`], converting between Frames needs
//! [`FrameSearch`], testing a point against a Region needs [`Region`], and
//! reading an Object from a NativeChannel needs [`Channel`].
//! `use starlink_ast::prelude::*;` brings every such trait in.
//! The concrete types are named from their modules as before; the prelude
//! re-exports no types.
//!
//! ```
//! use starlink_ast::prelude::*;
//! use starlink_ast::frames::{SkyFrame, SkySystem};
//!
//! let mut fk5 = SkyFrame::new(SkySystem::Fk5);
//! let mut galactic = SkyFrame::new(SkySystem::Galactic);
//! let to_galactic = fk5.convert_to(&mut galactic, &[])?.expect("conversion exists");
//! let (ra, dec) = (fk5.unformat(1, "12:51:26.3")?.0, fk5.unformat(2, "27:07:42")?.0);
//! let (l, b) = to_galactic.tran2(&[ra], &[dec], true)?;
//! assert!((b[0].to_degrees() - 90.0).abs() < 1e-3);
//! # let _ = l;
//! # Ok::<(), starlink_ast::error::AstError>(())
//! ```

pub use crate::channel::Channel;
pub use crate::frames::{Frame, FrameSearch};
pub use crate::mappings::Mapping;
pub use crate::regions::Region;
