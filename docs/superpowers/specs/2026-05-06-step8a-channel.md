# Step 8a: Basic Channel — Design Spec

## Summary

Implement the Channel trait and NativeChannel — the I/O abstraction
layer for reading/writing AST objects. The Channel trait defines
the interface that all format-specific implementations (FitsChan,
YamlChan, etc.) will implement. NativeChannel is the first concrete
implementation, wrapping our existing Native text parser/renderer
with streaming Source/Sink I/O, file support, and output
configuration attributes.

## Deliverables

1. `Channel` trait (public) — read, write, warnings, config.
2. `ChannelConfig` struct — Comment, Full, Skip, Strict, Indent,
   ReportLevel.
3. `Source` and `Sink` traits (public) — line-oriented I/O.
4. Built-in Source/Sink implementations: StringSource, StringSink,
   FileSource, FileSink.
5. `NativeChannel` struct — Channel impl using Native text format.
6. Refactor `to_native_string`/`from_native_string` as thin wrappers
   around NativeChannel.
7. Tests including C's `testchannel.c` port.

## Module Structure

```
crates/ast-core/src/channel/
├── mod.rs          # Channel trait, ChannelConfig, Source/Sink traits
├── native.rs       # NativeChannel implementation
├── source_sink.rs  # StringSource, StringSink, FileSource, FileSink
```

## Channel Trait

```rust
/// I/O abstraction for reading and writing AST objects.
pub trait Channel {
    /// Read the next AST object from this channel.
    ///
    /// Returns `None` at end-of-input.
    fn read(&mut self) -> AstResult<Option<AnyObject>>;

    /// Write an AST object to this channel.
    fn write(&mut self, obj: &AnyObject) -> AstResult<()>;

    /// Accumulated warnings from the last read/write operation.
    fn warnings(&self) -> &[String];

    /// Access channel configuration.
    fn config(&self) -> &ChannelConfig;

    /// Mutably access channel configuration.
    fn config_mut(&mut self) -> &mut ChannelConfig;
}
```

## ChannelConfig

```rust
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
```

Defaults match the C library: comment=true, full=0, skip=false,
strict=false, indent=3, report_level=0.

## Source and Sink Traits

```rust
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
```

## Built-in Source/Sink Implementations

```rust
/// Reads lines from an in-memory string.
pub struct StringSource { /* splits on \n, yields one at a time */ }

/// Accumulates written lines into a String.
pub struct StringSink { /* buffer: String */ }

/// Reads lines from a file (buffered).
pub struct FileSource { /* BufReader<File> */ }

/// Writes lines to a file (buffered).
pub struct FileSink { /* BufWriter<File> */ }
```

Constructors:
- `StringSource::new(text: &str)`
- `StringSink::new()`
- `StringSink::into_string(self) -> String`
- `FileSource::open(path: &Path) -> io::Result<Self>`
- `FileSink::create(path: &Path) -> io::Result<Self>`

## NativeChannel

```rust
/// Channel implementation for AST's Native text format.
pub struct NativeChannel {
    config: ChannelConfig,
    source: Option<Box<dyn Source>>,
    sink: Option<Box<dyn Sink>>,
    warnings: Vec<String>,
}
```

### Constructors

- `NativeChannel::new()` — empty, set source/sink later
- `NativeChannel::from_string(text: &str)` — string source for reading
- `NativeChannel::with_string_sink()` — string sink for writing
- `NativeChannel::from_file(path: &Path) -> io::Result<Self>`
- `NativeChannel::to_file(path: &Path) -> io::Result<Self>`

### read() Implementation

1. Collect all lines from source into a single string.
2. Call `native_read::parse_native(&text)` to get a Node.
3. Call `any_object_from_node(&node)` to reconstruct the object.
4. Return the object (or None if source is empty/exhausted).

For multiple objects: the parser finds the first `Begin`/`End`
block. To read multiple objects, the source retains its position
and subsequent `read()` calls find the next block. (Initially:
support single-object reads. Multi-object can be added later.)

### write() Implementation

1. Call the object's Serializable `to_node()` (via the existing
   dispatch in `serial/mod.rs`).
2. Call `native_write::render_native(&node)` with config-aware
   formatting:
   - If `config.comment == false`: suppress comment text
   - If `config.full == -1`: omit Unset (commented) attributes
   - If `config.full == 1`: include all attributes
   - Use `config.indent` for nesting level
3. Split the rendered text into lines and emit via sink.

### Refactoring Existing API

The existing public functions become wrappers:

```rust
pub fn to_native_string(obj: &AnyObject) -> AstResult<String> {
    let mut ch = NativeChannel::with_string_sink();
    ch.config_mut().comment = false;
    ch.config_mut().full = -1;
    ch.write(obj)?;
    Ok(ch.into_string())
}

pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    let mut ch = NativeChannel::from_string(s);
    ch.read()?.ok_or_else(|| AstError::new(ErrorCode::Rderr, "no object found"))
}
```

Note: `to_native_string` uses comment=false, full=-1 to match
the C `astToString` behavior (minimal output). Users who want
verbose output use NativeChannel directly with different config.

## Config Effect on Native Output

| Config | Effect |
|--------|--------|
| `comment = false` | No `# comment` text after attributes |
| `comment = true` | Include descriptive comments (default) |
| `full = -1` | Omit Unset (commented-out default) attributes |
| `full = 0` | Include Unset attributes as comments |
| `full = 1` | Include all possible attributes |
| `indent = N` | N spaces per nesting level (default 3) |
| `skip = true` | Skip non-Begin lines before object |

The native_write renderer needs to accept these options. Currently
it always writes comments and Unset values. The renderer will be
updated to respect a config parameter.

## Testing Strategy

**Round-trip tests:**
- Write to StringSink → read from StringSource → equal
- Write to temp file → read from file → equal
- For each object type: verify Channel round-trip

**Config tests:**
- Write with comment=false: no `#` in output
- Write with full=-1: no Unset lines
- Write with indent=6: verify wider indentation
- Write with full=1 vs full=-1: more verbose output

**Port testchannel.c:**
- File-based I/O round-trip with SkyFrame
- Error when reading with no source
- Multiple writes to same sink

**Backward compatibility:**
- All existing tests pass unchanged (to_native_string/from_native_string
  still work identically)

## What This Step Does NOT Include

- FitsChan (Step 8b — completely different model, FITS header cards)
- YamlChan (Step 8c — YAML format via libyaml)
- XmlChan, StcsChan, MocChan (later steps)
- KeyMap / warnings as KeyMap (use Vec<String> for now)
- IntraTransform registration (deferred)
- Multi-object streaming read (single object per read for now)
