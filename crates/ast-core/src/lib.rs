#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

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
pub mod keymap;
pub mod mappings;
pub(crate) mod math;
pub(crate) mod object_id;
pub(crate) mod pal;
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
pub use serial::{AnyObject, from_native_string, from_native_string_with, to_native_string};
pub use table::Table;

/// Whether this build counts library calls (the `call-counts` feature).
/// Timing a counting build measures the counters, so benchmarks refuse it.
pub const CALL_COUNTS_ENABLED: bool = cfg!(feature = "call-counts");
