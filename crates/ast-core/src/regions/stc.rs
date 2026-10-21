//! The IVOA Space-Time-Coordinates (STC) Regions (stc.c and its subclasses).
//!
//! An Stc wraps another Region, which it encapsulates, and a list of
//! AstroCoords elements, each a [`KeyMap`] that may hold a name for each axis
//! and Regions describing the value, error, resolution, size and pixel size
//! of a position. The Stc covers exactly the points the encapsulated Region
//! covers. The concrete classes, [`StcResourceProfile`], [`StcSearchLocation`],
//! [`StcCatalogEntryLocation`] and [`StcObsDataLocation`], differ only in
//! their class name, and the last in an observatory position it may hold, so
//! they are one generic type, [`Stc`], with a marker [`StcKind`] naming the
//! class.
//!
//! The encapsulated Region describes an area in the base Frame of the Stc's
//! own FrameSet, which is the Region's current Frame.
//!
//! Negated, Closed, MeshSize and FillFactor are read from the encapsulated
//! Region and set and cleared on both it and the Stc, as stc.c's accessors
//! do. The Stc's own fields, which C's region.c reads directly in
//! `RegOverlay` and `Dump`, are [`Region::raw_negated`] and
//! [`Region::raw_closed`]; an Stc built from a Region starts with the
//! Region's, and they need not agree with the encapsulated Region's after.
//!
//! The per-class methods stc.c gives an Stc, and how they map here:
//!
//! - Architecture, each a trait method: `Transform` (through
//!   `reg_base_mask`, which region.c `BTransform` also
//!   reaches), `Simplify`, `Equal`, `Overlap` and `OverlapX`, `RegBaseBox`,
//!   `RegBaseMesh`, `RegBasePick`, `RegPins`, `GetBounded`, `GetDefUnc`,
//!   `RegSetAttrib` and `RegClearAttrib` (one seam,
//!   [`Region::reg_frame_attrib`]), and `SetRegFS`.
//! - Ceremony: the attribute string dispatch (`RegionClass` is
//!   [`Stc::region_class`]), `GetObjSize`, `ManageLock`, the copy
//!   constructor and destructor, and the dump and load functions.
//!
//! Several of these forward to the encapsulated Region with points or
//! boxes in the Stc's base Frame, where the Region's own methods expect its
//! base Frame (`RegBaseBox`, `RegPins`); the two Frames are the same when the
//! Region's base->current Mapping is a UnitMap, as it is for a Region built
//! in the Frame it describes, and the port forwards as C does.

use std::marker::PhantomData;

use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::keymap::{AstType, KeyMap};
use crate::mappings::helpers::is_bad;
use crate::mappings::{DynMapping, Mapping, UnitMap, ViaSimplify};
use crate::serial::AnyObject;

use super::data::{RegionData, RegionGeometry};
use super::merge_util::mapping_into_region;
use super::{DynRegion, Overlap, PointList, Region};

/// The AstroCoords key for the axis names: a vector of strings, one per
/// axis of the encapsulated Region.
///
/// # See also
///
/// `AST__STCNAME`.
pub const STC_NAME: &str = "Name";
/// The AstroCoords key for the value: a Region giving the value of the
/// position.
///
/// # See also
///
/// `AST__STCVALUE`.
pub const STC_VALUE: &str = "Value";
/// The AstroCoords key for the error: a Region giving the error of the
/// position.
///
/// # See also
///
/// `AST__STCERROR`.
pub const STC_ERROR: &str = "Error";
/// The AstroCoords key for the resolution: a Region giving the resolution
/// of the position.
///
/// # See also
///
/// `AST__STCRES`.
pub const STC_RES: &str = "Resolution";
/// The AstroCoords key for the size: a Region giving the size of the
/// position.
///
/// # See also
///
/// `AST__STCSIZE`.
pub const STC_SIZE: &str = "Size";
/// The AstroCoords key for the pixel size: a Region giving the pixel size
/// of the position.
///
/// # See also
///
/// `AST__STCPIXSZ`.
pub const STC_PIXSZ: &str = "PixSize";

/// The AstroCoords keys that hold Regions, with the comment each is stored
/// with (stc.c `regkey` and `regcom`), in C's order.
const REG_KEYS: [(&str, &str); 5] = [
    (STC_ERROR, "AstroCoords error region"),
    (STC_RES, "AstroCoords resolution region"),
    (STC_SIZE, "AstroCoords size region"),
    (STC_PIXSZ, "AstroCoords pixel size region"),
    (STC_VALUE, "AstroCoords value region"),
];

mod sealed {
    pub trait Sealed {}
}

/// The class of an [`Stc`]: one of [`ResourceProfile`],
/// [`SearchLocation`], [`CatalogEntryLocation`] and [`ObsDataLocation`].
///
/// The trait is sealed; the four markers are its only implementations.
pub trait StcKind: sealed::Sealed + Clone + std::fmt::Debug + Send + 'static {
    /// The class name, such as `"StcResourceProfile"`.
    const CLASS: &'static str;
    /// The comment on the class name in a dump.
    const COMMENT: &'static str;
}

/// The [`StcKind`] of an [`StcResourceProfile`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceProfile;

impl sealed::Sealed for ResourceProfile {}

impl StcKind for ResourceProfile {
    const CLASS: &'static str = "StcResourceProfile";
    const COMMENT: &'static str = "Resource coverage";
}

/// An [`Stc`] describing the coverage of the datasets contained in a VO
/// resource, the IVOA STCResourceProfile.
///
/// It has no attributes or methods beyond those of every [`Stc`].
/// [`StcResourceProfileBuilder`] makes one.
// C: stcresourceprofile.c.
pub type StcResourceProfile = Stc<ResourceProfile>;

/// The builder for an [`StcResourceProfile`]; see [`StcBuilder`].
pub type StcResourceProfileBuilder = StcBuilder<ResourceProfile>;

/// The [`StcKind`] of an [`StcSearchLocation`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchLocation;

impl sealed::Sealed for SearchLocation {}

impl StcKind for SearchLocation {
    const CLASS: &'static str = "StcSearchLocation";
    const COMMENT: &'static str = "Query coverage";
}

/// An [`Stc`] describing the coverage of a VO query, the IVOA
/// SearchLocation.
///
/// It has no attributes or methods beyond those of every [`Stc`].
/// [`StcSearchLocationBuilder`] makes one.
// C: stcsearchlocation.c.
pub type StcSearchLocation = Stc<SearchLocation>;

/// The builder for an [`StcSearchLocation`]; see [`StcBuilder`].
pub type StcSearchLocationBuilder = StcBuilder<SearchLocation>;

/// The [`StcKind`] of an [`StcCatalogEntryLocation`].
#[derive(Debug, Clone, Copy, Default)]
pub struct CatalogEntryLocation;

impl sealed::Sealed for CatalogEntryLocation {}

impl StcKind for CatalogEntryLocation {
    const CLASS: &'static str = "StcCatalogEntryLocation";
    // stccatalogentrylocation.c gives the class the same comment as
    // StcResourceProfile.
    const COMMENT: &'static str = "Resource coverage";
}

/// An [`Stc`] describing the coverage of the datasets contained in a VO
/// resource, the IVOA STCCatalogEntryLocation.
///
/// It has no attributes or methods beyond those of every [`Stc`].
/// [`StcCatalogEntryLocationBuilder`] makes one.
// C: stccatalogentrylocation.c.
pub type StcCatalogEntryLocation = Stc<CatalogEntryLocation>;

/// The builder for an [`StcCatalogEntryLocation`]; see [`StcBuilder`].
pub type StcCatalogEntryLocationBuilder = StcBuilder<CatalogEntryLocation>;

/// The [`StcKind`] of an [`StcObsDataLocation`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ObsDataLocation;

impl sealed::Sealed for ObsDataLocation {}

impl StcKind for ObsDataLocation {
    const CLASS: &'static str = "StcObsDataLocation";
    const COMMENT: &'static str = "Observation coverage";
}

/// An [`Stc`] describing the coordinate space occupied by an observational
/// dataset, the IVOA ObsDataLocation.
///
/// An STC ObsDataLocation gives the extent of the observation in one
/// coordinate system and the location of the observatory in another.
/// As an [`Stc`], an StcObsDataLocation covers the extent of the
/// observation, not the observatory.
/// The observatory position, if one has been given, is a [`PointList`]
/// that [`obs`](Stc::obs) returns and [`stc_set_obs`](Stc::stc_set_obs)
/// sets; the builder does not take one.
/// For a terrestrial observatory, the
/// [ObsLon](crate::frames::Frame::obs_lon) and
/// [ObsLat](crate::frames::Frame::obs_lat) attributes of the encapsulated
/// Region's Frame can record the position instead, taking the observatory
/// to be at sea level.
///
/// [`StcObsDataLocationBuilder`] makes one.
// C: stcobsdatalocation.c.
pub type StcObsDataLocation = Stc<ObsDataLocation>;

/// The builder for an [`StcObsDataLocation`]; see [`StcBuilder`].
pub type StcObsDataLocationBuilder = StcBuilder<ObsDataLocation>;

/// A Region in the IVOA Space-Time Coordinate (STC) metadata system: a
/// Region it encapsulates, and a list of AstroCoords elements.
///
/// An Stc covers exactly the points of the Region it encapsulates.
/// Each AstroCoords element is a [`KeyMap`] describing a position, with
/// any of the keys [`STC_NAME`] (a name for each axis), [`STC_VALUE`],
/// [`STC_ERROR`], [`STC_RES`], [`STC_SIZE`] and [`STC_PIXSZ`] (each a
/// Region giving that item of the position).
/// The STC classes are described at
/// <http://hea-www.harvard.edu/~arots/nvometa/STC.html>.
///
/// `Stc` is generic over its class, a [`StcKind`], and is used through the
/// aliases [`StcResourceProfile`], [`StcSearchLocation`],
/// [`StcCatalogEntryLocation`] and [`StcObsDataLocation`], each made by the
/// [`StcBuilder`] of its class.
///
/// The encapsulated Region describes an area in the Frame the Stc was made
/// in, the base Frame of its FrameSet.
/// [`get_stc_region`](Self::get_stc_region) returns a copy of it,
/// [`region_class`](Self::region_class) names its class, and
/// [`get_stc_n_coord`](Self::get_stc_n_coord) and
/// [`get_stc_coord`](Self::get_stc_coord) give the AstroCoords elements.
///
/// [Negated](Region::is_negated), [Closed](Region::is_closed),
/// [MeshSize](Region::mesh_size) and [FillFactor](Region::fill_factor) are
/// those of the encapsulated Region: reading one reads the Region's value,
/// and setting or clearing one also sets or clears it on the Region.
/// The Stc is [bounded](Region::is_bounded) if the encapsulated Region is.
/// An Stc has no defining points of its own, so
/// [`region_points`](Region::region_points) returns none.
/// Its default uncertainty is the uncertainty set on the encapsulated
/// Region, if one is, and otherwise the default every Region has.
///
/// A Frame attribute set through the Stc changes the coordinate system it
/// is described in, as for any Region.
/// While the Stc is [Adaptive](Region::is_adaptive), it is remapped into
/// the new coordinate system so that it covers the same area.
/// Otherwise the change is made to both its Frames, and a setting is also
/// made to the encapsulated Region and to the Regions in the AstroCoords
/// elements, and a clearing to the encapsulated Region, so that the
/// numbers that define them keep their values.
/// [`with_integrity`](Self::with_integrity) makes any other change to its
/// Frame.
///
/// # Examples
///
/// ```
/// use ast_core::frames::BasicFrameBuilder;
/// use ast_core::regions::{CircleBuilder, Region, StcResourceProfileBuilder};
///
/// let frame = BasicFrameBuilder::new(2).build()?;
/// let circle = CircleBuilder::new(frame, vec![0.0, 0.0])?.radius(1.0).build()?;
/// let stc = StcResourceProfileBuilder::new(circle).build()?;
/// assert_eq!(stc.region_class(), "Circle");
/// assert_eq!(stc.get_stc_n_coord(), 0);
/// assert!(stc.point_in_region(&[0.5, 0.0])?);
/// assert!(!stc.point_in_region(&[2.0, 0.0])?);
/// # Ok::<(), ast_core::error::AstError>(())
/// ```
#[derive(Debug, Clone)]
pub struct Stc<K: StcKind> {
    /// The Stc's own Region data: its FrameSet, whose base Frame is the
    /// encapsulated Region's current Frame, and its own attribute fields.
    pub(crate) data: RegionData,
    /// The encapsulated Region (C's `region`).
    pub(crate) region: DynRegion,
    /// The AstroCoords elements (C's `coord`).
    pub(crate) coords: Vec<KeyMap>,
    /// The observatory position of an [`StcObsDataLocation`] (C's `obs`),
    /// which only that class can be given.
    pub(crate) obs: Option<PointList>,
    kind: PhantomData<K>,
}

/// The builder for an [`Stc`] of class `K`.
///
/// It is used through the aliases [`StcResourceProfileBuilder`],
/// [`StcSearchLocationBuilder`], [`StcCatalogEntryLocationBuilder`] and
/// [`StcObsDataLocationBuilder`].
pub struct StcBuilder<K: StcKind> {
    region: DynRegion,
    coords: Vec<KeyMap>,
    kind: PhantomData<K>,
}

impl<K: StcKind> StcBuilder<K> {
    /// Start an Stc encapsulating `region`.
    ///
    /// The Stc takes ownership of `region`, so later changes to the value
    /// passed in cannot reach it.
    /// If `region` is itself an Stc, the Stc encapsulates the Region it
    /// encapsulates, mapped into the Frame `region` represents.
    #[doc(alias = "astStcResourceProfile")]
    #[doc(alias = "astStcSearchLocation")]
    #[doc(alias = "astStcCatalogEntryLocation")]
    #[doc(alias = "astStcObsDataLocation")]
    pub fn new(region: impl Region + 'static) -> Self {
        Self::from_dyn(Box::new(region))
    }

    /// [`new`](Self::new) for a Region already boxed.
    pub fn from_dyn(region: DynRegion) -> Self {
        StcBuilder {
            region,
            coords: Vec::new(),
            kind: PhantomData,
        }
    }

    /// Give the Stc its AstroCoords elements, each a KeyMap describing one
    /// STC AstroCoords element.
    ///
    /// Each KeyMap may hold any of the keys [`STC_NAME`], [`STC_VALUE`],
    /// [`STC_ERROR`], [`STC_RES`], [`STC_SIZE`] and [`STC_PIXSZ`], and no
    /// others.
    /// Under [`STC_NAME`] it holds a vector of strings, the name of each
    /// axis of the encapsulated Region.
    /// Under each other key it holds a single Region describing that item
    /// of the position (its value, error, resolution, size or pixel size).
    /// The Region may be in any coordinate system that can be converted to
    /// that of the encapsulated Region; [`build`](Self::build) maps it
    /// into the encapsulated Region's Frame.
    /// Without this call the Stc has no AstroCoords elements.
    pub fn coords(mut self, coords: Vec<KeyMap>) -> Self {
        self.coords = coords;
        self
    }

    /// Build the Stc.
    ///
    /// The Stc takes the Negated, Closed, Adaptive, MeshSize and FillFactor
    /// values set on the encapsulated Region, and its Ident.
    /// Each Region in an AstroCoords element is copied and, where its Frame
    /// differs from the encapsulated Region's, mapped into that Frame and
    /// simplified.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Stckey`] if an AstroCoords element has a key other
    ///   than the six, a number of [`STC_NAME`] values other than the
    ///   number of axes or values that are not strings, more than one value
    ///   or a value that is not a Region under another key, or a Region
    ///   that cannot be converted to the coordinate system of the
    ///   encapsulated Region.
    /// - Passes on the error of mapping a Region into the Frame of the
    ///   encapsulated Region, and of simplifying it.
    // C: stc.c `astInitStc` and `MakeAstroCoordsKeyMap`.
    pub fn build(self) -> AstResult<Stc<K>> {
        let reg: DynRegion = match self.region.stc_region() {
            Some(inner) => {
                let fs = self.region.get_region_frameset();
                let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
                let frm = fs.current().snapshot();
                inner.reg_map_region(map.as_ref(), frm.as_ref())?
            }
            None => self.region.copy_region(),
        };

        let data = RegionData::new(FrameSet::new(reg.get_region_frame()), Vec::new());
        let mut new = Stc::<K> {
            data,
            region: reg.clone_region(),
            coords: Vec::new(),
            obs: None,
            kind: PhantomData,
        };

        // Attributes of the encapsulated Region become the Stc's own.
        new.reg_overlay(reg.as_ref(), true)?;
        if Frame::test_ident(reg.as_ref()) {
            new.data.ident = Some(Frame::ident(reg.as_ref()).to_string());
        }

        // The encapsulated Region's Frame is the Stc's base Frame, so it need
        // not be dumped with it. This follows the overlay so that the Stc
        // does not take the cleared value.
        new.region.set_region_fs_flag(false);

        for coord in &self.coords {
            let made = make_astro_coords_key_map(new.region.as_ref(), coord, K::CLASS)?;
            new.coords.push(made);
        }
        Ok(new)
    }
}

/// A copy of a KeyMap as C's `astCopy` makes one, which copies each Object
/// it holds, and so clears `RegionFS` on each Region in it (region.c `Copy`).
fn copy_key_map(keymap: &KeyMap) -> KeyMap {
    let mut copy = keymap.clone();
    clear_region_fs_in(&mut copy);
    copy
}

/// Clear `RegionFS` on every Region held in `keymap`, as copying them does.
fn clear_region_fs_in(keymap: &mut KeyMap) {
    for obj in keymap.objects_mut() {
        if let Some(region) = obj.as_region_mut() {
            region.clear_region_fs_deep();
        } else if let AnyObject::KeyMap(km) = obj {
            clear_region_fs_in(km);
        }
    }
}

/// The Region stored in `keymap` under `key`, if there is one.
fn key_map_region(keymap: &KeyMap, key: &str) -> AstResult<Option<DynRegion>> {
    Ok(match keymap.get_0a(key)? {
        Some(Some(obj)) => obj.into_region(),
        _ => None,
    })
}

/// Store `region` in `keymap` under `key`, as `astMapPut0A` does.
fn put_region(
    keymap: &mut KeyMap,
    key: &str,
    region: &dyn Region,
    comment: Option<&str>,
) -> AstResult<()> {
    let obj = AnyObject::from_mapping(region as &dyn Mapping).ok_or_else(|| {
        AstError::new(
            ErrorCode::Inter,
            "astMapPut0A(KeyMap): the Region is of a class that cannot be stored",
        )
    })?;
    keymap.put_0a(key, obj, comment)
}

/// Simplify a Region, which always gives a Region.
fn simplify_region(region: &dyn Region) -> AstResult<(DynRegion, bool)> {
    let (simplified, changed) = <dyn Mapping>::simplify_reporting_change(region as &dyn Mapping)?;
    let region = mapping_into_region(simplified).ok_or_else(|| {
        AstError::new(
            ErrorCode::Inter,
            "astSimplify: a Region simplified to something other than a Region",
        )
    })?;
    Ok((region, changed))
}

/// stc.c `MakeAstroCoordsKeyMap`: a copy of the AstroCoords KeyMap `coord`,
/// checked, with each Region it holds mapped into the Frame of `reg` and
/// simplified where the two Frames differ, and marked as not needing its
/// Frame in a dump.
fn make_astro_coords_key_map(reg: &dyn Region, coord: &KeyMap, class: &str) -> AstResult<KeyMap> {
    let stckey = |message: String| AstError::new(ErrorCode::Stckey, message);
    let mut result = copy_key_map(coord);
    let naxes = reg.naxes();
    for j in 0..result.map_size() {
        let key = result.map_key(j)?;
        let nv = result.map_length(&key);
        let ty = result.map_type(&key)?;
        if ![STC_NAME, STC_VALUE, STC_ERROR, STC_RES, STC_SIZE, STC_PIXSZ].contains(&key.as_str()) {
            return Err(stckey(format!(
                "astInitStc({class}): Unknown key \"{key}\" supplied in an AstroCoords list."
            )));
        } else if key == STC_NAME {
            if nv != naxes {
                return Err(stckey(format!(
                    "astInitStc({class}): {nv} \"{key}\" values supplied in an AstroCoords \
                     list, but the Stc has {naxes} axes. "
                )));
            } else if ty != AstType::String {
                return Err(stckey(format!(
                    "astInitStc({class}): The \"{key}\" values supplied in an AstroCoords \
                     list are not character strings. "
                )));
            }
        } else if nv != 1 {
            return Err(stckey(format!(
                "astInitStc({class}): {nv} \"{key}\" values supplied in an AstroCoords list, \
                 but only one is allowed. "
            )));
        } else if ty != AstType::Object {
            return Err(stckey(format!(
                "astInitStc({class}): The \"{key}\" value supplied in an AstroCoords list is \
                 not an AST Object pointer. "
            )));
        } else {
            let obj = match result.get_0a(&key)? {
                Some(Some(obj)) => obj,
                _ => {
                    return Err(stckey(format!(
                        "astInitStc({class}): The \"{key}\" value supplied in an AstroCoords \
                         list is a NULL pointer. "
                    )));
                }
            };
            let obj_class = crate::serial::any_object_to_node(&obj)
                .map(|node| node.class)
                .unwrap_or_default();
            let Some(region) = obj.into_region() else {
                return Err(stckey(format!(
                    "astInitStc({class}): The \"{key}\" value supplied in an AstroCoords list \
                     is a {obj_class}, not a Region. "
                )));
            };
            let Some(fs) = region.convert_to(reg.as_dyn_frame(), &[])? else {
                return Err(stckey(format!(
                    "astInitStc({class}): The \"{key}\" value supplied in an AstroCoords list \
                     cannot be converted to the coordinate system of its parent Stc object."
                )));
            };
            let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
            if !map.as_any().is::<UnitMap>() {
                let frm = fs.current().snapshot();
                let areg = region.reg_map_region(map.as_ref(), frm.as_ref())?;
                let (mut sareg, _) = simplify_region(areg.as_ref())?;
                sareg.set_region_fs_flag(false);
                put_region(&mut result, &key, sareg.as_ref(), None)?;
            } else if let Some(stored) = result.object_mut(&key).and_then(|obj| obj.as_region_mut())
            {
                // The KeyMap holds the Region C changes through its pointer.
                stored.set_region_fs_flag(false);
            }
        }
    }
    Ok(result)
}

/// Give each Region in the AstroCoords KeyMap `keymap` that was loaded with
/// no Frame of its own the Stc's base Frame `frame`, storing it again with its
/// comment, as stc.c `astLoadStc` does.
pub(crate) fn stc_set_coord_frames(keymap: &mut KeyMap, frame: &dyn Frame) -> AstResult<()> {
    for (key, comment) in REG_KEYS {
        if let Some(mut region) = key_map_region(keymap, key)?
            && region.has_dummy_fs()
        {
            region.set_region_fs(frame);
            put_region(keymap, key, region.as_ref(), Some(comment))?;
        }
    }
    Ok(())
}

impl<K: StcKind> Stc<K> {
    /// An Stc from its parts as a dump gives them.
    pub(crate) fn from_parts(
        data: RegionData,
        region: DynRegion,
        coords: Vec<KeyMap>,
        obs: Option<PointList>,
    ) -> Self {
        Stc {
            data,
            region,
            coords,
            obs,
            kind: PhantomData,
        }
    }

    /// A copy of the Region this Stc encapsulates.
    ///
    /// It describes an area in the Frame the Stc was made in, the base Frame
    /// of the Stc's FrameSet.
    /// A Frame attribute set through an [Adaptive](Region::is_adaptive) Stc
    /// changes only the Stc's current Frame, so the copy stays in the
    /// original coordinate system.
    /// On an Stc that is not Adaptive, such a setting is also made to the
    /// encapsulated Region, and the copy has it.
    /// It also carries any change made through the Stc to the attributes it
    /// shares with the encapsulated Region, such as Negated.
    #[doc(alias = "astGetStcRegion")]
    pub fn get_stc_region(&self) -> DynRegion {
        self.region.copy_region()
    }

    /// The RegionClass attribute: the class name of the Region this Stc
    /// encapsulates, such as `"Circle"`.
    ///
    /// It is read-only.
    #[doc(alias = "RegionClass")]
    pub fn region_class(&self) -> &'static str {
        super::class_name(self.region.as_any())
    }

    /// The number of AstroCoords elements this Stc holds.
    #[doc(alias = "astGetStcNCoord")]
    pub fn get_stc_n_coord(&self) -> usize {
        self.coords.len()
    }

    /// A copy of AstroCoords element `icoord`, counting from 1, as a KeyMap
    /// with the keys described by [`StcBuilder::coords`].
    ///
    /// When the Mapping from the Stc's base Frame to its current Frame is
    /// not a unit Mapping, as after System is set on an
    /// [Adaptive](Region::is_adaptive) Stc, the Regions in the KeyMap are
    /// mapped into the current Frame, and the axis names under
    /// [`STC_NAME`] are removed, since they may no longer apply.
    /// Otherwise the KeyMap holds the Regions as they are stored.
    /// Each Region in the KeyMap carries its own Frame when written to a
    /// Channel.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Stcind`] if `icoord` is zero or greater than
    ///   [`get_stc_n_coord`](Self::get_stc_n_coord).
    /// - Passes on the error of mapping a Region into the current
    ///   coordinate system.
    #[doc(alias = "astGetStcCoord")]
    pub fn get_stc_coord(&self, icoord: usize) -> AstResult<KeyMap> {
        let class = K::CLASS;
        let nc = self.coords.len();
        if icoord < 1 || icoord > nc {
            let more = if icoord < 1 {
                "The index of the first AstroCoord element is one, not zero.".to_string()
            } else if nc == 0 {
                format!("There are no AstroCoords elements in the supplied {class}.")
            } else if nc == 1 {
                format!("There is 1 AstroCoords element in the supplied {class}.")
            } else {
                format!("There are {nc} AstroCoords elements in the supplied {class}.")
            };
            return Err(AstError::new(
                ErrorCode::Stcind,
                format!(
                    "astGetStcCoord({class}): Supplied AstroCoords index ({icoord}) is invalid."
                ),
            )
            .with_context(more));
        }

        let mut result = copy_key_map(&self.coords[icoord - 1]);
        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let smap = map.simplify()?;
        let frm = self.data.frameset.current().snapshot();
        let unit = smap.as_any().is::<UnitMap>();
        if !unit {
            result.map_remove(STC_NAME)?;
        }
        for (key, _) in REG_KEYS {
            if let Some(mut reg) = key_map_region(&result, key)? {
                reg.set_region_fs_flag(true);
                let srereg = if unit {
                    reg
                } else {
                    let rereg = reg.reg_map_region(smap.as_ref(), frm.as_ref())?;
                    simplify_region(rereg.as_ref())?.0
                };
                put_region(&mut result, key, srereg.as_ref(), None)?;
            }
        }
        Ok(result)
    }

    /// Make a change to the Frame this Stc represents, keeping the Stc
    /// consistent with it.
    ///
    /// `f` is applied as a Frame attribute set through the Stc is.
    /// While the Stc is [Adaptive](Region::is_adaptive), the change is made
    /// to the Frame it represents, and the Stc is remapped into the new
    /// coordinate system so that it covers the same area.
    /// Otherwise the change is made to both the Frame it represents and the
    /// Frame it was defined in, and to the encapsulated Region and the
    /// Regions in the AstroCoords elements, so the numbers that define them
    /// keep their values.
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
    ///
    /// # Errors
    ///
    /// Passes on the error returned by `f`, and the error of remapping the
    /// Stc.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: Fn(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// The Regions held in the AstroCoords elements, each changed by `f`
    /// where it is stored.
    fn for_each_coord_region(
        &mut self,
        mut f: impl FnMut(&mut dyn Region) -> AstResult<()>,
    ) -> AstResult<()> {
        for keymap in &mut self.coords {
            for (key, _) in REG_KEYS {
                if let Some(region) = keymap.object_mut(key).and_then(|obj| obj.as_region_mut()) {
                    f(region)?;
                }
            }
        }
        Ok(())
    }
}

impl Stc<ObsDataLocation> {
    /// The position of the observatory, if the StcObsDataLocation has one.
    ///
    /// [`stc_set_obs`](Self::stc_set_obs) gives it one.
    pub fn obs(&self) -> Option<&PointList> {
        self.obs.as_ref()
    }

    /// Give the observatory position, or remove it with `None`.
    ///
    /// The position is a [`PointList`] in a coordinate system of its own,
    /// which need not be the Stc's.
    // C: the protected stcobsdatalocation.c `astStcSetObs`.
    pub fn stc_set_obs(&mut self, obs: Option<PointList>) {
        self.obs = obs;
    }
}

// ============================================================================
// RegionGeometry
// ============================================================================

impl<K: StcKind> RegionGeometry for Stc<K> {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    /// Which base-Frame points lie in the un-negated Stc: those the
    /// encapsulated Region, taken un-negated, does not reject.
    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let region = if self.region.is_negated() {
            self.region.negated_copy()
        } else {
            self.region.clone_region()
        };
        let out = region.transform(pts, false)?;
        Ok((0..out.npoint())
            .map(|i| out.point(i).iter().any(|&v| !is_bad(v)))
            .collect())
    }

    /// stc.c `Transform` once the points are in the base Frame: the
    /// encapsulated Region, with its own Negated value, transforms them, and
    /// a point it gives a bad value on every axis is outside.
    fn reg_base_mask(&self, base_pts: &PointSet) -> AstResult<Vec<bool>> {
        let out = self.region.transform(base_pts, false)?;
        Ok((0..out.npoint())
            .map(|i| out.point(i).iter().any(|&v| !is_bad(v)))
            .collect())
    }

    /// stc.c `RegBaseBox`: the encapsulated Region's base-Frame box.
    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.region.region_base_box()
    }

    /// stc.c `RegBaseMesh`: the encapsulated Region's mesh in its current
    /// Frame, which is the Stc's base Frame.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let mesh = self.region.reg_mesh()?;
        Ok((0..mesh.npoint()).map(|i| mesh.point(i).to_vec()).collect())
    }

    /// stc.c `GetBounded`: whether the encapsulated Region is bounded.
    fn is_bounded_inner(&self) -> AstResult<bool> {
        self.region.is_bounded()
    }

    fn remap_defining_points(&mut self, _mapping: &dyn Mapping, _forward: bool) -> AstResult<()> {
        // An Stc has no defining points of its own.
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region
// ============================================================================

impl<K: StcKind> Region for Stc<K> {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        let pts = PointSet::from_rows(std::iter::once(point));
        let out = Mapping::transform(self, &pts, true)?;
        Ok(out.point(0).iter().any(|&v| !is_bad(v)))
    }

    // stc.c `Overlap`: the overlap of the encapsulated Region with `that`.
    fn overlap(&self, that: &dyn Region) -> AstResult<Overlap> {
        self.region.overlap(that)
    }

    // stc.c `OverlapX`: the overlap of the encapsulated Region with `this`,
    // with "inside" turned round, since the Regions are the other way round.
    fn overlap_x(&self, this: &dyn Region) -> AstResult<Overlap> {
        Ok(match self.region.overlap(this)? {
            Overlap::FirstInside => Overlap::SecondInside,
            Overlap::SecondInside => Overlap::FirstInside,
            other => other,
        })
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.negate();
        Box::new(copy)
    }

    fn negate(&mut self) {
        let negated = !self.is_negated();
        self.set_negated(negated);
    }

    /// The Negated attribute of the Region this Stc encapsulates.
    ///
    /// [`Region::is_negated`] describes the attribute.
    // C: stc.c `GetNegated`.
    fn is_negated(&self) -> bool {
        self.region.is_negated()
    }

    fn test_negated(&self) -> bool {
        self.region.test_negated()
    }

    fn set_negated_raw(&mut self, negated: bool, set: bool) {
        self.data.negated = negated;
        self.data.negated_set = set;
        self.data.invalidate_cache();
    }

    /// Set [`Negated`](Region::is_negated) on this Stc and on the Region it
    /// encapsulates.
    // C: stc.c `SetNegated`.
    fn set_negated(&mut self, negated: bool) {
        self.set_negated_raw(negated, true);
        self.region.set_negated(negated);
    }

    fn clear_negated(&mut self) {
        self.set_negated_raw(false, false);
        self.region.clear_negated();
    }

    fn raw_negated(&self) -> (bool, bool) {
        (self.data.negated, self.data.negated_set)
    }

    /// The Closed attribute of the Region this Stc encapsulates.
    ///
    /// [`Region::is_closed`] describes the attribute.
    /// Setting or clearing it on the Stc also sets or clears it on the
    /// encapsulated Region.
    fn is_closed(&self) -> bool {
        self.region.is_closed()
    }

    fn set_closed(&mut self, closed: bool) {
        self.set_closed_raw(closed, true);
        self.region.set_closed(closed);
    }

    fn test_closed(&self) -> bool {
        self.region.test_closed()
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
        self.data.invalidate_cache();
    }

    fn clear_closed(&mut self) {
        self.set_closed_raw(true, false);
        self.region.clear_closed();
    }

    fn raw_closed(&self) -> (bool, bool) {
        (self.data.closed, self.data.closed_set)
    }

    /// The FillFactor attribute of the Region this Stc encapsulates.
    ///
    /// [`Region::fill_factor`] describes the attribute.
    /// Setting or clearing it on the Stc also sets or clears it on the
    /// encapsulated Region.
    fn fill_factor(&self) -> f64 {
        self.region.fill_factor()
    }

    fn set_fill_factor(&mut self, factor: f64) -> AstResult<()> {
        self.data.set_fill_factor(factor)?;
        self.region.set_fill_factor(factor)
    }

    fn test_fill_factor(&self) -> bool {
        self.region.test_fill_factor()
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
        self.region.clear_fill_factor();
    }

    /// The MeshSize attribute of the Region this Stc encapsulates.
    ///
    /// [`Region::mesh_size`] describes the attribute.
    /// Setting or clearing it on the Stc also sets or clears it on the
    /// encapsulated Region.
    fn mesh_size(&self) -> usize {
        self.region.mesh_size()
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
        self.region.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.region.test_mesh_size()
    }

    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
        self.region.clear_mesh_size();
    }

    /// Whether the Region this Stc encapsulates is bounded.
    ///
    /// [`Region::is_bounded`] describes the attribute.
    ///
    /// # Errors
    ///
    /// Passes on the error of the encapsulated Region's test.
    fn is_bounded(&self) -> AstResult<bool> {
        self.is_bounded_inner()
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn test_adaptive(&self) -> bool {
        self.data.adaptive_set
    }

    fn set_adaptive_raw(&mut self, adaptive: bool, set: bool) {
        self.data.adaptive = adaptive;
        self.data.adaptive_set = set;
    }

    fn has_dummy_fs(&self) -> bool {
        self.data.dummy_fs
    }

    // stc.c `SetRegFS`: the Stc takes the Frame, and so does the
    // encapsulated Region if it does not dump its own.
    fn set_region_fs(&mut self, frame: &dyn Frame) {
        self.data.set_region_fs(frame);
        if !self.region.region_fs_flag() {
            self.region.set_region_fs(frame);
        }
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn base_mesh(&self) -> AstResult<PointSet> {
        let mesh = self.reg_base_mesh()?;
        Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())))
    }

    fn base_grid(&self) -> AstResult<PointSet> {
        super::data::region_base_grid(self)
    }

    fn base_points(&self) -> AstResult<PointSet> {
        // An Stc has no defining points of its own.
        let naxes = self.data.frameset.base().naxes();
        Ok(PointSet::zeros(0, naxes))
    }

    fn get_region_frame(&self) -> DynFrame {
        self.data.frameset.current().snapshot()
    }

    fn get_region_frameset(&self) -> FrameSet {
        self.data.frameset.clone()
    }

    fn region_frameset_mut(&mut self) -> &mut FrameSet {
        &mut self.data.frameset
    }

    // stc.c `RegSetAttrib` and `RegClearAttrib`: the change is made to the
    // Stc's own Frames, then its base-Frame form to the encapsulated
    // Region, whose current Frame is that base Frame, and, for a setting
    // but not a clearing, to each AstroCoords Region. A Region without the
    // attribute (AST__BADAT) is passed over, the error annulled.
    fn reg_frame_attrib<'a>(
        &mut self,
        setting: &super::FrameSetting<'a>,
    ) -> AstResult<super::FrameSetting<'a>> {
        let base = super::data::region_frame_attrib(self, setting)?;
        let annul = |made: AstResult<super::FrameSetting<'_>>| match made {
            Err(e) if !super::data::is_bad_attribute(&e) => Err(e),
            _ => Ok(()),
        };
        annul(self.region.reg_frame_attrib(&base))?;
        if !setting.is_clear() {
            self.for_each_coord_region(|region| annul(region.reg_frame_attrib(&base)))?;
        }
        Ok(base)
    }

    fn store_uncertainty(&mut self, unc: Option<DynRegion>) {
        self.data.uncertainty = unc;
        self.data.invalidate_cache();
    }

    fn test_region_fs(&self) -> bool {
        self.data.region_fs_set
    }

    fn set_region_fs_raw(&mut self, value: bool, set: bool) {
        self.data.region_fs = value;
        self.data.region_fs_set = set;
    }

    fn region_fs_flag(&self) -> bool {
        self.data.region_fs
    }

    fn clear_region_fs_deep(&mut self) {
        // C copies the encapsulated Region and the AstroCoords KeyMaps, and
        // so each Region they hold (stc.c `Copy`), and the observatory
        // position (stcobsdatalocation.c `Copy`).
        self.clear_region_fs();
        if let Some(mut unc) = self.get_uncertainty() {
            unc.clear_region_fs_deep();
            self.store_uncertainty(Some(unc));
        }
        self.region.clear_region_fs_deep();
        for keymap in &mut self.coords {
            clear_region_fs_in(keymap);
        }
        if let Some(obs) = self.obs.as_mut() {
            obs.clear_region_fs_deep();
        }
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    // stc.c `GetDefUnc`: the encapsulated Region's uncertainty, where it has
    // one set, in its current Frame, which is the Stc's base Frame;
    // otherwise the Region default.
    fn get_def_unc(&self) -> AstResult<DynRegion> {
        if self.region.test_unc() {
            self.region.get_unc_frm(FrameRef::Current)
        } else {
            self.default_bounding_box_unc()
        }
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    // stc.c `RegBasePick`: the encapsulated Region's.
    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        self.region.reg_base_pick_axes(axes)
    }

    // stc.c `RegPins`: the encapsulated Region's.
    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        self.region.reg_pins_mask(pts, unc)
    }

    fn stc_region(&self) -> Option<&dyn Region> {
        Some(self.region.as_ref())
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame
// ============================================================================

impl<K: StcKind> Frame for Stc<K> {
    region_frame_attrs!(own_use_defs);

    /// The UseDefs attribute: the value set on this Stc, or else that of the
    /// Region it encapsulates.
    // C: stc.c `GetUseDefs`.
    fn use_defs(&self) -> bool {
        self.data.use_defs.unwrap_or_else(|| self.region.use_defs())
    }

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        crate::frames::find_frame::match_with_subclass(
            self.data.frameset.current(),
            target,
            match_sub,
        )
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        self.data.frameset.current().primary_frame(axis)
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.data.frameset.current().format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.data.frameset.current().unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.data.frameset.current().offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.data.frameset.current().norm(coords)
    }

    fn get_axis(&self, axis: usize) -> AxisKind {
        self.data.frameset.current().get_axis(axis)
    }

    // region.c `RemoveRegions`: a Region becomes its Frame.
    fn frame_without_regions(&self) -> Option<DynFrame> {
        Some(self.get_region_frame())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.data.frameset.current().map_from(previous)
    }

    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.data.frameset.current().convert_to(to, domains)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        match other.as_any().downcast_ref::<Stc<K>>() {
            Some(other) => self
                .data
                .frameset
                .current()
                .equal(other.data.frameset.current()),
            None => false,
        }
    }
}

// ============================================================================
// Mapping
// ============================================================================

impl<K: StcKind> Mapping for Stc<K> {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    // A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    // linear.
    fn is_linear(&self) -> bool {
        true
    }

    // region.c `RemoveRegions`: a Region becomes its Frame, which as a
    // Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<DynMapping> {
        Some(Box::new(UnitMap::new(self.nin())))
    }

    // region.c `ReportPoints`: the current Frame formats the values.
    fn report_points(&self, _forward: bool, inp: &PointSet, out: &PointSet) {
        let frame = self.data.frameset.current();
        crate::mappings::report::write_report(&crate::frames::frame_report_lines(
            frame, frame, inp, out,
        ));
    }

    region_mapping_ident!();

    // C's `Frame` overrides `astDoNotSimplify` to zero (frame.c:3511-3554).
    fn do_not_simplify(&self) -> bool {
        false
    }

    fn nin(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn nout(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert_orientation(&mut self) {
        // A Region's forward and inverse transforms are identical.
    }

    // stc.c `Transform`: the input copied, and a point made bad where the
    // encapsulated Region rejects it in the base Frame
    // (`reg_base_mask`).
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts)
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let result = self.transform(&PointSet::from_points(&pts), forward)?;
        out.copy_from(&result.as_points());
        Ok(())
    }

    // stc.c `Simplify`. After the Region simplification, a negated Stc has
    // the negation moved onto the encapsulated Region; the encapsulated
    // Region is mapped into the current Frame where the base->current Mapping
    // is not a UnitMap, and simplified; and so is each AstroCoords Region.
    // The Stc is copied, as C copies it, before the first change.
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        let parent = self.parent_region_simplify()?;
        let mut copied = parent.is_some();
        let mut stc = match parent {
            Some(p) => p
                .as_any()
                .downcast_ref::<Stc<K>>()
                .expect("parent_region_simplify preserves the Region class")
                .clone(),
            None => self.clone(),
        };
        // C's `astCopy` of the Stc, made once, before the first change.
        let copy = |stc: &mut Stc<K>, copied: &mut bool| {
            if !*copied {
                stc.clear_region_fs_deep();
                *copied = true;
            }
        };

        if stc.is_negated() {
            copy(&mut stc, &mut copied);
            stc.negate();
            stc.region.negate();
        }

        let map = stc
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let frm = stc.data.frameset.current().snapshot();
        let unit = map.as_any().is::<UnitMap>();

        // Simplifying a Region mapped into the current Frame always gives a
        // Region other than the encapsulated one.
        let simplify_in_current = |region: &dyn Region| -> AstResult<(DynRegion, bool)> {
            if unit {
                simplify_region(region)
            } else {
                let treg = region.reg_map_region(map.as_ref(), frm.as_ref())?;
                Ok((simplify_region(treg.as_ref())?.0, true))
            }
        };

        let (newreg, changed) = simplify_in_current(stc.region.as_ref())?;
        if changed {
            copy(&mut stc, &mut copied);
            stc.region = newreg;
            if !unit {
                stc.set_region_fs(frm.as_ref());
            }
        }

        for i in 0..stc.coords.len() {
            for (key, comment) in REG_KEYS {
                let Some(reg) = key_map_region(&stc.coords[i], key)? else {
                    continue;
                };
                let (newreg, changed) = simplify_in_current(reg.as_ref())?;
                if changed {
                    copy(&mut stc, &mut copied);
                    put_region(&mut stc.coords[i], key, newreg.as_ref(), Some(comment))?;
                }
            }
        }

        if copied {
            Ok((Box::new(stc), true))
        } else {
            Ok((self.clone_box(), false))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    // stc.c `Equal`: region.c `Equal`, then equal encapsulated Regions.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some_and(|other| {
            (self.region.as_ref() as &dyn Mapping).equal(other.region.as_ref() as &dyn Mapping)
        })
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}
