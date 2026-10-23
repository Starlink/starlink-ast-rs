//! # The AST user guide
//!
//! This guide explains the concepts of the AST library and how its parts fit
//! together, with worked examples.
//! The documentation of each type, trait and function describes that item in
//! full; the guide links to it wherever it names one.
//!
//! The source of this guide's text, and of each chapter's attribution, is
//! recorded in `README.md` beside these files.
//!
//! ## Contents
//!
//! - [Introduction](intro)
//! - [Overview of AST Concepts](concepts)
//! - [How To…](howto)
//! - [An AST Object Primer](objects)
//! - [Inter-Relating Coordinate Systems (Mappings)](mappings)
//! - [Compound Mappings (CmpMaps)](cmpmaps)
//! - [Representing Coordinate Systems (Frames)](frames)
//! - [Celestial Coordinate Systems (SkyFrames)](skyframes)
//! - [Spectral Coordinate Systems (SpecFrames)](specframes)
//! - [Time Systems (TimeFrames)](timeframes)
//! - [Using AST from Cargo](cargo)

/// Declares a chapter module whose documentation is `<name>.md`, as the build
/// script writes it with its figures inlined.
macro_rules! chapter {
    ($name:ident) => {
        #[doc = include_str!(concat!(env!("OUT_DIR"), "/guide/", stringify!($name), ".md"))]
        pub mod $name {}
    };
}

chapter!(intro);
chapter!(concepts);
chapter!(howto);
chapter!(objects);
chapter!(mappings);
chapter!(cmpmaps);
chapter!(frames);
chapter!(skyframes);
chapter!(specframes);
chapter!(timeframes);

chapter!(cargo);
