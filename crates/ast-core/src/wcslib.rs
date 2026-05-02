// All wrappers are pub(crate) and will be consumed by WcsMap.
// Until WcsMap lands, suppress dead-code warnings.
#![allow(dead_code)]

//! Safe Rust wrappers for the vendored wcslib projection functions.
//!
//! The C wcslib library is compiled from `vendor/wcslib/{proj,tpn,wcstrig,ast_memory_stub}.c`
//! into `libast_wcslib.a` by `build.rs`.  The C symbols keep their original
//! `ast`-prefixed names (e.g. `astPRJset`, `astTANfwd`).
//!
//! This module is `pub(crate)` and provides a safe [`PrjParams`] wrapper
//! around the raw `AstPrjPrm` C struct, together with a [`WcsProjection`]
//! enum covering all 31 FITS-WCS projection types.

use std::ffi::c_int;

use crate::error::{AstError, AstResult, ErrorCode};

// ---------------------------------------------------------------------------
// WcsProjection enum
// ---------------------------------------------------------------------------

/// FITS-WCS projection type.
///
/// Each variant corresponds to a 3-letter FITS projection code recognised
/// by the vendored wcslib.  `Ncp` and `Gls` are deprecated aliases for
/// `Sin` and `Sfl` respectively — they are handled at the `WcsMap` level
/// by setting appropriate projection parameters before delegating to the
/// underlying standard projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WcsProjection {
    /// Zenithal/azimuthal perspective
    Azp,
    /// Slant zenithal perspective
    Szp,
    /// Gnomonic (tangent plane)
    Tan,
    /// Stereographic
    Stg,
    /// Orthographic / synthesis
    Sin,
    /// Zenithal/azimuthal equidistant
    Arc,
    /// Zenithal/azimuthal polynomial
    Zpn,
    /// Zenithal/azimuthal equal-area
    Zea,
    /// Airy
    Air,
    /// Cylindrical perspective
    Cyp,
    /// Cylindrical equal-area
    Cea,
    /// Plate carree
    Car,
    /// Mercator
    Mer,
    /// Sanson-Flamsteed (global sinusoidal)
    Sfl,
    /// Parabolic
    Par,
    /// Mollweide
    Mol,
    /// Hammer-Aitoff
    Ait,
    /// Conic perspective
    Cop,
    /// Conic equal-area
    Coe,
    /// Conic equidistant
    Cod,
    /// Conic orthomorphic
    Coo,
    /// Bonne
    Bon,
    /// Polyconic
    Pco,
    /// Tangential spherical cube
    Tsc,
    /// COBE spherical cube
    Csc,
    /// Quadrilateralised spherical cube
    Qsc,
    /// North celestial pole (deprecated; alias for SIN)
    Ncp,
    /// Global sinusoidal (deprecated; alias for SFL)
    Gls,
    /// TAN with polynomial correction terms
    Tpn,
    /// HEALPix
    Hpx,
    /// HEALPix polar
    Xph,
}

impl WcsProjection {
    /// Return the 3-letter FITS projection code (uppercase).
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Azp => "AZP",
            Self::Szp => "SZP",
            Self::Tan => "TAN",
            Self::Stg => "STG",
            Self::Sin => "SIN",
            Self::Arc => "ARC",
            Self::Zpn => "ZPN",
            Self::Zea => "ZEA",
            Self::Air => "AIR",
            Self::Cyp => "CYP",
            Self::Cea => "CEA",
            Self::Car => "CAR",
            Self::Mer => "MER",
            Self::Sfl => "SFL",
            Self::Par => "PAR",
            Self::Mol => "MOL",
            Self::Ait => "AIT",
            Self::Cop => "COP",
            Self::Coe => "COE",
            Self::Cod => "COD",
            Self::Coo => "COO",
            Self::Bon => "BON",
            Self::Pco => "PCO",
            Self::Tsc => "TSC",
            Self::Csc => "CSC",
            Self::Qsc => "QSC",
            Self::Ncp => "NCP",
            Self::Gls => "GLS",
            Self::Tpn => "TPN",
            Self::Hpx => "HPX",
            Self::Xph => "XPH",
        }
    }

    /// Parse a 3-letter FITS projection code (case-insensitive).
    ///
    /// Returns `None` if the code is not recognised.
    pub(crate) fn from_code(s: &str) -> Option<Self> {
        // Fast path: check length first.
        if s.len() != 3 {
            return None;
        }
        // Uppercase the 3 bytes for comparison.
        let mut buf = [0u8; 3];
        for (i, b) in s.bytes().enumerate() {
            buf[i] = b.to_ascii_uppercase();
        }
        match &buf {
            b"AZP" => Some(Self::Azp),
            b"SZP" => Some(Self::Szp),
            b"TAN" => Some(Self::Tan),
            b"STG" => Some(Self::Stg),
            b"SIN" => Some(Self::Sin),
            b"ARC" => Some(Self::Arc),
            b"ZPN" => Some(Self::Zpn),
            b"ZEA" => Some(Self::Zea),
            b"AIR" => Some(Self::Air),
            b"CYP" => Some(Self::Cyp),
            b"CEA" => Some(Self::Cea),
            b"CAR" => Some(Self::Car),
            b"MER" => Some(Self::Mer),
            b"SFL" => Some(Self::Sfl),
            b"PAR" => Some(Self::Par),
            b"MOL" => Some(Self::Mol),
            b"AIT" => Some(Self::Ait),
            b"COP" => Some(Self::Cop),
            b"COE" => Some(Self::Coe),
            b"COD" => Some(Self::Cod),
            b"COO" => Some(Self::Coo),
            b"BON" => Some(Self::Bon),
            b"PCO" => Some(Self::Pco),
            b"TSC" => Some(Self::Tsc),
            b"CSC" => Some(Self::Csc),
            b"QSC" => Some(Self::Qsc),
            b"NCP" => Some(Self::Ncp),
            b"GLS" => Some(Self::Gls),
            b"TPN" => Some(Self::Tpn),
            b"HPX" => Some(Self::Hpx),
            b"XPH" => Some(Self::Xph),
            _ => None,
        }
    }

    /// Return the integer type code used by the C wcslib.
    ///
    /// These match the `WCS__*` constants in `vendor/wcslib/proj.c`.
    /// The encoding groups projections by class (zenithal=1xx,
    /// cylindrical=2xx, pseudo-cylindrical=3xx, etc.).
    pub(crate) fn type_code(&self) -> i32 {
        match self {
            // Zenithal (1xx)
            Self::Azp => 101,
            Self::Szp => 102,
            Self::Tan => 103,
            Self::Stg => 104,
            Self::Sin => 105,
            Self::Arc => 106,
            Self::Zpn => 107,
            Self::Zea => 108,
            Self::Air => 109,
            // Cylindrical (2xx)
            Self::Cyp => 201,
            Self::Cea => 202,
            Self::Car => 203,
            Self::Mer => 204,
            // Pseudo-cylindrical (3xx)
            Self::Sfl => 301,
            Self::Par => 302,
            Self::Mol => 303,
            // Pseudo-cylindrical (4xx)
            Self::Ait => 401,
            // Conic (5xx)
            Self::Cop => 501,
            Self::Coe => 502,
            Self::Cod => 503,
            Self::Coo => 504,
            // Polyconic (6xx)
            Self::Bon => 601,
            Self::Pco => 602,
            // Quad-cube (7xx)
            Self::Tsc => 701,
            Self::Csc => 702,
            Self::Qsc => 703,
            // HEALPix (8xx)
            Self::Hpx => 801,
            Self::Xph => 802,
            // Deprecated (use same code as underlying projection)
            Self::Ncp => 105, // -> SIN
            Self::Gls => 301, // -> SFL
            // TPN uses 999 (defined in tpn.c)
            Self::Tpn => 999,
        }
    }

    /// Convert an integer type code to a projection variant.
    ///
    /// Returns `None` if the code is not recognised.  Note that the
    /// deprecated `Ncp` (105) and `Gls` (301) are not returned — the
    /// canonical `Sin` and `Sfl` are returned instead.
    pub(crate) fn from_type_code(i: i32) -> Option<Self> {
        match i {
            101 => Some(Self::Azp),
            102 => Some(Self::Szp),
            103 => Some(Self::Tan),
            104 => Some(Self::Stg),
            105 => Some(Self::Sin),
            106 => Some(Self::Arc),
            107 => Some(Self::Zpn),
            108 => Some(Self::Zea),
            109 => Some(Self::Air),
            201 => Some(Self::Cyp),
            202 => Some(Self::Cea),
            203 => Some(Self::Car),
            204 => Some(Self::Mer),
            301 => Some(Self::Sfl),
            302 => Some(Self::Par),
            303 => Some(Self::Mol),
            401 => Some(Self::Ait),
            501 => Some(Self::Cop),
            502 => Some(Self::Coe),
            503 => Some(Self::Cod),
            504 => Some(Self::Coo),
            601 => Some(Self::Bon),
            602 => Some(Self::Pco),
            701 => Some(Self::Tsc),
            702 => Some(Self::Csc),
            703 => Some(Self::Qsc),
            801 => Some(Self::Hpx),
            802 => Some(Self::Xph),
            999 => Some(Self::Tpn),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Raw FFI types and extern declarations
// ---------------------------------------------------------------------------

/// Maximum number of projection parameters per axis.
const WCSLIB_MXPAR: usize = 100;

/// FFI representation of the C `struct AstPrjPrm` from `vendor/wcslib/proj.h`.
///
/// The `p` and `p2` pointers point into owned `Box<[f64; WCSLIB_MXPAR]>`
/// storage managed by [`PrjParams`].  The `fwd` and `rev` function
/// pointers are set by `astPRJset`.
#[repr(C)]
struct AstPrjPrm {
    code: [u8; 4],
    flag: c_int,
    phi0: f64,
    theta0: f64,
    r0: f64,
    p: *mut f64,
    p2: *mut f64,
    w: [f64; 20],
    n: c_int,
    fwd: Option<unsafe extern "C" fn(f64, f64, *mut AstPrjPrm, *mut f64, *mut f64) -> c_int>,
    rev: Option<unsafe extern "C" fn(f64, f64, *mut AstPrjPrm, *mut f64, *mut f64) -> c_int>,
}

unsafe extern "C" {
    fn astPRJset(code: *const u8, prm: *mut AstPrjPrm) -> c_int;
    fn astTPNset(prm: *mut AstPrjPrm) -> c_int;
}

// ---------------------------------------------------------------------------
// PrjParams safe wrapper
// ---------------------------------------------------------------------------

/// Safe wrapper around the C `AstPrjPrm` struct.
///
/// Owns the backing storage for the `p` and `p2` parameter arrays,
/// ensuring the pointers inside the C struct remain valid for the
/// lifetime of the wrapper.
///
/// This type is not `Clone` because cloning would create dangling
/// internal pointers.
pub(crate) struct PrjParams {
    inner: AstPrjPrm,
    /// Backing storage for `inner.p` (latitude-axis projection parameters).
    p_storage: Box<[f64; WCSLIB_MXPAR]>,
    /// Backing storage for `inner.p2` (longitude-axis projection parameters, TPN only).
    p2_storage: Box<[f64; WCSLIB_MXPAR]>,
}

// SAFETY: PrjParams is fully self-contained — the internal pointers
// reference only the co-owned Box allocations, and nothing is shared
// with other threads.
unsafe impl Send for PrjParams {}

impl PrjParams {
    /// Create a new projection parameter set for the given projection type.
    ///
    /// The C struct is zeroed, the parameter storage is allocated and
    /// pointed to, and `astPRJset` is called to initialise the projection.
    ///
    /// NCP and GLS are deprecated projections that are not directly
    /// supported by wcslib's `astPRJset`.  They must be mapped to SIN
    /// and SFL (with appropriate parameters) at the `WcsMap` level before
    /// calling this constructor.
    pub(crate) fn new(proj: WcsProjection) -> AstResult<Self> {
        // NCP and GLS are not recognised by astPRJset — they must be
        // converted to SIN/SFL with parameters at a higher level.
        if matches!(proj, WcsProjection::Ncp | WcsProjection::Gls) {
            return Err(AstError::new(
                ErrorCode::Wcsty,
                format!(
                    "Projection {} is deprecated and must be handled at the WcsMap level",
                    proj.code()
                ),
            ));
        }

        let mut p_storage = Box::new([0.0f64; WCSLIB_MXPAR]);
        let mut p2_storage = Box::new([0.0f64; WCSLIB_MXPAR]);

        // Build the null-terminated 3-letter code.
        let code_str = proj.code().as_bytes();
        let mut code = [0u8; 4];
        code[..code_str.len()].copy_from_slice(code_str);
        // code[3] is already 0 (null terminator)

        let mut inner = AstPrjPrm {
            code,
            flag: 0,
            phi0: 0.0,
            theta0: 0.0,
            r0: 0.0,
            p: p_storage.as_mut_ptr(),
            p2: p2_storage.as_mut_ptr(),
            w: [0.0; 20],
            n: 0,
            fwd: None,
            rev: None,
        };

        // TPN is not handled by astPRJset (it's defined in tpn.c with
        // its own astTPNset entry point).
        let status = if matches!(proj, WcsProjection::Tpn) {
            unsafe { astTPNset(&mut inner) }
        } else {
            unsafe { astPRJset(code.as_ptr(), &mut inner) }
        };
        if status != 0 {
            return Err(AstError::new(
                ErrorCode::Wcsty,
                format!(
                    "wcslib projection setup failed for '{}' (status {})",
                    proj.code(),
                    status
                ),
            ));
        }

        Ok(Self {
            inner,
            p_storage,
            p2_storage,
        })
    }

    /// Set a projection parameter.
    ///
    /// - `axis`: 0 for the latitude axis (`p`), 1 for the longitude axis (`p2`).
    /// - `m`: parameter index (0-based, must be < [`WCSLIB_MXPAR`]).
    /// - `value`: the parameter value.
    ///
    /// Sets the internal `flag` to 0 so that the next forward/reverse
    /// call triggers re-initialisation inside wcslib.
    ///
    /// # Panics
    ///
    /// Panics if `m >= WCSLIB_MXPAR` or `axis > 1`.
    pub(crate) fn set_param(&mut self, axis: usize, m: usize, value: f64) {
        assert!(m < WCSLIB_MXPAR, "parameter index {m} out of range");
        assert!(axis <= 1, "axis must be 0 (lat/p) or 1 (lon/p2)");

        match axis {
            0 => self.p_storage[m] = value,
            1 => self.p2_storage[m] = value,
            _ => unreachable!(),
        }

        // Force re-initialisation on next use.
        self.inner.flag = 0;
    }

    /// Read a projection parameter.
    ///
    /// - `axis`: 0 for the latitude axis (`p`), 1 for the longitude axis (`p2`).
    /// - `m`: parameter index (0-based).
    ///
    /// Returns `None` if `m >= WCSLIB_MXPAR` or `axis > 1`.
    pub(crate) fn get_param(&self, axis: usize, m: usize) -> Option<f64> {
        if m >= WCSLIB_MXPAR || axis > 1 {
            return None;
        }
        match axis {
            0 => Some(self.p_storage[m]),
            1 => Some(self.p2_storage[m]),
            _ => None,
        }
    }

    /// Forward projection: spherical (phi, theta) in degrees to
    /// intermediate world coordinates (x, y).
    ///
    /// Returns an error if the function pointer is not set or the C
    /// function returns a non-zero status.
    pub(crate) fn forward(&mut self, phi: f64, theta: f64) -> AstResult<(f64, f64)> {
        let fwd = self.inner.fwd.ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "wcslib forward projection function pointer is NULL",
            )
        })?;

        let mut x = 0.0;
        let mut y = 0.0;

        // SAFETY: the function pointer was set by astPRJset and operates
        // on the AstPrjPrm struct.  We pass valid mutable pointers.
        let status = unsafe { fwd(phi, theta, &mut self.inner, &mut x, &mut y) };

        match status {
            0 => Ok((x, y)),
            1 => Err(AstError::new(
                ErrorCode::Wcspa,
                "wcslib forward projection: invalid projection parameters",
            )),
            2 => Err(AstError::new(
                ErrorCode::Wcspa,
                format!("wcslib forward projection: invalid (phi, theta) = ({phi}, {theta})"),
            )),
            _ => Err(AstError::new(
                ErrorCode::Inter,
                format!("wcslib forward projection: unexpected status {status}"),
            )),
        }
    }

    /// Reverse projection: intermediate world coordinates (x, y) to
    /// spherical (phi, theta) in degrees.
    ///
    /// Returns an error if the function pointer is not set or the C
    /// function returns a non-zero status.
    pub(crate) fn reverse(&mut self, x: f64, y: f64) -> AstResult<(f64, f64)> {
        let rev = self.inner.rev.ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "wcslib reverse projection function pointer is NULL",
            )
        })?;

        let mut phi = 0.0;
        let mut theta = 0.0;

        // SAFETY: the function pointer was set by astPRJset and operates
        // on the AstPrjPrm struct.  We pass valid mutable pointers.
        let status = unsafe { rev(x, y, &mut self.inner, &mut phi, &mut theta) };

        match status {
            0 => Ok((phi, theta)),
            1 => Err(AstError::new(
                ErrorCode::Wcspa,
                "wcslib reverse projection: invalid projection parameters",
            )),
            2 => Err(AstError::new(
                ErrorCode::Wcspa,
                format!("wcslib reverse projection: invalid (x, y) = ({x}, {y})"),
            )),
            _ => Err(AstError::new(
                ErrorCode::Inter,
                format!("wcslib reverse projection: unexpected status {status}"),
            )),
        }
    }

    /// Return the reference point (phi0, theta0) in degrees.
    pub(crate) fn reference_point(&self) -> (f64, f64) {
        (self.inner.phi0, self.inner.theta0)
    }

    /// Return the radius of the generating sphere, r0.
    pub(crate) fn r0(&self) -> f64 {
        self.inner.r0
    }
}

impl std::fmt::Debug for PrjParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show the projection code and flag; omit the raw C internals.
        let code = std::str::from_utf8(&self.inner.code)
            .unwrap_or("???")
            .trim_end_matches('\0');
        f.debug_struct("PrjParams")
            .field("code", &code)
            .field("flag", &self.inner.flag)
            .field("phi0", &self.inner.phi0)
            .field("theta0", &self.inner.theta0)
            .field("r0", &self.inner.r0)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tan_reference_point() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        // TAN reference point is (0, 90) degrees — projects to (0, 0).
        let (x, y) = prj.forward(0.0, 90.0).unwrap();
        assert!(x.abs() < 1e-10, "x = {x}");
        assert!(y.abs() < 1e-10, "y = {y}");
    }

    #[test]
    fn tan_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        let (x, y) = prj.forward(10.0, 80.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 10.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 80.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn car_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Car).unwrap();
        let (x, y) = prj.forward(45.0, 30.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 45.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 30.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn sin_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Sin).unwrap();
        let (x, y) = prj.forward(5.0, 85.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 5.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 85.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn ait_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Ait).unwrap();
        let (x, y) = prj.forward(30.0, 45.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 30.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 45.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn arc_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Arc).unwrap();
        let (x, y) = prj.forward(15.0, 60.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 15.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 60.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn stg_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Stg).unwrap();
        let (x, y) = prj.forward(20.0, 70.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 20.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 70.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn mer_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Mer).unwrap();
        let (x, y) = prj.forward(25.0, 40.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 25.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 40.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn projection_code_round_trip() {
        for proj in [
            WcsProjection::Tan,
            WcsProjection::Sin,
            WcsProjection::Ait,
            WcsProjection::Car,
            WcsProjection::Azp,
            WcsProjection::Hpx,
            WcsProjection::Tpn,
            WcsProjection::Ncp,
            WcsProjection::Gls,
        ] {
            let code = proj.code();
            let restored = WcsProjection::from_code(code).unwrap();
            assert_eq!(proj, restored, "round-trip failed for {code}");
        }
    }

    #[test]
    fn projection_code_case_insensitive() {
        assert_eq!(WcsProjection::from_code("tan"), Some(WcsProjection::Tan));
        assert_eq!(WcsProjection::from_code("Ait"), Some(WcsProjection::Ait));
        assert_eq!(WcsProjection::from_code("XYZ"), None);
    }

    #[test]
    fn type_code_round_trip() {
        // All non-deprecated projections should round-trip through type_code.
        let projections = [
            WcsProjection::Azp,
            WcsProjection::Szp,
            WcsProjection::Tan,
            WcsProjection::Stg,
            WcsProjection::Sin,
            WcsProjection::Arc,
            WcsProjection::Zpn,
            WcsProjection::Zea,
            WcsProjection::Air,
            WcsProjection::Cyp,
            WcsProjection::Cea,
            WcsProjection::Car,
            WcsProjection::Mer,
            WcsProjection::Sfl,
            WcsProjection::Par,
            WcsProjection::Mol,
            WcsProjection::Ait,
            WcsProjection::Cop,
            WcsProjection::Coe,
            WcsProjection::Cod,
            WcsProjection::Coo,
            WcsProjection::Bon,
            WcsProjection::Pco,
            WcsProjection::Tsc,
            WcsProjection::Csc,
            WcsProjection::Qsc,
            WcsProjection::Hpx,
            WcsProjection::Xph,
            WcsProjection::Tpn,
        ];
        for proj in projections {
            let tc = proj.type_code();
            let restored = WcsProjection::from_type_code(tc).unwrap();
            assert_eq!(
                proj, restored,
                "type_code round-trip failed for {:?} (code {})",
                proj, tc
            );
        }
    }

    #[test]
    fn ncp_gls_rejected_by_new() {
        // NCP and GLS must be mapped to SIN/SFL at a higher level.
        assert!(PrjParams::new(WcsProjection::Ncp).is_err());
        assert!(PrjParams::new(WcsProjection::Gls).is_err());
    }

    #[test]
    fn set_param_resets_flag() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        // After init, flag should be non-zero.
        assert_ne!(prj.inner.flag, 0);
        // Setting a parameter should reset flag to 0.
        prj.set_param(0, 0, 1.0);
        assert_eq!(prj.inner.flag, 0);
        // Forward call should still work (triggers re-init inside wcslib).
        let result = prj.forward(0.0, 90.0);
        assert!(result.is_ok());
    }

    #[test]
    fn get_param_out_of_range() {
        let prj = PrjParams::new(WcsProjection::Tan).unwrap();
        assert!(prj.get_param(0, WCSLIB_MXPAR).is_none());
        assert!(prj.get_param(2, 0).is_none());
    }

    #[test]
    fn set_and_get_param() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        prj.set_param(0, 5, 42.0);
        assert_eq!(prj.get_param(0, 5), Some(42.0));
        prj.set_param(1, 10, -3.14);
        assert_eq!(prj.get_param(1, 10), Some(-3.14));
    }

    #[test]
    fn tpn_initialises() {
        // TPN is handled by tpn.c, not proj.c.  Verify it initialises.
        let prj = PrjParams::new(WcsProjection::Tpn);
        assert!(prj.is_ok(), "TPN should initialise successfully");
    }

    #[test]
    fn debug_format() {
        let prj = PrjParams::new(WcsProjection::Tan).unwrap();
        let dbg = format!("{prj:?}");
        assert!(
            dbg.contains("TAN"),
            "debug output should contain 'TAN': {dbg}"
        );
    }

    #[test]
    fn zea_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Zea).unwrap();
        let (x, y) = prj.forward(12.0, 55.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 12.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 55.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn mol_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Mol).unwrap();
        let (x, y) = prj.forward(35.0, 50.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 35.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 50.0).abs() < 1e-10, "theta = {theta}");
    }
}
