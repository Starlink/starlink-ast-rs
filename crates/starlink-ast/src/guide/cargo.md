# Using AST from Cargo

Add the library to the `[dependencies]` of your `Cargo.toml`:

```toml
[dependencies]
starlink-ast = { git = "https://github.com/Starlink/starlink-ast-rs" }
```

The crate is not yet published on crates.io, so it is taken from its repository.

The library includes the C sources of the PAL, ERFA and WCSLIB libraries it uses, and its build script compiles them.
Building it needs a C compiler, and nothing else installed.

A first program converts a position from FK5 equatorial coordinates to Galactic coordinates:

```rust
use starlink_ast::frames::{Frame, FrameSearch, SkyFrame, SkySystem};
use starlink_ast::mappings::Mapping;

let mut fk5 = SkyFrame::new(SkySystem::Fk5);
let mut galactic = SkyFrame::new(SkySystem::Galactic);
let fk5_to_galactic = fk5.convert_to(&mut galactic, &[])?.expect("conversion exists");

// The north Galactic pole, at RA 12h51m26.3s, Dec +27d07m42s (J2000).
let (ra, dec) = (fk5.unformat(1, "12:51:26.3")?.0, fk5.unformat(2, "27:07:42")?.0);
let (l, b) = fk5_to_galactic.tran2(&[ra], &[dec], true)?;
assert!((b[0].to_degrees() - 90.0).abs() < 1e-3);
# let _ = l;
# Ok::<(), starlink_ast::error::AstError>(())
```
