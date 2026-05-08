# Step 8a: Channel Trait and NativeChannel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Channel trait (I/O abstraction for reading/writing AST objects), NativeChannel (first concrete implementation using Native text format), Source/Sink traits, and built-in I/O implementations (string, file).

**Architecture:** Channel is a trait with a small surface (read/write/warnings/config). NativeChannel wraps the existing native_read/native_write infrastructure with streaming I/O via Source/Sink traits. The existing `to_native_string`/`from_native_string` become thin wrappers around NativeChannel. The native renderer is updated to respect ChannelConfig (comment/full/indent).

**Tech Stack:** Rust 2024 edition. Standard library I/O (std::fs, std::io::BufReader/BufWriter). No new dependencies.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/channel/mod.rs` | Channel trait, ChannelConfig, Source/Sink traits, re-exports |
| `crates/ast-core/src/channel/source_sink.rs` | StringSource, StringSink, FileSource, FileSink |
| `crates/ast-core/src/channel/native.rs` | NativeChannel implementation |
| `crates/ast-core/src/serial/native_write.rs` | Update renderer to accept config options |
| `crates/ast-core/src/serial/mod.rs` | Refactor to_native_string/from_native_string as wrappers |
| `crates/ast-core/src/lib.rs` | Add `pub mod channel;` and re-exports |
| `crates/ast-core/tests/test_channel.rs` | Channel tests (port of testchannel.c) |

---

### Task 1: Channel Trait and ChannelConfig

**Files:**
- Create: `crates/ast-core/src/channel/mod.rs`
- Modify: `crates/ast-core/src/lib.rs`

Define the Channel trait, ChannelConfig struct, and Source/Sink traits.

- [ ] **Step 1: Create channel/mod.rs**

```rust
//! Channel I/O abstraction for reading and writing AST objects.

pub mod native;
pub mod source_sink;

use crate::error::AstResult;
use crate::serial::AnyObject;

/// Configuration controlling Channel I/O behaviour.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Include comments in output (default: true).
    pub comment: bool,
    /// Output verbosity: -1=minimal, 0=normal, 1=verbose (default: 0).
    pub full: i8,
    /// Skip non-object data between objects (default: false).
    pub skip: bool,
    /// Report unexpected input data as errors (default: false).
    pub strict: bool,
    /// Indentation increment per nesting level (default: 3).
    pub indent: usize,
    /// Error reporting level (default: 0).
    pub report_level: i32,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            comment: true,
            full: 0,
            skip: false,
            strict: false,
            indent: 3,
            report_level: 0,
        }
    }
}

/// Provides lines of text for Channel reading.
pub trait Source {
    /// Read the next line. Returns None at end-of-input.
    fn read_line(&mut self) -> Option<String>;
}

/// Receives lines of text from Channel writing.
pub trait Sink {
    /// Write a single line of text.
    fn write_line(&mut self, line: &str);
}

/// I/O abstraction for reading and writing AST objects.
///
/// Concrete implementations (NativeChannel, FitsChan, YamlChan, etc.)
/// handle format-specific serialisation. The trait provides a uniform
/// interface for all formats.
pub trait Channel {
    /// Read the next AST object from this channel.
    fn read(&mut self) -> AstResult<Option<AnyObject>>;

    /// Write an AST object to this channel.
    fn write(&mut self, obj: &AnyObject) -> AstResult<()>;

    /// Accumulated warnings from the last read/write.
    fn warnings(&self) -> &[String];

    /// Access channel configuration.
    fn config(&self) -> &ChannelConfig;

    /// Mutably access channel configuration.
    fn config_mut(&mut self) -> &mut ChannelConfig;
}
```

- [ ] **Step 2: Add to lib.rs**

Add `pub mod channel;` and re-export key types:
```rust
pub mod channel;
pub use channel::{Channel, ChannelConfig, NativeChannel, Source, Sink};
```

- [ ] **Step 3: Create stub files**

Create empty `source_sink.rs` and `native.rs` with module doc comments.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p ast-core`

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/channel/ crates/ast-core/src/lib.rs
git commit -m "Add Channel trait, ChannelConfig, Source/Sink traits"
```

---

### Task 2: Source/Sink Built-in Implementations

**Files:**
- Modify: `crates/ast-core/src/channel/source_sink.rs`

Implement StringSource, StringSink, FileSource, FileSink.

- [ ] **Step 1: Implement StringSource and StringSink**

```rust
use super::{Source, Sink};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::fs::File;
use std::path::Path;

/// In-memory source that reads lines from a string.
pub struct StringSource {
    lines: Vec<String>,
    pos: usize,
}

impl StringSource {
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.lines().map(|l| l.to_string()).collect(),
            pos: 0,
        }
    }
}

impl Source for StringSource {
    fn read_line(&mut self) -> Option<String> {
        if self.pos < self.lines.len() {
            let line = self.lines[self.pos].clone();
            self.pos += 1;
            Some(line)
        } else {
            None
        }
    }
}

/// In-memory sink that accumulates lines into a String.
pub struct StringSink {
    buffer: String,
}

impl StringSink {
    pub fn new() -> Self { Self { buffer: String::new() } }
    pub fn into_string(self) -> String { self.buffer }
    pub fn as_str(&self) -> &str { &self.buffer }
}

impl Sink for StringSink {
    fn write_line(&mut self, line: &str) {
        self.buffer.push_str(line);
        self.buffer.push('\n');
    }
}
```

- [ ] **Step 2: Implement FileSource and FileSink**

```rust
/// File-based source (buffered line reader).
pub struct FileSource {
    reader: BufReader<File>,
}

impl FileSource {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self { reader: BufReader::new(file) })
    }
}

impl Source for FileSource {
    fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => {
                // Strip trailing newline
                if line.ends_with('\n') { line.pop(); }
                if line.ends_with('\r') { line.pop(); }
                Some(line)
            }
            Err(_) => None,
        }
    }
}

/// File-based sink (buffered writer).
pub struct FileSink {
    writer: BufWriter<File>,
}

impl FileSink {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self { writer: BufWriter::new(file) })
    }
}

impl Sink for FileSink {
    fn write_line(&mut self, line: &str) {
        let _ = writeln!(self.writer, "{}", line);
    }
}

impl Drop for FileSink {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
git add crates/ast-core/src/channel/source_sink.rs
git commit -m "Add StringSource, StringSink, FileSource, FileSink"
```

---

### Task 3: NativeChannel Implementation

**Files:**
- Modify: `crates/ast-core/src/channel/native.rs`

NativeChannel wraps our existing native_read/native_write with
Source/Sink streaming I/O.

- [ ] **Step 1: Implement NativeChannel struct and constructors**

```rust
use super::{Channel, ChannelConfig, Source, Sink};
use super::source_sink::{StringSource, StringSink, FileSource, FileSink};
use crate::error::AstResult;
use crate::serial::AnyObject;
use std::path::Path;

/// Channel for AST's Native text format.
pub struct NativeChannel {
    config: ChannelConfig,
    source: Option<Box<dyn Source>>,
    sink: Option<Box<dyn Sink>>,
    warnings: Vec<String>,
    /// Holds the StringSink for retrieval after writing.
    string_sink_buffer: Option<StringSink>,
}

impl NativeChannel {
    pub fn new() -> Self { ... }
    pub fn from_string(text: &str) -> Self { ... }
    pub fn with_string_sink() -> Self { ... }
    pub fn from_file(path: &Path) -> std::io::Result<Self> { ... }
    pub fn to_file(path: &Path) -> std::io::Result<Self> { ... }
    pub fn into_string(self) -> String { ... }
    pub fn set_source(&mut self, source: Box<dyn Source>) { ... }
    pub fn set_sink(&mut self, sink: Box<dyn Sink>) { ... }
}
```

- [ ] **Step 2: Implement Channel trait for NativeChannel**

`read()`:
1. Collect all lines from source into a string.
2. Call `crate::serial::native_read::parse_native(&text)`.
3. Call `crate::serial::any_object_from_node(&node)`.
4. Return the AnyObject.

`write()`:
1. Call `to_node` on the AnyObject (dispatch via match).
2. Call `crate::serial::native_write::render_native(&node)`
   with config options.
3. Split into lines, emit via sink.

- [ ] **Step 3: Verify and commit**

```bash
git add crates/ast-core/src/channel/native.rs
git commit -m "Add NativeChannel: Channel impl for Native text format"
```

---

### Task 4: Config-Aware Native Renderer

**Files:**
- Modify: `crates/ast-core/src/serial/native_write.rs`

Update the renderer to accept config options (comment, full, indent).

- [ ] **Step 1: Add config parameter to render_native**

Add a new function or parameter:

```rust
/// Render options for the Native text format.
pub(crate) struct RenderOptions {
    pub comment: bool,
    pub full: i8,
    pub indent: usize,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { comment: true, full: 0, indent: 3 }
    }
}

/// Render a Node tree with options.
pub(crate) fn render_native_with_options(
    node: &Node, options: &RenderOptions
) -> AstResult<String>
```

- `comment = false`: skip the `\t# comment` suffix on each line
- `full = -1`: skip NodeItems that are Unset
- `full = 1`: (no change from default — write everything)
- `indent`: use this value instead of hardcoded spacing

Keep existing `render_native` as a wrapper with default options.

- [ ] **Step 2: Verify existing tests still pass**

All serialisation tests should pass unchanged since
`render_native` still uses defaults.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/src/serial/native_write.rs
git commit -m "Add config-aware render_native_with_options"
```

---

### Task 5: Refactor to_native_string/from_native_string

**Files:**
- Modify: `crates/ast-core/src/serial/mod.rs`

Make the existing public API thin wrappers around NativeChannel.

- [ ] **Step 1: Refactor to_native_string**

```rust
pub fn to_native_string(obj: &AnyObject) -> AstResult<String> {
    let mut ch = crate::channel::NativeChannel::with_string_sink();
    ch.config_mut().comment = false;
    ch.config_mut().full = -1;
    ch.write(obj)?;
    Ok(ch.into_string())
}
```

- [ ] **Step 2: Refactor from_native_string**

```rust
pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    let mut ch = crate::channel::NativeChannel::from_string(s);
    ch.read()?.ok_or_else(|| AstError::new(ErrorCode::Rderr, "no object found"))
}
```

- [ ] **Step 3: Verify ALL existing tests pass**

This is critical — the refactoring must be transparent.
Run: `cargo test -p ast-core`

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/serial/mod.rs
git commit -m "Refactor to_native_string/from_native_string as NativeChannel wrappers"
```

---

### Task 6: Tests (port testchannel.c)

**Files:**
- Create: `crates/ast-core/tests/test_channel.rs`

- [ ] **Step 1: Write Channel tests**

Tests covering:
- Round-trip via StringSource/StringSink (write → read → equal)
- Round-trip via temp file (FileSource/FileSink)
- Config: comment=false produces output without `#`
- Config: full=-1 produces shorter output than full=0
- Config: indent=6 produces wider indentation
- Error: read with no source set
- Multiple object types: ZoomMap, SkyFrame, FrameSet
- NativeChannel::from_string + read matches from_native_string

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_channel`

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_channel.rs
git commit -m "Add Channel tests (port of testchannel.c)"
```

---

### Task 7: Final Verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all -- --check`

- [ ] **Step 3: Full test suite**

Run: `cargo test -p ast-core`

- [ ] **Step 4: Commit fixes if any**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. Channel trait defined with read/write/warnings/config
6. NativeChannel reads and writes Native text format
7. StringSource/StringSink work for in-memory I/O
8. FileSource/FileSink work for file I/O
9. Config attributes affect output (comment, full, indent)
10. to_native_string/from_native_string still work (backward compatible)
11. All existing serialisation tests pass unchanged
12. `git status` — clean
