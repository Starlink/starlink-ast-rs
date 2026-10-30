# Using AST from Cargo

Add the library to the `[dependencies]` of your `Cargo.toml`:

```toml
[dependencies]
starlink-ast = "0.1"
```

The versions published on [crates.io](https://crates.io/crates/starlink-ast) follow semantic versioning, so `"0.1"` accepts every `0.1.z` release.
To build against the development version instead, name the repository:

```toml
[dependencies]
starlink-ast = { git = "https://github.com/Starlink/starlink-ast-rs" }
```

The library is Rust throughout, including the PAL, ERFA and WCSLIB routines it uses.
Building it needs a Rust toolchain and no C compiler.

The traits whose methods are called on Mappings, Frames, Regions and Channels are re-exported by `starlink_ast::prelude`, so one `use starlink_ast::prelude::*;` makes them all callable.
The types are named from their modules.

A first program converts a position from FK5 equatorial coordinates to Galactic coordinates:

```rust
use starlink_ast::prelude::*;
use starlink_ast::frames::{SkyFrame, SkySystem};

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
