# AST -- World Coordinate Systems Library... in Rust

The Starlink [AST library](https://starlink.eao.hawaii.edu/starlink/AST) provides
a comprehensive range of facilities for
attaching world coordinate systems to astronomical data, for
retrieving and interpreting that information in a variety of formats,
including FITS-WCS, and for generating graphical output based on it.

This library should be of interest to anyone writing astronomical
applications which need to manipulate coordinate system data,
especially celestial or spectral coordinate systems.
AST is portable and environment-independent.

This repository is a project to create a from scratch rewrite of the C library in Rust, retaining a thing C wrapper that will enable existing users to link against it without changing their code.

## Documentation for classical interface

Full documentation of the C and Fortran versions is available in the Starlink User Notes:

- [SUN/210](https://starlink.eao.hawaii.edu/devdocs/sun210.htx/sun210.html) -- Fortran documentation
- [SUN/211](https://starlink.eao.hawaii.edu/devdocs/sun211.htx/sun211.html) -- C documentation
