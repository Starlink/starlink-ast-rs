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

chapter!(cargo);
