//! FITS-WCS sky projection mapping.

use std::cell::RefCell;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};
use crate::wcslib::{PrjParams, WcsProjection};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::perm::PermMap;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Radians to degrees (C `AST__DR2D`).
const DR2D: f64 = 180.0 / std::f64::consts::PI;

/// Degrees to radians (C `AST__DD2R`).
const DD2R: f64 = std::f64::consts::PI / 180.0;

/// A wcslib projection status as C's `Map` reads it (wcsmap.c): 0 gives the
/// projected values, 2 a point that cannot be projected (`None`), and any
/// other status means the projection parameters are unusable, which
/// `Transform` reports as `AST__WCSPA`.
fn wcslib_result((status, a, b): (std::os::raw::c_int, f64, f64)) -> AstResult<Option<(f64, f64)>> {
    match status {
        0 => Ok(Some((a, b))),
        2 => Ok(None),
        _ => Err(AstError::new(
            ErrorCode::Wcspa,
            "WcsMap: the projection parameter values are unusable",
        )),
    }
}

/// The number of `ProjP<m>` keys a legacy dump can hold (wcsmap.h `AST__WCSMX`).
const AST_WCSMX: usize = 10;

/// Prepare native spherical coordinates for a forward WCSLIB projection call.
///
/// The WcsMap forward input is longitude and latitude in radians; WCSLIB
/// expects degrees with the longitude in `[-180,180]` and the latitude in
/// `[-90,90]`. Any point whose latitude falls outside `[-90,90]` is converted
/// to the equivalent point on the complementary meridian. This mirrors
/// wcsmap.c:2820-2839.
///
/// `factor` scales the WcsMap input into radians. It is `1.0` whenever the
/// `TPNTan` attribute is non-zero (the default), and `AST__DD2R` when `TPNTan`
/// has been cleared, in which case the WcsMap works in degrees throughout.
///
/// Returns `(longitude, latitude)` in degrees, ready for `WcsFwd`.
fn forward_prep(in0: f64, in1: f64, factor: f64) -> (f64, f64) {
    let latitude = DR2D * pal::drange(factor * in1);
    if latitude > 90.0 {
        (
            DR2D * pal::drange(std::f64::consts::PI + factor * in0),
            180.0 - latitude,
        )
    } else if latitude < -90.0 {
        (
            DR2D * pal::drange(std::f64::consts::PI + factor * in0),
            -180.0 - latitude,
        )
    } else {
        (DR2D * pal::drange(factor * in0), latitude)
    }
}

/// A mapping that applies a FITS-WCS sky projection.
///
/// `WcsMap` transforms between native spherical coordinates (longitude, latitude
/// in radians) and projected coordinates (x, y), using the wcslib projection
/// library.  It supports all 31 FITS-WCS projection types defined in the
/// [`WcsProjection`] enum.
///
/// Axes other than the longitude and latitude axes are passed through unchanged.
///
/// # Example
///
/// ```
/// use ast_core::mappings::{Mapping, WcsMap, WcsProjection};
/// use ast_core::coords::PointSet;
///
/// // Create a TAN (gnomonic) projection with 2 axes,
/// // longitude on axis 1, latitude on axis 2.
/// let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
///
/// // Transform a point near the pole (in radians).
/// let lon = 0.1_f64;
/// let lat = 1.4_f64;
/// let pts = PointSet::from_rows([[lon, lat]]);
/// let out = wcs.transform(&pts, true).unwrap();
/// // Inverse should recover the original.
/// let back = wcs.transform(&out, false).unwrap();
/// assert!((back.point(0)[0] - lon).abs() < 1e-10);
/// assert!((back.point(0)[1] - lat).abs() < 1e-10);
/// ```
#[derive(Debug)]
pub struct WcsMap {
    /// The projection type requested at construction time.
    ///
    /// For NCP and GLS this stores the original deprecated code, while
    /// the underlying `PrjParams` uses the equivalent SIN or SFL.
    proj: WcsProjection,
    /// 0-based longitude axis index.
    lonax: usize,
    /// 0-based latitude axis index.
    latax: usize,
    /// Total number of coordinate axes (>= 2).
    ncoord: usize,
    /// Per-axis projection parameters.
    /// `params[0]` = latitude axis (wcslib `p`), `params[1]` = longitude axis (wcslib `p2`).
    /// Each inner vec is indexed by parameter number `m`.
    params: Vec<Vec<Option<f64>>>,
    /// wcslib C struct (interior mutability because `transform` takes `&self`
    /// but the C code mutates internal state on first use after param changes).
    prj: RefCell<PrjParams>,
    /// Whether the mapping is inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// Protected `LonCheck` attribute (wcsmap.c:4787). Controls whether the
    /// inverse projection clips returned longitude values to the projection's
    /// primary range. `None` = unset (defaults to `true`); `Some(v)` = the
    /// explicitly set value. Mirrors C's `loncheck` field, whose sentinel
    /// `-INT_MAX` means "not set" (astMAKE_TEST/GET at wcsmap.c:4818-4821).
    lon_check: Option<bool>,
    /// Protected `FITSProj` attribute (wcsmap.c:4711). Marks this WcsMap as
    /// the one that defines the FITS-WCS projection when a compound Mapping
    /// is written out through a FitsChan. A WcsMap used only as a distortion
    /// correction (e.g. the polynomial part of a `-ZPX` header) has it
    /// cleared. `None` = unset (defaults to `true`), matching C's `-INT_MAX`
    /// sentinel (wcsmap.c:4742-4746).
    fits_proj: Option<bool>,
    /// Protected `TPNTan` attribute (wcsmap.c:4750). When cleared, a TPN
    /// projection applies only its polynomial correction and omits the TAN
    /// projection itself. `None` = unset (defaults to `true`), matching C's
    /// `-INT_MAX` sentinel (wcsmap.c:4780-4782).
    tpn_tan: Option<bool>,
}

impl WcsMap {
    /// Create a new `WcsMap`.
    ///
    /// - `ncoord`: total number of coordinate axes (must be >= 2).
    /// - `proj`: the FITS-WCS projection type.
    /// - `lonax`: 1-based index of the longitude axis.
    /// - `latax`: 1-based index of the latitude axis.
    ///
    /// Returns an error if the arguments are invalid or the projection
    /// cannot be initialised.
    pub fn new(ncoord: usize, proj: WcsProjection, lonax: usize, latax: usize) -> AstResult<Self> {
        if ncoord < 2 {
            return Err(AstError::new(
                ErrorCode::Wcsnc,
                format!("WcsMap ncoord must be >= 2, got {ncoord}"),
            ));
        }
        if lonax == 0 || lonax > ncoord {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap lonax {lonax} out of range 1..={ncoord}"),
            ));
        }
        if latax == 0 || latax > ncoord {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap latax {latax} out of range 1..={ncoord}"),
            ));
        }
        if lonax == latax {
            return Err(AstError::new(
                ErrorCode::Wcsax,
                format!("WcsMap lonax and latax must differ, both are {lonax}"),
            ));
        }

        // For NCP and GLS, create the underlying projection as SIN/SFL
        // with appropriate default parameters.
        // For Unknown, we don't need a PrjParams — it acts as identity.
        let underlying = match proj {
            WcsProjection::Ncp => WcsProjection::Sin,
            WcsProjection::Gls => WcsProjection::Sfl,
            WcsProjection::Unknown => {
                // Unknown projection has no wcslib implementation.
                // It acts as an identity mapping and simplifies to UnitMap.
                let params = vec![Vec::new(), Vec::new()];
                // The `prj` field must hold *some* PrjParams, but it is never
                // consulted for an Unknown WcsMap: `transform_into`/
                // `transform_into_nan` short-circuit to an identity copy,
                // `simplified` returns a UnitMap, and `nat_lon`/`nat_lat`
                // return 0 directly. CAR is the cheapest inert placeholder.
                let prj = PrjParams::new(WcsProjection::Car)?;
                return Ok(Self {
                    proj,
                    lonax: lonax - 1,
                    latax: latax - 1,
                    ncoord,
                    params,
                    prj: RefCell::new(prj),
                    inverted: false,
                    simplified: SimplifiedRecord::default(),
                    ident: None,
                    mapping_flags: crate::mappings::MappingFlags::default(),
                    lon_check: None,
                    fits_proj: None,
                    tpn_tan: None,
                });
            }
            other => other,
        };

        let mut prj = PrjParams::new(underlying)?;

        // Initialise empty parameter arrays for 2 axes (lat=0, lon=1).
        let params = vec![Vec::new(), Vec::new()];

        // Assign default projection parameters (C astInitWcsMap_ → InitPrjPrm).
        prj.init_prj_prm(&params, true);

        Ok(Self {
            proj,
            lonax: lonax - 1,
            latax: latax - 1,
            ncoord,
            params,
            prj: RefCell::new(prj),
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
            lon_check: None,
            fits_proj: None,
            tpn_tan: None,
        })
    }

    /// Largest legal projection-parameter index for a 1-based axis index, or
    /// `None` when the axis is neither the longitude nor the latitude axis.
    ///
    /// Mirrors `astGetPVMax` (wcsmap.c:2152-2217), which reads the limit off
    /// the size of the per-axis parameter array — allocated from the
    /// projection table's `mxpar` / `mxpar2` columns — and returns -1 for any
    /// other axis. The lookup uses the declared projection type, as C's
    /// `FindPrjData( this->type )` does: `NCP` and `GLS` keep their own limits
    /// even though they transform through `SIN` and `SFL`.
    pub fn pv_max(&self, axis: usize) -> Option<usize> {
        if axis == 0 || axis > self.ncoord {
            return None;
        }
        if axis - 1 == self.lonax {
            Some(self.proj.pv_max_lon())
        } else if axis - 1 == self.latax {
            Some(self.proj.pv_max_lat())
        } else {
            None
        }
    }

    /// Set a projection parameter.
    ///
    /// - `axis`: 1-based axis index. Must be either `lonax()` or `latax()`.
    /// - `m`: parameter number (0-based), no greater than [`WcsMap::pv_max`]
    ///   for that axis.
    /// - `value`: the parameter value.
    pub fn set_pv(&mut self, axis: usize, m: usize, value: f64) -> AstResult<()> {
        // Determine which wcslib axis this corresponds to.
        // axis is 1-based.  latax is 0-based internally.
        // wcsmap.c SetPV: AST__AXIIN for an axis out of range, and for any
        // parameter on an axis other than the two celestial ones, where
        // `astGetPVMax` gives -1.
        let prj_axis = if axis == 0 || axis > self.ncoord {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "WcsMap: axis index ({axis}) is invalid in attribute PV{axis}_{m} - it \
                     should be in the range 1 to {}",
                    self.ncoord
                ),
            ));
        } else if axis - 1 == self.latax {
            0 // latitude -> p storage
        } else if axis - 1 == self.lonax {
            1 // longitude -> p2 storage
        } else {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "WcsMap: parameter index ({m}) is invalid in attribute PV{axis}_{m} for a \
                     \"-{}\" projection - axis {axis} is neither the longitude ({}) nor the \
                     latitude ({}) axis",
                    self.proj.code(),
                    self.lonax + 1,
                    self.latax + 1,
                ),
            ));
        };

        // Reject a parameter index the projection does not read, as C `SetPV`
        // does (wcsmap.c:3884-3890) — it raises AST__AXIIN naming the
        // projection's ctype and the limit `astGetPVMax` reports.
        let mxpar = self.pv_max(axis).unwrap_or(0);
        if m > mxpar {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "WcsMap set_pv parameter index ({m}) is invalid in attribute \
                     PV{axis}_{m} for a \"-{}\" projection - it should be in the \
                     range 0 to {mxpar}",
                    self.proj.code()
                ),
            ));
        }

        // Grow the parameter vector if needed.
        let pv = &mut self.params[prj_axis];
        if m >= pv.len() {
            pv.resize(m + 1, None);
        }
        // AST__BAD is C's unset value, so storing it unsets the parameter
        // (wcsmap.c `TestPV` tests for it).
        pv[m] = (value != BAD_VALUE).then_some(value);

        // Rebuild the wcslib parameter arrays with defaults reapplied and the
        // fwd/rev pointers reinstalled (C SetPV → InitPrjPrm, wcsmap.c:3939).
        self.prj
            .borrow_mut()
            .init_prj_prm(&self.params, self.tpn_tan());

        Ok(())
    }

    /// Return the FITS-WCS projection type.
    pub fn projection(&self) -> WcsProjection {
        self.proj
    }

    /// The value of the projection parameter `m` (0-based) of axis `axis`
    /// (1-based), as C's `astGetPV` gives it (wcsmap.c `GetPV`).
    ///
    /// A latitude parameter not set takes its default, which is bad for the
    /// conic and Bonne projections' PV2_1; a longitude parameter not set is
    /// bad, except that, other than for TPN, PVi_0 defaults to 0 and PVi_1
    /// and PVi_2 to the native longitude and latitude of the reference point
    /// in degrees. An axis index outside the Mapping, or a parameter index
    /// beyond [`Self::pv_max`] for the axis, is an `AST__AXIIN` error; an axis
    /// that is neither the longitude nor the latitude axis has no parameters.
    pub fn get_pv(&self, axis: usize, m: usize) -> AstResult<f64> {
        if axis == 0 || axis > self.ncoord {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "WcsMap: axis index ({axis}) is invalid in PV{axis}_{m}; it should be in \
                     the range 1 to {}",
                    self.ncoord
                ),
            ));
        }
        if self.pv_max(axis).is_none_or(|max| m > max) {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "WcsMap: parameter index ({m}) is invalid in PV{axis}_{m} for a \
                     \"{}\" projection",
                    self.proj.code()
                ),
            ));
        }
        let mut value = if axis - 1 == self.latax {
            self.prj.borrow().get_param(0, m).unwrap_or(BAD_VALUE)
        } else {
            self.params[1]
                .get(m)
                .copied()
                .flatten()
                .unwrap_or(BAD_VALUE)
        };
        if value == BAD_VALUE && axis - 1 == self.lonax && self.proj != WcsProjection::Tpn {
            value = match m {
                0 => 0.0,
                1 => self.nat_lon() * DR2D,
                2 => self.nat_lat() * DR2D,
                _ => BAD_VALUE,
            };
        }
        Ok(value)
    }

    /// Whether the projection is zenithal for the purpose of FITS-WCS
    /// LONPOLE and LATPOLE (wcsmap.c `IsZenithal`): TAN, STG, ARC, ZPN, ZEA,
    /// AIR and TPN always, AZP when PV2_2 is zero, SIN when PV2_1 and PV2_2
    /// are, and SZP according to its latitude parameter 2.
    ///
    /// The SZP test reads parameter 2, the native longitude of the source of
    /// projection, and compares it with +/-90, where its comment and the
    /// projection's geometry point to parameter 3, the native latitude, which
    /// defaults to 90; the port mirrors C (`docs/issues/c-library-quirks.md`,
    /// "`IsZenithal` tests the wrong SZP parameter").
    pub(crate) fn is_zenithal(&self) -> bool {
        let lat = |m: usize| self.get_pv(self.latax + 1, m).unwrap_or(BAD_VALUE);
        let or = |v: f64, default: f64| if v == BAD_VALUE { default } else { v };
        match self.proj {
            WcsProjection::Tan
            | WcsProjection::Stg
            | WcsProjection::Arc
            | WcsProjection::Zpn
            | WcsProjection::Zea
            | WcsProjection::Air
            | WcsProjection::Tpn => true,
            WcsProjection::Azp => or(lat(2), 0.0) == 0.0,
            WcsProjection::Sin => or(lat(1), 0.0) == 0.0 && or(lat(2), 0.0) == 0.0,
            WcsProjection::Szp => {
                let p = or(lat(2), 90.0);
                p == 90.0 || p == -90.0
            }
            _ => false,
        }
    }

    /// Unset projection parameter `m` of axis `axis` (1-based), as C's
    /// `astClearPV` does: an axis out of range is `ErrorCode::Axiin`, and a
    /// parameter the axis does not use is ignored.
    pub fn clear_pv(&mut self, axis: usize, m: usize) -> AstResult<()> {
        if axis == 0 || axis > self.ncoord {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "WcsMap: axis index ({axis}) is invalid in attribute PV{axis}_{m} - it \
                     should be in the range 1 to {}",
                    self.ncoord
                ),
            ));
        }
        let prj_axis = if axis - 1 == self.latax {
            Some(0)
        } else if axis - 1 == self.lonax {
            Some(1)
        } else {
            None
        };
        if let Some(prj_axis) = prj_axis
            && m <= self.pv_max(axis).unwrap_or(0)
            && let Some(slot) = self.params[prj_axis].get_mut(m)
        {
            *slot = None;
        }
        self.prj
            .borrow_mut()
            .init_prj_prm(&self.params, self.tpn_tan());
        Ok(())
    }

    /// Whether projection parameter `m` of axis `axis` (1-based) has been
    /// set, as C's `astTestPV` reports.
    pub fn test_pv(&self, axis: usize, m: usize) -> bool {
        self.stored_pv(axis, m).is_some()
    }

    /// The value set for projection parameter `m` of axis `axis` (1-based),
    /// or `None` when it is unset or the axis has no parameters.
    pub(crate) fn stored_pv(&self, axis: usize, m: usize) -> Option<f64> {
        let prj_axis = if axis == 0 || axis > self.ncoord {
            return None;
        } else if axis - 1 == self.latax {
            0
        } else if axis - 1 == self.lonax {
            1
        } else {
            return None;
        };
        self.params[prj_axis].get(m).copied().flatten()
    }

    /// Set the protected `LonCheck` attribute (wcsmap.c:4787). When `false`,
    /// the inverse projection accepts longitude values outside the
    /// projection's primary range instead of setting them bad. Mirrors C
    /// `astSetLonCheck`, whose `astMAKE_SET` stores `value != 0`.
    pub fn set_lon_check(&mut self, value: bool) {
        // `LonCheck` changes what the inverse projection returns for a
        // longitude outside the projection's primary range (wcsmap.c:2805), so a
        // conclusion an earlier simplification reached about this WcsMap no
        // longer holds. WcsMap's own MapMerge does not read the attribute, but C
        // discards the records on every setter it covers, because doing so costs
        // only a redundant simplification (wcsmap.c:4825-4828, the convention at
        // mapping.h:449-472). The clear is guarded on the value differing, as C's
        // is -- see `LutMap::set_interp` for why that guard matters to the
        // loader.
        if Some(value) != self.lon_check {
            self.simplified.clear();
        }
        self.lon_check = Some(value);
    }

    /// Return the effective `LonCheck` value (default `true` when unset),
    /// mirroring C `astGetLonCheck` (wcsmap.c:4818).
    pub fn lon_check(&self) -> bool {
        self.lon_check.unwrap_or(true)
    }

    /// Whether `LonCheck` has been explicitly set, mirroring C
    /// `astTestLonCheck` (wcsmap.c:4821).
    pub fn test_lon_check(&self) -> bool {
        self.lon_check.is_some()
    }

    /// Unset `LonCheck`, so that it reads as `true` (wcsmap.c `ClearLonCheck`,
    /// which also discards both simplified records).
    pub fn clear_lon_check(&mut self) {
        self.simplified.clear();
        self.lon_check = None;
    }

    /// Set the protected `FITSProj` attribute, mirroring C `astSetFITSProj`
    /// (wcsmap.c:4744). Clear it on a WcsMap that only supplies a distortion
    /// correction, so the FitsChan write path does not mistake it for the
    /// header's FITS-WCS projection.
    pub fn set_fits_proj(&mut self, value: bool) {
        // As for `set_lon_check` (wcsmap.c:4750-4753).
        if Some(value) != self.fits_proj {
            self.simplified.clear();
        }
        self.fits_proj = Some(value);
    }

    /// Return the effective `FITSProj` value (default `true` when unset),
    /// mirroring C `astGetFITSProj` (wcsmap.c:4742).
    pub fn fits_proj(&self) -> bool {
        self.fits_proj.unwrap_or(true)
    }

    /// Whether `FITSProj` has been explicitly set, mirroring C
    /// `astTestFITSProj` (wcsmap.c:4746).
    pub fn test_fits_proj(&self) -> bool {
        self.fits_proj.is_some()
    }

    /// Unset `FITSProj`, so that it reads as `true` (wcsmap.c
    /// `ClearFITSProj`, which also discards both simplified records).
    pub fn clear_fits_proj(&mut self) {
        self.simplified.clear();
        self.fits_proj = None;
    }

    /// Set the protected `TPNTan` attribute, mirroring C `astSetTPNTan`
    /// (wcsmap.c:3942). Clearing it makes a TPN projection apply only its
    /// polynomial correction, omitting the TAN projection itself. Like C, the
    /// change re-initialises the wcslib parameter block, which is what tells
    /// `astTPNfwd`/`astTPNrev` which form to use.
    pub fn set_tpn_tan(&mut self, value: bool) {
        self.tpn_tan = Some(value);
        self.prj
            .borrow_mut()
            .init_prj_prm(&self.params, self.tpn_tan());
    }

    /// Return the effective `TPNTan` value (default `true` when unset),
    /// mirroring C `astGetTPNTan` (wcsmap.c:4780).
    pub fn tpn_tan(&self) -> bool {
        self.tpn_tan.unwrap_or(true)
    }

    /// Whether `TPNTan` has been explicitly set, mirroring C `astTestTPNTan`
    /// (wcsmap.c:4782).
    pub fn test_tpn_tan(&self) -> bool {
        self.tpn_tan.is_some()
    }

    /// Unset `TPNTan`, so that it reads as `true`, and re-initialise the
    /// wcslib parameter block, as C's `ClearTPNTan` does.
    pub fn clear_tpn_tan(&mut self) {
        self.tpn_tan = None;
        self.prj
            .borrow_mut()
            .init_prj_prm(&self.params, self.tpn_tan());
    }

    /// Determine the acceptable longitude range and cyclicity for the
    /// inverse (xy->sky) transformation. Port of `LongRange`
    /// (wcsmap.c:2597).
    ///
    /// Returns `(cyclic, longhi, longlo)`. `cyclic` is true when
    /// `[a,0]` and `[a+2.PI,0]` map to the same Cartesian position (so the
    /// out-of-range longitude test can be skipped). `longhi`/`longlo` bound
    /// the primary longitude range the projection produces — either
    /// `[-180,180]` or `[0,360]` — used to reject inverse solutions that
    /// fall outside the physical domain of the projection.
    fn long_range(&self, prj: &mut PrjParams) -> (bool, f64, f64) {
        // Sample points near the projection origin (in degrees).
        const XX: [f64; 4] = [-1.0e-6, 0.0, 1.0e-6, 0.0];
        const YY: [f64; 4] = [0.0, 1.0e-6, 0.0, -1.0e-6];

        let mut high = 180.0;
        let mut low = -180.0;

        // Project each sample. If any inverse longitude exceeds 180 degrees,
        // the projection uses the [0,360] range instead of [-180,180]. `bb`
        // carries the latitude of the final sample forward to the cyclic
        // test, exactly as in the C code.
        let mut bb = 0.0;
        for point in 0..4 {
            let (status, aa, b) = prj.reverse_raw(XX[point], YY[point]);
            bb = b;
            if status == 0 && aa > 180.0 {
                high = 360.0;
                low = 0.0;
                break;
            }
        }

        // See if the projection is cyclic: transform sky positions
        // [90,bb] and [450,bb] into Cartesian and compare them.
        let (_s0, x0, y0) = prj.forward_raw(90.0, bb);
        let (_s1, x1, y1) = prj.forward_raw(450.0, bb);
        let cyclic = (x0 - x1).abs() < 1.0e-10 && (y0 - y1).abs() < 1.0e-10;

        (cyclic, high, low)
    }

    /// Return the 1-based longitude axis index.
    pub fn lonax(&self) -> usize {
        self.lonax + 1
    }

    /// Return the 1-based latitude axis index.
    pub fn latax(&self) -> usize {
        self.latax + 1
    }

    /// Native longitude of the fiducial point (radians).
    ///
    /// For zenithal projections (TAN, SIN, STG, ARC, etc.) this is 0.
    /// The underlying wcslib struct stores this in degrees; this method
    /// converts to radians (the AST convention).
    pub fn nat_lon(&self) -> f64 {
        // An Unknown (AST__WCSBAD) projection has no real reference point;
        // C `GetNatLon` is a fixed 0 for every projection type (wcsmap.c),
        // so return 0 explicitly rather than relying on the inert CAR
        // placeholder's reference point.
        if self.proj == WcsProjection::Unknown {
            return 0.0;
        }
        let (phi0, _theta0) = self.prj.borrow().reference_point();
        phi0.to_radians()
    }

    /// Native latitude of the fiducial point (radians).
    ///
    /// For zenithal projections (TAN, SIN, STG, ARC, etc.) this is pi/2.
    /// For cylindrical projections (CAR, MER, CEA, etc.) this is 0.
    /// The underlying wcslib struct stores this in degrees; this method
    /// converts to radians (the AST convention).
    pub fn nat_lat(&self) -> f64 {
        // An Unknown (AST__WCSBAD) projection has no real reference point; C
        // `GetNatLat` reads `FindPrjData(AST__WCSBAD)->theta0`, which is 0
        // (wcsmap.c:716). Return 0 explicitly rather than relying on the inert
        // CAR placeholder (whose reference point is also 0, but only by
        // coincidence).
        if self.proj == WcsProjection::Unknown {
            return 0.0;
        }
        // The conics have no fixed native latitude (C's table holds
        // AST__BAD, wcsmap.c:721-724): it is projection parameter 1 on the
        // latitude axis, through `astGetPV`, so bad when that is unset.
        if matches!(
            self.proj,
            WcsProjection::Cop | WcsProjection::Coe | WcsProjection::Cod | WcsProjection::Coo
        ) {
            let pv = self.get_pv(self.latax + 1, 1).unwrap_or(BAD_VALUE);
            return if pv == BAD_VALUE {
                BAD_VALUE
            } else {
                pv * DD2R
            };
        }
        let (_phi0, theta0) = self.prj.borrow().reference_point();
        theta0.to_radians()
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }

    /// C has no projection functions for NCP, so transforming through an NCP
    /// WcsMap is an `AST__WCSTY` error (wcsmap.c `Map` returns 1, which
    /// `Transform` reports). A FITS header's NCP projection is read as SIN
    /// with parameters, so only a WcsMap made as NCP reaches this.
    fn check_transformable(&self) -> AstResult<()> {
        if self.proj == WcsProjection::Ncp {
            return Err(AstError::new(
                ErrorCode::Wcsty,
                "WcsMap: the AIPS north celestial pole (NCP) projection cannot \
                 transform coordinates",
            ));
        }
        // PV2_1 of the conic and Bonne projections has no default, so C's
        // `Map` refuses to transform without it (wcsmap.c InitPrjPrm keeps it
        // bad, and `Map` returns 401, which `Transform` reports).
        let undefaultable = matches!(
            self.proj,
            WcsProjection::Bon
                | WcsProjection::Cop
                | WcsProjection::Coe
                | WcsProjection::Cod
                | WcsProjection::Coo
        );
        let pv1 = self.params[0].get(1).copied().flatten();
        if undefaultable && pv1.is_none_or(|v| v == BAD_VALUE) {
            return Err(AstError::new(
                ErrorCode::Wcspa,
                format!(
                    "WcsMap: required projection parameter PV{}_1 was not supplied for a \
                     {} projection",
                    self.latax + 1,
                    self.proj.code()
                ),
            ));
        }
        Ok(())
    }

    /// Rebuild PrjParams from stored parameters (used by clone and simplified).
    fn rebuild_prj(&self) -> AstResult<PrjParams> {
        let underlying = match self.proj {
            WcsProjection::Ncp => WcsProjection::Sin,
            WcsProjection::Gls => WcsProjection::Sfl,
            // Unknown (AST__WCSBAD) has no wcslib projection. The stored
            // PrjParams is never consulted (transform is identity, simplify is
            // a UnitMap, nat_lon/nat_lat return 0), so CAR is an inert
            // placeholder kept only so this field is populated.
            WcsProjection::Unknown => WcsProjection::Car,
            other => other,
        };
        let mut prj = PrjParams::new(underlying)?;
        prj.init_prj_prm(&self.params, self.tpn_tan());
        Ok(prj)
    }

    /// Create a copy of this WcsMap with different axis assignments.
    ///
    /// Used by the simplify engine to absorb a PermMap into a WcsMap
    /// by remapping which axes carry the longitude/latitude.
    /// `new_lonax` and `new_latax` are 0-based.
    pub(crate) fn with_permuted_axes(
        &self,
        new_ncoord: usize,
        new_lonax: usize,
        new_latax: usize,
    ) -> Option<Self> {
        if new_lonax == new_latax || new_lonax >= new_ncoord || new_latax >= new_ncoord {
            return None;
        }
        let prj = self.rebuild_prj().ok()?;
        Some(Self {
            proj: self.proj,
            lonax: new_lonax,
            latax: new_latax,
            ncoord: new_ncoord,
            params: self.params.clone(),
            prj: RefCell::new(prj),
            inverted: self.inverted,
            simplified: self.simplified,
            // C builds a fresh WcsMap for this rebuild (wcsmap.c:4667), and a
            // fresh Object has no Ident.
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
            lon_check: self.lon_check,
            fits_proj: self.fits_proj,
            tpn_tan: self.tpn_tan,
        })
    }

    /// 0-based longitude axis index (crate-internal).
    pub(crate) fn lonax_0based(&self) -> usize {
        self.lonax
    }

    /// 0-based latitude axis index (crate-internal).
    pub(crate) fn latax_0based(&self) -> usize {
        self.latax
    }

    /// Whether each projection parameter is unset in both WcsMaps or set in
    /// both to equal values, as wcsmap.c `Equal` compares them (fixed
    /// upstream in 47826767): an unset parameter is AST__BAD, so it differs
    /// from one set to zero.
    fn params_equal(&self, other: &WcsMap) -> bool {
        for axis in 0..2 {
            let p1 = self.params.get(axis).map(|v| v.as_slice()).unwrap_or(&[]);
            let p2 = other.params.get(axis).map(|v| v.as_slice()).unwrap_or(&[]);
            let max_len = p1.len().max(p2.len());
            for m in 0..max_len {
                let v1 = p1.get(m).copied().flatten().unwrap_or(BAD_VALUE);
                let v2 = p2.get(m).copied().flatten().unwrap_or(BAD_VALUE);
                if !ast_equal(v1, v2) {
                    return false;
                }
            }
        }
        true
    }
}

impl Clone for WcsMap {
    fn clone(&self) -> Self {
        let prj = self
            .rebuild_prj()
            .expect("WcsMap::clone: failed to rebuild PrjParams");
        Self {
            proj: self.proj,
            lonax: self.lonax,
            latax: self.latax,
            ncoord: self.ncoord,
            params: self.params.clone(),
            prj: RefCell::new(prj),
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            mapping_flags: self.mapping_flags.clone(),
            lon_check: self.lon_check,
            fits_proj: self.fits_proj,
            tpn_tan: self.tpn_tan,
        }
    }
}

impl Mapping for WcsMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        self.ncoord
    }

    fn nout(&self) -> usize {
        self.ncoord
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let mut out = PointSet::zeros(pts.npoint(), self.ncoord);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. Same logic as `transform_into`, but the
    /// is_bad branch and projection-`Err` branch emit `f64::NAN` instead
    /// of `BAD_VALUE` so downstream NaN-aware stages receive the protocol-
    /// canonical sentinel rather than a finite -1.8e308 that would
    /// produce numerical garbage through their arithmetic.
    ///
    /// The is_bad gate at the top is *kept* because the underlying
    /// projection routines (`wcslib`-port `prj.forward`/`reverse`) don't
    /// handle `BAD_VALUE` or `NaN` inputs gracefully — they may return
    /// Err, NaN, or garbage depending on which trig path they take. Gating
    /// here is cheap (one comparison per point per axis) and keeps the
    /// projection math on the well-tested finite-input path.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        self.check_transformable()?;
        if self.proj == WcsProjection::Unknown {
            for i in 0..pts.npoint() {
                out.point_mut(i).copy_from_slice(pts.point(i));
            }
            return Ok(());
        }

        let fwd = self.effective_forward(forward);
        let mut prj = self.prj.borrow_mut();

        // Reverse-path longitude clipping gate — identical to
        // `transform_into` (the C `Map` function has a single body; this
        // NaN-as-bad variant is a Rust-side optimization and must apply the
        // same domain check). See `long_range` and wcsmap.c:2878.
        let (docheck, cyclic, longhi, longlo) = if fwd {
            (false, false, 0.0, 0.0)
        } else {
            let (cyclic, longhi, longlo) = self.long_range(&mut prj);
            (self.lon_check(), cyclic, longhi, longlo)
        };

        // The WcsMap input and output values are normally in radians, but if
        // TPNTan has been cleared they are in degrees; WCSLIB always works in
        // degrees. `factor` scales the WcsMap input into radians
        // (wcsmap.c:2808).
        let factor = if self.tpn_tan() { 1.0 } else { DD2R };

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            outp.copy_from_slice(inp);

            let v1 = inp[self.lonax];
            let v2 = inp[self.latax];

            if is_bad(v1) || is_bad(v2) {
                outp[self.lonax] = f64::NAN;
                outp[self.latax] = f64::NAN;
                continue;
            }

            if fwd {
                // Wrap the native longitude/latitude into the ranges WCSLIB
                // requires, folding out-of-range latitudes onto the
                // complementary meridian (wcsmap.c:2820-2839).
                let (lon_deg, lat_deg) = forward_prep(v1, v2, factor);
                match wcslib_result(prj.forward_raw(lon_deg, lat_deg))? {
                    Some((x, y)) => {
                        outp[self.lonax] = (DD2R / factor) * x;
                        outp[self.latax] = (DD2R / factor) * y;
                    }
                    None => {
                        outp[self.lonax] = f64::NAN;
                        outp[self.latax] = f64::NAN;
                    }
                }
            } else {
                let x_deg = (DR2D * factor) * v1;
                let y_deg = (DR2D * factor) * v2;
                match wcslib_result(prj.reverse_raw(x_deg, y_deg))? {
                    Some((mut longitude, mut latitude)) => {
                        // Absorb a one-ULP rounding error at a limit of the
                        // accepted range (wcsmap.c, upstream 460efce3). A WinMap
                        // used by a FITS CAR mapping evaluates an expression such
                        // as `-0.5 degree + 181 * 0.5 degree` as exactly 90
                        // degrees where the compiler contracts the multiply and
                        // add into a fused operation, and as one representable
                        // double above 90 where it does not -- so without this the
                        // validity of a header depends on the compiler and the
                        // target. That value describes the same valid pole. Do not
                        // admit the next representable value, which differs by
                        // more than 90*EPSILON. Both latitude limits need it: the
                        // accepted range is closed, so -90 and +90 are inside it.
                        let mut abslat = latitude.abs();
                        if abslat > 90.0 && abslat - 90.0 <= 90.0 * f64::EPSILON {
                            latitude = if latitude < 0.0 { -90.0 } else { 90.0 };
                            abslat = 90.0;
                        }
                        // The accepted longitude range is half-open,
                        // [longlo,longhi), so only its lower limit needs the same
                        // treatment. Perturbing a longitude strictly below longhi
                        // by one ULP leaves it strictly below; a value at or above
                        // longhi is the wrap of longlo and is clipped
                        // deliberately, to stop a whole-sky grid mapping two
                        // pixels to the same meridian. `long_range` returns either
                        // [-180,180) or [0,360), so the width of the range is the
                        // scale at which a longitude computed within it rounds.
                        if docheck
                            && !cyclic
                            && longitude < longlo
                            && longlo - longitude <= (longhi - longlo) * f64::EPSILON
                        {
                            longitude = longlo;
                        }
                        if (!docheck || cyclic || (longitude < longhi && longitude >= longlo))
                            && abslat <= 90.0
                        {
                            outp[self.lonax] = (DD2R / factor) * longitude;
                            outp[self.latax] = (DD2R / factor) * latitude;
                        } else {
                            outp[self.lonax] = f64::NAN;
                            outp[self.latax] = f64::NAN;
                        }
                    }
                    None => {
                        outp[self.lonax] = f64::NAN;
                        outp[self.latax] = f64::NAN;
                    }
                }
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        self.check_transformable()?;
        // Unknown/blank projection acts as identity — pass through all coords.
        if self.proj == WcsProjection::Unknown {
            for i in 0..pts.npoint() {
                out.point_mut(i).copy_from_slice(pts.point(i));
            }
            return Ok(());
        }

        let fwd = self.effective_forward(forward);
        let mut prj = self.prj.borrow_mut();

        // For the inverse (xy->sky) transformation, determine whether
        // out-of-range longitude solutions should be rejected. C computes
        // `docheck` from the LonCheck attribute and `cyclic`/`longhi`/
        // `longlo` from LongRange (wcsmap.c:2797-2802). The gate is only
        // consulted on the reverse path.
        let (docheck, cyclic, longhi, longlo) = if fwd {
            (false, false, 0.0, 0.0)
        } else {
            let (cyclic, longhi, longlo) = self.long_range(&mut prj);
            (self.lon_check(), cyclic, longhi, longlo)
        };

        // The WcsMap input and output values are normally in radians, but if
        // TPNTan has been cleared they are in degrees; WCSLIB always works in
        // degrees. `factor` scales the WcsMap input into radians
        // (wcsmap.c:2808).
        let factor = if self.tpn_tan() { 1.0 } else { DD2R };

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);

            // Copy all axes through first.
            outp.copy_from_slice(inp);

            let v1 = inp[self.lonax];
            let v2 = inp[self.latax];

            if is_bad(v1) || is_bad(v2) {
                outp[self.lonax] = BAD_VALUE;
                outp[self.latax] = BAD_VALUE;
                continue;
            }

            if fwd {
                // Forward: native spherical (lon, lat in radians) -> projected (x, y).
                // Wrap the native longitude/latitude into the ranges WCSLIB
                // requires, folding out-of-range latitudes onto the
                // complementary meridian (wcsmap.c:2820-2839).
                let (lon_deg, lat_deg) = forward_prep(v1, v2, factor);
                match wcslib_result(prj.forward_raw(lon_deg, lat_deg))? {
                    Some((x, y)) => {
                        outp[self.lonax] = (DD2R / factor) * x;
                        outp[self.latax] = (DD2R / factor) * y;
                    }
                    None => {
                        outp[self.lonax] = BAD_VALUE;
                        outp[self.latax] = BAD_VALUE;
                    }
                }
            } else {
                // Inverse: projected (x, y) -> native spherical (lon, lat in radians)
                let x_deg = (DR2D * factor) * v1;
                let y_deg = (DR2D * factor) * v2;
                match wcslib_result(prj.reverse_raw(x_deg, y_deg))? {
                    Some((mut longitude, mut latitude)) => {
                        // Reject solutions outside the physical domain of the
                        // projection (wcsmap.c:2878). Non-cyclic projections
                        // (ARC, AIT, ZPN, ...) only accept longitudes within
                        // the primary range; the latitude must be within
                        // [-90,90]. `longitude`/`latitude` are in degrees here.
                        //
                        // Absorb a one-ULP rounding error at a limit of that
                        // range first (upstream 460efce3). A WinMap used by a
                        // FITS CAR mapping evaluates `-0.5 degree + 181 * 0.5
                        // degree` as exactly 90 degrees where the compiler fuses
                        // the multiply and add, and one representable double above
                        // 90 where it does not, so otherwise a header's validity
                        // depends on the compiler. That value describes the same
                        // valid pole. The next representable value is not
                        // admitted: it differs by more than 90*EPSILON. Both
                        // latitude limits need it, the range being closed.
                        let mut abslat = latitude.abs();
                        if abslat > 90.0 && abslat - 90.0 <= 90.0 * f64::EPSILON {
                            latitude = if latitude < 0.0 { -90.0 } else { 90.0 };
                            abslat = 90.0;
                        }
                        // The longitude range is half-open, [longlo,longhi), so
                        // only its lower limit needs this: perturbing a value
                        // strictly below longhi leaves it strictly below, while a
                        // value at or above longhi is the wrap of longlo and is
                        // clipped deliberately, to stop a whole-sky grid mapping
                        // two pixels to the same meridian.
                        if docheck
                            && !cyclic
                            && longitude < longlo
                            && longlo - longitude <= (longhi - longlo) * f64::EPSILON
                        {
                            longitude = longlo;
                        }
                        if (!docheck || cyclic || (longitude < longhi && longitude >= longlo))
                            && abslat <= 90.0
                        {
                            outp[self.lonax] = (DD2R / factor) * longitude;
                            outp[self.latax] = (DD2R / factor) * latitude;
                        } else {
                            outp[self.lonax] = BAD_VALUE;
                            outp[self.latax] = BAD_VALUE;
                        }
                    }
                    None => {
                        outp[self.lonax] = BAD_VALUE;
                        outp[self.latax] = BAD_VALUE;
                    }
                }
            }
        }
        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C allocates the returned output indices with astMalloc, which
        // gives NULL for none, so an empty selection gives no split.
        if in_axes.is_empty() {
            return Ok(None);
        }
        // Mirrors wcsmap.c::MapSplit (~3342). C first defers to the parent
        // (basic-Mapping) MapSplit, which only succeeds when every input is
        // selected (`nin == mapnin`). If that fails, the WcsMap-specific
        // override runs and handles partial input selections by case:
        //
        //   - both celestial axes kept       -> smaller WcsMap
        //   - neither celestial axis kept    -> UnitMap
        //   - only one celestial axis kept   -> PermMap that drops that axis
        //                                       (yields BAD on inverse) — only
        //                                       valid when nin > 1
        //
        // An input out of range is AST__AXIIN from the parent's MapSplit. A
        // repeated input is classified once per occurrence, as in C, so a
        // repeated celestial axis counts as that axis alone.
        let nin = in_axes.len();

        // Full-set path: delegate to the shared helper, which mirrors C's
        // parent (basic-Mapping) MapSplit. For an identity permutation it
        // returns `self`; for any other permutation it builds PermMap+self
        // and simplifies (the simplifier may absorb the perm back into a
        // single WcsMap with permuted axes).
        if let Some(result) = crate::mappings::helpers::base_map_split(self, in_axes)? {
            return Ok(Some(result));
        }

        // Partial-set path: WcsMap-specific override.
        let lonax = self.lonax;
        let latax = self.latax;
        let mut ilon: Option<usize> = None;
        let mut ilat: Option<usize> = None;
        let mut ilatlon: usize = 0;
        let mut result: Vec<usize> = Vec::with_capacity(nin);
        for (i, &iin) in in_axes.iter().enumerate() {
            if iin == lonax {
                ilon = Some(i);
                ilatlon = i;
            } else if iin == latax {
                ilat = Some(i);
                ilatlon = i;
            }
            result.push(iin);
        }

        match (ilon, ilat) {
            // Both celestial axes selected: returned mapping is a smaller
            // WcsMap with the same projection / params and Invert flag, with
            // the longitude/latitude axes remapped to their positions in the
            // kept-input ordering. Mirrors C's astWcsMap+CopyPV+SetInvert.
            (Some(ilon), Some(ilat)) => {
                let Some(mut new_wcs) = self.with_permuted_axes(nin, ilon, ilat) else {
                    return Ok(None);
                };
                // C sets the new WcsMap's Invert explicitly.
                new_wcs.set_invert(self.inverted);
                Ok(Some((result, Box::new(new_wcs))))
            }
            // Neither celestial axis selected: returned mapping is a UnitMap
            // over the kept pass-through axes. Mirrors C's astUnitMap branch.
            (None, None) => Ok(Some((result, Box::new(UnitMap::new(nin))))),
            // Only one celestial axis selected: drop it. The C code treats
            // this as nin > 1 only — for nin == 1 (the lone kept input is
            // the celestial), it frees the result and returns NULL.
            _ => {
                if nin <= 1 {
                    return Ok(None);
                }
                // Drop the celestial position from `result` (length becomes
                // nin - 1, matching the new mapping's nout).
                result.remove(ilatlon);

                // Build a PermMap with `nin` inputs and `nin - 1` outputs.
                // Forward: skip the celestial axis at position `ilatlon`.
                // Inverse: yield BAD at position `ilatlon` (C: INT_MAX, which
                // AST C's PermMap treats as out-of-range -> AST__BAD; our
                // PermMap encodes BAD with a 0 entry).
                let mut inperm: Vec<i64> = vec![0; nin];
                let mut outperm: Vec<i64> = vec![0; nin - 1];
                for i in 0..ilatlon {
                    inperm[i] = (i + 1) as i64;
                    outperm[i] = (i + 1) as i64;
                }
                inperm[ilatlon] = 0; // BAD on inverse for the dropped axis
                for i in (ilatlon + 1)..nin {
                    inperm[i] = i as i64;
                    outperm[i - 1] = (i + 1) as i64;
                }
                let pm = PermMap::new(&inperm, &outperm, &[])?;
                Ok(Some((result, Box::new(pm))))
            }
        }
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::simplify::wcs_map_merge(self, ctx, where_, series)
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        // Only handle inverse cancellation: same WcsMap in opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_wcs = other.as_any().downcast_ref::<WcsMap>()?;
        if self.proj == other_wcs.proj
            && self.lonax == other_wcs.lonax
            && self.latax == other_wcs.latax
            && self.ncoord == other_wcs.ncoord
            && self.params_equal(other_wcs)
        {
            // WcsMap always has nin == nout == ncoord.
            Some(Box::new(UnitMap::new(self.ncoord)))
        } else {
            None
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<WcsMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        self.proj == other.proj
            && self.lonax == other.lonax
            && self.latax == other.latax
            && self.ncoord == other.ncoord
            && self.params_equal(other)
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

/// Return the human-readable description for a projection type (for Native
/// format comments).
fn projection_description(proj: WcsProjection) -> &'static str {
    // Verbatim from the C projection table (wcsmap.c:685-716); the native
    // dump comment is "<desc> projection".
    match proj {
        WcsProjection::Azp => "zenithal perspective",
        WcsProjection::Szp => "slant zenithal perspective",
        WcsProjection::Tan => "gnomonic",
        WcsProjection::Stg => "stereographic",
        WcsProjection::Sin => "orthographic",
        WcsProjection::Arc => "zenithal equidistant",
        WcsProjection::Zpn => "zenithal polynomial",
        WcsProjection::Zea => "zenithal equal area",
        WcsProjection::Air => "Airy",
        WcsProjection::Cyp => "cylindrical perspective",
        WcsProjection::Cea => "cylindrical equal area",
        WcsProjection::Car => "Cartesian",
        WcsProjection::Mer => "Mercator",
        WcsProjection::Sfl => "Sanson-Flamsteed",
        WcsProjection::Par => "parabolic",
        WcsProjection::Mol => "Mollweide",
        WcsProjection::Ait => "Hammer-Aitoff",
        WcsProjection::Cop => "conical perspective",
        WcsProjection::Coe => "conical equal area",
        WcsProjection::Cod => "conical equidistant",
        WcsProjection::Coo => "conical orthomorphic",
        WcsProjection::Bon => "Bonne's equal area",
        WcsProjection::Pco => "polyconic",
        WcsProjection::Tsc => "tangential spherical cube",
        WcsProjection::Csc => "cobe quadrilateralized spherical cube",
        WcsProjection::Qsc => "quadrilateralized spherical cube",
        WcsProjection::Ncp => "AIPS north celestial pole",
        WcsProjection::Gls => "sinusoidal",
        WcsProjection::Tpn => "gnomonic polynomial",
        WcsProjection::Hpx => "HEALPix",
        WcsProjection::Xph => "polar HEALPix",
        WcsProjection::Unknown => "<null>",
    }
}

impl Serializable for WcsMap {
    fn class_name(&self) -> &'static str {
        "WcsMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("FITS-WCS sky projection".to_string());

        // Mapping base attributes.
        write_mapping_base(&mut node, self);

        // Type — 3-letter FITS projection code, always written. The comment
        // is "<desc> projection" with the first character upper-cased,
        // mirroring C `WcsMap::Dump` (wcsmap.c:5325-5326): the projection
        // descriptions themselves are lower-case in the C table.
        let descr = projection_description(self.proj);
        let mut comment = format!("{descr} projection");
        if let Some(first) = comment.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        node.push_string("Type", self.proj.code(), Some(comment));

        // FITSProj — written with `helpful = 0` (wcsmap.c:5333), so only an
        // explicitly set value appears. The comment reflects the value.
        if self.test_fits_proj() {
            let val = self.fits_proj();
            let comment = if val {
                "Defines the FITS-WCS projection"
            } else {
                "Does not define the FITS-WCS projection"
            };
            node.push_int("FitsPrj", val as i64, Some(comment.to_string()));
        }

        // LonCheck — written with `helpful = 0` (wcsmap.c:5341), so only an
        // explicitly set value appears (as an active card); an unset value is
        // suppressed. The comment reflects the value.
        if self.test_lon_check() {
            let val = self.lon_check();
            let comment = if val {
                "Check returned lon values"
            } else {
                "Do not check returned lon values"
            };
            node.push_int("LonChk", val as i64, Some(comment.to_string()));
        }

        // TPNTan — written with `helpful = 0` (wcsmap.c:5349), so only an
        // explicitly set value appears. The comment reflects the value.
        if self.test_tpn_tan() {
            let val = self.tpn_tan();
            let comment = if val {
                "Include TAN projection in TPN mapping"
            } else {
                "Exclude TAN projection from TPN mapping"
            };
            node.push_int("TpnTan", val as i64, Some(comment.to_string()));
        }

        // PV parameters. C `WcsMap::Dump` (wcsmap.c:5355) loops over the
        // internal axis index `i` in 0..nin and emits `PV{i+1}_{m}` for each,
        // so the cards come out in *axis* order (PV1_* before PV2_*). The
        // params are stored here by projection role — `params[0]` =
        // latitude axis, `params[1]` = longitude axis — so iterate axes in
        // index order and select the matching role's params for each.
        for axis in 0..self.ncoord {
            let pv = if axis == self.latax {
                Some(&self.params[0])
            } else if axis == self.lonax {
                Some(&self.params[1])
            } else {
                None
            };
            let Some(pv) = pv else { continue };
            let axis_1based = axis + 1;
            for (m, val) in pv.iter().enumerate() {
                if let Some(v) = val {
                    node.push_double(
                        &format!("PV{axis_1based}_{m}"),
                        *v,
                        Some(format!("Projection parameter {m} for axis {axis_1based}")),
                    );
                }
            }
        }

        // WcsAx1 / WcsAx2 — only written when non-default.
        if self.lonax != 0 {
            node.push_int(
                "WcsAx1",
                (self.lonax + 1) as i64,
                Some("Index of celestial longitude axis".to_string()),
            );
        }
        if self.latax != 1 {
            node.push_int(
                "WcsAx2",
                (self.latax + 1) as i64,
                Some("Index of celestial latitude axis".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("WcsMap");
        let base = read_mapping_base(whole);

        // Read projection type.
        // If the type is blank or unrecognised, fall back to Unknown (which
        // acts as identity and simplifies to UnitMap).
        let type_str = node.get_string("Type", "");
        let proj = WcsProjection::from_code(type_str.trim()).unwrap_or(WcsProjection::Unknown);

        // Read axis indices (1-based, defaults 1 and 2).
        let wcsax1 = node.get_int("WcsAx1", 1) as usize;
        let wcsax2 = node.get_int("WcsAx2", 2) as usize;

        let mut wcs = WcsMap::new(base.nin, proj, wcsax1, wcsax2)?;

        // `astLoadMapping` installs the record from the `IsSimp` card before any
        // class attribute is read (mapping.c:24137), so the setters below -- which
        // follow C's clear-on-set convention -- discard it. The record is keyed
        // on the orientation the loaded Mapping ends up in, which the `Invert`
        // card has already supplied.
        wcs.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);

        // FITSProj / LonCheck / TPNTan — each is only present when explicitly
        // set (helpful=0 in the dump). C reads all three (wcsmap.c:6016-6034)
        // before InitPrjPrm and the PV values, so TPNTan is in place by the
        // time the wcslib parameter block is built.
        // C assigns the structure component and only then re-applies it through
        // the setter, so the set changes nothing and the setter's guarded
        // `astClearIsSimple` does not fire (wcsmap.c:6016-6023). Seeding the
        // field first is what lets the `IsSimp` card the parent loader installed
        // survive.
        if node.get("FitsPrj").is_some() {
            let value = node.get_int("FitsPrj", 1) != 0;
            wcs.fits_proj = Some(value);
            wcs.set_fits_proj(value);
        }
        if node.get("LonChk").is_some() {
            let value = node.get_int("LonChk", 1) != 0;
            wcs.lon_check = Some(value);
            wcs.set_lon_check(value);
        }
        if node.get("TpnTan").is_some() {
            wcs.set_tpn_tan(node.get_int("TpnTan", 1) != 0);
        }

        // Dumps written before the PVi_m keys hold the latitude axis's
        // projection parameters as ProjP<m> (wcsmap.c Load). They are read
        // first, so a PVi_m key for the same parameter takes precedence.
        for m in 0..AST_WCSMX {
            let value = node.get_double(&format!("ProjP{m}"), BAD_VALUE);
            if value != BAD_VALUE {
                wcs.set_pv(wcs.latax(), m, value)?;
            }
        }

        // Scan items for PV{i}_{m} keys.
        for item in &node.items {
            if let Some(rest) = item.key.strip_prefix("PV") {
                if let Some((axis_str, m_str)) = rest.split_once('_') {
                    if let (Ok(axis), Ok(m)) = (axis_str.parse::<usize>(), m_str.parse::<usize>()) {
                        let value = match &item.value {
                            crate::serial::NodeValue::Double(v) => *v,
                            crate::serial::NodeValue::Int(v) => *v as f64,
                            _ => continue,
                        };
                        wcs.set_pv(axis, m, value)?;
                    }
                }
            }
        }

        if base.inverted {
            wcs.invert();
        }

        Ok(wcs)
    }
}
