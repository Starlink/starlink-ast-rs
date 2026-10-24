# starlink-ast

A pure-Rust port of the [C AST](https://starlink.eao.hawaii.edu/starlink/AST), the [Starlink Project](https://en.wikipedia.org/wiki/Starlink_Project)'s library for world coordinate systems in astronomical data.

AST describes coordinate systems and the relationships between them.
It attaches celestial, spectral, time and other coordinate systems to data, converts positions between them, and reads and writes them as FITS-WCS headers and in its own formats.
The library and its design are described in Berry, Warren-Smith and Jenness (2016), *AST: A library for modelling and manipulating coordinate systems*, Astronomy and Computing 15, 33, [doi:10.1016/j.ascom.2016.02.003](https://doi.org/10.1016/j.ascom.2016.02.003).

## Status

This port is evolving.
It follows the C library closely and is checked against it, and its Rust API will change between minor versions until it settles.
It has been created almost entirely using large-language model coding agents such as Anthropic Claude and OpenAI Codex, with human guidance.
A thin C-compatible shim, `starlink-ast-capi`, will let existing applications link against the Rust build without changing their code; it is not published yet.

## Using the crate

```toml
[dependencies]
starlink-ast = "0.1"
```

The crate carries the C sources of the PAL, ERFA and WCSLIB routines it still uses and compiles them in its build script, so building needs a C compiler and nothing else.

A first program converts a position from FK5 equatorial coordinates to Galactic coordinates:

```rust
use starlink_ast::prelude::*;
use starlink_ast::frames::{SkyFrame, SkySystem};

fn main() -> Result<(), starlink_ast::error::AstError> {
    let mut fk5 = SkyFrame::new(SkySystem::Fk5);
    let mut galactic = SkyFrame::new(SkySystem::Galactic);
    let fk5_to_galactic = fk5.convert_to(&mut galactic, &[])?.expect("conversion exists");

    // The north Galactic pole, at RA 12h51m26.3s, Dec +27d07m42s (J2000).
    let (ra, dec) = (fk5.unformat(1, "12:51:26.3")?.0, fk5.unformat(2, "27:07:42")?.0);
    let (l, b) = fk5_to_galactic.tran2(&[ra], &[dec], true)?;
    println!("l = {:.3} deg, b = {:.3} deg", l[0].to_degrees(), b[0].to_degrees());
    Ok(())
}
```

## Documentation

- The [user guide](https://docs.rs/starlink-ast/latest/starlink_ast/guide/index.html), ported from the C library's manual, is the place to start, and the [API reference](https://docs.rs/starlink-ast) documents every module and item.
- [SUN/211](https://www.starlink.ac.uk/docs/sun211.htx/sun211.html) is the C library's own guide and reference.
- The C library's source is at [Starlink/ast](https://github.com/Starlink/ast).

## License

Copyright 2026 Association of Universities for Research in Astronomy (AURA).
The crate is distributed under the GNU Lesser General Public License, version 3 or later, like the C library it is ported from.
`COPYRIGHT` lists the copyright holders of the C library and of the C sources the crate carries.
