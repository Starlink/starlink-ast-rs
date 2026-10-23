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
//! 1. [Introduction](intro)
//! 2. [Using AST from Cargo](cargo)

pub mod intro {
    #![doc = include_str!("intro.md")]
}

pub mod cargo {
    #![doc = include_str!("cargo.md")]
}
