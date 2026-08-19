//! Error types for the AST library.

use std::fmt;
use std::panic::Location;

include!(concat!(env!("OUT_DIR"), "/error_codes.rs"));

/// An error produced by the AST library.
#[derive(Debug, Clone)]
pub struct AstError {
    code: ErrorCode,
    message: String,
    context: Vec<String>,
    source_location: Option<&'static Location<'static>>,
}

impl AstError {
    /// Create a new AST error with the given code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let err = Self {
            code,
            message: message.into(),
            context: Vec::new(),
            source_location: None,
        };
        err.trace();
        err
    }

    /// Create a new AST error, capturing the caller's source location.
    #[track_caller]
    pub fn new_tracked(code: ErrorCode, message: impl Into<String>) -> Self {
        let err = Self {
            code,
            message: message.into(),
            context: Vec::new(),
            source_location: Some(Location::caller()),
        };
        err.trace();
        err
    }

    /// Emit this error as a `tracing::error!` event.
    ///
    /// Called once per constructed error, from `new` and `new_tracked`.
    /// [`AstError::with_context`] does not re-emit: it decorates an error that
    /// has already been reported, and emitting again would log one failure
    /// once per frame it propagates through.
    ///
    /// Compiles to nothing without the `tracing` feature.
    #[inline]
    fn trace(&self) {
        #[cfg(feature = "tracing")]
        tracing::error!(
            code = ?self.code,
            code.raw = self.code.as_raw(),
            code.description = %self.code,
            location = self
                .source_location
                .map(|l| l.to_string())
                .unwrap_or_default(),
            "{}",
            self.message,
        );
    }

    /// Return the error code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Return the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return the context stack.
    pub fn context(&self) -> &[String] {
        &self.context
    }

    /// Return the source location where this error was created, if tracked.
    pub fn source_location(&self) -> Option<&'static Location<'static>> {
        self.source_location
    }

    /// Add context to this error for propagation up call stacks.
    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context.push(ctx.into());
        self
    }
}

impl fmt::Display for AstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        for ctx in &self.context {
            write!(f, "\n  context: {ctx}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AstError {}

/// A Result type alias for AST operations.
pub type AstResult<T> = Result<T, AstError>;
