// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.
//!
//! AST describes coordinate systems and the relationships between them:
//! celestial, spectral, time and other coordinates, the Mappings that
//! convert between them, and the FITS headers and other formats that store
//! them.
//!
//! AST comes from the [Starlink Project](https://en.wikipedia.org/wiki/Starlink_Project).
//! The library and its design are described in Berry, Warren-Smith and
//! Jenness (2016), *AST: A library for modelling and manipulating coordinate
//! systems*, Astronomy and Computing 15, 33,
//! [doi:10.1016/j.ascom.2016.02.003](https://doi.org/10.1016/j.ascom.2016.02.003).
//! This port is evolving: it follows the C library closely and is checked
//! against it, and its API will change between minor versions until it
//! settles.
//!
//! **New to AST? Start with [the user guide](guide).**
//! It explains the library's concepts and how its parts fit together, with
//! worked examples, from an [overview](guide::concepts) and quick
//! [How To… recipes](guide::howto) to full chapters on Mappings, Frames,
//! FrameSets and FITS headers.
//! The documentation of each module and item below is the reference for the
//! API.
//! The [prelude] brings in the traits whose methods are called on Mappings,
//! Frames, Regions and Channels.
//!
//! ## The guide's chapters
//!
#![doc = include_str!("guide/contents.md")]

// The README's example is compiled and run with the doctests, so it cannot
// rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod channel;
pub mod constants;
pub mod coords;
pub mod error;
pub mod fitschan;
pub mod fitstable;
pub mod pixel;
#[macro_use]
pub mod frames;
pub mod call_counts;
pub mod guide;
pub mod keymap;
pub mod mappings;
pub(crate) mod math;
pub(crate) mod object_id;
pub(crate) mod pal;
pub mod prelude;
pub mod regions;
pub(crate) mod serial;
pub mod table;
pub(crate) mod units;
pub(crate) mod wcslib;
pub use channel::source_sink::{FileSink, FileSource, StringSink, StringSource};
pub use channel::{Channel, ChannelConfig, MocChan, MocFormat, NativeChannel, Sink, Source};
pub use fitschan::{AltAxes, Encoding, FitsChan};
pub use fitstable::{ColumnData, ColumnNull, FitsTable};
pub use keymap::{AstType, KeyMap, SortBy};
pub use serial::{AnyObject, from_native_string, from_native_string_with};
pub use table::Table;

/// Whether this build counts library calls (the `call-counts` feature).
/// Timing a counting build measures the counters, so benchmarks refuse it.
pub const CALL_COUNTS_ENABLED: bool = cfg!(feature = "call-counts");
