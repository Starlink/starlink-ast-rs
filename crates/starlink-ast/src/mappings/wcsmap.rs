// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! FITS-WCS sky projection mapping.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};
use crate::wcslib::WcsProjection;
use crate::wcslib::prj::{Prj, Status};

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

/// The projection a WcsMap of type `proj` performs, with the parameters
/// `params` and the `TPNTan` value `tpn_tan`, set up.
///
/// NCP and GLS are performed by SIN and SFL with the parameters a WcsMap
/// gives them. Unknown (AST__WCSBAD) has no projection: its WcsMap is the
/// identity, simplifies to a UnitMap and has a native latitude and longitude
/// of 0, so it never consults the projection, and CAR is an inert placeholder.
fn build_prj(proj: WcsProjection, params: &[Vec<Option<f64>>], tpn_tan: bool) -> Prj {
    let underlying = match proj {
        WcsProjection::Ncp => WcsProjection::Sin,
        WcsProjection::Gls => WcsProjection::Sfl,
        WcsProjection::Unknown => WcsProjection::Car,
        other => other,
    };
    Prj::new(underlying, params, tpn_tan)
        .expect("Prj::new accepts every projection but NCP, GLS and Unknown")
}

/// Implement a FITS-WCS sky projection.
///
/// A WcsMap represents the sky coordinate projections described in the FITS
/// world coordinate system (FITS-WCS) paper II, "Representations of Celestial
/// Coordinates in FITS" by M. Calabretta and E.W. Greisen.
/// That paper defines a set of functions, or sky projections, which transform
/// longitude-latitude pairs representing spherical celestial coordinates into
/// pairs of Cartesian coordinates, and back.
///
/// A WcsMap applies one of these projections to a specified pair of its
/// coordinates.
/// All the projections in the FITS-WCS paper are supported (see
/// [`WcsProjection`]), plus the deprecated "TAN with polynomial correction
/// terms" projection, TPN.
/// In FITS-WCS terms, the forward transformation converts "native spherical"
/// coordinates into "projection plane" coordinates (also called "intermediate
/// world coordinates"), and the inverse converts back; inverting the WcsMap
/// reverses this.
/// The two coordinates may be embedded in a space of more than two
/// dimensions, the other coordinates being copied unchanged.
/// For consistency with the rest of the library, a WcsMap handles angles in
/// radians, not the degrees used by FITS-WCS.
/// A point that cannot be transformed (many projections do not cover the
/// whole celestial sphere) transforms to [`BAD_VALUE`].
///
/// The projection type and the axes it applies to are fixed when the WcsMap
/// is created, and are given by [`projection`](WcsMap::projection),
/// [`lonax`](WcsMap::lonax) and [`latax`](WcsMap::latax).
///
/// Each of the two axes can also carry projection parameters, which specify
/// the precise form of the projection; see [`get_pv`](WcsMap::get_pv).
/// The parameters a projection needs, and their meanings, depend on its type:
/// most projections need none, or use parameters 1 and 2 of the latitude axis.
/// Consult the FITS-WCS paper for which parameters are required and which
/// have defaults, and set every required parameter that has no default with
/// [`set_pv`](WcsMap::set_pv) before transforming.
/// Transforming is an error (`ErrorCode::Wcspa`) if a required parameter is
/// not set or the parameters do not satisfy the paper's constraints, and
/// (`ErrorCode::Wcsty`) for an NCP projection, which has no transformation of
/// its own.
///
/// [`nat_lon`](WcsMap::nat_lon) and [`nat_lat`](WcsMap::nat_lat) give the
/// native coordinates of the projection's reference point.
///
/// # Example
///
/// ```
/// use starlink_ast::mappings::{Mapping, WcsMap, WcsProjection};
/// use starlink_ast::coords::PointSet;
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
/// assert!((back.axis(0)[0] - lon).abs() < 1e-10);
/// assert!((back.axis(1)[0] - lat).abs() < 1e-10);
/// ```
#[derive(Debug)]
pub struct WcsMap {
    /// The projection type requested at construction time.
    ///
    /// For NCP and GLS this stores the original deprecated code, while
    /// the underlying `Prj` uses the equivalent SIN or SFL.
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
    /// The projection, set up from `params` and `tpn_tan`; rebuilt whenever
    /// either changes. Boxed because its parameter arrays are large.
    prj: Box<Prj>,
    /// Whether the mapping is inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
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
    /// Create a WcsMap that applies the FITS-WCS projection `proj` to axes
    /// `lonax` (longitude) and `latax` (latitude) of an `ncoord`-dimensional
    /// space.
    ///
    /// `ncoord` is the number of coordinates of both input and output points,
    /// and must be at least 2; coordinates other than `lonax` and `latax` are
    /// copied unchanged.
    /// `lonax` and `latax` are 1-based and distinct.
    ///
    /// Projection parameters are set afterwards with
    /// [`set_pv`](WcsMap::set_pv); their validity is not checked until the
    /// WcsMap transforms coordinates.
    /// A conical equal-area projection on axes 2 and 3 of a 3-dimensional
    /// space, for instance, needs parameter 1 of the latitude axis (FITS
    /// keyword `PV3_1`), which has no default:
    ///
    /// ```
    /// use starlink_ast::mappings::{WcsMap, WcsProjection};
    ///
    /// let mut map = WcsMap::new(3, WcsProjection::Coe, 2, 3).unwrap();
    /// map.set_pv(3, 1, 40.0).unwrap();
    /// assert_eq!(map.get_pv(3, 1).unwrap(), 40.0);
    /// // A conic's reference point lies at the latitude PV3_1 gives.
    /// assert!((map.nat_lat() - 40f64.to_radians()).abs() < 1e-15);
    /// ```
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Wcsnc` if `ncoord` is less than 2.
    /// - `ErrorCode::Wcsax` if `lonax` or `latax` is not in `1..=ncoord`, or
    ///   they are equal.
    #[doc(alias = "astWcsMap")]
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

        // Initialize empty parameter arrays for 2 axes (lat=0, lon=1).
        let params = vec![Vec::new(), Vec::new()];

        // Assign default projection parameters (C astInitWcsMap_ → InitPrjPrm).
        // NCP and GLS are performed by SIN and SFL, and an Unknown WcsMap,
        // the identity, holds an inert placeholder (see `build_prj`).
        let prj = Box::new(build_prj(proj, &params, true));

        Ok(Self {
            proj,
            lonax: lonax - 1,
            latax: latax - 1,
            ncoord,
            params,
            prj,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            lon_check: None,
            fits_proj: None,
            tpn_tan: None,
        })
    }

    /// The largest legal projection parameter index `m` for the 1-based axis
    /// `axis`, as used by [`get_pv`](WcsMap::get_pv), or `None` when the axis
    /// is neither the longitude nor the latitude axis.
    ///
    /// The limits are fixed by the projection type the WcsMap was created
    /// with.
    #[doc(alias = "PVMax")]
    pub fn pv_max(&self, axis: usize) -> Option<usize> {
        // Mirrors astGetPVMax (wcsmap.c:2152-2217), which reads the limit off
        // the size of the per-axis parameter array — allocated from the
        // projection table's mxpar / mxpar2 columns — and returns -1 for any
        // other axis. The lookup uses the declared projection type, as C's
        // FindPrjData( this->type ) does: NCP and GLS keep their own limits
        // even though they transform through SIN and SFL.
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

    /// Set projection parameter `m` of the 1-based axis `axis`; see
    /// [`get_pv`](WcsMap::get_pv).
    ///
    /// [`BAD_VALUE`] leaves the parameter unset.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Axiin` if `axis` is not in `1..=ncoord`, is neither the
    /// longitude nor the latitude axis, or `m` is greater than
    /// [`pv_max`](WcsMap::pv_max) for the axis.
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
        self.reinit_prj();

        Ok(())
    }

    /// The type of FITS-WCS projection the WcsMap performs.
    ///
    /// Each [`WcsProjection`] corresponds to the 3-character code the FITS-WCS
    /// `CTYPEi` keyword uses to identify the projection, such as `TAN` (the
    /// tangent plane or gnomonic projection) or `AIT` (Hammer-Aitoff).
    /// [`WcsProjection::Tpn`] is an exception: it is not part of the FITS-WCS
    /// standard, and represents a TAN projection with polynomial correction
    /// terms as defined in an early draft of the FITS-WCS paper.
    /// See the FITS-WCS paper for the available projections.
    ///
    /// It is fixed when the WcsMap is created.
    #[doc(alias = "WcsType")]
    pub fn projection(&self) -> WcsProjection {
        self.proj
    }

    /// The value of projection parameter `m` of the 1-based axis `axis`, the
    /// FITS-WCS `PVi_m` value with `i` = `axis`.
    ///
    /// The parameters correspond exactly to the values of the FITS-WCS
    /// keywords `PV1_1`, `PV1_2` and so on, so a parameter that is an angle is
    /// in degrees, although the coordinates and other angles a WcsMap uses
    /// are in radians.
    /// The parameters a WcsMap uses depend on its
    /// [`projection`](WcsMap::projection); most projections use none, or
    /// parameters 1 and 2 of the latitude axis.
    /// `m` runs from 0 to [`pv_max`](WcsMap::pv_max) for the axis.
    /// The `ProjP(m)` parameters of earlier versions of FITS-WCS, for `m`
    /// from 0 to 9, are parameters `m` of the latitude axis.
    ///
    /// A latitude-axis parameter that has not been set takes the default the
    /// FITS-WCS paper defines for it.
    /// Parameter 1 of the conic and Bonne projections has no default and
    /// reads as [`BAD_VALUE`]; any other parameter with no default in the
    /// paper, including one the projection does not use, reads as zero.
    /// A longitude-axis parameter that has not been set reads as
    /// [`BAD_VALUE`], except as follows.
    /// The paper reserves parameters 1 and 2 of the longitude axis for the
    /// native longitude and latitude of the projection's fiducial point, in
    /// degrees; unset, they default to [`nat_lon`](WcsMap::nat_lon) and
    /// [`nat_lat`](WcsMap::nat_lat) in degrees, and parameter 0 to 0.
    /// The TPN projection does not follow this convention: all its
    /// parameters on both axes are polynomial correction terms, and the
    /// native longitude and latitude of its fiducial point are fixed at 0 and
    /// 90 degrees.
    ///
    /// Default: as above, from the FITS-WCS paper.
    ///
    /// [`set_pv`](WcsMap::set_pv), [`test_pv`](WcsMap::test_pv) and
    /// [`clear_pv`](WcsMap::clear_pv) set, test and clear it.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Axiin` if `axis` is not in `1..=ncoord`, is neither the
    /// longitude nor the latitude axis, or `m` is greater than
    /// [`pv_max`](WcsMap::pv_max) for the axis.
    #[doc(alias = "PVi_m")]
    pub fn get_pv(&self, axis: usize, m: usize) -> AstResult<f64> {
        // As C's astGetPV (wcsmap.c GetPV).
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
            self.prj.get_param(0, m).unwrap_or(BAD_VALUE)
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

    /// Clear projection parameter `m` of the 1-based axis `axis`, so that it
    /// reads as its default; see [`get_pv`](WcsMap::get_pv).
    ///
    /// A parameter the axis does not use is ignored.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Axiin` if `axis` is not in `1..=ncoord`.
    pub fn clear_pv(&mut self, axis: usize, m: usize) -> AstResult<()> {
        // As C's astClearPV.
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
        self.reinit_prj();
        Ok(())
    }

    /// Whether projection parameter `m` of the 1-based axis `axis` has been
    /// set; see [`get_pv`](WcsMap::get_pv).
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

    /// Set whether the inverse projection checks returned longitudes.
    ///
    /// When `false`, the inverse projection accepts longitude values outside
    /// the projection's primary range instead of setting them bad.
    pub fn set_lon_check(&mut self, value: bool) {
        // The protected LonCheck attribute (wcsmap.c:4787). Mirrors C
        // astSetLonCheck, whose astMAKE_SET stores `value != 0`.
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

    /// Whether the inverse projection sets bad a returned longitude that lies
    /// outside the projection's primary range.
    ///
    /// Default: true.
    pub fn lon_check(&self) -> bool {
        // C astGetLonCheck (wcsmap.c:4818).
        self.lon_check.unwrap_or(true)
    }

    /// Whether [`lon_check`](WcsMap::lon_check) has been set.
    pub fn test_lon_check(&self) -> bool {
        // C astTestLonCheck (wcsmap.c:4821).
        self.lon_check.is_some()
    }

    /// Clear [`lon_check`](WcsMap::lon_check), so that it reads as true.
    ///
    /// This also clears the record that the WcsMap has been simplified.
    pub fn clear_lon_check(&mut self) {
        // wcsmap.c ClearLonCheck, which also discards both simplified records.
        self.simplified.clear();
        self.lon_check = None;
    }

    /// Set whether this WcsMap defines the FITS-WCS projection when a
    /// compound Mapping containing it is written through a FitsChan.
    ///
    /// Set it false on a WcsMap that only supplies a distortion correction,
    /// so that it is not mistaken for the header's FITS-WCS projection.
    pub fn set_fits_proj(&mut self, value: bool) {
        // The protected FITSProj attribute, C astSetFITSProj (wcsmap.c:4744).
        // As for `set_lon_check` (wcsmap.c:4750-4753).
        if Some(value) != self.fits_proj {
            self.simplified.clear();
        }
        self.fits_proj = Some(value);
    }

    /// Whether this WcsMap defines the FITS-WCS projection when written
    /// through a FitsChan; see [`set_fits_proj`](WcsMap::set_fits_proj).
    ///
    /// Default: true.
    pub fn fits_proj(&self) -> bool {
        // C astGetFITSProj (wcsmap.c:4742).
        self.fits_proj.unwrap_or(true)
    }

    /// Whether [`fits_proj`](WcsMap::fits_proj) has been set.
    pub fn test_fits_proj(&self) -> bool {
        // C astTestFITSProj (wcsmap.c:4746).
        self.fits_proj.is_some()
    }

    /// Clear [`fits_proj`](WcsMap::fits_proj), so that it reads as true.
    ///
    /// This also clears the record that the WcsMap has been simplified.
    pub fn clear_fits_proj(&mut self) {
        // wcsmap.c ClearFITSProj, which also discards both simplified records.
        self.simplified.clear();
        self.fits_proj = None;
    }

    /// Set whether a TPN projection includes the TAN projection.
    ///
    /// When `false`, a TPN projection applies only its polynomial
    /// correction, omitting the TAN projection itself.
    pub fn set_tpn_tan(&mut self, value: bool) {
        // The protected TPNTan attribute, C astSetTPNTan (wcsmap.c:3942).
        // Like C, the change re-initializes the wcslib parameter block, which
        // is what tells astTPNfwd/astTPNrev which form to use.
        self.tpn_tan = Some(value);
        self.reinit_prj();
    }

    /// Whether a TPN projection includes the TAN projection as well as its
    /// polynomial correction; see [`set_tpn_tan`](WcsMap::set_tpn_tan).
    ///
    /// Default: true.
    pub fn tpn_tan(&self) -> bool {
        // C astGetTPNTan (wcsmap.c:4780).
        self.tpn_tan.unwrap_or(true)
    }

    /// Whether [`tpn_tan`](WcsMap::tpn_tan) has been set.
    pub fn test_tpn_tan(&self) -> bool {
        // C astTestTPNTan (wcsmap.c:4782).
        self.tpn_tan.is_some()
    }

    /// Clear [`tpn_tan`](WcsMap::tpn_tan), so that it reads as true.
    pub fn clear_tpn_tan(&mut self) {
        // Re-initializes the wcslib parameter block, as C's ClearTPNTan does.
        self.tpn_tan = None;
        self.reinit_prj();
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
    fn long_range(&self) -> (bool, f64, f64) {
        // Sample points near the projection origin (in degrees).
        const XX: [f64; 4] = [-1.0e-6, 0.0, 1.0e-6, 0.0];
        const YY: [f64; 4] = [0.0, 1.0e-6, 0.0, -1.0e-6];

        let mut high = 180.0;
        let mut low = -180.0;

        // Project each sample. If any inverse longitude exceeds 180 degrees,
        // the projection uses the [0,360] range instead of [-180,180]. `bb`
        // carries the latitude of the final sample forward to the cyclic
        // test, exactly as in the C code.
        //
        // C reads the projected values whatever the status, so the raw
        // entries are used, each call counted as C's `WcsRev` and `WcsFwd`
        // calls are. Unusable parameters give status `BadParams` and zero
        // values here, and are reported by the first good point of the
        // transformation.
        let mut bb = 0.0;
        for point in 0..4 {
            crate::call_counts::hit("wcs_rev");
            let (status, aa, b) = self.prj.rev_raw(XX[point], YY[point]);
            bb = b;
            if status == Status::Ok && aa > 180.0 {
                high = 360.0;
                low = 0.0;
                break;
            }
        }

        // See if the projection is cyclic: transform sky positions
        // [90,bb] and [450,bb] into Cartesian and compare them.
        crate::call_counts::hit_n("wcs_fwd", 2);
        let (_s0, x0, y0) = self.prj.fwd_raw(90.0, bb);
        let (_s1, x1, y1) = self.prj.fwd_raw(450.0, bb);
        let cyclic = (x0 - x1).abs() < 1.0e-10 && (y0 - y1).abs() < 1.0e-10;

        (cyclic, high, low)
    }

    /// How the inverse accepts the longitudes its projection gives: whether
    /// it tests them, from the LonCheck attribute and whether the projection
    /// is cyclic, and the range it accepts, worked out before projecting any
    /// point.
    // C computes `docheck`, `longhi` and `longlo` from LonCheck and
    // LongRange at wcsmap.c:2797-2802.
    fn lon_range(&self) -> LonRange {
        let (cyclic, hi, lo) = self.long_range();
        LonRange {
            tested: self.lon_check() && !cyclic,
            lo,
            hi,
            tol: self.proj.table_tol(),
        }
    }

    /// Project the WcsMap's longitude and latitude axes, `lon` and `lat`, in
    /// place: forward when `fwd`, from native spherical coordinates to
    /// projection-plane ones, and inverse otherwise.
    /// A bad input point, a point the projection cannot project and an
    /// inverse solution outside the projection's domain give `bad` in both.
    ///
    /// The slice entry converts each pair to degrees, projects it and
    /// converts it back, applying the inverse's domain checks, in one pass.
    fn project_in_place(
        &self,
        lon: &mut [f64],
        lat: &mut [f64],
        fwd: bool,
        bad: f64,
    ) -> AstResult<()> {
        self.project_in_place_with(lon, lat, fwd, bad, true)
    }

    /// [`project_in_place`](WcsMap::project_in_place), where the inverse
    /// redoes with the per-value projection each point whose longitude from
    /// the vectorized kernels is so near a limit of the accepted range that
    /// the kernels' drift could change whether it is accepted only when
    /// `lon_redo` is set.
    fn project_in_place_with(
        &self,
        lon: &mut [f64],
        lat: &mut [f64],
        fwd: bool,
        bad: f64,
        lon_redo: bool,
    ) -> AstResult<()> {
        let n = lon.len();
        let lat = &mut lat[..n];

        // The WcsMap input and output values are normally in radians, but if
        // TPNTan has been cleared they are in degrees; WCSLIB always works in
        // degrees. `factor` scales the WcsMap input into radians
        // (wcsmap.c:2808).
        let factor = if self.tpn_tan() { 1.0 } else { DD2R };
        // The scale from WCSLIB's degrees to the WcsMap output units.
        let to_output = DD2R / factor;

        if fwd {
            // Forward: native spherical (lon, lat in radians) -> projected
            // (x, y). Wrap the native longitude/latitude into the ranges
            // WCSLIB requires, folding out-of-range latitudes onto the
            // complementary meridian (wcsmap.c:2820-2839).
            return self.prj.fwd_slice(
                lon,
                lat,
                |lon, lat| {
                    if is_bad(lon) || is_bad(lat) {
                        (BAD_VALUE, BAD_VALUE)
                    } else {
                        forward_prep(lon, lat, factor)
                    }
                },
                |x, y| {
                    if x == BAD_VALUE && y == BAD_VALUE {
                        (bad, bad)
                    } else {
                        (x * to_output, y * to_output)
                    }
                },
            );
        }

        // Inverse: projected (x, y) -> native spherical (lon, lat in
        // radians).
        let range = self.lon_range();
        // The scale from the WcsMap input units to WCSLIB's degrees.
        let to_degrees = DR2D * factor;
        let prep = |x: f64, y: f64| {
            if is_bad(x) || is_bad(y) {
                (BAD_VALUE, BAD_VALUE)
            } else {
                (x * to_degrees, y * to_degrees)
            }
        };
        let finish = |longitude: f64, latitude: f64| {
            inverse_finish(&range, to_output, bad, longitude, latitude)
        };
        let redo = |longitude: f64, drift: f64| lon_redo && range.near_limit(longitude, drift);
        self.prj.rev_slice(lon, lat, prep, finish, redo)
    }

    /// The 1-based index of the longitude axis of the projection within the
    /// coordinate space of the WcsMap.
    ///
    /// It is fixed when the WcsMap is created.
    /// [`latax`](WcsMap::latax) gives the latitude axis.
    #[doc(alias = "WcsAxis")]
    pub fn lonax(&self) -> usize {
        self.lonax + 1
    }

    /// The 1-based index of the latitude axis of the projection within the
    /// coordinate space of the WcsMap.
    ///
    /// It is fixed when the WcsMap is created.
    /// [`lonax`](WcsMap::lonax) gives the longitude axis.
    #[doc(alias = "WcsAxis")]
    pub fn latax(&self) -> usize {
        self.latax + 1
    }

    /// The longitude of the reference point of the projection, in radians in
    /// the native spherical coordinate system.
    ///
    /// It is zero for every projection.
    /// See [`nat_lat`](WcsMap::nat_lat).
    #[doc(alias = "NatLon")]
    pub fn nat_lon(&self) -> f64 {
        // C's GetNatLon, which is 0 for every projection (wcsmap.c).
        0.0
    }

    /// The latitude of the reference point of the projection, in radians in
    /// the native spherical coordinate system.
    ///
    /// It is fixed for most projections: pi/2 (90 degrees) for every zenithal
    /// projection (TAN, SIN, STG, ARC and so on) and 0 for the cylindrical
    /// ones (CAR, MER, CEA and so on).
    /// For the conics it is parameter 1 of the latitude axis (see
    /// [`get_pv`](WcsMap::get_pv)), converted to radians, and [`BAD_VALUE`]
    /// while that parameter is unset.
    ///
    /// FITS-WCS paper II distinguishes the projection's reference point from
    /// its fiducial point, the point whose celestial coordinates the `CRVAL`
    /// keywords give.
    /// The native coordinates of the fiducial point default to
    /// [`nat_lon`](WcsMap::nat_lon) and `nat_lat`, but parameters 1 and 2 of
    /// the longitude axis can override them, so by default `CRVAL` gives the
    /// celestial coordinates of the reference point.
    #[doc(alias = "NatLat")]
    pub fn nat_lat(&self) -> f64 {
        // C's GetNatLat: the theta0 column of the PrjInfo table
        // (wcsmap.c:703-737).
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
        self.proj.table_theta0()
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

    /// Set the projection up again from the stored parameters and `TPNTan`,
    /// with defaults reapplied.
    // C's `InitPrjPrm`.
    fn reinit_prj(&mut self) {
        *self.prj = build_prj(self.proj, &self.params, self.tpn_tan());
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
        Some(Self {
            proj: self.proj,
            lonax: new_lonax,
            latax: new_latax,
            ncoord: new_ncoord,
            params: self.params.clone(),
            prj: self.prj.clone(),
            inverted: self.inverted,
            simplified: self.simplified,
            // C builds a fresh WcsMap for this rebuild (wcsmap.c:4667), and a
            // fresh Object has no Ident.
            ident: None,
            id: Default::default(),
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
        Self {
            proj: self.proj,
            lonax: self.lonax,
            latax: self.latax,
            ncoord: self.ncoord,
            params: self.params.clone(),
            prj: self.prj.clone(),
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            id: Default::default(),
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

    /// NaN-as-bad fast path: [`transform_into`](Mapping::transform_into)
    /// with `f64::NAN` for every bad output, so downstream NaN-aware stages
    /// receive the protocol's sentinel rather than a finite -1.8e308 that
    /// would produce numerical garbage through their arithmetic.
    ///
    /// A bad input still never reaches the projection: the projection
    /// routines may return an error, NaN or garbage for one, depending on
    /// which trigonometric path they take.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        self.check_transformable()?;
        if self.proj == WcsProjection::Unknown {
            out.copy_from(&pts);
            return Ok(());
        }
        // C's `Map` has a single body; this NaN-as-bad variant is a
        // Rust-side optimization and applies the same domain checks.
        out.copy_from(&pts);
        let n = pts.npoint();
        let (olon, olat) = out.axis_pair_mut(self.lonax, self.latax);
        self.project_in_place(
            &mut olon[..n],
            &mut olat[..n],
            self.effective_forward(forward),
            f64::NAN,
        )
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        self.check_transformable()?;
        // Unknown/blank projection acts as identity — pass through all coords.
        if self.proj == WcsProjection::Unknown {
            out.copy_from(&pts);
            return Ok(());
        }
        // Every axis passes through, then the longitude and latitude axes are
        // projected in place.
        out.copy_from(&pts);
        let n = pts.npoint();
        let (olon, olat) = out.axis_pair_mut(self.lonax, self.latax);
        self.project_in_place(
            &mut olon[..n],
            &mut olat[..n],
            self.effective_forward(forward),
            BAD_VALUE,
        )
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

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
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
// Serialization
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

        // FITSProj, LonCheck and TPNTan: active when set, otherwise not
        // helpful (wcsmap.c `Dump`).
        let val = self.fits_proj();
        node.write_int(
            "FitsPrj",
            self.test_fits_proj(),
            false,
            i64::from(val),
            if val {
                "Defines the FITS-WCS projection"
            } else {
                "Does not define the FITS-WCS projection"
            },
        );
        let val = self.lon_check();
        node.write_int(
            "LonChk",
            self.test_lon_check(),
            false,
            i64::from(val),
            if val {
                "Check returned lon values"
            } else {
                "Do not check returned lon values"
            },
        );
        let val = self.tpn_tan();
        node.write_int(
            "TpnTan",
            self.test_tpn_tan(),
            false,
            i64::from(val),
            if val {
                "Include TAN projection in TPN mapping"
            } else {
                "Exclude TAN projection from TPN mapping"
            },
        );

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

        // WcsAx1 / WcsAx2, set when not the default axis.
        node.write_int(
            "WcsAx1",
            self.lonax != 0,
            false,
            (self.lonax + 1) as i64,
            "Index of celestial longitude axis",
        );
        node.write_int(
            "WcsAx2",
            self.latax != 1,
            false,
            (self.latax + 1) as i64,
            "Index of celestial latitude axis",
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("WcsMap");
        let base = read_mapping_base(whole);

        // Read projection type.
        // If the type is blank or unrecognized, fall back to Unknown (which
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

        // PVi_m for each axis, m running to the largest index the axis
        // takes, 0 for an axis that is neither longitude nor latitude
        // (wcsmap.c Load).
        for i in 1..=Mapping::nin(&wcs) {
            let mxpar = wcs.pv_max(i).unwrap_or(0);
            for m in 0..=mxpar {
                let value = node.get_double(&format!("PV{i}_{m}"), BAD_VALUE);
                if value != BAD_VALUE {
                    wcs.set_pv(i, m, value)?;
                }
            }
        }

        if base.inverted {
            wcs.invert();
        }

        Ok(wcs)
    }
}

/// How a WcsMap's inverse accepts the native longitude its projection gives.
///
/// Where `tested`, which is where LonCheck is set and the projection is not
/// cyclic, a longitude is accepted in [`lo`, `hi`), which is [-180, 180) or
/// [0, 360), and one below `lo` by at most `tol`, the projection's
/// tolerance, is set to `lo`; any other is rejected. Elsewhere every
/// longitude is accepted.
#[derive(Clone, Copy, Debug)]
struct LonRange {
    tested: bool,
    lo: f64,
    hi: f64,
    tol: f64,
}

impl LonRange {
    /// Whether `longitude` is accepted, and the longitude to store where it
    /// is.
    ///
    /// The accepted range is half-open, so only its lower limit is treated
    /// as wcslib's bounds check treats both: rounding in a reverse
    /// projection can land a point on the boundary, such as the -180
    /// meridian of a pseudo-cylindrical projection, slightly outside it, by
    /// an amount that depends on the projection's arithmetic. Unlike wcslib,
    /// a value at or above `hi` is not set to `hi`: it is the wrap of `lo`
    /// and is clipped deliberately, to stop a whole-sky grid mapping two
    /// pixels to the same meridian.
    // wcslib's bounds check is `prjbchk`.
    #[inline(always)]
    fn decide(&self, longitude: f64) -> (bool, f64) {
        if !self.tested {
            return (true, longitude);
        }
        let snap = longitude < self.lo && self.lo - longitude <= self.tol;
        let longitude = if snap { self.lo } else { longitude };
        (longitude < self.hi && longitude >= self.lo, longitude)
    }

    /// Whether [`decide`](LonRange::decide) could decide differently for a
    /// longitude within `drift` of `longitude`.
    // `decide` decides `longitude < hi` and, below `lo`, `lo - longitude <=
    // tol`; `lo - longitude` is exact where it is near `tol` (Sterbenz for
    // `lo = -180`; a negation for `lo = 0`), so that is `longitude >= lo -
    // tol`, whose limit rounds to the nearest double, at most eps / 2 of it
    // away (eps = 2^-52). Two longitudes within `drift` of each other are
    // decided alike where one is beyond twice `drift` of both limits, plus
    // eps of the limit; twice covers evaluating the test from the kernels'
    // longitude rather than the per-value entry's. A longitude with no
    // drift is the per-value entry's, and is decided alike.
    #[inline(always)]
    fn near_limit(&self, longitude: f64, drift: f64) -> bool {
        if !self.tested || drift == 0.0 {
            return false;
        }
        let near = |limit: f64| {
            let margin = 2.0 * drift + f64::EPSILON * limit.abs();
            // False for a NaN, which is then near.
            let clear = (longitude - limit).abs() > margin;
            !clear
        };
        near(self.hi) || near(self.lo - self.tol)
    }
}

/// The pair a WcsMap's inverse stores for the native (`longitude`,
/// `latitude`) in degrees its projection gives: the pair in the WcsMap's
/// output units, scaled by `to_output`, or (`bad`, `bad`) for a bad point or
/// one outside the projection's domain.
///
/// Non-cyclic projections (ARC, AIT, ZPN, ...) accept only longitudes
/// within the range `range` gives, and every projection accepts only
/// latitudes within [-90, 90]. A latitude a little past either limit, by at
/// most the projection's tolerance, is set to the limit first, and one
/// further past is rejected. This is wcslib's bounds check, and the
/// tolerance also absorbs the one-ULP difference a fused multiply-add makes:
/// a WinMap used by a FITS CAR mapping evaluates `-0.5 + 181 * 0.5` degrees
/// as exactly 90 where the compiler contracts it and 90.00000000000001 where
/// it does not, so the validity of a header does not depend on the target.
/// Both latitude limits need it: the accepted range is closed, so -90 and
/// +90 are inside it.
// C's `Map` in wcsmap.c, from line 2878; wcslib's bounds check is
// `prjbchk`.
#[inline(always)]
fn inverse_finish(
    range: &LonRange,
    to_output: f64,
    bad: f64,
    longitude: f64,
    mut latitude: f64,
) -> (f64, f64) {
    if longitude == BAD_VALUE && latitude == BAD_VALUE {
        return (bad, bad);
    }
    let mut abslat = latitude.abs();
    if abslat > 90.0 && abslat - 90.0 <= range.tol {
        latitude = if latitude < 0.0 { -90.0 } else { 90.0 };
        abslat = 90.0;
    }
    let (accepted, longitude) = range.decide(longitude);
    if accepted && abslat <= 90.0 {
        (to_output * longitude, to_output * latitude)
    } else {
        (bad, bad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WcsMap of `proj` with the latitude-axis parameters `pv`, from `pv[0]`,
    /// on axes 1 and 2.
    fn wcsmap(proj: WcsProjection, pv: &[f64]) -> WcsMap {
        let mut map = WcsMap::new(2, proj, 1, 2).unwrap();
        for (m, &v) in pv.iter().enumerate() {
            map.set_pv(2, m, v).unwrap();
        }
        map
    }

    /// Projected (x, y), in the WcsMap's input units, of native longitudes
    /// at, a few ULP from and a few eighths of the projection's tolerance
    /// from each limit a WcsMap's inverse decides on, at latitudes across
    /// the sphere, as the per-value forward projection gives them.
    fn near_limit_inputs(map: &WcsMap) -> Vec<(f64, f64)> {
        let tol = map.proj.table_tol();
        let factor = if map.tpn_tan() { 1.0 } else { DD2R };
        let mut out = Vec::new();
        for limit in [180.0, -180.0 - tol, 360.0, -tol, -180.0] {
            for t in 0..200 {
                let theta = -79.5 + 159.0 * f64::from(t) / 199.0;
                for k in -40i64..=40 {
                    for phi in [
                        limit + k as f64 * tol / 8.0,
                        f64::from_bits(limit.to_bits().wrapping_add_signed(k)),
                    ] {
                        if let (Status::Ok, x, y) = map.prj.fwd_raw(phi, theta) {
                            out.push((x / (DR2D * factor), y / (DR2D * factor)));
                        }
                    }
                }
            }
        }
        out
    }

    /// The inverse of `map` at `pairs` through its slice entry, with the
    /// longitude redo on or off.
    fn inverse_slice(map: &WcsMap, pairs: &[(f64, f64)], lon_redo: bool) -> Vec<(f64, f64)> {
        let mut lon: Vec<f64> = pairs.iter().map(|q| q.0).collect();
        let mut lat: Vec<f64> = pairs.iter().map(|q| q.1).collect();
        map.project_in_place_with(&mut lon, &mut lat, false, BAD_VALUE, lon_redo)
            .unwrap();
        lon.into_iter().zip(lat).collect()
    }

    /// The inverse of `map` at `pairs` one point at a time through the
    /// per-value projection, which keeps wcslib's bits, and the same
    /// [`inverse_finish`] the slice entry stores with.
    fn inverse_per_point(map: &WcsMap, pairs: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let range = map.lon_range();
        let factor = if map.tpn_tan() { 1.0 } else { DD2R };
        let (to_degrees, to_output) = (DR2D * factor, DD2R / factor);
        pairs
            .iter()
            .map(
                |&(x, y)| match map.prj.rev_raw(x * to_degrees, y * to_degrees) {
                    (Status::Ok, lon, lat) => {
                        inverse_finish(&range, to_output, BAD_VALUE, lon, lat)
                    }
                    _ => (BAD_VALUE, BAD_VALUE),
                },
            )
            .collect()
    }

    /// How many pairs the slice entry and the per-value path give different
    /// bits for, and how many of them the one gives as bad and the other not.
    fn mismatches(got: &[(f64, f64)], want: &[(f64, f64)]) -> (usize, usize) {
        let mut differ = 0;
        let mut bad_or_good = 0;
        for (g, w) in got.iter().zip(want) {
            if g.0.to_bits() != w.0.to_bits() || g.1.to_bits() != w.1.to_bits() {
                differ += 1;
                bad_or_good += usize::from((g.0 == BAD_VALUE) != (w.0 == BAD_VALUE));
            }
        }
        (differ, bad_or_good)
    }

    /// The WcsMaps whose inverse projections' longitudes come from the
    /// vectorized kernels and are tested against their range: COO and BON
    /// with LonCheck set.
    fn tested_maps() -> Vec<WcsMap> {
        vec![
            wcsmap(WcsProjection::Coo, &[0.0, 45.0, 10.0]),
            wcsmap(WcsProjection::Coo, &[0.0, -30.0, 15.0]),
            wcsmap(WcsProjection::Bon, &[0.0, 45.0]),
            wcsmap(WcsProjection::Bon, &[0.0, -30.0]),
        ]
    }

    /// At longitudes at and near the limits of the range a WcsMap's inverse
    /// accepts, the slice entry, whose longitude comes from the kernels,
    /// gives the per-value path's bad points, and its bits wherever the
    /// longitude is near a limit; and with the longitude redo off, as if the
    /// kernels had no drift, it gives a point bad that the per-value path
    /// does not, or the reverse.
    #[test]
    fn inverse_longitudes_near_a_limit_are_decided_as_per_value() {
        let mut flipped_without_redo = 0;
        for map in tested_maps() {
            let code = map.proj.code();
            let range = map.lon_range();
            assert!(range.tested, "{code}: the longitude is not tested");
            let pairs = near_limit_inputs(&map);
            let want = inverse_per_point(&map, &pairs);
            let got = inverse_slice(&map, &pairs, true);
            // Within the smaller of COO's and BON's longitude drifts, 3 * 2^-52
            // of 180 degrees, of a limit, where the kernels' longitude is
            // within twice its drift of it and the point is redone.
            let redone = 3.0 * f64::EPSILON * 180.0;
            let mut near = 0;
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(
                    g.0 == BAD_VALUE,
                    w.0 == BAD_VALUE,
                    "{code}: {:?} gives {g:?} against {w:?}",
                    pairs[i]
                );
                let (_, lon, _) = map.prj.rev_raw(pairs[i].0 * DR2D, pairs[i].1 * DR2D);
                let limits = [range.hi, range.lo - range.tol];
                if limits.iter().any(|&l| (lon - l).abs() <= redone) {
                    near += 1;
                    assert!(
                        g.0.to_bits() == w.0.to_bits() && g.1.to_bits() == w.1.to_bits(),
                        "{code}: {:?} near a limit gives {g:?} against {w:?}",
                        pairs[i]
                    );
                }
            }
            assert!(near > 0, "{code}: no longitude is near a limit");
            let (_, bad_or_good) = mismatches(&inverse_slice(&map, &pairs, false), &want);
            flipped_without_redo += bad_or_good;
        }
        assert!(
            flipped_without_redo > 0,
            "the redo is never needed; is the test reaching the limits?"
        );
    }

    /// SZP's reverse longitude comes from the kernels, but SZP is cyclic for
    /// every parameter set the projection tests use, so a WcsMap's inverse
    /// never tests it.
    #[test]
    fn szp_is_cyclic_so_its_longitude_is_not_tested() {
        for pv in [&[][..], &[0.0, 2.0, 30.0, 60.0], &[0.0, 0.5, 0.0, -30.0]] {
            let map = wcsmap(WcsProjection::Szp, pv);
            assert!(map.long_range().0, "SZP {pv:?} is not cyclic");
            assert!(!map.lon_range().tested, "SZP {pv:?}");
        }
    }
}
