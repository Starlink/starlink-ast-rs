# Changelog

All notable changes to the published crates of this repository, `starlink-ast` and, once it is published, `starlink-ast-capi`, are recorded here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The crates follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as Cargo applies it: while a version is `0.y.z`, a change that breaks the public API raises `y` and anything else raises `z`.
An entry names its crate when the repository publishes more than one.

## [Unreleased]

### Added

- `starlink-ast`, a pure-Rust port of the Starlink AST library: the Mapping, Frame, FrameSet and Region classes, FitsChan for reading and writing FITS-WCS headers, the Native and MOC channels, KeyMap, Table and FitsTable, and a user guide ported from [SUN/211](https://starlink.eao.hawaii.edu/devdocs/sun211.htx/sun211.html), with transforms, simplification and serialization checked against the C library.


[Unreleased]: https://github.com/Starlink/starlink-ast-rs/commits/main
