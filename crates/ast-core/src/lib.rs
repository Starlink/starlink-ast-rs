#![warn(missing_docs)]

//! Starlink AST — World Coordinate Systems library.

pub mod constants;
pub mod coords;
pub mod error;
pub mod frames;
pub mod mappings;
pub mod regions;
pub(crate) mod serial;
pub use serial::{from_native_string, to_native_string, AnyObject};
