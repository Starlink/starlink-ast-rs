//! Error types for the AST library.

use std::fmt;

include!(concat!(env!("OUT_DIR"), "/error_codes.rs"));

/// An error produced by the AST library.
#[derive(Debug, Clone)]
pub struct AstError {
    code: ErrorCode,
    message: String,
}

impl AstError {
    /// Create a new AST error with the given code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Return the error code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Return the error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AstError {}

/// A Result type alias for AST operations.
pub type AstResult<T> = Result<T, AstError>;
