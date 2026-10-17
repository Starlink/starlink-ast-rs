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

/// The AstroCoords key for the axis names (stc.h `AST__STCNAME`).
pub const STC_NAME: &str = "Name";
/// The AstroCoords key for the value Region (stc.h `AST__STCVALUE`).
pub const STC_VALUE: &str = "Value";
/// The AstroCoords key for the error Region (stc.h `AST__STCERROR`).
pub const STC_ERROR: &str = "Error";
/// The AstroCoords key for the resolution Region (stc.h `AST__STCRES`).
pub const STC_RES: &str = "Resolution";
/// The AstroCoords key for the size Region (stc.h `AST__STCSIZE`).
pub const STC_SIZE: &str = "Size";
/// The AstroCoords key for the pixel size Region (stc.h `AST__STCPIXSZ`).
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

/// The concrete Stc class an [`Stc`] is.
pub trait StcKind: sealed::Sealed + Clone + std::fmt::Debug + Send + 'static {
    /// The AST class name.
    const CLASS: &'static str;
    /// The comment on the class name in a dump.
    const COMMENT: &'static str;
}

/// The marker for [`StcResourceProfile`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceProfile;

impl sealed::Sealed for ResourceProfile {}

impl StcKind for ResourceProfile {
    const CLASS: &'static str = "StcResourceProfile";
    const COMMENT: &'static str = "Resource coverage";
}

/// The area of a coordinate system that a resource covers (C's
/// `StcResourceProfile`, stcresourceprofile.c).
pub type StcResourceProfile = Stc<ResourceProfile>;

/// The builder for a [`StcResourceProfile`].
pub type StcResourceProfileBuilder = StcBuilder<ResourceProfile>;

/// The marker for [`StcSearchLocation`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchLocation;

impl sealed::Sealed for SearchLocation {}

impl StcKind for SearchLocation {
    const CLASS: &'static str = "StcSearchLocation";
    const COMMENT: &'static str = "Query coverage";
}

/// The area of a coordinate system that a query covers (C's
/// `StcSearchLocation`, stcsearchlocation.c).
pub type StcSearchLocation = Stc<SearchLocation>;

/// The builder for a [`StcSearchLocation`].
pub type StcSearchLocationBuilder = StcBuilder<SearchLocation>;

/// The marker for [`StcCatalogEntryLocation`].
#[derive(Debug, Clone, Copy, Default)]
pub struct CatalogEntryLocation;

impl sealed::Sealed for CatalogEntryLocation {}

impl StcKind for CatalogEntryLocation {
    const CLASS: &'static str = "StcCatalogEntryLocation";
    // stccatalogentrylocation.c gives the class the same comment as
    // StcResourceProfile.
    const COMMENT: &'static str = "Resource coverage";
}

/// The area of a coordinate system that a catalogue entry covers (C's
/// `StcCatalogEntryLocation`, stccatalogentrylocation.c).
pub type StcCatalogEntryLocation = Stc<CatalogEntryLocation>;

/// The builder for a [`StcCatalogEntryLocation`].
pub type StcCatalogEntryLocationBuilder = StcBuilder<CatalogEntryLocation>;

/// The marker for [`StcObsDataLocation`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ObsDataLocation;

impl sealed::Sealed for ObsDataLocation {}

impl StcKind for ObsDataLocation {
    const CLASS: &'static str = "StcObsDataLocation";
    const COMMENT: &'static str = "Observation coverage";
}

/// The area of a coordinate system that an observation covers, with the
/// position of the observatory (C's `StcObsDataLocation`,
/// stcobsdatalocation.c).
pub type StcObsDataLocation = Stc<ObsDataLocation>;

/// The builder for a [`StcObsDataLocation`].
pub type StcObsDataLocationBuilder = StcBuilder<ObsDataLocation>;

/// An STC Region: an encapsulated Region and its AstroCoords elements.
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

/// Builder for an [`Stc`], as the class's C constructor takes its
/// arguments.
pub struct StcBuilder<K: StcKind> {
    region: DynRegion,
    coords: Vec<KeyMap>,
    kind: PhantomData<K>,
}

impl<K: StcKind> StcBuilder<K> {
    /// A builder for an Stc encapsulating `region`.
    pub fn new(region: impl Region + 'static) -> Self {
        Self::from_dyn(Box::new(region))
    }

    /// A builder for an Stc encapsulating a boxed Region.
    pub fn from_dyn(region: DynRegion) -> Self {
        StcBuilder {
            region,
            coords: Vec::new(),
            kind: PhantomData,
        }
    }

    /// The AstroCoords elements, each a KeyMap whose keys are among
    /// [`STC_NAME`], [`STC_VALUE`], [`STC_ERROR`], [`STC_RES`], [`STC_SIZE`]
    /// and [`STC_PIXSZ`]: one name per axis under [`STC_NAME`], and one
    /// Region under each other key.
    pub fn coords(mut self, coords: Vec<KeyMap>) -> Self {
        self.coords = coords;
        self
    }

    /// Build the Stc, as stc.c `astInitStc` does. A Region that is itself an
    /// Stc contributes its encapsulated Region mapped into its current Frame;
    /// any other Region is copied. Each AstroCoords Region is mapped into the
    /// Frame of the encapsulated Region and simplified; a key, value or Region
    /// that cannot be used is AST__STCKEY.
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

    /// A copy of the encapsulated Region (C's `astGetStcRegion`), which
    /// describes an area in this Stc's base Frame.
    pub fn get_stc_region(&self) -> DynRegion {
        self.region.copy_region()
    }

    /// The class of the encapsulated Region, the `RegionClass` attribute.
    pub fn region_class(&self) -> &'static str {
        super::class_name(self.region.as_any())
    }

    /// The number of AstroCoords elements (C's `astGetStcNCoord`).
    pub fn get_stc_n_coord(&self) -> usize {
        self.coords.len()
    }

    /// A copy of AstroCoords element `icoord`, counting from 1, with its
    /// Regions in this Stc's current Frame (C's `astGetStcCoord`). Where that
    /// Frame differs from the base Frame the axis names are removed, since
    /// they may no longer apply. Each Region is marked as needing its Frame
    /// in a dump. An index out of range is AST__STCIND.
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

    /// Mutate the current frame with region integrity enforcement: remapping
    /// the Region when it is Adaptive, otherwise changing both Frames.
    ///
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
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
    /// The position of the observatory, if it has been given one.
    pub fn obs(&self) -> Option<&PointList> {
        self.obs.as_ref()
    }

    /// Give the observatory position, or remove it with `None` (C's
    /// protected `astStcSetObs`, stcobsdatalocation.c `StcSetObs`).
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

    /// stc.c `Overlap`: the overlap of the encapsulated Region with `that`.
    fn overlap(&self, that: &dyn Region) -> AstResult<Overlap> {
        self.region.overlap(that)
    }

    /// stc.c `OverlapX`: the overlap of the encapsulated Region with `this`,
    /// with "inside" turned round, since the Regions are the other way round.
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

    /// stc.c `GetNegated`: the encapsulated Region's value.
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

    /// stc.c `SetNegated`: set on the Stc and the encapsulated Region.
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

    /// stc.c `SetRegFS`: the Stc takes the Frame, and so does the
    /// encapsulated Region if it does not dump its own.
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

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        let mesh = self.reg_base_mesh()?;
        Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())))
    }

    fn get_region_grid(&self) -> AstResult<PointSet> {
        super::data::region_base_grid(self)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
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

    /// stc.c `RegSetAttrib` and `RegClearAttrib`: the change is made to the
    /// Stc's own Frames, then its base-Frame form to the encapsulated
    /// Region, whose current Frame is that base Frame, and, for a setting
    /// but not a clearing, to each AstroCoords Region. A Region without the
    /// attribute (AST__BADAT) is passed over, the error annulled.
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

    /// stc.c `GetDefUnc`: the encapsulated Region's uncertainty, where it has
    /// one set, in its current Frame, which is the Stc's base Frame;
    /// otherwise the Region default.
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

    /// stc.c `RegBasePick`: the encapsulated Region's.
    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        self.region.reg_base_pick_axes(axes)
    }

    /// stc.c `RegPins`: the encapsulated Region's.
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

    /// stc.c `GetUseDefs`: the value set on the Stc, or else the encapsulated
    /// Region's.
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

    /// region.c `RemoveRegions`: a Region becomes its Frame.
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

    /// A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    /// linear.
    fn is_linear(&self) -> bool {
        true
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame, which as a
    /// Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<DynMapping> {
        Some(Box::new(UnitMap::new(self.nin())))
    }

    /// region.c `ReportPoints`: the current Frame formats the values.
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

    /// stc.c `Transform`: the input copied, and a point made bad where the
    /// encapsulated Region rejects it in the base Frame
    /// (`reg_base_mask`).
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

    /// stc.c `Simplify`. After the Region simplification, a negated Stc has
    /// the negation moved onto the encapsulated Region; the encapsulated
    /// Region is mapped into the current Frame where the base->current Mapping
    /// is not a UnitMap, and simplified; and so is each AstroCoords Region.
    /// The Stc is copied, as C copies it, before the first change.
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

    /// stc.c `Equal`: region.c `Equal`, then equal encapsulated Regions.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some_and(|other| {
            (self.region.as_ref() as &dyn Mapping).equal(other.region.as_ref() as &dyn Mapping)
        })
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}
