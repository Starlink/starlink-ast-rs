# Changelog

All notable changes to the published crates of this repository, `starlink-ast` and, once it is published, `starlink-ast-capi`, are recorded here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The crates follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as Cargo applies it: while a version is `0.y.z`, a change that breaks the public API raises `y` and anything else raises `z`.
Each crate is versioned on its own, so a version heading names its crate, and an entry under `Unreleased` names its crate when the repository publishes more than one.

## [Unreleased]

### Changed

- `starlink-ast`: WcsMap projections give upstream wcslib 8.10's results where the wcslib code AST carries had differed: QSC within 0.008 degrees of face 4's centre, HPX with H and K other than 4 and 3, at the poles, on facet edges and at native longitude +180, ZPN with a degree-2 polynomial on the far side (now bad), PCO near the equator, COP at the poles, and MER near the pole.
- `starlink-ast`: a WcsMap reverse projection that lands a native longitude or latitude just past a limit, by up to wcslib's tolerance for the projection (1e-13 to 1e-11 degrees), returns the limit rather than bad, as wcslib does.
- `starlink-ast`: SlaMap's FK5 to FK4 conversion (`Fk54z`) applies the fictitious FK4 proper motion to the unit vector, as ERFA and SLALIB do, rather than linearly in RA and Dec; away from B1950 this changes points the motion carries close to a pole by up to 28 mas at B1980 and 0.3 arcsec at B2050, and others by about 1e-14 radians.
- `starlink-ast`: the vendored ERFA is version 2.0.1, with the IAU 2012 astronomical unit; spectral and time conversions give the same results as before.
- `starlink-ast`: SphMap computes its arctangents, sines and cosines with branchless kernels that the compiler vectorizes; on large point sets its forward transform runs about three times faster and its inverse about 1.7 times on Apple silicon, and 2.4 and 3.5 times faster on x86-64, where the loops run in a build for AVX2 and FMA when the CPU has them. Each sine, cosine and arctangent may differ from earlier releases by up to 1 ULP, so a Cartesian component, the product of a sine and a cosine, by up to 3 ULP; results do not depend on how many points are transformed together.
- `starlink-ast`: a WcsMap whose projection parameters are unusable reports `ErrorCode::Wcspa` from every transform that has a good point, rather than only from the first, after which it gave bad values; the C library reports it only once.
- `starlink-ast`: every WcsMap projection, and TPN's polynomial, is Rust, and the crate no longer compiles any C for wcslib; results are bit for bit those of the C projections they replace, and WcsMap transforms whole slices through the projection kernels.
- `starlink-ast`: WcsMap's TAN projection uses vectorizable trigonometry in bulk forward transforms (native spherical to projected coordinates), which on large point sets run between two and three times faster; results may differ from earlier releases by a few ULP.
- `starlink-ast`: WcsMap's SIN projection uses vectorizable trigonometry in bulk forward transforms (native spherical to projected coordinates), which on large point sets run about 1.7 times faster through an orthographic WcsMap, the projection itself 2.4 to 3 times for the orthographic form and 1.8 to 2.3 times for the synthesis form; results may differ from earlier releases by a few ULP, or, where the synthesis form's terms cancel, by a few units of 2^-52 of the larger term, and a point is bad exactly when it was.
- `starlink-ast`: WcsMap's TPN projection uses vectorizable trigonometry for the gnomonic stage of bulk forward transforms (native spherical to projected coordinates), which on large point sets run 10 to 20% faster; results may differ from earlier releases by up to 1e-10 of a coordinate, the square of the tolerance of TPN's Newton iteration, plus the rounding error of TPN's polynomials, and in tests by at most 1e-14 of a coordinate with macOS's libm on Apple silicon and 6e-13 with its x86-64 libm; points whose projection lies on or very near an axis of the projection plane give the same results as before.

### Fixed

- `starlink-ast`: the guide's Cargo chapter gives the crates.io dependency, `starlink-ast = "0.1"`, rather than saying the crate is unpublished and must come from its repository.

## [starlink-ast 0.1.0] - 2026-10-26

### Added

- `starlink-ast`, a pure-Rust port of the Starlink AST library: the Mapping, Frame, FrameSet and Region classes, FitsChan for reading and writing FITS-WCS headers, the Native and MOC channels, KeyMap, Table and FitsTable, and a user guide ported from [SUN/211](https://starlink.eao.hawaii.edu/devdocs/sun211.htx/sun211.html), with transforms, simplification and serialization checked against the C library.

[Unreleased]: https://github.com/Starlink/starlink-ast-rs/compare/starlink-ast-v0.1.0...main
[starlink-ast 0.1.0]: https://github.com/Starlink/starlink-ast-rs/releases/tag/starlink-ast-v0.1.0
