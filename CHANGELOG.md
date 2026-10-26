# Changelog

All notable changes to the published crates of this repository, `starlink-ast` and, once it is published, `starlink-ast-capi`, are recorded here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The crates follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as Cargo applies it: while a version is `0.y.z`, a change that breaks the public API raises `y` and anything else raises `z`.
Each crate is versioned on its own, so a version heading names its crate, and an entry under `Unreleased` names its crate when the repository publishes more than one.

## [Unreleased]

### Fixed

- `starlink-ast`: the guide's Cargo chapter gives the crates.io dependency, `starlink-ast = "0.1"`, rather than saying the crate is unpublished and must come from its repository.

## [starlink-ast 0.1.0] - 2026-10-26

### Added

- `starlink-ast`, a pure-Rust port of the Starlink AST library: the Mapping, Frame, FrameSet and Region classes, FitsChan for reading and writing FITS-WCS headers, the Native and MOC channels, KeyMap, Table and FitsTable, and a user guide ported from [SUN/211](https://starlink.eao.hawaii.edu/devdocs/sun211.htx/sun211.html), with transforms, simplification and serialization checked against the C library.

[Unreleased]: https://github.com/Starlink/starlink-ast-rs/compare/starlink-ast-v0.1.0...main
[starlink-ast 0.1.0]: https://github.com/Starlink/starlink-ast-rs/releases/tag/starlink-ast-v0.1.0
