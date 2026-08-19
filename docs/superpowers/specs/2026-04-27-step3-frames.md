# Step 3: BasicFrame and CmpFrame — Design Spec

## Summary

Implement `BasicFrame` (generic N-axis Cartesian frame) and `CmpFrame`
(compound frame combining two child frames). These are the first concrete
`Frame` implementations, providing Euclidean geometry, per-axis attributes
with the 1-based get/set/test/clear pattern, and builder construction.

## Deliverables

1. `BasicFrame` with Euclidean distance/angle/offset/norm.
2. Per-axis attributes: label, unit, symbol, format, bottom, top.
3. Frame-level attributes: title, domain, epoch.
4. `BasicFrameBuilder` for construction.
5. `CmpFrame` combining two child frames (binary tree, concatenated axes).
6. `Frame` trait impl for both classes.
7. Tests equivalent to C's `testframe.c` and `testcmpframe.c`.

## Module Structure

```
crates/ast-core/src/frames/
├── mod.rs           # trait + DynFrame + re-exports
├── basic.rs         # BasicFrame + BasicFrameBuilder
└── cmpframe.rs      # CmpFrame
```

## BasicFrame (src/frames/basic.rs)

### Structure

```rust
#[derive(Debug, Clone)]
pub struct BasicFrame {
    naxes: usize,
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>,
    labels: Vec<Option<String>>,
    units: Vec<Option<String>>,
    symbols: Vec<Option<String>>,
    formats: Vec<Option<String>>,
    bottoms: Vec<Option<f64>>,
    tops: Vec<Option<f64>>,
}
```

Each per-axis attribute is stored as `Vec<Option<T>>` of length `naxes`.
`None` means "use default"; `Some(v)` means explicitly set.

### Constructor (Builder)

```rust
impl BasicFrame {
    pub fn builder(naxes: usize) -> BasicFrameBuilder;
}

pub struct BasicFrameBuilder { /* same fields as BasicFrame */ }

impl BasicFrameBuilder {
    pub fn title(mut self, t: impl Into<String>) -> Self;
    pub fn domain(mut self, d: impl Into<String>) -> Self;
    pub fn epoch(mut self, e: f64) -> Self;
    pub fn label(mut self, axis: usize, s: impl Into<String>) -> Self;
    pub fn unit(mut self, axis: usize, s: impl Into<String>) -> Self;
    pub fn symbol(mut self, axis: usize, s: impl Into<String>) -> Self;
    pub fn format_str(mut self, axis: usize, s: impl Into<String>) -> Self;
    pub fn bottom(mut self, axis: usize, v: f64) -> Self;
    pub fn top(mut self, axis: usize, v: f64) -> Self;
    pub fn build(self) -> AstResult<BasicFrame>;
}
```

Builder methods for axis-indexed attributes use 1-based indexing.
`build()` validates that naxes > 0.

### Frame-level Attributes (non-indexed)

```rust
impl BasicFrame {
    pub fn title(&self) -> &str;            // default: ""
    pub fn set_title(&mut self, t: impl Into<String>);
    pub fn test_title(&self) -> bool;
    pub fn clear_title(&mut self);

    pub fn domain(&self) -> &str;           // default: ""
    pub fn set_domain(&mut self, d: impl Into<String>);
    pub fn test_domain(&self) -> bool;
    pub fn clear_domain(&mut self);

    pub fn epoch(&self) -> Option<f64>;     // default: None (unset)
    pub fn set_epoch(&mut self, e: f64);
    pub fn test_epoch(&self) -> bool;
    pub fn clear_epoch(&mut self);
}
```

### Per-axis Attributes (1-based indexing)

Each follows the four-method pattern. Axis validation: `axis == 0` or
`axis > naxes` returns `Err(AstError::new(ErrorCode::Axiin, ...))`.

```rust
impl BasicFrame {
    pub fn label(&self, axis: usize) -> AstResult<&str>;
    pub fn set_label(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()>;
    pub fn test_label(&self, axis: usize) -> AstResult<bool>;
    pub fn clear_label(&mut self, axis: usize) -> AstResult<()>;
    // default: "Axis 1", "Axis 2", etc.

    pub fn unit(&self, axis: usize) -> AstResult<&str>;
    pub fn set_unit(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()>;
    pub fn test_unit(&self, axis: usize) -> AstResult<bool>;
    pub fn clear_unit(&mut self, axis: usize) -> AstResult<()>;
    // default: ""

    pub fn symbol(&self, axis: usize) -> AstResult<&str>;
    pub fn set_symbol(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()>;
    pub fn test_symbol(&self, axis: usize) -> AstResult<bool>;
    pub fn clear_symbol(&mut self, axis: usize) -> AstResult<()>;
    // default: same as label

    pub fn format_str(&self, axis: usize) -> AstResult<&str>;
    pub fn set_format_str(&mut self, axis: usize, s: impl Into<String>) -> AstResult<()>;
    pub fn test_format_str(&self, axis: usize) -> AstResult<bool>;
    pub fn clear_format_str(&mut self, axis: usize) -> AstResult<()>;
    // default: "%g" equivalent

    pub fn bottom(&self, axis: usize) -> AstResult<f64>;
    pub fn set_bottom(&mut self, axis: usize, v: f64) -> AstResult<()>;
    pub fn test_bottom(&self, axis: usize) -> AstResult<bool>;
    pub fn clear_bottom(&mut self, axis: usize) -> AstResult<()>;
    // default: -f64::MAX (i.e. BAD_VALUE sentinel for "no bound")

    pub fn top(&self, axis: usize) -> AstResult<f64>;
    pub fn set_top(&mut self, axis: usize, v: f64) -> AstResult<()>;
    pub fn test_top(&self, axis: usize) -> AstResult<bool>;
    pub fn clear_top(&mut self, axis: usize) -> AstResult<()>;
    // default: f64::MAX
}
```

### Frame Trait Implementation (Euclidean Geometry)

```rust
impl Frame for BasicFrame {
    fn naxes(&self) -> usize { self.naxes }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        // Use format_str if set, otherwise default float formatting
        // Respects BAD_VALUE: formats as "<bad>" or similar
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        // Parse leading f64 from text, return (value, chars_consumed)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        // Euclidean: sqrt(sum((a[i]-b[i])^2)) over all axes
        // BAD_VALUE in either point returns BAD_VALUE
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        // Angle at vertex b: acos(dot(ba, bc) / (|ba| * |bc|))
        // Returns angle in radians
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        // Step along line from→to by distance dist
        // direction = normalize(to - from), result = from + dist * direction
    }

    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        // No-op for Cartesian frames
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        // BasicFrame attribute changes don't affect coordinate geometry
        // (no system/epoch that would require a transformation)
        Ok(Box::new(UnitMap::new(self.naxes)))
    }
}
```

### format() Details

- Default format: Rust's `{:.6g}`-equivalent (6 significant figures)
- If `format_str` is set, interpret as a precision number:
  - `"10.5"` → 5 decimal places, field width 10
  - Just use the precision part for `format!("{value:.prec$}")`
- BAD_VALUE input → format as `"<bad>"`

### unformat() Details

- Skip leading whitespace
- Parse f64 using Rust's `str::parse` on the longest valid prefix
- Return `(value, characters_consumed)` where characters_consumed
  includes the leading whitespace

## CmpFrame (src/frames/cmpframe.rs)

### Structure

```rust
pub struct CmpFrame {
    frame1: DynFrame,
    frame2: DynFrame,
}
```

### Constructor

```rust
impl CmpFrame {
    pub fn new(frame1: DynFrame, frame2: DynFrame) -> Self;
}
```

No validation needed — any two frames can be combined.

### Axis Routing

`CmpFrame` has `naxes = frame1.naxes() + frame2.naxes()`.

For axis-indexed operations, axes 1..=N1 route to frame1, and axes
N1+1..=N1+N2 route to frame2 (with index shifted by N1).

```rust
fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
    let n1 = self.frame1.naxes();
    if axis <= n1 {
        (&*self.frame1, axis)
    } else {
        (&*self.frame2, axis - n1)
    }
}
```

### Frame Trait Implementation

```rust
impl Frame for CmpFrame {
    fn naxes(&self) -> usize {
        self.frame1.naxes() + self.frame2.naxes()
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        let (frame, local_axis) = self.route_axis(axis);
        frame.format(local_axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        let (frame, local_axis) = self.route_axis(axis);
        frame.unformat(local_axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        // Euclidean over full compound space
        // (same as BasicFrame: sqrt of sum of squared diffs)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        // Compound angle using full-space vectors
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        // Step along line in full compound space
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        // Delegate to each child for its own axes
        let n1 = self.frame1.naxes();
        self.frame1.norm(&mut coords[..n1])?;
        self.frame2.norm(&mut coords[n1..])?;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(CmpFrame {
            frame1: self.frame1.snapshot(),
            frame2: self.frame2.snapshot(),
        })
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes())))
    }
}
```

### Debug

Manual `Debug` impl showing frame1/frame2 naxes (since `DynFrame`
doesn't implement Debug).

## Testing Strategy

### test_basicframe.rs (equivalent to C testframe.c)

Comment header: `// Rust equivalent of reference/starlink-ast/ast_tester/testframe.c`

Tests:
- Construction via builder (various naxes)
- Default attribute values (label defaults to "Axis N")
- set/get/test/clear for all attribute types
- Axis validation (0 and out-of-range rejected)
- Euclidean distance (2D, 3D)
- Angle computation
- Offset computation
- norm() is no-op
- format/unformat round-trip
- BAD_VALUE handling in distance/angle/offset
- snapshot() produces independent clone

### test_cmpframe.rs (equivalent to C testcmpframe.c)

Comment header: `// Rust equivalent of reference/starlink-ast/ast_tester/testcmpframe.c`

Tests:
- CmpFrame naxes is sum of children
- format/unformat delegates to correct child
- Axis routing (axes 1-2 → frame1, axes 3-5 → frame2)
- distance/angle/offset work over full space
- norm delegates to children
- snapshot clones both children independently

## Dependencies

This step uses `UnitMap` from Step 2 (for `map_from`).

## What This Step Does NOT Include

- `convert()` — deferred to Step 4 (FrameSet)
- Sexagesimal formatting — SkyFrame concern
- Epoch/System-dependent `map_from` — SkyFrame concern
- Serialisation (to_node/from_node)
- FrameSet integrity mechanism — Step 4
